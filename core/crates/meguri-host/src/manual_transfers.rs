//! Caller-authorized workers on the process engine. Collection work may defer
//! suspend; media uses ordinary admission and stops when the engine suspends.
use crate::CoreHost;
use meguri_engine::{EngineError, JobContext, JobHandle, JobKind};
use meguri_sync::media::MediaSnapshot;
use meguri_sync::{
    oneway::{
        Confirmation, ManualTransfers, MediaConfirmation, MediaPreview, Preview, TransferAccess, TransferError,
        worker_error,
    },
    policy::{Direction, Operation, OperationId, Scope, TransferPolicy},
};
use std::sync::{Arc, Mutex, PoisonError};

pub struct ManualTransferTask<T> {
    pub job: JobHandle,
    operation_id: Arc<Mutex<Option<OperationId>>>,
    media_snapshot: Arc<Mutex<Option<MediaSnapshot>>>,
    result: Arc<Mutex<Option<Result<T, TransferError>>>>,
}
impl<T> ManualTransferTask<T> {
    fn identified(self, id: OperationId) -> Self {
        *self.operation_id.lock().unwrap_or_else(PoisonError::into_inner) = Some(id);
        self
    }
    pub fn media_snapshot(&self) -> Option<MediaSnapshot> {
        self.media_snapshot
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
    /// First explicit stop cause wins. Unattributed engine/lifecycle cancellation
    /// is a system interruption. The worker keeps ownership until it drains.
    pub fn stop_media(&self, interrupted: bool) {
        if interrupted {
            self.job.cancel.interrupt();
        } else {
            self.job.cancel.cancel();
        }
    }
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
    /// Explicit local-only reconciliation for history and recovery presentation.
    /// Policy write errors remain visible; Files does not depend on this retry.
    pub fn reconcile_manual_transfer_replacement(&self) -> Result<Option<Operation>, TransferError> {
        if self.manual_transfer_history().active()?.is_none() {
            return Ok(None);
        }
        let slot = self.engine().acquire_slot(meguri_engine::LongOp::OneWaySync)?;
        self.manual_transfers.reconcile_local_replacement(&slot)
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
    /// Offline same-operation media decision; never invokes collection recovery.
    pub fn prepare_manual_media(
        &self,
        id: OperationId,
        access: TransferAccess,
    ) -> Result<ManualTransferTask<MediaPreview>, EngineError> {
        let identity = id.clone();
        let task = self.manual_worker(
            None,
            move |service, _, _| service.prepare_media(&id, &access),
            |_| Ok(()),
        )?;
        *task.operation_id.lock().unwrap_or_else(PoisonError::into_inner) = Some(identity);
        Ok(task)
    }
    /// Offline cancellation of a prepared permission decision, after collection
    /// completion. Active media jobs still use their own stop token and drain.
    pub fn cancel_prepared_manual_media(
        &self,
        confirmation: MediaConfirmation,
    ) -> Result<ManualTransferTask<Operation>, EngineError> {
        let identity = confirmation.operation_id().clone();
        let task = self.manual_worker(
            None,
            move |service, _, _| service.cancel_prepared_media(confirmation),
            |_| Ok(()),
        )?;
        Ok(task.identified(identity))
    }
    pub fn run_manual_media(
        &self,
        access: TransferAccess,
        confirmation: MediaConfirmation,
    ) -> Result<ManualTransferTask<Operation>, EngineError> {
        let media = self.media()?;
        let identity = confirmation.operation_id().clone();
        let task = self.manual_worker(
            Some(JobKind::MediaSync),
            move |service, context, snapshot| {
                service.run_media(
                    access.with_cancel(context.cancel.clone()),
                    confirmation,
                    &media,
                    &context.progress,
                    |value| {
                        *snapshot.lock().unwrap_or_else(PoisonError::into_inner) = Some(value.clone());
                        use meguri_sync::media::MediaPhase as P;
                        context
                            .progress
                            .set_phase(meguri_engine::ProgressPhase::Named(match value.phase {
                                P::Checking => "media_checking",
                                P::Listing => "media_listing",
                                P::Downloading => "media_downloading",
                                P::Uploading => "media_uploading",
                                P::Confirming => "media_confirming",
                                P::Resyncing => "media_resyncing",
                            }));
                        context.progress.report(value.totals.checked, None);
                    },
                )
            },
            |op| {
                use meguri_sync::policy::MediaStage;
                match op.media.stage {
                    MediaStage::Complete => Ok(()),
                    MediaStage::Cancelled => Err(worker_error(TransferError::Cancelled)),
                    MediaStage::Interrupted => Err(worker_error(TransferError::Interrupted)),
                    _ => Err(worker_error(TransferError::MediaIncomplete {
                        operation: op.id.clone(),
                        generation: op.actual_generation,
                    })),
                }
            },
        )?;
        *task.operation_id.lock().unwrap_or_else(PoisonError::into_inner) = Some(identity);
        Ok(task)
    }
    /// Offline, after the caller has cancelled and drained media. Retains all
    /// policy history, partial outcomes, overlap guards and cooldown deadlines.
    pub fn invalidate_manual_media_owner(&self) -> Result<(), EngineError> {
        if self
            .engine()
            .jobs()
            .running()
            .iter()
            .any(|j| j.name == JobKind::MediaSync.name())
        {
            return Err(EngineError::Busy {
                running_op: JobKind::MediaSync.name().into(),
            });
        }
        if self.collection().is_ok() {
            let media = self.media()?;
            let _lease = media.try_job(meguri_engine::media::MediaJobKind::Sync)?;
            media.store().invalidate_sync_owner().map_err(|_| {
                EngineError::Storage(meguri_storage::StorageError::Io {
                    detail: "media owner state unavailable".into(),
                })
            })?;
        }
        Ok(())
    }
    fn transfer_worker<T: Send + 'static>(
        &self,
        work: impl FnOnce(&ManualTransfers, &JobContext) -> Result<T, TransferError> + Send + 'static,
    ) -> Result<ManualTransferTask<T>, EngineError> {
        self.manual_worker(
            Some(JobKind::OneWaySync),
            move |service, context, _| work(service, context),
            |_| Ok(()),
        )
    }
    fn manual_worker<T: Send + 'static>(
        &self,
        kind: Option<JobKind>,
        work: impl FnOnce(&ManualTransfers, &JobContext, &Arc<Mutex<Option<MediaSnapshot>>>) -> Result<T, TransferError>
        + Send
        + 'static,
        terminal: impl FnOnce(&T) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<ManualTransferTask<T>, EngineError> {
        let service = self.manual_transfers.clone();
        let result = Arc::new(Mutex::new(None));
        let output = result.clone();
        let media_snapshot = Arc::new(Mutex::new(None));
        let snapshots = media_snapshot.clone();
        let run = move |context: &JobContext| {
            let typed = work(&service, context, &snapshots);
            let outcome = match &typed {
                Ok(value) => terminal(value),
                Err(error) => Err(worker_error(error.clone())),
            };
            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(typed);
            outcome
        };
        let job = match kind {
            Some(JobKind::MediaSync) => self.engine().start_job(JobKind::MediaSync, run)?,
            Some(kind) => self.engine().start_continued_job(kind, run)?,
            // Offline decisions need neither a media job nor continued processing.
            None => self
                .engine()
                .jobs()
                .start("manual media decision", self.engine().generation(), None, run),
        };
        Ok(ManualTransferTask {
            job,
            result,
            media_snapshot,
            operation_id: Arc::new(Mutex::new(None)),
        })
    }
}
