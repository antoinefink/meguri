//! Restorers (`specs/engine/undo.md` M10, R721): state outside the
//! collection that an undo or redo puts back, registered by the units that
//! own it (U43's session queue, U44's load-balancer cache and learning
//! cutoff).

use std::any::Any;
use std::sync::Arc;

use meguri_model::changes::ChangeSet;

use crate::error::panic_message;
use crate::undo::entry::StepInfo;

/// Which way a step was restored.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Direction {
    Undo,
    Redo,
}

/// One undo or redo, as restorers see it.
#[derive(Debug)]
pub struct RestoredStep<'a> {
    pub direction: Direction,
    pub step: &'a StepInfo,
    /// The undo or redo op's change set.
    pub changes: &'a ChangeSet,
    /// The state the step's op attached (`OpContext::set_undo_state`).
    pub state: Option<&'a (dyn Any + Send + Sync)>,
}

/// Puts back what an undo or redo changes outside the collection. Runs on
/// the writer after the undo or redo commits and before its change set is
/// published: keep it short, never submit an op, never call Swift.
pub trait UndoRestorer: Send + Sync {
    fn name(&self) -> &str;
    fn restored(&self, step: &RestoredStep<'_>);
}

/// Runs every restorer in registration order; one that panics is logged and
/// the others still run.
pub(crate) fn notify(restorers: &[Arc<dyn UndoRestorer>], step: &RestoredStep<'_>) {
    for restorer in restorers {
        let outcome = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| restorer.restored(step)));
        if let Err(payload) = outcome {
            let message = panic_message(payload.as_ref());
            tracing::warn!(restorer = restorer.name(), step = step.step.name, %message, "an undo restorer panicked");
        }
    }
}
