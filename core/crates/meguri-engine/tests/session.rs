//! C18 production API assertions. Authored in the restricted runner; root runs them.
mod common;
mod session_support;
use meguri_engine::{
    EngineError, OpSource, OpSpec, UndoScope,
    rendering::Platform,
    session::{AnswerOutcome, PeekResult, Refusal, StartResult},
};
use meguri_model::{
    clock::Clock,
    ids::{DeckConfigId, DeckId},
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, RowKey, Table};
use session_support::Harness;
fn accept(
    h: &Harness,
    id: meguri_engine::SessionId,
    head: &meguri_engine::session::Head,
    rating: Rating,
) -> meguri_engine::session::Head {
    let first = &head.items[0];
    match h
        .review
        .answer(
            id,
            first.card_id,
            rating,
            first.state_token,
            EpochMillis(h.clock.now_ms()),
            1000,
        )
        .unwrap()
    {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("answer: {other:?}"),
    }
}
#[test]
fn c5_start_peek_head3_refresh_and_u62_cache_use_production_ports() {
    let h = Harness::new(12);
    let before = common::steps::dump(&h.collection);
    let PeekResult::Card(peek) = h.review.peek_first_card(DeckId(1)).unwrap() else {
        panic!("missing peek")
    };
    assert_eq!(common::steps::dump(&h.collection), before);
    let (id, head) = h.start();
    assert_eq!(head.items.len(), 3);
    assert_eq!(peek, head.items[0]);
    assert_eq!(head.counts.new, 12);
    assert!(head.valid_until > h.clock.now_ms() / 1000);
    let keys = head.items.iter().map(|h| h.render_key).collect::<Vec<_>>();
    let a = h.review.render_head(id, &keys, Platform::Mac).unwrap();
    assert!(a[0].question.as_deref().unwrap().contains("front 0"));
    assert!(a[0].media_refs.iter().any(|r| r == "study.png"));
    assert!(!a[0].comparable.question_av.is_empty());
    let count = h.review.render_count();
    let b = h.review.render_head(id, &keys, Platform::Mac).unwrap();
    assert!(std::sync::Arc::ptr_eq(&a[0], &b[0]));
    assert_eq!(h.review.render_count(), count);
    let next = accept(&h, id, &head, Rating::Easy);
    assert_eq!(h.review.render_count(), count);
    assert_eq!(next.items[0].card_id, head.items[0].predicted_next[3].unwrap());
    h.settle();
    assert_eq!(h.logs().len(), 1);
    h.review.end(id).unwrap();
    assert!(h.review.head(id, 3).is_err());
}
#[test]
fn p11_r727_stale_zero_writes_rebuilds_and_next_token_answers() {
    let h = Harness::new(4);
    let (id, head) = h.start();
    let old = head.items[0].clone();
    h.clock.step_ms(1000);
    h.collection
        .run(OpSpec::named("external schedule", OpSource::SyncIncoming), move |ctx| {
            let mut card = ctx.scope().card(old.card_id)?.unwrap();
            card.reps += 1;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    let before = common::steps::dump(&h.collection);
    let AnswerOutcome::Stale { fresh_head, notice } = h
        .review
        .answer(
            id,
            head.items[0].card_id,
            Rating::Good,
            head.items[0].state_token,
            EpochMillis(h.clock.now_ms()),
            1,
        )
        .unwrap()
    else {
        panic!("expected stale")
    };
    assert!(notice);
    assert_ne!(fresh_head.items[0].state_token, head.items[0].state_token);
    assert_eq!(common::steps::dump(&h.collection), before);
    accept(&h, id, &fresh_head, Rating::Good);
    h.settle();
    assert_eq!(h.logs().len(), 1);
}
#[test]
fn c5_not_at_head_changes_no_rows_and_unknown_session_changes_nothing() {
    let h = Harness::new(5);
    let (id, head) = h.start();
    let before = common::steps::dump(&h.collection);
    let outcome = h
        .review
        .answer(
            id,
            h.cards[4],
            Rating::Good,
            head.items[0].state_token,
            EpochMillis(h.clock.now_ms()),
            1,
        )
        .unwrap();
    assert!(matches!(outcome, AnswerOutcome::NotAtHead { .. }));
    assert!(h.review.head(meguri_engine::SessionId(id.0 + 100), 3).is_err());
    assert_eq!(common::steps::dump(&h.collection), before);
}
#[test]
fn p11_r457_shown_card_survives_rebuild_and_p8_rebuild_occurs_in_background() {
    let h = Harness::new(5);
    let (id, head) = h.start();
    let shown = head.items[0].card_id;
    let stream = h.engine.stream().subscribe();
    h.collection
        .run(
            OpSpec::named("external due order", OpSource::SyncIncoming),
            move |ctx| {
                let mut card = ctx.scope().card(shown)?.unwrap();
                card.due = 900;
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    h.settle();
    let mut changed = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(e) = &event.kind
            && let Some(meguri_engine::session::SessionEvent::QueueHeadChanged { diff, .. }) = e.as_any().downcast_ref()
        {
            assert_eq!(diff.head.items[0].card_id, shown);
            changed = true;
        }
    }
    assert!(
        changed,
        "background observed commit must emit its head diff before an explicit head read"
    );
    assert_eq!(h.review.head(id, 3).unwrap().items[0].card_id, shown);
}
#[test]
fn p11_r722_committed_undo_restores_first_card_after_rebuild() {
    let h = Harness::new(6);
    let (id, head) = h.start();
    let shown = head.items[0].card_id;
    accept(&h, id, &head, Rating::Easy);
    h.settle();
    h.collection
        .run(OpSpec::named("background configuration", OpSource::Rollover), |ctx| {
            ctx.scope().set_config("collapseTime", &ConfigValue::from(600u64))?;
            Ok(())
        })
        .unwrap();
    h.settle();
    let head = h.review.undo(UndoScope::Session(id)).unwrap();
    assert_eq!(head.items[0].card_id, shown);
    assert!(h.logs().is_empty());
}
#[test]
fn p11_invalid_easy_days_in_unused_preset_refuses_without_select_or_hook() {
    let h = Harness::new(2);
    h.collection
        .run(
            OpSpec::named("unsupported unused balancing preset", OpSource::Maintenance),
            |ctx| {
                ctx.scope()
                    .set_config("loadBalancerEnabled", &ConfigValue::Bool(true))?;
                let mut preset = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                preset.id = DeckConfigId(99);
                let mut fields = (*preset.config).clone();
                fields.easy_days_percentages = vec![1.0, 1.0];
                preset.config = meguri_model::proto::ProtoBlob::new(fields);
                ctx.scope().put(&preset)?;
                Ok(())
            },
        )
        .unwrap();
    let before = common::steps::dump(&h.collection);
    assert!(
        matches!(h.review.start(DeckId(1), None).unwrap(), StartResult::Refused(Refusal::PresetUnsupported { preset_ids }) if preset_ids == vec![DeckConfigId(99)])
    );
    assert_eq!(common::steps::dump(&h.collection), before);
    assert!(h.review.active_session().is_none());
}
#[test]
fn c2_policy_refusal_and_writer_admission_enable_end_are_real() {
    let h = Harness::new(2);
    let (id, head) = h.start();
    assert_eq!(
        meguri_engine::sched::fsrs_policy::enable_fsrs(&h.collection, &Default::default()).unwrap_err(),
        EngineError::SessionActive
    );
    h.collection
        .run(OpSpec::named("sync off", OpSource::SyncIncoming), |ctx| {
            ctx.scope().set_config("fsrs", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
    // The admitted session continues with actual FSRS transitions.
    let fresh = h.review.head(id, 3).unwrap();
    assert_eq!(fresh.items[0].card_id, head.items[0].card_id);
    accept(&h, id, &fresh, Rating::Easy);
    h.review.end(id).unwrap();
    assert_eq!(h.logs().len(), 1);
    assert!(matches!(
        h.review.start(DeckId(1), None).unwrap(),
        StartResult::Refused(Refusal::FsrsPolicyPending)
    ));
    meguri_engine::sched::fsrs_policy::enable_fsrs(&h.collection, &Default::default()).unwrap();
    assert!(matches!(
        h.review.start(DeckId(1), None).unwrap(),
        StartResult::Started { .. }
    ));
}
#[test]
fn c16_flush_five_answers_retains_real_u25_journals() {
    let h = Harness::new(10);
    let (id, mut head) = h.start();
    for _ in 0..5 {
        head = accept(&h, id, &head, Rating::Easy);
    }
    h.settle();
    assert_eq!(h.logs().len(), 5);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Session(id)).unwrap().name(),
        Some("Answer Card")
    );
    h.review.undo(UndoScope::Global).unwrap();
    assert_eq!(h.logs().len(), 4);
    h.review.end(id).unwrap();
}
#[test]
fn c5_c6_flag_update_keeps_schedule_identity_and_changes_render_identity() {
    let rig = Harness::new(3);
    let (session, original) = rig.start();
    let selected = original.items[0].card_id;
    let edit = OpSpec::named("session flag identity check", OpSource::SyncIncoming)
        .stamp(meguri_storage::StampMode::KeepMtime);
    let count = rig
        .collection
        .run(edit, move |operation| {
            Ok(meguri_engine::flags::set_flag_in(operation.scope(), &[selected], 2)?)
        })
        .unwrap()
        .value;
    assert_eq!(count, 1);

    let refreshed = rig.review.head(session, 3).unwrap();
    let first = &refreshed.items[0];
    assert_eq!(first.card_id, selected);
    assert_eq!(first.flag, 2);
    assert_eq!(first.state_token, original.items[0].state_token);
    assert_ne!(first.render_key, original.items[0].render_key);
    assert_eq!(refreshed.items[1].state_token, original.items[1].state_token);

    // Scheduling identity and the writer's full-row precondition have
    // separate inputs, even when both are represented as digest bytes.
    let reader = rig.collection.reader(ReaderRole::Query).unwrap();
    let stored_identity = reader
        .row_digest(Table::Cards, &RowKey::Id(selected.0))
        .unwrap()
        .unwrap();
    assert_ne!(first.state_token, stored_identity);
}
#[test]
fn supported_home_200_answer_session_api_preserves_protected_cells_and_schema() {
    use meguri_model::{
        card_data::CardData,
        clock::{StepClock, UtcOffset},
        proto::deck::DeckKind,
    };
    use meguri_testkit::dump::preservation::{assert_changes_allowed, raw_dump};
    use std::{collections::BTreeSet, sync::Arc};

    fn capture(c: &meguri_engine::Collection) -> (serde_json::Value, meguri_storage::raw::RawRows) {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        let rows = raw_dump(|query| snapshot.execute_raw(query, &[])).unwrap();
        let objects = snapshot.execute_raw(
            "SELECT type, name, tbl_name, sql FROM sqlite_master WHERE name NOT IN ('sqlite_stat1', 'sqlite_stat4') ORDER BY type, name",
            &[],
        ).unwrap();
        (rows, objects)
    }

    // N11.1: retain the imported collection and add a supported study home on
    // its temporary copy. The Steady reference cards never enter this session.
    let source = common::fixture("../preservation/kitchen-sink.anki2");
    let frozen_file = std::fs::read(&source).unwrap();
    let (dir, path) = common::copy_fixture("../preservation/kitchen-sink.anki2");
    let clock = Arc::new(StepClock::new(session_support::NOW, UtcOffset::UTC, "UTC"));
    let (engine, collection, review) = session_support::open(&path, clock.clone());
    let (_, initial_schema) = capture(&collection);
    let home = meguri_engine::decks::ops::add(&collection, "Session preservation".into())
        .unwrap()
        .value
        .id;
    let preset = meguri_engine::deck_config::add(&collection, "Session preservation".into())
        .unwrap()
        .value;
    collection
        .run(
            OpSpec::named("supported session setup", OpSource::Maintenance),
            move |ctx| {
                meguri_engine::deck_config::update_in(ctx.scope(), preset, |fields| {
                    fields.new_per_day = 9999;
                    fields.reviews_per_day = 9999;
                })?;
                let imported = ctx.scope().deck(DeckId(1893488400002))?.unwrap();
                let mut deck = ctx.scope().deck(home)?.unwrap();
                deck.common = imported.common;
                deck.kind = imported.kind;
                let DeckKind::Normal(normal) = &mut deck.kind.kind else {
                    panic!("the supported home must be normal")
                };
                normal.config_id = preset.0;
                normal.new_limit = Some(9999);
                normal.review_limit = Some(9999);
                normal.new_limit_today = None;
                normal.review_limit_today = None;
                ctx.scope().put(&deck)?;
                ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
                ctx.scope()
                    .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
                Ok(())
            },
        )
        .unwrap();
    let mut cards = Vec::new();
    for number in 0..220 {
        let added = meguri_engine::notes::add(
            &collection,
            meguri_engine::notes::AddNote {
                notetype_id: meguri_model::ids::NotetypeId(1893488400008),
                fields: vec![format!("session item {number}"), "result".into(), "session test".into()],
                tags: Vec::new(),
                deck_id: home,
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap()
        .value;
        assert_eq!(added.cards.len(), 1);
        cards.push(added.cards[0]);
    }
    let setup_cards = cards.clone();
    collection
        .run(
            OpSpec::named("session opaque data setup", OpSource::Maintenance)
                .stamp(meguri_storage::StampMode::CallerSupplied),
            move |ctx| {
                for id in &setup_cards {
                    let mut card = ctx.scope().card(*id)?.unwrap();
                    card.flags = 0xa5;
                    card.data =
                        CardData::parse(r#"{"extensionBlock":{ "values" : [5, 13] },"cd":"{ \"value\": 29 }"}"#);
                    ctx.scope().put(&card)?;
                }
                Ok(())
            },
        )
        .unwrap();
    collection.check_rollover().unwrap();
    let h = Harness {
        dir,
        engine,
        collection,
        review,
        clock,
        cards,
    };
    let (before, schema_before) = capture(&h.collection);
    assert_eq!(schema_before, initial_schema);
    let (protected_cards, studied_deck) = {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        (
            h.cards
                .iter()
                .map(|id| reader.card(*id).unwrap().unwrap())
                .collect::<Vec<_>>(),
            reader.deck(home).unwrap().unwrap(),
        )
    };
    assert!(!studied_deck.common.unknown_fields().is_empty());
    assert!(!studied_deck.kind.unknown_fields().is_empty());
    let StartResult::Started {
        session_id, mut head, ..
    } = h.review.start(home, None).unwrap()
    else {
        panic!("supported fixture home was refused")
    };
    let mut answered = BTreeSet::new();
    for _ in 0..200 {
        let card = head.items[0].card_id;
        assert!(h.cards.contains(&card));
        assert!(
            answered.insert(card),
            "each Easy answer must graduate a distinct new card"
        );
        head = accept(&h, session_id, &head, Rating::Easy);
    }
    h.review.end(session_id).unwrap();
    assert_eq!(answered.len(), 200);
    let (after, schema_after) = capture(&h.collection);
    assert_eq!(
        schema_after, schema_before,
        "all persistent schema objects must survive"
    );

    // Enumerate actual new rows; existing revlog identities are protected.
    let old_logs = before["revlog"].as_object().unwrap();
    let new_logs = after["revlog"].as_object().unwrap();
    let inserted: Vec<_> = new_logs.keys().filter(|id| !old_logs.contains_key(*id)).collect();
    assert_eq!(new_logs.len(), old_logs.len() + 200);
    assert_eq!(inserted.len(), 200);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let mut logged_cards = BTreeSet::new();
    assert!(before["config"]["curDeck"].is_object());
    let mut allowed = vec!["/col/1/mod".to_owned()];
    for column in ["val", "usn", "mtime_secs"] {
        allowed.push(format!("/config/curDeck/{column}"));
    }
    for id in inserted {
        let row = reader
            .revlog_entry(meguri_model::ids::RevlogId(id.parse().unwrap()))
            .unwrap()
            .unwrap();
        assert!(answered.contains(&row.card_id));
        assert!(logged_cards.insert(row.card_id));
        assert_eq!(row.button, 4);
        assert_eq!(row.answer_millis, 1000);
        allowed.push(format!("/revlog/{id}"));
    }
    assert_eq!(logged_cards, answered);
    for id in &answered {
        for column in [
            "mod", "usn", "type", "queue", "due", "ivl", "factor", "reps", "lapses", "left", "data",
        ] {
            allowed.push(format!("/cards/{}/{column}", id.0));
        }
    }
    for column in ["common", "mtime_secs", "usn"] {
        allowed.push(format!("/decks/{}/{column}", home.0));
    }
    assert_changes_allowed(&before, &after, &allowed.iter().map(String::as_str).collect::<Vec<_>>()).unwrap();
    for original in &protected_cards {
        let current = reader.card(original.id).unwrap().unwrap();
        assert_eq!(current.original_deck_id, DeckId(0));
        assert_eq!(current.flags, 0xa5);
        assert_eq!(current.data.custom_data(), original.data.custom_data());
        assert_eq!(
            current.data.unknown_keys().collect::<Vec<_>>(),
            original.data.unknown_keys().collect::<Vec<_>>(),
        );
        assert_eq!(original.data.unknown_keys().count(), 1);
        assert_eq!(original.data.custom_data(), Some("{ \"value\": 29 }"));
        if answered.contains(&original.id) {
            assert_eq!(current.reps, original.reps + 1);
            assert!(current.data.memory_state.is_some());
            // Reconstruct the allowed data edits on the original opaque text.
            // A change to an unknown value, its spacing, order or cd fails.
            let mut expected = original.data.clone();
            expected.new_position = current.data.new_position;
            expected.memory_state = current.data.memory_state;
            expected.desired_retention = current.data.desired_retention;
            expected.decay = current.data.decay;
            expected.last_review_secs = current.data.last_review_secs;
            assert_eq!(after["cards"][original.id.0.to_string()]["data"], expected.to_text());
        } else {
            let key = original.id.0.to_string();
            assert_eq!(after["cards"][&key], before["cards"][&key]);
        }
    }
    let deck_after = reader.deck(home).unwrap().unwrap();
    drop(reader);
    let today = h.collection.today().unwrap();
    let mut expected_common = studied_deck.common.clone();
    if expected_common.last_day_studied != today {
        expected_common.last_day_studied = today;
        expected_common.new_studied = 0;
        expected_common.review_studied = 0;
        expected_common.milliseconds_studied = 0;
    }
    expected_common.new_studied += 200;
    expected_common.milliseconds_studied += 200_000;
    assert_eq!(deck_after.common.to_bytes(), expected_common.to_bytes());
    assert_eq!(deck_after.common.other(), studied_deck.common.other());
    assert_eq!(deck_after.common.unknown_fields(), studied_deck.common.unknown_fields());
    assert_eq!(deck_after.kind.original(), studied_deck.kind.original());
    assert_eq!(deck_after.kind.unknown_fields(), studied_deck.kind.unknown_fields());
    assert_eq!(h.logs().len(), old_logs.len() + 200);
    assert_eq!(std::fs::read(source).unwrap(), frozen_file);
}
#[test]
fn p11_p7_actual_answer_removes_buried_sibling_from_queue_and_counts() {
    let h = Harness::new(1);
    let child = meguri_engine::decks::ops::add(&h.collection, "Default::Sibling".into())
        .unwrap()
        .value
        .id;
    let first = h.cards[0];
    let sibling = h
        .collection
        .run(
            OpSpec::named("cross-preset sibling fixture", OpSource::Maintenance),
            move |ctx| {
                let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                let mut fields = (*p.config).clone();
                fields.bury_new = false;
                p.id = DeckConfigId(2);
                p.config = meguri_model::proto::ProtoBlob::new(fields.clone());
                ctx.scope().put(&p)?;
                fields.bury_new = true;
                p.id = DeckConfigId(1);
                p.config = meguri_model::proto::ProtoBlob::new(fields);
                ctx.scope().put(&p)?;
                let mut deck = ctx.scope().deck(child)?.unwrap();
                let mut kind = (*deck.kind).clone();
                let meguri_model::proto::deck::DeckKind::Normal(normal) = &mut kind.kind else {
                    panic!("normal deck")
                };
                normal.config_id = 1;
                deck.kind = meguri_model::proto::ProtoBlob::new(kind);
                ctx.scope().put(&deck)?;
                let mut parent = ctx.scope().deck(DeckId(1))?.unwrap();
                let mut kind = (*parent.kind).clone();
                let meguri_model::proto::deck::DeckKind::Normal(normal) = &mut kind.kind else {
                    panic!("normal parent")
                };
                normal.config_id = 2;
                parent.kind = meguri_model::proto::ProtoBlob::new(kind);
                ctx.scope().put(&parent)?;
                let mut c = ctx.scope().card(first)?.unwrap();
                c.deck_id = child;
                let sibling = ctx.scope().add_card(&c)?;
                Ok(sibling)
            },
        )
        .unwrap()
        .value;
    let (id, head) = h.start();
    assert_eq!(head.counts.new, 2);
    assert!(head.items.iter().any(|h| h.card_id == sibling));
    let shown = h.review.refresh_card(id, sibling).unwrap();
    let after = accept(&h, id, &shown, Rating::Easy);
    assert_eq!(after.counts.new, 0);
    assert!(after.items.iter().all(|h| h.card_id != first));
    h.settle();
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(first)
            .unwrap()
            .unwrap()
            .queue,
        meguri_model::card::CardQueue::BuriedByScheduler
    );
}
#[test]
fn p11_r460_r461_labels_now_rebuild_and_clear_history_after_shown_answer() {
    // Exercise both a creation-day boundary (the day number clamps to zero)
    // and the next day's D8 event with an existing undoable answer.
    for shift in [0, 86_400_000] {
        let h = Harness::new(5);
        h.clock
            .set_now_ms(session_support::NOW - 6 * 3600 * 1000 - 2000 + shift);
        let (id, initial) = h.start();
        let head = accept(&h, id, &initial, Rating::Easy);
        h.settle();
        let history = h.collection.peek_undo(UndoScope::Session(id)).unwrap();
        assert_eq!(history.name(), Some("Answer Card"));
        let shown = head.items[0].card_id;
        h.clock.step_ms(4000);
        h.collection.check_rollover().unwrap();
        h.settle();
        let new_day = h.review.head(id, 3).unwrap();
        assert_eq!(new_day.items[0].card_id, shown);
        assert_ne!(new_day.items[0].state_token, head.items[0].state_token);
        assert_eq!(h.collection.peek_undo(UndoScope::Session(id)).unwrap(), history);
        let answer = h
            .review
            .answer(
                id,
                shown,
                Rating::Good,
                new_day.items[0].state_token,
                EpochMillis(h.clock.now_ms()),
                1000,
            )
            .unwrap();
        assert!(
            matches!(answer, AnswerOutcome::Accepted { rolled_over: true, .. }),
            "crossing answer at shift {shift}: {answer:?}"
        );
        h.settle();
        assert_eq!(
            h.collection.peek_undo(UndoScope::Session(id)).unwrap(),
            meguri_engine::UndoStatus::Unavailable(meguri_engine::DisabledReason::ClearedByRollover)
        );
        assert_eq!(h.review.active_session(), Some(id));
    }
}
#[test]
fn p11_p8_background_rebuild_emits_at_observed_commit_without_head_poll() {
    let h = Harness::new(6);
    let (_id, head) = h.start();
    let queued = head.items[1].card_id;
    let expected_second = head.items[2].card_id;
    let stream = h.engine.stream().subscribe();
    let change = h
        .collection
        .run(
            OpSpec::named("observed queued due change", OpSource::SyncIncoming),
            move |ctx| {
                let mut card = ctx.scope().card(queued)?.unwrap();
                card.due = 999;
                ctx.scope().put(&card)?;
                Ok(())
            },
        )
        .unwrap();
    h.settle();
    let mut found = false;
    while let Some(event) = stream.try_recv() {
        if let meguri_engine::EventKind::Extension(extension) = &event.kind
            && let Some(meguri_engine::session::SessionEvent::QueueHeadChanged { diff, .. }) =
                extension.as_any().downcast_ref()
        {
            assert_eq!(event.cause_seq, Some(change.seq));
            assert_eq!(diff.head.items[0].card_id, head.items[0].card_id);
            assert_eq!(diff.head.items[1].card_id, expected_second);
            found = true;
        }
    }
    assert!(found, "an observed rebuild must publish before any explicit head poll");
}
