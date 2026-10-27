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
    pub continued_processing: bool,
}

/// The registry. Clones share it.
#[derive(Clone)]
pub struct JobRegistry {
    inner: Arc<Inner>,
}

#[derive(Default)]
struct RegistryState {
    running: BTreeMap<JobId, RunningJob>,
    paused: bool,
}

struct Inner {
    next_id: AtomicU64,
    running: Mutex<RegistryState>,
    pool: JobPool,
    threads: ThreadRegistry,
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
                running: Mutex::new(RegistryState::default()),
                pool: JobPool::new(threads, workers)?,
                threads: threads.clone(),
                stream,
            }),
        })
    }

    /// Value-triggered backup reevaluation on the existing pool; no timer.
    pub(crate) fn maintenance_work(&self, work: impl FnOnce() + Send + 'static) {
        let threads = self.inner.threads.clone();
        self.inner.pool.run(move || {
            let _qos = threads.utility_scope();
            work();
        });
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, RegistryState> {
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
        let mut state = self.lock();
        if state.paused {
            cancel.cancel();
            drop(state);
            drop(slot);
            drop(work); // Release captured reservations before publishing completion.
            self.inner.stream.publish(
                generation,
                None,
                EventKind::JobFinished {
                    id,
                    outcome: JobOutcome::Cancelled,
                },
            );
            return JobHandle { id, cancel, progress };
        }
        state.running.insert(
            id,
            RunningJob {
                id,
                name: name.to_owned(),
                cancel: Arc::clone(&cancel),
                continued_processing: false,
            },
        );
        drop(state);
        let context = JobContext {
            id,
            cancel: Arc::clone(&cancel),
            progress: progress.clone(),
            slot,
        };
        let registry = self.clone();
        self.inner.pool.run(move || {
            let outcome = if context.cancel.is_cancelled() {
                drop(work);
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
            registry.lock().running.remove(&id);
            registry
                .inner
                .stream
                .publish(generation, None, EventKind::JobFinished { id, outcome });
        });
        JobHandle { id, cancel, progress }
    }

    /// Closing this gate and taking the running snapshot use the same mutex
    /// as start, so no unregistered queued work escapes suspend enumeration.
    pub(crate) fn pause(&self, paused: bool) {
        self.lock().paused = paused;
    }

    pub(crate) fn set_continued(&self, id: JobId) {
        if let Some(job) = self.lock().running.get_mut(&id) {
            job.continued_processing = true;
        }
    }

    pub(crate) fn wait_empty_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        while !self.running().is_empty() {
            if crate::monotonic::now() >= deadline {
                return Err(EngineError::SuspendDeadline { phase: "jobs".into() });
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        Ok(())
    }

    /// Cancels a running or queued job by id; `false` when none has it.
    pub fn cancel(&self, id: JobId) -> bool {
        let token = self.lock().running.get(&id).map(|job| Arc::clone(&job.cancel));
        token.inspect(|token| token.cancel()).is_some()
    }

    /// The jobs started and not yet finished, by id.
    pub fn running(&self) -> Vec<RunningJob> {
        self.lock().running.values().cloned().collect()
    }
}
