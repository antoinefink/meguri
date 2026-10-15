//! Generations (`engine/lifecycle` L3) and the generation change after a
//! collection swap (L8).

use std::fmt;

use crate::collection::{Collection, CollectionCore};
use crate::error::EngineError;
use crate::executor::exclusive::{Admission, Privilege};
use crate::executor::writer::Swap;
use crate::lifecycle::Engine;
use crate::lifecycle::state::{ExclusivePhase, FailedReason, LifecycleState};

/// Names one opened collection: 0 before anything was opened, then one more
/// at every open and every replacement, for the life of the engine.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Generation(pub u64);

impl fmt::Display for Generation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// L8: closes the readers and the writer, runs `swap` with nothing open,
/// reopens through the open gate, moves to the next generation, publishes
/// `CollectionReplaced` once and tells every `GenerationListener`.
pub(crate) fn replace(
    engine: &Engine,
    core: &std::sync::Arc<CollectionCore>,
    privilege: Privilege,
    swap: Swap,
) -> Result<Collection, EngineError> {
    engine.set_exclusive_phase(ExclusivePhase::Replacing);
    core.exec.wait_idle();
    core.readers.close_all();
    let old = core.exec.generation();
    let replaced = core.exec.replace(swap);
    let reopened = replaced.factory.is_some();
    if let Some(factory) = replaced.factory {
        core.readers.set_factory(factory);
    }
    match replaced.result {
        Ok(new) => {
            for listener in engine.ports().generation_listeners {
                listener.collection_replaced(old, new);
            }
            engine.set_exclusive_phase(ExclusivePhase::Working);
            Ok(Collection {
                core: std::sync::Arc::clone(core),
                generation: new,
                privilege: Some(privilege),
            })
        }
        Err(error) => {
            if !reopened {
                // Nothing reopened: the collection is gone.
                core.exec.set_admission(Admission::Closed {
                    reason: "the collection could not be reopened after a swap".to_owned(),
                });
                engine.set_state(LifecycleState::Failed {
                    reason: FailedReason::Replace {
                        detail: error.to_string(),
                    },
                });
            } else {
                engine.set_exclusive_phase(ExclusivePhase::Working);
            }
            Err(error)
        }
    }
}
