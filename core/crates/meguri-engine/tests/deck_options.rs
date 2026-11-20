//! U49 transactions on real storage and the real scheduler memory path.
mod common;
use meguri_engine::{
    EngineError, OpSource, OpSpec, UndoScope, deck_config, decks, sched::deck_options as options, tags::RunOptions,
};
use meguri_model::{
    card::{CardQueue, CardType},
    config::ConfigEntry,
    ids::{CardId, DeckConfigId, DeckId, NoteId, RevlogId},
    proto::deck::{DayLimit, DeckKind},
    time::EpochSecs,
    usn::Usn,
};
use meguri_sched::options::{Field, Value};
use meguri_storage::{ReadRows, ReaderRole, StampMode, StoredConfig};
fn draft(h: &common::Harness, deck: DeckId) -> options::Draft {
    options::Draft::new(options::snapshot(&h.collection, deck).unwrap())
}
fn row(h: &common::Harness, id: DeckId) -> meguri_model::deck::Deck {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck(id)
        .unwrap()
        .unwrap()
}
fn dump(h: &common::Harness) -> common::steps::Dump {
    common::steps::dump(&h.collection)
}
#[test]
fn staged_add_clone_rename_and_save_are_one_step_with_exact_undo_and_redo() {
    let h = common::fresh();
    let mut d = draft(&h, DeckId(1));
    let before = dump(&h);
    d.edit(Some(1), Field::LearnSteps, Value::Text("2m 20m".into()))
        .unwrap();
    let clone = d.add("Clone", true, h.clock.as_ref()).unwrap().unwrap();
    assert_eq!(
        d.preset(clone).unwrap().values[&Field::LearnSteps],
        Value::Steps(vec![2., 20.])
    );
    d.rename("Clone edited", h.clock.as_ref()).unwrap();
    assert_eq!(d.use_count(clone).unwrap(), 0);
    assert!(d.has_changes());
    assert_eq!(dump(&h), before);
    h.clock.step_ms(60000);
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert_eq!(saved.undo.as_ref().unwrap().name, "Deck Options");
    let after = dump(&h);
    assert_ne!(before, after);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), after);
}
#[test]
fn unchanged_invalid_raw_fields_stay_raw_even_when_a_visible_field_changes() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("import", OpSource::Maintenance), |ctx| {
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.config.desired_retention = f32::NAN;
            p.config.initial_ease = f32::NAN;
            p.config.maximum_review_interval = 0;
            p.config.cap_answer_time_to_secs = 8000;
            ctx.scope().put(&p)?;
            Ok(())
        })
        .unwrap();
    let mut d = draft(&h, DeckId(1));
    assert_eq!(
        d.presets[0].values[&Field::DesiredRetention],
        Value::Number(f64::from(0.9f32))
    );
    let before = dump(&h);
    assert!(
        options::save(&h.collection, d.clone(), &Default::default())
            .unwrap()
            .changes
            .deck_configs
            .is_empty()
    );
    assert_eq!(dump(&h), before);
    d = draft(&h, DeckId(1));
    d.edit(Some(1), Field::BuryNew, Value::Boolean(true)).unwrap();
    options::save(&h.collection, d, &Default::default()).unwrap();
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    let p = rows.deck_config(DeckConfigId(1)).unwrap().unwrap();
    assert!(p.config.desired_retention.is_nan() && p.config.initial_ease.is_nan());
    assert_eq!(p.config.maximum_review_interval, 0);
    assert_eq!(p.config.cap_answer_time_to_secs, 8000);
}
#[test]
fn validation_stale_default_and_schema_refusals_leave_no_partial_effects() {
    let h = common::fresh();
    let mut d = draft(&h, DeckId(1));
    let before = dump(&h);
    d.edit(Some(1), Field::DesiredRetention, Value::Number(0.68)).unwrap();
    assert!(matches!(
        options::save(&h.collection, d, &Default::default()),
        Err(EngineError::DeckOptionsInvalid(_))
    ));
    assert_eq!(dump(&h), before);
    let mut d = draft(&h, DeckId(1));
    d.removals.insert(1);
    assert!(matches!(
        options::save(&h.collection, d, &Default::default()),
        Err(EngineError::DeckRefused(decks::Refusal::DefaultPreset))
    ));
    assert_eq!(dump(&h), before);
    let d = draft(&h, DeckId(1));
    deck_config::rename(&h.collection, DeckConfigId(1), "changed concurrently".into()).unwrap();
    let before = dump(&h);
    assert!(matches!(
        options::save(&h.collection, d, &Default::default()),
        Err(EngineError::Stale { .. })
    ));
    assert_eq!(dump(&h), before);
    let preset = deck_config::add(&h.collection, "Remove".into()).unwrap().value;
    h.collection
        .run(OpSpec::named("synced", OpSource::Maintenance), |ctx| {
            let mut col = ctx.scope().col()?;
            col.last_sync = col.schema_modified;
            ctx.scope().put_col(&col)?;
            Ok(())
        })
        .unwrap();
    let mut d = draft(&h, DeckId(1));
    d.remove(preset.0).unwrap();
    options::save(&h.collection, d, &Default::default()).unwrap();
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_config(preset)
            .unwrap()
            .is_none()
    );
}
#[test]
fn subtree_removed_and_missing_presets_receive_the_targets_five_values() {
    let h = common::fresh();
    let parent = decks::add(&h.collection, "Parent".into()).unwrap().value.id;
    let child = decks::add(&h.collection, "Parent::Child".into()).unwrap().value.id;
    let orphan = decks::add(&h.collection, "Orphan".into()).unwrap().value.id;
    let removed = deck_config::add(&h.collection, "Remove".into()).unwrap().value;
    deck_config::assign(&h.collection, orphan, removed, false).unwrap();
    let mut d = draft(&h, parent);
    d.subtree = true;
    d.remove(removed.0).unwrap();
    d.edit(None, Field::NewLimit, Value::Number(17.)).unwrap();
    d.edit(None, Field::ReviewLimitToday, Value::Number(52.)).unwrap();
    d.edit(None, Field::DeckDesiredRetention, Value::Number(0.85)).unwrap();
    let before = dump(&h);
    options::save(&h.collection, d, &Default::default()).unwrap();
    for id in [parent, child, orphan] {
        let deck = row(&h, id);
        let DeckKind::Normal(n) = &deck.kind.kind else { panic!() };
        assert_eq!(n.config_id, 1);
        assert_eq!(n.new_limit, Some(17));
        assert_eq!(n.review_limit_today.as_ref().unwrap().limit, 52);
        assert_eq!(n.desired_retention, Some(0.85));
    }
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .deck_config(removed)
            .unwrap()
            .is_none()
    );
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
    // A missing previous preset is reassigned, without applying step remap.
    h.collection
        .run(OpSpec::named("missing", OpSource::Maintenance), move |ctx| {
            let mut deck = ctx.scope().deck(orphan)?.unwrap();
            if let DeckKind::Normal(n) = &mut deck.kind.kind {
                n.config_id = 999;
            }
            ctx.scope().put(&deck)?;
            Ok(())
        })
        .unwrap();
    options::save(&h.collection, draft(&h, parent), &Default::default()).unwrap();
    assert!(matches!(&row(&h,orphan).kind.kind,DeckKind::Normal(n) if n.config_id==1));
}
#[test]
fn today_only_clear_on_day_zero_and_later_preserves_limit_then_falls_back() {
    for days in [0, 2] {
        let h = common::fresh();
        h.clock.step_ms(days * 86400000);
        let today = h.collection.today().unwrap();
        h.collection
            .run(OpSpec::named("limit", OpSource::Maintenance), move |ctx| {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                if let DeckKind::Normal(n) = &mut deck.kind.kind {
                    n.new_limit = Some(7);
                    n.new_limit_today = Some(DayLimit::new(50, today));
                }
                ctx.scope().put(&deck)?;
                Ok(())
            })
            .unwrap();
        let mut d = draft(&h, DeckId(1));
        d.edit(None, Field::NewLimitToday, Value::None).unwrap();
        options::save(&h.collection, d, &Default::default()).unwrap();
        let deck = row(&h, DeckId(1));
        let DeckKind::Normal(n) = &deck.kind.kind else { panic!() };
        if today == 0 {
            assert!(n.new_limit_today.is_none());
        } else {
            let limit = n.new_limit_today.as_ref().unwrap();
            assert_eq!((limit.limit, limit.today), (50, today - 1));
        }
        assert_eq!(
            meguri_sched::model::deck_sched::DeckSched::new(Some(n), &deck.common).new_limit(today, 20),
            Some(7)
        );
    }
}
#[test]
fn remap_and_memory_refresh_keep_due_and_intervals_and_undo_every_row() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("cards", OpSource::Maintenance), |ctx| {
            for (id, left) in [(100, 3), (101, 1002), (102, 0)] {
                let mut c = common::card(NoteId(id), 0);
                c.id = CardId(id);
                c.card_type = CardType::Learning;
                c.queue = CardQueue::Suspended;
                c.remaining_steps = left;
                c.due = 73;
                c.interval = 20;
                c.ease_factor = 2500;
                ctx.scope().put(&c)?;
                let mut log = common::revlog(c.id);
                log.id = RevlogId(1000 + id);
                ctx.scope().put(&log)?;
            }
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.config.learn_steps = vec![1., 10., 60.];
            ctx.scope().put(&p)?;
            Ok(())
        })
        .unwrap();
    let mut d = draft(&h, DeckId(1));
    d.edit(Some(1), Field::LearnSteps, Value::Text("5m 2h".into())).unwrap();
    d.edit(Some(1), Field::DesiredRetention, Value::Number(0.85)).unwrap();
    let before = dump(&h);
    options::save(&h.collection, d, &Default::default()).unwrap();
    {
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        for (id, left) in [(100, 2), (101, 1002), (102, 1)] {
            let c = rows.card(CardId(id)).unwrap().unwrap();
            assert_eq!((c.remaining_steps, c.due, c.interval), (left, 73, 20));
            assert_eq!(c.data.desired_retention, Some(0.85));
        }
    }
    let after = dump(&h);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), after);
}
#[test]
fn insertion_order_is_a_note_permutation_then_sequential_and_never_moves_nextpos() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("cards", OpSource::Maintenance), |ctx| {
            for id in 100..110 {
                let mut c = common::card(NoteId(id / 2), 0);
                c.id = CardId(id);
                c.due = 500 - id as i32;
                c.queue = CardQueue::Suspended;
                ctx.scope().put(&c)?;
            }
            Ok(())
        })
        .unwrap();
    let next = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .config_entry("nextPos")
        .unwrap();
    for order in [1., 0.] {
        let mut d = draft(&h, DeckId(1));
        d.edit(Some(1), Field::NewCardInsertOrder, Value::Number(order))
            .unwrap();
        options::save(&h.collection, d, &Default::default()).unwrap();
        let rows = h.collection.reader(ReaderRole::Query).unwrap();
        let mut cards = rows.cards_of_deck(DeckId(1)).unwrap();
        cards.sort_by_key(|c| c.id);
        let positions = cards.iter().map(|c| c.due).collect::<std::collections::BTreeSet<_>>();
        assert_eq!(positions, (1..=5).collect());
        for pair in cards.chunks(2) {
            assert_eq!(pair[0].due, pair[1].due);
        }
        if order == 0. {
            for c in &cards {
                assert_eq!(c.due, (c.note_id.0 - 49) as i32);
            }
        }
        assert_eq!(rows.config_entry("nextPos").unwrap(), next);
    }
}
#[test]
fn cancelled_write_and_failed_extension_roll_back_presets_decks_cards_and_history() {
    let h = common::fresh();
    let mut d = draft(&h, DeckId(1));
    d.add("Staged", false, h.clock.as_ref()).unwrap();
    d.edit(None, Field::NewLimit, Value::Number(5.)).unwrap();
    let before = dump(&h);
    let cancel = meguri_engine::CancelToken::new();
    let cancel_in = cancel.clone();
    let error = options::save_with(
        &h.collection,
        d.clone(),
        &RunOptions {
            cancel: Some(cancel),
            progress: None,
        },
        move |_| {
            cancel_in.cancel();
            Ok(())
        },
    )
    .unwrap_err();
    assert_eq!(error, EngineError::Cancelled);
    assert_eq!(dump(&h), before);
    assert!(
        options::save_with(&h.collection, d, &Default::default(), |_| Err(
            EngineError::InvalidArgument {
                message: "injected after preset write".into()
            }
        ))
        .is_err()
    );
    assert_eq!(dump(&h), before);
}
#[test]
fn preferences_validate_supplied_fields_and_reject_empty_requests_without_mutation() {
    let h = common::fresh();
    let before = dump(&h);
    let bad = options::PreferencesChange {
        rollover: Some(24.),
        learn_ahead_minutes: Some(0.5),
        load_balancer: Some(false),
    };
    assert!(matches!(
        options::save_preferences(&h.collection, bad, &Default::default()),
        Err(EngineError::DeckOptionsInvalid(_))
    ));
    assert_eq!(dump(&h), before);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    assert!(matches!(
        options::save_preferences(&h.collection, Default::default(), &Default::default()),
        Err(EngineError::InvalidArgument { .. })
    ));
    assert_eq!(dump(&h), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
}
fn preference_row(h: &common::Harness, key: &str) -> Option<StoredConfig> {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .config_entry(key)
        .unwrap()
}
fn preferences_without_rows() -> common::Harness {
    let h = common::fresh();
    h.collection
        .run(
            OpSpec::named("preferences absent setup", OpSource::Maintenance),
            |ctx| {
                for key in ["rollover", "collapseTime", "loadBalancerEnabled"] {
                    ctx.scope().remove_config(key)?;
                }
                Ok(())
            },
        )
        .unwrap();
    h
}
#[test]
fn rollover_only_preferences_materialize_defaults_and_undo_restores_each_previous_state() {
    let h = preferences_without_rows();
    for key in ["rollover", "collapseTime", "loadBalancerEnabled"] {
        assert_eq!(preference_row(&h, key), None);
    }
    let before = dump(&h);
    let saved = options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            rollover: Some(6.),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(saved.undo.as_ref().unwrap().name, "Preferences");
    assert_eq!(preference_row(&h, "collapseTime").unwrap().entry.value, b"1200");
    assert_eq!(preference_row(&h, "loadBalancerEnabled").unwrap().entry.value, b"true");
    assert_eq!(preference_row(&h, "rollover").unwrap().entry.value, b"6");
    let materialized = dump(&h);
    h.clock.step_ms(60000);
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            load_balancer: Some(false),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(preference_row(&h, "loadBalancerEnabled").unwrap().entry.value, b"false");
    let disabled = dump(&h);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), materialized);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), materialized);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), disabled);
}
#[test]
fn preferences_keep_omitted_seconds_exact_and_convert_explicit_minutes() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("learn ahead setup", OpSource::Maintenance), |ctx| {
            ctx.scope()
                .set_config("collapseTime", &meguri_storage::ConfigValue::from(333u64))?;
            Ok(())
        })
        .unwrap();
    let seconds = preference_row(&h, "collapseTime");
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            load_balancer: Some(false),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(preference_row(&h, "collapseTime"), seconds);
    assert_eq!(options::preferences(&h.collection).unwrap().learn_ahead_minutes, 5);
    for (minutes, expected) in [(0., b"0".as_slice()), (33., b"1980"), (999., b"59940")] {
        let before = dump(&h);
        options::save_preferences(
            &h.collection,
            options::PreferencesChange {
                learn_ahead_minutes: Some(minutes),
                ..Default::default()
            },
            &Default::default(),
        )
        .unwrap();
        assert_eq!(preference_row(&h, "collapseTime").unwrap().entry.value, expected);
        assert_eq!(preference_row(&h, "loadBalancerEnabled").unwrap().entry.value, b"false");
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(dump(&h), before);
    }
}
#[test]
fn preferences_equal_stored_rows_keep_raw_bytes_stamps_and_missing_rollover() {
    let h = common::fresh();
    h.collection
        .run(
            OpSpec::named("preferences raw setup", OpSource::Maintenance).stamp(StampMode::Raw),
            |ctx| {
                for (key, value) in [("collapseTime", " 1200 \n"), ("loadBalancerEnabled", " true \n")] {
                    ctx.scope().put_config_entry(&ConfigEntry {
                        key: key.into(),
                        usn: Usn(17),
                        mtime: EpochSecs(123),
                        value: value.as_bytes().to_vec(),
                    })?;
                }
                Ok(())
            },
        )
        .unwrap();
    let before = dump(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    h.clock.step_ms(60000);
    for change in [
        options::PreferencesChange {
            rollover: Some(4.),
            ..Default::default()
        },
        options::PreferencesChange {
            learn_ahead_minutes: Some(20.),
            load_balancer: Some(true),
            ..Default::default()
        },
    ] {
        let result = options::save_preferences(&h.collection, change, &Default::default()).unwrap();
        assert!(result.undo.is_none());
        assert_eq!(dump(&h), before);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        assert_eq!(preference_row(&h, "rollover"), None);
    }
}
#[test]
fn preferences_explicit_defaults_insert_missing_rows_without_materializing_rollover() {
    let h = preferences_without_rows();
    let before = dump(&h);
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            rollover: Some(4.),
            learn_ahead_minutes: Some(20.),
            load_balancer: Some(true),
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(preference_row(&h, "collapseTime").unwrap().entry.value, b"1200");
    assert_eq!(preference_row(&h, "loadBalancerEnabled").unwrap().entry.value, b"true");
    assert_eq!(preference_row(&h, "rollover"), None);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
}
#[test]
fn rollover_preferences_recompute_day_immediately_and_forward_day_unburies() {
    let h = common::fresh();
    // Day 2, 05:00 UTC. At rollover 6 the current scheduler day is 1.
    h.clock.step_ms(2 * 86400000 - 5 * 3600000);
    assert_eq!(h.collection.today().unwrap(), 2);
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            rollover: Some(6.),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(h.collection.today().unwrap(), 1);
    h.collection
        .run(OpSpec::named("buried", OpSource::Maintenance), |ctx| {
            let mut card = common::card(NoteId(1), 0);
            card.id = CardId(100);
            card.card_type = CardType::Review;
            card.queue = CardQueue::BuriedByUser;
            card.due = 2;
            card.interval = 10;
            ctx.scope().put(&card)?;
            ctx.scope()
                .set_config("lastUnburied", &meguri_storage::ConfigValue::from(1u64))?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(CardId(100))
            .unwrap()
            .unwrap()
            .queue,
        CardQueue::BuriedByUser
    );
    let next = h.collection.next_day_at().unwrap();
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            rollover: Some(4.),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(h.collection.today().unwrap(), 2);
    assert!(h.collection.next_day_at().unwrap() > next);
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(rows.card(CardId(100)).unwrap().unwrap().queue, CardQueue::Review);
    assert_eq!(
        rows.config_value("lastUnburied").unwrap().and_then(|v| v.as_u64()),
        Some(2)
    );
}
#[test]
fn snapshot_order_use_counts_staging_removal_and_ignore_count_are_real_reads() {
    let h = common::fresh();
    let z = deck_config::add(&h.collection, "z".into()).unwrap().value;
    let a = deck_config::add(&h.collection, "a".into()).unwrap().value;
    let other = decks::add(&h.collection, "Other".into()).unwrap().value.id;
    deck_config::assign(&h.collection, other, z, false).unwrap();
    let mut d = draft(&h, DeckId(1));
    assert_eq!(
        d.presets.iter().map(|p| p.name.as_str()).collect::<Vec<_>>(),
        ["a", "Default", "z"]
    );
    d.select(z.0).unwrap();
    assert_eq!(d.use_count(z.0).unwrap(), 2);
    assert_eq!(d.use_count(1).unwrap(), 0);
    d.subtree = true;
    assert_eq!(d.use_count(z.0).unwrap(), 2);
    d.remove(z.0).unwrap();
    assert_eq!(d.selected, 1);
    assert!(d.removals.contains(&z.0));
    let new = d.add(" Fresh ", false, h.clock.as_ref()).unwrap().unwrap();
    assert_eq!(d.preset(new).unwrap().name, "Fresh");
    d.remove(new).unwrap();
    assert!(!d.removals.contains(&new));
    let count = options::ignored_before_count(&h.collection, &d, a.0, "2030-01-01").unwrap();
    assert_eq!((count.included, count.total), (0, 0));
    d.edit(Some(1), Field::LearnSteps, Value::Text("broken".into()))
        .unwrap();
    let cloned = d.add("Invalid edit copied", true, h.clock.as_ref()).unwrap().unwrap();
    assert_eq!(
        d.preset(cloned).unwrap().values[&Field::LearnSteps],
        Value::Text("broken".into())
    );
}
#[test]
fn ignore_count_with_invalid_step_edits_keeps_stored_rows_and_staged_values() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("count setup", OpSource::Maintenance), |ctx| {
            for id in [100, 101] {
                let mut card = common::card(NoteId(id), 0);
                card.id = CardId(id);
                card.card_type = CardType::Review;
                card.queue = CardQueue::Review;
                ctx.scope().put(&card)?;
            }
            let mut log = common::revlog(CardId(100));
            log.id = RevlogId(common::NOW_MS);
            log.kind = meguri_model::revlog::RevlogKind::Learning;
            ctx.scope().put(&log)?;
            Ok(())
        })
        .unwrap();
    let mut d = draft(&h, DeckId(1));
    d.edit(Some(1), Field::LearnSteps, Value::Text("broken".into()))
        .unwrap();
    d.edit(Some(1), Field::RelearnSteps, Value::Text("also broken".into()))
        .unwrap();
    let clone = d.add("Invalid clone", true, h.clock.as_ref()).unwrap().unwrap();
    let staged = format!("{d:?}");
    let before = dump(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    for (key, expected) in [(1, (1, 2)), (clone, (0, 0))] {
        let count = options::ignored_before_count(&h.collection, &d, key, "2030-01-01").unwrap();
        assert_eq!((count.included, count.total), expected);
    }
    assert_eq!(format!("{d:?}"), staged);
    assert_eq!(dump(&h), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert!(
        d.validate("2030-01-15")
            .iter()
            .any(|issue| issue.field == Field::LearnSteps)
    );
}
#[test]
fn selecting_limit_and_retention_tabs_clears_only_more_specific_draft_values() {
    let h = common::fresh();
    let mut d = draft(&h, DeckId(1));
    d.set_limit(false, options::LimitScope::TodayOnly, 50.).unwrap();
    assert_eq!(d.limit_scope(false), options::LimitScope::TodayOnly);
    d.set_limit(false, options::LimitScope::ThisDeck, 7.).unwrap();
    assert_eq!(d.deck_values[&Field::NewLimitToday], Value::None);
    assert_eq!(d.limit_scope(false), options::LimitScope::ThisDeck);
    d.set_limit(false, options::LimitScope::Preset, 30.).unwrap();
    assert_eq!(d.deck_values[&Field::NewLimit], Value::None);
    assert_eq!(d.limit_scope(false), options::LimitScope::Preset);
    d.set_retention(true, 0.85).unwrap();
    d.set_retention(false, 0.92).unwrap();
    assert_eq!(d.deck_values[&Field::DeckDesiredRetention], Value::None);
    assert_eq!(d.presets[0].values[&Field::DesiredRetention], Value::Number(0.92));
}

#[test]
fn removing_a_preset_with_scm_at_ls_commits_without_confirmation() {
    let h = common::fresh();
    let preset = deck_config::add(&h.collection, "Remove".into()).unwrap().value;
    h.collection
        .run(OpSpec::named("synced", OpSource::Maintenance), |ctx| {
            let mut col = ctx.scope().col()?;
            col.last_sync = col.schema_modified;
            ctx.scope().put_col(&col)?;
            Ok(())
        })
        .unwrap();
    let before = dump(&h);
    let mut d = draft(&h, DeckId(1));
    h.clock.step_ms(1_000);
    d.remove(preset.0).unwrap();
    assert_eq!(d.schema_reasons(), [meguri_engine::SchemaReason::PresetRemoved]);
    options::save(&h.collection, d, &Default::default()).unwrap();
    let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert!(col.schema_modified > col.last_sync);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
}

// O17: the first version's page edits three preset values and keeps the deck's.
const DECK_VALUES: [Field; 5] = [
    Field::NewLimit,
    Field::ReviewLimit,
    Field::NewLimitToday,
    Field::ReviewLimitToday,
    Field::DeckDesiredRetention,
];
fn page_draft(h: &common::Harness, deck: DeckId) -> options::Draft {
    let mut d = draft(h, deck);
    for field in DECK_VALUES {
        d.keep(field).unwrap();
    }
    d
}
fn set_deck(h: &common::Harness, deck: DeckId, set: impl FnOnce(&mut meguri_model::proto::deck::NormalDeck) + Send + 'static) {
    h.collection
        .run(OpSpec::named("deck values", OpSource::Maintenance), move |ctx| {
            let mut row = ctx.scope().deck(deck)?.unwrap();
            if let DeckKind::Normal(n) = &mut row.kind.kind {
                set(n);
            }
            ctx.scope().put(&row)?;
            Ok(())
        })
        .unwrap();
}
fn normal(h: &common::Harness, deck: DeckId) -> meguri_model::proto::deck::NormalDeck {
    match &row(h, deck).kind.kind {
        DeckKind::Normal(n) => n.clone(),
        _ => panic!("normal deck"),
    }
}
fn preset_row(h: &common::Harness, id: DeckConfigId) -> meguri_model::deck_config::DeckConfig {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .deck_config(id)
        .unwrap()
        .unwrap()
}
#[test]
fn page_save_changes_only_the_shown_preset_value_and_keeps_every_deck_value() {
    let h = common::fresh();
    h.clock.step_ms(3 * 86400000);
    let today = h.collection.today().unwrap();
    set_deck(&h, DeckId(1), move |n| {
        n.new_limit = Some(5);
        n.review_limit = Some(50);
        n.new_limit_today = Some(DayLimit::new(10, today));
        n.review_limit_today = Some(DayLimit::new(70, today - 1));
        n.desired_retention = Some(0.85);
    });
    h.collection
        .run(OpSpec::named("hidden", OpSource::Maintenance), |ctx| {
            let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
            p.config.easy_multiplier = 1.7;
            p.config.learn_steps = vec![2., 30.];
            ctx.scope().put(&p)?;
            Ok(())
        })
        .unwrap();
    let deck_before = row(&h, DeckId(1));
    let preset_before = preset_row(&h, DeckConfigId(1));
    let mut d = page_draft(&h, DeckId(1));
    // O2 item 3: the facts behind the page's "own value" footers.
    assert_eq!(d.base.deck_values[&Field::NewLimit], Value::Number(5.));
    assert_eq!(d.base.deck_values[&Field::NewLimitToday], Value::Number(10.));
    assert_eq!(d.base.deck_values[&Field::ReviewLimitToday], Value::None);
    assert_eq!(d.base.deck_values[&Field::DeckDesiredRetention], Value::Number(f64::from(0.85f32)));
    d.edit(Some(1), Field::NewPerDay, Value::Number(15.)).unwrap();
    assert!(d.has_changes());
    assert!(d.validate(&d.base.today_date.clone()).is_empty());
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert_eq!(saved.undo.as_ref().unwrap().name, "Deck Options");
    assert!(saved.changes.decks.is_empty());
    assert_eq!(saved.changes.deck_configs.len(), 1);
    assert_eq!(format!("{:?}", row(&h, DeckId(1))), format!("{deck_before:?}"));
    let preset = preset_row(&h, DeckConfigId(1));
    assert_eq!(preset.config.new_per_day, 15);
    let mut expected = preset_before.config.clone();
    expected.new_per_day = 15;
    assert_eq!(format!("{:?}", *preset.config), format!("{:?}", *expected));
}
#[test]
fn page_switch_keeps_a_future_today_limit_that_none_would_remove() {
    let h = common::fresh();
    h.clock.step_ms(3 * 86400000);
    let today = h.collection.today().unwrap();
    set_deck(&h, DeckId(1), move |n| {
        n.new_limit_today = Some(DayLimit::new(12, today + 2));
    });
    let b = deck_config::add(&h.collection, "B".into()).unwrap().value;
    let before = dump(&h);
    let mut d = page_draft(&h, DeckId(1));
    d.select(b.0).unwrap();
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert!(saved.changes.deck_configs.is_empty());
    assert_eq!(saved.changes.decks.len(), 1);
    let n = normal(&h, DeckId(1));
    assert_eq!(n.config_id, b.0);
    let kept = n.new_limit_today.as_ref().unwrap();
    assert_eq!((kept.limit, kept.today), (12, today + 2));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&h), before);
    // O9's none, which the first version never sends, removes it.
    let mut d = draft(&h, DeckId(1));
    d.select(b.0).unwrap();
    options::save(&h.collection, d, &Default::default()).unwrap();
    assert!(normal(&h, DeckId(1)).new_limit_today.is_none());
}
#[test]
fn page_switch_to_another_insertion_order_writes_the_deck_and_resorts_new_cards() {
    let h = common::fresh();
    h.collection
        .run(OpSpec::named("cards", OpSource::Maintenance), |ctx| {
            for id in 100..110 {
                let mut c = common::card(NoteId(id / 2), 0);
                c.id = CardId(id);
                c.due = 500 - id as i32;
                ctx.scope().put(&c)?;
            }
            Ok(())
        })
        .unwrap();
    let random = deck_config::add(&h.collection, "Random".into()).unwrap().value;
    let mut d = draft(&h, DeckId(1));
    d.edit(Some(random.0), Field::NewCardInsertOrder, Value::Number(1.))
        .unwrap();
    options::save(&h.collection, d, &Default::default()).unwrap();
    let mut d = page_draft(&h, DeckId(1));
    d.select(random.0).unwrap();
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert!(saved.changes.deck_configs.is_empty());
    assert_eq!(saved.changes.decks.len(), 1);
    assert_eq!(normal(&h, DeckId(1)).config_id, random.0);
    let rows = h.collection.reader(ReaderRole::Query).unwrap();
    let cards = rows.cards_of_deck(DeckId(1)).unwrap();
    let positions = cards.iter().map(|c| c.due).collect::<std::collections::BTreeSet<_>>();
    assert_eq!(positions, (1..=5).collect());
}
#[test]
fn page_edit_of_a_shared_preset_writes_one_preset_row_for_every_deck() {
    let h = common::fresh();
    let shared = deck_config::add(&h.collection, "Shared".into()).unwrap().value;
    let decks = [
        DeckId(1),
        decks::add(&h.collection, "A".into()).unwrap().value.id,
        decks::add(&h.collection, "B".into()).unwrap().value.id,
    ];
    for deck in decks {
        deck_config::assign(&h.collection, deck, shared, false).unwrap();
    }
    let mut d = page_draft(&h, DeckId(1));
    assert_eq!(d.selected, shared.0);
    assert_eq!(d.use_count(shared.0).unwrap(), 3);
    d.edit(Some(shared.0), Field::ReviewsPerDay, Value::Number(420.))
        .unwrap();
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert_eq!(saved.changes.deck_configs.len(), 1);
    assert!(saved.changes.decks.is_empty());
    assert_eq!(preset_row(&h, shared).config.reviews_per_day, 420);
    for deck in decks {
        assert_eq!(normal(&h, deck).config_id, shared.0);
    }
}
#[test]
fn page_validation_refuses_out_of_range_values_and_ignores_kept_deck_values_in_warnings() {
    use meguri_sched::options::{Code, Severity};
    let h = common::fresh();
    let before = dump(&h);
    let mut d = page_draft(&h, DeckId(1));
    let today = d.base.today_date.clone();
    d.edit(Some(1), Field::NewPerDay, Value::Number(10000.)).unwrap();
    d.edit(Some(1), Field::ReviewsPerDay, Value::Number(2.5)).unwrap();
    d.edit(Some(1), Field::DesiredRetention, Value::Number(0.65)).unwrap();
    let errors = d
        .validate(&today)
        .into_iter()
        .filter(|i| i.severity == Severity::Error)
        .map(|i| (i.field, i.code))
        .collect::<Vec<_>>();
    assert!(errors.contains(&(Field::NewPerDay, Code::OutOfRange)));
    assert!(errors.contains(&(Field::ReviewsPerDay, Code::NotWhole)));
    assert!(errors.contains(&(Field::DesiredRetention, Code::OutOfRange)));
    assert!(matches!(
        options::save(&h.collection, d, &Default::default()),
        Err(EngineError::DeckOptionsInvalid(_))
    ));
    assert_eq!(dump(&h), before);
    // O17.1: the deck's own values play no part in the page's warnings.
    set_deck(&h, DeckId(1), |n| {
        n.new_limit = Some(50);
        n.desired_retention = Some(0.75);
    });
    let codes = |d: &options::Draft| d.validate(&today).into_iter().map(|i| i.code).collect::<Vec<_>>();
    let full = codes(&draft(&h, DeckId(1)));
    assert!(full.contains(&Code::ReviewsBelowTenTimesNew) && full.contains(&Code::RetentionLow));
    assert!(codes(&page_draft(&h, DeckId(1))).is_empty());
}
#[test]
fn untouched_page_writes_nothing_and_an_edit_after_keep_writes_the_value() {
    let h = common::fresh();
    let before = dump(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let d = page_draft(&h, DeckId(1));
    assert!(!d.has_changes());
    let saved = options::save(&h.collection, d, &Default::default()).unwrap();
    assert!(saved.undo.is_none());
    assert_eq!(dump(&h), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    let mut d = page_draft(&h, DeckId(1));
    assert!(matches!(
        d.keep(Field::NewPerDay),
        Err(EngineError::InvalidArgument { .. })
    ));
    d.edit(None, Field::NewLimit, Value::Number(9.)).unwrap();
    assert!(!d.kept.contains(&Field::NewLimit));
    options::save(&h.collection, d, &Default::default()).unwrap();
    assert_eq!(normal(&h, DeckId(1)).new_limit, Some(9));
}
