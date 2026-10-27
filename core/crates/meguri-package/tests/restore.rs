//! Local T7/T8 integration with the real package writer and verified replace.
use meguri_engine::{
    CancelToken, EngineError, EventKind, LongOp, ProgressSink,
    device_state::{DeviceState, PublicationDecision},
    ports::TransferBackup,
};
use meguri_model::{
    clock::Clock,
    faultpoint::{self, Fault},
};
use meguri_package::{
    CollectionPackageInspectContext, CollectionPackageInspectError, ForcedBackupProvider, inspect_collection_package,
    restore::RestoreError,
};
use meguri_storage::{
    ReadRows, ReaderRole, Record, StorageError,
    space::{SpaceProvider, StatvfsSpace},
};
use meguri_testkit::{bench::fixtures::open_engine_collection_with_backups, normal_support::Fixture};
use sha1::{Digest, Sha1};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
static SERIAL: Mutex<()> = Mutex::new(());
fn restore(f: &Fixture, name: &str) -> Result<meguri_package::restore::RestoredBackup, RestoreError> {
    f.host.restore_backup(name, &CancelToken::new(), &ProgressSink::new())
}
fn inspect(f: &Fixture, name: &str) -> meguri_package::InspectedCollection {
    inspect_collection_package(
        &f.dir.path().join("backups").join(name),
        &CollectionPackageInspectContext {
            collection_directory: f.collection.path().parent().unwrap(),
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: f.clock.as_ref(),
            random: f.random.as_ref(),
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap()
}
fn cards(f: &Fixture) -> Vec<Vec<meguri_storage::Cell>> {
    let c = f.host.collection().unwrap();
    let r = c.reader(ReaderRole::Query).unwrap();
    f.cards
        .iter()
        .map(|id| r.card(*id).unwrap().unwrap().to_cells())
        .collect()
}
fn bytes(f: &Fixture) -> (Vec<u8>, Option<Vec<u8>>, Vec<u8>) {
    (
        fs::read(f.collection.path()).unwrap(),
        fs::read(f.collection.path().with_file_name("collection.anki2-wal")).ok(),
        fs::read(
            f.collection
                .path()
                .parent()
                .unwrap()
                .join("collection.media/restore-sentinel"),
        )
        .unwrap(),
    )
}
fn sentinel(f: &Fixture) {
    fs::write(
        f.collection
            .path()
            .parent()
            .unwrap()
            .join("collection.media/restore-sentinel"),
        b"keep media bytes",
    )
    .unwrap();
    f.host.engine().maintenance().wait_idle_for_testing();
}
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/package")
        .join(name)
}
#[test]
fn five_real_answers_backup_restore_and_reopen_recover_earlier_cards_and_keep_safety_and_fence() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(5);
    sentinel(&f);
    let earlier_cards = cards(&f);
    let earlier = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    let old_generation = f.collection.generation();
    for id in &f.cards {
        f.clock.step_ms(1000);
        f.answer(*id, None);
    }
    let later = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    assert_eq!(inspect(&f, &later.name).facts().revlog, 5);
    let events = f.host.engine().stream().subscribe();
    let result = restore(&f, &earlier.name).unwrap();
    assert_eq!(result.generation.0, old_generation.0 + 1);
    assert!(result.cloud_publication_pending);
    assert_eq!(cards(&f), earlier_cards);
    assert_eq!(inspect(&f, result.safety_backup.as_ref().unwrap()).facts().revlog, 5);
    assert_eq!(
        fs::read(
            f.collection
                .path()
                .parent()
                .unwrap()
                .join("collection.media/restore-sentinel")
        )
        .unwrap(),
        b"keep media bytes"
    );
    let c = f.host.collection().unwrap();
    let col = c.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!(
        (col.modified.0, col.schema_modified.0),
        (f.clock.now_ms(), f.clock.now_ms())
    );
    assert_eq!(
        DeviceState::for_collection(c.path())
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    assert_eq!(
        std::iter::from_fn(|| events.try_recv())
            .filter(|e| matches!(&e.kind, EventKind::CollectionReplaced { .. }))
            .count(),
        1
    );
    f.host.close().unwrap();
    let reopened =
        open_engine_collection_with_backups(f.collection.path(), &f.dir.path().join("backups"), f.clock.clone())
            .unwrap();
    let c = reopened.collection().unwrap();
    let r = c.reader(ReaderRole::Query).unwrap();
    let reopened_cards: Vec<_> = f
        .cards
        .iter()
        .map(|id| r.card(*id).unwrap().unwrap().to_cells())
        .collect();
    assert_eq!(reopened_cards, earlier_cards);
    drop(r);
    let state = DeviceState::for_collection(c.path());
    state.set_manual_transfer_pending(false).unwrap();
    assert_eq!(
        state.publication().unwrap().unwrap().decision,
        PublicationDecision::Pending
    );
    reopened.close().unwrap();
}
#[test]
fn invalid_corrupt_newer_unsupported_cancel_session_and_busy_preserve_bytes_and_media() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(1);
    sentinel(&f);
    let backup = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    f.host.engine().maintenance().wait_idle_for_testing();
    let before = bytes(&f);
    let generation = f.collection.generation();
    for name in [
        "../escape.colpkg",
        "notes.txt",
        "server-copy-2030-01-15-10.00.00.unverified.anki2",
    ] {
        assert!(matches!(
            restore(&f, name),
            Err(RestoreError::Engine(EngineError::InvalidArgument { .. }))
        ));
    }
    for (file, kind) in [
        ("variants/corrupt-page.colpkg", "corrupt"),
        ("variants/meta-version-4.colpkg", "newer"),
        ("variants/schema-12.colpkg", "schema"),
    ] {
        let name = "backup-2000-01-01-00.00.00.colpkg";
        fs::copy(fixture(file), f.dir.path().join("backups").join(name)).unwrap();
        let error = restore(&f, name).unwrap_err();
        match (kind, error) {
            (
                "corrupt",
                RestoreError::Package(
                    CollectionPackageInspectError::Corrupt { .. }
                    | CollectionPackageInspectError::SchemaUnsupported { .. },
                ),
            )
            | ("newer", RestoreError::Package(CollectionPackageInspectError::TooNew))
            | ("schema", RestoreError::Package(CollectionPackageInspectError::SchemaUnsupported { .. })) => {}
            other => panic!("wrong refusal: {other:?}"),
        }
        assert_eq!(bytes(&f), before);
    }
    let cancel = CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        f.host.restore_backup(&backup.name, &cancel, &ProgressSink::new()),
        Err(RestoreError::Engine(EngineError::Cancelled))
    ));
    let slot = f.host.engine().acquire_slot(LongOp::CheckDatabase).unwrap();
    assert!(matches!(
        restore(&f, &backup.name),
        Err(RestoreError::Engine(EngineError::Busy { .. }))
    ));
    drop(slot);
    let meguri_engine::session::StartResult::Started { session_id, .. } =
        f.host.review().start(meguri_model::ids::DeckId(1), None).unwrap()
    else {
        panic!("session starts")
    };
    let during_session = bytes(&f);
    assert!(matches!(
        restore(&f, &backup.name),
        Err(RestoreError::Engine(EngineError::SessionActive))
    ));
    assert_eq!(bytes(&f), during_session);
    f.host.review().end(session_id).unwrap();
    assert_eq!(f.host.engine().generation(), generation);
}
#[test]
fn pre_rename_failure_preserves_generation_prior_publication_and_media_and_keeps_safety() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(0);
    sentinel(&f);
    let chosen = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    let before = bytes(&f);
    let publication = DeviceState::for_collection(f.collection.path()).publication().unwrap();
    faultpoint::arm("replace.after_check", Fault::IoError(std::io::ErrorKind::Other));
    let result = restore(&f, &chosen.name);
    faultpoint::disarm("replace.after_check");
    let Err(RestoreError::Replacement {
        safety_backup: Some(name),
        ..
    }) = result
    else {
        panic!("pre-rename failure with safety: {result:?}")
    };
    assert!(f.dir.path().join("backups").join(name).is_file());
    assert_eq!(bytes(&f), before);
    assert_eq!(f.host.engine().generation(), f.collection.generation());
    assert_eq!(
        DeviceState::for_collection(f.collection.path()).publication().unwrap(),
        publication
    );
}
struct NoSpace;
impl SpaceProvider for NoSpace {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        Ok(0)
    }
}
fn raw_provider(f: &Fixture, space: Arc<dyn SpaceProvider>) -> ForcedBackupProvider {
    ForcedBackupProvider::new(
        f.host.engine().clone(),
        f.collection.path().into(),
        f.dir.path().join("backups"),
        f.clock.clone(),
        f.random.clone(),
        space,
    )
}
#[test]
fn damaged_main_and_nonempty_wal_are_verified_never_shm_and_never_recorded() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(0);
    f.host.close().unwrap();
    let _ownership = meguri_storage::lockfile::CollectionLock::acquire(f.collection.path().parent().unwrap()).unwrap();
    fs::write(f.collection.path(), b"page-damaged database bytes").unwrap();
    fs::write(
        f.collection.path().with_file_name("collection.anki2-wal"),
        b"nonempty committed WAL witness",
    )
    .unwrap();
    fs::write(
        f.collection.path().with_file_name("collection.anki2-shm"),
        b"never copy shm",
    )
    .unwrap();
    let before = DeviceState::for_collection(f.collection.path()).last_backup().unwrap();
    use meguri_engine::ports::BackupService;
    let written = raw_provider(&f, Arc::new(StatvfsSpace))
        .before_restore(None, true, &CancelToken::default())
        .unwrap();
    let TransferBackup::Raw {
        main, wal: Some(wal), ..
    } = written.backup
    else {
        panic!("raw pair")
    };
    assert!(main.starts_with("corrupt-") && main.ends_with(".anki2"));
    for (source, destination) in [
        (f.collection.path().to_owned(), main),
        (f.collection.path().with_file_name("collection.anki2-wal"), wal),
    ] {
        assert_eq!(
            Sha1::digest(fs::read(source).unwrap()),
            Sha1::digest(fs::read(f.dir.path().join("backups").join(destination)).unwrap())
        );
    }
    assert_eq!(fs::read_dir(f.dir.path().join("backups")).unwrap().count(), 2);
    assert!(
        meguri_package::backup::listing::list(&f.dir.path().join("backups"))
            .unwrap()
            .is_empty()
    );
    assert_eq!(
        DeviceState::for_collection(f.collection.path()).last_backup().unwrap(),
        before
    );
}
fn fail_main_after_wal() {
    faultpoint::arm(
        "storage.atomic.after_sync",
        Fault::IoError(std::io::ErrorKind::StorageFull),
    );
}
#[test]
fn raw_space_and_main_failure_refuse_without_main_or_orphan_wal() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(0);
    f.host.close().unwrap();
    let _ownership = meguri_storage::lockfile::CollectionLock::acquire(f.collection.path().parent().unwrap()).unwrap();
    fs::write(f.collection.path(), b"damaged").unwrap();
    fs::write(f.collection.path().with_file_name("collection.anki2-wal"), b"wal").unwrap();
    use meguri_engine::ports::BackupService;
    assert!(matches!(
        raw_provider(&f, Arc::new(NoSpace)).before_restore(None, true, &CancelToken::default()),
        Err(EngineError::SafetyBackupFailed(_))
    ));
    assert!(!f.dir.path().join("backups").exists());
    faultpoint::arm("storage.atomic.after_rename", Fault::Callback(fail_main_after_wal));
    let result = raw_provider(&f, Arc::new(StatvfsSpace)).before_restore(None, true, &CancelToken::default());
    faultpoint::disarm("storage.atomic.after_rename");
    faultpoint::disarm("storage.atomic.after_sync");
    assert!(matches!(result, Err(EngineError::SafetyBackupFailed(_))));
    assert_eq!(fs::read_dir(f.dir.path().join("backups")).unwrap().count(), 0);
    assert_eq!(fs::read(f.collection.path()).unwrap(), b"damaged");
    assert_eq!(
        fs::read(f.collection.path().with_file_name("collection.anki2-wal")).unwrap(),
        b"wal"
    );
}

#[test]
fn combined_capacity_refusal_and_cancel_after_inspection_preserve_all_live_bytes() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(0);
    sentinel(&f);
    let chosen = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    let before = bytes(&f);
    let inputs = meguri_package::entry::EntryInputs {
        collection_directory: f.collection.path().parent().unwrap().into(),
        backups_directory: f.dir.path().join("backups"),
        clock: f.clock.clone(),
        random: f.random.clone(),
        space: Arc::new(NoSpace),
    };
    let failure = meguri_package::restore::restore_backup(
        f.host.engine(),
        &inputs,
        meguri_engine::OpenConfig::new(f.clock.clone(), f.random.clone()),
        f.host.backup().as_ref(),
        &chosen.name,
        &CancelToken::new(),
        &ProgressSink::new(),
    );
    assert!(matches!(
        failure,
        Err(RestoreError::Package(
            CollectionPackageInspectError::InsufficientSpace { .. }
        ))
    ));
    assert_eq!(bytes(&f), before);
    let cancel = CancelToken::new();
    static CANCEL: std::sync::OnceLock<Mutex<Option<Arc<CancelToken>>>> = std::sync::OnceLock::new();
    fn cancel_after_inspection() {
        CANCEL.get().unwrap().lock().unwrap().as_ref().unwrap().cancel();
    }
    *CANCEL.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(cancel.clone());
    faultpoint::arm("replace.after_check", Fault::Callback(cancel_after_inspection));
    let failure = f.host.restore_backup(&chosen.name, &cancel, &ProgressSink::new());
    faultpoint::disarm("replace.after_check");
    assert!(matches!(
        failure,
        Err(RestoreError::Replacement {
            error: meguri_engine::replace::ReplaceError::Lifecycle(EngineError::Cancelled),
            safety_backup: Some(_)
        })
    ));
    assert_eq!(bytes(&f), before);
    assert_eq!(f.host.engine().generation(), f.collection.generation());
}
#[test]
fn restore_allows_no_collection_and_failed_open_and_takes_a_raw_safety_pair_only_when_present() {
    let _serial = SERIAL.lock().unwrap();
    for mode in ["absent", "no_collection", "failed"] {
        let damaged = mode != "absent";
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("collection");
        fs::create_dir(&directory).unwrap();
        let live = directory.join("collection.anki2");
        let backups = root.path().join("backups");
        fs::create_dir(&backups).unwrap();
        let chosen = "server-copy-2030-01-15-10.00.00.colpkg";
        fs::copy(fixture("backup/standard.colpkg"), backups.join(chosen)).unwrap();
        let original = b"damaged page bytes";
        if mode == "failed" {
            fs::write(&live, original).unwrap();
            fs::write(live.with_file_name("collection.anki2-wal"), b"recovery WAL bytes").unwrap();
        }
        let clock = Arc::new(meguri_model::clock::StepClock::new(
            1_894_701_600_000,
            meguri_model::clock::UtcOffset::UTC,
            "UTC",
        ));
        let host =
            meguri_testkit::backup_support::open_restore_host(&live, &backups, clock.clone(), Arc::new(StatvfsSpace))
                .unwrap();
        if mode == "no_collection" {
            assert!(matches!(
                host.engine().state(),
                meguri_engine::LifecycleState::NoCollection
            ));
            fs::write(&live, original).unwrap();
            fs::write(live.with_file_name("collection.anki2-wal"), b"recovery WAL bytes").unwrap();
        }
        assert!(matches!(
            host.engine().state(),
            meguri_engine::LifecycleState::NoCollection | meguri_engine::LifecycleState::Failed { .. }
        ));
        let result = host
            .restore_backup(chosen, &CancelToken::new(), &ProgressSink::new())
            .unwrap();
        assert_eq!(result.safety_backup.is_some(), damaged);
        if let Some(name) = result.safety_backup {
            assert!(name.starts_with("corrupt-"));
            assert_eq!(fs::read(backups.join(&name)).unwrap(), original);
            assert_eq!(
                fs::read(backups.join(format!("{name}-wal"))).unwrap(),
                b"recovery WAL bytes"
            );
        }
        assert_eq!(
            DeviceState::for_collection(&live)
                .publication()
                .unwrap()
                .unwrap()
                .decision,
            PublicationDecision::Pending
        );
        assert!(matches!(
            host.engine().state(),
            meguri_engine::LifecycleState::Ready { .. }
        ));
        host.close().unwrap();
        let reopened = open_engine_collection_with_backups(&live, &backups, clock).unwrap();
        assert_eq!(
            DeviceState::for_collection(&live)
                .publication()
                .unwrap()
                .unwrap()
                .decision,
            PublicationDecision::Pending
        );
        reopened.close().unwrap();
    }
}
#[test]
fn failed_corrupt_live_restore_keeps_its_raw_copy_and_restores_local_review() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(0);
    let backup = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    f.host.engine().mark_corrupt("quick-check failure witness");
    let result = restore(&f, &backup.name).unwrap();
    assert!(result.safety_backup.unwrap().starts_with("corrupt-"));
    assert!(matches!(
        f.host.engine().state(),
        meguri_engine::LifecycleState::Ready { .. }
    ));
    assert_eq!(
        DeviceState::for_collection(f.collection.path())
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
}
#[test]
fn activated_restore_error_keeps_candidate_and_pending_fence_across_reopen() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(1);
    sentinel(&f);
    let chosen = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    f.clock.step_ms(1000);
    f.answer(f.cards[0], None);
    faultpoint::arm("replace.after_rename", Fault::IoError(std::io::ErrorKind::Other));
    let failure = restore(&f, &chosen.name).unwrap_err();
    faultpoint::disarm("replace.after_rename");
    assert!(failure.candidate_active(), "{failure:?}");
    assert_eq!(
        DeviceState::for_collection(f.collection.path())
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    f.host.close().unwrap();
    let host = open_engine_collection_with_backups(f.collection.path(), &f.dir.path().join("backups"), f.clock.clone())
        .unwrap();
    let c = host.collection().unwrap();
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .card(f.cards[0])
            .unwrap()
            .unwrap()
            .reps,
        0
    );
    assert_eq!(
        DeviceState::for_collection(c.path())
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    host.close().unwrap();
}

#[test]
fn maintenance_failure_after_activation_retains_restored_result_safety_and_pending_fence() {
    let _serial = SERIAL.lock().unwrap();
    let f = Fixture::new(1);
    sentinel(&f);
    let earlier_cards = cards(&f);
    let chosen = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    f.clock.step_ms(1000);
    f.answer(f.cards[0], None);
    faultpoint::arm(
        "restore.before_maintenance_reevaluation",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    let error = restore(&f, &chosen.name).unwrap_err();
    faultpoint::disarm("restore.before_maintenance_reevaluation");
    assert!(error.candidate_active());
    let RestoreError::AfterActivation { restored, .. } = error else {
        panic!("post-activation error")
    };
    assert_eq!(restored.name, chosen.name);
    assert_eq!(restored.generation.0, f.collection.generation().0 + 1);
    assert_eq!(restored.generation, f.host.collection().unwrap().generation());
    assert!(restored.cloud_publication_pending);
    let safety = restored.safety_backup.unwrap();
    assert_eq!(inspect(&f, &safety).facts().revlog, 1);
    assert_eq!(cards(&f), earlier_cards);
    let state = DeviceState::for_collection(f.collection.path());
    assert_eq!(
        state.publication().unwrap().unwrap().decision,
        PublicationDecision::Pending
    );
    f.host.close().unwrap();
    let reopened =
        open_engine_collection_with_backups(f.collection.path(), &f.dir.path().join("backups"), f.clock.clone())
            .unwrap();
    let c = reopened.collection().unwrap();
    let r = c.reader(ReaderRole::Query).unwrap();
    let actual: Vec<_> = f
        .cards
        .iter()
        .map(|id| r.card(*id).unwrap().unwrap().to_cells())
        .collect();
    assert_eq!(actual, earlier_cards);
    assert_eq!(
        state.publication().unwrap().unwrap().decision,
        PublicationDecision::Pending
    );
    assert!(f.dir.path().join("backups").join(safety).is_file());
    drop(r);
    reopened.close().unwrap();
}

#[test]
fn preactivation_raw_restore_failure_restores_exact_main_wal_and_keeps_both_safety_copies() {
    let _serial = SERIAL.lock().unwrap();
    static CANCEL: Mutex<Option<Arc<CancelToken>>> = Mutex::new(None);
    fn cancel_restore() {
        CANCEL.lock().unwrap().as_ref().unwrap().cancel();
    }
    for (point, cancelled) in [
        ("replace.after_pause_recorded", false),
        ("replace.wal.after_retire", false),
        ("replace.wal.after_retired_record", false),
        ("replace.after_live_wal_removed", false),
        ("replace.wal.after_retired_record", true),
    ] {
        let f = Fixture::new(0);
        let chosen = f.host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
        f.host.close().unwrap();
        let live = f.collection.path();
        fs::write(live, b"damaged main before raw restore").unwrap();
        fs::write(
            live.with_file_name("collection.anki2-wal"),
            b"retained raw WAL before restore",
        )
        .unwrap();
        let host = meguri_testkit::backup_support::open_restore_host(
            live,
            &f.dir.path().join("backups"),
            f.clock.clone(),
            Arc::new(StatvfsSpace),
        )
        .unwrap();
        let prior = DeviceState::for_collection(live).publication().unwrap();
        let cancel = CancelToken::new();
        *CANCEL.lock().unwrap() = Some(cancel.clone());
        faultpoint::arm(
            point,
            if cancelled {
                Fault::Callback(cancel_restore)
            } else {
                Fault::IoError(std::io::ErrorKind::Other)
            },
        );
        let result = host.restore_backup(&chosen.name, &cancel, &ProgressSink::new());
        faultpoint::disarm(point);
        let error = result.unwrap_err();
        assert!(!error.candidate_active());
        let RestoreError::Replacement {
            safety_backup: Some(name),
            ..
        } = error
        else {
            panic!("safety retained on refusal")
        };
        assert_eq!(fs::read(live).unwrap(), b"damaged main before raw restore");
        assert_eq!(
            fs::read(live.with_file_name("collection.anki2-wal")).unwrap(),
            b"retained raw WAL before restore"
        );
        assert_eq!(
            fs::read(f.dir.path().join("backups").join(&name)).unwrap(),
            b"damaged main before raw restore"
        );
        assert_eq!(
            fs::read(f.dir.path().join("backups").join(format!("{name}-wal"))).unwrap(),
            b"retained raw WAL before restore"
        );
        assert_eq!(DeviceState::for_collection(live).publication().unwrap(), prior);
        host.close().unwrap();
    }
}
