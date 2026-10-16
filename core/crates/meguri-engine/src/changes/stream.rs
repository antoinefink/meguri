//! The change stream (`engine/change-stream` S3): one ordered queue of event
//! envelopes, delivered to every subscriber in `seq` order. Op seqs and event
//! seqs are one counter. An op reserves its seq at commit and fills it in
//! once the in-core observers ran; events with higher seqs published
//! meanwhile wait for it, so every subscriber sees strict `seq` order.

use std::any::Any;
use std::collections::{BTreeMap, VecDeque};
use std::fmt;
use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Condvar, Mutex, MutexGuard, PoisonError, Weak};
use std::task::{Context, Poll, Waker};
use std::time::Duration;

use meguri_model::changes::ChangeSet;

use crate::jobs::{JobId, JobOutcome};
use crate::lifecycle::{Generation, LifecycleState};
use crate::monotonic;

/// A position in the engine's one sequence: ops and events, from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Seq(pub u64);

/// An event payload from a crate above the engine (stats, sync status, media
/// arrived, derived readiness, widget snapshot): carried, never read.
pub trait ExtensionEvent: Any + Send + Sync + fmt::Debug {
    fn as_any(&self) -> &dyn Any;
}

#[derive(Clone, Debug)]
pub enum EventKind {
    /// An op committed a non-empty change set; the event's seq is the op's.
    Changes(Arc<ChangeSet>),
    /// The collection file was swapped; exactly once per generation change.
    CollectionReplaced {
        generation: Generation,
    },
    Lifecycle(LifecycleState),
    /// The collection is available again (`engine/lifecycle` L6).
    Available,
    /// A declared long write or a maintenance task started on the writer.
    WriteDelay {
        op: &'static str,
        declared: Duration,
    },
    WriteDelayEnded {
        op: &'static str,
    },
    JobFinished {
        id: JobId,
        outcome: JobOutcome,
    },
    Extension(Arc<dyn ExtensionEvent>),
}

/// One envelope.
#[derive(Clone, Debug)]
pub struct Event {
    pub seq: Seq,
    /// The generation the event belongs to; 0 when no collection is open.
    pub generation: Generation,
    /// For events derived from a change set: that op's seq.
    pub cause_seq: Option<Seq>,
    pub kind: EventKind,
}

/// The stream. Clones share it.
#[derive(Clone, Default)]
pub struct ChangeStream {
    inner: Arc<Inner>,
}

impl fmt::Debug for ChangeStream {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.inner.lock();
        f.debug_struct("ChangeStream")
            .field("next_seq", &state.next_seq)
            .field("subscribers", &state.subscribers.len())
            .finish()
    }
}

#[derive(Default)]
struct Inner {
    state: Mutex<State>,
    /// Notified when `next_release` moves (`wait_settled`).
    released: Condvar,
}

struct State {
    /// The next seq to hand out.
    next_seq: u64,
    /// The lowest seq not yet released to subscribers.
    next_release: u64,
    /// Seqs handed out but not released: `None` while reserved and unfilled,
    /// `Some(None)` when given up, `Some(Some(event))` when filled.
    pending: BTreeMap<u64, Option<Option<Arc<Event>>>>,
    subscribers: Vec<Arc<Queue>>,
}

impl Default for State {
    fn default() -> State {
        State {
            next_seq: 1,
            next_release: 1,
            pending: BTreeMap::new(),
            subscribers: Vec::new(),
        }
    }
}

impl Inner {
    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

impl Drop for Inner {
    /// Ends every subscription: receivers waiting for an event get `None`.
    fn drop(&mut self) {
        let state = self.state.get_mut().unwrap_or_else(PoisonError::into_inner);
        let mut wakers = Vec::new();
        for queue in state.subscribers.drain(..) {
            let mut queue_state = queue.lock();
            queue_state.closed = true;
            wakers.append(&mut queue_state.wakers);
            queue.ready.notify_all();
        }
        wake(wakers);
    }
}

impl State {
    fn take_seq(&mut self) -> u64 {
        let seq = self.next_seq;
        self.next_seq += 1;
        seq
    }

    /// Releases every contiguous settled seq to the subscribers; returns the
    /// wakers of async receivers to wake once the lock is dropped.
    fn release(&mut self) -> Vec<Waker> {
        let mut wakers = Vec::new();
        while let Some(Some(settled)) = self.pending.get(&self.next_release) {
            let settled = settled.clone();
            self.pending.remove(&self.next_release);
            self.next_release += 1;
            if let Some(event) = settled {
                for queue in &self.subscribers {
                    queue.push(Arc::clone(&event), &mut wakers);
                }
            }
        }
        wakers
    }
}

impl ChangeStream {
    pub fn new() -> ChangeStream {
        ChangeStream::default()
    }

    /// Publishes an event at the next seq and returns the seq.
    pub fn publish(&self, generation: Generation, cause_seq: Option<Seq>, kind: EventKind) -> Seq {
        let mut state = self.inner.lock();
        let seq = state.take_seq();
        let event = Event {
            seq: Seq(seq),
            generation,
            cause_seq,
            kind,
        };
        state.pending.insert(seq, Some(Some(Arc::new(event))));
        self.release(state);
        Seq(seq)
    }

    /// Takes the next seq for an event filled in later. Every later event
    /// waits until it is filled or given up (dropping it gives it up).
    pub fn reserve(&self) -> Reservation {
        let seq = {
            let mut state = self.inner.lock();
            let seq = state.take_seq();
            state.pending.insert(seq, None);
            seq
        };
        Reservation {
            stream: self.clone(),
            seq: Seq(seq),
            settled: false,
        }
    }

    /// Every event published from now on, in seq order.
    pub fn subscribe(&self) -> Subscription {
        let queue = Arc::new(Queue::default());
        self.inner.lock().subscribers.push(Arc::clone(&queue));
        Subscription {
            stream: Arc::downgrade(&self.inner),
            queue,
        }
    }

    /// The seq the next published event or reservation will get.
    pub fn next_seq(&self) -> Seq {
        Seq(self.inner.lock().next_seq)
    }

    /// The last seq up to which nothing is pending: every op up to it has
    /// run its in-core observers and was published or given up.
    pub(crate) fn settled_through(&self) -> Seq {
        Seq(self.inner.lock().next_release - 1)
    }

    /// Waits until [`ChangeStream::settled_through`] reaches `seq`, a seq
    /// already handed out: every op up to it has run its in-core observers.
    pub(crate) fn wait_settled(&self, seq: Seq) {
        let state = self.inner.lock();
        let _settled = self
            .inner
            .released
            .wait_while(state, |state| state.next_release <= seq.0)
            .unwrap_or_else(PoisonError::into_inner);
    }

    fn settle(&self, seq: Seq, event: Option<Event>) {
        let mut state = self.inner.lock();
        state.pending.insert(seq.0, Some(event.map(Arc::new)));
        self.release(state);
    }

    /// Releases what is settled, then, the lock dropped, wakes
    /// `wait_settled` and async receivers.
    fn release(&self, mut state: MutexGuard<'_, State>) {
        let from = state.next_release;
        let wakers = state.release();
        let moved = state.next_release != from;
        drop(state);
        if moved {
            self.inner.released.notify_all();
        }
        wake(wakers);
    }
}

fn wake(wakers: Vec<Waker>) {
    for waker in wakers {
        waker.wake();
    }
}

/// A seq taken for an event filled in later (an op's change set after its
/// observers ran).
#[derive(Debug)]
pub struct Reservation {
    stream: ChangeStream,
    seq: Seq,
    settled: bool,
}

impl Reservation {
    pub fn seq(&self) -> Seq {
        self.seq
    }

    /// Publishes the event at the reserved seq.
    pub fn fill(mut self, generation: Generation, cause_seq: Option<Seq>, kind: EventKind) {
        self.settled = true;
        let event = Event {
            seq: self.seq,
            generation,
            cause_seq,
            kind,
        };
        self.stream.settle(self.seq, Some(event));
    }

    /// Publishes nothing at the reserved seq; later events go on.
    pub fn give_up(mut self) {
        self.settled = true;
        self.stream.settle(self.seq, None);
    }
}

impl Drop for Reservation {
    fn drop(&mut self) {
        if !self.settled {
            self.stream.settle(self.seq, None);
        }
    }
}

/// One subscriber's queue.
#[derive(Default)]
struct Queue {
    state: Mutex<QueueState>,
    ready: Condvar,
}

#[derive(Default)]
struct QueueState {
    events: VecDeque<Arc<Event>>,
    wakers: Vec<Waker>,
    closed: bool,
}

impl Queue {
    fn lock(&self) -> std::sync::MutexGuard<'_, QueueState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn push(&self, event: Arc<Event>, wakers: &mut Vec<Waker>) {
        let mut state = self.lock();
        state.events.push_back(event);
        wakers.append(&mut state.wakers);
        self.ready.notify_all();
    }
}

/// A subscription: receives each event published after it was made, once,
/// in seq order. Dropping it ends it.
pub struct Subscription {
    stream: Weak<Inner>,
    queue: Arc<Queue>,
}

impl fmt::Debug for Subscription {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Subscription")
            .field("queued", &self.queue.lock().events.len())
            .finish()
    }
}

impl Subscription {
    /// The next event, waiting as long as it takes. `None` once the stream
    /// is gone (or the subscription dropped) and the queue is empty.
    pub fn recv(&self) -> Option<Arc<Event>> {
        let mut state = self.queue.lock();
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            if state.closed || self.stream.strong_count() == 0 {
                return None;
            }
            // No timed wake-ups while idle (R1854): a dropped stream closes
            // every queue and notifies it.
            state = self.queue.ready.wait(state).unwrap_or_else(PoisonError::into_inner);
        }
    }

    /// The next event, or `None` after `timeout`.
    pub fn recv_timeout(&self, timeout: Duration) -> Option<Arc<Event>> {
        let deadline = monotonic::now() + timeout;
        let mut state = self.queue.lock();
        loop {
            if let Some(event) = state.events.pop_front() {
                return Some(event);
            }
            let now = monotonic::now();
            if now >= deadline {
                return None;
            }
            state = self
                .queue
                .ready
                .wait_timeout(state, deadline - now)
                .unwrap_or_else(PoisonError::into_inner)
                .0;
        }
    }

    /// The next event if one is queued.
    pub fn try_recv(&self) -> Option<Arc<Event>> {
        self.queue.lock().events.pop_front()
    }

    /// The next event as a future, for async receivers (never on the
    /// writer thread).
    pub fn next(&self) -> Next<'_> {
        Next { subscription: self }
    }
}

impl Drop for Subscription {
    fn drop(&mut self) {
        self.queue.lock().closed = true;
        if let Some(inner) = self.stream.upgrade() {
            inner.lock().subscribers.retain(|q| !Arc::ptr_eq(q, &self.queue));
        }
    }
}

/// The future of [`Subscription::next`].
pub struct Next<'a> {
    subscription: &'a Subscription,
}

impl Future for Next<'_> {
    /// `None` once the stream is gone and the queue is empty.
    type Output = Option<Arc<Event>>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Arc<Event>>> {
        let mut state = self.subscription.queue.lock();
        if let Some(event) = state.events.pop_front() {
            return Poll::Ready(Some(event));
        }
        if state.closed {
            return Poll::Ready(None);
        }
        if !state.wakers.iter().any(|w| w.will_wake(cx.waker())) {
            state.wakers.push(cx.waker().clone());
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seqs(subscription: &Subscription) -> Vec<u64> {
        std::iter::from_fn(|| subscription.try_recv())
            .map(|e| e.seq.0)
            .collect()
    }

    #[test]
    fn an_event_after_an_open_reservation_waits_for_it_and_a_given_up_seq_is_skipped() {
        let stream = ChangeStream::new();
        let subscription = stream.subscribe();
        let first = stream.reserve();
        let second = stream.reserve();
        assert_eq!(stream.publish(Generation(1), None, EventKind::Available), Seq(3));
        assert!(seqs(&subscription).is_empty(), "seq 3 waits for 1 and 2");
        second.fill(Generation(1), Some(Seq(1)), EventKind::Available);
        assert!(seqs(&subscription).is_empty(), "seq 2 waits for 1");
        first.give_up();
        assert_eq!(seqs(&subscription), [2, 3]);
        drop(stream.reserve());
        stream.publish(Generation(1), None, EventKind::Available);
        assert_eq!(seqs(&subscription), [5], "a dropped reservation is given up");
    }

    #[test]
    fn wait_settled_returns_once_every_seq_up_to_it_is_filled_or_given_up() {
        let stream = ChangeStream::new();
        stream.wait_settled(Seq(0));
        let first = stream.reserve();
        let second = stream.reserve();
        let waiter = {
            let stream = stream.clone();
            std::thread::spawn(move || stream.wait_settled(Seq(2)))
        };
        second.fill(Generation(1), None, EventKind::Available);
        std::thread::sleep(Duration::from_millis(20));
        assert!(!waiter.is_finished(), "seq 1 is still pending");
        first.give_up();
        waiter.join().unwrap();
        assert_eq!(stream.settled_through(), Seq(2));
    }

    #[test]
    fn a_subscription_sees_only_later_events_and_a_dropped_one_stops_receiving() {
        let stream = ChangeStream::new();
        stream.publish(Generation(0), None, EventKind::Available);
        let early = stream.subscribe();
        let late = stream.subscribe();
        stream.publish(Generation(0), None, EventKind::Available);
        assert_eq!(seqs(&early), [2]);
        drop(late);
        stream.publish(Generation(0), None, EventKind::Available);
        assert_eq!(seqs(&early), [3]);
        assert_eq!(stream.inner.lock().subscribers.len(), 1);
    }

    #[test]
    fn an_async_receiver_wakes_when_an_event_is_published() {
        use std::sync::Arc as StdArc;
        use std::task::Wake;

        struct Flag(std::sync::atomic::AtomicBool);
        impl Wake for Flag {
            fn wake(self: StdArc<Self>) {
                self.0.store(true, std::sync::atomic::Ordering::SeqCst);
            }
        }
        let stream = ChangeStream::new();
        let subscription = stream.subscribe();
        let flag = StdArc::new(Flag(std::sync::atomic::AtomicBool::new(false)));
        let waker = Waker::from(flag.clone());
        let mut cx = Context::from_waker(&waker);
        let mut next = std::pin::pin!(subscription.next());
        assert!(next.as_mut().poll(&mut cx).is_pending());
        stream.publish(Generation(0), None, EventKind::Available);
        assert!(flag.0.load(std::sync::atomic::Ordering::SeqCst), "woken");
        match next.as_mut().poll(&mut cx) {
            Poll::Ready(Some(event)) => assert_eq!(event.seq, Seq(1)),
            other => panic!("the event is there: {other:?}"),
        }
        let mut after = std::pin::pin!(subscription.next());
        assert!(after.as_mut().poll(&mut cx).is_pending());
        drop(stream);
        assert!(
            matches!(after.as_mut().poll(&mut cx), Poll::Ready(None)),
            "a dropped stream ends it"
        );
        assert!(subscription.recv().is_none());
    }
}
