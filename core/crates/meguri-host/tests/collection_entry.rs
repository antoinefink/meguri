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
    package_media(root, &[])
}
fn package_media(root: &Path, media: &[(&str, &[u8])]) -> PathBuf {
    let options = config(&root.join("source"));
    seed(&options);
    let host = CoreHost::open(options.clone()).unwrap();
    add_card(&host);
    let destination = root.join("input.colpkg");
    let c = host.collection().unwrap();
    let lease = c.reader(ReaderRole::Maintenance).unwrap();
    let context = CollectionPackageWriteContext {
        collection_directory: &options.collection_directory,
        purpose: Purpose::Export,
        finalisation: Finalisation::ExclusiveCreate,
        clock: options.clock.as_ref(),
        random: options.random.as_ref(),
        space: options.disk_space_provider.as_ref(),
        is_cancelled: &|| false,
    };
    if !media.is_empty() {
        for (name, bytes) in media {
            fs::write(options.collection_directory.join("collection.media").join(name), bytes).unwrap();
        }
        let plan = meguri_package::preflight_media_collection_export_from_live(&lease, &destination, &context).unwrap();
        meguri_package::write_media_collection_export_from_live(&lease, &destination, &context, plan)
            .unwrap()
            .source_validation
            .validate(&|| false)
            .unwrap();
    } else {
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
    }
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

const IMPORT_MEDIA: &[(&str, &[u8])] = &[
    ("same.bin", b"equal bytes"),
    ("collision.bin", b"new bytes"),
    ("_static.css", b"static bytes"),
    ("empty.dat", b""),
    ("unused.bin", b"unused bytes"),
];
fn assert_import_media(host: &CoreHost, replacement: bool) {
    let service = host.media().unwrap();
    for (name, expected) in IMPORT_MEDIA {
        assert_eq!(fs::read(service.store().media_dir().join(name)).unwrap(), *expected);
    }
    if replacement {
        assert_eq!(
            fs::read(service.store().media_dir().join("unrelated.bin")).unwrap(),
            b"keep unrelated"
        );
    }
    let baseline = service.store().sync_baseline().unwrap();
    assert!(baseline.owner.is_none());
    assert_eq!(baseline.usn, 0);
    assert!(service.store().download_intents().unwrap().is_empty());
    for entry in service.store().entries().unwrap() {
        assert!(entry.live);
        assert!(!entry.pending_download);
        assert_eq!(entry.dirty, entry.syncable());
    }
    let empty = service.store().sync_entry("empty.dat").unwrap().unwrap();
    assert_eq!(empty.sync_state, meguri_storage::media::MediaSyncState::None);
}
fn old_import_media(host: &CoreHost) {
    let media = host.media().unwrap();
    media.add("same.bin", b"equal bytes").unwrap();
    media.add("collision.bin", b"old bytes").unwrap();
    media.add("unrelated.bin", b"keep unrelated").unwrap();
    media
        .store()
        .bind_sync_owner(&meguri_storage::media::MediaBaselineOwner::new("synthetic-old-owner"))
        .unwrap();
    for p in media.store().pending().unwrap() {
        media.store().acknowledge(&p.name, p.change_counter).unwrap();
    }
}
#[test]
fn media_entry_initial_and_ready_merge_retain_versions_and_reinitialize_own_journal() {
    let _lane = LANE.lock().unwrap();
    for replacement in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let source_bytes = fs::read(&source).unwrap();
        let options = config(&root.path().join("target"));
        if replacement {
            seed(&options);
        }
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let old_host = replacement.then(|| process.open(options.clone()).unwrap());
        if let Some(h) = &old_host {
            old_import_media(h);
        }
        let generation = process.engine().generation();
        let stream = process.engine().stream().subscribe();
        let candidate = inspect(&process, &source, &options);
        assert_eq!(candidate.facts().media_count, IMPORT_MEDIA.len() as u64);
        let importer = process.clone();
        let inputs = options.clone();
        let backup = old_host.as_ref().map(CoreHost::backup);
        let (host, activated) = job(process.engine(), move |ctx| {
            if let Some(backup) = backup {
                (
                    None,
                    candidate
                        .activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                        .unwrap(),
                )
            } else {
                let completed = importer
                    .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                    .unwrap();
                (Some(completed.host), completed.outcome.unwrap())
            }
        });
        let host = old_host.or(host).unwrap();
        assert_eq!(activated.generation.0, generation.0 + 1);
        assert_eq!(activated.media.incoming, 5);
        assert_eq!(activated.media.reused, u64::from(replacement));
        assert_eq!(activated.media.added, if replacement { 4 } else { 5 });
        assert_eq!(activated.media.trashed, u64::from(replacement));
        assert_eq!(activated.backup.is_some(), replacement);
        let mut replaced = 0;
        while let Some(event) = stream.try_recv() {
            if matches!(event.kind, EventKind::CollectionReplaced { .. }) {
                replaced += 1;
            }
        }
        assert_eq!(replaced, 1);
        assert_import_media(&host, replacement);
        if replacement {
            let trash = host.media().unwrap().store().trash_entries().unwrap();
            let old = trash.iter().find(|t| t.name == "collision.bin").unwrap();
            assert_eq!(
                fs::read(
                    options
                        .collection_directory
                        .join("media.trash")
                        .join(format!("{}-{}/{}", old.trashed_ms, old.id, old.name))
                )
                .unwrap(),
                b"old bytes"
            );
            let receipt = activated.backup.unwrap();
            assert!(options.backups_directory.join(&receipt.name).is_file());
            let sidecar: serde_json::Value =
                serde_json::from_slice(&fs::read(options.collection_directory.join("meguri-sidecar.json")).unwrap())
                    .unwrap();
            assert_eq!(
                sidecar["deviceState"]["collectionMediaReplacements"][0]["backup"]["name"],
                receipt.name
            );
        }
        answer(&host);
        host.close().unwrap();
        let reopened = process.open(options.clone()).unwrap();
        assert_import_media(&reopened, replacement);
        assert_eq!(count(&reopened, Table::Revlog), 1);
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        reopened.close().unwrap();
    }
}

static MEDIA_CRASH_CAPTURE: Mutex<Option<(PathBuf, PathBuf, bool)>> = Mutex::new(None);
fn capture_media_intent() {
    let paths = MEDIA_CRASH_CAPTURE.lock().unwrap();
    let (root, collection, stop) = paths.as_ref().unwrap();
    fs::copy(collection.join("collection.anki2"), root.join("old-closed.db")).unwrap();
    if *stop {
        std::process::abort();
    }
}
#[test]
fn media_entry_crash_worker() {
    let Some(root) = std::env::var_os("MEGURI_LOCAL_MEDIA_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let point = std::env::var("MEGURI_LOCAL_MEDIA_CRASH_POINT").unwrap();
    let options = config(&root.join("target"));
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    let candidate = inspect(&process, &root.join("input.colpkg"), &options);
    if std::env::var_os("MEGURI_LOCAL_MEDIA_IDENTICAL").is_some() {
        meguri_model::faultpoint::arm(
            "replace.before_live_wal_check",
            meguri_model::faultpoint::Fault::Callback(equal_closed_media_databases),
        );
    }
    *MEDIA_CRASH_CAPTURE.lock().unwrap() = Some((
        root.clone(),
        options.collection_directory.clone(),
        point == "replace.after_pause_recorded",
    ));
    meguri_model::faultpoint::arm(
        "replace.after_pause_recorded",
        meguri_model::faultpoint::Fault::Callback(capture_media_intent),
    );
    if point != "replace.after_pause_recorded" {
        meguri_model::faultpoint::arm(&point, meguri_model::faultpoint::Fault::Abort);
    }
    let _ = activate(&process, candidate, Arc::new(host));
    panic!("media fault point not reached");
}
fn equal_closed_media_databases() {
    let state = MEDIA_CRASH_CAPTURE.lock().unwrap();
    let (_, collection, _) = state.as_ref().unwrap();
    let mut candidates = Vec::new();
    for entry in fs::read_dir(collection).unwrap() {
        let entry = entry.unwrap();
        if entry.file_name().to_string_lossy().starts_with(".meguri-stage-") {
            let path = entry.path().join("collection.anki2");
            if path.is_file() {
                candidates.push(path);
            }
        }
    }
    assert_eq!(candidates.len(), 1);
    // Fault-only preparation of equal *closed* database bytes. Keep separate
    // inodes, the real backup and genuinely different incoming media bytes.
    fs::copy(collection.join("collection.anki2"), &candidates[0]).unwrap();
    meguri_storage::atomic_file::full_sync(&fs::File::open(&candidates[0]).unwrap()).unwrap();
}
#[test]
fn media_entry_real_kills_recover_old_or_candidate_before_ready_and_converge() {
    let _lane = LANE.lock().unwrap();
    use std::os::unix::process::ExitStatusExt;
    for (point, identical) in [
        ("replace.after_pause_recorded", false),
        ("replace.media.after_trash", false),
        ("replace.media.after_publish", false),
        ("replace.media.after_journal", false),
        ("replace.after_rename", false),
        ("replace.after_dir_sync", false),
        ("replace.after_durable_activation", false),
        ("replace.media.after_publish", true),
        ("replace.after_rename", true),
        ("replace.after_dir_sync", true),
    ] {
        let root = tempfile::tempdir().unwrap();
        package_media(root.path(), IMPORT_MEDIA);
        let options = config(&root.path().join("target"));
        seed(&options);
        let h = CoreHost::open(options.clone()).unwrap();
        old_import_media(&h);
        let old_entries = h.media().unwrap().store().entries().unwrap();
        let old_baseline = h.media().unwrap().store().sync_baseline().unwrap();
        h.close().unwrap();
        let mut worker = std::process::Command::new(std::env::current_exe().unwrap());
        worker
            .args(["--exact", "media_entry_crash_worker", "--nocapture"])
            .env("MEGURI_LOCAL_MEDIA_CRASH_ROOT", root.path())
            .env("MEGURI_LOCAL_MEDIA_CRASH_POINT", point);
        if identical {
            worker.env("MEGURI_LOCAL_MEDIA_IDENTICAL", "1");
        }
        let status = worker.status().unwrap();
        assert_eq!(status.signal(), Some(6), "real media boundary {point}");
        let candidate = matches!(
            point,
            "replace.after_rename" | "replace.after_dir_sync" | "replace.after_durable_activation"
        );
        for _ in 0..2 {
            let reopened = CoreHost::open(options.clone()).unwrap();
            if candidate {
                assert_import_media(&reopened, true);
            } else {
                assert_eq!(
                    fs::read(options.collection_directory.join("collection.anki2")).unwrap(),
                    fs::read(root.path().join("old-closed.db")).unwrap()
                );
                assert_eq!(
                    fs::read(options.collection_directory.join("collection.media/collision.bin")).unwrap(),
                    b"old bytes"
                );
                assert_eq!(
                    fs::read(options.collection_directory.join("collection.media/unrelated.bin")).unwrap(),
                    b"keep unrelated"
                );
                assert!(
                    !options
                        .collection_directory
                        .join("collection.media/unused.bin")
                        .exists()
                );
                assert_eq!(reopened.media().unwrap().store().entries().unwrap(), old_entries);
                assert_eq!(reopened.media().unwrap().store().sync_baseline().unwrap(), old_baseline);
            }
            let sidecar: serde_json::Value =
                serde_json::from_slice(&fs::read(options.collection_directory.join("meguri-sidecar.json")).unwrap())
                    .unwrap();
            assert!(sidecar["deviceState"]["intent"].is_null());
            reopened.close().unwrap();
        }
    }
}
#[test]
fn media_entry_missing_or_corrupt_recovery_bytes_block_ready_and_preserve_evidence() {
    let _lane = LANE.lock().unwrap();
    use std::os::unix::process::ExitStatusExt;
    for corrupt_snapshot in [false, true] {
        let root = tempfile::tempdir().unwrap();
        package_media(root.path(), IMPORT_MEDIA);
        let options = config(&root.path().join("target"));
        seed(&options);
        let host = CoreHost::open(options.clone()).unwrap();
        old_import_media(&host);
        host.close().unwrap();
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "media_entry_crash_worker", "--nocapture"])
            .env("MEGURI_LOCAL_MEDIA_CRASH_ROOT", root.path())
            .env("MEGURI_LOCAL_MEDIA_CRASH_POINT", "replace.media.after_publish")
            .status()
            .unwrap();
        assert_eq!(status.signal(), Some(6));
        let sidecar_path = options.collection_directory.join("meguri-sidecar.json");
        let evidence = fs::read(&sidecar_path).unwrap();
        let sidecar: serde_json::Value = serde_json::from_slice(&evidence).unwrap();
        let owned = options
            .collection_directory
            .join("media.staging")
            .join(sidecar["deviceState"]["intent"]["media"]["root"].as_str().unwrap());
        if corrupt_snapshot {
            fs::write(owned.join("old-journal.db"), b"invalid recovery snapshot").unwrap();
        } else {
            fs::remove_file(owned.join("0")).unwrap();
        }
        let db = fs::read(options.collection_directory.join("collection.anki2")).unwrap();
        let collision = fs::read(options.collection_directory.join("collection.media/collision.bin")).ok();
        for _ in 0..2 {
            assert!(CoreHost::open(options.clone()).is_err());
            assert_eq!(fs::read(&sidecar_path).unwrap(), evidence);
            assert_eq!(
                fs::read(options.collection_directory.join("collection.anki2")).unwrap(),
                db
            );
            assert_eq!(
                fs::read(options.collection_directory.join("collection.media/collision.bin")).ok(),
                collision
            );
            assert!(owned.is_dir());
        }
    }
}

#[test]
fn media_initial_admission_crash_worker() {
    let Some(root) = std::env::var_os("MEGURI_INITIAL_ADMISSION_CRASH_ROOT") else {
        return;
    };
    let root = PathBuf::from(root);
    let options = config(&root.join("target"));
    let point = std::env::var("MEGURI_INITIAL_ADMISSION_CRASH_POINT").unwrap();
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    if std::env::var_os("MEGURI_INITIAL_ADMISSION_RECOVER").is_some() {
        meguri_model::faultpoint::arm(&point, meguri_model::faultpoint::Fault::Abort);
        let _ = process.open(options);
    } else {
        let candidate = inspect(&process, &root.join("input.colpkg"), &options);
        if std::env::var_os("MEGURI_INITIAL_ADMISSION_ROLLBACK").is_some() {
            // Fail after media publication, while the database is still absent.
            meguri_model::faultpoint::arm(
                "replace.media.after_final_database_verification",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        meguri_model::faultpoint::arm(&point, meguri_model::faultpoint::Fault::Abort);
        let importer = process.clone();
        let _ = job(process.engine(), move |ctx| {
            importer.import_initial(options, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
        });
    }
    panic!("initial admission fault point not reached: {point}");
}

fn kill_initial_admission(root: &Path, point: &str, candidate: bool, recover: bool) {
    use std::os::unix::process::ExitStatusExt;
    let mut worker = std::process::Command::new(std::env::current_exe().unwrap());
    worker
        .args(["--exact", "media_initial_admission_crash_worker", "--nocapture"])
        .env("MEGURI_INITIAL_ADMISSION_CRASH_ROOT", root)
        .env("MEGURI_INITIAL_ADMISSION_CRASH_POINT", point);
    if !candidate {
        worker.env("MEGURI_INITIAL_ADMISSION_ROLLBACK", "1");
    }
    if recover {
        worker.env("MEGURI_INITIAL_ADMISSION_RECOVER", "1");
    }
    let status = worker.status().unwrap();
    assert_eq!(status.signal(), Some(6), "initial admission boundary {point}");
}

fn initial_admission_record(options: &HostConfig) -> serde_json::Value {
    let sidecar: serde_json::Value =
        serde_json::from_slice(&fs::read(options.collection_directory.join("meguri-sidecar.json")).unwrap()).unwrap();
    let records = sidecar["deviceState"]["collectionMediaReplacements"]
        .as_array()
        .unwrap();
    assert_eq!(records.len(), 1);
    records[0].clone()
}

fn assert_initial_admission_converges(options: &HostConfig, candidate: bool) {
    for _ in 0..2 {
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        if candidate {
            let reopened = process.open(options.clone()).unwrap();
            assert_import_media(&reopened, false);
            assert_eq!(count(&reopened, Table::Notes), 1);
            reopened.close().unwrap();
        } else {
            let error = process.open(options.clone()).err().unwrap();
            assert!(
                matches!(error, EngineError::Open(meguri_storage::OpenError::NotFound)),
                "{error:?}"
            );
            assert!(!matches!(
                process.engine().state(),
                meguri_engine::LifecycleState::Ready { .. }
            ));
            assert!(!options.collection_directory.join("collection.anki2").exists());
            for suffix in ["", "-wal", "-shm", "-journal"] {
                assert!(
                    !options
                        .collection_directory
                        .join(format!("meguri-media.db{suffix}"))
                        .exists()
                );
            }
            assert_eq!(
                fs::read_dir(options.collection_directory.join("collection.media"))
                    .unwrap()
                    .count(),
                0
            );
            for (name, bytes) in IMPORT_MEDIA {
                assert!(
                    fs::read_dir(options.collection_directory.join("media.trash"))
                        .unwrap()
                        .any(|entry| { fs::read(entry.unwrap().path().join(name)).is_ok_and(|found| found == *bytes) }),
                    "lost incoming bytes after rollback: {name}"
                );
            }
        }
        let sidecar: serde_json::Value =
            serde_json::from_slice(&fs::read(options.collection_directory.join("meguri-sidecar.json")).unwrap())
                .unwrap();
        assert!(sidecar["deviceState"]["intent"].is_null());
        let record = initial_admission_record(options);
        assert_eq!(record["completed"], true);
        assert_eq!(record["candidateActive"], candidate);
        assert!(record["backup"].is_null());
        let admission = record["initial_admission"][0].as_str().unwrap();
        assert!(
            !options
                .collection_directory
                .join("media.staging")
                .join(admission)
                .exists()
        );
    }
}

#[test]
fn media_initial_admission_real_kills_recover_absent_or_candidate_twice() {
    let _lane = LANE.lock().unwrap();
    for candidate in [false, true] {
        for point in [
            "replace.media.before_completion",
            "replace.media.after_completion",
            "replace.media.admission.before_cleanup",
            "replace.media.admission.during_cleanup",
            "replace.media.admission.after_manifest_cleanup",
            "replace.media.admission.after_cleanup",
        ] {
            let root = tempfile::tempdir().unwrap();
            let source = package_media(root.path(), IMPORT_MEDIA);
            let source_bytes = fs::read(&source).unwrap();
            let options = config(&root.path().join("target"));
            kill_initial_admission(root.path(), point, candidate, false);
            let record = initial_admission_record(&options);
            let admission = options
                .collection_directory
                .join("media.staging")
                .join(record["initial_admission"][0].as_str().unwrap());
            if point == "replace.media.before_completion" {
                assert!(admission.join("admission.json").is_file());
                assert!(admission.join("ownership.json").is_file());
                assert!(admission.join("journal").is_file());
                assert_ne!(record["completed"], true);
                let sidecar: serde_json::Value = serde_json::from_slice(
                    &fs::read(options.collection_directory.join("meguri-sidecar.json")).unwrap(),
                )
                .unwrap();
                assert!(sidecar["deviceState"]["intent"].is_object());
            } else {
                assert_eq!(record["completed"], true);
            }
            if point == "replace.media.admission.after_manifest_cleanup" {
                assert!(admission.is_dir());
                assert!(!admission.join("admission.json").exists());
            }
            if point == "replace.media.admission.after_cleanup" {
                assert!(!admission.exists());
            }
            assert_initial_admission_converges(&options, candidate);
            assert_eq!(fs::read(&source).unwrap(), source_bytes);
        }
    }
}

#[test]
fn media_initial_admission_kills_inside_recovery_and_partial_completed_cleanup_converge() {
    let _lane = LANE.lock().unwrap();
    for candidate in [false, true] {
        for first in ["replace.media.before_completion", "replace.media.after_completion"] {
            for second in [
                "replace.media.admission.during_cleanup",
                "replace.media.admission.after_manifest_cleanup",
                "replace.media.admission.after_cleanup",
            ] {
                let root = tempfile::tempdir().unwrap();
                let source = package_media(root.path(), IMPORT_MEDIA);
                let source_bytes = fs::read(&source).unwrap();
                let options = config(&root.path().join("target"));
                kill_initial_admission(root.path(), first, candidate, false);
                kill_initial_admission(root.path(), second, candidate, true);
                assert_initial_admission_converges(&options, candidate);
                assert_eq!(fs::read(&source).unwrap(), source_bytes);
            }
        }
    }
}

#[test]
fn media_initial_active_admission_missing_or_corrupt_blocks_ready_and_preserves_evidence() {
    let _lane = LANE.lock().unwrap();
    for candidate in [false, true] {
        for missing in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = package_media(root.path(), IMPORT_MEDIA);
            let source_bytes = fs::read(&source).unwrap();
            let options = config(&root.path().join("target"));
            kill_initial_admission(root.path(), "replace.media.before_completion", candidate, false);
            let sidecar_path = options.collection_directory.join("meguri-sidecar.json");
            let evidence = fs::read(&sidecar_path).unwrap();
            let record = initial_admission_record(&options);
            let admission = options
                .collection_directory
                .join("media.staging")
                .join(record["initial_admission"][0].as_str().unwrap());
            let manifest = admission.join("admission.json");
            if missing {
                fs::remove_file(&manifest).unwrap();
            } else {
                fs::write(&manifest, b"invalid admission evidence").unwrap();
            }
            let database = fs::read(options.collection_directory.join("collection.anki2")).ok();
            for _ in 0..2 {
                let process = ProcessCore::new(options.engine.clone()).unwrap();
                let error = process.open(options.clone()).err().unwrap();
                assert!(error.to_string().contains("publication recovery blocked"), "{error:?}");
                assert!(!matches!(
                    process.engine().state(),
                    meguri_engine::LifecycleState::Ready { .. }
                ));
                assert_eq!(fs::read(&sidecar_path).unwrap(), evidence);
                assert_eq!(
                    fs::read(options.collection_directory.join("collection.anki2")).ok(),
                    database
                );
                assert!(admission.is_dir());
                assert!(admission.join("journal").is_file());
                for (name, bytes) in IMPORT_MEDIA {
                    let retained = if candidate {
                        fs::read(options.collection_directory.join("collection.media").join(name)).unwrap()
                    } else {
                        let path = fs::read_dir(options.collection_directory.join("media.trash"))
                            .unwrap()
                            .map(|entry| entry.unwrap().path().join(name))
                            .find(|path| path.is_file())
                            .unwrap();
                        fs::read(path).unwrap()
                    };
                    assert_eq!(retained, *bytes);
                }
            }
            assert_eq!(fs::read(&source).unwrap(), source_bytes);
        }
    }
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

struct CancelAtEntryPhase {
    engine: Engine,
    progress: std::sync::Weak<ProgressSink>,
    phase: ProgressPhase,
    tokens: Vec<Arc<CancelToken>>,
    deliveries: Mutex<Vec<ProgressPhase>>,
}
impl ProgressListener for CancelAtEntryPhase {
    fn on_progress(&self, progress: &Progress) {
        self.deliveries.lock().unwrap().push(progress.phase.clone());
        if progress.phase == self.phase {
            // Reenter both the sink and core read APIs to prove neither lock
            // is held during public callback delivery.
            assert_eq!(self.progress.upgrade().unwrap().phase(), progress.phase);
            let state = self.engine.state();
            if progress.phase == ProgressPhase::Committing {
                assert!(matches!(state, meguri_engine::LifecycleState::Ready { .. }));
            }
            if let Ok(collection) = self.engine.collection() {
                collection.reader(ReaderRole::Query).unwrap().col().unwrap();
            }
            for token in &self.tokens {
                token.cancel();
            }
        }
    }
}
#[test]
fn cancellation_after_committing_still_activates_and_finishes_recovery() {
    let _lane = LANE.lock().unwrap();
    for initial in [false, true] {
        for with_media in [false, true] {
            for fail_after_rename in [false, true] {
                let root = tempfile::tempdir().unwrap();
                let source = if with_media {
                    package_media(root.path(), IMPORT_MEDIA)
                } else {
                    package(root.path())
                };
                let source_bytes = fs::read(&source).unwrap();
                let options = config(&root.path().join("target"));
                if !initial {
                    seed(&options);
                }
                let process = ProcessCore::new(options.engine.clone()).unwrap();
                let host = (!initial).then(|| process.open(options.clone()).unwrap());
                if with_media && let Some(host) = &host {
                    old_import_media(host);
                }
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
                if fail_after_rename {
                    meguri_model::faultpoint::arm(
                        "replace.after_dir_sync",
                        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
                    );
                }
                let (outcome, imported_host, deliveries) = job(process.engine(), move |ctx| {
                    let progress = Arc::new(ctx.progress.clone());
                    let listener = Arc::new(CancelAtEntryPhase {
                        engine,
                        progress: Arc::downgrade(&progress),
                        phase: ProgressPhase::Committing,
                        tokens: vec![ctx.cancel.clone(), source_cancel],
                        deliveries: Mutex::new(Vec::new()),
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
                            .unwrap_or_else(|error| {
                                panic!("initial={initial}, media={with_media}, fault={fail_after_rename}, import_initial={error:?}")
                            });
                        (completed.outcome, Some(completed.host))
                    };
                    let deliveries = listener
                        .deliveries
                        .lock()
                        .unwrap()
                        .iter()
                        .filter(|phase| **phase == ProgressPhase::Committing)
                        .count();
                    for token in &listener.tokens {
                        assert!(
                            token.is_cancelled(),
                            "initial={initial}, media={with_media}, fault={fail_after_rename}, outcome={outcome:?}"
                        );
                    }
                    (outcome, imported_host, deliveries)
                });
                meguri_model::faultpoint::disarm("replace.after_dir_sync");
                assert_eq!(
                    deliveries, 1,
                    "initial={initial}, media={with_media}, outcome={outcome:?}"
                );
                let (active, backup) = if fail_after_rename {
                    let error = outcome.as_ref().unwrap_err();
                    assert!(error.candidate_active(), "{error:?}");
                    let EntryError::Replacement {
                        error:
                            ReplaceError::AfterActivation {
                                generation: Some(active),
                                source: activation_error,
                            },
                        backup,
                    } = entry_cause(error)
                    else {
                        panic!(
                            "late cancellation must finish activation and recovery: initial={initial}, media={with_media}, outcome={outcome:?}"
                        )
                    };
                    assert!(
                        matches!(
                            activation_error.as_ref(),
                            ReplaceError::Staging(meguri_storage::verified_replace::ReplaceError::Io(StorageError::Io {
                                detail
                            })) if detail == "injected fault at replace.after_dir_sync"
                        ),
                        "initial={initial}, media={with_media}, outcome={outcome:?}"
                    );
                    if with_media {
                        let EntryError::MediaOutcome {
                            media,
                            retained_reference,
                            ..
                        } = error
                        else {
                            panic!("activated media failure must retain its facts: {error:?}")
                        };
                        assert_eq!(media.incoming, IMPORT_MEDIA.len() as u64);
                        assert!(!retained_reference.is_empty());
                    }
                    (*active, backup.as_deref())
                } else {
                    let activation = outcome.as_ref().unwrap();
                    assert_eq!(
                        activation.media.incoming,
                        if with_media { IMPORT_MEDIA.len() as u64 } else { 0 }
                    );
                    (activation.generation, activation.backup.as_ref())
                };
                assert!(active > generation);
                assert_eq!(process.engine().generation(), active);
                assert_eq!(
                    process.engine().state(),
                    meguri_engine::LifecycleState::Ready { generation: active }
                );
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
                    assert!(options.backups_directory.join(&receipt.name).is_file());
                }
                assert_no_entry_staging(&options.collection_directory);
                assert_eq!(fs::read(&source).unwrap(), source_bytes);
                let host = host.or(imported_host).unwrap();
                if with_media {
                    assert_import_media(&host, !initial);
                }
                answer(&host);
                host.close().unwrap();
            }
        }
    }
}

#[test]
fn cancellation_from_last_public_pre_rename_phase_restores_old_view_or_absence() {
    let _lane = LANE.lock().unwrap();
    for initial in [false, true] {
        for cancel_inspection in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let source = package_media(root.path(), IMPORT_MEDIA);
            let source_bytes = fs::read(&source).unwrap();
            let options = config(&root.path().join("target"));
            if !initial {
                seed(&options);
            }
            let process = ProcessCore::new(options.engine.clone()).unwrap();
            let host = (!initial).then(|| process.open(options.clone()).unwrap());
            if let Some(host) = &host {
                old_import_media(host);
            }
            let store = host.as_ref().map(|host| host.media().unwrap().store().clone());
            let old_journal = store
                .as_ref()
                .map(|store| (store.pending().unwrap(), store.change_counter().unwrap()));
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
            let engine = process.engine().clone();
            let inputs = options.clone();
            let backup = host.as_ref().map(CoreHost::backup);
            let (outcome, phases) = job(process.engine(), move |ctx| {
                let progress = Arc::new(ctx.progress.clone());
                let listener = Arc::new(CancelAtEntryPhase {
                    engine,
                    progress: Arc::downgrade(&progress),
                    phase: ProgressPhase::Named("ready to rename"),
                    tokens: vec![if cancel_inspection {
                        source_cancel
                    } else {
                        ctx.cancel.clone()
                    }],
                    deliveries: Mutex::new(Vec::new()),
                });
                ctx.progress.set_listener(listener.clone());
                let outcome = if let Some(backup) = backup {
                    candidate.activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                } else {
                    importer
                        .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                        .and_then(|completed| completed.outcome)
                };
                assert!(listener.tokens[0].is_cancelled(), "{outcome:?}");
                let phases = listener.deliveries.lock().unwrap().clone();
                (outcome, phases)
            });
            assert!(ProgressPhase::Named("ready to rename").cancellable());
            assert_eq!(phases.last(), Some(&ProgressPhase::Named("ready to rename")));
            assert!(!phases.contains(&ProgressPhase::Committing), "{phases:?}");
            let error = outcome.unwrap_err();
            if initial {
                assert_eq!(entry_cause(&error), &EntryError::Cancelled, "{error:?}");
                for suffix in ["", "-wal", "-shm", "-journal"] {
                    assert!(
                        !options
                            .collection_directory
                            .join(format!("meguri-media.db{suffix}"))
                            .exists()
                    );
                }
                for (name, _) in IMPORT_MEDIA {
                    assert!(
                        !options
                            .collection_directory
                            .join("collection.media")
                            .join(name)
                            .exists()
                    );
                }
                assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_ok());
            } else {
                let EntryError::CancelledAfterBackup(receipt) = entry_cause(&error) else {
                    panic!("pre-rename cancellation must retain the safety receipt: {error:?}")
                };
                assert!(options.backups_directory.join(&receipt.name).is_file());
                let store = store.as_ref().unwrap();
                assert_eq!(
                    Some((store.pending().unwrap(), store.change_counter().unwrap())),
                    old_journal
                );
                assert_eq!(fs::read(store.media_dir().join("collision.bin")).unwrap(), b"old bytes");
                assert_eq!(fs::read(store.media_dir().join("same.bin")).unwrap(), b"equal bytes");
                assert_eq!(
                    fs::read(store.media_dir().join("unrelated.bin")).unwrap(),
                    b"keep unrelated"
                );
                for name in ["_static.css", "empty.dat", "unused.bin"] {
                    assert!(!store.media_dir().join(name).exists());
                }
            }
            assert_eq!(fs::read(&live).ok(), old_bytes);
            assert_eq!(process.engine().generation(), generation);
            assert_eq!(DeviceState::for_collection(&live).publication().unwrap(), fence);
            assert_no_entry_staging(&options.collection_directory);
            assert_no_r6_preparation(&options.collection_directory);
            assert_eq!(fs::read(&source).unwrap(), source_bytes);
            if let Some(host) = host {
                assert_eq!(count(&host, Table::Notes), 0);
                meguri_engine::decks::add(&host.collection().unwrap(), "after public phase cancellation".into())
                    .unwrap();
                host.close().unwrap();
            }
        }
    }
}

fn entry_cause(error: &EntryError) -> &EntryError {
    match error {
        EntryError::MediaOutcome { source, .. } | EntryError::AfterBackup { source, .. } => entry_cause(source),
        error => error,
    }
}
fn assert_no_r6_preparation(directory: &Path) {
    let staging = directory.join("media.staging");
    if !staging.exists() {
        return;
    }
    for entry in fs::read_dir(staging).unwrap() {
        assert!(!entry.unwrap().file_name().to_string_lossy().starts_with("r6-"));
    }
}
struct CurrentMediaObserver {
    engine: Engine,
    live: PathBuf,
    old_native: Option<(u64, u64)>,
    store: Option<meguri_storage::media::MediaStore>,
    counts: Mutex<Vec<meguri_storage::media::ImportMediaCounts>>,
}
impl ProgressListener for CurrentMediaObserver {
    fn on_progress(&self, _: &Progress) {}
    fn on_collection_media_prepared(&self, counts: &meguri_storage::media::ImportMediaCounts) {
        use std::os::unix::fs::MetadataExt;
        let _ = self.engine.state();
        if let Ok(collection) = self.engine.collection() {
            collection.reader(ReaderRole::Query).unwrap().col().unwrap();
        }
        assert_eq!(
            fs::symlink_metadata(&self.live).ok().map(|m| (m.dev(), m.ino())),
            self.old_native
        );
        if let Some(store) = &self.store {
            // A held journal lock would make this return false. Reopen the
            // same shared journal before letting preparation proceed.
            assert!(store.try_close().unwrap());
            store.resume().unwrap();
        }
        self.counts.lock().unwrap().push(counts.clone());
    }
}
#[test]
fn media_current_facts_precede_rename_outside_lifecycle_and_journal_locks() {
    use std::os::unix::fs::MetadataExt;
    let _lane = LANE.lock().unwrap();
    for initial in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let options = config(&root.path().join("target"));
        if !initial {
            seed(&options);
        }
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let host = (!initial).then(|| process.open(options.clone()).unwrap());
        if let Some(host) = &host {
            old_import_media(host);
        }
        let live = options.collection_directory.join("collection.anki2");
        let listener = Arc::new(CurrentMediaObserver {
            engine: process.engine().clone(),
            live: live.clone(),
            old_native: fs::symlink_metadata(&live).ok().map(|m| (m.dev(), m.ino())),
            store: host.as_ref().map(|h| h.media().unwrap().store().clone()),
            counts: Mutex::new(Vec::new()),
        });
        let observer = listener.clone();
        let candidate = inspect(&process, &source, &options);
        let importer = process.clone();
        let inputs = options.clone();
        let backup = host.as_ref().map(CoreHost::backup);
        let (imported, activation) = job(process.engine(), move |ctx| {
            ctx.progress.set_listener(observer);
            if let Some(backup) = backup {
                (
                    None,
                    candidate
                        .activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                        .unwrap(),
                )
            } else {
                let completed = importer
                    .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                    .unwrap();
                (Some(completed.host), completed.outcome.unwrap())
            }
        });
        assert_eq!(*listener.counts.lock().unwrap(), vec![activation.media.clone()]);
        assert_eq!(activation.media.incoming, 5);
        assert_eq!(activation.media.reused, u64::from(!initial));
        assert_eq!(activation.media.trashed, u64::from(!initial));
        host.or(imported).unwrap().close().unwrap();
    }
}

#[test]
fn media_initial_constructor_and_preintent_failures_restore_absence_and_owned_paths() {
    let _lane = LANE.lock().unwrap();
    for boundary in [
        "replace.media.after_journal_open",
        "media.recover.before_candidates",
        "replace.media.prepared",
        "replace.media.initial_before_ensure",
        "replace.media.before_worker_spawn",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let source_bytes = fs::read(&source).unwrap();
        let options = config(&root.path().join("target"));
        let unrelated = options
            .collection_directory
            .join("media.trash")
            .join("unrelated-evidence");
        fs::create_dir_all(&unrelated).unwrap();
        fs::write(unrelated.join("keep.bin"), b"unrelated evidence").unwrap();
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let candidate = inspect(&process, &source, &options);
        let importer = process.clone();
        let inputs = options.clone();
        meguri_model::faultpoint::arm(
            boundary,
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        let outcome = job(process.engine(), move |ctx| {
            importer
                .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                .map(|c| c.outcome)
        });
        meguri_model::faultpoint::disarm(boundary);
        assert!(outcome.is_err(), "{boundary}");
        let error = outcome.err().unwrap();
        assert!(
            !matches!(
                entry_cause(&error),
                EntryError::Replacement {
                    error: ReplaceError::MediaRecoveryRequired { .. },
                    ..
                }
            ),
            "{error:?}"
        );
        assert!(!options.collection_directory.join("collection.anki2").exists());
        for name in [
            "meguri-media.db",
            "meguri-media.db-wal",
            "meguri-media.db-shm",
            "meguri-media.db-journal",
        ] {
            assert!(!options.collection_directory.join(name).exists(), "{boundary}: {name}");
        }
        assert_no_r6_preparation(&options.collection_directory);
        assert_eq!(fs::read(unrelated.join("keep.bin")).unwrap(), b"unrelated evidence");
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_ok());
    }
}

#[test]
fn media_initial_unprovable_restoration_and_j4_failure_retain_typed_authority() {
    let _lane = LANE.lock().unwrap();
    for mode in 0..3 {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let options = config(&root.path().join("target"));
        let live = options.collection_directory.join("collection.anki2");
        let during_j4 = mode == 1;
        if mode != 0 {
            let store =
                meguri_storage::media::MediaStore::open(&live, options.clock.clone(), options.random.clone()).unwrap();
            if during_j4 {
                fs::write(store.staging_dir().join("orphan.bin"), b"recoverable old stage").unwrap();
            }
            assert!(store.try_close().unwrap());
        }
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let candidate = inspect(&process, &source, &options);
        let boundary = if during_j4 {
            "replace.media.initial_during_recovery"
        } else {
            "replace.media.after_journal_open"
        };
        meguri_model::faultpoint::arm(
            boundary,
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        if mode == 0 {
            meguri_model::faultpoint::arm(
                "replace.media.initial_restore",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        let importer = process.clone();
        let inputs = options.clone();
        let error = job(process.engine(), move |ctx| {
            importer
                .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                .err()
                .unwrap()
        });
        meguri_model::faultpoint::disarm(boundary);
        meguri_model::faultpoint::disarm("replace.media.initial_restore");
        let EntryError::Replacement {
            error: ReplaceError::MediaRecoveryRequired { recovery_reference, .. },
            ..
        } = entry_cause(&error)
        else {
            panic!("{error:?}");
        };
        let evidence = options
            .collection_directory
            .join("media.staging")
            .join(recovery_reference);
        assert!(evidence.join("ownership.json").is_file());
        assert!(evidence.join("admission.json").is_file());
        assert!(evidence.join("journal").is_file());
        assert!(options.collection_directory.join("meguri-media.db").is_file());
        assert!(!live.exists());
        assert!(meguri_storage::media::MediaStore::open(&live, options.clock.clone(), options.random.clone()).is_err());
        if during_j4 {
            let entries: Vec<_> = fs::read_dir(options.collection_directory.join("media.trash"))
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            assert!(entries.iter().any(|parent| parent.join("orphan.bin").is_file()
                && fs::read(parent.join("orphan.bin")).unwrap() == b"recoverable old stage"));
        }
        process.engine().close().unwrap();
    }
}

#[test]
fn media_preintent_ready_failures_clean_only_unpublished_owned_artifacts() {
    let _lane = LANE.lock().unwrap();
    for boundary in [
        "replace.media.prepared",
        "replace.media.before_swap_dispatch",
        "replace.media.before_worker_spawn",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let options = config(&root.path().join("target"));
        seed(&options);
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let host = process.open(options.clone()).unwrap();
        old_import_media(&host);
        let store = host.media().unwrap().store().clone();
        let unrelated = store.staging_dir().join("r6-unrelated");
        fs::create_dir(&unrelated).unwrap();
        fs::write(unrelated.join("proof.bin"), b"preserve recovery").unwrap();
        let trash_before: Vec<_> = fs::read_dir(store.trash_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        let pending_before = store.pending().unwrap();
        let candidate = inspect(&process, &source, &options);
        let backup = host.backup();
        meguri_model::faultpoint::arm(
            boundary,
            meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
        );
        let error = job(process.engine(), move |ctx| {
            candidate
                .activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                .unwrap_err()
        });
        meguri_model::faultpoint::disarm(boundary);
        let EntryError::Replacement {
            backup: Some(backup), ..
        } = entry_cause(&error)
        else {
            panic!("{error:?}");
        };
        assert!(options.backups_directory.join(&backup.name).is_file());
        let trash_after: Vec<_> = fs::read_dir(store.trash_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(trash_after, trash_before);
        assert_eq!(store.pending().unwrap(), pending_before);
        assert_eq!(fs::read(store.media_dir().join("collision.bin")).unwrap(), b"old bytes");
        let staging: Vec<_> = fs::read_dir(store.staging_dir())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(staging, vec![std::ffi::OsString::from("r6-unrelated")]);
        assert_eq!(fs::read(unrelated.join("proof.bin")).unwrap(), b"preserve recovery");
        host.close().unwrap();
    }
}

#[test]
fn media_cancellation_during_full_hashes_never_renames_and_restores_old_state() {
    use std::os::unix::fs::MetadataExt;
    let _lane = LANE.lock().unwrap();
    for boundary in [
        "replace.media.capacity_entry",
        "replace.media.old_hash_chunk",
        "replace.media.trash_hash_chunk",
        "replace.media.final_database_hash",
        "replace.media.after_final_database_verification",
        "replace.media.prepared",
    ] {
        let root = tempfile::tempdir().unwrap();
        let mut incoming = IMPORT_MEDIA.to_vec();
        incoming.push(("large.bin", b"new large-file version"));
        let source = package_media(root.path(), &incoming);
        let source_bytes = fs::read(&source).unwrap();
        let options = config(&root.path().join("target"));
        seed(&options);
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let host = process.open(options.clone()).unwrap();
        old_import_media(&host);
        let store = host.media().unwrap().store().clone();
        // A large admitted old file and a prior trash version exercise chunk
        // polling without changing the real job's existing 20-second deadline.
        host.media()
            .unwrap()
            .add("large.bin", &vec![7; 8 * 1024 * 1024])
            .unwrap();
        store.trash(&["large.bin".into()]).unwrap();
        host.media()
            .unwrap()
            .add("large.bin", &vec![8; 8 * 1024 * 1024])
            .unwrap();
        let live = options.collection_directory.join("collection.anki2");
        let inode = fs::metadata(&live).unwrap().ino();
        let generation = process.engine().generation();
        let pending = store.pending().unwrap();
        let counter = store.change_counter().unwrap();
        let candidate = inspect(&process, &source, &options);
        let backup = host.backup();
        let error = job(process.engine(), move |ctx| {
            *VALIDATION_CANCEL.lock().unwrap() = Some(ctx.cancel.clone());
            meguri_model::faultpoint::arm(
                boundary,
                meguri_model::faultpoint::Fault::Callback(cancel_at_validation_boundary),
            );
            candidate
                .activate_ready(ctx.slot().unwrap(), backup.as_ref(), &ctx.cancel, &ctx.progress)
                .unwrap_err()
        });
        meguri_model::faultpoint::disarm(boundary);
        *VALIDATION_CANCEL.lock().unwrap() = None;
        let EntryError::CancelledAfterBackup(backup) = entry_cause(&error) else {
            panic!("{boundary}: {error:?}");
        };
        assert!(options.backups_directory.join(&backup.name).is_file());
        assert_eq!(fs::metadata(&live).unwrap().ino(), inode);
        assert_eq!(process.engine().generation(), generation);
        assert_eq!(count(&host, Table::Notes), 0);
        assert_eq!(fs::read(store.media_dir().join("collision.bin")).unwrap(), b"old bytes");
        assert_eq!(fs::read(store.media_dir().join("same.bin")).unwrap(), b"equal bytes");
        assert_eq!(store.pending().unwrap(), pending);
        assert_eq!(store.change_counter().unwrap(), counter);
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        if !matches!(
            boundary,
            "replace.media.final_database_hash"
                | "replace.media.after_final_database_verification"
                | "replace.media.trash_hash_chunk"
        ) {
            assert_no_r6_preparation(&options.collection_directory);
        }
        host.collection()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .col()
            .unwrap();
        host.close().unwrap();
    }
}

#[test]
fn media_initial_cancellation_restores_no_old_database_and_journal_without_losing_payloads() {
    let _lane = LANE.lock().unwrap();
    for boundary in [
        "replace.media.prepared",
        "replace.media.final_database_hash",
        "replace.media.after_final_database_verification",
    ] {
        let root = tempfile::tempdir().unwrap();
        let source = package_media(root.path(), IMPORT_MEDIA);
        let source_bytes = fs::read(&source).unwrap();
        let options = config(&root.path().join("target"));
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let candidate = inspect(&process, &source, &options);
        let importer = process.clone();
        let inputs = options.clone();
        let error = job(process.engine(), move |ctx| {
            *VALIDATION_CANCEL.lock().unwrap() = Some(ctx.cancel.clone());
            meguri_model::faultpoint::arm(
                boundary,
                meguri_model::faultpoint::Fault::Callback(cancel_at_validation_boundary),
            );
            importer
                .import_initial(inputs, candidate, ctx.slot().unwrap(), &ctx.cancel, &ctx.progress)
                .err()
                .unwrap()
        });
        meguri_model::faultpoint::disarm(boundary);
        *VALIDATION_CANCEL.lock().unwrap() = None;
        assert_eq!(entry_cause(&error), &EntryError::Cancelled, "{boundary}: {error:?}");
        assert!(!options.collection_directory.join("collection.anki2").exists());
        for name in [
            "meguri-media.db",
            "meguri-media.db-wal",
            "meguri-media.db-shm",
            "meguri-media.db-journal",
        ] {
            assert!(!options.collection_directory.join(name).exists(), "{boundary}: {name}");
        }
        assert_eq!(fs::read(&source).unwrap(), source_bytes);
        if boundary == "replace.media.prepared" {
            assert_no_r6_preparation(&options.collection_directory);
        } else {
            let trash = options.collection_directory.join("media.trash");
            for (name, value) in IMPORT_MEDIA {
                assert!(
                    fs::read_dir(&trash).unwrap().any(|entry| {
                        let path = entry.unwrap().path().join(name);
                        path.is_file() && fs::read(path).unwrap() == *value
                    }),
                    "missing retained incoming bytes: {name}"
                );
            }
        }
        assert!(meguri_storage::lockfile::CollectionLock::acquire(&options.collection_directory).is_ok());
    }
}
