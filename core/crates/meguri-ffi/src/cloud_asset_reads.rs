//! Native record-ID reads retain the original Host, Runtime and core selection.
//! Metadata is descriptive; no frontend id/token can create or rebind authority.
use crate::{
    CoreError, callback,
    cloud_policy::CloudPolicyExecution,
    sync::{CloudRecord, CloudWorkflow, cloud_error},
};
use meguri_sync::cloud::{incoming as i, runtime::Runtime, workflow::FetchedRecord};
use std::sync::Arc;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudAssetReadKind {
    Retry,
    SaveConflictFirst,
}
impl From<i::AssetReadKind> for CloudAssetReadKind {
    fn from(kind: i::AssetReadKind) -> Self {
        match kind {
            i::AssetReadKind::Retry => Self::Retry,
            i::AssetReadKind::SaveConflictFirst => Self::SaveConflictFirst,
        }
    }
}

/// Enqueue only an already prepared all-fields record-ID SDK operation. Do no
/// UI, await, or synchronous core reentry here. Join its original reply later,
/// including when submit throws after this callback has returned true.
#[uniffi::export(foreign)]
pub trait CloudAssetReadQueue: Send + Sync {
    fn enqueue(&self) -> bool;
}

#[derive(uniffi::Object)]
pub struct CloudAssetReadPass {
    host: Arc<meguri_host::CoreHost>,
    core: Arc<Runtime>,
    // Retain the original collection for terminal cleanup, even after Host close
    // or replacement. Core still checks its full generation/file/account guard.
    collection: meguri_engine::Collection,
    sdk: String,
    execution: Option<Arc<CloudPolicyExecution>>,
    pass: i::AssetReadPass,
}

#[derive(uniffi::Object)]
pub struct CloudAssetReadWork {
    owner: Arc<CloudAssetReadPass>,
    request: i::AssetReadRequest,
}

#[cfg(test)]
impl CloudAssetReadWork {
    pub(crate) fn retained_for_test(&self) -> &i::AssetReadRequest {
        &self.request
    }
}

#[derive(Clone, uniffi::Record)]
pub struct CloudAssetReadIssuedWork {
    pub handle: Arc<CloudAssetReadWork>,
    pub name: String,
    pub zone: String,
    pub kind: CloudAssetReadKind,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudAssetReadCompletion {
    /// Intake acceptance only. Ready work is driven through the normal incoming
    /// writer/WAL path before return; a deferred writer is not an ACK or success.
    pub intake_accepted: bool,
}

#[uniffi::export]
impl CloudWorkflow {
    /// Only K12.3's uncharged first reads. Runtime rejects an active SDK handler;
    /// call this after handler_returned, never from the sent event itself.
    pub fn begin_first_asset_read_pass(&self, sdk: String) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        callback::enter("CloudWorkflow.begin_first_asset_read_pass");
        let collection = self.host.cloud_connection_collection()?;
        let pass = self
            .core
            .begin_asset_read_pass(&sdk, &collection, i::AssetReadKind::SaveConflictFirst)
            .map_err(cloud_error)?;
        Ok(Arc::new(CloudAssetReadPass {
            host: self.host.clone(),
            core: self.core.clone(),
            collection,
            sdk,
            execution: None,
            pass,
        }))
    }
}

#[uniffi::export]
impl CloudPolicyExecution {
    /// Start retry selection using this execution's backend-issued command.
    pub fn begin_asset_read_pass(self: Arc<Self>) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        callback::enter("CloudPolicyExecution.begin_asset_read_pass");
        let (host, core, collection, sdk) = self.asset_read_owner()?;
        let (driver, command) = self.asset_read_driver()?;
        let pass = driver
            .begin_native_asset_read_pass(command, &sdk, &collection)
            .map_err(cloud_error)?;
        Ok(Arc::new(CloudAssetReadPass {
            host,
            core,
            collection,
            sdk,
            execution: Some(self),
            pass,
        }))
    }
}

impl CloudAssetReadPass {
    fn owns(&self, work: &CloudAssetReadWork) -> Result<(), CoreError> {
        if !std::ptr::eq(self, Arc::as_ptr(&work.owner)) {
            return Err(cloud_error("wrong asset read owner"));
        }
        Ok(())
    }
    fn result(&self, work: &CloudAssetReadWork, result: Option<&FetchedRecord>) -> Result<bool, CoreError> {
        if let Some(execution) = &self.execution {
            let (driver, command) = execution.asset_read_driver()?;
            driver
                .native_asset_read_result(command, &self.collection, &work.request, result)
                .map_err(cloud_error)
        } else {
            self.core
                .asset_read_result(&self.sdk, &self.collection, &work.request, result)
                .map_err(cloud_error)
        }
    }
}

#[uniffi::export]
impl CloudAssetReadPass {
    pub fn next(self: Arc<Self>) -> Result<Option<CloudAssetReadIssuedWork>, CoreError> {
        callback::enter("CloudAssetReadPass.next");
        let request = if let Some(execution) = &self.execution {
            let (driver, command) = execution.asset_read_driver()?;
            driver
                .next_native_asset_read(command, &self.collection, &self.pass)
                .map_err(cloud_error)?
        } else {
            self.core
                .next_asset_read(&self.collection, &self.pass)
                .map_err(cloud_error)?
        };
        Ok(request.map(|request| CloudAssetReadIssuedWork {
            name: request.record_name().to_owned(),
            zone: request.zone().to_owned(),
            kind: request.kind().into(),
            handle: Arc::new(CloudAssetReadWork { owner: self, request }),
        }))
    }

    /// False means no SDK reply exists. Err may follow accepted enqueue: retain
    /// work and the native operation and deliver its original terminal result.
    pub fn submit(
        &self,
        work: Arc<CloudAssetReadWork>,
        queue: Arc<dyn CloudAssetReadQueue>,
    ) -> Result<bool, CoreError> {
        callback::enter("CloudAssetReadPass.submit");
        self.owns(&work)?;
        let enqueue = || callback::invoke("CloudAssetReadQueue.enqueue", || queue.enqueue());
        if let Some(execution) = &self.execution {
            let (driver, command) = execution.asset_read_driver()?;
            driver
                .submit_native_asset_read_queue(command, &self.collection, &work.request, enqueue)
                .map_err(cloud_error)
        } else {
            self.core
                .submit_asset_read_queue(&self.collection, &work.request, enqueue)
                .map_err(cloud_error)
        }
    }

    /// None is an actual failed read. Native retains its source-file lease
    /// through this blocking call. The existing bounded, no-follow payload
    /// intake validates bytes/digest and creates its own durable staging copy.
    pub fn complete(
        &self,
        work: Arc<CloudAssetReadWork>,
        record: Option<CloudRecord>,
    ) -> Result<CloudAssetReadCompletion, CoreError> {
        callback::enter("CloudAssetReadPass.complete");
        self.owns(&work)?;
        if record.as_ref().is_some_and(|record| {
            record.name != work.request.record_name()
                || record.record_type.as_deref() != Some("MGEntity")
                || record.zone.as_deref() != Some(work.request.zone())
        }) {
            // Rejecting metadata must still release the selected read's owner.
            // Record fields cannot select another entry for terminal cleanup.
            self.result(&work, None)?;
            return Err(cloud_error("unsupported asset read record metadata"));
        }
        let fetched = record.map(CloudRecord::fetched);
        let intake_accepted = self.result(&work, fetched.as_ref())?;
        if intake_accepted {
            // Same Host-owned normal writer/WAL drive as CloudWorkflow.fetched.
            // No Driver/Runtime/tracking guard spans this writer admission.
            loop {
                match self
                    .host
                    .apply_native_cloud_incoming(&self.sdk, std::time::Duration::from_secs(1))
                    .map_err(cloud_error)?
                {
                    i::IncomingDisposition::Progress(0) | i::IncomingDisposition::Deferred(_) => break,
                    i::IncomingDisposition::Progress(_) => {}
                }
            }
        }
        Ok(CloudAssetReadCompletion { intake_accepted })
    }

    /// Cancels only an unstarted selection. False requires retaining/joining
    /// the actual accepted SDK operation and completing its original work.
    pub fn cancel(&self, work: Arc<CloudAssetReadWork>) -> Result<bool, CoreError> {
        callback::enter("CloudAssetReadPass.cancel");
        self.owns(&work)?;
        Ok(self.core.cancel_asset_read(&work.request))
    }

    /// Stop selecting work while preserving accepted reads and retry accounting.
    /// Dropping this object invokes no external work and never resolves a new host.
    pub fn finish(&self) -> Result<(), CoreError> {
        callback::enter("CloudAssetReadPass.finish");
        self.core.cancel_asset_read_pass(&self.pass);
        Ok(())
    }
}
