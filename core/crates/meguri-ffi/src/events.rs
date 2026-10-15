//! The core event channel (KTD9). One `CoreEvent` envelope carries every
//! core→Swift event except progress, host events and search results, which
//! have their own channels. The engine's change stream (U24,
//! `engine/change-stream` S3) is the one ordered queue; a single dispatcher
//! thread, `meguri-events`, holds a subscription to it and delivers each
//! envelope to the Swift listener in `seq` order, never from a producer
//! thread and never while holding a lock. Events published before Swift
//! attaches its listener wait in the subscription.
//!
//! Variants are added by their owning units (KTD9 lists them). Stream kinds
//! that have no `CoreEventKind` variant yet are skipped (U37 adds them).

use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};

use meguri_engine::{ChangeStream, Event, EventKind, Generation, JobId, Qos, Seq, Subscription, ThreadRegistry};

use crate::callback;
use crate::error::CoreError;
use crate::jobs::JobOutcome;
use crate::runtime;

/// Name of the dispatcher thread; Swift tests assert it.
pub const DISPATCHER_THREAD_NAME: &str = "meguri-events";

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CoreEvent {
    /// Strictly increasing over the process's lifetime, from 1: the engine
    /// stream's seq.
    pub seq: u64,
    /// The collection generation the event belongs to; 0 before any
    /// collection is open.
    pub generation: u64,
    /// For derived events, the `seq` of the change that caused them.
    pub cause_seq: Option<u64>,
    pub kind: CoreEventKind,
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CoreEventKind {
    /// A job started by a `start_*` export ended (KTD9). Exactly one per job.
    JobFinished { id: u64, outcome: JobOutcome },
}

/// Implemented in Swift by `CoreEventStream`, which only yields each event
/// into an unbounded `AsyncStream` continuation.
#[uniffi::export(foreign)]
pub trait CoreEventListener: Send + Sync {
    fn on_event(&self, event: CoreEvent);
}

/// Sets the listener that receives every event from now on, replacing any
/// earlier one, and starts the dispatcher if it is not running.
#[uniffi::export]
pub fn set_event_listener(listener: Arc<dyn CoreEventListener>) -> Result<(), CoreError> {
    callback::enter("set_event_listener");
    dispatcher()?.set_listener(listener);
    Ok(())
}

/// Publishes an event into the process engine's change stream and returns
/// its `seq`.
pub fn publish(generation: u64, cause_seq: Option<u64>, kind: CoreEventKind) -> Result<u64, CoreError> {
    dispatcher()?;
    let stream = runtime::engine()?.stream();
    Ok(stream
        .publish(Generation(generation), cause_seq.map(Seq), stream_kind(kind))
        .0)
}

/// The stream kind of a `CoreEvent` kind.
fn stream_kind(kind: CoreEventKind) -> EventKind {
    match kind {
        CoreEventKind::JobFinished { id, outcome } => EventKind::JobFinished {
            id: JobId(id),
            outcome: outcome.into(),
        },
    }
}

/// The `CoreEvent` of a stream event; `None` for kinds Swift does not get
/// yet.
fn core_event(event: &Event) -> Option<CoreEvent> {
    let kind = match &event.kind {
        EventKind::JobFinished { id, outcome } => CoreEventKind::JobFinished {
            id: id.0,
            outcome: outcome.clone().into(),
        },
        EventKind::Changes(_)
        | EventKind::CollectionReplaced { .. }
        | EventKind::Lifecycle(_)
        | EventKind::Available
        | EventKind::WriteDelay { .. }
        | EventKind::WriteDelayEnded { .. }
        | EventKind::Extension(_) => return None,
    };
    Some(CoreEvent {
        seq: event.seq.0,
        generation: event.generation.0,
        cause_seq: event.cause_seq.map(|s| s.0),
        kind,
    })
}

/// The process dispatcher, subscribed to the process engine's stream.
fn dispatcher() -> Result<&'static Arc<Dispatcher>, CoreError> {
    static DISPATCHER: OnceLock<Result<Arc<Dispatcher>, CoreError>> = OnceLock::new();
    DISPATCHER
        .get_or_init(|| {
            let engine = runtime::engine()?;
            Dispatcher::start(engine.stream(), engine.threads())
        })
        .as_ref()
        .map_err(Clone::clone)
}

struct Dispatcher {
    subscription: Subscription,
    listener: Mutex<Option<Arc<dyn CoreEventListener>>>,
    attached: Condvar,
}

impl Dispatcher {
    /// Subscribes to `stream`, then starts the delivery thread through
    /// `threads`.
    fn start(stream: &ChangeStream, threads: &ThreadRegistry) -> Result<Arc<Dispatcher>, CoreError> {
        let dispatcher = Arc::new(Dispatcher {
            subscription: stream.subscribe(),
            listener: Mutex::new(None),
            attached: Condvar::new(),
        });
        let worker = Arc::clone(&dispatcher);
        threads
            .spawn(DISPATCHER_THREAD_NAME, Qos::UserInitiated, move |_| worker.run())
            .map_err(|e| CoreError::Internal {
                message: format!("cannot start the event dispatcher: {e}"),
            })?;
        Ok(dispatcher)
    }

    fn set_listener(&self, listener: Arc<dyn CoreEventListener>) {
        *self.listener.lock().unwrap_or_else(PoisonError::into_inner) = Some(listener);
        self.attached.notify_all();
    }

    /// The listener, waiting until there is one.
    fn listener(&self) -> Arc<dyn CoreEventListener> {
        let mut listener = self.listener.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(listener) = listener.clone() {
                return listener;
            }
            listener = self.attached.wait(listener).unwrap_or_else(PoisonError::into_inner);
        }
    }

    fn run(&self) {
        loop {
            // Events stay in the subscription until a listener is attached.
            self.listener();
            let Some(event) = self.subscription.recv() else {
                return;
            };
            let Some(event) = core_event(&event) else {
                continue;
            };
            let listener = self.listener();
            // A panicking listener must not stop delivery to later events.
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback::invoke("CoreEventListener::on_event", || listener.on_event(event))
            }));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;
    use std::thread;
    use std::time::Duration;

    use meguri_engine::{ChangeSet, OpSource};

    struct Recorder(mpsc::Sender<(CoreEvent, Option<String>)>);
    impl CoreEventListener for Recorder {
        fn on_event(&self, event: CoreEvent) {
            let thread = thread::current().name().map(str::to_owned);
            self.0.send((event, thread)).unwrap();
        }
    }

    fn finished(id: u64) -> EventKind {
        stream_kind(CoreEventKind::JobFinished {
            id,
            outcome: JobOutcome::Succeeded,
        })
    }

    fn start() -> (ChangeStream, Arc<Dispatcher>) {
        let stream = ChangeStream::new();
        let dispatcher = Dispatcher::start(&stream, &ThreadRegistry::new()).unwrap();
        (stream, dispatcher)
    }

    #[test]
    fn events_from_four_producers_arrive_in_seq_order_on_the_dispatcher_thread() {
        let (stream, dispatcher) = start();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        let producers: Vec<_> = (0..4u64)
            .map(|p| {
                let stream = stream.clone();
                thread::Builder::new()
                    .name(format!("producer-{p}"))
                    .spawn(move || {
                        for i in 0..2_500 {
                            stream.publish(Generation(0), None, finished(p * 10_000 + i));
                        }
                    })
                    .unwrap()
            })
            .collect();
        for producer in producers {
            producer.join().unwrap();
        }
        let mut last = 0;
        for _ in 0..10_000 {
            let (event, thread) = rx.recv_timeout(Duration::from_secs(10)).expect("event not delivered");
            assert!(event.seq > last, "seq {} after {last}", event.seq);
            last = event.seq;
            assert_eq!(thread.as_deref(), Some(DISPATCHER_THREAD_NAME));
        }
        assert_eq!(last, 10_000);
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "more than 10,000 events"
        );
    }

    #[test]
    fn engine_envelopes_are_delivered_once_in_seq_order_with_op_reservations_between_them() {
        let (stream, dispatcher) = start();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        // An op reserves its seq at commit and publishes after its observers;
        // a job ending meanwhile waits behind it.
        let op = stream.reserve();
        let job = stream.publish(Generation(1), None, finished(1));
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "held behind the op"
        );
        op.fill(
            Generation(1),
            None,
            EventKind::Changes(Arc::new(ChangeSet::empty(OpSource::UserUndoable))),
        );
        let empty_op = stream.reserve();
        let second = stream.publish(Generation(1), None, finished(2));
        empty_op.give_up();
        let delivered: Vec<u64> = (0..2)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap().0.seq)
            .collect();
        assert_eq!(delivered, [job.0, second.0], "each once, in seq order");
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn events_published_before_the_listener_wait_for_it() {
        let (stream, dispatcher) = start();
        assert_eq!(stream.publish(Generation(3), Some(Seq(1)), finished(1)), Seq(1));
        assert_eq!(stream.publish(Generation(3), None, finished(2)), Seq(2));
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        let (first, _) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            first,
            CoreEvent {
                seq: 1,
                generation: 3,
                cause_seq: Some(1),
                kind: CoreEventKind::JobFinished {
                    id: 1,
                    outcome: JobOutcome::Succeeded
                }
            }
        );
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap().0.seq, 2);
    }

    #[test]
    fn a_panicking_listener_does_not_stop_delivery() {
        struct PanicsOnFirst(Mutex<bool>, mpsc::Sender<u64>);
        impl CoreEventListener for PanicsOnFirst {
            fn on_event(&self, event: CoreEvent) {
                let first = std::mem::replace(&mut *self.0.lock().unwrap(), false);
                assert!(!first, "listener failure");
                self.1.send(event.seq).unwrap();
            }
        }
        let (stream, dispatcher) = start();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(PanicsOnFirst(Mutex::new(true), tx)));
        stream.publish(Generation(0), None, finished(1));
        stream.publish(Generation(0), None, finished(2));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    }

    #[test]
    fn the_listener_runs_without_the_dispatcher_lock_held() {
        struct Publishes(ChangeStream, mpsc::Sender<u64>);
        impl CoreEventListener for Publishes {
            fn on_event(&self, event: CoreEvent) {
                // Would deadlock if the dispatcher held a lock here.
                if event.seq == 1 {
                    self.0.publish(Generation(0), Some(Seq(1)), finished(2));
                }
                self.1.send(event.seq).unwrap();
            }
        }
        let (stream, dispatcher) = start();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Publishes(stream.clone(), tx)));
        stream.publish(Generation(0), None, finished(1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    }
}
