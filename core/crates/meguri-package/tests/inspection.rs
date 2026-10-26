//! Public synthetic fixtures plus independently authored archives. These run
//! the production P12 API; storage owns mutations of temporary candidate copies.
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_package::{
    CollectionPackageInspectContext, CollectionPackageInspectError as Error, InspectedCollection,
    inspect_collection_package,
};
use meguri_storage::{
    StorageError,
    space::{SpaceProvider, StatvfsSpace},
};
use std::{
    fs,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures")
        .join(name)
}
fn listing(path: &Path) -> Vec<std::ffi::OsString> {
    let mut result: Vec<_> = fs::read_dir(path).unwrap().map(|e| e.unwrap().file_name()).collect();
    result.sort();
    result
}
fn inspect(
    source: &Path,
    directory: &Path,
    space: &dyn SpaceProvider,
    cancel: &(dyn Fn() -> bool + Sync),
    live: u64,
    extra: u64,
) -> Result<InspectedCollection, Error> {
    inspect_collection_package(
        source,
        &CollectionPackageInspectContext {
            collection_directory: directory,
            live_collection_size: live,
            reserved_space_bytes: extra,
            clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
            random: &SeededRandom::new(42),
            space,
            is_cancelled: cancel,
        },
    )
}
fn frame(bytes: &[u8]) -> Vec<u8> {
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.set_pledged_src_size(Some(bytes.len() as u64)).unwrap();
    encoder.include_contentsize(true).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.write_all(bytes).unwrap();
    encoder.finish().unwrap()
}
fn archive(entries: &[(&str, Vec<u8>)], method: CompressionMethod) -> Vec<u8> {
    let mut zip = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, bytes) in entries {
        zip.start_file(*name, SimpleFileOptions::default().compression_method(method))
            .unwrap();
        zip.write_all(bytes).unwrap();
    }
    zip.finish().unwrap().into_inner()
}
fn entries() -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("meta", vec![8, 3]),
        (
            "collection.anki21b",
            frame(&fs::read(fixture("storage/schema-18.anki2")).unwrap()),
        ),
        ("media", frame(b"")),
    ]
}
fn check(bytes: &[u8]) -> Result<InspectedCollection, Error> {
    // Refusal-only helper; successful-handle tests own their parent directory.
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, bytes).unwrap();
    fs::write(directory.path().join("collection.anki2"), b"live collection").unwrap();
    fs::create_dir(directory.path().join(".meguri-stage-unrelated")).unwrap();
    fs::write(directory.path().join("meguri-sidecar.json"), b"device state").unwrap();
    fs::create_dir(directory.path().join("backups")).unwrap();
    fs::create_dir(directory.path().join("collection.media")).unwrap();
    let before = listing(directory.path());
    let result = inspect(&source, directory.path(), &StatvfsSpace, &|| false, 17, 0);
    assert_eq!(fs::read(&source).unwrap(), bytes);
    assert_eq!(
        fs::read(directory.path().join("collection.anki2")).unwrap(),
        b"live collection"
    );
    assert_eq!(
        fs::read(directory.path().join("meguri-sidecar.json")).unwrap(),
        b"device state"
    );
    assert!(result.is_err(), "refusal helper requires a refusal");
    assert_eq!(listing(directory.path()), before);
    result
}
fn bad(bytes: &[u8]) {
    assert!(matches!(check(bytes), Err(Error::Corrupt { .. })));
}

#[test]
fn public_backup_has_real_schema18_counts_and_owned_drop_and_handoff() {
    let source = fixture("package/backup/standard.colpkg");
    let original = fs::read(&source).unwrap();
    let directory = tempfile::tempdir().unwrap();
    fs::write(directory.path().join("collection.anki2"), b"unchanged live bytes").unwrap();
    let before = listing(directory.path());
    let result = inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0).unwrap();
    let facts = result.facts();
    assert_eq!(
        (
            facts.ver_found,
            facts.notes,
            facts.cards,
            facts.revlog,
            facts.media_count
        ),
        (18, 6, 11, 6, 0)
    );
    let path = result.collection_path();
    assert!(path.is_file());
    let expected = {
        let mut archive = zip::ZipArchive::new(Cursor::new(&original)).unwrap();
        let mut payload = Vec::new();
        for index in 0..archive.len() {
            let mut entry = archive.by_index(index).unwrap();
            if entry.name().unwrap().as_ref() == "collection.anki21b" {
                entry.read_to_end(&mut payload).unwrap();
                break;
            }
        }
        zstd::stream::decode_all(payload.as_slice()).unwrap()
    };
    let actual = fs::read(&path).unwrap();
    assert_eq!(&actual[..16], b"SQLite format 3\0");
    assert_eq!(
        &actual[100..],
        &expected[100..],
        "schema18 collection pages preserved beyond SQLite's mutable file header"
    );
    assert!(!path.parent().unwrap().join("meguri-sidecar.json").exists());
    let staged = result.into_staged();
    assert!(path.exists(), "consuming the preview transfers ownership");
    assert_eq!(staged.collection_path(), path);
    let owned = staged.into_directory();
    assert!(path.exists());
    drop(owned);
    assert!(!path.exists());
    let second = inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0).unwrap();
    let path = second.collection_path();
    drop(second);
    assert!(!path.exists());
    assert_eq!(listing(directory.path()), before);
    assert_eq!(
        fs::read(directory.path().join("collection.anki2")).unwrap(),
        b"unchanged live bytes"
    );
    assert_eq!(fs::read(source).unwrap(), original);
}
#[test]
fn public_schema11_package_upgrades_only_the_owned_copy() {
    let source = fixture("package/variants/schema-11-inside.colpkg");
    let original = fs::read(&source).unwrap();
    let directory = tempfile::tempdir().unwrap();
    let result = inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0).unwrap();
    assert_eq!(
        (
            result.facts().ver_found,
            result.facts().notes,
            result.facts().cards,
            result.facts().revlog
        ),
        (11, 6, 11, 6)
    );
    let path = result.collection_path();
    assert!(path.with_file_name("collection.anki2.pre-upgrade-v11").exists());
    // Re-enter the real storage gate on a separate copy: it is now schema18.
    let verify = directory.path().join("verification");
    fs::create_dir(&verify).unwrap();
    fs::copy(&path, verify.join("collection.anki2")).unwrap();
    let facts = meguri_storage::package_inspection::inspect_candidate(
        &verify.join("collection.anki2"),
        &meguri_storage::package_inspection::CandidateContext {
            clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
            random: &SeededRandom::new(2),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    assert_eq!(facts.ver_found, 18);
    assert_eq!(fs::read(source).unwrap(), original);
}
#[test]
fn general_layout_absent_map_unknown_entries_and_unknown_protobuf_are_supported() {
    for method in [CompressionMethod::Stored, CompressionMethod::Deflated] {
        let mut e = entries();
        e[0].1 = vec![0x3b, 8, 4, 0x3c, 8, 4, 8, 0x83, 0x80, 0x80, 0x80, 0x10];
        e[2].1 = frame(&[0x3b, 8, 9, 0x3c, 0x38, 5]); // unknown-only map is empty
        e.swap(0, 2);
        e.push(("../../ignored", b"not a collection".to_vec()));
        e.push(("unknown/", Vec::new()));
        e.push(("collection.anki2", b"never open this placeholder".to_vec()));
        for absent in [false, true] {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("input.colpkg");
            let selected: Vec<_> = e
                .iter()
                .filter(|(name, _)| !absent || *name != "media")
                .cloned()
                .collect();
            fs::write(&source, archive(&selected, method)).unwrap();
            let result = inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0).unwrap();
            assert_eq!(result.facts().ver_found, 18);
            assert_eq!(result.facts().media_count, 0);
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let result = inspect(
        &fixture("package/variants/no-media-map.colpkg"),
        directory.path(),
        &StatvfsSpace,
        &|| false,
        0,
        0,
    )
    .unwrap();
    assert_eq!(result.facts().media_count, 0);
}
#[test]
fn public_refusals_preserve_source_and_live_and_clean_only_own_staging() {
    for name in [
        "collection-not-zstd",
        "corrupt-page",
        "media-sha1-short",
        "media-traversal",
        "media-unnormalized",
        "meta-not-protobuf",
        "no-collection-entry",
        "not-a-zip",
    ] {
        bad(&fs::read(fixture(&format!("package/variants/{name}.colpkg"))).unwrap());
    }
    for name in ["meta-version-0", "meta-version-4"] {
        assert!(matches!(
            check(&fs::read(fixture(&format!("package/variants/{name}.colpkg"))).unwrap()),
            Err(Error::TooNew)
        ));
    }
    assert!(matches!(
        check(&fs::read(fixture("package/variants/schema-12.colpkg")).unwrap()),
        Err(Error::SchemaUnsupported {
            gate: meguri_storage::OpenError::TooNew { version: 12 }
        })
    ));
}
#[test]
fn refuses_legacy_meta_errors_and_needed_entry_damage() {
    for version in [0, 1, 2, 4] {
        let mut e = entries();
        e[0].1 = vec![8, version];
        let error = check(&archive(&e, CompressionMethod::Stored)).unwrap_err();
        assert_eq!(
            error,
            if matches!(version, 1 | 2) {
                Error::UnsupportedFormat
            } else {
                Error::TooNew
            }
        );
    }
    let mut e = entries();
    e.remove(0);
    assert!(matches!(
        check(&archive(&e, CompressionMethod::Stored)),
        Err(Error::UnsupportedFormat)
    ));
    for meta in [
        vec![0xff],
        vec![0x0a, 1, 3],
        vec![0x3b],
        vec![0x3b, 0x44],
        vec![0x3c],
        vec![8; 65_537],
    ] {
        let mut e = entries();
        e[0].1 = meta;
        bad(&archive(&e, CompressionMethod::Stored));
    }
    let good = archive(&entries(), CompressionMethod::Stored);
    let central: Vec<_> = good
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i))
        .collect();
    for index in &central {
        for (offset, value) in [(8, 1_u16), (10, 99_u16)] {
            let mut broken = good.clone();
            broken[index + offset..index + offset + 2].copy_from_slice(&value.to_le_bytes());
            bad(&broken);
        }
    }
    let mut crc = good.clone();
    crc[30 + 4 + 1] ^= 1;
    bad(&crc);
}
fn map_entry(name: &[u8], sha_len: u8) -> Vec<u8> {
    fn length(mut n: usize, bytes: &mut Vec<u8>) {
        while n >= 128 {
            bytes.push((n as u8 & 0x7f) | 0x80);
            n >>= 7;
        }
        bytes.push(n as u8);
    }
    let mut entry = vec![10];
    length(name.len(), &mut entry);
    entry.extend_from_slice(name);
    entry.extend_from_slice(&[16, 0, 26, sha_len]);
    entry.extend(vec![0; usize::from(sha_len)]);
    let mut map = vec![10];
    length(entry.len(), &mut map);
    map.extend(entry);
    map
}
fn local_media_map(items: &[(&str, &[u8])]) -> Vec<u8> {
    use sha1::{Digest, Sha1};
    fn integer(mut n: u64, out: &mut Vec<u8>) {
        while n >= 128 {
            out.push(n as u8 | 128);
            n >>= 7;
        }
        out.push(n as u8);
    }
    let mut map = Vec::new();
    for (name, bytes) in items {
        let mut record = vec![10];
        integer(name.len() as u64, &mut record);
        record.extend_from_slice(name.as_bytes());
        record.push(16);
        integer(bytes.len() as u64, &mut record);
        record.extend_from_slice(&[26, 20]);
        record.extend_from_slice(&Sha1::digest(bytes));
        map.push(10);
        integer(record.len() as u64, &mut map);
        map.extend(record);
    }
    map
}
#[test]
fn local_entry_validates_every_payload_and_keeps_collection_only_policy_explicit() {
    use meguri_engine::{CancelToken, Engine, EngineConfig, ProgressSink};
    use meguri_package::entry::{EntryInputs, inspect_entry};
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("destination");
    fs::create_dir(&directory).unwrap();
    let source = root.path().join("selected.colpkg");
    let inputs = EntryInputs {
        collection_directory: directory.clone(),
        backups_directory: root.path().join("backups"),
        clock: Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        random: Arc::new(SeededRandom::new(42)),
        space: Arc::new(StatvfsSpace),
    };
    let process = Engine::new(EngineConfig::default()).unwrap();
    let mut e = entries();
    e[2].1 = frame(&local_media_map(&[
        ("unused.png", b"independent image"),
        ("_static.dat", b""),
        ("audio.mp3", b"independent audio"),
    ]));
    e.extend([
        ("0", frame(b"independent image")),
        ("1", frame(b"")),
        ("2", frame(b"independent audio")),
    ]);
    fs::write(&source, archive(&e, CompressionMethod::Stored)).unwrap();
    let original = fs::read(&source).unwrap();
    let inspect_local = || inspect_entry(&process, &source, &inputs, CancelToken::new(), &ProgressSink::new());
    let candidate = inspect_local().unwrap();
    assert_eq!(
        (candidate.facts().media_count, candidate.facts().media_decoded_bytes),
        (3, 34)
    );
    assert_eq!(candidate.facts().source.media_count, 3);
    assert_eq!(candidate.facts().source.media_decoded_bytes, 34);
    assert!(!directory.join("collection.anki2").exists());
    assert!(!directory.join("collection.media").exists());
    drop(candidate);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    assert!(matches!(
        inspect(&source, &directory, &StatvfsSpace, &|| false, 0, 0),
        Err(Error::UnsupportedMedia)
    ));
    assert_eq!(fs::read(&source).unwrap(), original);
    // A late body, missing/extra/noncanonical member and conflicting supplied
    // names must all refuse before media, journal or DB publication.
    let mut cases = Vec::new();
    let mut corrupt_late = e.clone();
    corrupt_late[5].1 = frame(b"different audio");
    cases.push(corrupt_late);
    let mut missing = e.clone();
    missing.pop();
    cases.push(missing);
    let mut extra = e.clone();
    extra.push(("3", frame(b"extra")));
    cases.push(extra);
    let mut numeric = e.clone();
    numeric[5].0 = "02";
    cases.push(numeric);
    let mut frame_error = e.clone();
    frame_error[5].1.push(1);
    cases.push(frame_error);
    let mut conflicting = e.clone();
    conflicting[2].1 = frame(&local_media_map(&[("Photo.png", b"a"), ("photo.png", b"b")]));
    cases.push(conflicting);
    for entries in cases {
        let bytes = archive(&entries, CompressionMethod::Stored);
        fs::write(&source, &bytes).unwrap();
        assert!(inspect_local().is_err());
        assert_eq!(fs::read(&source).unwrap(), bytes);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
    }
    fs::write(&source, &original).unwrap();
    meguri_model::faultpoint::arm(
        "package.inspect.media_write",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::StorageFull),
    );
    assert!(matches!(
        inspect_local(),
        Err(meguri_package::entry::EntryError::Package(Error::DiskFull))
    ));
    meguri_model::faultpoint::disarm("package.inspect.media_write");
    assert_eq!(fs::read(&source).unwrap(), original);
    assert_eq!(fs::read_dir(&directory).unwrap().count(), 0);
}
#[test]
fn local_entry_media_preflight_preserves_invalid_destinations_and_skips_collection_only_sources() {
    use meguri_engine::{CancelToken, Engine, EngineConfig, ProgressSink};
    use meguri_package::entry::{EntryError, EntryInputs, inspect_entry};
    use std::sync::Arc;
    let tmp = tempfile::tempdir().unwrap();
    let plain = tmp.path().join("a.colpkg");
    let plain_zip = archive(&entries(), CompressionMethod::Stored);
    fs::write(&plain, &plain_zip).unwrap();
    let full = tmp.path().join("b.colpkg");
    let data = b"\x01\x80\x00\xff";
    let mut items = entries();
    items[2].1 = frame(&local_media_map(&[("a.png", data)]));
    items.push(("0", frame(data)));
    let full_zip = archive(&items, CompressionMethod::Stored);
    fs::write(&full, &full_zip).unwrap();
    let outside = tmp.path().join("c.bin");
    let bytes = b"\x03\xa0\x00\xfd";
    fs::write(&outside, bytes).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    for leaf in [
        "collection.media",
        "media.trash",
        "media.staging",
        "meguri-media.db",
        "meguri-media.db-wal",
        "meguri-media.db-journal",
    ] {
        let dest = tmp.path().join(leaf);
        fs::create_dir(&dest).unwrap();
        let block = dest.join(leaf);
        let mark = if leaf.starts_with("meguri-media.db") {
            fs::create_dir(&block).unwrap();
            block.join("d.bin")
        } else {
            block
        };
        let live = b"\x02\x90\x00\xfe";
        fs::write(&mark, live).unwrap();
        let saved = listing(&dest);
        let args = EntryInputs {
            collection_directory: dest.clone(),
            backups_directory: tmp.path().join("bak"),
            clock: Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
            random: Arc::new(SeededRandom::new(42)),
            space: Arc::new(StatvfsSpace),
        };
        let read = |path: &Path| inspect_entry(&engine, path, &args, CancelToken::new(), &ProgressSink::new());
        assert_eq!(fs::read(&plain).unwrap(), plain_zip);
        assert_eq!(fs::read(&full).unwrap(), full_zip);
        assert_eq!(fs::read(&mark).unwrap(), live);
        assert_eq!(fs::read(&outside).unwrap(), bytes);
        let token = read(&plain).unwrap();
        assert_eq!(token.facts().media_count, 0);
        drop(token);
        assert_eq!(listing(&dest), saved);
        assert_eq!(fs::read(&plain).unwrap(), plain_zip);
        assert_eq!(fs::read(&full).unwrap(), full_zip);
        assert_eq!(fs::read(&mark).unwrap(), live);
        assert_eq!(fs::read(&outside).unwrap(), bytes);
        assert!(matches!(read(&full), Err(EntryError::Package(Error::Corrupt { .. }))));
        assert_eq!(listing(&dest), saved);
        assert_eq!(fs::read(&plain).unwrap(), plain_zip);
        assert_eq!(fs::read(&full).unwrap(), full_zip);
        assert_eq!(fs::read(&mark).unwrap(), live);
        assert_eq!(fs::read(&outside).unwrap(), bytes);
        assert!(!dest.join("collection.anki2").exists());
    }
    assert_eq!(fs::read(plain).unwrap(), plain_zip);
    assert_eq!(fs::read(full).unwrap(), full_zip);
}
#[test]
fn local_entry_streams_oversize_local_media_without_applying_sync_limit() {
    use sha1::{Digest, Sha1};
    use std::sync::Arc;
    let root = tempfile::tempdir().unwrap();
    let destination = root.path().join("destination");
    fs::create_dir(&destination).unwrap();
    let source = root.path().join("large.colpkg");
    let mut zip = ZipWriter::new(fs::File::create(&source).unwrap());
    for (name, data) in entries().into_iter().take(2) {
        zip.start_file(
            name,
            SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
        )
        .unwrap();
        zip.write_all(&data).unwrap();
    }
    zip.start_file(
        "0",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    let chunk = [0x5au8; 65536];
    let chunks = 1601u64;
    let length = chunks * chunk.len() as u64;
    let mut digest = Sha1::new();
    let mut encoder = zstd::stream::write::Encoder::new(&mut zip, 3).unwrap();
    encoder.include_checksum(true).unwrap();
    encoder.set_pledged_src_size(Some(length)).unwrap();
    for _ in 0..chunks {
        encoder.write_all(&chunk).unwrap();
        digest.update(chunk);
    }
    encoder.finish().unwrap();
    fn integer(mut n: u64, out: &mut Vec<u8>) {
        while n >= 128 {
            out.push(n as u8 | 128);
            n >>= 7;
        }
        out.push(n as u8);
    }
    let mut record = vec![10, 9];
    record.extend_from_slice(b"large.bin");
    record.push(16);
    integer(length, &mut record);
    record.extend_from_slice(&[26, 20]);
    record.extend_from_slice(&digest.finalize());
    let mut map = vec![10];
    integer(record.len() as u64, &mut map);
    map.extend(record);
    zip.start_file(
        "media",
        SimpleFileOptions::default().compression_method(CompressionMethod::Stored),
    )
    .unwrap();
    zip.write_all(&frame(&map)).unwrap();
    zip.finish().unwrap();
    let inputs = meguri_package::entry::EntryInputs {
        collection_directory: destination.clone(),
        backups_directory: root.path().join("backups"),
        clock: Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        random: Arc::new(SeededRandom::new(42)),
        space: Arc::new(StatvfsSpace),
    };
    let engine = meguri_engine::Engine::new(meguri_engine::EngineConfig::default()).unwrap();
    let candidate = meguri_package::entry::inspect_entry(
        &engine,
        &source,
        &inputs,
        meguri_engine::CancelToken::new(),
        &meguri_engine::ProgressSink::new(),
    )
    .unwrap();
    assert_eq!(
        (candidate.facts().media_count, candidate.facts().media_decoded_bytes),
        (1, length)
    );
    assert!(length > 104_857_600);
    drop(candidate);
    assert_eq!(fs::read_dir(destination).unwrap().count(), 0);
    assert!(source.is_file());
}
#[test]
fn validates_whole_media_message_before_explicit_unsupported_refusal() {
    for name in [
        b"../x.jpg".as_slice(),
        b"con.jpg",
        b"",
        b"a/b",
        b"a\\b",
        b".",
        b"..",
        b"name.",
        b"name ",
        b"x?.png",
        b"e\xcc\x81.png",
        b"\xff",
    ] {
        let mut e = entries();
        e[2].1 = frame(&map_entry(name, 20));
        bad(&archive(&e, CompressionMethod::Stored));
    }
    for map in [
        map_entry(b"photo.jpg", 19),
        vec![8, 0],
        vec![10, 0],
        vec![0x3b],
        vec![0x3b, 0x44],
        vec![0x3c],
        vec![0x38, 0xff],
        vec![0x3f],
    ] {
        let mut e = entries();
        e[2].1 = frame(&map);
        bad(&archive(&e, CompressionMethod::Stored));
    }
    let mut long = entries();
    long[2].1 = frame(&map_entry(&[b'a'; 121], 20));
    bad(&archive(&long, CompressionMethod::Stored));
    let valid = map_entry("日本.mp3".as_bytes(), 20);
    let mut e = entries();
    e[2].1 = frame(&valid);
    assert!(matches!(
        check(&archive(&e, CompressionMethod::Stored)),
        Err(Error::UnsupportedMedia)
    ));
    let mut malformed = valid;
    malformed.extend(map_entry(b"../bad", 20));
    e[2].1 = frame(&malformed);
    bad(&archive(&e, CompressionMethod::Stored));
    for empty_map in [true, false] {
        let mut e = entries();
        if !empty_map {
            e.pop();
        }
        e.push(("0", b"carried media".to_vec()));
        assert!(matches!(
            check(&archive(&e, CompressionMethod::Stored)),
            Err(Error::UnsupportedMedia)
        ));
    }
}

#[test]
fn first_preflight_uses_declared_size_or_streaming_estimate_and_extra_before_writes() {
    struct Empty<'a> {
        directory: &'a Path,
        needed: std::sync::Mutex<Vec<u64>>,
    }
    impl SpaceProvider for Empty<'_> {
        fn available_bytes(&self, path: &Path) -> Result<u64, StorageError> {
            assert_eq!(path, self.directory);
            assert_eq!(listing(path).len(), 1, "no staging before preflight");
            self.needed.lock().unwrap().push(0);
            Ok(0)
        }
    }
    let raw = fs::read(fixture("storage/schema-18.anki2")).unwrap();
    for declared in [true, false] {
        let mut e = entries();
        if !declared {
            e[1].1 = zstd::stream::encode_all(raw.as_slice(), 3).unwrap();
        }
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.colpkg");
        fs::write(&source, archive(&e, CompressionMethod::Stored)).unwrap();
        let live = 100_000_000;
        let extra = 456;
        let estimate = if declared {
            raw.len() as u64
        } else {
            (e[1].1.len() as u64 * 4).max(live)
        };
        let space = Empty {
            directory: directory.path(),
            needed: Default::default(),
        };
        assert_eq!(
            inspect(&source, directory.path(), &space, &|| false, live, extra).unwrap_err(),
            Error::InsufficientSpace {
                needed: estimate + 4 * 1024 * 1024 + extra,
                available: 0
            }
        );
        assert_eq!(space.needed.lock().unwrap().len(), 1);
        assert!(matches!(
            inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, u64::MAX),
            Err(Error::Corrupt { .. })
        ));
        assert_eq!(listing(directory.path()).len(), 1);
    }
}
#[test]
fn upgrade_copy_preflight_fails_before_copy_and_cleans_extraction() {
    struct SecondFull(AtomicUsize);
    impl SpaceProvider for SecondFull {
        fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
            if self.0.fetch_add(1, Ordering::SeqCst) == 0 {
                Ok(u64::MAX)
            } else {
                assert!(
                    directory
                        .file_name()
                        .unwrap()
                        .to_string_lossy()
                        .starts_with(".meguri-stage-")
                );
                assert_eq!(listing(directory), vec![std::ffi::OsString::from("collection.anki2")]);
                Ok(0)
            }
        }
    }
    let directory = tempfile::tempdir().unwrap();
    let source = fixture("package/variants/schema-11-inside.colpkg");
    let space = SecondFull(AtomicUsize::new(0));
    assert!(matches!(
        inspect(&source, directory.path(), &space, &|| false, 0, 0),
        Err(Error::InsufficientSpace { available: 0, .. })
    ));
    assert_eq!(space.0.load(Ordering::SeqCst), 2);
    assert!(listing(directory.path()).is_empty());
}
#[test]
fn cancellation_during_extraction_and_disk_full_remove_owned_files() {
    let directory = tempfile::tempdir().unwrap();
    let source = fixture("package/backup/standard.colpkg");
    let original = fs::read(&source).unwrap();
    let cancel = || {
        fs::read_dir(directory.path()).unwrap().any(|entry| {
            let path = entry.unwrap().path().join("collection.anki2");
            fs::metadata(path).is_ok_and(|m| m.len() > 0)
        })
    };
    assert!(matches!(
        inspect(&source, directory.path(), &StatvfsSpace, &cancel, 0, 0),
        Err(Error::Cancelled)
    ));
    assert!(listing(directory.path()).is_empty());
    assert!(matches!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| true, 0, 0),
        Err(Error::Cancelled)
    ));
    use meguri_model::faultpoint::{self, Fault};
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            faultpoint::disarm("package.inspect.write");
        }
    }
    let _disarm = Disarm;
    faultpoint::arm("package.inspect.write", Fault::IoError(std::io::ErrorKind::StorageFull));
    assert!(matches!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0),
        Err(Error::DiskFull)
    ));
    assert!(listing(directory.path()).is_empty());
    assert_eq!(fs::read(source).unwrap(), original);
}
#[test]
fn zstd_checksum_window_frames_and_ratio_are_enforced_in_inspection() {
    let mut e = entries();
    let good = e[1].1.clone();
    let mut checksum = good.clone();
    *checksum.last_mut().unwrap() ^= 1;
    let mut trailing = good.clone();
    trailing.extend_from_slice(b"garbage");
    for broken in [
        checksum,
        trailing,
        good[..good.len() - 1].to_vec(),
        vec![0x28, 0xb5, 0x2f, 0xfd, 0, 0x90, 1, 0, 0],
    ] {
        e[1].1 = broken;
        bad(&archive(&e, CompressionMethod::Stored));
    }
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    for _ in 0..257 {
        encoder.write_all(&[0; 65536]).unwrap();
    }
    let bomb = encoder.finish().unwrap();
    assert!(bomb.len() < 1024);
    e[1].1 = bomb.clone();
    bad(&archive(&e, CompressionMethod::Stored));
    e = entries();
    e[2].1 = bomb;
    bad(&archive(&e, CompressionMethod::Stored));
    // Split a real collection across ordinary frames with a skippable frame
    // between them. No requirement for a declared size/checksum on all frames.
    let raw = fs::read(fixture("storage/schema-18.anki2")).unwrap();
    let middle = raw.len() / 2;
    let mut frames = frame(&raw[..middle]);
    frames.extend_from_slice(&[0x50, 0x2a, 0x4d, 0x18, 1, 0, 0, 0, 0xff]);
    frames.extend(frame(&raw[middle..]));
    e = entries();
    e[1].1 = frames;
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, archive(&e, CompressionMethod::Deflated)).unwrap();
    assert_eq!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
            .unwrap()
            .facts()
            .ver_found,
        18
    );
}

fn le16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().unwrap())
}
fn le32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().unwrap())
}
fn zip64(bytes: &[u8], count: u64, sentinels: bool) -> Vec<u8> {
    let end = bytes.len() - 22;
    let mut result = bytes[..end].to_vec();
    result.extend_from_slice(b"PK\x06\x06");
    result.extend_from_slice(&44_u64.to_le_bytes());
    result.extend_from_slice(&[45, 0, 45, 0]);
    result.extend_from_slice(&[0; 8]);
    result.extend_from_slice(&count.to_le_bytes());
    result.extend_from_slice(&count.to_le_bytes());
    result.extend_from_slice(&u64::from(le32(bytes, end + 12)).to_le_bytes());
    result.extend_from_slice(&u64::from(le32(bytes, end + 16)).to_le_bytes());
    result.extend_from_slice(b"PK\x06\x07");
    result.extend_from_slice(&0_u32.to_le_bytes());
    result.extend_from_slice(&(end as u64).to_le_bytes());
    result.extend_from_slice(&1_u32.to_le_bytes());
    result.extend_from_slice(&bytes[end..]);
    if sentinels {
        let new_end = result.len() - 22;
        result[new_end + 8..new_end + 20].fill(0xff);
    }
    result
}
#[test]
fn optional_zip64_never_overrides_classic_fields_to_bypass_preallocation_bound() {
    let good = archive(&entries(), CompressionMethod::Stored);
    let over_limit_archive = zip64(&good, 400_001, false);
    assert!(
        matches!(check(&over_limit_archive), Err(Error::Corrupt { detail }) if detail == "ZIP member count exceeds bound")
    );
    bad(&zip64(&good, 4, false)); // ordinary count=3 disagrees
    for sentinels in [false, true] {
        let mut bytes = b"ignored executable prefix".to_vec();
        bytes.extend(zip64(&good, 3, sentinels));
        let end = bytes.len() - 22;
        bytes[end + 20..end + 22].copy_from_slice(&65_535_u16.to_le_bytes());
        bytes.extend(vec![b'x'; 65_535]);
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("input.colpkg");
        fs::write(&source, &bytes).unwrap();
        assert_eq!(
            inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
                .unwrap()
                .facts()
                .ver_found,
            18
        );
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}
#[test]
fn duplicate_names_and_incorrect_sizes_fail_while_unused_methods_pass() {
    let mut e = entries();
    e.push(("xxxx", b"unused".to_vec()));
    let good = archive(&e, CompressionMethod::Stored);
    let last = good.windows(4).rposition(|b| b == b"PK\x01\x02").unwrap();
    assert_eq!(le16(&good, last + 28), 4);
    let mut duplicate = good.clone();
    duplicate[last + 46..last + 50].copy_from_slice(b"meta");
    bad(&duplicate);
    let mut unknown = good.clone();
    unknown[last + 8..last + 10].copy_from_slice(&1_u16.to_le_bytes());
    unknown[last + 10..last + 12].copy_from_slice(&99_u16.to_le_bytes());
    unknown[last + 46..last + 50].copy_from_slice(&[0xff; 4]); // unknown byte names never require UTF-8
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, unknown).unwrap();
    assert_eq!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
            .unwrap()
            .facts()
            .ver_found,
        18
    );
    let collection = good
        .windows(4)
        .enumerate()
        .filter_map(|(i, b)| (b == b"PK\x01\x02").then_some(i))
        .nth(1)
        .unwrap();
    for offset in [20, 24] {
        let mut lie = good.clone();
        lie[collection + offset..collection + offset + 4].copy_from_slice(&1_u32.to_le_bytes());
        bad(&lie);
    }
}
#[test]
fn actual_central_records_are_counted_even_when_eocd_lies() {
    // Central-only unknown records suffice to prove that the production
    // scanner stops before allocating zip's index. No local data is opened.
    let mut bytes = Vec::new();
    for index in 0..400_001 {
        let name = format!("unused-{index}");
        let mut record = [0; 46];
        record[..4].copy_from_slice(b"PK\x01\x02");
        record[28..30].copy_from_slice(&(name.len() as u16).to_le_bytes());
        bytes.extend_from_slice(&record);
        bytes.extend_from_slice(name.as_bytes());
    }
    let size = bytes.len() as u32;
    bytes.extend_from_slice(b"PK\x05\x06");
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&[1, 0, 1, 0]);
    bytes.extend_from_slice(&size.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    assert!(matches!(check(&bytes),Err(Error::Corrupt { detail }) if detail=="ZIP member count exceeds bound"));
}
#[test]
fn unknown_extras_and_comments_do_not_enter_the_zip_index() {
    let mut e = entries();
    e.push(("ignored", Vec::new()));
    let mut bytes = archive(&e, CompressionMethod::Stored);
    let central = bytes.windows(4).rposition(|b| b == b"PK\x01\x02").unwrap();
    // An unknown extra with a maximum 16-bit payload is never interpreted or
    // accumulated. Unlike needed ZIP64 extras it need not be semantically read.
    let inserted = 65_535_u16;
    bytes[central + 30..central + 32].copy_from_slice(&inserted.to_le_bytes());
    let point = central + 46 + usize::from(le16(&bytes, central + 28));
    bytes.splice(point..point, vec![0xaa; usize::from(inserted)]);
    let end = bytes.len() - 22;
    let size = le32(&bytes, end + 12) + u32::from(inserted);
    bytes[end + 12..end + 16].copy_from_slice(&size.to_le_bytes());
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, bytes).unwrap();
    assert_eq!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
            .unwrap()
            .facts()
            .ver_found,
        18
    );
}
#[test]
fn media_unknown_groups_obey_depth_and_fixed_wire_rules() {
    let mut e = entries();
    for depth in [100, 101] {
        let mut map = vec![0x3b; depth];
        map.extend(vec![0x3c; depth]);
        e[2].1 = frame(&map);
        let bytes = archive(&e, CompressionMethod::Stored);
        if depth == 101 {
            bad(&bytes);
        } else {
            let directory = tempfile::tempdir().unwrap();
            let source = directory.path().join("input.colpkg");
            fs::write(&source, bytes).unwrap();
            assert_eq!(
                inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
                    .unwrap()
                    .facts()
                    .media_count,
                0
            );
        }
    }
    let mut fields = vec![0x39];
    fields.extend_from_slice(&[0; 8]);
    fields.push(0x3d);
    fields.extend_from_slice(&[0; 4]);
    fields.extend_from_slice(&[0x3a, 2, 0xff, 0xff]);
    e[2].1 = frame(&fields);
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, archive(&e, CompressionMethod::Stored)).unwrap();
    assert_eq!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
            .unwrap()
            .facts()
            .media_count,
        0
    );
}
#[test]
fn decoded_media_absolute_limit_and_declared_collection_limit_are_enforced() {
    // Seed enough compressed bytes to permit over 64 MiB under the ratio
    // check, then append zeros to exercise the absolute media-size ceiling.
    let mut noise = vec![0; 96 * 1024];
    let mut state = 0x12345678_u32;
    for byte in &mut noise {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        *byte = state as u8;
    }
    let mut encoder = zstd::stream::write::Encoder::new(Vec::new(), 3).unwrap();
    encoder.write_all(&noise).unwrap();
    for _ in 0..1024 {
        encoder.write_all(&[0; 65536]).unwrap();
    }
    let encoded = encoder.finish().unwrap();
    assert!(encoded.len() > 64 * 1024 * 1024 / 1000);
    let mut e = entries();
    e[2].1 = encoded;
    bad(&archive(&e, CompressionMethod::Stored));
    let mut declared = vec![0x28, 0xb5, 0x2f, 0xfd, 0xe0];
    declared.extend_from_slice(&(8_u64 * 1024 * 1024 * 1024 + 1).to_le_bytes());
    declared.extend_from_slice(&[1, 0, 0]);
    e = entries();
    e[1].1 = declared;
    bad(&archive(&e, CompressionMethod::Stored));
}
#[test]
fn zip64_extensible_sector_is_scanned_with_fixed_buffers_even_with_prefix() {
    let good = archive(&entries(), CompressionMethod::Stored);
    let old_end = good.len() - 22;
    let mut bytes = zip64(&good, 3, true);
    let extension = vec![0xa5; 150_000];
    bytes[old_end + 4..old_end + 12].copy_from_slice(&(44_u64 + extension.len() as u64).to_le_bytes());
    bytes.splice(old_end + 56..old_end + 56, extension);
    let mut prefixed = b"prefix".to_vec();
    prefixed.extend(bytes);
    let directory = tempfile::tempdir().unwrap();
    let source = directory.path().join("input.colpkg");
    fs::write(&source, prefixed).unwrap();
    assert_eq!(
        inspect(&source, directory.path(), &StatvfsSpace, &|| false, 0, 0)
            .unwrap()
            .facts()
            .ver_found,
        18
    );
}

#[test]
fn malformed_probe_columns_are_corrupt_through_package_inspection() {
    use meguri_storage::package_inspection::testing::{ProbeMutation, mutate_probe_candidate};
    for mutation in [
        ProbeMutation::MissingVersionColumn,
        ProbeMutation::MissingConfigValueColumn,
    ] {
        let temp = tempfile::tempdir().unwrap();
        let path = temp.path().join("collection.anki2");
        fs::copy(fixture("storage/schema-18.anki2"), &path).unwrap();
        mutate_probe_candidate(&path, mutation);
        let mut e = entries();
        e[1].1 = frame(&fs::read(path).unwrap());
        bad(&archive(&e, CompressionMethod::Stored));
    }
}

#[test]
fn probe_value_limit_rejects_text_and_blob_in_actual_packages_and_keeps_inputs() {
    use meguri_storage::package_inspection::testing::{ProbeMutation, mutate_probe_candidate};
    for version in [11, 18] {
        for blob in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let path = temp.path().join("collection.anki2");
            fs::copy(fixture(&format!("storage/schema-{version}.anki2")), &path).unwrap();
            mutate_probe_candidate(
                &path,
                ProbeMutation::SchedulerValue {
                    bytes: 4 * 1024 * 1024 + 1,
                    blob,
                },
            );
            let mut e = entries();
            e[1].1 = frame(&fs::read(path).unwrap());
            assert!(
                matches!(check(&archive(&e, CompressionMethod::Stored)), Err(Error::Corrupt { detail }) if detail.contains("candidate probe value byte limit"))
            );
        }
    }
}

#[test]
fn filesystem_failures_still_return_io() {
    let temp = tempfile::tempdir().unwrap();
    assert!(matches!(
        inspect(
            &temp.path().join("absent.colpkg"),
            temp.path(),
            &StatvfsSpace,
            &|| false,
            0,
            0
        ),
        Err(Error::Io { .. })
    ));
    let source = temp.path().join("input.colpkg");
    fs::write(&source, archive(&entries(), CompressionMethod::Stored)).unwrap();
    let before = fs::read(&source).unwrap();
    // A regular file cannot contain the required staging directory.
    assert!(matches!(
        inspect(&source, &source, &StatvfsSpace, &|| false, 0, 0),
        Err(Error::Io { .. })
    ));
    assert_eq!(fs::read(source).unwrap(), before);
}

#[test]
fn package_gate_accepts_boundary_values_and_large_unrelated_config() {
    use meguri_storage::package_inspection::testing::{ProbeMutation, mutate_probe_candidate};
    for (version, mutation) in [
        (
            11,
            ProbeMutation::SchedulerValue {
                bytes: 4 * 1024 * 1024,
                blob: false,
            },
        ),
        (
            18,
            ProbeMutation::SchedulerValue {
                bytes: 4 * 1024 * 1024,
                blob: false,
            },
        ),
        (
            18,
            ProbeMutation::SchedulerValue {
                bytes: 4 * 1024 * 1024,
                blob: true,
            },
        ),
        (18, ProbeMutation::UnrelatedConfigValue { bytes: 8 * 1024 * 1024 }),
    ] {
        let temp = tempfile::tempdir().unwrap();
        let candidate = temp.path().join("collection.anki2");
        fs::copy(fixture(&format!("storage/schema-{version}.anki2")), &candidate).unwrap();
        mutate_probe_candidate(&candidate, mutation);
        let mut e = entries();
        e[1].1 = frame(&fs::read(candidate).unwrap());
        let bytes = archive(&e, CompressionMethod::Stored);
        let source = temp.path().join("input.colpkg");
        fs::write(&source, &bytes).unwrap();
        let result = inspect(&source, temp.path(), &StatvfsSpace, &|| false, 0, 0).unwrap();
        assert_eq!(result.facts().ver_found, version);
        assert_eq!(fs::read(&source).unwrap(), bytes);
    }
}

#[test]
fn coordinated_copy_inspection_refusals_preserve_live_source_and_unrelated_staging() {
    use meguri_engine::{CancelToken, Progress, ProgressListener, ProgressPhase, ProgressSink};
    use std::sync::Arc;
    struct CancelCopy(Arc<CancelToken>);
    impl ProgressListener for CancelCopy {
        fn on_progress(&self, p: &Progress) {
            if p.phase == ProgressPhase::Named("copy") {
                self.0.cancel();
            }
        }
    }
    for mode in ["media", "legacy", "invalid", "cancel", "space", "success"] {
        let directory = tempfile::tempdir().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("selected.colpkg");
        let mut e = entries();
        let bytes = match mode {
            "media" => {
                e.push(("0", b"media bytes".to_vec()));
                archive(&e, CompressionMethod::Stored)
            }
            "legacy" => {
                e.remove(0);
                archive(&e, CompressionMethod::Stored)
            }
            "invalid" => b"not a package".to_vec(),
            _ => archive(&e, CompressionMethod::Stored),
        };
        fs::write(&source, &bytes).unwrap();
        fs::write(directory.path().join("collection.anki2"), b"untouched live bytes").unwrap();
        let unrelated = directory.path().join(".meguri-stage-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep"), b"owned elsewhere").unwrap();
        let before = listing(directory.path());
        let cancel = CancelToken::new();
        let sink = ProgressSink::new();
        if mode == "cancel" {
            sink.set_listener(Arc::new(CancelCopy(cancel.clone())));
        }
        struct Zero;
        impl SpaceProvider for Zero {
            fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
                Ok(0)
            }
        }
        let space: &dyn SpaceProvider = if mode == "space" { &Zero } else { &StatvfsSpace };
        let result = meguri_package::copy_and_inspect_collection_package(
            &source,
            &CollectionPackageInspectContext {
                collection_directory: directory.path(),
                live_collection_size: 20,
                reserved_space_bytes: 8 * 1024 * 1024,
                clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
                random: &SeededRandom::new(43),
                space,
                is_cancelled: &|| cancel.is_cancelled(),
            },
            &sink,
        );
        match (mode, &result) {
            ("success", Ok((inspected, _, digest))) => {
                use sha2::{Digest, Sha256};
                assert_eq!(*digest, <[u8; 32]>::from(Sha256::digest(&bytes)));
                assert_eq!(inspected.facts().ver_found, 18);
            }
            ("media", Err(Error::UnsupportedMedia))
            | ("legacy", Err(Error::UnsupportedFormat))
            | ("invalid", Err(Error::Corrupt { .. }))
            | ("cancel", Err(Error::Cancelled))
            | ("space", Err(Error::InsufficientSpace { .. })) => {}
            _ => panic!("unexpected copy inspection result for {mode}: {result:?}"),
        }
        drop(result);
        assert_eq!(listing(directory.path()), before);
        assert_eq!(
            fs::read(directory.path().join("collection.anki2")).unwrap(),
            b"untouched live bytes"
        );
        assert_eq!(fs::read(unrelated.join("keep")).unwrap(), b"owned elsewhere");
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}

#[test]
fn combined_copy_preflight_reserves_source_extraction_upgrade_and_backup_before_first_write() {
    struct Empty<'a>(&'a Path);
    impl SpaceProvider for Empty<'_> {
        fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
            assert_eq!(directory, self.0);
            assert_eq!(listing(directory).len(), 0);
            Ok(0)
        }
    }
    let raw = fs::read(fixture("storage/schema-18.anki2")).unwrap();
    for declared in [true, false] {
        let mut e = entries();
        if !declared {
            e[1].1 = zstd::stream::encode_all(raw.as_slice(), 3).unwrap();
        }
        let dir = tempfile::tempdir().unwrap();
        let source_dir = tempfile::tempdir().unwrap();
        let source = source_dir.path().join("input.colpkg");
        let bytes = archive(&e, CompressionMethod::Stored);
        fs::write(&source, &bytes).unwrap();
        let live = 100_000_000;
        let backup = 456;
        let estimate = if declared {
            raw.len() as u64
        } else {
            (e[1].1.len() as u64 * 4).max(live)
        };
        let result = meguri_package::copy_and_inspect_collection_package(
            &source,
            &CollectionPackageInspectContext {
                collection_directory: dir.path(),
                live_collection_size: live,
                reserved_space_bytes: backup,
                clock: &FixedClock::new(1_894_701_600_000, UtcOffset::UTC),
                random: &SeededRandom::new(42),
                space: &Empty(dir.path()),
                is_cancelled: &|| false,
            },
            &meguri_engine::ProgressSink::new(),
        );
        assert!(
            matches!(result, Err(Error::InsufficientSpace { needed, available: 0 }) if needed == estimate * 2 + bytes.len() as u64 + backup + 4 * 1024 * 1024)
        );
        assert_eq!(listing(dir.path()).len(), 0);
        assert_eq!(fs::read(source).unwrap(), bytes);
    }
}
