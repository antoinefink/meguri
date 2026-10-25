//! Generations (`engine/lifecycle` L3) and the generation change after a
//! collection swap (L8).

use std::fmt;
use std::sync::Arc;

use crate::changes::stream::EventKind;
use crate::collection::{Collection, CollectionCore};
use crate::error::EngineError;
use crate::executor::exclusive::{Admission, Privilege};
use crate::executor::writer::{NewGeneration, Swap};
use crate::lifecycle::state::{ExclusivePhase, FailedReason, LifecycleState};
use crate::lifecycle::{Engine, is_open};

/// Names one opened collection: 0 before anything was opened, then one more
/// at every open and every replacement, for the life of the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(pub u64);

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// What a replace left: the privileged handle of the new generation whenever
/// the generation moved, and the error when the swap or the reopen failed.
/// A swap that failed after changing the file leaves both.
pub(crate) struct Replacement {
    pub(crate) moved: Option<Collection>,
    pub(crate) error: Option<EngineError>,
}

/// L8: closes the readers and the writer, runs `swap` with nothing open,
/// reopens through the open gate, and, when the file changed, moves to the
/// next generation, whose first day check the writer has committed by
/// then; then opens the readers, publishes `CollectionReplaced` once and
/// tells every `GenerationListener`. A first check that fails leaves
/// nothing open, as a failed reopen does.
pub(crate) fn replace(engine: &Engine, core: &Arc<CollectionCore>, privilege: Privilege, swap: Swap) -> Replacement {
    engine.set_exclusive_phase(core, ExclusivePhase::Replacing);
    core.exec.wait_idle();
    core.readers.close_all();
    let old = core.exec.generation();
    let mut replaced = core.exec.replace(swap);
    let mut reopened = replaced.factory.is_some();
    if let Some(factory) = replaced.factory {
        core.readers.set_factory(factory);
    }
    if replaced.reopened {
        let collection = Collection {
            core: Arc::clone(core),
            generation: core.exec.generation(),
            privilege: Some(privilege),
            undo_session: None,
        };
        for listener in engine.ports().generation_listeners {
            if let Err(error) = listener.collection_reopened(&collection) {
                // A reopened SQLite file alone is insufficient for Ready.
                // Keep admission closed and the retired journal evidence;
                // never publish a successful replacement with a broken provider.
                replaced.error = Some(error);
                replaced.generation = None;
                reopened = false;
                core.readers.close_all();
                break;
            }
        }
    }
    let moved = replaced.generation.map(|moved: NewGeneration| {
        let new = moved.generation;
        moved
            .announce
            .fill(new, None, EventKind::CollectionReplaced { generation: new });
        for listener in engine.ports().generation_listeners {
            listener.collection_replaced(old, new);
        }
        Collection {
            core: Arc::clone(core),
            generation: new,
            privilege: Some(privilege),
            undo_session: None,
        }
    });
    if reopened {
        engine.set_exclusive_phase(core, ExclusivePhase::Working);
    } else if let Some(error) = &replaced.error {
        // Nothing reopened: the collection is gone.
        core.exec.set_admission(Admission::Closed {
            reason: "the collection could not be reopened after a swap".to_owned(),
        });
        let mut state = engine.lock_state();
        // Unless it was closed meanwhile (the writer refused the swap).
        if is_open(&state, core) {
            engine.set_state_locked(
                &mut state,
                LifecycleState::Failed {
                    reason: FailedReason::Replace {
                        detail: error.to_string(),
                    },
                },
            );
        }
    }
    Replacement {
        moved,
        error: replaced.error,
    }
}
