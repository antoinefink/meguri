//! The job registry (`engine/jobs` J1, J2): hands out ids, tokens and
//! progress sinks, runs the work on the pool, keeps the running jobs for
//! cancellation, and publishes exactly one `JobFinished` per job.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};

use crate::cancel::CancelToken;
use crate::changes::stream::{ChangeStream, EventKind};
use crate::error::{EngineError, panic_message};
use crate::jobs::pool::JobPool;
use crate::jobs::{JobContext, JobHandle, JobId, JobOutcome};
use crate::lifecycle::{Generation, SlotGuard};
use crate::progress::ProgressSink;
use crate::threads::ThreadRegistry;

/// A running (or queued) job, as listed.
#[derive(Clone, Debug)]
pub struct RunningJob {
    pub id: JobId,
    pub name: String,
    pub cancel: Arc<CancelToken>,
}

/// The registry. Clones share it.
#[derive(Clone)]
pub struct JobRegistry {
    inner: Arc<Inner>,
}

struct Inner {
    next_id: AtomicU64,
    running: Mutex<BTreeMap<JobId, RunningJob>>,
    pool: JobPool,
    stream: ChangeStream,
}

impl fmt::Debug for JobRegistry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobRegistry")
            .field("running", &self.running().len())
            .finish()
    }
}

impl JobRegistry {
    /// A registry whose work runs on `workers` threads of `threads`, and
    /// whose `JobFinished` events go to `stream`.
    pub fn new(threads: &ThreadRegistry, stream: ChangeStream, workers: usize) -> io::Result<JobRegistry> {
        Ok(JobRegistry {
            inner: Arc::new(Inner {
                next_id: AtomicU64::new(1),
                running: Mutex::new(BTreeMap::new()),
                pool: JobPool::new(threads, workers)?,
                stream,
            }),
        })
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, BTreeMap<JobId, RunningJob>> {
        self.inner.running.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Starts `work` and returns its handle at once. The job holds `slot`
    /// (if any) until its work returns; the slot is released before
    /// `JobFinished` is published, with `generation`.
    pub fn start(
        &self,
        name: &str,
        generation: Generation,
        slot: Option<SlotGuard>,
        work: impl FnOnce(&JobContext) -> Result<(), EngineError> + Send + 'static,
    ) -> JobHandle {
        let id = JobId(self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        let progress = ProgressSink::new();
        self.lock().insert(
            id,
            RunningJob {
                id,
                name: name.to_owned(),
                cancel: Arc::clone(&cancel),
            },
        );
        let context = JobContext {
            id,
            cancel: Arc::clone(&cancel),
            progress: progress.clone(),
            slot,
        };
        let registry = self.clone();
        self.inner.pool.run(move || {
            let outcome = if context.cancel.is_cancelled() {
                JobOutcome::Cancelled
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&context))) {
                    Ok(Ok(())) => JobOutcome::Succeeded,
                    Ok(Err(EngineError::Cancelled)) => JobOutcome::Cancelled,
                    Ok(Err(error)) => JobOutcome::Failed { error },
                    Err(payload) => JobOutcome::Failed {
                        error: EngineError::Internal {
                            message: format!("panic: {}", panic_message(payload.as_ref())),
                        },
                    },
                }
            };
            // The slot goes before the event, so a caller reacting to it is
            // not refused.
            drop(context);
            registry.lock().remove(&id);
            registry
                .inner
                .stream
                .publish(generation, None, EventKind::JobFinished { id, outcome });
        });
        JobHandle { id, cancel, progress }
    }

    /// Cancels a running or queued job by id; `false` when none has it.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self.lock().get(&id).map(|job| Arc::clone(&job.cancel));
        token.inspect(|token| token.cancel()).is_some()
    }

    /// The jobs started and not yet finished, by id.
    pub fn running(&self) -> Vec<RunningJob> {
        self.lock().values().cloned().collect()
    }
}
