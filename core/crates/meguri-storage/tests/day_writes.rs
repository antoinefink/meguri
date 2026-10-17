//! The day clock's raw writes (`storage/ids-usn-mtime` S3, `sched/day-clock`
//! D5, D7): a config key stamped as a local change without moving
//! `col.mod`, and a card's queue changed with its `mod` and `usn` kept.

mod common;

use common::{NOW_MS, copy_fixture, exec, open};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::CardId;
use meguri_storage::raw::SqlValue;
use meguri_storage::{CardQueueRow, ConfigValue, JournalOptions, ReadRows, StampMode, StorageError, Written};

fn clock() -> StepClock {
    StepClock::new(NOW_MS, UtcOffset::UTC, "UTC")
}

fn config_row(writer: &meguri_storage::Writer, key: &str) -> Vec<SqlValue> {
    let rows = writer
        .execute_raw(
            "SELECT usn, mtime_secs, CAST(val AS TEXT) FROM config WHERE KEY = ?1",
            &[SqlValue::Text(key.to_owned())],
        )
        .unwrap();
    rows.rows.into_iter().next().unwrap_or_default()
}

fn col_mod(writer: &meguri_storage::Writer) -> SqlValue {
    exec(writer, "SELECT mod FROM col").rows[0][0].clone()
}

#[test]
fn a_day_clock_config_write_stamps_the_row_and_leaves_col_mod() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    let before = col_mod(writer);
    let now_secs = NOW_MS / 1000;

    let mut scope = writer
        .begin_write(StampMode::Raw, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    // A new key, a changed key and a key that already holds the value.
    assert_eq!(
        scope
            .set_config_keeping_col_mod("lastUnburied", &ConfigValue::from(6_i64))
            .unwrap(),
        Written::Inserted
    );
    assert_eq!(
        scope
            .set_config_keeping_col_mod("rollover", &ConfigValue::from(5_i64))
            .unwrap(),
        Written::Updated
    );
    assert_eq!(
        scope
            .set_config_keeping_col_mod("collapseTime", &ConfigValue::from(1200_i64))
            .unwrap(),
        Written::Unchanged
    );
    let committed = scope.commit().unwrap();
    assert!(committed.changed && committed.col_mod.is_none());
    assert_eq!(committed.journal.unwrap().len(), 2);

    assert_eq!(
        config_row(writer, "lastUnburied"),
        [
            SqlValue::Integer(-1),
            SqlValue::Integer(now_secs),
            SqlValue::Text("6".to_owned())
        ]
    );
    assert_eq!(
        config_row(writer, "rollover"),
        [
            SqlValue::Integer(-1),
            SqlValue::Integer(now_secs),
            SqlValue::Text("5".to_owned())
        ]
    );
    // The fixture's own stamps on the key that kept its value.
    assert_eq!(
        config_row(writer, "collapseTime"),
        [
            SqlValue::Integer(0),
            SqlValue::Integer(0),
            SqlValue::Text("1200".to_owned())
        ]
    );
    assert_eq!(col_mod(writer), before);
}

#[test]
fn the_day_clock_writes_only_in_a_raw_scope() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let collection = open(&path).unwrap();
    let clock = clock();
    let mut scope = collection.writer().begin_write(StampMode::Local, &clock, None).unwrap();
    assert!(matches!(
        scope.set_config_keeping_col_mod("lastUnburied", &ConfigValue::from(1_i64)),
        Err(StorageError::Mode { .. })
    ));
    assert!(matches!(
        scope.set_card_queue(CardId(1), 0),
        Err(StorageError::Mode { .. })
    ));
}

#[test]
fn buried_cards_are_listed_and_only_their_queue_changes() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let collection = open(&path).unwrap();
    let writer = collection.writer();
    let clock = clock();
    exec(
        writer,
        "UPDATE cards SET queue = -2, mod = 7, usn = 3 WHERE id = (SELECT min(id) FROM cards)",
    );
    let all = "SELECT id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, \
               flags, data FROM cards ORDER BY id";
    let before = exec(writer, all).rows;
    let expected: Vec<CardQueueRow> = before
        .iter()
        .filter(|row| matches!(row[7], SqlValue::Integer(-2 | -3)))
        .map(|row| {
            let int = |i: usize| match row[i] {
                SqlValue::Integer(v) => v,
                ref other => panic!("{other:?}"),
            };
            CardQueueRow {
                id: CardId(int(0)),
                card_type: int(6),
                queue: int(7),
                due: int(8),
            }
        })
        .collect();
    assert!(expected.len() >= 2, "the fixture has a user-buried card besides ours");
    assert_eq!(writer.cards_in_queues(&[-2, -3]).unwrap(), expected);

    let mut scope = writer
        .begin_write(StampMode::Raw, &clock, Some(JournalOptions::in_memory()))
        .unwrap();
    for card in &expected {
        assert_eq!(scope.set_card_queue(card.id, 0).unwrap(), Written::Updated);
        assert_eq!(scope.set_card_queue(card.id, 0).unwrap(), Written::Unchanged);
    }
    assert_eq!(scope.set_card_queue(CardId(424_242), 0).unwrap(), Written::Unchanged);
    let committed = scope.commit().unwrap();
    assert!(committed.col_mod.is_none());
    assert_eq!(committed.journal.unwrap().len(), expected.len());

    let after = exec(writer, all).rows;
    for (old, new) in before.iter().zip(&after) {
        let mut wanted = old.clone();
        if expected.iter().any(|c| SqlValue::Integer(c.id.0) == old[0]) {
            wanted[7] = SqlValue::Integer(0);
        }
        assert_eq!(*new, wanted);
    }
    assert!(writer.cards_in_queues(&[-2, -3]).unwrap().is_empty());
}
