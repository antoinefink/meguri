//! Live P10/P11 publication and the captured backup reference (T2.3).
use std::{
    fs::{self, File},
    io::Read,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::{NoteId, NotetypeId},
    note::Note,
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_package::{
    CollectionPackageWriteContext, CollectionPackageWriteError as Error, Finalisation, LiveCollectionPackageWritten,
    write_collection_package_from_live,
};
use meguri_storage::{
    OpenOptions, OpenedCollection, ReadRows, Reader, ReaderRole, StampMode, StorageError,
    atomic_file::Purpose,
    open_collection,
    space::{SpaceProvider, StatvfsSpace},
};

const NOW: i64 = 1_894_701_600_000;
const SENTINEL: &[u8] = &[0x91, 0x00, 0xe7, 0x42, 0xff];

fn clock(at: i64) -> FixedClock {
    FixedClock::new(at, UtcOffset::UTC)
}
fn live() -> (tempfile::TempDir, PathBuf, OpenedCollection) {
    let directory = tempfile::tempdir().unwrap();
    let path = directory.path().join("collection.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let collection = open_collection(&path, &OpenOptions::new(&clock(NOW), &SeededRandom::new(7)).managed()).unwrap();
    (directory, path, collection)
}
fn add_note(collection: &OpenedCollection, at: i64, text: &str) -> NoteId {
    let clock = clock(at);
    let mut scope = collection.writer().begin_write(StampMode::Local, &clock, None).unwrap();
    let id = scope
        .add_note(&Note {
            guid: format!("live-{at}"),
            notetype_id: NotetypeId(1),
            fields: text.into(),
            sort_field: text.into(),
            ..Note::default()
        })
        .unwrap();
    scope.commit().unwrap();
    id
}
fn write_with(
    reader: &Reader,
    destination: &Path,
    staging: &Path,
    space: &dyn SpaceProvider,
    finalisation: Finalisation,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<LiveCollectionPackageWritten, Error> {
    write_collection_package_from_live(
        reader,
        destination,
        &CollectionPackageWriteContext {
            collection_directory: staging,
            purpose: Purpose::Backup,
            finalisation,
            clock: &clock(NOW),
            random: &SeededRandom::new(7),
            space,
            is_cancelled: cancel,
        },
    )
}
fn listing(path: &Path) -> Vec<std::ffi::OsString> {
    let mut entries: Vec<_> = fs::read_dir(path).unwrap().map(|e| e.unwrap().file_name()).collect();
    entries.sort();
    entries
}
fn source_bytes(directory: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    ["collection.anki2", "collection.anki2-wal", "meguri-sidecar.json"]
        .into_iter()
        .map(|name| {
            let path = directory.join(name);
            let bytes = fs::read(&path).unwrap();
            (path, bytes)
        })
        .collect()
}
fn assert_preserved(files: &[(PathBuf, Vec<u8>)]) {
    for (path, bytes) in files {
        assert_eq!(&fs::read(path).unwrap(), bytes, "{} changed", path.display());
    }
}

#[test]
fn live_wal_package_keeps_its_mod_while_new_answers_commit_during_publication() {
    let (directory, _, collection) = live();
    let old_view = collection.open_reader(ReaderRole::Sync).unwrap();
    let sync = old_view.begin_snapshot().unwrap();
    assert!(sync.col().is_ok());
    let before = add_note(&collection, NOW, "committed in WAL before the snapshot");
    assert!(sync.note(before).unwrap().is_none());
    assert!(
        fs::metadata(directory.path().join("collection.anki2-wal"))
            .unwrap()
            .len()
            > 32
    );
    let maintenance = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let destination = directory.path().join("live.colpkg");
    let (captured_tx, captured_rx) = mpsc::channel();
    let (resume_tx, resume_rx) = mpsc::channel();
    let resume_rx = Mutex::new(resume_rx);
    let calls = AtomicUsize::new(0);
    let cancel = || {
        // Entry and preflight checks precede VACUUM; the next check is in
        // common publication, after the captured copy has been closed.
        if calls.fetch_add(1, Ordering::SeqCst) == 2 {
            captured_tx.send(()).unwrap();
            resume_rx.lock().unwrap().recv_timeout(Duration::from_secs(30)).unwrap();
        }
        false
    };
    let ((facts, maintenance), after) = std::thread::scope(|scope| {
        let destination = &destination;
        let staging = directory.path();
        let cancel = &cancel;
        let task = scope.spawn(move || {
            let facts = write_with(
                &maintenance,
                destination,
                staging,
                &StatvfsSpace,
                Finalisation::ExclusiveCreate,
                cancel,
            )
            .unwrap();
            (facts, maintenance)
        });
        captured_rx.recv_timeout(Duration::from_secs(30)).unwrap();
        // The original writer commits while the same live reader publishes.
        let after = add_note(&collection, NOW + 10_000, "committed after the captured view");
        resume_tx.send(()).unwrap();
        (task.join().unwrap(), after)
    });
    assert_eq!(facts.snapshot_modified, EpochMillis(NOW));
    assert_eq!(collection.writer().col().unwrap().modified, EpochMillis(NOW + 10_000));
    assert!(maintenance.note(after).unwrap().is_some());
    assert!(sync.note(before).unwrap().is_none(), "sync retains its original view");

    // Inspect the generated ZIP with zip/zstd directly, without the package
    // reader or its verifier deciding what the test observes.
    let mut archive = zip::ZipArchive::new(File::open(&destination).unwrap()).unwrap();
    assert_eq!(archive.len(), 4);
    for (index, name) in ["meta", "collection.anki21b", "collection.anki2", "media"]
        .into_iter()
        .enumerate()
    {
        let entry = archive.by_index(index).unwrap();
        assert_eq!(entry.name().unwrap().as_ref(), name);
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
    }
    let mut meta = Vec::new();
    archive.by_name("meta").unwrap().read_to_end(&mut meta).unwrap();
    assert_eq!(meta, [0x08, 0x03]);
    let bytes = zstd::stream::decode_all(archive.by_name("collection.anki21b").unwrap()).unwrap();
    assert_eq!(&bytes[..16], b"SQLite format 3\0");
    assert_eq!(&bytes[18..20], &[2, 2]);
    assert!(
        zstd::stream::decode_all(archive.by_name("media").unwrap())
            .unwrap()
            .is_empty()
    );
    let restored_directory = tempfile::tempdir().unwrap();
    let restored_path = restored_directory.path().join("captured.anki2");
    fs::write(&restored_path, bytes).unwrap();
    let restored = open_collection(&restored_path, &OpenOptions::new(&clock(NOW), &SeededRandom::new(7))).unwrap();
    assert_eq!(restored.writer().col().unwrap().modified, facts.snapshot_modified);
    assert_eq!(
        restored.writer().note(before).unwrap(),
        collection.writer().note(before).unwrap()
    );
    assert!(restored.writer().note(after).unwrap().is_none());
    restored.close().unwrap();
    let later = add_note(&collection, NOW + 20_000, "writer remains usable after publication");
    assert!(maintenance.note(later).unwrap().is_some());
    assert_eq!(facts.snapshot_modified, EpochMillis(NOW));
    drop(sync);
    drop(old_view);
    drop(maintenance);
    collection.close().unwrap();
}

#[test]
fn live_preflight_uses_actual_database_and_wal_on_both_filesystems_before_staging() {
    struct Capacity<'a> {
        full: &'a Path,
        visits: Mutex<Vec<PathBuf>>,
    }
    impl SpaceProvider for Capacity<'_> {
        fn available_bytes(&self, path: &Path) -> Result<u64, StorageError> {
            self.visits.lock().unwrap().push(path.to_owned());
            Ok(if path == self.full { 0 } else { u64::MAX })
        }
    }
    for full_staging in [true, false] {
        let (directory, path, collection) = live();
        add_note(&collection, NOW, "WAL contributes to the estimate");
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        // Neither context directory is the source: metadata must come from
        // the connected reader, not a guessed collection path in the context.
        let staging = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let destination = output.path().join("existing.colpkg");
        fs::write(&destination, SENTINEL).unwrap();
        let original = source_bytes(directory.path());
        let source_listing = listing(directory.path());
        let needed = 2
            * (fs::metadata(&path).unwrap().len()
                + fs::metadata(directory.path().join("collection.anki2-wal"))
                    .unwrap()
                    .len())
            + 4 * 1024 * 1024;
        let space = Capacity {
            full: if full_staging { staging.path() } else { output.path() },
            visits: Mutex::new(Vec::new()),
        };
        assert_eq!(
            write_with(
                &reader,
                &destination,
                staging.path(),
                &space,
                Finalisation::Replace,
                &|| false
            ),
            Err(Error::InsufficientSpace { needed, available: 0 })
        );
        let visits = space.visits.lock().unwrap();
        let mut expected = vec![staging.path().to_owned()];
        if !full_staging {
            expected.push(output.path().to_owned());
        }
        assert_eq!(*visits, expected);
        assert!(listing(staging.path()).is_empty());
        assert_eq!(
            listing(output.path()),
            vec![std::ffi::OsString::from("existing.colpkg")]
        );
        assert_eq!(fs::read(destination).unwrap(), SENTINEL);
        assert_eq!(listing(directory.path()), source_listing);
        assert_preserved(&original);
        drop(reader);
        collection.close().unwrap();
    }
}

struct NoPreflight;
impl SpaceProvider for NoPreflight {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        panic!("refusal must precede capacity checks and staging");
    }
}

#[test]
fn live_destination_collision_refuses_before_preflight_and_preserves_both_files() {
    let (directory, _, collection) = live();
    add_note(&collection, NOW, "keep this WAL");
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let destination = directory.path().join("existing.colpkg");
    fs::write(&destination, SENTINEL).unwrap();
    let before = listing(directory.path());
    let original = source_bytes(directory.path());
    assert_eq!(
        write_with(
            &reader,
            &destination,
            directory.path(),
            &NoPreflight,
            Finalisation::ExclusiveCreate,
            &|| false
        ),
        Err(Error::DestinationExists)
    );
    assert_eq!(fs::read(destination).unwrap(), SENTINEL);
    assert_eq!(listing(directory.path()), before);
    assert_preserved(&original);
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn live_destination_aliases_and_sqlite_state_paths_refuse_before_staging() {
    use std::os::unix::fs::symlink;
    let (directory, source, collection) = live();
    add_note(&collection, NOW, "keep collection and WAL bytes");
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let symlink_path = directory.path().join("linked.colpkg");
    let hardlink_path = directory.path().join("hardlinked.colpkg");
    symlink(&source, &symlink_path).unwrap();
    fs::hard_link(&source, &hardlink_path).unwrap();
    let original = source_bytes(directory.path());
    let before = listing(directory.path());
    for destination in [
        source,
        symlink_path,
        hardlink_path,
        directory.path().join("collection.anki2-wal"),
        directory.path().join("collection.anki2-shm"),
        directory.path().join("collection.anki2-journal"),
    ] {
        assert!(matches!(
            write_with(
                &reader,
                &destination,
                directory.path(),
                &NoPreflight,
                Finalisation::Replace,
                &|| false
            ),
            Err(Error::Io { .. })
        ));
        assert_eq!(listing(directory.path()), before);
        assert_preserved(&original);
    }
    drop(reader);
    collection.close().unwrap();
}

#[test]
fn live_cancel_at_entry_snapshot_streaming_and_verification_preserves_source_and_output() {
    for cancel_at in [0, 2, 3, 6, 12] {
        let (directory, _, collection) = live();
        add_note(&collection, NOW, "committed WAL survives cancellation");
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let destination = directory.path().join("cancel.colpkg");
        let exists = cancel_at % 2 == 0;
        if exists {
            fs::write(&destination, SENTINEL).unwrap();
        }
        let before = listing(directory.path());
        let original = source_bytes(directory.path());
        let calls = AtomicUsize::new(0);
        let cancel = || calls.fetch_add(1, Ordering::SeqCst) >= cancel_at;
        assert_eq!(
            write_with(
                &reader,
                &destination,
                directory.path(),
                &StatvfsSpace,
                Finalisation::Replace,
                &cancel
            ),
            Err(Error::Cancelled)
        );
        if exists {
            assert_eq!(fs::read(&destination).unwrap(), SENTINEL);
        } else {
            assert!(!destination.exists());
        }
        assert_eq!(listing(directory.path()), before);
        assert_preserved(&original);
        let later = add_note(&collection, NOW + 1000, "still writable after cancellation");
        assert!(reader.note(later).unwrap().is_some());
        drop(reader);
        collection.close().unwrap();
    }
}

#[test]
fn live_writer_requires_maintenance_outside_an_existing_read_transaction_before_staging() {
    let (directory, _, collection) = live();
    let destination = directory.path().join("not-created.colpkg");
    let before = listing(directory.path());
    for role in ReaderRole::ALL {
        if role == ReaderRole::Maintenance {
            continue;
        }
        let reader = collection.open_reader(role).unwrap();
        assert!(matches!(
            write_with(
                &reader,
                &destination,
                directory.path(),
                &NoPreflight,
                Finalisation::Replace,
                &|| false
            ),
            Err(Error::Io { .. })
        ));
    }
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let transaction = reader.begin_snapshot().unwrap();
    assert!(matches!(
        write_with(
            &reader,
            &destination,
            directory.path(),
            &NoPreflight,
            Finalisation::Replace,
            &|| false
        ),
        Err(Error::Io { .. })
    ));
    assert!(transaction.col().is_ok());
    assert_eq!(listing(directory.path()), before);
    drop(transaction);
    drop(reader);
    collection.close().unwrap();
}
