//! Collection-only writer contract from package/container P11.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_package::{CollectionPackageWriteContext, Finalisation, write_collection_package_from_standalone};
use meguri_storage::atomic_file::Purpose;
use meguri_storage::space::StatvfsSpace;

#[test]
fn rf2_collection_writer_produces_latest_four_entry_package() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2");
    let fixture_bytes = fs::read(&fixture).expect("read the public schema-18 fixture");
    assert!(fixture_bytes.len() >= 100);
    assert_eq!(&fixture_bytes[..16], b"SQLite format 3\0");
    assert_eq!(&fixture_bytes[18..20], &[2, 2]);

    let directory = tempfile::tempdir().expect("writable operation directory");
    let source = directory.path().join("collection.anki2");
    fs::copy(&fixture, &source).expect("copy the public fixture without opening SQLite");
    let original_source = fs::read(&source).expect("read the actual writer input");
    assert_eq!(original_source, fixture_bytes);
    // Establish the intended source header before it reaches the production API.
    assert_eq!(&original_source[..16], b"SQLite format 3\0");
    assert_eq!(&original_source[18..20], &[2, 2]);
    assert!(!directory.path().join("collection.anki2-wal").exists());
    assert!(!directory.path().join("collection.anki2-journal").exists());

    let destination = directory.path().join("latest.colpkg");
    let clock = FixedClock::new(1_894_701_600_000, UtcOffset::UTC);
    let random = SeededRandom::new(42);
    let space = StatvfsSpace;
    let is_cancelled = || false;
    let context = CollectionPackageWriteContext {
        collection_directory: directory.path(),
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: &clock,
        random: &random,
        space: &space,
        is_cancelled: &is_cancelled,
    };

    let result = write_collection_package_from_standalone(&source, &destination, &context);
    // Source preservation also applies when the writer returns an error.
    assert_eq!(fs::read(&source).expect("source remains readable"), original_source);
    assert_eq!(fs::read(&fixture).expect("fixture remains readable"), fixture_bytes);
    result.expect("P11 must publish a latest collection-only package from valid input");
    assert!(destination.is_file(), "success must publish the requested .colpkg");

    // Read with zip and zstd directly, independently of production verification.
    let mut archive = zip::ZipArchive::new(File::open(&destination).expect("published package"))
        .expect("published output is a zip archive");
    let expected_names = ["meta", "collection.anki21b", "collection.anki2", "media"];
    assert_eq!(archive.len(), 4, "P11 specifies exactly four entries");
    let mut payloads = Vec::new();
    for (index, expected_name) in expected_names.into_iter().enumerate() {
        let mut entry = archive.by_index(index).expect("read entry in archive order");
        assert_eq!(entry.name().expect("UTF-8 entry name").into_owned(), expected_name);
        assert_eq!(entry.compression(), zip::CompressionMethod::Stored);
        let mut payload = Vec::new();
        entry
            .read_to_end(&mut payload)
            .expect("read complete entry and verify its CRC");
        payloads.push(payload);
    }

    assert_eq!(payloads[0], [0x08, 0x03], "exact latest-format meta bytes");
    let collection = zstd::stream::decode_all(payloads[1].as_slice()).expect("zstd collection");
    assert!(collection.len() >= 100, "complete SQLite header");
    assert_eq!(&collection[..16], b"SQLite format 3\0");
    assert_eq!(collection[18], 2, "snapshot write version is WAL");
    assert_eq!(collection[19], 2, "snapshot read version is WAL");
    // A VACUUM snapshot need not be byte-identical to the source database.

    let placeholder = &payloads[2];
    assert!(placeholder.len() >= 100, "complete raw SQLite placeholder header");
    assert_eq!(&placeholder[..16], b"SQLite format 3\0");
    assert_eq!(placeholder[18], 1, "placeholder write version is rollback");
    assert_eq!(placeholder[19], 1, "placeholder read version is rollback");

    let media = zstd::stream::decode_all(payloads[3].as_slice()).expect("zstd empty media map");
    assert!(media.is_empty(), "collection-only media decodes to zero bytes");
    assert_eq!(
        fs::read(&source).expect("source after archive inspection"),
        original_source
    );
}

use meguri_package::CollectionPackageWriteError as Error;
use meguri_storage::{StorageError, space::SpaceProvider};

fn source_in(directory: &Path) -> std::path::PathBuf {
    let source = directory.join("source.anki2");
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &source,
    )
    .unwrap();
    // A device-only sidecar must never be consumed or changed by this API.
    fs::write(directory.join("meguri-sidecar.json"), b"device state").unwrap();
    source
}
fn listing(directory: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    names.sort();
    names
}
fn write_with(
    source: &Path,
    destination: &Path,
    staging: &Path,
    space: &dyn SpaceProvider,
    finalisation: Finalisation,
    cancel: &(dyn Fn() -> bool + Sync),
) -> Result<(), Error> {
    write_collection_package_from_standalone(
        source,
        destination,
        &CollectionPackageWriteContext {
            collection_directory: staging,
            purpose: Purpose::Export,
            finalisation,
            clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
            random: &SeededRandom::new(7),
            space,
            is_cancelled: cancel,
        },
    )
}

#[test]
fn rejects_empty_wal_and_journal_files_and_preserves_existing_files() {
    for suffix in ["-wal", "-journal"] {
        let directory = tempfile::tempdir().unwrap();
        let source = source_in(directory.path());
        let original = fs::read(&source).unwrap();
        let auxiliary_path = directory.path().join(format!("source.anki2{suffix}"));
        fs::write(&auxiliary_path, []).unwrap();
        let destination = directory.path().join("old.colpkg");
        fs::write(&destination, b"old package").unwrap();
        let before = listing(directory.path());
        assert_eq!(
            write_with(
                &source,
                &destination,
                directory.path(),
                &StatvfsSpace,
                Finalisation::Replace,
                &|| false
            ),
            Err(Error::SourceNotFinalised)
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(fs::read(auxiliary_path).unwrap(), b"");
        assert_eq!(fs::read(destination).unwrap(), b"old package");
        assert_eq!(listing(directory.path()), before);
    }
}

#[test]
fn preflights_both_directories_before_any_staging_write() {
    struct Capacity<'a> {
        full_directory: &'a Path,
    }
    impl SpaceProvider for Capacity<'_> {
        fn available_bytes(&self, path: &Path) -> Result<u64, StorageError> {
            Ok(if path == self.full_directory { 0 } else { u64::MAX })
        }
    }
    for full_staging in [true, false] {
        let staging = tempfile::tempdir().unwrap();
        let output = tempfile::tempdir().unwrap();
        let source = source_in(staging.path());
        let original = fs::read(&source).unwrap();
        let destination = output.path().join("result.colpkg");
        let before = listing(staging.path());
        let space = Capacity {
            full_directory: if full_staging { staging.path() } else { output.path() },
        };
        assert_eq!(
            write_with(
                &source,
                &destination,
                staging.path(),
                &space,
                Finalisation::Replace,
                &|| false
            ),
            Err(Error::InsufficientSpace {
                needed: 2 * original.len() as u64 + 4 * 1024 * 1024,
                available: 0
            })
        );
        assert!(!destination.exists());
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(listing(staging.path()), before);
        assert!(listing(output.path()).is_empty());
    }
}

#[test]
fn exclusive_collision_preserves_existing_output_and_replace_publishes() {
    let directory = tempfile::tempdir().unwrap();
    let source = source_in(directory.path());
    let original = fs::read(&source).unwrap();
    let destination = directory.path().join("result.colpkg");
    fs::write(&destination, b"existing package").unwrap();
    let before = listing(directory.path());
    assert_eq!(
        write_with(
            &source,
            &destination,
            directory.path(),
            &StatvfsSpace,
            Finalisation::ExclusiveCreate,
            &|| false
        ),
        Err(Error::DestinationExists)
    );
    assert_eq!(fs::read(&destination).unwrap(), b"existing package");
    assert_eq!(listing(directory.path()), before);
    write_with(
        &source,
        &destination,
        directory.path(),
        &StatvfsSpace,
        Finalisation::Replace,
        &|| false,
    )
    .unwrap();
    let archive = zip::ZipArchive::new(File::open(&destination).unwrap()).unwrap();
    assert_eq!(archive.len(), 4);
    assert_eq!(fs::read(source).unwrap(), original);
    assert_eq!(
        fs::read(directory.path().join("meguri-sidecar.json")).unwrap(),
        b"device state"
    );
    assert_eq!(listing(directory.path()), before);
}

#[test]
fn cancellation_at_entry_and_after_staging_preserves_existing_output_and_cleans_up() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    for cancel_at in [0, 2, 3, 6, 12] {
        let directory = tempfile::tempdir().unwrap();
        let source = source_in(directory.path());
        let original = fs::read(&source).unwrap();
        let destination = directory.path().join("result.colpkg");
        let existed = cancel_at % 2 == 0;
        if existed {
            fs::write(&destination, b"existing package").unwrap();
        }
        let before = listing(directory.path());
        let calls = AtomicUsize::new(0);
        let cancel = || calls.fetch_add(1, Ordering::SeqCst) >= cancel_at;
        assert_eq!(
            write_with(
                &source,
                &destination,
                directory.path(),
                &StatvfsSpace,
                Finalisation::Replace,
                &cancel
            ),
            Err(Error::Cancelled)
        );
        assert_eq!(fs::read(source).unwrap(), original);
        if existed {
            assert_eq!(fs::read(destination).unwrap(), b"existing package");
        } else {
            assert!(!destination.exists());
        }
        assert_eq!(listing(directory.path()), before);
    }
}

#[test]
fn atomic_faults_preserve_before_publication_and_report_honestly_after_it() {
    use meguri_model::faultpoint::{self, Fault};
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            faultpoint::disarm_all();
        }
    }
    let _disarm = Disarm;
    for (point, kind, published) in [
        ("storage.atomic.after_write", std::io::ErrorKind::StorageFull, false),
        ("storage.atomic.after_sync", std::io::ErrorKind::Other, false),
        ("storage.atomic.before_rename", std::io::ErrorKind::Other, false),
        ("storage.atomic.after_rename", std::io::ErrorKind::Other, true),
    ] {
        let directory = tempfile::tempdir().unwrap();
        let source = source_in(directory.path());
        let original = fs::read(&source).unwrap();
        let destination = directory.path().join("result.colpkg");
        fs::write(&destination, b"existing package").unwrap();
        let before = listing(directory.path());
        faultpoint::arm(point, Fault::IoError(kind));
        let result = write_with(
            &source,
            &destination,
            directory.path(),
            &StatvfsSpace,
            Finalisation::Replace,
            &|| false,
        );
        faultpoint::disarm(point);
        if kind == std::io::ErrorKind::StorageFull {
            assert_eq!(result, Err(Error::DiskFull));
        } else {
            assert!(matches!(result, Err(Error::Io { .. })));
        }
        if published {
            assert_eq!(
                zip::ZipArchive::new(File::open(&destination).unwrap()).unwrap().len(),
                4
            );
        } else {
            assert_eq!(fs::read(destination).unwrap(), b"existing package");
        }
        assert_eq!(fs::read(source).unwrap(), original);
        assert_eq!(listing(directory.path()), before);
    }
}

#[test]
fn missing_media_map_reads_as_empty() {
    use meguri_package::{CollectionPackageInspectContext, inspect_collection_package};
    use meguri_storage::{OpenOptions, ReadRows, ReaderRole, open_collection};
    use std::io::{Cursor, Write};
    use zip::{ZipWriter, write::SimpleFileOptions};

    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("synthetic.anki2");
    let clock = FixedClock::new(1_894_701_600_000, UtcOffset::UTC);
    let random = SeededRandom::new(124);
    meguri_engine::bootstrap::create(&source, &clock, &random).unwrap();
    let collection = fs::read(&source).unwrap();
    let encoded = zstd::stream::encode_all(collection.as_slice(), 3).unwrap();
    let package = directory.path().join("synthetic.colpkg");
    let make = |media: Option<&[u8]>| {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        let options = SimpleFileOptions::default().compression_method(zip::CompressionMethod::Stored);
        archive.start_file("meta", options).unwrap();
        archive.write_all(&[8, 3]).unwrap();
        archive.start_file("collection.anki21b", options).unwrap();
        archive.write_all(&encoded).unwrap();
        if let Some(media) = media {
            archive.start_file("media", options).unwrap();
            archive.write_all(&zstd::stream::encode_all(media, 3).unwrap()).unwrap();
        }
        archive.finish().unwrap().into_inner()
    };
    let context = CollectionPackageInspectContext {
        collection_directory: directory.path(),
        live_collection_size: 0,
        reserved_space_bytes: 0,
        clock: &clock,
        random: &random,
        space: &StatvfsSpace,
        is_cancelled: &|| false,
    };
    let bytes = make(None);
    fs::write(&package, &bytes).unwrap();
    let before = listing(directory.path());
    let inspected = inspect_collection_package(&package, &context).expect("absent map is empty");
    assert_eq!(inspected.media_count(), 0);
    assert_eq!(inspected.media_bytes(), 0);
    assert_eq!(inspected.facts().media_count, 0);
    assert_eq!(
        (
            inspected.facts().notes,
            inspected.facts().cards,
            inspected.facts().revlog
        ),
        (0, 0, 0)
    );
    assert_eq!(inspected.facts().ver_found, 18);
    let opened = open_collection(&inspected.collection_path(), &OpenOptions::new(&clock, &random)).unwrap();
    let reader = opened.reader_factory().open(ReaderRole::Query).unwrap();
    assert!(reader.quick_check().unwrap().is_ok());
    assert!(
        reader.deck(meguri_model::ids::DeckId(1)).unwrap().is_some(),
        "expected bootstrap collection"
    );
    drop(reader);
    opened.close().unwrap();
    drop(inspected);
    assert_eq!(listing(directory.path()), before);
    assert_eq!(fs::read(&package).unwrap(), bytes);
    assert_eq!(fs::read(&source).unwrap(), collection);

    // A present, valid zstd frame with an invalid protobuf map is Corrupt.
    let malformed = make(Some(&[0xff]));
    fs::write(&package, &malformed).unwrap();
    assert!(matches!(
        inspect_collection_package(&package, &context),
        Err(Error::Corrupt { .. })
    ));
    assert_eq!(fs::read(&package).unwrap(), malformed);
    assert_eq!(fs::read(source).unwrap(), collection);
    assert_eq!(listing(directory.path()), before);
}
