//! SDK fetch inference uses the existing own-operation transport. Handles keep
//! the original Host/Collection, call, point and authority; DTOs select nothing.
use crate::{
    CoreError, callback,
    cloud_connection::{CloudControlRead, CloudControlTarget, error},
    sync::CloudWorkflow,
};
use meguri_sync::cloud::{adapter::FetchRead, runtime::Runtime};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

#[derive(uniffi::Object)]
pub struct CloudFetchRead {
    host: Arc<meguri_host::CoreHost>,
    core: Arc<Runtime>,
    collection: meguri_engine::Collection,
    read: FetchRead,
    consumed: AtomicBool,
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
                        consumed: AtomicBool::new(false),
                    })
                })
                .collect(),
        })
    }
}
#[uniffi::export]
impl CloudFetchRead {
    pub fn target(&self) -> Result<CloudControlTarget, CoreError> {
        callback::enter("CloudFetchRead.target");
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
    ) -> Result<Option<Arc<CloudFetchRead>>, CoreError> {
        callback::enter("CloudFetchRead.complete");
        if self.consumed.swap(true, Ordering::AcqRel) {
            return Err(crate::sync::cloud_error("consumed root read"));
        }
        let next = self
            .core
            .adapter_complete_fetch_read(
                &self.read,
                &self.collection,
                result.into(),
                discard,
                failure.map(Into::into),
                terminal.map(Into::into),
            )
            .map_err(error)?;
        Ok(next.map(|read| {
            Arc::new(Self {
                host: self.host.clone(),
                core: self.core.clone(),
                collection: self.collection.clone(),
                read,
                consumed: AtomicBool::new(false),
            })
        }))
    }
}
