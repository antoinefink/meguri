mod common;

use meguri_storage::media::{MediaError, MediaInventory, MediaStore, UnsupportedMedia};
use std::{
    fs::{self, File, FileTimes},
    os::unix::{ffi::OsStrExt, fs::{MetadataExt, symlink}},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, UNIX_EPOCH},
};

fn open(root: &Path) -> MediaStore {
    MediaStore::open(
        &root.join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap()
}

fn inventory(store: &MediaStore, limit: u64) -> MediaInventory {
    store.promised_inventory(limit, &mut || true).unwrap()
}

#[test]
fn inventory_lists_every_class_in_name_order_with_observed_sizes() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join("supported.bin"), b"abc").unwrap();
    fs::write(store.media_dir().join("empty.bin"), b"").unwrap();
    fs::write(store.media_dir().join("large.bin"), b"abcde").unwrap();
    fs::write(store.media_dir().join("invalid?.bin"), b"invalid").unwrap();
    fs::create_dir(store.media_dir().join("directory")).unwrap();
    fs::create_dir(store.media_dir().join("invalid?directory")).unwrap();
    symlink("supported.bin", store.media_dir().join("link.bin")).unwrap();
    fs::write(store.media_dir().join(".DS_Store"), b"ignored").unwrap();
    let listed = inventory(&store, 4);
    assert_eq!(listed.max_entry_bytes(), 4);
    assert_eq!(listed.ignored(), 1);
    let facts: Vec<_> = listed.files().iter().map(|f| (f.name.as_str(), f.size, f.unsupported)).collect();
    assert_eq!(facts, [
        ("directory", 0, Some(UnsupportedMedia::NotRegularFile)),
        ("empty.bin", 0, Some(UnsupportedMedia::Empty)),
        ("invalid?.bin", 7, Some(UnsupportedMedia::InvalidName)),
        ("invalid?directory", 0, Some(UnsupportedMedia::InvalidName)),
        ("large.bin", 5, Some(UnsupportedMedia::OverLimit)),
        ("link.bin", 0, Some(UnsupportedMedia::NotRegularFile)),
        ("supported.bin", 3, None),
    ]);
    assert!(listed.files().iter().all(|f| !f.lossy_name));
    let debug = format!("{listed:?}");
    for name in facts.iter().map(|f| f.0).chain([".DS_Store"]) {
        assert!(!debug.contains(name));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn inventory_retains_non_utf8_names_as_lossy_exclusions_without_stat_facts() {
    use std::ffi::OsStr;
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let disk = OsStr::from_bytes(b"invalid-\xff.bin");
    fs::write(store.media_dir().join(disk), b"bytes").unwrap();
    let listed = inventory(&store, 4);
    assert_eq!(listed.files().len(), 1);
    let file = &listed.files()[0];
    assert_eq!(file.name, disk.to_string_lossy());
    assert!(file.lossy_name);
    assert_eq!(file.unsupported, Some(UnsupportedMedia::InvalidName));
    assert_eq!(file.size, 0);
}

#[cfg(target_os = "linux")]
#[test]
fn duplicate_nfc_spellings_exclude_decomposed_entries_and_put_canonical_first() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join("é.bin"), b"abc").unwrap();
    fs::write(store.media_dir().join("e\u{301}.bin"), b"second").unwrap();
    let listed = inventory(&store, 4);
    assert_eq!(listed.files().len(), 2);
    assert_eq!(listed.files()[0].name, "é.bin");
    assert_eq!(listed.files()[0].unsupported, None);
    assert_eq!(listed.files()[0].size, 3);
    assert_eq!(listed.files()[1].name, "é.bin");
    assert_eq!(listed.files()[1].unsupported, Some(UnsupportedMedia::InvalidName));
    assert_eq!(listed.files()[1].size, 6);
}

#[test]
fn lone_decomposed_spelling_is_supported_without_renaming() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let path = store.media_dir().join("e\u{301}.bin");
    fs::write(&path, b"abc").unwrap();
    let before = snapshot(store.media_dir());
    let listed = inventory(&store, 4);
    assert_eq!(listed.files().len(), 1);
    assert_eq!(listed.files()[0].name, "é.bin");
    assert_eq!(listed.files()[0].unsupported, None);
    assert_eq!(snapshot(store.media_dir()), before);
}

#[test]
fn journal_matches_require_supported_exact_name_and_equal_size_and_mtime() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let path = store.media_dir().join("facts.bin");
    fs::write(&path, b"abc").unwrap();
    File::open(&path).unwrap().set_times(FileTimes::new().set_modified(
        UNIX_EPOCH + Duration::new(1_700_000_000, 123_000_000),
    )).unwrap();
    store.check_changes().unwrap();
    let entry = store.entries().unwrap().into_iter().find(|e| e.name == "facts.bin").unwrap();
    assert!(inventory(&store, 4).files()[0].matches_journal(&entry));
    assert!(!inventory(&store, 2).files()[0].matches_journal(&entry));
    let old_time = fs::metadata(&path).unwrap().modified().unwrap();
    File::open(&path).unwrap().set_times(FileTimes::new().set_modified(
        old_time + Duration::from_nanos(1),
    )).unwrap();
    let changed = inventory(&store, 4);
    assert_eq!(fs::metadata(&path).unwrap().mtime(), entry.mtime_secs);
    assert_ne!(fs::metadata(&path).unwrap().mtime_nsec(), i64::from(entry.mtime_nanos));
    assert!(!changed.files()[0].matches_journal(&entry));
    File::open(&path).unwrap().set_times(FileTimes::new().set_modified(
        old_time + Duration::from_secs(1),
    )).unwrap();
    assert!(!inventory(&store, 4).files()[0].matches_journal(&entry));
    fs::write(&path, b"abcd").unwrap();
    File::open(&path).unwrap().set_times(FileTimes::new().set_modified(old_time)).unwrap();
    assert!(!inventory(&store, 4).files()[0].matches_journal(&entry));
    let mut other = entry.clone();
    other.name = "other.bin".into();
    let mut absent = entry.clone();
    absent.live = false;
    fs::write(&path, b"abc").unwrap();
    File::open(&path).unwrap().set_times(FileTimes::new().set_modified(old_time)).unwrap();
    let restored = inventory(&store, 4);
    assert!(restored.files()[0].matches_journal(&entry));
    assert!(!restored.files()[0].matches_journal(&other));
    assert!(!restored.files()[0].matches_journal(&absent));
}

#[test]
fn inventory_is_bound_to_the_listed_directory_inode() {
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let other = open(other_dir.path());
    let listed = inventory(&store, 4);
    assert!(listed.is_from(&store));
    assert!(!listed.is_from(&other));
    let moved = dir.path().join("old-media");
    fs::rename(store.media_dir(), &moved).unwrap();
    assert!(!listed.is_from(&store));
    fs::create_dir(store.media_dir()).unwrap();
    assert!(!listed.is_from(&store));
}

// Captures journal/sidecar bytes, recursive listings, symlink bytes and all
// modification times, without touching a journal API that could recover.
type Snapshot = Vec<(PathBuf, u64, i64, i64, Vec<u8>)>;
fn snapshot(root: &Path) -> Snapshot {
    fn visit(root: &Path, path: &Path, result: &mut Snapshot) {
        let m = fs::symlink_metadata(path).unwrap();
        let bytes = if m.is_file() {
            fs::read(path).unwrap()
        } else if m.file_type().is_symlink() {
            fs::read_link(path).unwrap().as_os_str().as_bytes().to_vec()
        } else {
            Vec::new()
        };
        result.push((path.strip_prefix(root).unwrap().into(), m.len(), m.mtime(), m.mtime_nsec(), bytes));
        if m.is_dir() {
            for entry in fs::read_dir(path).unwrap() {
                visit(root, &entry.unwrap().path(), result);
            }
        }
    }
    let mut result = Vec::new();
    visit(root, root, &mut result);
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[test]
fn inventory_never_recovers_a_pending_removal_receipt_or_changes_files() {
    use meguri_model::faultpoint::{self, Fault};
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.ensure_static_media("removed.bin", b"original").unwrap();
    faultpoint::arm("media.trash.after_move_before_observation", Fault::IoError(std::io::ErrorKind::Other));
    let result = store.trash(&["removed.bin".into()]);
    faultpoint::disarm("media.trash.after_move_before_observation");
    assert!(matches!(result, Err(MediaError::Io(_))));
    assert!(!store.media_dir().join("removed.bin").exists());
    fs::write(store.media_dir().join("e\u{301}.bin"), b"abc").unwrap();
    fs::write(store.media_dir().join("empty.bin"), b"").unwrap();
    let before = snapshot(dir.path());
    assert!(before.iter().any(|e| e.0.file_name().is_some_and(|n| n == "intent.json")));
    let diagnostics = store.diagnostics();
    let listed = inventory(&store, 4);
    assert_eq!(listed.files().len(), 2);
    assert_eq!(snapshot(dir.path()), before);
    assert_eq!(store.diagnostics(), diagnostics);
    // This read does recover, proving the pre-existing receipt was actionable.
    store.entries().unwrap();
    assert_ne!(snapshot(dir.path()), before);
}

#[test]
fn cancellation_is_polled_before_listing_and_for_each_entry_without_a_guard() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    assert_eq!(store.promised_inventory(4, &mut || false), Err(MediaError::Cancelled));
    fs::write(store.media_dir().join("one.bin"), b"a").unwrap();
    fs::write(store.media_dir().join("two.bin"), b"b").unwrap();
    let mut calls = 0;
    let result = store.promised_inventory(4, &mut || {
        store.entries().unwrap();
        calls += 1;
        calls < 3
    });
    assert_eq!(result, Err(MediaError::Cancelled));
    assert_eq!(calls, 3);
}

#[test]
fn closed_and_import_owned_stores_refuse_before_listing_or_callbacks() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let owner = store.own_collection_import().unwrap();
    assert_eq!(
        store.promised_inventory(4, &mut || panic!("owned store invoked callback")),
        Err(MediaError::InvalidManifest("collection media replacement owns the store".into())),
    );
    drop(owner);
    assert!(store.try_close().unwrap());
    fs::rename(store.media_dir(), dir.path().join("suspended-media")).unwrap();
    assert_eq!(
        store.promised_inventory(4, &mut || panic!("closed store invoked callback")),
        Err(MediaError::Database("media journal is closed".into())),
    );
}
