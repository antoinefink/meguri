//! Registered connection work: real Host, Runtime, opaque slot and stage.
use super::*;
use meguri_engine::{EventKind, JobKind, JobOutcome};
use std::sync::mpsc;

struct Release(mpsc::Sender<()>);
impl Drop for Release {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
fn finished(events: &meguri_engine::changes::stream::Subscription, id: u64) -> JobOutcome {
    loop {
        let event = events.recv_timeout(Duration::from_secs(10)).expect("terminal event");
        if let EventKind::JobFinished { id: got, outcome } = &event.kind
            && got.0 == id
        {
            return outcome.clone();
        }
    }
}
fn hold_pool(fixture: &Fixture) -> Vec<Release> {
    (0..4)
        .map(|_| {
            let (entered, reached) = mpsc::channel();
            let (release, resume) = mpsc::channel();
            fixture
                .workflow
                .host
                .engine()
                .start_job(JobKind::BulkOp, move |_| {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(20)).unwrap();
                    Ok(())
                })
                .unwrap();
            reached.recv_timeout(Duration::from_secs(5)).unwrap();
            Release(release)
        })
        .collect()
}
fn adoption(
    fixture: &Fixture,
) -> (
    Arc<CloudConnection>,
    Arc<CloudAdoptionWork>,
    CloudControlRecord,
    std::path::PathBuf,
) {
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    connection.confirm_adoption(preview).unwrap();
    let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
    let stage = fixture.workflow.host.cloud_connection_directory().join(op.staging_name);
    let CloudPublicationWork::Adoption { handle, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption");
    };
    (connection, handle, active, stage)
}
fn stage_bytes(path: &std::path::Path) -> std::collections::BTreeMap<std::path::PathBuf, Vec<u8>> {
    fn read(
        root: &std::path::Path,
        path: &std::path::Path,
        out: &mut std::collections::BTreeMap<std::path::PathBuf, Vec<u8>>,
    ) {
        for entry in std::fs::read_dir(path).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                read(root, &path, out);
            } else {
                out.insert(
                    path.strip_prefix(root).unwrap().to_owned(),
                    std::fs::read(path).unwrap(),
                );
            }
        }
    }
    let mut out = Default::default();
    read(path, path, &mut out);
    out
}

#[test]
fn connection_jobs_admit_during_held_runtime_and_cancel_before_claim_without_stage_or_policy_effects() {
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, active, stage) = adoption(&fixture);
    let before = fixture.workflow.core.adoption_op().unwrap();
    let bytes = stage_bytes(&stage);
    let policy = fixture.workflow.host.cloud_policy_driver().unwrap().state();
    let events = fixture.workflow.host.engine().stream().subscribe();
    std::thread::scope(|scope| {
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let runtime = fixture.workflow.core.clone();
        let held = scope.spawn(move || {
            runtime.with_state_held_for_testing(|| {
                entered.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(20)).unwrap();
            })
        });
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let (contended, observed) = mpsc::channel();
        fixture.workflow.core.observe_state_contention_for_testing(contended);
        let next = connection.clone().start_next_publication_work().unwrap();
        observed
            .recv_timeout(Duration::from_secs(5))
            .expect("worker reached held Runtime");
        let reply = connection
            .clone()
            .start_adoption_result(
                handle.clone(),
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record: active.clone() },
                },
            )
            .unwrap();
        // A synchronous Runtime facade on Maintenance could not return either
        // admission while this real state guard remains held.
        let unrelated = fixture
            .workflow
            .host
            .engine()
            .start_job(JobKind::BulkOp, |_| Ok(()))
            .unwrap();
        assert!(!fixture.workflow.host.engine().jobs().running().is_empty());
        next.handle.cancel.cancel().unwrap();
        reply.handle.cancel.cancel().unwrap();
        drop(release);
        held.join().unwrap();
        let mut pending: std::collections::BTreeSet<_> =
            [next.handle.id, reply.handle.id, unrelated.id.0].into_iter().collect();
        while !pending.is_empty() {
            let event = events.recv_timeout(Duration::from_secs(10)).unwrap();
            if let EventKind::JobFinished { id, .. } = &event.kind {
                pending.remove(&id.0);
            }
        }
        assert!(matches!(next.result.result(), Err(CoreError::Cancelled)));
        assert!(matches!(reply.result.result(), Err(CoreError::Cancelled)));
    });
    assert_eq!(fixture.workflow.core.adoption_op().unwrap(), before);
    assert_eq!(stage_bytes(&stage), bytes);
    assert_eq!(fixture.workflow.host.cloud_policy_driver().unwrap().state(), policy);
    assert_eq!(
        fixture.workflow.host.engine().slot_holder(),
        Some(meguri_engine::LongOp::CloudAdopt)
    );
    // The exact issued task remains usable after cancellation; no reacquire.
    connection
        .adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn connection_reply_cancelled_behind_next_mutex_does_not_consume_opaque_task() {
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, active, stage) = adoption(&fixture);
    let before = stage_bytes(&stage);
    let events = fixture.workflow.host.engine().stream().subscribe();
    std::thread::scope(|scope| {
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        let runtime = fixture.workflow.core.clone();
        let held = scope.spawn(move || {
            runtime
                .with_adoption_next_held_for_testing(|| {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(20)).unwrap();
                })
                .unwrap()
        });
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let (contended, observed) = mpsc::channel();
        fixture
            .workflow
            .core
            .observe_adoption_next_contention_for_testing(contended)
            .unwrap();
        let job = connection
            .clone()
            .start_adoption_result(
                handle.clone(),
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record: active.clone() },
                },
            )
            .unwrap();
        observed
            .recv_timeout(Duration::from_secs(5))
            .expect("worker waits for actual opaque claim");
        job.handle.cancel.cancel().unwrap();
        drop(release);
        held.join().unwrap();
        assert_eq!(finished(&events, job.handle.id), JobOutcome::Cancelled);
        assert!(matches!(job.result.result(), Err(CoreError::Cancelled)));
    });
    assert_eq!(stage_bytes(&stage), before);
    connection
        .adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn connection_queued_jobs_cancel_without_claim_and_revalidate_original_generation_and_owner() {
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, active, stage) = adoption(&fixture);
    let before = stage_bytes(&stage);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let reply = connection
        .clone()
        .start_adoption_result(
            handle.clone(),
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active.clone() },
            },
        )
        .unwrap();
    reply.handle.cancel.cancel().unwrap();
    drop(pool);
    assert_eq!(finished(&events, reply.handle.id), JobOutcome::Cancelled);
    assert!(matches!(reply.result.result(), Err(CoreError::Cancelled)));
    assert_eq!(stage_bytes(&stage), before);
    connection
        .adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    fixture.workflow.host.close().unwrap();
    assert!(!stage.exists());
    failure_is(
        connection.clone().start_next_publication_work(),
        CloudConnectionFailure::Closed,
    );

    let replacement = Fixture::new(Keys::new([]));
    let connection = replacement.workflow.connection().unwrap();
    let engine = replacement.workflow.host.engine();
    let events = engine.stream().subscribe();
    let pool = hold_pool(&replacement);
    let saved = connection.start_next_publication_work().unwrap();
    let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive
        .replace(|path| {
            assert!(path.exists());
            Ok(())
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    drop(pool);
    assert!(matches!(finished(&events, saved.handle.id), JobOutcome::Failed { .. }));
    failure_is(saved.result.result(), CloudConnectionFailure::AuthorityChanged);
    replacement.workflow.host.close().unwrap();
    failure_is(saved.result.result(), CloudConnectionFailure::AuthorityChanged);
}

#[test]
fn connection_claimed_real_reply_retains_result_and_mandatory_abort_before_close_retires_collection() {
    for malformed in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let (connection, handle, active, stage) = adoption(&fixture);
        let collection = fixture.workflow.host.collection().unwrap();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let (claimed, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        fixture
            .workflow
            .core
            .set_adoption_reply_claim_hook_for_testing(move || {
                claimed.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(20)).unwrap();
            });
        let release = Release(release);
        let reply = if malformed {
            CloudControlRead::Missing
        } else {
            CloudControlRead::Found { record: active }
        };
        let job = connection
            .start_adoption_result(handle, CloudAdoptionResult::Read { read: reply })
            .unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        std::thread::scope(|scope| {
            let closing = scope.spawn(|| fixture.workflow.host.close());
            // Positive cancellation by close proves its join has started. This
            // bound is only a deadlock watchdog, not the behavior assertion.
            let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
            while !job.handle.cancel.is_cancelled().unwrap() {
                assert!(meguri_engine::monotonic::now() < deadline);
                std::thread::yield_now();
            }
            assert!(collection.reader(ReaderRole::Query).unwrap().col().is_ok());
            assert!(stage.exists());
            drop(release);
            closing.join().unwrap().unwrap();
        });
        let outcome = finished(&events, job.handle.id);
        if malformed {
            assert!(matches!(outcome, JobOutcome::Failed { .. }));
            failure_is(job.result.result(), CloudConnectionFailure::Tracking);
        } else {
            assert_eq!(outcome, JobOutcome::Succeeded);
            assert!(job.result.result().unwrap().unwrap().applied);
        }
        assert!(collection.reader(ReaderRole::Query).is_err());
        assert!(!stage.exists());
        assert!(fixture.workflow.host.engine().jobs().running().is_empty());
    }
}

#[test]
fn connection_post_claim_cancel_preserves_actual_followup_and_temporary_failure_restores_next() {
    for temporary in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let (connection, handle, active, _) = adoption(&fixture);
        let expected_active = active.clone();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let (claimed, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        fixture
            .workflow
            .core
            .set_adoption_reply_claim_hook_for_testing(move || {
                claimed.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(20)).unwrap();
            });
        let release = Release(release);
        let read = if temporary {
            CloudControlRead::Failed
        } else {
            CloudControlRead::Found { record: active }
        };
        let job = connection
            .clone()
            .start_adoption_result(handle, CloudAdoptionResult::Read { read })
            .unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        job.handle.cancel.cancel().unwrap();
        drop(release);
        let outcome = finished(&events, job.handle.id);
        if temporary {
            assert!(matches!(outcome, JobOutcome::Failed { .. }));
            failure_is(job.result.result(), CloudConnectionFailure::AccountUnavailable);
        } else {
            assert_eq!(outcome, JobOutcome::Succeeded);
            assert!(job.result.result().unwrap().unwrap().applied);
        }
        let CloudPublicationWork::Adoption { action, .. } = connection.next_publication_work().unwrap() else {
            panic!("followup retained");
        };
        let target = match (temporary, action) {
            (true, CloudAdoptionAction::ReadActive { target, expected }) => {
                assert_eq!(expected.encrypted, expected_active.encrypted);
                assert_eq!(expected.system_fields, expected_active.system_fields);
                target
            }
            (false, CloudAdoptionAction::Read { target }) => target,
            _ => panic!("followup retained"),
        };
        assert_eq!(target.name, if temporary { "active" } else { "epoch" });
        fixture.workflow.host.close().unwrap();
    }
}

#[test]
fn connection_close_drain_timeout_then_cleanup_failure_keeps_original_host_and_paused_admission_for_retry() {
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, active, stage) = adoption(&fixture);
    let generation = fixture.workflow.host.cloud_connection_generation().unwrap();
    let events = fixture.workflow.host.engine().stream().subscribe();
    let (claimed, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    fixture
        .workflow
        .core
        .set_adoption_reply_claim_hook_for_testing(move || {
            claimed.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(20)).unwrap();
        });
    let release = Release(release);
    let job = connection
        .clone()
        .start_adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    reached.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(
        matches!(fixture.workflow.host.close(), Err(meguri_engine::EngineError::SuspendDeadline { phase }) if phase == "jobs")
    );
    assert_eq!(fixture.workflow.host.cloud_connection_generation().unwrap(), generation);
    assert!(stage.exists());
    drop(release);
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
    assert!(job.result.result().unwrap().unwrap().applied);
    // Fail the original cleanup's filesystem identity read, without replacing
    // the original file or changing its inode. No collection operation runs.
    let path = fixture
        .workflow
        .host
        .cloud_connection_directory()
        .join("collection.anki2");
    let parked = path.with_extension("parked");
    std::fs::rename(&path, &parked).unwrap();
    let failed = fixture.workflow.host.close();
    std::fs::rename(&parked, &path).unwrap();
    assert!(matches!(failed, Err(meguri_engine::EngineError::Busy { running_op }) if running_op == "cloud adoption"));
    assert_eq!(fixture.workflow.host.cloud_connection_generation().unwrap(), generation);
    assert!(stage.exists());
    let paused = connection.start_next_publication_work().unwrap();
    assert_eq!(finished(&events, paused.handle.id), JobOutcome::Cancelled);
    assert!(matches!(paused.result.result(), Err(CoreError::Cancelled)));
    fixture.workflow.host.close().unwrap();
    assert!(!stage.exists());
}

#[test]
fn connection_successful_suspend_then_close_uses_retained_adoption_tracking_and_retries_cleanup_failure() {
    for fail_cleanup in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let (_connection, _handle, _, stage) = adoption(&fixture);
        let host = &fixture.workflow.host;
        let generation = host.cloud_connection_generation().unwrap();
        let collection = host.collection().unwrap();
        let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
        {
            let db = meguri_storage::rusqlite::Connection::open_with_flags(
                host.cloud_connection_directory().join(c::TRACKING_FILE),
                meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
            )
            .unwrap();
            let version: i64 = db
                .query_row("SELECT version FROM cloud_publication_schema", [], |row| row.get(0))
                .unwrap();
            assert_eq!(version, 4, "actual Runtime installation must use publication V4");
            db.prepare(
                "SELECT binding,epoch_knowledge,status,active_system_fields FROM cloud_root_readability LIMIT 0",
            )
            .expect("actual publication V4 installation must include the root-readability ledger");
        }
        assert_eq!(
            host.engine().suspend(Duration::from_secs(5)).unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
        // Runtime retired its binding and tracking connection; ordinary reader
        // access must stay closed while exact retained-row cleanup succeeds.
        assert!(collection.open_evidence().is_err());
        assert!(stage.exists());
        assert_eq!(host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        assert_eq!(
            fixture.workflow.core.adoption_op_for_host_close(host.engine()).unwrap(),
            Some(op),
            "retained V4 reopen must preserve the exact adoption instead of RecoveryRefused"
        );
        if fail_cleanup {
            let path = collection.path();
            let parked = path.with_extension("cleanup-retry");
            std::fs::rename(path, &parked).unwrap();
            let result = host.close();
            std::fs::rename(&parked, path).unwrap();
            assert!(
                matches!(result, Err(meguri_engine::EngineError::Busy { running_op }) if running_op == "cloud adoption")
            );
            assert_eq!(host.cloud_connection_generation().unwrap(), generation);
            assert!(collection.open_evidence().is_err());
            assert!(stage.exists());
            assert_eq!(host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        }
        host.close().unwrap();
        assert!(!stage.exists());
        assert!(host.engine().jobs().running().is_empty());
        assert_eq!(host.engine().slot_holder(), None);
        assert!(host.cloud_connection_generation().is_err());
    }
}

/// All tracking files, including sidecars and a parked original, must survive
/// refusals unchanged. Reads here never open SQLite at the retained path.
fn tracking_bytes(directory: &std::path::Path) -> std::collections::BTreeMap<std::ffi::OsString, Vec<u8>> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(Result::unwrap)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("meguri-cloud.db"))
        .map(|entry| (entry.file_name(), std::fs::read(entry.path()).unwrap()))
        .collect()
}

#[test]
fn connection_suspended_adoption_close_refuses_missing_replaced_or_older_store_without_file_changes() {
    use std::io::{Seek, SeekFrom, Write};
    for change in [
        "missing",
        "replaced",
        "older",
        "publication-v2",
        "publication-newer",
        "publication-empty",
        "publication-duplicate",
        "publication-missing-ledger",
        "publication-malformed-ledger",
    ] {
        let fixture = Fixture::new(Keys::new([]));
        let (_connection, _handle, _, stage) = adoption(&fixture);
        let host = &fixture.workflow.host;
        let generation = host.cloud_connection_generation().unwrap();
        let collection = host.collection().unwrap();
        assert_eq!(
            host.engine().suspend(Duration::from_secs(5)).unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
        let directory = host.cloud_connection_directory();
        let path = directory.join("meguri-cloud.db");
        // Suspension released all SQLite connections. Preserve the exact
        // original and change only the retained-store authority under test.
        let original = std::fs::read(&path).unwrap();
        let parked = directory.join("meguri-cloud.db.parked-original");
        let publication_change = match change {
            "publication-v2" => Some((
                "UPDATE cloud_publication_schema SET version=2",
                "UPDATE cloud_publication_schema SET version=4",
            )),
            "publication-newer" => Some((
                "UPDATE cloud_publication_schema SET version=5",
                "UPDATE cloud_publication_schema SET version=4",
            )),
            "publication-empty" => Some((
                "DELETE FROM cloud_publication_schema",
                "INSERT INTO cloud_publication_schema VALUES(4)",
            )),
            "publication-duplicate" => Some((
                "INSERT INTO cloud_publication_schema VALUES(4)",
                "DELETE FROM cloud_publication_schema; INSERT INTO cloud_publication_schema VALUES(4)",
            )),
            "publication-missing-ledger" => Some((
                "ALTER TABLE cloud_epoch_crt RENAME TO parked_epoch_crt",
                "ALTER TABLE parked_epoch_crt RENAME TO cloud_epoch_crt",
            )),
            "publication-malformed-ledger" => Some((
                "ALTER TABLE cloud_epoch_crt RENAME COLUMN authority TO parked_authority",
                "ALTER TABLE cloud_epoch_crt RENAME COLUMN parked_authority TO authority",
            )),
            _ => None,
        };
        if let Some((corrupt, _)) = publication_change {
            // Deliberately corrupt the installed schema on the original inode.
            // Drop the fixture writer before snapshotting the refusal evidence.
            let db = meguri_storage::rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(corrupt).unwrap();
        } else if change == "older" {
            // SQLite's persisted big-endian user_version. Keep the same inode
            // and all rows so only the no-migration requirement can reject it.
            let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
            file.seek(SeekFrom::Start(60)).unwrap();
            file.write_all(&2_u32.to_be_bytes()).unwrap();
            file.sync_all().unwrap();
        } else {
            std::fs::rename(&path, &parked).unwrap();
            if change == "replaced" {
                // Identical rows and bytes in a different file are insufficient.
                std::fs::copy(&parked, &path).unwrap();
            }
        }
        let bytes = tracking_bytes(directory);
        for _ in 0..2 {
            assert!(
                matches!(
                    host.close(),
                    Err(meguri_engine::EngineError::CollectionUnavailable { .. })
                ),
                "{change}"
            );
            assert_eq!(
                tracking_bytes(directory),
                bytes,
                "{change} refusal changed tracking files"
            );
            assert_eq!(host.cloud_connection_generation().unwrap(), generation);
            assert_eq!(host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
            assert!(collection.open_evidence().is_err());
            assert!(stage.exists());
        }
        if let Some((_, restore)) = publication_change {
            let db = meguri_storage::rusqlite::Connection::open(&path).unwrap();
            db.execute_batch(restore).unwrap();
        } else if change == "older" {
            std::fs::write(&path, original).unwrap();
        } else {
            if change == "replaced" {
                std::fs::remove_file(&path).unwrap();
            }
            std::fs::rename(&parked, &path).unwrap();
        }
        host.close().unwrap();
        assert!(!stage.exists());
        assert_eq!(host.engine().slot_holder(), None);
    }
}

#[test]
fn connection_host_close_flushes_unrelated_registered_work_before_join_for_ordinary_host_and_live_adopt() {
    use meguri_engine::session::JournalStage;
    use std::sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    };
    for live_adopt in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let adoption = live_adopt.then(|| adoption(&fixture));
        let host = &fixture.workflow.host;
        let events = host.engine().stream().subscribe();
        let (release, flushed) = mpsc::channel();
        let (entered, reached) = mpsc::channel();
        let cancelled = Arc::new(AtomicBool::new(false));
        let witness = cancelled.clone();
        let ordinary = host
            .engine()
            .start_job(JobKind::BulkOp, move |context| {
                entered.send(()).unwrap();
                flushed
                    .recv_timeout(Duration::from_secs(10))
                    .expect("ordinary job needs participant flush");
                witness.store(context.cancel.is_cancelled(), Ordering::SeqCst);
                Ok(())
            })
            .unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let release = Mutex::new(Some(release));
        host.review()
            .journal_hook_for_testing(Arc::new(move |stage| {
                if stage == JournalStage::Compact
                    && let Some(release) = release.lock().unwrap().take()
                {
                    release.send(()).unwrap();
                }
            }))
            .unwrap();
        // On the old all-job early drain this returns the five-second jobs
        // deadline without reaching Compact. Its failure path drops the flush
        // sender and joins the original instead of leaving an admitted worker.
        let result = host.close();
        if result.is_err() {
            host.review().journal_hook_for_testing(Arc::new(|_| {})).unwrap();
        }
        assert_eq!(finished(&events, ordinary.id.0), JobOutcome::Succeeded);
        assert!(cancelled.load(Ordering::SeqCst), "close must cancel before flush");
        result.unwrap();
        assert!(host.engine().jobs().running().is_empty());
        if let Some((_, _, _, stage)) = adoption {
            assert!(!stage.exists());
        }
    }
}

#[test]
fn connection_suspend_timeout_keeps_claimed_reply_then_retry_drains_without_losing_actual_result() {
    use meguri_storage::{ReadRows, RowKey, Table};
    for malformed in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let (connection, handle, active, stage) = adoption(&fixture);
        let host = &fixture.workflow.host;
        let engine = host.engine();
        let collection = host.collection().unwrap();
        let generation = engine.generation();
        let collection_generation = collection.generation();
        let host_generation = host.cloud_connection_generation().unwrap();
        // Copy every stored cell in all collection tables. Each call drops its
        // snapshot and lease before any claim, suspend or terminal wait.
        let rows = || {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            Table::ALL
                .into_iter()
                .map(|table| {
                    let mut rows = Vec::new();
                    let mut after = None;
                    loop {
                        let page = snapshot.row_page(table, after.as_ref()).unwrap();
                        let Some(last) = page.last() else {
                            break;
                        };
                        after = Some(RowKey::from_stored(table, last).unwrap());
                        rows.extend(page);
                    }
                    (table, rows)
                })
                .collect::<Vec<_>>()
        };
        let before = rows();
        let events = engine.stream().subscribe();
        let (claimed, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        fixture
            .workflow
            .core
            .set_adoption_reply_claim_hook_for_testing(move || {
                claimed.send(()).unwrap();
                resume.recv_timeout(Duration::from_secs(20)).unwrap();
            });
        let release = Release(release);
        let read = if malformed {
            CloudControlRead::Missing
        } else {
            CloudControlRead::Found { record: active }
        };
        let job = connection
            .start_adoption_result(handle, CloudAdoptionResult::Read { read })
            .unwrap();
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let timed_out = engine.suspend(Duration::from_millis(100));
        let held_stage = stage.exists();
        let held_generation = engine.generation();
        let held_collection_generation = collection.generation();
        let held_host_generation = host.cloud_connection_generation();
        let held_admission_closed = collection.open_evidence().is_err();
        drop(release);
        let outcome = finished(&events, job.handle.id);
        let result = job.result.result();
        // Observe mandatory abort cleanup before retry/close could perform it.
        let adoption_op = fixture.workflow.core.adoption_op();
        let completed_stage = stage.exists();
        let completed_slot = engine.slot_holder();
        let completed_jobs_empty = engine.jobs().running().is_empty();
        let completed_generation = engine.generation();
        let completed_collection_generation = collection.generation();
        let completed_host_generation = host.cloud_connection_generation();
        let completed_admission_closed = collection.open_evidence().is_err();
        let after = rows();
        let suspended = engine.suspend(Duration::from_secs(5));
        let suspended_stage = stage.exists();
        let closed = host.close();
        // Teardown precedes the expected RED exact-error assertion, so the
        // admission error currently masking Tracking cannot leave a live Host.
        assert!(matches!(timed_out,
            Err(meguri_engine::EngineError::SuspendDeadline { phase }) if phase == "jobs"));
        assert!(held_stage);
        assert_eq!(collection_generation, generation);
        assert_eq!(held_generation, generation);
        assert_eq!(held_collection_generation, collection_generation);
        assert_eq!(held_host_generation.unwrap(), host_generation);
        assert!(held_admission_closed);
        assert_eq!(completed_generation, generation);
        assert_eq!(completed_collection_generation, collection_generation);
        assert_eq!(completed_host_generation.unwrap(), host_generation);
        assert!(completed_admission_closed, "reply completion reopened public admission");
        assert!(completed_jobs_empty);
        assert_eq!(suspended.unwrap(), meguri_engine::SuspendOutcome::Suspended);
        closed.unwrap();
        assert!(!stage.exists());
        assert!(engine.jobs().running().is_empty());
        assert_eq!(engine.slot_holder(), None);
        if malformed {
            assert!(adoption_op.unwrap().is_none());
            assert!(!completed_stage);
            assert_eq!(completed_slot, None);
            assert_eq!(after, before, "malformed reply changed collection rows");
            assert!(matches!(outcome, JobOutcome::Failed { .. }));
            failure_is(result, CloudConnectionFailure::Tracking);
        } else {
            assert!(completed_stage);
            assert!(suspended_stage);
            assert_eq!(outcome, JobOutcome::Succeeded);
            assert!(result.unwrap().unwrap().applied);
        }
    }
}

#[test]
fn connection_initial_identity_completion_after_failed_close_and_successful_retry_rejects_original_owner() {
    let (fixture, _process, _config, identity, _keys) = held_no_collection_host();
    let policy = fixture.workflow.policy().unwrap();
    let host = &fixture.workflow.host;
    let generation = host.cloud_connection_generation().unwrap();
    let (reached, release) = identity.arm();
    std::thread::scope(|scope| {
        let read = scope.spawn(move || policy.refresh_account());
        let release = ReleaseNoCollectionRead(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let (entered, registered) = mpsc::channel();
        let (finish, done) = mpsc::channel();
        host.start_cloud_connection_job(generation, "CloudConnection.adoption_result", move |_| {
            entered.send(()).unwrap();
            done.recv_timeout(Duration::from_secs(20)).unwrap();
            Ok(())
        })
        .unwrap();
        let finish = Release(finish);
        registered.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(matches!(host.close(), Err(meguri_engine::EngineError::SuspendDeadline { phase }) if phase == "jobs"));
        assert_eq!(host.cloud_connection_generation().unwrap(), generation);
        drop(finish);
        host.close().unwrap();
        release.0.send(()).unwrap();
        assert!(
            read.join().unwrap().is_err(),
            "late identity result crossed successful close after retry"
        );
        assert!(host.cloud_connection_generation().is_err());
    });
}

// Reuse the file-identity provider seam. Only the selected cleanup identity
// read fails; no Runtime hook or new production fault point is needed.
struct DiagnosticFiles {
    remaining: std::sync::atomic::AtomicIsize,
    reads: AtomicUsize,
    before_read: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl r::FileIdentityProvider for DiagnosticFiles {
    fn identity(&self, path: &std::path::Path) -> Result<r::NativeFileIdentity, r::RuntimeError> {
        self.reads.fetch_add(1, Ordering::SeqCst);
        let before_read = self.before_read.lock().unwrap().take();
        if let Some(before_read) = before_read {
            before_read();
        }
        let remaining = self
            .remaining
            .try_update(Ordering::SeqCst, Ordering::SeqCst, |value| {
                Some(if value > 0 { value - 1 } else { value })
            })
            .unwrap();
        if remaining == 0 {
            Err(r::RuntimeError::FileIdentityUnavailable)
        } else {
            r::FileIdentityProvider::identity(&NativeFiles, path)
        }
    }
}
fn diagnostic_fixture() -> (Fixture, Arc<DiagnosticFiles>) {
    let mut fixture = Fixture::new(Keys::new([]));
    fixture.workflow.host.close().unwrap();
    let files = Arc::new(DiagnosticFiles {
        remaining: std::sync::atomic::AtomicIsize::new(-1),
        reads: AtomicUsize::new(0),
        before_read: Mutex::new(None),
    });
    let mut config = HostConfig::new(
        fixture._directory.path().join("collection"),
        fixture._directory.path().join("backups"),
        fixture._directory.path().join("widget"),
        fixture.clock.clone(),
        fixture.random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity: Arc::new(Account),
        files: files.clone(),
    });
    let host = Arc::new(CoreHost::open(config).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    core.retain_adapter_inspection();
    fixture.workflow = CloudWorkflow {
        host,
        core,
        identity: None,
    };
    (fixture, files)
}

// Keep the public failure enum visible even when baseline teardown also fails.
fn diagnostic_cleanup_must_succeed(result: Result<(), CoreError>) {
    match result {
        Ok(()) => {},
        Err(CoreError::CloudConnection { failure }) => panic!("adoption cleanup failed: {failure:?}"),
        Err(_) => panic!("adoption cleanup failed with another error family"),
    }
}

fn adoption_validate_work(fixture: &Fixture) -> (Arc<CloudConnection>, Arc<CloudAdoptionWork>, std::path::PathBuf) {
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let (page, action) = adoption_action(&connection);
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
    connection.adoption_result(page, CloudAdoptionResult::Page {
        records: vec![], deleted: vec![], next: None,
    }).unwrap();
    let (local, action) = adoption_action(&connection);
    assert!(matches!(action, CloudAdoptionAction::Validate));
    let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
    assert_eq!(op.step, c::adoption::AdoptStep::Staged);
    let directory = fixture.workflow.host.cloud_connection_directory().join(op.staging_name);
    (connection, local, directory)
}

fn corrupt_adoption_candidate(directory: &std::path::Path) {
    use std::io::Write;
    let path = directory.join("collection.anki2");
    let identity = r::FileIdentityProvider::identity(&NativeFiles, &path).unwrap();
    let length = std::fs::metadata(&path).unwrap().len();
    assert_eq!(&std::fs::read(&path).unwrap()[..16], b"SQLite format 3\0");
    // Overwrite only the synthetic candidate's header, preserving its inode,
    // length and live collection. No SQL or fabricated authority is involved.
    let mut file = std::fs::OpenOptions::new().write(true).open(&path).unwrap();
    file.write_all(b"invalid database").unwrap();
    file.sync_all().unwrap();
    assert_eq!(std::fs::metadata(&path).unwrap().len(), length);
    assert_eq!(r::FileIdentityProvider::identity(&NativeFiles, &path).unwrap(), identity);
}

#[test]
fn connection_adoption_local_corrupt_candidate_reaches_real_validation_and_aborts() {
    let (fixture, files) = diagnostic_fixture();
    let (connection, local, directory) = adoption_validate_work(&fixture);
    corrupt_adoption_candidate(&directory);
    // With the same candidate and a working provider, Validate's actual body
    // refuses it and its mandatory abort succeeds; this is no authority fault.
    failure_is(connection.perform_adoption_local(local), CloudConnectionFailure::Tracking);
    assert_eq!(files.remaining.load(Ordering::SeqCst), -1);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    assert!(connection.recovery_status().unwrap().is_none());
    assert!(!directory.exists());
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
}

fn local_failed_abort_cleanup(body_failure: bool) {
    use meguri_storage::{ReadRows, RowKey, Table};
    for close in [false, true] {
        let (fixture, files) = diagnostic_fixture();
        let (connection, local, directory) = adoption_validate_work(&fixture);
        let collection = fixture.workflow.host.collection().unwrap();
        let generation = collection.generation();
        let identity = r::FileIdentityProvider::identity(&NativeFiles, collection.path()).unwrap();
        let rows = || {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            Table::ALL.into_iter().map(|table| {
                let mut rows = Vec::new();
                let mut after = None;
                loop {
                    let page = snapshot.row_page(table, after.as_ref()).unwrap();
                    let Some(last) = page.last() else { break; };
                    after = Some(RowKey::from_stored(table, last).unwrap());
                    rows.extend(page);
                }
                (table, rows)
            }).collect::<Vec<_>>()
        };
        let before_rows = rows();
        let before = fixture.workflow.core.adoption_op().unwrap().unwrap();
        let operation = connection.recovery_status().unwrap().unwrap().operation;
        let other = fixture.workflow.connection().unwrap();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let reads = files.reads.load(Ordering::SeqCst);
        let worker = connection.clone();
        let original = local.clone();
        let (output, result) = mpsc::channel();
        // Use the actual public Local handle inside a registered Host job.
        // The pool barrier lets us arm the provider only after Host admission.
        let job = fixture.workflow.host.start_cloud_connection_job(
            generation, "CloudConnection.perform_adoption_local", move |_| {
                let typed = worker.perform_adoption_local(original);
                let outcome = typed.as_ref().map(|_| ()).map_err(|_| {
                    meguri_engine::EngineError::CollectionUnavailable { reason: "local adoption failed".into() }
                });
                output.send(typed).unwrap();
                outcome
            },
        ).unwrap();
        assert_eq!(files.reads.load(Ordering::SeqCst), reads);
        assert!(matches!(result.try_recv(), Err(mpsc::TryRecvError::Empty)));
        if body_failure {
            corrupt_adoption_candidate(&directory);
        }
        let bytes = stage_bytes(&directory);
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *files.before_read.lock().unwrap() = Some(Box::new(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(20)).unwrap();
        }));
        // A: authority and abort fail. B: authority succeeds, real Validate
        // rejects the corrupted candidate, then the abort identity read fails.
        files.remaining.store(if body_failure { 1 } else { 0 }, Ordering::SeqCst);
        let release = Release(release);
        drop(pool);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(!connection.recovery_status().unwrap().unwrap().adoption_cancellable);
        failure_is(connection.cancel_operation(operation.clone()), CloudConnectionFailure::Busy);
        failure_is(other.cancel_operation(operation.clone()), CloudConnectionFailure::WrongOwner);
        failure_is(connection.perform_adoption_local(local.clone()), CloudConnectionFailure::AuthorityChanged);
        drop(release);
        assert!(matches!(finished(&events, job.id.0), JobOutcome::Failed { .. }));
        let original_result = result.recv_timeout(Duration::from_secs(5)).unwrap();
        failure_is(original_result.clone(), CloudConnectionFailure::FileIdentityUnavailable);
        assert_eq!(files.reads.load(Ordering::SeqCst), reads + 2);
        let retained = || {
            assert_eq!(fixture.workflow.core.adoption_op().unwrap().as_ref(), Some(&before));
            assert_eq!(stage_bytes(&directory), bytes);
            assert_eq!(fixture.workflow.host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
            assert_eq!(collection.generation(), generation);
            assert_eq!(r::FileIdentityProvider::identity(&NativeFiles, collection.path()).unwrap(), identity);
            assert_eq!(rows(), before_rows);
        };
        retained();
        failure_is(connection.perform_adoption_local(local.clone()), CloudConnectionFailure::AuthorityChanged);
        failure_is(connection.adoption_result(local.clone(), CloudAdoptionResult::Failed), CloudConnectionFailure::AuthorityChanged);
        failure_is(other.cancel_operation(operation.clone()), CloudConnectionFailure::WrongOwner);
        let cancellable = connection.recovery_status().unwrap().unwrap().adoption_cancellable;
        // The test-only patch must fail here with Busy on unchanged R19.
        failure_is(connection.cancel_operation(operation.clone()), CloudConnectionFailure::FileIdentityUnavailable);
        retained();
        assert!(cancellable);
        assert!(connection.recovery_status().unwrap().unwrap().adoption_cancellable);
        files.remaining.store(-1, Ordering::SeqCst);
        assert!(matches!(connection.next_publication_work().unwrap(), CloudPublicationWork::Idle));
        if close {
            fixture.workflow.host.close().unwrap();
        } else {
            diagnostic_cleanup_must_succeed(connection.cancel_operation(operation.clone()));
            failure_is(connection.cancel_operation(operation), CloudConnectionFailure::NotOffered);
            assert!(connection.recovery_status().unwrap().is_none());
            assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
            failure_is(connection.perform_adoption_local(local), CloudConnectionFailure::AuthorityChanged);
            assert_eq!(rows(), before_rows);
            fixture.workflow.host.close().unwrap();
        }
        assert!(!directory.exists());
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
        assert!(fixture.workflow.host.engine().jobs().running().is_empty());
        failure_is(original_result, CloudConnectionFailure::FileIdentityUnavailable);
    }
}

#[test]
fn connection_adoption_local_authority_and_abort_failure_settles_for_cancel_or_drained_close() {
    local_failed_abort_cleanup(false);
}

#[test]
fn connection_adoption_local_validation_and_abort_failure_settles_for_cancel_or_drained_close() {
    local_failed_abort_cleanup(true);
}

#[test]
fn connection_adoption_cancel_before_reply_claim_keeps_original_stale_without_diagnostic() {
    let (fixture, files) = diagnostic_fixture();
    let (connection, handle, _, directory) = adoption(&fixture);
    let operation = connection.recovery_status().unwrap().unwrap().operation;
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let job = connection.clone().start_adoption_result(
        handle,
        CloudAdoptionResult::Read { read: CloudControlRead::Missing },
    ).unwrap();
    let provider_entries = Arc::new(AtomicUsize::new(0));
    let claim_entries = Arc::new(AtomicUsize::new(0));
    let entries = claim_entries.clone();
    fixture.workflow.core.set_adoption_reply_claim_hook_for_testing(move || {
        entries.fetch_add(1, Ordering::SeqCst);
    });
    let entries = provider_entries.clone();
    let (entered, reached) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    *files.before_read.lock().unwrap() = Some(Box::new(move || {
        entries.fetch_add(1, Ordering::SeqCst);
        entered.send(()).unwrap();
        resume.recv_timeout(Duration::from_secs(20)).unwrap();
    }));
    std::thread::scope(|scope| {
        let cancel_connection = connection.clone();
        let cancel = scope.spawn(move || cancel_connection.cancel_operation(operation));
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        let cancellable = connection.recovery_status().unwrap().unwrap().adoption_cancellable;
        drop(pool);
        let outcome = finished(&events, job.handle.id);
        let result = job.result.result();
        let diagnostic = job.result.failure_diagnostic().unwrap();
        let claimed = claim_entries.load(Ordering::SeqCst);
        let provider_entered = provider_entries.load(Ordering::SeqCst);
        assert!(directory.exists());
        assert_eq!(fixture.workflow.host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        drop(release);
        diagnostic_cleanup_must_succeed(cancel.join().unwrap());
        assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
        assert!(connection.recovery_status().unwrap().is_none());
        assert!(!directory.exists());
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
        fixture.workflow.host.close().unwrap();
        assert_eq!(provider_entered, 1);
        assert!(!cancellable);
        assert_eq!(claimed, 0);
        assert!(matches!(outcome, JobOutcome::Failed { .. }));
        failure_is(result, CloudConnectionFailure::AuthorityChanged);
        assert!(diagnostic.is_none());
        failure_is(job.result.result(), CloudConnectionFailure::AuthorityChanged);
        assert!(job.result.failure_diagnostic().unwrap().is_none());
    });
}

#[test]
fn connection_adoption_failed_abort_settles_only_for_public_cancel_or_drained_close() {
    for close in [false, true] {
        let (fixture, files) = diagnostic_fixture();
        let (connection, active, record, directory) = adoption(&fixture);
        connection.adoption_result(active, CloudAdoptionResult::Read {
            read: CloudControlRead::Found { record },
        }).unwrap();
        let (epoch, _) = adoption_action(&connection);
        let before = fixture.workflow.core.adoption_op().unwrap().unwrap();
        let bytes = stage_bytes(&directory);
        let operation = connection.recovery_status().unwrap().unwrap().operation;
        let other = fixture.workflow.connection().unwrap();
        let events = fixture.workflow.host.engine().stream().subscribe();
        let claims = Arc::new(AtomicUsize::new(0));
        let entries = claims.clone();
        let armed = files.clone();
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        fixture.workflow.core.set_adoption_reply_claim_hook_for_testing(move || {
            entries.fetch_add(1, Ordering::SeqCst);
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(20)).unwrap();
            // Authority succeeds; the actual mandatory abort identity read fails.
            armed.remaining.store(1, Ordering::SeqCst);
        });
        let job = connection.clone().start_adoption_result(
            epoch.clone(),
            CloudAdoptionResult::Read { read: CloudControlRead::Found {
                record: CloudControlRecord { encrypted: None, system_fields: vec![] },
            } },
        ).unwrap();
        let release = Release(release);
        reached.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(claims.load(Ordering::SeqCst), 1);
        assert!(!connection.recovery_status().unwrap().unwrap().adoption_cancellable);
        failure_is(connection.cancel_operation(operation.clone()), CloudConnectionFailure::Busy);
        failure_is(other.cancel_operation(operation.clone()), CloudConnectionFailure::WrongOwner);
        assert_eq!(fixture.workflow.core.adoption_op().unwrap().as_ref(), Some(&before));
        assert_eq!(stage_bytes(&directory), bytes);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        drop(release);
        assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
        failure_is(job.result.result(), CloudConnectionFailure::FileIdentityUnavailable);
        let snapshot = CloudAdoptionFailureDiagnostic {
            task: CloudAdoptionDiagnosticTask::ReadEpoch,
            last_entered_stage: CloudAdoptionDiagnosticStage::EpochShape,
            primary_category: Some(CloudAdoptionFailureCategory::Validation),
            cleanup_category: Some(CloudAdoptionFailureCategory::FileIdentityUnavailable),
        };
        assert_eq!(job.result.failure_diagnostic().unwrap(), Some(snapshot.clone()));
        assert_eq!(fixture.workflow.core.adoption_op().unwrap().as_ref(), Some(&before));
        assert_eq!(stage_bytes(&directory), bytes);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        // Settling abort cleanup never reopens the original reply, even before cleanup.
        let replay = connection.clone().start_adoption_result(epoch, CloudAdoptionResult::Failed).unwrap();
        assert!(matches!(finished(&events, replay.handle.id), JobOutcome::Failed { .. }));
        failure_is(replay.result.result(), CloudConnectionFailure::AuthorityChanged);
        assert!(replay.result.failure_diagnostic().unwrap().is_none());
        failure_is(other.cancel_operation(operation.clone()), CloudConnectionFailure::WrongOwner);
        let cancellable = connection.recovery_status().unwrap().unwrap().adoption_cancellable;
        // Baseline must report Busy here, before any teardown failure.
        failure_is(connection.cancel_operation(operation.clone()), CloudConnectionFailure::FileIdentityUnavailable);
        assert_eq!(fixture.workflow.core.adoption_op().unwrap().as_ref(), Some(&before));
        assert_eq!(stage_bytes(&directory), bytes);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
        files.remaining.store(-1, Ordering::SeqCst);
        if close {
            fixture.workflow.host.close().unwrap();
        } else {
            assert!(matches!(connection.next_publication_work().unwrap(), CloudPublicationWork::Idle));
            diagnostic_cleanup_must_succeed(connection.cancel_operation(operation.clone()));
            failure_is(connection.cancel_operation(operation), CloudConnectionFailure::NotOffered);
            assert!(connection.recovery_status().unwrap().is_none());
            assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
            fixture.workflow.host.close().unwrap();
        }
        assert!(cancellable);
        assert!(!directory.exists());
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
        assert_eq!(claims.load(Ordering::SeqCst), 1);
        failure_is(job.result.result(), CloudConnectionFailure::FileIdentityUnavailable);
        assert_eq!(job.result.failure_diagnostic().unwrap(), Some(snapshot));
        failure_is(replay.result.result(), CloudConnectionFailure::AuthorityChanged);
        assert!(replay.result.failure_diagnostic().unwrap().is_none());
    }
}

#[test]
fn connection_adoption_diagnostic_primary_cleanup_and_combined_preserve_original_error_precedence() {
    for (reads, primary, cleanup, returned, stage) in [
        (
            -1,
            Some(CloudAdoptionFailureCategory::Validation),
            None,
            CloudConnectionFailure::Tracking,
            CloudAdoptionDiagnosticStage::EpochShape,
        ),
        (
            0,
            None,
            Some(CloudAdoptionFailureCategory::FileIdentityUnavailable),
            CloudConnectionFailure::FileIdentityUnavailable,
            CloudAdoptionDiagnosticStage::Authority,
        ),
        (
            1,
            Some(CloudAdoptionFailureCategory::Validation),
            Some(CloudAdoptionFailureCategory::FileIdentityUnavailable),
            CloudConnectionFailure::FileIdentityUnavailable,
            CloudAdoptionDiagnosticStage::EpochShape,
        ),
    ] {
        let (fixture, files) = diagnostic_fixture();
        let (connection, active, record, directory) = adoption(&fixture);
        connection
            .adoption_result(
                active,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record },
                },
            )
            .unwrap();
        let (epoch, _) = adoption_action(&connection);
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let job = connection
            .clone()
            .start_adoption_result(
                epoch,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found {
                        record: CloudControlRecord {
                            encrypted: None,
                            system_fields: vec![],
                        },
                    },
                },
            )
            .unwrap();
        assert!(job.result.result().unwrap().is_none());
        assert!(job.result.failure_diagnostic().unwrap().is_none());
        {
            // Reuse the claim seam so all earlier Host/admission reads stay native.
            // The authority check reads first; mandatory abort reads next.
            let armed = files.clone();
            fixture
                .workflow
                .core
                .set_adoption_reply_claim_hook_for_testing(move || {
                    armed.remaining.store(reads, Ordering::SeqCst);
                });
            drop(pool);
            assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
            failure_is(job.result.result(), returned);
            let expected = CloudAdoptionFailureDiagnostic {
                task: CloudAdoptionDiagnosticTask::ReadEpoch,
                last_entered_stage: stage,
                primary_category: primary,
                cleanup_category: cleanup,
            };
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(expected.clone()));
            failure_is(job.result.result(), returned);
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(expected.clone()));
            files.remaining.store(-1, Ordering::SeqCst);
            if cleanup.is_some() {
                assert_eq!(
                    fixture.workflow.host.engine().slot_holder(),
                    Some(meguri_engine::LongOp::CloudAdopt)
                );
                assert!(fixture.workflow.core.adoption_op().unwrap().is_some());
                assert!(directory.exists());
                let operation = connection.recovery_status().unwrap().unwrap().operation;
                diagnostic_cleanup_must_succeed(connection.cancel_operation(operation));
            }
            assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
            assert!(connection.recovery_status().unwrap().is_none());
            assert!(!directory.exists());
            fixture.workflow.host.close().unwrap();
            failure_is(job.result.result(), returned);
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(expected));
        }
    }
}

#[test]
fn connection_adoption_diagnostic_exact_results_remain_isolated_from_followup_and_stale_handles() {
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, _, _) = adoption(&fixture);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let first = connection
        .clone()
        .start_adoption_result(
            handle.clone(),
            CloudAdoptionResult::Read {
                read: CloudControlRead::Failed,
            },
        )
        .unwrap();
    assert!(first.result.failure_diagnostic().unwrap().is_none());
    drop(pool);
    assert!(matches!(finished(&events, first.handle.id), JobOutcome::Failed { .. }));
    failure_is(first.result.result(), CloudConnectionFailure::AccountUnavailable);
    let snapshot = first.result.failure_diagnostic().unwrap().unwrap();
    assert_eq!(
        snapshot.primary_category,
        Some(CloudAdoptionFailureCategory::AccountUnavailable)
    );
    assert_eq!(snapshot.cleanup_category, None);
    let (followup, _) = adoption_action(&connection);
    let second = connection
        .clone()
        .start_adoption_result(
            followup,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Missing,
            },
        )
        .unwrap();
    assert!(matches!(finished(&events, second.handle.id), JobOutcome::Failed { .. }));
    failure_is(second.result.result(), CloudConnectionFailure::Tracking);
    let second_snapshot = second.result.failure_diagnostic().unwrap().unwrap();
    assert_eq!(
        second_snapshot.primary_category,
        Some(CloudAdoptionFailureCategory::Validation)
    );
    assert_ne!(snapshot, second_snapshot);
    let stale = connection
        .start_adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Missing,
            },
        )
        .unwrap();
    assert!(matches!(finished(&events, stale.handle.id), JobOutcome::Failed { .. }));
    assert!(stale.result.result().is_err());
    assert!(stale.result.failure_diagnostic().unwrap().is_none());
    fixture.workflow.host.close().unwrap();
    assert_eq!(first.result.failure_diagnostic().unwrap(), Some(snapshot));
    assert_eq!(second.result.failure_diagnostic().unwrap(), Some(second_snapshot));
}

#[test]
fn connection_adoption_page_failure_keeps_its_diagnostic_after_pre_entry_failed_reply() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let (page, _) = adoption_action(&connection);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let original = connection
        .clone()
        .start_adoption_result(
            page.clone(),
            CloudAdoptionResult::Page {
                records: vec![],
                deleted: vec!["synthetic-deleted".into()],
                next: None,
            },
        )
        .unwrap();
    assert!(original.result.failure_diagnostic().unwrap().is_none());
    drop(pool);
    assert!(matches!(
        finished(&events, original.handle.id),
        JobOutcome::Failed { .. }
    ));
    failure_is(original.result.result(), CloudConnectionFailure::Tracking);
    let snapshot = original.result.failure_diagnostic().unwrap().unwrap();
    assert_eq!(
        snapshot,
        CloudAdoptionFailureDiagnostic {
            task: CloudAdoptionDiagnosticTask::Fetch,
            last_entered_stage: CloudAdoptionDiagnosticStage::PageValidation,
            primary_category: Some(CloudAdoptionFailureCategory::Validation),
            cleanup_category: None,
        }
    );
    assert!(connection.recovery_status().unwrap().is_none());
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    let followup = connection
        .start_adoption_result(page, CloudAdoptionResult::Failed)
        .unwrap();
    assert!(matches!(
        finished(&events, followup.handle.id),
        JobOutcome::Failed { .. }
    ));
    failure_is(followup.result.result(), CloudConnectionFailure::AuthorityChanged);
    assert!(followup.result.failure_diagnostic().unwrap().is_none());
    assert_eq!(original.result.failure_diagnostic().unwrap(), Some(snapshot.clone()));
    fixture.workflow.host.close().unwrap();
    failure_is(original.result.result(), CloudConnectionFailure::Tracking);
    failure_is(followup.result.result(), CloudConnectionFailure::AuthorityChanged);
    assert!(followup.result.failure_diagnostic().unwrap().is_none());
    assert_eq!(original.result.failure_diagnostic().unwrap(), Some(snapshot));
}

#[test]
fn connection_adoption_diagnostic_payload_helper_reports_its_last_entered_stage() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let (mut record, _) = adoption_config_record(b"synthetic-external", true);
    let (page, _) = adoption_action(&connection);
    connection
        .adoption_result(
            page,
            CloudAdoptionResult::Page {
                records: vec![record.clone()],
                deleted: vec![],
                next: None,
            },
        )
        .unwrap();
    let (payload, _) = adoption_action(&connection);
    let missing = fixture._directory.path().join("synthetic-missing-payload");
    record.payload_file = Some(missing.to_str().unwrap().into());
    let events = fixture.workflow.host.engine().stream().subscribe();
    let job = connection
        .start_adoption_result(payload, CloudAdoptionResult::Payload { record })
        .unwrap();
    assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
    assert!(job.result.result().is_err());
    let snapshot = job.result.failure_diagnostic().unwrap().unwrap();
    assert_eq!(snapshot.task, CloudAdoptionDiagnosticTask::FetchPayload);
    assert_eq!(snapshot.last_entered_stage, CloudAdoptionDiagnosticStage::PayloadRead);
    assert!(snapshot.primary_category.is_some());
    assert_eq!(snapshot.cleanup_category, None);
    fixture.workflow.host.close().unwrap();
}

fn diagnostic_epoch_record(fixture: &Fixture) -> CloudStagingRecord {
    let epoch = fixture
        .workflow
        .core
        .adoption_op()
        .unwrap()
        .unwrap()
        .epoch_record
        .unwrap();
    CloudStagingRecord {
        name: "epoch".into(),
        record_type: "MGEpoch".into(),
        record: CloudControlRecord {
            encrypted: epoch.encrypted,
            system_fields: epoch.system_fields.0,
        },
        payload_file: None,
    }
}

fn assert_page_epoch_rejection(stage: CloudAdoptionDiagnosticStage, mutate: impl Fn(&mut CloudStagingRecord)) {
    for cleanup_fails in [false, true] {
        let (fixture, files) = diagnostic_fixture();
        let connection = fixture.workflow.connection().unwrap();
        adoption_fetch_ready(&connection);
        let operation = fixture.workflow.core.adoption_op().unwrap().unwrap();
        let directory = fixture
            .workflow
            .host
            .cloud_connection_directory()
            .join(operation.staging_name);
        let mut record = diagnostic_epoch_record(&fixture);
        mutate(&mut record);
        let (page, _) = adoption_action(&connection);
        let events = fixture.workflow.host.engine().stream().subscribe();
        let pool = hold_pool(&fixture);
        let job = connection
            .clone()
            .start_adoption_result(
                page.clone(),
                CloudAdoptionResult::Page {
                    records: vec![record],
                    deleted: vec![],
                    next: None,
                },
            )
            .unwrap();
        assert!(job.result.result().unwrap().is_none());
        assert!(job.result.failure_diagnostic().unwrap().is_none());
        {
            let armed = files.clone();
            fixture
                .workflow
                .core
                .set_adoption_reply_claim_hook_for_testing(move || {
                    armed
                        .remaining
                        .store(if cleanup_fails { 1 } else { -1 }, Ordering::SeqCst);
                });
            drop(pool);
            assert!(matches!(finished(&events, job.handle.id), JobOutcome::Failed { .. }));
            let returned = if cleanup_fails {
                CloudConnectionFailure::FileIdentityUnavailable
            } else {
                CloudConnectionFailure::Tracking
            };
            failure_is(job.result.result(), returned);
            let snapshot = CloudAdoptionFailureDiagnostic {
                task: CloudAdoptionDiagnosticTask::Fetch,
                last_entered_stage: stage,
                primary_category: Some(CloudAdoptionFailureCategory::Validation),
                cleanup_category: cleanup_fails.then_some(CloudAdoptionFailureCategory::FileIdentityUnavailable),
            };
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(snapshot.clone()));
            failure_is(job.result.result(), returned);
            files.remaining.store(-1, Ordering::SeqCst);
            if cleanup_fails {
                assert_eq!(
                    fixture.workflow.host.engine().slot_holder(),
                    Some(meguri_engine::LongOp::CloudAdopt)
                );
                assert!(fixture.workflow.core.adoption_op().unwrap().is_some());
                assert!(connection.recovery_status().unwrap().is_some());
                assert!(directory.exists());
                let operation = connection.recovery_status().unwrap().unwrap().operation;
                diagnostic_cleanup_must_succeed(connection.cancel_operation(operation));
            }
            assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
            assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
            assert!(connection.recovery_status().unwrap().is_none());
            assert!(!directory.exists());
            let followup = connection
                .clone()
                .start_adoption_result(page, CloudAdoptionResult::Failed)
                .unwrap();
            assert!(matches!(
                finished(&events, followup.handle.id),
                JobOutcome::Failed { .. }
            ));
            failure_is(followup.result.result(), CloudConnectionFailure::AuthorityChanged);
            assert!(followup.result.failure_diagnostic().unwrap().is_none());
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(snapshot.clone()));
            fixture.workflow.host.close().unwrap();
            failure_is(job.result.result(), returned);
            failure_is(followup.result.result(), CloudConnectionFailure::AuthorityChanged);
            assert!(followup.result.failure_diagnostic().unwrap().is_none());
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(snapshot));
        }
    }
}

#[test]
fn connection_adoption_page_epoch_shape_diagnostic_preserves_exact_result_and_cleanup() {
    assert_page_epoch_rejection(CloudAdoptionDiagnosticStage::PageEpochShape, |record| {
        record.payload_file = Some("synthetic-unread-payload".into());
        record.record.encrypted = None;
        record.record.system_fields.clear();
    });
}

#[test]
fn connection_adoption_page_epoch_data_diagnostic_preserves_exact_result_and_cleanup() {
    for encrypted in [None, Some(vec![]), Some(vec![0])] {
        assert_page_epoch_rejection(CloudAdoptionDiagnosticStage::PageEpochData, |record| {
            record.record.encrypted = encrypted.clone();
            record.record.system_fields.clear();
        });
    }
}

#[test]
fn connection_adoption_page_epoch_system_fields_diagnostic_preserves_exact_result_and_cleanup() {
    for system_fields in [vec![], vec![3]] {
        assert_page_epoch_rejection(CloudAdoptionDiagnosticStage::PageEpochSystemFields, |record| {
            record.record.system_fields = system_fields.clone();
        });
    }
}

#[test]
fn connection_adoption_page_epoch_unchanged_projection_is_accepted_without_staging_a_record() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let operation = fixture.workflow.core.adoption_op().unwrap().unwrap();
    let directory = fixture
        .workflow
        .host
        .cloud_connection_directory()
        .join(operation.staging_name);
    let before = stage_bytes(&directory);
    let record = diagnostic_epoch_record(&fixture);
    let (page, _) = adoption_action(&connection);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let job = connection
        .clone()
        .start_adoption_result(
            page,
            CloudAdoptionResult::Page {
                records: vec![record],
                deleted: vec![],
                next: Some(vec![7]),
            },
        )
        .unwrap();
    assert!(job.result.result().unwrap().is_none());
    assert!(job.result.failure_diagnostic().unwrap().is_none());
    drop(pool);
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
    assert!(job.result.result().unwrap().unwrap().applied);
    assert!(job.result.failure_diagnostic().unwrap().is_none());
    assert_eq!(stage_bytes(&directory), before);
    assert!(matches!(adoption_action(&connection).1,
        CloudAdoptionAction::Fetch { cursor: Some(cursor), .. } if cursor == vec![7]));
    assert_eq!(
        fixture.workflow.host.engine().slot_holder(),
        Some(meguri_engine::LongOp::CloudAdopt)
    );
    connection
        .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
        .unwrap();
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    assert!(!directory.exists());
    fixture.workflow.host.close().unwrap();
    assert!(job.result.result().unwrap().unwrap().applied);
    assert!(job.result.failure_diagnostic().unwrap().is_none());
}

fn resume_waiting_adoption(fixture: &Fixture, connection: &Arc<CloudConnection>) -> bool {
    let events = fixture.workflow.host.engine().stream().subscribe();
    let operation = connection.recovery_status().unwrap().unwrap().operation;
    let job = connection.clone().start_resume_adoption(operation).unwrap();
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
    job.result.result().unwrap().unwrap().resumed
}

fn assert_recovery_policy_wait(fixture: &Fixture, waiting: bool, phase: &str) {
    use c::policy::{Attention, Lifecycle};
    let lifecycle = if fixture.workflow.host.collection().is_ok() {
        Lifecycle::Ready
    } else {
        Lifecycle::NoCollection
    };
    let projection = fixture.workflow.core.policy_projection(
        lifecycle, fixture.workflow.host.engine().generation().0,
    ).unwrap();
    assert_eq!(
        projection.facts.summary.attention.get(&Attention::RecoveryOpWaiting).copied(),
        waiting.then_some(1),
        "{phase}",
    );
    assert!(!projection.facts.summary.attention_known, "{phase}");
}

fn prepare_foreign_builds_before_adoption(fixture: &Fixture) -> Vec<r::Runtime> {
    let collection = fixture.workflow.host.collection().unwrap();
    [
        ("synthetic-u114", c::Environment::Production),
        ("synthetic-other-build", c::Environment::Development),
    ].into_iter().map(|(container, environment)| {
        let runtime = r::Runtime::new(
            fixture.workflow.host.cloud_connection_directory().to_path_buf(),
            r::CloudInputs {
                scope: c::BuildScope {
                    container: c::ContainerId::new(container).unwrap(), environment,
                },
                identity: Arc::new(Account),
                files: Arc::new(NativeFiles),
            },
        );
        // Bind with actual native evidence before CloudAdopt closes ordinary
        // collection admission. Retain this scoped store through the real wait.
        runtime.attach_adapter_clock(fixture.clock.clone());
        meguri_engine::ports::OpenStep::run(&runtime, &collection).unwrap();
        assert!(!runtime.policy_projection(c::policy::Lifecycle::Ready, collection.generation().0).unwrap().tracking_stopped);
        assert!(runtime.adoption_op().unwrap().is_none());
        runtime
    }).collect()
}

fn assert_foreign_builds_ignore_adoption_wait(fixture: &Fixture, runtimes: &[r::Runtime]) {
    use c::policy::{Attention, Lifecycle};
    let collection = fixture.workflow.host.collection().unwrap();
    assert_eq!(runtimes.len(), 2);
    for runtime in runtimes {
        let projection = runtime.policy_projection(Lifecycle::Ready, collection.generation().0).unwrap();
        assert!(!projection.tracking_stopped);
        assert!(projection.facts.binding.is_none());
        assert!(runtime.adoption_op().unwrap().is_none());
        assert!(!projection.facts.summary.attention.contains_key(&Attention::RecoveryOpWaiting));
        assert!(!projection.facts.summary.attention_known);
    }
}

#[test]
fn adoption_policy_ready_wait_tracks_review_without_treating_ready_work_as_waiting() {
    use c::entity_codec as codec;
    let fixture = Fixture::new(Keys::new([]));
    fixture.baseline_with_accepted_epoch();
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    // The candidate needs its own scheduler config through real page intake;
    // the live collection's accepted Epoch does not supply candidate rows.
    let key = b"schedVer";
    let payload = codec::Payload::Config(codec::Config {
        key: key.to_vec(),
        mtime_secs: 1,
        val: b"2".to_vec(),
    }).encode().unwrap();
    let entity = codec::Entity::build(codec::EntityFields {
        kind: codec::Kind::Config,
        key: key.to_vec(),
        origin: [1; 16],
        erev: 1,
        generation: None,
        tombstone: false,
        deleted_at_ms: 0,
        writer: [2; 16],
        base_sha256: None,
    }, &payload).unwrap();
    assert!(entity.payload.is_some());
    let record = CloudStagingRecord {
        name: codec::entity_name(&[19; 32], codec::Kind::Config, key).unwrap(),
        record_type: "MGEntity".into(),
        record: CloudControlRecord {
            encrypted: Some(entity.encode().unwrap()),
            system_fields: vec![4],
        },
        payload_file: None,
    };
    let (page, action) = adoption_action(&connection);
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
    connection.adoption_result(page, CloudAdoptionResult::Page {
        records: vec![record], deleted: vec![], next: None,
    }).unwrap();
    let (validate, action) = adoption_action(&connection);
    assert!(matches!(action, CloudAdoptionAction::Validate));
    let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
    assert_eq!(op.step, c::adoption::AdoptStep::Staged);
    let stage = fixture.workflow.host.cloud_connection_directory().join(op.staging_name);
    assert_recovery_policy_wait(&fixture, false, "staged local validation is work");
    connection.perform_adoption_local(validate).unwrap();
    let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
    assert_eq!(op.step, c::adoption::AdoptStep::Ready);
    assert_recovery_policy_wait(&fixture, false, "Ready still has a root recheck to run");
    let (recheck, action) = adoption_action(&connection);
    assert!(matches!(action, CloudAdoptionAction::ReadActive { .. }));
    connection.adoption_result(recheck, CloudAdoptionResult::Read {
        read: CloudControlRead::Found { record: CloudControlRecord {
            encrypted: op.active.encrypted,
            system_fields: op.active.system_fields.0,
        } },
    }).unwrap();
    assert_recovery_policy_wait(&fixture, false, "activation can run without a review");
    let review = fixture.workflow.host.review().begin_unqueued_session().unwrap();
    assert!(matches!(connection.next_publication_work().unwrap(), CloudPublicationWork::Idle));
    assert_recovery_policy_wait(&fixture, true, "Ready waits for review to end");
    assert!(connection.recovery_status().unwrap().unwrap().adoption_cancellable);
    review.end().unwrap();
    assert_recovery_policy_wait(&fixture, false, "review ended; activation is work again");
    let review = fixture.workflow.host.review().begin_unqueued_session().unwrap();
    assert_recovery_policy_wait(&fixture, true, "Ready waits for the new review");
    connection.cancel_operation(connection.recovery_status().unwrap().unwrap().operation).unwrap();
    assert_recovery_policy_wait(&fixture, false, "Cancel retires Ready review wait");
    assert!(!stage.exists());
    review.end().unwrap();
    fixture.workflow.host.close().unwrap();
}

#[test]
fn publication_policy_wait_still_uses_accepted_failure_and_existing_trigger_semantics() {
    let fixture = Fixture::new(keys());
    let (connection, request) = active_request(&fixture);
    assert_recovery_policy_wait(&fixture, false, "publication work is running");
    assert!(connection.submit_publication_request(
        request.clone(), Arc::new(Queue(AtomicUsize::new(0))),
    ).unwrap());
    assert_recovery_policy_wait(&fixture, false, "publication awaits original reply");
    connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }).unwrap();
    assert_recovery_policy_wait(&fixture, true, "publication failure waits");
    connection.trigger().unwrap();
    assert_recovery_policy_wait(&fixture, false, "existing publication trigger releases wait");
    connection.cancel_operation(connection.recovery_status().unwrap().unwrap().operation).unwrap();
    assert_recovery_policy_wait(&fixture, false, "publication Cancel");
    fixture.workflow.host.close().unwrap();
}

#[test]
fn adoption_transport_wait_retains_exact_page_or_payload_until_explicit_resume_after_deadline() {
    for (initial, bound) in [(false, false), (false, true), (true, false)] {
        for payload in [false, true] {
            let fixture = if initial {
                Fixture::without_collection(Keys::new([]))
            } else {
                Fixture::new(Keys::new([]))
            };
            if bound {
                fixture.baseline();
            }
            let foreign_runtimes = if initial {
                Vec::new()
            } else {
                prepare_foreign_builds_before_adoption(&fixture)
            };
            let connection = fixture.workflow.connection().unwrap();
            adoption_fetch_ready(&connection);
            let (page, _) = adoption_action(&connection);
            let (record, bytes) = adoption_config_record(b"waiting-payload", true);
            let continuation = Some(vec![91, 92, 93]);
            let handle = if payload {
                connection
                    .adoption_result(
                        page,
                        CloudAdoptionResult::Page {
                            records: vec![record.clone()],
                            deleted: vec![],
                            next: continuation.clone(),
                        },
                    )
                    .unwrap();
                adoption_action(&connection).0
            } else {
                page
            };
            assert_recovery_policy_wait(&fixture, false, "issued page/payload before failure");
            let original = fixture.workflow.core.adoption_op().unwrap().unwrap();
            let stage = fixture
                .workflow
                .host
                .cloud_connection_directory()
                .join(&original.staging_name);
            let before = stage_bytes(&stage);
            let descriptor = handle.retained_work_for_test().clone();
            let guard = fixture.workflow.core.adapter_guard();
            let events = fixture.workflow.host.engine().stream().subscribe();
            let pool = hold_pool(&fixture);
            let job = connection
                .clone()
                .start_adoption_result(
                    handle.clone(),
                    CloudAdoptionResult::TransportFailed {
                        code: 4,
                        retry_after_duration_ms: Some(120_000),
                    },
                )
                .unwrap();
            assert!(job.result.result().unwrap().is_none());
            assert!(!fixture.workflow.core.adoption_op().unwrap().unwrap().transport_wait);
            assert_eq!(stage_bytes(&stage), before);
            assert_recovery_policy_wait(&fixture, false, "failure result queued, not accepted");
            drop(pool);
            assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
            assert!(job.result.result().unwrap().unwrap().applied);
            let waiting = fixture.workflow.core.adoption_op().unwrap().unwrap();
            assert!(waiting.transport_wait);
            assert_recovery_policy_wait(&fixture, true, "accepted transport failure");
            if !initial {
                assert_foreign_builds_ignore_adoption_wait(&fixture, &foreign_runtimes);
                assert_recovery_policy_wait(&fixture, true, "other builds leave original wait intact");
            }
            assert_eq!(waiting.id, original.id);
            assert_eq!(waiting.candidate, original.candidate);
            assert_eq!(waiting.staging_name, original.staging_name);
            assert_eq!(stage_bytes(&stage), before);
            assert_eq!(fixture.workflow.core.adapter_guard(), guard);
            let status = connection.recovery_status().unwrap().unwrap();
            assert!(status.adoption_transport_wait);
            assert_eq!(status.retry_after_ms, waiting.retry_after_ms);
            assert_eq!(waiting.retry_after_ms, Some(NOW + 120_000));
            if initial {
                assert!(
                    !fixture
                        .workflow
                        .host
                        .cloud_connection_directory()
                        .join("collection.anki2")
                        .exists()
                );
            }
            // Waiting never reports due work, including a replayed failed fact.
            assert!(
                connection
                    .adoption_result(
                        handle,
                        CloudAdoptionResult::TransportFailed {
                            code: 4,
                            retry_after_duration_ms: None,
                        }
                    )
                    .is_err()
            );
            fixture.clock.step_ms(5_000);
            if !initial {
                connection.trigger().unwrap();
            }
            assert!(matches!(
                connection.next_publication_work().unwrap(),
                CloudPublicationWork::Idle
            ));
            assert_recovery_policy_wait(&fixture, true, "before retry deadline");
            assert!(!resume_waiting_adoption(&fixture, &connection));
            assert_recovery_policy_wait(&fixture, true, "early explicit Resume starts nothing");
            assert_eq!(fixture.workflow.core.adoption_op().unwrap().unwrap(), waiting);
            fixture.clock.step_ms(115_001);
            if !initial {
                connection.trigger().unwrap();
            }
            assert!(matches!(
                connection.next_publication_work().unwrap(),
                CloudPublicationWork::Idle
            ));
            assert_recovery_policy_wait(&fixture, true, "deadline expired without Resume");
            assert!(resume_waiting_adoption(&fixture, &connection));
            assert_recovery_policy_wait(&fixture, false, "accepted explicit Resume");
            let (repeated, action) = adoption_action(&connection);
            assert_eq!(repeated.retained_work_for_test(), &descriptor);
            if payload {
                assert!(
                    matches!(action, CloudAdoptionAction::FetchPayload { name, expected, .. }
                    if name == record.name && expected.encrypted == record.record.encrypted && expected.system_fields == record.record.system_fields)
                );
                let file = tempfile::NamedTempFile::new().unwrap();
                std::fs::write(file.path(), bytes).unwrap();
                let mut returned = record;
                returned.payload_file = Some(file.path().to_str().unwrap().into());
                connection
                    .adoption_result(repeated, CloudAdoptionResult::Payload { record: returned })
                    .unwrap();
                let (_, action) = adoption_action(&connection);
                assert!(matches!(action, CloudAdoptionAction::Fetch { cursor, .. } if cursor == continuation));
            } else {
                assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
                connection
                    .adoption_result(
                        repeated,
                        CloudAdoptionResult::Page {
                            records: vec![],
                            deleted: vec![],
                            next: continuation.clone(),
                        },
                    )
                    .unwrap();
                let (_, action) = adoption_action(&connection);
                assert!(matches!(action, CloudAdoptionAction::Fetch { cursor, .. } if cursor == continuation));
            }
            connection
                .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
                .unwrap();
            assert!(!stage.exists());
            assert_recovery_policy_wait(&fixture, false, "Cancel retires the operation");
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn adoption_transport_failure_during_account_unavailability_still_requires_explicit_resume() {
    for unavailable in [
        r::IdentityStatus::ReadFailed,
        r::IdentityStatus::TemporarilyUnavailable,
        r::IdentityStatus::Restricted,
        r::IdentityStatus::Configuration,
    ] {
        for (initial, bound, payload) in [
            (false, false, false), (false, false, true),
            (false, true, false), (false, true, true),
            (true, false, false), (true, false, true),
        ] {
            let account = c::OpaqueBytes(b"synthetic-account-A".to_vec());
            let identity = Arc::new(CachedIdentity(Mutex::new(Some(r::IdentityStatus::Available(
                account.clone(),
            )))));
            let fixture = Fixture::with_collection(Keys::new([]), Some(identity.clone()), !initial);
            if bound {
                fixture.baseline();
            }
            let connection = fixture.workflow.connection().unwrap();
            adoption_fetch_ready(&connection);
            // The original page/payload work is issued while this account is
            // verified. Only its later typed transport failure lacks proof.
            let (page, _) = adoption_action(&connection);
            let handle = if payload {
                let (record, _) = adoption_config_record(b"unavailable-payload", true);
                connection
                    .adoption_result(
                        page,
                        CloudAdoptionResult::Page {
                            records: vec![record],
                            deleted: vec![],
                            next: Some(vec![81, 82]),
                        },
                    )
                    .unwrap();
                adoption_action(&connection).0
            } else {
                page
            };
            assert_recovery_policy_wait(&fixture, false, "issued work with available account");
            let descriptor = handle.retained_work_for_test().clone();
            let original = fixture.workflow.core.adoption_op().unwrap().unwrap();
            let stage = fixture
                .workflow
                .host
                .cloud_connection_directory()
                .join(&original.staging_name);
            let before = stage_bytes(&stage);
            identity.set(unavailable.clone());
            assert_recovery_policy_wait(&fixture, false, "account unavailable before accepted failure");
            let events = fixture.workflow.host.engine().stream().subscribe();
            let job = connection
                .clone()
                .start_adoption_result(
                    handle,
                    CloudAdoptionResult::TransportFailed {
                        code: 4,
                        retry_after_duration_ms: Some(120_000),
                    },
                )
                .unwrap();
            assert_eq!(finished(&events, job.handle.id), JobOutcome::Succeeded);
            assert!(job.result.result().unwrap().unwrap().applied);
            let waiting = fixture.workflow.core.adoption_op().unwrap().unwrap();
            let mut expected = original;
            expected.transport_wait = true;
            expected.retry_after_ms = Some(NOW + 120_000);
            assert_eq!(waiting, expected);
            assert_recovery_policy_wait(&fixture, true, "accepted failure while account unavailable");
            assert_eq!(stage_bytes(&stage), before);
            assert!(matches!(
                connection.next_publication_work().unwrap(),
                CloudPublicationWork::Idle
            ));
            let operation = connection.recovery_status().unwrap().unwrap().operation;
            let resume = connection.clone().start_resume_adoption(operation).unwrap();
            assert!(matches!(finished(&events, resume.handle.id), JobOutcome::Failed { .. }));
            failure_is(resume.result.result(), CloudConnectionFailure::AccountUnavailable);
            assert_recovery_policy_wait(&fixture, true, "Resume without account proof retains wait");
            assert_eq!(fixture.workflow.core.adoption_op().unwrap().unwrap(), waiting);
            identity.set(r::IdentityStatus::Available(account));
            fixture.clock.step_ms(5_000);
            if !initial {
                connection.trigger().unwrap();
            }
            assert!(matches!(
                connection.next_publication_work().unwrap(),
                CloudPublicationWork::Idle
            ));
            assert_recovery_policy_wait(&fixture, true, "before retry deadline after account return");
            assert!(!resume_waiting_adoption(&fixture, &connection));
            assert_recovery_policy_wait(&fixture, true, "early explicit Resume retains wait");
            assert_eq!(fixture.workflow.core.adoption_op().unwrap().unwrap(), waiting);
            fixture.clock.step_ms(115_001);
            if !initial {
                connection.trigger().unwrap();
            }
            assert!(matches!(
                connection.next_publication_work().unwrap(),
                CloudPublicationWork::Idle
            ));
            assert_recovery_policy_wait(&fixture, true, "deadline expired without Resume");
            assert!(resume_waiting_adoption(&fixture, &connection));
            assert_recovery_policy_wait(&fixture, false, "accepted explicit Resume after account return");
            let (repeated, _) = adoption_action(&connection);
            assert_eq!(repeated.retained_work_for_test(), &descriptor);
            assert_eq!(stage_bytes(&stage), before);
            connection
                .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
                .unwrap();
            assert!(!stage.exists());
            assert_recovery_policy_wait(&fixture, false, "Cancel after resumed work");
            fixture.workflow.host.close().unwrap();
        }
    }
}

#[test]
fn adoption_transport_fact_rejects_unallowed_codes_and_nonfetch_tasks() {
    for code in [2, 20, 11, 0, -1, 999] {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        adoption_fetch_ready(&connection);
        let (handle, _) = adoption_action(&connection);
        assert!(
            connection
                .adoption_result(
                    handle,
                    CloudAdoptionResult::TransportFailed {
                        code,
                        retry_after_duration_ms: Some(1),
                    }
                )
                .is_err()
        );
        assert!(connection.recovery_status().unwrap().is_none());
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
        fixture.workflow.host.close().unwrap();
    }
    let fixture = Fixture::new(Keys::new([]));
    let (connection, handle, _, stage) = adoption(&fixture);
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::TransportFailed {
                    code: 4,
                    retry_after_duration_ms: None,
                }
            )
            .is_err()
    );
    assert!(connection.recovery_status().unwrap().is_none());
    assert!(!stage.exists());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn adoption_wait_cancel_drains_queued_resume_and_rejects_late_facts() {
    let fixture = Fixture::without_collection(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let (handle, _) = adoption_action(&connection);
    connection
        .adoption_result(
            handle.clone(),
            CloudAdoptionResult::TransportFailed {
                code: 3,
                retry_after_duration_ms: None,
            },
        )
        .unwrap();
    let waiting = fixture.workflow.core.adoption_op().unwrap().unwrap();
    assert_recovery_policy_wait(&fixture, true, "waiting before queued Resume");
    let stage = fixture
        .workflow
        .host
        .cloud_connection_directory()
        .join(&waiting.staging_name);
    let events = fixture.workflow.host.engine().stream().subscribe();
    let pool = hold_pool(&fixture);
    let operation = connection.recovery_status().unwrap().unwrap().operation;
    let job = connection.clone().start_resume_adoption(operation.clone()).unwrap();
    assert!(job.result.result().unwrap().is_none() && stage.exists());
    job.handle.cancel.cancel().unwrap();
    drop(pool);
    assert_eq!(finished(&events, job.handle.id), JobOutcome::Cancelled);
    assert!(job.result.result().is_err());
    assert_eq!(fixture.workflow.core.adoption_op().unwrap().unwrap(), waiting);
    assert_recovery_policy_wait(&fixture, true, "cancelled Resume retains transport wait");
    connection.cancel_operation(operation).unwrap();
    assert_recovery_policy_wait(&fixture, false, "Cancel clears transport wait");
    assert!(!stage.exists());
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::TransportFailed {
                    code: 4,
                    retry_after_duration_ms: None,
                }
            )
            .is_err()
    );
    assert!(connection.recovery_status().unwrap().is_none());
    fixture.workflow.host.close().unwrap();
}
