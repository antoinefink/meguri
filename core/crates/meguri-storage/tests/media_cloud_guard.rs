mod common;
use meguri_model::faultpoint::{self, Fault};
use meguri_storage::media::{CloudInstall, MediaStore};
use sha1::{Digest, Sha1};
use std::{
    fs,
    path::PathBuf,
    sync::{Arc, Mutex},
};

fn open(root: &std::path::Path) -> MediaStore {
    MediaStore::open(
        &root.join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap()
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
static TARGET: Mutex<Option<PathBuf>> = Mutex::new(None);
fn insert_user_file() {
    fs::write(TARGET.lock().unwrap().as_ref().unwrap(), b"synthetic user bytes").unwrap();
}

#[test]
fn expected_absence_reaches_publication_without_overwriting_a_racing_file() {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    let incoming = root.path().join("incoming");
    fs::write(&incoming, b"synthetic server bytes").unwrap();
    let target = store.media_dir().join("photo.jpg");
    *TARGET.lock().unwrap() = Some(target.clone());
    faultpoint::arm("media.install.before_publish", Fault::Callback(insert_user_file));
    let result = store.cloud_install("photo.jpg", &incoming, None);
    faultpoint::disarm("media.install.before_publish");
    TARGET.lock().unwrap().take();
    assert_eq!(result.unwrap(), CloudInstall::Changed);
    assert_eq!(fs::read(target).unwrap(), b"synthetic user bytes");
    assert_eq!(fs::read(incoming).unwrap(), b"synthetic server bytes");
    assert!(store.trash_entries().unwrap().is_empty());
}

#[test]
fn preexisting_hash_bytes_without_a_move_receipt_never_prove_completion() {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    let bytes = b"synthetic original content";
    store.add("photo.jpg", bytes).unwrap();
    let new = store.cloud_hash_name("photo.jpg", &hash(bytes)).unwrap();
    store.add(&new, bytes).unwrap();
    let proof = store
        .cloud_move_evidence("photo.jpg", &new, &hash(bytes))
        .unwrap()
        .unwrap();
    drop(store);
    fs::write(
        root.path().join("collection.media/photo.jpg"),
        b"synthetic changed original",
    )
    .unwrap();
    let store = open(root.path());
    assert!(
        !store
            .cloud_move_completed("photo.jpg", &new, &hash(bytes), &proof)
            .unwrap()
    );
    assert!(
        !store
            .cloud_move_guarded("photo.jpg", &new, &hash(bytes), &proof)
            .unwrap()
    );
    assert_eq!(
        fs::read(store.media_dir().join("photo.jpg")).unwrap(),
        b"synthetic changed original"
    );
}

#[test]
fn actual_preexisting_destination_move_receipt_survives_reopen_and_a_new_old_name() {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    let bytes = b"synthetic original content";
    store.add("photo.jpg", bytes).unwrap();
    let new = store.cloud_hash_name("photo.jpg", &hash(bytes)).unwrap();
    store.add(&new, bytes).unwrap();
    let proof = store
        .cloud_move_evidence("photo.jpg", &new, &hash(bytes))
        .unwrap()
        .unwrap();
    assert!(
        store
            .cloud_move_guarded("photo.jpg", &new, &hash(bytes), &proof)
            .unwrap()
    );
    assert_eq!(store.trash_entries().unwrap().len(), 1);
    drop(store);
    fs::write(
        root.path().join("collection.media/photo.jpg"),
        b"synthetic new old name",
    )
    .unwrap();
    let store = open(root.path());
    assert!(
        store
            .cloud_move_completed("photo.jpg", &new, &hash(bytes), &proof)
            .unwrap()
    );
    assert_eq!(
        fs::read(store.media_dir().join("photo.jpg")).unwrap(),
        b"synthetic new old name"
    );
    assert_eq!(fs::read(store.media_dir().join(new)).unwrap(), bytes);
}

fn replace_before_trash() {
    faultpoint::disarm("media.trash.before_move");
    let path = TARGET.lock().unwrap().take().unwrap();
    let mut replacement = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
    std::io::Write::write_all(&mut replacement, b"synthetic camera edit").unwrap();
    replacement.as_file().sync_all().unwrap();
    replacement.persist(path).unwrap();
}
fn edit_before_trash() {
    use std::os::unix::fs::MetadataExt;
    faultpoint::disarm("media.trash.before_move");
    let path = TARGET.lock().unwrap().take().unwrap();
    let before = fs::metadata(&path).unwrap();
    fs::write(&path, b"synthetic camera edit").unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
}
fn guarded_trash_rejects_write(callback: fn()) {
    use sha2::Digest;
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    let original = b"synthetic original content";
    store.add("photo.jpg", original).unwrap();
    let target = store.media_dir().join("photo.jpg");
    *TARGET.lock().unwrap() = Some(target.clone());
    faultpoint::arm("media.trash.before_move", Fault::Callback(callback));
    let digest: [u8; 32] = sha2::Sha256::digest(original).into();
    let removed = store.cloud_trash("photo.jpg", &hash(original), &digest, original.len() as u64);
    faultpoint::disarm("media.trash.before_move");
    assert!(!removed.unwrap());
    assert_eq!(fs::read(&target).unwrap(), b"synthetic camera edit");
    assert!(store.trash_entries().unwrap().is_empty());
    drop(store);
    let store = open(root.path());
    assert_eq!(fs::read(target).unwrap(), b"synthetic camera edit");
    assert!(store.trash_entries().unwrap().is_empty());
}
#[test]
fn cloud_trash_refuses_replacement_after_its_content_check() {
    guarded_trash_rejects_write(replace_before_trash);
}
#[test]
fn cloud_trash_refuses_same_inode_write_after_its_content_check() {
    guarded_trash_rejects_write(edit_before_trash);
}

fn edit_during_move() {
    use std::os::unix::fs::MetadataExt;
    let mut path = TARGET.lock().unwrap().take().unwrap();
    if path.is_dir() {
        path = fs::read_dir(path)
            .unwrap()
            .map(|entry| entry.unwrap().path().join("photo.jpg"))
            .find(|path| path.is_file())
            .unwrap();
    }
    let before = fs::metadata(&path).unwrap();
    fs::write(&path, b"synthetic camera edit").unwrap();
    let after = fs::metadata(&path).unwrap();
    assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
}
fn move_refuses_in_place_edit(destination_present: bool, after_move: bool) {
    let root = tempfile::tempdir().unwrap();
    let store = open(root.path());
    let original = b"synthetic original content";
    store.add("photo.jpg", original).unwrap();
    let new = store.cloud_hash_name("photo.jpg", &hash(original)).unwrap();
    if destination_present {
        store.add(&new, original).unwrap();
    }
    let proof = store
        .cloud_move_evidence("photo.jpg", &new, &hash(original))
        .unwrap()
        .unwrap();
    let point = match (destination_present, after_move) {
        (true, false) => "media.trash.before_move",
        (true, true) => "media.trash.after_move_before_verify",
        (false, false) => "media.cloud_move.before_move",
        (false, true) => "media.cloud_move.after_move_before_verify",
    };
    let edit_path = if after_move && destination_present {
        store.trash_dir().to_path_buf()
    } else if after_move {
        store.media_dir().join(&new)
    } else {
        store.media_dir().join("photo.jpg")
    };
    *TARGET.lock().unwrap() = Some(edit_path);
    faultpoint::arm(point, Fault::Callback(edit_during_move));
    let result = store.cloud_move_guarded("photo.jpg", &new, &hash(original), &proof);
    faultpoint::disarm(point);
    assert!(!result.unwrap());
    assert!(
        !store
            .cloud_move_completed("photo.jpg", &new, &hash(original), &proof)
            .unwrap()
    );
    assert_eq!(
        fs::read(store.media_dir().join("photo.jpg")).unwrap(),
        b"synthetic camera edit"
    );
    if destination_present {
        assert_eq!(fs::read(store.media_dir().join(&new)).unwrap(), original);
    } else {
        assert!(!store.media_dir().join(&new).exists());
    }
    assert!(store.trash_entries().unwrap().is_empty());
    drop(store);
    let store = open(root.path());
    assert!(
        !store
            .cloud_move_completed("photo.jpg", &new, &hash(original), &proof)
            .unwrap()
    );
    assert_eq!(
        fs::read(store.media_dir().join("photo.jpg")).unwrap(),
        b"synthetic camera edit"
    );
    assert!(store.trash_entries().unwrap().is_empty());
}
#[test]
fn dedup_move_refuses_same_inode_write_after_hashing() {
    move_refuses_in_place_edit(true, false);
}
#[test]
fn new_destination_move_refuses_same_inode_write_after_hashing() {
    move_refuses_in_place_edit(false, false);
}
#[test]
fn dedup_move_restores_same_inode_write_after_rename() {
    move_refuses_in_place_edit(true, true);
}
#[test]
fn new_destination_move_restores_same_inode_write_after_rename() {
    move_refuses_in_place_edit(false, true);
}
