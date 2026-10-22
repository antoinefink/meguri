//! Collection-only writer contract from package/container P11.

use std::fs::{self, File};
use std::io::Read;
use std::path::Path;

use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_package::{
    CollectionPackageWriteContext, Finalisation, write_collection_package_from_standalone,
};
use meguri_storage::atomic_file::Purpose;
use meguri_storage::space::StatvfsSpace;

#[test]
fn rf2_collection_writer_produces_latest_four_entry_package() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/storage/schema-18.anki2");
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
        entry.read_to_end(&mut payload).expect("read complete entry and verify its CRC");
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
    assert_eq!(fs::read(&source).expect("source after archive inspection"), original_source);
}
