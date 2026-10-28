//! In-core observers (`engine/change-stream` S2): on the writer thread, after
//! the commit, before publication, with the rows the op changed; failures
//! never undo a commit.

mod common;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::{Harness, add_note_with_card, card, change_seqs, drain, note, revlog};
use meguri_engine::changes::observers::RowAfter;
use meguri_engine::{ChangeObserver, EngineError, EventKind, ObservedCommit, OpSpec, Ports, Subscription};
use meguri_model::card::CardQueue;
use meguri_model::changes::OpSource;
use meguri_storage::journal::{ColumnSet, NoteDigest, digest};
use meguri_storage::{
    Cell, ConfigValue, ReadRows, ReaderRole, Record, RowImage, RowKey, StampMode, StorageError, Table,
};

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
        for table in Table::ALL {
            let _keys: Vec<_> = payload.row_keys(table).collect();
        }
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
        let probe = Arc::new(FailureProbe::new(Some(false), true));
        let h = Harness::open_with(Ports::new().observer(probe.clone()));
        let (_, cid) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["front", "back"]).value;
        probe.clear();
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
        assert!(probe.seen.lock().unwrap().is_empty());
        assert!(probe.failed.lock().unwrap().is_empty());
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

#[test]
fn generic_rows_keep_exact_stored_cells_and_composite_keys_after_later_commits_and_close() {
    let capture = Arc::new(CaptureOwner::default());
    let h = Harness::open_with(Ports::new().observer(capture.clone()));
    let (nid, _) = add_note_with_card(&h.collection, OpSource::UserUndoable, &["generic", "back"]).value;
    let id = -123;
    let mut card = card(nid, 0);
    card.id = meguri_model::ids::CardId(id);
    let mut log = revlog(card.id);
    log.id = meguri_model::ids::RevlogId(id);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let mut deck = reader.deck(meguri_model::ids::DeckId(1)).unwrap().unwrap().to_cells();
    deck[0] = Cell::Integer(id);
    deck[1] = Cell::text("Generic deck");
    let mut preset = reader
        .deck_config(meguri_model::ids::DeckConfigId(1))
        .unwrap()
        .unwrap()
        .to_cells();
    preset[0] = Cell::Integer(id);
    preset[1] = Cell::text("Generic preset");
    drop(reader);
    let rows = vec![
        (Table::Cards, RowKey::Id(id), card.to_cells()),
        (Table::Revlog, RowKey::Id(id), log.to_cells()),
        (Table::Decks, RowKey::Id(id), deck),
        (Table::DeckConfig, RowKey::Id(id), preset),
        (
            Table::Config,
            RowKey::text("generic raw config"),
            vec![
                Cell::text("generic raw config"),
                Cell::Integer(0),
                Cell::Integer(0),
                Cell::text(" 1 "),
            ],
        ),
        (
            Table::Notetypes,
            RowKey::Id(id),
            vec![
                Cell::Integer(id),
                Cell::text("Generic type"),
                Cell::Integer(0),
                Cell::Integer(0),
                Cell::Blob(vec![]),
            ],
        ),
        (
            Table::Fields,
            RowKey::Pair(id, 7),
            vec![
                Cell::Integer(id),
                Cell::Integer(7),
                Cell::text("Generic field"),
                Cell::Blob(vec![]),
            ],
        ),
        (
            Table::Templates,
            RowKey::Pair(id, 9),
            vec![
                Cell::Integer(id),
                Cell::Integer(9),
                Cell::text("Generic template"),
                Cell::Integer(0),
                Cell::Integer(0),
                Cell::Blob(vec![]),
            ],
        ),
        (
            Table::Tags,
            RowKey::text("Generic::Tag"),
            vec![
                Cell::text("Generic::Tag"),
                Cell::Integer(0),
                Cell::Integer(0),
                Cell::Null,
            ],
        ),
    ];
    let added_rows = rows.clone();
    let added = h
        .collection
        .run(
            OpSpec::named("generic stored rows", OpSource::SyncIncoming).stamp(StampMode::Raw),
            move |ctx| {
                for (table, key, image) in added_rows {
                    ctx.scope().restore(table, &key, Some(&image))?;
                }
                Ok(())
            },
        )
        .unwrap();
    let added_view = capture.0.lock().unwrap().pop().unwrap();
    let stored = |rows: &[(Table, RowKey, RowImage)]| {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        rows.iter()
            .map(|(table, key, _)| reader.row_image(*table, key).unwrap().unwrap())
            .collect::<Vec<_>>()
    };
    let before = stored(&rows);
    for ((table, key, _), image) in rows.iter().zip(&before) {
        let delta = added_view.row(*table, key).unwrap().unwrap();
        assert_eq!(delta.table, *table);
        assert_eq!(&delta.key, key);
        assert_eq!(delta.before, None);
        assert_eq!(delta.after, RowAfter::Row(image.clone()));
        assert_eq!(added_view.row_keys(*table).collect::<Vec<_>>(), [key]);
        assert_eq!(added_view.rows(*table).collect::<Result<Vec<_>, _>>().unwrap(), [delta]);
        assert!(added_view.row(*table, &RowKey::Id(-999)).unwrap().is_none());
    }
    // These writes exercise storage classes and opaque bytes, not model decoding.
    let mut edited = rows.clone();
    for (table, _, image) in &mut edited {
        match table {
            Table::Cards => image[16] = Cell::Integer(3),
            Table::Revlog => image[7] = Cell::Integer(123),
            Table::Config => image[3] = Cell::Blob(b" 2 ".to_vec()),
            Table::Fields | Table::Templates => image[2] = Cell::text("Renamed part"),
            Table::Tags => {
                image[0] = Cell::text("generic::TAG");
                image[3] = Cell::Blob(vec![0, 255, 1]);
            }
            _ => image[1] = Cell::text(format!("Renamed {}", table.name())),
        }
    }
    h.collection
        .run(
            OpSpec::named("generic update", OpSource::SyncIncoming).stamp(StampMode::Raw),
            move |ctx| {
                for (table, key, image) in edited {
                    ctx.scope().restore(table, &key, Some(&image))?;
                }
                Ok(())
            },
        )
        .unwrap();
    let updated_view = capture.0.lock().unwrap().pop().unwrap();
    let after = stored(&rows);
    let removed_rows = rows.clone();
    h.collection
        .run(
            OpSpec::named("generic delete", OpSource::SyncIncoming).stamp(StampMode::Raw),
            move |ctx| {
                for (table, key, _) in removed_rows {
                    ctx.scope().restore(table, &key, None)?;
                }
                Ok(())
            },
        )
        .unwrap();
    let removed_view = capture.0.lock().unwrap().pop().unwrap();
    h.engine.close().unwrap();
    capture.0.lock().unwrap().clear();
    drop(h);
    std::thread::scope(|threads| {
        threads
            .spawn(|| {
                assert_eq!(added_view.seq(), added.seq);
                for (((table, key, _), before), after) in rows.iter().zip(&before).zip(&after) {
                    let added = added_view.row(*table, key).unwrap().unwrap();
                    assert_eq!(added.after, RowAfter::Row(before.clone()));
                    let updated = updated_view.row(*table, key).unwrap().unwrap();
                    assert_eq!(updated.before.as_ref(), Some(before));
                    assert_eq!(updated.after, RowAfter::Row(after.clone()));
                    assert_eq!(format!("{:?}", updated.after), "RowAfter::Row");
                    assert_eq!(format!("{updated:?}"), "RowDelta");
                    let removed = removed_view.rows(*table).next().unwrap().unwrap();
                    assert_eq!(removed.before.as_ref(), Some(after));
                    assert_eq!(removed.after, RowAfter::Absent);
                    assert_eq!(format!("{:?}", removed.after), "RowAfter::Absent");
                }
                let alias = updated_view
                    .row(Table::Tags, &RowKey::text("GENERIC::tag"))
                    .unwrap()
                    .unwrap();
                assert_eq!(alias.key, RowKey::text("Generic::Tag"));
                assert_eq!(updated_view.row_keys(Table::Tags).count(), 1);
                assert_eq!(
                    RowAfter::Row(
                        removed_view
                            .row(Table::Tags, &RowKey::text("generic::tag"))
                            .unwrap()
                            .unwrap()
                            .before
                            .unwrap()
                    ),
                    alias.after
                );
            })
            .join()
            .unwrap();
    });
}

#[test]
fn generic_notes_keep_exact_before_and_journal_digest_after_later_writes_and_close() {
    let capture = Arc::new(CaptureOwner::default());
    let h = Harness::open_with(Ports::new().observer(capture.clone()));
    let (nid, _) = add_note_with_card(&h.collection, OpSource::SyncIncoming, &["original", "back"]).value;
    let added = capture.0.lock().unwrap().pop().unwrap();
    let key = RowKey::Id(nid.0);
    let original = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Notes, &key)
        .unwrap()
        .unwrap();
    common::set_first_field(&h.collection, OpSource::SyncIncoming, nid, "changed").unwrap();
    let updated = capture.0.lock().unwrap().pop().unwrap();
    let edited = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Notes, &key)
        .unwrap()
        .unwrap();
    h.collection
        .run(
            OpSpec::named("generic note delete", OpSource::SyncIncoming),
            move |ctx| {
                ctx.scope().remove_note(nid)?;
                Ok(())
            },
        )
        .unwrap();
    let removed = capture.0.lock().unwrap().pop().unwrap();
    h.engine.close().unwrap();
    capture.0.lock().unwrap().clear();
    drop(h);
    let expected_add = RowAfter::NoteDigest(NoteDigest {
        sha256: digest(&original),
        changed: ColumnSet::all(Table::Notes),
    });
    let expected_edit = RowAfter::NoteDigest(NoteDigest {
        sha256: digest(&edited),
        changed: ColumnSet::changed(&original, &edited),
    });
    assert_ne!(original, edited);
    for _ in 0..2 {
        for (view, before, after) in [
            (&added, None, &expected_add),
            (&updated, Some(&original), &expected_edit),
            (&removed, Some(&edited), &RowAfter::Absent),
        ] {
            let reads = view.image_reads();
            assert_eq!(view.row_keys(Table::Notes).collect::<Vec<_>>(), [&key]);
            let rows = view.rows(Table::Notes);
            assert_eq!(view.image_reads(), reads, "keys and iterator creation read no images");
            let delta = view.row(Table::Notes, &key).unwrap().unwrap();
            assert_eq!(delta.table, Table::Notes);
            assert_eq!(delta.key, key);
            assert_eq!(delta.before.as_ref(), before);
            assert_eq!(&delta.after, after);
            assert_eq!(format!("{delta:?}"), "RowDelta");
            assert_eq!(rows.collect::<Result<Vec<_>, _>>().unwrap(), [delta]);
            assert!(view.row(Table::Notes, &RowKey::Id(-999)).unwrap().is_none());
        }
        assert_eq!(format!("{expected_add:?}"), "RowAfter::NoteDigest");
        assert_eq!(format!("{expected_edit:?}"), "RowAfter::NoteDigest");
    }
    added.fail_image_reads(true);
    assert_eq!(added.row(Table::Notes, &key).unwrap().unwrap().after, expected_add);
    added.fail_image_reads(false);
    updated.fail_image_reads(true);
    assert!(matches!(updated.row(Table::Notes, &key), Err(StorageError::Io { .. })));
    assert!(matches!(
        updated.rows(Table::Notes).next().unwrap(),
        Err(StorageError::Io { .. })
    ));
    updated.fail_image_reads(false);
    assert_eq!(updated.row(Table::Notes, &key).unwrap().unwrap().after, expected_edit);
}

#[test]
fn generic_access_skips_no_ops_and_created_then_deleted_keys_and_reads_only_visited_rows() {
    let capture = Arc::new(CaptureOwner::default());
    let h = Harness::open_with(Ports::new().observer(capture.clone()));
    h.collection
        .run(OpSpec::named("generic seed", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("unchanged", &ConfigValue::Integer(1))?;
            Ok(())
        })
        .unwrap();
    h.collection
        .run(OpSpec::named("generic churn", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("unchanged", &ConfigValue::Integer(1))?;
            ctx.scope().set_config("gone", &ConfigValue::Integer(1))?;
            ctx.scope().remove_config("gone")?;
            ctx.scope().set_config("first", &ConfigValue::Integer(1))?;
            ctx.scope().set_config("second", &ConfigValue::Integer(2))?;
            Ok(())
        })
        .unwrap();
    let view = capture.0.lock().unwrap().pop().unwrap();
    let reads = view.image_reads();
    let clone = view.clone();
    assert_eq!(
        clone.row_keys(Table::Config).collect::<Vec<_>>(),
        [&RowKey::text("first"), &RowKey::text("second")]
    );
    let mut rows = clone.rows(Table::Config);
    assert_eq!(view.image_reads(), reads);
    assert!(view.row(Table::Config, &RowKey::text("unchanged")).unwrap().is_none());
    assert!(view.row(Table::Config, &RowKey::text("gone")).unwrap().is_none());
    assert_eq!(rows.next().unwrap().unwrap().key, RowKey::text("first"));
    // A failure armed after consuming one row belongs to the next row only.
    view.fail_image_reads(true);
    assert!(matches!(rows.next().unwrap(), Err(StorageError::Io { .. })));
    assert!(matches!(
        view.row(Table::Config, &RowKey::text("first")),
        Err(StorageError::Io { .. })
    ));
    assert_eq!(view.row_keys(Table::Config).count(), 2);
    view.fail_image_reads(false);
    assert!(
        view.row(Table::Config, &RowKey::text("second"))
            .unwrap()
            .unwrap()
            .before
            .is_none()
    );
    let reads = view.image_reads();
    view.row(Table::Config, &RowKey::text("first")).unwrap().unwrap();
    assert_eq!(
        view.image_reads() - reads,
        1,
        "only the present side of this row is read"
    );
}

/// Old-style observe remains the default target; failures and hook panics
/// are independent per observer, and neither changes commit publication.
struct FailureProbe {
    outcome: Option<bool>, // None succeeds; false errors; true panics.
    hook_panics: bool,
    seen: Mutex<Vec<meguri_engine::Seq>>,
    failed: Mutex<Vec<meguri_engine::Seq>>,
}
impl FailureProbe {
    fn new(outcome: Option<bool>, hook_panics: bool) -> Self {
        Self {
            outcome,
            hook_panics,
            seen: Mutex::new(Vec::new()),
            failed: Mutex::new(Vec::new()),
        }
    }
    fn clear(&self) {
        self.seen.lock().unwrap().clear();
        self.failed.lock().unwrap().clear();
    }
}
impl ChangeObserver for FailureProbe {
    fn name(&self) -> &str {
        "failure-probe"
    }
    fn observe(&self, commit: &ObservedCommit<'_>) -> Result<(), EngineError> {
        self.seen.lock().unwrap().push(commit.seq);
        match self.outcome {
            None => Ok(()),
            Some(false) => Err(EngineError::InvalidArgument {
                message: "observer refused".into(),
            }),
            Some(true) => panic!("observer panic"),
        }
    }
    fn observe_failed(&self, seq: meguri_engine::Seq) {
        self.failed.lock().unwrap().push(seq);
        assert!(!self.hook_panics, "failure hook panic");
    }
}

#[test]
fn error_and_panic_hooks_run_once_with_op_seq_and_hook_panics_leave_dispatch_and_publication_intact() {
    let probes: Vec<_> = [false, true]
        .into_iter()
        .flat_map(|panic| [false, true].map(|hook_panics| Arc::new(FailureProbe::new(Some(panic), hook_panics))))
        .collect();
    let following = Arc::new(FailureProbe::new(None, false));
    let mut ports = Ports::new();
    for probe in &probes {
        ports = ports.observer(probe.clone());
    }
    let h = Harness::open_with(ports.observer(following.clone()));
    for probe in probes.iter().chain(std::iter::once(&following)) {
        probe.clear();
    }
    let events = h.engine.stream().subscribe();
    let result = add_note_with_card(&h.collection, OpSource::SyncIncoming, &["committed", "back"]);
    for probe in &probes {
        assert_eq!(*probe.seen.lock().unwrap(), [result.seq]);
        assert_eq!(*probe.failed.lock().unwrap(), [result.seq]);
    }
    assert_eq!(*following.seen.lock().unwrap(), [result.seq]);
    assert!(following.failed.lock().unwrap().is_empty());
    let published: Vec<_> = std::iter::from_fn(|| events.try_recv()).collect();
    assert_eq!(change_seqs(&published), [result.seq.0]);
    assert_eq!(
        common::note_fields(&h.collection, result.value.0),
        "committed\u{1f}back"
    );
}

type LiveNote = (meguri_engine::Seq, RowImage, meguri_engine::CommitPayload);
#[derive(Default)]
struct LiveNotes(Mutex<Vec<LiveNote>>);
impl ChangeObserver for LiveNotes {
    fn name(&self) -> &str {
        "live-note-rows"
    }
    fn observe(&self, _: &ObservedCommit<'_>) -> Result<(), EngineError> {
        panic!("the rows callback must be dispatched")
    }
    fn observe_with_rows(
        &self,
        commit: &ObservedCommit<'_>,
        rows: &meguri_engine::ObserverRows<'_>,
    ) -> Result<(), EngineError> {
        for key in commit.payload.row_keys(Table::Notes) {
            let row = rows.row_image(Table::Notes, key)?.expect("stored note");
            assert!(matches!(
                commit.payload.row(Table::Notes, key)?.unwrap().after,
                RowAfter::NoteDigest(_)
            ));
            self.0.lock().unwrap().push((commit.seq, row, commit.payload.clone()));
        }
        assert!(
            rows.row_image(Table::Config, &RowKey::text("missing-live-row"))?
                .is_none()
        );
        Ok(())
    }
}

#[test]
fn callback_reads_real_note_row_at_each_commit_while_owned_payload_keeps_only_its_original_digest() {
    let live = Arc::new(LiveNotes::default());
    let h = Harness::open_with(Ports::new().observer(live.clone()));
    let first = add_note_with_card(&h.collection, OpSource::SyncIncoming, &["first", "back"]);
    let nid = first.value.0;
    let original = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Notes, &RowKey::Id(nid.0))
        .unwrap()
        .unwrap();
    let second = common::set_first_field(&h.collection, OpSource::SyncIncoming, nid, "second").unwrap();
    let later = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Notes, &RowKey::Id(nid.0))
        .unwrap()
        .unwrap();
    let seen = std::mem::take(&mut *live.0.lock().unwrap());
    assert_eq!(seen.len(), 2);
    assert_eq!((seen[0].0, seen[1].0), (first.seq, second.seq));
    assert_eq!(seen[0].1, original);
    assert_eq!(seen[1].1, later);
    assert_ne!(seen[0].1, seen[1].1);
    let before_close = seen[0].2.row(Table::Notes, &RowKey::Id(nid.0)).unwrap().unwrap();
    h.engine.close().unwrap();
    // Keeping the payload cannot retain a live reader/writer capability.
    assert_eq!(
        seen[0].2.row(Table::Notes, &RowKey::Id(nid.0)).unwrap().unwrap(),
        before_close
    );
    assert!(matches!(before_close.after, RowAfter::NoteDigest(_)));
}

#[test]
fn borrowed_writer_maximum_includes_prior_unjournaled_history_and_signed_raw_ids() {
    use meguri_engine::ObserverRows;
    use meguri_model::ids::CardId;
    struct Maximum(Mutex<Vec<(i64, i64, i64)>>);
    impl ChangeObserver for Maximum {
        fn name(&self) -> &str {
            "borrowed-history-maximum"
        }
        fn observe(&self, _: &ObservedCommit<'_>) -> Result<(), EngineError> {
            panic!("the rows callback must be dispatched")
        }
        fn observe_with_rows(&self, commit: &ObservedCommit<'_>, rows: &ObserverRows<'_>) -> Result<(), EngineError> {
            if commit.changes.config_keys.iter().any(|k| k.key == "read-history") {
                assert_eq!(commit.payload.rows(Table::Revlog).count(), 0);
                self.0.lock().unwrap().push((
                    rows.max_revlog_id(CardId(41))?,
                    rows.max_revlog_id(CardId(42))?,
                    rows.max_revlog_id(CardId(43))?,
                ));
            }
            Ok(())
        }
    }
    let observer = Arc::new(Maximum(Mutex::new(Vec::new())));
    let h = Harness::open_with(Ports::new().observer(observer.clone()));
    h.collection
        .run(
            OpSpec::named("raw history", OpSource::SyncIncoming).stamp(StampMode::Raw),
            |ctx| {
                for (card, id, kind) in [
                    (41, 1_735_689_600_123, 3),
                    (41, 1_735_689_600_999, 5),
                    (42, i64::MIN, 4),
                    (42, -1, 5),
                ] {
                    let mut row = revlog(CardId(card)).to_cells();
                    row[0] = Cell::Integer(id);
                    row[3] = Cell::Integer(0);
                    row[8] = Cell::Integer(kind);
                    ctx.scope().restore(Table::Revlog, &RowKey::Id(id), Some(&row))?;
                }
                Ok(())
            },
        )
        .unwrap();
    h.collection
        .run(
            OpSpec::named("read prior raw history", OpSource::UserNonUndoable),
            |ctx| {
                ctx.scope().set_config("read-history", &ConfigValue::Integer(1))?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(*observer.0.lock().unwrap(), [(1_735_689_600_999, -1, 0)]);
}
