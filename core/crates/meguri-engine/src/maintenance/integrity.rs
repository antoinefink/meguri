use super::*;
use crate::{Collection, device_state::LastQuickCheck};

#[derive(Clone)]
pub(super) struct HealthInputs {
    pub path: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub status: Option<Arc<dyn super::status::StatusProvider>>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Sound,
    Corrupt,
}
pub(crate) fn is_corruption(error: Option<&StorageError>) -> bool {
    matches!(error, Some(StorageError::Sqlite { code: 11 | 26, .. }))
}
pub fn verdict(result: &Result<meguri_storage::integrity::IntegrityReport, StorageError>) -> Option<Verdict> {
    match result {
        Ok(report) => Some(if report.diagnostics.first().is_some_and(|value| value == "ok") {
            Verdict::Sound
        } else {
            Verdict::Corrupt
        }),
        Err(error) if is_corruption(Some(error)) => Some(Verdict::Corrupt),
        Err(_) => None,
    }
}
pub fn quick_check_due(now: i64, last: Option<i64>, unclean: bool) -> bool {
    unclean
        || last.is_none_or(|last| {
            let now = now.div_euclid(1_000);
            let last = last.div_euclid(1_000);
            now < last || i128::from(now) - i128::from(last) >= 86_400
        })
}
pub fn optimize_due(now: i64, last: Option<i64>, start: Option<i64>) -> bool {
    let last = last.into_iter().chain(start).max();
    last.is_some_and(|last| i128::from(now.div_euclid(1_000)) - i128::from(last.div_euclid(1_000)) >= 1_209_600)
}
fn idle(engine: &Engine) -> Option<Collection> {
    idle_in(engine, &engine.lock_state())
}
fn idle_in(engine: &Engine, state: &crate::lifecycle::LcState) -> Option<Collection> {
    if state.suspended || state.slot.is_some() || !matches!(state.state, LifecycleState::Ready { .. }) {
        return None;
    }
    let current = state.current.as_ref()?;
    if current
        .core
        .exec
        .active_session
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_some()
    {
        return None;
    }
    Some(Collection {
        core: current.core.clone(),
        generation: engine.generation(),
        privilege: None,
        undo_session: None,
    })
}
/// Session admission holds this through its writer barrier and active flag.
/// Cancels only this maintenance owner's statements, never a backup token.
pub(crate) struct IdlePause(Arc<Maintenance>);
impl Drop for IdlePause {
    fn drop(&mut self) {
        self.0.lock().idle_pauses -= 1;
    }
}
impl Maintenance {
    pub(crate) fn pause_idle(self: &Arc<Self>) -> IdlePause {
        let mut s = self.lock();
        s.idle_pauses += 1;
        for token in [&s.checking, &s.optimizing].into_iter().flatten() {
            token.interrupt();
        }
        self.idle.notify_all();
        IdlePause(self.clone())
    }
    pub(crate) fn opened(&self, unclean: bool) {
        self.lock().unclean_due = unclean;
    }
    /// Serialized only against other integrity evaluations. No reader or SQL
    /// handle is retained on the caller after this method returns.
    pub(super) fn check_idle(&self, caller: Option<&Arc<CancelToken>>) -> Result<bool, EngineError> {
        // Session/lifecycle interruptions belong only to this health check.
        // The exported owner also wakes lane waits and is polled by SQLite.
        let cancel = CancelToken::new();
        let _lane = self.health_lane.lock(caller.map(|token| token.as_ref()))?;
        let Some(engine) = self.engine.upgrade() else {
            return Ok(false);
        };
        let Some(collection) = idle(&engine) else {
            return Ok(false);
        };
        let (inputs, unclean, unreadable_checked, epoch) = {
            let s = self.lock();
            if s.suspended || s.idle_pauses != 0 {
                return Ok(false);
            }
            let Some(inputs) = s.health.clone() else {
                return Ok(false);
            };
            (inputs, s.unclean_due, s.unreadable_checked, s.epoch)
        };
        let records = DeviceState::for_collection(&inputs.path).maintenance_records();
        let unreadable = records.quick_check.is_err();
        let last = records.quick_check.ok().flatten().map(|r| r.instant);
        if (unreadable && unreadable_checked && !unclean) || !quick_check_due(inputs.clock.now_ms(), last, unclean) {
            return Ok(true);
        }
        {
            let lifecycle = engine.lock_state();
            if idle_in(&engine, &lifecycle).is_none() {
                return Ok(false);
            }
            let mut s = self.lock();
            if s.suspended || s.idle_pauses != 0 || s.epoch != epoch {
                return Ok(false);
            }
            s.checking = Some(cancel.clone());
            s.active += 1;
        }
        let _active = Active(self);
        let reader = collection.reader(ReaderRole::Maintenance);
        let result = reader.as_ref().map(|reader| {
            reader
                .quick_check_cancellable(&|| cancel.is_cancelled() || caller.is_some_and(|token| token.is_cancelled()))
        });
        self.lock().checking = None;
        let result = match result {
            Ok(result) => result,
            Err(error) => return Err(error.clone()),
        };
        let outcome = verdict(&result);
        let stopped = cancel.is_cancelled() || caller.is_some_and(|token| token.is_cancelled());
        let outcome = if stopped && outcome != Some(Verdict::Corrupt) {
            None
        } else {
            outcome
        };
        if let Some(outcome) = outcome {
            if outcome == Verdict::Corrupt {
                collection
                    .core
                    .exec
                    .set_admission(crate::executor::exclusive::Admission::Corrupt);
            }
            let record = LastQuickCheck {
                instant: inputs.clock.now_ms(),
                passed: outcome == Verdict::Sound,
            };
            if let Err(error) = DeviceState::for_collection(&inputs.path).set_last_quick_check(&record) {
                tracing::warn!(%error, "quick check verdict could not be recorded");
            }
            let mut s = self.lock();
            if s.epoch != epoch {
                return Ok(false);
            }
            s.unclean_due = false;
            s.unreadable_checked |= unreadable;
            drop(s);
            if outcome == Verdict::Corrupt {
                // Reader was dropped before lifecycle mutation.
                drop(reader);
                engine.mark_corrupt_generation(collection.generation(), "quick check found a damaged collection");
                return Ok(false);
            }
        }
        if let Some(caller) = caller {
            caller.check()?;
        }
        Ok(outcome == Some(Verdict::Sound))
    }
    pub(super) fn report_corruption(&self, collection: &Collection) {
        let Some(engine) = self.engine.upgrade() else {
            return;
        };
        let lifecycle = engine.lock_state();
        if engine.generation() != collection.generation() || !matches!(lifecycle.state, LifecycleState::Ready { .. }) {
            return;
        }
        let inputs = self.lock().health.clone();
        if let Some(inputs) = inputs
            && let Err(error) = DeviceState::for_collection(&inputs.path).set_last_quick_check(&LastQuickCheck {
                instant: inputs.clock.now_ms(),
                passed: false,
            })
        {
            tracing::warn!(%error, "corruption verdict could not be recorded");
        }
        drop(lifecycle);
        engine.mark_corrupt_generation(collection.generation(), "optimize found a damaged collection");
    }
    pub(super) fn schedule_optimize(&self) -> Result<(), EngineError> {
        let Some(engine) = self.engine.upgrade() else {
            return Ok(());
        };
        let lifecycle = engine.lock_state();
        let Some(collection) = idle_in(&engine, &lifecycle) else {
            return Ok(());
        };
        let mut s = self.lock();
        if s.suspended || s.idle_pauses != 0 || s.optimizing.is_some() {
            return Ok(());
        }
        let Some(inputs) = s.health.clone() else {
            return Ok(());
        };
        let records = DeviceState::for_collection(&inputs.path).maintenance_records();
        let (Ok(last), Ok(start)) = (records.optimize, records.cadence_start) else {
            return Ok(());
        };
        if last.is_none() && start.is_none() {
            // Only this field, never lastOptimize, represents first evaluation.
            return DeviceState::for_collection(&inputs.path)
                .set_optimize_cadence_start(EpochMillis(inputs.clock.now_ms()))
                .map_err(Into::into);
        }
        if !optimize_due(inputs.clock.now_ms(), last.map(|v| v.0), start.map(|v| v.0)) {
            return Ok(());
        }
        let cancel = CancelToken::new();
        let epoch = s.epoch;
        s.optimizing = Some(cancel.clone());
        s.active += 1;
        drop(s);
        drop(lifecycle);
        let owner = engine.maintenance().clone();
        self.jobs.maintenance_work(move || {
            let _active = Active(&owner);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                // Recheck on the worker; each queued statement also checks its
                // token on the writer. Session admission cancels before waiting.
                if idle(&engine).is_none() {
                    cancel.interrupt();
                }
                super::optimize::optimize(&collection, &cancel)
            }))
            .unwrap_or_else(|p| Err(EngineError::from_panic(p.as_ref())));
            // Completion metadata belongs to this Ready generation. Freeze
            // lifecycle ownership before sidecar I/O, never after taking the
            // maintenance mutex (the writer never takes this lifecycle lock).
            let lifecycle = engine.lock_state();
            let mut s = owner.lock();
            let current = s.epoch == epoch
                && engine.generation() == collection.generation()
                && matches!(lifecycle.state, LifecycleState::Ready { .. });
            let corrupt = matches!(&result, Err(EngineError::Storage(error)) if is_corruption(Some(error)));
            if current
                && corrupt
                && let Err(error) = DeviceState::for_collection(&inputs.path).set_last_quick_check(&LastQuickCheck {
                    instant: inputs.clock.now_ms(),
                    passed: false,
                })
            {
                tracing::warn!(%error, "optimize corruption verdict could not be recorded");
            }
            if current
                && result.is_ok()
                && !cancel.is_cancelled()
                && let Err(error) =
                    DeviceState::for_collection(&inputs.path).set_last_optimize(EpochMillis(inputs.clock.now_ms()))
            {
                tracing::warn!(%error, "optimize completion could not be recorded");
            }
            if s.epoch == epoch {
                s.optimizing = None;
            }
            drop(s);
            drop(lifecycle);
            if current && corrupt {
                engine.mark_corrupt_generation(collection.generation(), "optimize found a damaged collection");
            }
        });
        Ok(())
    }
}
pub(super) fn idle_trigger(trigger: MaintenanceTrigger) -> bool {
    matches!(
        trigger,
        MaintenanceTrigger::Opened | MaintenanceTrigger::Background | MaintenanceTrigger::BackgroundProcessing
    )
}
