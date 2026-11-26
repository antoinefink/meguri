//! Writes that change nothing and the stamps of real changes
//! (`storage/ids-usn-mtime` S2 to S6; R776, R906, R1456, R1471, R1472). The
//! scenario replays compare stored rows with the oracle's goldens.

mod common;

use common::{NOW_MS, card, deck, empty_collection, exec, expected_steps, new_col, note, open, revlog, tag};
use meguri_model::card::{CardQueue, CardType};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::col::Col;
use meguri_model::ids::NoteId;
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, JournalOptions, ReadRows, RowKey, StampMode, Table, Written};
use serde_json::{Value, json};

const HOUR_MS: i64 = 3_600_000;

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

/// A raw query's rows as golden `db_exec` values.
fn rows(writer: &meguri_storage::Writer, sql: &str) -> Value {
    Value::Array(
        exec(writer, sql)
            .rows
            .iter()
            .map(|row| {
                Value::Array(
                    row.iter()
                        .map(|v| match v {
                            SqlValue::Null => Value::Null,
                            SqlValue::Integer(i) => json!(i),
                            SqlValue::Real(f) => json!(f),
                            SqlValue::Text(t) => json!(t),
                            SqlValue::Blob(_) => json!("blob"),
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

#[test]
fn an_identical_note_rewrite_changes_nothing_and_a_real_edit_is_stamped() {
    let steps = expected_steps("storage/noop/identical-note-rewrite");
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    let mut cat = note(1, &["cat", "gato"]);
    cat.tags = " animal ".to_owned();
    let id = scope.add_note(&cat).unwrap();
    scope.add_card(&card(id, 0)).unwrap();
    scope.commit().unwrap();
    exec(writer, "UPDATE notes SET usn = 7");
    let select = "SELECT notes.mod, notes.usn, notes.flds, notes.tags, col.mod FROM notes, col";

    clock.step_ms(HOUR_MS);
    let stored = writer.note(id).unwrap().unwrap();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.put(&stored).unwrap(), Written::Unchanged);
    let committed = scope.commit().unwrap();
    assert!(!committed.changed && committed.col_mod.is_none());
    assert!(committed.journal.unwrap().is_empty());
    assert_eq!(rows(writer, select), steps[3]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let mut edited = writer.note(id).unwrap().unwrap();
    edited.fields = "cat\u{1f}gata".to_owned();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.put(&edited).unwrap(), Written::Updated);
    scope.commit().unwrap();
    assert_eq!(rows(writer, select), steps[7]["db_exec"]["rows"]);
}

#[test]
fn answering_stamps_the_card_the_review_log_row_and_col_mod() {
    let steps = expected_steps("storage/stamps/answer");
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let mut cards = Vec::new();
    for fields in [["sun", "sol"], ["moon", "luna"]] {
        let id = scope.add_note(&note(1, &fields)).unwrap();
        cards.push(scope.add_card(&card(id, 0)).unwrap());
    }
    scope.commit().unwrap();
    exec(writer, "UPDATE cards SET usn = 7");
    assert_eq!(rows(writer, "SELECT mod, usn FROM col"), steps[1]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let mut answered = writer.card(cards[0]).unwrap().unwrap();
    answered.card_type = CardType::Learning;
    answered.queue = CardQueue::Learning;
    answered.reps = 1;
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    scope.put(&answered).unwrap();
    scope.add_revlog(&revlog(cards[0], 2000)).unwrap();
    scope.commit().unwrap();

    let observed = &steps[4];
    for (card, name) in cards.iter().zip(["#c1", "#c2"]) {
        let stored = writer.card(*card).unwrap().unwrap();
        assert_eq!(json!(stored.mtime.0), observed["card"][name]["mod"], "{name}");
        assert_eq!(json!(stored.usn.0), observed["card"][name]["usn"], "{name}");
    }
    assert_eq!(
        rows(writer, "SELECT usn FROM revlog"),
        json!([[observed["revlog"]["#c1"][0]["usn"]]])
    );
    assert_eq!(rows(writer, "SELECT mod, usn FROM col"), steps[5]["db_exec"]["rows"]);
}

#[test]
fn a_change_moves_col_mod_and_leaves_crt_scm_usn_and_ls() {
    let steps = expected_steps("storage/stamps/col-row");
    let before = &steps[0]["db_exec"]["rows"][0];
    let col = Col {
        created: EpochSecs(before[1].as_i64().unwrap()),
        modified: EpochMillis(before[2].as_i64().unwrap()),
        schema_modified: EpochMillis(before[3].as_i64().unwrap()),
        ..new_col(NOW_MS)
    };
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.put_col(&col).unwrap();
    scope.commit().unwrap();
    let select = "SELECT id, crt, mod, scm, ver, dty, usn, ls, conf, models, decks, dconf, tags FROM col";
    assert_eq!(rows(writer, select), steps[0]["db_exec"]["rows"]);

    clock.step_ms(HOUR_MS);
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["sun", "sol"])).unwrap();
    scope.add_card(&card(id, 0)).unwrap();
    let committed = scope.commit().unwrap();
    assert_eq!(committed.col_mod, Some(EpochMillis(NOW_MS + HOUR_MS)));
    assert_eq!(rows(writer, select), steps[3]["db_exec"]["rows"]);
}

#[test]
fn a_keep_mtime_write_keeps_the_incoming_mtime_with_a_pending_usn() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["old", "text"])).unwrap();
    scope.commit().unwrap();
    exec(writer, "UPDATE notes SET usn = 4");

    clock.step_ms(HOUR_MS);
    let mut incoming = writer.note(id).unwrap().unwrap();
    incoming.fields = "new\u{1f}text".to_owned();
    incoming.mtime = EpochSecs(1_800_000_000);
    let mut scope = writer.begin_write(StampMode::KeepMtime, &clock, None).unwrap();
    assert_eq!(scope.put(&incoming).unwrap(), Written::Updated);
    let committed = scope.commit().unwrap();
    let stored = writer.note(id).unwrap().unwrap();
    assert_eq!((stored.mtime, stored.usn), (EpochSecs(1_800_000_000), Usn::PENDING));
    assert_eq!(committed.col_mod, Some(EpochMillis(NOW_MS + HOUR_MS)));
    assert_eq!(writer.col().unwrap().modified, EpochMillis(NOW_MS + HOUR_MS));
}

#[test]
fn an_undo_restore_writes_the_prior_usn_mtime_and_col_mod_exactly() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["a", "b"])).unwrap();
    scope.commit().unwrap();
    exec(writer, "UPDATE notes SET usn = 9, mod = 1700000123");
    let original = writer.note(id).unwrap().unwrap();
    let original_col_mod = writer.col().unwrap().modified;

    clock.step_ms(HOUR_MS);
    let mut edited = original.clone();
    edited.fields = "a\u{1f}c".to_owned();
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    scope.put(&edited).unwrap();
    let journal = scope.commit().unwrap().journal.unwrap();
    assert_eq!(writer.note(id).unwrap().unwrap().usn, Usn::PENDING);

    clock.step_ms(HOUR_MS);
    let mut undo = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    for entry in journal.entries() {
        let before = journal.before(entry).unwrap();
        if entry.table() == Table::Col {
            let Some(cells) = before else {
                unreachable!("col existed")
            };
            undo.restore(Table::Col, entry.key(), Some(&cells)).unwrap();
        } else {
            undo.restore(entry.table(), entry.key(), before.as_ref()).unwrap();
        }
    }
    let committed = undo.commit().unwrap();
    assert!(committed.col_mod.is_none(), "an undo sets col.mod itself");
    let restored = writer.note(id).unwrap().unwrap();
    assert_eq!(restored, original);
    assert_eq!(writer.col().unwrap().modified, original_col_mod);
}

#[test]
fn marking_a_thousand_rows_with_a_server_usn_keeps_it_and_leaves_col_mod() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let ids: Vec<NoteId> = (0..1000)
        .map(|i| scope.add_note(&note(1, &[&format!("n{i}"), "x"])).unwrap())
        .collect();
    scope.commit().unwrap();
    let col_mod = writer.col().unwrap().modified;

    clock.step_ms(HOUR_MS);
    let keys: Vec<RowKey> = ids.iter().map(|id| RowKey::Id(id.0)).collect();
    let mut scope = writer
        .begin_write(StampMode::CallerSupplied, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.mark_usn(Table::Notes, &keys, Usn(42)).unwrap(), 1000);
    // Marking again changes nothing.
    assert_eq!(scope.mark_usn(Table::Notes, &keys, Usn(42)).unwrap(), 0);
    let committed = scope.commit().unwrap();
    assert_eq!(committed.journal.unwrap().len(), 1000);
    assert!(committed.col_mod.is_none());
    assert_eq!(writer.col().unwrap().modified, col_mod);
    for id in &ids {
        let stored = writer.note(*id).unwrap().unwrap();
        assert_eq!(stored.usn, Usn(42));
        assert_eq!(stored.mtime, EpochSecs(NOW_MS / 1000));
    }
    assert_eq!(
        common::int(&exec(writer, "SELECT count(*) FROM notes WHERE usn = -1")),
        0
    );
    // A Local scope cannot mark: it would stamp.
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    assert!(scope.mark_usn(Table::Notes, &keys, Usn(43)).is_err());
}

/// One target of the property: a row of some table, rewritten as stored or
/// with a change.
#[derive(Clone, Copy, Debug)]
enum Target {
    Note(usize),
    Card(usize),
    Deck(usize),
    Config(usize),
    Tag(usize),
}

/// The stamps a target's row carries: `(usn, mtime)`; tags have no mtime.
fn stamps(writer: &meguri_storage::Writer, target: Target, ids: &Ids) -> (i64, i64) {
    match target {
        Target::Note(i) => {
            let n = writer.note(ids.notes[i]).unwrap().unwrap();
            (i64::from(n.usn.0), n.mtime.0)
        }
        Target::Card(i) => {
            let c = writer.card(ids.cards[i]).unwrap().unwrap();
            (i64::from(c.usn.0), c.mtime.0)
        }
        Target::Deck(i) => {
            let d = writer.deck(ids.decks[i]).unwrap().unwrap();
            (i64::from(d.usn.0), d.mtime.0)
        }
        Target::Config(i) => {
            let e = writer.config_entry(&format!("key{i}")).unwrap().unwrap().entry;
            (i64::from(e.usn.0), e.mtime.0)
        }
        Target::Tag(i) => (i64::from(writer.tag(&format!("tag{i}")).unwrap().unwrap().usn.0), 0),
    }
}

struct Ids {
    notes: Vec<NoteId>,
    cards: Vec<meguri_model::ids::CardId>,
    decks: Vec<meguri_model::ids::DeckId>,
}

/// Rewrites a target as stored (`change` false) or with one column
/// changed, in the scope.
fn rewrite(scope: &mut meguri_storage::WriteScope<'_>, target: Target, change: bool, salt: u64, ids: &Ids) -> Written {
    match target {
        Target::Note(i) => {
            let mut n = scope.note(ids.notes[i]).unwrap().unwrap();
            if change {
                n.fields = format!("v{salt}\u{1f}x");
            }
            scope.put(&n).unwrap()
        }
        Target::Card(i) => {
            let mut c = scope.card(ids.cards[i]).unwrap().unwrap();
            if change {
                c.due = i32::try_from(salt % 100_000).unwrap() + 2;
            }
            scope.put(&c).unwrap()
        }
        Target::Deck(i) => {
            let mut d = scope.deck(ids.decks[i]).unwrap().unwrap();
            if change {
                d.common.study_collapsed = !d.common.study_collapsed;
            }
            scope.put(&d).unwrap()
        }
        Target::Config(i) => {
            let key = format!("key{i}");
            let value = if change {
                ConfigValue::from(salt as i64 + 1_000_000)
            } else {
                scope.config_value(&key).unwrap().unwrap()
            };
            scope.set_config(&key, &value).unwrap()
        }
        Target::Tag(i) => {
            let mut t = scope.tag(&format!("tag{i}")).unwrap().unwrap();
            if change {
                t.collapsed = !t.collapsed;
            }
            scope.put(&t).unwrap()
        }
    }
}

/// S4 and S5 as a property: over 10,000 random operations mixing real and
/// identical rewrites (an identical one rewrites the stored row, stamps
/// included), an identical rewrite never changes its row's usn or mtime, an
/// operation of only identical rewrites never moves `col.mod`, and a real
/// one stamps usn -1, mtime now and `col.mod` now.
#[test]
fn identical_rewrites_never_stamp_over_ten_thousand_random_operations() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    // Commits need no durability here.
    exec(writer, "PRAGMA synchronous = OFF");
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let mut ids = Ids {
        notes: Vec::new(),
        cards: Vec::new(),
        decks: Vec::new(),
    };
    for i in 0..8 {
        let id = scope.add_note(&note(1, &[&format!("n{i}"), "x"])).unwrap();
        ids.notes.push(id);
        ids.cards.push(scope.add_card(&card(id, 0)).unwrap());
        ids.decks.push(scope.add_deck(&deck(&format!("deck{i}"))).unwrap());
        scope
            .set_config(&format!("key{i}"), &ConfigValue::from(i as i64))
            .unwrap();
        scope.put(&tag(&format!("tag{i}"))).unwrap();
    }
    scope.commit().unwrap();

    let random = SeededRandom::new(0x6e6f_6f70);
    for case in 0..10_000u64 {
        clock.step_ms(1500);
        let now = EpochMillis(clock_now(&clock));
        let col_mod = writer.col().unwrap().modified;
        let count = 1 + random.next_u64() % 3;
        let mut planned = Vec::new();
        for _ in 0..count {
            let i = (random.next_u64() % 8) as usize;
            let target = match random.next_u64() % 5 {
                0 => Target::Note(i),
                1 => Target::Card(i),
                2 => Target::Deck(i),
                3 => Target::Config(i),
                _ => Target::Tag(i),
            };
            let change = random.next_u64().is_multiple_of(2);
            planned.push((target, change));
        }
        let before: Vec<(i64, i64)> = planned.iter().map(|(t, _)| stamps(writer, *t, &ids)).collect();
        let mut scope = writer
            .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
            .unwrap();
        let mut outcomes = Vec::new();
        for (target, change) in &planned {
            outcomes.push(rewrite(&mut scope, *target, *change, case, &ids));
        }
        // A planned change always changes its row the first time in an
        // operation: its value is new to this case, or a toggle.
        let real = planned.iter().any(|(_, change)| *change);
        let committed = scope.commit().unwrap();
        assert_eq!(committed.changed, real, "case {case}");
        let journal = committed.journal.unwrap();
        if real {
            assert_eq!(writer.col().unwrap().modified, now, "case {case}: col.mod");
        } else {
            assert_eq!(writer.col().unwrap().modified, col_mod, "case {case}: col.mod moved");
            assert!(journal.is_empty(), "case {case}");
        }
        for (((target, change), written), was) in planned.iter().zip(&outcomes).zip(&before) {
            let after = stamps(writer, *target, &ids);
            if !*change {
                // Rewriting the row as stored is never a write, also after a
                // real change of it in the same operation.
                assert_eq!(*written, Written::Unchanged, "case {case}: {target:?}");
            }
            let changed_in_op = planned.iter().any(|(t, c)| *c && same_target(*t, *target));
            if !changed_in_op {
                assert_eq!(after, *was, "case {case}: {target:?} stamped by an identical rewrite");
            }
            if written.changed() {
                let mtime = if matches!(target, Target::Tag(_)) {
                    0
                } else {
                    now.whole_secs().0
                };
                assert_eq!(after, (-1, mtime), "case {case}: {target:?}");
            }
        }
    }
}

fn same_target(a: Target, b: Target) -> bool {
    std::mem::discriminant(&a) == std::mem::discriminant(&b)
        && match (a, b) {
            (Target::Note(x), Target::Note(y))
            | (Target::Card(x), Target::Card(y))
            | (Target::Deck(x), Target::Deck(y))
            | (Target::Config(x), Target::Config(y))
            | (Target::Tag(x), Target::Tag(y)) => x == y,
            _ => false,
        }
}

fn clock_now(clock: &StepClock) -> i64 {
    meguri_model::clock::Clock::now_ms(clock)
}

#[test]
fn answer_field_merge_saves_membership_and_preserves_later_identity_ordinal_and_flag_edits() {
    use meguri_model::ids::{DeckId, NoteId};
    use meguri_storage::Record;

    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    let mut original = card(NoteId(10), 0);
    original.id = meguri_model::ids::CardId(10);
    original.deck_id = DeckId(20);
    original.original_deck_id = DeckId(1);
    original.original_due = 105;
    original.due = -100_000;
    scope.put(&original).unwrap();
    scope.commit().unwrap();
    // Capture the answer's input before another saved operation changes fields
    // the answer does not own. Use actual committed rows, not a fabricated
    // expected cell list shared with the production merge implementation.
    let mut answer = writer.card(original.id).unwrap().unwrap();
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    let mut edited = scope.card(original.id).unwrap().unwrap();
    edited.note_id = NoteId(11);
    edited.ord = 2;
    edited.flags = 0xb5;
    scope.put(&edited).unwrap();
    scope.commit().unwrap();
    let edited = writer.row_image(Table::Cards, &RowKey::Id(original.id.0)).unwrap().unwrap();
    answer.deck_id = DeckId(1);
    answer.original_deck_id = DeckId(0);
    answer.original_due = 0;
    answer.card_type = CardType::Review;
    answer.queue = CardQueue::Review;
    answer.interval = 1;
    answer.due = 101;
    answer.reps += 1;
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    scope.put_answer_card(&answer).unwrap();
    scope.commit().unwrap();
    let saved = writer.row_image(Table::Cards, &RowKey::Id(original.id.0)).unwrap().unwrap();
    for index in [0, 1, 3, 16] {
        assert_eq!(saved[index], edited[index], "protected column {index}");
    }
    let saved_card = writer.card(original.id).unwrap().unwrap();
    assert_eq!((saved_card.deck_id, saved_card.original_deck_id, saved_card.original_due, saved_card.due),
        (DeckId(1), DeckId(0), 0, 101));
    let mut expected = answer.to_cells();
    for index in [0, 1, 3, 16] { expected[index] = edited[index].clone(); }
    assert_eq!(saved, expected);
    collection.close().unwrap();
}
