//! The core event channel (KTD9). One `CoreEvent` envelope carries every
//! core→Swift event except progress, host events and search results, which
//! have their own channels. The engine's change stream (U24,
//! `engine/change-stream` S3) is the one ordered queue; a single dispatcher
//! thread, `meguri-events`, holds a subscription to it and delivers each
//! envelope to the Swift listener in `seq` order, never from a producer
//! thread and never while holding a lock. The runtime starts it with the
//! process engine, so events published before Swift attaches its listener
//! wait in the subscription.
//!
//! Variants are added by their owning units (KTD9 lists them). Stream kinds
//! that have no `CoreEventKind` variant yet (write
//! delays, extensions) are skipped until their units add one.

use std::sync::{Arc, Condvar, Mutex, PoisonError};

use meguri_engine::{ChangeStream, Event, EventKind, Generation, JobId, Qos, Seq, Subscription, ThreadRegistry};

use crate::callback;
use crate::changes::ChangeSet;
use crate::day_clock;
use crate::error::CoreError;
use crate::jobs::JobOutcome;
use crate::review_session::*;
use crate::runtime;

/// Name of the dispatcher thread; Swift tests assert it.
pub const DISPATCHER_THREAD_NAME: &str = "meguri-events";

#[derive(Clone, Debug, PartialEq, uniffi::Record)]
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

// Lowered into a buffer for Swift by value; one per event, never stored.
#[allow(clippy::large_enum_variant)]
#[derive(Clone, Debug, PartialEq, uniffi::Enum)]
pub enum CoreEventKind {
    MaintenanceDeadline {
        at: Option<i64>,
    },
    Lifecycle {
        state: crate::lifecycle::CollectionLifecycle,
    },
    Available,
    MediaArrived {
        names: Vec<String>,
    },
    SessionStarted {
        session_id: u64,
        deck_id: i64,
    },
    SessionEnded {
        session_id: u64,
    },
    QueueHeadChanged {
        session_id: u64,
        diff: ReviewHeadDiff,
    },
    CurrentCardChanged {
        session_id: u64,
        card_id: i64,
        change: ReviewCurrentChange,
    },
    CommitFailed {
        session_id: u64,
        card_id: i64,
        error: CoreError,
    },
    WaitingUntil {
        session_id: u64,
        instant: i64,
    },
    Finished {
        session_id: u64,
        state: ReviewDeckStudyState,
    },
    PresetUnsupported {
        session_id: u64,
        preset_ids: Vec<i64>,
    },
    RecoveryDropped {
        event: u64,
        card_id: i64,
        reason: String,
    },

    /// An op committed these changes (U24, `engine/change-stream` S1); the
    /// envelope's `seq` is the op's.
    Changes {
        changes: ChangeSet,
    },
    /// The collection file was replaced and `generation` is open now
    /// (`engine/lifecycle` L8); exactly once per replacement.
    CollectionReplaced {
        generation: u64,
    },
    /// A job started by a `start_*` export ended (KTD9). Exactly one per job.
    JobFinished {
        id: u64,
        outcome: JobOutcome,
    },
    /// The scheduler day changed, either way (U40, `sched/day-clock` D8),
    /// after the rollover op committed: queues and counts are stale.
    RolledOver {
        old_day: u32,
        new_day: u32,
    },
    /// The device's IANA time zone differs from the one the day clock last
    /// saw, even when the day and the offset did not change (D8).
    TimeZoneChanged {
        zone_id: String,
    },
    /// The next day boundary moved (D8), epoch seconds: re-arm the day
    /// clock's timer (U51).
    NextDayAtChanged {
        next_day_at: i64,
    },
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
    runtime::dispatcher()?.set_listener(listener);
    Ok(())
}

/// Publishes an event into the process engine's change stream and returns
/// its `seq`. `Changes` and `CollectionReplaced` come only from the engine's
/// executor and lifecycle: they are refused here.
pub fn publish(generation: u64, cause_seq: Option<u64>, kind: CoreEventKind) -> Result<u64, CoreError> {
    let kind = stream_kind(kind)?;
    let stream = runtime::engine()?.stream();
    Ok(stream.publish(Generation(generation), cause_seq.map(Seq), kind).0)
}

/// The stream kind of a `CoreEvent` kind that may be published from here.
fn stream_kind(kind: CoreEventKind) -> Result<EventKind, CoreError> {
    match kind {
        CoreEventKind::JobFinished { id, outcome } => Ok(EventKind::JobFinished {
            id: JobId(id),
            outcome: outcome.into(),
        }),
        CoreEventKind::MaintenanceDeadline { .. }
        | CoreEventKind::Lifecycle { .. }
        | CoreEventKind::MediaArrived { .. }
        | CoreEventKind::Available
        | CoreEventKind::SessionStarted { .. }
        | CoreEventKind::SessionEnded { .. }
        | CoreEventKind::QueueHeadChanged { .. }
        | CoreEventKind::CurrentCardChanged { .. }
        | CoreEventKind::CommitFailed { .. }
        | CoreEventKind::WaitingUntil { .. }
        | CoreEventKind::Finished { .. }
        | CoreEventKind::PresetUnsupported { .. }
        | CoreEventKind::RecoveryDropped { .. }
        | CoreEventKind::Changes { .. }
        | CoreEventKind::CollectionReplaced { .. }
        | CoreEventKind::RolledOver { .. }
        | CoreEventKind::TimeZoneChanged { .. }
        | CoreEventKind::NextDayAtChanged { .. } => Err(CoreError::InvalidArgument {
            message: "only the engine publishes changes, lifecycle, day clock and review session events".to_owned(),
        }),
    }
}

/// The `CoreEvent` of a stream event; `None` for kinds Swift does not get
/// yet.
fn core_event(event: &Event) -> Option<CoreEvent> {
    let kind = match &event.kind {
        EventKind::Changes(changes) => CoreEventKind::Changes {
            changes: changes.as_ref().into(),
        },
        EventKind::CollectionReplaced { generation } => CoreEventKind::CollectionReplaced {
            generation: generation.0,
        },
        EventKind::JobFinished { id, outcome } => CoreEventKind::JobFinished {
            id: id.0,
            outcome: outcome.clone().into(),
        },
        kind @ (EventKind::RolledOver { .. }
        | EventKind::TimeZoneChanged { .. }
        | EventKind::NextDayAtChanged { .. }) => day_clock::core_event_kind(kind)?,
        EventKind::Extension(value) => {
            if let Some(media) = value.as_any().downcast_ref::<meguri_engine::media::MediaArrived>() {
                CoreEventKind::MediaArrived {
                    names: media.names.clone(),
                }
            } else {
                session_event_kind(value.as_any().downcast_ref::<meguri_engine::session::SessionEvent>()?)
            }
        }
        EventKind::Lifecycle(state) => CoreEventKind::Lifecycle {
            state: state.clone().into(),
        },
        EventKind::Available => CoreEventKind::Available,
        EventKind::MaintenanceDeadline { at } => CoreEventKind::MaintenanceDeadline { at: *at },
        EventKind::WriteDelay { .. } | EventKind::WriteDelayEnded { .. } => return None,
    };
    Some(CoreEvent {
        seq: event.seq.0,
        generation: event.generation.0,
        cause_seq: event.cause_seq.map(|s| s.0),
        kind,
    })
}

/// The delivery thread and its subscription; the process's one comes from
/// `runtime::dispatcher()`.
pub(crate) struct Dispatcher {
    subscription: Subscription,
    listener: Mutex<Option<Arc<dyn CoreEventListener>>>,
    attached: Condvar,
}

impl Dispatcher {
    /// Subscribes to `stream`, then starts the delivery thread through
    /// `threads`.
    pub(crate) fn start(stream: &ChangeStream, threads: &ThreadRegistry) -> Result<Arc<Dispatcher>, CoreError> {
        let dispatcher = Arc::new(Dispatcher {
            subscription: stream.subscribe(),
            listener: Mutex::new(None),
            attached: Condvar::new(),
        });
        let worker = Arc::clone(&dispatcher);
        threads
            .spawn_owned(DISPATCHER_THREAD_NAME, Qos::UserInitiated, "events", move |_| {
                worker.run()
            })
            .map_err(|e| CoreError::Internal {
                message: format!("cannot start the event dispatcher: {e}"),
            })?;
        Ok(dispatcher)
    }

    pub(crate) fn set_listener(&self, listener: Arc<dyn CoreEventListener>) {
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

    use crate::changes::{self, CardChange, ChangeKind, IdChange, NameChange, NotetypeChange, OpSource};

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
        .unwrap()
    }

    /// `changes::sample()` as Swift must receive it, written out.
    fn sample_record() -> ChangeSet {
        let id = |id, kind| IdChange { id, kind };
        let name = |name: &str, kind| NameChange {
            name: name.to_owned(),
            kind,
        };
        ChangeSet {
            whole_collection: false,
            source: OpSource::LocalApi,
            cards: vec![
                CardChange {
                    id: 11,
                    note_id: 21,
                    kind: ChangeKind::Updated,
                    memory_state_only: true,
                },
                CardChange {
                    id: 12,
                    note_id: 21,
                    kind: ChangeKind::Added,
                    memory_state_only: false,
                },
            ],
            notes: vec![id(21, ChangeKind::Added)],
            notetypes: vec![NotetypeChange {
                id: 31,
                kind: ChangeKind::Updated,
                schema_changed: true,
            }],
            decks: vec![id(41, ChangeKind::Removed)],
            deck_configs: vec![id(51, ChangeKind::Added)],
            config_keys: vec![name("curDeck", ChangeKind::Updated)],
            tags: vec![name("leech", ChangeKind::Added)],
            revlog_added: vec![61],
            revlog_removed: vec![62],
            media_added: vec!["a.jpg".to_owned()],
            media_removed: vec!["b.mp3".to_owned()],
            skipped_incoming_cards: vec![71, 72],
            skipped_incoming_notes: vec![71, 73],
            skipped_incoming_decks: vec![71, 74],
            collection_row: true,
        }
    }

    /// A change set naming only card `id`.
    fn card_changed(id: i64) -> EventKind {
        let mut changes = changes::sample();
        changes.cards.truncate(1);
        changes.cards[0].id = meguri_model::ids::CardId(id);
        EventKind::Changes(Arc::new(changes))
    }

    fn start() -> (ChangeStream, Arc<Dispatcher>) {
        let stream = ChangeStream::new();
        let dispatcher = Dispatcher::start(&stream, &ThreadRegistry::new()).unwrap();
        (stream, dispatcher)
    }

    #[test]
    fn dispatcher_has_a_non_sqlite_suspend_owner() {
        let engine = meguri_engine::Engine::new(meguri_engine::EngineConfig::default()).unwrap();
        let _dispatcher = Dispatcher::start(engine.stream(), engine.threads()).unwrap();
        let thread = engine
            .threads()
            .threads()
            .into_iter()
            .find(|t| t.name == DISPATCHER_THREAD_NAME)
            .unwrap();
        assert_eq!(thread.suspend_owner.as_deref(), Some("events"));
        assert!(!thread.busy);
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
        let op_seq = op.seq().0;
        let job = stream.publish(Generation(1), None, finished(1));
        assert!(
            rx.recv_timeout(Duration::from_millis(50)).is_err(),
            "held behind the op"
        );
        op.fill(Generation(1), None, EventKind::Changes(Arc::new(changes::sample())));
        let empty_op = stream.reserve();
        let second = stream.publish(Generation(1), None, finished(2));
        empty_op.give_up();
        let replaced = stream.publish(
            Generation(2),
            None,
            EventKind::CollectionReplaced {
                generation: Generation(2),
            },
        );
        let delivered: Vec<CoreEvent> = (0..4)
            .map(|_| rx.recv_timeout(Duration::from_secs(5)).unwrap().0)
            .collect();
        assert_eq!(
            delivered.iter().map(|e| e.seq).collect::<Vec<_>>(),
            [op_seq, job.0, second.0, replaced.0],
            "each once, in seq order"
        );
        assert_eq!(
            delivered[0],
            CoreEvent {
                seq: op_seq,
                generation: 1,
                cause_seq: None,
                kind: CoreEventKind::Changes {
                    changes: sample_record()
                },
            }
        );
        assert_eq!(
            delivered[3],
            CoreEvent {
                seq: replaced.0,
                generation: 2,
                cause_seq: None,
                kind: CoreEventKind::CollectionReplaced { generation: 2 },
            }
        );
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn a_thousand_op_change_sets_from_four_threads_reach_the_listener_once_in_seq_order() {
        let (stream, dispatcher) = start();
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        // Each op as the writer publishes it: a seq reserved at commit, filled
        // with its change set once its observers ran.
        let threads: Vec<_> = (0..4i64)
            .map(|t| {
                let stream = stream.clone();
                thread::spawn(move || {
                    (0..250)
                        .map(|i| {
                            let op = stream.reserve();
                            let seq = op.seq().0;
                            op.fill(Generation(1), None, card_changed(t * 1_000 + i));
                            (seq, t * 1_000 + i)
                        })
                        .collect::<Vec<_>>()
                })
            })
            .collect();
        let mut published: Vec<(u64, i64)> = threads.into_iter().flat_map(|t| t.join().unwrap()).collect();
        published.sort_unstable();

        let delivered: Vec<(u64, i64)> = (0..1_000)
            .map(|_| {
                let (event, thread) = rx
                    .recv_timeout(Duration::from_secs(10))
                    .expect("change set not delivered");
                assert_eq!(thread.as_deref(), Some(DISPATCHER_THREAD_NAME));
                match event.kind {
                    CoreEventKind::Changes { changes } => (event.seq, changes.cards[0].id),
                    other => panic!("{other:?}"),
                }
            })
            .collect();
        assert_eq!(delivered, published, "every op once, in seq order");
        assert!(rx.recv_timeout(Duration::from_millis(50)).is_err());
    }

    #[test]
    fn publishing_changes_or_a_replacement_from_outside_the_engine_is_refused() {
        for kind in [
            CoreEventKind::Changes {
                changes: sample_record(),
            },
            CoreEventKind::CollectionReplaced { generation: 2 },
            CoreEventKind::RolledOver { old_day: 1, new_day: 2 },
            CoreEventKind::TimeZoneChanged {
                zone_id: "UTC".to_owned(),
            },
            CoreEventKind::NextDayAtChanged { next_day_at: 1 },
            CoreEventKind::MaintenanceDeadline { at: Some(1) },
        ] {
            assert!(matches!(publish(1, None, kind), Err(CoreError::InvalidArgument { .. })));
        }
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

fn session_event_kind(event: &meguri_engine::session::SessionEvent) -> CoreEventKind {
    use meguri_engine::session::SessionEvent as E;
    match event {
        E::SessionStarted { session_id, deck_id } => CoreEventKind::SessionStarted {
            session_id: session_id.0,
            deck_id: deck_id.0,
        },
        E::SessionEnded { session_id } => CoreEventKind::SessionEnded {
            session_id: session_id.0,
        },
        E::QueueHeadChanged { session_id, diff } => CoreEventKind::QueueHeadChanged {
            session_id: session_id.0,
            diff: diff.clone().into(),
        },
        E::CurrentCardChanged {
            session_id,
            card_id,
            change,
        } => CoreEventKind::CurrentCardChanged {
            session_id: session_id.0,
            card_id: card_id.0,
            change: change.clone().into(),
        },
        E::CommitFailed {
            session_id,
            card_id,
            error,
        } => CoreEventKind::CommitFailed {
            session_id: session_id.0,
            card_id: card_id.0,
            error: error.clone().into(),
        },
        E::WaitingUntil { session_id, instant } => CoreEventKind::WaitingUntil {
            session_id: session_id.0,
            instant: *instant,
        },
        E::Finished { session_id, state } => CoreEventKind::Finished {
            session_id: session_id.0,
            state: state.clone().into(),
        },
        E::PresetUnsupported { session_id, preset_ids } => CoreEventKind::PresetUnsupported {
            session_id: session_id.0,
            preset_ids: preset_ids.iter().map(|id| id.0).collect(),
        },
        E::RecoveryDropped { event, card_id, reason } => CoreEventKind::RecoveryDropped {
            event: *event,
            card_id: card_id.0,
            reason: reason.clone(),
        },
    }
}

#[cfg(test)]
mod review_tests {
    use super::*;
    use crate::review_session::tests::{sample_diff, sample_finished};
    use meguri_engine::{SessionId, session::SessionEvent as E};
    use meguri_model::ids::{CardId, DeckConfigId, DeckId};
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn all_session_events_keep_typed_payloads_envelopes_order_and_publish_protection() {
        let stream = ChangeStream::default();
        let threads = ThreadRegistry::new();
        let dispatcher = Dispatcher::start(&stream, &threads).unwrap();
        struct Recorder(mpsc::Sender<CoreEvent>);
        impl CoreEventListener for Recorder {
            fn on_event(&self, event: CoreEvent) {
                self.0.send(event).unwrap();
            }
        }
        let (tx, rx) = mpsc::channel();
        dispatcher.set_listener(Arc::new(Recorder(tx)));
        let events = vec![
            (
                E::SessionStarted {
                    session_id: SessionId(91),
                    deck_id: DeckId(-4),
                },
                CoreEventKind::SessionStarted {
                    session_id: 91,
                    deck_id: -4,
                },
            ),
            (
                E::SessionEnded {
                    session_id: SessionId(91),
                },
                CoreEventKind::SessionEnded { session_id: 91 },
            ),
            (
                E::QueueHeadChanged {
                    session_id: SessionId(91),
                    diff: sample_diff(),
                },
                CoreEventKind::QueueHeadChanged {
                    session_id: 91,
                    diff: sample_diff().into(),
                },
            ),
            (
                E::CurrentCardChanged {
                    session_id: SessionId(91),
                    card_id: CardId(-41),
                    change: meguri_engine::session::CurrentChange::Eligible,
                },
                CoreEventKind::CurrentCardChanged {
                    session_id: 91,
                    card_id: -41,
                    change: ReviewCurrentChange::Eligible,
                },
            ),
            (
                E::CurrentCardChanged {
                    session_id: SessionId(91),
                    card_id: CardId(-41),
                    change: meguri_engine::session::CurrentChange::Removed,
                },
                CoreEventKind::CurrentCardChanged {
                    session_id: 91,
                    card_id: -41,
                    change: ReviewCurrentChange::Removed,
                },
            ),
            (
                E::CurrentCardChanged {
                    session_id: SessionId(91),
                    card_id: CardId(-41),
                    change: meguri_engine::session::CurrentChange::MemoryOnly,
                },
                CoreEventKind::CurrentCardChanged {
                    session_id: 91,
                    card_id: -41,
                    change: ReviewCurrentChange::MemoryOnly,
                },
            ),
            (
                E::CommitFailed {
                    session_id: SessionId(91),
                    card_id: CardId(-41),
                    error: meguri_engine::EngineError::CommitCompleted {
                        seq: Seq(73),
                        message: "durability follow-up".into(),
                    },
                },
                CoreEventKind::CommitFailed {
                    session_id: 91,
                    card_id: -41,
                    error: CoreError::CommitCompleted {
                        seq: 73,
                        message: "durability follow-up".into(),
                    },
                },
            ),
            (
                E::WaitingUntil {
                    session_id: SessionId(91),
                    instant: -123,
                },
                CoreEventKind::WaitingUntil {
                    session_id: 91,
                    instant: -123,
                },
            ),
            (
                E::Finished {
                    session_id: SessionId(91),
                    state: sample_finished(),
                },
                CoreEventKind::Finished {
                    session_id: 91,
                    state: sample_finished().into(),
                },
            ),
            (
                E::PresetUnsupported {
                    session_id: SessionId(91),
                    preset_ids: vec![DeckConfigId(-3), DeckConfigId(7)],
                },
                CoreEventKind::PresetUnsupported {
                    session_id: 91,
                    preset_ids: vec![-3, 7],
                },
            ),
            (
                E::RecoveryDropped {
                    event: u64::MAX,
                    card_id: CardId(-41),
                    reason: "stale accepted context".into(),
                },
                CoreEventKind::RecoveryDropped {
                    event: u64::MAX,
                    card_id: -41,
                    reason: "stale accepted context".into(),
                },
            ),
        ];
        let mut expected = Vec::new();
        for (event, kind) in events {
            assert!(matches!(
                stream_kind(kind.clone()),
                Err(CoreError::InvalidArgument { .. })
            ));
            let seq = stream.publish(Generation(29), Some(Seq(17)), EventKind::Extension(Arc::new(event)));
            expected.push(CoreEvent {
                seq: seq.0,
                generation: 29,
                cause_seq: Some(17),
                kind,
            });
        }
        for event in expected {
            assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), event);
        }
        // Unknown extensions keep the old skip behavior.
        #[derive(Debug)]
        struct Other;
        impl meguri_engine::ExtensionEvent for Other {
            fn as_any(&self) -> &dyn std::any::Any {
                self
            }
        }
        assert!(
            core_event(&Event {
                seq: Seq(100),
                generation: Generation(29),
                cause_seq: None,
                kind: EventKind::Extension(Arc::new(Other))
            })
            .is_none()
        );
        drop(stream);
    }

    #[test]
    fn media_arrival_keeps_collection_generation_and_exact_names() {
        let names = vec!["É.PNG".into(), "E\u{301}.PNG".into()];
        let event = core_event(&Event {
            seq: Seq(101),
            generation: Generation(7),
            cause_seq: None,
            kind: EventKind::Extension(Arc::new(meguri_engine::media::MediaArrived { names: names.clone() })),
        })
        .unwrap();
        assert_eq!(event.generation, 7);
        assert_eq!(event.kind, CoreEventKind::MediaArrived { names });
    }

    #[test]
    fn finished_counts_preserve_signed_differences_readiness_and_buried_flags() {
        assert_eq!(
            ReviewDeckStudyState::from(sample_finished()),
            ReviewDeckStudyState {
                deck: -4,
                queue_counts: ReviewCounts {
                    new: 1,
                    learning: 2,
                    review: 3
                },
                list_counts: ReviewCounts {
                    new: 4,
                    learning: 5,
                    review: 6
                },
                difference: ReviewDifference {
                    new: -3,
                    learning: 3,
                    review: -9
                },
                finished: true,
                learning_remaining_today: 14,
                learning_later_today: 15,
                next_learning_due: Some(-123),
                next_learning_ready_at: Some(-456),
                review_limit_reached: true,
                new_limit_reached: false,
                learning_remaining: 16,
                secs_until_next_learn: -17,
                review_remaining: false,
                new_remaining: true,
                sibling_buried: 18,
                user_buried: 19,
                tomorrow: ReviewCounts {
                    new: 7,
                    learning: 8,
                    review: 9
                },
                empty: false,
            }
        );
    }
}
