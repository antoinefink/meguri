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
                .payload
                .cards()
                .map(|d| {
                    let d = d.unwrap();
                    (d.id.0, queue(d.before_card().unwrap()), queue(d.after_card().unwrap()))
                })
                .collect(),
            revlog_added: commit
                .payload
                .revlog_added_ids()
                .map(|id| commit.payload.revlog_added(id).unwrap().unwrap().id.0)
                .collect(),
            revlog_removed: commit
                .payload
                .revlog_removed_ids()
                .map(|id| {
                    let r = commit.payload.revlog_removed(id).unwrap().unwrap();
                    (r.id.0, r.answer_millis)
                })
                .collect(),
            config: commit
                .changes
                .config_keys
                .iter()
                .map(|key| {
                    let c = commit.payload.config(&key.key).unwrap().unwrap();
                    (c.key, c.before, c.after)
                })
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

#[derive(Default)]
struct CaptureOwner(Mutex<Vec<meguri_engine::CommitPayload>>);
impl ChangeObserver for CaptureOwner {
    fn name(&self) -> &str {
        "owned-view"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        let reads = commit.payload.image_reads();
        let payload = commit.payload.clone();
        let _keys: Vec<_> = payload.card_ids().collect();
        assert_eq!(reads, payload.image_reads(), "capture and keys do not read images");
        self.0.lock().unwrap().push(payload);
        Ok(())
    }
}

#[test]
fn two_owned_observers_retain_sync_images_after_conflicts_undo_discard_and_close() {
    use meguri_model::ids::{CardId, RevlogId};
    use meguri_storage::Record;
    let a = Arc::new(CaptureOwner::default());
    let b = Arc::new(CaptureOwner::default());
    let h = Harness::open_with(Ports::default().observer(a.clone()).observer(b.clone()));
    let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
    let original = h
        .collection
        .reader(meguri_storage::ReaderRole::Query)
        .unwrap()
        .card(cid)
        .unwrap()
        .unwrap();
    a.0.lock().unwrap().clear();
    b.0.lock().unwrap().clear();
    let committed = h
        .collection
        .run(OpSpec::named("incoming payload", OpSource::SyncIncoming), move |ctx| {
            let mut card = ctx.scope().card(cid)?.unwrap();
            card.flags = 2;
            ctx.scope().put(&card)?;
            let rid = ctx.scope().add_revlog(&revlog(cid))?;
            ctx.scope()
                .set_config("payload-key", &ConfigValue::String("captured".into()))?;
            Ok(rid)
        })
        .unwrap();
    let view_a = a.0.lock().unwrap().pop().unwrap();
    let view_b = b.0.lock().unwrap().pop().unwrap();
    assert_eq!((view_a.seq(), view_b.seq()), (committed.seq, committed.seq));
    assert_eq!(view_a.generation(), committed.generation);
    let owner = view_a.journal_owner().unwrap();
    let after = view_a.card(cid).unwrap().unwrap().after_card().unwrap().unwrap();
    h.collection
        .clear_undo(meguri_engine::ClearCause::CheckDatabase)
        .unwrap();
    h.collection
        .run(
            OpSpec::named("conflicting later write", OpSource::SyncIncoming),
            move |ctx| {
                let mut card = ctx.scope().card(cid)?.unwrap();
                card.flags = 7;
                ctx.scope().put(&card)?;
                ctx.scope().remove_revlog(committed.value)?;
                ctx.scope()
                    .set_config("payload-key", &ConfigValue::String("later".into()))?;
                Ok(())
            },
        )
        .unwrap();
    h.engine.close().unwrap();
    a.0.lock().unwrap().clear();
    b.0.lock().unwrap().clear();
    std::thread::scope(|threads| {
        for view in [&view_a, &view_b] {
            let (original, after) = (&original, &after);
            threads.spawn(move || {
                for _ in 0..2 {
                    let card = view.card(cid).unwrap().unwrap();
                    assert_eq!(
                        card.before_card().unwrap().as_ref().map(Record::to_cells),
                        Some(original.to_cells())
                    );
                    assert_eq!(
                        card.after_card().unwrap().as_ref().map(Record::to_cells),
                        Some(after.to_cells())
                    );
                    assert!(view.card(CardId(-1)).unwrap().is_none());
                    let rid = view.revlog_added_ids().next().unwrap();
                    assert_eq!(view.revlog_added(rid).unwrap().unwrap().card_id, cid);
                    assert!(view.revlog_removed(rid).unwrap().is_none());
                    assert!(view.revlog_added(RevlogId(-1)).unwrap().is_none());
                    let config = view.config("payload-key").unwrap().unwrap();
                    assert!(config.before.is_none());
                    assert_eq!(config.after.as_deref(), Some(b"\"captured\"".as_slice()));
                    assert!(view.config("unchanged-key").unwrap().is_none());
                }
            });
        }
    });
    drop(view_a);
    assert!(owner.upgrade().is_some());
    drop(view_b);
    assert!(
        owner.upgrade().is_none(),
        "undo and writer do not keep the last backing owner"
    );
}

#[test]
fn metadata_read_or_typed_card_failure_rolls_back_before_commit_and_publication() {
    use meguri_storage::{Cell, Record, RowKey, Table};
    for unreadable in [true, false] {
        let h = Harness::open();
        let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
        let before = common::steps::dump(&h.collection);
        let events = h.engine.stream().subscribe();
        let result = h.collection.run(
            OpSpec::named("bad metadata", OpSource::UserUndoable).stamp(meguri_storage::StampMode::CallerSupplied),
            move |ctx| {
                let mut row = ctx.scope().card(cid)?.unwrap().to_cells();
                if unreadable {
                    row[16] = Cell::Integer(2);
                } else {
                    row[1] = Cell::text("invalid nid");
                }
                ctx.scope().restore(Table::Cards, &RowKey::Id(cid.0), Some(&row))?;
                if unreadable {
                    ctx.scope().journal().unwrap().fail_image_reads(true);
                }
                Ok(())
            },
        );
        assert!(matches!(result, Err(EngineError::Storage(_))));
        assert_eq!(common::steps::dump(&h.collection), before);
        assert!(events.try_recv().is_none());
    }
}

struct ReadFailure;
impl ChangeObserver for ReadFailure {
    fn name(&self) -> &str {
        "payload-io"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        if let Some(id) = commit.payload.card_ids().next() {
            commit.payload.fail_image_reads(true);
            // This is an actual failed access, not a synthetic observer error.
            let read = commit.payload.card(id);
            commit.payload.fail_image_reads(false);
            read?;
        }
        Ok(())
    }
}

#[test]
fn observer_io_failure_still_delivers_once_to_remaining_observers_and_publishes_once() {
    let recorder = Arc::new(Recorder {
        seen: Mutex::default(),
        stream: Mutex::default(),
    });
    let h = Harness::open_with(
        Ports::default()
            .observer(Arc::new(ReadFailure))
            .observer(recorder.clone()),
    );
    recorder.seen.lock().unwrap().clear();
    let events = h.engine.stream().subscribe();
    let result = add_note_with_card(&h.collection, OpSource::SyncIncoming, &["survives", "back"]);
    assert_eq!(recorder.seen.lock().unwrap().len(), 1);
    assert_eq!(change_seqs(&drain(&events, Duration::from_millis(100))), [result.seq.0]);
    assert_eq!(result.changes.cards[0].note_id, result.value.0);
}

#[test]
fn removed_card_revlog_config_and_final_collection_are_repeatable_owned_payloads() {
    let capture = Arc::new(CaptureOwner::default());
    let h = Harness::open_with(Ports::new().observer(capture.clone()));
    let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
    let rid = h
        .collection
        .run(OpSpec::named("payload setup", OpSource::UserUndoable), move |ctx| {
            ctx.scope().set_config("removed-payload", &ConfigValue::Integer(7))?;
            Ok(ctx.scope().add_revlog(&revlog(cid))?)
        })
        .unwrap()
        .value;
    capture.0.lock().unwrap().clear();
    h.clock.step_ms(1000);
    let removed = h
        .collection
        .run(OpSpec::named("remove payloads", OpSource::UserUndoable), move |ctx| {
            ctx.scope().remove_card(cid)?;
            ctx.scope().remove_revlog(rid)?;
            ctx.scope().remove_config("removed-payload")?;
            Ok(())
        })
        .unwrap();
    let view = capture.0.lock().unwrap().pop().unwrap();
    let col = view.collection().unwrap().unwrap();
    assert_eq!(
        col.modified,
        h.collection
            .reader(meguri_storage::ReaderRole::Query)
            .unwrap()
            .col()
            .unwrap()
            .modified
    );
    h.collection
        .clear_undo(meguri_engine::ClearCause::CheckDatabase)
        .unwrap();
    h.engine.close().unwrap();
    for _ in 0..2 {
        let delta = view.card(cid).unwrap().unwrap();
        assert!(delta.before_card().unwrap().is_some());
        assert!(delta.after_card().unwrap().is_none());
        assert_eq!(view.revlog_removed(rid).unwrap().unwrap().card_id, cid);
        assert!(view.revlog_added(rid).unwrap().is_none());
        let config = view.config("removed-payload").unwrap().unwrap();
        assert_eq!(config.before.as_deref(), Some(b"7".as_slice()));
        assert!(config.after.is_none());
        assert_eq!(view.collection().unwrap(), Some(col.clone()));
        assert_eq!(view.seq(), removed.seq);
    }
}
