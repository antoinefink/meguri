//! Concrete worker entry points for the future native adapter. All network work
//! is caller-explicit and uses the process engine's continued-processing jobs.
use crate::CoreHost;
use meguri_engine::{EngineError, JobContext, JobHandle, JobKind};
use meguri_sync::{
    oneway::{Confirmation, ManualTransfers, Preview, TransferAccess, TransferError, worker_error},
    policy::{Direction, Operation, OperationId, Scope, TransferPolicy},
};
use std::sync::{Arc, Mutex, PoisonError};

pub struct ManualTransferTask<T> {
    pub job: JobHandle,
    operation_id: Arc<Mutex<Option<OperationId>>>,
    result: Arc<Mutex<Option<Result<T, TransferError>>>>,
}
impl<T> ManualTransferTask<T> {
    /// Exact policy identity for this preparation, including failed/cancelled
    /// workers. No identity means this worker never completed policy admission.
    pub fn operation_id(&self) -> Option<OperationId> {
        self.operation_id.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Take the worker's value after its JobFinished event, whether it succeeded
    /// or failed. Cancellation before worker entry produces no value here;
    /// consult JobOutcome::Cancelled for that case.
    pub fn take_result(&self) -> Option<Result<T, TransferError>> {
        self.result.lock().unwrap_or_else(PoisonError::into_inner).take()
    }
}
impl CoreHost {
    pub fn manual_transfer_history(&self) -> &TransferPolicy {
        self.manual_transfers.policy()
    }
    pub fn prepare_manual_transfer(
        &self,
        access: TransferAccess,
        direction: Direction,
        scope: Scope,
    ) -> Result<ManualTransferTask<Preview>, EngineError> {
        let identity = Arc::new(Mutex::new(None));
        let output = identity.clone();
        let mut task = self.transfer_worker(move |service, context| {
            service.prepare_identified(
                context.slot().ok_or(TransferError::Unavailable)?,
                access.with_cancel(context.cancel.clone()),
                direction,
                scope,
                &context.progress,
                |id| *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(id.clone()),
            )
        })?;
        task.operation_id = identity;
        Ok(task)
    }
    pub fn confirm_manual_transfer(
        &self,
        access: TransferAccess,
        confirmation: Confirmation,
    ) -> Result<ManualTransferTask<Operation>, EngineError> {
        self.transfer_worker(move |service, context| {
            service.confirm(
                context.slot().ok_or(TransferError::Unavailable)?,
                access.with_cancel(context.cancel.clone()),
                confirmation,
                &context.progress,
            )
        })
    }
    /// Offline, caller-requested recovery preview. Unknown remote completion is
    /// typed NeedsDecision and remains globally fenced across credential changes.
    pub fn prepare_manual_transfer_recovery(
        &self,
        id: OperationId,
    ) -> Result<ManualTransferTask<Preview>, EngineError> {
        self.transfer_worker(move |service, context| {
            service.prepare_recovery(
                context.slot().ok_or(TransferError::Unavailable)?,
                &id,
                &context.cancel,
                &context.progress,
            )
        })
    }
    pub fn cancel_prepared_manual_transfer(
        &self,
        id: &OperationId,
        interrupted: bool,
    ) -> Result<Operation, TransferError> {
        self.manual_transfers.cancel_prepared(id, interrupted)
    }
    fn transfer_worker<T: Send + 'static>(
        &self,
        work: impl FnOnce(&ManualTransfers, &JobContext) -> Result<T, TransferError> + Send + 'static,
    ) -> Result<ManualTransferTask<T>, EngineError> {
        let service = self.manual_transfers.clone();
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        let job = self.engine().start_continued_job(JobKind::OneWaySync, move |context| {
            let typed = work(&service, context);
            let outcome = typed.as_ref().map(|_| ()).map_err(|e| worker_error(e.clone()));
            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(typed);
            outcome
        })?;
        Ok(ManualTransferTask {
            job,
            result,
            operation_id: Arc::new(Mutex::new(None)),
        })
    }
}
