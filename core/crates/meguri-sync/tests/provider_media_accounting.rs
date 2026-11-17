use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
use meguri_storage::media::{MediaBaselineOwner, MediaInventory, MediaPageDecision, MediaStore, MediaSyncState, UnsupportedMedia};
use meguri_sync::{
    policy::{MediaStage, Operation, Scope},
    provider_media::{MAX_ENTRY_BYTES, MediaAccounting, PromisedState, account_media},
};
use serde_json::json;
use std::{fs::{self, File, FileTimes}, path::Path, sync::Arc, time::{Duration, UNIX_EPOCH}};

fn open(root: &Path) -> MediaStore {
    let store = MediaStore::open(
        &root.join("collection.anki2"),
        Arc::new(FixedClock::new(1_700_000_000_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(71)),
    ).unwrap();
    store.bind_sync_owner(&MediaBaselineOwner::new("synthetic-provider-account")).unwrap();
    store
}

fn operation() -> Operation {
    serde_json::from_value(json!({
        "id": "synthetic-operation", "account": "synthetic-account",
        "configured_endpoint": "https://sync.invalid/", "target_endpoint": "https://sync.invalid/",
        "direction": "Export", "scope": "CollectionAndMedia", "phase": "MediaIncomplete",
        "acceptance": "ExactOk", "boundary": null, "guard": null, "recovery_guard": null,
        "recovery_local": null, "local": null, "remote": null, "candidate": null,
        "activation_identity": null, "original": null, "safety_backups": [], "server_copy": null,
        "prepared_at": 1700000000, "actual_generation": 42, "collection_complete": true,
        "resulting_publication": null, "media": {
            "stage": "Complete", "consent": null, "counters": {
                "uploaded_files": 0, "uploaded_deletions": 0, "downloaded_files": 0,
                "downloaded_deletions": 0, "checked": 0, "bytes_downloaded": 0,
                "bytes_uploaded": 0, "resynced": false, "oversized": [], "empty": [], "awaiting": 0
            }, "remote_outcome_unknown": false, "exclusions_verified": true, "failure_kind": null
        }, "receipt": null, "revision": 17
    })).unwrap()
}

fn inventory(store: &MediaStore) -> MediaInventory {
    store.promised_inventory(MAX_ENTRY_BYTES, &mut || true).unwrap()
}

fn account(store: &MediaStore, op: &Operation) -> MediaAccounting {
    account_media(store, &inventory(store), op).unwrap()
}

fn clean(store: &MediaStore, name: &str) {
    store.ensure_static_media(name, b"abc").unwrap();
    let entry = store.sync_entry(name).unwrap().unwrap();
    let baseline = store.sync_baseline().unwrap();
    store.commit_media_page(&baseline, baseline.usn, &[
        MediaPageDecision::Clean { name: name.into(), stamp: entry.change_counter },
    ]).unwrap();
    let entry = store.sync_entry(name).unwrap().unwrap();
    assert_eq!(entry.sync_state, MediaSyncState::Live);
    assert!(entry.live && !entry.dirty && !entry.pending_download);
}

fn await_download(store: &MediaStore, name: &str) {
    let stamp = store.sync_entry(name).unwrap().map(|e| e.change_counter);
    let baseline = store.sync_baseline().unwrap();
    store.commit_media_page(&baseline, baseline.usn + 1, &[
        MediaPageDecision::Download {
            name: name.into(), sha1: "0".repeat(40), usn: baseline.usn + 1, stamp,
        },
    ]).unwrap();
}

#[test]
fn classification_requires_clean_live_matching_facts() {
    for state in ["clean", "dirty", "download", "deleted", "awaiting", "none", "missing"] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        let name = "classified.bin";
        match state {
            "clean" => clean(&store, name),
            "dirty" => { store.ensure_static_media(name, b"abc").unwrap(); }
            "download" => { clean(&store, name); await_download(&store, name); }
            "deleted" => {
                clean(&store, name);
                store.trash(&[name.into()]).unwrap();
                fs::write(store.media_dir().join(name), b"abc").unwrap();
            }
            "awaiting" => {
                await_download(&store, name);
                fs::write(store.media_dir().join(name), b"abc").unwrap();
            }
            "none" => { clean(&store, name); store.reset_sync().unwrap(); }
            "missing" => { fs::write(store.media_dir().join(name), b"abc").unwrap(); }
            _ => unreachable!(),
        }
        let entry = store.sync_entry(name).unwrap();
        match state {
            "clean" => assert!(!entry.as_ref().unwrap().dirty),
            "dirty" => {
                assert_eq!(entry.as_ref().unwrap().sync_state, MediaSyncState::Live);
                assert!(entry.as_ref().unwrap().dirty);
            }
            "download" => {
                let e = entry.as_ref().unwrap();
                assert_eq!(e.sync_state, MediaSyncState::Live);
                assert!(!e.dirty && e.pending_download);
            }
            "deleted" => assert_eq!(entry.as_ref().unwrap().sync_state, MediaSyncState::Deleted),
            "awaiting" => assert_eq!(entry.as_ref().unwrap().sync_state, MediaSyncState::Awaiting),
            "none" => assert_eq!(entry.as_ref().unwrap().sync_state, MediaSyncState::None),
            "missing" => assert!(entry.is_none()),
            _ => unreachable!(),
        }
        let accounting = account(&store, &operation());
        assert!(accounting.verified());
        assert_eq!(accounting.remote_pending(), 0);
        assert_eq!(accounting.files().len(), 1);
        assert_eq!(accounting.files()[0].name, name);
        assert!(!accounting.files()[0].lossy_name);
        assert_eq!(accounting.files()[0].state, if state == "clean" { PromisedState::Transferred } else { PromisedState::Pending });
        assert_eq!(accounting.is_complete(), state == "clean");
    }
}

#[test]
fn same_name_changes_after_reconciliation_invalidate_completion() {
    for mtime_only in [true, false] {
        let dir = tempfile::tempdir().unwrap();
        let store = open(dir.path());
        let path = store.media_dir().join("changed.bin");
        fs::write(&path, b"abc").unwrap();
        File::open(&path).unwrap().set_times(FileTimes::new().set_modified(
            UNIX_EPOCH + Duration::new(1_700_000_000, 123_000_000),
        )).unwrap();
        clean(&store, "changed.bin");
        let op = operation();
        assert!(account(&store, &op).is_complete());
        let before = fs::metadata(&path).unwrap();
        if mtime_only {
            File::open(&path).unwrap().set_times(FileTimes::new().set_modified(
                before.modified().unwrap() + Duration::from_nanos(1),
            )).unwrap();
        } else {
            fs::write(&path, b"changed-size").unwrap();
            File::open(&path).unwrap().set_times(FileTimes::new().set_modified(before.modified().unwrap())).unwrap();
        }
        let accounting = account(&store, &op);
        assert!(accounting.verified());
        assert_eq!(accounting.files()[0].state, PromisedState::Pending);
        assert!(!accounting.is_complete());
    }
}

#[cfg(target_os = "linux")]
#[test]
fn duplicate_nfc_spelling_never_counts_as_a_second_transferred_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    clean(&store, "é.bin");
    assert!(account(&store, &operation()).is_complete());
    fs::write(store.media_dir().join("e\u{301}.bin"), b"abc").unwrap();
    let accounting = account(&store, &operation());
    assert_eq!(accounting.files().iter().map(|f| (f.name.as_str(), f.state)).collect::<Vec<_>>(), [
        ("é.bin", PromisedState::Transferred),
        ("é.bin", PromisedState::Excluded(UnsupportedMedia::InvalidName)),
    ]);
    assert!(!accounting.is_complete());
}

#[test]
fn exclusions_keep_the_inventory_reason_and_block_completion() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join("empty.bin"), b"").unwrap();
    fs::write(store.media_dir().join("invalid?.bin"), b"abc").unwrap();
    fs::create_dir(store.media_dir().join("directory")).unwrap();
    symlink("empty.bin", store.media_dir().join("link.bin")).unwrap();
    File::create(store.media_dir().join("oversized.bin")).unwrap().set_len(MAX_ENTRY_BYTES + 1).unwrap();
    let mut op = operation();
    op.media.counters.oversized.push("oversized.bin".into());
    op.media.counters.empty.push("empty.bin".into());
    let listed = inventory(&store);
    let accounting = account_media(&store, &listed, &op).unwrap();
    assert_eq!(accounting.files().len(), 5);
    for (file, promised) in listed.files().iter().zip(accounting.files()) {
        assert_eq!(promised.state, PromisedState::Excluded(file.unsupported.unwrap()));
        assert_eq!(promised.name, file.name);
    }
    assert!(accounting.verified());
    assert!(!accounting.is_complete());
    assert_eq!(accounting.transferred(), 0);
    assert_eq!(accounting.affected().count(), 5);
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_exclusion_preserves_lossy_flag_without_debug_disclosure() {
    use std::{ffi::OsStr, os::unix::ffi::OsStrExt};
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join(OsStr::from_bytes(b"lossy-\xff.bin")), b"abc").unwrap();
    let listed = inventory(&store);
    let accounting = account_media(&store, &listed, &operation()).unwrap();
    assert_eq!(accounting.files()[0].name, "lossy-\u{fffd}.bin");
    assert!(accounting.files()[0].lossy_name);
    assert_eq!(accounting.files()[0].state, PromisedState::Excluded(UnsupportedMedia::InvalidName));
    assert!(!accounting.is_complete());
    assert!(!format!("{accounting:?} {listed:?}").contains("lossy-"));
}

#[test]
fn absent_remote_awaiting_names_count_exactly_and_prevent_completion() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    await_download(&store, "absent.bin");
    await_download(&store, "present.bin");
    fs::write(store.media_dir().join("present.bin"), b"abc").unwrap();
    let accounting = account(&store, &operation());
    assert!(accounting.verified());
    assert_eq!(accounting.remote_pending(), 1);
    assert_eq!(accounting.files().len(), 1);
    assert_eq!(accounting.files()[0].state, PromisedState::Pending);
    assert!(!accounting.is_complete());
    fs::remove_file(store.media_dir().join("present.bin")).unwrap();
    let accounting = account(&store, &operation());
    assert!(accounting.files().is_empty());
    assert_eq!(accounting.remote_pending(), 2);
    assert!(!accounting.is_complete());
}

#[test]
fn complete_media_stage_with_verified_oversize_is_still_incomplete() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    File::create(store.media_dir().join("sparse.bin")).unwrap().set_len(MAX_ENTRY_BYTES + 1).unwrap();
    let mut op = operation();
    op.media.counters.oversized.push("sparse.bin".into());
    let accounting = account(&store, &op);
    assert!(accounting.verified());
    assert_eq!(accounting.files()[0].state, PromisedState::Excluded(UnsupportedMedia::OverLimit));
    assert!(!accounting.is_complete());
}

#[test]
fn exclusion_counters_must_match_the_listed_exclusion_reason() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    clean(&store, "supported.bin");
    fs::write(store.media_dir().join("empty.bin"), b"").unwrap();
    for (oversized, name) in [(false, "supported.bin"), (false, "absent.bin"), (true, "empty.bin"), (true, "absent.bin")] {
        let mut op = operation();
        if oversized { op.media.counters.oversized.push(name.into()); }
        else { op.media.counters.empty.push(name.into()); }
        let accounting = account(&store, &op);
        assert!(!accounting.verified());
        assert!(!accounting.is_complete());
        assert_eq!(op.media.counters.empty.len() + op.media.counters.oversized.len(), 1);
    }
}

#[test]
fn every_verification_precondition_is_required_individually() {
    let dir = tempfile::tempdir().unwrap();
    let other_dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    let other = open(other_dir.path());
    clean(&store, "verified.bin");
    clean(&other, "verified.bin");
    let listed = inventory(&store);
    let op = operation();
    assert!(account_media(&store, &listed, &op).unwrap().is_complete());
    let mut variants = Vec::new();
    let mut changed = op.clone(); changed.scope = Scope::CollectionOnly; variants.push(changed);
    let mut changed = op.clone(); changed.collection_complete = false; variants.push(changed);
    let mut changed = op.clone(); changed.actual_generation = None; variants.push(changed);
    let mut changed = op.clone(); changed.media.stage = MediaStage::Running; variants.push(changed);
    let mut changed = op.clone(); changed.media.exclusions_verified = false; variants.push(changed);
    for changed in variants {
        let accounting = account_media(&store, &listed, &changed).unwrap();
        assert!(!accounting.verified());
        assert!(!accounting.is_complete());
        assert_eq!(accounting.generation(), changed.actual_generation.unwrap_or(0));
    }
    let wrong_limit = store.promised_inventory(MAX_ENTRY_BYTES + 1, &mut || true).unwrap();
    let wrong_store = inventory(&other);
    for invalid in [&wrong_limit, &wrong_store] {
        let accounting = account_media(&store, invalid, &op).unwrap();
        assert!(!accounting.verified());
        assert!(!accounting.is_complete());
    }
}

#[test]
fn complete_accounting_is_bound_to_operation_and_reports_affected_in_inventory_order() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    clean(&store, "middle.bin");
    clean(&store, "second.bin");
    let op = operation();
    let accounting = account(&store, &op);
    assert!(accounting.verified() && accounting.is_complete());
    assert_eq!(accounting.operation(), &op.id);
    assert_eq!(accounting.operation_revision(), op.revision);
    assert_eq!(accounting.generation(), 42);
    assert_eq!(accounting.transferred(), 2);
    assert_eq!(accounting.affected().count(), 0);
    fs::write(store.media_dir().join("a-pending.bin"), b"abc").unwrap();
    fs::write(store.media_dir().join("z-empty.bin"), b"").unwrap();
    File::open(store.media_dir().join("a-pending.bin")).unwrap().set_times(
        FileTimes::new().set_modified(UNIX_EPOCH + Duration::from_secs(1_700_000_001)),
    ).unwrap();
    let listed = inventory(&store);
    let accounting = account_media(&store, &listed, &op).unwrap();
    assert_eq!(accounting.transferred(), 2);
    assert!(!accounting.is_complete());
    assert_eq!(accounting.affected().map(|f| (f.name.as_str(), f.state)).collect::<Vec<_>>(), [
        ("a-pending.bin", PromisedState::Pending),
        ("z-empty.bin", PromisedState::Excluded(UnsupportedMedia::Empty)),
    ]);
    for file in listed.files() {
        assert!(!format!("{accounting:?}").contains(&file.name));
        assert!(!format!("{listed:?}").contains(&file.name));
    }
    assert_eq!(accounting.clone(), accounting);
    assert_eq!(listed.clone(), listed);
}
