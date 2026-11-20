//! RF4 storage only: sync/one-way O3/O6.2/O7.3, sync/anki-transfers M2/M3.
#![cfg(feature = "test-hooks")]

mod common;

use std::{fs, path::Path};

use common::{clock, empty_collection, exec, int, listing, query};
use meguri_model::{
    config::ConfigEntry,
    grave::GraveKind,
    ids::{CardId, DeckConfigId, DeckId, NoteId, NotetypeId, RevlogId},
    rng::SeededRandom,
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_storage::{
    OpenOptions, OpenedCollection, ReadRows, ReaderRole, StampMode, StorageError,
    one_way::{
        CollectionStamps, CollectionTotals, LocalTransferFacts, PendingContent, PendingDefinitions, PendingRows,
        UserContentRows,
    },
    open_collection,
    verified_replace::{self, ReplaceError, ReplacementStage},
};

const UPLOAD_AT: EpochMillis = EpochMillis(1_894_701_960_000);

fn managed(path: &Path) -> OpenedCollection {
    open_collection(path, &OpenOptions::new(&clock(), &SeededRandom::new(7)).managed()).unwrap()
}

// Public builders, with explicit pending/zero/positive/other-negative USNs.
// No foreign collection source or importer is needed for this contract.
fn seed(collection: &OpenedCollection) {
    let frozen = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &frozen, None)
        .unwrap();
    for (id, usn) in [(10, -1), (20, 0), (30, 7), (40, -2)] {
        let mut note = common::note(id, &["question", "answer"]);
        note.id = NoteId(id);
        note.usn = Usn(usn);
        note.mtime = EpochSecs(101);
        scope.put(&note).unwrap();
        let mut card = common::card(note.id, 0);
        card.id = CardId(id);
        card.usn = Usn(usn);
        card.mtime = EpochSecs(102);
        scope.put(&card).unwrap();
        let mut review = common::revlog(card.id, 1234);
        review.id = RevlogId(id);
        review.usn = Usn(usn);
        scope.put(&review).unwrap();
        let mut notetype = common::notetype(&format!("Type {id}"), &["Front", "Back"], &["Card"]);
        notetype.id = NotetypeId(id);
        notetype.usn = Usn(usn);
        notetype.mtime = EpochSecs(103);
        notetype.templates[0].usn = Usn(usn);
        notetype.templates[0].mtime = EpochSecs(107);
        scope.put_notetype(&notetype).unwrap();
        let mut deck = common::deck(&format!("Deck {id}"));
        deck.id = DeckId(id);
        deck.usn = Usn(usn);
        deck.mtime = EpochSecs(104);
        scope.put(&deck).unwrap();
        let mut preset = common::preset(&format!("Preset {id}"));
        preset.id = DeckConfigId(id);
        preset.usn = Usn(usn);
        preset.mtime = EpochSecs(105);
        scope.put(&preset).unwrap();
        let mut tag = common::tag(&format!("tag{id}"));
        tag.usn = Usn(usn);
        tag.collapsed = true;
        tag.config = Some(vec![8, 1]);
        scope.put(&tag).unwrap();
        scope
            .put_config_entry(&ConfigEntry {
                key: format!("test{id}"),
                usn: Usn(usn),
                mtime: EpochSecs(106),
                value: br#"{ "exact": 1.00 }"#.to_vec(),
            })
            .unwrap();
        scope.add_grave(id, GraveKind::Note, Usn(usn)).unwrap();
    }
    let mut col = scope.col().unwrap();
    col.modified = EpochMillis(3000);
    col.schema_modified = EpochMillis(2000);
    col.last_sync = EpochMillis(1000);
    col.usn = Usn(4);
    col.dirty = 9;
    scope.put_col(&col).unwrap();
    scope.commit().unwrap();
}

fn seeded_facts() -> LocalTransferFacts {
    LocalTransferFacts {
        stamps: CollectionStamps {
            modified: EpochMillis(3000),
            schema_modified: EpochMillis(2000),
            last_sync: EpochMillis(1000),
            usn: Usn(4),
        },
        totals: CollectionTotals {
            cards: 4,
            notes: 4,
            reviews: 4,
        },
        pending_reviews: 1,
        pending_rows: PendingRows {
            content: PendingContent { notes: 1, cards: 1 },
            definitions: PendingDefinitions {
                decks: 1,
                deck_config: 1,
                notetypes: 1,
                tags: 1,
            },
            config: 1,
            graves: 1,
        },
        last_review: Some(EpochMillis(40)),
    }
}

#[test]
fn facts_count_without_writes_and_preserve_the_callers_snapshot() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    let reader = collection.open_reader(ReaderRole::Sync).unwrap();
    let empty = reader.transfer_facts().unwrap();
    assert_eq!(
        (
            empty.totals.cards,
            empty.totals.notes,
            empty.totals.reviews,
            empty.pending_reviews
        ),
        (0, 0, 0, 0)
    );
    assert_eq!(empty.pending_rows, PendingRows::default());
    assert!(!empty.has_cards());
    assert_eq!(empty.last_review, None);
    seed(&collection);
    let snapshot = reader.begin_snapshot().unwrap();
    let before = fs::read(&path).unwrap();
    let wal_path = directory.path().join("collection.anki2-wal");
    let wal = fs::read(&wal_path).unwrap();
    let sidecar = fs::read(directory.path().join("meguri-sidecar.json")).unwrap();
    let data_version = query(&reader, "PRAGMA data_version");
    for _ in 0..3 {
        assert_eq!(reader.transfer_facts().unwrap(), seeded_facts());
        assert_eq!(snapshot.transfer_facts().unwrap(), seeded_facts());
    }
    assert_eq!(int(&query(&reader, "SELECT total_changes()")), 0);
    assert_eq!(query(&reader, "PRAGMA data_version"), data_version);
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&wal_path).unwrap(), wal);
    assert_eq!(fs::read(directory.path().join("meguri-sidecar.json")).unwrap(), sidecar);

    let frozen = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::Local, &frozen, None)
        .unwrap();
    let note = scope.add_note(&common::note(10, &["later", "committed"])).unwrap();
    let card = scope.add_card(&common::card(note, 0)).unwrap();
    scope.add_revlog(&common::revlog(card, 100)).unwrap();
    scope.commit().unwrap();
    assert_eq!(reader.transfer_facts().unwrap(), seeded_facts());
    assert_eq!(snapshot.transfer_facts().unwrap(), seeded_facts());
    assert!(matches!(reader.begin_snapshot(), Err(StorageError::TransactionOpen)));
    drop(snapshot);
    let fresh = reader.transfer_facts().unwrap();
    assert!(fresh.has_cards());
    assert_eq!(
        (
            fresh.totals.cards,
            fresh.totals.notes,
            fresh.totals.reviews,
            fresh.pending_reviews
        ),
        (5, 5, 5, 2)
    );
    assert_eq!(
        (fresh.pending_rows.content.cards, fresh.pending_rows.content.notes),
        (2, 2)
    );
    assert_eq!(fresh.stamps.modified, EpochMillis(common::NOW_MS));
    assert_eq!(fresh.last_review, Some(EpochMillis(common::NOW_MS)));
    // A facts read outside a transaction leaves none behind either.
    drop(reader.begin_snapshot().unwrap());
    drop(reader);
    collection.close().unwrap();
}

// Every column outside O6.2's declared changes is compared byte-for-byte by
// SQLite storage class, including config, templates and all row timestamps.
const PRESERVED: &[&str] = &[
    "SELECT id, crt, ver, dty, conf, models, decks, dconf, tags FROM col",
    "SELECT id, guid, mid, mod, tags, flds, sfld, csum, flags, data FROM notes ORDER BY id",
    "SELECT id, nid, did, ord, mod, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data FROM cards ORDER BY id",
    "SELECT id, cid, ease, ivl, lastIvl, factor, time, type FROM revlog ORDER BY id",
    "SELECT id, name, mtime_secs, common, kind FROM decks ORDER BY id",
    "SELECT id, name, mtime_secs, config FROM deck_config ORDER BY id",
    "SELECT id, name, mtime_secs, config FROM notetypes ORDER BY id",
    "SELECT tag, collapsed, config FROM tags ORDER BY tag",
    "SELECT * FROM config ORDER BY key",
    "SELECT * FROM templates ORDER BY ntid, ord",
    "SELECT * FROM fields ORDER BY ntid, ord",
];

#[test]
fn content_witness_moves_with_a_row_that_no_collection_stamp_records() {
    let (_directory, path) = empty_collection();
    let collection = managed(&path);
    seed(&collection);
    let reader = collection.open_reader(ReaderRole::Query).unwrap();
    let before = reader.content_witness().unwrap();
    let facts = reader.transfer_facts().unwrap();
    assert_eq!(reader.content_witness().unwrap(), before);
    // A row stamped again with no pending mark and no change of col.mod.
    exec(collection.writer(), "UPDATE decks SET mtime_secs = (SELECT max(mtime_secs) + 5 FROM decks) WHERE id = (SELECT min(id) FROM decks)");
    assert_eq!(reader.transfer_facts().unwrap(), facts);
    assert_ne!(reader.content_witness().unwrap(), before);
    let stamped = reader.content_witness().unwrap();
    // A row added with no pending mark moves a count.
    exec(collection.writer(), "INSERT INTO tags (tag, usn, collapsed) VALUES ('witness', 0, 0)");
    assert_eq!(reader.transfer_facts().unwrap(), facts);
    assert_ne!(reader.content_witness().unwrap(), stamped);
}

#[test]
fn upload_transforms_only_the_copy_includes_wal_and_finalises_transformed_statistics() {
    let (directory, path) = empty_collection();
    let collection = managed(&path);
    // Pin a view before the WAL edits to prove capture uses its own fresh view.
    let query_reader = collection.open_reader(ReaderRole::Query).unwrap();
    let pinned = query_reader.begin_snapshot().unwrap();
    seed(&collection);
    // Enough deleted content to free real pages during the transform.
    exec(
        collection.writer(),
        "WITH RECURSIVE ids(x) AS (VALUES(1000) UNION ALL SELECT x + 1 FROM ids WHERE x < 3000) INSERT INTO graves SELECT x, 1, 7 FROM ids",
    );
    exec(collection.writer(), "ANALYZE");
    let sync_reader = collection.open_reader(ReaderRole::Sync).unwrap();
    let preserved = PRESERVED.iter().map(|sql| query(&sync_reader, sql)).collect::<Vec<_>>();
    let before = fs::read(&path).unwrap();
    let wal_path = directory.path().join("collection.anki2-wal");
    let wal = fs::read(&wal_path).unwrap();
    assert!(wal.len() > 32);
    let sidecar = fs::read(directory.path().join("meguri-sidecar.json")).unwrap();
    let stage = ReplacementStage::new(directory.path()).unwrap();
    sync_reader
        .prepare_full_upload(&stage, UPLOAD_AT, &SeededRandom::new(9))
        .unwrap();
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&wal_path).unwrap(), wal);
    assert_eq!(fs::read(directory.path().join("meguri-sidecar.json")).unwrap(), sidecar);
    assert_eq!(pinned.transfer_facts().unwrap().totals.cards, 0);
    assert_eq!(sync_reader.transfer_facts().unwrap(), seeded_facts());
    assert_eq!(&fs::read(stage.path()).unwrap()[18..20], &[1, 1]);
    assert_eq!(listing(stage.directory()), ["collection.anki2".to_owned()].into());

    // The witness reads no usn and no collection stamp: the collection and the
    // candidate prepared from it give the same value, graves and all.
    let witness = sync_reader.content_witness().unwrap();
    assert_eq!(meguri_storage::one_way::closed_content_witness(&stage.path()).unwrap(), Some(witness));

    // Immutable read cannot change the prepared header or create sidecars.
    let uri = format!("file:{}?mode=ro&immutable=1", stage.path().display());
    let prepared = meguri_storage::rusqlite::Connection::open_with_flags(
        uri,
        meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY
            | meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    prepared
        .create_collation("unicase", meguri_text::fold::compare_folded)
        .unwrap();
    assert_eq!(
        prepared
            .query_row("PRAGMA integrity_check", [], |r| r.get::<_, String>(0))
            .unwrap(),
        "ok"
    );
    assert_eq!(
        prepared
            .query_row("PRAGMA freelist_count", [], |r| r.get::<_, i64>(0))
            .unwrap(),
        0
    );
    for (table, index, expected) in [
        ("notes", "ix_notes_usn", "4 2"),
        ("cards", "ix_cards_usn", "4 2"),
        ("revlog", "ix_revlog_usn", "4 2"),
        ("notetypes", "idx_notetypes_usn", "4 4"),
    ] {
        let stat: String = prepared
            .query_row(
                "SELECT stat FROM sqlite_stat1 WHERE tbl = ?1 AND idx = ?2",
                [table, index],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(stat, expected, "{table}");
    }
    assert_eq!(
        prepared
            .query_row("SELECT count(*) FROM sqlite_stat1 WHERE tbl = 'graves'", [], |r| r
                .get::<_, i64>(0))
            .unwrap(),
        0
    );
    prepared.close().unwrap();

    // Opening through the normal gate may switch journal mode. Inspect an
    // independent test copy so the exact prepared artifact remains untouched.
    let prepared_bytes = fs::read(stage.path()).unwrap();
    let inspection = ReplacementStage::new(directory.path()).unwrap();
    fs::copy(stage.path(), inspection.path()).unwrap();
    let copy = common::open(&inspection.path()).unwrap();
    let reader = copy.open_reader(ReaderRole::Maintenance).unwrap();
    assert_eq!(int(&query(&reader, "SELECT count(*) FROM graves")), 0);
    for table in ["notes", "cards", "revlog"] {
        let rows = query(&reader, &format!("SELECT usn FROM {table} ORDER BY id"));
        use meguri_storage::raw::SqlValue::Integer;
        assert_eq!(
            rows.rows,
            vec![vec![Integer(0)], vec![Integer(0)], vec![Integer(7)], vec![Integer(-2)]],
            "{table}"
        );
    }
    for (table, order) in [
        ("decks", "id"),
        ("deck_config", "id"),
        ("notetypes", "id"),
        ("tags", "tag"),
    ] {
        use meguri_storage::raw::SqlValue::Integer;
        assert_eq!(
            query(&reader, &format!("SELECT usn FROM {table} ORDER BY {order}")).rows,
            vec![vec![Integer(0)]; 4],
            "{table}"
        );
    }
    let col = reader.col().unwrap();
    assert_eq!(
        (col.usn, col.modified, col.schema_modified, col.last_sync),
        (Usn(5), UPLOAD_AT, UPLOAD_AT, UPLOAD_AT)
    );
    for (sql, expected) in PRESERVED.iter().zip(preserved) {
        assert_eq!(query(&reader, sql), expected, "{sql}");
    }
    drop(reader);
    copy.close().unwrap();
    assert_eq!(fs::read(stage.path()).unwrap(), prepared_bytes);
    assert_eq!(listing(stage.directory()), ["collection.anki2".to_owned()].into());
    drop(pinned);
    drop(query_reader);
    drop(sync_reader);
    collection.close().unwrap();
}

#[test]
fn upload_overflow_refuses_and_rolls_back_the_candidate_without_live_mutation() {
    let (directory, path) = empty_collection();
    let collection = common::open(&path).unwrap();
    seed(&collection);
    exec(collection.writer(), "UPDATE col SET usn = 2147483647");
    let before = fs::read(&path).unwrap();
    let wal_path = directory.path().join("collection.anki2-wal");
    let wal = fs::read(&wal_path).unwrap();
    let reader = collection.open_reader(ReaderRole::Sync).unwrap();
    let stage = ReplacementStage::new(directory.path()).unwrap();
    assert_eq!(
        reader.prepare_full_upload(&stage, UPLOAD_AT, &SeededRandom::new(9)),
        Err(ReplaceError::CallerChangeFailed(StorageError::Mode {
            detail: "full-upload col.usn overflow".into()
        }))
    );
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(fs::read(&wal_path).unwrap(), wal);
    let copy = common::open(&stage.path()).unwrap();
    assert_eq!(copy.writer().col().unwrap().usn, Usn(i32::MAX));
    assert_eq!(copy.writer().col().unwrap().modified, EpochMillis(3000));
    assert_eq!(int(&exec(copy.writer(), "SELECT count(*) FROM graves")), 4);
    assert_eq!(
        int(&exec(copy.writer(), "SELECT count(*) FROM notes WHERE usn = -1")),
        1
    );
    copy.close().unwrap();
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn upload_refuses_existing_or_dangling_destination_and_sidecars_before_snapshot() {
    use std::os::unix::fs::symlink;
    let (directory, path) = empty_collection();
    let collection = common::open(&path).unwrap();
    let reader = collection.open_reader(ReaderRole::Sync).unwrap();
    let before = fs::read(&path).unwrap();
    for suffix in ["", "-wal", "-shm", "-journal"] {
        for dangling in [false, true] {
            let stage = ReplacementStage::new(directory.path()).unwrap();
            let destination = stage.directory().join(format!("collection.anki2{suffix}"));
            if dangling {
                symlink("missing", &destination).unwrap();
            } else {
                fs::write(&destination, b"").unwrap();
            }
            let entries = listing(stage.directory());
            assert_eq!(
                reader.prepare_full_upload(&stage, UPLOAD_AT, &SeededRandom::new(9)),
                Err(ReplaceError::Io(StorageError::DestinationExists))
            );
            assert_eq!(listing(stage.directory()), entries);
            if dangling {
                assert_eq!(fs::read_link(destination).unwrap(), Path::new("missing"));
            } else {
                assert_eq!(fs::read(destination).unwrap(), b"");
            }
        }
    }
    let stage = ReplacementStage::new(directory.path()).unwrap();
    let snapshot = reader.begin_snapshot().unwrap();
    assert_eq!(
        reader.prepare_full_upload(&stage, UPLOAD_AT, &SeededRandom::new(9)),
        Err(ReplaceError::Io(StorageError::TransactionOpen))
    );
    assert!(snapshot.col().is_ok());
    assert!(listing(stage.directory()).is_empty());
    assert_eq!(fs::read(&path).unwrap(), before);
    drop(snapshot);
    // Upload's internal Sync entry must not widen the public export API.
    assert!(matches!(
        reader.snapshot_live(stage.directory()),
        Err(StorageError::Mode { .. })
    ));
    assert!(matches!(reader.live_snapshot_source(), Err(StorageError::Mode { .. })));
    assert!(listing(stage.directory()).is_empty());
    for role in ReaderRole::ALL {
        if role == ReaderRole::Sync {
            continue;
        }
        let other = collection.open_reader(role).unwrap();
        assert!(matches!(
            other.prepare_full_upload(&stage, UPLOAD_AT, &SeededRandom::new(9)),
            Err(ReplaceError::Io(StorageError::Mode { .. }))
        ));
        assert!(listing(stage.directory()).is_empty());
    }
    let elsewhere = tempfile::tempdir().unwrap();
    let wrong_directory = ReplacementStage::new(elsewhere.path()).unwrap();
    assert!(
        reader
            .prepare_full_upload(&wrong_directory, UPLOAD_AT, &SeededRandom::new(9))
            .is_err()
    );
    assert!(listing(wrong_directory.directory()).is_empty());
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn full_download_uses_the_existing_typed_caller_transform() {
    let (directory, path) = empty_collection();
    let collection = common::open(&path).unwrap();
    seed(&collection);
    collection.close().unwrap();
    let stage = ReplacementStage::new(directory.path()).unwrap();
    fs::copy(&path, stage.path()).unwrap();
    let before = fs::read(&path).unwrap();
    verified_replace::prepare(
        &stage,
        &path,
        &OpenOptions::new(&clock(), &SeededRandom::new(7)),
        &clock(),
        false,
        |scope| {
            let mut col = scope.col()?;
            col.last_sync = col.modified;
            scope.put_col(&col)?;
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(&fs::read(stage.path()).unwrap()[18..20], &[1, 1]);
    assert_eq!(fs::read(&path).unwrap(), before);
    let copy = common::open(&stage.path()).unwrap();
    let col = copy.writer().col().unwrap();
    assert_eq!(
        (col.last_sync, col.modified, col.schema_modified, col.usn),
        (EpochMillis(3000), EpochMillis(3000), EpochMillis(2000), Usn(4))
    );
    copy.close().unwrap();
}

// sync/cloudkit-recovery V27.8, first row: the V27.1 row test.

#[test]
fn user_content_rows_count_only_what_v27_1_names() {
    let (_dir, path) = empty_collection();
    let collection = managed(&path);
    let reader = collection.open_reader(ReaderRole::Sync).unwrap();
    // A new collection holds none, whatever stock note types it was created with.
    let created = reader.user_content_rows().unwrap();
    assert_eq!(created, UserContentRows::default());
    assert!(created.none());

    // Presets, config and tags do not count.
    let frozen = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &frozen, None)
        .unwrap();
    let mut preset = common::preset("Changed preset");
    preset.id = DeckConfigId(77);
    preset.mtime = EpochSecs(105);
    scope.put(&preset).unwrap();
    scope.put(&common::tag("changed")).unwrap();
    scope
        .put_config_entry(&ConfigEntry {
            key: "changed".into(),
            usn: Usn(-1),
            mtime: EpochSecs(106),
            value: b"1".to_vec(),
        })
        .unwrap();
    scope.commit().unwrap();
    assert!(reader.user_content_rows().unwrap().none());

    // Each of a note, a card, a review, a second deck and a saved note type counts alone.
    type Change = fn(&mut UserContentRows);
    let cases: [(&str, Change); 5] = [
        ("note", |rows| rows.notes = 1),
        ("card", |rows| rows.cards = 1),
        ("review", |rows| rows.reviews = 1),
        ("deck", |rows| rows.other_decks = 1),
        ("notetype", |rows| rows.saved_notetypes = 1),
    ];
    for (case, expect) in cases {
        let (_dir, path) = empty_collection();
        let collection = managed(&path);
        let mut scope = collection
            .writer()
            .begin_write(StampMode::CallerSupplied, &frozen, None)
            .unwrap();
        match case {
            "note" => {
                let mut note = common::note(10, &["question", "answer"]);
                note.id = NoteId(10);
                scope.put(&note).unwrap();
            }
            "card" => {
                let mut card = common::card(NoteId(10), 0);
                card.id = CardId(10);
                scope.put(&card).unwrap();
            }
            "review" => {
                let mut review = common::revlog(CardId(10), 1234);
                review.id = RevlogId(10);
                scope.put(&review).unwrap();
            }
            "deck" => {
                let mut deck = common::deck("Second");
                deck.id = DeckId(10);
                scope.put(&deck).unwrap();
            }
            _ => {
                let mut notetype = common::notetype("Made here", &["Front", "Back"], &["Card"]);
                notetype.id = NotetypeId(10);
                notetype.mtime = EpochSecs(103);
                scope.put_notetype(&notetype).unwrap();
            }
        }
        scope.commit().unwrap();
        let mut expected = UserContentRows::default();
        expect(&mut expected);
        let rows = collection.open_reader(ReaderRole::Sync).unwrap().user_content_rows().unwrap();
        assert_eq!(rows, expected, "{case}");
        assert!(!rows.none(), "{case}");
    }
}

#[test]
fn stock_notetype_as_created_is_not_user_content_until_saved() {
    let (_dir, path) = empty_collection();
    let collection = managed(&path);
    let frozen = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &frozen, None)
        .unwrap();
    // storage/new-collection B7 writes a stock row with mtime_secs 0.
    let mut stock = common::notetype("Stock", &["Front", "Back"], &["Card"]);
    stock.id = NotetypeId(20);
    stock.mtime = EpochSecs(0);
    scope.put_notetype(&stock).unwrap();
    scope.commit().unwrap();
    let reader = collection.open_reader(ReaderRole::Sync).unwrap();
    assert!(reader.user_content_rows().unwrap().none());
    // notetypes/records N3.2: every local save stamps the row with the current time.
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &frozen, None)
        .unwrap();
    stock.mtime = EpochSecs(1_894_701_960);
    scope.put_notetype(&stock).unwrap();
    scope.commit().unwrap();
    assert_eq!(reader.user_content_rows().unwrap().saved_notetypes, 1);
}

// sync/meguri-provider MP6.3: the row test of a retained server copy.

fn fixture(name: &str) -> (tempfile::TempDir, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage").join(name),
        &path,
    )
    .unwrap();
    (dir, path)
}

#[test]
fn server_copy_user_content_reads_the_served_file_without_changing_it() {
    use meguri_storage::one_way::{server_copy_user_content, verify_server_copy};
    // A served collection that holds none.
    let (_dir, path) = empty_collection();
    let before = fs::read(&path).unwrap();
    let rows = server_copy_user_content(&path).unwrap().unwrap();
    assert!(rows.none());
    assert_eq!(fs::read(&path).unwrap(), before);
    assert_eq!(listing(path.parent().unwrap()).into_iter().collect::<Vec<_>>(), ["collection.anki2"]);

    // One deck beside the default is user content, though the file has no card.
    let collection = managed(&path);
    let frozen = clock();
    let mut scope = collection
        .writer()
        .begin_write(StampMode::CallerSupplied, &frozen, None)
        .unwrap();
    let mut deck = common::deck("Second");
    deck.id = DeckId(10);
    scope.put(&deck).unwrap();
    scope.commit().unwrap();
    collection.close().unwrap();
    let rows = server_copy_user_content(&path).unwrap().unwrap();
    assert_eq!(
        rows,
        UserContentRows {
            other_decks: 1,
            ..UserContentRows::default()
        }
    );
    assert!(!rows.none());
    // The two reads of one gate agree on what was served.
    assert!(!verify_server_copy(&path).unwrap().unwrap().has_cards());
}

#[test]
fn server_copy_user_content_proves_nothing_for_a_legacy_or_refused_file() {
    use meguri_storage::one_way::server_copy_user_content;
    // A legacy schema is packaged but not read: no rows, so nothing is proved.
    let (_dir, path) = fixture("schema-11.anki2");
    let before = fs::read(&path).unwrap();
    assert_eq!(server_copy_user_content(&path).unwrap(), None);
    assert_eq!(fs::read(&path).unwrap(), before);

    // Refused versions, a leftover journal and a file that is no collection are errors, never "none".
    let (_dir, path) = fixture("ver-19-delete.anki2");
    assert!(server_copy_user_content(&path).is_err());
    let (_dir, path) = empty_collection();
    fs::write(path.with_file_name("collection.anki2-wal"), b"").unwrap();
    assert!(server_copy_user_content(&path).is_err());
    let (_dir, path) = empty_collection();
    fs::write(&path, b"not a collection").unwrap();
    assert!(server_copy_user_content(&path).is_err());
    assert!(server_copy_user_content(&path.with_file_name("absent.anki2")).is_err());
}
