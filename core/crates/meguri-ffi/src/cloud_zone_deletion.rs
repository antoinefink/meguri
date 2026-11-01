//! Retained issued DeleteZones capability; native metadata cannot recreate it.
use crate::{
    CoreError, callback,
    cloud_connection::{CloudControlRead, CloudControlTarget},
    cloud_policy::CloudPolicyExecution,
    sync::cloud_error,
};
use meguri_sync::cloud::{runtime::Runtime, zone_deletion as z};
use std::sync::{Arc, atomic::Ordering};

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudZoneDeletionFailure {
    pub code: Option<i64>,
    pub retry_after_ms: Option<i64>,
    pub encrypted_data_reset: bool,
}
impl From<CloudZoneDeletionFailure> for z::FailureFacts {
    fn from(f: CloudZoneDeletionFailure) -> Self {
        Self {
            code: f.code,
            retry_after_ms: f.retry_after_ms,
            encrypted_data_reset: f.encrypted_data_reset,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudZoneDeletionResult {
    pub zone: String,
    /// None means an actual successful per-zone deletion callback.
    pub failure: Option<CloudZoneDeletionFailure>,
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudZoneDeletionDiagnostic {
    Read {
        found: bool,
        allowed: u64,
        failure: Option<CloudZoneDeletionFailure>,
        terminal: Option<CloudZoneDeletionFailure>,
    },
    Deleted {
        results: Vec<CloudZoneDeletionResult>,
        terminal: Option<CloudZoneDeletionFailure>,
    },
}
impl From<z::FailureFacts> for CloudZoneDeletionFailure {
    fn from(f: z::FailureFacts) -> Self {
        Self {
            code: f.code,
            retry_after_ms: f.retry_after_ms,
            encrypted_data_reset: f.encrypted_data_reset,
        }
    }
}
impl From<z::Diagnostic> for CloudZoneDeletionDiagnostic {
    fn from(d: z::Diagnostic) -> Self {
        match d {
            z::Diagnostic::Read {
                found,
                allowed,
                failure,
                terminal,
            } => Self::Read {
                found,
                allowed: allowed as u64,
                failure: failure.map(Into::into),
                terminal: terminal.map(Into::into),
            },
            z::Diagnostic::Deleted { results, terminal } => Self::Deleted {
                results: results
                    .into_iter()
                    .map(|r| CloudZoneDeletionResult {
                        zone: r.zone,
                        failure: r.failure.map(Into::into),
                    })
                    .collect(),
                terminal: terminal.map(Into::into),
            },
        }
    }
}
#[uniffi::export(foreign)]
pub trait CloudZoneDeletionQueue: Send + Sync {
    /// Only synchronous admission of the fully prepared ORIGINAL operation.
    fn enqueue(&self) -> bool;
}
#[derive(uniffi::Object)]
pub struct CloudZoneDeletionPass {
    execution: Arc<CloudPolicyExecution>,
    _host: Arc<meguri_host::CoreHost>,
    _core: Arc<Runtime>,
    collection: meguri_engine::Collection,
    pass: Arc<z::Pass>,
}
#[uniffi::export]
impl CloudPolicyExecution {
    pub fn begin_zone_deletion_pass(self: Arc<Self>) -> Result<Option<Arc<CloudZoneDeletionPass>>, CoreError> {
        callback::enter("CloudPolicyExecution.begin_zone_deletion_pass");
        let (host, core, collection, sdk) = self.asset_read_owner()?;
        if self.zone_deletion_begun.swap(true, Ordering::SeqCst) {
            return Err(cloud_error("zone deletion already begun"));
        }
        let (driver, command) = self.asset_read_driver()?;
        let pass = driver
            .begin_native_zone_deletion(command, &sdk, &collection)
            .map_err(cloud_error)?;
        Ok(pass.map(|pass| {
            Arc::new(CloudZoneDeletionPass {
                execution: self,
                _host: host,
                _core: core,
                collection,
                pass,
            })
        }))
    }
}
impl CloudZoneDeletionPass {
    fn owner(&self) -> Result<(), CoreError> {
        // Also validates original policy, Host and Collection generation.
        self.execution.asset_read_owner().map(|_| ())
    }
}
#[uniffi::export]
impl CloudZoneDeletionPass {
    pub fn active_target(&self) -> Result<CloudControlTarget, CoreError> {
        callback::enter("CloudZoneDeletionPass.active_target");
        self.owner()?;
        Ok(CloudControlTarget {
            zone: "meguri-root".into(),
            name: "active".into(),
            record_type: "MGActive".into(),
        })
    }
    pub fn submit_read(&self, queue: Arc<dyn CloudZoneDeletionQueue>) -> Result<bool, CoreError> {
        callback::enter("CloudZoneDeletionPass.submit_read");
        self.owner()?;
        let (driver, _) = self.execution.asset_read_driver()?;
        driver
            .submit_native_zone_deletion_read(&self.pass, &self.collection, || {
                callback::invoke("CloudZoneDeletionQueue.enqueue", || queue.enqueue())
            })
            .map_err(cloud_error)
    }
    pub fn complete_read(
        &self,
        read: CloudControlRead,
        failure: Option<CloudZoneDeletionFailure>,
        terminal: Option<CloudZoneDeletionFailure>,
    ) -> Result<Vec<String>, CoreError> {
        callback::enter("CloudZoneDeletionPass.complete_read");
        // Always consume the original queued reply, including stale owners.
        let owner = self.owner();
        let (driver, _) = self.execution.asset_read_driver()?;
        let result = driver
            .native_zone_deletion_read_result(
                &self.pass,
                &self.collection,
                read.into(),
                failure.map(Into::into),
                terminal.map(Into::into),
            )
            .map_err(cloud_error);
        owner?;
        result
    }
    pub fn submit_deletion(&self, queue: Arc<dyn CloudZoneDeletionQueue>) -> Result<bool, CoreError> {
        callback::enter("CloudZoneDeletionPass.submit_deletion");
        self.owner()?;
        let (driver, _) = self.execution.asset_read_driver()?;
        driver
            .submit_native_zone_deletion(&self.pass, &self.collection, || {
                callback::invoke("CloudZoneDeletionQueue.enqueue", || queue.enqueue())
            })
            .map_err(cloud_error)
    }
    pub fn complete_deletion(
        &self,
        results: Vec<CloudZoneDeletionResult>,
        terminal: Option<CloudZoneDeletionFailure>,
    ) -> Result<(), CoreError> {
        callback::enter("CloudZoneDeletionPass.complete_deletion");
        let owner = self.owner();
        let (driver, _) = self.execution.asset_read_driver()?;
        let result = driver
            .native_zone_deletion_result(
                &self.pass,
                &self.collection,
                results
                    .into_iter()
                    .map(|r| z::ZoneResult {
                        zone: r.zone,
                        failure: r.failure.map(Into::into),
                    })
                    .collect(),
                terminal.map(Into::into),
            )
            .map_err(cloud_error);
        owner?;
        result
    }
    pub fn finish(&self) -> Result<(), CoreError> {
        callback::enter("CloudZoneDeletionPass.finish");
        if !self.pass.finish() {
            return Err(cloud_error("zone deletion original reply not joined"));
        }
        Ok(())
    }
}
