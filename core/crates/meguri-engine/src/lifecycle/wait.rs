//! The long-operation slot (`engine/lifecycle` L5, R171) and the
//! availability wait the local servers use (L6; R1706, R1797). The collection
//! is available while the state is `Ready` and the slot is free; each time it
//! becomes available again one `Available` event is published.

use std::fmt;
use std::time::Duration;

use crate::collection::Collection;
use crate::error::EngineError;
use crate::lifecycle::state::{FailedReason, LifecycleState, LongOp};
use crate::lifecycle::{Engine, Exclusive, Generation, LcState};
use crate::monotonic;

/// The long-operation slot while one operation holds it. Dropping it frees
/// the slot (and ends an exclusive phase it entered). It belongs to the
/// collection it was taken on (L5): once that one closes, or another is
/// opened, it can no longer enter an exclusive phase.
pub struct SlotGuard {
    pub(crate) engine: Engine,
    pub(crate) id: u64,
    pub(crate) op: LongOp,
}

impl fmt::Debug for SlotGuard {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("SlotGuard").field("op", &self.op).finish()
    }
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct SlotHolder {
    pub id: u64,
    pub op: LongOp,
    /// The generation the holder belongs to: the current one when it took
    /// the slot, then the one its own replacement made (L5, L8).
    pub generation: Generation,
}

impl SlotGuard {
    pub fn op(&self) -> LongOp {
        self.op
    }

    /// Enters the exclusive phase (L7); only for the replacement ops.
    pub fn enter_exclusive(&self, cancellable: bool) -> Result<Exclusive<'_>, EngineError> {
        Exclusive::enter(self, cancellable)
    }

    /// Whether this guard still holds the slot for the current collection:
    /// `CollectionUnavailable` once a close freed the slot, `StaleGeneration`
    /// once another collection was opened (L5).
    pub(crate) fn check_held(&self, state: &LcState) -> Result<(), EngineError> {
        match state.slot {
            Some(holder) if holder.id == self.id => {
                let current = self.engine.generation();
                if holder.generation == current {
                    Ok(())
                } else {
                    Err(EngineError::StaleGeneration {
                        planned: holder.generation,
                        current,
                    })
                }
            }
            _ => Err(EngineError::unavailable(format!(
                "the slot of {} was freed when its collection closed",
                self.op.name()
            ))),
        }
    }
}

impl Drop for SlotGuard {
    fn drop(&mut self) {
        let mut state = self.engine.lock_state();
        if state.slot.is_some_and(|holder| holder.id == self.id) {
            state.slot = None;
            self.engine.update_availability(&mut state);
        }
    }
}

impl Engine {
    /// Takes the slot for `op`, or `Busy` naming the holder (L5).
    pub fn acquire_slot(&self, op: LongOp) -> Result<SlotGuard, EngineError> {
        let mut state = self.lock_state();
        if state.suspended {
            return Err(EngineError::unavailable("the collection is suspended"));
        }
        if let Some(holder) = state.slot {
            return Err(EngineError::Busy {
                running_op: holder.op.name().to_owned(),
            });
        }
        let allowed = match &state.state {
            LifecycleState::Ready { .. } => true,
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. },
            } => op.allowed_when_corrupt(),
            LifecycleState::NoCollection | LifecycleState::Failed { .. } => op.replaces_collection(),
            LifecycleState::Opening | LifecycleState::Upgrading { .. } | LifecycleState::ExclusiveBusy { .. } => false,
        };
        if !allowed {
            return Err(EngineError::unavailable(state.state.describe()));
        }
        let id = self.new_token();
        state.slot = Some(SlotHolder {
            id,
            op,
            generation: self.generation(),
        });
        self.update_availability(&mut state);
        Ok(SlotGuard {
            engine: self.clone(),
            id,
            op,
        })
    }

    /// The operation holding the slot.
    pub fn slot_holder(&self) -> Option<LongOp> {
        self.lock_state().slot.map(|holder| holder.op)
    }

    /// Whether the collection is available now (L6).
    pub fn is_available(&self) -> bool {
        available(&self.lock_state())
    }

    /// Waits until the collection is available, at most `timeout`, and
    /// returns a handle of the current generation; `CollectionUnavailable`
    /// naming the slot holder or the state otherwise (L6).
    pub fn wait_available(&self, timeout: Duration) -> Result<Collection, EngineError> {
        let deadline = monotonic::now() + timeout;
        let mut state = self.lock_state();
        loop {
            if available(&state)
                && let Some(current) = &state.current
            {
                return Ok(Collection {
                    core: current.core.clone(),
                    generation: self.generation(),
                    privilege: None,
                });
            }
            let now = monotonic::now();
            if now >= deadline {
                let reason = match state.slot {
                    Some(holder) => format!("{} is running", holder.op.name()),
                    None => state.state.describe(),
                };
                return Err(EngineError::unavailable(reason));
            }
            state = self.wait_changed(state, deadline - now);
        }
    }

    /// Publishes `Available` when the collection has just become available,
    /// and wakes every waiter.
    pub(crate) fn update_availability(&self, state: &mut LcState) {
        let now = available(state);
        if now && !state.available {
            self.inner
                .stream
                .publish(self.generation(), None, crate::changes::stream::EventKind::Available);
        }
        state.available = now;
        self.inner.changed.notify_all();
    }
}

fn available(state: &LcState) -> bool {
    !state.suspended
        && matches!(state.state, LifecycleState::Ready { .. })
        && state.slot.is_none()
        && state.current.is_some()
}
