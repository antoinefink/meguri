//! U132 policy and real writer maintenance on disposable common fixtures.
mod common;
use common::{Harness, NOW_MS};
use meguri_engine::{
    CancelToken, Engine, EngineConfig, EngineError, FailedReason, LifecycleState, LongOp, OpSource, OpSpec, OpenStep,
    Ports,
    device_state::{DeviceState, LastQuickCheck},
    executor::catalog,
    maintenance::{
        MaintenanceTrigger as Trigger,
        integrity::{Verdict, optimize_due, quick_check_due, verdict},
        optimize::optimize,
    },
};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    faultpoint::{self, Fault},
    time::EpochMillis,
};
use meguri_storage::{ReadRows, ReaderRole, StorageError};
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

fn trigger(h: &Harness, trigger: Trigger) {
    h.engine.run_due_maintenance(trigger).unwrap();
    h.engine.maintenance().wait_idle_for_testing();
}
fn state(h: &Harness) -> DeviceState {
    DeviceState::for_collection(&h.path)
}
fn raw_writer(c: &meguri_engine::Collection, sql: &'static str) -> meguri_storage::raw::RawRows {
    c.submit_maintenance(catalog::ANALYZE, Duration::from_secs(1), move |writer| {
        Ok(writer.execute_raw(sql, &[])?)
    })
    .wait()
    .unwrap()
}

#[test]
fn cadence_uses_whole_seconds_and_later_record_and_backward_clock() {
    for (elapsed, due) in [(1_209_599, false), (1_209_600, true), (-1, false)] {
        assert_eq!(optimize_due(NOW_MS + elapsed * 1000, Some(NOW_MS), None), due);
        assert_eq!(
            optimize_due(NOW_MS + elapsed * 1000 + 999, None, Some(NOW_MS + 999)),
            due
        );
    }
    assert!(!optimize_due(NOW_MS + 1_209_600_000, Some(NOW_MS), Some(NOW_MS + 1000)));
    assert!(!optimize_due(NOW_MS + 1_209_600_000, Some(NOW_MS + 1000), Some(NOW_MS)));
    assert!(!optimize_due(NOW_MS, None, None));
}

#[test]
fn first_idle_evaluation_sets_the_origin_and_only_due_completion_sets_success() {
    let h = Harness::open();
    trigger(&h, Trigger::Opened);
    let initial = state(&h).maintenance_records();
    assert_eq!(initial.cadence_start.unwrap(), Some(EpochMillis(NOW_MS)));
    assert_eq!(initial.optimize.unwrap(), None);
    let status = h.engine.maintenance_status().unwrap();
    assert_eq!(status.last_optimize.unwrap(), None);
    h.clock.step_ms(1_209_599_000);
    trigger(&h, Trigger::BackgroundProcessing);
    assert_eq!(state(&h).last_optimize().unwrap(), None);
    h.clock.step_ms(1000);
    trigger(&h, Trigger::BackgroundProcessing);
    let completed_at = EpochMillis(h.clock.now_ms());
    assert_eq!(state(&h).last_optimize().unwrap(), Some(completed_at));
}

#[test]
fn malformed_cadence_records_stay_untouched_and_other_status_fields_are_independent() {
    for key in ["lastOptimize", "optimizeCadenceStart"] {
        let h = Harness::open();
        state(&h)
            .set_last_quick_check(&LastQuickCheck {
                instant: NOW_MS,
                passed: true,
            })
            .unwrap();
        let sidecar = h.dir.path().join("meguri-sidecar.json");
        meguri_storage::sidecar::update_document(&sidecar, |doc| {
            doc["deviceState"]
                .as_object_mut()
                .unwrap()
                .insert(key.into(), serde_json::json!({"unknown": 7}));
            Ok(())
        })
        .unwrap();
        let before = std::fs::read(&sidecar).unwrap();
        trigger(&h, Trigger::BackgroundProcessing);
        let status = h.engine.maintenance_status().unwrap();
        assert!(status.last_optimize.is_err());
        assert!(status.last_quick_check.unwrap().unwrap().passed);
        assert!(status.last_backup.is_ok());
        assert!(status.last_check_database.is_ok());
        assert_eq!(std::fs::read(&sidecar).unwrap(), before);
    }
}

#[test]
fn daily_quick_check_threshold_rollback_and_unclean_retry() {
    for (elapsed, expected) in [(86_399, false), (86_400, true), (-1, true)] {
        assert_eq!(quick_check_due(NOW_MS + elapsed * 1000, Some(NOW_MS), false), expected);
    }
    assert!(quick_check_due(NOW_MS, None, false));
    assert!(quick_check_due(NOW_MS, Some(NOW_MS), true));
    let h = Harness::open();
    trigger(&h, Trigger::Opened);
    let first = state(&h).last_quick_check().unwrap().unwrap();
    h.clock.step_ms(86_399_000);
    trigger(&h, Trigger::Background);
    assert_eq!(state(&h).last_quick_check().unwrap().unwrap(), first);
    h.clock.step_ms(1000);
    trigger(&h, Trigger::BackgroundProcessing);
    assert_eq!(state(&h).last_quick_check().unwrap().unwrap().instant, h.clock.now_ms());
    h.clock.set_now_ms(NOW_MS - 1000);
    trigger(&h, Trigger::BackgroundProcessing);
    assert_eq!(state(&h).last_quick_check().unwrap().unwrap().instant, NOW_MS - 1000);

    h.engine.close().unwrap();
    meguri_storage::sidecar::Sidecar::update(&h.dir.path().join("meguri-sidecar.json"), |s, _| {
        s.clean_shutdown = false
    })
    .unwrap();
    let c = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(h.engine.previous_clean_shutdown(), Some(false));
    faultpoint::arm(
        "storage.maintenance.quick_check",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    h.engine.run_due_maintenance(Trigger::Opened).unwrap();
    faultpoint::disarm("storage.maintenance.quick_check");
    assert_eq!(state(&h).last_quick_check().unwrap().unwrap().instant, NOW_MS - 1000);
    h.clock.step_ms(1000);
    h.engine.run_due_maintenance(Trigger::Background).unwrap();
    assert_eq!(state(&h).last_quick_check().unwrap().unwrap().instant, NOW_MS);
    assert!(c.reader(ReaderRole::Interactive).unwrap().col().is_ok());
}

#[test]
fn verdicts_distinguish_corruption_from_no_verdict() {
    use meguri_storage::integrity::IntegrityReport;
    assert_eq!(
        verdict(&Ok(IntegrityReport {
            diagnostics: vec!["ok".into()]
        })),
        Some(Verdict::Sound)
    );
    assert_eq!(
        verdict(&Ok(IntegrityReport {
            diagnostics: vec!["broken btree".into()]
        })),
        Some(Verdict::Corrupt)
    );
    for (diagnostics, expected) in [
        (vec![], Verdict::Corrupt),
        (vec!["OK"], Verdict::Corrupt),
        (vec!["ok", "damaged page"], Verdict::Sound),
        (vec!["ok", "ok"], Verdict::Sound),
    ] {
        assert_eq!(
            verdict(&Ok(IntegrityReport {
                diagnostics: diagnostics.into_iter().map(str::to_owned).collect(),
            })),
            Some(expected)
        );
    }
    for code in [7, 9, 10, 13, 5, 11, 26] {
        let result = Err(StorageError::Sqlite {
            code,
            extended_code: code | 256,
            message: "injected outcome".into(),
        });
        assert_eq!(
            verdict(&result),
            if [11, 26].contains(&code) {
                Some(Verdict::Corrupt)
            } else {
                None
            }
        );
    }
}

#[test]
fn quit_deadline_and_long_operations_do_not_evaluate_idle_maintenance() {
    let h = Harness::open();
    trigger(&h, Trigger::Quit);
    trigger(&h, Trigger::Deadline);
    assert!(state(&h).last_quick_check().unwrap().is_none());
    assert!(state(&h).maintenance_records().cadence_start.unwrap().is_none());
    let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
    trigger(&h, Trigger::BackgroundProcessing);
    assert!(state(&h).last_quick_check().unwrap().is_none());
    assert!(state(&h).maintenance_records().cadence_start.unwrap().is_none());
    drop(slot);
    trigger(&h, Trigger::BackgroundProcessing);
    assert!(state(&h).last_quick_check().unwrap().is_some());
}

#[test]
fn real_optimize_preserves_rows_stamps_history_and_temp_store() {
    let h = Harness::open();
    common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["optimize", "back"]);
    let before = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let rows = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    optimize(&h.collection, &CancelToken::new()).unwrap();
    let after = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!(
        (before.modified, before.schema_modified, before.usn, before.last_sync),
        (after.modified, after.schema_modified, after.usn, after.last_sync)
    );
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM notes"), rows);
    assert_eq!(common::query_int(&h.collection, "PRAGMA freelist_count"), 0);
    assert!(
        h.collection
            .reader(ReaderRole::Maintenance)
            .unwrap()
            .quick_check()
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        raw_writer(&h.collection, "PRAGMA temp_store").rows,
        vec![vec![meguri_storage::raw::SqlValue::Integer(2)]]
    );
    // A normal op can still begin and commit: no transaction leaked.
    h.collection
        .run(OpSpec::named("after optimize", OpSource::UserUndoable), |_| Ok(()))
        .unwrap();
}

#[test]
fn cancelled_and_failed_optimizers_record_no_success_and_restore_temp_store() {
    let h = Harness::open();
    state(&h)
        .set_optimize_cadence_start(EpochMillis(NOW_MS - 1_209_600_000))
        .unwrap();
    faultpoint::arm("storage.maintenance.vacuum", Fault::IoError(std::io::ErrorKind::Other));
    trigger(&h, Trigger::BackgroundProcessing);
    faultpoint::disarm("storage.maintenance.vacuum");
    assert_eq!(state(&h).last_optimize().unwrap(), None);
    assert_eq!(
        raw_writer(&h.collection, "PRAGMA temp_store").rows,
        vec![vec![meguri_storage::raw::SqlValue::Integer(2)]]
    );
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(optimize(&h.collection, &cancel), Err(EngineError::Cancelled));
    assert_eq!(state(&h).last_optimize().unwrap(), None);
    assert!(matches!(h.engine.state(), LifecycleState::Ready { .. }));
}

struct StepCounter(Arc<AtomicUsize>);
impl OpenStep for StepCounter {
    fn name(&self) -> &str {
        "integrity-open-counter"
    }
    fn run(&self, _: &meguri_engine::Collection) -> Result<(), EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[test]
fn recorded_corruption_skips_open_steps_keeps_bytes_and_refuses_writes() {
    let h = Harness::open();
    state(&h)
        .set_last_quick_check(&LastQuickCheck {
            instant: NOW_MS,
            passed: false,
        })
        .unwrap();
    h.engine.close().unwrap();
    let before = std::fs::read(&h.path).unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let c = h
        .engine
        .open_raw(
            &h.path,
            common::open_config(&h.clock).ports(Ports::new().open_step(Arc::new(StepCounter(calls.clone())))),
        )
        .unwrap();
    assert!(matches!(
        h.engine.state(),
        LifecycleState::Failed {
            reason: FailedReason::Corrupt { .. }
        }
    ));
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(std::fs::read(&h.path).unwrap(), before);
    assert!(
        c.reader(ReaderRole::Maintenance)
            .unwrap()
            .quick_check()
            .unwrap()
            .is_ok()
    );
    assert_eq!(
        c.run(OpSpec::named("refused", OpSource::UserUndoable), |_| Ok(()))
            .unwrap_err(),
        EngineError::CollectionCorrupt
    );
    assert_eq!(optimize(&c, &CancelToken::new()), Err(EngineError::CollectionCorrupt));
    assert!(h.engine.maintenance_status().unwrap().remedies.unwrap().is_empty());
}

#[test]
fn status_reads_actual_main_and_wal_and_registered_media_without_scanning() {
    let h = Harness::open();
    let media = meguri_engine::media::MediaService::open(&h.collection, h.clock.clone()).unwrap();
    media.add("first.txt", b"abc").unwrap();
    media.add("second.txt", b"12345").unwrap();
    media.store().trash(&["second.txt".into()]).unwrap();
    let before = media.store().diagnostics();
    let sidecar = std::fs::read(h.dir.path().join("meguri-sidecar.json")).unwrap();
    let status = h.engine.maintenance_status().unwrap();
    let main = std::fs::metadata(&h.path).unwrap().len();
    let wal = std::fs::metadata(h.path.with_file_name("collection.anki2-wal"))
        .map(|m| m.len())
        .unwrap_or(0);
    assert_eq!(status.collection_bytes.unwrap(), main + wal);
    assert_eq!(status.media_bytes.unwrap(), 3);
    assert_eq!(status.trash_bytes.unwrap(), 5);
    assert_eq!(media.store().diagnostics(), before);
    assert_eq!(
        std::fs::read(h.dir.path().join("meguri-sidecar.json")).unwrap(),
        sidecar
    );
    h.engine.close().unwrap();
    assert!(matches!(
        h.engine.maintenance_status(),
        Err(EngineError::CollectionUnavailable { .. })
    ));
}

#[test]
fn sqlite_cancels_during_vacuum_without_leaking_callback_or_transaction() {
    let h = Harness::open();
    h.collection
        .run(OpSpec::named("vacuum input", OpSource::UserUndoable), |ctx| {
            for _ in 0..1500 {
                ctx.scope().add_note(&common::note(&["cancel input", "back"]))?;
            }
            Ok(())
        })
        .unwrap();
    let before = common::query_int(&h.collection, "SELECT count(*) FROM notes");
    let calls = Arc::new(AtomicUsize::new(0));
    let checked = calls.clone();
    let result = h
        .collection
        .submit_maintenance(catalog::VACUUM, Duration::from_secs(30), move |writer| {
            // The first call precedes VACUUM; the later calls come from SQLite
            // while it is executing its VM program, on the real executor writer.
            writer
                .vacuum(&|| checked.fetch_add(1, Ordering::SeqCst) >= 8)
                .map_err(Into::into)
        })
        .wait();
    assert!(matches!(
        result,
        Err(EngineError::Storage(StorageError::Sqlite { code: 9, .. }))
    ));
    assert!(calls.load(Ordering::SeqCst) >= 9);
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM notes"), before);
    assert_eq!(
        raw_writer(&h.collection, "PRAGMA temp_store").rows,
        vec![vec![meguri_storage::raw::SqlValue::Integer(2)]]
    );
    h.collection
        .run(
            OpSpec::named("after interrupted vacuum", OpSource::UserUndoable),
            |ctx| {
                ctx.scope().add_note(&common::note(&["unrelated committed", "back"]))?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        common::query_int(&h.collection, "SELECT count(*) FROM notes"),
        before + 1
    );
    assert!(
        h.collection
            .reader(ReaderRole::Maintenance)
            .unwrap()
            .quick_check()
            .unwrap()
            .is_ok()
    );
}

fn hold_in_sqlite() {
    use std::io::{BufRead, Write};
    println!("U132_IN_SQLITE");
    std::io::stdout().flush().unwrap();
    let mut release = String::new();
    std::io::stdin().lock().read_line(&mut release).unwrap();
}
#[test]
fn vacuum_kill_worker() {
    let Some(path) = std::env::var_os("MEGURI_U132_KILL_PATH") else {
        return;
    };
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine
        .open_raw(std::path::Path::new(&path), common::open_config(&clock))
        .unwrap();
    collection
        .run(OpSpec::named("before kill", OpSource::UserUndoable), |ctx| {
            for _ in 0..1500 {
                ctx.scope().add_note(&common::note(&["u132 committed", "back"]))?;
            }
            Ok(())
        })
        .unwrap();
    faultpoint::arm("storage.maintenance.progress", Fault::Callback(hold_in_sqlite));
    optimize(&collection, &CancelToken::new()).unwrap();
    panic!("parent should kill during VACUUM");
}
#[test]
fn process_kill_in_vacuum_reopens_with_committed_rows_and_integrity() {
    use std::{
        io::{BufRead, BufReader},
        process::{Command, Stdio},
    };
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let mut child = Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "vacuum_kill_worker", "--nocapture"])
        .env("MEGURI_U132_KILL_PATH", &path)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = BufReader::new(child.stdout.take().unwrap());
    let mut line = String::new();
    loop {
        line.clear();
        assert!(
            stdout.read_line(&mut line).unwrap() > 0,
            "worker ended before entering SQLite"
        );
        if line.starts_with("U132_IN_SQLITE") {
            break;
        }
    }
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let h = Harness::open_path(dir, path);
    assert_eq!(
        common::query_int(
            &h.collection,
            "SELECT count(*) FROM notes WHERE flds LIKE 'u132 committed%'"
        ),
        1500
    );
    h.engine.close().unwrap();
    assert!(meguri_storage::integrity::integrity_check(&h.path).unwrap().is_ok());
}

#[test]
fn real_restore_clears_recorded_corruption_in_new_generation_and_survives_reopen() {
    use meguri_storage::space::StatvfsSpace;
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let backup_dir = common::backup_directory(&path);
    let backups = backup_dir.path().to_path_buf();
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock.clone(), Arc::new(StatvfsSpace))
            .unwrap();
    let backup = host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    let device = DeviceState::for_collection(&path);
    device
        .set_last_quick_check(&LastQuickCheck {
            instant: NOW_MS,
            passed: false,
        })
        .unwrap();
    host.close().unwrap();
    std::fs::write(&path, b"unreadable collection for recovery ownership").unwrap();
    let damaged = std::fs::read(&path).unwrap();
    let old_journal = std::fs::read(path.with_file_name("meguri-pending-answers.jsonl")).unwrap();
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock.clone(), Arc::new(StatvfsSpace))
            .unwrap();
    if let Ok(reader) = host.collection().unwrap().reader(ReaderRole::Query) {
        assert!(reader.col().is_err());
    }
    assert!(matches!(
        host.engine().state(),
        LifecycleState::Failed {
            reason: FailedReason::Corrupt { .. }
        }
    ));
    assert_eq!(
        host.engine().maintenance_status().unwrap().remedies.unwrap(),
        vec![meguri_engine::maintenance::status::Remedy::Restore]
    );
    let generation = host.engine().generation();
    clock.step_ms(1000);
    let restored = host
        .restore_backup(&backup.name, &CancelToken::new(), &meguri_engine::ProgressSink::new())
        .unwrap();
    assert_eq!(restored.generation.0, generation.0 + 1);
    let archived = std::fs::read_dir(dir.path())
        .unwrap()
        .map(Result::unwrap)
        .find(|entry| entry.file_name().to_string_lossy().ends_with(".recovery.jsonl"))
        .expect("old production journal archived");
    assert_eq!(std::fs::read(archived.path()).unwrap(), old_journal);
    host.review().flush_for_background().unwrap();
    assert!(restored.cloud_publication_pending);
    assert_eq!(
        std::fs::read(backups.join(restored.safety_backup.as_ref().unwrap())).unwrap(),
        damaged
    );
    assert_eq!(
        device.publication().unwrap().unwrap().decision,
        meguri_engine::device_state::PublicationDecision::Pending
    );
    assert_eq!(
        device.last_quick_check().unwrap(),
        Some(LastQuickCheck {
            instant: clock.now_ms(),
            passed: true
        })
    );
    assert!(matches!(host.engine().state(), LifecycleState::Ready { .. }));
    host.close().unwrap();
    let reopened =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock, Arc::new(StatvfsSpace)).unwrap();
    assert!(matches!(reopened.engine().state(), LifecycleState::Ready { .. }));
    reopened.close().unwrap();
}

static OPTIMIZE_BARRIER: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
fn stop_before_vacuum() {
    let (entered, release) = OPTIMIZE_BARRIER.lock().unwrap().take().unwrap();
    entered.send(()).unwrap();
    release.recv().unwrap();
}
#[test]
fn quick_check_then_real_backup_finish_before_trigger_return_while_optimizer_continues() {
    use meguri_storage::space::StatvfsSpace;
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let backup_dir = common::backup_directory(&path);
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, backup_dir.path(), clock, Arc::new(StatvfsSpace))
            .unwrap();
    let device = DeviceState::for_collection(&path);
    device
        .set_optimize_cadence_start(EpochMillis(NOW_MS - 1_209_600_000))
        .unwrap();
    let (entered, recv) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    *OPTIMIZE_BARRIER.lock().unwrap() = Some((entered, wait));
    faultpoint::arm("storage.maintenance.vacuum", Fault::Callback(stop_before_vacuum));
    host.engine().run_due_maintenance(Trigger::Opened).unwrap();
    recv.recv_timeout(Duration::from_secs(10)).unwrap();
    // The writer is deliberately occupied, yet this read never queues on it.
    let status = host.engine().maintenance_status().unwrap();
    assert_eq!(
        status.last_quick_check.unwrap(),
        Some(LastQuickCheck {
            instant: NOW_MS,
            passed: true
        })
    );
    assert!(status.last_backup.unwrap().is_some());
    assert!(status.last_optimize.unwrap().is_none());
    assert!(
        host.engine()
            .threads()
            .threads()
            .iter()
            .any(|t| t.qos == meguri_engine::Qos::Utility && t.busy)
    );
    release.send(()).unwrap();
    host.engine().maintenance().wait_idle_for_testing();
    faultpoint::disarm("storage.maintenance.vacuum");
    assert_eq!(device.last_optimize().unwrap(), Some(EpochMillis(NOW_MS)));
    host.close().unwrap();
}

static QUICK_BARRIER: Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>> = Mutex::new(None);
fn stop_before_quick_check() {
    let (entered, release) = QUICK_BARRIER.lock().unwrap().take().unwrap();
    entered.send(()).unwrap();
    release.recv().unwrap();
}
#[test]
fn pending_quick_check_does_not_hold_the_interactive_reader_or_writer() {
    let h = Harness::open();
    let (entered, recv) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    *QUICK_BARRIER.lock().unwrap() = Some((entered, wait));
    faultpoint::arm(
        "storage.maintenance.quick_check",
        Fault::Callback(stop_before_quick_check),
    );
    let engine = h.engine.clone();
    let work = h
        .engine
        .threads()
        .spawn("u132-opened-trigger", meguri_engine::Qos::Utility, move |_| {
            engine.run_due_maintenance(Trigger::Opened)
        })
        .unwrap();
    recv.recv_timeout(Duration::from_secs(10)).unwrap();
    assert!(h.collection.reader(ReaderRole::Interactive).unwrap().col().is_ok());
    h.collection
        .run(
            OpSpec::named("interactive during quick check", OpSource::UserUndoable),
            |_| Ok(()),
        )
        .unwrap();
    release.send(()).unwrap();
    work.join().unwrap().unwrap();
    faultpoint::disarm("storage.maintenance.quick_check");
}

#[test]
fn session_start_cancels_a_pending_check_and_idle_triggers_skip_an_active_session() {
    use meguri_storage::space::StatvfsSpace;
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let backup_dir = common::backup_directory(&path);
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, backup_dir.path(), clock, Arc::new(StatvfsSpace))
            .unwrap();
    let (entered, recv) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    *QUICK_BARRIER.lock().unwrap() = Some((entered, wait));
    faultpoint::arm(
        "storage.maintenance.quick_check",
        Fault::Callback(stop_before_quick_check),
    );
    let engine = host.engine().clone();
    let work = engine
        .threads()
        .clone()
        .spawn("u132-check-cancel", meguri_engine::Qos::Utility, move |_| {
            engine.run_due_maintenance(Trigger::BackgroundProcessing)
        })
        .unwrap();
    recv.recv_timeout(Duration::from_secs(10)).unwrap();
    let session = host.review().begin_unqueued_session().unwrap();
    release.send(()).unwrap();
    work.join().unwrap().unwrap();
    faultpoint::disarm("storage.maintenance.quick_check");
    let device = DeviceState::for_collection(&path);
    assert!(device.last_quick_check().unwrap().is_none());
    host.engine()
        .run_due_maintenance(Trigger::BackgroundProcessing)
        .unwrap();
    assert!(device.maintenance_records().cadence_start.unwrap().is_none());
    session.end().unwrap();
    host.engine()
        .run_due_maintenance(Trigger::BackgroundProcessing)
        .unwrap();
    assert!(device.last_quick_check().unwrap().unwrap().passed);
    host.close().unwrap();
}

fn sibling(path: &std::path::Path, suffix: &str) -> std::path::PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    name.into()
}
fn recovery_bytes(path: &std::path::Path) -> Vec<Option<Vec<u8>>> {
    ["", "-wal", "-journal"]
        .map(|suffix| match std::fs::read(sibling(path, suffix)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("reading recovery evidence: {error}"),
        })
        .into()
}
type DiagnosticEvidence = (Option<Vec<u8>>, Option<(u64, u64)>);
fn diagnostic_evidence(path: &std::path::Path) -> Vec<DiagnosticEvidence> {
    use std::os::unix::fs::MetadataExt;
    ["", "-wal", "-journal", "-shm"]
        .map(|suffix| {
            let path = sibling(path, suffix);
            match std::fs::symlink_metadata(&path) {
                Ok(metadata) => (
                    Some(std::fs::read(path).unwrap()),
                    Some((metadata.dev(), metadata.ino())),
                ),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => (None, None),
                Err(error) => panic!("reading diagnostic evidence: {error}"),
            }
        })
        .into()
}
fn assert_diagnostic_evidence(path: &std::path::Path, expected: &[DiagnosticEvidence], phase: &str, case: &str) {
    use sha2::{Digest, Sha256};
    let digest = |bytes: &Option<Vec<u8>>| {
        bytes.as_ref().map(|bytes| {
            let hash = Sha256::digest(bytes)
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            (bytes.len(), hash)
        })
    };
    for ((actual, expected), suffix) in diagnostic_evidence(path)
        .iter()
        .zip(expected)
        .zip(["", "-wal", "-journal", "-shm"])
    {
        assert!(
            actual.0 == expected.0,
            "{phase}: {case} {suffix:?}: actual size/hash {:?}, expected {:?}",
            digest(&actual.0),
            digest(&expected.0),
        );
        assert_eq!(actual.1, expected.1, "{phase}: {case} {suffix:?}: native identity");
    }
}
fn record_corrupt(path: &std::path::Path) {
    DeviceState::for_collection(path)
        .set_last_quick_check(&LastQuickCheck {
            instant: NOW_MS,
            passed: false,
        })
        .unwrap();
}

#[test]
fn recorded_corruption_preserves_upgrade_mode_hot_journal_and_live_wal() {
    for case in ["upgrade", "delete", "hot", "wal"] {
        let (dir, path) = common::copy_fixture("schema-18.anki2");
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let publisher = Engine::new(EngineConfig::default()).unwrap();
        publisher.open_raw(&path, common::open_config(&clock)).unwrap();
        publisher.close().unwrap();
        drop(publisher);
        // Publication belongs to the place. Install the recovery bytes only
        // after its real lifecycle initialized that identity; do not upgrade
        // the schema-17 evidence before the recorded-corrupt no-write open.
        if case == "upgrade" {
            std::fs::copy(common::fixture("schema-17.anki2"), &path).unwrap();
        } else {
            let (_source_dir, source) = common::copy_fixture("schema-18.anki2");
            let db = meguri_storage::raw::PlainDatabase::open(&source).unwrap();
            if case == "wal" {
                db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE col SET mod=987654321;")
                    .unwrap();
                std::fs::copy(&source, &path).unwrap();
                std::fs::copy(sibling(&source, "-wal"), sibling(&path, "-wal")).unwrap();
            } else {
                db.execute_batch("PRAGMA journal_mode=DELETE").unwrap();
                if case == "hot" {
                    db.execute_batch(
                        "PRAGMA cache_size=2; BEGIN IMMEDIATE;
                        UPDATE col SET mod=987654321;
                        CREATE TABLE recovery_spill(payload BLOB);
                        WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i+1 FROM n WHERE i<400)
                        INSERT INTO recovery_spill SELECT zeroblob(2000) FROM n;",
                    )
                    .unwrap();
                }
                std::fs::copy(&source, &path).unwrap();
                if case == "hot" {
                    std::fs::copy(sibling(&source, "-journal"), sibling(&path, "-journal")).unwrap();
                    db.execute_batch("ROLLBACK").unwrap();
                    assert!(std::fs::metadata(sibling(&path, "-journal")).unwrap().len() > 0);
                }
            }
            db.close().unwrap();
        }
        record_corrupt(&path);
        let original = diagnostic_evidence(&path);
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let c = engine
            .open_raw(
                &path,
                common::open_config(&clock).ports(Ports::new().open_step(Arc::new(StepCounter(calls.clone())))),
            )
            .unwrap();
        assert!(matches!(
            engine.state(),
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. }
            }
        ));
        assert_eq!(calls.load(Ordering::SeqCst), 0);
        assert_diagnostic_evidence(&path, &original, "open", case);
        {
            let reader = c.reader(ReaderRole::Maintenance);
            if case == "hot" {
                // Recovery requires writes; diagnostic failure must keep the owner.
                assert!(reader.as_ref().map(|r| r.quick_check().is_err()).unwrap_or(true));
            } else {
                let reader = reader.unwrap();
                assert!(reader.quick_check().unwrap().is_ok());
                if case == "wal" {
                    assert_eq!(reader.col().unwrap().modified.0, 987654321);
                }
            }
        }
        assert!(engine.maintenance_status().is_ok());
        assert_diagnostic_evidence(&path, &original, "read/status", case);
        // The advisory lock is held even if no SQLite probe succeeded.
        let other = Engine::new(EngineConfig::default()).unwrap();
        assert!(other.open_raw(&path, common::open_config(&clock)).is_err());
        engine.close().unwrap();
        assert_diagnostic_evidence(&path, &original, "close", case);
        let reopened = other.open_raw(&path, common::open_config(&clock)).unwrap();
        drop(reopened);
        drop(other);
        assert_diagnostic_evidence(&path, &original, "drop", case);
        drop(dir);
    }
}

// Damage a real b-tree root after a checkpoint. The maintenance reader has
// never cached that page. No SQL writes or repair are used to simulate verdicts.
fn damage_root(collection: &meguri_engine::Collection, path: &std::path::Path, table: &str) {
    use std::io::{Seek, SeekFrom, Write};
    let root = common::query_int(
        collection,
        &format!("SELECT rootpage FROM sqlite_master WHERE name='{table}'"),
    );
    let page_size = common::query_int(collection, "PRAGMA page_size");
    raw_writer(collection, "PRAGMA wal_checkpoint(TRUNCATE)");
    assert!(root > 1);
    let mut file = std::fs::OpenOptions::new().write(true).open(path).unwrap();
    file.seek(SeekFrom::Start(u64::try_from((root - 1) * page_size).unwrap()))
        .unwrap();
    file.write_all(&[0]).unwrap();
    file.sync_all().unwrap();
}

#[test]
fn live_corrupt_close_suspend_and_drop_preserve_nonempty_wal_and_release_locks() {
    for shutdown_case in ["close", "suspend-close", "drop", "fallback", "failed-suspend-drop"] {
        let harness = Harness::open();
        damage_root(&harness.collection, &harness.path, "revlog");
        raw_writer(&harness.collection, "PRAGMA wal_autocheckpoint=0");
        raw_writer(&harness.collection, "UPDATE col SET mod=987654321");
        trigger(&harness, Trigger::Background);
        std::fs::write(sibling(&harness.path, "-journal"), b"retained rollback evidence").unwrap();
        assert!(matches!(
            harness.engine.state(),
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. }
            }
        ));
        assert!(!state(&harness).last_quick_check().unwrap().unwrap().passed);
        let evidence = recovery_bytes(&harness.path);
        assert!(evidence[1].as_ref().is_some_and(|bytes| bytes.len() > 32));
        assert!(meguri_storage::lockfile::CollectionLock::acquire(harness.dir.path()).is_err());
        match shutdown_case {
            "close" => harness.engine.close().unwrap(),
            "suspend-close" => {
                harness.engine.suspend(Duration::from_secs(5)).unwrap();
                assert_eq!(recovery_bytes(&harness.path), evidence);
                harness.engine.close().unwrap();
            }
            "failed-suspend-drop" => {
                assert!(harness.engine.suspend(Duration::ZERO).is_err());
            }
            "drop" => {}
            "fallback" => harness.collection.stop_writer_for_testing(),
            _ => unreachable!(),
        }
        let Harness {
            dir: directory,
            engine,
            collection,
            ..
        } = harness;
        drop(collection);
        drop(engine);
        assert_eq!(
            recovery_bytes(&directory.path().join("collection.anki2")),
            evidence,
            "{shutdown_case}"
        );
        let lock = meguri_storage::lockfile::CollectionLock::acquire(directory.path()).unwrap();
        let probe = meguri_storage::raw::PlainDatabase::open(&directory.path().join("collection.anki2")).unwrap();
        probe
            .execute_batch("PRAGMA busy_timeout=0; BEGIN IMMEDIATE; ROLLBACK;")
            .unwrap();
        probe.close().unwrap();
        drop(lock);
    }
}

#[test]
fn corruption_rejects_active_and_queued_writes_without_publishing_changes() {
    let harness = Harness::open();
    // Finish opening work before recording the state that must survive rollback.
    harness
        .collection
        .run(OpSpec::named("Rollover", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    let media_service = meguri_engine::media::MediaService::open(&harness.collection, harness.clock.clone()).unwrap();
    let mut staged_media = media_service.begin_staging().unwrap();
    staged_media.add("attachment.png", b"unpublished attachment").unwrap();
    let original_note_count = common::query_int(&harness.collection, "SELECT count(*) FROM notes");
    let original_collection = harness.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let original_undo = harness.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap();
    let original_sequence = harness.collection.last_seq();
    let subscription = harness.engine.stream().subscribe();
    damage_root(&harness.collection, &harness.path, "revlog");
    let (started_sender, started_receiver) = mpsc::channel();
    let (resume_sender, resume_receiver) = mpsc::channel();
    let active_write =
        harness
            .collection
            .submit(OpSpec::named("active write", OpSource::UserUndoable), move |context| {
                let note_id = context
                    .scope()
                    .add_note(&common::note(&["<img src='attachment.png'>", "back"]))?;
                context.commit_media(staged_media, &[note_id])?;
                started_sender.send(()).unwrap();
                resume_receiver.recv().unwrap();
                Ok(())
            });
    started_receiver.recv_timeout(Duration::from_secs(10)).unwrap();
    let queued_write = harness
        .collection
        .submit(OpSpec::named("queued write", OpSource::UserUndoable), |_| Ok(()));
    harness.engine.run_due_maintenance(Trigger::Background).unwrap();
    assert!(matches!(
        harness.engine.state(),
        LifecycleState::Failed {
            reason: FailedReason::Corrupt { .. }
        }
    ));
    assert!(!state(&harness).last_quick_check().unwrap().unwrap().passed);
    resume_sender.send(()).unwrap();
    assert_eq!(active_write.wait().unwrap_err(), EngineError::CollectionCorrupt);
    assert_eq!(queued_write.wait().unwrap_err(), EngineError::CollectionCorrupt);
    assert_eq!(
        common::query_int(&harness.collection, "SELECT count(*) FROM notes"),
        original_note_count
    );
    let current_collection = harness.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert_eq!(
        (
            current_collection.modified,
            current_collection.schema_modified,
            current_collection.usn
        ),
        (
            original_collection.modified,
            original_collection.schema_modified,
            original_collection.usn
        )
    );
    assert_eq!(harness.collection.last_seq(), original_sequence);
    assert_eq!(
        harness.collection.peek_undo(meguri_engine::UndoScope::Global).unwrap(),
        original_undo
    );
    assert!(common::change_seqs(&common::drain(&subscription, Duration::ZERO)).is_empty());
    assert!(!media_service.store().media_dir().join("attachment.png").exists());
    assert!(state(&harness).last_backup().unwrap().is_none());
    assert!(state(&harness).last_optimize().unwrap().is_none());
    assert_eq!(
        harness
            .collection
            .run(OpSpec::named("later write", OpSource::UserUndoable), |_| Ok(()))
            .unwrap_err(),
        EngineError::CollectionCorrupt
    );
}

#[test]
fn fresh_corrupt_host_refused_restore_close_and_drop_keep_recovery_ownership() {
    use meguri_storage::space::StatvfsSpace;
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let backup_dir = common::backup_directory(&path);
    let backups = backup_dir.path().to_path_buf();
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock.clone(), Arc::new(StatvfsSpace))
            .unwrap();
    let backup = host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    host.close().unwrap();
    drop(host);
    record_corrupt(&path);
    std::fs::write(&path, b"damaged header").unwrap();
    let original = recovery_bytes(&path);
    for explicit_close in [true, false] {
        let host =
            meguri_testkit::backup_support::open_restore_host(&path, &backups, clock.clone(), Arc::new(StatvfsSpace))
                .unwrap();
        let generation = host.engine().generation();
        faultpoint::arm(
            "replace.before_live_wal_check",
            Fault::IoError(std::io::ErrorKind::Other),
        );
        let refused = host.restore_backup(&backup.name, &CancelToken::new(), &meguri_engine::ProgressSink::new());
        faultpoint::disarm("replace.before_live_wal_check");
        assert!(refused.is_err());
        assert_eq!(host.engine().generation(), generation);
        assert!(host.engine().slot_holder().is_none());
        assert!(matches!(
            host.engine().state(),
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. }
            }
        ));
        assert!(host.engine().maintenance_status().is_ok());
        assert_eq!(recovery_bytes(&path), original);
        if explicit_close {
            host.close().unwrap();
        }
        drop(host);
        assert_eq!(recovery_bytes(&path), original);
    }
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock, Arc::new(StatvfsSpace)).unwrap();
    let restored = host
        .restore_backup(&backup.name, &CancelToken::new(), &meguri_engine::ProgressSink::new())
        .unwrap();
    assert!(restored.cloud_publication_pending);
    host.close().unwrap();
}

struct OpeningStatus(Engine, Arc<AtomicUsize>);
impl OpenStep for OpeningStatus {
    fn name(&self) -> &str {
        "opening status refusal"
    }
    fn run(&self, _: &meguri_engine::Collection) -> Result<(), EngineError> {
        assert!(matches!(
            self.0.maintenance_status(),
            Err(EngineError::CollectionUnavailable { .. })
        ));
        self.1.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}
#[test]
fn status_refuses_no_collection_opening_and_exclusive() {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    assert!(matches!(
        engine.maintenance_status(),
        Err(EngineError::CollectionUnavailable { .. })
    ));
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let calls = Arc::new(AtomicUsize::new(0));
    engine
        .open_raw(
            &path,
            common::open_config(&clock)
                .ports(Ports::new().open_step(Arc::new(OpeningStatus(engine.clone(), calls.clone())))),
        )
        .unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert!(engine.maintenance_status().is_ok());
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let exclusive = slot.enter_exclusive(true).unwrap();
    assert!(matches!(
        engine.maintenance_status(),
        Err(EngineError::CollectionUnavailable { .. })
    ));
    drop(exclusive);
    drop(slot);
    engine.close().unwrap();
}

#[test]
fn restore_remedy_filters_raw_and_unverified_entries_but_accepts_local_backup_and_server_copy() {
    use meguri_engine::maintenance::status::Remedy;
    use meguri_storage::space::StatvfsSpace;
    let (dir, path) = common::copy_fixture("schema-18.anki2");
    let backup_dir = common::backup_directory(&path);
    let backups = backup_dir.path().to_path_buf();
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock.clone(), Arc::new(StatvfsSpace))
            .unwrap();
    let receipt = host.engine().back_up_now(&CancelToken::default()).unwrap().receipt;
    host.close().unwrap();
    drop(host);
    let saved = dir.path().join("saved.colpkg");
    std::fs::rename(backups.join(&receipt.name), &saved).unwrap();
    record_corrupt(&path);
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock, Arc::new(StatvfsSpace)).unwrap();
    assert!(host.engine().maintenance_status().unwrap().remedies.unwrap().is_empty());
    for name in [
        "corrupt-2030-01-15-10.00.00.anki2",
        "corrupt-2030-01-15-10.00.00.anki2-wal",
        "server-copy-2030-01-15-10.00.00.unverified.anki2",
    ] {
        std::fs::copy(&path, backups.join(name)).unwrap();
        assert!(host.engine().maintenance_status().unwrap().remedies.unwrap().is_empty());
    }
    std::fs::copy(&saved, backups.join(&receipt.name)).unwrap();
    assert_eq!(
        host.engine().maintenance_status().unwrap().remedies.unwrap(),
        vec![Remedy::Restore]
    );
    std::fs::remove_file(backups.join(&receipt.name)).unwrap();
    std::fs::copy(&saved, backups.join("server-copy-2030-01-15-10.00.00.colpkg")).unwrap();
    assert_eq!(
        host.engine().maintenance_status().unwrap().remedies.unwrap(),
        vec![Remedy::Restore]
    );
    host.close().unwrap();
}

#[test]
fn sqlite_heap_worker() {
    if std::env::var_os("MEGURI_U132_HEAP_WORKER").is_none() {
        return;
    }
    let h = Harness::open();
    let note = common::note(&[&"a".repeat(16384), "back"]);
    h.collection
        .run(
            OpSpec::new(catalog::BULK_OP, OpSource::UserNonUndoable).declare_long_write(Duration::from_secs(30)),
            move |ctx| {
                for _ in 0..2048 {
                    ctx.scope().add_note(&note)?;
                }
                Ok(())
            },
        )
        .unwrap();
    raw_writer(&h.collection, "PRAGMA wal_checkpoint(TRUNCATE)");
    let file_size = std::fs::metadata(&h.path).unwrap().len();
    // SQLite's limit is process-global and cannot be raised with this pragma.
    // The parent invokes this worker in its own test process, also in nextest.
    let saw_file = h
        .collection
        .submit_maintenance(catalog::VACUUM, Duration::from_secs(30), move |writer| {
            writer.execute_raw("PRAGMA cache_size=-1024", &[])?;
            writer.execute_raw("PRAGMA shrink_memory", &[])?;
            let limit = writer.execute_raw("PRAGMA hard_heap_limit=16777216", &[])?;
            assert_eq!(limit.rows, vec![vec![meguri_storage::raw::SqlValue::Integer(16777216)]]);
            assert!(file_size > 16777216);
            let calls = AtomicUsize::new(0);
            let observed = AtomicUsize::new(0);
            writer.vacuum(&|| {
                // with_cancel asks once before starting the SQLite statement.
                if calls.fetch_add(1, Ordering::SeqCst) == 0 {
                    assert_eq!(
                        writer.execute_raw("PRAGMA temp_store", &[]).unwrap().rows,
                        vec![vec![meguri_storage::raw::SqlValue::Integer(1)]]
                    );
                    observed.store(1, Ordering::SeqCst);
                }
                false
            })?;
            assert_eq!(
                writer.execute_raw("PRAGMA temp_store", &[])?.rows,
                vec![vec![meguri_storage::raw::SqlValue::Integer(2)]]
            );
            Ok(observed.load(Ordering::SeqCst))
        })
        .wait()
        .unwrap();
    assert_eq!(saw_file, 1);
    assert_eq!(common::query_int(&h.collection, "PRAGMA freelist_count"), 0);
    {
        let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
        // This worker's global heap cap applies to the final reader too.
        reader.execute_raw("PRAGMA cache_size=-1024", &[]).unwrap();
        assert_eq!(
            reader.execute_raw("PRAGMA cache_size", &[]).unwrap().rows,
            vec![vec![meguri_storage::raw::SqlValue::Integer(-1024)]]
        );
        assert_eq!(
            reader.execute_raw("PRAGMA hard_heap_limit", &[]).unwrap().rows,
            vec![vec![meguri_storage::raw::SqlValue::Integer(16777216)]]
        );
        assert!(reader.quick_check().unwrap().is_ok());
        assert_eq!(
            reader.execute_raw("PRAGMA hard_heap_limit", &[]).unwrap().rows,
            vec![vec![meguri_storage::raw::SqlValue::Integer(16777216)]]
        );
    }
    h.engine.close().unwrap();
}
#[test]
fn in_place_vacuum_uses_file_temporary_storage_below_database_size_heap_limit() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "sqlite_heap_worker", "--nocapture", "--test-threads=1"])
        .env("MEGURI_U132_HEAP_WORKER", "1")
        .status()
        .unwrap();
    assert!(status.success(), "isolated SQLite heap-limit worker failed");
}

#[test]
fn automatic_optimizer_queued_and_active_session_and_suspend_cancellation_is_scoped() {
    use meguri_storage::space::StatvfsSpace;
    for queued in [true, false] {
        for suspend in [false, true] {
            let (_dir, path) = common::copy_fixture("schema-18.anki2");
            let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
            let backup_dir = common::backup_directory(&path);
            let host = Arc::new(
                meguri_testkit::backup_support::open_restore_host(
                    &path,
                    backup_dir.path(),
                    clock,
                    Arc::new(StatvfsSpace),
                )
                .unwrap(),
            );
            let c = host.collection().unwrap();
            c.run(
                OpSpec::named("optimizer race input", OpSource::UserNonUndoable),
                |ctx| {
                    for _ in 0..100 {
                        ctx.scope()
                            .add_note(&common::note(&["optimizer cancellation", "back"]))?;
                    }
                    Ok(())
                },
            )
            .unwrap();
            host.engine()
                .run_due_maintenance(Trigger::BackgroundProcessing)
                .unwrap();
            // Only U132 cadence is evaluated by BackgroundProcessing. No backup
            // retry or interval timing is involved in this race.
            DeviceState::for_collection(&path)
                .set_optimize_cadence_start(EpochMillis(NOW_MS - 1_209_600_000))
                .unwrap();
            let (entered, waiting) = mpsc::channel();
            let (release, blocked) = mpsc::channel();
            let blocker = if queued {
                Some(c.submit(
                    OpSpec::named("optimizer queue blocker", OpSource::UserUndoable),
                    move |_| {
                        entered.send(()).unwrap();
                        blocked.recv().unwrap();
                        Ok(())
                    },
                ))
            } else {
                *OPTIMIZE_BARRIER.lock().unwrap() = Some((entered, blocked));
                faultpoint::arm("storage.maintenance.progress", Fault::Callback(stop_before_vacuum));
                None
            };
            if queued {
                waiting.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            // For active cancellation, keep the quick check out of the progress
            // barrier so the barrier witnesses the actual VACUUM program.
            DeviceState::for_collection(&path)
                .set_last_quick_check(&LastQuickCheck {
                    instant: NOW_MS,
                    passed: true,
                })
                .unwrap();
            host.engine()
                .run_due_maintenance(Trigger::BackgroundProcessing)
                .unwrap();
            if queued {
                // The optimizer is scheduled independently of the occupied writer.
                c.wait_queued_op_for_testing(catalog::VACUUM.name());
            } else {
                waiting.recv_timeout(Duration::from_secs(10)).unwrap();
            }
            let control_host = host.clone();
            let control = host
                .engine()
                .threads()
                .spawn_owned("u132-control-race", meguri_engine::Qos::Utility, "jobs", move |_| {
                    if suspend {
                        control_host.engine().suspend(Duration::from_secs(30)).unwrap();
                        None
                    } else {
                        Some(control_host.review().begin_unqueued_session().unwrap())
                    }
                })
                .unwrap();
            host.engine().maintenance().wait_optimizer_cancelled_for_testing();
            if !queued {
                faultpoint::disarm("storage.maintenance.progress");
            }
            release.send(()).unwrap();
            if let Some(blocker) = blocker {
                blocker.wait().unwrap();
            }
            let session = control.join().unwrap();
            host.engine().maintenance().wait_idle_for_testing();
            if suspend {
                host.engine().resume().unwrap();
            }
            if let Some(session) = session {
                session.end().unwrap();
            }
            assert!(DeviceState::for_collection(&path).last_optimize().unwrap().is_none());
            c.run(
                OpSpec::named("later unrelated operation", OpSource::UserUndoable),
                |ctx| {
                    ctx.scope().add_note(&common::note(&[
                        "ordinary operation survives late cancellation",
                        "back",
                    ]))?;
                    Ok(())
                },
            )
            .unwrap();
            assert_eq!(
                raw_writer(&c, "PRAGMA temp_store").rows,
                vec![vec![meguri_storage::raw::SqlValue::Integer(2)]]
            );
            assert!(
                c.reader(ReaderRole::Maintenance)
                    .unwrap()
                    .quick_check()
                    .unwrap()
                    .is_ok()
            );
            host.close().unwrap();
        }
    }
}

static CORRUPT_OPTIMIZE_CALLS: AtomicUsize = AtomicUsize::new(0);
fn count_corrupt_optimize() {
    CORRUPT_OPTIMIZE_CALLS.fetch_add(1, Ordering::SeqCst);
}

#[test]
fn live_corrupt_verdict_prevents_due_periodic_backup_and_optimizer_on_production_host() {
    use meguri_storage::space::StatvfsSpace;
    let (_dir, path) = common::copy_fixture("schema-18.anki2");
    let backup_dir = common::backup_directory(&path);
    let backups = backup_dir.path().to_path_buf();
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host =
        meguri_testkit::backup_support::open_restore_host(&path, &backups, clock, Arc::new(StatvfsSpace)).unwrap();
    let c = host.collection().unwrap();
    DeviceState::for_collection(&path)
        .set_optimize_cadence_start(EpochMillis(NOW_MS - 1_209_600_000))
        .unwrap();
    damage_root(&c, &path, "revlog");
    CORRUPT_OPTIMIZE_CALLS.store(0, Ordering::SeqCst);
    faultpoint::arm("storage.maintenance.vacuum", Fault::Callback(count_corrupt_optimize));
    host.engine().run_due_maintenance(Trigger::Background).unwrap();
    host.engine().maintenance().wait_idle_for_testing();
    faultpoint::disarm("storage.maintenance.vacuum");
    assert_eq!(CORRUPT_OPTIMIZE_CALLS.load(Ordering::SeqCst), 0);
    assert!(matches!(
        host.engine().state(),
        LifecycleState::Failed {
            reason: FailedReason::Corrupt { .. }
        }
    ));
    let records = DeviceState::for_collection(&path).maintenance_records();
    assert!(!records.quick_check.unwrap().unwrap().passed);
    assert!(records.backup.unwrap().is_none());
    assert!(records.optimize.unwrap().is_none());
    match std::fs::read_dir(&backups) {
        Ok(entries) => assert_eq!(entries.map(Result::unwrap).count(), 0),
        Err(error) => assert_eq!(error.kind(), std::io::ErrorKind::NotFound),
    }
    assert_eq!(
        c.run(
            OpSpec::named("write to damaged page owner", OpSource::UserUndoable),
            |_| Ok(())
        )
        .unwrap_err(),
        EngineError::CollectionCorrupt
    );
    assert_eq!(optimize(&c, &CancelToken::new()), Err(EngineError::CollectionCorrupt));
    host.close().unwrap();
}
