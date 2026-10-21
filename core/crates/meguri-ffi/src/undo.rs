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
        summary: MutationSummary,
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
    // U43's pending-answer redo stack is private and has no peek API. A
    // collection-only peek would incorrectly report None for pending redo.
    if matches!(scope, ReviewUndoScope::Session { .. }) {
        return Err(CoreError::Unsupported {
            feature: "session redo status requires ReviewService::peek_redo".into(),
        });
    }
    Ok(runtime::host()?.collection()?.peek_redo(scope.into())?.into())
}
fn start(scope: ReviewUndoScope, redo: bool) -> Result<UndoJobStart, CoreError> {
    let host = runtime::host()?;
    let c = host.collection()?;
    let result = Arc::new(Mutex::new(None));
    let out = result.clone();
    // Always use a job: the writer chooses the target when taking the op,
    // and a short peek can become a long write before then (U25 M4.1).
    let handle = runtime::engine()?.start_job(meguri_engine::JobKind::BulkOp, move |j| {
        j.cancel.check()?;
        let session = matches!(scope, ReviewUndoScope::Session { .. }) || host.review().active_session().is_some();
        let scope = scope.into();
        let value = if session {
            UndoResult::Session {
                head: if redo {
                    host.review().redo(scope)?
                } else {
                    host.review().undo(scope)?
                }
                .into(),
            }
        } else {
            let r = if redo { c.redo(scope)? } else { c.undo(scope)? };
            UndoResult::Global {
                summary: MutationSummary::of(&r),
                step: r.value.into(),
            }
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
