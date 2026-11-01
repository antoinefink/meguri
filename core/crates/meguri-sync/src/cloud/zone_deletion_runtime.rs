use super::*;
use crate::cloud::{
    publication::{RecordRead, ZoneDeletionGuard},
    zone_deletion::{Diagnostic, ZoneResult},
};

impl Runtime {
    pub fn zone_deletion_due(&self, instance: &str) -> Result<bool, RuntimeError> {
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        Ok(
            with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.retained_zones(b.binding.id))?
                .values()
                .any(|row| row.deletion_due),
        )
    }
    /// Recheck the WHOLE original selection at final synchronous queue admission.
    /// No provider or callback runs with Runtime/tracking authority held.
    pub fn submit_zone_deletion_batch<T>(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        read: RecordRead,
        zones: &[String],
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        self.submit_zone_deletion(guard, collection, || {
            let current = self.zone_deletion_check(guard, read, collection)?;
            if zones.is_empty() || current != zones || zones.iter().any(|z| !z.starts_with("c-")) {
                return Err(RuntimeError::StaleIncarnation);
            }
            Ok(submit())
        })?
    }
    pub fn zone_deletion_diagnostics(&self) -> Vec<Diagnostic> {
        self.state().adapter.zone_deletions.clone()
    }
    pub fn record_zone_deletion_diagnostic(&self, diagnostic: Diagnostic) {
        let mut s = self.state();
        // Bounded process-local debug evidence, never record payloads/accounts.
        if s.adapter.zone_deletions.len() == 64 {
            s.adapter.zone_deletions.remove(0);
        }
        s.adapter.zone_deletions.push(diagnostic);
    }
    pub fn zone_deletion_read_reset(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .isolate_runtime_binding(id, IsolationReason::EncryptedDataReset)
        })?;
        let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
        b.binding.facts.state = BindingState::Isolated(IsolationReason::EncryptedDataReset);
        b.usable = false;
        b.verified = false;
        s.root_confirmed = false;
        s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        Ok(())
    }
    pub fn zone_deletion_batch_result(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        results: &[ZoneResult],
    ) -> Result<(), RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let reset = results.iter().any(|r| {
            r.failure
                .as_ref()
                .is_some_and(|f| f.code == Some(26) && f.encrypted_data_reset)
        });
        with_tracking_access(|| -> Result<(), Error> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            for result in results {
                if result.failure.as_ref().is_none_or(|f| f.code == Some(26)) {
                    store.remove_publication_zone(id, &result.zone)?;
                }
            }
            if reset {
                store.isolate_runtime_binding(id, IsolationReason::EncryptedDataReset)?;
            }
            Ok(())
        })?;
        if reset {
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.binding.facts.state = BindingState::Isolated(IsolationReason::EncryptedDataReset);
            b.usable = false;
            b.verified = false;
            s.root_confirmed = false;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        }
        Ok(())
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Synthetic SDK tests use real retained rows, with an elapsed retention
    /// interval. No binding, ACK or production due-query result is supplied.
    pub fn prepare_zone_deletion_fixture_for_testing(&self, instance: &str) -> Result<Vec<String>, RuntimeError> {
        let now = self.adapter_now()?;
        let seen = now.checked_sub(30 * 86_400_000).ok_or(Error::InvalidData)?;
        let mut zones: Vec<_> = [61, 62, 63]
            .map(|b| super::super::adapter::collection_zone(EpochId([b; 16])))
            .into();
        zones.sort();
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        Self::gate_locked(&s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if self.inputs.scope.container.as_str() != "synthetic-u114"
            || zones
                .iter()
                .any(|z| z == &super::super::adapter::collection_zone(b.binding.facts.epoch))
        {
            return Err(Error::InvalidData.into());
        }
        let id = b.binding.id;
        with_tracking_access(|| {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            store.record_publication_zones(id, &zones, seen)?;
            store.mark_publication_zones_due(id, now)
        })?;
        Ok(zones)
    }
}
