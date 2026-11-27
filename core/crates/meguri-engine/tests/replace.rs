//! R1-R10 durable primitive and RF4 manual-transfer publication boundary.
mod common;
mod session_support;
use common::{Harness, NOW_MS};
use meguri_engine::{
    DisabledReason, Engine, EngineConfig, EngineError, EventKind, Generation, LongOp, OpSource, UndoScope, UndoStatus,
    device_state::{AccountBinding, DeviceState, PendingDecisionReason, Publication, PublicationDecision},
    replace::{CloudAdoptInput, ReplaceError, ReplaceReason, ReplacementStage, StagingError},
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

// Both reasons require pending publication independently of package stamping.
const FENCED_REASONS: [ReplaceReason; 2] = [ReplaceReason::Restore, ReplaceReason::FullDownload];
fn replacement_op(reason: ReplaceReason) -> LongOp {
    match reason {
        ReplaceReason::Restore => LongOp::Restore,
        ReplaceReason::ColpkgImport => LongOp::ColpkgImport,
        ReplaceReason::FullDownload | ReplaceReason::FullUpload => LongOp::OneWaySync,
        ReplaceReason::CloudAdopt => LongOp::CloudAdopt,
    }
}
fn download_change(reason: ReplaceReason, scope: &mut meguri_storage::WriteScope<'_>) -> Result<(), StorageError> {
    if reason == ReplaceReason::FullDownload {
        let mut col = scope.col()?;
        col.last_sync = col.modified;
        scope.put_col(&col)?;
    }
    Ok(())
}

fn cloud_input(local_only: bool) -> CloudAdoptInput {
    CloudAdoptInput {
        operation: serde_json::from_value(serde_json::json!([6106, 1])).unwrap(),
        candidate: Publication {
            identity: serde_json::from_value(serde_json::json!([6106, 2])).unwrap(),
            binding: if local_only {
                AccountBinding::LocalOnly
            } else {
                binding()
            },
            decision: PublicationDecision::Clear,
        },
    }
}

fn cloud_stage(directory: &Path) -> ReplacementStage {
    let candidate = stage(directory, "schema-18.anki2");
    let db = meguri_storage::raw::PlainDatabase::open(&candidate.path()).unwrap();
    db.execute_batch(&format!(
        "UPDATE col SET mod={}, scm={}, ls=19, usn=23, dty=946",
        NOW_MS - 5000,
        NOW_MS - 6000
    ))
    .unwrap();
    drop(db);
    candidate
}

fn assert_cloud_stamps(collection: &meguri_engine::Collection) {
    let col = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!(
        (col.modified.0, col.schema_modified.0, col.last_sync.0, col.usn.0),
        (NOW_MS - 5000, NOW_MS - 6000, 19, 23)
    );
}

// Synthetic prior decisions are made by real replacements. Only the legacy
// absence fixture removes the newly optional member through the shared lock.
fn seed_cloud_prior(engine: &Engine, live: &Path, prior: &str) {
    if prior == "clear" {
        return;
    }
    let reason = if prior == "colpkg" {
        ReplaceReason::ColpkgImport
    } else {
        ReplaceReason::Restore
    };
    let slot = engine.acquire_slot(replacement_op(reason)).unwrap();
    slot.replace_verified(stage(live.parent().unwrap(), "schema-18.anki2"), reason, |scope| {
        let mut col = scope.col()?;
        col.dirty = 722;
        scope.put_col(&col)?;
        Ok(())
    })
    .unwrap();
    drop(slot);
    if prior == "legacy" {
        meguri_storage::sidecar::update_document(&live.parent().unwrap().join("meguri-sidecar.json"), |doc| {
            doc["deviceState"]
                .as_object_mut()
                .unwrap()
                .remove("pendingDecisionReason");
            Ok(())
        })
        .unwrap();
    }
}

fn prior_reason(prior: &str) -> Option<PendingDecisionReason> {
    match prior {
        "restore" => Some(PendingDecisionReason::Restore),
        "colpkg" => Some(PendingDecisionReason::ColpkgImport),
        "clear" | "legacy" => None,
        _ => panic!("unknown synthetic prior"),
    }
}

fn assert_reason(live: &Path, expected: Option<PendingDecisionReason>) {
    assert_eq!(
        DeviceState::for_collection(live).pending_decision_reason().unwrap(),
        expected
    );
    let document = r6_document(live);
    let stored = document["deviceState"].get("pendingDecisionReason");
    match expected {
        Some(reason) => assert_eq!(stored, Some(&serde_json::to_value(reason).unwrap())),
        None => assert!(stored.is_none(), "absence must not become JSON null"),
    }
}

#[test]
fn cloud_adopt_from_clear_or_pending_uses_exact_caller_decision_stamps_and_lifecycle() {
    for prior in ["clear", "restore", "colpkg", "legacy"] {
        for local_only in [false, true] {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            let engine = Engine::new(EngineConfig::default()).unwrap();
            engine.open_raw(&live, config(binding(), 7)).unwrap();
            seed_cloud_prior(&engine, &live, prior);
            // Use the current handle after any prior replacement.
            engine.close().unwrap();
            let old = engine.open_raw(&live, config(binding(), 8)).unwrap();
            assert_reason(&live, prior_reason(prior));
            let before = publication(&live);
            let device = DeviceState::for_collection(&live);
            let old_token = if prior == "clear" {
                Some(device.publication_token(&binding()).unwrap())
            } else {
                assert!(device.publication_token(&binding()).is_err());
                None
            };
            common::add_note_with_card(&old, OpSource::UserUndoable, &["synthetic undo", "back"]);
            assert!(matches!(
                old.peek_undo(UndoScope::Global).unwrap(),
                UndoStatus::Available(_)
            ));
            // Open a real reader and release its lease before exclusive drain.
            drop(old.reader(ReaderRole::Query).unwrap());
            let subscription = engine.stream().subscribe();
            let slot = engine.acquire_slot(LongOp::CloudAdopt).unwrap();
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            let expected = cloud_input(local_only).candidate;
            assert_ne!(expected.identity, before.identity);
            let new = exclusive
                .replace_verified_cloud_adopt(cloud_stage(dir.path()), cloud_input(local_only))
                .unwrap();
            drop(exclusive);
            drop(slot);
            assert_eq!(new.generation(), Generation(old.generation().0 + 1));
            assert!(matches!(
                old.reader(ReaderRole::Query),
                Err(EngineError::StaleGeneration { .. })
            ));
            assert_eq!(
                new.peek_undo(UndoScope::Global).unwrap(),
                UndoStatus::Unavailable(DisabledReason::ClearedByOneWay)
            );
            assert_eq!(publication(&live), expected);
            assert_reason(&live, None);
            assert_cloud_stamps(&new);
            assert_eq!(marker(&new), 946);
            if let Some(token) = old_token {
                assert!(device.validate_token(&token, &binding()).is_err());
            }
            if local_only {
                assert!(device.publication_token(&binding()).is_err());
                assert!(device.publication_token(&AccountBinding::LocalOnly).is_err());
            } else {
                let token = device.publication_token(&binding()).unwrap();
                device.validate_token(&token, &binding()).unwrap();
            }
            let events = common::drain(&subscription, Duration::from_millis(20));
            let replacements: Vec<_> = events
                .iter()
                .filter(|e| matches!(e.kind, EventKind::CollectionReplaced { .. }))
                .collect();
            assert_eq!(replacements.len(), 1);
            assert!(
                matches!(&replacements[0].kind, EventKind::CollectionReplaced { generation } if *generation == new.generation())
            );
            assert_eq!(replacements[0].generation, new.generation());
            assert!(
                !events
                    .iter()
                    .any(|e| matches!(&e.kind, EventKind::Changes(changes) if changes.whole_collection))
            );
            let sidecar = meguri_storage::sidecar::Sidecar::load(&dir.path().join("meguri-sidecar.json")).unwrap();
            assert_eq!((sidecar.last_mod, sidecar.last_scm), (NOW_MS - 5000, NOW_MS - 6000));
            engine.close().unwrap();
            let reopened = engine.open_raw(&live, config(binding(), 9)).unwrap();
            assert_eq!(publication(&live), expected);
            assert_reason(&live, None);
            assert_cloud_stamps(&reopened);
            engine.close().unwrap();
        }
    }
}

#[test]
fn cloud_adopt_without_typed_caller_values_is_invalid_before_any_mutation() {
    #[derive(Debug)]
    struct CountedRandom {
        draws: std::sync::atomic::AtomicUsize,
        inner: SeededRandom,
    }
    impl meguri_model::rng::RandomSource for CountedRandom {
        fn next_u64(&self) -> u64 {
            self.draws.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            meguri_model::rng::RandomSource::next_u64(&self.inner)
        }
    }
    for initial in [false, true] {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let random = Arc::new(CountedRandom {
            draws: std::sync::atomic::AtomicUsize::new(0),
            inner: SeededRandom::new(7),
        });
        let open_config = || {
            let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
            let mut open = meguri_engine::OpenConfig::new(clock, random.clone());
            open.publication_binding = binding();
            open
        };
        if !initial {
            engine.open_raw(&live, open_config()).unwrap();
        } else {
            fs::remove_file(&live).unwrap();
        }
        let slot = engine.acquire_slot(LongOp::CloudAdopt).unwrap();
        let mut exclusive = if initial {
            None
        } else {
            Some(slot.enter_exclusive(true).unwrap())
        };
        let candidate = cloud_stage(dir.path());
        let original_candidate = fs::read(candidate.path()).unwrap();
        let mut held_candidate = fs::File::open(candidate.path()).unwrap();
        let durable_files = || {
            recovery_files(dir.path())
                .into_iter()
                .filter(|(path, _)| {
                    !path
                        .components()
                        .any(|c| c.as_os_str().to_string_lossy().starts_with(".meguri-stage-"))
                })
                .collect::<Vec<_>>()
        };
        let before = durable_files();
        let draws = random.draws.load(std::sync::atomic::Ordering::SeqCst);
        let generation = engine.generation();
        let result = if let Some(exclusive) = exclusive.as_mut() {
            exclusive.replace_verified(candidate, ReplaceReason::CloudAdopt, |_| {
                panic!("missing input reached stage change")
            })
        } else {
            slot.replace_verified_initial(&live, open_config(), candidate, ReplaceReason::CloudAdopt, |_| {
                panic!("missing input reached initial stage change")
            })
        };
        assert!(matches!(
            result,
            Err(ReplaceError::Lifecycle(EngineError::InvalidArgument { .. }))
        ));
        assert_eq!(engine.generation(), generation);
        assert_eq!(
            random.draws.load(std::sync::atomic::Ordering::SeqCst),
            draws,
            "R6.6 RNG count"
        );
        let mut after = Vec::new();
        std::io::Read::read_to_end(&mut held_candidate, &mut after).unwrap();
        assert_eq!(after, original_candidate);
        // Stage ownership may clean up the rejected file; every other byte,
        // including the sidecar and intent, must be exactly as before.
        assert_eq!(durable_files(), before);
        if initial {
            assert!(!live.exists());
            assert!(!dir.path().join("meguri-sidecar.json").exists());
            assert!(DeviceState::for_collection(&live).publication().unwrap().is_none());
        }
    }
}

#[test]
fn pending_decision_reason_roundtrips_restore_colpkg_and_shared_manual_schema_only() {
    for (reason, stored, typed) in [
        (ReplaceReason::Restore, "restore", PendingDecisionReason::Restore),
        (
            ReplaceReason::ColpkgImport,
            "colpkgImport",
            PendingDecisionReason::ColpkgImport,
        ),
    ] {
        let h = Harness::open();
        assert_reason(&h.path, None);
        let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
        slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |_| Ok(()))
            .unwrap();
        drop(slot);
        assert_eq!(r6_document(&h.path)["deviceState"]["pendingDecisionReason"], stored);
        assert_reason(&h.path, Some(typed));
        // Force a real read/modify/durable-write round trip of the shared state.
        DeviceState::for_collection(&h.path)
            .set_manual_transfer_pending(false)
            .unwrap();
        h.engine.close().unwrap();
        h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
        assert_reason(&h.path, Some(typed));
        h.engine.close().unwrap();
    }
    // Frozen manual-transfer operations are deliberately not exercised here.
    let manual: PendingDecisionReason = serde_json::from_str("\"manualImport\"").unwrap();
    assert_eq!(manual, PendingDecisionReason::ManualImport);
    assert_eq!(serde_json::to_string(&manual).unwrap(), "\"manualImport\"");
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        DeviceState::for_collection(&dir.path().join("collection.anki2"))
            .pending_decision_reason()
            .unwrap(),
        None
    );
}

#[test]
fn malformed_pending_decision_reason_errors_without_repairing_original_bytes() {
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!("futureReason"),
        serde_json::json!(17),
        serde_json::json!(false),
        serde_json::json!({}),
    ] {
        let h = Harness::open();
        seed_cloud_prior(&h.engine, &h.path, "restore");
        h.engine.close().unwrap();
        let sidecar = h.dir.path().join("meguri-sidecar.json");
        let mut document = r6_document(&h.path);
        document["deviceState"]["pendingDecisionReason"] = invalid;
        fs::write(&sidecar, serde_json::to_vec_pretty(&document).unwrap()).unwrap();
        let original = fs::read(&sidecar).unwrap();
        let collection_bytes = fs::read(&h.path).unwrap();
        let device = DeviceState::for_collection(&h.path);
        assert!(device.pending_decision_reason().is_err());
        assert!(device.publication().is_err());
        assert!(device.set_manual_transfer_pending(false).is_err());
        assert!(h.engine.open_raw(&h.path, common::open_config(&h.clock)).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), original);
        assert_eq!(fs::read(&h.path).unwrap(), collection_bytes);
    }
}

#[test]
fn pre_swap_failure_or_cloud_cancellation_restores_exact_prior_reason_including_absence() {
    for prior in ["clear", "restore", "colpkg", "legacy"] {
        for (reason, cancel) in [
            (ReplaceReason::Restore, false),
            (ReplaceReason::ColpkgImport, false),
            (ReplaceReason::CloudAdopt, false),
            (ReplaceReason::CloudAdopt, true),
        ] {
            let h = Harness::open();
            seed_cloud_prior(&h.engine, &h.path, prior);
            let before = publication(&h.path);
            let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            CLOUD_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
            faultpoint::arm(
                "replace.after_pause_recorded",
                if cancel {
                    Fault::Callback(|| CLOUD_CANCEL.store(true, std::sync::atomic::Ordering::SeqCst))
                } else {
                    Fault::IoError(std::io::ErrorKind::Other)
                },
            );
            let result = if reason == ReplaceReason::CloudAdopt {
                let mut candidate = cloud_stage(h.dir.path());
                // Existing production cancellation seam, flipped only after
                // the real intent is durable. No test-owned recovery policy.
                if cancel {
                    candidate.media_cancel = Some(Arc::new(|| CLOUD_CANCEL.load(std::sync::atomic::Ordering::SeqCst)));
                }
                exclusive.replace_verified_cloud_adopt(candidate, cloud_input(true))
            } else {
                exclusive.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |_| Ok(()))
            };
            faultpoint::disarm_all();
            assert!(result.is_err());
            if cancel {
                assert!(matches!(result, Err(ReplaceError::Lifecycle(EngineError::Cancelled))));
            }
            assert_eq!(publication(&h.path), before);
            assert_reason(&h.path, prior_reason(prior));
            CLOUD_CANCEL.store(false, std::sync::atomic::Ordering::SeqCst);
        }
    }
}
static CLOUD_CANCEL: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
fn manual_import_fences_export_preserves_and_neither_stamps_packages() {
    for reason in [ReplaceReason::FullDownload, ReplaceReason::FullUpload] {
        for pending in [false, true] {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            let engine = Engine::new(EngineConfig::default()).unwrap();
            engine.open_raw(&live, config(binding(), 7)).unwrap();
            if pending {
                let slot = engine.acquire_slot(LongOp::Restore).unwrap();
                slot.replace_verified(stage(dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| Ok(()))
                    .unwrap();
            }
            let device = DeviceState::for_collection(&live);
            let prior = publication(&live);
            let token = if pending {
                assert!(device.publication_token(&binding()).is_err());
                None
            } else {
                Some(device.publication_token(&binding()).unwrap())
            };
            let candidate = stage(dir.path(), "schema-18.anki2");
            let db = meguri_storage::raw::PlainDatabase::open(&candidate.path()).unwrap();
            db.execute_batch(&format!(
                "UPDATE col SET mod={}, scm={}, ls=19, usn=23",
                NOW_MS - 5000,
                NOW_MS - 6000
            ))
            .unwrap();
            drop(db);
            let subscription = engine.stream().subscribe();
            let slot = engine.acquire_slot(LongOp::OneWaySync).unwrap();
            let new = slot
                .replace_verified(candidate, reason, |scope| {
                    let mut col = scope.col()?;
                    assert_eq!(col.modified.0, NOW_MS - 5000);
                    assert_eq!(col.schema_modified.0, NOW_MS - 6000);
                    assert_eq!(col.last_sync.0, 19);
                    if reason == ReplaceReason::FullDownload {
                        col.last_sync = col.modified;
                    }
                    scope.put_col(&col)?;
                    Ok(())
                })
                .unwrap();
            let active = publication(&live);
            assert_eq!(
                active.decision,
                if pending || reason == ReplaceReason::FullDownload {
                    PublicationDecision::Pending
                } else {
                    PublicationDecision::Clear
                }
            );
            assert_eq!(active.binding, binding());
            assert_ne!(active.identity, prior.identity);
            if let Some(token) = token {
                assert!(device.validate_token(&token, &binding()).is_err());
            }
            if active.decision == PublicationDecision::Clear {
                let fresh = device.publication_token(&binding()).unwrap();
                device.validate_token(&fresh, &binding()).unwrap();
            } else {
                assert!(device.publication_token(&binding()).is_err());
            }
            let col = new.reader(ReaderRole::Query).unwrap().col().unwrap();
            assert_eq!(col.modified.0, NOW_MS - 5000);
            assert_eq!(col.schema_modified.0, NOW_MS - 6000);
            assert_eq!(
                col.last_sync.0,
                if reason == ReplaceReason::FullDownload {
                    NOW_MS - 5000
                } else {
                    19
                }
            );
            assert_eq!(col.usn.0, 23);
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
            device.set_manual_transfer_pending(true).unwrap();
            device.set_manual_transfer_pending(false).unwrap();
            assert_eq!(publication(&live), active);
            drop(slot);
            engine.close().unwrap();
            for _ in 0..2 {
                let reopened = engine.open_raw(&live, config(binding(), 7)).unwrap();
                assert_eq!(publication(&live), active);
                assert_eq!(reopened.reader(ReaderRole::Query).unwrap().col().unwrap(), col);
                engine.close().unwrap();
            }
        }
    }
}

#[test]
fn initial_manual_download_is_pending_bound_and_keeps_server_stamps() {
    for binding in [binding(), AccountBinding::LocalOnly] {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("collection.anki2");
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let slot = engine.acquire_slot(LongOp::OneWaySync).unwrap();
        let mut stamps = None;
        let new = slot
            .replace_verified_initial(
                &live,
                config(binding.clone(), 7),
                stage(dir.path(), "schema-18.anki2"),
                ReplaceReason::FullDownload,
                |scope| {
                    let mut col = scope.col()?;
                    assert_ne!(col.modified.0, NOW_MS);
                    assert_ne!(col.schema_modified.0, NOW_MS);
                    col.last_sync = col.modified;
                    stamps = Some((col.modified, col.schema_modified));
                    scope.put_col(&col)?;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(new.generation(), Generation(1));
        let col = new.reader(ReaderRole::Query).unwrap().col().unwrap();
        assert_eq!((col.modified, col.schema_modified), stamps.unwrap());
        assert_eq!(col.last_sync, col.modified);
        let active = publication(&live);
        assert_eq!(active.binding, binding);
        assert_eq!(active.decision, PublicationDecision::Pending);
        assert!(DeviceState::for_collection(&live).publication_token(&binding).is_err());
        drop(slot);
        engine.close().unwrap();
        for _ in 0..2 {
            engine.open_raw(&live, config(binding.clone(), 7)).unwrap();
            assert_eq!(publication(&live), active);
            engine.close().unwrap();
        }
    }
}

#[test]
fn wrong_slot_and_bad_candidate_never_close_or_mutate_live_collection() {
    for reason in FENCED_REASONS {
        let h = Harness::open();
        let generation = h.collection.generation();
        let prior = publication(&h.path);
        let slot = h.engine.acquire_slot(LongOp::CheckDatabase).unwrap();
        assert!(matches!(
            slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |_| Ok(())),
            Err(ReplaceError::NotExclusive)
        ));
        drop(slot);
        let bytes = fs::read(&h.path).unwrap();
        let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
        let bad = stage(h.dir.path(), "ver-10-delete.anki2");
        let owned = bad.directory().to_owned();
        assert!(matches!(
            slot.replace_verified(bad, reason, |_| panic!("refused candidate reached caller")),
            Err(ReplaceError::Staging(StagingError::StagingRefused(_)))
        ));
        assert!(!owned.exists());
        assert_eq!(h.engine.generation(), generation);
        assert_eq!(publication(&h.path), prior);
        assert_eq!(fs::read(&h.path).unwrap(), bytes);
        assert!(h.collection.reader(ReaderRole::Query).is_ok());
    }
}

#[test]
fn pre_rename_faults_preserve_old_bytes_and_exact_clear_or_pending_decision() {
    for reason in FENCED_REASONS {
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
                let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
                faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
                let result = slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
                    download_change(reason, scope)
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
}

#[test]
fn post_rename_errors_are_honest_and_fenced_before_writer_reopens() {
    for reason in FENCED_REASONS {
        for point in [
            "replace.after_rename",
            "replace.after_dir_sync",
            "replace.after_durable_activation",
        ] {
            let h = Harness::open();
            let prior = publication(&h.path);
            let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
            let result = exclusive.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
                download_change(reason, scope)?;
                let mut col = scope.col()?;
                col.dirty = 722;
                scope.put_col(&col)?;
                Ok(())
            });
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
}

#[test]
fn a_failed_intent_durable_save_does_not_rename_or_destroy_evidence() {
    for reason in FENCED_REASONS {
        let h = Harness::open();
        let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
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
        let result = exclusive.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
            download_change(reason, scope)
        });
        faultpoint::disarm_all();
        assert!(result.is_err());
        assert_eq!(publication(&h.path), old);
        assert_eq!(h.engine.generation(), Generation(1));
    }
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
            ReplaceReason::FullUpload,
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
    let slot = engine.acquire_slot(LongOp::OneWaySync).unwrap();
    slot.replace_verified(
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
    assert!(device.validate_token(&manual_token, &binding()).is_err());
    assert!(device.validate_token(&token, &binding()).is_err());
    device.set_manual_transfer_pending(true).unwrap();
    device.set_manual_transfer_pending(false).unwrap();
    assert_eq!(publication(&live).decision, PublicationDecision::Pending);
    assert!(device.publication_token(&binding()).is_err());
    assert!(device.validate_token(&token, &AccountBinding::Unknown).is_err());
}

#[test]
fn verified_import_and_transfers_refuse_review_at_both_entries_without_mutation() {
    use meguri_engine::SuspendParticipant;
    let h = session_support::Harness::new(3);
    let (id, _) = h.start();
    h.review.flush().unwrap();
    let live = h.dir.path().join("collection.anki2");
    let prior = publication(&live);
    let generation = h.engine.generation();
    let before = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let bytes = fs::read(&live).unwrap();
    let wal = fs::read(live.with_extension("anki2-wal")).unwrap();
    for reason in [
        ReplaceReason::ColpkgImport,
        ReplaceReason::FullDownload,
        ReplaceReason::FullUpload,
    ] {
        for late in [false, true] {
            let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
            let candidate = stage(h.dir.path(), "schema-18.anki2");
            let result = if late {
                // Bypass the early convenience entry; the drained exclusive
                // seam must independently refuse the still-active session.
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive.replace_verified(candidate, reason, |_| panic!("active review reached preparation"))
            } else {
                assert!(matches!(
                    slot.check_verified_review(reason),
                    Err(EngineError::SessionActive)
                ));
                slot.replace_verified(candidate, reason, |_| panic!("active review reached preparation"))
            };
            assert!(matches!(
                result,
                Err(ReplaceError::Lifecycle(EngineError::SessionActive))
            ));
            assert_eq!(h.review.active_session(), Some(id));
            assert_eq!(h.engine.generation(), generation);
            assert_eq!(publication(&live), prior);
            assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().col().unwrap(), before);
            assert_eq!(fs::read(&live).unwrap(), bytes);
            assert_eq!(fs::read(live.with_extension("anki2-wal")).unwrap(), wal);
        }
    }
}

#[test]
fn cloud_adopt_refuses_active_review_at_check_and_typed_exclusive_without_mutation() {
    use meguri_engine::{OpSpec, SuspendParticipant, WalCoverage};
    use meguri_storage::{OpenError, StampMode, lockfile::CollectionLock};

    let mut h = session_support::Harness::new(3);
    let live = h.dir.path().join("collection.anki2");
    // Seed a real pending Restore decision without losing the review fixture:
    // save closed bytes, then stage them after reopen's sweep.
    h.engine.close().unwrap();
    let saved_bytes = fs::read(&live).unwrap();
    (h.engine, h.collection, h.review) = session_support::open(&live, h.clock.clone());
    let restored = ReplacementStage::new(h.dir.path()).unwrap();
    fs::write(restored.path(), saved_bytes).unwrap();
    let restore = h.engine.acquire_slot(LongOp::Restore).unwrap();
    h.collection = restore
        .replace_verified(restored, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    drop(restore);
    assert_eq!(publication(&live).decision, PublicationDecision::Pending);
    assert_reason(&live, Some(PendingDecisionReason::Restore));

    let (id, _) = h.start();
    h.review.flush().unwrap();
    h.collection
        .run(
            OpSpec::named("cloud adoption refusal barrier", OpSource::Rollover).stamp(StampMode::Raw),
            |_| Ok(()),
        )
        .unwrap();
    let head = h.review.head(id, 3).unwrap();
    assert!(!head.items.is_empty());
    // A completed op/flush is not a WAL receipt. Take the snapshot first,
    // wait for its actual coverage, then capture bytes; release all reader
    // leases before the exclusive drain.
    let (before, identity, seq) = {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        (snapshot.col().unwrap(), snapshot.wal_identity(), snapshot.seq())
    };
    match h
        .collection
        .wait_wal_covered(identity, seq, Duration::from_secs(1))
        .unwrap()
    {
        WalCoverage::Covered(receipt) => {
            assert_eq!(receipt.identity(), identity);
            assert!(receipt.through() >= seq);
        }
        other => panic!("active-review fixture lacks durable WAL coverage: {other:?}"),
    }
    let bytes = fs::read(&live).unwrap();
    let wal = fs::read(live.with_extension("anki2-wal")).unwrap();
    let sidecar = h.dir.path().join("meguri-sidecar.json");
    let sidecar_bytes = fs::read(&sidecar).unwrap();
    let prior = publication(&live);
    let generation = h.engine.generation();
    let ready = h.engine.state();
    assert_eq!(ready, meguri_engine::LifecycleState::Ready { generation });
    let slot = h.engine.acquire_slot(LongOp::CloudAdopt).unwrap();
    let candidate = cloud_stage(h.dir.path());
    let candidate_path = candidate.path();
    let candidate_bytes = fs::read(&candidate_path).unwrap();
    let mut held_candidate = fs::File::open(&candidate_path).unwrap();
    let input = cloud_input(false);
    assert_ne!(input.candidate.identity, prior.identity);
    assert_ne!(input.candidate.decision, prior.decision);

    // V14.1 and V14.2 step 7: wait in Ready with the L5 slot held,
    // outside L7's exclusive phase, while the real session remains active.
    assert_eq!(
        slot.check_verified_review(ReplaceReason::CloudAdopt),
        Err(EngineError::SessionActive)
    );
    assert_eq!(h.engine.state(), ready);
    assert_eq!(h.review.active_session(), Some(id));
    assert_eq!(h.review.head(id, 3).unwrap(), head);
    assert_eq!(h.engine.generation(), generation);
    assert!(matches!(
        CollectionLock::acquire(h.dir.path()),
        Err(OpenError::OpenInAnotherProgram)
    ));
    assert!(matches!(
        h.engine.acquire_slot(LongOp::Restore),
        Err(EngineError::Busy { running_op }) if running_op == LongOp::CloudAdopt.name()
    ));
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().col().unwrap(), before);
    assert_eq!(fs::read(&live).unwrap(), bytes);
    assert_eq!(fs::read(live.with_extension("anki2-wal")).unwrap(), wal);
    assert_eq!(fs::read(&candidate_path).unwrap(), candidate_bytes);
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_bytes);
    assert_eq!(publication(&live), prior);
    assert_reason(&live, Some(PendingDecisionReason::Restore));

    // Bypass the early check to exercise the typed production seam itself;
    // CloudAdopt must never inherit Restore's active-session replacement.
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    let result = exclusive.replace_verified_cloud_adopt(candidate, input);
    assert!(matches!(
        result,
        Err(ReplaceError::Lifecycle(EngineError::SessionActive))
    ));
    assert_eq!(exclusive.collection().generation(), generation);
    assert_eq!(exclusive.collection().path(), live);
    drop(exclusive);
    assert_eq!(h.engine.state(), ready);
    assert_eq!(h.review.active_session(), Some(id));
    assert_eq!(h.review.head(id, 3).unwrap(), head);
    assert_eq!(h.engine.generation(), generation);
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().col().unwrap(), before);
    assert!(matches!(
        CollectionLock::acquire(h.dir.path()),
        Err(OpenError::OpenInAnotherProgram)
    ));
    assert!(matches!(
        h.engine.acquire_slot(LongOp::Restore),
        Err(EngineError::Busy { running_op }) if running_op == LongOp::CloudAdopt.name()
    ));
    assert_eq!(fs::read(&live).unwrap(), bytes);
    assert_eq!(fs::read(live.with_extension("anki2-wal")).unwrap(), wal);
    // The entry consumes the stage and may unlink it on refusal. Its held
    // file must retain the original bytes even after that owned cleanup.
    let mut candidate_after = Vec::new();
    std::io::Read::read_to_end(&mut held_candidate, &mut candidate_after).unwrap();
    assert_eq!(candidate_after, candidate_bytes);
    if candidate_path.exists() {
        assert_eq!(fs::read(&candidate_path).unwrap(), candidate_bytes);
    }
    assert_eq!(fs::read(&sidecar).unwrap(), sidecar_bytes);
    assert_eq!(publication(&live), prior);
    assert_reason(&live, Some(PendingDecisionReason::Restore));
    drop(slot);
    assert_eq!(h.engine.state(), ready);
    assert_eq!(h.review.active_session(), Some(id));
    let next = h.engine.acquire_slot(LongOp::Restore).unwrap();
    drop(next);
}

#[test]
fn restore_and_legacy_replace_still_support_active_session() {
    for verified in [false, true] {
        let h = session_support::Harness::new(3);
        h.start();
        let live = h.dir.path().join("collection.anki2");
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        assert!(slot.check_verified_review(ReplaceReason::Restore).is_ok());
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        if verified {
            exclusive
                .replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
                    Ok(())
                })
                .unwrap();
        } else {
            let source = h.dir.path().join("low-level-replacement.anki2");
            fs::copy(common::fixture("schema-18.anki2"), &source).unwrap();
            exclusive
                .replace(move |path| fs::rename(source, path).map_err(|e| EngineError::Storage(e.into())))
                .unwrap();
        }
        assert!(h.review.active_session().is_none());
        assert_eq!(
            publication(&live).decision,
            if verified {
                PublicationDecision::Pending
            } else {
                PublicationDecision::Clear
            }
        );
    }
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
    let reason: ReplaceReason = serde_json::from_str(&std::env::var("MEGURI_REPLACE_REASON").unwrap()).unwrap();
    let cloud = reason == ReplaceReason::CloudAdopt;
    let cloud_prior = if cloud {
        mode.split('-').nth(1).unwrap()
    } else {
        "clear"
    };
    let engine = Engine::new(EngineConfig::default()).unwrap();
    if mode == "raw-recover" {
        faultpoint::arm(&point, Fault::Abort);
        let _ = engine.open_raw(&live, config(binding(), 991));
        panic!("recovery fault point was not reached: {point}");
    }
    let raw = mode == "raw" || mode == "raw-identical";
    let initial = mode == "absent" || mode == "identical" || raw || (cloud && cloud_prior == "absent");
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
    if cloud && !initial {
        seed_cloud_prior(&engine, &live, cloud_prior);
        engine.close().unwrap();
        fs::copy(&live, live.with_extension("cloud-old-before")).unwrap();
        engine.open_raw(&live, config(binding(), 992)).unwrap();
    }
    if let Some(prior) = DeviceState::for_collection(&live).publication().unwrap() {
        fs::write(directory.join("prior.json"), serde_json::to_vec(&prior).unwrap()).unwrap();
    }
    let candidate = if cloud {
        cloud_stage(directory)
    } else {
        stage(
            directory,
            if mode == "older" {
                "schema-11.anki2"
            } else {
                "schema-18.anki2"
            },
        )
    };
    *PATHS.lock().unwrap() = Some((live.clone(), candidate.path()));
    faultpoint::arm("replace.after_pause_recorded", Fault::Callback(capture_intent_boundary));
    if point != "replace.after_pause_recorded" {
        faultpoint::arm(&point, Fault::Abort);
    }
    if mode == "identical" || mode == "raw-identical" {
        faultpoint::arm("replace.after_finalise", Fault::Callback(make_identical_before_intent));
    }
    let slot = engine.acquire_slot(replacement_op(reason)).unwrap();
    if cloud {
        let input = cloud_input(mode.ends_with("-local"));
        if initial {
            slot.replace_verified_cloud_adopt_initial(&live, config(binding(), 991), candidate, input)
                .unwrap();
        } else {
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            exclusive.replace_verified_cloud_adopt(candidate, input).unwrap();
        }
        panic!("CloudAdopt fault point was not reached: {point}");
    }
    let change = |scope: &mut meguri_storage::WriteScope<'_>| -> Result<(), StorageError> {
        download_change(reason, scope)?;
        let mut col = scope.col()?;
        col.dirty = 722;
        scope.put_col(&col)?;
        Ok(())
    };
    if raw {
        let backups = directory.parent().unwrap().join("raw-backups");
        fs::create_dir_all(&backups).unwrap();
        slot.restore_verified_initial_before_commit(&live, config(binding(), 991), candidate, || {
            let main = backups.join("retained-main.anki2");
            let wal = backups.join("retained-main.anki2-wal");
            fs::copy(&live, &main).map_err(StorageError::from)?;
            fs::copy(live.with_file_name("collection.anki2-wal"), &wal).map_err(StorageError::from)?;
            meguri_engine::device_state::RawRestoreAuthority::verify(&live, &main, &wal)
                .map(Some)
                .map_err(Into::into)
        })
        .unwrap();
    } else if initial {
        slot.replace_verified_initial(&live, config(binding(), 991), candidate, reason, change)
            .unwrap();
    } else {
        slot.replace_verified(candidate, reason, change).unwrap();
    }
    panic!("fault point was not reached: {point}");
}
fn crash(live: &Path, point: &str, mode: &str, reason: ReplaceReason) {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "replacement_crash_worker", "--nocapture"])
        .env("MEGURI_REPLACE_LIVE", live)
        .env("MEGURI_REPLACE_POINT", point)
        .env("MEGURI_REPLACE_MODE", mode)
        .env("MEGURI_REPLACE_REASON", serde_json::to_string(&reason).unwrap())
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(6),
        "worker must abort at the real fault boundary: {point}"
    );
}

const CLOUD_CRASH_POINTS: [&str; 7] = [
    "replace.after_check",
    "replace.after_finalise",
    "replace.after_pause_recorded",
    "replace.after_live_wal_removed",
    "replace.after_rename",
    "replace.after_dir_sync",
    "replace.after_durable_activation",
];

fn cloud_swapped(point: &str) -> bool {
    matches!(
        point,
        "replace.after_rename" | "replace.after_dir_sync" | "replace.after_durable_activation"
    )
}

fn assert_recorded_file_proof(live: &Path, intent: &serde_json::Value, candidate: bool) {
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    let bytes = fs::read(live).unwrap();
    let file_key = if candidate { "candidate_file" } else { "old_file" };
    let native_key = if candidate { "candidate_native" } else { "old_native" };
    assert_eq!(intent[file_key]["length"].as_u64().unwrap(), bytes.len() as u64);
    let digest: [u8; 32] = Sha256::digest(&bytes).into();
    assert_eq!(intent[file_key]["sha256"], serde_json::to_value(digest).unwrap());
    let native = fs::symlink_metadata(live).unwrap();
    assert_eq!(intent[native_key]["device"].as_u64().unwrap(), native.dev());
    assert_eq!(intent[native_key]["inode"].as_u64().unwrap(), native.ino());
}

fn recover_cloud_twice(
    live: &Path,
    expected: &Publication,
    reason: Option<PendingDecisionReason>,
    expected_marker: i64,
) {
    for _ in 0..2 {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine.open_raw(live, config(binding(), 144)).unwrap();
        assert_eq!(&publication(live), expected);
        assert_reason(live, reason);
        assert!(r6_document(live)["deviceState"]["intent"].is_null());
        assert_eq!(marker(&collection), expected_marker);
        if expected_marker == 946 {
            assert_cloud_stamps(&collection);
        }
        engine.close().unwrap();
        assert!(meguri_storage::integrity::integrity_check(live).unwrap().is_ok());
    }
}

fn retirement_config() -> meguri_engine::OpenConfig {
    let mut open = config(AccountBinding::LocalOnly, 149);
    open.retire_icloud_publication = true;
    open.startup_publication_binding = Some(binding());
    open
}

fn recover_retired_twice(
    live: &Path,
    expected: &Publication,
    reason: Option<PendingDecisionReason>,
    expected_marker: i64,
) {
    for _ in 0..2 {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine.open_raw(live, retirement_config()).unwrap();
        assert_eq!(&publication(live), expected);
        assert_reason(live, reason);
        assert_eq!(marker(&collection), expected_marker);
        assert!(r6_document(live)["deviceState"]["intent"].is_null());
        assert_eq!(
            DeviceState::for_collection(live).settled_publication().unwrap().as_ref(),
            Some(expected)
        );
        engine.close().unwrap();
    }
}

#[test]
fn icloud_retirement_stable_connected_publication_requires_opt_in_and_preserves_archive() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    let expected = publication(&live);
    let sidecar = dir.path().join("meguri-sidecar.json");
    let original = fs::read(&sidecar).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let ordinary = config(AccountBinding::LocalOnly, 8);
    assert!(!ordinary.retire_icloud_publication);
    assert!(engine.open_raw(&live, ordinary).is_err());
    assert_eq!(fs::read(&sidecar).unwrap(), original);
    recover_retired_twice(&live, &expected, None, 0);
    assert_eq!(r6_document(&live)["deviceState"]["active"], serde_json::to_value(&expected).unwrap());
    // Tokens belong only to explicit Connected authority. An archival open
    // must never manufacture a LocalOnly publication token for the app.
    assert!(DeviceState::for_collection(&live).publication_token(&binding()).is_ok());
    assert!(DeviceState::for_collection(&live).publication_token(&AccountBinding::LocalOnly).is_err());
}

#[test]
fn icloud_retirement_uninitialized_collection_uses_local_ownership_without_account_input() {
    let (_dir, live) = common::copy_fixture("schema-18.anki2");
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let mut open = retirement_config();
    open.publication_binding = binding();
    open.startup_publication_binding = Some(AccountBinding::Unknown);
    open.startup_identity_read_failed = true;
    engine.open_raw(&live, open).unwrap();
    let expected = publication(&live);
    assert_eq!(expected.binding, AccountBinding::LocalOnly);
    assert_eq!(expected.decision, PublicationDecision::Clear);
    engine.close().unwrap();
    recover_retired_twice(&live, &expected, None, 0);
}

#[test]
fn icloud_retirement_regular_replacements_recover_exact_local_decision_and_reason() {
    for reason in [
        ReplaceReason::Restore,
        ReplaceReason::ColpkgImport,
        ReplaceReason::FullDownload,
        ReplaceReason::FullUpload,
    ] {
        for mode in ["clear", "pending"] {
            for point in [
                "replace.after_pause_recorded",
                "replace.after_rename",
                "replace.after_durable_activation",
            ] {
                let (_dir, live) = common::copy_fixture("schema-18.anki2");
                settled_old(&live);
                crash(&live, point, mode, reason);
                let before = r6_document(&live);
                let swapped = cloud_swapped(point);
                let intent = &before["deviceState"]["intent"];
                assert_recorded_file_proof(&live, intent, swapped);
                let expected: Publication =
                    serde_json::from_value(intent[if swapped { "candidate" } else { "prior" }].clone()).unwrap();
                let expected_reason = if swapped && reason != ReplaceReason::FullUpload {
                    Some(match reason {
                        ReplaceReason::Restore => PendingDecisionReason::Restore,
                        ReplaceReason::ColpkgImport => PendingDecisionReason::ColpkgImport,
                        ReplaceReason::FullDownload => PendingDecisionReason::ManualImport,
                        _ => unreachable!(),
                    })
                } else if mode == "pending" {
                    Some(PendingDecisionReason::Restore)
                } else {
                    None
                };
                recover_retired_twice(
                    &live,
                    &expected,
                    expected_reason,
                    if swapped || mode == "pending" { 722 } else { 0 },
                );
                if swapped {
                    let after = r6_document(&live);
                    assert_eq!(after["deviceState"]["lastReplacement"]["candidate"], intent["candidate"]);
                    assert_eq!(after["deviceState"]["lastReplacement"]["prior"], intent["prior"]);
                }
            }
        }
    }
}

#[test]
fn icloud_retirement_cloud_adopt_recovers_pre_and_post_rename_without_live_account() {
    for mode in ["cloud-clear-bound", "cloud-restore-bound", "cloud-colpkg-local"] {
        for point in [
            "replace.after_pause_recorded",
            "replace.after_rename",
            "replace.after_durable_activation",
        ] {
            let (_dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(&live, point, mode, ReplaceReason::CloudAdopt);
            let before = r6_document(&live);
            let intent = &before["deviceState"]["intent"];
            let swapped = cloud_swapped(point);
            assert_recorded_file_proof(&live, intent, swapped);
            let mut expected: Publication =
                serde_json::from_value(intent[if swapped { "candidate" } else { "prior" }].clone()).unwrap();
            if swapped && point == "replace.after_rename" && !mode.ends_with("-local") {
                // A new offline activation cannot use its archival Connected
                // binding as live authority. Already activated decisions stay.
                expected.decision = PublicationDecision::Pending;
            }
            let prior_kind = mode.split('-').nth(1).unwrap();
            recover_retired_twice(
                &live,
                &expected,
                if swapped { None } else { prior_reason(prior_kind) },
                if swapped {
                    946
                } else if prior_kind == "clear" {
                    0
                } else {
                    722
                },
            );
            if swapped {
                let after = r6_document(&live);
                assert_eq!(
                    after["deviceState"]["cloudAdoptActivation"]["replacement"]["candidate"],
                    intent["candidate"]
                );
                assert_eq!(after["deviceState"]["lastReplacement"]["candidate"], intent["candidate"]);
                if point == "replace.after_durable_activation" {
                    assert_eq!(after["deviceState"]["replacements"], before["deviceState"]["replacements"]);
                    assert_eq!(
                        after["deviceState"]["cloudAdoptActivation"],
                        before["deviceState"]["cloudAdoptActivation"]
                    );
                }
            }
        }
    }
}

#[test]
fn icloud_retirement_settled_cloud_account_fence_and_archive_are_preserved() {
    let (_dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    crash(&live, "replace.after_rename", "cloud-restore-bound", ReplaceReason::CloudAdopt);
    let engine = Engine::new(EngineConfig::default()).unwrap();
    // An earlier launch without the removal flag fenced this exact adoption.
    engine.open_raw(&live, config(AccountBinding::LocalOnly, 80)).unwrap();
    assert_eq!(publication(&live).decision, PublicationDecision::Pending);
    engine.close().unwrap();
    let sidecar = live.parent().unwrap().join("meguri-sidecar.json");
    let before = fs::read(&sidecar).unwrap();
    let expected = publication(&live);
    recover_retired_twice(&live, &expected, None, 946);
    assert_eq!(fs::read(&sidecar).unwrap(), before);
}

#[test]
fn icloud_retirement_finished_adoption_preserves_exact_archive_content_for_clear_and_pending() {
    for pending in [false, true] {
        let (_dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, "replace.after_rename", "cloud-restore-bound", ReplaceReason::CloudAdopt);
        let engine = Engine::new(EngineConfig::default()).unwrap();
        engine
            .open_raw(&live, config(if pending { AccountBinding::Unknown } else { binding() }, 80))
            .unwrap();
        engine.close().unwrap();
        let expected = publication(&live);
        assert_eq!(expected.binding, binding());
        assert_eq!(
            expected.decision,
            if pending { PublicationDecision::Pending } else { PublicationDecision::Clear }
        );
        let archive = r6_document(&live);
        for _ in 0..2 {
            let engine = Engine::new(EngineConfig::default()).unwrap();
            let collection = engine.open_raw(&live, retirement_config()).unwrap();
            assert_eq!(publication(&live), expected);
            assert_reason(&live, None);
            assert_eq!(marker(&collection), 946);
            assert!(r6_document(&live)["deviceState"]["intent"].is_null());
            assert_eq!(
                DeviceState::for_collection(&live).settled_publication().unwrap().as_ref(),
                Some(&expected)
            );
            engine.close().unwrap();
            assert_eq!(r6_document(&live), archive);
        }
    }
}

#[test]
fn icloud_retirement_keeps_connected_local_replacement_successor_chain_before_and_after_open() {
    let (dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    let before = publication(&live).identity;
    let engine = Engine::new(EngineConfig::default()).unwrap();
    engine.open_raw(&live, config(binding(), 155)).unwrap();
    let mut predecessors = vec![before];
    for _ in 0..2 {
        let slot = engine.acquire_slot(LongOp::Restore).unwrap();
        slot.replace_verified(stage(dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| Ok(()))
            .unwrap();
        predecessors.push(publication(&live).identity);
    }
    engine.close().unwrap();
    let expected = publication(&live);
    let archive = r6_document(&live);
    let device = DeviceState::for_collection(&live);
    for predecessor in &predecessors[..2] {
        let proof = device.local_replacement_successor(*predecessor).unwrap().unwrap();
        assert_eq!(proof.before(), *predecessor);
        assert_eq!(proof.after(), expected.identity);
    }
    recover_retired_twice(&live, &expected, Some(PendingDecisionReason::Restore), 0);
    assert_eq!(r6_document(&live)["deviceState"], archive["deviceState"]);
    for predecessor in &predecessors[..2] {
        assert_eq!(
            device.local_replacement_successor(*predecessor).unwrap().unwrap().after(),
            expected.identity
        );
    }
    // A new local replacement must extend the same Connected archival chain.
    engine.open_raw(&live, retirement_config()).unwrap();
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage(dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    drop(slot);
    let successor = publication(&live);
    assert_eq!(successor.binding, binding());
    assert_ne!(successor.identity, expected.identity);
    for predecessor in predecessors {
        let proof = device.local_replacement_successor(predecessor).unwrap().unwrap();
        assert_eq!(proof.before(), predecessor);
        assert_eq!(proof.after(), successor.identity);
    }
    engine.close().unwrap();
    recover_retired_twice(&live, &successor, Some(PendingDecisionReason::Restore), 0);
    assert_eq!(
        device.local_replacement_successor(before).unwrap().unwrap().after(),
        successor.identity
    );
}

#[test]
fn icloud_retirement_absence_entry_settles_exact_initial_proof_without_allocating_identity() {
    for mode in ["cloud-absent-bound", "cloud-absent-local"] {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("collection.anki2");
        crash(&live, "replace.after_pause_recorded", mode, ReplaceReason::CloudAdopt);
        let before = r6_document(&live);
        let prior: Publication = serde_json::from_value(before["deviceState"]["intent"]["prior"].clone()).unwrap();
        assert!(before["deviceState"]["intent"]["old_file"].is_null());
        assert!(before["deviceState"]["intent"]["old_native"].is_null());
        assert_eq!(live.symlink_metadata().unwrap_err().kind(), std::io::ErrorKind::NotFound);
        let engine = Engine::new(EngineConfig::default()).unwrap();
        assert!(engine.reconcile_retired_icloud_absence_raw(&live, &config(AccountBinding::LocalOnly, 1)).is_err());
        assert_eq!(r6_document(&live), before);
        for _ in 0..2 {
            engine.reconcile_retired_icloud_absence_raw(&live, &retirement_config()).unwrap();
            assert_eq!(engine.state(), meguri_engine::LifecycleState::NoCollection);
            assert_eq!(engine.generation(), Generation(0));
            assert_eq!(publication(&live), prior);
            assert!(r6_document(&live)["deviceState"]["intent"].is_null());
            assert_eq!(live.symlink_metadata().unwrap_err().kind(), std::io::ErrorKind::NotFound);
        }
    }
    // A missing file without R6 evidence is never assigned a publication.
    let dir = tempfile::tempdir().unwrap();
    let live = dir.path().join("collection.anki2");
    let engine = Engine::new(EngineConfig::default()).unwrap();
    engine.reconcile_retired_icloud_absence_raw(&live, &retirement_config()).unwrap();
    assert!(DeviceState::for_collection(&live).publication().unwrap().is_none());
    assert!(!dir.path().join("meguri-sidecar.json").exists());
    // Unknown presence (including a dangling link) or surviving journal bytes
    // cannot be interpreted as absence and cannot acquire an identity.
    for damage in ["file", "symlink", "wal"] {
        let path = if damage == "wal" {
            live.with_file_name("collection.anki2-wal")
        } else {
            live.clone()
        };
        if damage == "symlink" {
            std::os::unix::fs::symlink("missing-collection", &path).unwrap();
        } else {
            fs::write(&path, b"unproven collection bytes").unwrap();
        }
        let before = recovery_files(dir.path());
        assert!(engine.reconcile_retired_icloud_absence_raw(&live, &retirement_config()).is_err());
        assert_eq!(recovery_files(dir.path()), before);
        assert!(DeviceState::for_collection(&live).publication().unwrap().is_none());
        fs::remove_file(path).unwrap();
    }
}

#[test]
fn icloud_retirement_unmatched_proof_fails_closed_and_keeps_all_recovery_bytes() {
    for reason in [ReplaceReason::Restore, ReplaceReason::CloudAdopt] {
        for point in ["replace.after_pause_recorded", "replace.after_rename"] {
            for damage in ["native", "bytes", "wal", "metadata"] {
                let (dir, live) = common::copy_fixture("schema-18.anki2");
                settled_old(&live);
                crash(
                    &live,
                    point,
                    if reason == ReplaceReason::CloudAdopt {
                        "cloud-restore-bound"
                    } else {
                        "clear"
                    },
                    reason,
                );
                match damage {
                    "native" => {
                        let bytes = fs::read(&live).unwrap();
                        fs::rename(&live, live.with_extension("held-native")).unwrap();
                        fs::write(&live, bytes).unwrap();
                    }
                    "bytes" => fs::write(&live, b"unmatched closed collection bytes").unwrap(),
                    "wal" => fs::write(live.with_file_name("collection.anki2-wal"), b"unsettled WAL").unwrap(),
                    "metadata" => fs::write(dir.path().join("meguri-sidecar.json"), b"{\"deviceState\":").unwrap(),
                    _ => unreachable!(),
                }
                let before = recovery_files(dir.path());
                let engine = Engine::new(EngineConfig::default()).unwrap();
                assert!(engine.open_raw(&live, retirement_config()).is_err());
                assert_eq!(recovery_files(dir.path()), before, "{reason:?}: {point}, {damage}");
            }
        }
    }
}

#[test]
fn cloud_adopt_kill_at_every_r8_boundary_recovers_proven_old_or_exact_candidate() {
    for mode in [
        "cloud-clear-bound",
        "cloud-restore-bound",
        "cloud-colpkg-local",
        "cloud-legacy-local",
    ] {
        for point in CLOUD_CRASH_POINTS {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(&live, point, mode, ReplaceReason::CloudAdopt);
            let prior: Publication = serde_json::from_slice(&fs::read(dir.path().join("prior.json")).unwrap()).unwrap();
            let expected = cloud_input(mode.ends_with("-local"));
            let document = r6_document(&live);
            let intent = &document["deviceState"]["intent"];
            if !intent.is_null() {
                assert_eq!(intent["operation"], serde_json::to_value(expected.operation).unwrap());
                assert_eq!(intent["reason"], "CloudAdopt");
                assert_eq!(intent["candidate"], serde_json::to_value(&expected.candidate).unwrap());
                assert_eq!(intent["prior"], serde_json::to_value(&prior).unwrap());
                assert_eq!(intent["candidate"]["decision"], "Clear");
                assert_recorded_file_proof(&live, intent, cloud_swapped(point));
                assert!(
                    DeviceState::for_collection(&live)
                        .publication_token(&binding())
                        .is_err()
                );
            }
            let prior_kind = mode.split('-').nth(1).unwrap();
            if cloud_swapped(point) {
                assert_eq!(
                    fs::read(&live).unwrap(),
                    fs::read(live.with_extension("candidate-final")).unwrap()
                );
                recover_cloud_twice(&live, &expected.candidate, None, 946);
            } else {
                assert_eq!(
                    fs::read(&live).unwrap(),
                    fs::read(live.with_extension("cloud-old-before")).unwrap()
                );
                recover_cloud_twice(
                    &live,
                    &prior,
                    prior_reason(prior_kind),
                    if prior_kind == "clear" { 0 } else { 722 },
                );
            }
        }
    }
}

#[test]
fn cloud_adopt_no_old_file_succeeds_and_recovers_each_real_crash_boundary() {
    for local_only in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let live = dir.path().join("collection.anki2");
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let slot = engine.acquire_slot(LongOp::CloudAdopt).unwrap();
        let new = slot
            .replace_verified_cloud_adopt_initial(
                &live,
                config(binding(), 7),
                cloud_stage(dir.path()),
                cloud_input(local_only),
            )
            .unwrap();
        assert_eq!(new.generation(), Generation(1));
        assert_eq!(publication(&live), cloud_input(local_only).candidate);
        assert_reason(&live, None);
        assert_cloud_stamps(&new);
        drop(slot);
        engine.close().unwrap();
        for point in CLOUD_CRASH_POINTS {
            let dir = tempfile::tempdir().unwrap();
            let live = dir.path().join("collection.anki2");
            crash(
                &live,
                point,
                if local_only {
                    "cloud-absent-local"
                } else {
                    "cloud-absent-bound"
                },
                ReplaceReason::CloudAdopt,
            );
            if cloud_swapped(point) {
                let document = r6_document(&live);
                let intent = &document["deviceState"]["intent"];
                assert!(intent["old_file"].is_null());
                assert!(intent["old_native"].is_null());
                assert_recorded_file_proof(&live, intent, true);
                recover_cloud_twice(&live, &cloud_input(local_only).candidate, None, 946);
            } else {
                assert!(!live.exists());
                let prior = DeviceState::for_collection(&live).publication().unwrap();
                for _ in 0..2 {
                    let engine = Engine::new(EngineConfig::default()).unwrap();
                    assert!(engine.open_raw(&live, config(binding(), 8)).is_err());
                    assert!(!live.exists());
                    assert_eq!(DeviceState::for_collection(&live).publication().unwrap(), prior);
                    if prior.is_some() {
                        assert_reason(&live, None);
                        assert!(r6_document(&live)["deviceState"]["intent"].is_null());
                    }
                }
            }
        }
    }
}

#[test]
fn cloud_adopt_startup_uses_actual_account_input_and_leaves_no_pending_reason_on_mismatch() {
    for point in [
        "replace.after_rename",
        "replace.after_dir_sync",
        "replace.after_durable_activation",
    ] {
        for local_only in [false, true] {
            let (_dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(
                &live,
                point,
                if local_only {
                    "cloud-restore-local"
                } else {
                    "cloud-restore-bound"
                },
                ReplaceReason::CloudAdopt,
            );
            // This is the actual platform input for this launch. Do not edit
            // saved account facts to make the comparison appear to happen.
            let changed_account = AccountBinding::Connected {
                account: "different-synthetic-account".into(),
                configuration: "test-app".into(),
                environment: "test".into(),
            };
            let expected = cloud_input(local_only).candidate;
            for _ in 0..2 {
                let engine = Engine::new(EngineConfig::default()).unwrap();
                let collection = engine.open_raw(&live, config(changed_account.clone(), 88)).unwrap();
                let active = publication(&live);
                assert_eq!(active.identity, expected.identity);
                assert_eq!(
                    active.decision,
                    if local_only {
                        PublicationDecision::Clear
                    } else {
                        PublicationDecision::Pending
                    }
                );
                if local_only {
                    assert_eq!(active.binding, AccountBinding::LocalOnly);
                } else {
                    assert_ne!(
                        active.binding, changed_account,
                        "authorization must not transfer to the current account"
                    );
                }
                assert_reason(&live, None);
                assert_eq!(marker(&collection), 946);
                assert_cloud_stamps(&collection);
                assert!(r6_document(&live)["deviceState"]["intent"].is_null());
                assert!(
                    DeviceState::for_collection(&live)
                        .publication_token(&changed_account)
                        .is_err()
                );
                engine.close().unwrap();
            }
        }
    }
}

#[test]
fn cloud_adopt_requires_both_native_and_finalized_byte_proof_before_resolving_an_intent() {
    for point in ["replace.after_pause_recorded", "replace.after_rename"] {
        for damage in ["native", "bytes", "metadata"] {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(&live, point, "cloud-restore-bound", ReplaceReason::CloudAdopt);
            match damage {
                "native" => {
                    let bytes = fs::read(&live).unwrap();
                    fs::rename(&live, live.with_extension("held-native")).unwrap();
                    fs::write(&live, bytes).unwrap();
                }
                "bytes" => fs::write(&live, b"neither finalized file identity").unwrap(),
                "metadata" => fs::write(dir.path().join("meguri-sidecar.json"), b"{\"deviceState\":").unwrap(),
                _ => unreachable!(),
            }
            let evidence = recovery_files(dir.path());
            let engine = Engine::new(EngineConfig::default()).unwrap();
            assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
            assert!(
                DeviceState::for_collection(&live)
                    .publication_token(&binding())
                    .is_err()
            );
            assert_eq!(
                recovery_files(dir.path()),
                evidence,
                "unproven recovery must retain its evidence"
            );
        }
    }
}

#[test]
fn cloud_adopt_activated_completion_requires_the_original_single_link_regular_inode() {
    use std::os::unix::fs::MetadataExt;

    for damage in ["absent", "directory", "symlink", "different-inode", "hardlink"] {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, "replace.after_durable_activation", "cloud-restore-bound", ReplaceReason::CloudAdopt);
        let before = r6_document(&live);
        let state = &before["deviceState"];
        assert_eq!(state["intent"]["phase"], "Activated");
        assert_eq!(state["cloudAdoptActivation"]["replacement"], state["lastReplacement"]);
        assert_recorded_file_proof(&live, &state["intent"], true);
        let original = fs::symlink_metadata(&live).unwrap();
        let held = live.with_extension("held-activation");
        fs::rename(&live, &held).unwrap();
        match damage {
            "absent" => {}
            "directory" => fs::create_dir(&live).unwrap(),
            "symlink" => std::os::unix::fs::symlink(&held, &live).unwrap(),
            "different-inode" => {
                fs::copy(&held, &live).unwrap();
                let substitute = fs::symlink_metadata(&live).unwrap();
                assert_ne!((substitute.dev(), substitute.ino()), (original.dev(), original.ino()));
                assert_eq!(fs::read(&live).unwrap(), fs::read(&held).unwrap());
            }
            "hardlink" => {
                fs::hard_link(&held, &live).unwrap();
                let linked = fs::symlink_metadata(&live).unwrap();
                assert_eq!((linked.dev(), linked.ino()), (original.dev(), original.ino()));
                assert_eq!(linked.nlink(), 2);
            }
            _ => unreachable!(),
        }
        let evidence = recovery_files(dir.path());
        let engine = Engine::new(EngineConfig::default()).unwrap();
        if damage == "absent" {
            // Exercise the host's exact absence entry too: it must not consume
            // this already activated journal when its candidate has vanished.
            assert!(engine.reconcile_retired_icloud_absence_raw(&live, &retirement_config()).is_err());
            assert_eq!(recovery_files(dir.path()), evidence);
            assert_eq!(r6_document(&live), before);
        }
        let error = engine.open_raw(&live, retirement_config()).unwrap_err();
        if damage != "absent" {
            // open_raw reconciles under ownership before the storage open gate.
            // In particular directories and symlinks must reach this R6 check.
            assert!(
                matches!(&error, EngineError::Storage(StorageError::Io { detail })
                    if detail.contains("activated candidate native identity differs from R6 proof")),
                "{damage}: {error}"
            );
        }
        assert_eq!(recovery_files(dir.path()), evidence, "{damage}");
        assert_eq!(r6_document(&live), before, "{damage}");
        match damage {
            "absent" => assert_eq!(live.symlink_metadata().unwrap_err().kind(), std::io::ErrorKind::NotFound),
            "directory" => {
                assert!(live.symlink_metadata().unwrap().is_dir());
                fs::remove_dir(&live).unwrap();
            }
            _ => fs::remove_file(&live).unwrap(),
        }
        drop(engine);
        fs::rename(&held, &live).unwrap();
        let restored = fs::symlink_metadata(&live).unwrap();
        assert_eq!((restored.dev(), restored.ino()), (original.dev(), original.ino()));
        assert_eq!(restored.nlink(), 1);
        let expected: Publication = serde_json::from_value(state["active"].clone()).unwrap();
        recover_retired_twice(&live, &expected, None, 946);
        let after = r6_document(&live);
        assert_eq!(after["deviceState"]["cloudAdoptActivation"], state["cloudAdoptActivation"]);
        assert_eq!(after["deviceState"]["replacements"], state["replacements"]);
    }
}

#[test]
fn cloud_adopt_activated_completion_accepts_same_inode_after_wal_changes() {
    use std::os::unix::fs::MetadataExt;

    let (_dir, live) = common::copy_fixture("schema-18.anki2");
    settled_old(&live);
    crash(&live, "replace.after_durable_activation", "cloud-restore-bound", ReplaceReason::CloudAdopt);
    let before = r6_document(&live);
    let original = fs::symlink_metadata(&live).unwrap();
    let bytes = fs::read(&live).unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&live).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; UPDATE col SET dty=999; PRAGMA wal_checkpoint(TRUNCATE);")
        .unwrap();
    drop(db);
    let current = fs::symlink_metadata(&live).unwrap();
    assert_eq!((current.dev(), current.ino()), (original.dev(), original.ino()));
    assert_ne!(fs::read(&live).unwrap(), bytes);
    let expected: Publication = serde_json::from_value(before["deviceState"]["active"].clone()).unwrap();
    recover_retired_twice(&live, &expected, None, 999);
    let after = r6_document(&live);
    assert_eq!(after["deviceState"]["cloudAdoptActivation"], before["deviceState"]["cloudAdoptActivation"]);
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
        let sidecar: serde_json::Value =
            serde_json::from_slice(&fs::read(live.parent().unwrap().join("meguri-sidecar.json")).unwrap()).unwrap();
        assert!(sidecar["deviceState"]["intent"].is_null());
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
fn crash_every_boundary_recovers_exact_old_or_candidate_decision_and_restarts_twice() {
    for reason in [
        ReplaceReason::Restore,
        ReplaceReason::FullDownload,
        ReplaceReason::FullUpload,
    ] {
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
                crash(&live, point, mode, reason);
                let prior: Publication =
                    serde_json::from_slice(&fs::read(dir.path().join("prior.json")).unwrap()).unwrap();
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
                    mode == "pending" || (activated && reason != ReplaceReason::FullUpload),
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
}
#[test]
fn absent_old_and_copy_only_upgrade_recover_from_real_crashes() {
    for reason in FENCED_REASONS {
        for mode in ["absent", "older"] {
            for point in ["replace.after_pause_recorded", "replace.after_rename"] {
                let dir = tempfile::tempdir().unwrap();
                let live = dir.path().join("collection.anki2");
                if mode == "older" {
                    fs::copy(common::fixture("schema-18.anki2"), &live).unwrap();
                    settled_old(&live);
                }
                crash(&live, point, mode, reason);
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
}
#[test]
fn invalid_binding_or_unreadable_metadata_preserves_recovery_evidence() {
    for reason in FENCED_REASONS {
        for mode in [
            "account",
            "configuration",
            "environment",
            "local-only",
            "uncertain",
            "unreadable",
        ] {
            let (dir, live) = common::copy_fixture("schema-18.anki2");
            settled_old(&live);
            crash(&live, "replace.after_pause_recorded", "clear", reason);
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
                "configuration" => AccountBinding::Connected {
                    account: "synthetic-account".into(),
                    configuration: "another-app".into(),
                    environment: "test".into(),
                },
                "environment" => AccountBinding::Connected {
                    account: "synthetic-account".into(),
                    configuration: "test-app".into(),
                    environment: "another-environment".into(),
                },
                "local-only" => AccountBinding::LocalOnly,
                "uncertain" => AccountBinding::Unknown,
                _ => binding(),
            };
            let engine = Engine::new(EngineConfig::default()).unwrap();
            assert!(engine.open_raw(&live, config(binding.clone(), 55)).is_err());
            assert!(DeviceState::for_collection(&live).publication_token(&binding).is_err());
            assert_eq!(fs::read(&sidecar).unwrap(), evidence);
            assert!(staging.iter().all(|p| p.exists()));
        }
    }
}
#[test]
fn obsolete_clear_download_journals_fail_closed_in_both_phases() {
    for point in ["replace.after_pause_recorded", "replace.after_durable_activation"] {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, point, "clear", ReplaceReason::FullDownload);
        let sidecar = dir.path().join("meguri-sidecar.json");
        let mut doc: serde_json::Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        assert_eq!(doc["deviceState"]["intent"]["prior"]["decision"], "Clear");
        assert_eq!(doc["deviceState"]["intent"]["candidate"]["decision"], "Pending");
        doc["deviceState"]["intent"]["candidate"]["decision"] = "Clear".into();
        if point == "replace.after_durable_activation" {
            doc["deviceState"]["active"]["decision"] = "Clear".into();
        }
        fs::write(&sidecar, serde_json::to_vec(&doc).unwrap()).unwrap();
        let evidence = fs::read(&sidecar).unwrap();
        let bytes = fs::read(&live).unwrap();
        for _ in 0..2 {
            let engine = Engine::new(EngineConfig::default()).unwrap();
            let generation = engine.generation();
            assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
            assert_eq!(engine.generation(), generation);
            assert_eq!(fs::read(&live).unwrap(), bytes);
            assert_eq!(fs::read(&sidecar).unwrap(), evidence);
            assert!(
                DeviceState::for_collection(&live)
                    .publication_token(&binding())
                    .is_err()
            );
        }
    }
}

#[test]
fn identical_closed_bytes_use_native_identity_and_activated_phase_ignores_later_hash_changes() {
    for reason in FENCED_REASONS {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        crash(&live, "replace.after_pause_recorded", "identical", reason);
        let sidecar = dir.path().join("meguri-sidecar.json");
        let mut evidence: serde_json::Value = serde_json::from_slice(&fs::read(&sidecar).unwrap()).unwrap();
        assert_ne!(
            evidence["deviceState"]["intent"]["old_native"],
            evidence["deviceState"]["intent"]["candidate_native"]
        );
        // Legacy records lacking native identities still cannot guess when
        // both byte identities match. Preserve that refusal and its evidence.
        let legacy_path = dir.path().join("legacy-sidecar.json");
        evidence["deviceState"]["intent"]
            .as_object_mut()
            .unwrap()
            .remove("old_native");
        evidence["deviceState"]["intent"]
            .as_object_mut()
            .unwrap()
            .remove("candidate_native");
        fs::write(&legacy_path, serde_json::to_vec(&evidence).unwrap()).unwrap();
        let native = fs::read(&sidecar).unwrap();
        fs::copy(&legacy_path, &sidecar).unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
        assert_eq!(fs::read(&sidecar).unwrap(), fs::read(&legacy_path).unwrap());
        drop(engine);
        fs::write(&sidecar, native).unwrap();
        assert_eq!(recover_twice(&live, false, 722).decision, PublicationDecision::Clear);

        let (_dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, "replace.after_durable_activation", "clear", reason);
        let activated = publication(&live);
        // Real activated production journal, then a later external write changes
        // the hash. Activated identity, rather than the old digest, is authoritative.
        let db = meguri_storage::raw::PlainDatabase::open(&live).unwrap();
        db.execute_batch("UPDATE col SET dty=999").unwrap();
        drop(db);
        assert_eq!(recover_twice(&live, true, 999), activated);
    }
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
    for reason in FENCED_REASONS {
        let (dir, live) = common::copy_fixture("schema-18.anki2");
        settled_old(&live);
        crash(&live, "replace.after_pause_recorded", "clear", reason);
        let sidecar = dir.path().join("meguri-sidecar.json");
        let evidence = fs::read(&sidecar).unwrap();
        fs::write(&live, b"neither final file identity").unwrap();
        for _ in 0..2 {
            let engine = Engine::new(EngineConfig::default()).unwrap();
            assert!(engine.open_raw(&live, config(binding(), 55)).is_err());
            assert_eq!(fs::read(&sidecar).unwrap(), evidence);
        }
    }
}

#[test]
fn repeated_timestamps_produce_distinct_stored_ids() {
    for reason in FENCED_REASONS {
        let h = Harness::open();
        let mut prior = publication(&h.path);
        let mut stamps = None;
        for _ in 0..3 {
            let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
            let current = slot
                .replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
                    download_change(reason, scope)
                })
                .unwrap();
            let col = current.reader(ReaderRole::Query).unwrap().col().unwrap();
            let current_stamps = (col.modified, col.schema_modified, col.last_sync);
            if let Some(previous) = stamps {
                assert_eq!(current_stamps, previous);
            }
            if reason == ReplaceReason::Restore {
                assert_eq!(col.modified.0, NOW_MS);
                assert_eq!(col.schema_modified.0, NOW_MS);
            } else {
                assert_ne!(col.modified.0, NOW_MS);
                assert_ne!(col.schema_modified.0, NOW_MS);
                assert_eq!(col.last_sync, col.modified);
            }
            stamps = Some(current_stamps);
            let active = publication(&h.path);
            assert_ne!(active.identity, prior.identity);
            assert_eq!(active.decision, PublicationDecision::Pending);
            prior = active;
        }
    }
}

#[test]
fn failed_activation_save_refuses_writer_until_restart_proves_the_candidate() {
    for reason in FENCED_REASONS {
        let h = Harness::open();
        let prior = publication(&h.path);
        faultpoint::arm(
            "replace.after_dir_sync",
            Fault::Callback(|| {
                faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
            }),
        );
        let slot = h.engine.acquire_slot(replacement_op(reason)).unwrap();
        let result = slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), reason, |scope| {
            download_change(reason, scope)?;
            let mut col = scope.col()?;
            col.dirty = 722;
            scope.put_col(&col)?;
            Ok(())
        });
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
}

#[test]
fn import_and_transfers_recheck_session_started_during_participant_flush() {
    use meguri_engine::{Ports, SuspendParticipant, sched::queue::QueueProvider, session::ReviewService};
    use meguri_model::ids::DeckId;
    struct CapturedEvidence {
        collection_bytes: Vec<u8>,
        wal_bytes: Vec<u8>,
        publication: Publication,
    }
    struct StartDuringFlush {
        review: Arc<ReviewService>,
        armed: std::sync::atomic::AtomicBool,
        live: PathBuf,
        captured: Mutex<Option<CapturedEvidence>>,
    }
    impl SuspendParticipant for StartDuringFlush {
        fn name(&self) -> &str {
            "start-review-during-import-flush"
        }
        fn flush(&self) -> Result<(), EngineError> {
            if self.armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
                self.review.start(DeckId(1), None)?;
                *self.captured.lock().unwrap() = Some(CapturedEvidence {
                    collection_bytes: fs::read(&self.live).unwrap(),
                    wal_bytes: fs::read(self.live.with_extension("anki2-wal")).unwrap(),
                    publication: publication(&self.live),
                });
            }
            Ok(())
        }
    }
    for reason in [
        ReplaceReason::ColpkgImport,
        ReplaceReason::FullDownload,
        ReplaceReason::FullUpload,
    ] {
        let fixture = session_support::Harness::new(3);
        let live = fixture.dir.path().join("collection.anki2");
        fixture.engine.close().unwrap();
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let queues = Arc::new(QueueProvider::default());
        let review = ReviewService::new(&engine, queues.clone()).unwrap();
        let race = Arc::new(StartDuringFlush {
            review: review.clone(),
            armed: std::sync::atomic::AtomicBool::new(true),
            live: live.clone(),
            captured: Mutex::new(None),
        });
        let ports = Ports::new()
            .observer(queues.clone())
            .generation_listener(queues)
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone())
            .suspend_participant(race.clone());
        let mut open = common::open_config(&fixture.clock);
        open.ports = ports;
        engine.open_raw(&live, open).unwrap();
        assert!(review.active_session().is_none());
        let generation = engine.generation();
        let slot = engine.acquire_slot(replacement_op(reason)).unwrap();
        let result = slot.replace_verified(stage(fixture.dir.path(), "schema-18.anki2"), reason, |_| {
            panic!("review race reached validation transaction")
        });
        assert!(matches!(
            result,
            Err(ReplaceError::Lifecycle(EngineError::SessionActive))
        ));
        assert!(review.active_session().is_some());
        assert_eq!(engine.generation(), generation);
        let captured = race.captured.lock().unwrap();
        let evidence = captured.as_ref().expect("session started during flush");
        assert_eq!(&fs::read(&live).unwrap(), &evidence.collection_bytes);
        assert_eq!(
            &fs::read(live.with_extension("anki2-wal")).unwrap(),
            &evidence.wal_bytes
        );
        assert_eq!(&publication(&live), &evidence.publication);
    }
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

#[test]
fn legacy_r6_last_replacement_migrates_to_chain_before_second_activation() {
    let h = Harness::open();
    let before = publication(&h.path).identity;
    for legacy in [true, false] {
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
            Ok(())
        })
        .unwrap();
        drop(slot);
        if legacy {
            // Keep the first activation in the legacy single-record form.
            // The next activation must carry that link into the proof chain.
            meguri_storage::sidecar::update_document(&h.dir.path().join("meguri-sidecar.json"), |doc| {
                let device_fields = doc["deviceState"].as_object_mut().unwrap();
                device_fields.remove("replacements");
                Ok(())
            })
            .unwrap();
        }
    }
    let device = DeviceState::for_collection(&h.path);
    let proof = device.local_replacement_successor(before).unwrap().unwrap();
    assert!(proof.before() == before);
    assert!(proof.after() == publication(&h.path).identity);
    assert!(proof.after() != before);
    h.engine.close().unwrap();
}

#[test]
fn local_restore_consumer_checks_review_before_entry_and_after_a_session_wins_the_race() {
    use meguri_engine::SuspendParticipant;
    let h = session_support::Harness::new(3);
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    slot.check_restore_review().unwrap();
    let (id, _) = h.start();
    h.review.flush().unwrap();
    let live = h.dir.path().join("collection.anki2");
    let before = (
        fs::read(&live).unwrap(),
        fs::read(live.with_extension("anki2-wal")).unwrap(),
    );
    assert_eq!(slot.check_restore_review(), Err(EngineError::SessionActive));
    let exclusive = slot.enter_exclusive(true).unwrap();
    assert_eq!(slot.check_restore_review(), Err(EngineError::SessionActive));
    drop(exclusive);
    assert_eq!(h.review.active_session(), Some(id));
    assert_eq!(
        (
            fs::read(&live).unwrap(),
            fs::read(live.with_extension("anki2-wal")).unwrap()
        ),
        before
    );
    assert_eq!(h.engine.generation(), h.collection.generation());
}

fn raw_recovery_fixture() -> (tempfile::TempDir, PathBuf, Publication) {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("collection");
    fs::create_dir(&directory).unwrap();
    let live = directory.join("collection.anki2");
    fs::copy(common::fixture("schema-18.anki2"), &live).unwrap();
    settled_old(&live);
    let prior = publication(&live);
    fs::write(&live, b"damaged main retained without SQLite writes").unwrap();
    fs::write(
        live.with_file_name("collection.anki2-wal"),
        b"nonempty old WAL recovery bytes",
    )
    .unwrap();
    (root, live, prior)
}
fn r6_document(live: &Path) -> serde_json::Value {
    serde_json::from_slice(&fs::read(live.parent().unwrap().join("meguri-sidecar.json")).unwrap()).unwrap()
}
fn old_reader_rejects_wal_intent(live: &Path) {
    #[derive(serde::Deserialize)]
    enum LegacyPhase {
        Intent,
        Activated,
    }
    let path = live.parent().unwrap().join("meguri-sidecar.json");
    let before = fs::read(&path).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&before).unwrap();
    // This is the required strict field used by the existing main-only reader.
    // Neither an optional member nor a version number participates in refusal.
    assert!(serde_json::from_value::<LegacyPhase>(document["deviceState"]["intent"]["phase"].clone()).is_err());
    assert_eq!(fs::read(path).unwrap(), before);
}
fn assert_raw_copies(root: &Path) {
    assert_eq!(
        fs::read(root.join("raw-backups/retained-main.anki2")).unwrap(),
        b"damaged main retained without SQLite writes"
    );
    assert_eq!(
        fs::read(root.join("raw-backups/retained-main.anki2-wal")).unwrap(),
        b"nonempty old WAL recovery bytes"
    );
}

#[test]
fn icloud_retirement_initial_restore_settles_archival_owner_before_raw_recovery() {
    for failed in [false, true] {
        for point in ["replace.after_pause_recorded", "replace.wal.after_retire"] {
            let (root, live, prior) = raw_recovery_fixture();
            crash(&live, point, "raw", ReplaceReason::Restore);
            let engine = Engine::new(EngineConfig::default()).unwrap();
            if failed {
                // A previous ordinary fallback open fails before it can settle
                // a Connected local intent. Initial restore must do that work.
                let before = recovery_files(root.path());
                assert!(engine.open_raw(&live, config(AccountBinding::LocalOnly, 200)).is_err());
                assert!(matches!(engine.state(), meguri_engine::LifecycleState::Failed { .. }));
                assert_eq!(recovery_files(root.path()), before);
            } else {
                assert_eq!(engine.state(), meguri_engine::LifecycleState::NoCollection);
            }
            let slot = engine.acquire_slot(LongOp::Restore).unwrap();
            let mut open = retirement_config();
            open.startup_publication_binding = Some(AccountBinding::Unknown);
            open.startup_identity_read_failed = true;
            let recovered = slot.restore_verified_initial_before_commit(
                &live,
                open,
                stage(live.parent().unwrap(), "schema-18.anki2"),
                || {
                    assert_eq!(
                        DeviceState::for_collection(&live).settled_publication().unwrap(),
                        Some(prior.clone())
                    );
                    assert!(r6_document(&live)["deviceState"]["intent"].is_null());
                    assert_eq!(fs::read(&live).unwrap(), b"damaged main retained without SQLite writes");
                    assert_eq!(
                        fs::read(live.with_file_name("collection.anki2-wal")).unwrap(),
                        b"nonempty old WAL recovery bytes"
                    );
                    let backups = root.path().join("restore-backups");
                    fs::create_dir(&backups).map_err(StorageError::from)?;
                    let main = backups.join("retained-main.anki2");
                    let wal = backups.join("retained-main.anki2-wal");
                    fs::copy(&live, &main).map_err(StorageError::from)?;
                    fs::copy(live.with_file_name("collection.anki2-wal"), &wal).map_err(StorageError::from)?;
                    meguri_engine::device_state::RawRestoreAuthority::verify(&live, &main, &wal)
                        .map(Some)
                        .map_err(Into::into)
                },
            ).unwrap();
            drop(slot);
            assert_eq!(marker(&recovered), 0);
            let after = publication(&live);
            assert_eq!(after.binding, prior.binding);
            assert_eq!(after.decision, PublicationDecision::Pending);
            assert_ne!(after.identity, prior.identity);
            assert_reason(&live, Some(PendingDecisionReason::Restore));
            assert_eq!(
                DeviceState::for_collection(&live)
                    .local_replacement_successor(prior.identity)
                    .unwrap()
                    .unwrap()
                    .after(),
                after.identity
            );
            assert_raw_copies(root.path());
            engine.close().unwrap();
            recover_retired_twice(&live, &after, Some(PendingDecisionReason::Restore), 0);
            assert_eq!(
                DeviceState::for_collection(&live)
                    .local_replacement_successor(prior.identity)
                    .unwrap()
                    .unwrap()
                    .after(),
                after.identity
            );
        }
    }
}

#[test]
fn icloud_retirement_initial_restore_unknown_file_refuses_before_recovery_hook() {
    let (root, live, _) = raw_recovery_fixture();
    crash(&live, "replace.after_pause_recorded", "raw", ReplaceReason::Restore);
    fs::write(&live, b"unproven replacement file").unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let candidate = stage(live.parent().unwrap(), "schema-18.anki2");
    // Stage teardown is allowed on refusal; all durable recovery bytes stay.
    let candidate_directory = candidate.directory().to_owned();
    let mut before = recovery_files(root.path());
    before.retain(|(path, _)| !path.starts_with(&candidate_directory));
    let mut hook_ran = false;
    let result = slot.restore_verified_initial_before_commit(&live, retirement_config(), candidate, || {
        hook_ran = true;
        Ok(None)
    });
    assert!(result.is_err());
    assert!(!hook_ran);
    assert_eq!(engine.state(), meguri_engine::LifecycleState::NoCollection);
    assert_eq!(engine.generation(), Generation(0));
    let mut after = recovery_files(root.path());
    after.retain(|(path, _)| !path.starts_with(&candidate_directory));
    assert_eq!(after, before);
}

#[test]
fn icloud_retirement_retained_wal_uses_existing_exact_pair_recovery() {
    for point in [
        "replace.after_pause_recorded",
        "replace.wal.after_retire",
        "replace.after_rename",
        "replace.after_durable_activation",
    ] {
        let (root, live, prior) = raw_recovery_fixture();
        crash(&live, point, "raw", ReplaceReason::Restore);
        let before = r6_document(&live);
        let candidate = cloud_swapped(point);
        let expected: Publication = serde_json::from_value(
            before["deviceState"]["intent"][if candidate { "candidate" } else { "prior" }].clone(),
        )
        .unwrap();
        for _ in 0..2 {
            let engine = Engine::new(EngineConfig::default()).unwrap();
            let opened = engine.open_raw(&live, retirement_config());
            if candidate {
                opened.unwrap();
                assert_reason(&live, Some(PendingDecisionReason::Restore));
                engine.close().unwrap();
            } else {
                assert!(opened.is_err(), "regular recovery must leave the damaged old main damaged");
                assert_eq!(expected.identity, prior.identity);
                assert_eq!(fs::read(&live).unwrap(), b"damaged main retained without SQLite writes");
                assert_eq!(
                    fs::read(live.with_file_name("collection.anki2-wal")).unwrap(),
                    b"nonempty old WAL recovery bytes"
                );
            }
            assert_eq!(publication(&live), expected);
            assert!(r6_document(&live)["deviceState"]["intent"].is_null());
            assert_raw_copies(root.path());
        }
    }
    let (root, live, _) = raw_recovery_fixture();
    crash(&live, "replace.after_pause_recorded", "raw", ReplaceReason::Restore);
    fs::write(root.path().join("raw-backups/retained-main.anki2-wal"), b"substituted retained WAL").unwrap();
    let before = recovery_files(root.path());
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(engine.open_raw(&live, retirement_config()).is_err());
    assert_eq!(recovery_files(root.path()), before);
}

fn reopen_raw_twice(root: &Path, live: &Path, prior: &Publication, candidate: bool) {
    for _ in 0..2 {
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let opened = engine.open_raw(live, config(binding(), 177));
        if candidate {
            let c = opened.unwrap();
            assert!(meguri_storage::integrity::integrity_check(c.path()).unwrap().is_ok());
            assert_eq!(publication(live).decision, PublicationDecision::Pending);
            assert_ne!(publication(live).identity, prior.identity);
            engine.close().unwrap();
        } else {
            assert!(opened.is_err(), "the damaged old main is never repaired by recovery");
            assert_eq!(&publication(live), prior);
            assert_eq!(fs::read(live).unwrap(), b"damaged main retained without SQLite writes");
            assert_eq!(
                fs::read(live.with_file_name("collection.anki2-wal")).unwrap(),
                b"nonempty old WAL recovery bytes"
            );
        }
        assert!(r6_document(live)["deviceState"]["intent"].is_null());
        assert_raw_copies(root);
    }
}
#[test]
fn retained_wal_kills_reconcile_exact_old_pair_or_isolated_candidate_before_open() {
    for point in [
        "replace.wal.after_authority",
        "replace.after_pause_recorded",
        "storage.recovery_move.before_directory_sync",
        "replace.wal.after_retire",
        "replace.wal.after_retired_record",
        "replace.after_live_wal_removed",
        "replace.after_rename",
        "replace.after_dir_sync",
        "replace.wal.after_resolution",
        "replace.after_durable_activation",
        "replace.media.before_completion",
        "replace.media.after_completion",
    ] {
        let (root, live, prior) = raw_recovery_fixture();
        crash(&live, point, "raw", ReplaceReason::Restore);
        let candidate = matches!(
            point,
            "replace.after_rename"
                | "replace.after_dir_sync"
                | "replace.wal.after_resolution"
                | "replace.after_durable_activation"
                | "replace.media.before_completion"
                | "replace.media.after_completion"
        );
        if point != "replace.wal.after_authority" && point != "replace.media.after_completion" {
            old_reader_rejects_wal_intent(&live);
        }
        if candidate {
            assert!(!live.with_file_name("collection.anki2-wal").exists());
            assert_eq!(
                fs::read(&live).unwrap(),
                fs::read(live.with_extension("candidate-final")).unwrap()
            );
        }
        reopen_raw_twice(root.path(), &live, &prior, candidate);
    }
}
#[test]
fn retained_wal_rollback_kills_restart_the_same_move_before_any_old_open() {
    for point in [
        "replace.wal.before_restore_move",
        "storage.recovery_move.before_directory_sync",
        "replace.wal.after_restore_move",
        "replace.wal.after_resolution",
        "replace.media.before_completion",
        "replace.media.after_completion",
    ] {
        let (root, live, prior) = raw_recovery_fixture();
        crash(&live, "replace.wal.after_retired_record", "raw", ReplaceReason::Restore);
        crash(&live, point, "raw-recover", ReplaceReason::Restore);
        if point != "replace.media.after_completion" {
            old_reader_rejects_wal_intent(&live);
        }
        assert_eq!(fs::read(&live).unwrap(), b"damaged main retained without SQLite writes");
        reopen_raw_twice(root.path(), &live, &prior, false);
    }
}
fn recovery_files(root: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    fn visit(root: &Path, result: &mut Vec<(PathBuf, Vec<u8>)>) {
        for entry in fs::read_dir(root).unwrap() {
            let entry = entry.unwrap();
            let kind = entry.file_type().unwrap();
            if kind.is_dir() {
                visit(&entry.path(), result);
            } else if kind.is_file() && entry.file_name() != "meguri.lock" {
                result.push((entry.path(), fs::read(entry.path()).unwrap()));
            } else if kind.is_symlink() {
                use std::os::unix::ffi::OsStrExt;
                result.push((
                    entry.path(),
                    fs::read_link(entry.path()).unwrap().as_os_str().as_bytes().to_vec(),
                ));
            }
        }
    }
    let mut result = Vec::new();
    visit(root, &mut result);
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}
#[test]
fn retained_wal_substituted_missing_corrupt_and_ambiguous_evidence_is_never_consumed() {
    for point in [
        "replace.after_pause_recorded",
        "replace.after_rename",
        "replace.after_durable_activation",
    ] {
        for damage in [
            "main-native",
            "main-missing",
            "main-corrupt",
            "wal-native",
            "wal-corrupt",
            "copy-native",
            "copy-missing",
            "copy-corrupt",
            "backup-root",
            "backup-root-link",
            "collection-root",
            "copy-link",
            "copy-hardlink",
            "both-wals",
            "neither-wal",
            "equal-main",
        ] {
            let (root, live, _) = raw_recovery_fixture();
            crash(&live, point, "raw", ReplaceReason::Restore);
            let document = r6_document(&live);
            let wal = &document["deviceState"]["intent"]["wal"];
            let retired = PathBuf::from(wal["retired"].as_str().unwrap());
            let original = live.with_file_name("collection.anki2-wal");
            let present = if original.exists() { &original } else { &retired };
            let copy = root.path().join("raw-backups/retained-main.anki2-wal");
            match damage {
                "main-native" | "wal-native" | "copy-native" => {
                    let path = match damage {
                        "main-native" => &live,
                        "wal-native" => present,
                        _ => &copy,
                    };
                    let bytes = fs::read(path).unwrap();
                    fs::rename(path, path.with_extension("held-evidence")).unwrap();
                    fs::write(path, bytes).unwrap();
                }
                "main-missing" => {
                    fs::rename(&live, live.with_extension("held-main")).unwrap();
                }
                "main-corrupt" => {
                    fs::write(&live, b"changed main bytes").unwrap();
                }
                "wal-corrupt" => {
                    fs::write(present, b"changed WAL bytes").unwrap();
                }
                "copy-missing" => {
                    fs::rename(&copy, copy.with_extension("held-evidence")).unwrap();
                }
                "copy-corrupt" => {
                    fs::write(&copy, b"changed retained bytes").unwrap();
                }
                "backup-root" => {
                    let path = root.path().join("raw-backups");
                    fs::rename(&path, root.path().join("held-backups")).unwrap();
                    fs::create_dir(&path).unwrap();
                    for name in ["retained-main.anki2", "retained-main.anki2-wal"] {
                        fs::copy(root.path().join("held-backups").join(name), path.join(name)).unwrap();
                    }
                }
                "copy-link" | "copy-hardlink" => {
                    let held = copy.with_extension("held-evidence");
                    fs::rename(&copy, &held).unwrap();
                    if damage == "copy-link" {
                        std::os::unix::fs::symlink(&held, &copy).unwrap();
                    } else {
                        fs::hard_link(&held, &copy).unwrap();
                    }
                }
                "backup-root-link" => {
                    let path = root.path().join("raw-backups");
                    let held = root.path().join("held-backups");
                    fs::rename(&path, &held).unwrap();
                    std::os::unix::fs::symlink(&held, &path).unwrap();
                }
                "equal-main" => {
                    let mut document = r6_document(&live);
                    document["deviceState"]["intent"]["candidate_file"] =
                        document["deviceState"]["intent"]["old_file"].clone();
                    fs::write(
                        live.parent().unwrap().join("meguri-sidecar.json"),
                        serde_json::to_vec(&document).unwrap(),
                    )
                    .unwrap();
                }
                "collection-root" => {
                    let path = live.parent().unwrap();
                    let held = root.path().join("held-collection");
                    fs::rename(path, &held).unwrap();
                    fs::create_dir(path).unwrap();
                    for entry in fs::read_dir(held).unwrap() {
                        let entry = entry.unwrap();
                        if entry.file_type().unwrap().is_file() {
                            fs::copy(entry.path(), path.join(entry.file_name())).unwrap();
                        }
                    }
                }
                "both-wals" => {
                    fs::copy(present, if original.exists() { &retired } else { &original }).unwrap();
                }
                "neither-wal" => {
                    fs::rename(present, present.with_extension("held-evidence")).unwrap();
                }
                _ => unreachable!(),
            }
            let before = recovery_files(root.path());
            for _ in 0..2 {
                let engine = Engine::new(EngineConfig::default()).unwrap();
                assert!(
                    engine.open_raw(&live, config(binding(), 199)).is_err(),
                    "{point}: {damage}"
                );
                drop(engine);
                assert_eq!(recovery_files(root.path()), before, "{point}: {damage}");
                old_reader_rejects_wal_intent(&live);
            }
        }
    }
}

#[test]
fn raw_restore_matching_final_databases_block_the_old_wal_move() {
    let (root, live, _) = raw_recovery_fixture();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let candidate = stage(live.parent().unwrap(), "schema-18.anki2");
    let candidate_path = candidate.path();
    let backups = root.path().join("raw-backups");
    fs::create_dir(&backups).unwrap();
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let result = slot.restore_verified_initial_before_commit(&live, config(binding(), 200), candidate, || {
        // Force equality only after R7 finalisation, under the existing hook.
        fs::copy(&candidate_path, &live).map_err(StorageError::from)?;
        let main = backups.join("retained-main.anki2");
        let wal = backups.join("retained-main.anki2-wal");
        fs::copy(&live, &main).map_err(StorageError::from)?;
        fs::copy(live.with_file_name("collection.anki2-wal"), &wal).map_err(StorageError::from)?;
        meguri_engine::device_state::RawRestoreAuthority::verify(&live, &main, &wal)
            .map(Some)
            .map_err(Into::into)
    });
    assert!(result.is_err());
    assert_eq!(
        fs::read(&live).unwrap(),
        fs::read(backups.join("retained-main.anki2")).unwrap()
    );
    assert_eq!(
        fs::read(live.with_file_name("collection.anki2-wal")).unwrap(),
        b"nonempty old WAL recovery bytes"
    );
    assert!(r6_document(&live)["deviceState"]["intent"].is_null());
    assert!(!fs::read_dir(live.parent().unwrap()).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with(".meguri-restore-wal-")
    }));
}

#[test]
fn replacement_witness_extensions_stay_with_the_old_candidate_across_real_replacements() {
    for legacy in [true, false] {
        let h = Harness::open();
        let before = publication(&h.path).identity;
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
            Ok(())
        })
        .unwrap();
        drop(slot);
        let old_identity = publication(&h.path).identity;
        assert_ne!(old_identity, before);
        let extension = serde_json::json!({"synthetic": "old-witness-extension"});
        meguri_storage::sidecar::update_document(&h.dir.path().join("meguri-sidecar.json"), |doc| {
            let fields = doc["deviceState"].as_object_mut().unwrap();
            fields.get_mut("lastReplacement").unwrap()["futureWitnessMember"] = extension.clone();
            if legacy {
                // Use the existing legacy compatibility fixture: only the
                // single activation witness predates the retained chain.
                fields.remove("replacements");
            } else {
                fields.get_mut("replacements").unwrap()[0]["futureWitnessMember"] = extension.clone();
            }
            Ok(())
        })
        .unwrap();
        let old_witness = r6_document(&h.path)["deviceState"]["lastReplacement"].clone();
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        slot.replace_verified(stage(h.dir.path(), "schema-18.anki2"), ReplaceReason::Restore, |_| {
            Ok(())
        })
        .unwrap();
        drop(slot);
        let new_identity = publication(&h.path).identity;
        assert_ne!(new_identity, old_identity);
        let document = r6_document(&h.path);
        let fields = &document["deviceState"];
        let retained = fields["replacements"].as_array().unwrap();
        assert_eq!(retained.len(), 2);
        let old_id = serde_json::to_value(old_identity).unwrap();
        let new_id = serde_json::to_value(new_identity).unwrap();
        let old = retained
            .iter()
            .find(|record| record["candidate"]["identity"] == old_id)
            .unwrap();
        let new = retained
            .iter()
            .find(|record| record["candidate"]["identity"] == new_id)
            .unwrap();
        assert_eq!(old, &old_witness, "legacy={legacy}");
        assert_eq!(old.get("futureWitnessMember"), Some(&extension));
        assert!(new.get("futureWitnessMember").is_none());
        assert_eq!(fields["lastReplacement"]["candidate"]["identity"], new_id);
        assert!(fields["lastReplacement"].get("futureWitnessMember").is_none());
        let proof = DeviceState::for_collection(&h.path)
            .local_replacement_successor(before)
            .unwrap()
            .unwrap();
        assert_eq!(proof.after(), new_identity);
        h.engine.close().unwrap();
    }
}
