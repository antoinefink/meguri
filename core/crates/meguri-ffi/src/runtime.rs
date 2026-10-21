//! The process's engine (U24) and its event dispatcher: one of each per
//! process, created together on first use with no collection open. The
//! engine's change stream feeds the dispatcher, its job registry runs every
//! job, and its thread registry names the dispatcher thread. U37 opens the
//! collection on it through `CoreHost::open` (U38).

use std::sync::{Arc, Mutex, OnceLock, PoisonError};

use meguri_engine::{Engine, EngineConfig};

use crate::error::CoreError;
use crate::events::Dispatcher;

struct Runtime {
    process: meguri_host::ProcessCore,
    dispatcher: Arc<Dispatcher>,
    host: Mutex<HostState>,
}

impl Runtime {
    fn new(config: EngineConfig) -> Result<Self, CoreError> {
        let process = meguri_host::ProcessCore::new(config).map_err(CoreError::from)?;
        let engine = process.engine();
        // Subscribe before exposing the process engine or review service.
        let dispatcher = Dispatcher::start(engine.stream(), engine.threads())?;
        Ok(Self {
            process,
            dispatcher,
            host: Mutex::new(HostState::Empty),
        })
    }
}

fn runtime() -> Result<&'static Runtime, CoreError> {
    static RUNTIME: OnceLock<Result<Runtime, CoreError>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| Runtime::new(EngineConfig::default()))
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn engine() -> Result<&'static Engine, CoreError> {
    runtime().map(|runtime| runtime.process.engine())
}

pub(crate) fn dispatcher() -> Result<&'static Arc<Dispatcher>, CoreError> {
    runtime().map(|runtime| &runtime.dispatcher)
}

pub(crate) fn process() -> Result<&'static meguri_host::ProcessCore, CoreError> {
    runtime().map(|runtime| &runtime.process)
}
pub(crate) fn review() -> Result<&'static meguri_engine::session::ReviewService, CoreError> {
    let process = process()?;
    process.engine().collection()?;
    Ok(process.review())
}

// Keep the Arc in the slot while closing: an error or unwind cannot drop the
// only owner. Opening and closing exclude each other without holding this
// lock across CoreHost work or any foreign callback.
enum HostState {
    Empty,
    Opening,
    Open(Arc<meguri_host::CoreHost>),
    Closing(Arc<meguri_host::CoreHost>),
}

pub(crate) struct OpenReservation {
    runtime: &'static Runtime,
    pending: bool,
}
pub(crate) fn reserve_open() -> Result<OpenReservation, CoreError> {
    let runtime = runtime()?;
    let mut state = runtime.host.lock().unwrap_or_else(PoisonError::into_inner);
    if !matches!(*state, HostState::Empty) {
        return Err(CoreError::Busy {
            running_op: "collection open or close".into(),
        });
    }
    *state = HostState::Opening;
    Ok(OpenReservation { runtime, pending: true })
}
impl OpenReservation {
    pub(crate) fn open(mut self, config: meguri_host::HostConfig) -> Result<(), CoreError> {
        let host = Arc::new(self.runtime.process.open(config)?);
        // No fallible call between receiving the host and retaining it.
        *self.runtime.host.lock().unwrap_or_else(PoisonError::into_inner) = HostState::Open(host);
        self.pending = false;
        Ok(())
    }
}
impl Drop for OpenReservation {
    fn drop(&mut self) {
        if self.pending {
            *self.runtime.host.lock().unwrap_or_else(PoisonError::into_inner) = HostState::Empty;
        }
    }
}
pub(crate) fn host() -> Result<Arc<meguri_host::CoreHost>, CoreError> {
    match &*runtime()?.host.lock().unwrap_or_else(PoisonError::into_inner) {
        HostState::Open(host) => Ok(host.clone()),
        _ => Err(CoreError::Unavailable {
            reason: "collection host is not ready".into(),
        }),
    }
}
pub(crate) fn close() -> Result<(), CoreError> {
    let runtime = runtime()?;
    let host = {
        let mut state = runtime.host.lock().unwrap_or_else(PoisonError::into_inner);
        match &*state {
            HostState::Empty => return Ok(()),
            HostState::Open(host) => {
                let host = host.clone();
                *state = HostState::Closing(host.clone());
                host
            }
            _ => {
                return Err(CoreError::Busy {
                    running_op: "collection open or close".into(),
                });
            }
        }
    };
    struct Restore<'a>(&'a Runtime);
    impl Drop for Restore<'_> {
        fn drop(&mut self) {
            let mut state = self.0.host.lock().unwrap_or_else(PoisonError::into_inner);
            if let HostState::Closing(host) = &*state {
                *state = HostState::Open(host.clone());
            }
        }
    }
    let _restore = Restore(runtime);
    host.close()?;
    *runtime.host.lock().unwrap_or_else(PoisonError::into_inner) = HostState::Empty;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        CoreEvent, CoreEventKind, CoreEventListener,
        review_session::{ReviewAnswerOutcome, ReviewHead, ReviewStartResult},
    };
    use meguri_engine::{
        OpSource, OpSpec, SessionId,
        session::{AnswerOutcome, StartResult},
    };
    use meguri_host::HostConfig;
    use meguri_model::{
        clock::{StepClock, UtcOffset},
        ids::DeckId,
        rng::SeededRandom,
        time::EpochMillis,
    };
    use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table, space::StatvfsSpace};
    use std::{sync::mpsc, time::Duration};

    #[test]
    fn runtime_composition_opens_reviews_dispatches_and_reopens_on_one_engine() {
        const NOW: i64 = 1_894_701_600_000;
        let dir = tempfile::tempdir().unwrap();
        let collection_dir = dir.path().join("collection");
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&collection_dir).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        meguri_engine::bootstrap::create(
            &collection_dir.join("collection.anki2"),
            clock.as_ref(),
            &SeededRandom::new(43),
        )
        .unwrap();
        let config = HostConfig::new(
            collection_dir,
            backups,
            dir.path().join("widget.snapshot"),
            clock,
            Arc::new(SeededRandom::new(43)),
            Arc::new(StatvfsSpace),
        );
        // Local runtime uses exactly the process runtime constructor; it avoids
        // replacing the process listener or racing closed-collection tests.
        let runtime = Runtime::new(config.engine.clone()).unwrap();
        let process = &runtime.process;
        assert!(matches!(
            process.review().start(DeckId(1), None),
            Err(meguri_engine::EngineError::CollectionUnavailable { .. })
        ));
        struct Recorder(mpsc::Sender<CoreEvent>);
        impl CoreEventListener for Recorder {
            fn on_event(&self, event: CoreEvent) {
                let _ = self.0.send(event);
            }
        }
        let (tx, rx) = mpsc::channel();
        runtime.dispatcher.set_listener(Arc::new(Recorder(tx)));
        let mut incompatible = config.clone();
        incompatible.engine.job_workers += 1;
        assert!(matches!(
            process.open(incompatible),
            Err(meguri_engine::EngineError::InvalidArgument { .. })
        ));
        let host = process.open(config.clone()).unwrap();
        assert!(std::ptr::eq(process.review(), host.review()));
        assert!(
            process.open(config.clone()).is_err(),
            "a second open cannot clean up the active host"
        );
        let c = host.collection().unwrap();
        c.run(OpSpec::named("bridge fixture policy", OpSource::Maintenance), |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
            ctx.scope()
                .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
        let nt = meguri_engine::notetypes::add(
            &c,
            meguri_engine::notetypes::NewNotetype {
                name: "Bridge session".into(),
                kind: meguri_engine::notetypes::NotetypeKind::Standard,
                fields: vec!["Front".into(), "Back".into()],
                templates: vec![meguri_engine::notetypes::NewTemplate {
                    name: "Card".into(),
                    front: "{{Front}}<img src=\"bridge.png\">[sound:bridge.mp3]".into(),
                    back: "{{FrontSide}}/{{Back}}".into(),
                    ..Default::default()
                }],
                css: Some(".card { color: black; }".into()),
            },
        )
        .unwrap()
        .value
        .id;
        let note = meguri_engine::notes::add(
            &c,
            meguri_engine::notes::AddNote {
                notetype_id: nt,
                fields: vec!["bridge sentinel".into(), "answer".into()],
                tags: vec![],
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        let card = c.reader(ReaderRole::Query).unwrap().cards_of_note(note.id).unwrap()[0].id;
        let ReviewStartResult::Started {
            session_id,
            generation,
            head,
            ..
        } = ReviewStartResult::from(process.review().start(DeckId(1), None).unwrap())
        else {
            panic!("start refused")
        };
        assert_eq!(head.items[0].card_id, card.0);
        assert_eq!(generation, c.generation().0);
        let native = if cfg!(target_os = "ios") {
            meguri_engine::rendering::Platform::Ios
        } else {
            meguri_engine::rendering::Platform::Mac
        };
        let rendered = process
            .review()
            .render_head(
                SessionId(session_id),
                &[head.items[0].render_key.bytes.clone().try_into().unwrap()],
                native,
            )
            .unwrap();
        let rendered = crate::render::RenderedCard::from(rendered[0].as_ref().clone());
        assert!(rendered.media_refs.contains(&"bridge.png".into()));
        assert!(rendered.question.unwrap().contains("bridge sentinel"));
        assert_eq!(
            rendered.question_av,
            vec![crate::render::RenderAv::Sound {
                name: "bridge.mp3".into()
            }]
        );
        let AnswerOutcome::Accepted { head_diff, .. } = process
            .review()
            .answer(
                SessionId(session_id),
                card,
                meguri_sched::answer::Rating::Easy,
                head.items[0].state_token.bytes.clone().try_into().unwrap(),
                EpochMillis(NOW),
                u64::MAX,
            )
            .unwrap()
        else {
            panic!("answer refused")
        };
        assert!(matches!(
            ReviewAnswerOutcome::from(AnswerOutcome::Accepted {
                head_diff,
                leech: false,
                suspended: false,
                rolled_over: false
            }),
            ReviewAnswerOutcome::Accepted { .. }
        ));
        process.review().flush_for_background().unwrap();
        assert_eq!(
            c.reader(ReaderRole::Query)
                .unwrap()
                .ids_after(Table::Revlog, None, 100)
                .unwrap()
                .len(),
            1
        );
        // A job sentinel on the same engine brackets the session stream.
        let marker = process.engine().stream().publish(
            c.generation(),
            None,
            meguri_engine::EventKind::JobFinished {
                id: meguri_engine::JobId(9_043_001),
                outcome: meguri_engine::JobOutcome::Succeeded,
            },
        );
        let mut delivered = Vec::new();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let done = event.seq == marker.0;
            delivered.push(event);
            if done {
                break;
            }
        }
        assert!(delivered.windows(2).all(|w| w[0].seq < w[1].seq));
        assert!(
            delivered
                .iter()
                .any(|e| e.generation == generation
                    && e.kind == CoreEventKind::SessionStarted { session_id, deck_id: 1 })
        );
        assert!(
            delivered
                .iter()
                .any(|e| matches!(&e.kind, CoreEventKind::Changes { changes } if changes.revlog_added.len() == 1))
        );
        host.close().unwrap();
        assert_eq!(process.review().active_session(), None);
        // Close ends this session and releases the retained review provider binding.
        assert_eq!(
            process.review().head(SessionId(session_id), 3).unwrap_err(),
            meguri_engine::EngineError::CollectionUnavailable {
                reason: "review provider is not bound".into(),
            }
        );
        let reopened = process.open(config).unwrap();
        drop(host); // An already-closed host must not close its successor.
        assert!(std::ptr::eq(process.review(), reopened.review()));
        assert!(reopened.collection().is_ok());
        let StartResult::Started {
            session_id: second,
            head,
            ..
        } = process.review().start(DeckId(1), None).unwrap()
        else {
            panic!("reopen start refused")
        };
        assert_ne!(second.0, session_id);
        assert_eq!(
            ReviewHead::from(head).counts.new,
            0,
            "reopen sees the actual committed answer"
        );
        process.review().end(second).unwrap();
        reopened.close().unwrap();
        let final_marker = process.engine().stream().publish(
            meguri_engine::Generation(0),
            None,
            meguri_engine::EventKind::JobFinished {
                id: meguri_engine::JobId(9_043_002),
                outcome: meguri_engine::JobOutcome::Succeeded,
            },
        );
        let mut lifecycle = Vec::new();
        loop {
            let event = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            let done = event.seq == final_marker.0;
            lifecycle.push(event);
            if done {
                break;
            }
        }
        assert!(lifecycle.windows(2).all(|w| w[0].seq < w[1].seq));
        assert!(lifecycle.first().is_some_and(|e| e.seq > marker.0));
        assert!(
            lifecycle
                .iter()
                .any(|e| e.kind == CoreEventKind::SessionEnded { session_id })
        );
        assert!(lifecycle.iter().any(|e| e.kind
            == CoreEventKind::SessionStarted {
                session_id: second.0,
                deck_id: 1
            }));
        assert!(
            lifecycle
                .iter()
                .any(|e| e.kind == CoreEventKind::SessionEnded { session_id: second.0 })
        );
    }
}
