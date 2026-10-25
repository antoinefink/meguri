//! The sync runtime (KTD14, rust-stack §3): one tokio runtime per process,
//! created when a job first needs it, with two workers. Blocking entry points
//! run a job on it under a select on the job's cancel token (T9); the app may
//! drop it when it goes to the background (C8).
//!
//! Its threads, workers and blocking-pool threads alike, list themselves in
//! the engine's thread registry from their start to their end (`engine/jobs`
//! J6, KTD9), at user-initiated QoS like the job pool's (J2: the user started
//! the sync and watches it).

use std::cell::{Cell, RefCell};
use std::future::Future;
use std::sync::{Arc, Condvar, Mutex, PoisonError};

use meguri_engine::{CancelToken, Qos, RegisteredThread, ThreadRegistry};
use tokio::runtime::{Builder, Runtime};

use crate::error::{NetworkCause, NetworkError, SyncError};

/// The name of every runtime thread, in the OS and in the registry.
const THREAD_NAME: &str = "meguri-sync";

static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

thread_local! {
    /// A runtime thread's entry in the registry; dropping it unlists the
    /// thread.
    static LISTED: RefCell<Option<RegisteredThread>> = const { RefCell::new(None) };
    static HAS_PARKED: Cell<bool> = const { Cell::new(false) };
}

/// Workers report activity through tokio's park hooks. Blocking-pool
/// threads instead use `blocking_work` around each lookup, since their idle
/// keep-alive wait does not call those hooks.
fn set_busy(busy: bool) {
    LISTED.with(|listed| {
        if let Some(thread) = listed.borrow().as_ref() {
            thread.set_busy(busy);
        }
    });
}

fn runtime(threads: &ThreadRegistry) -> Result<Arc<Runtime>, SyncError> {
    let mut slot = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(runtime) = slot.as_ref() {
        return Ok(Arc::clone(runtime));
    }
    let registry = threads.clone();
    let first_parks = Arc::new((Mutex::new(0_usize), Condvar::new()));
    let parked = first_parks.clone();
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name(THREAD_NAME)
        .on_thread_start(move || {
            let thread = registry.register_current_owned(THREAD_NAME, Qos::UserInitiated, "jobs");
            LISTED.with(|listed| *listed.borrow_mut() = Some(thread));
        })
        .on_thread_stop(|| LISTED.with(|listed| drop(listed.borrow_mut().take())))
        .on_thread_park(move || {
            set_busy(false);
            if !HAS_PARKED.replace(true) {
                let (count, changed) = &*parked;
                *count.lock().unwrap_or_else(PoisonError::into_inner) += 1;
                changed.notify_all();
            }
        })
        .on_thread_unpark(|| set_busy(true))
        .enable_all()
        .build()
        .map_err(|err| {
            SyncError::Network(NetworkError {
                cause: NetworkCause::Connect,
                head_arrived: false,
                detail: format!("the sync runtime did not start: {err}"),
            })
        })?;
    // Workers can run their first task before an unpark hook. Do not admit
    // work until both have reached their first park; all subsequent work is
    // covered by the hooks. Pool threads (including unused spares) start idle
    // and only a blocking_work guard marks them busy.
    let (count, changed) = &*first_parks;
    let mut count = count.lock().unwrap_or_else(PoisonError::into_inner);
    while *count < 2 {
        count = changed.wait(count).unwrap_or_else(PoisonError::into_inner);
    }
    drop(count);
    let runtime = Arc::new(runtime);
    *slot = Some(Arc::clone(&runtime));
    Ok(runtime)
}

/// Runs async sync work to its end on the calling thread, which it blocks
/// (the FFI calls it off the main thread). Returns `Cancelled` within
/// 200 ms of `cancel` firing, dropping the work and so closing its
/// connections (T9). Never call it from inside the runtime.
///
/// `threads` is the engine's registry. The call that starts the runtime
/// lists its threads there until [`shut_down`]; a process has one engine, so
/// every call passes the same registry.
pub fn block_on<T>(
    threads: &ThreadRegistry,
    cancel: &CancelToken,
    work: impl Future<Output = Result<T, SyncError>>,
) -> Result<T, SyncError> {
    if cancel.is_cancelled() {
        return Err(SyncError::Cancelled);
    }
    runtime(threads)?.block_on(with_cancel(cancel, work))
}

/// Drive a stateful protocol whose owner must run cleanup after cancellation.
/// Individual requests still use T9; dropping the whole protocol would skip
/// H12's abort and undo transitions.
pub(crate) fn drive<T>(threads: &ThreadRegistry, work: impl Future<Output = T>) -> Result<T, SyncError> {
    Ok(runtime(threads)?.block_on(work))
}

/// The C8 hook: drops the runtime once no job runs on it; the next job
/// creates a new one.
pub fn shut_down() {
    let runtime = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner).take();
    if let Some(runtime) = runtime.and_then(|r| Arc::try_unwrap(r).ok()) {
        runtime.shutdown_background();
    }
}

/// `work` raced against the cancel token. Whatever `work` returned, a
/// cancelled token makes the result `Cancelled` (T9).
pub(crate) async fn with_cancel<T>(
    cancel: &CancelToken,
    work: impl Future<Output = Result<T, SyncError>>,
) -> Result<T, SyncError> {
    let result = tokio::select! {
        biased;
        () = cancel.cancelled() => Err(SyncError::Cancelled),
        result = work => result,
    };
    if cancel.is_cancelled() {
        Err(SyncError::Cancelled)
    } else {
        result
    }
}

/// Blocking work must remain visible until it actually ends, even if its
/// awaiting job is cancelled. Idle pool threads own no SQLite handles.
pub(crate) fn blocking_work<T>(work: impl FnOnce() -> T) -> T {
    struct IdleOnDrop;
    impl Drop for IdleOnDrop {
        fn drop(&mut self) {
            set_busy(false);
        }
    }
    set_busy(true);
    let _idle = IdleOnDrop;
    work()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::time::Duration;

    #[test]
    fn a_cancelled_lookup_stays_busy_until_its_blocking_closure_ends() {
        let threads = ThreadRegistry::new();
        let runner = threads.clone();
        let inspected = threads.clone();
        let cancel = Arc::new(CancelToken::new());
        let token = cancel.clone();
        let (entered, started) = mpsc::channel();
        let (release, finish) = mpsc::channel();
        let (ended, done) = mpsc::channel();
        let job = std::thread::spawn(move || {
            block_on(&runner, &token, async move {
                tokio::task::spawn_blocking(move || {
                    blocking_work(|| {
                        // Inspect this thread while the lookup closure is active.
                        assert!(
                            inspected
                                .threads()
                                .iter()
                                .any(|t| t.busy && t.suspend_owner.as_deref() == Some("jobs"))
                        );
                        entered.send(()).unwrap();
                        finish.recv().unwrap();
                    })
                })
                .await
                .unwrap();
                Ok(())
            })
        });
        started.recv_timeout(Duration::from_secs(2)).unwrap();
        cancel.cancel();
        assert_eq!(job.join().unwrap(), Err(SyncError::Cancelled));
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(1);
        while threads.busy_threads().len() != 1 {
            assert!(
                meguri_engine::monotonic::now() < deadline,
                "cancel cannot interrupt getaddrinfo: {:?}",
                threads.threads()
            );
            std::thread::yield_now();
        }
        // Queue a second closure so pool reuse also passes through the hooks.
        let runtime = runtime(&threads).unwrap();
        release.send(()).unwrap();
        runtime.block_on(async {
            tokio::task::spawn_blocking(move || blocking_work(|| ended.send(()).unwrap()))
                .await
                .unwrap();
        });
        done.recv_timeout(Duration::from_secs(2)).unwrap();
        let deadline = meguri_engine::monotonic::now() + Duration::from_secs(1);
        while !threads.busy_threads().is_empty() {
            assert!(
                meguri_engine::monotonic::now() < deadline,
                "idle pool threads: {:?}",
                threads.threads()
            );
            std::thread::sleep(Duration::from_millis(1));
        }
        drop(runtime);
        shut_down();
    }
}
