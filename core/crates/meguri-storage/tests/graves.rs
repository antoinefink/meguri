//! Graves (`storage/graves` G2, G3; R1473): deleting a note graves each of
//! its cards and the note, a grave that exists is kept as it is, and a
//! repeated deletion adds nothing. The scenario replay compares stored rows
//! with the oracle's golden of `storage/graves/delete-note`.

mod common;

use common::{NOW_MS, card, empty_collection, exec, expected_steps, note, open};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::grave::GraveKind;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{JournalOptions, ReadRows, StampMode, Table, Written};
use serde_json::{Value, json};

const HOUR_MS: i64 = 3_600_000;

fn rows(writer: &meguri_storage::Writer, sql: &str) -> Value {
    Value::Array(
        exec(writer, sql)
            .rows
            .iter()
            .map(|row| {
                Value::Array(
                    row.iter()
                        .map(|v| match v {
                            SqlValue::Integer(i) => json!(i),
                            other => panic!("{other:?}"),
                        })
                        .collect(),
                )
            })
            .collect(),
    )
}

#[test]
fn deleting_a_note_with_three_cards_adds_four_graves_once() {
    let steps = expected_steps("storage/graves/delete-note");
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope.add_note(&note(1, &["river", "rio"])).unwrap();
    let cards: Vec<_> = (0..3).map(|ord| scope.add_card(&card(id, ord)).unwrap()).collect();
    scope.commit().unwrap();
    let ids = json!({"#c1": cards[0].0, "#c2": cards[1].0, "#c3": cards[2].0, "#n1": id.0});
    assert_eq!(ids, steps[0]["ids"]);
    exec(
        writer,
        &format!("INSERT INTO graves (oid, type, usn) VALUES ({}, 0, 5)", cards[1].0),
    );

    let graves = "SELECT oid, type, usn FROM graves ORDER BY type, oid";
    clock.step_ms(HOUR_MS);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.remove_note_and_cards(id).unwrap(), Written::Removed);
    let journal = scope.commit().unwrap().journal.unwrap();
    assert_eq!(rows(writer, graves), steps[4]["db_exec"]["rows"]);
    assert_eq!(rows(writer, "SELECT mod FROM col"), steps[5]["db_exec"]["rows"]);
    // The existing grave was no change: three card graves and a note grave
    // added, three cards and a note removed, and col.mod.
    let added: Vec<_> = journal
        .entries()
        .filter(|e| e.table() == Table::Graves)
        .map(|e| (e.key().clone(), e.existed_before()))
        .collect();
    assert_eq!(added.len(), 3, "{added:?}");
    assert!(added.iter().all(|(_, existed)| !existed));
    assert_eq!(journal.len(), 3 + 3 + 1 + 1);

    clock.step_ms(HOUR_MS);
    let mut scope = writer
        .begin_write(StampMode::Local, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    assert_eq!(scope.remove_note_and_cards(id).unwrap(), Written::Unchanged);
    let committed = scope.commit().unwrap();
    assert!(!committed.changed && committed.journal.unwrap().is_empty());
    assert_eq!(rows(writer, graves), steps[8]["db_exec"]["rows"]);
    assert_eq!(rows(writer, "SELECT mod FROM col"), steps[9]["db_exec"]["rows"]);
    assert!(cards.iter().all(|c| writer.card(*c).unwrap().is_none()));
}

#[test]
fn a_grave_insert_keeps_an_existing_grave_and_takes_the_usn_its_mode_gives() {
    let (_dir, path) = empty_collection();
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = StepClock::new(NOW_MS, UtcOffset::UTC, "UTC");

    // A sync's incoming deletion keeps the server's usn (G4).
    let mut scope = writer.begin_write(StampMode::CallerSupplied, &clock, None).unwrap();
    assert_eq!(
        scope.add_grave(77, GraveKind::Deck, Usn(12)).unwrap(),
        Written::Inserted
    );
    scope.commit().unwrap();
    assert_eq!(writer.grave(77, GraveKind::Deck).unwrap().unwrap().usn, Usn(12));

    // A local one stamps -1, and inserting it again is no change (G2).
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    assert_eq!(scope.add_grave(78, GraveKind::Note, Usn(3)).unwrap(), Written::Inserted);
    assert_eq!(
        scope.add_grave(77, GraveKind::Deck, Usn::PENDING).unwrap(),
        Written::Unchanged
    );
    scope.commit().unwrap();
    assert_eq!(writer.grave(78, GraveKind::Note).unwrap().unwrap().usn, Usn::PENDING);
    assert_eq!(writer.grave(77, GraveKind::Deck).unwrap().unwrap().usn, Usn(12));

    // An existing grave alone is no change: col.mod stays.
    let col_mod = writer.col().unwrap().modified;
    clock.step_ms(HOUR_MS);
    let mut scope = writer.begin_write(StampMode::Local, &clock, None).unwrap();
    assert_eq!(
        scope.add_grave(78, GraveKind::Note, Usn::PENDING).unwrap(),
        Written::Unchanged
    );
    assert!(!scope.commit().unwrap().changed);
    assert_eq!(writer.col().unwrap().modified, col_mod);
}
