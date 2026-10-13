//! The core event channel (KTD9). One `CoreEvent` envelope carries every
//! core→Swift event except progress, host events and search results, which
//! have their own channels. Producers publish into one ordered queue; a single
//! dispatcher thread, `meguri-events`, delivers each envelope to the Swift
//! listener in `seq` order, never from a producer thread and never while
//! holding a lock. Events published before Swift attaches its listener wait
//! in the queue.
//!
//! Variants are added by their owning units (KTD9 lists them). The engine
//! change stream (U24) feeds this queue once it exists.

use std::collections::VecDeque;
use std::sync::{Arc, Condvar, Mutex, OnceLock, PoisonError};
use std::thread;

use crate::callback;
use crate::error::CoreError;
use crate::jobs::JobOutcome;

/// Name of the dispatcher thread; Swift tests assert it.
pub const DISPATCHER_THREAD_NAME: &str = "meguri-events";

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CoreEvent {
    /// Strictly increasing over the process's lifetime, from 1.
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

/// Publishes an event into the process-wide channel and returns its `seq`.
pub fn publish(generation: u64, cause_seq: Option<u64>, kind: CoreEventKind) -> Result<u64, CoreError> {
    Ok(dispatcher()?.publish(generation, cause_seq, kind))
}

fn dispatcher() -> Result<&'static Arc<Dispatcher>, CoreError> {
    static DISPATCHER: OnceLock<Result<Arc<Dispatcher>, CoreError>> = OnceLock::new();
    DISPATCHER.get_or_init(Dispatcher::start).as_ref().map_err(Clone::clone)
}

struct Dispatcher {
    state: Mutex<State>,
    wake: Condvar,
}

struct State {
    next_seq: u64,
    queue: VecDeque<CoreEvent>,
    listener: Option<Arc<dyn CoreEventListener>>,
}

impl Dispatcher {
    /// Creates a dispatcher and its delivery thread.
    fn start() -> Result<Arc<Dispatcher>, CoreError> {
        let dispatcher = Arc::new(Dispatcher {
            state: Mutex::new(State {
                next_seq: 1,
                queue: VecDeque::new(),
                listener: None,
            }),
            wake: Condvar::new(),
        });
        let worker = Arc::clone(&dispatcher);
        thread::Builder::new()
            .name(DISPATCHER_THREAD_NAME.to_owned())
            .spawn(move || worker.run())
            .map_err(|e| CoreError::Internal {
                message: format!("cannot start the event dispatcher: {e}"),
            })?;
        Ok(dispatcher)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn set_listener(&self, listener: Arc<dyn CoreEventListener>) {
        self.lock().listener = Some(listener);
        self.wake.notify_all();
    }

    /// Assigns the next `seq` and enqueues under one lock, so the queue order
    /// is the `seq` order whatever thread publishes.
    fn publish(&self, generation: u64, cause_seq: Option<u64>, kind: CoreEventKind) -> u64 {
        let seq = {
            let mut state = self.lock();
            let seq = state.next_seq;
            state.next_seq += 1;
            state.queue.push_back(CoreEvent {
                seq,
                generation,
                cause_seq,
                kind,
            });
            seq
        };
        self.wake.notify_all();
        seq
    }

    fn run(&self) {
        loop {
            let (event, listener) = {
                let mut state = self.lock();
                loop {
                    if let Some(listener) = state.listener.clone()
                        && let Some(event) = state.queue.pop_front()
                    {
                        break (event, listener);
                    }
                    state = self.wake.wait(state).unwrap_or_else(PoisonError::into_inner);
                }
            };
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
    use std::time::Duration;

    struct Recorder(mpsc::Sender<(CoreEvent, Option<String>)>);
    impl CoreEventListener for Recorder {
        fn on_event(&self, event: CoreEvent) {
            let thread = thread::current().name().map(str::to_owned);
            self.0.send((event, thread)).unwrap();
        }
    }

    fn finished(id: u64) -> CoreEventKind {
        CoreEventKind::JobFinished {
            id,
            outcome: JobOutcome::Succeeded,
        }
    }

    #[test]
    fn events_from_four_producers_arrive_in_seq_order_on_the_dispatcher_thread() {
        let dispatcher = Dispatcher::start().unwrap();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        let producers: Vec<_> = (0..4u64)
            .map(|p| {
                let dispatcher = Arc::clone(&dispatcher);
                thread::Builder::new()
                    .name(format!("producer-{p}"))
                    .spawn(move || {
                        for i in 0..2_500 {
                            dispatcher.publish(0, None, finished(p * 10_000 + i));
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
    fn events_published_before_the_listener_wait_for_it() {
        let dispatcher = Dispatcher::start().unwrap();
        assert_eq!(dispatcher.publish(3, Some(1), finished(1)), 1);
        assert_eq!(dispatcher.publish(3, None, finished(2)), 2);
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        let (first, _) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(
            first,
            CoreEvent {
                seq: 1,
                generation: 3,
                cause_seq: Some(1),
                kind: finished(1)
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
        let dispatcher = Dispatcher::start().unwrap();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(PanicsOnFirst(Mutex::new(true), tx)));
        dispatcher.publish(0, None, finished(1));
        dispatcher.publish(0, None, finished(2));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    }

    #[test]
    fn the_listener_runs_without_the_dispatcher_lock_held() {
        struct Publishes(Arc<Dispatcher>, mpsc::Sender<u64>);
        impl CoreEventListener for Publishes {
            fn on_event(&self, event: CoreEvent) {
                // Would deadlock if the dispatcher held its state lock here.
                if event.seq == 1 {
                    self.0.publish(0, Some(1), finished(2));
                }
                self.1.send(event.seq).unwrap();
            }
        }
        let dispatcher = Dispatcher::start().unwrap();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Publishes(Arc::clone(&dispatcher), tx)));
        dispatcher.publish(0, None, finished(1));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 1);
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), 2);
    }
}
