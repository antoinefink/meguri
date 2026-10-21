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
    clock::{StepClock, UtcOffset},
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
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let c = engine
        .open_raw(
            path,
            OpenConfig::new(
                Arc::new(StepClock::new(at, UtcOffset::UTC, "UTC")),
                Arc::new(SeededRandom::new(43)),
            ),
        )
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
        OpenConfig::new(
            Arc::new(StepClock::new(at, offset, "recorded-test")),
            Arc::new(SeededRandom::new(43)),
        )
        .ports(ports),
    );
    (engine, review, stream, result)
}
fn replacement_events(stream: &meguri_engine::Subscription) -> Vec<meguri_engine::Generation> {
    let mut result = Vec::new();
    while let Some(event) = stream.try_recv() {
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
