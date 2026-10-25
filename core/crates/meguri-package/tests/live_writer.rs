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

// E7/P15 has an explicit opt-in seam. All tests above keep exercising P11.
fn media_write(
    reader: &Reader,
    destination: &Path,
    directory: &Path,
    space: &dyn SpaceProvider,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<meguri_package::MediaCollectionExportWritten, Error> {
    let context = CollectionPackageWriteContext {
        collection_directory: directory,
        purpose: Purpose::Export,
        finalisation: Finalisation::Replace,
        clock: &clock(NOW),
        random: &SeededRandom::new(7),
        space,
        is_cancelled: cancel,
    };
    let plan = meguri_package::preflight_media_collection_export_from_live(reader, destination, &context)?;
    meguri_package::write_media_collection_export_from_live(reader, destination, &context, plan)
}
fn sha1_file(path: &Path) -> (u64, [u8; 20]) {
    use sha1::{Digest, Sha1};
    let mut input = File::open(path).unwrap();
    let mut hash = Sha1::new();
    let mut length = 0;
    let mut chunk = [0; 64 * 1024];
    loop {
        let n = input.read(&mut chunk).unwrap();
        if n == 0 {
            break;
        }
        hash.update(&chunk[..n]);
        length += n as u64;
    }
    (length, hash.finalize().into())
}
// Independent decoder for the writer's canonical protobuf subset. It deliberately
// does not call the production map parser, package reader, or publication verifier.
fn map_number(bytes: &mut &[u8]) -> u64 {
    let mut value = 0;
    for shift in (0..70).step_by(7) {
        let b = bytes[0];
        *bytes = &bytes[1..];
        value |= u64::from(b & 127) << shift;
        if b < 128 {
            return value;
        }
    }
    panic!("bad generated varint")
}
fn map_bytes<'a>(bytes: &mut &'a [u8]) -> &'a [u8] {
    let n = map_number(bytes) as usize;
    let (payload, rest) = bytes.split_at(n);
    *bytes = rest;
    payload
}
fn decoded_map(mut bytes: &[u8]) -> Vec<(String, u64, [u8; 20])> {
    let mut records = Vec::new();
    while !bytes.is_empty() {
        assert_eq!(map_number(&mut bytes), 10);
        let mut record = map_bytes(&mut bytes);
        assert_eq!(map_number(&mut record), 10);
        let name = String::from_utf8(map_bytes(&mut record).to_vec()).unwrap();
        assert_eq!(map_number(&mut record), 16);
        let length = map_number(&mut record);
        assert_eq!(map_number(&mut record), 26);
        let hash = map_bytes(&mut record).try_into().unwrap();
        assert!(record.is_empty(), "no field 255 or invented metadata");
        records.push((name, length, hash));
    }
    records
}
// Capture the real generated journal, including presence/absence and every
// SQLite sidecar. Expected bytes come from the source before any export.
fn media_journal_bytes(directory: &Path) -> Vec<(String, Option<Vec<u8>>)> {
    [
        "meguri-media.db",
        "meguri-media.db-wal",
        "meguri-media.db-shm",
        "meguri-media.db-journal",
    ]
    .into_iter()
    .map(|name| {
        let bytes = match fs::read(directory.join(name)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("generated journal read failed: {error}"),
        };
        (name.to_owned(), bytes)
    })
    .collect()
}
#[test]
fn user_export_includes_direct_unregistered_empty_large_unicode_and_case_variant_media() {
    use sha1::{Digest, Sha1};
    let (directory, _, collection) = live();
    let media = directory.path().join("collection.media");
    fs::create_dir(&media).unwrap();
    let note = add_note(&collection, NOW, "<img src=\"referenced.png\">");
    for (name, bytes) in [
        ("referenced.png", b"referenced".as_slice()),
        ("unused.bin", b"unused"),
        ("_static.css", b"static"),
        ("empty", b""),
        ("日本.mp3", b"Unicode"),
        ("Case.png", b"upper"),
        ("case.png", b"lower"),
    ] {
        fs::write(media.join(name), bytes).unwrap();
    }
    // This part requires a case-sensitive volume; Apple test infrastructure may
    // provide one, otherwise test the rest without pretending two entries exist.
    let case_distinct = fs::read(media.join("Case.png")).unwrap() == b"upper";
    let large = media.join("large.bin");
    File::create(&large).unwrap().set_len(104_857_601).unwrap();
    fs::write(media.join("Thumbs.db"), b"OS metadata").unwrap();
    fs::create_dir(media.join("child")).unwrap();
    fs::write(media.join("child/not-exported"), b"no recursion").unwrap();
    let store = meguri_storage::media::MediaStore::open(
        &directory.path().join("collection.anki2"),
        std::sync::Arc::new(clock(NOW)),
    )
    .unwrap();
    assert_eq!(
        store
            .add("journal-pending.png", b"generated pending attachment")
            .unwrap(),
        "journal-pending.png"
    );
    let pending = store.pending().unwrap();
    assert!(
        pending
            .iter()
            .any(|item| item.name == "journal-pending.png" && item.sha1.is_some())
    );
    let counter = store.change_counter().unwrap();
    assert!(counter > 0);
    let diagnostics = store.diagnostics();
    let journal = media_journal_bytes(directory.path());
    let names = listing(&media);
    let mut expected: Vec<_> = names
        .iter()
        .filter_map(|name| {
            let path = media.join(name);
            (path.is_file() && name != "Thumbs.db").then(|| {
                let (length, hash) = sha1_file(&path);
                (name.to_str().unwrap().to_owned(), length, hash)
            })
        })
        .collect();
    expected.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    if case_distinct {
        assert!(expected.iter().any(|e| e.0 == "Case.png") && expected.iter().any(|e| e.0 == "case.png"));
    }
    let source = source_bytes(directory.path());
    let maintenance = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let output = directory.path().join("user.colpkg");
    let written = media_write(&maintenance, &output, directory.path(), &StatvfsSpace, &|| false).unwrap();
    written.source_validation.validate(&|| false).unwrap();
    let mut zip = zip::ZipArchive::new(File::open(&output).unwrap()).unwrap();
    assert_eq!(zip.len(), expected.len() + 4);
    let map = zstd::stream::decode_all(zip.by_name("media").unwrap()).unwrap();
    assert_eq!(decoded_map(&map), expected);
    for (index, (_, length, hash)) in expected.iter().enumerate() {
        let entry = zip.by_index(index + 3).unwrap();
        assert_eq!(entry.name().unwrap().as_ref(), index.to_string());
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut decoder = zstd::stream::read::Decoder::new(entry).unwrap();
        let mut digest = Sha1::new();
        let mut size = 0;
        let mut chunk = [0; 64 * 1024];
        loop {
            let n = decoder.read(&mut chunk).unwrap();
            if n == 0 {
                break;
            }
            digest.update(&chunk[..n]);
            size += n as u64;
        }
        assert_eq!(size, *length);
        let actual: [u8; 20] = digest.finalize().into();
        assert_eq!(actual, *hash);
    }
    let captured = directory.path().join("decoded-snapshot.anki2");
    let mut file = File::create(&captured).unwrap();
    std::io::copy(
        &mut zstd::stream::read::Decoder::new(zip.by_name("collection.anki21b").unwrap()).unwrap(),
        &mut file,
    )
    .unwrap();
    drop(file);
    let saved = open_collection(&captured, &OpenOptions::new(&clock(NOW), &SeededRandom::new(7))).unwrap();
    assert!(saved.writer().note(note).unwrap().is_some());
    assert_eq!(
        saved.writer().col().unwrap().modified,
        written.written.snapshot_modified
    );
    drop(saved);
    drop(zip);
    let inspection_context = meguri_package::CollectionPackageInspectContext {
        collection_directory: directory.path(),
        live_collection_size: 0,
        reserved_space_bytes: 0,
        clock: &clock(NOW),
        random: &SeededRandom::new(7),
        space: &StatvfsSpace,
        is_cancelled: &|| false,
    };
    assert!(matches!(
        meguri_package::inspect_collection_package(&output, &inspection_context),
        Err(Error::UnsupportedMedia)
    ));
    assert_preserved(&source);
    assert_eq!(listing(&media), names);
    for (name, length, hash) in &expected {
        assert_eq!(sha1_file(&media.join(name)), (*length, *hash));
    }
    assert_eq!(media_journal_bytes(directory.path()), journal);
    assert_eq!(store.pending().unwrap(), pending);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(store.diagnostics(), diagnostics);
    // The same real journal and independent pending facts must survive refusal.
    fs::write(media.join(":.DS_Store"), b"unsafe colon name").unwrap();
    let refused_names = listing(&media);
    let refused_output = directory.path().join("refused.colpkg");
    assert!(matches!(
        media_write(&maintenance, &refused_output, directory.path(), &StatvfsSpace, &|| {
            false
        }),
        Err(Error::ExportSourceInvalid { .. })
    ));
    assert!(!refused_output.exists());
    assert_eq!(listing(&media), refused_names);
    for (name, length, hash) in &expected {
        assert_eq!(sha1_file(&media.join(name)), (*length, *hash));
    }
    assert_preserved(&source);
    assert_eq!(media_journal_bytes(directory.path()), journal);
    assert_eq!(store.pending().unwrap(), pending);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(store.diagnostics(), diagnostics);
}

static MEDIA_TEST_SERIAL: Mutex<()> = Mutex::new(());
thread_local! { static MEDIA_ACTION: std::cell::RefCell<Option<Box<dyn FnOnce()>>> = const { std::cell::RefCell::new(None) }; }
fn media_action() {
    MEDIA_ACTION.with(|action| {
        if let Some(action) = action.borrow_mut().take() {
            action();
        }
    });
}
struct MediaHook(&'static str);
impl Drop for MediaHook {
    fn drop(&mut self) {
        meguri_model::faultpoint::disarm(self.0);
        MEDIA_ACTION.with(|a| {
            a.borrow_mut().take();
        });
    }
}
fn media_hook(point: &'static str, action: impl FnOnce() + 'static) -> MediaHook {
    MEDIA_ACTION.with(|a| *a.borrow_mut() = Some(Box::new(action)));
    meguri_model::faultpoint::arm(point, meguri_model::faultpoint::Fault::Callback(media_action));
    MediaHook(point)
}
#[test]
fn media_capture_rejects_same_length_rewrite_replacement_root_and_inventory_changes() {
    let _serial = MEDIA_TEST_SERIAL.lock().unwrap();
    for (point, change) in [
        ("package.export.capture_chunk", 0),
        ("package.export.capture_chunk", 1),
        ("package.export.capture_chunk", 2),
        ("package.export.before_verification", 3),
        ("package.export.before_verification", 4),
        ("package.export.before_verification", 5),
    ] {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        let path = media.join("a");
        fs::write(&path, [1; 128 * 1024]).unwrap();
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let destination = directory.path().join("user.colpkg");
        fs::write(&destination, SENTINEL).unwrap();
        let mutate = path.clone();
        let root = media.clone();
        let _hook = media_hook(point, move || match change {
            0 => fs::write(mutate, [2; 128 * 1024]).unwrap(),
            1 => {
                fs::rename(&mutate, root.join("old")).unwrap();
                fs::write(mutate, [1; 128 * 1024]).unwrap();
            }
            2 => {
                fs::rename(&root, root.with_extension("old")).unwrap();
                fs::create_dir(root).unwrap();
            }
            3 => fs::write(root.join("added"), b"new").unwrap(),
            4 => fs::remove_file(mutate).unwrap(),
            _ => fs::rename(mutate, root.join("renamed")).unwrap(),
        });
        assert!(media_write(&reader, &destination, directory.path(), &StatvfsSpace, &|| false).is_err());
        assert_eq!(fs::read(&destination).unwrap(), SENTINEL);
        assert!(
            !listing(directory.path())
                .iter()
                .any(|n| n.to_string_lossy().starts_with(".meguri-"))
        );
    }
}
#[test]
fn source_proof_detects_late_changes_and_initially_absent_root_becoming_present() {
    for present in [false, true] {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        if present {
            fs::create_dir(&media).unwrap();
            fs::write(media.join("a"), b"before").unwrap();
        }
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let written = media_write(
            &reader,
            &directory.path().join("out"),
            directory.path(),
            &StatvfsSpace,
            &|| false,
        )
        .unwrap();
        if present {
            fs::write(media.join("a"), b"AFTER!").unwrap();
        } else {
            fs::create_dir(&media).unwrap();
        }
        assert!(written.source_validation.validate(&|| false).is_err());
    }
}
#[test]
fn media_preflight_refuses_links_unsafe_names_invalid_utf8_and_sparse_uint32_overflow_without_writes() {
    use std::os::unix::{ffi::OsStrExt, fs::symlink};
    for case in 0..10 {
        let (directory, source, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        match case {
            0 => symlink(&source, media.join("link")).unwrap(),
            1 => fs::hard_link(&source, media.join("alias")).unwrap(),
            2 => fs::write(media.join("con.jpg"), b"unsafe").unwrap(),
            3 => fs::write(media.join(std::ffi::OsStr::from_bytes(b"bad\xff")), b"bad").unwrap(),
            4 => {
                File::create(media.join("oversized"))
                    .unwrap()
                    .set_len(u64::from(u32::MAX) + 1)
                    .unwrap();
            }
            5 => {
                fs::remove_dir(&media).unwrap();
                symlink(directory.path(), &media).unwrap();
            }
            6 => {
                fs::remove_dir(&media).unwrap();
                fs::write(&media, b"not a directory").unwrap();
            }
            7 => {
                let wal = directory.path().join("collection.anki2-wal");
                fs::hard_link(wal, media.join("aux-alias")).unwrap();
            }
            8 => fs::write(media.join(":.DS_Store"), b"unsafe colon name").unwrap(),
            _ => fs::write(media.join(":xds_store"), b"unsafe colon name").unwrap(),
        }
        let before = listing(directory.path());
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let output = directory.path().join("out");
        assert!(media_write(&reader, &output, directory.path(), &StatvfsSpace, &|| false).is_err());
        assert_eq!(listing(directory.path()), before);
        assert!(!output.exists());
    }
}
#[test]
fn existing_internal_staging_and_output_aliases_refuse_without_mutating_them() {
    use std::os::unix::fs::symlink;
    for case in 0..5 {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        let stage = directory.path().join(".meguri-stage-existing");
        fs::create_dir(&stage).unwrap();
        let snapshot = stage.join("snapshot.anki2");
        fs::write(&snapshot, b"existing unpublished snapshot").unwrap();
        let output = directory.path().join("out.colpkg");
        match case {
            0 => fs::hard_link(&snapshot, media.join("safe-snapshot.bin")).unwrap(),
            1 => {
                fs::write(&output, b"existing output").unwrap();
                fs::hard_link(&output, media.join("safe-output.bin")).unwrap();
            }
            2 => {
                fs::rename(&stage, directory.path().join("foreign")).unwrap();
                symlink(directory.path().join("foreign"), &stage).unwrap();
            }
            3 => {
                fs::remove_file(&snapshot).unwrap();
                symlink(directory.path().join("collection.anki2"), &snapshot).unwrap();
            }
            _ => {
                let temporary = directory.path().join(".meguri-tmp-export-existing");
                fs::write(&temporary, b"unpublished output").unwrap();
                fs::hard_link(temporary, media.join("safe-temp.bin")).unwrap();
            }
        }
        let root_names = listing(directory.path());
        let media_names = listing(&media);
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        assert!(matches!(
            media_write(&reader, &output, directory.path(), &StatvfsSpace, &|| false),
            Err(Error::ExportSourceInvalid { .. })
        ));
        assert_eq!(listing(directory.path()), root_names);
        assert_eq!(listing(&media), media_names);
        if case != 2 && case != 3 {
            assert_eq!(fs::read(&snapshot).unwrap(), b"existing unpublished snapshot");
        }
        if case == 1 {
            assert_eq!(fs::read(&output).unwrap(), b"existing output");
        }
        if case == 2 {
            assert!(fs::symlink_metadata(stage).unwrap().file_type().is_symlink());
        }
        if case == 3 {
            assert!(fs::symlink_metadata(snapshot).unwrap().file_type().is_symlink());
        }
    }
}
#[test]
fn pending_media_namespace_aliases_refuse_before_export_writes() {
    use std::os::unix::fs::symlink;
    for case in 0..4 {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        let pending = directory.path().join("media.staging");
        fs::create_dir(&media).unwrap();
        fs::create_dir(&pending).unwrap();
        let staged = pending.join("pending-file");
        fs::write(&staged, b"pending operation bytes").unwrap();
        fs::write(media.join("ordinary.bin"), b"live media bytes").unwrap();
        match case {
            0 => fs::hard_link(&staged, media.join("safe-pending.bin")).unwrap(),
            1 => {
                fs::rename(&pending, directory.path().join("retained-pending")).unwrap();
                symlink(directory.path().join("retained-pending"), &pending).unwrap();
            }
            2 => {
                fs::rename(&pending, directory.path().join("retained-pending")).unwrap();
                fs::write(&pending, b"occupied namespace").unwrap();
            }
            _ => symlink(media.join("ordinary.bin"), pending.join("linked-file")).unwrap(),
        }
        let root_names = listing(directory.path());
        let media_names = listing(&media);
        let retained = if case == 1 || case == 2 {
            directory.path().join("retained-pending")
        } else {
            pending.clone()
        };
        let pending_names = listing(&retained);
        let original_files = source_bytes(directory.path());
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let destination = directory.path().join("out.colpkg");
        assert!(matches!(
            media_write(&reader, &destination, directory.path(), &StatvfsSpace, &|| false),
            Err(Error::ExportSourceInvalid { .. })
        ));
        assert!(!destination.exists());
        assert_eq!(listing(directory.path()), root_names);
        assert_eq!(listing(&media), media_names);
        assert_eq!(listing(&retained), pending_names);
        assert_eq!(
            fs::read(retained.join("pending-file")).unwrap(),
            b"pending operation bytes"
        );
        assert_eq!(fs::read(media.join("ordinary.bin")).unwrap(), b"live media bytes");
        if case == 0 {
            assert_eq!(
                fs::read(media.join("safe-pending.bin")).unwrap(),
                b"pending operation bytes"
            );
        }
        if case == 1 {
            assert!(fs::symlink_metadata(&pending).unwrap().file_type().is_symlink());
        }
        if case == 2 {
            assert_eq!(fs::read(&pending).unwrap(), b"occupied namespace");
        }
        if case == 3 {
            assert!(
                fs::symlink_metadata(pending.join("linked-file"))
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
        }
        assert_preserved(&original_files);
    }
}
#[test]
fn ignored_looking_symlinks_and_nonregular_entries_still_refuse() {
    use std::{
        ffi::CString,
        os::unix::{ffi::OsStrExt, fs::symlink},
    };
    for link in [false, true] {
        let (directory, source, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        let path = media.join(".DS_Store");
        if link {
            symlink(&source, &path).unwrap();
        } else {
            let name = CString::new(path.as_os_str().as_bytes()).unwrap();
            // SAFETY: terminated path and fixed permissions; no pointer retained.
            assert_eq!(unsafe { libc::mkfifo(name.as_ptr(), 0o600) }, 0);
        }
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        assert!(matches!(
            media_write(
                &reader,
                &directory.path().join("out"),
                directory.path(),
                &StatvfsSpace,
                &|| false
            ),
            Err(Error::ExportSourceInvalid { .. })
        ));
        assert!(fs::symlink_metadata(&path).is_ok());
    }
}
#[test]
#[cfg(not(target_vendor = "apple"))]
fn non_apple_export_refuses_non_nfc_without_renaming() {
    let (directory, _, collection) = live();
    let media = directory.path().join("collection.media");
    fs::create_dir(&media).unwrap();
    fs::write(media.join("e\u{301}.png"), b"decomposed").unwrap();
    let before = listing(&media);
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    assert!(
        media_write(
            &reader,
            &directory.path().join("out"),
            directory.path(),
            &StatvfsSpace,
            &|| false
        )
        .is_err()
    );
    assert_eq!(listing(&media), before);
}
#[test]
fn media_preflight_queries_shared_device_for_combined_concurrent_cost_before_writes() {
    struct SharedVolume {
        root: PathBuf,
        queries: Mutex<Vec<PathBuf>>,
    }
    impl SpaceProvider for SharedVolume {
        fn available_bytes(&self, requested: &Path) -> Result<u64, StorageError> {
            let names = listing(&self.root);
            assert!(!self.root.join("out").exists());
            assert!(names.iter().all(|name| !name.to_string_lossy().starts_with(".meguri-")));
            self.queries.lock().unwrap().push(requested.to_path_buf());
            Ok(12 * 1024 * 1024)
        }
    }

    let (fixture, _, opened) = live();
    let media_folder = fixture.path().join("collection.media");
    fs::create_dir(&media_folder).unwrap();
    let source_file = File::create(media_folder.join("file")).unwrap();
    source_file.set_len(8 * 1024 * 1024).unwrap();
    let volume = SharedVolume {
        root: fixture.path().to_path_buf(),
        queries: Mutex::new(Vec::new()),
    };
    let maintenance = opened.open_reader(ReaderRole::Maintenance).unwrap();
    let result = media_write(
        &maintenance,
        &fixture.path().join("out"),
        fixture.path(),
        &volume,
        &|| false,
    );
    // The shared volume must fit the capture and archive at the same time.
    match result {
        Err(Error::InsufficientSpace { needed, .. }) => assert!(needed > 16 * 1024 * 1024),
        _ => panic!("shared-volume preflight must report insufficient capacity"),
    }
    assert_eq!(volume.queries.lock().unwrap().len(), 2);
}

#[test]
fn cancellation_in_media_capture_compression_and_verification_preserves_destination() {
    let _serial = MEDIA_TEST_SERIAL.lock().unwrap();
    use std::sync::{Arc, atomic::AtomicBool};
    for point in [
        "package.export.capture_chunk",
        "package.export.compress_chunk",
        "package.export.verify_chunk",
    ] {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        fs::write(media.join("a"), [1; 128 * 1024]).unwrap();
        let before = sha1_file(&media.join("a"));
        let cancelled = Arc::new(AtomicBool::new(false));
        let token = cancelled.clone();
        let _hook = media_hook(point, move || {
            token.store(true, Ordering::SeqCst);
        });
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let destination = directory.path().join("out");
        fs::write(&destination, SENTINEL).unwrap();
        assert!(matches!(
            media_write(&reader, &destination, directory.path(), &StatvfsSpace, &|| cancelled
                .load(Ordering::SeqCst)),
            Err(Error::Cancelled)
        ));
        assert_eq!(fs::read(&destination).unwrap(), SENTINEL);
        assert_eq!(sha1_file(&media.join("a")), before);
        assert!(
            !listing(directory.path())
                .iter()
                .any(|n| n.to_string_lossy().starts_with(".meguri-"))
        );
    }
}

#[test]
fn reviews_can_commit_while_media_copy_and_compression_are_held() {
    let _serial = MEDIA_TEST_SERIAL.lock().unwrap();
    for point in ["package.export.capture_chunk", "package.export.compress_chunk"] {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        fs::write(media.join("a"), [1; 128 * 1024]).unwrap();
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let (entered, wait) = mpsc::channel();
        let (release, released) = mpsc::channel();
        let output = directory.path().join("out");
        std::thread::scope(|scope| {
            let root = directory.path();
            let task = scope.spawn(move || {
                let _hook = media_hook(point, move || {
                    entered.send(()).unwrap();
                    released.recv_timeout(Duration::from_secs(30)).unwrap();
                });
                media_write(&reader, &output, root, &StatvfsSpace, &|| false).unwrap()
            });
            wait.recv_timeout(Duration::from_secs(30)).unwrap();
            // A real storage transaction on the original writer completes
            // before the held media work is allowed to proceed.
            let note = add_note(&collection, NOW + 1000, "commit while media is held");
            assert!(collection.writer().note(note).unwrap().is_some());
            release.send(()).unwrap();
            task.join().unwrap();
        });
    }
}

#[test]
#[cfg(target_vendor = "apple")]
fn apple_export_normalizes_only_archive_name_and_refuses_representable_nfc_collisions() {
    let (directory, _, collection) = live();
    let media = directory.path().join("collection.media");
    fs::create_dir(&media).unwrap();
    let decomposed = "e\u{301}.png";
    fs::write(media.join(decomposed), b"bytes").unwrap();
    let original = listing(&media);
    let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
    let path = directory.path().join("out");
    media_write(&reader, &path, directory.path(), &StatvfsSpace, &|| false).unwrap();
    let mut zip = zip::ZipArchive::new(File::open(&path).unwrap()).unwrap();
    let map = decoded_map(&zstd::stream::decode_all(zip.by_name("media").unwrap()).unwrap());
    assert_eq!(map[0].0, "é.png");
    assert_eq!(listing(&media), original);
    drop(zip);
    // Normalization-insensitive Apple volumes cannot represent this collision.
    fs::write(media.join("é.png"), b"other").unwrap();
    if listing(&media).len() == 2 {
        assert!(media_write(&reader, &path, directory.path(), &StatvfsSpace, &|| false).is_err());
    }
}

#[test]
fn media_disk_exhaustion_is_disk_full_with_destination_and_source_preserved() {
    let _serial = MEDIA_TEST_SERIAL.lock().unwrap();
    for point in [
        "package.export.capture_chunk",
        "package.export.compress_chunk",
        "package.export.verify_chunk",
    ] {
        let (directory, _, collection) = live();
        let media = directory.path().join("collection.media");
        fs::create_dir(&media).unwrap();
        fs::write(media.join("a"), [1; 128 * 1024]).unwrap();
        let before = sha1_file(&media.join("a"));
        let reader = collection.open_reader(ReaderRole::Maintenance).unwrap();
        let destination = directory.path().join("out");
        fs::write(&destination, SENTINEL).unwrap();
        meguri_model::faultpoint::arm(
            point,
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::StorageFull),
        );
        let _hook = MediaHook(point);
        assert!(matches!(
            media_write(&reader, &destination, directory.path(), &StatvfsSpace, &|| false),
            Err(Error::DiskFull)
        ));
        assert_eq!(fs::read(&destination).unwrap(), SENTINEL);
        assert_eq!(sha1_file(&media.join("a")), before);
        assert!(
            !listing(directory.path())
                .iter()
                .any(|n| n.to_string_lossy().starts_with(".meguri-"))
        );
    }
}
