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
    /// Confirms an opaque consumer belongs to this process engine.
    pub fn belongs_to(&self, engine: &Engine) -> bool {
        std::sync::Arc::ptr_eq(&self.engine.inner, &engine.inner)
    }

    pub fn op(&self) -> LongOp {
        self.op
    }

    /// Early admission check for verified replacement consumers, before any
    /// write-bearing preparation. Verified replacement rechecks after draining.
    /// Exclusive entry itself does not check: transfer callers must call
    /// `check_verified_review` again after exclusive entry, before write-bearing
    /// preparation or remote mutation.
    pub fn check_verified_review(&self, reason: crate::replace::ReplaceReason) -> Result<(), EngineError> {
        let state = self.engine.lock_state();
        self.check_held(&state)?;
        if reason.refuses_active_review()
            && state
                .current
                .as_ref()
                .is_some_and(|c| c.core.exec.active_session.lock().unwrap().is_some())
        {
            return Err(EngineError::SessionActive);
        }
        Ok(())
    }

    /// Verified entry with no open collection (R2/L5). Holds ownership across
    /// validation, recovery, swap and the ordinary L2 open. A real existing file
    /// still requires the consumer's safety backup before invoking this seam.
    pub fn replace_verified_initial(
        &self,
        live: &std::path::Path,
        config: crate::OpenConfig,
        stage: crate::replace::ReplacementStage,
        reason: crate::replace::ReplaceReason,
        change: impl FnOnce(&mut meguri_storage::WriteScope<'_>) -> Result<(), meguri_storage::StorageError>,
    ) -> Result<Collection, crate::replace::ReplaceError> {
        self.replace_verified_initial_before_commit(live, config, stage, reason, change, || Ok(()))
    }

    /// Initial-entry counterpart of `Exclusive::replace_verified_before_commit`.
    /// The hook runs without core locks or reader leases, before swap ownership
    /// is retained. Media entry remains cancellable until rename; recovery
    /// itself always runs to completion.
    pub fn replace_verified_initial_before_commit(
        &self,
        live: &std::path::Path,
        config: crate::OpenConfig,
        mut stage: crate::replace::ReplacementStage,
        reason: crate::replace::ReplaceReason,
        change: impl FnOnce(&mut meguri_storage::WriteScope<'_>) -> Result<(), meguri_storage::StorageError>,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<Collection, crate::replace::ReplaceError> {
        use crate::replace::ReplaceError;
        if self.op != reason.long_op() {
            return Err(ReplaceError::NotExclusive);
        }
        let prior = {
            let _control = self.engine.inner.control_lane.lock().unwrap();
            let mut state = self.engine.lock_state();
            self.check_held(&state)?;
            if state.current.is_some()
                || !matches!(
                    state.state,
                    LifecycleState::NoCollection | LifecycleState::Failed { .. }
                )
            {
                return Err(ReplaceError::NotExclusive);
            }
            let prior = state.state.clone();
            self.engine.set_state_locked(&mut state, LifecycleState::Opening);
            prior
        };
        let directory = live
            .parent()
            .filter(|p| !p.as_os_str().is_empty())
            .unwrap_or(std::path::Path::new("."));
        let activated = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            meguri_model::faultpoint!("replace.initial.before_ownership")
                .map_err(meguri_storage::StorageError::from)
                .map_err(EngineError::from)?;
            let ownership = std::sync::Arc::new(
                meguri_storage::lockfile::CollectionLock::acquire(directory).map_err(EngineError::from)?,
            );
            // RF2 needs T7 for every existing entry. Check under ownership
            // before touching the sidecar or preparing the candidate.
            if reason == crate::replace::ReplaceReason::ColpkgImport {
                match live.symlink_metadata() {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Ok(_) => {
                        return Err(EngineError::Unsupported {
                            feature: "pre-import safety backup of an unopened collection (T7)".into(),
                        }
                        .into());
                    }
                    Err(e) => return Err(EngineError::from(meguri_storage::StorageError::from(e)).into()),
                }
            }
            crate::device_state::reconcile(live, &config.publication_binding, config.clock.clone())
                .map_err(EngineError::from)?;
            let mut initializers = config.initializers.clone();
            initializers.register(meguri_search::sqlfns::register);
            initializers.register(meguri_sched::sqlfns::register);
            let options = meguri_storage::OpenOptions::new(config.clock.as_ref(), config.random.as_ref())
                .initializers(initializers);
            meguri_storage::verified_replace::prepare(
                &stage,
                live,
                &options,
                config.clock.as_ref(),
                reason.package(),
                change,
            )?;
            let media = if let Some(incoming) = stage.take_local_media() {
                if reason != crate::replace::ReplaceReason::ColpkgImport {
                    return Err(ReplaceError::NotExclusive);
                }
                // Retain collection admission before any media constructor can
                // publish a journal or durable recovery evidence.
                self.engine.lock_state().recovery_ownership = Some(ownership.clone());
                Some(crate::replace::PreparedMedia::new_initial(
                    live,
                    config.clock.clone(),
                    &mut stage,
                    incoming,
                )?)
            } else {
                None
            };
            if let Err(error) = before_commit() {
                return Err(match media {
                    Some(media) => media.abort(error.into()),
                    None => error.into(),
                });
            }
            let media_recovered = media.as_ref().map(|m| m.recovered.clone());
            // Retain the existing identity before beginning the first intent.
            let ensured = meguri_model::faultpoint!("replace.media.initial_before_ensure")
                .map_err(meguri_storage::StorageError::from)
                .and_then(|()| crate::device_state::ensure(live, &config.publication_binding, config.random.as_ref()));
            if let Err(error) = ensured {
                let error = EngineError::from(error).into();
                return Err(match media {
                    Some(media) => media.abort(error),
                    None => error,
                });
            }
            self.engine.lock_state().recovery_ownership = Some(ownership.clone());
            let swapped = crate::replace::run_swap(
                live,
                stage,
                reason,
                config.publication_binding.clone(),
                config.random.clone(),
                config.clock.clone(),
                activated.clone(),
                media,
                self.engine.threads().clone(),
            );
            let changed = activated.load(std::sync::atomic::Ordering::Acquire);
            if media_recovered
                .as_ref()
                .is_some_and(|r| !r.load(std::sync::atomic::Ordering::Acquire))
            {
                return Err(swapped
                    .err()
                    .unwrap_or_else(|| EngineError::internal("media recovery incomplete").into()));
            }
            crate::device_state::reconcile(live, &config.publication_binding, config.clock.clone())
                .map_err(EngineError::from)?;
            match swapped {
                Err(error) if !changed => Err(error),
                outcome => Ok::<_, ReplaceError>((ownership, outcome.err())),
            }
        }))
        .unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref()).into()));
        let (ownership, swap_error) = match prepared {
            Ok(prepared) => prepared,
            Err(error) => {
                let changed = activated.load(std::sync::atomic::Ordering::Acquire);
                let recovery_required = matches!(&error, ReplaceError::MediaRecoveryRequired { .. });
                if changed || recovery_required {
                    self.engine.set_state(LifecycleState::Failed {
                        reason: FailedReason::Replace {
                            detail: error.to_string(),
                        },
                    });
                } else {
                    self.engine.lock_state().recovery_ownership = None;
                    self.engine.set_state(prior);
                }
                return if changed {
                    Err(ReplaceError::AfterActivation {
                        generation: None,
                        source: Box::new(error),
                    })
                } else {
                    Err(error)
                };
            }
        };
        let old = self.engine.generation();
        let announcement = self.engine.inner.stream.reserve();
        let opened = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.engine.open_raw_owned(live, config, Some(ownership))
        }))
        .unwrap_or_else(|payload| {
            let error = EngineError::from_panic(payload.as_ref());
            self.engine.set_state(LifecycleState::Failed {
                reason: FailedReason::Replace {
                    detail: error.to_string(),
                },
            });
            Err(error)
        })
        .map_err(|error| ReplaceError::AfterActivation {
            generation: None,
            source: Box::new(error.into()),
        })?;
        {
            let mut state = self.engine.lock_state();
            if let Some(holder) = state.slot.as_mut().filter(|s| s.id == self.id) {
                holder.generation = opened.generation();
            }
        }
        announcement.fill(
            opened.generation(),
            None,
            crate::EventKind::CollectionReplaced {
                generation: opened.generation(),
            },
        );
        for listener in self.engine.ports().generation_listeners {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                listener.collection_replaced(old, opened.generation())
            }))
            .map_err(|payload| ReplaceError::AfterActivation {
                generation: Some(opened.generation()),
                source: Box::new(EngineError::from_panic(payload.as_ref()).into()),
            })?;
        }
        match swap_error {
            Some(error) => Err(ReplaceError::AfterActivation {
                generation: Some(opened.generation()),
                source: Box::new(error),
            }),
            None => Ok(opened),
        }
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
                    undo_session: None,
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
