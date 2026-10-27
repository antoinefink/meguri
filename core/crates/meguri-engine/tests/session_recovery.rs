//! Actual collection/journal recovery and replacement. Authored; root executes.
mod common;
mod session_support;

use meguri_engine::{
    Collection, Engine, EngineConfig, EngineError, EventKind, LifecycleState, LongOp, OpSource, OpSpec, OpenConfig,
    Ports, SessionId,
    executor::catalog,
    sched::queue::QueueProvider,
    session::{AnswerOutcome, Head, JournalStage, ReviewService, SessionEvent, StartResult},
};
use meguri_model::{
    clock::{Clock, StepClock, UtcOffset},
    ids::{CardId, DeckId, RevlogId},
    rng::SeededRandom,
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use session_support::{Harness, NOW};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Duration,
};

fn start(review: &ReviewService) -> (SessionId, Head) {
    match review.start(DeckId(1), None).unwrap() {
        StartResult::Started { session_id, head, .. } => (session_id, head),
        other => panic!("{other:?}"),
    }
}
fn accept(review: &ReviewService, id: SessionId, head: &Head, rating: Rating, at: i64) -> Head {
    let item = &head.items[0];
    match review
        .answer(id, item.card_id, rating, item.state_token, EpochMillis(at), 123)
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("{other:?}"),
    }
}
fn logs(c: &Collection) -> Vec<i64> {
    c.reader(ReaderRole::Query)
        .unwrap()
        .ids_after(Table::Revlog, None, 100_000)
        .unwrap()
}
fn journal_path(path: &Path) -> PathBuf {
    path.parent().unwrap().join("meguri-pending-answers.jsonl")
}
fn no_review(path: &Path, at: i64) -> (Engine, Collection) {
    no_review_with_clock(path, Arc::new(StepClock::new(at, UtcOffset::UTC, "UTC")))
}
fn no_review_with_clock(path: &Path, clock: Arc<StepClock>) -> (Engine, Collection) {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let c = engine
        .open_raw(path, OpenConfig::new(clock, Arc::new(SeededRandom::new(43))))
        .unwrap();
    (engine, c)
}
fn try_review(
    path: &Path,
    at: i64,
    offset: UtcOffset,
) -> (
    Engine,
    Arc<ReviewService>,
    meguri_engine::Subscription,
    Result<Collection, EngineError>,
) {
    try_review_with_clock(path, Arc::new(StepClock::new(at, offset, "recorded-test")))
}
fn try_review_with_clock(
    path: &Path,
    clock: Arc<StepClock>,
) -> (
    Engine,
    Arc<ReviewService>,
    meguri_engine::Subscription,
    Result<Collection, EngineError>,
) {
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let stream = engine.stream().subscribe();
    let queue = Arc::new(QueueProvider::default());
    let review = ReviewService::new(&engine, queue.clone()).unwrap();
    let ports = Ports::new()
        .observer(queue.clone())
        .generation_listener(queue)
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review.clone());
    let result = engine.open_raw(
        path,
        OpenConfig::new(clock, Arc::new(SeededRandom::new(43))).ports(ports),
    );
    (engine, review, stream, result)
}
fn replacement_events(stream: &meguri_engine::Subscription) -> Vec<meguri_engine::Generation> {
    let mut result = Vec::new();
    while let Some(event) = stream.try_recv() {
        assert!(
            !matches!(
                &event.kind,
                EventKind::RolledOver { .. } | EventKind::TimeZoneChanged { .. } | EventKind::NextDayAtChanged { .. }
            ),
            "replacement must initialize the day clock quietly"
        );
        if let EventKind::CollectionReplaced { generation } = event.kind {
            result.push(generation);
        }
    }
    result
}

#[test]
fn c12_replace_rebinds_success_unchanged_failure_and_changed_failure_before_readmission() {
    for mode in ["success", "unchanged", "changed-error"] {
        let h = Harness::new(4);
        let (old_id, head) = h.start();
        accept(&h.review, old_id, &head, Rating::Easy, NOW);
        h.settle();
        let old_generation = h.collection.generation();
        let stream = h.engine.stream().subscribe();
        let replacement = h.dir.path().join("replacement.anki2");
        if mode != "unchanged" {
            // A fully public first-party collection, closed before publication.
            let source = Harness::new(4);
            source.engine.close().unwrap();
            fs::copy(source.collection.path(), &replacement).unwrap();
        }
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let failure = EngineError::InvalidArgument {
            message: "deterministic swap failure".into(),
        };
        let returned = failure.clone();
        let result = exclusive.replace(move |live| {
            if mode != "unchanged" {
                fs::rename(replacement, live).unwrap();
            }
            if mode == "success" { Ok(()) } else { Err(returned) }
        });
        if mode == "success" {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err(), failure);
        }
        drop(exclusive);
        drop(slot);
        let current = h.engine.collection().unwrap();
        let after_replace = current.last_seq();
        current.check_rollover().unwrap();
        assert_eq!(
            current.last_seq(),
            after_replace,
            "{mode}: rebind must initialize current D8 before readmission"
        );
        let moved = mode != "unchanged";
        assert_eq!(current.generation().0, old_generation.0 + u64::from(moved));
        assert_eq!(
            replacement_events(&stream),
            if moved { vec![current.generation()] } else { vec![] }
        );
        assert!(matches!(h.engine.state(), LifecycleState::Ready { .. }));
        assert_eq!(h.review.active_session(), None);
        let before_late = logs(&current);
        assert!(
            h.review
                .answer(
                    old_id,
                    head.items[0].card_id,
                    Rating::Easy,
                    head.items[0].state_token,
                    EpochMillis(NOW),
                    123
                )
                .is_err()
        );
        assert!(h.review.head(old_id, 3).is_err());
        assert_eq!(
            h.review.active_session(),
            None,
            "late old calls cannot reopen a session"
        );
        assert_eq!(logs(&current), before_late);
        let (id, next) = start(&h.review);
        assert_ne!(old_id, id);
        accept(&h.review, id, &next, Rating::Easy, NOW);
        h.review.flush_for_background().unwrap();
        let expected = if moved { 1 } else { 2 };
        assert_eq!(logs(&current).len(), expected);
        let path = current.path().to_owned();
        h.engine.close().unwrap();
        for _ in 0..2 {
            let (engine, c, _) = session_support::open(&path, h.clock.clone());
            assert_eq!(logs(&c).len(), expected);
            engine.close().unwrap();
        }
    }
}

#[test]
fn c12_replace_checkpoint_failure_reopens_and_rebinds_the_unchanged_file() {
    let h = Harness::new(4);
    let path = h.collection.path().to_owned();
    h.engine.close().unwrap();
    let vfs = meguri_storage::counting_vfs::CountingVfs::for_path(&path);
    let (engine, c, review) = session_support::open(&path, h.clock.clone());
    let (id, head) = start(&review);
    accept(&review, id, &head, Rating::Easy, NOW);
    review.flush_for_background().unwrap();
    let generation = c.generation();
    let stream = engine.stream().subscribe();
    let slot = engine.acquire_slot(LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    vfs.fail_next_main_sync();
    let called = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let call = called.clone();
    assert!(
        exclusive
            .replace(move |_| {
                call.store(true, std::sync::atomic::Ordering::Release);
                Ok(())
            })
            .is_err()
    );
    drop(exclusive);
    drop(slot);
    assert!(!called.load(std::sync::atomic::Ordering::Acquire));
    assert!(
        vfs.records()
            .iter()
            .any(|r| r.result == meguri_storage::rusqlite::ffi::SQLITE_IOERR_FSYNC)
    );
    assert_eq!(engine.generation(), generation);
    let after_replace = c.last_seq();
    c.check_rollover().unwrap();
    assert_eq!(
        c.last_seq(),
        after_replace,
        "failed swap rebind must leave D8 initialized"
    );
    assert!(replacement_events(&stream).is_empty());
    assert_eq!(review.active_session(), None);
    let (id, head) = start(&review);
    accept(&review, id, &head, Rating::Easy, NOW);
    review.flush_for_background().unwrap();
    assert_eq!(logs(&c).len(), 2);
    engine.close().unwrap();
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&path, h.clock.clone());
        assert_eq!(logs(&c).len(), 2);
        engine.close().unwrap();
    }
}

#[test]
fn c12_rebind_io_or_corruption_failure_forbids_ready_and_keeps_retired_evidence() {
    for mode in ["io", "corrupt"] {
        let h = Harness::new(3);
        let (id, head) = h.start();
        accept(&h.review, id, &head, Rating::Easy, NOW);
        h.settle();
        let stream = h.engine.stream().subscribe();
        let path = h.collection.path().to_owned();
        let journal = journal_path(&path);
        let evidence = h.dir.path().join("retired-evidence");
        let saved = evidence.clone();
        let target = journal.clone();
        let slot = h.engine.acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        let error = exclusive
            .replace(move |_| {
                let bytes = fs::read(&target).unwrap();
                assert!(String::from_utf8_lossy(&bytes).contains("Retired"));
                fs::write(saved, bytes).unwrap();
                if mode == "io" {
                    fs::remove_file(&target).unwrap();
                    fs::create_dir(&target).unwrap();
                } else {
                    fs::write(&target, b"complete-corruption\n").unwrap();
                }
                Err(EngineError::InvalidArgument {
                    message: "swap kept old file".into(),
                })
            })
            .unwrap_err();
        drop(exclusive);
        drop(slot);
        assert!(
            !error.to_string().contains("swap kept old file"),
            "rebind failure must propagate: {error}"
        );
        assert!(matches!(h.engine.state(), LifecycleState::Failed { .. }));
        assert!(replacement_events(&stream).is_empty());
        assert!(h.review.start(DeckId(1), None).is_err());
        assert!(
            h.collection
                .run(OpSpec::named("closed admission", OpSource::Maintenance), |_| Ok(()))
                .is_err()
        );
        assert!(String::from_utf8_lossy(&fs::read(evidence).unwrap()).contains("Retired"));
        if mode == "corrupt" {
            assert_eq!(fs::read(journal).unwrap(), b"complete-corruption\n");
        }
    }
}

// Repeated fixture protocol: capture real file-worker bytes, close all owners,
// then restore a captured SQLite image in place (same native inode). No replay
// implementation or synthetic answer/receipt is substituted for production.
struct CapturedAnswer {
    _dir: tempfile::TempDir,
    path: PathBuf,
    card: CardId,
    assigned: i64,
    before_db: Vec<u8>,
    before: common::steps::Dump,
    after: common::steps::Dump,
    intent: Vec<u8>,
    prepared: Vec<u8>,
}
fn capture_answer(setup: impl FnOnce(&Harness)) -> CapturedAnswer {
    let h = Harness::new(3);
    setup(&h);
    let (id, head) = h.start();
    let card = head.items[0].card_id;
    let before = common::steps::dump(&h.collection);
    let before_logs = logs(&h.collection);
    let path = h.collection.path().to_owned();
    let file = path.clone();
    let before_db = h
        .collection
        .submit_maintenance(catalog::WAL_CHECKPOINT, Duration::ZERO, move |w| {
            assert!(w.checkpoint()?.succeeded());
            Ok(fs::read(file).map_err(meguri_storage::StorageError::from)?)
        })
        .wait()
        .unwrap();
    let journal = journal_path(&path);
    let captures = Arc::new(Mutex::new(Vec::new()));
    let output = captures.clone();
    h.review
        .journal_hook_for_testing(Arc::new(move |stage| {
            if matches!(stage, JournalStage::IntentSynced | JournalStage::PreparedSynced) {
                output.lock().unwrap().push((stage, fs::read(&journal).unwrap()));
            }
        }))
        .unwrap();
    accept(&h.review, id, &head, Rating::Easy, NOW);
    h.settle();
    let assigned = logs(&h.collection)
        .into_iter()
        .find(|id| !before_logs.contains(id))
        .unwrap();
    let after = common::steps::dump(&h.collection);
    h.engine.close().unwrap();
    let stages = captures.lock().unwrap();
    let frame = |stage| stages.iter().find(|(s, _)| *s == stage).unwrap().1.clone();
    let captured = CapturedAnswer {
        path,
        card,
        assigned,
        before_db,
        before,
        after,
        intent: frame(JournalStage::IntentSynced),
        prepared: frame(JournalStage::PreparedSynced),
        _dir: h.dir,
    };
    drop(h.review);
    captured
}
fn restore_uncommitted(a: &CapturedAnswer, journal: &[u8]) {
    fs::write(&a.path, &a.before_db).unwrap();
    fs::write(journal_path(&a.path), journal).unwrap();
}
fn dropped(stream: &meguri_engine::Subscription) -> Vec<(u64, CardId, String)> {
    let mut result = Vec::new();
    while let Some(event) = stream.try_recv() {
        if let EventKind::Extension(payload) = &event.kind
            && let Some(SessionEvent::RecoveryDropped { event, card_id, reason }) = payload.as_any().downcast_ref()
        {
            result.push((*event, *card_id, reason.clone()));
        }
    }
    result
}
fn reopen_twice_matches(a: &CapturedAnswer, expected: &common::steps::Dump) {
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&a.path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(&common::steps::dump(&c), expected);
        engine.close().unwrap();
    }
}

#[test]
fn c12_historical_answer_recovery_precedes_current_day_and_offset_maintenance_twice() {
    use meguri_model::{card::CardQueue, ids::DeckConfigId};
    use meguri_storage::{Cell, ConfigValue, StampMode};

    for creation_offset in [false, true] {
        let mut buried = CardId(0);
        let mut recorded_day = 0;
        let a = capture_answer(|h| {
            h.collection
                .run(
                    OpSpec::named("recorded day policy", OpSource::SyncIncoming).stamp(StampMode::Raw),
                    move |ctx| {
                        if creation_offset {
                            ctx.scope().set_config("creationOffset", &ConfigValue::from(0_i64))?;
                        } else {
                            ctx.scope().remove_config("creationOffset")?;
                        }
                        ctx.scope().set_config("rollover", &ConfigValue::from(4_i64))?;
                        Ok(())
                    },
                )
                .unwrap();
            recorded_day = h.collection.today().unwrap();
            buried = h.cards[2];
            h.collection
                .run(
                    OpSpec::named("independent recorded-day burial", OpSource::SyncIncoming).stamp(StampMode::Raw),
                    move |ctx| {
                        ctx.scope()
                            .set_config("lastUnburied", &ConfigValue::from(i64::from(recorded_day)))?;
                        let mut card = ctx.scope().card(buried)?.unwrap();
                        card.queue = CardQueue::BuriedByScheduler;
                        card.flags = 5;
                        ctx.scope().put(&card)?;
                        let mut preset = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                        let mut bytes = preset.config.to_bytes().into_owned();
                        // Supported opaque field 100: keep the real T13 checks.
                        bytes.extend_from_slice(&[0xa0, 0x06, 0x07]);
                        preset.config = meguri_model::proto::ProtoBlob::decode(&bytes).unwrap();
                        ctx.scope().put(&preset)?;
                        ctx.scope()
                            .set_config("historical-opaque", &ConfigValue::String("retained".into()))?;
                        Ok(())
                    },
                )
                .unwrap();
        });
        assert_ne!(a.card, buried, "the automatic-unbury witness is not the answered card");

        // The real accepted answer is the reference, including its assigned
        // full revlog row, card data, stamps, deck counters and opaque cells.
        let (engine, c) = no_review(&a.path, NOW);
        assert_eq!(common::steps::dump(&c), a.after);
        let r = c.reader(ReaderRole::Query).unwrap();
        let mut accepted_rows = Vec::new();
        for table in [
            Table::Notes,
            Table::Cards,
            Table::Revlog,
            Table::Decks,
            Table::DeckConfig,
        ] {
            for id in r.ids_after(table, None, 100_000).unwrap() {
                accepted_rows.push((table, id, r.row_image(table, &RowKey::Id(id)).unwrap().unwrap()));
            }
        }
        let accepted_col = r.col().unwrap();
        assert_eq!(r.card(buried).unwrap().unwrap().queue, CardQueue::BuriedByScheduler);
        assert_eq!(
            r.config_value("lastUnburied").unwrap(),
            Some(ConfigValue::from(i64::from(recorded_day)))
        );
        let creation = r.config_value("creationOffset").unwrap();
        assert_eq!(creation.is_some(), creation_offset);
        drop(r);
        engine.close().unwrap();

        let future = NOW + 3 * 86_400_000;
        let offset = UtcOffset::from_seconds(9 * 3600).unwrap();
        let clock = Arc::new(StepClock::new(future, offset, "future-device"));
        // Independently run current D8 on the already-applied answer. Only
        // localOffset, lastUnburied and the separate buried card's queue may
        // change. No replay decision or receipt is fabricated for this image.
        let (engine, c) = no_review_with_clock(&a.path, clock.clone());
        let future_day = c.today().unwrap();
        assert!(future_day > recorded_day);
        let r = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            r.col().unwrap(),
            accepted_col,
            "automatic D8 preserves col, including mod"
        );
        for (table, id, image) in &accepted_rows {
            let current = r.row_image(*table, &RowKey::Id(*id)).unwrap().unwrap();
            if *table == Table::Cards && *id == buried.0 {
                assert_eq!(current.len(), image.len());
                let changes: Vec<_> = image.iter().zip(&current).filter(|(old, new)| old != new).collect();
                assert_eq!(
                    changes,
                    vec![(&Cell::Integer(-2), &Cell::Integer(0))],
                    "unbury preserves every other card cell, including mod/usn"
                );
            } else {
                assert_eq!(&current, image, "current D8 must preserve {table:?}/{id}");
            }
        }
        assert_eq!(
            r.config_value("localOffset").unwrap(),
            Some(ConfigValue::from(-540_i64))
        );
        assert_eq!(
            r.config_value("lastUnburied").unwrap(),
            Some(ConfigValue::from(i64::from(future_day)))
        );
        assert_eq!(r.config_value("creationOffset").unwrap(), creation);
        assert_eq!(r.config_value("rollover").unwrap(), Some(ConfigValue::from(4_i64)));
        assert_eq!(
            r.config_value("historical-opaque").unwrap(),
            Some(ConfigValue::String("retained".into()))
        );
        drop(r);
        let expected = common::steps::dump(&c);
        let current_only_seq = c.last_seq();
        engine.close().unwrap();

        for journal in [&a.intent, &a.prepared] {
            restore_uncommitted(&a, journal);
            for opening in 0..2 {
                let (engine, review, stream, result) = try_review_with_clock(&a.path, clock.clone());
                let c = result.unwrap();
                assert!(matches!(engine.state(), LifecycleState::Ready { .. }));
                assert_eq!(review.active_session(), None);
                assert_eq!(logs(&c), [a.assigned]);
                assert_eq!(
                    common::steps::dump(&c),
                    expected,
                    "open {opening}: recorded answer plus only current D8 effects"
                );
                if opening == 1 {
                    assert_eq!(
                        c.last_seq(),
                        current_only_seq,
                        "terminal replay must run zero historical operations on the second open"
                    );
                }
                let seq = c.last_seq();
                c.check_rollover().unwrap();
                assert_eq!(c.last_seq(), seq, "current D8 must be initialized before Ready");
                assert_eq!(c.today().unwrap(), future_day);
                assert_eq!(clock.now_ms(), future, "recovery must not rewind the supplied clock");
                assert_eq!(clock.utc_offset_at(future), offset);
                while let Some(event) = stream.try_recv() {
                    assert!(
                        !matches!(
                            &event.kind,
                            EventKind::RolledOver { .. }
                                | EventKind::TimeZoneChanged { .. }
                                | EventKind::NextDayAtChanged { .. }
                        ),
                        "historical recovery and first current D8 must emit no live day events"
                    );
                    if let EventKind::Extension(payload) = &event.kind {
                        assert!(
                            payload.as_any().downcast_ref::<SessionEvent>().is_none(),
                            "recovery must neither drop the accepted answer nor publish session/head events"
                        );
                    }
                }
                engine.close().unwrap();
            }
        }
    }
}

#[test]
fn c12_prepared_taken_and_capped_ids_replay_exact_full_collection_twice() {
    use meguri_model::{
        revlog::{RevlogEntry, RevlogKind},
        usn::Usn,
    };
    for capped in [false, true] {
        let a = capture_answer(|h| {
            let other = h.cards[2];
            h.collection
                .run(
                    OpSpec::named("taken allocator ids", OpSource::SyncIncoming).stamp(meguri_storage::StampMode::Raw),
                    move |ctx| {
                        for id in [NOW, if capped { NOW + 2 * 86_400_000 } else { NOW + 17 }] {
                            ctx.scope().put(&RevlogEntry {
                                id: RevlogId(id),
                                card_id: other,
                                usn: Usn(3),
                                button: 2,
                                interval: 1,
                                last_interval: 0,
                                ease_factor: 2500,
                                answer_millis: 9,
                                kind: RevlogKind::Learning,
                            })?;
                        }
                        Ok(())
                    },
                )
                .unwrap();
        });
        assert_eq!(a.assigned, if capped { NOW + 1 } else { NOW + 18 });
        restore_uncommitted(&a, &a.prepared);
        reopen_twice_matches(&a, &a.after);
    }
}

#[test]
fn c12_exact_full_row_witness_skips_without_overwriting_later_card_edits() {
    let a = capture_answer(|_| {});
    fs::write(journal_path(&a.path), &a.prepared).unwrap();
    let (engine, c) = no_review(&a.path, NOW);
    let card = a.card;
    c.run(
        OpSpec::named("later external card", OpSource::SyncIncoming),
        move |ctx| {
            let mut row = ctx.scope().card(card)?.unwrap();
            row.due += 7;
            row.interval += 9;
            row.flags = 5;
            ctx.scope().put(&row)?;
            Ok(())
        },
    )
    .unwrap();
    let expected = common::steps::dump(&c);
    engine.close().unwrap();
    reopen_twice_matches(&a, &expected);
}

#[test]
fn c12_different_full_row_at_assigned_id_drops_without_fallback_or_row_changes() {
    let a = capture_answer(|_| {});
    restore_uncommitted(&a, &a.prepared);
    let (engine, c) = no_review(&a.path, NOW);
    let assigned = a.assigned;
    let card = a.card;
    c.run(
        OpSpec::named("conflicting assigned witness", OpSource::SyncIncoming).stamp(meguri_storage::StampMode::Raw),
        move |ctx| {
            ctx.scope().put(&meguri_model::revlog::RevlogEntry {
                id: RevlogId(assigned),
                card_id: card,
                usn: meguri_model::usn::Usn(-1),
                button: 4,
                interval: 1,
                last_interval: 0,
                ease_factor: 2500,
                answer_millis: 999,
                kind: meguri_model::revlog::RevlogKind::Learning,
            })?;
            Ok(())
        },
    )
    .unwrap();
    let expected = common::steps::dump(&c);
    engine.close().unwrap();
    // Keep the actual stream from before open, so the explicit conflict report
    // is observed even though resolution happens before Ready.
    let (engine, _review, stream, result) = try_review(&a.path, NOW, UtcOffset::UTC);
    let c = result.unwrap();
    assert_eq!(common::steps::dump(&c), expected);
    let conflicts = dropped(&stream);
    assert_eq!(conflicts.len(), 1);
    assert_eq!(conflicts[0].1, a.card);
    assert!(conflicts[0].2.contains("differs"));
    assert_eq!(logs(&c), [assigned]);
    engine.close().unwrap();
    reopen_twice_matches(&a, &expected);
}

#[test]
fn c12_unrelated_future_revlog_cannot_witness_the_prepared_event() {
    let a = capture_answer(|_| {});
    restore_uncommitted(&a, &a.prepared);
    let (engine, c) = no_review(&a.path, NOW);
    let other = c
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(Table::Cards, None, 10)
        .unwrap()
        .into_iter()
        .map(CardId)
        .find(|id| *id != a.card)
        .unwrap();
    c.run(
        OpSpec::named("unrelated future log", OpSource::SyncIncoming).stamp(meguri_storage::StampMode::Raw),
        move |ctx| {
            ctx.scope().put(&meguri_model::revlog::RevlogEntry {
                id: RevlogId(NOW + 100_000),
                card_id: other,
                usn: meguri_model::usn::Usn(5),
                button: 1,
                interval: 7,
                last_interval: 3,
                ease_factor: 2600,
                answer_millis: 50,
                kind: meguri_model::revlog::RevlogKind::Review,
            })?;
            Ok(())
        },
    )
    .unwrap();
    let future = c
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Revlog, &RowKey::Id(NOW + 100_000))
        .unwrap()
        .unwrap();
    engine.close().unwrap();
    for _ in 0..2 {
        let (engine, c, _) = session_support::open(&a.path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(logs(&c), [a.assigned, NOW + 100_000]);
        let r = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            r.row_image(Table::Revlog, &RowKey::Id(NOW + 100_000)).unwrap().unwrap(),
            future
        );
        assert_eq!(
            r.card(a.card).unwrap().unwrap().reps,
            1,
            "future timestamp must not skip this event"
        );
        drop(r);
        engine.close().unwrap();
    }
}

// Mutate only the framing/guard under test in captured production bytes.
fn change_record(bytes: &[u8], index: usize, change: impl FnOnce(&mut serde_json::Value)) -> Vec<u8> {
    use sha2::{Digest, Sha256};
    let mut frames: Vec<serde_json::Value> = bytes
        .split(|b| *b == b'\n')
        .filter(|s| !s.is_empty())
        .map(|s| serde_json::from_slice(s).unwrap())
        .collect();
    let frame = &mut frames[index];
    let mut record: serde_json::Value = serde_json::from_str(frame["payload"].as_str().unwrap()).unwrap();
    change(&mut record);
    let payload = serde_json::to_string(&record).unwrap();
    let mut hash = Sha256::new();
    hash.update(b"meguri.pending.frame.v1\0");
    hash.update(frame["number"].as_u64().unwrap().to_le_bytes());
    hash.update(payload.as_bytes());
    let checksum: [u8; 32] = hash.finalize().into();
    frame["payload"] = payload.into();
    frame["checksum"] = serde_json::to_value(checksum).unwrap();
    let mut result = Vec::new();
    for frame in frames {
        result.extend(serde_json::to_vec(&frame).unwrap());
        result.push(b'\n');
    }
    result
}

#[test]
fn c12_torn_final_tail_replays_valid_prefix_once_and_preserves_complete_rows() {
    let a = capture_answer(|_| {});
    let mut torn = a.prepared.clone();
    torn.extend_from_slice(b"{\"number\":4,\"payload\":\"unacknowledged");
    restore_uncommitted(&a, &torn);
    reopen_twice_matches(&a, &a.after);
    let journal = fs::read(journal_path(&a.path)).unwrap();
    assert!(!String::from_utf8_lossy(&journal).contains("unacknowledged"));
}

#[test]
fn c12_complete_interior_checksum_sequence_dependency_version_corruption_retains_file_and_forbids_ready() {
    let a = capture_answer(|_| {});
    for mode in ["complete", "interior", "checksum", "sequence", "dependency", "version"] {
        let corrupt = match mode {
            "complete" => {
                let mut b = a.prepared.clone();
                b.extend_from_slice(b"invalid-complete-frame\n");
                b
            }
            "interior" => {
                let mut b = a.prepared.clone();
                let at = b.iter().position(|c| *c == b'\n').unwrap();
                b.insert(at, b'\n');
                b
            }
            "checksum" | "sequence" => {
                let mut frames: Vec<serde_json::Value> = a
                    .prepared
                    .split(|b| *b == b'\n')
                    .filter(|s| !s.is_empty())
                    .map(|s| serde_json::from_slice(s).unwrap())
                    .collect();
                if mode == "checksum" {
                    frames[1]["checksum"][0] = (frames[1]["checksum"][0].as_u64().unwrap() ^ 1).into();
                } else {
                    frames[1]["number"] = 17.into();
                }
                frames
                    .into_iter()
                    .flat_map(|f| {
                        let mut b = serde_json::to_vec(&f).unwrap();
                        b.push(b'\n');
                        b
                    })
                    .collect()
            }
            "dependency" => change_record(&a.prepared, 1, |r| {
                r["Intent"]["answer"]["base"] = serde_json::json!({"Predecessor": 8})
            }),
            "version" => change_record(&a.prepared, 0, |r| r["Header"]["version"] = 99.into()),
            _ => unreachable!(),
        };
        restore_uncommitted(&a, &corrupt);
        let (engine, review, _, result) = try_review(&a.path, NOW, UtcOffset::UTC);
        assert!(result.is_err(), "{mode}");
        assert!(matches!(engine.state(), LifecycleState::Failed { .. }), "{mode}");
        assert_eq!(
            fs::read(journal_path(&a.path)).unwrap(),
            corrupt,
            "{mode}: retain exact evidence"
        );
        assert!(review.start(DeckId(1), None).is_err());
        drop(review);
        drop(engine);
        let (engine, c) = no_review(&a.path, NOW);
        assert_eq!(common::steps::dump(&c), a.before, "{mode}: no replay writes");
        engine.close().unwrap();
    }
    // Failed-open owners have unwound: repair only the disposable test input,
    // then a real public open can recover with no leaked lock/file worker.
    restore_uncommitted(&a, &a.prepared);
    reopen_twice_matches(&a, &a.after);
}

#[test]
fn c12_recovery_changed_semantic_home_global_policy_and_history_drop_without_answer_writes() {
    use meguri_model::ids::DeckConfigId;
    use meguri_storage::ConfigValue;
    for mode in [
        "parameters",
        "learning",
        "retention",
        "cap",
        "bury",
        "leech",
        "home",
        "home-dr",
        "global-steps",
        "ahead",
        "creation-presence",
        "rollover",
        "history",
        "unsupported",
    ] {
        let a = capture_answer(|_| {});
        restore_uncommitted(&a, &a.intent);
        let (engine, c) = no_review(&a.path, NOW);
        let card = a.card;
        c.run(
            OpSpec::named("external semantic edit", OpSource::SyncIncoming).stamp(meguri_storage::StampMode::Raw),
            move |ctx| {
                match mode {
                    "home" | "home-dr" => {
                        let mut d = ctx.scope().deck(DeckId(1))?.unwrap();
                        let meguri_model::proto::deck::DeckKind::Normal(n) = &mut d.kind.kind else {
                            panic!()
                        };
                        if mode == "home" {
                            n.config_id = 900;
                        } else {
                            n.desired_retention = Some(0.8);
                        }
                        ctx.scope().put(&d)?;
                    }
                    "global-steps" => {
                        ctx.scope()
                            .set_config("fsrsShortTermWithStepsEnabled", &ConfigValue::Bool(true))?;
                    }
                    "ahead" => {
                        ctx.scope().set_config("collapseTime", &ConfigValue::from(321_i64))?;
                    }
                    "creation-presence" => {
                        if ctx.scope().config_value("creationOffset")?.is_some() {
                            ctx.scope().remove_config("creationOffset")?;
                        } else {
                            ctx.scope().set_config("creationOffset", &ConfigValue::from(0_i64))?;
                        }
                    }
                    "rollover" => {
                        ctx.scope().set_config("rollover", &ConfigValue::from(5_i64))?;
                    }
                    "history" => {
                        ctx.scope().put(&meguri_model::revlog::RevlogEntry {
                            id: RevlogId(NOW - 1000),
                            card_id: card,
                            usn: meguri_model::usn::Usn(4),
                            button: 3,
                            interval: 10,
                            last_interval: 3,
                            ease_factor: 2500,
                            answer_millis: 50,
                            kind: meguri_model::revlog::RevlogKind::Review,
                        })?;
                    }
                    _ => {
                        let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                        match mode {
                            "parameters" => p.config.set_fsrs_params_6(vec![1.0; 21]),
                            "learning" => p.config.learn_steps = vec![2.0, 20.0],
                            "retention" => p.config.desired_retention = 0.8,
                            "cap" => p.config.cap_answer_time_to_secs = 1,
                            "bury" => p.config.bury_new = !p.config.bury_new,
                            "leech" => p.config.leech_threshold += 1,
                            "unsupported" => {
                                let mut bytes = p.config.to_bytes().into_owned();
                                bytes.extend_from_slice(&[0x3a, 0x01, 0x01]);
                                p.config = meguri_model::proto::ProtoBlob::decode(&bytes).unwrap();
                            }
                            _ => unreachable!(),
                        }
                        ctx.scope().put(&p)?;
                    }
                }
                Ok(())
            },
        )
        .unwrap();
        let expected = common::steps::dump(&c);
        engine.close().unwrap();
        let (engine, _review, stream, result) = try_review(&a.path, NOW, UtcOffset::UTC);
        let c = result.unwrap();
        let reports = dropped(&stream);
        assert_eq!(reports.len(), 1, "{mode}: explicit stale/drop report");
        assert_eq!(reports[0].1, a.card);
        assert_eq!(
            common::steps::dump(&c),
            expected,
            "{mode}: captured event cannot overwrite changed semantics"
        );
        assert!(!logs(&c).contains(&a.assigned), "{mode}");
        engine.close().unwrap();
        reopen_twice_matches(&a, &expected);
    }
}

#[test]
fn c12_recovery_allows_unrelated_mtime_opaque_counters_and_admitted_fsrs_off() {
    use meguri_model::ids::DeckConfigId;
    use meguri_storage::ConfigValue;
    let a = capture_answer(|_| {});
    restore_uncommitted(&a, &a.prepared);
    let (engine, c) = no_review(&a.path, NOW);
    c.run(
        OpSpec::named("allowed external edits", OpSource::SyncIncoming).stamp(meguri_storage::StampMode::Raw),
        |ctx| {
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.mtime.0 += 300;
            p.config.new_per_day = 17;
            let mut bytes = p.config.to_bytes().into_owned();
            bytes.extend_from_slice(&[0xa0, 0x06, 0x07]); // unrelated protobuf field 100
            p.config = meguri_model::proto::ProtoBlob::decode(&bytes).unwrap();
            ctx.scope().put(&p)?;
            let mut d = ctx.scope().deck(DeckId(1))?.unwrap();
            d.mtime.0 += 99;
            d.common.new_studied += 9;
            ctx.scope().put(&d)?;
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(false))?;
            ctx.scope()
                .set_config("unrelated-test-value", &ConfigValue::String("opaque".into()))?;
            Ok(())
        },
    )
    .unwrap();
    let r = c.reader(ReaderRole::Query).unwrap();
    let preset = r.row_image(Table::DeckConfig, &RowKey::Id(1)).unwrap();
    let before_counter = r.deck(DeckId(1)).unwrap().unwrap().common.new_studied;
    drop(r);
    engine.close().unwrap();
    let mut stable = None;
    for _ in 0..2 {
        let (engine, c, review) = session_support::open(&a.path, Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC")));
        assert_eq!(logs(&c), [a.assigned]);
        let r = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(r.card(a.card).unwrap().unwrap().reps, 1);
        assert_eq!(r.row_image(Table::DeckConfig, &RowKey::Id(1)).unwrap(), preset);
        assert_eq!(
            r.config_value("unrelated-test-value").unwrap(),
            Some(ConfigValue::String("opaque".into()))
        );
        assert_eq!(
            r.deck(DeckId(1)).unwrap().unwrap().common.new_studied,
            before_counter + 1
        );
        drop(r);
        assert!(
            matches!(review.start(DeckId(1), None).unwrap(), StartResult::Refused(_)),
            "new admission remains current-policy checked"
        );
        let dump = common::steps::dump(&c);
        if let Some(expected) = &stable {
            assert_eq!(&dump, expected);
        } else {
            stable = Some(dump);
        }
        engine.close().unwrap();
    }
}

// These are synchronous I/O-boundary regressions. Child-process termination
// at the recovery-journal boundaries remains execution-pending (R3 P2).
#[derive(Clone, Copy, PartialEq, Eq)]
enum RecoveryFailure {
    BeforeSwap,
    RetainedWal,
    MarkerPublication,
    ForeignBinding,
    Activated,
}

static MARKER_FAILURE_ROOT: Mutex<Option<PathBuf>> = Mutex::new(None);
fn fail_marker_after_archive_rename() {
    use meguri_model::faultpoint::{self, Fault};
    let mut root = MARKER_FAILURE_ROOT.lock().unwrap();
    let Some(path) = root.as_ref() else {
        return;
    };
    if recovery_archives(path).is_empty() {
        return;
    }
    // This callback runs after archive rename; AtomicFile still syncs its
    // directory before prepare starts marker publication. The next rename is
    // the marker. Leave the error armed across rollback too, exercising its
    // own reported post-rename failure and exact installed-original proof.
    root.take();
    faultpoint::arm("storage.atomic.after_rename", Fault::IoError(std::io::ErrorKind::Other));
}
type ForeignJournal = (PathBuf, Vec<u8>, (u64, u64));
static FOREIGN_JOURNAL: Mutex<Option<ForeignJournal>> = Mutex::new(None);
fn substitute_retired_marker() {
    use meguri_model::faultpoint::{self, Fault};
    let mut foreign = FOREIGN_JOURNAL.lock().unwrap();
    let (path, bytes, identity) = foreign.as_mut().unwrap();
    *bytes = fs::read(&*path).unwrap();
    assert!(String::from_utf8_lossy(bytes).contains("Retired"));
    let replacement = path.with_file_name("foreign-journal.jsonl");
    fs::write(&replacement, &*bytes).unwrap();
    fs::rename(&replacement, &*path).unwrap();
    *identity = native(path);
    // The exact marker bytes on a foreign inode are still an unknown binding.
    // Stop before activation so R6 proves old, then require rollback to refuse
    // that foreign inode without destroying either it or the archived answers.
    faultpoint::arm(
        "replace.media.after_final_database_verification",
        Fault::IoError(std::io::ErrorKind::Other),
    );
}
fn recovery_archives(collection: &Path) -> Vec<PathBuf> {
    fs::read_dir(collection.parent().unwrap())
        .unwrap()
        .map(Result::unwrap)
        .filter(|e| e.file_name().to_string_lossy().ends_with(".recovery.jsonl"))
        .map(|e| e.path())
        .collect()
}
fn native(path: &Path) -> (u64, u64) {
    use std::os::unix::fs::MetadataExt;
    let metadata = fs::symlink_metadata(path).unwrap();
    (metadata.dev(), metadata.ino())
}
fn wal_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push("-wal");
    name.into()
}
fn install_real_retained_wal(path: &Path) {
    // Capture a real SQLite WAL on an independent disposable copy. Closing
    // that connection only cleans the source; no live owner of path is opened
    // or moved, and the captured journal's native main binding stays intact.
    let source_dir = tempfile::tempdir().unwrap();
    let source = source_dir.path().join("collection.anki2");
    fs::copy(path, &source).unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&source).unwrap();
    db.execute_batch("PRAGMA journal_mode=WAL; PRAGMA wal_autocheckpoint=0; UPDATE col SET mod=987654321;")
        .unwrap();
    fs::copy(&source, path).unwrap();
    fs::copy(wal_path(&source), wal_path(path)).unwrap();
    assert!(fs::metadata(wal_path(path)).unwrap().len() > 32);
    db.close().unwrap();
}

#[test]
fn recovery_restore_archives_real_unresolved_answers_and_preserves_preactivation_failure() {
    recovery_restore_failure(RecoveryFailure::BeforeSwap);
}
#[test]
fn recovery_restore_retained_wal_refusal_restores_original_journal_and_publication() {
    recovery_restore_failure(RecoveryFailure::RetainedWal);
}
#[test]
fn recovery_restore_marker_postrename_failure_restores_original_journal_before_swap() {
    recovery_restore_failure(RecoveryFailure::MarkerPublication);
}
#[test]
fn recovery_restore_retained_wal_activation_never_replays_retired_answers() {
    recovery_restore_failure(RecoveryFailure::Activated);
}
#[test]
fn recovery_restore_refuses_to_overwrite_foreign_marker_even_with_identical_bytes() {
    recovery_restore_failure(RecoveryFailure::ForeignBinding);
}
fn recovery_restore_failure(failure: RecoveryFailure) {
    use meguri_engine::{
        FailedReason, ProgressSink,
        device_state::{DeviceState, LastQuickCheck, PublicationDecision},
    };
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::space::StatvfsSpace;

    for prepared in [false, true] {
        // Real production Intent/Prepared prefixes retain native binding and
        // checksums. The whole original, including its torn tail, is evidence.
        let captured = capture_answer(|_| {});
        restore_uncommitted(&captured, &captured.intent);
        let mut old = if prepared {
            captured.prepared.clone()
        } else {
            captured.intent.clone()
        };
        old.extend_from_slice(b"torn recovery tail");
        let header_end = captured.intent.iter().position(|b| *b == b'\n').unwrap() + 1;
        fs::write(journal_path(&captured.path), &captured.intent[..header_end]).unwrap();
        let backup_dir = common::backup_directory(&captured.path);
        let backups = backup_dir.path();
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        let host = meguri_testkit::backup_support::open_restore_host(
            &captured.path,
            backups,
            clock.clone(),
            Arc::new(StatvfsSpace),
        )
        .unwrap();
        let expected_logs = logs(&host.collection().unwrap());
        let backup = host.engine().back_up_now(&Default::default()).unwrap().receipt;
        host.close().unwrap();
        drop(host);
        fs::write(journal_path(&captured.path), &old).unwrap();
        // All new boundaries include real retained WAL bytes, including the
        // marker publication error before a swap has even begun.
        if failure != RecoveryFailure::BeforeSwap {
            install_real_retained_wal(&captured.path);
        }
        fs::write(&captured.path, b"unreadable main retaining real unresolved journal").unwrap();
        let damaged = fs::read(&captured.path).unwrap();
        let main_native = native(&captured.path);
        let journal_native = native(&journal_path(&captured.path));
        let old_wal = fs::read(wal_path(&captured.path)).ok();
        let wal_native = old_wal.as_ref().map(|_| native(&wal_path(&captured.path)));
        let device = DeviceState::for_collection(&captured.path);
        device
            .set_last_quick_check(&LastQuickCheck {
                instant: NOW,
                passed: false,
            })
            .unwrap();
        let prior = device.publication().unwrap().unwrap();
        let host = meguri_testkit::backup_support::open_restore_host(
            &captured.path,
            backups,
            clock.clone(),
            Arc::new(StatvfsSpace),
        )
        .unwrap();
        let generation = host.engine().generation();
        let point = match failure {
            RecoveryFailure::BeforeSwap => "replace.before_live_wal_check",
            RecoveryFailure::RetainedWal => "replace.after_live_wal_removed",
            RecoveryFailure::MarkerPublication => {
                *MARKER_FAILURE_ROOT.lock().unwrap() = Some(captured.path.clone());
                "storage.atomic.after_rename"
            }
            RecoveryFailure::ForeignBinding => {
                *FOREIGN_JOURNAL.lock().unwrap() = Some((journal_path(&captured.path), Vec::new(), (0, 0)));
                "replace.after_live_wal_removed"
            }
            RecoveryFailure::Activated => "replace.after_rename",
        };
        let fault = if failure == RecoveryFailure::MarkerPublication {
            Fault::Callback(fail_marker_after_archive_rename)
        } else if failure == RecoveryFailure::ForeignBinding {
            Fault::Callback(substitute_retired_marker)
        } else {
            Fault::IoError(std::io::ErrorKind::Other)
        };
        faultpoint::arm(point, fault);
        let refused = host.restore_backup(&backup.name, &Default::default(), &ProgressSink::new());
        faultpoint::disarm(point);
        faultpoint::disarm("replace.media.after_final_database_verification");
        let foreign = if failure == RecoveryFailure::ForeignBinding {
            Some(FOREIGN_JOURNAL.lock().unwrap().take().unwrap())
        } else {
            None
        };
        let expected_journal = foreign.as_ref().map_or(&old, |(_, bytes, _)| bytes);
        let refusal = refused.unwrap_err();
        if foreign.is_some() {
            assert!(refusal.to_string().contains("binding changed"), "{refusal}");
        } else {
            assert!(refusal.to_string().contains(point), "{refusal}");
        }
        assert_eq!(refusal.candidate_active(), failure == RecoveryFailure::Activated);
        if failure == RecoveryFailure::Activated {
            // The post-activation error keeps the candidate, new generation,
            // pending publication fence, and retired old-answer evidence.
            assert_eq!(host.engine().generation().0, generation.0 + 1);
            assert!(matches!(host.engine().state(), LifecycleState::Ready { .. }));
            assert_ne!(native(&captured.path), main_native);
            assert_ne!(device.publication().unwrap().unwrap().identity, prior.identity);
            assert_eq!(
                device.publication().unwrap().unwrap().decision,
                PublicationDecision::Pending
            );
            host.review().flush_for_background().unwrap();
            assert_eq!(logs(&host.collection().unwrap()), expected_logs);
            let archives = recovery_archives(&captured.path);
            assert_eq!(archives.len(), 1);
            assert_eq!(fs::read(&archives[0]).unwrap(), old);
            // The live WAL belongs to the candidate; original WAL bytes must
            // remain fenced at R6.5's retired location, never on the candidate.
            let retired = fs::read_dir(captured.path.parent().unwrap())
                .unwrap()
                .map(Result::unwrap)
                .filter(|e| e.file_name().to_string_lossy().starts_with(".meguri-restore-wal-"))
                .collect::<Vec<_>>();
            assert_eq!(retired.len(), 1);
            assert_eq!(fs::read(retired[0].path()).unwrap(), old_wal.clone().unwrap());
            assert_ne!(fs::read(wal_path(&captured.path)).ok(), old_wal);
        } else {
            assert_eq!(host.engine().generation(), generation);
            assert!(matches!(
                host.engine().state(),
                LifecycleState::Failed {
                    reason: FailedReason::Corrupt { .. }
                }
            ));
            assert_eq!(&fs::read(journal_path(&captured.path)).unwrap(), expected_journal);
            if let Some((_, _, identity)) = &foreign {
                assert_eq!(native(&journal_path(&captured.path)), *identity);
            }
            assert_eq!(fs::read(&captured.path).unwrap(), damaged);
            assert_eq!(native(&captured.path), main_native);
            assert_eq!(fs::read(wal_path(&captured.path)).ok(), old_wal);
            assert_eq!(old_wal.as_ref().map(|_| native(&wal_path(&captured.path))), wal_native);
            assert_eq!(device.publication().unwrap().unwrap(), prior);
            assert!(meguri_storage::lockfile::CollectionLock::acquire(captured.path.parent().unwrap()).is_err());
            let archives = recovery_archives(&captured.path);
            assert_eq!(archives.len(), 1);
            assert_eq!(fs::read(&archives[0]).unwrap(), old);
            if failure == RecoveryFailure::MarkerPublication {
                assert!(
                    MARKER_FAILURE_ROOT.lock().unwrap().is_none(),
                    "marker fault was armed after archive rename"
                );
                assert_ne!(
                    native(&journal_path(&captured.path)),
                    journal_native,
                    "marker and rollback both published before reporting their post-rename errors"
                );
            }
        }
        host.close().unwrap();
        drop(host);
        let host = meguri_testkit::backup_support::open_restore_host(
            &captured.path,
            backups,
            clock.clone(),
            Arc::new(StatvfsSpace),
        )
        .unwrap();
        if failure != RecoveryFailure::Activated {
            // Closing/reopening the damaged owner must be no-write as well.
            assert_eq!(fs::read(&captured.path).unwrap(), damaged);
            assert_eq!(fs::read(wal_path(&captured.path)).ok(), old_wal);
            assert_eq!(&fs::read(journal_path(&captured.path)).unwrap(), expected_journal);
            if let Some((_, _, identity)) = &foreign {
                assert_eq!(native(&journal_path(&captured.path)), *identity);
            }
            assert_eq!(device.publication().unwrap().unwrap(), prior);
            let generation = host.engine().generation();
            clock.step_ms(1000);
            let restored = host
                .restore_backup(&backup.name, &Default::default(), &ProgressSink::new())
                .unwrap();
            assert_eq!(restored.generation.0, generation.0 + 1);
            assert!(restored.cloud_publication_pending);
            let safety = restored.safety_backup.as_ref().unwrap();
            assert_eq!(fs::read(backups.join(safety)).unwrap(), damaged);
            if let Some(wal) = &old_wal {
                assert_eq!(fs::read(backups.join(format!("{safety}-wal"))).unwrap(), *wal);
            }
        }
        assert!(matches!(host.engine().state(), LifecycleState::Ready { .. }));
        host.review().flush_for_background().unwrap();
        assert_eq!(
            logs(&host.collection().unwrap()),
            expected_logs,
            "old answers must not replay"
        );
        assert_eq!(
            device.publication().unwrap().unwrap().decision,
            PublicationDecision::Pending
        );
        let archives = recovery_archives(&captured.path);
        assert_eq!(archives.len(), 1);
        assert_eq!(fs::read(&archives[0]).unwrap(), old);
        host.close().unwrap();
        drop(host);
        let reopened =
            meguri_testkit::backup_support::open_restore_host(&captured.path, backups, clock, Arc::new(StatvfsSpace))
                .unwrap();
        assert!(matches!(reopened.engine().state(), LifecycleState::Ready { .. }));
        assert_eq!(logs(&reopened.collection().unwrap()), expected_logs);
        assert_eq!(fs::read(&archives[0]).unwrap(), old);
        assert_eq!(
            device.publication().unwrap().unwrap().decision,
            PublicationDecision::Pending
        );
        reopened.close().unwrap();
    }
}
