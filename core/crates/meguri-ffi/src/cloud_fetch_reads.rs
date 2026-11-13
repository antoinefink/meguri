//! SDK fetch inference uses the existing own-operation transport. Handles keep
//! the original Host/Collection, call, point and authority; DTOs select nothing.
use crate::{
    CoreError, callback,
    cloud_connection::{CloudControlRead, CloudControlTarget, error},
    sync::CloudWorkflow,
};
use crate::cloud_policy::CloudPolicyEarlyEnd;
use meguri_sync::cloud::{adapter::FetchRead, runtime::Runtime};
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

#[derive(uniffi::Object)]
pub struct CloudFetchRead {
    host: Arc<meguri_host::CoreHost>,
    core: Arc<Runtime>,
    collection: meguri_engine::Collection,
    read: FetchRead,
    consumed: AtomicBool,
    // 0: open, 1: cancelled, 2: result accepted. Cancellation never waits for Core.
    acceptance: AtomicU8,
}
/// Native transport shape validation failed on the original record-ID reply.
/// This is a fact, not evidence that Core accepted that reply.
#[derive(Clone, uniffi::Enum)]
pub enum CloudFetchReadNativeFailure {
    MalformedRecord,
}
#[derive(uniffi::Object)]
pub struct CloudSyncNowRequest {
    host: Arc<meguri_host::CoreHost>,
    core: Arc<Runtime>,
    collection: meguri_engine::Collection,
    request: meguri_sync::cloud::adapter::SyncNowRequest,
    read_issued: AtomicBool,
    captured: AtomicBool,
}
#[uniffi::export]
impl CloudSyncNowRequest {
    pub fn epoch_read(&self) -> Result<Option<Arc<CloudFetchRead>>, CoreError> {
        callback::enter("CloudSyncNowRequest.epoch_read");
        require_preparation_host(&self.host)?;
        if self.read_issued.swap(true, Ordering::AcqRel) { return Err(crate::sync::cloud_error("epoch preparation already requested")); }
        Ok(self.core.sync_now_epoch_read_owned(&self.request, &self.collection).map_err(error)?
            .map(|read| CloudFetchRead::epoch(self.host.clone(), self.core.clone(), self.collection.clone(), read)))
    }
    pub fn begin_exchange(&self) -> Result<u64, CoreError> {
        callback::enter("CloudSyncNowRequest.begin_exchange");
        require_preparation_host(&self.host)?;
        if self.captured.swap(true, Ordering::AcqRel) { return Err(crate::sync::cloud_error("request already captured")); }
        self.core.capture_sync_now_request(&self.request, &self.collection).map_err(error)
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudFetchReadPlan {
    pub qualified: bool,
    pub reads: Vec<Arc<CloudFetchRead>>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudRootReadDiagnostic {
    pub active: bool,
    pub lost_collection: bool,
    pub outcome: String,
    pub a7_failure: bool,
    pub accepted: bool,
    pub failure: Option<crate::cloud_zone_deletion::CloudZoneDeletionFailure>,
    pub terminal: Option<crate::cloud_zone_deletion::CloudZoneDeletionFailure>,
}
impl From<meguri_sync::cloud::adapter::RootReadDiagnostic> for CloudRootReadDiagnostic {
    fn from(value: meguri_sync::cloud::adapter::RootReadDiagnostic) -> Self {
        Self {
            active: value.active,
            lost_collection: value.lost_collection,
            outcome: value.outcome,
            a7_failure: value.a7_failure,
            accepted: value.accepted,
            failure: value.failure.map(Into::into),
            terminal: value.terminal.map(Into::into),
        }
    }
}
#[uniffi::export]
impl CloudWorkflow {
    pub fn sync_now_request(&self, sdk: String) -> Result<Arc<CloudSyncNowRequest>, CoreError> {
        callback::enter("CloudWorkflow.sync_now_request");
        require_preparation_host(&self.host)?;
        let collection = self.host.collection()?;
        let request = self.core.admit_sync_now(&sdk, &collection).map_err(error)?;
        Ok(Arc::new(CloudSyncNowRequest { host: self.host.clone(), core: self.core.clone(), collection, request,
            read_issued: AtomicBool::new(false), captured: AtomicBool::new(false) }))
    }
    pub fn sync_now_epoch_read(&self, sdk: String) -> Result<Option<Arc<CloudFetchRead>>, CoreError> {
        callback::enter("CloudWorkflow.sync_now_epoch_read");
        let collection = self.host.collection()?;
        require_preparation_host(&self.host)?;
        Ok(self.core.adapter_sync_now_epoch_read(&sdk, &collection).map_err(error)?.map(|read| {
            Arc::new(CloudFetchRead { host: self.host.clone(), core: self.core.clone(), collection,
                read, consumed: AtomicBool::new(false), acceptance: AtomicU8::new(0) })
        }))
    }
    pub fn known_loss_read(&self, sdk: String, identity_read: u64) -> Result<Option<Arc<CloudFetchRead>>, CoreError> {
        callback::enter("CloudWorkflow.known_loss_read");
        let collection = self.host.collection()?;
        Ok(self
            .core
            .adapter_known_loss_read_after_identity(&sdk, identity_read)
            .map_err(error)?
            .map(|read| {
                Arc::new(CloudFetchRead {
                    host: self.host.clone(),
                    core: self.core.clone(),
                    collection,
                    read,
                    consumed: AtomicBool::new(false), acceptance: AtomicU8::new(0),
                })
            }))
    }
    pub fn fetch_read_plan(&self, sdk: String, id: u64) -> Result<CloudFetchReadPlan, CoreError> {
        callback::enter("CloudWorkflow.fetch_read_plan");
        let collection = self.host.collection()?;
        let qualified = self.core.adapter_fetch_qualified(&sdk, id).map_err(error)?;
        let reads = self.core.adapter_fetch_reads(&sdk, id, &collection).map_err(error)?;
        Ok(CloudFetchReadPlan {
            qualified,
            reads: reads
                .into_iter()
                .map(|read| {
                    Arc::new(CloudFetchRead {
                        host: self.host.clone(),
                        core: self.core.clone(),
                        collection: collection.clone(),
                        read,
                        consumed: AtomicBool::new(false), acceptance: AtomicU8::new(0),
                    })
                })
                .collect(),
        })
    }
}
fn require_preparation_host(host: &meguri_host::CoreHost) -> Result<(), CoreError> {
    if !matches!(host.engine().state(), meguri_engine::LifecycleState::Ready { .. }) {
        return Err(crate::sync::cloud_error("epoch preparation requires Ready"));
    }
    Ok(())
}
impl CloudFetchRead {
    pub(crate) fn epoch(host: Arc<meguri_host::CoreHost>, core: Arc<Runtime>, collection: meguri_engine::Collection, read: FetchRead) -> Arc<Self> {
        Arc::new(Self { host, core, collection, read, consumed: AtomicBool::new(false), acceptance: AtomicU8::new(0) })
    }
}
#[uniffi::export]
impl CloudFetchRead {
    pub fn epoch_preparation_outcome(&self, code: Option<i64>, retry_after_ms: Option<i64>, cancelled: bool,
        failed: bool) -> Result<Option<CloudPolicyEarlyEnd>, CoreError>
    {
        callback::enter("CloudFetchRead.epoch_preparation_outcome");
        use meguri_sync::cloud::{adapter::EpochPreparationOutcome as O, policy::OperationError as E};
        let outcome = self.read.epoch_preparation_outcome().ok_or_else(|| crate::sync::cloud_error("wrong read purpose"))?;
        // Only accepted replies set these refusals. Discarded or stale bytes
        // cannot turn into a failed outcome, even if cancellation follows.
        match outcome {
            O::Conflict => return Ok(Some(CloudPolicyEarlyEnd::Failed { class: E::ConflictRecovery.into() })),
            O::Unsupported => return Ok(Some(CloudPolicyEarlyEnd::Failed { class: E::UnsupportedSchema.into() })),
            _ => {}
        }
        if let Some(class) = self.core.epoch_preparation_failure_class(&self.read) {
            return Ok(Some(CloudPolicyEarlyEnd::Failed { class: class.into() }));
        }
        let class = code.map(crate::cloud_policy::operation_error_class).unwrap_or(E::Other);
        let failed_class = matches!(class, E::Configuration | E::QuotaFull | E::ConflictRecovery | E::UnsupportedSchema);
        if failed && failed_class {
            return Ok(Some(CloudPolicyEarlyEnd::Failed { class: class.into() }));
        }
        // Final enqueue validation can surface a generic error after this
        // original read's cancel fence closed. Retain that atomic cause after
        // accepted refusals and account/binding/tracking/SDK failure precedence.
        // Consumed while still open also identifies an explicit discard or
        // original validation refusal, never an accepted malformed result.
        let phase = self.acceptance.load(Ordering::Acquire);
        if cancelled || phase == 1 || (self.consumed.load(Ordering::Acquire) && phase == 0) {
            return Ok(Some(CloudPolicyEarlyEnd::Cancelled));
        }
        if failed { return Ok(Some(CloudPolicyEarlyEnd::Waiting { class: Some(class.into()), retry_after_ms })); }
        Ok(match outcome {
            O::Ready => None,
            O::Unknown => Some(CloudPolicyEarlyEnd::Waiting { class: None, retry_after_ms: None }),
            O::Conflict => Some(CloudPolicyEarlyEnd::Failed { class: E::ConflictRecovery.into() }),
            O::Unsupported => Some(CloudPolicyEarlyEnd::Failed { class: E::UnsupportedSchema.into() }),
        })
    }
    pub fn cancel(&self) -> Result<(), CoreError> {
        callback::enter("CloudFetchRead.cancel");
        let _ = self.acceptance.compare_exchange(0, 1, Ordering::AcqRel, Ordering::Acquire);
        Ok(())
    }
    pub fn validate_enqueue(&self) -> Result<(), CoreError> {
        callback::enter("CloudFetchRead.validate_enqueue");
        if self.acceptance.load(Ordering::Acquire) != 0 || self.consumed.load(Ordering::Acquire) {
            return Err(crate::sync::cloud_error("cancelled or consumed root read"));
        }
        self.core.validate_fetch_read_enqueue(&self.read, &self.collection).map_err(error)
    }
    pub fn target(&self) -> Result<CloudControlTarget, CoreError> {
        callback::enter("CloudFetchRead.target");
        if self.read.is_epoch_preparation() { require_preparation_host(&self.host)?; }
        self.core
            .validate_fetch_read(&self.read, &self.collection)
            .map_err(error)?;
        Ok(CloudControlTarget {
            zone: self.read.zone().into(),
            name: if self.read.is_active() { "active" } else { "epoch" }.into(),
            record_type: if self.read.is_active() { "MGActive" } else { "MGEpoch" }.into(),
        })
    }
    pub fn validate(&self) -> Result<(), CoreError> {
        callback::enter("CloudFetchRead.validate");
        if self.consumed.load(Ordering::Acquire) {
            return Err(crate::sync::cloud_error("consumed root read"));
        }
        if self.read.is_epoch_preparation() { require_preparation_host(&self.host)?; }
        self.core
            .validate_fetch_read(&self.read, &self.collection)
            .map_err(error)
    }
    pub fn complete(
        &self,
        result: CloudControlRead,
        discard: bool,
        failure: Option<crate::cloud_zone_deletion::CloudZoneDeletionFailure>,
        terminal: Option<crate::cloud_zone_deletion::CloudZoneDeletionFailure>,
        native_failure: Option<CloudFetchReadNativeFailure>,
    ) -> Result<Option<Arc<CloudFetchRead>>, CoreError> {
        callback::enter("CloudFetchRead.complete");
        if self.consumed.swap(true, Ordering::AcqRel) {
            return Err(crate::sync::cloud_error("consumed root read"));
        }
        // Full provider/file validation is outside native admission. The CAS
        // is the cancellation/result linearization point, after that work and
        // before Core's guarded acceptance. Runtime rechecks original authority.
        let validation: Result<(), CoreError> = (|| {
            if !discard {
                if self.read.is_epoch_preparation() { require_preparation_host(&self.host)?; }
                self.core.validate_fetch_read(&self.read, &self.collection).map_err(error)?;
            }
            Ok(())
        })();
        let discard = discard || validation.is_err()
            || self.acceptance.compare_exchange(0, 2, Ordering::AcqRel, Ordering::Acquire).is_err();
        let next = match native_failure {
            Some(CloudFetchReadNativeFailure::MalformedRecord) => self.core.adapter_complete_malformed_fetch_read(
                &self.read, &self.collection, discard, failure.map(Into::into), terminal.map(Into::into),
            ),
            None => self.core.adapter_complete_fetch_read(
                &self.read,
                &self.collection,
                result.into(),
                discard,
                failure.map(Into::into),
                terminal.map(Into::into),
            ),
        };
        // Even a revoked original read retains its native facts exactly once.
        // Core's discard path logs them without accepting a body or authority.
        // Preserve the original prevalidation error over completion errors.
        validation?;
        let next = next.map_err(error)?;
        Ok(next.map(|read| {
            Arc::new(Self {
                host: self.host.clone(),
                core: self.core.clone(),
                collection: self.collection.clone(),
                read,
                consumed: AtomicBool::new(false), acceptance: AtomicU8::new(0),
            })
        }))
    }
}
