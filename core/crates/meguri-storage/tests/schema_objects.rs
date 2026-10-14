//! The expected-object check and the schema guard (`storage/schema-18` S6,
//! R2016): after open, ten open/close cycles, an upgrade and a 1,000-write
//! run, `sqlite_master` holds exactly the objects Anki's files hold; foreign
//! objects are kept and never refuse a file; a missing required table does.

mod common;

use common::{anki_made_objects, copy_fixture, exec, listing, object_set, open};
use meguri_storage::OpenError;
use meguri_storage::raw::SqlValue;

#[test]
fn an_anki_made_file_holds_exactly_the_expected_objects_after_ten_open_close_cycles() {
    let (dir, path) = copy_fixture("schema-18.anki2");
    for cycle in 0..10 {
        let opened = open(&path).unwrap();
        assert_eq!(object_set(opened.writer()), anki_made_objects(), "cycle {cycle}");
        let found = opened.expected_objects();
        assert!(
            found.missing_tables.is_empty() && found.missing_indexes.is_empty(),
            "{found:?}"
        );
        assert!(found.foreign.is_empty(), "{found:?}");
        opened.close().unwrap();
        assert_eq!(
            listing(dir.path()).into_iter().collect::<Vec<_>>(),
            ["collection.anki2"],
            "cycle {cycle}: -wal or -shm left"
        );
    }
}

#[test]
fn every_upgrade_ends_with_exactly_the_expected_objects() {
    for version in [11, 14, 15, 16, 17] {
        let (_dir, path) = copy_fixture(&format!("schema-{version}.anki2"));
        let opened = open(&path).unwrap();
        assert_eq!(object_set(opened.writer()), anki_made_objects(), "from {version}");
        opened.close().unwrap();
        let reopened = open(&path).unwrap();
        assert_eq!(
            object_set(reopened.writer()),
            anki_made_objects(),
            "from {version}, reopened"
        );
        reopened.close().unwrap();
    }
}

#[test]
fn a_thousand_writes_leave_the_objects_unchanged() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let writer = opened.writer();
    let before = object_set(writer);
    // Every definition too: a column added to a table changes no name.
    let definitions = sorted_objects(writer);
    let deck = SqlValue::Integer(1);
    for i in 0..1000_i64 {
        let sql = match i % 5 {
            0 => "INSERT INTO graves (oid, type, usn) VALUES (?1, 0, -1)",
            1 => "UPDATE cards SET mod = ?1, usn = -1 WHERE id = (SELECT min(id) FROM cards)",
            2 => "INSERT OR REPLACE INTO config (KEY, usn, mtime_secs, val) VALUES ('meguriTest', -1, ?1, X'31')",
            3 => "INSERT INTO tags (tag, usn, collapsed) VALUES ('t' || ?1, -1, 0)",
            _ => "DELETE FROM graves WHERE oid = ?1 - 4",
        };
        writer.execute_raw(sql, &[SqlValue::Integer(i)]).unwrap();
        if i % 100 == 99 {
            assert!(writer.checkpoint().unwrap().succeeded());
        }
    }
    writer
        .execute_raw("UPDATE decks SET mtime_secs = 1 WHERE id = ?1", &[deck])
        .unwrap();
    assert_eq!(object_set(writer), before);
    assert_eq!(sorted_objects(writer), definitions);
    opened.close().unwrap();
    let reopened = open(&path).unwrap();
    assert_eq!(object_set(reopened.writer()), before);
    assert_eq!(sorted_objects(reopened.writer()), definitions);
    reopened.close().unwrap();
}

fn sorted_objects(writer: &meguri_storage::Writer) -> Vec<meguri_storage::SchemaObject> {
    let mut objects = writer.schema_objects().unwrap();
    objects.sort();
    objects
}

#[test]
fn foreign_objects_and_missing_indexes_are_reported_and_left_alone() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    for sql in [
        "CREATE TABLE addon_data (k text, v text)",
        "CREATE VIEW addon_view AS SELECT id FROM notes",
        "CREATE TRIGGER addon_trigger AFTER DELETE ON notes BEGIN DELETE FROM addon_data; END",
        "DROP INDEX ix_cards_sched",
        "CREATE INDEX ix_cards_sched ON notes (mid)",
    ] {
        exec(opened.writer(), sql);
    }
    opened.close().unwrap();

    let reopened = open(&path).unwrap();
    let found = reopened.expected_objects();
    assert!(found.missing_tables.is_empty());
    // An index of that name on another table is not the required one.
    assert_eq!(found.missing_indexes, ["ix_cards_sched"]);
    let mut foreign: Vec<(&str, &str)> = found
        .foreign
        .iter()
        .map(|o| (o.kind.as_str(), o.name.as_str()))
        .collect();
    foreign.sort();
    assert_eq!(
        foreign,
        [
            ("index", "ix_cards_sched"),
            ("table", "addon_data"),
            ("trigger", "addon_trigger"),
            ("view", "addon_view")
        ]
    );
    let objects = object_set(reopened.writer());
    reopened.close().unwrap();
    // Opening neither recreates the index nor drops anything.
    let again = open(&path).unwrap();
    assert_eq!(object_set(again.writer()), objects);
    again.close().unwrap();
}

#[test]
fn a_missing_required_table_refuses_the_file() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    exec(opened.writer(), "DROP TABLE templates");
    opened.close().unwrap();
    assert_eq!(
        open(&path).unwrap_err(),
        OpenError::NotACollection {
            missing_tables: vec!["templates".to_owned()]
        }
    );
}
