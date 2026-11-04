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
        let CloudPublicationWork::Adoption {
            action: CloudAdoptionAction::Read { target },
            ..
        } = connection.next_publication_work().unwrap()
        else {
            panic!("followup retained");
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
        assert_eq!(
            host.engine().suspend(Duration::from_secs(5)).unwrap(),
            meguri_engine::SuspendOutcome::Suspended
        );
        // Runtime retired its binding and tracking connection; ordinary reader
        // access must stay closed while exact retained-row cleanup succeeds.
        assert!(collection.open_evidence().is_err());
        assert!(stage.exists());
        assert_eq!(host.engine().slot_holder(), Some(meguri_engine::LongOp::CloudAdopt));
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
    for change in ["missing", "replaced", "older"] {
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
        if change == "older" {
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
        if change == "older" {
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
    before_read: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl r::FileIdentityProvider for DiagnosticFiles {
    fn identity(&self, path: &std::path::Path) -> Result<r::NativeFileIdentity, r::RuntimeError> {
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
        std::thread::scope(|scope| {
            // A failed abort leaves the reply claim consumed. Admit public
            // cancellation before that claim, then hold its cleanup read until
            // the fault is disabled; a fresh post-failure cancel would be Busy.
            let cancellation = if cleanup.is_some() {
                let operation = connection.recovery_status().unwrap().unwrap().operation;
                let (entered, reached) = mpsc::channel();
                let (release, resume) = mpsc::channel();
                *files.before_read.lock().unwrap() = Some(Box::new(move || {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(20)).unwrap();
                }));
                let cancel_connection = connection.clone();
                let cancel = scope.spawn(move || cancel_connection.cancel_operation(operation));
                let release = Release(release);
                reached.recv_timeout(Duration::from_secs(5)).unwrap();
                Some((release, cancel))
            } else {
                None
            };
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
            if let Some((release, cancel)) = cancellation {
                assert_eq!(
                    fixture.workflow.host.engine().slot_holder(),
                    Some(meguri_engine::LongOp::CloudAdopt)
                );
                assert!(connection.recovery_status().unwrap().is_some());
                drop(release);
                cancel.join().unwrap().unwrap();
            }
            assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
            assert!(connection.recovery_status().unwrap().is_none());
            assert!(!directory.exists());
            fixture.workflow.host.close().unwrap();
            failure_is(job.result.result(), returned);
            assert_eq!(job.result.failure_diagnostic().unwrap(), Some(expected));
        });
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
        std::thread::scope(|scope| {
            // Admit cancellation before the reply claim, as in the control-read
            // diagnostic cases, so a failed abort can be drained publicly.
            let cancellation = if cleanup_fails {
                let operation = connection.recovery_status().unwrap().unwrap().operation;
                let (entered, reached) = mpsc::channel();
                let (release, resume) = mpsc::channel();
                *files.before_read.lock().unwrap() = Some(Box::new(move || {
                    entered.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(20)).unwrap();
                }));
                let cancel_connection = connection.clone();
                let cancel = scope.spawn(move || cancel_connection.cancel_operation(operation));
                let release = Release(release);
                reached.recv_timeout(Duration::from_secs(5)).unwrap();
                Some((release, cancel))
            } else {
                None
            };
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
            if let Some((release, cancel)) = cancellation {
                assert_eq!(
                    fixture.workflow.host.engine().slot_holder(),
                    Some(meguri_engine::LongOp::CloudAdopt)
                );
                assert!(fixture.workflow.core.adoption_op().unwrap().is_some());
                assert!(connection.recovery_status().unwrap().is_some());
                assert!(directory.exists());
                drop(release);
                cancel.join().unwrap().unwrap();
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
        });
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
