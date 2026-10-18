mod common;
use meguri_storage::media::{MediaStore, NoteReference};
use std::{fs, sync::Arc};

fn open(dir: &std::path::Path) -> MediaStore {
    MediaStore::open(&dir.join("collection.anki2"), Arc::new(common::clock())).unwrap()
}
fn reference() -> NoteReference {
    NoteReference {
        note_id: 1,
        mtime: 17,
        fields_sha256: "aa".repeat(32),
    }
}
#[test]
fn media_staging_rollback_removes_only_unpublished_bytes_and_journal_entries() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let before = store.change_counter().unwrap();
    {
        let mut op = store.begin_staging().unwrap();
        assert_eq!(op.add("Photo.JPG", b"a").unwrap(), "photo.jpg");
        assert_eq!(op.add("PHOTO.JPG", b"a").unwrap(), "photo.jpg");
        assert!(op.add("Photo.JPG", b"b").unwrap().starts_with("photo-"));
        assert_eq!(fs::read_dir(store.media_dir()).unwrap().count(), 0);
    }
    assert_eq!(fs::read_dir(store.staging_dir()).unwrap().count(), 0);
    assert_eq!(store.change_counter().unwrap(), before);
    assert!(store.pending().unwrap().is_empty());
}
#[test]
fn media_staging_committed_recovery_publishes_without_a_post_commit_flag() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut op = store.begin_staging().unwrap();
    op.add("Photo.JPG", b"committed").unwrap();
    op.set_references(&[reference()]).unwrap();
    std::mem::forget(op); // Simulate abrupt process exit; no destructor ran.
    drop(store);
    let store = open(dir.path());
    let report = store.recover(|r| Ok(r == &reference())).unwrap();
    assert_eq!(report.published, 1);
    assert_eq!(fs::read(store.media_dir().join("photo.jpg")).unwrap(), b"committed");
    assert_eq!(store.recover(|_| Ok(false)).unwrap().published, 0);
    assert_eq!(store.trash_summary().unwrap().entries, 0);
}
#[test]
fn media_staging_uncommitted_recovery_preserves_bytes_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut op = store.begin_staging().unwrap();
    op.add("Photo.JPG", b"uncommitted").unwrap();
    op.set_references(&[reference()]).unwrap();
    std::mem::forget(op);
    let report = store.recover(|_| Ok(false)).unwrap();
    assert_eq!(report.trashed, 1);
    assert_eq!(fs::read_dir(store.media_dir()).unwrap().count(), 0);
    assert_eq!(store.trash_summary().unwrap().entries, 1);
    store.restore().unwrap();
    assert_eq!(fs::read(store.media_dir().join("photo.jpg")).unwrap(), b"uncommitted");
}
#[test]
fn media_staging_same_content_in_another_op_survives_the_first_rollback() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let mut first = store.begin_staging().unwrap();
    first.add("Photo.JPG", b"shared").unwrap();
    let mut second = store.begin_staging().unwrap();
    assert_eq!(second.add("Photo.JPG", b"shared").unwrap(), "photo.jpg");
    drop(first);
    second.set_references(&[reference()]).unwrap();
    second.publish().unwrap();
    assert_eq!(fs::read(store.media_dir().join("photo.jpg")).unwrap(), b"shared");
}
