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
    let gate = Gate::arm("storage.backup.before_rename");
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
    // The backend accepts the full u32 domain, including zero retention.
    let extreme = BackupLimits {
        daily: 0,
        weekly: u32::MAX,
        monthly: 0,
        minimum_interval_mins: u32::MAX,
    };
    maintenance::set_backup_limits(extreme.clone()).unwrap();
    assert_eq!(maintenance::backup_limits().unwrap(), extreme);
}

#[test]
fn restore_replaces_generation_retains_current_safety_copy_and_publication_choice() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let original_name = f.deck_name();
    let backup = f.create();
    let generation = f.generation();
    let original_scope = meguri_ffi::filtered_study::filtered_study_deck_scope(1, generation).unwrap();
    f.rename("Current collection must survive in safety copy");
    let started = maintenance::start_restore_backup(backup.name.clone()).unwrap();
    let (terminal, events) = finish(&f.rx, started.job.id);
    assert_eq!(terminal, JobOutcome::Succeeded);
    let RestoreBackupOutcome::Restored { restored } = started.result.outcome().unwrap() else {
        panic!("restore receipt")
    };
    assert_eq!(restored.generation, generation + 1);
    use meguri_ffi::filtered_study::{self, FilteredStudyCommand, CustomStudyMode, CustomStudyState, CustomStudyRequest};
    let stale = meguri_ffi::CoreError::StaleGeneration { planned: generation, current: generation + 1 };
    assert_eq!(filtered_study::filtered_study_deck_scope(1, generation).unwrap_err(), stale);
    assert_eq!(filtered_study::start_filtered_study(FilteredStudyCommand::Custom { deck: 1,
        request: CustomStudyRequest { mode: CustomStudyMode::IncreaseNew, amount: 5,
            state: CustomStudyState::New, include_tags: vec![], exclude_tags: vec![] } }, generation).unwrap_err(), stale);
    assert_eq!(filtered_study::filtered_study_deck_scope(1, generation + 1).unwrap(), original_scope);
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
    let fixture = Fixture::new();
    let saved = fixture.create();
    fixture.space.bytes.store(MB, Ordering::SeqCst);

    let attempt = maintenance::start_create_backup().unwrap();
    let terminal = finish(&fixture.rx, attempt.job.id).0;
    assert_ne!(terminal, JobOutcome::Succeeded);
    let CreateBackupOutcome::Failed { failure: creation } = attempt.result.outcome().unwrap() else {
        panic!("backup creation did not retain its refusal")
    };
    let collection = collection_for_testing().unwrap();
    let (_, snapshot_bytes) = collection.backup_reader().unwrap().live_snapshot_source().unwrap();
    assert_eq!(
        creation.cause,
        MaintenanceError::InsufficientSpace {
            needed: snapshot_bytes * 2 + 4 * MB,
            available: MB,
        }
    );

    let refused = fixture.failed_restore(&saved.name);
    let MaintenanceError::InsufficientSpace { needed, available } = refused.cause else {
        panic!("restore lost the capacity error category")
    };
    let wal_bytes = match fs::metadata(fixture.path().with_file_name("collection.anki2-wal")) {
        Ok(metadata) => metadata.len(),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => 0,
        Err(error) => panic!("could not measure the fixture WAL: {error}"),
    };
    let live_bytes = fs::metadata(fixture.path()).unwrap().len() + wal_bytes;
    struct LimitedVolume;
    impl meguri_storage::space::SpaceProvider for LimitedVolume {
        fn available_bytes(&self, _: &Path) -> Result<u64, meguri_storage::StorageError> {
            Ok(MB)
        }
    }
    // The public refusal must carry the package inspector's complete preflight,
    // including the reservation for preserving the current collection.
    let inspection = meguri_package::CollectionPackageInspectContext {
        collection_directory: Path::new(&fixture.location.collection_directory),
        live_collection_size: live_bytes,
        reserved_space_bytes: live_bytes * 2 + 4 * MB,
        clock: &FixedClock::new(NOW, UtcOffset::UTC),
        random: &SeededRandom::new(133),
        space: &LimitedVolume,
        is_cancelled: &|| false,
    };
    let inspection_error = meguri_package::inspect_collection_package(&fixture.backup_path(&saved.name), &inspection)
        .expect_err("one MiB cannot cover restore inspection and recovery storage");
    match inspection_error {
        meguri_package::CollectionPackageInspectError::InsufficientSpace {
            needed: package_needed,
            available: package_available,
        } => assert_eq!((needed, available), (package_needed, package_available)),
        other => panic!("package inspection returned a different refusal: {other:?}"),
    }
    assert_eq!(available, MB);
    assert!(needed > available);

    fixture.space.failed.store(true, Ordering::SeqCst);
    let unreadable = maintenance::start_create_backup().unwrap();
    assert_ne!(finish(&fixture.rx, unreadable.job.id).0, JobOutcome::Succeeded);
    let CreateBackupOutcome::Failed { failure: creation } = unreadable.result.outcome().unwrap() else {
        panic!("unreadable capacity did not produce an owned backup failure")
    };
    assert!(matches!(creation.cause, MaintenanceError::Io { .. }));
    let restore = fixture.failed_restore(&saved.name);
    assert!(matches!(restore.cause, MaintenanceError::Io { .. }));
}

struct PoolHold(Vec<mpsc::Sender<()>>);
impl PoolHold {
    fn acquire(engine: &Engine) -> Self {
        let mut hold = Self(Vec::new());
        let (arrived, arrivals) = mpsc::channel();
        let count = meguri_engine::EngineConfig::default().job_workers;
        for index in 0..count {
            let (resume, stopped) = mpsc::channel();
            hold.0.push(resume);
            let arrived = arrived.clone();
            engine
                .jobs()
                .start("pool reservation", engine.generation(), None, move |_| {
                    arrived.send(index).unwrap();
                    stopped
                        .recv_timeout(Duration::from_secs(20))
                        .expect("pool reservation released");
                    Ok(())
                });
        }
        let mut confirmed = std::collections::BTreeSet::new();
        for _ in 0..count {
            assert!(confirmed.insert(arrivals.recv_timeout(Duration::from_secs(20)).unwrap()));
        }
        assert_eq!(confirmed.len(), count);
        hold
    }
}
impl Drop for PoolHold {
    fn drop(&mut self) {
        for resume in self.0.drain(..) {
            let _ = resume.send(());
        }
    }
}
#[test]
fn cancellation_owns_results_for_jobs_removed_before_execution() {
    let _serial = SERIAL.lock().unwrap();
    let fixture = Fixture::new();
    let held = PoolHold::acquire(&fixture.engine);
    let backup_task = maintenance::start_create_backup().unwrap();
    let replacement_task = maintenance::start_restore_backup("backup-2030-01-15-10.00.00.colpkg".into()).unwrap();
    let policy_task = maintenance::start_run_due_maintenance(MaintenanceTrigger::Background).unwrap();
    assert_eq!(backup_task.result.outcome().unwrap(), CreateBackupOutcome::Pending);
    assert_eq!(
        replacement_task.result.outcome().unwrap(),
        RestoreBackupOutcome::Pending
    );
    assert_eq!(policy_task.result.outcome().unwrap(), DueMaintenanceOutcome::Pending);
    for control in [&backup_task.job, &replacement_task.job, &policy_task.job] {
        control.cancel.cancel().unwrap();
    }
    drop(held);
    let selected = std::collections::BTreeSet::from([backup_task.job.id, replacement_task.job.id, policy_task.job.id]);
    let mut outstanding = selected.clone();
    let mut seen = std::collections::BTreeSet::new();
    while !outstanding.is_empty() {
        let delivered = fixture
            .rx
            .recv_timeout(Duration::from_secs(20))
            .expect("queued cancellation terminal");
        if let CoreEventKind::JobFinished { id, outcome } = delivered.kind
            && selected.contains(&id)
        {
            assert!(seen.insert(id), "selected job delivered more than one terminal");
            assert_eq!(outcome, JobOutcome::Cancelled);
            assert!(outstanding.remove(&id));
        }
    }
    assert_eq!(seen, selected);
    let CreateBackupOutcome::Failed {
        failure: backup_failure,
    } = backup_task.result.outcome().unwrap()
    else {
        panic!("unstarted backup must own cancellation")
    };
    let RestoreBackupOutcome::Failed {
        failure: restore_failure,
    } = replacement_task.result.outcome().unwrap()
    else {
        panic!("unstarted restore must own cancellation")
    };
    let DueMaintenanceOutcome::Failed { failure: due_failure } = policy_task.result.outcome().unwrap() else {
        panic!("unstarted due work must own cancellation")
    };
    for failure in [backup_failure, restore_failure, due_failure] {
        assert_eq!(failure.cause, MaintenanceError::Cancelled);
        assert!(!failure.candidate_active);
        assert_eq!(failure.generation, None);
        assert_eq!(
            failure.backup_publication,
            meguri_ffi::collection_entry::ImportBackupPublication::Unpublished
        );
    }
    assert!(maintenance::list_backups().unwrap().is_empty());
    fixture.create();
}

#[test]
fn exported_due_backup_cancel_before_verification_leaves_no_publication() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    maintenance::set_backup_limits(BackupLimits {
        daily: 12,
        weekly: 10,
        monthly: 9,
        minimum_interval_mins: 0,
    })
    .unwrap();
    let gate = Gate::arm("storage.atomic.after_sync");
    let started = maintenance::start_run_due_maintenance(MaintenanceTrigger::Deadline).unwrap();
    gate.reached(); // AtomicFile has synced; real read-back verification still polls cancellation.
    assert_eq!(started.result.outcome().unwrap(), DueMaintenanceOutcome::Pending);
    assert!(maintenance::list_backups().unwrap().is_empty());
    assert!(maintenance::maintenance_status().unwrap().last_backup.is_none());
    started.job.cancel.cancel().unwrap();
    gate.resume();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Cancelled);
    let DueMaintenanceOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("owned due cancellation")
    };
    assert_eq!(failure.cause, MaintenanceError::Cancelled);
    assert_eq!(
        failure.backup_publication,
        meguri_ffi::collection_entry::ImportBackupPublication::Unpublished
    );
    assert!(!failure.candidate_active);
    assert!(maintenance::list_backups().unwrap().is_empty());
    assert!(maintenance::maintenance_status().unwrap().last_backup.is_none());
    drop(gate);
    f.create(); // Same runtime and collection remain usable.
}

#[test]
fn exported_due_backup_past_last_cancel_check_keeps_durable_completion() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let gate = Gate::arm("storage.backup.before_rename");
    let started = maintenance::start_run_due_maintenance(MaintenanceTrigger::Deadline).unwrap();
    gate.reached(); // Verification has finished; AtomicFile has no later cancel check.
    assert_eq!(started.result.outcome().unwrap(), DueMaintenanceOutcome::Pending);
    assert!(maintenance::list_backups().unwrap().is_empty());
    started.job.cancel.cancel().unwrap();
    gate.resume();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Succeeded);
    assert!(matches!(
        started.result.outcome().unwrap(),
        DueMaintenanceOutcome::Completed { .. }
    ));
    let listing = maintenance::list_backups().unwrap();
    assert_eq!(listing.len(), 1);
    assert_eq!(
        maintenance::maintenance_status().unwrap().last_backup.unwrap().name,
        listing[0].name
    );
    drop(gate);
    f.advance(1000);
    f.create();
}

#[test]
fn exported_due_health_cancel_after_entry_keeps_check_and_backup_unpublished() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let gate = Gate::arm("storage.maintenance.quick_check");
    let started = maintenance::start_run_due_maintenance(MaintenanceTrigger::Opened).unwrap();
    gate.reached();
    assert_eq!(started.result.outcome().unwrap(), DueMaintenanceOutcome::Pending);
    assert!(maintenance::maintenance_status().unwrap().last_quick_check.is_none());
    assert!(maintenance::list_backups().unwrap().is_empty());
    started.job.cancel.cancel().unwrap();
    gate.resume();
    assert_eq!(finish(&f.rx, started.job.id).0, JobOutcome::Cancelled);
    let DueMaintenanceOutcome::Failed { failure } = started.result.outcome().unwrap() else {
        panic!("owned check cancellation")
    };
    assert_eq!(failure.cause, MaintenanceError::Cancelled);
    assert!(!failure.candidate_active);
    assert!(maintenance::maintenance_status().unwrap().last_quick_check.is_none());
    assert!(maintenance::list_backups().unwrap().is_empty());
    assert!(matches!(
        lifecycle::collection_lifecycle().unwrap(),
        CollectionLifecycle::Ready { .. }
    ));
    drop(gate);
    f.due(MaintenanceTrigger::Opened);
    assert!(
        maintenance::maintenance_status()
            .unwrap()
            .last_quick_check
            .unwrap()
            .passed
    );
    f.advance(1000);
    f.create();
}

#[test]
fn background_and_quit_use_real_trigger_jobs_and_terminate_without_self_drain() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    maintenance::set_backup_limits(BackupLimits {
        daily: 12,
        weekly: 10,
        monthly: 9,
        minimum_interval_mins: 0,
    })
    .unwrap();
    f.create();
    let before = maintenance::list_backups().unwrap().len();
    for (trigger, name) in [
        (MaintenanceTrigger::Background, "Background change"),
        (MaintenanceTrigger::Quit, "Quit change"),
    ] {
        f.rename(name);
        assert_eq!(f.due(trigger), None);
    }
    assert_eq!(maintenance::list_backups().unwrap().len(), before + 2);
    assert_eq!(f.due(MaintenanceTrigger::Background), None);
    assert_eq!(f.due(MaintenanceTrigger::Quit), None);
    assert_eq!(maintenance::list_backups().unwrap().len(), before + 2);
}

#[test]
fn status_maps_every_field_counts_milliseconds_large_bytes_and_independent_errors() {
    use meguri_engine::{device_state as D, maintenance::status as S};
    let status = MaintenanceStatus::from(S::MaintenanceStatus {
        last_backup: Ok(Some(D::LastBackup {
            name: "record.colpkg".into(),
            instant: 1_894_701_600_123,
            snapshot_mod: -7001,
        })),
        last_check_database: Ok(Some(D::LastCheckDatabase {
            instant: 1_894_701_600_456,
            result: D::CheckDatabaseResult::Problems(D::CheckDatabaseProblems {
                card_properties: 1,
                new_positions_too_high: 2,
                card_last_review_time: 3,
                cards_without_note: 4,
                decks_missing: 5,
                field_counts: 6,
                duplicate_card_ords: 7,
                cards_without_template: 8,
                revlog_properties: 9,
                notes_invalid_utf8: 10,
                future_ids: 11,
                notetypes_recovered: u64::MAX,
            }),
        })),
        last_optimize: Ok(Some(meguri_model::time::EpochMillis(1_894_701_600_789))),
        last_quick_check: Ok(Some(D::LastQuickCheck {
            instant: 1_894_701_600_987,
            passed: false,
        })),
        collection_bytes: Ok(5_000_000_001),
        media_bytes: Ok(6_000_000_002),
        trash_bytes: Ok(u64::MAX),
        remedies: Ok(vec![S::Remedy::Restore]),
    });
    assert!(status.failures.is_empty());
    assert_eq!(
        status.last_backup.unwrap(),
        LastBackup {
            name: "record.colpkg".into(),
            instant_ms: 1_894_701_600_123,
            snapshot_modified_ms: -7001
        }
    );
    let check = status.last_check_database.unwrap();
    assert_eq!(check.instant_ms, 1_894_701_600_456);
    assert_eq!(
        check.result,
        CheckDatabaseResult::Problems {
            problems: CheckDatabaseProblems {
                card_properties: 1,
                new_positions_too_high: 2,
                card_last_review_time: 3,
                cards_without_note: 4,
                decks_missing: 5,
                field_counts: 6,
                duplicate_card_ords: 7,
                cards_without_template: 8,
                revlog_properties: 9,
                notes_invalid_utf8: 10,
                future_ids: 11,
                notetypes_recovered: u64::MAX,
            }
        }
    );
    assert_eq!(status.last_optimize_ms, Some(1_894_701_600_789));
    assert_eq!(
        status.last_quick_check,
        Some(LastQuickCheck {
            instant_ms: 1_894_701_600_987,
            passed: false
        })
    );
    assert_eq!(
        (status.collection_bytes, status.media_bytes, status.trash_bytes),
        (Some(5_000_000_001), Some(6_000_000_002), Some(u64::MAX))
    );
    assert_eq!(status.remedies, vec![MaintenanceRemedy::Restore]);
    let status = MaintenanceStatus::from(S::MaintenanceStatus {
        last_backup: Err(meguri_storage::StorageError::Io {
            detail: "backup record".into(),
        }),
        last_check_database: Err(meguri_storage::StorageError::Io {
            detail: "check record".into(),
        }),
        last_optimize: Err(meguri_storage::StorageError::Io {
            detail: "optimize record".into(),
        }),
        last_quick_check: Err(meguri_storage::StorageError::Io {
            detail: "quick record".into(),
        }),
        collection_bytes: Err(meguri_storage::StorageError::DestinationExists),
        media_bytes: Err(meguri_engine::EngineError::CollectionCorrupt),
        trash_bytes: Err(meguri_engine::EngineError::Busy {
            running_op: "trash read".into(),
        }),
        remedies: Err(meguri_engine::EngineError::CollectionUnavailable {
            reason: "listing failed".into(),
        }),
    });
    assert!(
        status.last_backup.is_none()
            && status.last_check_database.is_none()
            && status.last_optimize_ms.is_none()
            && status.last_quick_check.is_none()
    );
    assert!(
        status.collection_bytes.is_none()
            && status.media_bytes.is_none()
            && status.trash_bytes.is_none()
            && status.remedies.is_empty()
    );
    use MaintenanceStatusField as F;
    assert_eq!(
        status.failures.iter().map(|f| f.field).collect::<Vec<_>>(),
        vec![
            F::LastBackup,
            F::LastCheckDatabase,
            F::LastOptimize,
            F::LastQuickCheck,
            F::CollectionBytes,
            F::MediaBytes,
            F::TrashBytes,
            F::Remedies
        ]
    );
    assert!(matches!(status.failures[0].cause, MaintenanceError::Io { .. }));
    assert!(matches!(status.failures[1].cause, MaintenanceError::Io { .. }));
    assert!(matches!(status.failures[2].cause, MaintenanceError::Io { .. }));
    assert!(matches!(status.failures[3].cause, MaintenanceError::Io { .. }));
    assert_eq!(status.failures[4].cause, MaintenanceError::DestinationExists);
    assert_eq!(status.failures[5].cause, MaintenanceError::CollectionCorrupt);
    assert_eq!(
        status.failures[6].cause,
        MaintenanceError::Busy {
            running_op: "trash read".into()
        }
    );
    assert_eq!(status.failures[7].cause, MaintenanceError::CollectionUnavailable);
}

#[test]
fn persisted_full_status_records_are_read_without_turning_malformed_fields_into_absence() {
    use meguri_engine::device_state::{CheckDatabaseResult as Result, DeviceState, LastCheckDatabase as Check};
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new();
    let receipt = f.create();
    let records = DeviceState::for_collection(&f.path());
    records
        .set_last_check_database(&Check {
            instant: NOW + 123,
            result: Result::Corrupt,
        })
        .unwrap();
    records
        .set_last_optimize(meguri_model::time::EpochMillis(NOW + 456))
        .unwrap();
    let status = maintenance::maintenance_status().unwrap();
    assert!(status.failures.is_empty());
    assert_eq!(status.last_backup.unwrap().name, receipt.name);
    assert_eq!(
        status.last_check_database.unwrap(),
        LastCheckDatabase {
            instant_ms: NOW + 123,
            result: CheckDatabaseResult::Corrupt
        }
    );
    assert_eq!(status.last_optimize_ms, Some(NOW + 456));
    assert!(status.collection_bytes.unwrap() > 0);
    assert_eq!((status.media_bytes, status.trash_bytes), (Some(0), Some(0)));
    meguri_storage::sidecar::update_document(
        &Path::new(&f.location.collection_directory).join("meguri-sidecar.json"),
        |doc| {
            doc["deviceState"]["lastBackup"] = serde_json::json!({"invalid": 1});
            doc["deviceState"]["lastCheckDatabase"] =
                serde_json::json!({"instant": NOW, "result": {"card_properties": "invalid"}});
            Ok(())
        },
    )
    .unwrap();
    let status = maintenance::maintenance_status().unwrap();
    assert!(status.last_backup.is_none() && status.last_check_database.is_none());
    assert!(
        status
            .failures
            .iter()
            .any(|f| f.field == MaintenanceStatusField::LastBackup)
    );
    assert!(
        status
            .failures
            .iter()
            .any(|f| f.field == MaintenanceStatusField::LastCheckDatabase)
    );
    assert_eq!(status.last_optimize_ms, Some(NOW + 456));
}

#[test]
fn replacement_failure_uses_only_owned_activation_authority_including_unknown_generation() {
    use meguri_engine::replace::ReplaceError;
    use meguri_package::restore::RestoreError;
    for generation in [None, Some(meguri_engine::Generation(73))] {
        let failure = MaintenanceFailure::from(RestoreError::Replacement {
            error: ReplaceError::AfterActivation {
                generation,
                source: Box::new(ReplaceError::Lifecycle(meguri_engine::EngineError::Cancelled)),
            },
            safety_backup: Some("safety.colpkg".into()),
        });
        assert!(failure.candidate_active);
        assert_eq!(failure.generation, generation.map(|g| g.0));
        assert_eq!(failure.cause, MaintenanceError::Cancelled);
        assert_eq!(failure.safety_backup.as_deref(), Some("safety.colpkg"));
        assert!(
            failure.restored.is_none(),
            "replacement error does not own a completed restore"
        );
    }
}
