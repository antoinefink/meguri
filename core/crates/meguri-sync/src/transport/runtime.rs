//! The sync runtime (KTD14, rust-stack §3): one tokio runtime per process,
//! created when a job first needs it, with two workers. Blocking entry points
//! run a job on it under a select on the job's cancel token (T9); the app may
//! drop it when it goes to the background (C8).

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};

use meguri_engine::CancelToken;
use tokio::runtime::{Builder, Runtime};

use crate::error::{NetworkCause, NetworkError, SyncError};

static RUNTIME: Mutex<Option<Arc<Runtime>>> = Mutex::new(None);

fn runtime() -> Result<Arc<Runtime>, SyncError> {
    let mut slot = RUNTIME.lock().unwrap_or_else(PoisonError::into_inner);
    if let Some(runtime) = slot.as_ref() {
        return Ok(Arc::clone(runtime));
    }
    let runtime = Builder::new_multi_thread()
        .worker_threads(2)
        .thread_name("meguri-sync")
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
pub fn block_on<T>(cancel: &CancelToken, work: impl Future<Output = Result<T, SyncError>>) -> Result<T, SyncError> {
    if cancel.is_cancelled() {
        return Err(SyncError::Cancelled);
    }
    runtime()?.block_on(with_cancel(cancel, work))
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
