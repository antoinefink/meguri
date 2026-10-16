//! The derived store's file, stamp, components, staging swap and reader
//! attachment (`specs/storage/derived-store.md` D1 to D5, D7, D9). The engine
//! side (threads, observer, readiness, generations) is tested in
//! `meguri-engine/tests/derived_lag.rs`.

mod common;

use std::fs;
use std::path::Path;

use common::{copy_fixture, exec, object_set, open, query};
use meguri_storage::derived::{
    self, CollectionMark, ComponentRecord, ComponentSchema, DerivedFile, Found, IndexDef, RebuildReason, Registry,
    Stamp, TableDef, Target, Verdict, assess,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::rusqlite::{self, Connection};
use meguri_storage::{ReaderRole, StorageError};

/// A component with one keyed table and one index.
fn mirror(version: u32) -> ComponentSchema {
    ComponentSchema::new("mirror", version)
        .table(TableDef::columns("rows", "(k INTEGER PRIMARY KEY, v TEXT NOT NULL)"))
        .index(IndexDef::new("rows_v", "rows", "(v)"))
}

fn other() -> ComponentSchema {
    ComponentSchema::new("other", 1).table(TableDef::columns("items", "(id INTEGER PRIMARY KEY, n INTEGER)"))
}

fn registry(schemas: Vec<ComponentSchema>) -> Registry {
    Registry::new(schemas).unwrap()
}

/// `(k, v)` rows of `mirror__rows` through any connection.
fn rows(conn: &Connection, table: &str) -> Vec<(i64, String)> {
    let mut statement = conn.prepare(&format!("SELECT k, v FROM {table} ORDER BY k")).unwrap();
    statement
        .query_map([], |row| Ok((row.get(0)?, row.get(1)?)))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn names(conn: &Connection, kind: &str) -> Vec<String> {
    let mut statement = conn
        .prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")
        .unwrap();
    statement
        .query_map([kind], |row| row.get(0))
        .unwrap()
        .collect::<rusqlite::Result<_>>()
        .unwrap()
}

fn pragma(conn: &Connection, name: &str) -> i64 {
    conn.query_row(&format!("PRAGMA {name}"), [], |row| row.get(0)).unwrap()
}

fn mark(crt: i64, scm: i64, col_mod: i64) -> CollectionMark {
    CollectionMark { crt, scm, col_mod }
}

/// Opens the store at `path`, registers `schemas`, fills `mirror` with
/// `values` (live, ready) and stamps it clean at `at`; returns the file.
fn filled(path: &Path, values: &[(i64, &str)], at: CollectionMark) -> DerivedFile {
    let (mut file, _) = DerivedFile::open(path).unwrap();
    let registry = registry(vec![mirror(1)]);
    let tx = file.begin().unwrap();
    tx.sync_components(&registry).unwrap();
    let schema = registry.get("mirror").unwrap();
    {
        let tables = tx.tables(schema, Target::Live);
        let table = tables.table("rows").unwrap().to_owned();
        for (k, v) in values {
            tables
                .conn()
                .execute(&format!("INSERT INTO \"{table}\" (k, v) VALUES (?1, ?2)"), (k, v))
                .unwrap();
        }
    }
    tx.set_ready("mirror", true).unwrap();
    tx.set_stamp(&Stamp { mark: at, clean: true }).unwrap();
    tx.commit().unwrap();
    file
}

#[test]
fn the_file_sits_next_to_the_collection() {
    assert_eq!(
        derived::derived_path(Path::new("/a/b/collection.anki2")),
        Path::new("/a/b/meguri-derived.db")
    );
    assert_eq!(derived::FILE_NAME, "meguri-derived.db");
}

#[test]
fn a_missing_file_is_created_with_the_format_and_no_stamp_and_stray_sidecars_go() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    fs::write(dir.path().join("meguri-derived.db-wal"), b"stray wal bytes").unwrap();
    fs::write(dir.path().join("meguri-derived.db-shm"), b"stray shm bytes").unwrap();
    let (file, found) = DerivedFile::open(&path).unwrap();
    assert_eq!(found, Found::Created);
    assert_eq!(file.stamp().unwrap(), None);
    assert!(file.components().unwrap().is_empty());
    file.close().unwrap();

    let conn = Connection::open(&path).unwrap();
    assert_eq!(pragma(&conn, "application_id"), 0x4D47_5244);
    assert_eq!(pragma(&conn, "user_version"), 1);
    let mode: String = conn.query_row("PRAGMA journal_mode", [], |row| row.get(0)).unwrap();
    assert_eq!(mode, "wal");
    assert_eq!(names(&conn, "table"), ["meguri_components", "meguri_stamp"]);
}

#[test]
fn a_good_file_is_opened_with_its_stamp_rows_and_content() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let at = mark(1_600_000_000, 1_700_000_000_000, 1_800_000_000_000);
    filled(&path, &[(1, "a"), (2, "b")], at).close().unwrap();
    assert!(!dir.path().join("meguri-derived.db-wal").exists(), "close checkpoints");

    let (file, found) = DerivedFile::open(&path).unwrap();
    assert_eq!(found, Found::Opened);
    assert_eq!(file.stamp().unwrap(), Some(Stamp { mark: at, clean: true }));
    let records = file.components().unwrap();
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].name, "mirror");
    assert_eq!(records[0].version, 1);
    assert!(records[0].ready);
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(rows(&conn, "mirror__rows"), [(1, "a".to_owned()), (2, "b".to_owned())]);
}

#[test]
fn a_file_truncated_mid_page_is_recreated_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let long = "x".repeat(600);
    let values: Vec<(i64, String)> = (0..400).map(|k| (k, format!("{k}{long}"))).collect();
    let borrowed: Vec<(i64, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    filled(&path, &borrowed, mark(1, 2, 3)).close().unwrap();
    let size = fs::metadata(&path).unwrap().len();
    assert!(size > 40 * 4096, "{size} bytes: enough pages to cut");
    // Half the pages and part of the next one.
    let cut = (size / 4096 / 2) * 4096 + 1000;
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .set_len(cut)
        .unwrap();

    let (file, found) = DerivedFile::open(&path).unwrap();
    assert!(matches!(found, Found::Recreated { .. }), "{found:?}");
    assert_eq!(file.stamp().unwrap(), None);
    assert!(file.components().unwrap().is_empty());
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(names(&conn, "table"), ["meguri_components", "meguri_stamp"]);
}

#[test]
fn a_file_with_a_damaged_page_is_recreated_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let long = "y".repeat(600);
    let values: Vec<(i64, String)> = (0..400).map(|k| (k, format!("{k}{long}"))).collect();
    let borrowed: Vec<(i64, &str)> = values.iter().map(|(k, v)| (*k, v.as_str())).collect();
    filled(&path, &borrowed, mark(1, 2, 3)).close().unwrap();
    // The file keeps its length; one page in the middle is overwritten.
    let mut bytes = fs::read(&path).unwrap();
    let pages = bytes.len() / 4096;
    let page = pages / 2;
    bytes[page * 4096..(page + 1) * 4096].fill(0xFF);
    fs::write(&path, &bytes).unwrap();

    let (file, found) = DerivedFile::open(&path).unwrap();
    assert!(matches!(found, Found::Recreated { .. }), "{found:?}");
    assert_eq!(file.stamp().unwrap(), None);
    file.close().unwrap();
}

#[test]
fn garbage_a_foreign_database_and_another_format_are_recreated() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);

    fs::write(&path, vec![0xA5_u8; 3 * 4096 + 17]).unwrap();
    let (file, found) = DerivedFile::open(&path).unwrap();
    assert!(matches!(found, Found::Recreated { .. }), "garbage: {found:?}");
    file.close().unwrap();

    fs::remove_file(&path).unwrap();
    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("CREATE TABLE someone_elses (x); INSERT INTO someone_elses VALUES (1);")
            .unwrap();
    }
    let (file, found) = DerivedFile::open(&path).unwrap();
    assert!(matches!(found, Found::Recreated { .. }), "foreign: {found:?}");
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert!(!names(&conn, "table").contains(&"someone_elses".to_owned()));
    drop(conn);

    {
        let conn = Connection::open(&path).unwrap();
        conn.execute_batch("PRAGMA user_version = 2").unwrap();
    }
    let (file, found) = DerivedFile::open(&path).unwrap();
    assert!(matches!(found, Found::Recreated { .. }), "other format: {found:?}");
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(pragma(&conn, "user_version"), 1);
}

#[test]
fn the_verdicts_follow_d5_2() {
    let ready = ComponentRecord {
        name: "mirror".to_owned(),
        version: 1,
        ready: true,
        build: 1,
    };
    let not_ready = ComponentRecord {
        ready: false,
        ..ready.clone()
    };
    let now = mark(100, 2_000, 9_000);
    let clean = |m| Stamp { mark: m, clean: true };
    let unclean = |m| Stamp { mark: m, clean: false };

    // A reset (new or changed component) and a missing stamp rebuild.
    assert_eq!(
        assess(
            Some(&clean(now)),
            &ready,
            Some(RebuildReason::VersionChanged { from: 0 }),
            &now
        ),
        Verdict::Rebuild(RebuildReason::VersionChanged { from: 0 })
    );
    assert_eq!(
        assess(None, &ready, None, &now),
        Verdict::Rebuild(RebuildReason::NoStamp)
    );
    assert_eq!(
        assess(Some(&clean(now)), &not_ready, None, &now),
        Verdict::Rebuild(RebuildReason::NotReady)
    );
    // Another collection.
    assert_eq!(
        assess(Some(&clean(mark(101, 2_000, 9_000))), &ready, None, &now),
        Verdict::Rebuild(RebuildReason::OtherCollection)
    );
    assert_eq!(
        assess(Some(&unclean(mark(100, 2_001, 9_000))), &ready, None, &now),
        Verdict::Rebuild(RebuildReason::OtherCollection)
    );
    // The collection went back (a restored file), clean or not.
    for stamp in [clean(mark(100, 2_000, 9_001)), unclean(mark(100, 2_000, 9_001))] {
        assert_eq!(
            assess(Some(&stamp), &ready, None, &now),
            Verdict::Rebuild(RebuildReason::CollectionOlder)
        );
    }
    // Changed while Meguri did not have it open.
    assert_eq!(
        assess(Some(&clean(mark(100, 2_000, 8_000))), &ready, None, &now),
        Verdict::Rebuild(RebuildReason::ExternalChange)
    );
    // A crash: catch up, with or without a lag in col.mod.
    assert_eq!(
        assess(Some(&unclean(mark(100, 2_000, 8_000))), &ready, None, &now),
        Verdict::CatchUp { since: 8_000 }
    );
    assert_eq!(
        assess(Some(&unclean(now)), &ready, None, &now),
        Verdict::CatchUp { since: 9_000 }
    );
    assert_eq!(assess(Some(&clean(now)), &ready, None, &now), Verdict::Fresh);
}

#[test]
fn syncing_components_creates_resets_and_drops_their_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let at = mark(1, 2, 3);
    let mut file = filled(&path, &[(1, "a")], at);
    {
        let tx = file.begin().unwrap();
        let synced = tx.sync_components(&registry(vec![mirror(1), other()])).unwrap();
        assert_eq!(synced.len(), 2);
        assert_eq!(synced[0].record.name, "mirror");
        assert_eq!(synced[0].reset, None, "unchanged: kept");
        assert!(synced[0].record.ready);
        assert_eq!(synced[1].record.name, "other");
        assert_eq!(synced[1].reset, Some(RebuildReason::NewComponent));
        assert!(!synced[1].record.ready);
        // A staging table left by an interrupted rebuild.
        tx.begin_staging(registry(vec![other()]).get("other").unwrap()).unwrap();
        tx.commit().unwrap();
    }
    file.close().unwrap();

    let (mut file, _) = DerivedFile::open(&path).unwrap();
    {
        let tx = file.begin().unwrap();
        let synced = tx.sync_components(&registry(vec![mirror(2)])).unwrap();
        assert_eq!(synced.len(), 1);
        assert_eq!(synced[0].reset, Some(RebuildReason::VersionChanged { from: 1 }));
        assert!(!synced[0].record.ready);
        assert_eq!(synced[0].record.version, 2);
        tx.commit().unwrap();
    }
    let records = file.components().unwrap();
    assert_eq!(
        records.iter().map(|r| r.name.as_str()).collect::<Vec<_>>(),
        ["mirror"],
        "other's row is gone"
    );
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    let tables = names(&conn, "table");
    assert!(tables.contains(&"mirror__rows".to_owned()), "{tables:?}");
    assert!(
        !tables
            .iter()
            .any(|t| t.starts_with("other__") || t.starts_with("meguri_build__")),
        "{tables:?}"
    );
    assert!(rows(&conn, "mirror__rows").is_empty(), "a version change empties it");
    // A table dropped behind the store's back resets the component.
    conn.execute_batch("DROP TABLE mirror__rows").unwrap();
    drop(conn);
    let (mut file, _) = DerivedFile::open(&path).unwrap();
    let tx = file.begin().unwrap();
    let synced = tx.sync_components(&registry(vec![mirror(2)])).unwrap();
    assert_eq!(synced[0].reset, Some(RebuildReason::MissingTable));
    tx.commit().unwrap();
    file.close().unwrap();
}

#[test]
fn component_and_table_names_are_checked() {
    for bad in ["", "Mirror", "1x", "a__b", "meguri_x", "with space", &"x".repeat(41)] {
        let error = Registry::new(vec![ComponentSchema::new(bad, 1).table(TableDef::columns("t", "(a)"))]);
        assert!(matches!(error, Err(StorageError::Derived { .. })), "{bad:?}: {error:?}");
    }
    let duplicate = Registry::new(vec![mirror(1), mirror(2)]);
    assert!(matches!(duplicate, Err(StorageError::Derived { .. })));
    let bad_table = Registry::new(vec![
        ComponentSchema::new("ok", 1).table(TableDef::columns("A b", "(a)")),
    ]);
    assert!(matches!(bad_table, Err(StorageError::Derived { .. })));
    let no_table = Registry::new(vec![
        ComponentSchema::new("ok", 1)
            .table(TableDef::columns("t", "(a)"))
            .index(IndexDef::new("i", "missing", "(a)")),
    ]);
    assert!(matches!(no_table, Err(StorageError::Derived { .. })));
    let zero = Registry::new(vec![ComponentSchema::new("ok", 0).table(TableDef::columns("t", "(a)"))]);
    assert!(matches!(zero, Err(StorageError::Derived { .. })));
}

#[test]
fn a_rebuild_swaps_in_one_transaction_and_a_reader_sees_old_or_new_never_part() {
    let (_dir, collection) = copy_fixture("schema-18.anki2");
    let opened = open(&collection).unwrap();
    let path = derived::derived_path(&collection);
    let mut file = filled(&path, &[(1, "old"), (2, "old")], mark(1, 2, 3));
    let registry = registry(vec![mirror(1)]);
    let schema = registry.get("mirror").unwrap();
    let mut reader = opened.open_reader(ReaderRole::Query).unwrap();
    reader.attach_derived(&path, 1).unwrap();

    let before_build = |file: &DerivedFile| {
        file.components()
            .unwrap()
            .into_iter()
            .find(|r| r.name == "mirror")
            .unwrap()
            .build
    };
    let old_build = before_build(&file);
    {
        let tx = file.begin().unwrap();
        tx.begin_staging(schema).unwrap();
        tx.commit().unwrap();
    }
    // Three batches, each its own transaction.
    for batch in 0..3_i64 {
        let tx = file.begin().unwrap();
        {
            let tables = tx.tables(schema, Target::Staging);
            let table = tables.table("rows").unwrap().to_owned();
            assert_ne!(table, "mirror__rows");
            for k in batch * 10..batch * 10 + 10 {
                tables
                    .conn()
                    .execute(&format!("INSERT INTO \"{table}\" (k, v) VALUES (?1, 'new')"), [k])
                    .unwrap();
            }
        }
        tx.commit().unwrap();
        let seen = query(&reader, "SELECT count(*), min(v) FROM derived.mirror__rows");
        assert_eq!(
            seen.rows[0],
            [SqlValue::Integer(2), SqlValue::Text("old".into())],
            "batch {batch}"
        );
    }

    // A read transaction begun before the swap keeps the old content.
    let snapshot = reader.begin_snapshot().unwrap();
    assert_eq!(
        snapshot
            .execute_raw("SELECT count(*) FROM derived.mirror__rows", &[])
            .unwrap()
            .rows[0][0],
        SqlValue::Integer(2)
    );
    {
        let tx = file.begin().unwrap();
        tx.finish_staging(schema).unwrap();
        tx.commit().unwrap();
    }
    assert_eq!(
        snapshot
            .execute_raw("SELECT count(*), min(v) FROM derived.mirror__rows", &[])
            .unwrap()
            .rows[0],
        [SqlValue::Integer(2), SqlValue::Text("old".into())]
    );
    drop(snapshot);
    let seen = query(&reader, "SELECT count(*), min(v), max(v) FROM derived.mirror__rows");
    assert_eq!(
        seen.rows[0],
        [
            SqlValue::Integer(30),
            SqlValue::Text("new".into()),
            SqlValue::Text("new".into())
        ]
    );

    let record = file
        .components()
        .unwrap()
        .into_iter()
        .find(|r| r.name == "mirror")
        .unwrap();
    assert!(record.ready);
    assert_eq!(record.build, old_build + 1);
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(
        names(&conn, "index")
            .into_iter()
            .filter(|n| !n.starts_with("sqlite_"))
            .collect::<Vec<_>>(),
        [format!("mirror__rows_v__{}", old_build + 1)],
        "the new index, named by the build; the old one went with its table"
    );
    assert!(!names(&conn, "table").iter().any(|t| t.starts_with("meguri_build__")));
    drop(reader);
    opened.close().unwrap();
}

#[test]
fn a_discarded_staging_build_leaves_the_live_tables() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let mut file = filled(&path, &[(1, "live")], mark(1, 2, 3));
    let registry = registry(vec![mirror(1)]);
    let schema = registry.get("mirror").unwrap();
    let tx = file.begin().unwrap();
    tx.begin_staging(schema).unwrap();
    {
        let tables = tx.tables(schema, Target::Staging);
        let table = tables.table("rows").unwrap().to_owned();
        tables
            .conn()
            .execute(&format!("INSERT INTO \"{table}\" VALUES (9, 'staged')"), [])
            .unwrap();
    }
    tx.discard_staging(schema).unwrap();
    tx.commit().unwrap();
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(rows(&conn, "mirror__rows"), [(1, "live".to_owned())]);
    assert!(!names(&conn, "table").iter().any(|t| t.starts_with("meguri_build__")));
}

#[test]
fn a_full_text_table_survives_the_swap() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let registry = registry(vec![
        ComponentSchema::new("text", 1).table(TableDef::virtual_table("words", "fts5(body)")),
    ]);
    let schema = registry.get("text").unwrap();
    let (mut file, _) = DerivedFile::open(&path).unwrap();
    {
        let tx = file.begin().unwrap();
        tx.sync_components(&registry).unwrap();
        tx.begin_staging(schema).unwrap();
        {
            let tables = tx.tables(schema, Target::Staging);
            let table = tables.table("words").unwrap().to_owned();
            tables
                .conn()
                .execute(
                    &format!("INSERT INTO \"{table}\" (rowid, body) VALUES (7, 'quick brown fox')"),
                    [],
                )
                .unwrap();
        }
        tx.finish_staging(schema).unwrap();
        tx.commit().unwrap();
    }
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    let found: i64 = conn
        .query_row(
            "SELECT rowid FROM text__words WHERE text__words MATCH 'brown'",
            [],
            |row| row.get(0),
        )
        .unwrap();
    assert_eq!(found, 7);
}

#[test]
fn a_savepoint_rolls_back_only_its_part() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(derived::FILE_NAME);
    let mut file = filled(&path, &[], mark(1, 2, 3));
    let registry = registry(vec![mirror(1)]);
    let schema = registry.get("mirror").unwrap();
    let tx = file.begin().unwrap();
    let insert = |k: i64| {
        let tables = tx.tables(schema, Target::Live);
        tables
            .conn()
            .execute("INSERT INTO mirror__rows VALUES (?1, 'x')", [k])
            .map(|_| ())
    };
    insert(1).unwrap();
    let kept = tx.savepoint().unwrap();
    insert(2).unwrap();
    kept.release().unwrap();
    let dropped = tx.savepoint().unwrap();
    insert(3).unwrap();
    dropped.rollback().unwrap();
    tx.commit().unwrap();
    file.close().unwrap();
    let conn = Connection::open(&path).unwrap();
    assert_eq!(rows(&conn, "mirror__rows"), [(1, "x".to_owned()), (2, "x".to_owned())]);
}

#[test]
fn readers_attach_the_store_read_only_and_join_it_with_the_collection() {
    let (_dir, collection) = copy_fixture("schema-18.anki2");
    let opened = open(&collection).unwrap();
    let crt = match &exec(opened.writer(), "SELECT crt FROM col").rows[0][0] {
        SqlValue::Integer(v) => *v,
        other => panic!("{other:?}"),
    };
    let path = derived::derived_path(&collection);
    filled(&path, &[(1, "joined")], mark(crt, 2, 3)).close().unwrap();
    // Reopen it as the derived writer would hold it while readers attach.
    let (file, _) = DerivedFile::open(&path).unwrap();

    for role in [ReaderRole::Query, ReaderRole::Maintenance, ReaderRole::Interactive] {
        let mut reader = opened.open_reader(role).unwrap();
        assert_eq!(reader.derived_epoch(), None);
        reader.attach_derived(&path, 4).unwrap();
        assert_eq!(reader.derived_epoch(), Some(4));
        let joined = query(
            &reader,
            "SELECT c.crt, d.v FROM col c JOIN derived.mirror__rows d ON d.k = c.id",
        );
        assert_eq!(
            joined.rows,
            [[SqlValue::Integer(crt), SqlValue::Text("joined".into())]],
            "{role:?}"
        );
        let write = reader.execute_raw("INSERT INTO derived.mirror__rows VALUES (5, 'no')", &[]);
        assert!(write.is_err(), "{role:?}: a reader writes nothing");
        // Attaching again with the same epoch is a no-op; another reattaches.
        reader.attach_derived(&path, 4).unwrap();
        reader.attach_derived(&path, 5).unwrap();
        assert_eq!(reader.derived_epoch(), Some(5));
        reader.detach_derived().unwrap();
        assert_eq!(reader.derived_epoch(), None);
        assert!(
            reader
                .execute_raw("SELECT count(*) FROM derived.mirror__rows", &[])
                .is_err(),
            "{role:?}: detached"
        );
    }
    file.close().unwrap();
    opened.close().unwrap();
}

#[test]
fn derived_activity_leaves_the_collection_schema_alone_and_backups_hold_no_derived_table() {
    let (dir, collection) = copy_fixture("schema-18.anki2");
    let opened = open(&collection).unwrap();
    let before = object_set(opened.writer());
    let definitions = exec(
        opened.writer(),
        "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name",
    );
    let path = derived::derived_path(&collection);
    let mut file = filled(&path, &[(1, "a")], mark(1, 2, 3));
    let registry = registry(vec![mirror(1), other()]);
    {
        let tx = file.begin().unwrap();
        tx.sync_components(&registry).unwrap();
        let schema = registry.get("mirror").unwrap();
        tx.begin_staging(schema).unwrap();
        tx.finish_staging(schema).unwrap();
        tx.reset_component(registry.get("other").unwrap()).unwrap();
        tx.commit().unwrap();
    }
    let mut maintenance = opened.open_reader(ReaderRole::Maintenance).unwrap();
    maintenance.attach_derived(&path, 1).unwrap();
    query(&maintenance, "SELECT count(*) FROM derived.mirror__rows");
    let backup = dir.path().join("backup.anki2");
    maintenance
        .execute_raw("VACUUM INTO ?1", &[SqlValue::Text(backup.display().to_string())])
        .unwrap();

    assert_eq!(object_set(opened.writer()), before);
    assert_eq!(
        exec(
            opened.writer(),
            "SELECT type, name, tbl_name, sql FROM sqlite_master ORDER BY type, name"
        ),
        definitions
    );
    let copy = Connection::open(&backup).unwrap();
    let tables = names(&copy, "table");
    assert!(tables.contains(&"cards".to_owned()), "{tables:?}");
    assert!(
        !tables.iter().any(|t| t.contains("__") || t.starts_with("meguri_")),
        "{tables:?}"
    );
    drop(maintenance);
    file.close().unwrap();
    opened.close().unwrap();
}
