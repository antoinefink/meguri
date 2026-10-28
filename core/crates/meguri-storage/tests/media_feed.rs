mod common;

use meguri_storage::media::{
    MediaAbsenceCause as Cause, MediaBaselineOwner, MediaError, MediaFeedItem, MediaFileState, MediaPageDecision,
    MediaStore,
};
use rusqlite::Connection;
use std::{fs, os::unix::process::ExitStatusExt, path::Path, sync::Arc};

fn store(root: &Path) -> MediaStore {
    MediaStore::open(
        &root.join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap()
}
fn item(store: &MediaStore, name: &str) -> MediaFeedItem {
    store
        .media_feed_snapshot()
        .unwrap()
        .items
        .into_iter()
        .find(|v| v.name == name)
        .unwrap()
}
fn manual_row(root: &Path, name: &str) -> Vec<rusqlite::types::Value> {
    let db = Connection::open(root.join("meguri-media.db")).unwrap();
    let mut statement = db.prepare("SELECT * FROM media WHERE name=?1").unwrap();
    let columns = statement.column_count();
    statement
        .query_row([name], |row| (0..columns).map(|i| row.get(i)).collect())
        .unwrap()
}
fn floor(store: &MediaStore) -> u64 {
    let status = store.media_feed_status().unwrap();
    store
        .set_media_feed_floor(status.feed_id, Some(status.counter))
        .unwrap();
    status.counter
}

// A physical v2 journal, rather than downgrading a new journal that already
// contains feed metadata. Only migration fixtures use SQL in these tests.
fn version_two(root: &Path, marker: bool) {
    let db = Connection::open(root.join("meguri-media.db")).unwrap();
    db.execute_batch("PRAGMA application_id=1296519757; PRAGMA user_version=2;
        CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        INSERT INTO meta VALUES('counter','7'),('last_usn','4'),('baseline_epoch','0');
        CREATE TABLE media(name TEXT PRIMARY KEY,case_key TEXT NOT NULL,sha1 TEXT,size INTEGER NOT NULL,
        mtime_secs INTEGER NOT NULL,mtime_nanos INTEGER NOT NULL,live INTEGER NOT NULL,dirty INTEGER NOT NULL,
        change_counter INTEGER NOT NULL,pending_download INTEGER NOT NULL DEFAULT 0,
        sync_state INTEGER NOT NULL DEFAULT 0);
        INSERT INTO media VALUES('gone.png','gone.png',NULL,0,0,0,0,1,6,0,2);
        INSERT INTO media VALUES('wait.png','wait.png',NULL,0,0,0,0,0,7,1,3);
        CREATE TABLE staged_ops(id INTEGER PRIMARY KEY AUTOINCREMENT,refs TEXT NOT NULL DEFAULT '[]');
        CREATE TABLE staged_files(op_id INTEGER,idx INTEGER,name TEXT,sha1 TEXT,size INTEGER,path TEXT,replace_existing INTEGER);
        CREATE TABLE trash_entries(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,sha1 TEXT NOT NULL,
        size INTEGER NOT NULL,trashed_ms INTEGER NOT NULL,path TEXT UNIQUE);").unwrap();
    if marker {
        db.execute("INSERT INTO meta VALUES('folder_mtime','recorded-marker')", [])
            .unwrap();
    }
}

#[test]
fn media_feed_v2_migration_uses_only_recorded_marker_for_completeness() {
    let mut identities = Vec::new();
    for marker in [false, true] {
        let root = tempfile::tempdir().unwrap();
        version_two(root.path(), marker);
        let media = store(root.path());
        let snapshot = media.media_feed_snapshot().unwrap();
        assert_eq!(snapshot.status.floor, None);
        assert_eq!(snapshot.status.complete, marker);
        assert_eq!(snapshot.status.counter, 7);
        assert_eq!(snapshot.items.len(), 2);
        assert!(
            snapshot
                .items
                .iter()
                .all(|v| v.state == MediaFileState::Absent(Cause::Missing))
        );
        identities.push(snapshot.status.feed_id);
        assert_eq!(media.sync_baseline().unwrap().usn, 4);
        assert!(media.sync_entry("gone.png").unwrap().unwrap().dirty);
        drop(media);
        let reopened = store(root.path());
        assert_eq!(reopened.media_feed_snapshot().unwrap(), snapshot);
        let db = Connection::open(root.path().join("meguri-media.db")).unwrap();
        assert_eq!(
            db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
                .unwrap(),
            3
        );
    }
    assert_ne!(identities[0], identities[1]);
}

#[test]
fn media_feed_removed_and_missing_survive_reset_and_logout_with_original_counter() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
    media.add("removed.png", b"removed").unwrap();
    media.add("missing.png", b"missing").unwrap();
    let after = floor(&media);
    media.trash(&["removed.png".into()]).unwrap();
    fs::remove_file(media.media_dir().join("missing.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    let removed = item(&media, "removed.png");
    let missing = item(&media, "missing.png");
    assert_eq!(removed.state, MediaFileState::Absent(Cause::Removed));
    assert_eq!(missing.state, MediaFileState::Absent(Cause::Missing));
    assert!(!removed.retained_removal && !missing.retained_removal);
    let status = media.media_feed_status().unwrap();
    media.reset_sync().unwrap();
    media.invalidate_sync_owner().unwrap();
    let feed = media.media_feed(after).unwrap();
    assert_eq!(feed.items.len(), 2);
    for original in [removed, missing] {
        let retained = feed.items.iter().find(|v| v.name == original.name).unwrap();
        assert!(retained.retained_removal);
        assert_eq!(retained.change_counter, original.change_counter);
        assert_eq!(retained.state, original.state);
    }
    assert_eq!(feed.status.feed_id, status.feed_id);
    assert_eq!(feed.status.floor, status.floor);
    assert!(feed.status.complete);
}

#[test]
fn media_feed_retention_floor_and_present_again_cleanup() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("a.png", b"a").unwrap();
    floor(&media);
    media.trash(&["a.png".into()]).unwrap();
    let absent = item(&media, "a.png");
    media.reset_sync().unwrap();
    let id = media.media_feed_status().unwrap().feed_id;
    media.set_media_feed_floor(id, Some(absent.change_counter - 1)).unwrap();
    assert!(item(&media, "a.png").retained_removal);
    media.add("a.png", b"again").unwrap();
    assert!(matches!(item(&media, "a.png").state, MediaFileState::Present(_)));
    assert_eq!(media.media_feed_snapshot().unwrap().items.len(), 1);
    media.trash(&["a.png".into()]).unwrap();
    let second = item(&media, "a.png");
    media.reset_sync().unwrap();
    media.set_media_feed_floor(id, Some(second.change_counter)).unwrap();
    assert!(media.media_feed_snapshot().unwrap().items.is_empty());
}

#[test]
fn media_feed_floor_then_delete_then_snapshot_keeps_unseen_removal() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("none.png", b"known").unwrap();
    media.reset_sync().unwrap();
    let start = floor(&media);
    // T4.3 removes a present `none` row, with no protocol deletion record.
    fs::remove_file(media.media_dir().join("none.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    let snapshot = media.media_feed_snapshot().unwrap();
    assert_eq!(snapshot.items.len(), 1);
    let removed = &snapshot.items[0];
    assert!(removed.retained_removal && removed.change_counter > start);
    assert_eq!(removed.state, MediaFileState::Absent(Cause::Missing));
    assert_eq!(media.media_feed(start).unwrap().items, snapshot.items);
    assert!(snapshot.status.counter >= removed.change_counter);
    assert!(media.sync_entry("none.png").unwrap().is_none());
}

#[test]
fn media_feed_no_consumer_retains_no_row_removals_and_unset_clears_retention() {
    for consumer in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let media = store(root.path());
        media.add("gone.png", b"gone").unwrap();
        if consumer {
            floor(&media);
        }
        media.trash(&["gone.png".into()]).unwrap();
        media.reset_sync().unwrap();
        let status = media.media_feed_status().unwrap();
        assert_eq!(media.media_feed_snapshot().unwrap().items.len(), usize::from(consumer));
        media.set_media_feed_floor(status.feed_id, None).unwrap();
        assert!(media.media_feed_snapshot().unwrap().items.is_empty());
    }
}

#[test]
fn media_feed_unchanged_facts_and_clean_marks_do_not_touch_identity_floor_or_causes() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
    media.add("live.png", b"live").unwrap();
    media.add("absent.png", b"absent").unwrap();
    floor(&media);
    media.trash(&["absent.png".into()]).unwrap();
    let before = media.media_feed_snapshot().unwrap();
    for entry in media.pending().unwrap() {
        assert!(media.acknowledge(&entry.name, entry.change_counter).unwrap());
    }
    assert_eq!(media.media_feed_snapshot().unwrap(), before);
    let captured = media.sync_entry("live.png").unwrap().unwrap();
    let pending = meguri_storage::media::PendingChange {
        name: captured.name,
        sha1: captured.sha1,
        change_counter: captured.change_counter,
    };
    media
        .register_sync_file(&media.sync_baseline().unwrap(), &pending, || true)
        .unwrap();
    let after = media.media_feed_snapshot().unwrap();
    assert!(after.status.counter > before.status.counter);
    assert_eq!(after.status.feed_id, before.status.feed_id);
    assert_eq!(after.status.floor, before.status.floor);
    for old in before.items {
        assert_eq!(item(&media, &old.name).state, old.state);
    }
}

#[test]
fn media_feed_completeness_requires_finished_registration_and_survives_marker_reset() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    fs::write(media.media_dir().join("a.png"), b"a").unwrap();
    fs::write(media.media_dir().join("b.png"), b"b").unwrap();
    assert!(!media.media_feed_status().unwrap().complete);
    assert_eq!(media.register_full(|done, _| done == 0), Err(MediaError::Cancelled));
    assert!(!media.entries().unwrap().is_empty());
    assert!(!media.media_feed_snapshot().unwrap().status.complete);
    drop(media);
    let media = store(root.path());
    assert!(!media.media_feed_status().unwrap().complete);
    media.register_full(|_, _| true).unwrap();
    assert!(media.media_feed_status().unwrap().complete);
    media.reset_sync().unwrap();
    assert_eq!(media.change_marker().unwrap(), None);
    assert!(media.media_feed_status().unwrap().complete);
}

#[test]
fn media_feed_recreated_journal_changes_identity_and_old_floor_writes_are_refused() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("still-local.png", b"local").unwrap();
    let old = media.media_feed_status().unwrap();
    drop(media);
    fs::remove_file(root.path().join("meguri-media.db")).unwrap();
    let media = store(root.path());
    let fresh = media.media_feed_status().unwrap();
    assert_ne!(fresh.feed_id, old.feed_id);
    assert!(!fresh.complete);
    assert_eq!(fresh.floor, None);
    assert!(media.media_feed_snapshot().unwrap().items.is_empty());
    assert!(media.set_media_feed_floor(old.feed_id, Some(0)).is_err());
    assert!(media.set_media_feed_floor(fresh.feed_id, Some(1)).is_err());
    floor(&media);
    media.register_full(|_, _| true).unwrap();
    let status = media.media_feed_status().unwrap();
    media
        .set_media_feed_floor(status.feed_id, Some(status.counter))
        .unwrap();
    assert!(media.set_media_feed_floor(status.feed_id, Some(0)).is_err());
    assert!(media.media_feed(status.counter + 1).is_err());
    assert!(media.try_close().unwrap());
    assert!(media.media_feed_snapshot().is_err());
    media.resume().unwrap();
    assert_eq!(media.media_feed_status().unwrap(), status_with_floor(status));
}
fn status_with_floor(mut status: meguri_storage::media::MediaFeedStatus) -> meguri_storage::media::MediaFeedStatus {
    status.floor = Some(status.counter);
    status
}

#[test]
fn media_feed_protocol_trash_row_deletion_keeps_removed_cause() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
    media.add("server-delete.png", b"file").unwrap();
    let start = floor(&media);
    let entry = media.sync_entry("server-delete.png").unwrap().unwrap();
    media
        .commit_media_page(
            &media.sync_baseline().unwrap(),
            1,
            &[MediaPageDecision::Remove {
                name: entry.name,
                stamp: entry.change_counter,
            }],
        )
        .unwrap();
    let feed = media.media_feed(start).unwrap();
    assert_eq!(feed.items.len(), 1);
    assert_eq!(feed.items[0].state, MediaFileState::Absent(Cause::Removed));
    assert!(feed.items[0].retained_removal);
    assert!(feed.items[0].change_counter > start);
    assert_eq!(media.sync_baseline().unwrap().usn, 1);
}

#[test]
fn media_feed_failed_reset_rolls_back_retention_with_the_protocol_rows() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("gone.png", b"file").unwrap();
    floor(&media);
    media.trash(&["gone.png".into()]).unwrap();
    let before = media.media_feed_snapshot().unwrap();
    let entries = media.entries().unwrap();
    meguri_model::faultpoint::arm(
        "media.sync.reset.before_commit",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = media.reset_sync();
    meguri_model::faultpoint::disarm("media.sync.reset.before_commit");
    assert!(result.is_err());
    assert_eq!(media.entries().unwrap(), entries);
    assert_eq!(media.media_feed_snapshot().unwrap(), before);
}

#[test]
fn media_feed_a_scan_that_records_an_old_marker_does_not_claim_completeness() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    fs::write(media.media_dir().join("first.png"), b"first").unwrap();
    let mut changed = false;
    media
        .register_full(|done, total| {
            if done == total && !changed {
                fs::write(media.media_dir().join("late.png"), b"late").unwrap();
                changed = true;
            }
            true
        })
        .unwrap();
    assert!(media.change_marker().unwrap().is_some());
    assert!(!media.media_feed_status().unwrap().complete);
    media.register_full(|_, _| true).unwrap();
    assert!(media.media_feed_status().unwrap().complete);
    assert!(matches!(item(&media, "late.png").state, MediaFileState::Present(_)));
}

#[test]
fn media_feed_explicit_trash_of_none_and_trash_of_already_missing_are_distinct() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("none.png", b"").unwrap();
    media.add("missing.png", b"missing").unwrap();
    floor(&media);
    fs::remove_file(media.media_dir().join("missing.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    let missing = item(&media, "missing.png");
    media.trash(&["missing.png".into()]).unwrap();
    assert_eq!(item(&media, "missing.png"), missing);
    media.reset_sync().unwrap();
    media.trash(&["none.png".into()]).unwrap();
    assert_eq!(item(&media, "none.png").state, MediaFileState::Absent(Cause::Removed));
    assert!(item(&media, "none.png").retained_removal);
    assert_eq!(
        item(&media, "missing.png").state,
        MediaFileState::Absent(Cause::Missing)
    );
}

#[test]
fn media_feed_staged_recovery_cannot_mark_a_live_namesake_removed() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    let mut stage = media.begin_staging().unwrap();
    assert_eq!(stage.add("same.png", b"staged").unwrap(), "same.png");
    fs::write(media.media_dir().join("same.png"), b"live").unwrap();
    media.register_full(|_, _| true).unwrap();
    floor(&media);
    let before = media.media_feed_snapshot().unwrap();
    // No witnesses: failed publication retains an inactive recovery manifest.
    assert!(stage.publish().is_err());
    drop(media);
    let media = store(root.path());
    media.recover(|_| Ok(false)).unwrap();
    assert_eq!(media.media_feed_snapshot().unwrap(), before);
    fs::remove_file(media.media_dir().join("same.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    assert_eq!(item(&media, "same.png").state, MediaFileState::Absent(Cause::Missing));
}

#[test]
fn media_feed_import_replacement_publishes_a_new_complete_identity_atomically() {
    use meguri_storage::media::IncomingMedia;
    use sha1::{Digest, Sha1};
    let root = tempfile::tempdir().unwrap();
    let stage = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("old.png", b"old").unwrap();
    floor(&media);
    media.trash(&["old.png".into()]).unwrap();
    media.reset_sync().unwrap();
    let before = media.media_feed_snapshot().unwrap();
    fs::write(stage.path().join("media-0"), b"new").unwrap();
    let mut owner = media.own_collection_import().unwrap();
    let mut intent = media
        .prepare_collection_import(
            stage.path(),
            vec![IncomingMedia {
                name: "new.png".into(),
                length: 3,
                sha1: format!("{:x}", Sha1::digest(b"new")),
            }],
            None,
            &|| false,
            false,
        )
        .unwrap();
    assert!(media.media_feed_snapshot().is_err());
    intent.publish(&media, |_| Ok(()), &|| false).unwrap();
    intent.cleanup(&media).unwrap();
    owner.resolved();
    drop(owner);
    let after = media.media_feed_snapshot().unwrap();
    assert_ne!(after.status.feed_id, before.status.feed_id);
    assert!(after.status.complete);
    assert_eq!(after.status.floor, None);
    assert_eq!(after.items.len(), 1);
    assert_eq!(after.items[0].name, "new.png");
    assert!(matches!(after.items[0].state, MediaFileState::Present(_)));
}

#[test]
fn media_feed_interrupted_protocol_trash_keeps_cause_but_same_stat_reappearance_clears_it() {
    for reappears in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let media = store(root.path());
        media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
        media.add("a.png", b"a").unwrap();
        floor(&media);
        let entries = media.entries().unwrap();
        meguri_model::faultpoint::arm(
            "media.sync.page.after_trash",
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        let result = media.commit_media_page(
            &media.sync_baseline().unwrap(),
            1,
            &[MediaPageDecision::Remove {
                name: "a.png".into(),
                stamp: entries[0].change_counter,
            }],
        );
        meguri_model::faultpoint::disarm("media.sync.page.after_trash");
        assert!(result.is_err());
        assert_eq!(media.entries().unwrap(), entries);
        assert_eq!(media.sync_baseline().unwrap().usn, 0);
        if reappears {
            let saved = fs::read_dir(media.trash_dir())
                .unwrap()
                .next()
                .unwrap()
                .unwrap()
                .path()
                .join("a.png");
            fs::rename(saved, media.media_dir().join("a.png")).unwrap();
            media.register_full(|_, _| true).unwrap();
            fs::remove_file(media.media_dir().join("a.png")).unwrap();
        }
        media.register_full(|_, _| true).unwrap();
        assert_eq!(
            item(&media, "a.png").state,
            MediaFileState::Absent(if reappears { Cause::Missing } else { Cause::Removed })
        );
    }
}

#[test]
fn media_feed_awaiting_bookkeeping_preserves_absence_until_actual_presence() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
    media.add("a.png", b"a").unwrap();
    floor(&media);
    media.trash(&["a.png".into()]).unwrap();
    media.reset_sync().unwrap();
    let removed = item(&media, "a.png");
    media
        .commit_media_page(
            &media.sync_baseline().unwrap(),
            1,
            &[MediaPageDecision::Download {
                name: "a.png".into(),
                sha1: "ab".repeat(20),
                usn: 1,
                stamp: None,
            }],
        )
        .unwrap();
    let awaiting = media.media_feed(removed.change_counter).unwrap().items.pop().unwrap();
    assert!(!awaiting.retained_removal);
    assert_eq!(awaiting.state, MediaFileState::Absent(Cause::Removed));
    let status = media.media_feed_status().unwrap();
    media.reset_sync().unwrap();
    media
        .set_media_feed_floor(status.feed_id, Some(removed.change_counter))
        .unwrap();
    let retained = media.media_feed(removed.change_counter).unwrap();
    assert_eq!(retained.items.len(), 1);
    assert!(retained.items[0].retained_removal);
    assert_eq!(retained.items[0].change_counter, awaiting.change_counter);
    assert_eq!(retained.items[0].state, awaiting.state);
    media.add("a.png", b"new local").unwrap();
    assert_eq!(media.media_feed_snapshot().unwrap().items.len(), 1);
    assert!(matches!(item(&media, "a.png").state, MediaFileState::Present(_)));
}

#[test]
fn media_feed_awaiting_unsyncable_facts_are_independent_and_survive_protocol_reset() {
    use sha1::{Digest, Sha1};
    for size in [0, 104_857_601] {
        let root = tempfile::tempdir().unwrap();
        let media = store(root.path());
        media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
        media
            .commit_media_page(
                &media.sync_baseline().unwrap(),
                1,
                &[MediaPageDecision::Download {
                    name: "waiting.png".into(),
                    sha1: "ab".repeat(20),
                    usn: 1,
                    stamp: None,
                }],
            )
            .unwrap();
        let manual = media.sync_entry("waiting.png").unwrap().unwrap();
        let raw_manual = manual_row(root.path(), "waiting.png");
        let path = media.media_dir().join("waiting.png");
        fs::File::create(&path).unwrap().set_len(size).unwrap();
        media.register_full(|_, _| true).unwrap();
        assert_eq!(media.sync_entry("waiting.png").unwrap().unwrap(), manual);
        assert_eq!(manual_row(root.path(), "waiting.png"), raw_manual);
        let actual = item(&media, "waiting.png");
        let MediaFileState::Present(facts) = &actual.state else {
            panic!("actual bytes must be present")
        };
        assert_eq!(facts.size, size);
        assert_eq!(facts.sha1, format!("{:x}", Sha1::digest(fs::read(&path).unwrap())));
        assert!(media.media_feed_status().unwrap().complete);
        drop(media);
        let media = store(root.path());
        assert_eq!(item(&media, "waiting.png"), actual);
        assert_eq!(media.sync_entry("waiting.png").unwrap().unwrap(), manual);
        assert_eq!(manual_row(root.path(), "waiting.png"), raw_manual);
        media.register_full(|_, _| true).unwrap();
        assert_eq!(item(&media, "waiting.png"), actual);
        assert_eq!(manual_row(root.path(), "waiting.png"), raw_manual);
        // Protocol bookkeeping may change its own stamp, but cannot clear facts.
        media
            .commit_media_page(
                &media.sync_baseline().unwrap(),
                1,
                &[MediaPageDecision::Download {
                    name: manual.name.clone(),
                    sha1: "cd".repeat(20),
                    usn: 1,
                    stamp: Some(manual.change_counter),
                }],
            )
            .unwrap();
        assert_eq!(item(&media, "waiting.png"), actual);
        media.reset_sync().unwrap();
        assert_eq!(item(&media, "waiting.png"), actual);
        media.register_full(|_, _| true).unwrap();
        assert!(matches!(item(&media, "waiting.png").state, MediaFileState::Present(_)));
        fs::write(&path, b"syncable").unwrap();
        media.register_full(|_, _| true).unwrap();
        let synced = media.sync_entry("waiting.png").unwrap().unwrap();
        assert!(synced.live && synced.dirty && synced.syncable());
        let MediaFileState::Present(facts) = item(&media, "waiting.png").state else {
            panic!()
        };
        assert_eq!(facts.sha1, format!("{:x}", Sha1::digest(b"syncable")));
    }
}

#[test]
fn media_feed_awaiting_external_disappearance_keeps_the_entire_manual_row() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
    media
        .commit_media_page(
            &media.sync_baseline().unwrap(),
            1,
            &[MediaPageDecision::Download {
                name: "waiting.png".into(),
                sha1: "ab".repeat(20),
                usn: 1,
                stamp: None,
            }],
        )
        .unwrap();
    let manual = media.sync_entry("waiting.png").unwrap().unwrap();
    let raw_manual = manual_row(root.path(), "waiting.png");
    fs::write(media.media_dir().join("waiting.png"), b"").unwrap();
    media.register_full(|_, _| true).unwrap();
    assert_eq!(media.sync_entry("waiting.png").unwrap().unwrap(), manual);
    fs::remove_file(media.media_dir().join("waiting.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    assert_eq!(media.sync_entry("waiting.png").unwrap().unwrap(), manual);
    assert_eq!(manual_row(root.path(), "waiting.png"), raw_manual);
    let missing = item(&media, "waiting.png");
    assert_eq!(missing.state, MediaFileState::Absent(Cause::Missing));
    drop(media);
    let media = store(root.path());
    assert_eq!(item(&media, "waiting.png"), missing);
    assert_eq!(media.sync_entry("waiting.png").unwrap().unwrap(), manual);
    assert_eq!(manual_row(root.path(), "waiting.png"), raw_manual);
    fs::write(media.media_dir().join("waiting.png"), b"now syncable").unwrap();
    media.register_full(|_, _| true).unwrap();
    let live = media.sync_entry("waiting.png").unwrap().unwrap();
    assert!(live.live && live.dirty && live.pending_download && live.syncable());
    let MediaFileState::Present(facts) = item(&media, "waiting.png").state else {
        panic!()
    };
    assert_eq!(facts.sha1, live.sha1.unwrap());
    assert_eq!(facts.size, 12);
}

#[test]
fn media_feed_move_before_observation_and_sql_failure_recover_removed_once() {
    for sql_failure in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let media = store(root.path());
        media.bind_sync_owner(&MediaBaselineOwner::new("m6:feed-test")).unwrap();
        media.add("a.png", b"original").unwrap();
        let start = floor(&media);
        let before = media.entries().unwrap();
        if sql_failure {
            Connection::open(root.path().join("meguri-media.db"))
                .unwrap()
                .execute_batch(
                    "CREATE TRIGGER reject_observation BEFORE UPDATE OF explicit_removal ON media
                 WHEN NEW.explicit_removal=1 BEGIN SELECT RAISE(ABORT,'real observation failure'); END;",
                )
                .unwrap();
        } else {
            meguri_model::faultpoint::arm(
                "media.trash.after_move_before_observation",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        let result = media.commit_media_page(
            &media.sync_baseline().unwrap(),
            1,
            &[MediaPageDecision::Remove {
                name: "a.png".into(),
                stamp: before[0].change_counter,
            }],
        );
        meguri_model::faultpoint::disarm("media.trash.after_move_before_observation");
        assert!(result.is_err());
        assert!(!media.media_dir().join("a.png").exists());
        // Read the protocol row directly while its observation trigger refuses
        // replay. This is deliberately a real SQL failure, not an injected one.
        let db = Connection::open(root.path().join("meguri-media.db")).unwrap();
        assert_eq!(
            u64::try_from(
                db.query_row("SELECT change_counter FROM media WHERE name='a.png'", [], |r| r
                    .get::<_, i64>(0))
                    .unwrap(),
            )
            .unwrap(),
            before[0].change_counter
        );
        assert!(
            !db.query_row("SELECT explicit_removal FROM media WHERE name='a.png'", [], |r| r
                .get::<_, bool>(0))
                .unwrap()
        );
        if sql_failure {
            db.execute_batch("DROP TRIGGER reject_observation").unwrap();
        }
        drop(db);
        assert_eq!(media.sync_baseline().unwrap().usn, 0);
        drop(media);
        let media = store(root.path());
        media.register_full(|_, _| true).unwrap();
        let removed = item(&media, "a.png");
        assert_eq!(removed.state, MediaFileState::Absent(Cause::Removed));
        assert!(removed.change_counter > start);
        media.register_full(|_, _| true).unwrap();
        assert_eq!(item(&media, "a.png"), removed);
        drop(media);
        assert_eq!(item(&store(root.path()), "a.png"), removed);
    }
}

#[test]
fn media_feed_rng() {
    use meguri_model::rng::{RandomSource, SeededRandom};
    let root = tempfile::tempdir().unwrap();
    let random = Arc::new(SeededRandom::new(313));
    let expected = SeededRandom::new(313);
    let next_id = || {
        let mut bytes = [0; 16];
        bytes[..8].copy_from_slice(&expected.next_u64().to_le_bytes());
        bytes[8..].copy_from_slice(&expected.next_u64().to_le_bytes());
        meguri_storage::media::MediaFeedId(bytes)
    };
    let open = || {
        MediaStore::open(
            &root.path().join("collection.anki2"),
            Arc::new(common::clock()),
            random.clone(),
        )
        .unwrap()
    };
    let media = open();
    let first = media.media_feed_status().unwrap().feed_id;
    assert_eq!(first, next_id());
    assert!(media.try_close().unwrap());
    media.resume().unwrap();
    assert_eq!(media.media_feed_status().unwrap().feed_id, first);
    drop(media);
    let media = open();
    assert_eq!(media.media_feed_status().unwrap().feed_id, first);
    drop(media);
    fs::remove_file(root.path().join("meguri-media.db")).unwrap();
    let media = open();
    assert_eq!(media.media_feed_status().unwrap().feed_id, next_id());
    assert_ne!(media.media_feed_status().unwrap().feed_id, first);
    assert_eq!(random.next_u64(), expected.next_u64());
}

#[test]
fn media_feed_unmoved_intent_cannot_turn_a_later_external_loss_into_removal() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("a.png", b"original").unwrap();
    floor(&media);
    let before = media.entries().unwrap();
    meguri_model::faultpoint::arm(
        "media.trash.before_move",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = media.trash(&["a.png".into()]);
    meguri_model::faultpoint::disarm("media.trash.before_move");
    assert!(result.is_err());
    assert_eq!(media.entries().unwrap(), before);
    assert_eq!(fs::read(media.media_dir().join("a.png")).unwrap(), b"original");
    drop(media);
    let media = store(root.path());
    fs::remove_file(media.media_dir().join("a.png")).unwrap();
    media.register_full(|_, _| true).unwrap();
    assert_eq!(item(&media, "a.png").state, MediaFileState::Absent(Cause::Missing));
}

#[test]
fn media_feed_only_the_receipts_original_inode_can_prove_the_trash_move() {
    for recreate in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let media = store(root.path());
        media.add("a.png", b"original").unwrap();
        floor(&media);
        meguri_model::faultpoint::arm(
            "media.trash.after_move_before_observation",
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        assert!(media.trash(&["a.png".into()]).is_err());
        meguri_model::faultpoint::disarm("media.trash.after_move_before_observation");
        let payload = fs::read_dir(media.trash_dir())
            .unwrap()
            .next()
            .unwrap()
            .unwrap()
            .path()
            .join("a.png");
        fs::rename(&payload, root.path().join("retained-original")).unwrap();
        fs::write(&payload, b"original").unwrap();
        if recreate {
            fs::rename(root.path().join("retained-original"), media.media_dir().join("a.png")).unwrap();
        }
        drop(media);
        let media = store(root.path());
        if recreate {
            media.register_full(|_, _| true).unwrap();
            assert!(matches!(item(&media, "a.png").state, MediaFileState::Present(_)));
            fs::remove_file(media.media_dir().join("a.png")).unwrap();
        }
        media.register_full(|_, _| true).unwrap();
        assert_eq!(item(&media, "a.png").state, MediaFileState::Absent(Cause::Missing));
    }
}

#[test]
fn media_feed_move_crash_child() {
    let Some(root) = std::env::var_os("MEGURI_FEED_CRASH_ROOT") else {
        return;
    };
    let media = store(Path::new(&root));
    meguri_model::faultpoint::arm(
        "media.trash.after_move_before_observation",
        meguri_model::faultpoint::Fault::Abort,
    );
    media.trash(&["a.png".into()]).unwrap();
    panic!("crash boundary was not reached");
}

#[test]
fn media_feed_real_process_death_between_move_and_observation_recovers_once() {
    let root = tempfile::tempdir().unwrap();
    let media = store(root.path());
    media.add("a.png", b"original").unwrap();
    let start = floor(&media);
    let id = media.media_feed_status().unwrap().feed_id;
    drop(media);
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "media_feed_move_crash_child", "--nocapture"])
        .current_dir(root.path())
        .env("MEGURI_FEED_CRASH_ROOT", root.path())
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(libc::SIGABRT));
    let media = store(root.path());
    assert!(!media.media_dir().join("a.png").exists());
    media.register_full(|_, _| true).unwrap();
    let removed = item(&media, "a.png");
    assert_eq!(removed.state, MediaFileState::Absent(Cause::Removed));
    assert!(removed.change_counter > start);
    assert_eq!(media.media_feed_status().unwrap().feed_id, id);
    let snapshot = media.media_feed_snapshot().unwrap();
    media.register_full(|_, _| true).unwrap();
    assert_eq!(media.media_feed_snapshot().unwrap(), snapshot);
    drop(media);
    assert_eq!(store(root.path()).media_feed_snapshot().unwrap(), snapshot);
}
