//! Jobs (KTD9): long work started by a short `start_*` export that returns a
//! [`JobHandle`] at once. The work runs on the process engine's bounded job
//! pool (`engine/jobs` J2), reports progress through the handle, and ends
//! with exactly one `JobFinished` event. This module is the FFI face: the
//! handle, the outcome and [`cancel_job`]; the registry is the engine's.

use std::sync::Arc;

use meguri_engine::{EngineError, Generation, JobId};

use crate::callback;
use crate::cancel::CancelToken;
use crate::error::CoreError;
use crate::progress::JobProgress;
use crate::runtime;

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

impl From<meguri_engine::JobOutcome> for JobOutcome {
    fn from(outcome: meguri_engine::JobOutcome) -> JobOutcome {
        match outcome {
            meguri_engine::JobOutcome::Succeeded => JobOutcome::Succeeded,
            meguri_engine::JobOutcome::Cancelled => JobOutcome::Cancelled,
            meguri_engine::JobOutcome::Failed { error } => JobOutcome::Failed { error: error.into() },
        }
    }
}

impl From<JobOutcome> for meguri_engine::JobOutcome {
    fn from(outcome: JobOutcome) -> meguri_engine::JobOutcome {
        match outcome {
            JobOutcome::Succeeded => meguri_engine::JobOutcome::Succeeded,
            JobOutcome::Cancelled => meguri_engine::JobOutcome::Cancelled,
            JobOutcome::Failed { error } => meguri_engine::JobOutcome::Failed { error: error.into() },
        }
    }
}

/// Cancels a running job by id. `false` when no job with that id is running
/// (it already finished, or never existed).
#[uniffi::export]
pub fn cancel_job(id: u64) -> Result<bool, CoreError> {
    callback::enter("cancel_job");
    Ok(runtime::engine()?.jobs().cancel(JobId(id)))
}

/// What the job's work sees: its id, its token and its progress channel.
#[derive(Debug)]
pub struct JobContext {
    pub id: u64,
    pub cancel: Arc<CancelToken>,
    pub progress: Arc<JobProgress>,
}

/// Starts `work` on the engine's job pool and returns its handle.
/// `generation` is the collection generation the `JobFinished` event
/// carries. A panic in `work` ends the job as `Failed(Internal)`.
pub fn start(
    name: &str,
    generation: u64,
    work: impl FnOnce(&JobContext) -> Result<(), CoreError> + Send + 'static,
) -> Result<JobHandle, CoreError> {
    let engine = runtime::engine()?;
    let handle = engine.jobs().start(
        name,
        Generation(generation),
        None,
        move |job| -> Result<(), EngineError> {
            let context = JobContext {
                id: job.id.0,
                cancel: CancelToken::wrap(Arc::clone(&job.cancel)),
                progress: JobProgress::wrap(job.id.0, job.progress.clone()),
            };
            work(&context).map_err(EngineError::from)
        },
    );
    Ok(JobHandle {
        id: handle.id.0,
        cancel: CancelToken::wrap(handle.cancel),
        progress: JobProgress::wrap(handle.id.0, handle.progress),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::sync::{Mutex, OnceLock};
    use std::thread;
    use std::time::Duration;

    use crate::events::{self, CoreEvent, CoreEventKind, CoreEventListener};

    /// One process-wide listener (cargo test runs tests on threads of one
    /// process) that copies every JobFinished to each test's channel; each
    /// test then waits only for its own job ids.
    #[derive(Default)]
    struct Broadcast(Mutex<Vec<mpsc::Sender<(u64, JobOutcome)>>>);
    impl CoreEventListener for Broadcast {
        fn on_event(&self, event: CoreEvent) {
            let CoreEventKind::JobFinished { id, outcome } = event.kind else {
                return;
            };
            for tx in self.0.lock().unwrap().iter() {
                let _ = tx.send((id, outcome.clone()));
            }
        }
    }

    fn listen() -> mpsc::Receiver<(u64, JobOutcome)> {
        static BROADCAST: OnceLock<Arc<Broadcast>> = OnceLock::new();
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

    #[test]
    fn every_error_crosses_into_a_job_outcome_unchanged() {
        for error in [
            CoreError::Cancelled,
            CoreError::Busy { running_op: "x".into() },
            CoreError::Unavailable { reason: "y".into() },
            CoreError::Unsupported { feature: "z".into() },
            CoreError::InvalidArgument { message: "w".into() },
            CoreError::Internal { message: "v".into() },
        ] {
            assert_eq!(CoreError::from(EngineError::from(error.clone())), error);
        }
    }
}
