mod common;
use meguri_storage::media::MediaStore;
use std::{fs, sync::Arc};
fn store(dir: &std::path::Path) -> MediaStore {
    MediaStore::open(&dir.join("collection.anki2"), Arc::new(common::clock())).unwrap()
}
#[test]
fn media_trash_survives_a_deleted_journal_and_restores_newest_numeric_version_first() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    for version in 0..12 {
        media.add("a.png", version.to_string().as_bytes()).unwrap();
        media.trash(&["a.png".into()]).unwrap();
    }
    assert_eq!(media.trash_summary().unwrap().entries, 12);
    drop(media);
    fs::remove_file(dir.path().join("meguri-media.db")).unwrap();
    let media = store(dir.path());
    media.restore().unwrap();
    assert_eq!(fs::read(media.media_dir().join("a.png")).unwrap(), b"11");
    assert_eq!(media.trash_summary().unwrap().entries, 0);
    assert_eq!(fs::read_dir(media.media_dir()).unwrap().count(), 12);
}
#[test]
fn media_restore_hash_collision_preserves_the_prior_hash_named_file_in_trash() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    media.add("a.png", b"new").unwrap();
    media.trash(&["a.png".into()]).unwrap();
    media.add("a.png", b"old").unwrap();
    let hash_name = media.add("a.png", b"new").unwrap();
    fs::write(media.media_dir().join(&hash_name), b"outside").unwrap();
    media.register_full(|_, _| true).unwrap();
    media.restore().unwrap();
    assert_eq!(fs::read(media.media_dir().join(&hash_name)).unwrap(), b"new");
    assert_eq!(fs::read(media.media_dir().join("a.png")).unwrap(), b"old");
    assert_eq!(media.trash_summary().unwrap().entries, 1);
}
