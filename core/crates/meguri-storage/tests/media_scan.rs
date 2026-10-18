mod common;
use meguri_storage::media::{MediaError, MediaStore};
use rusqlite::{Connection, params};
use std::{fs, os::unix::fs::MetadataExt, sync::Arc};

type ScanHook = Box<dyn FnOnce() + Send>;
static SCAN_HOOK: std::sync::Mutex<Option<ScanHook>> = std::sync::Mutex::new(None);

#[test]
fn media_reopening_preserves_the_unchanged_folder_fast_path() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let store = MediaStore::open(&path, Arc::new(common::clock())).unwrap();
    store.add("registered.png", b"registered").unwrap();
    drop(store);
    let store = MediaStore::open(&path, Arc::new(common::clock())).unwrap();
    store.check_changes().unwrap();
    assert_eq!(store.diagnostics().scans, 0);
    assert_eq!(store.diagnostics().hashes, 0);
    fs::write(store.media_dir().join("outside.png"), b"outside").unwrap();
    drop(store);
    let store = MediaStore::open(&path, Arc::new(common::clock())).unwrap();
    store.check_changes().unwrap();
    assert_eq!(store.diagnostics().scans, 1);
    assert!(store.entries().unwrap().iter().any(|e| e.name == "outside.png"));
}
fn before_hash() {
    let hook = SCAN_HOOK.lock().unwrap().take();
    if let Some(hook) = hook {
        hook();
    }
}

#[test]
fn media_scanning_does_not_hold_the_journal_lock_while_hashing() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
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
        meguri_model::faultpoint::Fault::Callback(before_hash),
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
fn media_unchanged_folder_at_30000_files_does_not_enumerate_or_hash() {
    let dir = tempfile::tempdir().unwrap();
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
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
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
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
        .prepare("EXPLAIN QUERY PLAN SELECT name,sha1,change_counter FROM media WHERE dirty=1 ORDER BY name")
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
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
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
    let store = MediaStore::open(&dir.path().join("collection.anki2"), Arc::new(common::clock())).unwrap();
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
