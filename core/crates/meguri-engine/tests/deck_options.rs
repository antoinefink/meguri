//! U49 transactions on real storage and the real scheduler memory path.
mod common;
use meguri_engine::{
    EngineError, OpSource, OpSpec, UndoScope, deck_config, decks, sched::deck_options as options, tags::RunOptions,
};
use meguri_model::{
    card::{CardQueue, CardType},
    ids::{CardId, DeckConfigId, DeckId, NoteId, RevlogId},
    proto::deck::{DayLimit, DeckKind},
};
use meguri_sched::options::{Field, Value};
use meguri_storage::{ReadRows, ReaderRole};
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
    let before = dump(&h);
    assert!(matches!(
        d.remove(preset.0, false),
        Err(EngineError::NeedsSchemaConfirmation { .. })
    ));
    d.removals.insert(preset.0);
    d.presets.retain(|p| p.key != preset.0);
    assert!(matches!(
        options::save(&h.collection, d, &Default::default()),
        Err(EngineError::NeedsSchemaConfirmation { .. })
    ));
    assert_eq!(dump(&h), before);
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
    d.remove(removed.0, true).unwrap();
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
fn preferences_validate_first_and_are_one_step_without_adding_default_keys() {
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
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            load_balancer: Some(true),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(dump(&h), before);
    options::save_preferences(
        &h.collection,
        options::PreferencesChange {
            learn_ahead_minutes: Some(33.),
            load_balancer: Some(false),
            ..Default::default()
        },
        &Default::default(),
    )
    .unwrap();
    assert_eq!(options::preferences(&h.collection).unwrap().learn_ahead_minutes, 33);
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
    d.remove(z.0, true).unwrap();
    assert_eq!(d.selected, 1);
    assert!(d.removals.contains(&z.0));
    let new = d.add(" Fresh ", false, h.clock.as_ref()).unwrap().unwrap();
    assert_eq!(d.preset(new).unwrap().name, "Fresh");
    d.remove(new, false).unwrap();
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
