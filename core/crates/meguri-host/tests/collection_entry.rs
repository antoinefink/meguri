//! RF2 uses real providers, jobs, review and storage observations. No package
//! conformance adapter is activated by these focused orchestration checks.
use meguri_engine::{
    CancelToken, Engine, EngineError, EventKind, JobContext, JobKind, OpSource, OpSpec, Progress, ProgressListener,
    ProgressPhase, ProgressSink,
    device_state::{DeviceState, PublicationDecision},
    ports::BackupService,
    replace::ReplaceError,
    session::{AnswerOutcome, StartResult},
};
use meguri_host::{CoreHost, HostConfig, ProcessCore};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_package::{
    CollectionPackageInspectContext, CollectionPackageWriteContext, Finalisation,
    entry::{self, EntryError, EntryInspection},
    inspect_collection_package,
};
use meguri_storage::{
    ConfigValue, ReadRows, ReaderRole, StorageError, Table,
    atomic_file::Purpose,
    space::{SpaceProvider, StatvfsSpace},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};
const NOW: i64 = 1_894_701_600_000;
static LANE: Mutex<()> = Mutex::new(());
fn config(root: &Path) -> HostConfig {
    let dir = root.join("collection");
    fs::create_dir_all(&dir).unwrap();
    HostConfig::new(
        dir,
        root.join("backups"),
        root.join("widget"),
        Arc::new(FixedClock::new(NOW, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(42)),
        Arc::new(StatvfsSpace),
    )
}
fn seed(config: &HostConfig) {
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
}
fn add_card(host: &CoreHost) {
    let c = host.collection().unwrap();
    c.run(OpSpec::named("entry fixture policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("Entry fixture".into()),
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &c,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["entry front".into(), "entry back".into()],
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
}
fn answer(host: &CoreHost) {
    let StartResult::Started { session_id, head, .. } = host.review().start(DeckId(1), None).unwrap() else {
        panic!("review must start")
    };
    let card = &head.items[0];
    assert!(matches!(
        host.review()
            .answer(
                session_id,
                card.card_id,
                meguri_sched::answer::Rating::Easy,
                card.state_token,
                EpochMillis(NOW),
                123
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    host.review().end(session_id).unwrap();
}
fn package(root: &Path) -> PathBuf {
    let options = config(&root.join("source"));
    seed(&options);
    let host = CoreHost::open(options.clone()).unwrap();
    add_card(&host);
    let destination = root.join("input.colpkg");
    let c = host.collection().unwrap();
    let lease = c.reader(ReaderRole::Maintenance).unwrap();
    meguri_package::write_collection_package_from_live(
        &lease,
        &destination,
        &CollectionPackageWriteContext {
            collection_directory: &options.collection_directory,
            purpose: Purpose::Export,
            finalisation: Finalisation::ExclusiveCreate,
            clock: options.clock.as_ref(),
            random: options.random.as_ref(),
            space: options.disk_space_provider.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    drop(lease);
    host.close().unwrap();
    destination
}
fn inspect(process: &ProcessCore, source: &Path, options: &HostConfig) -> EntryInspection {
    entry::inspect_entry(
        process.engine(),
        source,
        &options.entry_inputs(),
        CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap()
}
fn job<T: Send + 'static>(engine: &Engine, work: impl FnOnce(&JobContext) -> T + Send + 'static) -> T {
    let events = engine.stream().subscribe();
    let (tx, rx) = mpsc::channel();
    let handle = engine
        .start_job(JobKind::ColpkgImport, move |ctx| {
            tx.send(work(ctx)).ok();
            Ok(())
        })
        .unwrap();
    let result = rx.recv_timeout(Duration::from_secs(20)).unwrap();
    loop {
        let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == handle.id) {
            break;
        }
    }
    result
}
fn activate(
    process: &ProcessCore,
    inspection: EntryInspection,
    host: Arc<CoreHost>,
) -> Result<entry::Activation, EntryError> {
    job(process.engine(), move |ctx| {
        inspection.activate_ready(ctx.slot().unwrap(), host.backup().as_ref(), &ctx.cancel, &ctx.progress)
    })
}
fn count(host: &CoreHost, table: Table) -> usize {
    host.collection()
        .unwrap()
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(table, None, 100)
        .unwrap()
        .len()
}
#[test]
fn no_file_import_composes_real_host_reviews_saves_and_restarts_without_touching_source() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let bytes = fs::read(&source).unwrap();
    let options = config(&root.path().join("target"));
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let candidate = inspect(&process, &source, &options);
    assert!(!candidate.facts().replacing_existing);
    assert_eq!(candidate.facts().source.notes, 1);
    let importer = process.clone();
    let inputs = options.clone();
    let completed = job(process.engine(), move |ctx| {
        importer.import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
    })
    .unwrap();
    assert!(completed.outcome.unwrap().backup.is_none());
    assert!(std::ptr::eq(process.review(), completed.host.review()));
    answer(&completed.host);
    assert_eq!(count(&completed.host, Table::Revlog), 1);
    let generation = completed.host.collection().unwrap().generation();
    let publication = DeviceState::for_collection(&options.collection_directory.join("collection.anki2"))
        .publication()
        .unwrap()
        .unwrap();
    assert_eq!(publication.decision, PublicationDecision::Pending);
    completed.host.close().unwrap();
    let reopened = process.open(options).unwrap();
    assert!(reopened.collection().unwrap().generation() > generation);
    assert_eq!(count(&reopened, Table::Revlog), 1);
    assert_eq!(fs::read(source).unwrap(), bytes);
    reopened.close().unwrap();
}
#[test]
fn ready_import_backup_contains_committed_old_answer_and_source_is_unchanged() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let bytes = fs::read(&source).unwrap();
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    add_card(&host);
    answer(&host);
    let old = host.collection().unwrap();
    let candidate = inspect(&process, &source, &options);
    assert!(candidate.facts().replacing_existing);
    let activation = activate(&process, candidate, host.clone()).unwrap();
    assert!(activation.generation > old.generation());
    assert!(!old.is_current());
    assert_eq!(count(&host, Table::Revlog), 0);
    let receipt = activation.backup.unwrap();
    let backup = inspect_collection_package(
        &options.backups_directory.join(receipt.name),
        &CollectionPackageInspectContext {
            collection_directory: &options.collection_directory,
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: options.clock.as_ref(),
            random: options.random.as_ref(),
            space: options.disk_space_provider.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    assert_eq!(backup.facts().revlog, 1);
    assert_eq!(fs::read(source).unwrap(), bytes);
    drop(backup);
    host.close().unwrap();
}
#[test]
fn confirmations_expire_on_committed_work_replacement_or_inspection_cancellation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    let stale = inspect(&process, &source, &options);
    meguri_engine::decks::add(&host.collection().unwrap(), "committed work".into()).unwrap();
    assert_eq!(
        activate(&process, stale, host.clone()),
        Err(EntryError::StaleConfirmation)
    );
    let stale_generation = inspect(&process, &source, &options);
    let current = inspect(&process, &source, &options);
    activate(&process, current, host.clone()).unwrap();
    assert_eq!(
        activate(&process, stale_generation, host.clone()),
        Err(EntryError::StaleConfirmation)
    );
    let cancel = CancelToken::new();
    let cancelled = entry::inspect_entry(
        process.engine(),
        &source,
        &options.entry_inputs(),
        cancel.clone(),
        &ProgressSink::new(),
    )
    .unwrap();
    cancel.cancel();
    let generation = host.collection().unwrap().generation();
    assert_eq!(activate(&process, cancelled, host.clone()), Err(EntryError::Cancelled));
    assert_eq!(host.collection().unwrap().generation(), generation);
    host.close().unwrap();
}
#[test]
fn event_only_progress_does_not_expire_confirmation_and_long_slot_is_real() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    let candidate = inspect(&process, &source, &options);
    let c = host.collection().unwrap();
    let seq = c.last_seq();
    process
        .engine()
        .stream()
        .publish(c.generation(), None, EventKind::Available);
    assert_eq!(c.last_seq(), seq);
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let held = process
        .engine()
        .start_job(JobKind::ColpkgImport, move |_| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            Ok(())
        })
        .unwrap();
    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(matches!(
        process.engine().start_job(JobKind::ColpkgImport, |_| Ok(())),
        Err(EngineError::Busy { .. })
    ));
    let events = process.engine().stream().subscribe();
    release_tx.send(()).unwrap();
    loop {
        if matches!(events.recv_timeout(Duration::from_secs(5)).unwrap().kind, EventKind::JobFinished { id, .. } if id == held.id)
        {
            break;
        }
    }
    activate(&process, candidate, host.clone()).unwrap();
    host.close().unwrap();
}
struct StartOnActivation {
    host: Arc<CoreHost>,
}
impl ProgressListener for StartOnActivation {
    fn on_progress(&self, p: &Progress) {
        if p.phase == ProgressPhase::Named("activation") {
            assert!(matches!(
                self.host.review().start(DeckId(1), None).unwrap(),
                StartResult::Started { .. }
            ));
        }
    }
}
#[test]
fn review_session_racing_exclusive_entry_refuses_before_backup_or_swap() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    add_card(&host);
    let candidate = inspect(&process, &source, &options);
    let h = host.clone();
    let generation = host.collection().unwrap().generation();
    let outcome = job(process.engine(), move |ctx| {
        ctx.progress
            .set_listener(Arc::new(StartOnActivation { host: h.clone() }));
        candidate.activate_ready(ctx.slot().unwrap(), h.backup().as_ref(), &ctx.cancel, &ctx.progress)
    });
    assert!(matches!(outcome, Err(EntryError::Engine(EngineError::SessionActive))));
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(!options.backups_directory.exists());
    host.review().end(host.review().active_session().unwrap()).unwrap();
    host.close().unwrap();
}
struct FailBackup;
impl BackupService for FailBackup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        Err(EngineError::Unsupported {
            feature: "injected backup failure".into(),
        })
    }
}
#[test]
fn backup_failure_refuses_without_activation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process
        .open(options.clone().with_backup_for_testing(Arc::new(FailBackup)))
        .unwrap();
    let candidate = inspect(&process, &source, &options);
    let c = host.collection().unwrap();
    let generation = c.generation();
    let backup = host.backup();
    let result = job(process.engine(), move |ctx| {
        candidate.activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
    });
    assert!(matches!(
        result,
        Err(EntryError::Engine(EngineError::CollectionUnavailable { .. }))
    ));
    assert_eq!(host.collection().unwrap().generation(), generation);
    meguri_engine::decks::add(&c, "still live".into()).unwrap();
    host.close().unwrap();
}
struct LowSpace;
impl SpaceProvider for LowSpace {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        Ok(0)
    }
}
#[test]
fn low_space_invalid_source_and_unopened_or_symlink_destination_never_activate_or_leave_staging() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let mut low = options.entry_inputs();
    low.space = Arc::new(LowSpace);
    assert!(matches!(
        entry::inspect_entry(
            process.engine(),
            &source,
            &low,
            CancelToken::new(),
            &ProgressSink::new()
        ),
        Err(EntryError::Package(
            meguri_package::CollectionPackageInspectError::InsufficientSpace { .. }
        ))
    ));
    assert_eq!(fs::read_dir(&options.collection_directory).unwrap().count(), 0);
    let invalid = root.path().join("invalid.colpkg");
    fs::write(&invalid, b"not a ZIP").unwrap();
    assert!(
        entry::inspect_entry(
            process.engine(),
            &invalid,
            &options.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new()
        )
        .is_err()
    );
    assert_eq!(fs::read_dir(&options.collection_directory).unwrap().count(), 0);
    let live = options.collection_directory.join("collection.anki2");
    fs::write(&live, b"damaged but exists").unwrap();
    assert!(matches!(
        entry::inspect_entry(
            process.engine(),
            &source,
            &options.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new()
        ),
        Err(EntryError::Engine(EngineError::Unsupported { .. }))
    ));
    assert_eq!(fs::read(&live).unwrap(), b"damaged but exists");
    fs::remove_file(&live).unwrap();
    std::os::unix::fs::symlink(root.path().join("absent"), &live).unwrap();
    assert!(
        entry::inspect_entry(
            process.engine(),
            &source,
            &options.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new()
        )
        .is_err()
    );
    assert!(fs::symlink_metadata(live).unwrap().file_type().is_symlink());
    assert_eq!(process.engine().generation().0, 0);
}
#[test]
fn initial_postactivation_error_returns_live_host_and_publication_fence() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let candidate = inspect(&process, &source, &options);
    let importer = process.clone();
    let inputs = options.clone();
    meguri_model::faultpoint::arm(
        "replace.after_dir_sync",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let completed = job(process.engine(), move |ctx| {
        importer.import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
    })
    .unwrap();
    meguri_model::faultpoint::disarm("replace.after_dir_sync");
    assert!(matches!(
        completed.outcome,
        Err(EntryError::Replacement {
            error: ReplaceError::AfterActivation {
                generation: Some(_),
                ..
            },
            ..
        })
    ));
    assert!(completed.host.collection().is_ok());
    assert!(process.open(options.clone()).is_err());
    assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_err());
    assert_eq!(
        DeviceState::for_collection(&options.collection_directory.join("collection.anki2"))
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    answer(&completed.host);
    completed.host.close().unwrap();
}

struct CommitOnActivation {
    host: Arc<CoreHost>,
}
impl ProgressListener for CommitOnActivation {
    fn on_progress(&self, p: &Progress) {
        if p.phase == ProgressPhase::Named("activation") {
            // Admitted just after the first confirmation check. Exclusive entry
            // must drain this write and compare the committed seq again.
            let c = self.host.collection().unwrap();
            drop(
                c.submit(OpSpec::named("entry drain race", OpSource::UserUndoable), |ctx| {
                    ctx.scope().set_config("entryDrainRace", &ConfigValue::Bool(true))?;
                    Ok(())
                }),
            );
        }
    }
}
#[test]
fn work_admitted_between_confirmation_check_and_exclusive_drain_expires_preview() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    let candidate = inspect(&process, &source, &options);
    let h = host.clone();
    let generation = host.collection().unwrap().generation();
    let outcome = job(process.engine(), move |ctx| {
        ctx.progress
            .set_listener(Arc::new(CommitOnActivation { host: h.clone() }));
        candidate.activate_ready(ctx.slot().unwrap(), h.backup().as_ref(), &ctx.cancel, &ctx.progress)
    });
    assert_eq!(outcome, Err(EntryError::StaleConfirmation));
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(!options.backups_directory.exists());
    host.close().unwrap();
}

struct CancelAfterBackup {
    real: Arc<dyn BackupService>,
}
impl BackupService for CancelAfterBackup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.real.before_deck_delete()
    }
    fn before_colpkg_import(
        &self,
        c: &meguri_engine::Collection,
        cancel: &CancelToken,
    ) -> Result<meguri_engine::ports::BackupReceipt, EngineError> {
        let receipt = self.real.before_colpkg_import(c, cancel)?;
        cancel.cancel();
        Ok(receipt)
    }
}
#[test]
fn cancellation_after_real_backup_retains_receipt_and_live_generation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    let candidate = inspect(&process, &source, &options);
    let generation = host.collection().unwrap().generation();
    let backup = CancelAfterBackup { real: host.backup() };
    let outcome = job(process.engine(), move |ctx| {
        candidate.activate_ready(ctx.slot().unwrap(), &backup, &ctx.cancel, &ctx.progress)
    });
    let Err(EntryError::CancelledAfterBackup(receipt)) = outcome else {
        panic!("cancel must keep published backup receipt")
    };
    assert!(options.backups_directory.join(receipt.name).is_file());
    assert_eq!(host.collection().unwrap().generation(), generation);
    meguri_engine::decks::add(&host.collection().unwrap(), "after cancelled import".into()).unwrap();
    host.close().unwrap();
}
#[test]
fn ready_afteractivation_failure_keeps_new_host_generation_fence_and_real_backup() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    let candidate = inspect(&process, &source, &options);
    let generation = host.collection().unwrap().generation();
    meguri_model::faultpoint::arm(
        "replace.after_dir_sync",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let outcome = activate(&process, candidate, host.clone());
    meguri_model::faultpoint::disarm("replace.after_dir_sync");
    let Err(EntryError::Replacement {
        error: ReplaceError::AfterActivation {
            generation: Some(active),
            ..
        },
        backup: Some(receipt),
    }) = outcome
    else {
        panic!("candidate active with backup")
    };
    assert!(active > generation);
    assert_eq!(host.collection().unwrap().generation(), active);
    assert!(options.backups_directory.join(receipt.name).is_file());
    assert_eq!(
        DeviceState::for_collection(&options.collection_directory.join("collection.anki2"))
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    answer(&host);
    host.close().unwrap();
}

#[test]
fn initial_provider_open_failure_keeps_host_and_advisory_ownership_until_explicit_close() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    // Opening the media journal must fail when its file path is a directory.
    let journal_directory = options.collection_directory.join("meguri-media.db");
    fs::create_dir(&journal_directory).unwrap();
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let importer = process.clone();
    let inputs = options.clone();
    let candidate = inspect(&process, &source, &options);
    let completed = job(process.engine(), move |ctx| {
        importer.import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
    })
    .unwrap();
    assert!(matches!(
        completed.outcome,
        Err(EntryError::Replacement {
            error: ReplaceError::AfterActivation { generation: None, .. },
            ..
        })
    ));
    assert!(completed.host.collection().is_err());
    assert!(options.collection_directory.join("collection.anki2").is_file());
    assert!(process.open(options.clone()).is_err());
    assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_err());
    assert_eq!(
        DeviceState::for_collection(&options.collection_directory.join("collection.anki2"))
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    completed.host.close().unwrap();
    fs::remove_dir(journal_directory).unwrap();
    let reopened = process.open(options).unwrap();
    answer(&reopened);
    reopened.close().unwrap();
}
#[test]
fn real_ready_collection_survives_invalid_inputs_and_deck_package_selection() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let source = package(root.path());
    let options = config(&root.path().join("target"));
    seed(&options);
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    add_card(&host);
    answer(&host);
    let generation = host.collection().unwrap().generation();
    let fixture_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/package/variants");
    for name in [
        "not-a-zip.colpkg",
        "media-traversal.colpkg",
        "schema-12.colpkg",
        "meta-version-4.colpkg",
    ] {
        let selected = fixture_root.join(name);
        let bytes = fs::read(&selected).unwrap();
        assert!(
            entry::inspect_entry(
                process.engine(),
                &selected,
                &options.entry_inputs(),
                CancelToken::new(),
                &ProgressSink::new()
            )
            .is_err()
        );
        assert_eq!(fs::read(selected).unwrap(), bytes);
        assert_eq!(host.collection().unwrap().generation(), generation);
        assert_eq!(count(&host, Table::Revlog), 1);
    }
    let deck_package = root.path().join("selected.apkg");
    fs::copy(&source, &deck_package).unwrap();
    assert!(matches!(
        entry::inspect_entry(
            process.engine(),
            &deck_package,
            &options.entry_inputs(),
            CancelToken::new(),
            &ProgressSink::new()
        ),
        Err(EntryError::Package(
            meguri_package::CollectionPackageInspectError::UnsupportedFormat
        ))
    ));
    assert!(!options.backups_directory.exists());
    meguri_engine::decks::add(&host.collection().unwrap(), "after refused packages".into()).unwrap();
    host.close().unwrap();
}

static VALIDATION_CANCEL: Mutex<Option<Arc<CancelToken>>> = Mutex::new(None);
fn cancel_at_validation_boundary() {
    VALIDATION_CANCEL.lock().unwrap().as_ref().unwrap().cancel();
}
fn assert_no_entry_staging(directory: &Path) {
    for entry in fs::read_dir(directory).unwrap() {
        assert!(
            !entry
                .unwrap()
                .file_name()
                .to_string_lossy()
                .starts_with(".meguri-stage-")
        );
    }
}
#[test]
fn ready_and_initial_validation_remain_cancellable_by_both_tokens() {
    let _lane = LANE.lock().unwrap();
    for initial in [false, true] {
        for cancel_inspection in [false, true] {
            for boundary in ["replace.after_check", "replace.after_finalise"] {
                let root = tempfile::tempdir().unwrap();
                let source = package(root.path());
                let options = config(&root.path().join("target"));
                if !initial {
                    seed(&options);
                }
                let process = ProcessCore::new(options.engine.clone()).unwrap();
                let host = (!initial).then(|| process.open(options.clone()).unwrap());
                let live = options.collection_directory.join("collection.anki2");
                let old_bytes = fs::read(&live).ok();
                let generation = process.engine().generation();
                let fence = DeviceState::for_collection(&live).publication().unwrap();
                let source_cancel = CancelToken::new();
                let candidate = entry::inspect_entry(
                    process.engine(),
                    &source,
                    &options.entry_inputs(),
                    source_cancel.clone(),
                    &ProgressSink::new(),
                )
                .unwrap();
                let importer = process.clone();
                let inputs = options.clone();
                let backup = host.as_ref().map(CoreHost::backup);
                let (outcome, phase) = job(process.engine(), move |ctx| {
                    *VALIDATION_CANCEL.lock().unwrap() = Some(if cancel_inspection {
                        source_cancel
                    } else {
                        ctx.cancel.clone()
                    });
                    meguri_model::faultpoint::arm(
                        boundary,
                        meguri_model::faultpoint::Fault::Callback(cancel_at_validation_boundary),
                    );
                    let outcome = if let Some(backup) = backup {
                        candidate.activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                    } else {
                        importer
                            .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                            .and_then(|completed| completed.outcome)
                    };
                    (outcome, ctx.progress.phase())
                });
                meguri_model::faultpoint::disarm(boundary);
                *VALIDATION_CANCEL.lock().unwrap() = None;
                assert_ne!(phase, ProgressPhase::Committing);
                if initial {
                    assert_eq!(outcome, Err(EntryError::Cancelled));
                    assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_ok());
                } else {
                    let Err(EntryError::CancelledAfterBackup(receipt)) = outcome else {
                        panic!("validation cancellation must retain the real backup receipt")
                    };
                    assert!(options.backups_directory.join(receipt.name).is_file());
                }
                assert_eq!(fs::read(&live).ok(), old_bytes);
                assert_eq!(process.engine().generation(), generation);
                assert_eq!(DeviceState::for_collection(&live).publication().unwrap(), fence);
                assert_no_entry_staging(&options.collection_directory);
                if let Some(host) = host {
                    meguri_engine::decks::add(&host.collection().unwrap(), "after validation cancel".into()).unwrap();
                    host.close().unwrap();
                }
            }
        }
    }
}

struct CancelOnCommitting {
    engine: Engine,
    activation_cancel: Arc<CancelToken>,
    inspection_cancel: Arc<CancelToken>,
    deliveries: Mutex<usize>,
}
impl ProgressListener for CancelOnCommitting {
    fn on_progress(&self, progress: &Progress) {
        if progress.phase == ProgressPhase::Committing {
            // Reenter read APIs to cover callback delivery outside core locks.
            let _ = self.engine.state();
            if let Ok(collection) = self.engine.collection() {
                collection.reader(ReaderRole::Query).unwrap().col().unwrap();
            }
            *self.deliveries.lock().unwrap() += 1;
            self.activation_cancel.cancel();
            self.inspection_cancel.cancel();
        }
    }
}
#[test]
fn cancellation_after_committing_still_activates_and_finishes_recovery() {
    let _lane = LANE.lock().unwrap();
    for initial in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = package(root.path());
        let options = config(&root.path().join("target"));
        if !initial {
            seed(&options);
        }
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let host = (!initial).then(|| process.open(options.clone()).unwrap());
        let generation = process.engine().generation();
        let source_cancel = CancelToken::new();
        let candidate = entry::inspect_entry(
            process.engine(),
            &source,
            &options.entry_inputs(),
            source_cancel.clone(),
            &ProgressSink::new(),
        )
        .unwrap();
        let engine = process.engine().clone();
        let importer = process.clone();
        let inputs = options.clone();
        let backup = host.as_ref().map(CoreHost::backup);
        meguri_model::faultpoint::arm(
            "replace.after_dir_sync",
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        let (outcome, imported_host, deliveries) = job(process.engine(), move |ctx| {
            let listener = Arc::new(CancelOnCommitting {
                engine,
                activation_cancel: ctx.cancel.clone(),
                inspection_cancel: source_cancel,
                deliveries: Mutex::new(0),
            });
            ctx.progress.set_listener(listener.clone());
            let (outcome, imported_host) = if let Some(backup) = backup {
                (
                    candidate.activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress),
                    None,
                )
            } else {
                let completed = importer
                    .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                    .unwrap();
                (completed.outcome, Some(completed.host))
            };
            let deliveries = *listener.deliveries.lock().unwrap();
            (outcome, imported_host, deliveries)
        });
        meguri_model::faultpoint::disarm("replace.after_dir_sync");
        assert_eq!(deliveries, 1);
        let Err(EntryError::Replacement {
            error:
                ReplaceError::AfterActivation {
                    generation: Some(active),
                    ..
                },
            backup,
        }) = outcome
        else {
            panic!("late cancellation must finish activation and recovery")
        };
        assert!(active > generation);
        assert_eq!(process.engine().generation(), active);
        assert_eq!(
            DeviceState::for_collection(&options.collection_directory.join("collection.anki2"))
                .publication()
                .unwrap()
                .unwrap()
                .decision,
            PublicationDecision::Pending
        );
        assert_eq!(backup.is_some(), !initial);
        if let Some(receipt) = backup {
            assert!(options.backups_directory.join(receipt.name).is_file());
        }
        assert_no_entry_staging(&options.collection_directory);
        let host = host.or(imported_host).unwrap();
        answer(&host);
        host.close().unwrap();
    }
}
