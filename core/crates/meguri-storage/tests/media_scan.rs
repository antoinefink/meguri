mod common;
use meguri_storage::media::{MediaBaselineOwner, MediaError, MediaStore, MediaSyncState};
use rusqlite::{Connection, params};
use std::{fs, os::unix::fs::MetadataExt, sync::Arc};

type ScanHook = Box<dyn FnOnce() + Send>;
static SCAN_HOOK: std::sync::Mutex<Option<ScanHook>> = std::sync::Mutex::new(None);

// Pause journal inspection to test cancellation before and after the first effect.
// Recreating a removed directory supplies that effect. With the directory present,
// cancellation must preserve every byte of the journal and its existing sidecars.
#[test]
fn media_open_held_journal_probe_cancels_only_before_first_effect() {
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        mpsc,
    };
    use std::time::Duration;
    for create_folder in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        let store = MediaStore::open(&path, Arc::new(common::clock()), common::media_random()).unwrap();
        store.add("retained.png", b"retained bytes").unwrap();
        drop(store);
        if create_folder {
            fs::remove_dir(dir.path().join("media.trash")).unwrap();
        }
        let journal_bytes = || {
            ["", "-wal", "-shm", "-journal"].map(|suffix| {
                match fs::read(dir.path().join(format!("meguri-media.db{suffix}"))) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => panic!("journal snapshot: {e}"),
                }
            })
        };
        let before = journal_bytes();
        let cancel = Arc::new(AtomicBool::new(false));
        let effected = Arc::new(AtomicBool::new(false));
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *SCAN_HOOK.lock().unwrap() = Some(Box::new(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).expect("probe release");
        }));
        meguri_model::faultpoint::arm(
            "media.journal.after_probe",
            meguri_model::faultpoint::Fault::Callback(run_scan_hook),
        );
        let result = std::thread::scope(|threads| {
            let worker = threads.spawn(|| {
                MediaStore::open_with_boundary(&path, Arc::new(common::clock()), common::media_random(), &mut || {
                    if !effected.load(Ordering::SeqCst) && cancel.load(Ordering::SeqCst) {
                        return Err(MediaError::Cancelled);
                    }
                    effected.store(true, Ordering::SeqCst);
                    Ok(())
                })
            });
            let held = reached.recv_timeout(Duration::from_secs(5));
            let prior_effect = effected.load(Ordering::SeqCst);
            cancel.store(true, Ordering::SeqCst);
            // Always release before asserting, including a failed handshake.
            let _ = release.send(());
            let result = worker.join().unwrap();
            held.expect("real journal probe completed before admission");
            assert_eq!(prior_effect, create_folder);
            result
        });
        meguri_model::faultpoint::disarm("media.journal.after_probe");
        if create_folder {
            let store = result.expect("folder creation preserves actual successful open after cancellation");
            assert_eq!(store.entries().unwrap().len(), 1);
            assert!(dir.path().join("media.trash").is_dir());
            drop(store);
        } else {
            assert!(matches!(result, Err(MediaError::Cancelled)));
            assert!(!effected.load(Ordering::SeqCst));
            assert_eq!(journal_bytes(), before);
            let store = MediaStore::open(&path, Arc::new(common::clock()), common::media_random()).unwrap();
            assert_eq!(store.entries().unwrap().len(), 1);
        }
        assert_eq!(
            fs::read(dir.path().join("collection.media/retained.png")).unwrap(),
            b"retained bytes"
        );
    }
}

#[test]
fn media_reopening_preserves_the_unchanged_folder_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let store = MediaStore::open(&path, Arc::new(common::clock()), common::media_random()).unwrap();
    store.add("registered.png", b"registered").unwrap();
    drop(store);
    let store = MediaStore::open(&path, Arc::new(common::clock()), common::media_random()).unwrap();
    store.check_changes().unwrap();
    assert_eq!(store.diagnostics().scans, 0);
    assert_eq!(store.diagnostics().hashes, 0);
    fs::write(store.media_dir().join("outside.png"), b"outside").unwrap();
    drop(store);
    let store = MediaStore::open(&path, Arc::new(common::clock()), common::media_random()).unwrap();
    store.check_changes().unwrap();
    assert_eq!(store.diagnostics().scans, 1);
    assert!(store.entries().unwrap().iter().any(|e| e.name == "outside.png"));
}

#[test]
fn marker_report_skips_callbacks_and_refuses_suspension_before_folder_access() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    store.check_changes_report().unwrap();
    let before = store.diagnostics();
    let report = store
        .check_changes_report_with_progress(|_, _| panic!("matching marker must skip file callbacks"))
        .unwrap();
    assert!(report.completed);
    assert_eq!((report.listed, report.seen, report.hashed), (false, 0, 0));
    assert_eq!(store.last_check_report().unwrap(), Some(report));
    assert_eq!(store.diagnostics(), before);
    assert!(store.try_close().unwrap());
    let moved = dir.path().join("suspended-media");
    fs::rename(store.media_dir(), &moved).unwrap();
    let result = store.check_changes_report_with_progress(|_, _| panic!("suspended store invoked callback"));
    assert_eq!(result, Err(MediaError::Database("media journal is closed".into())));
    assert_eq!(store.diagnostics(), before);
    fs::rename(moved, store.media_dir()).unwrap();
    store.resume().unwrap();
    assert!(!store.check_changes_report().unwrap().listed);
}

#[test]
fn marker_preflight_io_failure_replaces_a_nonempty_report_and_recovers() {
    for with_progress in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaStore::open(
            &dir.path().join("collection.anki2"),
            Arc::new(common::clock()),
            common::media_random(),
        )
        .unwrap();
        fs::write(store.media_dir().join("cached.png"), b"cached").unwrap();
        let successful = store.register_full_report(|_, _| true).unwrap();
        assert!(successful.completed && successful.listed);
        assert_eq!((successful.seen, successful.hashed, successful.added), (1, 1, 1));
        let before = store.diagnostics();
        let marker = store.change_marker().unwrap();
        assert!(marker.is_some());
        let counter = store.change_counter().unwrap();
        let entries = store.entries().unwrap();
        let moved = dir.path().join("missing-media");
        fs::rename(store.media_dir(), &moved).unwrap();
        let result = if with_progress {
            store.check_changes_report_with_progress(|_, _| panic!("missing folder invoked callback"))
        } else {
            store.check_changes_report()
        };
        assert!(matches!(result, Err(MediaError::Io(_))));
        let failed = store.last_check_report().unwrap().unwrap();
        assert_ne!(failed, successful);
        assert!(!failed.completed && !failed.listed);
        assert_eq!(
            (
                failed.seen,
                failed.hashed,
                failed.added,
                failed.changed,
                failed.removed,
                failed.renamed,
                failed.skipped,
            ),
            (0, 0, 0, 0, 0, 0, 0)
        );
        assert_eq!(store.diagnostics(), before);
        assert_eq!(store.change_marker().unwrap(), marker);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(store.entries().unwrap(), entries);
        fs::rename(moved, store.media_dir()).unwrap();
        let restored = store
            .check_changes_report_with_progress(|_, _| panic!("restored marker must skip callbacks"))
            .unwrap();
        assert!(restored.completed);
        assert_eq!((restored.listed, restored.seen, restored.hashed), (false, 0, 0));
        assert_eq!(store.last_check_report().unwrap(), Some(restored));
        assert_eq!(store.diagnostics(), before);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(store.entries().unwrap(), entries);
    }
}

fn run_scan_hook() {
    let hook = SCAN_HOOK.lock().unwrap().take();
    if let Some(hook) = hook {
        hook();
    }
}

#[test]
fn media_scanning_does_not_hold_the_journal_lock_while_hashing() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    fs::write(store.media_dir().join("scan.png"), b"scan").unwrap();
    let concurrent = store.clone();
    let finished = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let observed = finished.clone();
    let writer = Arc::new(std::sync::Mutex::new(None));
    let writer_for_hook = writer.clone();
    *SCAN_HOOK.lock().unwrap() = Some(Box::new(move || {
        let (send, receive) = std::sync::mpsc::sync_channel(1);
        *writer_for_hook.lock().unwrap() = Some(std::thread::spawn(move || {
            concurrent.add("concurrent.png", b"concurrent").unwrap();
            let _ = send.send(());
        }));
        observed.store(
            receive.recv_timeout(std::time::Duration::from_secs(1)).is_ok(),
            std::sync::atomic::Ordering::SeqCst,
        );
    }));
    meguri_model::faultpoint::arm(
        "media.scan.before_hash",
        meguri_model::faultpoint::Fault::Callback(run_scan_hook),
    );
    let result = store.register_full(|_, _| true);
    meguri_model::faultpoint::disarm_all();
    writer.lock().unwrap().take().unwrap().join().unwrap();
    result.unwrap();
    assert!(
        finished.load(std::sync::atomic::Ordering::SeqCst),
        "a scan held the journal during file hashing"
    );
}

#[test]
fn scan_reset_keeps_the_marker_unset_until_absent_retained_facts_are_removed() {
    for rebind in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let store = MediaStore::open(
            &dir.path().join("collection.anki2"),
            Arc::new(common::clock()),
            common::media_random(),
        )
        .unwrap();
        store.bind_sync_owner(&MediaBaselineOwner::new("m6:before")).unwrap();
        store.add("absent.png", b"absent").unwrap();
        fs::remove_file(store.media_dir().join("absent.png")).unwrap();
        let cutoff = store.change_counter().unwrap();
        let folder_time = fs::metadata(store.media_dir()).unwrap();
        let marker = format!("{}:{}", folder_time.mtime(), folder_time.mtime_nsec());
        let snapshot_store = store.clone();
        let checked = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let observed = checked.clone();
        *SCAN_HOOK.lock().unwrap() = Some(Box::new(move || {
            // This test-only callback observes the guard between the two
            // snapshot reads. It never waits for or mutates the journal.
            assert!(!snapshot_store.try_close().unwrap());
            observed.store(true, std::sync::atomic::Ordering::SeqCst);
        }));
        meguri_model::faultpoint::arm(
            "media.scan.after_cutoff",
            meguri_model::faultpoint::Fault::Callback(run_scan_hook),
        );
        let mut reset = false;
        let result = store.register_full(|_, _| {
            if !reset {
                if rebind {
                    store.bind_sync_owner(&MediaBaselineOwner::new("m6:after")).unwrap();
                } else {
                    store.reset_sync().unwrap();
                }
                reset = true;
            }
            true
        });
        meguri_model::faultpoint::disarm("media.scan.after_cutoff");
        result.unwrap();
        assert!(checked.load(std::sync::atomic::Ordering::SeqCst));
        let retained = store.sync_entry("absent.png").unwrap().unwrap();
        assert!(retained.live && retained.change_counter > cutoff);
        assert_eq!(retained.sync_state, MediaSyncState::None);
        let after = fs::metadata(store.media_dir()).unwrap();
        assert_eq!(
            (after.mtime(), after.mtime_nsec()),
            (folder_time.mtime(), folder_time.mtime_nsec())
        );
        assert_eq!(store.change_marker().unwrap(), None);
        assert!(store.check_changes_report().unwrap().listed);
        assert!(store.sync_entry("absent.png").unwrap().is_none());
        assert_eq!(store.change_marker().unwrap(), Some(marker));
        assert!(!store.check_changes_report().unwrap().listed);
    }
}

#[test]
fn media_unchanged_folder_at_30000_files_does_not_enumerate_or_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    for i in 0..30_000 {
        fs::write(store.media_dir().join(format!("file-{i}.png")), []).unwrap();
    }
    // A persisted marker is all the fast path needs; avoid 30k durable
    // registration writes in the setup of this latency regression.
    let metadata = fs::metadata(store.media_dir()).unwrap();
    let db = Connection::open(dir.path().join("meguri-media.db")).unwrap();
    db.execute(
        "INSERT INTO meta VALUES('folder_mtime',?1)",
        [format!("{}:{}", metadata.mtime(), metadata.mtime_nsec())],
    )
    .unwrap();
    drop(db);
    let before = store.diagnostics();
    let started = meguri_testkit::wallclock::Stopwatch::start();
    store.check_changes().unwrap();
    let elapsed = started.elapsed();
    assert_eq!(store.diagnostics(), before);
    assert!(
        elapsed < std::time::Duration::from_millis(500),
        "unchanged check took {elapsed:?}"
    );
}

#[test]
fn media_pending_query_uses_its_partial_index_with_20000_clean_tombstones() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    store.add("dirty.png", b"dirty").unwrap();
    let mut db = Connection::open(dir.path().join("meguri-media.db")).unwrap();
    let tx = db.transaction().unwrap();
    {
        let mut insert = tx.prepare("INSERT INTO media(name,case_key,sha1,size,mtime_secs,mtime_nanos,live,dirty,change_counter,pending_download) VALUES(?1,?1,NULL,0,0,0,0,0,0,0)").unwrap();
        for i in 0..20_000 {
            insert.execute(params![format!("deleted-{i}")]).unwrap();
        }
    }
    tx.commit().unwrap();
    let plan: Vec<String> = db
        .prepare(
            "EXPLAIN QUERY PLAN SELECT name,sha1,change_counter FROM media WHERE dirty=1 ORDER BY change_counter,name",
        )
        .unwrap()
        .query_map([], |row| row.get(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap();
    assert!(plan.iter().any(|detail| detail.contains("media_dirty")), "{plan:?}");
    assert_eq!(
        store
            .pending()
            .unwrap()
            .iter()
            .map(|c| c.name.as_str())
            .collect::<Vec<_>>(),
        ["dirty.png"]
    );
}

#[test]
fn media_cancelled_scan_leaves_completed_rows_and_requires_a_fresh_scan() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    fs::write(store.media_dir().join("a.png"), b"a").unwrap();
    fs::write(store.media_dir().join("b.png"), b"b").unwrap();
    assert_eq!(store.register_full(|done, _| done == 0), Err(MediaError::Cancelled));
    assert_eq!(store.entries().unwrap().len(), 1);
    let scans = store.diagnostics().scans;
    store.check_changes().unwrap();
    assert_eq!(store.diagnostics().scans, scans + 1);
    assert_eq!(store.pending().unwrap().len(), 2);
}

#[test]
fn media_add_during_a_scan_is_not_mistaken_for_a_deleted_file() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(
        &dir.path().join("collection.anki2"),
        Arc::new(common::clock()),
        common::media_random(),
    )
    .unwrap();
    store.add("first.png", b"first").unwrap();
    let mut added = false;
    store
        .register_full(|_, _| {
            if !added {
                store.add("during.png", b"during").unwrap();
                added = true;
            }
            true
        })
        .unwrap();
    let entry = store
        .entries()
        .unwrap()
        .into_iter()
        .find(|e| e.name == "during.png")
        .unwrap();
    assert!(entry.live && entry.dirty);
    assert_eq!(fs::read(store.media_dir().join("during.png")).unwrap(), b"during");
}
