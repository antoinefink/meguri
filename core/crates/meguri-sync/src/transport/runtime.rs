//! The sync runtime (KTD14, rust-stack §3): one tokio runtime per process,
//! created when a job first needs it, with two workers. Blocking entry points
//! run a job on it under a select on the job's cancel token (T9); the app may
//! drop it when it goes to the background (C8).
//!
//! Its threads, workers and blocking-pool threads alike, list themselves in
//! the engine's thread registry from their start to their end (`engine/jobs`
//! J6, KTD9), at user-initiated QoS like the job pool's (J2: the user started
//! the sync and watches it).

use std::cell::RefCell;
use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

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
}

/// Busy or idle, from tokio's park hooks. Only workers park through them: a
/// blocking-pool thread stays busy from its start until it ends, so it never
/// looks idle while it runs a name lookup. Shutting the runtime down ends its
/// idle blocking threads at once.
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
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name(THREAD_NAME)
        .on_thread_start(move || {
            let thread = registry.register_current(THREAD_NAME, Qos::UserInitiated);
            thread.set_busy(true);
            LISTED.with(|listed| *listed.borrow_mut() = Some(thread));
        })
        .on_thread_stop(|| LISTED.with(|listed| drop(listed.borrow_mut().take())))
        .on_thread_park(|| set_busy(false))
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
