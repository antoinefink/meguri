mod common;
use common::Harness;
use meguri_engine::{
    Collection, EngineError, OpSpec, UndoScope,
    progress::{Progress, ProgressListener, ProgressPhase, ProgressSink},
    sched::fsrs_policy::{PolicyStatus, enable_fsrs, policy_status},
    tags::RunOptions,
};
use meguri_model::{
    card::{CardQueue, CardType},
    changes::OpSource,
    ids::{CardId, NoteId, RevlogId},
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, StampMode};
use std::{
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
    time::Duration,
};

fn set_fsrs(c: &Collection, value: bool, source: OpSource) {
    c.run(OpSpec::named("policy_test_sync", source), move |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(value))?;
        Ok(())
    })
    .unwrap();
}
fn config(c: &Collection, key: &str) -> Option<ConfigValue> {
    c.reader(ReaderRole::Query).unwrap().config_value(key).unwrap()
}
fn populate(count: usize) -> Harness {
    let h = common::fresh();
    h.collection
        .run(
            OpSpec::named("policy_fixture", OpSource::Maintenance).stamp(StampMode::Raw),
            move |ctx| {
                for n in 0..count {
                    let mut card = common::card(NoteId(1), 0);
                    card.id = CardId(100 + n as i64);
                    card.card_type = CardType::Review;
                    card.queue = CardQueue::Review;
                    card.interval = 20;
                    card.ease_factor = 2500;
                    card.due = 300;
                    card.data =
                        meguri_model::card_data::CardData::parse(r#"{"s":1,"d":5,"lrt":123,"cd":"{}","unknown":7}"#);
                    ctx.scope().put(&card)?;
                    let mut review = common::revlog(card.id);
                    review.id = RevlogId(1000 + n as i64);
                    review.interval = 100;
                    ctx.scope().put(&review)?;
                }
                Ok(())
            },
        )
        .unwrap();
    h
}
#[test]
fn fresh_policy_is_read_only_and_open_normalizes_only_scheduler() {
    let h = common::fresh();
    let before = common::steps::dump(&h.collection);
    assert_eq!(policy_status(&h.collection).unwrap(), PolicyStatus::NeedsEnable);
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(config(&h.collection, "sched2021"), Some(ConfigValue::Bool(true)));
    assert_eq!(config(&h.collection, "creationOffset"), Some(ConfigValue::from(0i64)));
    assert_eq!(config(&h.collection, "fsrs"), None);
}
#[test]
fn enable_preserves_schedule_and_revlog_and_repeat_preserves_all_bytes_and_undo() {
    let h = populate(3);
    let before = common::steps::dump(&h.collection);
    let report = enable_fsrs(&h.collection, &RunOptions::default()).unwrap();
    assert_eq!((report.value.processed, report.value.changed), (3, 3));
    assert_eq!(policy_status(&h.collection).unwrap(), PolicyStatus::Ok);
    let after = common::steps::dump(&h.collection);
    assert_eq!(after["revlog"], before["revlog"]);
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    for n in 0..3 {
        let card = rows.card(CardId(100 + n)).unwrap().unwrap();
        assert_eq!(
            (card.due, card.interval, card.queue, card.card_type),
            (300, 20, CardQueue::Review, CardType::Review)
        );
        assert!(card.data.memory_state.is_some());
        assert_eq!(card.data.last_review_secs, Some(123));
    }
    drop(rows);
    let added = common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["undo", "back"]);
    assert!(!added.changes.is_empty());
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let before_repeat = common::steps::dump(&h.collection);
    assert_eq!(
        enable_fsrs(&h.collection, &RunOptions::default())
            .unwrap()
            .value
            .changed,
        0
    );
    assert_eq!(common::steps::dump(&h.collection), before_repeat);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
}
#[test]
fn seen_on_is_durable_device_state_and_sync_off_does_not_normalize_scheduler() {
    let h = common::fresh();
    set_fsrs(&h.collection, true, OpSource::SyncIncoming);
    set_fsrs(&h.collection, false, OpSource::SyncIncoming);
    h.collection
        .run(OpSpec::named("sync_scheduler", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("sched2021", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(config(&h.collection, "sched2021"), Some(ConfigValue::Bool(false)));
    assert_eq!(policy_status(&h.collection).unwrap(), PolicyStatus::NeedsReEnable);
    h.engine.close().unwrap();
    let collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(policy_status(&collection).unwrap(), PolicyStatus::NeedsReEnable);
    assert_eq!(config(&collection, "sched2021"), Some(ConfigValue::Bool(true)));
    assert_eq!(
        common::query_int(&collection, "SELECT count(*) FROM config WHERE key LIKE '%seen%'"),
        0
    );
}
struct CancelBatch(Arc<meguri_engine::CancelToken>);
impl ProgressListener for CancelBatch {
    fn on_progress(&self, p: &Progress) {
        if p.phase == ProgressPhase::Named("Recomputing memory") && p.completed >= 256 {
            self.0.cancel();
        }
    }
}
fn ticks() -> Duration {
    static N: AtomicU64 = AtomicU64::new(0);
    Duration::from_secs(N.fetch_add(1, Ordering::Relaxed))
}
#[test]
fn cancellation_after_card_writes_rolls_back_flag_cards_and_undo() {
    let h = populate(600);
    common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["keep", "undo"]);
    let before = common::steps::dump(&h.collection);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    let progress = ProgressSink::with_clock(ticks);
    progress.set_listener(Arc::new(CancelBatch(cancel.clone())));
    assert_eq!(
        enable_fsrs(
            &h.collection,
            &RunOptions {
                cancel: Some(cancel),
                progress: Some(progress)
            }
        )
        .unwrap_err(),
        EngineError::Cancelled
    );
    assert_eq!(common::steps::dump(&h.collection), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(policy_status(&h.collection).unwrap(), PolicyStatus::NeedsEnable);
}

#[test]
fn scheduler_open_handles_strict_bool_types_without_creating_legacy_offset() {
    use meguri_storage::raw::SqlValue;
    for value in [
        SqlValue::Blob(b"false".to_vec()),
        SqlValue::Blob(b"1".to_vec()),
        SqlValue::Blob(br#""true""#.to_vec()),
        SqlValue::Text("true".into()),
        SqlValue::Blob(b"null".to_vec()),
        SqlValue::Blob(b"not json".to_vec()),
    ] {
        let h = common::fresh();
        h.engine.close().unwrap();
        common::day::raw(
            &h.path,
            h.clock.as_ref(),
            &[
                ("DELETE FROM config WHERE key IN ('creationOffset','sched2021')", vec![]),
                (
                    "INSERT INTO config(key,usn,mtime_secs,val) VALUES ('sched2021',0,1,?1)",
                    vec![value.clone()],
                ),
                (
                    "INSERT OR REPLACE INTO config(key,usn,mtime_secs,val) VALUES ('fsrs',0,1,?1)",
                    vec![value],
                ),
            ],
        );
        let c = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
        assert_eq!(config(&c, "sched2021"), Some(ConfigValue::Bool(true)));
        assert_eq!(config(&c, "creationOffset"), None);
        assert_eq!(policy_status(&c).unwrap(), PolicyStatus::NeedsEnable);
        enable_fsrs(&c, &Default::default()).unwrap();
        let row = c
            .reader(ReaderRole::Query)
            .unwrap()
            .config_entry("fsrs")
            .unwrap()
            .unwrap();
        assert!(row.is_blob);
        assert_eq!(row.entry.value, b"true");
    }
}

#[test]
fn seen_mark_failure_does_not_turn_a_committed_enable_into_an_error_and_retries() {
    let h = common::fresh();
    let marker = h.path.with_file_name("collection.anki2.meguri-fsrs-seen");
    std::fs::create_dir(&marker).unwrap(); // Publishing the device mark will fail.
    enable_fsrs(&h.collection, &Default::default()).unwrap();
    set_fsrs(&h.collection, false, OpSource::SyncIncoming);
    assert_eq!(policy_status(&h.collection).unwrap(), PolicyStatus::NeedsReEnable);
    std::fs::remove_dir(&marker).unwrap();
    set_fsrs(&h.collection, true, OpSource::SyncIncoming); // Next S2 check retries.
    assert_eq!(std::fs::read(&marker).unwrap(), b"1\n");
    set_fsrs(&h.collection, false, OpSource::SyncIncoming);
    h.engine.close().unwrap();
    let c = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(policy_status(&c).unwrap(), PolicyStatus::NeedsReEnable);
}

#[test]
fn replacement_marks_seen_on_but_defers_scheduler_switch_until_open() {
    let h = common::fresh();
    let other = common::fresh();
    set_fsrs(&other.collection, true, OpSource::SyncIncoming);
    other
        .collection
        .run(OpSpec::named("replacement_v2", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("sched2021", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
    other.engine.close().unwrap();
    let source = other.path.clone();
    let slot = h.engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(false).unwrap();
    exclusive
        .replace(move |live| {
            std::fs::copy(&source, live)
                .map(|_| ())
                .map_err(|error| EngineError::Internal {
                    message: error.to_string(),
                })
        })
        .unwrap();
    drop(exclusive);
    drop(slot);
    let c = h.engine.collection().unwrap();
    assert_eq!(policy_status(&c).unwrap(), PolicyStatus::Ok);
    assert_eq!(config(&c, "sched2021"), Some(ConfigValue::Bool(false)));
    set_fsrs(&c, false, OpSource::SyncIncoming);
    assert_eq!(policy_status(&c).unwrap(), PolicyStatus::NeedsReEnable);
    h.engine.close().unwrap();
    let c = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(config(&c, "sched2021"), Some(ConfigValue::Bool(true)));
    assert_eq!(policy_status(&c).unwrap(), PolicyStatus::NeedsReEnable);
}

#[test]
fn compatibility_data_and_unsupported_rows_survive_enable_recompute_sync_and_reopen() {
    use meguri_engine::sched::{
        fsrs_policy::unsupported_home_presets,
        memory::{self, Target},
    };
    use meguri_model::{
        ids::{DeckConfigId, DeckId},
        proto::{ProtoBlob, deck::DeckKind},
    };
    let h = populate(2);
    h.collection
        .run(OpSpec::named("preservation_fixture", OpSource::Maintenance), |ctx| {
            for key in [
                "cardStateCustomizer",
                "fsrsLegacyEvaluate",
                "fsrsHealthCheck",
                "fsrsShortTermWithStepsEnabled",
                "futureSchedulingKey",
            ] {
                ctx.scope().set_config(
                    key,
                    &ConfigValue::String("throw new Error('must never execute');".into()),
                )?;
            }
            let mut future = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            let mut supported = future.clone();
            supported.id = DeckConfigId(2);
            supported.name = "Supported".into();
            ctx.scope().put(&supported)?;
            let mut fields = (*future.config).clone();
            fields.set_fsrs_params_6(vec![1.0; 34]);
            fields.easy_days_percentages = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
            fields.initial_ease = 3.1;
            future.config = ProtoBlob::new(fields);
            let mut legacy: serde_json::Value =
                serde_json::from_str(&meguri_model::schema11::deck_config::deck_config_to_json(&future)).unwrap();
            legacy["fsrsParams7"] = serde_json::json!([1]);
            legacy["future"] = serde_json::json!({"bytes":"keep me"});
            future = meguri_model::schema11::deck_config::deck_config_from_json(&legacy.to_string()).unwrap();
            ctx.scope().put(&future)?;
            let mut deck = meguri_engine::decks::new_deck("Supported".into());
            deck.id = DeckId(2);
            let mut kind = (*deck.kind).clone();
            if let DeckKind::Normal(normal) = &mut kind.kind {
                normal.config_id = 2;
            }
            deck.kind = ProtoBlob::new(kind);
            ctx.scope().put(&deck)?;
            let mut card = ctx.scope().card(CardId(101))?.unwrap();
            card.deck_id = DeckId(2);
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    let before = common::steps::dump(&h.collection);
    {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            unsupported_home_presets(&*reader, &[DeckId(2), DeckId(1)]).unwrap(),
            [DeckConfigId(1)]
        );
    }
    let preserved = || {
        let after = common::steps::dump(&h.collection);
        assert_eq!(after["deck_config"], before["deck_config"]);
        assert_eq!(after["decks"], before["decks"]);
        assert_eq!(after["revlog"], before["revlog"]);
        assert_eq!(after["cards"][0], before["cards"][0]);
        let keys = [
            "cardStateCustomizer",
            "fsrsLegacyEvaluate",
            "fsrsHealthCheck",
            "fsrsShortTermWithStepsEnabled",
            "futureSchedulingKey",
        ];
        for key in keys {
            assert_eq!(
                config(&h.collection, key),
                Some(ConfigValue::String("throw new Error('must never execute');".into()))
            );
        }
    };
    let report = enable_fsrs(&h.collection, &Default::default()).unwrap().value;
    assert_eq!(report.skipped_presets, [DeckConfigId(1)]);
    assert_eq!(report.changed, 1);
    preserved();
    memory::recompute(
        &h.collection,
        Target::All,
        OpSource::Maintenance,
        "Memory",
        &Default::default(),
    )
    .unwrap();
    preserved();
    h.collection
        .run(OpSpec::named("sync_other_config", OpSource::SyncIncoming), |ctx| {
            ctx.scope()
                .set_config("unrelatedSyncValue", &ConfigValue::Integer(42))?;
            Ok(())
        })
        .unwrap();
    preserved();
    h.engine.close().unwrap();
    let collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    let after = common::steps::dump(&collection);
    assert_eq!(after["deck_config"], before["deck_config"]);
    assert_eq!(after["decks"], before["decks"]);
    assert_eq!(after["revlog"], before["revlog"]);
    assert_eq!(after["cards"][0], before["cards"][0]);
    assert_eq!(
        config(&collection, "cardStateCustomizer"),
        Some(ConfigValue::String("throw new Error('must never execute');".into()))
    );
}
