//! Conditional reset/evidence regressions; no provider proof setters.
mod common;
use meguri_storage::media::{MediaBaselineOwner, MediaBoundCheckError, MediaStore};
use std::sync::Arc;
fn store(root: &std::path::Path) -> MediaStore {
    MediaStore::open(&root.join("collection.anki2"), Arc::new(common::clock()), common::media_random()).unwrap()
}
#[test]
fn conditional_reset_refuses_wrong_owner_epoch_and_cursor_without_mutation() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let owner = MediaBaselineOwner::new("synthetic-account");
    store.bind_sync_owner(&owner).unwrap();
    store.add("generated.txt", b"generated public bytes").unwrap();
    let baseline = store.sync_baseline().unwrap();
    let entries = store.entries().unwrap();
    let mut stale = baseline.clone();
    stale.owner = Some(MediaBaselineOwner::new("foreign"));
    assert!(matches!(store.reset_bound_sync(&stale), Err(MediaBoundCheckError::Changed)));
    assert_eq!(store.sync_baseline().unwrap(), baseline);
    assert_eq!(store.entries().unwrap(), entries);
    let mut epoch = baseline.clone(); epoch.epoch += 1;
    let mut cursor = baseline.clone(); cursor.usn += 1;
    for stale in [epoch, cursor] {
        assert!(matches!(store.reset_bound_sync(&stale), Err(MediaBoundCheckError::Changed)));
    }
    assert_eq!(store.sync_baseline().unwrap(), baseline);
    assert_eq!(store.entries().unwrap(), entries);
}
#[test]
fn reset_evidence_is_exact_and_foreign_same_owner_reset_cannot_mint_it() {
    let root = tempfile::tempdir().unwrap();
    let store = store(root.path());
    let owner = MediaBaselineOwner::new("synthetic-account");
    store.bind_sync_owner(&owner).unwrap();
    store.add("generated.txt", b"retained bytes").unwrap();
    let initial = store.sync_baseline().unwrap();
    let first = store.reset_bound_sync(&initial).unwrap();
    assert_eq!(first.before(), &initial);
    assert!(first.is_from(&store));
    assert_eq!(first.after(), &store.sync_baseline().unwrap());
    assert_eq!(first.after().usn, 0);
    assert_eq!(first.after().owner, initial.owner);
    assert_ne!(first.after().epoch, initial.epoch);
    let second = store.reset_bound_sync(first.after()).unwrap();
    assert_eq!(second.before(), first.after());
    assert_eq!(second.after(), &store.sync_baseline().unwrap());
    store.with_bound_sync_baseline(second.after(), || ()).unwrap();
    store.reset_sync().unwrap(); // A different operation's same-owner reset.
    let foreign = store.sync_baseline().unwrap();
    assert!(matches!(store.reset_bound_sync(second.after()), Err(MediaBoundCheckError::Changed)));
    assert!(matches!(store.with_bound_sync_baseline(second.after(), || panic!("stale evidence reached publication")),
        Err(MediaBoundCheckError::Changed)));
    assert_eq!(store.sync_baseline().unwrap(), foreign);
    assert_eq!(std::fs::read(store.media_dir().join("generated.txt")).unwrap(), b"retained bytes");
    let other_root = tempfile::tempdir().unwrap();
    assert!(!first.is_from(&self::store(other_root.path())));
}
