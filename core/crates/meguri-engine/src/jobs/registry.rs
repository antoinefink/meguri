//! The job registry (`engine/jobs` J1, J2): hands out ids, tokens and
//! progress sinks, runs the work on the pool, keeps the running jobs for
//! cancellation, and publishes exactly one `JobFinished` per job.

use std::collections::BTreeMap;
use std::fmt;
use std::io;
use std::sync::atomic::{AtomicU8, AtomicU64, Ordering};
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
    completion: CompletionMode,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CompletionMode {
    Ordinary,
    Required,
    OriginalResource(ResourceMode),
}

/// Compact state published by the same retained Host alongside its canonical
/// close-state transitions. Rust composition API only; never a frontend mode.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum OriginalHostState {
    Prepared,
    Active,
    Closing,
    Closed,
}

#[derive(Clone, Default)]
pub struct OriginalHostLiveness(Arc<AtomicU8>);

impl OriginalHostLiveness {
    pub fn publish(&self, state: OriginalHostState) {
        self.0.store(state as u8, Ordering::Release);
    }

    fn is_active(&self) -> bool {
        self.0.load(Ordering::Acquire) == OriginalHostState::Active as u8
    }
}

/// Internal retained-capability authority. Only the engine constructs the
/// original collection facts; callers cannot supply a mode or generation id.
#[derive(Clone)]
pub struct OriginalResourceAuthority {
    host: OriginalHostLiveness,
    collection: crate::Collection,
    engine_generation: Arc<AtomicU64>,
    registry: std::sync::Weak<Inner>,
    same_collection: bool,
}

impl OriginalResourceAuthority {
    pub(crate) fn new(
        registry: &JobRegistry,
        host: OriginalHostLiveness,
        collection: &crate::Collection,
        engine_generation: Arc<AtomicU64>,
    ) -> Self {
        Self {
            host,
            collection: collection.clone(),
            same_collection: Arc::ptr_eq(&engine_generation, &collection.core.exec.generation_cell),
            engine_generation,
            registry: Arc::downgrade(&registry.inner),
        }
    }

    // Only bounded atomic loads of original facts, including at registration
    // under the registry mutex. No owner resolution or executor admission lock.
    /// Bounded original Host/collection facts, also for ordinary asset selection.
    /// This grants no resource mode or registry admission.
    pub fn is_live(&self) -> bool {
        self.same_collection
            && self.host.is_active()
            && self.collection.generation() == self.collection.current_generation()
            && self.collection.generation().0 == self.engine_generation.load(Ordering::SeqCst)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResourceMode {
    Full,
    DisposalOnly,
}

/// Per-call authority, latched at registration. Recheck after waits and before
/// the first effect. This is admission only: it marks no effect or body result.
pub struct ResourceAuthority {
    original: OriginalResourceAuthority,
    mode: ResourceMode,
}

impl ResourceAuthority {
    pub fn recheck(&mut self) -> ResourceMode {
        if self.mode == ResourceMode::Full && !self.original.is_live() {
            self.mode = ResourceMode::DisposalOnly;
        }
        self.mode
    }
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
    resource_cutoff: bool,
    #[cfg(feature = "test-hooks")]
    busy_tail: Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>,
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
        self.start_selected(name, generation, slot, None, CompletionMode::Ordinary, move |ctx, _| {
            work(ctx)
        })
    }

    /// Original-SDK Ended accounting on the existing pool. Accepted work runs
    /// despite cancellation; paused admission still drops its body. No slot.
    pub fn start_required_completion(
        &self,
        name: &str,
        generation: Generation,
        work: impl FnOnce(&JobContext) -> Result<(), EngineError> + Send + 'static,
    ) -> JobHandle {
        self.start_selected(name, generation, None, None, CompletionMode::Required, move |ctx, _| {
            work(ctx)
        })
    }

    /// Internal original-resource selector for retained Rust capabilities. It
    /// bypasses the control lane, uses no slot, and always runs accepted bodies.
    /// Later FFI wrappers alone bind this authority to opaque frontend handles.
    pub fn start_original_resource(
        &self,
        name: &str,
        original: OriginalResourceAuthority,
        work: impl FnOnce(&JobContext, ResourceAuthority) -> Result<(), EngineError> + Send + 'static,
    ) -> JobHandle {
        let generation = original.collection.generation();
        self.start_selected(
            name,
            generation,
            None,
            Some(original),
            CompletionMode::OriginalResource(ResourceMode::DisposalOnly),
            move |ctx, authority| work(ctx, authority.expect("resource selector authority")),
        )
    }

    fn start_selected(
        &self,
        name: &str,
        generation: Generation,
        slot: Option<SlotGuard>,
        original: Option<OriginalResourceAuthority>,
        mut completion: CompletionMode,
        work: impl FnOnce(&JobContext, Option<ResourceAuthority>) -> Result<(), EngineError> + Send + 'static,
    ) -> JobHandle {
        let id = JobId(self.inner.next_id.fetch_add(1, Ordering::SeqCst));
        let cancel = CancelToken::new();
        let progress = ProgressSink::new();
        let mut state = self.lock();
        let mut authority = original.map(|original| {
            let mode = if !state.paused
                && !state.resource_cutoff
                && original.registry.ptr_eq(&Arc::downgrade(&self.inner))
                && original.is_live()
            {
                ResourceMode::Full
            } else {
                ResourceMode::DisposalOnly
            };
            completion = CompletionMode::OriginalResource(mode);
            ResourceAuthority { original, mode }
        });
        if state.paused && authority.is_none() {
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
                completion,
            },
        );
        #[cfg(feature = "test-hooks")]
        let busy_tail = state.busy_tail.take();
        drop(state);
        let context = JobContext {
            id,
            cancel: Arc::clone(&cancel),
            progress: progress.clone(),
            slot,
        };
        let registry = self.clone();
        let run = move || {
            if let Some(authority) = &mut authority {
                authority.recheck();
            }
            let outcome = if completion == CompletionMode::Ordinary && context.cancel.is_cancelled() {
                drop(work);
                JobOutcome::Cancelled
            } else {
                match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| work(&context, authority))) {
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
        };
        #[cfg(feature = "test-hooks")]
        if let Some((entered, release)) = busy_tail {
            self.inner.pool.run_with_busy_tail_for_testing(run, entered, release);
            return JobHandle { id, cancel, progress };
        }
        self.inner.pool.run(run);
        JobHandle { id, cancel, progress }
    }

    /// Closing this gate and taking the running snapshot use the same mutex
    /// as start, so no unregistered queued work escapes suspend enumeration.
    pub(crate) fn pause(&self, paused: bool) {
        self.lock().paused = paused;
    }

    /// Collection open or successful resume re-enables resource work.
    pub(crate) fn reset_resource_admission(&self) {
        self.lock().resource_cutoff = false;
    }

    /// The registration mutex is the cutoff linearization point. Lock order
    /// is registry then thread registry; no lock is retained while waiting.
    pub(crate) fn resource_cutoff_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        loop {
            {
                let mut state = self.lock();
                if state.running.is_empty() && self.inner.threads.busy_threads().is_empty() {
                    if crate::monotonic::now() >= deadline {
                        return Err(EngineError::SuspendDeadline {
                            phase: "resource cutoff".into(),
                        });
                    }
                    state.resource_cutoff = true;
                    return Ok(());
                }
            }
            if crate::monotonic::now() >= deadline {
                return Err(EngineError::SuspendDeadline {
                    phase: "resource cutoff".into(),
                });
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    /// Holds an actual pool worker after its job has returned/left the running
    /// map, before the pool drops its BusyGuard. No production work hook.
    #[cfg(feature = "test-hooks")]
    pub fn hold_next_busy_tail_for_testing(
        &self,
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    ) {
        self.lock().busy_tail = Some((entered, release));
    }

    pub(crate) fn set_continued(&self, id: JobId) {
        if let Some(job) = self.lock().running.get_mut(&id) {
            job.continued_processing = true;
        }
    }

    pub(crate) fn wait_empty_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        self.wait_until(deadline, |_| true)
    }

    pub(crate) fn wait_connection_replies_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        self.wait_until(deadline, |job| {
            matches!(
                job.completion,
                CompletionMode::Required | CompletionMode::OriginalResource(ResourceMode::Full)
            ) || matches!(
                job.name.as_str(),
                "CloudConnection.next_publication_work" | "CloudConnection.adoption_result"
            )
        })
    }

    /// Required SDK completions and adoption/reply workers retain resources
    /// Host cleanup is about to release. Other jobs may need participant flush.
    pub(crate) fn wait_host_recovery_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
        self.wait_until(deadline, |job| {
            matches!(
                job.completion,
                CompletionMode::Required | CompletionMode::OriginalResource(ResourceMode::Full)
            ) || matches!(
                job.name.as_str(),
                "CloudConnection.next_publication_work" | "CloudConnection.adoption_result" | "iCloud adoption"
            )
        })
    }

    fn wait_until(
        &self,
        deadline: std::time::Duration,
        include: impl Fn(&RunningJob) -> bool,
    ) -> Result<(), EngineError> {
        while self.running().iter().any(&include) {
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

#[cfg(all(test, feature = "test-hooks"))]
mod resource_cutoff_tests {
    use super::*;
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn resource_jobs_cutoff_rechecks_actual_busy_tail_under_registration_mutex() {
        let threads = ThreadRegistry::new();
        let registry = JobRegistry::new(&threads, ChangeStream::new(), 4).unwrap();
        let (entered, tail) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        registry.hold_next_busy_tail_for_testing(entered, wait);
        registry.start("pre-cutoff registered job", Generation(0), None, |_| Ok(()));
        tail.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(registry.lock().running.is_empty());
        let failed = registry.resource_cutoff_until(crate::monotonic::now() + Duration::from_millis(30));
        let marked = registry.lock().resource_cutoff;
        drop(release); // release before assertions, even if cutoff incorrectly succeeded
        assert_eq!(
            failed,
            Err(EngineError::SuspendDeadline {
                phase: "resource cutoff".into()
            })
        );
        assert!(!marked);
        registry
            .resource_cutoff_until(crate::monotonic::now() + Duration::from_secs(5))
            .unwrap();
        assert!(registry.lock().resource_cutoff);
        registry.pause(false);
        assert!(
            registry.lock().resource_cutoff,
            "generic close unpause cannot reopen authority"
        );
        // A completed attempt remains successful, but the next attempt must
        // independently drain work registered since that earlier cutoff.
        let (entered, tail) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        registry.hold_next_busy_tail_for_testing(entered, wait);
        registry.start("post-cutoff ordinary test work", Generation(0), None, |_| Ok(()));
        tail.recv_timeout(Duration::from_secs(5)).unwrap();
        let result = registry.resource_cutoff_until(crate::monotonic::now() + Duration::from_millis(30));
        let still_sealed = registry.lock().resource_cutoff;
        drop(release);
        assert_eq!(
            result,
            Err(EngineError::SuspendDeadline {
                phase: "resource cutoff".into()
            })
        );
        assert!(still_sealed, "a failed fresh drain keeps admission sealed");
        registry
            .resource_cutoff_until(crate::monotonic::now() + Duration::from_secs(5))
            .unwrap();
    }
}
