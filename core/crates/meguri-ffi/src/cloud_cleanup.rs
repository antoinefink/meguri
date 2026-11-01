//! One original RunCleanup, qualified local computation, then A21-false inventory.
use crate::{CoreError, callback, cloud_policy::CloudPolicyExecution, sync::cloud_error};
use meguri_sync::cloud::{
    publication::ZoneDeletionGuard,
    runtime::{Runtime, ZoneListingRequest},
};
use std::sync::{Arc, Mutex, PoisonError, atomic::Ordering};

#[uniffi::export(foreign)]
pub trait CloudCleanupQueue: Send + Sync {
    /// Synchronous enqueue of the fully prepared original all-zone operation.
    fn enqueue(&self) -> bool;
}
#[derive(Clone, Copy)]
enum Phase {
    Ready,
    Submitting,
    Queued,
    Finished,
}
struct Inventory {
    phase: Phase,
    request: Option<ZoneListingRequest>,
}
#[derive(uniffi::Object)]
pub struct CloudCleanupPass {
    execution: Arc<CloudPolicyExecution>,
    _host: Arc<meguri_host::CoreHost>,
    core: Arc<Runtime>,
    collection: meguri_engine::Collection,
    sdk: String,
    enqueue_guard: ZoneDeletionGuard,
    inventory: Mutex<Inventory>,
}
#[uniffi::export]
impl CloudPolicyExecution {
    pub fn begin_cleanup_pass(self: Arc<Self>) -> Result<Arc<CloudCleanupPass>, CoreError> {
        callback::enter("CloudPolicyExecution.begin_cleanup_pass");
        let (host, core, collection, sdk) = self.asset_read_owner()?;
        self.validate_execution()?;
        if self.cleanup_begun.swap(true, Ordering::SeqCst) {
            return Err(cloud_error("cleanup already begun"));
        }
        let (driver, command) = self.asset_read_driver()?;
        // This is the ONLY local computation path. It consumes the original
        // drain receipt, including its input instant, without caller clock data.
        driver
            .compute_qualified_local_cleanup(command, &collection)
            .map_err(cloud_error)?;
        self.validate_execution()?;
        // A21 is currently false. Report/capture ONCE, after the committed
        // local transaction and its durable owned-unlink replay have returned.
        let request = core.begin_zone_listing(&sdk, &collection).map_err(cloud_error)?;
        // Reuse the existing own-operation admission guard (account, file,
        // lifecycle, gate and SDK); no foreign callback runs under core locks.
        let enqueue_guard = core.begin_zone_deletion_read(&sdk, &collection).map_err(cloud_error)?;
        Ok(Arc::new(CloudCleanupPass {
            execution: self,
            _host: host,
            core,
            collection,
            sdk,
            enqueue_guard,
            inventory: Mutex::new(Inventory {
                phase: Phase::Ready,
                request: Some(request),
            }),
        }))
    }
}
impl CloudCleanupPass {
    fn owner(&self) -> Result<(), CoreError> {
        self.execution.asset_read_owner()?;
        let (driver, command) = self.execution.asset_read_driver()?;
        driver.validate_native_sdk(command, &self.sdk).map_err(cloud_error)
    }
}
#[uniffi::export]
impl CloudCleanupPass {
    pub fn submit_inventory(&self, queue: Arc<dyn CloudCleanupQueue>) -> Result<bool, CoreError> {
        callback::enter("CloudCleanupPass.submit_inventory");
        {
            let mut state = self.inventory.lock().unwrap_or_else(PoisonError::into_inner);
            if !matches!(state.phase, Phase::Ready) {
                return Err(cloud_error("cleanup inventory already submitted"));
            }
            state.phase = Phase::Submitting;
        }
        let result: Result<bool, CoreError> = (|| {
            self.owner()?;
            self.core
                .submit_zone_deletion(&self.enqueue_guard, &self.collection, || {
                    self.owner()?;
                    Ok(callback::invoke("CloudCleanupQueue.enqueue", || queue.enqueue()))
                })
                .map_err(cloud_error)?
        })();
        self.inventory.lock().unwrap_or_else(PoisonError::into_inner).phase = if matches!(result, Ok(true)) {
            Phase::Queued
        } else {
            Phase::Finished
        };
        result
    }
    /// None is a failed/partial/cancelled listing, never authoritative absence.
    /// Consumes the original request even if its owner is no longer current.
    pub fn complete_inventory(&self, zones: Option<Vec<String>>) -> Result<bool, CoreError> {
        callback::enter("CloudCleanupPass.complete_inventory");
        let request = {
            let mut state = self.inventory.lock().unwrap_or_else(PoisonError::into_inner);
            if !matches!(state.phase, Phase::Queued) {
                return Err(cloud_error("cleanup inventory has no original reply"));
            }
            state.phase = Phase::Finished;
            state
                .request
                .take()
                .ok_or_else(|| cloud_error("cleanup inventory consumed"))?
        };
        self.owner()?;
        self.core
            .complete_zone_listing(request, &self.collection, zones.as_deref())
            .map_err(cloud_error)
    }
    pub fn finish(&self) -> Result<(), CoreError> {
        callback::enter("CloudCleanupPass.finish");
        let mut state = self.inventory.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(state.phase, Phase::Queued | Phase::Submitting) {
            return Err(cloud_error("cleanup original inventory reply not joined"));
        }
        state.phase = Phase::Finished;
        state.request = None;
        Ok(())
    }
}
