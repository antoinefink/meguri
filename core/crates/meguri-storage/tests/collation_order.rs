//! The `unicase` collation through the indexes (`storage/schema-18` S3, S4;
//! R1451): order, and plain inserts of a name equal under folding to a
//! stored one failing with SQLite's constraint error and storing nothing.
//! The scenario `storage/open/collation` shows the same with `INSERT OR
//! IGNORE` against the oracle.

mod common;

use common::{copy_fixture, exec, int, open, query};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReaderRole, StorageError, Writer};

/// SQLite's extended codes for a unique index and a primary key.
const CONSTRAINT_UNIQUE: i32 = 2067;
const CONSTRAINT_PRIMARYKEY: i32 = 1555;

const ORDER: &str = "SELECT column1 FROM (VALUES ('Straße'), ('STRASSE'), ('ä'), ('Ä'), ('Ω'), ('ω'), ('b'), ('Z'), ('_x')) \
                     ORDER BY column1 COLLATE unicase, column1";

fn texts(rows: &meguri_storage::raw::RawRows) -> Vec<String> {
    rows.rows
        .iter()
        .map(|row| match &row[0] {
            SqlValue::Text(t) => t.clone(),
            other => panic!("{other:?}"),
        })
        .collect()
}

#[test]
fn names_sort_through_the_collation_on_the_writer_and_every_reader() {
    // S4's order, ties broken by code point.
    let expected = ["_x", "b", "STRASSE", "Straße", "Z", "Ä", "ä", "Ω", "ω"];
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    assert_eq!(texts(&exec(opened.writer(), ORDER)), expected);
    for role in ReaderRole::ALL {
        let reader = opened.open_reader(role).unwrap();
        assert_eq!(texts(&query(&reader, ORDER)), expected, "{role:?}");
    }
    opened.close().unwrap();
}

fn assert_refused(writer: &Writer, sql: &str, code: i32) {
    match writer.execute_raw(sql, &[]) {
        Err(StorageError::Sqlite { extended_code, .. }) => assert_eq!(extended_code, code, "{sql}"),
        other => panic!("{sql}: {other:?}"),
    }
}

#[test]
fn a_name_equal_under_folding_to_a_stored_one_is_refused_and_nothing_is_stored() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    let opened = open(&path).unwrap();
    let w = opened.writer();
    let count = |sql: &str| int(&exec(w, sql));

    // Decks.
    exec(
        w,
        "INSERT INTO decks SELECT 4000000001, 'Straße', 0, 0, common, kind FROM decks WHERE id = 1",
    );
    let decks = count("SELECT count(*) FROM decks");
    for name in ["STRASSE", "strasse", "STRAẞE"] {
        assert_refused(
            w,
            &format!("INSERT INTO decks SELECT 4000000002, '{name}', 0, 0, common, kind FROM decks WHERE id = 1"),
            CONSTRAINT_UNIQUE,
        );
    }
    assert_eq!(count("SELECT count(*) FROM decks"), decks);

    // Note types, and fields and templates within one note type.
    let ntid = count("SELECT min(id) FROM notetypes");
    let name = match &exec(w, &format!("SELECT name FROM notetypes WHERE id = {ntid}")).rows[0][0] {
        SqlValue::Text(t) => t.to_uppercase(),
        other => panic!("{other:?}"),
    };
    assert_refused(
        w,
        &format!("INSERT INTO notetypes SELECT 4000000003, '{name}', 0, 0, config FROM notetypes WHERE id = {ntid}"),
        CONSTRAINT_UNIQUE,
    );
    let field = match &exec(w, &format!("SELECT name FROM fields WHERE ntid = {ntid} AND ord = 0")).rows[0][0] {
        SqlValue::Text(t) => t.to_lowercase(),
        other => panic!("{other:?}"),
    };
    assert_refused(
        w,
        &format!("INSERT INTO fields SELECT ntid, 90, '{field}', config FROM fields WHERE ntid = {ntid} AND ord = 0"),
        CONSTRAINT_UNIQUE,
    );
    let template = match &exec(
        w,
        &format!("SELECT name FROM templates WHERE ntid = {ntid} AND ord = 0"),
    )
    .rows[0][0]
    {
        SqlValue::Text(t) => t.to_uppercase(),
        other => panic!("{other:?}"),
    };
    assert_refused(
        w,
        &format!(
            "INSERT INTO templates SELECT ntid, 90, '{template}', 0, 0, config FROM templates WHERE ntid = {ntid} AND ord = 0"
        ),
        CONSTRAINT_UNIQUE,
    );
    // The same field name in another note type is fine.
    exec(
        w,
        &format!(
            "INSERT INTO fields SELECT 4000000004, 0, '{field}', config FROM fields WHERE ntid = {ntid} AND ord = 0"
        ),
    );

    // Tags (the primary key).
    exec(w, "INSERT INTO tags (tag, usn, collapsed) VALUES ('Ä', 0, 0)");
    assert_refused(
        w,
        "INSERT INTO tags (tag, usn, collapsed) VALUES ('ä', 0, 0)",
        CONSTRAINT_PRIMARYKEY,
    );
    assert_eq!(
        count("SELECT count(*) FROM tags WHERE tag = 'ä'"),
        1,
        "found through the collation"
    );
    assert_eq!(count("SELECT count(*) FROM tags WHERE tag = 'ä' COLLATE BINARY"), 0);

    // Presets have no unique index on their name.
    exec(
        w,
        "INSERT INTO deck_config SELECT 4000000005, upper(name), 0, 0, config FROM deck_config WHERE id = 1",
    );
    assert_eq!(count("SELECT count(*) FROM deck_config WHERE name = 'default'"), 2);

    assert_eq!(
        count("SELECT count(*) FROM pragma_integrity_check WHERE integrity_check = 'ok'"),
        1
    );
    opened.close().unwrap();
}
