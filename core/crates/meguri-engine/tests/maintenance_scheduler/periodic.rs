use super::common::{NOW_MS, copy_fixture, open_config};
use meguri_engine::{
    CancelToken, Collection, Engine, EngineConfig, EngineError, EventKind, LongOp, OpSource, OpSpec, Ports,
    SuspendParticipant,
    device_state::{DeviceState, LastBackup},
    maintenance::{BackupEntryTimes, LiveBackup, MaintenanceTrigger as Trigger, interval_deadline},
    ports::{BackupReceipt, BackupService},
};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    time::EpochMillis,
};
use meguri_storage::{ReadRows, ReaderRole, StampMode};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

#[derive(Default)]
struct Entries {
    times: Mutex<Vec<i128>>,
    reads: AtomicUsize,
}
impl BackupEntryTimes for Entries {
    fn read(&self, _: &Path) -> Result<Vec<i128>, EngineError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        Ok(self.times.lock().unwrap().clone())
    }
}
type During = Box<dyn FnOnce(&Collection, &CancelToken) -> Result<(), EngineError> + Send>;
#[derive(Default)]
struct Writer {
    calls: AtomicUsize,
    hook: Mutex<Option<During>>,
}
impl BackupService for Writer {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        unreachable!()
    }
    fn backup_live(&self, c: &Collection, cancel: &CancelToken) -> Result<LiveBackup, EngineError> {
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let lease = c.backup_reader()?;
        let snapshot = lease.snapshot()?;
        let snapshot_mod = snapshot.col()?.modified.0;
        drop(snapshot);
        drop(lease);
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook(c, cancel)?;
        }
        cancel.check()?;
        let receipt = BackupReceipt {
            name: format!("witness-{count}"),
            size_bytes: 1,
            instant: EpochMillis(NOW_MS),
            snapshot_modified: EpochMillis(snapshot_mod),
        };
        DeviceState::for_collection(c.path()).set_last_backup(&LastBackup {
            snapshot_mod,
            instant: NOW_MS,
            name: receipt.name.clone(),
        })?;
        Ok(LiveBackup {
            receipt,
            deleted_names: vec![],
        })
    }
}
struct Policy {
    engine: Engine,
    c: Collection,
    clock: Arc<StepClock>,
    entries: Arc<Entries>,
    writer: Arc<Writer>,
    _dir: tempfile::TempDir,
    backups: tempfile::TempDir,
    path: PathBuf,
}
impl Policy {
    fn new(minutes: u32) -> Self {
        let (dir, path) = copy_fixture("schema-18.anki2");
        let backups = tempfile::tempdir().unwrap();
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let entries = Arc::new(Entries::default());
        let writer = Arc::new(Writer::default());
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let mut options = open_config(&clock)
            .ports(Ports::new().backup_service(writer.clone()))
            .backups_directory(backups.path().to_owned());
        options.backup_entry_times = entries.clone();
        let c = engine.open_raw(&path, options).unwrap();
        meguri_engine::backup_limits::save(
            &c,
            meguri_engine::backup_limits::BackupLimits {
                minimum_interval_mins: minutes,
                ..Default::default()
            },
        )
        .unwrap();
        engine.maintenance().wait_idle_for_testing();
        Self {
            engine,
            c,
            clock,
            entries,
            writer,
            _dir: dir,
            backups,
            path,
        }
    }
    fn reference(&self) -> i64 {
        let modified = self.c.reader(ReaderRole::Query).unwrap().col().unwrap().modified.0;
        DeviceState::for_collection(&self.path)
            .set_last_backup(&LastBackup {
                snapshot_mod: modified,
                instant: self.clock.now_ms(),
                name: "retained-reference".into(),
            })
            .unwrap();
        self.engine.maintenance().reevaluate().unwrap();
        modified
    }
    fn set_mod(&self, value: i64) {
        self.c
            .run(
                OpSpec::named("periodic mod", OpSource::Rollover).stamp(StampMode::Raw),
                move |ctx| {
                    let mut col = ctx.scope().col()?;
                    col.modified = EpochMillis(value);
                    ctx.scope().put_col(&col)?;
                    Ok(())
                },
            )
            .unwrap();
        self.engine.maintenance().wait_idle_for_testing();
    }
    fn deadline(&self) -> Option<i64> {
        self.engine.maintenance().deadline().map(|at| at.0)
    }
    fn run(&self, trigger: Trigger) {
        self.engine.run_due_maintenance(trigger).unwrap();
        self.engine.maintenance().wait_idle_for_testing();
    }
}
#[test]
fn exact_creation_boundaries_future_entries_zero_and_large_intervals() {
    let c = 123_456_789_012_345i128;
    let end_ms = (c + 60_000_000_000 + 999_999) / 1_000_000;
    assert_eq!(interval_deadline(end_ms as i64 - 1, 1, &[c]), Some(end_ms as i64));
    assert_eq!(interval_deadline(end_ms as i64, 1, &[c]), None);
    assert_eq!(interval_deadline(0, 1, &[1]), None, "future entry");
    assert_eq!(interval_deadline(0, 0, &[0]), None);
    assert_eq!(interval_deadline(0, 1_073_741_824, &[0]), Some(64_424_509_440_000));
    assert_eq!(
        interval_deadline(i64::MIN, u32::MAX, &[i128::from(i64::MIN) * 1_000_000]),
        Some(i64::MIN + i64::from(u32::MAX) * 60_000)
    );
    assert_eq!(interval_deadline(0, 1, &[-60_000_000_001]), None);
}
#[test]
fn commits_cache_one_deadline_and_undo_to_reference_disarms_once() {
    let p = Policy::new(30);
    let reference = p.reference();
    *p.entries.times.lock().unwrap() = vec![i128::from(NOW_MS - 600_000) * 1_000_000];
    let events = p.engine.stream().subscribe();
    let reads = p.entries.reads.load(Ordering::SeqCst);
    p.set_mod(reference + 1);
    assert_eq!(p.deadline(), Some(NOW_MS + 1_200_000));
    for n in 2..=101 {
        p.set_mod(reference + n);
    }
    assert_eq!(p.entries.reads.load(Ordering::SeqCst), reads + 1);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0, "no autonomous timer or poll");
    p.set_mod(reference);
    p.set_mod(reference);
    assert_eq!(p.deadline(), None);
    let deadlines: Vec<_> = std::iter::from_fn(|| events.try_recv())
        .filter_map(|e| match &e.kind {
            EventKind::MaintenanceDeadline { at } => Some(*at),
            _ => None,
        })
        .collect();
    assert_eq!(deadlines, [Some(NOW_MS + 1_200_000), None]);
}
#[test]
fn failed_attempt_floor_exactly_five_minutes_and_background_and_quit_ignore_it() {
    let p = Policy::new(0);
    *p.writer.hook.lock().unwrap() = Some(Box::new(|_, _| {
        Err(EngineError::Storage(meguri_storage::StorageError::Io {
            detail: "synthetic disk failure".into(),
        }))
    }));
    p.run(Trigger::Opened);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(p.deadline(), Some(NOW_MS + 300_000));
    p.clock.step_ms(299_999);
    p.run(Trigger::Deadline);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 1);
    p.clock.step_ms(1);
    p.run(Trigger::Deadline);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 2);
    assert_eq!(p.deadline(), None);
    p.set_mod(NOW_MS + 1);
    p.run(Trigger::BackgroundProcessing);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 2);
    p.run(Trigger::Background);
    p.set_mod(NOW_MS + 2);
    p.run(Trigger::Quit);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 4);
}
#[test]
fn a_new_entry_moves_only_at_recheck_and_exclusive_boundaries_clear_and_rearm() {
    let p = Policy::new(30);
    let reference = p.reference();
    p.set_mod(reference - 1);
    assert_eq!(p.deadline(), Some(NOW_MS));
    *p.entries.times.lock().unwrap() = vec![i128::from(NOW_MS) * 1_000_000];
    p.run(Trigger::Deadline);
    assert_eq!(p.deadline(), Some(NOW_MS + 1_800_000));
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    let slot = p.engine.acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    assert_eq!(p.deadline(), None);
    p.run(Trigger::Background);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    drop(exclusive);
    p.engine.maintenance().wait_idle_for_testing();
    assert_eq!(p.deadline(), Some(NOW_MS + 1_800_000));
}
#[test]
fn snapshot_reference_is_not_the_live_value_at_backup_end() {
    let p = Policy::new(0);
    let old = p.c.reader(ReaderRole::Query).unwrap().col().unwrap().modified.0;
    *p.writer.hook.lock().unwrap() = Some(Box::new(move |c, _| {
        c.run(OpSpec::named("concurrent change", OpSource::UserUndoable), |ctx| {
            ctx.scope().set_config("periodic witness", &1u64.into())?;
            Ok(())
        })?;
        Ok(())
    }));
    p.clock.step_ms(1);
    p.run(Trigger::Background);
    assert_eq!(
        DeviceState::for_collection(&p.path)
            .last_backup()
            .unwrap()
            .unwrap()
            .snapshot_mod,
        old
    );
    assert_eq!(p.deadline(), Some(NOW_MS + 300_001));
}
#[test]
fn reference_survives_reopen_and_relaunch_even_when_the_backup_was_retained_away() {
    let p = Policy::new(0);
    p.reference();
    p.engine.close().unwrap();
    let mut options = open_config(&p.clock)
        .ports(Ports::new().backup_service(p.writer.clone()))
        .backups_directory(p.backups.path().to_owned());
    options.backup_entry_times = p.entries.clone();
    p.engine.open_raw(&p.path, options.clone()).unwrap();
    p.run(Trigger::Opened);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    p.engine.close().unwrap();
    let other = Engine::new(EngineConfig::default()).unwrap();
    other.open_raw(&p.path, options).unwrap();
    other.run_due_maintenance(Trigger::Opened).unwrap();
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    assert_eq!(other.maintenance().deadline(), None);
}
#[test]
fn suspend_cancels_a_running_periodic_copy_keeps_deadline_and_does_not_count_attempt() {
    let p = Policy::new(0);
    let (started, entered) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    *p.writer.hook.lock().unwrap() = Some(Box::new(move |_, cancel| {
        started.send(cancel.clone()).unwrap();
        resume.recv_timeout(Duration::from_secs(10)).unwrap();
        cancel.check()
    }));
    std::thread::scope(|scope| {
        let run = scope.spawn(|| p.engine.run_due_maintenance(Trigger::Deadline));
        let cancel = entered.recv_timeout(Duration::from_secs(10)).unwrap();
        // Wake the test when suspension cancels the copy. Keep its worker
        // blocked until cancellation has been observed.
        struct Cancelled(mpsc::Sender<()>);
        impl std::task::Wake for Cancelled {
            fn wake(self: Arc<Self>) {
                self.0.send(()).unwrap();
            }
        }
        let (stopped, stopping) = mpsc::channel();
        let waker = std::task::Waker::from(Arc::new(Cancelled(stopped)));
        let mut context = std::task::Context::from_waker(&waker);
        let mut future = std::pin::pin!(cancel.cancelled());
        assert!(std::future::Future::poll(future.as_mut(), &mut context).is_pending());
        let maintenance = p.engine.maintenance().clone();
        let suspend =
            scope.spawn(move || maintenance.suspend_until(meguri_engine::monotonic::now() + Duration::from_secs(10)));
        stopping.recv_timeout(Duration::from_secs(10)).unwrap();
        release.send(()).unwrap();
        run.join().unwrap().unwrap();
        suspend.join().unwrap().unwrap();
    });
    assert_eq!(DeviceState::for_collection(&p.path).last_backup().unwrap(), None);
    assert_eq!(p.deadline(), Some(NOW_MS));
    p.engine.maintenance().resume().unwrap();
    p.run(Trigger::Deadline);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 2);
    assert_eq!(p.deadline(), None);
}

#[test]
fn resume_rollover_delivers_the_pending_arm_once_without_a_later_user_commit() {
    let p = Policy::new(30);
    let reference = p.reference();
    assert_eq!(p.deadline(), None);
    let events = p.engine.stream().subscribe();
    let reads = p.entries.reads.load(Ordering::SeqCst);
    p.engine.suspend(Duration::from_secs(2)).unwrap();
    // An offline change while SQLite is closed is discovered by real resume.
    // Rollover and its privileged commit then run while backups are suspended.
    let database = meguri_storage::raw::PlainDatabase::open(&p.path).unwrap();
    database
        .execute_batch(&format!("UPDATE col SET mod = {}", reference - 1))
        .unwrap();
    drop(database);
    p.clock.step_ms(86_400_000);
    let report = p.engine.resume().unwrap();
    assert!(report.external_change);
    assert!(report.rolled_over);
    p.engine.maintenance().wait_idle_for_testing();
    assert_eq!(p.deadline(), Some(p.clock.now_ms()));
    assert_eq!(p.entries.reads.load(Ordering::SeqCst), reads + 1);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    let arms: Vec<_> = std::iter::from_fn(|| events.try_recv())
        .filter_map(|event| match event.kind {
            EventKind::MaintenanceDeadline { at } => Some(at),
            _ => None,
        })
        .collect();
    assert_eq!(arms, [Some(p.clock.now_ms())]);
}

#[test]
fn quit_expires_while_periodic_owner_is_still_blocked() {
    let p = Policy::new(0);
    let (started, entered) = mpsc::channel();
    let (release, blocked) = mpsc::channel();
    *p.writer.hook.lock().unwrap() = Some(Box::new(move |_, _| {
        started.send(()).unwrap();
        blocked.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(())
    }));
    std::thread::scope(|scope| {
        let owner = scope.spawn(|| p.engine.run_due_maintenance(Trigger::Background));
        entered.recv_timeout(Duration::from_secs(10)).unwrap();
        let (finished, result) = mpsc::channel();
        let start = meguri_engine::monotonic::now();
        let engine = &p.engine;
        let quit = scope.spawn(move || finished.send(engine.run_due_maintenance(Trigger::Quit)).unwrap());
        let outcome = result.recv_timeout(Duration::from_secs(10)).unwrap();
        assert_eq!(outcome, Err(EngineError::Cancelled));
        assert!(meguri_engine::monotonic::now() - start < Duration::from_millis(2200));
        assert_eq!(p.writer.calls.load(Ordering::SeqCst), 1);
        assert_eq!(DeviceState::for_collection(&p.path).last_backup().unwrap(), None);
        release.send(()).unwrap();
        owner.join().unwrap().unwrap();
        quit.join().unwrap();
    });
    p.engine.maintenance().wait_idle_for_testing();
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 1);
    assert_eq!(p.deadline(), None);
    assert_eq!(
        DeviceState::for_collection(&p.path)
            .last_backup()
            .unwrap()
            .unwrap()
            .name,
        "witness-1"
    );
}

#[test]
fn actual_resume_keeps_an_armed_deadline_without_an_unrequested_scan() {
    let p = Policy::new(30);
    let reference = p.reference();
    *p.entries.times.lock().unwrap() = vec![i128::from(NOW_MS) * 1_000_000];
    p.set_mod(reference + 1);
    let retained = p.deadline();
    assert_eq!(retained, Some(NOW_MS + 1_800_000));
    let reads = p.entries.reads.load(Ordering::SeqCst);
    let events = p.engine.stream().subscribe();
    p.engine.suspend(Duration::from_secs(2)).unwrap();
    p.clock.step_ms(86_400_000);
    assert!(p.engine.resume().unwrap().rolled_over);
    p.engine.maintenance().wait_idle_for_testing();
    assert_eq!(p.deadline(), retained);
    assert_eq!(p.entries.reads.load(Ordering::SeqCst), reads);
    assert_eq!(p.writer.calls.load(Ordering::SeqCst), 0);
    assert!(!std::iter::from_fn(|| events.try_recv()).any(|e| matches!(e.kind, EventKind::MaintenanceDeadline { .. })));
}
