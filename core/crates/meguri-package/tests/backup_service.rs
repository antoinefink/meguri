//! Real T5 writer/admission. No server or protocol work.
use meguri_engine::{CancelToken, EngineError, LongOp, maintenance::BackupError};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::normal_support::Fixture;

#[test]
fn back_up_now_writes_when_unchanged_and_returns_real_name_size_and_reference() {
    let f = Fixture::new(0);
    let modified = f.collection.reader(ReaderRole::Query).unwrap().col().unwrap().modified;
    let first = f.host.engine().back_up_now(&CancelToken::default()).unwrap();
    let second = f.host.engine().back_up_now(&CancelToken::default()).unwrap();
    assert_ne!(first.receipt.name, second.receipt.name);
    for written in [first, second.clone()] {
        let path = f.dir.path().join("backups").join(&written.receipt.name);
        assert!(written.receipt.size_bytes > 0);
        assert_eq!(std::fs::metadata(path).unwrap().len(), written.receipt.size_bytes);
        assert_eq!(written.receipt.snapshot_modified, modified);
    }
    let record = meguri_engine::device_state::DeviceState::for_collection(f.collection.path())
        .last_backup()
        .unwrap()
        .unwrap();
    assert_eq!(record.name, second.receipt.name);
    assert_eq!(record.snapshot_mod, modified.0);
    f.host.engine().maintenance().wait_idle_for_testing();
    assert_eq!(f.host.engine().maintenance().deadline(), None);
}
#[test]
fn back_up_now_has_exact_ready_exclusive_failed_and_closed_admission() {
    let f = Fixture::new(0);
    let slot = f.host.engine().acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    assert!(matches!(
        f.host.engine().back_up_now(&CancelToken::default()),
        Err(BackupError::Engine(EngineError::CollectionUnavailable { .. }))
    ));
    drop(exclusive);
    drop(slot);
    f.host.engine().mark_corrupt("synthetic corrupt admission");
    assert_eq!(
        f.host.engine().back_up_now(&CancelToken::default()),
        Err(BackupError::CollectionCorrupt)
    );
    f.host.close().unwrap();
    assert!(matches!(
        f.host.engine().back_up_now(&CancelToken::default()),
        Err(BackupError::Engine(EngineError::CollectionUnavailable { .. }))
    ));
}

#[test]
fn quit_expires_before_the_forced_provider_owner_releases_its_copy() {
    use meguri_engine::{device_state::DeviceState, maintenance::MaintenanceTrigger};
    use meguri_model::faultpoint::{self, Fault};
    use std::sync::{Mutex, OnceLock, mpsc};
    use std::time::Duration;
    type Barrier = (mpsc::Sender<()>, mpsc::Receiver<()>);
    static COPY: OnceLock<Mutex<Option<Barrier>>> = OnceLock::new();
    fn hold_copy() {
        let barrier = COPY.get().unwrap().lock().unwrap().take();
        if let Some((entered, release)) = barrier {
            entered.send(()).unwrap();
            release.recv_timeout(Duration::from_secs(10)).unwrap();
        }
    }
    use meguri_model::clock::Clock;
    let f = Fixture::new(1);
    f.host.engine().maintenance().wait_idle_for_testing();
    let (entered, started) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *COPY.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some((entered, blocked));
    faultpoint::arm("storage.backup.before_rename", Fault::Callback(hold_copy));
    std::thread::scope(|scope| {
        let (completed, completion) = mpsc::channel();
        let owner_engine = f.host.engine().clone();
        let owner = scope.spawn(move || {
            let outcome = owner_engine.back_up_now(&CancelToken::default());
            completed.send(()).unwrap();
            outcome
        });
        started.recv_timeout(Duration::from_secs(10)).unwrap();
        use std::os::unix::fs::MetadataExt;
        let temporary: Vec<_> = std::fs::read_dir(f.dir.path().join("backups"))
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".meguri-tmp-backup-")
            })
            .collect();
        assert_eq!(temporary.len(), 1);
        let copy_path = &temporary[0];
        let copy_identity = std::fs::metadata(copy_path).unwrap();
        let copy_bytes = std::fs::read(copy_path).unwrap();
        assert!(!copy_bytes.is_empty());
        let start = meguri_engine::monotonic::now();
        let (finished, result) = mpsc::channel();
        let quit_engine = f.host.engine().clone();
        let quit = scope.spawn(move || {
            finished
                .send(quit_engine.run_due_maintenance(MaintenanceTrigger::Quit))
                .unwrap();
        });
        let after_quit = result.recv_timeout(Duration::from_secs(10)).unwrap().unwrap();
        assert_eq!(
            after_quit,
            Some(meguri_model::time::EpochMillis(f.clock.now_ms() + 300_000))
        );
        assert!(meguri_engine::monotonic::now() - start < Duration::from_millis(2200));
        assert!(matches!(completion.try_recv(), Err(mpsc::TryRecvError::Empty)));
        let still_owned = std::fs::metadata(copy_path).unwrap();
        assert_eq!(
            (still_owned.dev(), still_owned.ino()),
            (copy_identity.dev(), copy_identity.ino())
        );
        assert_eq!(std::fs::read(copy_path).unwrap(), copy_bytes);
        assert_eq!(
            DeviceState::for_collection(f.collection.path()).last_backup().unwrap(),
            None
        );
        assert!(
            meguri_package::backup::listing::list(&f.dir.path().join("backups"))
                .unwrap_or_default()
                .is_empty()
        );
        release.send(()).unwrap();
        let written = owner.join().unwrap().unwrap();
        quit.join().unwrap();
        let published = std::fs::metadata(f.dir.path().join("backups").join(&written.receipt.name)).unwrap();
        assert_eq!(
            (published.dev(), published.ino()),
            (copy_identity.dev(), copy_identity.ino())
        );
        assert_eq!(
            DeviceState::for_collection(f.collection.path())
                .last_backup()
                .unwrap()
                .unwrap()
                .name,
            written.receipt.name
        );
    });
    faultpoint::disarm("storage.backup.before_rename");
    f.host.engine().maintenance().wait_idle_for_testing();
}
