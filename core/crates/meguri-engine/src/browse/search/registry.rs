//! What every search session of one engine shares (`browser/result-sets`
//! H3): the search workers' readers, the cancel tokens of the requests that
//! run, and the hooks the lifecycle calls. A generation change (L8 step 1),
//! a suspend (U32) and a close cancel every running request, so its reader
//! comes back at once, and refuse new ones until they are done; the
//! generation change then releases every handle of the old generation
//! before `CollectionReplaced` is delivered (H6).

use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::task::Wake;

use crate::cancel::CancelToken;
use crate::lifecycle::Generation;
use crate::readers::{SEARCH_READERS, SERVER_READERS};

use super::SessionShared;

#[derive(Default)]
pub(crate) struct SearchRegistry {
    state: Mutex<State>,
    /// Notified when a reader comes back, a token is set or a halt ends.
    changed: Condvar,
    next_server_reader: AtomicUsize,
    /// Q10's chunk size when a test sets one (H16); 0 otherwise.
    stream_chunk: AtomicUsize,
    /// Handle ids are unique across the engine's sessions, so one session
    /// never takes another's handle for its own (H15).
    next_handle: AtomicU64,
}

#[derive(Default)]
struct State {
    /// Search readers in use, by index.
    busy: [bool; SEARCH_READERS],
    /// The tokens of the requests that hold a worker now.
    running: Vec<(u64, Arc<CancelToken>)>,
    next_run: u64,
    /// Halts in force: while any is, every request is cancelled.
    halts: usize,
    sessions: Vec<Weak<SessionShared>>,
    next_session: u64,
}

impl SearchRegistry {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Wakes every request waiting for a reader, so it sees its token.
    pub(super) fn wake(&self) {
        let _state = self.lock();
        self.changed.notify_all();
    }

    pub(super) fn add_session(&self, session: Weak<SessionShared>) -> u64 {
        let mut state = self.lock();
        state.sessions.retain(|weak| weak.strong_count() > 0);
        state.sessions.push(session);
        state.next_session += 1;
        state.next_session
    }

    /// A request takes its worker: its token is listed for [`Self::halt`],
    /// and set at once while a halt is in force.
    pub(super) fn run(&self, token: &Arc<CancelToken>) -> Running<'_> {
        let mut state = self.lock();
        state.next_run += 1;
        let key = state.next_run;
        if state.halts > 0 {
            token.interrupt();
        }
        state.running.push((key, Arc::clone(token)));
        Running { registry: self, key }
    }

    /// A search reader for the request whose token is `token`: waits while
    /// every one is in use, and gives none once the token is set.
    pub(super) fn reader(&self, token: &CancelToken) -> Option<ReaderSlot<'_>> {
        let mut state = self.lock();
        loop {
            if token.is_cancelled() {
                return None;
            }
            if let Some(index) = state.busy.iter().position(|busy| !busy) {
                state.busy[index] = true;
                return Some(ReaderSlot { registry: self, index });
            }
            state = self.changed.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// Q10 step 4's chunk size: `default`, or what a test set.
    pub(super) fn stream_chunk(&self, default: usize) -> usize {
        match self.stream_chunk.load(Ordering::Relaxed) {
            0 => default,
            chunk => chunk,
        }
    }

    #[cfg(feature = "test-hooks")]
    pub(super) fn set_stream_chunk(&self, chunk: usize) {
        self.stream_chunk.store(chunk, Ordering::Relaxed);
    }

    /// A handle id no session of this engine has used.
    pub(super) fn next_handle(&self) -> super::HandleId {
        super::HandleId(self.next_handle.fetch_add(1, Ordering::Relaxed) + 1)
    }

    /// The server reader the next one-shot search uses (H13).
    pub(super) fn server_reader(&self) -> usize {
        self.next_server_reader.fetch_add(1, Ordering::Relaxed) % SERVER_READERS
    }

    /// Cancels every running request; requests that start before the guard
    /// drops are cancelled as they start.
    pub(crate) fn halt(&self) -> Halt<'_> {
        let mut state = self.lock();
        state.halts += 1;
        for (_, token) in &state.running {
            token.interrupt();
        }
        self.changed.notify_all();
        Halt { registry: self }
    }

    /// H6: releases the handles of generations before `current` in every
    /// session, before `CollectionReplaced` is delivered.
    pub(crate) fn release_before(&self, current: Generation) {
        let sessions: Vec<_> = self.lock().sessions.iter().filter_map(Weak::upgrade).collect();
        for session in sessions {
            session.release_before(current);
        }
    }
}

pub(super) struct Running<'a> {
    registry: &'a SearchRegistry,
    key: u64,
}

impl Drop for Running<'_> {
    fn drop(&mut self) {
        self.registry.lock().running.retain(|(key, _)| *key != self.key);
    }
}

pub(super) struct ReaderSlot<'a> {
    registry: &'a SearchRegistry,
    pub(super) index: usize,
}

impl Drop for ReaderSlot<'_> {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        state.busy[self.index] = false;
        self.registry.changed.notify_all();
    }
}

pub(crate) struct Halt<'a> {
    registry: &'a SearchRegistry,
}

impl Drop for Halt<'_> {
    fn drop(&mut self) {
        let mut state = self.registry.lock();
        state.halts -= 1;
        self.registry.changed.notify_all();
    }
}

/// Carries a caller's cancel token over to a request's own token
/// (`engine/jobs` J4): the caller's token wakes it, which sets the request's
/// token and wakes a request waiting for a reader.
pub(super) struct Bridge {
    pub(super) request: Arc<CancelToken>,
    pub(super) registry: Weak<SearchRegistry>,
}

impl Wake for Bridge {
    fn wake(self: Arc<Self>) {
        self.request.cancel();
        if let Some(registry) = self.registry.upgrade() {
            registry.wake();
        }
    }
}

/// Links `caller` to `request`: once `caller` is cancelled, so is `request`.
pub(super) fn bridge(caller: &CancelToken, request: &Arc<CancelToken>, registry: &Arc<SearchRegistry>) {
    use std::future::Future;
    let waker = std::task::Waker::from(Arc::new(Bridge {
        request: Arc::clone(request),
        registry: Arc::downgrade(registry),
    }));
    let mut context = std::task::Context::from_waker(&waker);
    let mut cancelled = std::pin::pin!(caller.cancelled());
    if cancelled.as_mut().poll(&mut context).is_ready() {
        request.cancel();
    }
}
