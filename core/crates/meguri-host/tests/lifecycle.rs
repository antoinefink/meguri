use std::{path::Path, sync::Arc};

use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{ReadRows, ReaderRole, space::StatvfsSpace};

fn config(root: &Path) -> HostConfig {
    let collection = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/preservation/kitchen-sink.anki2"),
        collection.join("collection.anki2"),
    )
    .unwrap();
    HostConfig::new(
        collection,
        backups,
        root.join("app-group/widget.snapshot"),
        Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC")),
        Arc::new(SeededRandom::new(38)),
        Arc::new(StatvfsSpace),
    )
}

#[test]
fn open_wires_derived_and_media_and_close_releases_retained_journal_handles() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let collection = host.collection().unwrap();
    assert!(collection.reader(ReaderRole::Query).unwrap().col().is_ok());
    assert!(host.derived().path().unwrap().is_file());
    let media = host.media().unwrap();
    media.add("owned.txt", b"synthetic media").unwrap();
    assert_eq!(media.store().pending().unwrap().len(), 1);
    host.close().unwrap();
    assert!(collection.reader(ReaderRole::Query).is_err());
    assert!(media.store().pending().is_err(), "all clones share the closed journal");
    assert!(std::fs::metadata(dir.path().join("collection/collection.anki2-wal")).map_or(true, |m| m.len() == 0));
}

#[test]
fn suspend_closes_media_across_clones_and_resume_reopens_the_same_store() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let media = host.media().unwrap();
    media.add("owned.txt", b"synthetic media").unwrap();
    let before = media.store().pending().unwrap();
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    assert!(media.store().pending().is_err());
    host.engine().resume().unwrap();
    assert_eq!(media.store().pending().unwrap(), before);
    host.close().unwrap();
}

#[test]
fn an_open_failure_releases_workers_and_the_collection_ownership_lock() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    // The media journal's path cannot redirect outside the private directory.
    let journal = config.collection_directory.join("meguri-media.db");
    std::os::unix::fs::symlink(dir.path().join("outside"), &journal).unwrap();
    assert!(CoreHost::open(config.clone()).is_err());
    std::fs::remove_file(journal).unwrap();
    let host = CoreHost::open(config).unwrap();
    host.close().unwrap();
}

#[test]
fn an_unavailable_backup_refuses_a_real_deck_deletion_before_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let collection = host.collection().unwrap();
    let id = meguri_engine::decks::add(&collection, "Backup refusal".into())
        .unwrap()
        .value
        .id;
    let kind = meguri_engine::notetypes::add_stock(
        &collection,
        meguri_engine::notetypes::stock::StockKind::Basic,
        Some("Backup test".into()),
    )
    .unwrap()
    .value
    .id;
    let added = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["synthetic front".into(), "synthetic back".into()],
            tags: vec![],
            deck_id: id,
            guid: None,
            update_add_defaults: false,
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert_eq!(added.value.cards.len(), 1);
    let before = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        (
            reader.col().unwrap(),
            reader.execute_raw("SELECT * FROM decks ORDER BY id", &[]).unwrap(),
        )
    };
    assert!(meguri_engine::decks::delete(&collection, vec![id], host.backup()).is_err());
    let reader = collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        (
            reader.col().unwrap(),
            reader.execute_raw("SELECT * FROM decks ORDER BY id", &[]).unwrap()
        ),
        before
    );
    drop(reader);
    host.close().unwrap();
}
