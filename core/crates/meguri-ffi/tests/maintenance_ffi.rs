//! U133a test-first contract. One serialized fixture owns the real FFI runtime;
//! assertions read owned typed results only after the matching JobFinished.
use meguri_engine::{Engine, LongOp, OpSource, OpSpec, executor::catalog};
use meguri_ffi::{
    CoreEvent, CoreEventKind, CoreEventListener, JobOutcome, ProgressListener, ProgressUpdate,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    lifecycle::{self, CollectionFailedReason, CollectionLifecycle},
    maintenance::{self, *},
    review_session,
    test_hooks::{collection_for_testing, engine_for_testing},
};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    faultpoint::{self, Fault},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, raw::SqlValue};
use std::{
    fs,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicI64, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

const NOW: i64 = 1_894_701_600_000; // 2030-01-15 10:00 UTC, supplied to every native call.
const MB: u64 = 1024 * 1024;
static SERIAL: Mutex<()> = Mutex::new(());
struct Clock(AtomicI64);
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space {
    bytes: AtomicU64,
    calls: AtomicUsize,
    failed: AtomicBool,
}
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        self.calls.fetch_add(1, Ordering::SeqCst);
        if self.failed.load(Ordering::SeqCst) {
            DiskCapacity::Failed {
                detail: "fixture volume unavailable".into(),
            }
        } else {
            DiskCapacity::Available {
                bytes: self.bytes.load(Ordering::SeqCst),
            }
        }
    }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) {
        let _ = self.0.send(event);
    }
}
#[derive(Default)]
struct Progress(Mutex<Vec<ProgressUpdate>>);
impl ProgressListener for Progress {
    fn on_progress(&self, update: ProgressUpdate) {
        self.0.lock().unwrap().push(update);
    }
}
fn finish(rx: &mpsc::Receiver<CoreEvent>, id: u64) -> (JobOutcome, Vec<CoreEvent>) {
    let mut events = Vec::new();
    loop {
        let event = rx
            .recv_timeout(Duration::from_secs(20))
            .expect("matching maintenance terminal");
        let outcome = match &event.kind {
            CoreEventKind::JobFinished { id: got, outcome } if *got == id => Some(outcome.clone()),
            _ => None,
        };
        events.push(event);
        if let Some(outcome) = outcome {
            return (outcome, events);
        }
    }
}
struct Fixture {
    _dir: tempfile::TempDir,
    location: CollectionLocation,
    clock: Arc<Clock>,
    space: Arc<Space>,
    rx: mpsc::Receiver<CoreEvent>,
    engine: Engine,
}
impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().unwrap();
        let collection = dir.path().join("collection");
        let backups = dir.path().join("backups");
        fs::create_dir_all(&collection).unwrap();
        fs::create_dir_all(&backups).unwrap();
        meguri_engine::bootstrap::create(
            &collection.join("collection.anki2"),
            &FixedClock::new(NOW, UtcOffset::UTC),
            &SeededRandom::new(133),
        )
        .unwrap();
        let location = CollectionLocation {
            collection_directory: collection.to_str().unwrap().into(),
            backups_directory: backups.to_str().unwrap().into(),
            app_group_snapshot: dir.path().join("widget.snapshot").to_str().unwrap().into(),
            manual_transfer_directory: dir.path().join("ManualTransfers").to_str().unwrap().into(),
        };
        let (tx, rx) = mpsc::channel();
        meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
        let mut f = Self {
            _dir: dir,
            location,
            clock: Arc::new(Clock(AtomicI64::new(NOW))),
            space: Arc::new(Space {
                bytes: AtomicU64::new(u64::MAX),
                calls: AtomicUsize::new(0),
                failed: AtomicBool::new(false),
            }),
            rx,
            engine: engine_for_testing().unwrap(),
        };
        f.open();
        // Same setup as collection_ffi: real review eligibility, no second engine.
        let c = collection_for_testing().unwrap();
        c.run(
            OpSpec::named("maintenance fixture policy", OpSource::Maintenance),
            |ctx| {
                ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
                ctx.scope()
                    .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
                Ok(())
            },
        )
        .unwrap();
        let nt = meguri_engine::notetypes::read::by_name(&*c.reader(ReaderRole::Query).unwrap(), "Basic")
            .unwrap()
            .unwrap()
            .id;
        meguri_engine::notes::add(
            &c,
            meguri_engine::notes::AddNote {
                notetype_id: nt,
                fields: vec!["maintenance front".into(), "maintenance back".into()],
                tags: vec![],
                deck_id: meguri_model::ids::DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
        f.engine.maintenance().wait_idle_for_testing();
        f
    }
    fn open(&mut self) {
        let job = collection::start_open(self.location.clone(), self.clock.clone(), self.space.clone(), 133).unwrap();
        assert_eq!(finish(&self.rx, job.id).0, JobOutcome::Succeeded);
        self.engine.maintenance().wait_idle_for_testing();
    }
    fn backup_path(&self, name: &str) -> PathBuf {
        Path::new(&self.location.backups_directory).join(name)
    }
    fn path(&self) -> PathBuf {
        Path::new(&self.location.collection_directory).join("collection.anki2")
    }
    fn generation(&self) -> u64 {
        self.engine.generation().0
    }
    fn advance(&self, ms: i64) {
        self.clock.0.fetch_add(ms, Ordering::SeqCst);
    }
    fn create(&self) -> meguri_ffi::collection_entry::ImportBackupReceipt {
        let started = maintenance::start_create_backup().unwrap();
        assert_eq!(finish(&self.rx, started.job.id).0, JobOutcome::Succeeded);
        match started.result.outcome().unwrap() {
            CreateBackupOutcome::Created { backup } => backup,
            _ => panic!("drained backup must own its typed receipt"),
        }
    }
    fn due(&self, trigger: MaintenanceTrigger) -> Option<i64> {
        let started = maintenance::start_run_due_maintenance(trigger).unwrap();
        assert_eq!(finish(&self.rx, started.job.id).0, JobOutcome::Succeeded);
        self.engine.maintenance().wait_idle_for_testing();
        match started.result.outcome().unwrap() {
            DueMaintenanceOutcome::Completed { deadline_ms } => deadline_ms,
            _ => panic!("drained trigger must own its deadline, including None"),
        }
    }
    fn failed_restore(&self, name: &str) -> MaintenanceFailure {
        let started = maintenance::start_restore_backup(name.into()).unwrap();
        let terminal = finish(&self.rx, started.job.id).0;
        assert!(!matches!(terminal, JobOutcome::Succeeded));
        match started.result.outcome().unwrap() {
            RestoreBackupOutcome::Failed { failure } => failure,
            _ => panic!("typed failure must survive a failed JobFinished"),
        }
    }
    fn deck_name(&self) -> String {
        collection_for_testing()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .deck(meguri_model::ids::DeckId(1))
            .unwrap()
            .unwrap()
            .name
    }
    fn rename(&self, name: &str) {
        self.advance(1000);
        meguri_engine::decks::rename(
            &collection_for_testing().unwrap(),
            meguri_model::ids::DeckId(1),
            name.into(),
        )
        .unwrap();
        self.engine.maintenance().wait_idle_for_testing();
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        faultpoint::disarm_all();
        lifecycle::close_collection().unwrap();
    }
}

// Existing fault primitives provide a positive worker handshake; no sleeps,
// unbounded result map, foreign callback reentry or product testing hooks.
struct GateState {
    entered: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}
static GATE: Mutex<Option<GateState>> = Mutex::new(None);
fn stop_at_fault() {
    let state = GATE.lock().unwrap().take();
    if let Some(state) = state {
        state.entered.send(()).unwrap();
        state
            .release
            .recv_timeout(Duration::from_secs(20))
            .expect("release maintenance fault gate");
    }
}
struct Gate {
    point: &'static str,
    entered: mpsc::Receiver<()>,
    release: mpsc::Sender<()>,
}
impl Gate {
    fn arm(point: &'static str) -> Self {
        let (entered, rx) = mpsc::channel();
        let (release, wait) = mpsc::channel();
        *GATE.lock().unwrap() = Some(GateState { entered, release: wait });
        faultpoint::arm(point, Fault::Callback(stop_at_fault));
        Self {
            point,
            entered: rx,
            release,
        }
    }
    fn reached(&self) {
        self.entered
            .recv_timeout(Duration::from_secs(20))
            .expect("native boundary reached");
    }
    fn resume(&self) {
        self.release.send(()).unwrap();
    }
}
impl Drop for Gate {
    fn drop(&mut self) {
        let _ = self.release.send(());
        faultpoint::disarm(self.point);
        GATE.lock().unwrap().take();
    }
}

#[test]
fn forced_backup_real_receipt_progress_listing_labels_and_config_operation() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let limits = BackupLimits {
        daily: 21,
        weekly: 13,
        monthly: 7,
        minimum_interval_mins: 45,
    };
    assert!(maintenance::set_backup_limits(limits.clone()).unwrap());
    assert!(!maintenance::set_backup_limits(limits.clone()).unwrap());
    assert_eq!(maintenance::backup_limits().unwrap(), limits);
    let c = collection_for_testing().unwrap();
    let stored = meguri_engine::backup_limits::read(&*c.reader(ReaderRole::Query).unwrap()).unwrap();
    assert_eq!(
        (
            stored.daily,
            stored.weekly,
            stored.monthly,
            stored.minimum_interval_mins
        ),
        (21, 13, 7, 45)
    );
    let gate = Gate::arm("package.export.compress_chunk");
    let started = maintenance::start_create_backup().unwrap();
    gate.reached();
    let progress = Arc::new(Progress::default());
    started.job.progress.set_listener(progress.clone()).unwrap();
    gate.resume();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Succeeded);
    let CreateBackupOutcome::Created { backup } = started.result.outcome().unwrap() else {
        panic!("receipt")
    };
    assert_eq!(backup.name, "backup-2030-01-15-10.00.00.colpkg");
    assert_eq!(
        backup.size_bytes,
        fs::metadata(f.backup_path(&backup.name)).unwrap().len()
    );
    assert!(backup.size_bytes > 0);
    let updates = progress.0.lock().unwrap();
    assert!(!updates.is_empty(), "attached before native work finishes");
    for update in updates.iter() {
        assert_eq!(update.job_id, started.job.id);
        if let Some(total) = update.total {
            assert!(update.completed <= total);
        }
    }
    for pair in updates.windows(2) {
        if pair[0].phase == pair[1].phase {
            assert!(pair[0].completed <= pair[1].completed);
            assert_eq!(pair[0].total, pair[1].total);
        }
    }
    drop(updates);
    // Repeat forced create despite unchanged col.mod; it must produce another file.
    f.advance(1000);
    let newest = f.create();
    assert_eq!(newest.name, "backup-2030-01-15-10.00.01.colpkg");
    fs::copy(
        f.backup_path(&backup.name),
        f.backup_path("server-copy-2029-01-01-00.00.00.colpkg"),
    )
    .unwrap();
    fs::write(
        f.backup_path("server-copy-2028-01-01-00.00.00.unverified.anki2"),
        b"unverified bytes",
    )
    .unwrap();
    let listing = maintenance::list_backups().unwrap();
    assert_eq!(
        listing.iter().map(|b| b.name.as_str()).collect::<Vec<_>>(),
        vec![
            newest.name.as_str(),
            backup.name.as_str(),
            "server-copy-2029-01-01-00.00.00.colpkg",
            "server-copy-2028-01-01-00.00.00.unverified.anki2"
        ]
    );
    assert_eq!(
        listing.iter().map(|b| b.kind.clone()).collect::<Vec<_>>(),
        vec![
            BackupKind::Backup,
            BackupKind::Backup,
            BackupKind::ServerCopy,
            BackupKind::ServerCopyUnverified
        ]
    );
    assert_eq!(listing[0].size_bytes, newest.size_bytes);
}

#[test]
fn restore_replaces_generation_retains_current_safety_copy_and_publication_choice() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let original_name = f.deck_name();
    let backup = f.create();
    f.rename("Current collection must survive in safety copy");
    let generation = f.generation();
    let started = maintenance::start_restore_backup(backup.name.clone()).unwrap();
    let (terminal, events) = finish(&f.rx, started.job.id);
    assert_eq!(terminal, JobOutcome::Succeeded);
    let RestoreBackupOutcome::Restored { restored } = started.result.outcome().unwrap() else {
        panic!("restore receipt")
    };
    assert_eq!(restored.generation, generation + 1);
    assert!(restored.cloud_publication_pending);
    assert_eq!(restored.name, backup.name);
    assert_eq!(f.deck_name(), original_name);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready {
            generation: generation + 1
        }
    );
    assert_eq!(
        events
            .iter()
            .filter(|e| e.kind
                == CoreEventKind::CollectionReplaced {
                    generation: generation + 1
                })
            .count(),
        1
    );
    let safety = restored.safety_backup.unwrap();
    assert!(f.backup_path(&safety).is_file());
    assert!(
        maintenance::list_backups()
            .unwrap()
            .iter()
            .any(|entry| entry.name == safety)
    );
    // Restore the retained safety copy through exactly the same public surface.
    f.advance(1000);
    let back = maintenance::start_restore_backup(safety).unwrap();
    assert_eq!(finish(&f.rx, back.job.id).0, JobOutcome::Succeeded);
    assert!(matches!(
        back.result.outcome().unwrap(),
        RestoreBackupOutcome::Restored { .. }
    ));
    assert_eq!(f.deck_name(), "Current collection must survive in safety copy");
}

#[test]
fn corrupt_page_and_active_review_refuse_without_replacement() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let backup = f.create();
    let generation = f.generation();
    let bad = "backup-2020-01-01-00.00.00.colpkg";
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/fixtures/package/variants/corrupt-page.colpkg"),
        f.backup_path(bad),
    )
    .unwrap();
    let name = f.deck_name();
    let failure = f.failed_restore(bad);
    assert_eq!(failure.cause, MaintenanceError::Corrupt);
    assert!(!failure.candidate_active);
    assert_eq!(f.generation(), generation);
    assert_eq!(f.deck_name(), name);
    let start = review_session::start_session(1, None).unwrap();
    assert_eq!(finish(&f.rx, start.handle.id).0, JobOutcome::Succeeded);
    let review_session::ReviewStartResult::Started { session_id, .. } = start.result.result().unwrap().unwrap() else {
        panic!("real fixture review must start")
    };
    assert_eq!(f.failed_restore(&backup.name).cause, MaintenanceError::SessionActive);
    let end = review_session::end_session(session_id).unwrap();
    assert_eq!(finish(&f.rx, end.id).0, JobOutcome::Succeeded);
    assert_eq!(f.generation(), generation);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation }
    );
}

static INSPECTION_WRITES: AtomicUsize = AtomicUsize::new(0);
fn count_inspection() {
    INSPECTION_WRITES.fetch_add(1, Ordering::SeqCst);
}
#[test]
fn shared_one_way_slot_refuses_restore_before_native_io_without_starting_any_service() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let backup = f.create();
    let slot = f.engine.acquire_slot(LongOp::OneWaySync).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    let before = f.space.calls.load(Ordering::SeqCst);
    INSPECTION_WRITES.store(0, Ordering::SeqCst);
    faultpoint::arm("package.inspect.write", Fault::Callback(count_inspection));
    // Only the shared engine admission is occupied. No server or transport is constructed.
    let failure = f.failed_restore(&backup.name);
    assert_eq!(
        failure.cause,
        MaintenanceError::Busy {
            running_op: "one-way sync".into()
        }
    );
    assert_eq!(f.space.calls.load(Ordering::SeqCst), before);
    assert_eq!(INSPECTION_WRITES.load(Ordering::SeqCst), 0);
    drop(exclusive);
    drop(slot);
    faultpoint::disarm("package.inspect.write");
}

#[test]
fn cancellation_before_rename_preserves_generation_lifecycle_original_data_and_safety() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let backup = f.create();
    f.rename("Original live data");
    let generation = f.generation();
    let gate = Gate::arm("replace.after_check");
    let started = maintenance::start_restore_backup(backup.name).unwrap();
    gate.reached();
    started.job.cancel.cancel().unwrap();
    gate.resume();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Cancelled);
    let RestoreBackupOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("cancel receipt")
    };
    assert_eq!(failure.cause, MaintenanceError::Cancelled);
    assert!(!failure.candidate_active && failure.restored.is_none());
    assert!(f.backup_path(&failure.safety_backup.unwrap()).is_file());
    assert_eq!(f.generation(), generation);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { generation }
    );
    assert_eq!(f.deck_name(), "Original live data");
}

#[test]
fn after_activation_failure_keeps_new_generation_and_owned_restore_facts() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let backup = f.create();
    f.rename("Later state");
    let generation = f.generation();
    faultpoint::arm(
        "restore.before_maintenance_reevaluation",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    let failure = f.failed_restore(&backup.name);
    faultpoint::disarm("restore.before_maintenance_reevaluation");
    assert!(matches!(failure.cause, MaintenanceError::Io { .. }));
    assert!(failure.candidate_active);
    assert_eq!(failure.generation, Some(generation + 1));
    let restored = failure.restored.unwrap();
    assert_eq!(restored.generation, generation + 1);
    assert!(restored.cloud_publication_pending);
    assert_eq!(restored.safety_backup, failure.safety_backup);
    assert!(f.backup_path(&failure.safety_backup.unwrap()).is_file());
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready {
            generation: generation + 1
        }
    );
}

#[test]
fn deadline_changed_then_elapsed_backs_up_once_unchanged_has_no_deadline() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    f.create();
    assert_eq!(f.due(MaintenanceTrigger::Deadline), None);
    let initial = maintenance::list_backups().unwrap().len();
    // Fixture timestamps are explicit clock values, independent of the runner's date.
    for entry in maintenance::list_backups().unwrap() {
        let stamp = std::time::UNIX_EPOCH + Duration::from_millis(NOW as u64);
        fs::File::open(f.backup_path(&entry.name))
            .unwrap()
            .set_times(fs::FileTimes::new().set_modified(stamp))
            .unwrap();
    }
    f.rename("Changed after backup");
    f.advance(31 * 60 * 1000);
    assert_eq!(f.due(MaintenanceTrigger::Deadline), None);
    assert_eq!(maintenance::list_backups().unwrap().len(), initial + 1);
    assert_eq!(f.due(MaintenanceTrigger::Deadline), None);
    assert_eq!(maintenance::list_backups().unwrap().len(), initial + 1);
}

static CHECKS: AtomicUsize = AtomicUsize::new(0);
fn count_check() {
    CHECKS.fetch_add(1, Ordering::SeqCst);
}
#[test]
fn background_processing_checks_once_in_one_day_and_status_keeps_field_failures() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    f.create();
    f.advance(24 * 60 * 60 * 1000);
    CHECKS.store(0, Ordering::SeqCst);
    faultpoint::arm("storage.maintenance.quick_check", Fault::Callback(count_check));
    f.due(MaintenanceTrigger::BackgroundProcessing);
    let first = maintenance::maintenance_status().unwrap();
    f.advance(1000);
    f.due(MaintenanceTrigger::BackgroundProcessing);
    let second = maintenance::maintenance_status().unwrap();
    faultpoint::disarm("storage.maintenance.quick_check");
    assert_eq!(CHECKS.load(Ordering::SeqCst), 1);
    assert_eq!(first.last_quick_check, second.last_quick_check);
    assert!(second.last_quick_check.unwrap().passed);
    assert!(second.failures.is_empty());
    let sidecar = Path::new(&f.location.collection_directory).join("meguri-sidecar.json");
    meguri_storage::sidecar::update_document(&sidecar, |doc| {
        doc["deviceState"]["lastOptimize"] = serde_json::json!({"invalid": 1});
        Ok(())
    })
    .unwrap();
    let status = maintenance::maintenance_status().unwrap();
    assert!(status.last_optimize_ms.is_none());
    assert!(status.last_quick_check.unwrap().passed);
    assert!(
        status
            .failures
            .iter()
            .any(|failure| failure.field == MaintenanceStatusField::LastOptimize)
    );
}

fn query_int(c: &meguri_engine::Collection, sql: &str) -> i64 {
    let rows = c.reader(ReaderRole::Query).unwrap().execute_raw(sql, &[]).unwrap();
    match rows.rows[0][0] {
        SqlValue::Integer(n) => n,
        _ => panic!("integer fixture metadata"),
    }
}
#[test]
fn opened_unclean_real_page_damage_reports_corrupt_and_public_restore_recovers() {
    let _serial = SERIAL.lock().unwrap();
    let mut f = Fixture::new();
    let backup = f.create();
    let c = collection_for_testing().unwrap();
    let root = query_int(&c, "SELECT rootpage FROM sqlite_master WHERE name='notes'");
    let size = query_int(&c, "PRAGMA page_size");
    c.submit_maintenance(catalog::ANALYZE, Duration::from_secs(1), |writer| {
        Ok(writer.execute_raw("PRAGMA wal_checkpoint(TRUNCATE)", &[])?)
    })
    .wait()
    .unwrap();
    lifecycle::close_collection().unwrap();
    meguri_storage::sidecar::Sidecar::update(
        &Path::new(&f.location.collection_directory).join("meguri-sidecar.json"),
        |sidecar, _| sidecar.clean_shutdown = false,
    )
    .unwrap();
    assert!(root > 1);
    let mut file = fs::OpenOptions::new().write(true).open(f.path()).unwrap();
    file.seek(SeekFrom::Start(u64::try_from((root - 1) * size).unwrap()))
        .unwrap();
    file.write_all(&[0]).unwrap();
    file.sync_all().unwrap();
    drop(file);
    f.open();
    let generation = f.generation();
    f.due(MaintenanceTrigger::Opened);
    assert!(matches!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Failed {
            reason: CollectionFailedReason::Corrupt { .. }
        }
    ));
    let status = maintenance::maintenance_status().unwrap();
    assert!(!status.last_quick_check.unwrap().passed);
    assert_eq!(status.remedies, vec![MaintenanceRemedy::Restore]); // Current U132 ordered remedy set.
    f.advance(1000);
    let started = maintenance::start_restore_backup(backup.name).unwrap();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Succeeded);
    let RestoreBackupOutcome::Restored { restored } = started.result.outcome().unwrap() else {
        panic!("recovery restore")
    };
    assert_eq!(restored.generation, generation + 1);
    assert_eq!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready {
            generation: generation + 1
        }
    );
    assert!(restored.cloud_publication_pending);
    assert!(f.backup_path(&restored.safety_backup.unwrap()).is_file());
}

#[test]
fn native_capacity_one_mb_preserves_actual_needed_available_for_create_and_restore() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let backup = f.create();
    f.space.bytes.store(MB, Ordering::SeqCst);
    let started = maintenance::start_create_backup().unwrap();
    assert!(!matches!(finish(&f.rx, started.job.id).0, JobOutcome::Succeeded));
    let CreateBackupOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("space failure")
    };
    let MaintenanceError::InsufficientSpace { needed, available } = failure.cause else {
        panic!("typed capacity")
    };
    let c = collection_for_testing().unwrap();
    let (_, native_size) = c.backup_reader().unwrap().live_snapshot_source().unwrap();
    assert_eq!((needed, available), (native_size * 2 + 4 * MB, MB));
    let failure = f.failed_restore(&backup.name);
    let MaintenanceError::InsufficientSpace { needed, available } = failure.cause else {
        panic!("restore capacity")
    };
    // Compare with real package preflight using the same live size and safety reserve.
    let live_size = fs::metadata(f.path()).unwrap().len()
        + match fs::metadata(f.path().with_file_name("collection.anki2-wal")) {
            Ok(m) => m.len(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
            Err(e) => panic!("{e}"),
        };
    struct OneMB;
    impl meguri_storage::space::SpaceProvider for OneMB {
        fn available_bytes(&self, _: &Path) -> Result<u64, meguri_storage::StorageError> {
            Ok(MB)
        }
    }
    let native = meguri_package::inspect_collection_package(
        &f.backup_path(&backup.name),
        &meguri_package::CollectionPackageInspectContext {
            collection_directory: Path::new(&f.location.collection_directory),
            live_collection_size: live_size,
            reserved_space_bytes: live_size * 2 + 4 * MB,
            clock: &FixedClock::new(NOW, UtcOffset::UTC),
            random: &SeededRandom::new(133),
            space: &OneMB,
            is_cancelled: &|| false,
        },
    )
    .err()
    .expect("real package preflight must refuse");
    let meguri_package::CollectionPackageInspectError::InsufficientSpace {
        needed: native_needed,
        available: native_available,
    } = native
    else {
        panic!("native preflight capacity cause")
    };
    assert_eq!((needed, available), (native_needed, native_available));
    assert!(needed > available && available == MB);
    f.space.failed.store(true, Ordering::SeqCst);
    let started = maintenance::start_create_backup().unwrap();
    assert!(!matches!(finish(&f.rx, started.job.id).0, JobOutcome::Succeeded));
    let CreateBackupOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("capacity read failure")
    };
    assert!(matches!(failure.cause, MaintenanceError::Io { .. }));
    assert!(matches!(
        f.failed_restore(&backup.name).cause,
        MaintenanceError::Io { .. }
    ));
}

#[test]
fn queued_cancellation_still_publishes_owned_failure_when_worker_body_never_runs() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let workers = meguri_engine::EngineConfig::default().job_workers;
    let release = Arc::new(std::sync::Barrier::new(workers + 1));
    let (entered, waiting) = mpsc::channel();
    let mut blockers = Vec::new();
    for _ in 0..workers {
        let entered = entered.clone();
        let release = release.clone();
        blockers.push(f.engine.jobs().start(
            "hold maintenance fixture worker",
            f.engine.generation(),
            None,
            move |_| {
                entered.send(()).unwrap();
                release.wait();
                Ok(())
            },
        ));
    }
    for _ in 0..workers {
        waiting.recv_timeout(Duration::from_secs(20)).unwrap();
    }
    let started = maintenance::start_create_backup().unwrap();
    started.job.cancel.cancel().unwrap();
    release.wait();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Cancelled);
    let CreateBackupOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("closure-drop cancellation must not leave Pending after JobFinished")
    };
    assert_eq!(failure.cause, MaintenanceError::Cancelled);
    assert!(!failure.candidate_active);
    // Core job bookkeeping publishes terminal before removing running entries;
    // fixture close performs its normal registry drain, including the blockers.
    drop(blockers);
}
