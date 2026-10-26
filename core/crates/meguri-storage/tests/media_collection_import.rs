mod common;
use meguri_storage::media::{IncomingMedia, MediaError, MediaStore};
use sha1::{Digest, Sha1};
use std::{
    fs,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};
static CANCEL_PREPARATION: AtomicBool = AtomicBool::new(false);
fn cancel_preparation() {
    CANCEL_PREPARATION.store(true, Ordering::Release);
}
fn item(name: &str, value: &[u8]) -> IncomingMedia {
    IncomingMedia {
        name: name.into(),
        length: value.len() as u64,
        sha1: format!("{:x}", Sha1::digest(value)),
    }
}
#[test]
fn media_import_partial_preparation_cleans_reservations_and_preserves_unrelated_evidence() {
    for cancelled in [false, true] {
        CANCEL_PREPARATION.store(false, Ordering::Release);
        let root = tempfile::tempdir().unwrap();
        let stage = tempfile::tempdir().unwrap();
        let store = MediaStore::open(&root.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
        store.add("collision.bin", b"old bytes").unwrap();
        let unrelated = store.staging_dir().join("r6-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("keep.bin"), b"unrelated recovery").unwrap();
        let trash = store.trash_dir().join("unrelated-trash");
        fs::create_dir(&trash).unwrap();
        fs::write(trash.join("keep.bin"), b"unrelated trash").unwrap();
        let pending = store.pending().unwrap();
        let counter = store.change_counter().unwrap();
        fs::write(stage.path().join("media-0"), b"candidate bytes").unwrap();
        let mut incoming = vec![item("collision.bin", b"candidate bytes")];
        if cancelled {
            meguri_model::faultpoint::arm(
                "replace.media.prepared",
                meguri_model::faultpoint::Fault::Callback(cancel_preparation),
            );
        } else {
            incoming.push(item("../invalid.bin", b"invalid"));
        }
        let mut owner = store.own_collection_import().unwrap();
        let outcome = store.prepare_collection_import(
            stage.path(),
            incoming,
            None,
            &|| CANCEL_PREPARATION.load(Ordering::Acquire),
            false,
        );
        meguri_model::faultpoint::disarm("replace.media.prepared");
        let failure = outcome
            .err()
            .expect("invalid or cancelled preparation must not hand off");
        assert_eq!(
            failure.error,
            if cancelled {
                MediaError::Cancelled
            } else {
                MediaError::InvalidName
            }
        );
        assert!(failure.recovery_reference.is_none());
        owner.resolved();
        drop(owner);
        assert_eq!(store.pending().unwrap(), pending);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(fs::read(store.media_dir().join("collision.bin")).unwrap(), b"old bytes");
        assert_eq!(fs::read_dir(store.staging_dir()).unwrap().count(), 1);
        assert_eq!(fs::read_dir(store.trash_dir()).unwrap().count(), 1);
        assert_eq!(fs::read(unrelated.join("keep.bin")).unwrap(), b"unrelated recovery");
        assert_eq!(fs::read(trash.join("keep.bin")).unwrap(), b"unrelated trash");
        assert_eq!(fs::read(stage.path().join("media-0")).unwrap(), b"candidate bytes");
    }
}
#[test]
fn media_import_failed_unpublished_cleanup_retains_owned_reservation_authority() {
    let root = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&root.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
    store.add("collision.bin", b"old bytes").unwrap();
    fs::write(stage.path().join("media-0"), b"candidate bytes").unwrap();
    let mut owner = store.own_collection_import().unwrap();
    meguri_model::faultpoint::arm(
        "replace.media.unpublished_cleanup",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let failure = store
        .prepare_collection_import(
            stage.path(),
            vec![
                item("collision.bin", b"candidate bytes"),
                item("../invalid.bin", b"invalid"),
            ],
            None,
            &|| false,
            false,
        )
        .err()
        .unwrap();
    meguri_model::faultpoint::disarm("replace.media.unpublished_cleanup");
    let reference = failure.recovery_reference.unwrap();
    let evidence = store.staging_dir().join(&reference);
    assert!(evidence.join("unpublished.json").is_file());
    assert!(evidence.join("old-journal.db").is_file());
    assert_eq!(fs::read(evidence.join("0")).unwrap(), b"candidate bytes");
    assert_eq!(fs::read(store.media_dir().join("collision.bin")).unwrap(), b"old bytes");
    owner.recovery_required();
    drop(owner);
    assert!(store.pending().is_err());
    assert!(MediaStore::open(&root.path().join("collection.anki2"), Arc::new(common::clock())).is_err());
}
