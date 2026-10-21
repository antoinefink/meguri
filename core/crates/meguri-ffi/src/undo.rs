//! U25 status and job adapters. U43 owns restoration while a session is active.
use crate::{
    CoreError, callback,
    jobs::JobHandle,
    mutations::MutationSummary,
    review_session::{ReviewHead, ReviewStepInfo, ReviewUndoScope, ReviewUndoStatus},
    runtime,
};
use std::sync::{Arc, Mutex};
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum UndoResult {
    Global {
        summary: Box<MutationSummary>,
        step: ReviewStepInfo,
    },
    Session {
        head: ReviewHead,
    },
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct UndoJobStart {
    pub handle: JobHandle,
    pub result: Arc<UndoJob>,
}
#[derive(Debug, uniffi::Object)]
pub struct UndoJob {
    result: Arc<Mutex<Option<UndoResult>>>,
}
#[uniffi::export]
impl UndoJob {
    pub fn result(&self) -> Result<Option<UndoResult>, CoreError> {
        callback::enter("UndoJob::result");
        Ok(self.result.lock().unwrap().clone())
    }
}
#[uniffi::export]
pub fn peek_undo(scope: ReviewUndoScope) -> Result<ReviewUndoStatus, CoreError> {
    callback::enter("peek_undo");
    let host = runtime::host()?;
    Ok(match scope {
        ReviewUndoScope::Session { .. } => host.review().peek_undo(scope.into())?,
        ReviewUndoScope::Global => host.collection()?.peek_undo(meguri_engine::UndoScope::Global)?,
    }
    .into())
}
#[uniffi::export]
pub fn peek_redo(scope: ReviewUndoScope) -> Result<ReviewUndoStatus, CoreError> {
    callback::enter("peek_redo");
    Ok(runtime::host()?.review().peek_redo(scope.into())?.into())
}
fn start(scope: ReviewUndoScope, redo: bool) -> Result<UndoJobStart, CoreError> {
    let host = runtime::host()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    // Always use a job: the writer chooses the target when taking the op,
    // and a short peek can become a long write before then (U25 M4.1).
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let value = match host
            .review()
            .restore_controlled(scope.into(), redo, j.cancel.clone(), j.progress.clone())?
        {
            meguri_engine::session::HistoryRestore::Session(head) => UndoResult::Session { head: head.into() },
            meguri_engine::session::HistoryRestore::Global(r) => UndoResult::Global {
                summary: Box::new(MutationSummary::of(&r)),
                step: r.value.into(),
            },
        };
        *out.lock().unwrap() = Some(value);
        Ok(())
    })?;
    Ok(UndoJobStart {
        handle: crate::jobs::wrap(handle),
        result: Arc::new(UndoJob { result }),
    })
}
#[uniffi::export]
pub fn start_undo(scope: ReviewUndoScope) -> Result<UndoJobStart, CoreError> {
    callback::enter("start_undo");
    start(scope, false)
}
#[uniffi::export]
pub fn start_redo(scope: ReviewUndoScope) -> Result<UndoJobStart, CoreError> {
    callback::enter("start_redo");
    start(scope, true)
}

/// Admission and pending restoration are one service operation. None means
/// use a history job; this export never falls through into a collection write.
#[uniffi::export]
pub fn try_pending_review_restore(scope: ReviewUndoScope, redo: bool) -> Result<Option<ReviewHead>, CoreError> {
    callback::enter("try_pending_review_restore");
    Ok(runtime::host()?
        .review()
        .try_pending_restore(scope.into(), redo)?
        .map(Into::into))
}
