mod common;

use meguri_storage::media::{MediaError, MediaStore};
use std::{fs, sync::Arc};

fn store() -> (tempfile::TempDir, MediaStore) {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
    (dir, store)
}

#[test]
fn media_add_lowercases_deduplicates_and_hashes_conflicts() {
    let (_dir, store) = store();
    assert_eq!(store.add("Photo.JPG", b"first").unwrap(), "photo.jpg");
    let before = store.change_counter().unwrap();
    assert_eq!(store.add("PHOTO.JPG", b"first").unwrap(), "photo.jpg");
    assert_eq!(store.change_counter().unwrap(), before);
    let other = store.add("photo.jpg", b"other").unwrap();
    assert!(other.starts_with("photo-") && other.ends_with(".jpg"));
    assert_eq!(other.len(), "photo-".len() + 40 + ".jpg".len());
    assert_eq!(fs::read(store.media_dir().join("photo.jpg")).unwrap(), b"first");
    assert_eq!(fs::read(store.media_dir().join(other)).unwrap(), b"other");
}

#[test]
fn media_lookup_preserves_existing_spelling_and_rejects_paths() {
    let (_dir, store) = store();
    fs::write(store.media_dir().join("MixedCase.png"), b"original").unwrap();
    store.register_full(|_, _| true).unwrap();
    assert_eq!(store.lookup("mixedcase.png").unwrap().as_deref(), Some("MixedCase.png"));
    assert_eq!(store.add("MIXEDCASE.PNG", b"original").unwrap(), "MixedCase.png");
    assert_eq!(store.lookup("../secret").unwrap(), None);
    assert_eq!(store.lookup("MixedCase.png/other").unwrap(), None);
}

#[test]
fn media_ignored_names_empty_names_and_symlinks_never_enter_index() {
    let (dir, store) = store();
    assert_eq!(store.add("???", b"a"), Err(MediaError::InvalidName));
    for name in ["Thumbs.DB", "thumbſ-db", ".DS_Store", "xds_store"] {
        assert_eq!(store.add(name, b"a"), Err(MediaError::IgnoredName));
        fs::write(store.media_dir().join(name), b"ignored").unwrap();
    }
    let outside = dir.path().join("outside");
    fs::write(&outside, b"private").unwrap();
    std::os::unix::fs::symlink(&outside, store.media_dir().join("link.png")).unwrap();
    fs::create_dir(store.media_dir().join("folder.png")).unwrap();
    store.register_full(|_, _| true).unwrap();
    assert!(store.pending().unwrap().is_empty());
    assert_eq!(store.lookup("link.png").unwrap(), None);
    assert_eq!(store.lookup("folder.png").unwrap(), None);
}

#[test]
fn media_static_same_bytes_never_change_the_journal() {
    let (_dir, store) = store();
    store.ensure_static_media("_Logo.PNG", b"v1").unwrap();
    let before = store.change_counter().unwrap();
    for _ in 0..100 {
        store.ensure_static_media("_Logo.PNG", b"v1").unwrap();
    }
    assert_eq!(store.change_counter().unwrap(), before);
    store.ensure_static_media("_Logo.PNG", b"v2").unwrap();
    assert_eq!(fs::read(store.media_dir().join("_Logo.PNG")).unwrap(), b"v2");
    assert_eq!(store.trash_summary().unwrap().entries, 1);
}

#[test]
fn media_empty_and_oversized_files_are_local_but_not_pending() {
    let (_dir, store) = store();
    assert_eq!(store.add("empty.png", b"").unwrap(), "empty.png");
    let path = store.media_dir().join("large.mp4");
    let f = fs::File::create(&path).unwrap();
    f.set_len(104_857_601).unwrap();
    store.register_full(|_, _| true).unwrap();
    assert_eq!(store.lookup("large.mp4").unwrap().as_deref(), Some("large.mp4"));
    assert!(store.pending().unwrap().is_empty());
}

#[test]
fn media_directory_collision_does_not_overwrite_or_follow_it() {
    let (_dir, store) = store();
    fs::create_dir(store.media_dir().join("image.png")).unwrap();
    let name = store.add("image.png", b"safe").unwrap();
    assert_ne!(name, "image.png");
    assert!(store.media_dir().join("image.png").is_dir());
    assert_eq!(fs::read(store.media_dir().join(name)).unwrap(), b"safe");
}

#[test]
fn media_immediate_add_reusing_an_uncommitted_name_keeps_independent_bytes() {
    let (_dir, store) = store();
    let mut op = store.begin_staging().unwrap();
    let staged = op.add("a.jpg", b"same").unwrap();
    assert_eq!(store.add("a.jpg", b"same").unwrap(), staged);
    drop(op);
    assert_eq!(fs::read(store.media_dir().join(staged)).unwrap(), b"same");
}

#[test]
fn media_hash_name_reuses_a_present_case_variant() {
    let (_dir, store) = store();
    store.add("photo.jpg", b"a").unwrap();
    let name = store.add("photo.jpg", b"b").unwrap();
    let upper = name.to_uppercase();
    fs::rename(store.media_dir().join(&name), store.media_dir().join(&upper)).unwrap();
    store.register_full(|_, _| true).unwrap();
    assert_eq!(store.add("photo.jpg", b"b").unwrap(), upper);
}

#[test]
fn media_failed_static_publication_restores_bytes_and_the_exact_journal_state() {
    let (_dir, store) = store();
    store.ensure_static_media("_asset.png", b"old").unwrap();
    for pending in store.pending().unwrap() {
        store.acknowledge(&pending.name, pending.change_counter).unwrap();
    }
    let entries = store.entries().unwrap();
    let counter = store.change_counter().unwrap();
    meguri_model::faultpoint::arm(
        "storage.atomic.before_rename",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = store.ensure_static_media("_asset.png", b"new");
    meguri_model::faultpoint::disarm_all();
    assert!(result.is_err());
    assert_eq!(fs::read(store.media_dir().join("_asset.png")).unwrap(), b"old");
    assert_eq!(store.entries().unwrap(), entries);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(store.trash_summary().unwrap().entries, 0);
}

#[test]
fn media_post_rename_error_is_not_reported_as_an_unchanged_name_collision() {
    let (_dir, store) = store();
    meguri_model::faultpoint::arm(
        "storage.atomic.after_rename",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = store.add("a.jpg", b"published");
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(result, Err(MediaError::Io(_))));
    assert_eq!(fs::read(store.media_dir().join("a.jpg")).unwrap(), b"published");
    store.check_changes().unwrap();
    assert_eq!(store.pending().unwrap().len(), 1);
}
