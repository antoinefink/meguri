//! Native record-ID reads retain the original Host, Runtime and core selection.
//! Metadata is descriptive; no frontend id/token can create or rebind authority.
use crate::{
    CoreError, callback,
    cloud_policy::{CloudPolicy, CloudPolicyExecution, CloudPolicyOutput, operation_error_class},
    sync::{CloudRecord, CloudWorkflow, cloud_error},
};
use meguri_sync::cloud::{
    incoming as i, policy as p,
    runtime::{Runtime, RuntimeError},
    workflow::FetchedRecord,
};
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
    policy: Arc<CloudPolicy>,
    activity: meguri_sync::cloud::runtime::AssetReadActivityGuard,
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

/// A stored empty value is exhaustion; a missing body result remains None.
#[derive(Clone, uniffi::Record)]
pub struct CloudAssetReadNextValue {
    pub work: Option<CloudAssetReadIssuedWork>,
}

#[derive(uniffi::Object)]
pub struct CloudAssetReadNextResult {
    cell: Arc<crate::cloud_policy::policy_jobs::Cell<CloudAssetReadNextValue>>,
}
#[uniffi::export]
impl CloudAssetReadNextResult {
    pub fn result(&self) -> Result<Option<CloudAssetReadNextValue>, CoreError> {
        callback::enter("CloudAssetReadNextResult.result");
        self.cell
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .transpose()
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudAssetReadNextJob {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<CloudAssetReadNextResult>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudAssetReadCompletion {
    /// Intake acceptance only. Ready work is driven through the normal incoming
    /// writer/WAL path before return; a deferred writer is not an ACK or success.
    pub intake_accepted: bool,
}

/// Raw SDK facts, never retained SDK errors, records or payloads.
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudAssetReadFailureFacts {
    pub code: Option<i64>,
    pub retry_after_ms: Option<i64>,
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudAssetReadActivityFacts {
    pub started: bool,
    pub failures: Vec<CloudAssetReadFailureFacts>,
    pub failed: bool,
    pub app_cancelled: bool,
}

#[uniffi::export]
impl CloudWorkflow {
    /// Only K12.3's uncharged first reads. Runtime rejects an active SDK handler;
    /// call this after handler_returned, never from the sent event itself.
    pub fn begin_first_asset_read_pass(&self, sdk: String) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        callback::enter("CloudWorkflow.begin_first_asset_read_pass");
        let collection = self.host.cloud_connection_collection()?;
        let policy = CloudPolicy::for_workflow(self)?;
        let pass = self
            .core
            .begin_asset_read_pass(&sdk, &collection, i::AssetReadKind::SaveConflictFirst)
            .map_err(cloud_error)?;
        CloudAssetReadPass::created(
            self.host.clone(),
            self.core.clone(),
            collection,
            sdk,
            None,
            policy,
            pass,
        )
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
        let policy = self.policy().clone();
        CloudAssetReadPass::created(host, core, collection, sdk, Some(self), policy, pass)
    }
}

// Map only the sampled PolicyWork cancellation, never a fresh token read over
// a later failure. Legacy synchronous refusals keep cloud_error.
fn creation_error(error: RuntimeError) -> CoreError {
    match error {
        RuntimeError::Engine(meguri_engine::EngineError::Cancelled) => CoreError::Cancelled,
        error => cloud_error(error),
    }
}

impl CloudWorkflow {
    pub(crate) fn begin_first_asset_read_pass_with_work(
        &self,
        sdk: String,
        policy: Arc<CloudPolicy>,
        work: &mut p::PolicyWork,
    ) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        let collection = self.host.cloud_connection_collection()?;
        let pass = self
            .core
            .begin_asset_read_pass_with_work(&sdk, &collection, i::AssetReadKind::SaveConflictFirst, work)
            .map_err(creation_error)?;
        CloudAssetReadPass::created(
            self.host.clone(),
            self.core.clone(),
            collection,
            sdk,
            None,
            policy,
            pass,
        )
    }
}

impl CloudPolicyExecution {
    pub(crate) fn begin_asset_read_pass_with_work(
        self: Arc<Self>,
        work: &mut p::PolicyWork,
    ) -> Result<Arc<CloudAssetReadPass>, CoreError> {
        let (host, core, collection, sdk) = self.asset_read_owner()?;
        let (driver, command) = self.asset_read_driver()?;
        let pass = driver
            .begin_native_asset_read_pass_with_work(command, &sdk, &collection, work)
            .map_err(|error| match error {
                p::DriverError::Runtime(error) => creation_error(error),
                error => cloud_error(error),
            })?;
        let policy = self.policy().clone();
        CloudAssetReadPass::created(host, core, collection, sdk, Some(self), policy, pass)
    }
}

impl CloudAssetReadPass {
    fn created(
        host: Arc<meguri_host::CoreHost>,
        core: Arc<Runtime>,
        collection: meguri_engine::Collection,
        sdk: String,
        execution: Option<Arc<CloudPolicyExecution>>,
        policy: Arc<CloudPolicy>,
        pass: i::AssetReadPass,
    ) -> Result<Arc<Self>, CoreError> {
        let activity = match core.asset_read_activity_guard(&pass) {
            Ok(activity) => activity,
            Err(error) => {
                core.cancel_asset_read_pass(&pass);
                return Err(cloud_error(error));
            }
        };
        Ok(Arc::new(Self {
            policy,
            activity,
            host,
            core,
            collection,
            sdk,
            execution,
            pass,
        }))
    }
}

impl CloudAssetReadPass {
    fn issued(self: Arc<Self>, request: Option<i::AssetReadRequest>) -> CloudAssetReadNextValue {
        CloudAssetReadNextValue {
            work: request.map(|request| CloudAssetReadIssuedWork {
                name: request.record_name().to_owned(),
                zone: request.zone().to_owned(),
                kind: request.kind().into(),
                handle: Arc::new(CloudAssetReadWork { owner: self, request }),
            }),
        }
    }

    fn next_with_work(self: Arc<Self>, work: &mut p::PolicyWork) -> Result<CloudAssetReadNextValue, CoreError> {
        // Reuse the original resource's bounded facts without selecting a
        // cleanup mode. This is ordinary work, never DisposalOnly admission.
        let original = self.host.original_resource_authority(&self.collection);
        let compact_error = Arc::new(std::sync::Mutex::new(None));
        let rejected = compact_error.clone();
        let mut selection = work.clone().with_compact_owner_check(Arc::new(move || {
            if original.is_live() {
                return Ok(());
            }
            let error = CoreError::CloudPolicy {
                failure: crate::cloud_policy::CloudPolicyFailure::Closed,
            };
            *rejected.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = Some(error.clone());
            Err(meguri_engine::EngineError::from(error))
        }));
        let request = if let Some(execution) = &self.execution {
            let (driver, command) = execution.asset_read_driver()?;
            driver
                .next_native_asset_read_with_work(command, &self.collection, &self.pass, &mut selection)
                .map_err(|error| match error {
                    p::DriverError::Runtime(RuntimeError::Engine(meguri_engine::EngineError::Cancelled)) => {
                        CoreError::Cancelled
                    }
                    error => cloud_error(error),
                })
        } else {
            self.core
                .next_asset_read_with_work(&self.collection, &self.pass, &mut selection)
                .map_err(creation_error)
        };
        // Only a sampled pre-effect rejection can supply this error. Never
        // check a fresh owner/token over a selected handle or later failure.
        if let Some(error) = compact_error
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take()
        {
            return Err(error);
        }
        Ok(self.issued(request?))
    }

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
    /// Activity belongs to this actual pass, including after finish. An empty
    /// pass reports nothing. All raw errors precede the ONE ended round; cancel
    /// clears active state without resetting or incrementing failing cycles.
    pub fn operation_activity(&self, facts: CloudAssetReadActivityFacts) -> Result<Vec<CloudPolicyOutput>, CoreError> {
        use meguri_sync::cloud::policy as p;
        callback::enter("CloudAssetReadPass.operation_activity");
        let current = self.host.cloud_connection_collection()?;
        if current.generation() != self.collection.generation() || !self.collection.is_current() {
            return Err(cloud_error("stale asset read activity host"));
        }
        self.core
            .validate_asset_read_activity(&self.collection, &self.activity)
            .map_err(cloud_error)?;
        if let Some(execution) = &self.execution {
            execution.validate_execution()?;
        }
        let operation = p::Operation {
            zone: p::Zone::Collection,
            kind: p::OperationKind::Read,
        };
        let mut outputs = Vec::new();
        for failure in &facts.failures {
            let code = failure.code.unwrap_or(0);
            if code == 20 {
                continue;
            }
            outputs.push(
                self.policy
                    .output(self.host.cloud_policy_input(p::DeviceEvent::OperationError {
                        instance: self.sdk.clone(),
                        operation,
                        class: operation_error_class(code),
                        code,
                        retry_after_ms: failure.retry_after_ms,
                    }))?,
            );
        }
        if !facts.started && (facts.app_cancelled || facts.failures.iter().any(|f| f.code == Some(20))) {
            self.host
                .cloud_policy_driver()
                .ok_or_else(|| cloud_error("asset activity driver closed"))?
                .cancel_original_asset_read_activity(&self.collection, &self.activity)
                .map_err(cloud_error)?;
        } else {
            outputs.push(
                self.policy
                    .output(self.host.cloud_policy_input(p::DeviceEvent::Operation {
                        instance: self.sdk.clone(),
                        operation,
                        phase: if facts.started {
                            p::Activity::Started
                        } else {
                            p::Activity::Ended {
                                outcome: if facts.failed || facts.failures.iter().any(|f| f.code != Some(20)) {
                                    p::Outcome::Failed
                                } else {
                                    p::Outcome::Ok
                                },
                                saved: 0,
                            }
                        },
                    }))?,
            );
        }
        Ok(outputs)
    }

    pub fn start_next(self: Arc<Self>) -> Result<CloudAssetReadNextJob, CoreError> {
        callback::enter("CloudAssetReadPass.start_next");
        let (handle, cell) = self
            .policy
            .clone()
            .start_policy("CloudAssetReadPass.next", move |_, work| self.next_with_work(work))?;
        Ok(CloudAssetReadNextJob {
            handle,
            result: Arc::new(CloudAssetReadNextResult { cell }),
        })
    }

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
        Ok(self.issued(request).work)
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
