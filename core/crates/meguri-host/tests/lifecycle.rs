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
    loop {
        match host.review().try_pending_restore(UndoScope::Global, false) {
            Err(EngineError::CollectionUnavailable { reason }) if reason == "review lifecycle is busy" => break,
            Ok(None) => {}
            other => panic!("unexpected restore admission: {other:?}"),
        }
        assert!(
            meguri_engine::monotonic::now() < deadline,
            "restore never owned lifecycle"
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
    // First pause end_inner's drain while the session still exists: the close
    // owns lifecycle admission, but Engine::close has not cancelled the jobs.
    let (end_enter, end_entered) = mpsc::channel();
    let (end_release, end_wait) = mpsc::channel();
    let end_release = Release(end_release);
    let end_wait = Mutex::new(Some(end_wait));
    // Stop the review provider's flush in Engine::close, after end_inner has
    // removed the session. The same close reservation must still exclude start.
    let closing = Arc::downgrade(&host);
    let (enter, entered) = mpsc::channel();
    let (release, wait) = mpsc::channel();
    let shutdown_release = Release(release);
    let wait = Mutex::new(Some(wait));
    host.review()
        .journal_hook_for_testing(Arc::new(move |stage| {
            if stage == meguri_engine::session::JournalStage::Compact {
                let (enter, wait) = if closing.upgrade().unwrap().review().active_session().is_some() {
                    (&end_enter, &end_wait)
                } else {
                    (&enter, &wait)
                };
                let wait = wait.lock().unwrap().take();
                if let Some(wait) = wait {
                    enter.send(()).unwrap();
                    wait.recv_timeout(Duration::from_secs(10)).unwrap();
                }
            }
        }))
        .unwrap();
    let closing_host = host.clone();
    let (closed, close_result) = mpsc::channel();
    let close = callers
        .spawn("close-test-shutdown", meguri_engine::Qos::Default, move |_| {
            let _ = closed.send(closing_host.close());
        })
        .unwrap();
    end_entered.recv_timeout(Duration::from_secs(5)).unwrap();
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
    entered.recv_timeout(Duration::from_secs(5)).unwrap();
    assert_eq!(host.review().active_session(), None);
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
    assert_eq!(host.review().active_session(), None);
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
fn an_unavailable_backup_refuses_a_real_deck_deletion_before_mutation() {
    let dir = tempfile::tempdir().unwrap();
    let host = CoreHost::open(config(dir.path())).unwrap();
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
fn real_deck_counts_provider_is_wired_observes_writes_and_keeps_backup_unavailable() {
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
    assert!(host.backup().before_deck_delete().is_err());
    host.close().unwrap();
}
