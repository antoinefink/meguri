//! Storage assertions for the P7-authored notice and P11 standalone VACUUM.
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    collection_package::{NOTICE, create_placeholder, snapshot_standalone},
    rusqlite::{Connection, OpenFlags},
};
use std::{fs, path::Path};

#[test]
fn placeholder_is_authored_schema_eleven_notice_with_only_p7_objects() {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collection.anki2");
    let now = 1_894_701_600_123_i64;
    create_placeholder(&path, &FixedClock::new(now, UtcOffset::UTC), &SeededRandom::new(42)).unwrap();
    assert_eq!(&fs::read(&path).unwrap()[18..20], &[1, 1]);
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let objects: Vec<String> = conn
        .prepare("SELECT name FROM sqlite_master ORDER BY name")
        .unwrap()
        .query_map([], |row| row.get(0))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    let mut expected = vec![
        "col",
        "notes",
        "cards",
        "revlog",
        "graves",
        "ix_notes_usn",
        "ix_cards_usn",
        "ix_revlog_usn",
        "ix_cards_nid",
        "ix_cards_sched",
        "ix_revlog_cid",
        "ix_notes_csum",
    ];
    expected.sort();
    assert_eq!(objects, expected);
    let (version, crt, modified, scm): (i64, i64, i64, i64) = conn
        .query_row("SELECT ver,crt,mod,scm FROM col", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!(
        (version, crt, modified, scm),
        (11, now.div_euclid(86_400_000) * 86_400, now, now)
    );
    let (conf, models, decks, dconf): (String, String, String, String) = conn
        .query_row("SELECT conf,models,decks,dconf FROM col", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    let conf: serde_json::Value = serde_json::from_str(&conf).unwrap();
    assert_eq!(conf["schedVer"], 2);
    assert_eq!(conf["curModel"], 1);
    let model = meguri_model::schema11::col::notetypes_from_json(&models)
        .unwrap()
        .pop()
        .unwrap();
    assert_eq!(model.name, "Notice");
    assert_eq!(model.fields.len(), 1);
    assert_eq!(model.fields[0].name, "Text");
    assert_eq!(model.templates.len(), 1);
    assert_eq!(model.templates[0].config.q_format, "{{Text}}");
    assert_eq!(model.templates[0].config.a_format, "{{Text}}");
    assert_eq!(
        model.config.css,
        ".card { font-family: system-ui, sans-serif; font-size: 18px; margin: 2em; }"
    );
    assert_eq!(meguri_model::schema11::col::decks_from_json(&decks).unwrap().len(), 1);
    assert_eq!(
        meguri_model::schema11::col::deck_configs_from_json(&dconf)
            .unwrap()
            .len(),
        1
    );
    let (id, text, sfld, csum): (i64, String, String, u32) = conn
        .query_row("SELECT id,flds,sfld,csum FROM notes", [], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?))
        })
        .unwrap();
    assert_eq!(id, now);
    assert_eq!(text, NOTICE);
    assert_eq!(sfld, meguri_text::checksum::sort_text(NOTICE));
    assert_eq!(csum, meguri_text::checksum::note_checksum(NOTICE));
    assert_eq!(
        conn.query_row("SELECT count(*) FROM notes", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM cards WHERE id=nid AND did=1 AND type=0 AND queue=0 AND due=1",
            [],
            |r| r.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM revlog", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    assert_eq!(
        conn.query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    drop(conn);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 1);
}

#[test]
fn snapshot_vacuums_with_unicase_and_keeps_source_and_sidecar_unchanged() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &source,
    )
    .unwrap();
    let original = fs::read(&source).unwrap();
    fs::write(directory.path().join("meguri-sidecar.json"), b"private device state").unwrap();
    let snapshot = directory.path().join("snapshot.anki2");
    snapshot_standalone(&source, &snapshot).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(
        fs::read(directory.path().join("meguri-sidecar.json")).unwrap(),
        b"private device state"
    );
    assert_eq!(&fs::read(&snapshot).unwrap()[18..20], &[2, 2]);
    assert_eq!(
        fs::read_dir(directory.path()).unwrap().count(),
        3,
        "unexpected files beside the snapshot"
    );
    assert!(meguri_storage::integrity::integrity_check(&snapshot).unwrap().is_ok());
}

#[test]
fn vacuum_snapshot_contains_the_actual_authored_source_rows() {
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("source.anki2");
    create_placeholder(
        &source,
        &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
        &SeededRandom::new(7),
    )
    .unwrap();
    let original = fs::read(&source).unwrap();
    let snapshot = directory.path().join("snapshot.anki2");
    snapshot_standalone(&source, &snapshot).unwrap();
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(&fs::read(&snapshot).unwrap()[18..20], &[2, 2]);
    assert_eq!(fs::read_dir(directory.path()).unwrap().count(), 2);
    let conn = Connection::open_with_flags(&snapshot, OpenFlags::SQLITE_OPEN_READ_ONLY).unwrap();
    let text: String = conn.query_row("SELECT flds FROM notes", [], |row| row.get(0)).unwrap();
    assert_eq!(text, NOTICE);
    assert_eq!(
        conn.query_row(
            "SELECT count(*) FROM cards WHERE nid=(SELECT id FROM notes) AND due=1",
            [],
            |row| row.get::<_, i64>(0)
        )
        .unwrap(),
        1
    );
    assert_eq!(
        conn.query_row("SELECT count(*) FROM revlog", [], |row| row.get::<_, i64>(0))
            .unwrap(),
        0
    );
    drop(conn);
    assert_eq!(fs::read(&source).unwrap(), original);
}
