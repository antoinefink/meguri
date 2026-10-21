//! Direct host shutdown must use S4 even when ordinary admission is closed.
use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};

use meguri_engine::{
    EngineError, EventKind, LifecycleState, OpSource, OpSpec, SessionId, SuspendOutcome,
    session::{AnswerOutcome, Head, SessionEvent, StartResult},
};
use meguri_host::{CoreHost, HostConfig, ProcessCore};
use meguri_model::{
    clock::{StepClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReaderRole, space::StatvfsSpace};

const NOW: i64 = 1_894_701_600_000;
// The sidecar fault is process-wide; keep these three disposable hosts apart.
static TEST_LANE: Mutex<()> = Mutex::new(());

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
        root.join("widget.snapshot"),
        Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")),
        Arc::new(SeededRandom::new(37)),
        Arc::new(StatvfsSpace),
    )
}

fn start(host: &CoreHost) -> (DeckId, SessionId, Head) {
    let c = host.collection().unwrap();
    c.run(OpSpec::named("direct close policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let deck = meguri_engine::decks::add(&c, "Direct close".into()).unwrap().value.id;
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("Direct close".into()),
    )
    .unwrap()
    .value
    .id;
    for n in 0..3 {
        meguri_engine::notes::add(
            &c,
            meguri_engine::notes::AddNote {
                notetype_id: kind,
                fields: vec![format!("direct close front {n}"), "back".into()],
                tags: vec![],
                deck_id: deck,
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    }
    let StartResult::Started { session_id, head, .. } = host.review().start(deck, None).unwrap() else {
        panic!("direct close fixture must start");
    };
    assert_eq!(head.items.len(), 3);
    (deck, session_id, head)
}

fn accept(host: &CoreHost, session: SessionId, head: &Head) -> Head {
    let item = &head.items[0];
    let AnswerOutcome::Accepted { head_diff, .. } = host
        .review()
        .answer(
            session,
            item.card_id,
            Rating::Easy,
            item.state_token,
            EpochMillis(NOW),
            123,
        )
        .unwrap()
    else {
        panic!("fixture answer must be accepted");
    };
    head_diff.head
}

fn ended(events: &meguri_engine::Subscription, session: SessionId, generation: meguri_engine::Generation) -> usize {
    let mut count = 0;
    while let Some(event) = events.try_recv() {
        if let EventKind::Extension(payload) = &event.kind
            && let Some(SessionEvent::SessionEnded { session_id }) = payload.as_any().downcast_ref()
        {
            assert_eq!(*session_id, session);
            assert_eq!(event.generation, generation);
            count += 1;
        }
    }
    count
}

fn direct_close_after_suspend(failed: bool) {
    let dir = tempfile::tempdir().unwrap();
    let options = config(dir.path());
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    let (deck, session, head) = start(&host);
    let c = host.collection().unwrap();
    let generation = c.generation();
    let answered = head.items[0].card_id;
    accept(&host, session, &head);
    host.review().application_barrier_for_testing().unwrap();
    let (saved_card, saved_logs) = {
        let rows = c.reader(ReaderRole::Query).unwrap();
        (
            rows.execute_raw(&format!("SELECT * FROM cards WHERE id = {}", answered.0), &[])
                .unwrap(),
            rows.execute_raw(
                &format!("SELECT * FROM revlog WHERE cid = {} ORDER BY id", answered.0),
                &[],
            )
            .unwrap(),
        )
    };
    assert_eq!(saved_card.rows.len(), 1);
    assert_eq!(saved_logs.rows.len(), 1);
    let media = host.media().unwrap();
    media.add("direct-close.txt", b"durable media").unwrap();
    assert!(host.derived().wait_idle(Duration::from_secs(5)));
    let derived = host.derived().path().unwrap();
    let events = host.engine().stream().subscribe();
    if failed {
        // A real reader lease forces a failed quiesce and closes admission.
        let reader = c.reader(ReaderRole::Sync).unwrap();
        let snapshot = reader.snapshot().unwrap();
        assert!(matches!(
            host.engine().suspend(Duration::from_millis(30)),
            Err(EngineError::SuspendDeadline { .. })
        ));
        drop(snapshot);
        drop(reader);
    } else {
        assert_eq!(
            host.engine().suspend(Duration::from_secs(5)).unwrap(),
            SuspendOutcome::Suspended
        );
    }
    assert_eq!(host.review().active_session(), Some(session));
    assert!(
        c.run(OpSpec::named("suspended admission probe", OpSource::Rollover), |_| Ok(
            ()
        ))
        .is_err()
    );
    // Also cover the weak-only binding: successful suspend has no external
    // Collection retaining the core when Engine::close releases its owner.
    let retained = if failed {
        Some(c)
    } else {
        drop(c);
        None
    };
    host.close()
        .expect("direct close must not require public resume or session end");
    assert_eq!(host.engine().state(), LifecycleState::NoCollection);
    assert_eq!(host.review().active_session(), None);
    assert!(host.review().head(session, 3).is_err());
    assert!(host.collection().is_err());
    if let Some(c) = retained {
        assert!(c.reader(ReaderRole::Query).is_err());
    }
    assert!(media.store().pending().is_err());
    assert!(
        host.engine()
            .threads()
            .threads()
            .iter()
            .all(|t| t.name != "meguri-review-file" && t.name != "meguri-derived-writer")
    );
    for path in [options.collection_directory.join("collection.anki2"), derived] {
        let wal = path.with_file_name(format!("{}-wal", path.file_name().unwrap().to_str().unwrap()));
        assert!(std::fs::metadata(wal).map_or(true, |m| m.len() == 0));
    }
    host.close().unwrap();
    assert_eq!(
        ended(&events, session, generation),
        1,
        "close emits exactly one real session end"
    );
    // An independent engine proves the advisory lock was actually released,
    // while the old CoreHost and media clone are still retained.
    let lock_probe = CoreHost::open(options.clone()).unwrap();
    lock_probe.close().unwrap();
    let reopened = process.open(options).unwrap();
    assert_eq!(reopened.review().active_session(), None);
    assert!(reopened.review().head(session, 3).is_err());
    let next = reopened.collection().unwrap();
    assert_ne!(next.generation(), generation);
    {
        let rows = next.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            rows.execute_raw(&format!("SELECT * FROM cards WHERE id = {}", answered.0), &[])
                .unwrap(),
            saved_card
        );
        assert_eq!(
            rows.execute_raw(
                &format!("SELECT * FROM revlog WHERE cid = {} ORDER BY id", answered.0),
                &[]
            )
            .unwrap(),
            saved_logs
        );
    }
    assert_eq!(
        std::fs::read(reopened.media().unwrap().store().media_dir().join("direct-close.txt")).unwrap(),
        b"durable media"
    );
    let StartResult::Started {
        session_id: new_session,
        generation: new_generation,
        head: new_head,
        ..
    } = reopened.review().start(deck, None).unwrap()
    else {
        panic!("reopened session must start");
    };
    assert_ne!(new_session, session);
    assert_eq!(new_generation, next.generation());
    assert_eq!(reopened.review().head(new_session, 3).unwrap(), new_head);
    assert!(new_head.items.iter().all(|i| i.card_id != answered));
    reopened.close().unwrap();
}

#[test]
fn active_session_direct_close_after_successful_suspend() {
    let _lane = TEST_LANE.lock().unwrap();
    direct_close_after_suspend(false);
}

#[test]
fn active_session_direct_close_after_failed_suspend() {
    let _lane = TEST_LANE.lock().unwrap();
    direct_close_after_suspend(true);
}

#[test]
fn shutdown_failure_retains_session_and_journal_then_resume_can_answer_and_retry() {
    let _lane = TEST_LANE.lock().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let options = config(dir.path());
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open(options.clone()).unwrap();
    let (_, session, initial) = start(&host);
    let head = accept(&host, session, &initial);
    host.engine().flush_for_background().unwrap();
    let c = host.collection().unwrap();
    let generation = c.generation();
    let events = host.engine().stream().subscribe();
    let journal = options.collection_directory.join("meguri-pending-answers.jsonl");
    let saved_journal = std::fs::read(&journal).unwrap();
    let media = host.media().unwrap();
    meguri_model::faultpoint::arm(
        "storage.sidecar.before_save",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let result = host.close();
    meguri_model::faultpoint::disarm("storage.sidecar.before_save");
    assert!(result.is_err(), "failed shutdown must never report closed");
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert_eq!(host.review().active_session(), Some(session));
    assert_eq!(std::fs::read(&journal).unwrap(), saved_journal);
    assert_eq!(ended(&events, session, generation), 0);
    assert!(
        process.open(options.clone()).is_err(),
        "failed close retains process ownership"
    );
    assert!(
        CoreHost::open(options).is_err(),
        "failed close retains the advisory lock"
    );
    host.engine().resume().unwrap();
    assert_eq!(host.review().head(session, 3).unwrap(), head);
    assert!(media.store().pending().is_ok(), "resume retains the media binding");
    // An accepted answer proves the temporary ending flag was restored.
    accept(&host, session, &head);
    host.close().unwrap();
    assert_eq!(host.review().active_session(), None);
    assert!(media.store().pending().is_err());
    assert_eq!(ended(&events, session, generation), 1);
}
