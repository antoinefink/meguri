//! Jobs (KTD9): long work started by a short `start_*` export that returns a
//! [`JobHandle`] at once. The work runs on a core-owned thread, reports
//! progress through the handle, and ends with exactly one `JobFinished` event.
//!
//! This is the FFI side only. U24 moves the registry and a bounded pool into
//! the engine (with C7 arbitration); the handle, the outcome and
//! [`cancel_job`] stay here.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::thread;

use crate::callback;
use crate::cancel::CancelToken;
use crate::error::CoreError;
use crate::events::{self, CoreEventKind};
use crate::progress::JobProgress;

#[derive(Clone, Debug, uniffi::Record)]
pub struct JobHandle {
    pub id: u64,
    pub cancel: Arc<CancelToken>,
    pub progress: Arc<JobProgress>,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JobOutcome {
    Succeeded,
    Cancelled,
    Failed { error: CoreError },
}

/// Cancels a running job by id. `false` when no job with that id is running
/// (it already finished, or never existed).
#[uniffi::export]
pub fn cancel_job(id: u64) -> Result<bool, CoreError> {
    callback::enter("cancel_job");
    let token = running().get(&id).cloned();
    Ok(token.inspect(|t| t.trigger()).is_some())
}

/// What the job's work sees: its id, its token and its progress channel.
#[derive(Debug)]
pub struct JobContext {
    pub id: u64,
    pub cancel: Arc<CancelToken>,
    pub progress: Arc<JobProgress>,
}

/// Starts `work` on its own thread and returns its handle. `generation` is the
/// collection generation the `JobFinished` event carries. A panic in `work`
/// ends the job as `Failed(Internal)`.
pub fn start(
    name: &str,
    generation: u64,
    work: impl FnOnce(&JobContext) -> Result<(), CoreError> + Send + 'static,
) -> Result<JobHandle, CoreError> {
    static NEXT_ID: AtomicU64 = AtomicU64::new(1);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let context = JobContext {
        id,
        cancel: CancelToken::new_arc(),
        progress: JobProgress::new(id),
    };
    let handle = JobHandle {
        id,
        cancel: Arc::clone(&context.cancel),
        progress: Arc::clone(&context.progress),
    };
    running().insert(id, Arc::clone(&context.cancel));
    let spawned = thread::Builder::new()
        .name(format!("meguri-job-{id}-{name}"))
        .spawn(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&context)))
                .unwrap_or_else(|payload| Err(CoreError::from_panic(payload.as_ref())));
            let outcome = match result {
                Ok(()) => JobOutcome::Succeeded,
                Err(CoreError::Cancelled) => JobOutcome::Cancelled,
                Err(error) => JobOutcome::Failed { error },
            };
            running().remove(&id);
            // Only fails when the dispatcher thread could not start, and then no
            // event can reach Swift at all.
            let _ = events::publish(generation, None, CoreEventKind::JobFinished { id, outcome });
        });
    if let Err(e) = spawned {
        running().remove(&id);
        return Err(CoreError::Internal {
            message: format!("cannot start job {name}: {e}"),
        });
    }
    Ok(handle)
}

/// The tokens of running jobs, by id.
fn running() -> std::sync::MutexGuard<'static, HashMap<u64, Arc<CancelToken>>> {
    static RUNNING: LazyLock<Mutex<HashMap<u64, Arc<CancelToken>>>> = LazyLock::new(Mutex::default);
    RUNNING.lock().unwrap_or_else(PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    use crate::events::{CoreEvent, CoreEventListener};

    /// One process-wide listener (cargo test runs tests on threads of one
    /// process) that copies every JobFinished to each test's channel; each
    /// test then waits only for its own job ids.
    #[derive(Default)]
    struct Broadcast(Mutex<Vec<mpsc::Sender<(u64, JobOutcome)>>>);
    impl CoreEventListener for Broadcast {
        fn on_event(&self, event: CoreEvent) {
            let CoreEventKind::JobFinished { id, outcome } = event.kind;
            for tx in self.0.lock().unwrap().iter() {
                let _ = tx.send((id, outcome.clone()));
            }
        }
    }

    fn listen() -> mpsc::Receiver<(u64, JobOutcome)> {
        static BROADCAST: std::sync::OnceLock<Arc<Broadcast>> = std::sync::OnceLock::new();
        let broadcast = BROADCAST.get_or_init(|| {
            let broadcast = Arc::new(Broadcast::default());
            events::set_event_listener(broadcast.clone()).unwrap();
            broadcast
        });
        let (tx, rx) = mpsc::channel();
        broadcast.0.lock().unwrap().push(tx);
        rx
    }

    /// The outcome of job `id`, skipping other tests' jobs.
    fn outcome_of(rx: &mpsc::Receiver<(u64, JobOutcome)>, id: u64) -> JobOutcome {
        loop {
            let (got, outcome) = rx.recv_timeout(Duration::from_secs(10)).expect("no JobFinished event");
            if got == id {
                return outcome;
            }
        }
    }

    #[test]
    fn each_job_ends_with_one_job_finished_carrying_its_outcome() {
        let rx = listen();
        let ok = start("ok", 0, |_| Ok(())).unwrap();
        assert_eq!(outcome_of(&rx, ok.id), JobOutcome::Succeeded);
        let failed = start("fails", 0, |_| {
            Err(CoreError::Busy {
                running_op: "check".into(),
            })
        })
        .unwrap();
        assert_eq!(
            outcome_of(&rx, failed.id),
            JobOutcome::Failed {
                error: CoreError::Busy {
                    running_op: "check".into()
                }
            }
        );
        let panicked = start("panics", 0, |_| panic!("job bug")).unwrap();
        let outcome = outcome_of(&rx, panicked.id);
        assert!(
            matches!(&outcome, JobOutcome::Failed { error: CoreError::Internal { message } } if message.contains("job bug")),
            "{outcome:?}"
        );
        let ids = [ok.id, failed.id, panicked.id];
        let extra: Vec<_> = rx.try_iter().filter(|(id, _)| ids.contains(id)).collect();
        assert!(extra.is_empty(), "extra JobFinished events: {extra:?}");
    }

    #[test]
    fn cancel_job_cancels_a_running_job_through_its_token() {
        let rx = listen();
        let handle = start("loop", 0, |ctx| {
            loop {
                ctx.cancel.check()?;
                thread::sleep(Duration::from_millis(1));
            }
        })
        .unwrap();
        assert_eq!(cancel_job(handle.id), Ok(true));
        assert_eq!(outcome_of(&rx, handle.id), JobOutcome::Cancelled);
        assert_eq!(cancel_job(handle.id), Ok(false), "a finished job is no longer running");
        assert_eq!(cancel_job(u64::MAX), Ok(false));
    }
}
