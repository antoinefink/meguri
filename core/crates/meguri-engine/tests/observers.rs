//! In-core observers (`engine/change-stream` S2): on the writer thread, after
//! the commit, before publication, with the rows the op changed; failures
//! never undo a commit.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Harness, add_note_with_card, card, change_seqs, drain, note, revlog};
use meguri_engine::{ChangeObserver, EngineError, EventKind, ObservedCommit, OpSpec, Ports, Subscription};
use meguri_model::card::CardQueue;
use meguri_model::changes::OpSource;
use meguri_storage::{ConfigValue, ReadRows};

/// A config key with its value before and after.
type ConfigSeen = (String, Option<Vec<u8>>, Option<Vec<u8>>);

/// What one observed commit looked like.
#[derive(Clone, Debug, Default)]
struct Seen {
    seq: u64,
    thread: Option<String>,
    /// Card ids with their queue before and after.
    cards: Vec<(i64, Option<CardQueue>, Option<CardQueue>)>,
    revlog_added: Vec<i64>,
    revlog_removed: Vec<(i64, u32)>,
    config: Vec<ConfigSeen>,
    /// Whether the commit's event was already on the stream.
    published_before: bool,
}

struct Recorder {
    seen: Mutex<Vec<Seen>>,
    stream: Mutex<Option<Subscription>>,
}

impl ChangeObserver for Recorder {
    fn name(&self) -> &str {
        "recorder"
    }

    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        let published_before = self
            .stream
            .lock()
            .unwrap()
            .as_ref()
            .and_then(|s| s.try_recv())
            .is_some_and(|e| e.seq == commit.seq);
        let queue = |card: Option<meguri_model::card::Card>| card.map(|c| c.queue);
        self.seen.lock().unwrap().push(Seen {
            seq: commit.seq.0,
            thread: std::thread::current().name().map(str::to_owned),
            cards: commit
                .cards
                .iter()
                .map(|d| (d.id.0, queue(d.before_card().unwrap()), queue(d.after_card().unwrap())))
                .collect(),
            revlog_added: commit.revlog_added.iter().map(|r| r.id.0).collect(),
            revlog_removed: commit
                .revlog_removed
                .iter()
                .map(|r| (r.id.0, r.answer_millis))
                .collect(),
            config: commit
                .config
                .iter()
                .map(|c| (c.key.clone(), c.before.clone(), c.after.clone()))
                .collect(),
            published_before,
        });
        Ok(())
    }
}

#[test]
fn an_observer_gets_the_changed_rows_on_the_writer_before_the_change_set_is_published() {
    let recorder = Arc::new(Recorder {
        seen: Mutex::default(),
        stream: Mutex::default(),
    });
    let h = Harness::open_with(Ports::default().observer(recorder.clone()));
    // The day clock's first check at open (`sched/day-clock` D8) wrote.
    recorder.seen.lock().unwrap().clear();
    *recorder.stream.lock().unwrap() = Some(h.engine.stream().subscribe());
    let subscription = h.engine.stream().subscribe();

    let first = h
        .collection
        .run(OpSpec::named("answer", OpSource::UserUndoable), |ctx| {
            let nid = ctx.scope().add_note(&note(&["n", "x"]))?;
            let cid = ctx.scope().add_card(&card(nid, 0))?;
            let rid = ctx.scope().add_revlog(&revlog(cid))?;
            ctx.scope().set_config("curDeck", &ConfigValue::Integer(1))?;
            Ok((cid, rid))
        })
        .unwrap();
    let (cid, rid) = first.value;
    h.clock.step_ms(1_000);
    let second = h
        .collection
        .run(OpSpec::named("suspend", OpSource::UserUndoable), move |ctx| {
            let mut stored = ctx.scope().card(cid)?.unwrap();
            stored.queue = CardQueue::Suspended;
            ctx.scope().put(&stored)?;
            ctx.scope().remove_revlog(rid)?;
            ctx.scope().set_config("curDeck", &ConfigValue::Integer(2))?;
            Ok(())
        })
        .unwrap();

    let seen = recorder.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 2);
    assert_eq!((seen[0].seq, seen[1].seq), (first.seq.0, second.seq.0));
    for commit in &seen {
        assert_eq!(commit.thread.as_deref(), Some("meguri-writer"));
        assert!(!commit.published_before, "observers run before publication");
    }
    assert_eq!(seen[0].cards, [(cid.0, None, Some(CardQueue::New))]);
    assert_eq!(seen[0].revlog_added, [rid.0]);
    assert!(
        seen[0]
            .config
            .iter()
            .any(|(k, _, after)| k == "curDeck" && after.as_deref() == Some(b"1".as_slice()))
    );
    assert_eq!(
        seen[1].cards,
        [(cid.0, Some(CardQueue::New), Some(CardQueue::Suspended))]
    );
    assert_eq!(
        seen[1].revlog_removed,
        [(rid.0, 4000)],
        "removed rows come from before-images"
    );
    assert_eq!(
        seen[1].config,
        [("curDeck".to_owned(), Some(b"1".to_vec()), Some(b"2".to_vec()))]
    );
    assert_eq!(
        change_seqs(&drain(&subscription, Duration::from_millis(100))),
        [first.seq.0, second.seq.0]
    );
}

struct Failing {
    panic: bool,
}

impl ChangeObserver for Failing {
    fn name(&self) -> &str {
        if self.panic { "panics" } else { "fails" }
    }

    fn observe(&self, _commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        if self.panic {
            panic!("observer bug");
        }
        Err(EngineError::Internal {
            message: "derived store is full".into(),
        })
    }
}

#[test]
fn a_failing_or_panicking_observer_never_undoes_the_commit_and_the_others_still_run() {
    let recorder = Arc::new(Recorder {
        seen: Mutex::default(),
        stream: Mutex::default(),
    });
    let ports = Ports::default()
        .observer(Arc::new(Failing { panic: true }))
        .observer(Arc::new(Failing { panic: false }))
        .observer(recorder.clone());
    let h = Harness::open_with(ports);
    // The day clock's first check at open (`sched/day-clock` D8) wrote.
    recorder.seen.lock().unwrap().clear();
    let subscription = h.engine.stream().subscribe();
    let result = add_note_with_card(&h.collection, OpSource::UserUndoable, &["kept", "x"]);
    let (nid, _) = result.value;
    let reader = h.collection.reader(meguri_storage::ReaderRole::Query).unwrap();
    assert!(reader.note(nid).unwrap().is_some(), "the commit stands");
    drop(reader);
    assert_eq!(recorder.seen.lock().unwrap().len(), 1, "later observers still ran");
    let events = drain(&subscription, Duration::from_millis(100));
    assert!(matches!(events[0].kind, EventKind::Changes(_)));
    assert_eq!(events[0].seq, result.seq);
    // The writer goes on.
    add_note_with_card(&h.collection, OpSource::UserUndoable, &["next", "x"]);
}

#[test]
fn observers_see_no_op_that_changed_nothing() {
    let recorder = Arc::new(Recorder {
        seen: Mutex::default(),
        stream: Mutex::default(),
    });
    let h = Harness::open_with(Ports::default().observer(recorder.clone()));
    // The day clock's first check at open (`sched/day-clock` D8) wrote.
    recorder.seen.lock().unwrap().clear();
    h.collection
        .run(OpSpec::named("nothing", OpSource::UserUndoable), |_| Ok(()))
        .unwrap();
    assert!(recorder.seen.lock().unwrap().is_empty());
}
