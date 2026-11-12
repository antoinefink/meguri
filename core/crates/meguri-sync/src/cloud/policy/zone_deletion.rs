//! Native P4 ports over the original Runtime zone-deletion authority.
use super::*;
use crate::cloud::{
    publication::RecordRead,
    zone_deletion::{Diagnostic, FailureFacts, Pass, Phase, ZoneResult},
};

impl Driver {
    pub fn begin_native_zone_deletion(
        &self,
        command: &Command,
        sdk: &str,
        collection: &Collection,
    ) -> Result<Option<Arc<Pass>>, DriverError> {
        self.validate(command)?;
        if command.action.kind != ActionKind::DeleteZones {
            return Err(DriverError::NotCoreAction);
        }
        self.runtime.validate_sdk(sdk)?;
        // The real durable due query precedes preparation of any network work.
        if !self.runtime.zone_deletion_due(sdk)? {
            return Ok(None);
        }
        let guard = self.runtime.begin_zone_deletion_read(sdk, collection)?;
        self.validate(command)?;
        let mut slot = self.zone_deletion.lock().unwrap_or_else(PoisonError::into_inner);
        if slot.upgrade().is_some_and(|pass| !pass.finished()) {
            return Ok(None);
        }
        let pass = Arc::new(Pass {
            command: command.clone(),
            guard,
            phase: Mutex::new(Phase::ReadReady),
        });
        *slot = Arc::downgrade(&pass);
        Ok(Some(pass))
    }
    pub fn submit_native_zone_deletion_read(
        &self,
        pass: &Pass,
        collection: &Collection,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<bool, DriverError> {
        {
            let mut phase = pass.phase.lock().unwrap_or_else(PoisonError::into_inner);
            if !matches!(*phase, Phase::ReadReady) {
                return Err(DriverError::StaleCommand);
            }
            *phase = Phase::ReadQueuing;
        }
        let result = (|| {
            self.validate(&pass.command)?;
            self.runtime.submit_zone_deletion(&pass.guard, collection, || {
                self.validate(&pass.command)?;
                Ok::<bool, DriverError>(enqueue())
            })?
        })();
        *pass.phase.lock().unwrap_or_else(PoisonError::into_inner) = if matches!(result, Ok(true)) {
            Phase::ReadQueued
        } else {
            Phase::Finished
        };
        result
    }
    pub fn native_zone_deletion_read_result(
        &self,
        pass: &Pass,
        collection: &Collection,
        read: RecordRead,
        failure: Option<FailureFacts>,
        terminal: Option<FailureFacts>,
    ) -> Result<Vec<String>, DriverError> {
        {
            let mut phase = pass.phase.lock().unwrap_or_else(PoisonError::into_inner);
            if !matches!(*phase, Phase::ReadQueued) {
                return Err(DriverError::StaleCommand);
            }
            *phase = Phase::Checking;
        }
        let result = (|| {
            self.validate(&pass.command)?;
            let facts: Vec<_> = failure.iter().chain(terminal.iter()).collect();
            let reason = if facts.iter().any(|f| f.code == Some(26) && f.encrypted_data_reset) {
                Some(crate::cloud::IsolationReason::EncryptedDataReset)
            } else if facts.iter().any(|f| f.code == Some(28)) {
                Some(crate::cloud::IsolationReason::ZoneDeletedByUser)
            } else if matches!(read, RecordRead::Missing | RecordRead::ZoneMissing)
                || facts.iter().any(|f| f.code == Some(26))
            {
                Some(crate::cloud::IsolationReason::ActiveMissing)
            } else {
                None
            };
            if let Some(reason) = reason {
                self.runtime.zone_deletion_read_loss(&pass.guard, collection, reason)?;
                return Ok(Vec::new());
            }
            Ok(self.runtime.zone_deletion_check(
                &pass.guard,
                if failure.is_some() || terminal.is_some() {
                    RecordRead::Failed
                } else {
                    read.clone()
                },
                collection,
            )?)
        })();
        self.runtime.record_zone_deletion_diagnostic(Diagnostic::Read {
            found: matches!(read, RecordRead::Found(_)),
            allowed: result.as_ref().map_or(0, Vec::len),
            failure,
            terminal,
        });
        *pass.phase.lock().unwrap_or_else(PoisonError::into_inner) = match &result {
            Ok(zones) if !zones.is_empty() => Phase::DeleteReady {
                read,
                zones: zones.clone(),
            },
            _ => Phase::Finished,
        };
        result
    }
    pub fn submit_native_zone_deletion(
        &self,
        pass: &Pass,
        collection: &Collection,
        enqueue: impl FnOnce() -> bool,
    ) -> Result<bool, DriverError> {
        let (read, zones) = {
            let mut phase = pass.phase.lock().unwrap_or_else(PoisonError::into_inner);
            let Phase::DeleteReady { read, zones } = &*phase else {
                return Err(DriverError::StaleCommand);
            };
            let selected = (read.clone(), zones.clone());
            *phase = Phase::DeleteQueuing;
            selected
        };
        let result = (|| {
            self.validate(&pass.command)?;
            self.runtime
                .submit_zone_deletion_batch(&pass.guard, collection, read, &zones, || {
                    self.validate(&pass.command)?;
                    Ok::<bool, DriverError>(enqueue())
                })?
        })();
        *pass.phase.lock().unwrap_or_else(PoisonError::into_inner) = if matches!(result, Ok(true)) {
            Phase::DeleteQueued { zones }
        } else {
            Phase::Finished
        };
        result
    }
    pub fn native_zone_deletion_result(
        &self,
        pass: &Pass,
        collection: &Collection,
        results: Vec<ZoneResult>,
        terminal: Option<FailureFacts>,
    ) -> Result<(), DriverError> {
        {
            let mut phase = pass.phase.lock().unwrap_or_else(PoisonError::into_inner);
            let Phase::DeleteQueued { zones } = &*phase else {
                return Err(DriverError::StaleCommand);
            };
            // Exact original set, once each. No foreign string becomes authority.
            let actual: std::collections::BTreeSet<_> = results.iter().map(|r| &r.zone).collect();
            if actual.len() != results.len() || actual != zones.iter().collect() {
                return Err(DriverError::StaleCommand);
            }
            *phase = Phase::Completing;
        }
        self.runtime.record_zone_deletion_diagnostic(Diagnostic::Deleted {
            results: results.clone(),
            terminal: terminal.clone(),
        });
        let result = self.validate(&pass.command).and_then(|()| {
            Ok(self.runtime.zone_deletion_batch_result_with_terminal(
                &pass.guard,
                collection,
                &results,
                terminal.as_ref(),
            )?)
        });
        *pass.phase.lock().unwrap_or_else(PoisonError::into_inner) = Phase::Finished;
        result
    }
}
