mod common;
use meguri_storage::media::MediaStore;
use std::{fs, sync::Arc};

#[test]
fn media_restore_never_discards_bytes_owned_only_by_an_uncommitted_stage() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    media.add("a.png", b"recoverable").unwrap();
    media.trash(&["a.png".into()]).unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("a.png", b"recoverable").unwrap();
    media.restore().unwrap();
    drop(stage);
    assert_eq!(fs::read(media.media_dir().join("a.png")).unwrap(), b"recoverable");
}
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

#[test]
fn media_trash_preserves_a_255_byte_name_even_without_the_journal() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    let name = format!("{}.png", "x".repeat(251));
    fs::write(media.media_dir().join(&name), b"long outside name").unwrap();
    media.trash(std::slice::from_ref(&name)).unwrap();
    assert_eq!(media.trash_summary().unwrap().entries, 1);
    drop(media);
    fs::remove_file(dir.path().join("meguri-media.db")).unwrap();
    let media = store(dir.path());
    assert_eq!(media.trash_entries().unwrap()[0].name, name);
    media.restore().unwrap();
    assert_eq!(fs::read(media.media_dir().join(&name)).unwrap(), b"long outside name");
    assert_eq!(media.trash_summary().unwrap().entries, 0);
}

#[test]
fn media_recovery_still_restores_the_earlier_flat_trash_layout() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    fs::write(media.trash_dir().join("17-1--earlier.png"), b"earlier version").unwrap();
    drop(media);
    let media = store(dir.path());
    assert_eq!(media.trash_summary().unwrap().entries, 1);
    media.restore().unwrap();
    assert_eq!(
        fs::read(media.media_dir().join("earlier.png")).unwrap(),
        b"earlier version"
    );
    assert_eq!(media.trash_summary().unwrap().entries, 0);
}

#[test]
fn media_restore_readds_an_occupied_untracked_hash_name() {
    let dir = tempfile::tempdir().unwrap();
    let media = store(dir.path());
    media.add("readme", b"first").unwrap();
    let name = media.add("readme", b"second").unwrap();
    assert!(name.ends_with('.'));
    media.trash(std::slice::from_ref(&name)).unwrap();
    fs::write(media.media_dir().join(&name), b"outside").unwrap();
    media.restore().unwrap();
    assert_eq!(fs::read(media.media_dir().join(&name)).unwrap(), b"outside");
    let restored = fs::read_dir(media.media_dir())
        .unwrap()
        .filter_map(|e| {
            let p = e.unwrap().path();
            (fs::read(&p).unwrap() == b"second").then_some(p)
        })
        .collect::<Vec<_>>();
    assert_eq!(restored.len(), 1);
    assert_eq!(media.trash_summary().unwrap().entries, 0);
}
