mod common;
use meguri_model::faultpoint::{self, Fault};
use meguri_storage::media::{
    MediaBaselineOwner, MediaDownloadRecord, MediaError, MediaPageDecision, MediaStore, MediaSyncState,
};
use rusqlite::{Connection, params};
use sha1::{Digest, Sha1};
use std::{collections::BTreeSet, fs, os::unix::fs::MetadataExt, path::Path, sync::Arc};

fn open(root: &Path) -> MediaStore {
    MediaStore::open(&root.join("collection.anki2"), Arc::new(common::clock())).unwrap()
}
fn owner(value: &str) -> MediaBaselineOwner {
    MediaBaselineOwner::new(value)
}
fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha1::digest(bytes))
}
fn download(store: &MediaStore, name: &str, bytes: &[u8], usn: u64, high: u64) {
    let stamp = store.sync_entry(name).unwrap().map(|e| e.change_counter);
    store
        .commit_media_page(
            &store.sync_baseline().unwrap(),
            high,
            &[MediaPageDecision::Download {
                name: name.into(),
                sha1: hash(bytes),
                usn,
                stamp,
            }],
        )
        .unwrap();
}
fn save(store: &MediaStore, name: &str, bytes: &[u8]) {
    let intent = store
        .download_intents()
        .unwrap()
        .into_iter()
        .find(|i| i.name == name)
        .unwrap();
    store.ensure_static_media(name, bytes).unwrap();
    store
        .commit_media_downloads(
            &store.sync_baseline().unwrap(),
            &[MediaDownloadRecord {
                intent,
                stored_name: Some(name.into()),
                publication: None,
            }],
        )
        .unwrap();
}
struct Armed(&'static str);
impl Armed {
    fn io(name: &'static str) -> Self {
        faultpoint::arm(name, Fault::IoError(std::io::ErrorKind::Other));
        Self(name)
    }
}
impl Drop for Armed {
    fn drop(&mut self) {
        faultpoint::disarm(self.0);
    }
}

#[test]
fn cursor_never_passes_an_unsaved_file_and_download_batches_survive_reopen() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    download(&store, "first.png", b"first", 3, 5);
    download(&store, "second.png", b"second", 9, 10);
    assert_eq!(store.sync_baseline().unwrap().usn, 2);
    let intent = store.download_intents().unwrap()[0].clone();
    assert!(
        store
            .commit_media_downloads(
                &store.sync_baseline().unwrap(),
                &[MediaDownloadRecord {
                    intent,
                    stored_name: Some("first.png".into()),
                    publication: None,
                }]
            )
            .is_err()
    );
    assert_eq!(store.sync_baseline().unwrap().usn, 2);
    assert!(store.is_awaiting("FIRST.PNG").unwrap());
    store.register_full(|_, _| true).unwrap();
    assert!(store.pending().unwrap().is_empty());
    assert_eq!(store.awaiting_names().unwrap(), ["first.png", "second.png"]);
    drop(store);
    let store = open(dir.path());
    save(&store, "first.png", b"first");
    assert_eq!(store.sync_baseline().unwrap().usn, 8);
    assert!(!store.is_awaiting("first.png").unwrap());
    assert!(store.is_awaiting("second.png").unwrap());
    drop(store);
    let store = open(dir.path());
    save(&store, "second.png", b"second");
    assert_eq!(store.sync_baseline().unwrap().usn, 10);
    assert_eq!(store.clean_live_count().unwrap(), Some(2));
    assert!(store.download_intents().unwrap().is_empty());
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap().usn, 10);
    assert_eq!(fs::read(store.media_dir().join("first.png")).unwrap(), b"first");
    assert_eq!(
        store.sync_entry("second.png").unwrap().unwrap().sync_state,
        MediaSyncState::Live
    );
}

#[test]
fn page_and_download_faults_roll_back_state_and_cursor_together() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    let initial = store.sync_baseline().unwrap();
    let armed = Armed::io("media.sync.page.before_commit");
    assert!(
        store
            .commit_media_page(
                &initial,
                4,
                &[MediaPageDecision::Download {
                    name: "clip.png".into(),
                    sha1: hash(b"clip"),
                    usn: 3,
                    stamp: None
                }]
            )
            .is_err()
    );
    drop(armed);
    assert_eq!(store.sync_baseline().unwrap(), initial);
    assert!(store.download_intents().unwrap().is_empty());
    assert!(store.sync_entry("clip.png").unwrap().is_none());
    download(&store, "clip.png", b"clip", 3, 4);
    let intent = store.download_intents().unwrap().pop().unwrap();
    store.ensure_static_media("clip.png", b"clip").unwrap();
    let before = store.sync_baseline().unwrap();
    let entry = store.sync_entry("clip.png").unwrap();
    let counter = store.change_counter().unwrap();
    let armed = Armed::io("media.sync.downloads.before_commit");
    assert!(
        store
            .commit_media_downloads(
                &before,
                &[MediaDownloadRecord {
                    intent: intent.clone(),
                    stored_name: Some("clip.png".into()),
                    publication: None,
                }]
            )
            .is_err()
    );
    drop(armed);
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert_eq!(store.sync_entry("clip.png").unwrap(), entry);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(store.download_intents().unwrap(), [intent]);
    // The published bytes survived: a re-listed matching server hash can
    // confirm them, without recording an unsaved download or transferring again.
    let stamp = store.sync_entry("clip.png").unwrap().unwrap().change_counter;
    store
        .commit_media_page(
            &before,
            4,
            &[MediaPageDecision::Clean {
                name: "clip.png".into(),
                stamp,
            }],
        )
        .unwrap();
    assert_eq!(store.sync_baseline().unwrap().usn, 4);
    assert!(!store.pending_any().unwrap());
}

#[test]
fn two_repaired_download_names_share_one_file_and_commit_all_intents_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    download(&store, "clip:1.png", b"shared", 3, 5);
    download(&store, "clip?1.png", b"shared", 5, 6);
    download(&store, "later.png", b"later", 9, 10);
    let stored = store.add("clip:1.png", b"shared").unwrap();
    assert_eq!(store.add("clip?1.png", b"shared").unwrap(), stored);
    assert_eq!(stored, "clip1.png");
    let records = store
        .download_intents()
        .unwrap()
        .into_iter()
        .filter(|i| i.name != "later.png")
        .map(|intent| MediaDownloadRecord {
            intent,
            stored_name: Some(stored.clone()),
            publication: None,
        })
        .collect::<Vec<_>>();
    assert_eq!(records.len(), 2);
    let before = store.sync_baseline().unwrap();
    let entries = store.entries().unwrap();
    let intents = store.download_intents().unwrap();
    let counter = store.change_counter().unwrap();
    let armed = Armed::io("media.sync.downloads.before_commit");
    let failed = store.commit_media_downloads(&before, &records);
    drop(armed);
    assert!(matches!(&failed, Err(MediaError::Io(_))), "{failed:?}");
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert_eq!(store.entries().unwrap(), entries);
    assert_eq!(store.download_intents().unwrap(), intents);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(fs::read(store.media_dir().join(&stored)).unwrap(), b"shared");
    assert_eq!(store.commit_media_downloads(&before, &records).unwrap(), 8);
    // One stored row plus both original-name deletions get one stamp each.
    assert_eq!(store.change_counter().unwrap(), counter + 3);
    for record in &records {
        let original = store.sync_entry(&record.intent.name).unwrap().unwrap();
        assert_eq!(original.sync_state, MediaSyncState::Deleted);
        assert!(original.dirty && !original.live && !original.pending_download);
        assert!(original.change_counter > counter);
    }
    let shared = store.sync_entry(&stored).unwrap().unwrap();
    assert_eq!(shared.sync_state, MediaSyncState::Live);
    assert!(shared.live && shared.dirty && !shared.pending_download);
    assert_eq!(shared.sha1, Some(hash(b"shared")));
    assert_eq!(store.awaiting_names().unwrap(), ["later.png"]);
    assert_eq!(store.download_intents().unwrap().len(), 1);
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap().usn, 8);
    assert_eq!(store.sync_entry(&stored).unwrap(), Some(shared));
    save(&store, "later.png", b"later");
    assert_eq!(store.sync_baseline().unwrap().usn, 10);
}

#[test]
fn shared_download_destination_rejects_conflicting_hashes_without_journal_changes() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    download(&store, "clip:1.png", b"shared", 3, 5);
    download(&store, "clip?1.png", b"different", 5, 6);
    let stored = store.add("clip:1.png", b"shared").unwrap();
    let intents = store.download_intents().unwrap();
    let records = intents
        .iter()
        .cloned()
        .map(|intent| MediaDownloadRecord {
            intent,
            stored_name: Some(stored.clone()),
            publication: None,
        })
        .collect::<Vec<_>>();
    let before = store.sync_baseline().unwrap();
    let entries = store.entries().unwrap();
    let counter = store.change_counter().unwrap();
    assert!(store.commit_media_downloads(&before, &records).is_err());
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert_eq!(store.entries().unwrap(), entries);
    assert_eq!(store.download_intents().unwrap(), intents);
    assert_eq!(store.change_counter().unwrap(), counter);
    assert_eq!(fs::read(store.media_dir().join(stored)).unwrap(), b"shared");
}

#[test]
fn resets_bind_owner_atomically_keep_facts_and_invalidate_old_callbacks() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:CaseSensitive")).unwrap();
    store.add("keep.png", b"keep").unwrap();
    store.add("gone.png", b"gone").unwrap();
    store.trash(&["gone.png".into()]).unwrap();
    download(&store, "waiting.png", b"waiting", 4, 5);
    let before = store.sync_baseline().unwrap();
    let entries = store.entries().unwrap();
    let counter = store.change_counter().unwrap();
    let armed = Armed::io("media.sync.reset.before_commit");
    assert!(store.bind_sync_owner(&owner("m6:casesensitive")).is_err());
    drop(armed);
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert_eq!(store.entries().unwrap(), entries);
    assert_eq!(store.change_counter().unwrap(), counter);
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap(), before);
    assert!(store.bind_sync_owner(&owner("m6:casesensitive")).unwrap());
    let reset = store.sync_baseline().unwrap();
    assert_eq!(reset.owner, Some(owner("m6:casesensitive")));
    assert_eq!(reset.usn, 0);
    assert!(reset.epoch > before.epoch);
    assert!(store.commit_media_page(&before, 6, &[]).is_err());
    assert!(store.change_marker().unwrap().is_none());
    assert!(store.download_intents().unwrap().is_empty());
    let kept = store.entries().unwrap();
    assert_eq!(kept.len(), 1);
    let old = entries.iter().find(|e| e.name == "keep.png").unwrap();
    assert_eq!(
        (
            kept[0].sha1.clone(),
            kept[0].size,
            kept[0].mtime_secs,
            kept[0].mtime_nanos
        ),
        (old.sha1.clone(), old.size, old.mtime_secs, old.mtime_nanos)
    );
    assert_eq!(kept[0].sync_state, MediaSyncState::None);
    assert!(kept[0].change_counter > counter && !kept[0].dirty);
    assert_eq!(
        store.open_file("keep.png").unwrap().unwrap().metadata().unwrap().len(),
        4
    );
    assert_eq!(store.trash_entries().unwrap().len(), 1);
    assert!(!store.bind_sync_owner(&owner("m6:casesensitive")).unwrap());
    let report = store.check_changes_report().unwrap();
    assert!(report.listed && report.completed);
    assert_eq!((report.hashed, report.added), (0, 1));
    assert_eq!(store.pending_batch(1).unwrap()[0].name, "keep.png");
    store.reset_sync().unwrap();
    assert_eq!(store.sync_baseline().unwrap().owner, reset.owner);
    store.invalidate_sync_owner().unwrap();
    assert_eq!(store.sync_baseline().unwrap().owner, None);
    assert!(
        store
            .commit_media_page(&store.sync_baseline().unwrap(), 1, &[])
            .is_err()
    );
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap().owner, None);
    assert_eq!(store.entries().unwrap()[0].sync_state, MediaSyncState::None);
    assert_eq!(store.last_check_report().unwrap(), Some(report));
}

#[test]
fn stale_uploads_and_recreated_names_keep_new_changes_and_clean_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    store.ensure_static_media("asset.png", b"old").unwrap();
    let old = store.pending_batch(1).unwrap();
    let initial = store.sync_baseline().unwrap();
    let armed = Armed::io("media.sync.upload.before_commit");
    assert!(store.commit_media_upload(&initial, &old, 1).is_err());
    drop(armed);
    assert_eq!(store.sync_baseline().unwrap(), initial);
    assert_eq!(store.pending_batch(1).unwrap(), old);
    store.ensure_static_media("asset.png", b"new").unwrap();
    store
        .commit_media_upload(&store.sync_baseline().unwrap(), &old, 1)
        .unwrap();
    assert!(store.pending_any().unwrap());
    assert!(store.pending_batch(1).unwrap()[0].change_counter > old[0].change_counter);
    // A gap/no-op does not advance the cursor, but accepted stamps are clean.
    let fresh = store.pending_batch(1).unwrap();
    let before = store.sync_baseline().unwrap();
    assert_eq!(store.commit_media_upload(&before, &fresh, 100).unwrap(), before.usn);
    store.trash(&["asset.png".into()]).unwrap();
    let deletion = store.pending_batch(1).unwrap();
    assert_eq!(deletion[0].sha1, None);
    store
        .commit_media_upload(&store.sync_baseline().unwrap(), &deletion, 2)
        .unwrap();
    drop(store);
    let store = open(dir.path());
    let clean = store.sync_entry("asset.png").unwrap().unwrap();
    assert_eq!(clean.sync_state, MediaSyncState::Deleted);
    assert!(!clean.dirty);
    assert_eq!(store.clean_live_count().unwrap(), Some(0));
    store
        .commit_media_page(
            &store.sync_baseline().unwrap(),
            3,
            &[MediaPageDecision::Remove {
                name: "asset.png".into(),
                stamp: clean.change_counter,
            }],
        )
        .unwrap();
    assert!(store.sync_entry("asset.png").unwrap().is_none());
    store.ensure_static_media("asset.png", b"recreated").unwrap();
    assert!(!store.acknowledge("asset.png", clean.change_counter).unwrap());
    assert!(store.pending_batch(1).unwrap()[0].change_counter > clean.change_counter);
    let counter = store.change_counter().unwrap();
    drop(store);
    assert_eq!(open(dir.path()).change_counter().unwrap(), counter);
}

#[test]
fn indexed_pending_batches_use_stamp_order_and_awaiting_has_its_own_index() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    store.add("z.png", b"z").unwrap();
    store.add("a.png", b"a").unwrap();
    store.add("m.png", b"m").unwrap();
    assert_eq!(store.pending_batch(0).unwrap().len(), 0);
    assert_eq!(
        store
            .pending_batch(2)
            .unwrap()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["z.png", "a.png"]
    );
    download(&store, "waiting.png", b"waiting", 7, 8);
    let db = Connection::open(dir.path().join("meguri-media.db")).unwrap();
    for (sql, index) in [
        (
            "SELECT name FROM media WHERE dirty=1 ORDER BY change_counter,name LIMIT 2",
            "media_dirty",
        ),
        ("SELECT EXISTS(SELECT 1 FROM media WHERE dirty=1)", "media_dirty"),
        (
            "SELECT name FROM media WHERE sync_state=3 AND case_key='waiting.png'",
            "media_download",
        ),
        (
            "SELECT name FROM media WHERE sync_state=3 ORDER BY case_key,name",
            "media_download",
        ),
    ] {
        let plan = db
            .prepare(&format!("EXPLAIN QUERY PLAN {sql}"))
            .unwrap()
            .query_map([], |r| r.get::<_, String>(3))
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert!(plan.iter().any(|p| p.contains(index)), "{plan:?}");
        assert!(!plan.iter().any(|p| p.contains("TEMP B-TREE")), "{plan:?}");
    }
    store.register_full(|_, _| true).unwrap();
    assert_eq!(
        store.sync_entry("waiting.png").unwrap().unwrap().sync_state,
        MediaSyncState::Awaiting
    );
    assert!(!store.pending().unwrap().iter().any(|c| c.name == "waiting.png"));
    // An outside file changes awaiting into pending live, without losing the
    // durable server intent that still pins the cursor.
    fs::write(store.media_dir().join("waiting.png"), b"local").unwrap();
    store.register_full(|_, _| true).unwrap();
    assert!(!store.is_awaiting("waiting.png").unwrap());
    assert_eq!(
        store.sync_entry("waiting.png").unwrap().unwrap().sync_state,
        MediaSyncState::Live
    );
    assert!(store.pending().unwrap().iter().any(|c| c.name == "waiting.png"));
    assert_eq!(store.sync_baseline().unwrap().usn, 6);
    assert_eq!(store.download_intents().unwrap().len(), 1);
}

#[test]
fn empty_and_oversized_transitions_keep_facts_and_required_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.add("empty.png", b"before").unwrap();
    store.add("large.png", b"before").unwrap();
    fs::write(store.media_dir().join("empty.png"), []).unwrap();
    fs::OpenOptions::new()
        .write(true)
        .open(store.media_dir().join("large.png"))
        .unwrap()
        .set_len(104_857_601)
        .unwrap();
    let report = store.register_full_report(|_, _| true).unwrap();
    assert_eq!(report.removed, 2);
    for e in store.entries().unwrap() {
        assert!(e.live && e.sha1.is_some() && e.dirty);
        assert_eq!(e.sync_state, MediaSyncState::Deleted);
    }
    assert!(store.pending().unwrap().iter().all(|c| c.sha1.is_none()));
    assert_eq!(store.live_count().unwrap(), 0);
    store.reset_sync().unwrap();
    let report = store.register_full_report(|_, _| true).unwrap();
    assert_eq!(report.hashed, 0);
    assert!(!store.pending_any().unwrap());
    assert!(
        store
            .entries()
            .unwrap()
            .iter()
            .all(|e| e.sync_state == MediaSyncState::None && e.live)
    );
    fs::remove_file(store.media_dir().join("empty.png")).unwrap();
    store.register_full(|_, _| true).unwrap();
    assert!(store.sync_entry("empty.png").unwrap().is_none());
    fs::write(store.media_dir().join("large.png"), b"small again").unwrap();
    store.register_full(|_, _| true).unwrap();
    assert_eq!(store.pending_batch(1).unwrap()[0].name, "large.png");
}

#[test]
fn stale_awaiting_cleanup_and_suspension_preserve_durable_state() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    download(&store, "stale.png", b"stale", 3, 5);
    let baseline = store.sync_baseline().unwrap();
    let intents = store.download_intents().unwrap();
    assert!(store.try_close().unwrap());
    let clone = store.clone();
    assert!(clone.sync_baseline().is_err());
    assert!(clone.sync_entry("stale.png").is_err());
    assert!(clone.pending_batch(1).is_err());
    assert!(clone.pending_any().is_err());
    assert!(clone.is_awaiting("stale.png").is_err());
    assert!(clone.awaiting_names().is_err());
    assert!(clone.live_count().is_err());
    assert!(clone.clean_live_count().is_err());
    assert!(clone.download_intents().is_err());
    assert!(clone.reset_sync().is_err());
    assert!(clone.bind_sync_owner(&owner("m6:two")).is_err());
    assert!(clone.invalidate_sync_owner().is_err());
    assert!(clone.commit_media_page(&baseline, 6, &[]).is_err());
    assert!(clone.commit_media_upload(&baseline, &[], 6).is_err());
    assert!(clone.commit_media_downloads(&baseline, &[]).is_err());
    assert!(clone.finish_media_listing(&baseline, &BTreeSet::new()).is_err());
    assert!(clone.last_check_report().is_err());
    assert!(clone.change_marker().is_err());
    clone.resume().unwrap();
    assert_eq!(store.sync_baseline().unwrap(), baseline);
    assert_eq!(store.download_intents().unwrap(), intents);
    assert_eq!(store.finish_media_listing(&baseline, &BTreeSet::new()).unwrap(), 5);
    drop(store);
    drop(clone);
    let store = open(dir.path());
    assert!(store.awaiting_names().unwrap().is_empty());
    assert_eq!(store.sync_baseline().unwrap().usn, 5);
}

// An authored v1 database, not a current-schema round-trip: migration must keep
// facts, pending changes, high counter, exact timestamps, stage and trash bytes.
fn v1_fixture(root: &Path) {
    fs::create_dir(root.join("collection.media")).unwrap();
    fs::create_dir(root.join("media.staging")).unwrap();
    fs::create_dir_all(root.join("media.trash/5-1")).unwrap();
    fs::write(root.join("collection.media/old.png"), b"old").unwrap();
    fs::write(root.join("media.staging/1-1--staged.png"), b"staged").unwrap();
    fs::write(root.join("media.trash/5-1/trashed.png"), b"trashed").unwrap();
    let m = fs::metadata(root.join("collection.media/old.png")).unwrap();
    let db = Connection::open(root.join("meguri-media.db")).unwrap();
    db.execute_batch("PRAGMA application_id=1296519757; PRAGMA user_version=1;
        CREATE TABLE meta(key TEXT PRIMARY KEY,value TEXT NOT NULL);
        INSERT INTO meta VALUES('counter','80'),('last_usn','12');
        CREATE TABLE media(name TEXT PRIMARY KEY,case_key TEXT NOT NULL,sha1 TEXT,size INTEGER NOT NULL,mtime_secs INTEGER NOT NULL,mtime_nanos INTEGER NOT NULL,live INTEGER NOT NULL,dirty INTEGER NOT NULL,change_counter INTEGER NOT NULL,pending_download INTEGER NOT NULL DEFAULT 0);
        CREATE INDEX media_case ON media(case_key) WHERE live=1;
        CREATE INDEX media_dirty ON media(name) WHERE dirty=1;
        CREATE INDEX media_download ON media(name) WHERE pending_download=1;
        CREATE TABLE staged_ops(id INTEGER PRIMARY KEY AUTOINCREMENT,refs TEXT NOT NULL DEFAULT '[]');
        CREATE TABLE staged_files(op_id INTEGER NOT NULL,idx INTEGER NOT NULL,name TEXT NOT NULL,sha1 TEXT NOT NULL,size INTEGER NOT NULL,path TEXT NOT NULL,replace_existing INTEGER NOT NULL DEFAULT 0,PRIMARY KEY(op_id,idx));
        CREATE INDEX staged_name ON staged_files(name);
        CREATE TABLE trash_entries(id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL,sha1 TEXT NOT NULL,size INTEGER NOT NULL,trashed_ms INTEGER NOT NULL,path TEXT UNIQUE);
        INSERT INTO media VALUES('gone.png','gone.png',NULL,0,0,0,0,1,42,0);").unwrap();
    db.execute(
        "INSERT INTO media VALUES('old.png','old.png',?1,3,?2,?3,1,1,41,0)",
        params![hash(b"old"), m.mtime(), m.mtime_nsec()],
    )
    .unwrap();
    db.execute(
        "INSERT INTO media VALUES('wait.png','wait.png',NULL,0,0,0,0,0,43,1)",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO staged_ops VALUES(1,?1)",
        [format!(
            "[{{\"note_id\":12,\"mtime\":7,\"fields_sha256\":\"{}\"}}]",
            "a".repeat(64)
        )],
    )
    .unwrap();
    db.execute(
        "INSERT INTO staged_files VALUES(1,1,'staged.png',?1,6,'1-1--staged.png',0)",
        [hash(b"staged")],
    )
    .unwrap();
    db.execute(
        "INSERT INTO trash_entries VALUES(1,'trashed.png',?1,7,5,'5-1/trashed.png')",
        [hash(b"trashed")],
    )
    .unwrap();
}
#[test]
fn v1_migration_reopens_with_facts_pending_counter_stages_and_trash_intact() {
    let dir = tempfile::tempdir().unwrap();
    v1_fixture(dir.path());
    let before = fs::metadata(dir.path().join("collection.media/old.png")).unwrap();
    let store = open(dir.path());
    assert_eq!(store.change_counter().unwrap(), 80);
    assert_eq!(store.sync_baseline().unwrap().usn, 12);
    assert_eq!(store.sync_baseline().unwrap().owner, None);
    let entries = store.entries().unwrap();
    let old = store.sync_entry("old.png").unwrap().unwrap();
    assert_eq!(
        (old.size, old.mtime_secs, old.mtime_nanos),
        (3, before.mtime(), before.mtime_nsec() as u32)
    );
    assert_eq!(old.sha1, Some(hash(b"old")));
    assert_eq!(old.sync_state, MediaSyncState::Live);
    assert_eq!(
        store
            .pending_batch(2)
            .unwrap()
            .iter()
            .map(|e| (e.name.as_str(), e.change_counter))
            .collect::<Vec<_>>(),
        [("old.png", 41), ("gone.png", 42)]
    );
    assert!(store.is_awaiting("wait.png").unwrap());
    assert_eq!(store.download_intents().unwrap()[0].usn, None);
    assert_eq!(store.trash_entries().unwrap()[0].sha1, hash(b"trashed"));
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.entries().unwrap(), entries);
    assert_eq!(store.change_counter().unwrap(), 80);
    let report = store
        .recover(|witness| {
            assert_eq!(witness.note_id, 12);
            Ok(true)
        })
        .unwrap();
    assert_eq!(report.published, 1);
    assert_eq!(fs::read(store.media_dir().join("staged.png")).unwrap(), b"staged");
    assert_eq!(store.trash_entries().unwrap().len(), 1);
    assert!(store.change_counter().unwrap() > 80);
    store.bind_sync_owner(&owner("m6:new")).unwrap();
    assert_eq!(store.sync_baseline().unwrap().usn, 0);
    assert!(store.download_intents().unwrap().is_empty());
    assert!(
        store
            .entries()
            .unwrap()
            .iter()
            .all(|e| e.live && e.sync_state == MediaSyncState::None)
    );
}

#[test]
fn foreign_wal_refusal_changes_neither_database_nor_sidecars() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("meguri-media.db");
    let db = Connection::open(&path).unwrap();
    db.execute_batch(
        "PRAGMA journal_mode=WAL; CREATE TABLE foreign_data(value); INSERT INTO foreign_data VALUES('synthetic');",
    )
    .unwrap();
    let originals = ["meguri-media.db", "meguri-media.db-wal", "meguri-media.db-shm"]
        .map(|name| (name, fs::read(dir.path().join(name)).unwrap()));
    assert!(matches!(
        MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())),
        Err(MediaError::Database(_))
    ));
    for (name, bytes) in originals {
        assert_eq!(fs::read(dir.path().join(name)).unwrap(), bytes);
    }
    drop(db);
}

#[test]
fn migration_failure_rolls_back_the_version_and_can_be_reopened() {
    let dir = tempfile::tempdir().unwrap();
    v1_fixture(dir.path());
    let armed = Armed::io("media.sync.migration.before_commit");
    assert!(MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).is_err());
    drop(armed);
    let db = Connection::open(dir.path().join("meguri-media.db")).unwrap();
    assert_eq!(
        db.pragma_query_value(None, "user_version", |r| r.get::<_, i64>(0))
            .unwrap(),
        1
    );
    assert_eq!(
        db.query_row("SELECT value FROM meta WHERE key='counter'", [], |r| r
            .get::<_, String>(0))
            .unwrap(),
        "80"
    );
    let columns = db
        .prepare("PRAGMA table_info(media)")
        .unwrap()
        .query_map([], |r| r.get::<_, String>(1))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert!(!columns.iter().any(|c| c == "sync_state"));
    drop(db);
    let store = open(dir.path());
    assert_eq!(store.change_counter().unwrap(), 80);
    assert_eq!(store.trash_entries().unwrap().len(), 1);
    assert!(store.is_awaiting("wait.png").unwrap());
    assert_eq!(store.recover(|_| Ok(true)).unwrap().published, 1);
}

#[test]
fn server_deletions_trash_each_present_file_and_never_leave_pending_deletions() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    store.bind_sync_owner(&owner("m6:one")).unwrap();
    store.add("absent.png", b"absent").unwrap();
    store.add("present.png", b"present").unwrap();
    for c in store.pending().unwrap() {
        store.acknowledge(&c.name, c.change_counter).unwrap();
    }
    let absent = store.sync_entry("absent.png").unwrap().unwrap();
    let present = store.sync_entry("present.png").unwrap().unwrap();
    fs::remove_file(store.media_dir().join("absent.png")).unwrap();
    let before = store.sync_baseline().unwrap();
    store
        .commit_media_page(
            &before,
            2,
            &[
                MediaPageDecision::Remove {
                    name: absent.name,
                    stamp: absent.change_counter,
                },
                MediaPageDecision::Remove {
                    name: present.name,
                    stamp: present.change_counter,
                },
            ],
        )
        .unwrap();
    drop(store);
    let store = open(dir.path());
    assert_eq!(store.sync_baseline().unwrap().usn, 2);
    assert!(store.entries().unwrap().is_empty());
    assert!(!store.pending_any().unwrap());
    assert_eq!(store.trash_entries().unwrap()[0].name, "present.png");
    assert!(!store.media_dir().join("present.png").exists());
}

#[test]
fn full_checks_normalize_outside_names_and_persist_cancelled_reports() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join("e\u{301}.png"), b"synthetic").unwrap();
    let report = store.register_full_report(|_, _| true).unwrap();
    assert_eq!((report.renamed, report.added, report.hashed), (1, 1, 1));
    assert_eq!(store.pending_batch(1).unwrap()[0].name, "é.png");
    assert!(store.media_dir().join("é.png").is_file());
    let marker = store.change_marker().unwrap();
    assert_eq!(store.register_full_report(|_, _| false), Err(MediaError::Cancelled));
    assert_eq!(store.change_marker().unwrap(), marker);
    drop(store);
    let store = open(dir.path());
    let report = store.last_check_report().unwrap().unwrap();
    assert!(!report.completed && report.listed);
    assert_eq!((report.seen, report.hashed), (0, 0));
}

#[test]
fn cancelled_hash_keeps_no_partial_facts_and_reopen_finishes_the_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = open(dir.path());
    fs::write(store.media_dir().join("outside.png"), vec![7; 131_072]).unwrap();
    let mut calls = 0;
    let result = store.register_full(|_, _| {
        calls += 1;
        calls < 3
    });
    assert_eq!(result, Err(MediaError::Cancelled));
    assert!(store.entries().unwrap().is_empty());
    assert!(store.change_marker().unwrap().is_none());
    drop(store);
    let store = open(dir.path());
    let failed = store.last_check_report().unwrap().unwrap();
    assert!(!failed.completed);
    assert_eq!(failed.hashed, 1);
    store.check_changes().unwrap();
    assert_eq!(store.pending_batch(1).unwrap()[0].name, "outside.png");
    assert_eq!(store.sync_entry("outside.png").unwrap().unwrap().size, 131_072);
}
