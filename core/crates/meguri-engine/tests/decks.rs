//! Meguri-only contracts of decks/decks D16; golden coverage lives in testkit.
mod common;
use common::{Harness, NOW_MS, card, col_mod, note, query_int};
use meguri_engine::{
    EngineError, OpSource, OpSpec, UndoScope, UndoStatus,
    decks::{self, CollapseScope},
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};

fn names(h: &Harness) -> Vec<String> {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_rows()
        .unwrap()
        .iter()
        .map(|d| d.human_name())
        .collect()
}
/// D8 step 6: the summary read right before the delete, as a caller
/// with no question passes it.
fn confirmed(h: &Harness, ids: &[meguri_model::ids::DeckId]) -> decks::ConfirmedLosses {
    decks::ConfirmedLosses::Counts(decks::deletion_summary(&*h.collection.reader(ReaderRole::Query).unwrap(), ids).unwrap())
}
fn top(h: &Harness) -> Option<&'static str> {
    match h.collection.peek_undo(UndoScope::Global).unwrap() {
        UndoStatus::Available(s) => Some(s.name),
        _ => None,
    }
}

#[test]
fn naming_parents_rename_move_and_single_undo() {
    let h = common::fresh();
    let a = decks::add(&h.collection, "A::B::C".into()).unwrap();
    let rows = h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap();
    let a_id = rows.iter().find(|d| d.name == "A").unwrap().id;
    assert_eq!(a.value.id.0, NOW_MS + 2);
    let before = names(&h);
    h.clock.step_ms(60_000);
    let rename = decks::rename(&h.collection, a_id, "X::A".into()).unwrap();
    assert_eq!(rename.value.human_name(), "X::A");
    assert_eq!(
        rename
            .changes
            .decks
            .iter()
            .filter(|d| d.kind == meguri_engine::ChangeKind::Updated)
            .count(),
        3
    );
    assert_eq!(top(&h), Some("Rename Deck"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(names(&h), before);
    let root = decks::add(&h.collection, "Target".into()).unwrap().value.id;
    let moved = decks::move_decks(&h.collection, vec![a_id], Some(root)).unwrap();
    assert_eq!(moved.value, 1);
    assert_eq!(moved.changes.decks.len(), 3);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(
        decks::names::native_name("e\u{1}\u{301}:: 日本語 ::😀"),
        "é\u{1f}日本語\u{1f}😀"
    );
    assert!(names(&h).contains(&"A::B::C".into()));
    assert!(matches!(
        decks::rename(&h.collection, a_id, "a::b::A".into()),
        Err(EngineError::DeckRefused(decks::Refusal::UnderDescendant))
    ));
}

#[derive(Default)]
struct Backup(AtomicUsize);
impl meguri_engine::ports::BackupService for Backup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[test]
fn delete_120_cards_summary_changes_backup_and_undo() {
    let h = common::fresh();
    let a = decks::add(&h.collection, "Home".into()).unwrap().value.id;
    let b = decks::add(&h.collection, "Home::B".into()).unwrap().value.id;
    let c = decks::add(&h.collection, "Home::C".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable), move |ctx| {
            for i in 0..120 {
                let n = ctx.scope().add_note(&note(&["front", "back"]))?;
                let mut card = card(n, 0);
                card.deck_id = [a, b, c][i % 3];
                ctx.scope().add_card(&card)?;
            }
            ctx.scope()
                .set_config(&format!("_deck_{}_lastNotetype", a.0), &ConfigValue::from(4_i64))?;
            ctx.scope()
                .set_config(&format!("_deck_{}_", a.0), &ConfigValue::from(7_i64))?;
            Ok(())
        })
        .unwrap();
    let before_mod = col_mod(&h.collection);
    let summary = decks::deletion_summary(&*h.collection.reader(ReaderRole::Query).unwrap(), &[a, b, a]).unwrap();
    assert_eq!(summary, decks::DeletionSummary { decks: 3, cards: 120 });
    let backup = Arc::new(Backup::default());
    h.clock.step_ms(60_000);
    let deleted = decks::delete(&h.collection, vec![a, b], decks::ConfirmedLosses::Counts(summary), backup.clone()).unwrap();
    assert_eq!(deleted.value, summary);
    assert_eq!(backup.0.load(Ordering::SeqCst), 1);
    assert_eq!(deleted.changes.decks.len(), 3);
    assert_eq!(deleted.changes.cards.len(), 120);
    assert_eq!(deleted.changes.notes.len(), 120);
    assert_eq!(
        deleted.changes.decks.iter().map(|d| d.id).collect::<Vec<_>>(),
        [a, b, c]
    );
    assert_eq!(
        deleted.changes.cards.iter().map(|d| d.id.0).collect::<Vec<_>>(),
        (NOW_MS..NOW_MS + 120).collect::<Vec<_>>()
    );
    assert_eq!(
        deleted.changes.notes.iter().map(|d| d.id.0).collect::<Vec<_>>(),
        (NOW_MS..NOW_MS + 120).collect::<Vec<_>>()
    );
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 243);
    assert_eq!(top(&h), Some("Delete Deck"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col_mod(&h.collection), before_mod);
    assert_eq!(
        query_int(
            &h.collection,
            "SELECT count(*) FROM cards WHERE did IN (SELECT id FROM decks WHERE name LIKE 'Home%')"
        ),
        120
    );
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value(&format!("_deck_{}_lastNotetype", a.0))
            .unwrap(),
        Some(ConfigValue::from(4_i64))
    );
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 243);
}

#[test]
fn collapse_scope_selection_and_not_found() {
    let h = common::fresh();
    let a = decks::add(&h.collection, "A".into()).unwrap().value.id;
    let b = decks::add(&h.collection, "B".into()).unwrap().value.id;
    let backup = Arc::new(Backup::default());
    decks::delete(&h.collection, vec![b], confirmed(&h, &[b]), backup.clone()).unwrap();
    assert_eq!(backup.0.load(Ordering::SeqCst), 0);
    decks::collapse(&h.collection, vec![a], CollapseScope::Browser, false).unwrap();
    assert_eq!(top(&h), Some("Expand/Collapse"));
    let deck = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(a)
        .unwrap()
        .unwrap();
    assert!(deck.common.study_collapsed);
    assert!(!deck.common.browser_collapsed);
    decks::select(&h.collection, a).unwrap();
    assert_eq!(top(&h), Some("Expand/Collapse"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(top(&h), Some("Delete Deck"));
    assert_eq!(
        decks::current(&*h.collection.reader(ReaderRole::Query).unwrap()).unwrap(),
        a
    );
    let before = col_mod(&h.collection);
    assert!(matches!(
        decks::select(&h.collection, b),
        Err(EngineError::NotFound { .. })
    ));
    assert!(matches!(
        decks::move_decks(&h.collection, vec![a], Some(b)),
        Err(EngineError::NotFound { .. })
    ));
    assert!(matches!(
        decks::rename(&h.collection, b, "gone".into()),
        Err(EngineError::NotFound { .. })
    ));
    assert_eq!(col_mod(&h.collection), before);
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(names(&h).contains(&"B".into()));
}

#[test]
fn bulk_deck_edits_at_2000_decks_are_bounded() {
    use meguri_engine::deck_config;
    use meguri_model::{ids::DeckConfigId, proto::deck::DeckKind};
    use meguri_testkit::wallclock::Stopwatch;
    use std::time::Duration;

    let h = common::fresh();
    let (ids, preset) = h
        .collection
        .run(common::tagging::setup_spec(), |ctx| {
            let preset = deck_config::add_in(ctx.scope(), "Bulk")?;
            let mut ids = vec![ctx.scope().add_deck(&decks::new_deck("Root".into()))?];
            // Default + Root + 1,998 descendants: R1805's 2,000 decks.
            for i in 0..1998 {
                ids.push(ctx.scope().add_deck(&decks::new_deck(format!("Root\u{1f}{i}")))?);
            }
            Ok((ids, preset))
        })
        .unwrap()
        .value;
    let before = h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap();
    h.clock.step_ms(60_000);

    let timer = Stopwatch::start();
    let collapsed = decks::collapse(&h.collection, ids.clone(), CollapseScope::Study, false).unwrap();
    let collapse_time = timer.elapsed();
    assert_eq!(collapsed.changes.decks.len(), 1999);

    let timer = Stopwatch::start();
    let assigned = deck_config::assign(&h.collection, ids[0], preset, true).unwrap();
    let assign_time = timer.elapsed();
    assert_eq!(assigned.changes.decks.len(), 1999);

    let timer = Stopwatch::start();
    let removed = deck_config::remove(&h.collection, preset, DeckConfigId(1)).unwrap();
    let remove_time = timer.elapsed();
    assert_eq!(removed.changes.decks.len(), 1999);
    let rows = h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap();
    assert_eq!(rows.len(), 2000);
    for row in rows.iter().filter(|d| ids.contains(&d.id)) {
        assert!(!row.common.study_collapsed);
        assert!(row.common.browser_collapsed);
        assert!(matches!(&row.kind.kind, DeckKind::Normal(n) if n.config_id == 1));
        assert_eq!(row.mtime.0, (NOW_MS + 60_000) / 1000);
        assert_eq!(row.usn.0, -1);
    }
    // Each bulk edit remains one undo step; the original rows are restored.
    for _ in 0..3 {
        h.collection.undo(UndoScope::Global).unwrap();
    }
    let restored = h.collection.reader(ReaderRole::Query).unwrap().deck_rows().unwrap();
    let cells =
        |rows: &[meguri_model::deck::Deck]| rows.iter().map(meguri_storage::Record::to_cells).collect::<Vec<_>>();
    assert_eq!(cells(&restored), cells(&before));

    eprintln!("2,000 decks: collapse={collapse_time:?}, assign={assign_time:?}, remove={remove_time:?}");
    // A loose test-build regression bound for table rescans, not the
    // device/frame budget of R1822. nextest runs this test alone.
    for (op, elapsed) in [
        ("collapse", collapse_time),
        ("assign", assign_time),
        ("remove", remove_time),
    ] {
        assert!(elapsed < Duration::from_secs(2), "{op}: {elapsed:?}");
    }
}

#[test]
fn backup_failure_aborts_and_concurrent_card_addition_replans() {
    struct BackupWithWrite {
        collection: meguri_engine::Collection,
        deck: meguri_model::ids::DeckId,
        calls: AtomicUsize,
    }
    impl meguri_engine::ports::BackupService for BackupWithWrite {
        fn before_deck_delete(&self) -> Result<(), EngineError> {
            // This succeeds only off the writer, with the maintenance
            // snapshot released. Its write invalidates the first summary.
            if self.calls.fetch_add(1, Ordering::SeqCst) == 0 {
                let deck = self.deck;
                self.collection
                    .run(OpSpec::named("concurrent_add", OpSource::UserUndoable), move |ctx| {
                        let nid = ctx.scope().add_note(&note(&["extra", "back"]))?;
                        let mut card = card(nid, 0);
                        card.deck_id = deck;
                        ctx.scope().add_card(&card)?;
                        Ok(())
                    })?;
            }
            Ok(())
        }
    }
    struct FailedBackup;
    impl meguri_engine::ports::BackupService for FailedBackup {
        fn before_deck_delete(&self) -> Result<(), EngineError> {
            Err(EngineError::Internal {
                message: "backup failed".into(),
            })
        }
    }
    let h = common::fresh();
    let a = decks::add(&h.collection, "A".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("seed", OpSource::UserUndoable), move |ctx| {
            let nid = ctx.scope().add_note(&note(&["one", "back"]))?;
            let mut card = card(nid, 0);
            card.deck_id = a;
            ctx.scope().add_card(&card)?;
            Ok(())
        })
        .unwrap();
    let before = col_mod(&h.collection);
    assert!(decks::delete(&h.collection, vec![a], decks::ConfirmedLosses::Any, Arc::new(FailedBackup)).is_err());
    assert_eq!(col_mod(&h.collection), before);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 1);
    let backup = Arc::new(BackupWithWrite {
        collection: h.collection.clone(),
        deck: a,
        calls: AtomicUsize::new(0),
    });
    // `Any` (AnkiConnect's `cardsToo`): the card added during the backup is
    // removed after a replan; counts would refuse it (deck_delete_losses.rs).
    let deleted = decks::delete(&h.collection, vec![a], decks::ConfirmedLosses::Any, backup.clone()).unwrap();
    assert_eq!(deleted.value, decks::DeletionSummary { cards: 2, decks: 1 });
    assert_eq!(backup.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn rename_under_old_name_moves_subtree_and_keeps_future_blobs() {
    use meguri_model::proto::ProtoBlob;
    let h = common::fresh();
    let a = decks::add(&h.collection, "A".into()).unwrap().value.id;
    let b = decks::add(&h.collection, "A::B".into()).unwrap().value.id;
    let (common_bytes, kind_bytes) = h
        .collection
        .run(OpSpec::named("future", OpSource::UserNonUndoable), move |ctx| {
            let mut row = ctx.scope().deck(a)?.unwrap();
            let mut common = row.common.to_bytes().into_owned();
            common.extend([0x98, 6, 7]); // unknown field 99
            row.common = ProtoBlob::decode(&common).unwrap();
            let mut kind = row.kind.to_bytes().into_owned();
            kind.extend([0x98, 6, 9]);
            row.kind = ProtoBlob::decode(&kind).unwrap();
            ctx.scope().put(&row)?;
            Ok((common, kind))
        })
        .unwrap()
        .value;
    decks::rename(&h.collection, a, "A::Z::A".into()).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let row = r.deck(a).unwrap().unwrap();
    assert_eq!(row.common.to_bytes().as_ref(), common_bytes);
    assert_eq!(row.kind.to_bytes().as_ref(), kind_bytes);
    assert_eq!(r.deck(b).unwrap().unwrap().human_name(), "A::Z::A::B");
    drop(r);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(names(&h), ["A", "A::B", "Default"]);
}

/// A filtered deck `Filt` (id 500) at the top level, built as Anki leaves one.
fn add_filtered(scope: &mut meguri_storage::WriteScope<'_>) -> Result<meguri_model::ids::DeckId, EngineError> {
    use meguri_model::proto::{
        ProtoBlob,
        deck::{DeckKind, DeckKindMessage, FilteredDeck},
    };
    let mut deck = scope.deck(meguri_model::ids::DeckId(1))?.unwrap();
    deck.id = meguri_model::ids::DeckId(500);
    deck.name = "Filt".into();
    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
    scope.put(&deck)?;
    Ok(deck.id)
}

/// Moves a card into a filtered deck as a filtered-deck build does.
fn pull_into(
    scope: &mut meguri_storage::WriteScope<'_>,
    card: meguri_model::ids::CardId,
    filtered: meguri_model::ids::DeckId,
) -> Result<(), EngineError> {
    let mut row = scope.card(card)?.unwrap();
    row.original_deck_id = row.deck_id;
    row.original_due = row.due;
    row.deck_id = filtered;
    row.due = -100_000;
    scope.put(&row)?;
    Ok(())
}

fn summary_of(h: &Harness, ids: &[meguri_model::ids::DeckId]) -> decks::DeletionSummary {
    decks::deletion_summary(&*h.collection.reader(ReaderRole::Query).unwrap(), ids).unwrap()
}

#[test]
fn delete_undo_restores_every_row_and_review_history_and_redo_reapplies() {
    use common::{revlog, steps::dump};
    let h = common::fresh();
    let home = decks::add(&h.collection, "Home".into()).unwrap().value.id;
    let sub = decks::add(&h.collection, "Home::Sub".into()).unwrap().value.id;
    let other = decks::add(&h.collection, "Other".into()).unwrap().value.id;
    let (filt, kept_note, other_in_filt) = h
        .collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable), move |ctx| {
            let s = ctx.scope();
            let filt = add_filtered(s)?;
            let mut add = |deck, front: &str| -> Result<_, EngineError> {
                let nid = s.add_note(&note(&[front, "back"]))?;
                let mut c = card(nid, 0);
                c.deck_id = deck;
                Ok((nid, s.add_card(&c)?))
            };
            let (_, h1) = add(home, "h1")?;
            let (_, h2) = add(home, "h2")?;
            add(sub, "s1")?;
            let (_, o1) = add(other, "o1")?;
            add(other, "o2")?;
            // One note with a card in Home and one in Other: it keeps a card.
            let shared = s.add_note(&note(&["shared", "back"]))?;
            for (ord, deck) in [(0, home), (1, other)] {
                let mut c = card(shared, ord);
                c.deck_id = deck;
                s.add_card(&c)?;
            }
            s.add_revlog(&revlog(h1))?;
            s.add_revlog(&revlog(h1))?;
            pull_into(s, h2, filt)?;
            pull_into(s, o1, filt)?;
            s.set_config(&format!("_deck_{}_lastNotetype", home.0), &ConfigValue::from(4_i64))?;
            Ok((filt, shared, o1))
        })
        .unwrap()
        .value;
    h.clock.step_ms(60_000);
    let before = dump(&h.collection);
    // h1, h2 (in Filt, home Home), s1 and the shared note's Home card.
    let summary = summary_of(&h, &[home]);
    assert_eq!(summary, decks::DeletionSummary { decks: 2, cards: 4 });
    let backup = Arc::new(Backup::default());
    let deleted = decks::delete(&h.collection, vec![home], confirmed(&h, &[home]), backup.clone()).unwrap();
    assert_eq!(deleted.value, summary);
    assert_eq!(backup.0.load(Ordering::SeqCst), 1);
    assert_eq!(top(&h), Some("Delete Deck"));
    let after = dump(&h.collection);
    // Review history is never touched by a delete.
    assert_eq!(after["revlog"], before["revlog"]);
    assert_eq!(after["revlog"].len(), 2);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM notes"), 3);
    assert_eq!(
        query_int(&h.collection, &format!("SELECT count(*) FROM notes WHERE id = {}", kept_note.0)),
        1
    );
    assert_eq!(
        query_int(&h.collection, &format!("SELECT did FROM cards WHERE id = {}", other_in_filt.0)),
        filt.0
    );
    assert_eq!(
        query_int(&h.collection, "SELECT count(*) FROM graves WHERE type = 0 AND usn = -1"),
        4
    );
    assert_eq!(
        query_int(&h.collection, "SELECT count(*) FROM graves WHERE type = 1 AND usn = -1"),
        3
    );
    assert_eq!(
        query_int(&h.collection, "SELECT count(*) FROM graves WHERE type = 2 AND usn = -1"),
        2
    );
    assert_eq!(
        query_int(
            &h.collection,
            &format!("SELECT count(*) FROM config WHERE KEY = '_deck_{}_lastNotetype'", home.0)
        ),
        0
    );
    assert_ne!(after["col"], before["col"]);

    h.collection.undo(UndoScope::Global).unwrap();
    // Decks, cards, notes, graves, config keys, review history and col.mod.
    assert_eq!(dump(&h.collection), before);
    assert_eq!(summary_of(&h, &[home]), summary);
    match h.collection.peek_redo(UndoScope::Global).unwrap() {
        UndoStatus::Available(step) => assert_eq!(step.name, "Delete Deck"),
        other => panic!("no redo step: {other:?}"),
    }

    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h.collection), after);
    assert_eq!(top(&h), Some("Delete Deck"));
    assert!(!names(&h).iter().any(|n| n.starts_with("Home")));
}

#[test]
fn filtered_deck_delete_returns_cards_home_and_undo_puts_them_back() {
    use common::steps::dump;
    let h = common::fresh();
    let home = decks::add(&h.collection, "Home".into()).unwrap().value.id;
    let (filt, pulled) = h
        .collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable), move |ctx| {
            let s = ctx.scope();
            let filt = add_filtered(s)?;
            let mut pulled = vec![];
            for front in ["a", "b"] {
                let nid = s.add_note(&note(&[front, "back"]))?;
                let mut c = card(nid, 0);
                c.deck_id = home;
                c.due = 7;
                let cid = s.add_card(&c)?;
                pull_into(s, cid, filt)?;
                pulled.push(cid);
            }
            Ok((filt, pulled))
        })
        .unwrap()
        .value;
    h.clock.step_ms(60_000);
    let before = dump(&h.collection);
    // N20.7: no card is lost, so the UI deletes at once.
    assert_eq!(summary_of(&h, &[filt]), decks::DeletionSummary { decks: 1, cards: 0 });
    let backup = Arc::new(Backup::default());
    let deleted = decks::delete(&h.collection, vec![filt], confirmed(&h, &[filt]), backup.clone()).unwrap();
    assert_eq!(deleted.value, decks::DeletionSummary { decks: 1, cards: 0 });
    assert_eq!(backup.0.load(Ordering::SeqCst), 0);
    assert_eq!(top(&h), Some("Delete Deck"));
    for cid in &pulled {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let row = reader.card(*cid).unwrap().unwrap();
        assert_eq!((row.deck_id, row.original_deck_id.0, row.due, row.original_due), (home, 0, 7, 0));
    }
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 2);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves"), 1);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h.collection), before);
}

#[test]
fn deleting_a_deck_whose_cards_sit_in_a_filtered_deck_keeps_the_filtered_deck() {
    let h = common::fresh();
    let home = decks::add(&h.collection, "Home".into()).unwrap().value.id;
    let filt = h
        .collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable), move |ctx| {
            let s = ctx.scope();
            let filt = add_filtered(s)?;
            let nid = s.add_note(&note(&["only", "back"]))?;
            let mut c = card(nid, 0);
            c.deck_id = home;
            let cid = s.add_card(&c)?;
            pull_into(s, cid, filt)?;
            Ok(filt)
        })
        .unwrap()
        .value;
    // The card is counted for its home deck, not for the filtered deck.
    assert_eq!(summary_of(&h, &[home]), decks::DeletionSummary { decks: 1, cards: 1 });
    assert_eq!(summary_of(&h, &[filt]), decks::DeletionSummary { decks: 1, cards: 0 });
    let deleted = decks::delete(&h.collection, vec![home], confirmed(&h, &[home]), Arc::new(Backup::default())).unwrap();
    assert_eq!(deleted.value.cards, 1);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 0);
    assert!(names(&h).contains(&"Filt".into()));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(
        query_int(&h.collection, &format!("SELECT count(*) FROM cards WHERE did = {}", filt.0)),
        1
    );
}

#[test]
fn deleting_the_nested_default_deck_keeps_it_renamed_at_the_top_and_undo_restores_it() {
    use common::steps::dump;
    use meguri_model::ids::DeckId;
    let h = common::fresh();
    decks::rename(&h.collection, DeckId(1), "Outer::Inner".into()).unwrap();
    let outer = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_rows()
        .unwrap()
        .into_iter()
        .find(|d| d.name == "Outer")
        .unwrap()
        .id;
    decks::add(&h.collection, "Default".into()).unwrap();
    let kid = decks::add(&h.collection, "Outer::Inner::Kid".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable), move |ctx| {
            for (front, deck) in [("d", DeckId(1)), ("k", kid)] {
                let nid = ctx.scope().add_note(&note(&[front, "back"]))?;
                let mut c = card(nid, 0);
                c.deck_id = deck;
                ctx.scope().add_card(&c)?;
            }
            Ok(())
        })
        .unwrap();
    h.clock.step_ms(60_000);
    let before = dump(&h.collection);
    // The default deck counts among the decks; it asks as any deck does.
    assert_eq!(summary_of(&h, &[outer]), decks::DeletionSummary { decks: 3, cards: 2 });
    decks::delete(&h.collection, vec![outer], confirmed(&h, &[outer]), Arc::new(Backup::default())).unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let default = reader.deck(DeckId(1)).unwrap().unwrap();
    assert_eq!(default.human_name(), "Default+");
    assert_eq!(default.mtime.0, (NOW_MS + 60_000) / 1000);
    assert_eq!(default.usn.0, -1);
    assert!(reader.deck(outer).unwrap().is_none());
    assert!(reader.deck(kid).unwrap().is_none());
    drop(reader);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM cards"), 0);
    assert_eq!(query_int(&h.collection, "SELECT count(*) FROM graves WHERE type = 2"), 2);
    assert_eq!(
        query_int(&h.collection, "SELECT count(*) FROM graves WHERE type = 2 AND oid = 1"),
        0
    );
    assert_eq!(top(&h), Some("Delete Deck"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h.collection), before);
    assert!(names(&h).contains(&"Outer::Inner::Kid".into()));
}
