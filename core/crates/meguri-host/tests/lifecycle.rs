use std::{path::Path, sync::Arc};

use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{ReadRows, ReaderRole, space::StatvfsSpace};

fn config(root: &Path) -> HostConfig {
    let collection = root.join("collection");
    let backups = root.join("backups");
    std::fs::create_dir_all(&collection).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/preservation/kitchen-sink.anki2"),
        collection.join("collection.anki2"),
    )
    .unwrap();
    HostConfig::new(
        collection,
        backups,
        root.join("app-group/widget.snapshot"),
        Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC")),
        Arc::new(SeededRandom::new(38)),
        Arc::new(StatvfsSpace),
    )
}

#[test]
fn open_wires_derived_and_media_and_close_releases_retained_journal_handles() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let collection = host.collection().unwrap();
    assert!(collection.reader(ReaderRole::Query).unwrap().col().is_ok());
    // D5 queues binding on the derived writer; collection Ready does not wait for its file.
    assert!(host.derived().wait_idle(std::time::Duration::from_secs(5)));
    assert!(host.derived().path().expect("derived writer bound its file").is_file());
    let media = host.media().unwrap();
    media.add("owned.txt", b"synthetic media").unwrap();
    assert_eq!(media.store().pending().unwrap().len(), 1);
    host.close().unwrap();
    assert!(collection.reader(ReaderRole::Query).is_err());
    assert!(media.store().pending().is_err(), "all clones share the closed journal");
    assert!(std::fs::metadata(dir.path().join("collection/collection.anki2-wal")).map_or(true, |m| m.len() == 0));
}

#[test]
fn host_close_unwind_retains_active_session_and_accepts_another_answer() {
    use meguri_engine::{
        EventKind, LifecycleState, OpSource, OpSpec,
        session::{AnswerOutcome, SessionEvent, StartResult},
    };
    use meguri_model::time::EpochMillis;
    use meguri_sched::answer::Rating;

    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let c = host.collection().unwrap();
    c.run(OpSpec::named("close unwind policy", OpSource::Maintenance), |ctx| {
        ctx.scope()
            .set_config("fsrs", &meguri_storage::ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &meguri_storage::ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let deck = meguri_engine::decks::add(&c, "Close unwind".into()).unwrap().value.id;
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("Close unwind".into()),
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &c,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["close unwind front".into(), "close unwind back".into()],
            tags: vec![],
            deck_id: deck,
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    let StartResult::Started { session_id, head, .. } = host.review().start(deck, None).unwrap() else {
        panic!("close unwind fixture must start");
    };
    let ready = LifecycleState::Ready {
        generation: c.generation(),
    };
    assert_eq!(host.engine().state(), ready);
    let events = host.engine().stream().subscribe();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        host.review().try_close_host(
            || panic!("shutdown unwound before doing work"),
            || panic!("unwound shutdown published closed"),
        )
    }));
    assert!(result.is_err());
    assert_eq!(host.engine().state(), ready);
    assert_eq!(host.collection().unwrap().generation(), c.generation());
    assert_eq!(host.review().active_session(), Some(session_id));
    assert_eq!(host.review().head(session_id, 3).unwrap(), head);
    while let Some(event) = events.try_recv() {
        if let EventKind::Extension(payload) = &event.kind {
            assert!(
                !matches!(
                    payload.as_any().downcast_ref::<SessionEvent>(),
                    Some(SessionEvent::SessionEnded { .. })
                ),
                "unwound shutdown ended the retained session"
            );
        }
    }
    let card = &head.items[0];
    assert!(matches!(
        host.review()
            .answer(
                session_id,
                card.card_id,
                Rating::Easy,
                card.state_token,
                EpochMillis(1_894_701_600_000),
                100,
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    // A successful retry also proves unwind released lifecycle admission.
    host.close().unwrap();
    assert_eq!(host.review().active_session(), None);
}

#[test]
fn close_refuses_restore_admission_and_retains_the_host_for_retry() {
    use meguri_engine::{CancelToken, EngineError, JobKind, OpSource, OpSpec, ProgressPhase, ProgressSink, UndoScope};
    use std::{
        sync::{Mutex, mpsc},
        time::Duration,
    };

    struct Release(mpsc::Sender<()>);
    impl Drop for Release {
        fn drop(&mut self) {
            let _ = self.0.send(());
        }
    }
    struct StartGate {
        enter: mpsc::Sender<()>,
        wait: Mutex<Option<mpsc::Receiver<()>>>,
    }
    impl meguri_engine::session::SessionStartHook for StartGate {
        fn session_starting(&self) {
            let wait = self.wait.lock().unwrap().take();
            if let Some(wait) = wait {
                self.enter.send(()).unwrap();
                wait.recv_timeout(Duration::from_secs(10)).unwrap();
            }
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let options = config(dir.path());
    let process = meguri_host::ProcessCore::new(options.engine.clone()).unwrap();
    let host = Arc::new(process.open(options.clone()).unwrap());
    let c = host.collection().unwrap();
    c.run(OpSpec::named("close fixture policy", OpSource::Maintenance), |ctx| {
        ctx.scope()
            .set_config("fsrs", &meguri_storage::ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &meguri_storage::ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let deck = meguri_engine::decks::add(&c, "Close admission".into())
        .unwrap()
        .value
        .id;
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("Close admission".into()),
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &c,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["close front".into(), "close back".into()],
            tags: vec![],
            deck_id: deck,
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    // Park a controlled start inside admission, before it installs any shown
    // state. Close must refuse even though active_session is still None.
    let (enter, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let start_release = Release(release);
    host.review().add_session_start_hook(Arc::new(StartGate {
        enter,
        wait: Mutex::new(Some(wait)),
    }));
    let callers = meguri_engine::ThreadRegistry::default();
    let starting_host = host.clone();
    let start = callers
        .spawn("close-test-start", meguri_engine::Qos::Default, move |_| {
            starting_host
                .review()
                .start_controlled(deck, None, CancelToken::new(), ProgressSink::new())
        })
        .unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(host.review().active_session(), None);
    for _ in 0..2 {
        let closing_host = host.clone();
        let (send, recv) = mpsc::channel();
        let close = callers
            .spawn("close-test-before-install", meguri_engine::Qos::Default, move |_| {
                let _ = send.send(closing_host.close());
            })
            .unwrap();
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err(EngineError::Busy {
                running_op: "review lifecycle".into()
            })
        );
        close.join().unwrap();
        assert_eq!(host.collection().unwrap().generation(), c.generation());
        assert_eq!(host.review().active_session(), None);
        assert!(c.reader(ReaderRole::Query).unwrap().deck(deck).unwrap().is_some());
    }
    drop(start_release);
    let meguri_engine::session::StartResult::Started { session_id, head, .. } = start.join().unwrap().unwrap() else {
        panic!("close fixture must start")
    };
    let (enter, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let release = Release(release);
    let blocked = c.submit(OpSpec::named("close writer barrier", OpSource::Rollover), move |_| {
        enter.send(()).unwrap();
        wait.recv_timeout(Duration::from_secs(10)).unwrap();
        Ok(())
    });
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    let cancel = CancelToken::new();
    assert!(!c.has_queued_op_for_testing("durability_control"));
    let restoring_host = host.clone();
    let restoring_cancel = cancel.clone();
    let restore = host
        .engine()
        .threads()
        .spawn("close-test-restore", meguri_engine::Qos::Default, move |_| {
            restoring_host
                .review()
                .restore_controlled(UndoScope::Global, false, restoring_cancel, ProgressSink::new())
        })
        .unwrap();
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
    while !c.has_queued_op_for_testing("durability_control") {
        assert!(
            meguri_engine::monotonic::now() < deadline,
            "restore never queued durability control"
        );
        std::thread::yield_now();
    }
    for _ in 0..2 {
        let closing_host = host.clone();
        let (send, recv) = mpsc::channel();
        let caller = host
            .engine()
            .threads()
            .spawn("close-test-caller", meguri_engine::Qos::Default, move |_| {
                let _ = send.send(closing_host.close());
            })
            .unwrap();
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(1))
                .expect("close blocked behind restore"),
            Err(EngineError::Busy {
                running_op: "review lifecycle".into()
            })
        );
        caller.join().unwrap();
        assert_eq!(host.collection().unwrap().generation(), c.generation());
        assert_eq!(host.review().active_session(), Some(session_id));
        assert_eq!(host.review().head(session_id, 3).unwrap(), head);
        assert!(c.reader(ReaderRole::Query).unwrap().deck(deck).unwrap().is_some());
    }
    // Control remains callable after refusal; cancellation does not end the
    // session, and a later successful close must actually release everything.
    cancel.cancel();
    drop(release);
    blocked.wait().unwrap();
    assert!(matches!(restore.join().unwrap(), Err(EngineError::Cancelled)));
    assert_eq!(host.review().active_session(), Some(session_id));
    // Enter both registered workers before close can take either control lane.
    // Keep them registered through Engine::close's cancellation as well.
    let mut restores = Vec::new();
    for redo in [false, true] {
        let restoring_host = host.clone();
        let (enter, entered) = mpsc::channel();
        let (admit, admission) = mpsc::channel();
        let (finish, finishing) = mpsc::channel();
        let (send, recv) = mpsc::channel();
        let admission_release = Release(admit);
        let finish_release = Release(finish);
        let job = host
            .engine()
            .start_job(JobKind::BulkOp, move |ctx| {
                ctx.cancel.check()?;
                enter.send(()).unwrap();
                admission.recv_timeout(Duration::from_secs(10)).unwrap();
                let result = restoring_host
                    .review()
                    .restore_controlled(UndoScope::Global, redo, ctx.cancel.clone(), ctx.progress.clone())
                    .map(|_| ());
                send.send(result).unwrap();
                finishing.recv_timeout(Duration::from_secs(10)).unwrap();
                let result = restoring_host
                    .review()
                    .restore_controlled(UndoScope::Global, redo, ctx.cancel.clone(), ctx.progress.clone())
                    .map(|_| ());
                send.send(result.clone()).unwrap();
                result
            })
            .unwrap();
        entered.recv_timeout(Duration::from_secs(5)).unwrap();
        assert!(
            host.engine()
                .jobs()
                .running()
                .iter()
                .any(|running| running.id == job.id)
        );
        restores.push((job, admission_release, finish_release, recv));
    }
    // Hold the engine control lane in a background flush. Close must reserve
    // review lifecycle while waiting for it, before cancelling registered jobs.
    let (end_enter, end_entered) = mpsc::channel();
    let (end_release, end_wait) = mpsc::channel();
    let end_release = Release(end_release);
    let end_wait = Mutex::new(Some(end_wait));
    // Then stop Engine::close's own flush after job cancellation. The retained
    // session must stay protected until shutdown has actually succeeded.
    let (enter, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let shutdown_release = Release(release);
    let wait = Mutex::new(Some(wait));
    host.review()
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == meguri_engine::session::JournalStage::Compact {
                if let Some(wait) = end_wait.lock().unwrap().take() {
                    end_enter.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(10)).unwrap();
                } else if let Some(wait) = wait.lock().unwrap().take() {
                    enter.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            }
        }))
        .unwrap();
    let flushing_engine = host.engine().clone();
    let flush = callers
        .spawn("close-test-control-lane", meguri_engine::Qos::Default, move |_| {
            flushing_engine.flush_for_background()
        })
        .unwrap();
    end_entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert!(host.review().peek_undo(UndoScope::Session(session_id)).is_ok());
    let closing_host = host.clone();
    let (closed, close_result) = mpsc::channel();
    let close = callers
        .spawn("close-test-shutdown", meguri_engine::Qos::Default, move |_| {
            let _ = closed.send(closing_host.close());
        })
        .unwrap();
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(5);
    loop {
        match host.review().peek_undo(UndoScope::Session(session_id)) {
            Err(EngineError::CollectionUnavailable { reason }) if reason == "answer chain is restoring" => break,
            Ok(_) => {}
            other => panic!("unexpected close reservation: {other:?}"),
        }
        assert!(
            meguri_engine::monotonic::now() < deadline,
            "close never owned lifecycle"
        );
        std::thread::yield_now();
    }
    let undo = c.peek_undo(UndoScope::Global).unwrap();
    let redo = c.peek_redo(UndoScope::Global).unwrap();
    for (job, admission, _, recv) in &restores {
        assert!(!job.cancel.is_cancelled());
        admission.0.send(()).unwrap();
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err(EngineError::CollectionUnavailable {
                reason: "review lifecycle is busy".into()
            })
        );
        assert_eq!(job.progress.phase(), ProgressPhase::Planning);
        assert_eq!(host.collection().unwrap().generation(), c.generation());
        assert_eq!(host.review().active_session(), Some(session_id));
        assert_eq!(host.review().head(session_id, 3).unwrap(), head);
        assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), undo);
        assert_eq!(c.peek_redo(UndoScope::Global).unwrap(), redo);
    }
    drop(end_release);
    flush.join().unwrap().unwrap();
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(host.review().active_session(), Some(session_id));
    for (job, _, finish, recv) in &restores {
        assert!(job.cancel.is_cancelled());
        finish.0.send(()).unwrap();
        assert_eq!(
            recv.recv_timeout(Duration::from_secs(1)).unwrap(),
            Err(EngineError::Cancelled)
        );
        assert_eq!(job.progress.phase(), ProgressPhase::Planning);
    }
    let starting_host = host.clone();
    let (send, recv) = mpsc::channel();
    let refused = callers
        .spawn("close-test-during-shutdown", meguri_engine::Qos::Default, move |_| {
            let _ = send.send(starting_host.review().start_controlled(
                deck,
                None,
                CancelToken::new(),
                ProgressSink::new(),
            ));
        })
        .unwrap();
    assert_eq!(
        recv.recv_timeout(Duration::from_secs(1))
            .expect("controlled start blocked during shutdown")
            .unwrap_err(),
        EngineError::CollectionUnavailable {
            reason: "review lifecycle is busy".into()
        }
    );
    refused.join().unwrap();
    assert_eq!(host.review().active_session(), Some(session_id));
    drop(shutdown_release);
    close_result.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
    close.join().unwrap();
    assert!(host.engine().jobs().running().is_empty());
    assert_eq!(host.review().active_session(), None);
    assert!(host.collection().is_err());
    assert!(c.reader(ReaderRole::Query).is_err());
    host.close().unwrap();
    let reopened = process.open(options).unwrap();
    assert_eq!(reopened.review().active_session(), None);
    assert_eq!(
        reopened.review().head(session_id, 3),
        Err(EngineError::InvalidArgument {
            message: "NoSession".into()
        })
    );
    let meguri_engine::session::StartResult::Started {
        session_id: next,
        generation,
        head: next_head,
        ..
    } = reopened.review().start(deck, None).unwrap()
    else {
        panic!("reopened close fixture must start")
    };
    assert_ne!(next, session_id);
    assert_ne!(generation, c.generation());
    assert_eq!(generation, reopened.collection().unwrap().generation());
    assert_eq!(reopened.review().head(next, 3).unwrap(), next_head);
    assert!(reopened.review().head(session_id, 3).is_err());
    reopened.close().unwrap();
}

#[test]
fn suspend_closes_media_across_clones_and_resume_reopens_the_same_store() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let media = host.media().unwrap();
    media.add("owned.txt", b"synthetic media").unwrap();
    let before = media.store().pending().unwrap();
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    assert!(media.store().pending().is_err());
    host.engine().resume().unwrap();
    assert_eq!(media.store().pending().unwrap(), before);
    host.close().unwrap();
}

#[test]
fn production_backup_registration_allows_fallback_drop_and_reopen_without_close() {
    use meguri_engine::ports::{BackupFailureCause, BackupPublication, SafetyBackupFailure};
    use meguri_engine::{CancelToken, EngineError, LifecycleState, LongOp, OpSource, OpSpec};
    use meguri_storage::{ConfigValue, OpenError, lockfile::CollectionLock};

    let dir = tempfile::tempdir().unwrap();
    let options = config(dir.path());
    let process = meguri_host::ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    let engine = host.engine().clone();
    let owner = engine.downgrade();
    let threads = engine.threads().clone();
    let collection = host.collection().unwrap();
    let backup = host.backup(); // The real, registered production provider.
    collection
        .run(OpSpec::named("fallback drop witness", OpSource::Maintenance), |ctx| {
            ctx.scope()
                .set_config("fallbackDropWitness", &ConfigValue::Bool(true))?;
            Ok(())
        })
        .unwrap();

    // Settle prior finite work to isolate the registered provider's ownership.
    assert!(host.derived().wait_idle(std::time::Duration::from_secs(5)));
    engine.maintenance().wait_idle_for_testing();

    // This deterministic handoff refuses CoreHost's best-effort drop close.
    // The last engine owner must still shut down the writer on its own.
    let handoff = engine.reserve_collection_export_handoff().unwrap();
    drop(host);
    drop(process);
    assert_eq!(engine.slot_holder(), Some(LongOp::CollectionExportHandoff));
    assert_eq!(
        engine.state(),
        LifecycleState::Ready {
            generation: collection.generation(),
        }
    );
    assert!(threads.threads().iter().any(|thread| thread.name == "meguri-writer"));
    assert!(matches!(
        CollectionLock::acquire(&options.collection_directory),
        Err(OpenError::OpenInAnotherProgram)
    ));
    drop(engine);
    drop(handoff);

    assert!(
        owner.upgrade().is_none(),
        "the registered provider must not keep its engine alive"
    );
    assert!(threads.threads().iter().all(|thread| thread.name != "meguri-writer"));
    assert!(collection.reader(ReaderRole::Query).is_err());
    // Retained provider handles fail before reading files or publishing anything.
    for error in [
        backup
            .before_safety_operation(&collection, &CancelToken::new())
            .unwrap_err(),
        backup
            .protect_backup("backup-2030-01-15-10.00.00.colpkg")
            .err()
            .expect("owner has dropped"),
    ] {
        assert_eq!(
            error,
            EngineError::SafetyBackupFailed(Box::new(SafetyBackupFailure {
                cause: BackupFailureCause::Engine(Box::new(EngineError::CollectionUnavailable {
                    reason: "the backup provider's engine has dropped".into(),
                })),
                publication: BackupPublication::Unpublished,
            }))
        );
    }
    assert_eq!(std::fs::read_dir(&options.backups_directory).unwrap().count(), 0);
    drop(collection);
    drop(backup);

    let reopened = CoreHost::open(options).unwrap();
    assert_eq!(
        reopened
            .collection()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("fallbackDropWitness")
            .unwrap(),
        Some(ConfigValue::Bool(true))
    );
    reopened.close().unwrap();
}

#[test]
fn an_open_failure_releases_workers_and_the_collection_ownership_lock() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    // The media journal's path cannot redirect outside the private directory.
    let journal = config.collection_directory.join("meguri-media.db");
    std::os::unix::fs::symlink(dir.path().join("outside"), &journal).unwrap();
    assert!(CoreHost::open(config.clone()).is_err());
    std::fs::remove_file(journal).unwrap();
    let host = CoreHost::open(config).unwrap();
    host.close().unwrap();
}

#[test]
fn a_real_backup_capacity_failure_refuses_a_real_deck_deletion_before_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let gate = Arc::new(BackupCapacity::default());
    let mut inputs = config(dir.path());
    inputs.disk_space_provider = gate.clone();
    let host = CoreHost::open(inputs).unwrap();
    gate.low.store(true, std::sync::atomic::Ordering::SeqCst);
    let collection = host.collection().unwrap();
    let id = meguri_engine::decks::add(&collection, "Backup refusal".into())
        .unwrap()
        .value
        .id;
    let kind = meguri_engine::notetypes::add_stock(
        &collection,
        meguri_engine::notetypes::stock::StockKind::Basic,
        Some("Backup test".into()),
    )
    .unwrap()
    .value
    .id;
    let added = meguri_engine::notes::add(
        &collection,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["synthetic front".into(), "synthetic back".into()],
            tags: vec![],
            deck_id: id,
            guid: None,
            update_add_defaults: false,
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert_eq!(added.value.cards.len(), 1);
    let before = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        (
            reader.col().unwrap(),
            reader.execute_raw("SELECT * FROM decks ORDER BY id", &[]).unwrap(),
        )
    };
    assert!(meguri_engine::decks::delete(&collection, vec![id], host.backup()).is_err());
    let reader = collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        (
            reader.col().unwrap(),
            reader.execute_raw("SELECT * FROM decks ORDER BY id", &[]).unwrap()
        ),
        before
    );
    drop(reader);
    host.close().unwrap();
}

fn committed_publication(host: &CoreHost) -> meguri_engine::media::MediaService {
    use meguri_engine::{EngineError, OpSource, OpSpec, notes, notetypes};
    let collection = host.collection().unwrap();
    let kind = notetypes::add_stock(&collection, notetypes::StockKind::Basic, Some("Recovery test".into()))
        .unwrap()
        .value
        .id;
    let media = host.media().unwrap();
    let mut stage = media.begin_staging().unwrap();
    stage.add("recovered.png", b"durable synthetic media").unwrap();
    meguri_model::faultpoint::arm(
        "media.stage.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = collection.run(OpSpec::named("media_note", OpSource::UserNonUndoable), move |ctx| {
        let (scope, random) = ctx.scope_and_random();
        let added = notes::add_in(
            scope,
            random,
            &notes::AddNote {
                notetype_id: kind,
                fields: vec!["<img src='recovered.png'>".into(), "back".into()],
                tags: vec![],
                deck_id: meguri_model::ids::DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
        )?;
        ctx.commit_media(stage, &[added.id])?;
        Ok(())
    });
    meguri_model::faultpoint::disarm_all();
    assert!(
        matches!(result, Err(EngineError::MediaCommitPending { .. })),
        "{result:?}"
    );
    assert!(!media.store().media_dir().join("recovered.png").exists());
    media
}

#[test]
fn startup_recovers_committed_media_before_exposing_the_host() {
    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let host = CoreHost::open(config.clone()).unwrap();
    let media = committed_publication(&host);
    host.close().unwrap();
    drop(host);
    let reopened = CoreHost::open(config).unwrap();
    assert_eq!(
        std::fs::read(media.store().media_dir().join("recovered.png")).unwrap(),
        b"durable synthetic media"
    );
    assert!(
        media.store().pending().is_err(),
        "the retired host's journal stays closed"
    );
    assert_eq!(reopened.media().unwrap().store().pending().unwrap().len(), 1);
    reopened.close().unwrap();
}

#[test]
fn resume_recovers_committed_media_before_admitting_operations() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let media = committed_publication(&host);
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    host.engine().resume().unwrap();
    assert_eq!(
        std::fs::read(media.store().media_dir().join("recovered.png")).unwrap(),
        b"durable synthetic media"
    );
    assert_eq!(media.store().pending().unwrap().len(), 1);
    host.close().unwrap();
}

#[test]
fn failed_resume_recovery_keeps_admission_closed_until_recovery_succeeds() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let media = committed_publication(&host);
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    meguri_model::faultpoint::arm(
        "media.stage.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = host.engine().resume();
    meguri_model::faultpoint::disarm_all();
    assert!(result.is_err());
    assert!(
        host.collection()
            .unwrap()
            .run(
                meguri_engine::OpSpec::named("blocked", meguri_engine::OpSource::UserNonUndoable),
                |_| Ok(())
            )
            .is_err()
    );
    assert!(media.add("blocked.txt", b"must not publish").is_err());
    assert!(!media.store().media_dir().join("blocked.txt").exists());
    assert!(media.store().pending().is_err());
    host.engine().resume().unwrap();
    assert_eq!(
        std::fs::read(media.store().media_dir().join("recovered.png")).unwrap(),
        b"durable synthetic media"
    );
    host.close().unwrap();
}

#[test]
fn resume_recovery_runs_after_external_change_checks_without_changing_generation() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let path = config.collection_directory.join("collection.anki2");
    let host = CoreHost::open(config).unwrap();
    let media = committed_publication(&host);
    let generation = host.engine().generation();
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    let before = std::fs::metadata(&path).unwrap();
    {
        let db = meguri_storage::raw::PlainDatabase::open(&path).unwrap();
        db.execute("UPDATE col SET mod = mod + 1", &[]).unwrap();
    }
    let after = std::fs::metadata(&path).unwrap();
    assert_eq!((after.dev(), after.ino()), (before.dev(), before.ino()));
    assert!(host.engine().resume().unwrap().external_change);
    assert_eq!(host.engine().generation(), generation);
    assert_eq!(
        std::fs::read(media.store().media_dir().join("recovered.png")).unwrap(),
        b"durable synthetic media"
    );
    assert_eq!(media.store().pending().unwrap().len(), 1);
    host.media().unwrap().add("current.txt", b"current generation").unwrap();
    host.close().unwrap();
}

#[test]
fn resume_rejects_atomic_replacement_and_keeps_unretired_journal_and_admission_closed() {
    use std::os::unix::fs::MetadataExt;

    let dir = tempfile::tempdir().unwrap();
    let config = config(dir.path());
    let path = config.collection_directory.join("collection.anki2");
    let journal = config.collection_directory.join("meguri-pending-answers.jsonl");
    let host = CoreHost::open(config).unwrap();
    let collection = host.collection().unwrap();
    let media = committed_publication(&host);
    let generation = host.engine().generation();
    // This fixture submits no review answers. Count through an owned reader
    // before suspension; an independent fd must not close behind SQLite owners.
    let before_logs = collection
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(meguri_storage::Table::Revlog, None, 100_000)
        .unwrap()
        .len();
    host.engine().suspend(std::time::Duration::from_secs(2)).unwrap();
    let evidence = std::fs::read(&journal).unwrap();
    assert!(!String::from_utf8_lossy(&evidence).contains("Retired"));
    let original = std::fs::metadata(&path).unwrap();
    // Called only while every managed collection connection is closed.
    let review_log_count = || {
        let db = meguri_storage::rusqlite::Connection::open_with_flags(
            &path,
            meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
        )
        .unwrap();
        let count = db
            .query_row("SELECT count(*) FROM revlog", [], |row| row.get::<_, i64>(0))
            .unwrap();
        usize::try_from(count).unwrap()
    };
    let replacement = dir.path().join("external.anki2");
    std::fs::copy(&path, &replacement).unwrap();
    {
        let db = meguri_storage::raw::PlainDatabase::open(&replacement).unwrap();
        db.execute("UPDATE col SET mod = mod + 1", &[]).unwrap();
    }
    std::fs::rename(&replacement, &path).unwrap();
    let published = std::fs::metadata(&path).unwrap();
    assert_ne!((published.dev(), published.ino()), (original.dev(), original.ino()));
    assert_eq!(review_log_count(), before_logs);
    let mut recovered_pending = None;
    for _ in 0..2 {
        let error = host.engine().resume().unwrap_err();
        let detail = format!("{error:?}").to_ascii_lowercase();
        assert!(
            detail.contains("identity") || detail.contains("native"),
            "resume must report the file binding failure: {error}"
        );
        assert_eq!(host.engine().generation(), generation);
        assert!(
            collection
                .run(
                    meguri_engine::OpSpec::named("blocked replacement", meguri_engine::OpSource::UserNonUndoable),
                    |_| Ok(())
                )
                .is_err()
        );
        assert!(media.add("blocked.txt", b"must not publish").is_err());
        assert!(!media.store().media_dir().join("blocked.txt").exists());
        // Media recovery precedes review identity validation. Its read store
        // may remain open, but recovery is complete and retry is idempotent.
        assert_eq!(
            std::fs::read(media.store().media_dir().join("recovered.png")).unwrap(),
            b"durable synthetic media"
        );
        let pending = media.store().pending().unwrap();
        assert_eq!(pending.len(), 1);
        if let Some(expected) = &recovered_pending {
            assert_eq!(&pending, expected);
        } else {
            recovered_pending = Some(pending);
        }
        let retained = std::fs::read(&journal).unwrap();
        assert_eq!(retained, evidence, "resume must preserve the original binding");
        assert!(!String::from_utf8_lossy(&retained).contains("Retired"));
    }
    host.close().unwrap();
    assert_eq!(
        review_log_count(),
        before_logs,
        "refusal must not replay or insert answers"
    );
}

#[test]
fn real_deck_counts_and_forced_backup_providers_are_wired() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let c = host.collection().unwrap();
    let first = host.queues().deck_counts(&c).unwrap();
    assert!(!first.nodes.is_empty());
    let built = host.queues().queue(&c, None).unwrap();
    meguri_engine::decks::add(&c, "U41 provider".into()).unwrap();
    assert_eq!(
        built.validity.action(),
        meguri_sched::queue::rebuild_rules::Action::Rebuild
    );
    let tree = host.queues().deck_counts(&c).unwrap();
    assert!(tree.nodes.iter().any(|n| n.name == "U41 provider"));
    meguri_host::wiring::require(meguri_engine::ports::ServicePort::DeckCounts).unwrap();
    host.backup().before_deck_delete().unwrap();
    assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
    host.close().unwrap();
}

#[derive(Default)]
struct BackupCapacity {
    low: std::sync::atomic::AtomicBool,
}
impl meguri_storage::space::SpaceProvider for BackupCapacity {
    fn available_bytes(&self, _: &Path) -> Result<u64, meguri_storage::StorageError> {
        Ok(if self.low.load(std::sync::atomic::Ordering::SeqCst) {
            0
        } else {
            u64::MAX
        })
    }
}

fn backup_receipt(host: &CoreHost) -> meguri_engine::ports::BackupReceipt {
    host.backup()
        .before_colpkg_import(&host.collection().unwrap(), &meguri_engine::CancelToken::default())
        .unwrap()
}
fn inspect_backup(host: &CoreHost, name: &str) -> meguri_package::InspectedCollection {
    let c = host.collection().unwrap();
    let clock = StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(721);
    meguri_package::inspect_collection_package(
        &c.path().parent().unwrap().parent().unwrap().join("backups").join(name),
        &meguri_package::CollectionPackageInspectContext {
            collection_directory: c.path().parent().unwrap(),
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: &clock,
            random: &random,
            space: &StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap()
}

#[test]
fn production_forced_backup_recovers_a_committed_old_wal_answer_and_releases_before_replace() {
    use meguri_engine::{
        JobKind, OpSource, OpSpec,
        session::{AnswerOutcome, StartResult},
    };
    use meguri_model::time::EpochMillis;
    use meguri_sched::answer::Rating;
    use std::{sync::mpsc, time::Duration};

    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let c = host.collection().unwrap();
    c.run(OpSpec::named("synthetic review policy", OpSource::Maintenance), |ctx| {
        ctx.scope()
            .set_config("fsrs", &meguri_storage::ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &meguri_storage::ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let deck = meguri_engine::decks::add(&c, "Safety old answer".into())
        .unwrap()
        .value
        .id;
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("Safety old answer".into()),
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &c,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["safety front".into(), "safety back".into()],
            tags: vec![],
            deck_id: deck,
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    // Pin a real read of the main file before the answer, so its committed
    // bytes remain in WAL rather than relying on checkpoint timing.
    let old_reader = c.reader(ReaderRole::Sync).unwrap();
    let old_snapshot = old_reader.snapshot().unwrap();
    let old_reviews = old_snapshot
        .execute_raw("SELECT * FROM revlog ORDER BY id", &[])
        .unwrap();
    let StartResult::Started { session_id, head, .. } = host.review().start(deck, None).unwrap() else {
        panic!("synthetic review must start");
    };
    let card = &head.items[0];
    assert!(matches!(
        host.review()
            .answer(
                session_id,
                card.card_id,
                Rating::Easy,
                card.state_token,
                EpochMillis(1_894_701_600_000),
                100
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    host.review().end(session_id).unwrap();
    let reviews = c
        .reader(ReaderRole::Query)
        .unwrap()
        .execute_raw("SELECT * FROM revlog ORDER BY id", &[])
        .unwrap();
    assert_eq!(reviews.rows.len(), old_reviews.rows.len() + 1);
    assert_eq!(
        old_snapshot
            .execute_raw("SELECT * FROM revlog ORDER BY id", &[])
            .unwrap(),
        old_reviews
    );
    assert!(
        std::fs::metadata(c.path().with_file_name("collection.anki2-wal"))
            .unwrap()
            .len()
            > 32
    );
    drop(old_snapshot);
    drop(old_reader); // exclusive drain must not wait on our test's reader.
    let expected_mod = c.reader(ReaderRole::Query).unwrap().col().unwrap().modified;
    let provider = host.backup();
    let candidate = meguri_engine::replace::ReplacementStage::new(c.path().parent().unwrap()).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        candidate.path(),
    )
    .unwrap();
    let (send, recv) = mpsc::channel();
    host.engine()
        .start_job(JobKind::ColpkgImport, move |ctx| {
            let result = (|| {
                let mut exclusive = ctx.slot().unwrap().enter_exclusive(true)?;
                let privileged = exclusive.collection();
                let receipt = provider.before_colpkg_import(privileged, &ctx.cancel)?;
                ctx.cancel.check()?;
                let next = exclusive
                    .replace_verified(candidate, meguri_engine::replace::ReplaceReason::ColpkgImport, |_| {
                        Ok(())
                    })
                    .map_err(|error| meguri_engine::EngineError::Internal {
                        message: error.to_string(),
                    })?;
                Ok((receipt, next.generation()))
            })();
            send.send(result.clone()).unwrap();
            result.map(|_| ())
        })
        .unwrap();
    let (receipt, generation) = recv
        .recv_timeout(Duration::from_secs(20))
        .expect("backup lease leaked into replace")
        .unwrap();
    assert_eq!(generation.0, c.generation().0 + 1);
    assert_eq!(receipt.snapshot_modified, expected_mod);
    let captured = inspect_backup(&host, &receipt.name);
    let clock = StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(721);
    let db = meguri_storage::open_collection(
        &captured.collection_path(),
        &meguri_storage::OpenOptions::new(&clock, &random),
    )
    .unwrap();
    let read = db.open_reader(ReaderRole::Query).unwrap();
    assert_eq!(
        read.execute_raw("SELECT * FROM revlog ORDER BY id", &[]).unwrap(),
        reviews
    );
    assert_eq!(read.col().unwrap().modified, receipt.snapshot_modified);
    assert_eq!(
        std::fs::metadata(dir.path().join("backups").join(&receipt.name))
            .unwrap()
            .len(),
        receipt.size_bytes
    );
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("collection/meguri-sidecar.json")).unwrap()).unwrap();
    assert_eq!(
        document["deviceState"]["lastBackup"]["snapshotMod"],
        receipt.snapshot_modified.0
    );
    assert_eq!(document["deviceState"]["lastBackup"]["name"], receipt.name);
    drop(read);
    db.close().unwrap();
    host.close().unwrap();
}

#[test]
fn forced_empty_unchanged_backups_keep_every_file_and_preserve_r6_and_unknown_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let mut inputs = config(dir.path());
    // Use an empty public schema fixture, rather than relying on zero cards
    // to mean there is no collection file.
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        inputs.collection_directory.join("collection.anki2"),
    )
    .unwrap();
    inputs.backups_directory = dir.path().join("missing/parents/backups");
    let host = CoreHost::open(inputs).unwrap();
    let c = host.collection().unwrap();
    c.run(
        meguri_engine::OpSpec::named("empty synthetic backup fixture", meguri_engine::OpSource::Maintenance),
        |ctx| {
            let scope = ctx.scope();
            for row in scope.revlog_after(None, 10_000)? {
                scope.remove_revlog(row.id)?;
            }
            for row in scope.cards_after(None, 10_000)? {
                scope.remove_card(row.id)?;
            }
            for row in scope.notes_after(None, 10_000)? {
                scope.remove_note(row.id)?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .cards_after(None, 1)
            .unwrap()
            .is_empty()
    );
    assert!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .notes_after(None, 1)
            .unwrap()
            .is_empty()
    );
    let path = c.path().parent().unwrap().join("meguri-sidecar.json");
    meguri_storage::sidecar::update_document(&path, |doc| {
        doc.insert("foreignRoot".into(), serde_json::json!({"keep": [1, "two"]}));
        let state = doc.get_mut("deviceState").unwrap().as_object_mut().unwrap();
        let active = state.get_mut("active").unwrap();
        active["decision"] = "Pending".into();
        active["futureIdentityMember"] = serde_json::json!(["opaque"]);
        state.insert("oneWayChoicePending".into(), true.into());
        state.insert("futureState".into(), serde_json::json!({"nested": 92}));
        state.insert(
            "lastBackup".into(),
            serde_json::json!({
                "snapshotMod": 0_i64,
                "instant": 1_894_701_599_i64,
                "name": "backup-2030-01-15-09.59.59.colpkg",
                "futureBackup": "preserve"
            }),
        );
        Ok(())
    })
    .unwrap();
    let before: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    let first = backup_receipt(&host);
    let second = backup_receipt(&host);
    assert_ne!(first.name, second.name);
    assert_eq!(first.instant, second.instant);
    assert_eq!(first.snapshot_modified, second.snapshot_modified);
    assert_eq!(
        std::fs::read_dir(dir.path().join("missing/parents/backups"))
            .unwrap()
            .count(),
        2
    );
    let mut after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    assert_eq!(after["deviceState"]["lastBackup"]["futureBackup"], "preserve");
    after["deviceState"]["lastBackup"] = before["deviceState"]["lastBackup"].clone();
    assert_eq!(after, before);
    host.close().unwrap();
}

#[test]
fn forced_capacity_and_cancel_failures_preserve_live_and_do_not_create_backup_parents() {
    use std::sync::atomic::Ordering;
    for cancelled in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let space = Arc::new(BackupCapacity::default());
        let mut inputs = config(dir.path());
        inputs.disk_space_provider = space.clone();
        inputs.backups_directory = dir.path().join("missing/backup");
        let host = CoreHost::open(inputs).unwrap();
        let c = host.collection().unwrap();
        let files = [
            c.path().to_owned(),
            c.path().with_file_name("collection.anki2-wal"),
            c.path().with_file_name("meguri-sidecar.json"),
        ];
        let bytes: Vec<_> = files.iter().map(|p| std::fs::read(p).unwrap()).collect();
        let cancel = meguri_engine::CancelToken::default();
        if cancelled {
            cancel.cancel();
        } else {
            space.low.store(true, Ordering::SeqCst);
        }
        let error = host.backup().before_colpkg_import(&c, &cancel).unwrap_err();
        let meguri_engine::EngineError::SafetyBackupFailed(failure) = error else {
            panic!("typed safety failure")
        };
        assert_eq!(
            failure.publication,
            meguri_engine::ports::BackupPublication::Unpublished
        );
        assert!(matches!(
            (cancelled, failure.cause),
            (true, meguri_engine::ports::BackupFailureCause::Cancelled)
                | (
                    false,
                    meguri_engine::ports::BackupFailureCause::InsufficientSpace { available: 0, .. }
                )
        ));
        assert!(!dir.path().join("missing").exists());
        for (path, expected) in files.iter().zip(bytes) {
            assert_eq!(std::fs::read(path).unwrap(), expected);
        }
        // The original live writer is still usable after refusal.
        meguri_engine::decks::add(&c, "after refusal".into()).unwrap();
        space.low.store(false, Ordering::SeqCst);
        host.close().unwrap();
    }
}

#[test]
fn missing_or_unreadable_backup_metadata_retains_the_durable_file_and_refuses_import() {
    for missing in [false, true] {
        let dir = tempfile::tempdir().unwrap();
        let host = CoreHost::open(config(dir.path())).unwrap();
        let c = host.collection().unwrap();
        let sidecar = c.path().with_file_name("meguri-sidecar.json");
        let original = std::fs::read(&sidecar).unwrap();
        if missing {
            std::fs::remove_file(&sidecar).unwrap();
        } else {
            std::fs::write(&sidecar, b"{broken metadata").unwrap();
        }
        let before = std::fs::read(c.path()).unwrap();
        let error = host
            .backup()
            .before_colpkg_import(&c, &meguri_engine::CancelToken::default())
            .unwrap_err();
        let meguri_engine::EngineError::SafetyBackupFailed(failure) = error else {
            panic!("typed metadata failure")
        };
        let meguri_engine::ports::BackupPublication::Durable(receipt) = failure.publication else {
            panic!("published file must be reported")
        };
        assert!(dir.path().join("backups").join(&receipt.name).is_file());
        assert_eq!(std::fs::read(c.path()).unwrap(), before);
        if missing {
            assert!(!sidecar.exists());
        } else {
            assert_eq!(std::fs::read(&sidecar).unwrap(), b"{broken metadata");
        }
        let captured = inspect_backup(&host, &receipt.name);
        assert!(captured.collection_path().is_file(), "retained backup is recoverable");
        // Repair synthetic corruption solely to release the test host cleanly.
        std::fs::write(&sidecar, original).unwrap();
        host.close().unwrap();
    }
}

#[test]
fn invalid_required_backup_identity_metadata_retains_the_durable_file_without_rewriting_sidecar() {
    for (field, value) in [
        ("identity", None),
        ("binding", None),
        ("decision", None),
        ("identity", Some(serde_json::json!([1, -1]))),
        (
            "binding",
            Some(serde_json::json!({"Connected": {"account": "test", "configuration": "test"}})),
        ),
        (
            "binding",
            Some(serde_json::json!({"Connected": {
                "account": "test", "configuration": "test", "environment": "test", "unexpected": true
            }})),
        ),
        ("decision", Some(serde_json::json!("Unsupported"))),
    ] {
        let dir = tempfile::tempdir().unwrap();
        let host = CoreHost::open(config(dir.path())).unwrap();
        let c = host.collection().unwrap();
        let sidecar = c.path().with_file_name("meguri-sidecar.json");
        let original = std::fs::read(&sidecar).unwrap();
        meguri_storage::sidecar::update_document(&sidecar, |doc| {
            let active = doc["deviceState"]["active"].as_object_mut().unwrap();
            active.insert("futureIdentityMember".into(), serde_json::json!(["opaque"]));
            match value {
                Some(value) => {
                    active.insert(field.into(), value);
                }
                None => {
                    active.remove(field);
                }
            }
            Ok(())
        })
        .unwrap();
        let invalid = std::fs::read(&sidecar).unwrap();
        let error = host
            .backup()
            .before_colpkg_import(&c, &meguri_engine::CancelToken::default())
            .unwrap_err();
        let meguri_engine::EngineError::SafetyBackupFailed(failure) = error else {
            panic!("typed metadata failure for {field}")
        };
        assert!(matches!(
            failure.cause,
            meguri_engine::ports::BackupFailureCause::Engine(ref error)
                if matches!(error.as_ref(),
                    meguri_engine::EngineError::Storage(meguri_storage::StorageError::Io { detail })
                        if detail == "publication recovery blocked: invalid maintenance record")
        ));
        let meguri_engine::ports::BackupPublication::Durable(receipt) = failure.publication else {
            panic!("published file must be reported for {field}")
        };
        assert_eq!(std::fs::read(&sidecar).unwrap(), invalid);
        assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 1);
        assert_eq!(
            std::fs::metadata(dir.path().join("backups").join(&receipt.name))
                .unwrap()
                .len(),
            receipt.size_bytes
        );
        let captured = inspect_backup(&host, &receipt.name);
        assert!(captured.collection_path().is_file(), "retained backup is recoverable");
        // Repair synthetic corruption solely to release the test host cleanly.
        std::fs::write(&sidecar, original).unwrap();
        host.close().unwrap();
    }
}

#[test]
fn all_61_occupied_backup_names_refuse_before_snapshot_or_capacity_checks() {
    let dir = tempfile::tempdir().unwrap();
    let space = Arc::new(BackupCapacity::default());
    let mut inputs = config(dir.path());
    inputs.disk_space_provider = space.clone();
    let host = CoreHost::open(inputs).unwrap();
    let folder = dir.path().join("backups");
    std::os::unix::fs::symlink("absent", folder.join("backup-2030-01-15-10.00.00.colpkg")).unwrap();
    for second in 1..=60 {
        let name = format!("backup-2030-01-15-10.{:02}.{:02}.colpkg", second / 60, second % 60);
        std::fs::write(folder.join(name), b"occupied sentinel").unwrap();
    }
    space.low.store(true, std::sync::atomic::Ordering::SeqCst);
    let c = host.collection().unwrap();
    let before = std::fs::read(c.path().with_file_name("meguri-sidecar.json")).unwrap();
    let error = host
        .backup()
        .before_colpkg_import(&c, &meguri_engine::CancelToken::default())
        .unwrap_err();
    let meguri_engine::EngineError::SafetyBackupFailed(failure) = error else {
        panic!("typed collision")
    };
    assert_eq!(
        failure.cause,
        meguri_engine::ports::BackupFailureCause::DestinationExists
    );
    assert_eq!(
        failure.publication,
        meguri_engine::ports::BackupPublication::Unpublished
    );
    assert_eq!(std::fs::read_dir(folder).unwrap().count(), 61);
    assert_eq!(
        std::fs::read(c.path().with_file_name("meguri-sidecar.json")).unwrap(),
        before
    );
    space.low.store(false, std::sync::atomic::Ordering::SeqCst);
    host.close().unwrap();
}

#[test]
fn publication_name_races_are_bounded_to_three_real_snapshots_without_overwrite() {
    use meguri_model::faultpoint::{self, Fault};
    use std::sync::{Mutex, OnceLock};
    struct Race {
        folder: std::path::PathBuf,
        attempt: usize,
    }
    static RACE: OnceLock<Mutex<Option<Race>>> = OnceLock::new();
    fn occupy() {
        let mut guard = RACE.get().unwrap().lock().unwrap();
        let race = guard.as_mut().unwrap();
        let name = format!("backup-2030-01-15-10.00.{:02}.colpkg", race.attempt);
        std::fs::write(race.folder.join(name), b"racing writer sentinel").unwrap();
        race.attempt += 1;
    }
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    *RACE.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(Race {
        folder: dir.path().join("backups"),
        attempt: 0,
    });
    faultpoint::arm("storage.backup.before_rename", Fault::Callback(occupy));
    let error = host
        .backup()
        .before_colpkg_import(&host.collection().unwrap(), &meguri_engine::CancelToken::default())
        .unwrap_err();
    faultpoint::disarm("storage.backup.before_rename");
    let meguri_engine::EngineError::SafetyBackupFailed(failure) = error else {
        panic!("typed collision")
    };
    assert_eq!(
        failure.cause,
        meguri_engine::ports::BackupFailureCause::DestinationExists
    );
    assert_eq!(RACE.get().unwrap().lock().unwrap().as_ref().unwrap().attempt, 3);
    assert_eq!(std::fs::read_dir(dir.path().join("backups")).unwrap().count(), 3);
    for entry in std::fs::read_dir(dir.path().join("backups")).unwrap() {
        assert_eq!(std::fs::read(entry.unwrap().path()).unwrap(), b"racing writer sentinel");
    }
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("collection/meguri-sidecar.json")).unwrap()).unwrap();
    assert!(document["deviceState"].get("lastBackup").is_none());
    host.close().unwrap();
}

#[test]
fn late_cancellation_retains_publication_and_records_the_captured_reference() {
    use meguri_model::faultpoint::{self, Fault};
    use std::sync::{Mutex, OnceLock};
    static TOKEN: OnceLock<Mutex<Option<Arc<meguri_engine::CancelToken>>>> = OnceLock::new();
    fn cancel_after_rename() {
        TOKEN.get().unwrap().lock().unwrap().as_ref().unwrap().cancel();
    }
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    *TOKEN.get_or_init(|| Mutex::new(None)).lock().unwrap() = Some(cancel.clone());
    faultpoint::arm("storage.atomic.after_rename", Fault::Callback(cancel_after_rename));
    let receipt = host
        .backup()
        .before_colpkg_import(&host.collection().unwrap(), &cancel)
        .unwrap();
    faultpoint::disarm("storage.atomic.after_rename");
    assert!(cancel.is_cancelled());
    assert!(dir.path().join("backups").join(&receipt.name).is_file());
    let document: serde_json::Value =
        serde_json::from_slice(&std::fs::read(dir.path().join("collection/meguri-sidecar.json")).unwrap()).unwrap();
    assert_eq!(document["deviceState"]["lastBackup"]["name"], receipt.name);
    assert_eq!(
        document["deviceState"]["lastBackup"]["snapshotMod"],
        receipt.snapshot_modified.0
    );
    host.close().unwrap();
}

#[test]
fn cloud_connection_start_racing_close_is_registered_before_drain_or_rejected_by_original_host() {
    use meguri_engine::{EngineError, EventKind};
    use std::sync::{
        Barrier,
        atomic::{AtomicUsize, Ordering},
    };
    let dir = tempfile::tempdir().unwrap();
    let host = Arc::new(CoreHost::open(config(dir.path())).unwrap());
    let generation = host.cloud_connection_generation().unwrap();
    let events = host.engine().stream().subscribe();
    let race = Arc::new(Barrier::new(2));
    let completed = Arc::new(AtomicUsize::new(0));
    std::thread::scope(|scope| {
        let starter = host.clone();
        let start_race = race.clone();
        let done = completed.clone();
        let starts = scope.spawn(move || {
            start_race.wait();
            (0..32)
                .map(|_| {
                    let done = done.clone();
                    starter.start_cloud_connection_job(generation, "racing reply", move |_| {
                        done.fetch_add(1, Ordering::SeqCst);
                        Ok(())
                    })
                })
                .collect::<Vec<_>>()
        });
        race.wait();
        host.close().unwrap();
        let starts = starts.join().unwrap();
        let admitted: std::collections::BTreeSet<_> = starts
            .into_iter()
            .filter_map(|result| match result {
                Ok(job) => Some(job.id),
                Err(EngineError::CollectionUnavailable { .. }) => None,
                other => panic!("unexpected start result: {other:?}"),
            })
            .collect();
        assert!(
            host.engine().jobs().running().is_empty(),
            "start escaped original Host close drain"
        );
        let mut finished_jobs = std::collections::BTreeSet::new();
        while finished_jobs.len() < admitted.len() {
            let event = events.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            if let EventKind::JobFinished { id, .. } = &event.kind
                && admitted.contains(id)
            {
                assert!(finished_jobs.insert(*id), "duplicate terminal");
            }
        }
        assert!(completed.load(Ordering::SeqCst) <= admitted.len());
    });
    assert!(matches!(
        host.start_cloud_connection_job(generation, "after close", |_| panic!("closed Host admitted work")),
        Err(EngineError::CollectionUnavailable { .. })
    ));
}
