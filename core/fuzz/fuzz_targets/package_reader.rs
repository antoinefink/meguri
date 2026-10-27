#![no_main]
//! P14: exercise the real bounded package reader/inspection paths. Synthetic
//! in-target seeds reach SQLite and media-map decoding without external data.
use libfuzzer_sys::fuzz_target;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_package::{
    CollectionPackageInspectContext, copy_and_inspect_collection_package, inspect_collection_package,
};
use meguri_storage::{StorageError, space::SpaceProvider};
use std::{
    fs,
    io::{Cursor, Write},
    path::Path,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

struct Capacity;
impl SpaceProvider for Capacity {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        Ok(64 * 1024 * 1024)
    }
}
fn seed() -> &'static [u8] {
    static SEED: OnceLock<Vec<u8>> = OnceLock::new();
    SEED.get_or_init(|| {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("collection.anki2");
        meguri_engine::bootstrap::create(
            &path,
            &FixedClock::new(0, UtcOffset::UTC),
            &SeededRandom::new(124),
        )
        .unwrap();
        fs::read(path).unwrap()
    })
}
fn envelope(data: &[u8]) -> Vec<u8> {
    let mode = data.first().copied().unwrap_or(5) % 6;
    if mode == 0 {
        return data.get(1..).unwrap_or_default().to_vec();
    }
    let body = data.get(1..).unwrap_or_default();
    let meta: &[u8] = if mode == 1 { body } else { &[8, 3] };
    let collection = match mode {
        3 => zstd::stream::encode_all(body, 1).unwrap(),
        4 => body.to_vec(),
        _ => zstd::stream::encode_all(seed(), 1).unwrap(),
    };
    let method = if data.first().is_some_and(|byte| byte & 0x80 != 0) {
        CompressionMethod::Deflated
    } else {
        CompressionMethod::Stored
    };
    let options = SimpleFileOptions::default().compression_method(method);
    let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in [("meta", meta), ("collection.anki21b", collection.as_slice())] {
        archive.start_file(name, options).unwrap();
        archive.write_all(content).unwrap();
    }
    if mode != 5 {
        archive.start_file("media", options).unwrap();
        let map = zstd::stream::encode_all(if mode == 2 { body } else { &[] }, 1).unwrap();
        archive.write_all(&map).unwrap();
    }
    let mut archive = archive.finish().unwrap().into_inner();
    // Empty mode 5 is a valid missing-map seed; other bodies mutate ZIP framing.
    if mode == 5 && body.len() >= 3 {
        let offset = usize::from(u16::from_le_bytes([body[0], body[1]])) % archive.len();
        archive[offset] ^= body[2];
    }
    archive
}
fn names(path: &Path) -> Vec<std::ffi::OsString> {
    let mut names: Vec<_> = fs::read_dir(path)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    names
}
fuzz_target!(|data: &[u8]| {
    if data.len() > 64 * 1024 {
        return;
    }
    let _ = meguri_package::package_meta_version(data);
    let bytes = envelope(data);
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    let live = directory.path().join("collection.anki2");
    let sidecar = directory.path().join("meguri-sidecar.json");
    fs::write(&source, &bytes).unwrap();
    fs::write(&live, b"protected synthetic live collection").unwrap();
    fs::write(&sidecar, b"protected synthetic local state").unwrap();
    let before = names(directory.path());
    for copy in [false, true] {
        // Bound decoded work in addition to production's per-entry, ratio,
        // window and total limits. No wall clock or persistent resources.
        let checks = AtomicUsize::new(0);
        let cancelled = || checks.fetch_add(1, Ordering::Relaxed) >= 256;
        let clock = FixedClock::new(0, UtcOffset::UTC);
        let random = SeededRandom::new(124);
        let context = CollectionPackageInspectContext {
            collection_directory: directory.path(),
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: &clock,
            random: &random,
            space: &Capacity,
            is_cancelled: &cancelled,
        };
        if copy {
            drop(copy_and_inspect_collection_package(
                &source,
                &context,
                &meguri_engine::ProgressSink::new(),
            ));
        } else {
            drop(inspect_collection_package(&source, &context));
        }
        assert_eq!(
            names(directory.path()),
            before,
            "unpublished scratch must be removed on every outcome"
        );
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(fs::read(&live).unwrap(), b"protected synthetic live collection");
        assert_eq!(
            fs::read(&sidecar).unwrap(),
            b"protected synthetic local state"
        );
    }
});
