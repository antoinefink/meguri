//! R1-R10/RF2 durable primitive, using the production gate and lifecycle.
mod common;
mod session_support;
use common::{Harness, NOW_MS};
use meguri_engine::{
    Engine, EngineConfig, EngineError, EventKind, Generation, LongOp, OpSource,
    device_state::{AccountBinding, DeviceState, Publication, PublicationDecision},
    replace::{ReplaceError, ReplaceReason, ReplacementStage, StagingError},
};
use meguri_model::faultpoint::{self, Fault};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_storage::{ReadRows, ReaderRole, StorageError};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

fn stage(directory: &Path, fixture: &str) -> ReplacementStage {
    let stage = ReplacementStage::new(directory).unwrap();
    fs::copy(common::fixture(fixture), stage.path()).unwrap();
    stage
}
fn binding() -> AccountBinding {
    AccountBinding::Connected {
        account: "synthetic-account".into(),
        configuration: "test-app".into(),
        environment: "test".into(),
    }
}
fn config(binding: AccountBinding, seed: u64) -> meguri_engine::OpenConfig {
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let mut config = meguri_engine::OpenConfig::new(clock, Arc::new(SeededRandom::new(seed)));
    config.publication_binding = binding;
    config
}
fn publication(live: &Path) -> Publication {
    DeviceState::for_collection(live).publication().unwrap().unwrap()
}
fn marker(collection: &meguri_engine::Collection) -> i64 {
    collection.reader(ReaderRole::Query).unwrap().col().unwrap().dirty
}

#[test]
fn real_import_and_restore_stamp_once_expire_handles_and_publish_one_event() {
    for reason in [ReplaceReason::Restore, ReplaceReason::ColpkgImport] {
        let h = Harness::open();
        let prior = publication(&h.path);
        let old = h.collection.clone();
        let subscription = h.engine.stream().subscribe();
        let slot = h
            .engine
            .acquire_slot(if reason == ReplaceReason::Restore {
                LongOp::Restore
            } else {
                LongOp::ColpkgImport
            })
            .unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let new = exclusive
            .replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
                let col = scope.col()?;
                assert_eq!(col.modified, EpochMillis(NOW_MS));
                assert_eq!(col.schema_modified, EpochMillis(NOW_MS));
                assert_eq!(scope.now(), EpochMillis(NOW_MS));
                let mut changed = col;
                changed.dirty = 722;
                scope.put_col(&changed)?;
                Ok(())
            })
            .unwrap();
        drop(exclusive);
        drop(slot);
        assert_eq!(new.generation(), Generation(2));
        assert_eq!(marker(&new), 722);
        assert!(matches!(
            old.reader(ReaderRole::Query),
            Err(EngineError::StaleGeneration { .. })
        ));
        let events = common::drain(&subscription, Duration::from_millis(20));
        assert_eq!(
            events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::CollectionReplaced { .. }))
                .count(),
            1
        );
        // Day rollover may legitimately have its own targeted change set.
        assert!(
            !events
                .iter()
                .any(|e| matches!(&e.kind, EventKind::Changes(changes) if changes.whole_collection))
        );
        let current = publication(&h.path);
        assert_ne!(current.identity, prior.identity);
        assert_eq!(current.decision, PublicationDecision::Pending);
        assert_eq!(current.binding, AccountBinding::LocalOnly);
        let sidecar = meguri_storage::sidecar::Sidecar::load(&h.dir.path().join("meguri-sidecar.json")).unwrap();
        assert_eq!(sidecar.last_mod, NOW_MS);
        assert_eq!(sidecar.last_scm, NOW_MS);
        assert!(sidecar.extra.contains_key("deviceState"));
        DeviceState::for_collection(&h.path)
            .set_manual_transfer_pending(false)
            .unwrap();
        assert_eq!(publication(&h.path), current);
        common::add_note_with_card(&new, OpSource::UserUndoable, &["local review remains writable", "back"]);
        h.engine.close().unwrap();
        for _ in 0..2 {
            let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
            assert_eq!(publication(&h.path), current);
            assert_eq!(marker(&reopened), 722);
            h.engine.close().unwrap();
        }
    }
}

#[test]
fn manual_one_way_reasons_preserve_cloud_decisions_and_do_not_stamp_packages() {
    let h = Harness::open();
    for pending in [false, true] {
        if pending {
            let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
            slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
                Ok(())
            })
            .unwrap();
        }
        let prior = publication(&h.path);
        for reason in [ReplaceReason::FullDownload, ReplaceReason::FullUpload] {
            let subscription = h.engine.stream().subscribe();
            let slot = h.engine.acquire_slot(LongOp::OneWaySync).unwrap();
            let new = slot
                .replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
                    let mut col = scope.col()?;
                    assert_ne!(col.schema_modified.0, NOW_MS);
                    if reason == ReplaceReason::FullDownload {
                        col.last_sync = col.modified;
                    }
                    scope.put_col(&col)?;
                    Ok(())
                })
                .unwrap();
            let active = publication(&h.path);
            assert_eq!(active.decision, prior.decision);
            assert_eq!(active.binding, prior.binding);
            assert_ne!(active.identity, prior.identity);
            assert_ne!(
                new.reader(ReaderRole::Query).unwrap().col().unwrap().schema_modified.0,
                NOW_MS
            );
            let events = common::drain(&subscription, Duration::from_millis(10));
            assert_eq!(
                events
                    .iter()
                    .filter(|e| matches!(e.kind, EventKind::CollectionReplaced { .. }))
                    .count(),
                1
            );
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(&e.kind, EventKind::Changes(changes) if changes.whole_collection))
            );
        }
    }
}

#[test]
fn wrong_slot_and_bad_candidate_never_close_or_mutate_live_collection() {
    let h = Harness::open();
    let generation = h.collection.generation();
    let prior = publication(&h.path);
    let slot = h.engine.acquire_slot(LongOp::CheckDatabase).unwrap();
    assert!(matches!(
        slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| Ok(
            ()
        )),
        Err(ReplaceError::NotExclusive)
    ));
    drop(slot);
    let bytes = fs::read(&h.path).unwrap();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let bad = stage(h.dir.path(), "ver-10-delete.anki2");
    let owned = bad.directory().to_owned();
    assert!(matches!(
        slot.replace_verified(bad, ReplaceReason::Restore, |_| panic!(
            "refused candidate reached caller"
        )),
        Err(ReplaceError::Staging(StagingError::StagingRefused(_)))
    ));
    assert!(!owned.exists());
    assert_eq!(h.engine.generation(), generation);
    assert_eq!(publication(&h.path), prior);
    assert_eq!(fs::read(&h.path).unwrap(), bytes);
    assert!(h.collection.reader(ReaderRole::Query).is_ok());
}

#[test]
fn pre_rename_faults_preserve_old_bytes_and_exact_clear_or_pending_decision() {
    for already_pending in [false, true] {
        for point in [
            "replace.after_check",
            "replace.after_finalise",
            "replace.after_pause_recorded",
            "replace.after_live_wal_removed",
        ] {
            let h = Harness::open();
            if already_pending {
                let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
                slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
                    Ok(())
                })
                .unwrap();
            }
            // A clean checkpoint baseline makes byte equality meaningful.
            h.engine.close().unwrap();
            let old = fs::read(&h.path).unwrap();
            h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
            let generation = h.engine.generation();
            let prior = publication(&h.path);
            let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
            faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
            let result = slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
                Ok(())
            });
            faultpoint::disarm_all();
            assert!(result.is_err(), "{point}");
            assert_eq!(h.engine.generation(), generation, "{point}");
            assert_eq!(publication(&h.path), prior, "{point}");
            h.engine.close().unwrap();
            assert_eq!(fs::read(&h.path).unwrap(), old, "{point}");
        }
    }
}

#[test]
fn post_rename_errors_are_honest_and_fenced_before_writer_reopens() {
    for point in [
        "replace.after_rename",
        "replace.after_dir_sync",
        "replace.after_durable_activation",
    ] {
        let h = Harness::open();
        let prior = publication(&h.path);
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        let result = exclusive.replace_verified(
            stage(h.dir.path(), "schema-18.anki2"),
            ReplaceReason::Restore,
            |scope| {
                let mut col = scope.col()?;
                col.dirty = 722;
                scope.put_col(&col)?;
                Ok(())
            },
        );
        faultpoint::disarm_all();
        assert!(result.is_err());
        // A repeated activation-boundary I/O error may refuse the immediate
        // reopen. A restart must still recover; it must never reopen unfenced.
        if !matches!(h.engine.state(), meguri_engine::LifecycleState::Failed { .. }) {
            let current = exclusive.collection();
            assert_ne!(current.generation(), h.collection.generation());
            assert_eq!(marker(current), 722);
            assert_eq!(publication(&h.path).decision, PublicationDecision::Pending);
        }
        drop(exclusive);
        drop(slot);
        h.engine.close().unwrap();
        let current = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
        assert_eq!(marker(&current), 722);
        assert_ne!(publication(&h.path).identity, prior.identity);
        assert_eq!(publication(&h.path).decision, PublicationDecision::Pending);
    }
}

#[test]
fn a_failed_intent_durable_save_does_not_rename_or_destroy_evidence() {
    let h = Harness::open();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    // Arm only after finalisation, to avoid failing F3's close beforehand.
    faultpoint::arm(
        "replace.after_finalise",
        Fault::Callback(|| {
            // Close writes F3 once, then the intent writes. Count only those saves.
            SAVE_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
            faultpoint::arm("storage.sidecar.before_save", Fault::Callback(fail_second_save));
        }),
    );
    let old = publication(&h.path);
    let result = exclusive.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
        Ok(())
    });
    faultpoint::disarm_all();
    assert!(result.is_err());
    assert_eq!(publication(&h.path), old);
    assert_eq!(h.engine.generation(), Generation(1));
}
static SAVE_COUNT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
fn fail_second_save() {
    if SAVE_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    }
}

#[test]
fn prior_publication_token_and_manual_transfer_do_not_allow_replacement() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    let engine = Engine::new(EngineConfig::default()).unwrap();
    engine.open_raw(&live, config(binding(), 7)).unwrap();
    let device = DeviceState::for_collection(&live);
    let token = device.publication_token(&binding()).unwrap();
    device.validate_token(&token, &binding()).unwrap();
    let manual = engine.acquire_slot(LongOp::OneWaySync).unwrap();
    manual
        .replace_verified(
            stage(dir.path(), "schema-18.anki2"),
            ReplaceReason::FullDownload,
            |scope| {
                let mut col = scope.col()?;
                col.last_sync = col.modified;
                scope.put_col(&col)?;
                Ok(())
            },
        )
        .unwrap();
    drop(manual);
    assert!(device.validate_token(&token, &binding()).is_err());
    let manual_token = device.publication_token(&binding()).unwrap();
    device.validate_token(&manual_token, &binding()).unwrap();
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage(dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    assert!(device.validate_token(&manual_token, &binding()).is_err());
    assert!(device.validate_token(&token, &binding()).is_err());
    device.set_manual_transfer_pending(true).unwrap();
    device.set_manual_transfer_pending(false).unwrap();
    assert_eq!(publication(&live).decision, PublicationDecision::Pending);
    assert!(device.publication_token(&binding()).is_err());
    assert!(device.validate_token(&token, &AccountBinding::Unknown).is_err());
}

#[test]
fn import_refuses_review_but_low_level_replace_still_supports_active_session() {
    let h = session_support::Harness::new(3);
    let (id, _) = h.start();
    let live = h.dir.path().join("collection.anki2");
    let slot = h.engine.acquire_slot(LongOp::ColpkgImport).unwrap();
    assert!(matches!(
        slot.replace_verified(
            stage(h.dir.path(), "schema-18.anki2"),
            ReplaceReason::ColpkgImport,
            |_| Ok(())
        ),
        Err(ReplaceError::Lifecycle(EngineError::SessionActive))
    ));
    assert_eq!(h.review.active_session(), Some(id));
    drop(slot);
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let source = h.dir.path().join("low-level-replacement.anki2");
    fs::copy(common::fixture("schema-18.anki2"), &source).unwrap();
    exclusive
        .replace(move |path| fs::rename(source, path).map_err(|e| EngineError::Storage(e.into())))
        .unwrap();
    assert_eq!(publication(&live).decision, PublicationDecision::Clear);
}

// The worker always reaches real production fault points. Parents inspect the
// resulting journal and open twice; no synthetic successful recovery records.
static PATHS: Mutex<Option<(PathBuf, PathBuf)>> = Mutex::new(None);
fn capture_intent_boundary() {
    let paths = PATHS.lock().unwrap();
    let (live, candidate) = paths.as_ref().unwrap();
    if live.exists() {
        fs::copy(live, live.with_extension("old-closed")).unwrap();
    }
    fs::copy(candidate, live.with_extension("candidate-final")).unwrap();
    if std::env::var("MEGURI_REPLACE_POINT").unwrap() == "replace.after_pause_recorded" {
        std::process::abort();
    }
}
fn make_identical_before_intent() {
    let paths = PATHS.lock().unwrap();
    let (live, candidate) = paths.as_ref().unwrap();
    // Initial-entry test: no live connection exists. This injects genuinely
    // identical closed bytes before the production intent hashes both files.
    fs::copy(candidate, live).unwrap();
}
#[test]
fn replacement_crash_worker() {
    let Some(live) = std::env::var_os("MEGURI_REPLACE_LIVE") else {
        return;
    };
    let live = PathBuf::from(live);
    let directory = live.parent().unwrap();
    let mode = std::env::var("MEGURI_REPLACE_MODE").unwrap();
    let point = std::env::var("MEGURI_REPLACE_POINT").unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let initial = mode == "absent" || mode == "identical";
    if !initial {
        engine.open_raw(&live, config(binding(), 991)).unwrap();
    }
    if mode == "pending" {
        let slot = engine.acquire_slot(LongOp::Restore).unwrap();
        slot.replace_verified(stage(directory, "schema-18.anki2"), ReplaceReason::Restore, |scope| {
            let mut col = scope.col()?;
            col.dirty = 722;
            scope.put_col(&col)?;
            Ok(())
        })
        .unwrap();
    }
    if let Some(prior) = DeviceState::for_collection(&live).publication().unwrap() {
        fs::write(directory.join("prior.json"), serde_json::to_vec(&prior).unwrap()).unwrap();
    }
    let candidate = stage(
        directory,
        if mode == "older" {
            "schema-11.anki2"
        } else {
            "schema-18.anki2"
        },
    );
    *PATHS.lock().unwrap() = Some((live.clone(), candidate.path()));
    faultpoint::arm("replace.after_pause_recorded", Fault::Callback(capture_intent_boundary));
    if point != "replace.after_pause_recorded" {
        faultpoint::arm(&point, Fault::Abort);
    }
    if mode == "identical" {
        faultpoint::arm("replace.after_finalise", Fault::Callback(make_identical_before_intent));
    }
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let change = |scope: &mut meguri_storage::WriteScope<'_>| -> Result<(), StorageError> {
        let mut col = scope.col()?;
        col.dirty = 722;
        scope.put_col(&col)?;
        Ok(())
    };
    if initial {
        slot.replace_verified_initial(&live, config(binding(), 991), candidate, ReplaceReason::Restore, change)
            .unwrap();
    } else {
        slot.replace_verified(candidate, ReplaceReason::Restore, change)
            .unwrap();
    }
    panic!("fault point was not reached: {point}");
}
fn crash(live: &Path, point: &str, mode: &str) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "replacement_crash_worker", "--nocapture"])
        .env("MEGURI_REPLACE_LIVE", live)
        .env("MEGURI_REPLACE_POINT", point)
        .env("MEGURI_REPLACE_MODE", mode)
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(6),
        "worker must abort at the real fault boundary: {point}"
    );
}
fn settled_old(live: &Path) {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    engine.open_raw(live, config(binding(), 7)).unwrap();
    engine.close().unwrap();
}
fn recover_twice(live: &Path, pending: bool, expected_marker: i64) -> Publication {
    let mut recovered = None;
    for _ in 0..2 {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine.open_raw(live, config(binding(), 144)).unwrap();
        assert_eq!(marker(&collection), expected_marker);
        let active = publication(live);
        assert_eq!(
            active.decision,
            if pending {
                PublicationDecision::Pending
            } else {
                PublicationDecision::Clear
            }
        );
        if let Some(prior) = recovered.as_ref() {
            assert_eq!(&active, prior);
        }
        recovered = Some(active);
        engine.close().unwrap();
        assert!(meguri_storage::integrity::integrity_check(live).unwrap().is_ok());
    }
    recovered.unwrap()
}
#[test]
fn crash_every_boundary_recovers_exact_old_or_pending_candidate_and_restarts_twice() {
    for mode in ["clear", "pending"] {
        for point in [
            "replace.after_check",
            "replace.after_finalise",
            "replace.after_pause_recorded",
            "replace.after_live_wal_removed",
            "replace.after_rename",
            "replace.after_dir_sync",
            "replace.after_durable_activation",
        ] {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(&live, point, mode);
            let prior: Publication = serde_json::from_slice(&fs::read(dir.path().join("prior.json")).unwrap()).unwrap();
            let activated = matches!(
                point,
                "replace.after_rename" | "replace.after_dir_sync" | "replace.after_durable_activation"
            );
            if !activated && live.with_extension("old-closed").exists() {
                assert_eq!(
                    fs::read(&live).unwrap(),
                    fs::read(live.with_extension("old-closed")).unwrap()
                );
            }
            let current = recover_twice(
                &live,
                activated || mode == "pending",
                if activated || mode == "pending" { 722 } else { 0 },
            );
            if activated {
                assert_ne!(current.identity, prior.identity);
            } else {
                assert_eq!(current, prior);
            }
        }
    }
}
#[test]
fn absent_old_and_copy_only_upgrade_recover_from_real_crashes() {
    for mode in ["absent", "older"] {
        for point in ["replace.after_pause_recorded", "replace.after_rename"] {
            let dir = tempfile::tempdir().unwrap();
            let live = dir.path().join("collection.anki2");
            if mode == "older" {
                fs::copy(common::fixture("schema-18.anki2"), &live).unwrap();
                settled_old(&live);
            }
            crash(&live, point, mode);
            if mode == "absent" && point == "replace.after_pause_recorded" {
                for _ in 0..2 {
                    let engine = Engine::new(EngineConfig::default()).unwrap();
                    assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
                    assert!(!live.exists());
                    assert_eq!(publication(&live).decision, PublicationDecision::Clear);
                }
            } else {
                recover_twice(
                    &live,
                    point == "replace.after_rename",
                    if point == "replace.after_rename" { 722 } else { 0 },
                );
            }
        }
    }
}
#[test]
fn invalid_binding_or_unreadable_metadata_preserves_recovery_evidence() {
    for mode in ["account", "uncertain", "unreadable"] {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, "replace.after_pause_recorded", "clear");
        let sidecar = dir.path().join("meguri-sidecar.json");
        if mode == "unreadable" {
            fs::write(&sidecar, b"{\"deviceState\":").unwrap();
        }
        let evidence = fs::read(&sidecar).unwrap();
        let staging: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .filter_map(Result::ok)
            .map(|e| e.path())
            .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(".meguri-stage-"))
            .collect();
        assert!(!staging.is_empty());
        let binding = match mode {
            "account" => AccountBinding::Connected {
                account: "another-account".into(),
                configuration: "test-app".into(),
                environment: "test".into(),
            },
            "uncertain" => AccountBinding::Unknown,
            _ => binding(),
        };
        let engine = Engine::new(EngineConfig::default()).unwrap();
        assert!(engine.open_raw(&live, config(binding, 55)).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), evidence);
        assert!(staging.iter().all(|p| p.exists()));
    }
}
#[test]
fn identical_closed_bytes_are_ambiguous_and_activated_phase_ignores_later_hash_changes() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    crash(&live, "replace.after_pause_recorded", "identical");
    let sidecar = dir.path().join("meguri-sidecar.json");
    let evidence = fs::read(&sidecar).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
    assert_eq!(fs::read(&sidecar).unwrap(), evidence);
    assert!(
        DeviceState::for_collection(&live)
            .publication_token(&binding())
            .is_err()
    );

    let (_dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    crash(&live, "replace.after_durable_activation", "clear");
    let activated = publication(&live);
    // Real activated production journal, then a later external write changes
    // the hash. Activated identity, rather than the old digest, is authoritative.
    let db = meguri_storage::raw::PlainDatabase::open(&live).unwrap();
    db.execute_batch("UPDATE col SET dty=999").unwrap();
    drop(db);
    assert_eq!(recover_twice(&live, true, 999), activated);
}

#[test]
fn local_production_review_after_pending_activation_commits_and_survives_restart() {
    use meguri_engine::{SuspendParticipant, session::AnswerOutcome};
    use meguri_model::{clock::Clock, ids::DeckId};
    let h = session_support::Harness::new(3);
    let live = h.dir.path().join("collection.anki2");
    h.engine.suspend(Duration::from_secs(5)).unwrap();
    let candidate = ReplacementStage::new(h.dir.path()).unwrap();
    fs::copy(&live, candidate.path()).unwrap();
    h.engine.resume().unwrap();
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let current = slot
        .replace_verified(candidate, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    drop(slot);
    let active = publication(&live);
    assert_eq!(active.decision, PublicationDecision::Pending);
    let meguri_engine::session::StartResult::Started { session_id, head, .. } =
        h.review.start(DeckId(1), None).unwrap()
    else {
        panic!("pending publication blocked review");
    };
    let first = &head.items[0];
    assert!(matches!(
        h.review
            .answer(
                session_id,
                first.card_id,
                meguri_sched::answer::Rating::Easy,
                first.state_token,
                EpochMillis(h.clock.now_ms()),
                100
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    h.review.flush().unwrap();
    assert_eq!(
        current
            .reader(ReaderRole::Query)
            .unwrap()
            .ids_after(meguri_storage::Table::Revlog, None, 10)
            .unwrap()
            .len(),
        1
    );
    h.engine.close().unwrap();
    for _ in 0..2 {
        let (engine, reopened, _) = session_support::open(&live, h.clock.clone());
        assert_eq!(
            reopened
                .reader(ReaderRole::Query)
                .unwrap()
                .ids_after(meguri_storage::Table::Revlog, None, 10)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(publication(&live), active);
        engine.close().unwrap();
    }
}

#[test]
fn live_wal_refusal_keeps_the_generation_and_old_collection_integrity() {
    let h = Harness::open();
    *PATHS.lock().unwrap() = Some((h.path.clone(), PathBuf::new()));
    faultpoint::arm(
        "replace.before_live_wal_check",
        Fault::Callback(|| {
            let paths = PATHS.lock().unwrap();
            let live = &paths.as_ref().unwrap().0;
            fs::write(live.with_extension("anki2-wal"), b"injected nonempty WAL").unwrap();
        }),
    );
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let result = slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
        Ok(())
    });
    faultpoint::disarm_all();
    assert!(matches!(
        result,
        Err(ReplaceError::Staging(StagingError::LiveWalNotEmpty))
    ));
    assert_eq!(h.engine.generation(), h.collection.generation());
    drop(slot);
    h.engine.close().unwrap();
    assert!(meguri_storage::integrity::integrity_check(&h.path).unwrap().is_ok());
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(marker(&reopened), 0);
}

#[test]
fn neither_identity_matching_keeps_real_intent_and_blocks_startup_before_sweep() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    crash(&live, "replace.after_pause_recorded", "clear");
    let sidecar = dir.path().join("meguri-sidecar.json");
    let evidence = fs::read(&sidecar).unwrap();
    fs::write(&live, b"neither final file identity").unwrap();
    for _ in 0..2 {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), evidence);
    }
}

#[test]
fn repeated_package_timestamps_produce_distinct_stored_ids() {
    let h = Harness::open();
    let mut prior = publication(&h.path);
    for _ in 0..3 {
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let current = slot
            .replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
                Ok(())
            })
            .unwrap();
        let col = current.reader(ReaderRole::Query).unwrap().col().unwrap();
        assert_eq!(col.modified.0, NOW_MS);
        assert_eq!(col.schema_modified.0, NOW_MS);
        let active = publication(&h.path);
        assert_ne!(active.identity, prior.identity);
        prior = active;
    }
}

#[test]
fn failed_activation_save_refuses_writer_until_restart_proves_the_candidate() {
    let h = Harness::open();
    let prior = publication(&h.path);
    faultpoint::arm(
        "replace.after_dir_sync",
        Fault::Callback(|| {
            faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
        }),
    );
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    let result = slot.replace_verified(
        stage(h.dir.path(), "schema-18.anki2"),
        ReplaceReason::Restore,
        |scope| {
            let mut col = scope.col()?;
            col.dirty = 722;
            scope.put_col(&col)?;
            Ok(())
        },
    );
    faultpoint::disarm_all();
    assert!(matches!(
        result,
        Err(ReplaceError::AfterActivation { generation: None, .. })
    ));
    assert!(matches!(h.engine.state(), meguri_engine::LifecycleState::Failed { .. }));
    assert_eq!(
        publication(&h.path),
        prior,
        "intent blocks both identities before activation save succeeds"
    );
    assert!(
        DeviceState::for_collection(&h.path)
            .publication_token(&AccountBinding::LocalOnly)
            .is_err()
    );
    drop(slot);
    h.engine.close().unwrap();
    let reopened = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(marker(&reopened), 722);
    assert_eq!(publication(&h.path).decision, PublicationDecision::Pending);
    assert_ne!(publication(&h.path).identity, prior.identity);
}

#[test]
fn import_rechecks_session_started_during_participant_flush() {
    use meguri_engine::{Ports, SuspendParticipant, sched::queue::QueueProvider, session::ReviewService};
    use meguri_model::ids::DeckId;
    struct StartDuringFlush {
        review: Arc<ReviewService>,
        armed: std::sync::atomic::AtomicBool,
    }
    impl SuspendParticipant for StartDuringFlush {
        fn name(&self) -> &str {
            "start-review-during-import-flush"
        }
        fn flush(&self) -> Result<(), EngineError> {
            if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.review.start(DeckId(1), None)?;
            }
            Ok(())
        }
    }
    let fixture = session_support::Harness::new(3);
    let live = fixture.dir.path().join("collection.anki2");
    fixture.engine.close().unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let queues = Arc::new(QueueProvider::default());
    let review = ReviewService::new(&engine, queues.clone()).unwrap();
    let race = Arc::new(StartDuringFlush {
        review: review.clone(),
        armed: std::sync::atomic::AtomicBool::new(true),
    });
    let ports = Ports::new()
        .observer(queues.clone())
        .generation_listener(queues)
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review.clone())
        .suspend_participant(race);
    let mut open = common::open_config(&fixture.clock);
    open.ports = ports;
    engine.open_raw(&live, open).unwrap();
    assert!(review.active_session().is_none());
    let generation = engine.generation();
    let slot = engine.acquire_slot(LongOp::ColpkgImport).unwrap();
    let result = slot.replace_verified(
        stage(fixture.dir.path(), "schema-18.anki2"),
        ReplaceReason::ColpkgImport,
        |_| panic!("review race reached validation transaction"),
    );
    assert!(matches!(
        result,
        Err(ReplaceError::Lifecycle(EngineError::SessionActive))
    ));
    assert!(review.active_session().is_some());
    assert_eq!(engine.generation(), generation);
}

#[test]
fn initial_import_with_an_unopened_existing_collection_fails_closed_until_t7() {
    for dangling in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("collection.anki2");
        if dangling {
            std::os::unix::fs::symlink("missing-collection", &live).unwrap();
        } else {
            fs::write(&live, b"synthetic damaged collection").unwrap();
        }
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let slot = engine.acquire_slot(LongOp::ColpkgImport).unwrap();
        let candidate = stage(dir.path(), "schema-18.anki2");
        let result = slot.replace_verified_initial(
            &live,
            config(binding(), 23),
            candidate,
            ReplaceReason::ColpkgImport,
            |_| panic!("T7 refusal must precede candidate changes"),
        );
        assert!(matches!(
            result,
            Err(ReplaceError::Lifecycle(EngineError::Unsupported { .. }))
        ));
        if dangling {
            assert!(live.symlink_metadata().unwrap().file_type().is_symlink());
        } else {
            assert_eq!(fs::read(&live).unwrap(), b"synthetic damaged collection");
        }
        assert!(!dir.path().join("meguri-sidecar.json").exists());
    }
}

static INITIAL_IMPORT_DESTINATION: Mutex<Option<(PathBuf, bool)>> = Mutex::new(None);

fn create_destination_before_initial_ownership() {
    let destination = INITIAL_IMPORT_DESTINATION.lock().unwrap();
    let (live, dangling) = destination.as_ref().unwrap();
    let _ownership = meguri_storage::lockfile::CollectionLock::acquire(live.parent().unwrap()).unwrap();
    assert_eq!(
        live.symlink_metadata().unwrap_err().kind(),
        std::io::ErrorKind::NotFound
    );
    if *dangling {
        std::os::unix::fs::symlink("missing-collection", live).unwrap();
    } else {
        fs::write(live, b"collection created before import ownership").unwrap();
    }
}

#[test]
fn initial_import_rechecks_absence_under_ownership_before_any_mutation() {
    for dangling in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("collection.anki2");
        let sidecar = dir.path().join("meguri-sidecar.json");
        let sidecar_bytes = b"existing sidecar evidence";
        fs::write(&sidecar, sidecar_bytes).unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let slot = engine.acquire_slot(LongOp::ColpkgImport).unwrap();
        let prior = engine.state();
        let generation = engine.generation();
        let candidate = stage(dir.path(), "schema-18.anki2");
        let candidate_bytes = fs::read(candidate.path()).unwrap();
        // Keep the inode readable after the refused stage is cleaned up.
        let mut candidate_file = fs::File::open(candidate.path()).unwrap();
        assert_eq!(
            live.symlink_metadata().unwrap_err().kind(),
            std::io::ErrorKind::NotFound
        );
        *INITIAL_IMPORT_DESTINATION.lock().unwrap() = Some((live.clone(), dangling));
        faultpoint::arm(
            "replace.initial.before_ownership",
            Fault::Callback(create_destination_before_initial_ownership),
        );
        let mut caller_ran = false;
        let result = slot.replace_verified_initial(
            &live,
            config(binding(), 23),
            candidate,
            ReplaceReason::ColpkgImport,
            |_| {
                caller_ran = true;
                Ok(())
            },
        );
        faultpoint::disarm_all();
        *INITIAL_IMPORT_DESTINATION.lock().unwrap() = None;
        assert!(matches!(
            result,
            Err(ReplaceError::Lifecycle(EngineError::Unsupported { .. }))
        ));
        assert!(!caller_ran);
        assert_eq!(engine.generation(), generation);
        assert_eq!(engine.state(), prior);
        if dangling {
            assert!(live.symlink_metadata().unwrap().file_type().is_symlink());
            assert_eq!(fs::read_link(&live).unwrap(), Path::new("missing-collection"));
        } else {
            assert_eq!(fs::read(&live).unwrap(), b"collection created before import ownership");
        }
        assert_eq!(fs::read(&sidecar).unwrap(), sidecar_bytes);
        let mut candidate_after = Vec::new();
        std::io::Read::read_to_end(&mut candidate_file, &mut candidate_after).unwrap();
        assert_eq!(candidate_after, candidate_bytes);
    }
}
