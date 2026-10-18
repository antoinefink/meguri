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
    common::add_note_with_card(&h.collection, OpSource::UserUndoable, &["before", "enable"]);
    assert!(h.collection.peek_undo(UndoScope::Global).unwrap().name().is_some());
    let before = common::steps::dump(&h.collection);
    let report = enable_fsrs(&h.collection, &RunOptions::default()).unwrap();
    assert_eq!((report.value.processed, report.value.changed), (3, 3));
    assert!(h.collection.peek_undo(UndoScope::Global).unwrap().name().is_none());
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

const KEPT_CONFIG: [&str; 7] = [
    "cardStateCustomizer",
    "fsrsLegacyEvaluate",
    "fsrsHealthCheck",
    "fsrsShortTermWithStepsEnabled",
    "newCardsIgnoreReviewLimit",
    "applyAllParentLimits",
    "futureSchedulingKey",
];

#[derive(Debug, PartialEq)]
struct CompatibilityDigest {
    config: Vec<Vec<meguri_storage::raw::SqlValue>>,
    unsupported_presets: Vec<Vec<meguri_storage::raw::SqlValue>>,
    unsupported_cards: Vec<Vec<meguri_storage::raw::SqlValue>>,
    normal_compat: Vec<Vec<u8>>,
    decks: Vec<Vec<meguri_storage::raw::SqlValue>>,
    revlog: Vec<Vec<meguri_storage::raw::SqlValue>>,
}
fn compatibility_digest(c: &Collection) -> CompatibilityDigest {
    use meguri_model::{ids::DeckConfigId, proto::ProtoBlob};
    use meguri_storage::raw::SqlValue;
    let dump = common::steps::dump(c);
    let id = |row: &Vec<SqlValue>| match row[0] {
        SqlValue::Integer(id) => id,
        _ => panic!("integer row id"),
    };
    let config = dump["config"]
        .iter()
        .filter(|row| matches!(&row[0],SqlValue::Text(key) if KEPT_CONFIG.contains(&key.as_str())))
        .cloned()
        .collect();
    let reader = c.reader(ReaderRole::Query).unwrap();
    let normal_compat = [1, 5]
        .into_iter()
        .map(|id| {
            let preset = reader.deck_config(DeckConfigId(id)).unwrap().unwrap();
            if id == 1 {
                return preset.config.to_bytes().into_owned();
            }
            let mut fields = (*preset.config).clone();
            // The existing preset-save leg below intentionally edits this one
            // field. Everything else, including exact opaque field bytes, stays.
            fields.reviews_per_day = 0;
            ProtoBlob::new(fields).to_bytes().into_owned()
        })
        .collect();
    CompatibilityDigest {
        config,
        unsupported_presets: dump["deck_config"]
            .iter()
            .filter(|row| (2..=4).contains(&id(row)))
            .cloned()
            .collect(),
        unsupported_cards: dump["cards"]
            .iter()
            .filter(|row| (101..=104).contains(&id(row)))
            .cloned()
            .collect(),
        normal_compat,
        decks: dump["decks"].clone(),
        revlog: dump["revlog"].clone(),
    }
}

#[test]
fn compatibility_data_and_unsupported_rows_survive_enable_recompute_preset_save_sync_and_reopen() {
    use meguri_engine::sched::{
        fsrs_policy::unsupported_home_presets,
        memory::{self, Target},
    };
    use meguri_model::{
        config::ConfigEntry,
        ids::{DeckConfigId, DeckId},
        proto::{ProtoBlob, deck::DeckKind},
        time::EpochSecs,
        usn::Usn,
    };
    let h = populate(4);
    h.collection
        .run(
            OpSpec::named("preservation_fixture", OpSource::Maintenance).stamp(StampMode::Raw),
            |ctx| {
                for (key, value) in [
                    ("cardStateCustomizer", r#""throw new Error('must never execute');""#),
                    ("fsrsLegacyEvaluate", " true "),
                    ("fsrsHealthCheck", " false "),
                    ("fsrsShortTermWithStepsEnabled", " true "),
                    ("newCardsIgnoreReviewLimit", " true "),
                    ("applyAllParentLimits", " true "),
                    ("futureSchedulingKey", r#"{ "future": [1, 2] }"#),
                ] {
                    ctx.scope().put_config_entry(&ConfigEntry {
                        key: key.into(),
                        usn: Usn(17),
                        mtime: EpochSecs(123),
                        value: value.as_bytes().to_vec(),
                    })?;
                }
                let base = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                for id in 1..=5 {
                    let mut preset = base.clone();
                    preset.id = DeckConfigId(id);
                    preset.name = format!("Preset {id}");
                    let mut fields = (*preset.config).clone();
                    fields.easy_days_percentages = vec![0.1, 0.2, 0.3, 0.4, 0.5, 0.6, 0.7];
                    fields.initial_ease = 3.1;
                    fields.easy_multiplier = 1.6;
                    fields.hard_multiplier = 1.3;
                    fields.lapse_multiplier = 0.2;
                    fields.interval_multiplier = 1.1;
                    fields.graduating_interval_good = 3;
                    fields.graduating_interval_easy = 7;
                    fields.maximum_review_interval = 12345;
                    fields.minimum_lapse_interval = 2;
                    preset.config = ProtoBlob::new(fields);
                    let mut legacy: serde_json::Value =
                        serde_json::from_str(&meguri_model::schema11::deck_config::deck_config_to_json(&preset))
                            .unwrap();
                    legacy["fsrsWeights"] = serde_json::json!(vec![1.0; 17]);
                    legacy["fsrsParams5"] = serde_json::json!(vec![1.0; 19]);
                    legacy["future"] = serde_json::json!({"bytes":"keep me"});
                    if id == 3 {
                        legacy["fsrsParams7"] = serde_json::json!([1]);
                    }
                    preset = meguri_model::schema11::deck_config::deck_config_from_json(&legacy.to_string()).unwrap();
                    if id == 2 {
                        preset.config.set_fsrs_params_6(vec![1.0; 34]);
                    }
                    let mut bytes = preset.config.to_bytes().into_owned();
                    if id == 4 {
                        bytes.extend_from_slice(&[0x3a, 0x01, 0x42]);
                    } // Unknown field7, nonempty.
                    if id == 1 || id == 5 {
                        bytes.extend_from_slice(&[0xa0, 0x1f, 0x81, 0x00]);
                    } // Field500; noncanonical varint preserved.
                    preset.config = ProtoBlob::decode(&bytes).unwrap();
                    ctx.scope().put(&preset)?;
                    let mut deck = if id == 1 {
                        ctx.scope().deck(DeckId(1))?.unwrap()
                    } else {
                        meguri_engine::decks::new_deck(format!("Deck {id}"))
                    };
                    deck.id = DeckId(id);
                    let mut kind = (*deck.kind).clone();
                    if let DeckKind::Normal(normal) = &mut kind.kind {
                        normal.config_id = id;
                        normal.extend_new = 23;
                        normal.extend_review = 41;
                    }
                    deck.kind = ProtoBlob::new(kind);
                    deck.common.learning_studied = 17;
                    ctx.scope().put(&deck)?;
                    if id <= 4 {
                        let mut card = ctx.scope().card(CardId(99 + id))?.unwrap();
                        card.deck_id = DeckId(id);
                        ctx.scope().put(&card)?;
                    }
                }
                let mut filtered = meguri_engine::decks::new_deck("Filtered".into());
                filtered.id = DeckId(6);
                // Seed a kept filtered definition, with nondefault preview settings.
                let mut kind = meguri_model::proto::deck::FilteredDeck::default();
                kind.reschedule = false;
                kind.preview_delay = 17;
                kind.delays = vec![3.0, 9.0];
                kind.preview_hard_secs = 23;
                kind.preview_good_secs = 29;
                kind.preview_again_secs = 31;
                kind.search_terms = vec![meguri_model::proto::deck::SearchTerm::new("is:due".into(), 37, 2)];
                filtered.kind = ProtoBlob::new(meguri_model::proto::deck::DeckKindMessage::new(DeckKind::Filtered(
                    kind,
                )));
                ctx.scope().put(&filtered)?;
                let mut card = ctx.scope().card(CardId(101))?.unwrap();
                card.id = CardId(104);
                card.deck_id = DeckId(6);
                card.original_deck_id = DeckId(2);
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    let before = compatibility_digest(&h.collection);
    assert_eq!(before.config.len(), KEPT_CONFIG.len());
    assert_eq!(before.unsupported_presets.len(), 3);
    assert_eq!(before.unsupported_cards.len(), 4);
    {
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(
            unsupported_home_presets(&*rows, &[DeckId(4), DeckId(3), DeckId(2), DeckId(1)]).unwrap(),
            [DeckConfigId(2), DeckConfigId(3), DeckConfigId(4)]
        );
        assert_eq!(
            unsupported_home_presets(&*rows, &[DeckId(6)]).unwrap(),
            [DeckConfigId(2)]
        );
        assert!(
            unsupported_home_presets(&*rows, &[DeckId(1), DeckId(5)])
                .unwrap()
                .is_empty()
        );
    }
    let report = enable_fsrs(&h.collection, &Default::default()).unwrap().value;
    assert_eq!(
        report.skipped_presets,
        [DeckConfigId(2), DeckConfigId(3), DeckConfigId(4)]
    );
    assert_eq!(report.changed, 1);
    assert_eq!(compatibility_digest(&h.collection), before);
    memory::recompute(
        &h.collection,
        Target::All,
        OpSource::Maintenance,
        "Memory",
        &Default::default(),
    )
    .unwrap();
    assert_eq!(compatibility_digest(&h.collection), before);
    // Real existing P10 preset primitive; the full U49 options save remains
    // its provider's later leg, as do U44 answers and U47 optimization.
    h.collection
        .run(OpSpec::named("save_other_preset", OpSource::UserUndoable), |ctx| {
            meguri_engine::deck_config::update_in(ctx.scope(), DeckConfigId(5), |fields| fields.reviews_per_day = 19)
        })
        .unwrap();
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_config(DeckConfigId(5))
            .unwrap()
            .unwrap()
            .config
            .reviews_per_day,
        19
    );
    assert_eq!(compatibility_digest(&h.collection), before);
    h.collection
        .run(OpSpec::named("sync_other_rows", OpSource::SyncIncoming), |ctx| {
            ctx.scope()
                .set_config("unrelatedSyncValue", &ConfigValue::Integer(42))?;
            let mut card = ctx.scope().card(CardId(100))?.unwrap();
            card.flags = 3;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    assert_eq!(compatibility_digest(&h.collection), before);
    h.engine.close().unwrap();
    let collection = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_eq!(compatibility_digest(&collection), before);
}

#[test]
fn missing_home_preset_uses_factory_values_without_changing_the_decks_assignment() {
    use meguri_model::{
        ids::{DeckConfigId, DeckId},
        proto::{ProtoBlob, deck::DeckKind},
    };
    let h = populate(2);
    h.collection
        .run(OpSpec::named("missing_preset_fixture", OpSource::Maintenance), |ctx| {
            let mut preset = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            preset.config.set_fsrs_params_6(vec![1.0; 34]);
            ctx.scope().put(&preset)?;
            let mut deck = meguri_engine::decks::new_deck("Missing preset".into());
            deck.id = DeckId(2);
            let mut kind = (*deck.kind).clone();
            if let DeckKind::Normal(normal) = &mut kind.kind {
                normal.config_id = 9999;
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
    let report = enable_fsrs(&h.collection, &Default::default()).unwrap().value;
    assert_eq!((report.processed, report.changed), (1, 1));
    assert_eq!(report.skipped_presets, [DeckConfigId(1)]);
    let after = common::steps::dump(&h.collection);
    assert_eq!(after["decks"], before["decks"]);
    assert_eq!(after["deck_config"], before["deck_config"]);
    assert_eq!(after["cards"][0], before["cards"][0]);
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(rows.deck_config(DeckConfigId(9999)).unwrap().is_none());
    let data = rows.card(CardId(101)).unwrap().unwrap().data;
    assert!(data.memory_state.is_some());
    assert_eq!(data.desired_retention, Some(0.9));
    assert_eq!(data.decay, Some(0.154));
}
