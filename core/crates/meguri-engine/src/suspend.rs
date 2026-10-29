//! Control-lane durability and suspend orchestration (engine/suspend S1–S4).
use crate::executor::{
    exclusive::{Admission, Privilege},
    writer::WriterEnv,
};
use crate::monotonic;
use crate::{Engine, EngineError, JobContext, JobHandle, JobId, JobKind};
use meguri_storage::{OpenOptions, ReaderFactory, open_collection};
use std::sync::{MutexGuard, PoisonError, TryLockError};
use std::time::Duration;

/// Failed background WAL syncs reach the control/event consumer even though
/// the covered ops have already returned. One failure and recovery per episode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DurabilityStatus {
    Failed {
        through: crate::Seq,
        error: meguri_storage::StorageError,
    },
    Recovered {
        through: crate::Seq,
    },
}

impl crate::changes::stream::ExtensionEvent for DurabilityStatus {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SuspendOutcome {
    Suspended,
    Deferred { job_id: JobId },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ResumeReport {
    pub external_change: bool,
    pub rolled_over: bool,
    pub next_day_at: i64,
}

pub(crate) enum Control {
    Flush,
    ReviewIdentity,
    Suspend { deadline: Duration },
    Resume,
}
pub(crate) struct ControlReply {
    pub review_identity: Option<((u64, u64), [u64; 2])>,
    pub external_change: bool,
    pub factory: Option<ReaderFactory>,
}

fn timeout(phase: &str) -> EngineError {
    EngineError::SuspendDeadline { phase: phase.into() }
}
fn check_deadline(deadline: Duration, phase: &str) -> Result<(), EngineError> {
    if monotonic::now() >= deadline {
        Err(timeout(phase))
    } else {
        Ok(())
    }
}
impl Engine {
    pub(crate) fn control_lane_until(&self, deadline: Duration) -> Result<MutexGuard<'_, ()>, EngineError> {
        loop {
            check_deadline(deadline, "control lane")?;
            match self.inner.control_lane.try_lock() {
                Ok(lane) => return Ok(lane),
                Err(TryLockError::Poisoned(error)) => return Ok(error.into_inner()),
                Err(TryLockError::WouldBlock) => {
                    std::thread::sleep(Duration::from_millis(1).min(deadline.saturating_sub(monotonic::now())));
                }
            }
        }
    }

    // Job registration must refuse reentrant calls from a participant or op
    // while a control call is waiting for it, instead of deadlocking it.
    pub(crate) fn job_control_lane(&self) -> Result<MutexGuard<'_, ()>, EngineError> {
        match self.inner.control_lane.try_lock() {
            Ok(lane) => Ok(lane),
            Err(TryLockError::Poisoned(error)) => Ok(error.into_inner()),
            Err(TryLockError::WouldBlock) => Err(EngineError::unavailable("a lifecycle control call is running")),
        }
    }
    pub fn flush_for_background(&self) -> Result<(), EngineError> {
        let _lane = self.inner.control_lane.lock().unwrap_or_else(PoisonError::into_inner);
        self.flush_background_inner()
    }
    pub(crate) fn flush_background_inner(&self) -> Result<(), EngineError> {
        let collection = self.collection()?;
        for participant in self.ports().suspend_participants {
            participant.flush()?;
        }
        collection.core.exec.wait_idle();
        collection.core.exec.control(Control::Flush).wait().map(drop)
    }
    pub fn suspend(&self, timeout_budget: Duration) -> Result<SuspendOutcome, EngineError> {
        let deadline = monotonic::now() + timeout_budget;
        let _lane = self.control_lane_until(deadline)?;
        self.collection()?;
        self.jobs().pause(true);
        let jobs = self.jobs().running();
        for job in &jobs {
            if !job.continued_processing {
                job.cancel.interrupt();
            }
        }
        if let Some(job) = jobs.iter().find(|j| j.continued_processing) {
            self.jobs().pause(false);
            return Ok(SuspendOutcome::Deferred { job_id: job.id });
        }
        self.quiesce_for_suspend(deadline)?;
        Ok(SuspendOutcome::Suspended)
    }

    /// Also used by close, after it cancels *all* jobs, including continued
    /// processing. An error always closes admission; unfinished flushes are
    /// remembered separately from a quiesce that can safely be retried.
    pub(crate) fn quiesce_for_suspend(&self, deadline: Duration) -> Result<(), EngineError> {
        let collection = self.collection()?;
        let result = self.quiesce_inner(&collection, deadline);
        if result.is_err() {
            let mut state = self.lock_state();
            state.suspended = true;
            collection.core.exec.set_admission(Admission::Closed {
                reason: "suspension is incomplete".into(),
            });
            self.update_availability(&mut state);
        }
        result
    }

    fn quiesce_inner(&self, collection: &crate::Collection, deadline: Duration) -> Result<(), EngineError> {
        let _idle = self.maintenance().pause_idle();
        let exec = &collection.core.exec;
        exec.review_accepting.store(false, std::sync::atomic::Ordering::Release);
        // Repeatable after a timeout: participants must make their flush and
        // suspend idempotent. They flush before we stop public admission.
        if self.lock_state().suspend_flush_pending {
            // A failed flush left admission closed. A retry has the same
            // write-admitting flush phase as the first attempt.
            {
                let mut state = self.lock_state();
                if state.suspended && matches!(state.state, crate::LifecycleState::Ready { .. }) {
                    state.suspended = false;
                    exec.set_admission(Admission::Open);
                    self.update_availability(&mut state);
                }
            }
            for participant in self.ports().suspend_participants {
                check_deadline(deadline, participant.name())?;
                participant.flush_until(deadline)?;
            }
            let mut state = self.lock_state();
            state.suspend_flush_pending = false;
            state.suspended = true;
            exec.set_admission(Admission::Closed {
                reason: "the collection is suspended".into(),
            });
            self.update_availability(&mut state);
        }
        exec.wait_idle_until(deadline)?;
        for participant in self.ports().suspend_participants {
            participant.suspend_until(deadline)?;
        }
        self.jobs().wait_empty_until(deadline)?;
        collection.core.readers.close_until(deadline)?;
        for participant in self.ports().suspend_participants {
            participant.release_connections_until(deadline)?;
        }
        while !self.threads().busy_threads().is_empty() {
            check_deadline(deadline, "thread registry")?;
            std::thread::sleep(Duration::from_millis(1));
        }
        let participants = self.ports().suspend_participants;
        for thread in self.threads().threads() {
            let owned = thread.suspend_owner.as_deref().is_some_and(|owner| {
                // Events only deliver value envelopes; they never own SQLite handles.
                owner == "writer"
                    || owner == "jobs"
                    || owner == "events"
                    || participants.iter().any(|p| p.name() == owner)
            });
            if !owned {
                return Err(EngineError::InvalidArgument {
                    message: format!("thread {} has no suspend participant", thread.name),
                });
            }
        }
        check_deadline(deadline, "checkpoint")?;
        exec.control(Control::Suspend { deadline }).wait()?;
        exec.wait_idle_until(deadline)?;
        while !self.threads().busy_threads().is_empty() {
            check_deadline(deadline, "thread registry")?;
            std::thread::sleep(Duration::from_millis(1));
        }
        check_deadline(deadline, "completion")?;
        Ok(())
    }
    pub fn resume(&self) -> Result<ResumeReport, EngineError> {
        let _lane = self.control_lane_until(monotonic::now() + Duration::from_secs(5))?;
        let state = self.state();
        if !matches!(state, crate::LifecycleState::Ready { .. }) {
            return Err(EngineError::unavailable(state.describe()));
        }
        let mut collection = self.collection()?;
        let exec = &collection.core.exec;
        let privilege = Privilege(self.new_token());
        {
            let mut state = self.lock_state();
            state.suspended = true;
            self.update_availability(&mut state);
        }
        exec.set_admission(Admission::Privileged {
            privilege,
            reason: "the collection is resuming".into(),
        });
        exec.wait_idle();
        let report = exec.control(Control::Resume).wait()?;
        if let Some(factory) = report.factory {
            collection.core.readers.set_factory(factory);
        }
        collection.privilege = Some(privilege);
        let day = collection.check_rollover()?;
        for participant in self.ports().suspend_participants {
            participant.resume_with_collection(&collection)?;
        }
        self.jobs().pause(false);
        exec.set_admission(Admission::Open);
        let mut state = self.lock_state();
        state.suspended = false;
        state.suspend_flush_pending = true;
        self.update_availability(&mut state);
        Ok(ResumeReport {
            external_change: report.external_change,
            rolled_over: day.rolled_over_from.is_some(),
            next_day_at: day.next_day_at,
        })
    }
    pub fn start_continued_job(
        &self,
        kind: JobKind,
        work: impl FnOnce(&JobContext) -> Result<(), EngineError> + Send + 'static,
    ) -> Result<JobHandle, EngineError> {
        if !matches!(kind, JobKind::OneWaySync | JobKind::ColpkgImport | JobKind::ApkgImport) {
            return Err(EngineError::InvalidArgument {
                message: "this job cannot use continued processing".into(),
            });
        }
        // Serialize registration and policy selection with suspend.
        let _lane = self.job_control_lane()?;
        if self.lock_state().suspended {
            return Err(EngineError::unavailable("the collection is suspended"));
        }
        let slot = kind.long_op().map(|op| self.acquire_slot(op)).transpose()?;
        let handle = self.jobs().start(kind.name(), self.generation(), slot, work);
        self.jobs().set_continued(handle.id);
        Ok(handle)
    }
}

pub(crate) fn control(env: &mut WriterEnv<'_>, action: Control) -> Result<ControlReply, EngineError> {
    let mut reply = ControlReply {
        review_identity: None,
        external_change: false,
        factory: None,
    };
    match action {
        Control::ReviewIdentity => {
            let collection = env
                .collection
                .as_ref()
                .ok_or_else(|| EngineError::unavailable("collection closed"))?;
            reply.review_identity = Some((
                collection.writer().native_identity()?,
                [env.reopen.random.next_u64(), env.reopen.random.next_u64()],
            ));
        }
        Control::Flush => {
            if let Some(collection) = env.collection.as_ref() {
                crate::executor::writer::sync_wal(env.shared, collection.writer(), true)?;
            }
        }
        Control::Suspend { deadline } => {
            check_deadline(deadline, "checkpoint")?;
            if let Some(collection) = env.collection.as_ref()
                && !env.shared.corrupt.load(std::sync::atomic::Ordering::Acquire)
            {
                let result = collection
                    .writer()
                    .checkpoint_with_timeout(deadline.saturating_sub(monotonic::now()))
                    .inspect_err(|_| env.shared.coverage.failed())?;
                if !result.succeeded() {
                    env.shared.coverage.failed();
                    return Err(EngineError::unavailable("checkpoint blocked by an external reader"));
                }
            }
            if let Some(opened) = env.collection.take() {
                let closed = crate::executor::writer::close_owned_collection(env.shared, opened);
                closed?;
            }
            if env.suspended_file.is_none() {
                *env.suspended_file = Some(super::executor::live_file::LiveFile::before_swap(&env.shared.path));
            }
            *env.external_version = None;
        }
        Control::Resume => {
            crate::device_state::reconcile(
                &env.shared.path,
                &env.reopen.binding(),
                env.reopen.clock.clone(),
                env.reopen.random.clone(),
            )?;
            let changed_file = env
                .suspended_file
                .as_mut()
                .is_some_and(|stamp| stamp.release_and_check());
            if env.collection.is_none() {
                let options = OpenOptions::new(env.reopen.clock.as_ref(), env.reopen.random.as_ref())
                    .initializers(env.reopen.initializers.clone())
                    .managed();
                let collection = open_collection(&env.shared.path, &options)?;
                let evidence = crate::collection::OpenEvidence::capture(&collection, env.shared.generation())?;
                reply.external_change = changed_file || collection.external_change;
                *env.external_version = Some(collection.writer().external_data_version()?);
                *env.collection = Some(collection);
                *env.shared.open_evidence.lock().unwrap_or_else(PoisonError::into_inner) = Some(evidence);
                crate::executor::writer::initialize_coverage(
                    env.shared,
                    env.collection.as_ref().expect("resumed writer"),
                );
            } else if let Some(collection) = env.collection.as_ref() {
                let version = collection.writer().external_data_version()?;
                reply.external_change = env.external_version.is_some_and(|v| v != version);
                *env.external_version = Some(version);
                collection.save_sidecar(false)?;
            }
            if let Some(collection) = env.collection.as_ref() {
                if reply.external_change {
                    crate::external_change::notify(env.shared, collection.writer(), env.touched)?;
                }
                reply.factory = Some(collection.reader_factory());
            }
            *env.suspended_file = None;
        }
    }
    Ok(reply)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn control_lane_contention_counts_toward_suspend_deadline() {
        let engine = Engine::new(crate::EngineConfig::default()).unwrap();
        let lane = engine.inner.control_lane.lock().unwrap();
        let caller = engine.clone();
        let started = monotonic::now();
        let result = std::thread::spawn(move || caller.suspend(Duration::from_millis(20)))
            .join()
            .unwrap();
        drop(lane);
        assert_eq!(
            result,
            Err(EngineError::SuspendDeadline {
                phase: "control lane".into()
            })
        );
        assert!(monotonic::now() - started < Duration::from_millis(200));
    }
}
