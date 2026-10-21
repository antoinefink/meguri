//! Product atomicity/refusal/session tests complement the frozen golden guard.
mod common;
mod session_support;
use common::{Harness, card, fresh, note, steps::dump};
use meguri_engine::{
    Collection, EngineError, OpSource, OpSpec, StepKind, UndoScope,
    sched::ops::{self, Context, Refusal, Request, Targets, UnburyMode},
};
use meguri_model::{
    card::{CardQueue, CardType},
    card_data::CardData,
    clock::Clock,
    ids::*,
    proto::{
        ProtoBlob,
        deck::{DeckKind, DeckKindMessage, FilteredDeck},
    },
    time::EpochMillis,
};
use meguri_sched::answer::Rating;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, RowKey, Table};

fn setup() -> (Harness, Vec<CardId>, NoteId) {
    let h = fresh();
    let (ids, note_id) = h.collection.run(OpSpec::named("U48 fixture", OpSource::Maintenance), |ctx| {
        let s = ctx.scope();
        s.set_config("fsrs", &ConfigValue::from(true))?;
        s.set_config("loadBalancerEnabled", &ConfigValue::from(false))?;
        s.set_config("nextPos", &ConfigValue::from(u64::from(u32::MAX)))?;
        let mut p = s.deck_config(DeckConfigId(1))?.unwrap();
        p.config.bury_new = true;
        s.put(&p)?;
        let nid = s.add_note(&note(&["a", "b"]))?;
        let mut ids = Vec::new();
        for i in 0..4 {
            let mut c = card(nid, i);
            c.due = i32::from(i) + 1;
            c.data = CardData::parse("{\"s\":4.5,\"d\":5.048,\"dr\":0.9,\"decay\":0.1542,\"cd\":\"{x}\",\"zz\":{\"n\":1e400,\"v\":[1, 2]}}");
            if i >= 2 { c.card_type = CardType::Review; c.queue = CardQueue::Review; c.interval = 10; c.ease_factor = 2500; c.reps = 7; c.lapses = 3; }
            if i == 1 { c.queue = CardQueue::Suspended; }
            ids.push(s.add_card(&c)?);
        }
        Ok((ids, nid))
    }).unwrap().value;
    h.collection
        .clear_undo(meguri_engine::ClearCause::CheckDatabase)
        .unwrap();
    h.clock.step_ms(1000);
    (h, ids, note_id)
}
fn run(c: &Collection, request: Request) -> meguri_engine::OpResult<ops::ResultValue> {
    ops::run(c, request, &Default::default()).unwrap()
}
fn read(c: &Collection, id: CardId) -> meguri_model::card::Card {
    c.reader(ReaderRole::Query).unwrap().card(id).unwrap().unwrap()
}
fn exact_undo_redo(c: &Collection, before: common::steps::Dump, name: &str) {
    let after = dump(c);
    let step = c.peek_undo(UndoScope::Global).unwrap();
    assert_eq!(step.name(), Some(name));
    c.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(c), before);
    c.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(c), after);
}
#[test]
fn note_bury_skips_suspended_normalizes_json_and_noop_keeps_exact_rows() {
    let (h, ids, note) = setup();
    let c = &h.collection;
    let before = dump(c);
    let result = run(c, Request::Bury(Targets::Notes(vec![note, NoteId(-1)])));
    assert_eq!(result.value.count, 3);
    assert!(
        matches!(c.peek_undo(UndoScope::Global).unwrap(), meguri_engine::UndoStatus::Available(step) if step.kind == StepKind::Other)
    );
    assert_eq!(read(c, ids[1]).queue, CardQueue::Suspended);
    assert!(
        read(c, ids[0])
            .data
            .to_text()
            .contains("\"zz\":{\"n\":1e400,\"v\":[1, 2]}")
    );
    assert_eq!(common::query_int(c, "SELECT count(*) FROM revlog"), 0);
    exact_undo_redo(c, before, "Bury");
    let before = dump(c);
    let no = run(c, Request::Bury(Targets::Notes(vec![note])));
    assert_eq!(no.value.count, 0);
    assert!(no.undo.is_none());
    assert!(no.changes.is_empty());
    assert_eq!(dump(c), before);
    let result = run(c, Request::Unsuspend(ids.clone()));
    assert_eq!(result.value.count, 4);
    assert_eq!(read(c, ids[0]).queue, CardQueue::New);
    assert_eq!(read(c, ids[2]).queue, CardQueue::Review);
}
#[test]
fn suspend_cards_is_one_step_missing_is_noop_and_duplicates_are_typed_before_writes() {
    let (h, ids, note) = setup();
    let c = &h.collection;
    let before = dump(c);
    assert_eq!(run(c, Request::Suspend(Targets::Cards(ids.clone()))).value.count, 3);
    exact_undo_redo(c, before, "Suspend");
    let before = dump(c);
    let status = c.peek_undo(UndoScope::Global).unwrap();
    let requests = vec![
        Request::Bury(Targets::Cards(vec![ids[0], ids[0]])),
        Request::Suspend(Targets::Notes(vec![note, note])),
        Request::Unsuspend(vec![ids[0], ids[0]]),
        Request::SetDue {
            cards: vec![ids[0], ids[0]],
            days: "3".into(),
            context: None,
        },
        Request::Reset {
            cards: vec![ids[0], ids[0]],
            restore_position: false,
            reset_counts: true,
            context: None,
        },
        Request::GradeNow {
            cards: vec![ids[0], ids[0]],
            rating: Rating::Good,
        },
    ];
    for request in requests {
        assert!(matches!(
            ops::run(c, request, &Default::default()),
            Err(EngineError::InvalidArgument { .. })
        ));
        assert_eq!(dump(c), before);
        assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    }
    let no = run(c, Request::Suspend(Targets::Cards(vec![CardId(-1)])));
    assert_eq!(no.value.count, 0);
    assert!(no.undo.is_none());
    assert_eq!(dump(c), before);
}
#[test]
fn set_due_ae26_context_and_marker_share_exact_undo() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let day = c.check_rollover().unwrap();
    let id = ids[2];
    c.run(OpSpec::named("last review", OpSource::Maintenance), move |ctx| {
        let mut card = ctx.scope().card(id)?.unwrap();
        card.data.last_review_secs = Some(day.next_day_at - 496_800);
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap();
    let old = read(c, id);
    let before = dump(c);
    let r = run(
        c,
        Request::SetDue {
            cards: vec![id],
            days: "3!".into(),
            context: Some(Context::Reviewer),
        },
    );
    assert_eq!(r.value.count, 1);
    let card = read(c, id);
    assert_eq!((card.interval, card.due), (8, day.today.wrapping_add(3) as i32));
    assert_eq!(card.data.memory_state, old.data.memory_state);
    assert_eq!(common::query_int(c, "SELECT factor FROM revlog"), 549);
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .config_value("setDueReviewer")
            .unwrap(),
        Some(ConfigValue::from("3!"))
    );
    exact_undo_redo(c, before, "Set Due Date");
    for (text, kind) in [
        ("", ops::InvalidDays::Empty),
        ("abc", ops::InvalidDays::Malformed),
        ("-1", ops::InvalidDays::Malformed),
        ("3--4", ops::InvalidDays::Malformed),
        ("4294967296", ops::InvalidDays::TooLarge),
    ] {
        let before = dump(c);
        let status = c.peek_undo(UndoScope::Global).unwrap();
        assert!(
            matches!(ops::run(c, Request::SetDue { cards: vec![id], days: text.into(), context: Some(Context::Browser) }, &Default::default()), Err(EngineError::SchedOp(Refusal::InvalidDays(k))) if k == kind)
        );
        assert_eq!(dump(c), before);
        assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    }
}
#[test]
fn reset_wraps_positions_in_caller_order_and_preserves_owned_json_and_tags() {
    let (h, ids, note_id) = setup();
    let c = &h.collection;
    c.run(OpSpec::named("reset fixture", OpSource::Maintenance), move |ctx| {
        let mut n = ctx.scope().note(note_id)?.unwrap();
        n.tags = " leech ".into();
        ctx.scope().put(&n)?;
        Ok(())
    })
    .unwrap();
    let before = dump(c);
    run(
        c,
        Request::Reset {
            cards: vec![ids[2], ids[0]],
            restore_position: false,
            reset_counts: true,
            context: Some(Context::Browser),
        },
    );
    assert_eq!((read(c, ids[2]).due, read(c, ids[0]).due), (-1, 0));
    let card = read(c, ids[2]);
    assert_eq!((card.reps, card.lapses, card.interval), (0, 0, 0));
    assert!(card.data.memory_state.is_none());
    assert!(card.data.new_position.is_none());
    assert_eq!(card.data.desired_retention, Some(0.9));
    assert!(card.data.to_text().contains("\"zz\""));
    assert_eq!(
        c.reader(ReaderRole::Query).unwrap().config_value("nextPos").unwrap(),
        Some(ConfigValue::from(1u64))
    );
    assert_eq!(
        common::query_int(c, "SELECT count(*) FROM revlog WHERE type=4 AND factor=0 AND time=0"),
        2
    );
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .note(note_id)
            .unwrap()
            .unwrap()
            .tags,
        " leech "
    );
    exact_undo_redo(c, before, "Reset Card");
}
#[test]
fn grade_now_answers_four_cards_in_one_step_and_one_undo_including_siblings() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let before = dump(c);
    let r = run(
        c,
        Request::GradeNow {
            cards: ids.clone(),
            rating: Rating::Good,
        },
    );
    assert_eq!(r.value.count, 4);
    assert!(
        matches!(c.peek_undo(UndoScope::Global).unwrap(), meguri_engine::UndoStatus::Available(step) if step.kind == StepKind::Other)
    );
    assert_eq!(
        common::query_int(c, "SELECT count(*) FROM revlog WHERE ease=3 AND time=0"),
        4
    );
    let rows = c.reader(ReaderRole::Query).unwrap();
    for (index, id) in ids.iter().enumerate() {
        let log = rows
            .revlog_entry(RevlogId(h.clock.now_ms() + index as i64))
            .unwrap()
            .unwrap();
        assert_eq!(log.card_id, *id);
        assert!(rows.card(*id).unwrap().unwrap().queue.stored() >= 0);
    }
    drop(rows);
    exact_undo_redo(c, before, "Grade Now");
}
#[test]
fn grade_now_refuses_missing_filtered_and_all_unsupported_presets_without_changes() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let before = dump(c);
    assert!(matches!(
        ops::run(
            c,
            Request::GradeNow {
                cards: vec![ids[0], CardId(-1)],
                rating: Rating::Good
            },
            &Default::default()
        ),
        Err(EngineError::NotFound {
            table: Table::Cards,
            ..
        })
    ));
    assert_eq!(dump(c), before);
    let id = ids[3];
    c.run(OpSpec::named("filtered refusal", OpSource::Maintenance), move |ctx| {
        let mut card = ctx.scope().card(id)?.unwrap();
        card.original_deck_id = DeckId(1);
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap();
    let before = dump(c);
    let status = c.peek_undo(UndoScope::Global).unwrap();
    assert!(
        matches!(ops::run(c, Request::GradeNow { cards: vec![ids[0], id], rating: Rating::Good }, &Default::default()), Err(EngineError::SchedOp(Refusal::InFilteredDeck { card })) if card==id)
    );
    assert_eq!(dump(c), before);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    let selected = ids.clone();
    c.run(
        OpSpec::named("unsupported presets", OpSource::Maintenance),
        move |ctx| {
            for (index, id) in selected.into_iter().enumerate() {
                let preset_id = DeckConfigId(if index % 2 == 0 { 91 } else { 17 });
                let mut p = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
                p.id = preset_id;
                p.config.set_fsrs_params_6(vec![1.0; 34]);
                ctx.scope().put(&p)?;
                let mut d = ctx.scope().deck(DeckId(1))?.unwrap();
                d.id = DeckId(100 + index as i64);
                d.name = format!("Preset {index}");
                if let DeckKind::Normal(n) = &mut d.kind.kind {
                    n.config_id = preset_id.0;
                }
                ctx.scope().put(&d)?;
                let mut card = ctx.scope().card(id)?.unwrap();
                card.deck_id = d.id;
                card.original_deck_id = DeckId(0);
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    let before = dump(c);
    let status = c.peek_undo(UndoScope::Global).unwrap();
    assert!(
        matches!(ops::run(c, Request::GradeNow { cards: ids, rating: Rating::Good }, &Default::default()), Err(EngineError::SchedOp(Refusal::PresetUnsupported { preset_ids })) if preset_ids==vec![DeckConfigId(17),DeckConfigId(91)])
    );
    assert_eq!(dump(c), before);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
}
#[test]
fn empty_filtered_preserves_negative_queues_and_deck_and_restores_preview_and_interday() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let selected = ids.clone();
    c.run(OpSpec::named("filtered fixture", OpSource::Maintenance), move |ctx| {
        let mut d = ctx.scope().deck(DeckId(1))?.unwrap();
        d.id = DeckId(42);
        d.name = "Filtered".into();
        d.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
        ctx.scope().put(&d)?;
        for (i, id) in selected.iter().enumerate() {
            let mut c = ctx.scope().card(*id)?.unwrap();
            c.deck_id = d.id;
            c.original_deck_id = DeckId(1);
            c.original_due = if i == 0 { 1_894_701_000 } else { 2 };
            c.due = -100_000;
            c.card_type = CardType::Relearning;
            c.queue = if i == 2 {
                CardQueue::BuriedByUser
            } else {
                CardQueue::Preview
            };
            ctx.scope().put(&c)?;
        }
        Ok(())
    })
    .unwrap();
    let deck = c
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Decks, &RowKey::Id(42))
        .unwrap();
    let before = dump(c);
    run(c, Request::EmptyFiltered { deck: DeckId(42) });
    assert_eq!(read(c, ids[0]).queue, CardQueue::Learning);
    assert_eq!(read(c, ids[1]).queue, CardQueue::InterdayLearning);
    assert_eq!(read(c, ids[2]).queue, CardQueue::BuriedByUser);
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Decks, &RowKey::Id(42))
            .unwrap(),
        deck
    );
    exact_undo_redo(c, before, "Empty");
    let before = dump(c);
    assert!(matches!(
        ops::run(c, Request::EmptyFiltered { deck: DeckId(1) }, &Default::default()),
        Err(EngineError::SchedOp(Refusal::NotFiltered { .. }))
    ));
    assert!(matches!(
        ops::run(c, Request::EmptyFiltered { deck: DeckId(-1) }, &Default::default()),
        Err(EngineError::NotFound { .. })
    ));
    assert_eq!(dump(c), before);
}
#[test]
fn empty_filtered_excludes_rows_with_only_an_original_deck_match() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let selected = [ids[0], ids[1]];
    c.run(
        OpSpec::named("filtered membership fixture", OpSource::Maintenance),
        move |ctx| {
            for (id, name) in [(42, "Filtered"), (43, "External")] {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.id = DeckId(id);
                deck.name = name.into();
                deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
                ctx.scope().put(&deck)?;
            }
            for (index, id) in selected.into_iter().enumerate() {
                let mut card = ctx.scope().card(id)?.unwrap();
                card.deck_id = DeckId(if index == 0 { 42 } else { 43 });
                card.original_deck_id = DeckId(if index == 0 { 1 } else { 42 });
                card.original_due = 7 + index as i32;
                card.due = -100_000;
                card.queue = CardQueue::Preview;
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    h.clock.step_ms(1000);
    let before = dump(c);
    let external = c
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(ids[1].0))
        .unwrap();
    assert_eq!(run(c, Request::EmptyFiltered { deck: DeckId(42) }).value.count, 1);
    let returned = read(c, ids[0]);
    assert_eq!((returned.deck_id, returned.original_deck_id), (DeckId(1), DeckId(0)));
    assert_eq!(
        (returned.due, returned.original_due, returned.queue),
        (7, 0, CardQueue::New)
    );
    assert_eq!(dump(c)["decks"], before["decks"]);
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(ids[1].0))
            .unwrap(),
        external
    );
    exact_undo_redo(c, before, "Empty");
}
#[test]
fn admitted_session_operation_drains_pending_answer_and_returns_real_head_and_scope() {
    let h = session_support::Harness::new(5);
    let (id, head) = h.start();
    let answered = &head.items[0];
    h.review
        .answer(
            id,
            answered.card_id,
            Rating::Easy,
            answered.state_token,
            EpochMillis(h.clock.now_ms()),
            0,
        )
        .unwrap();
    let target = head.items[1].card_id;
    let r = h
        .review
        .sched_op(
            Request::Suspend(Targets::Cards(vec![target])),
            Some(id),
            &Default::default(),
        )
        .unwrap();
    assert!(
        matches!(h.collection.peek_undo(UndoScope::Session(id)).unwrap(), meguri_engine::UndoStatus::Available(step) if step.session == Some(id) && step.kind == StepKind::Other)
    );
    assert_eq!(r.value.head_diff.as_ref().unwrap().head, h.review.head(id, 3).unwrap());
    let fresh = meguri_engine::sched::queue::QueueProvider::default()
        .compatibility_queue(&h.collection, Some(DeckId(1)))
        .unwrap();
    assert_eq!(r.value.head_diff.as_ref().unwrap().head.counts, fresh.counts);
    assert_eq!(
        r.value
            .head_diff
            .as_ref()
            .unwrap()
            .head
            .items
            .iter()
            .map(|h| h.card_id)
            .collect::<Vec<_>>(),
        fresh.presentation().iter().take(3).map(|e| e.id).collect::<Vec<_>>()
    );
    assert!(
        !r.value
            .head_diff
            .unwrap()
            .head
            .items
            .iter()
            .any(|h| h.card_id == target)
    );
    assert_eq!(h.logs().len(), 1);
    assert!(matches!(
        ops::run(&h.collection, Request::Unsuspend(vec![target]), &Default::default()),
        Err(EngineError::SessionActive)
    ));
    h.review.undo(UndoScope::Session(id)).unwrap();
    assert!(read(&h.collection, target).queue.stored() >= 0);
    h.review
        .sched_op(Request::Bury(Targets::Cards(vec![target])), None, &Default::default())
        .unwrap();
    assert!(!h.review.head(id, 3).unwrap().items.iter().any(|h| h.card_id == target));
    let before = dump(&h.collection);
    assert!(
        h.review
            .sched_op(
                Request::Unsuspend(vec![target]),
                Some(meguri_engine::SessionId(id.0 + 100)),
                &Default::default()
            )
            .is_err()
    );
    assert_eq!(dump(&h.collection), before);
    h.review.end(id).unwrap();
}
#[test]
fn failures_after_an_earlier_card_write_roll_back_cards_logs_counters_config_and_history() {
    for operation in 0..3 {
        let (h, ids, _) = setup();
        let c = &h.collection;
        let veto = ids[1].0;
        c.submit_maintenance(meguri_engine::executor::catalog::ANALYZE, std::time::Duration::from_millis(10), move |writer| {
            writer.execute_raw(&format!("CREATE TRIGGER veto_second_log BEFORE INSERT ON revlog WHEN new.cid={veto} BEGIN SELECT RAISE(ABORT,'synthetic U48 log veto'); END"), &[])?; Ok(())
        }).wait().unwrap();
        let before = dump(c);
        let status = c.peek_undo(UndoScope::Global).unwrap();
        let request = match operation {
            0 => Request::SetDue {
                cards: vec![ids[0], ids[1]],
                days: "3".into(),
                context: Some(Context::Browser),
            },
            1 => Request::Reset {
                cards: vec![ids[0], ids[1]],
                restore_position: false,
                reset_counts: true,
                context: Some(Context::Reviewer),
            },
            _ => Request::GradeNow {
                cards: vec![ids[0], ids[1]],
                rating: Rating::Good,
            },
        };
        assert!(ops::run(c, request, &Default::default()).is_err());
        assert_eq!(dump(c), before);
        assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    }
}
#[test]
fn unbury_modes_and_query_use_actual_deck_descendants_and_current_due() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let selected = ids.clone();
    c.run(
        OpSpec::named("deck burial fixture", OpSource::Maintenance),
        move |ctx| {
            for (id, name) in [(42, "Home"), (43, "Home\u{1f}Sub"), (44, "Homeward")] {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.id = DeckId(id);
                deck.name = name.into();
                ctx.scope().put(&deck)?;
            }
            for (i, id) in selected.into_iter().enumerate() {
                let mut c = ctx.scope().card(id)?.unwrap();
                c.deck_id = DeckId(if i == 3 { 44 } else { 43 });
                c.card_type = CardType::Learning;
                c.due = if i == 0 { 1_894_701_900 } else { 3 };
                c.queue = if i == 0 {
                    CardQueue::BuriedByScheduler
                } else if i == 2 {
                    CardQueue::Suspended
                } else {
                    CardQueue::BuriedByUser
                };
                ctx.scope().put(&c)?;
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(
        ops::buried_kinds(c, DeckId(42)).unwrap(),
        ops::BuriedKinds {
            manual: true,
            siblings: true
        }
    );
    let before = dump(c);
    assert_eq!(
        run(
            c,
            Request::UnburyDeck {
                deck: DeckId(42),
                mode: UnburyMode::Siblings
            }
        )
        .value
        .count,
        1
    );
    assert_eq!(read(c, ids[0]).queue, CardQueue::Learning);
    assert_eq!(read(c, ids[1]).queue, CardQueue::BuriedByUser);
    exact_undo_redo(c, before, "Unbury/Unsuspend");
    assert_eq!(
        run(
            c,
            Request::UnburyDeck {
                deck: DeckId(42),
                mode: UnburyMode::Manual
            }
        )
        .value
        .count,
        1
    );
    assert_eq!(read(c, ids[1]).queue, CardQueue::InterdayLearning);
    assert_eq!(read(c, ids[2]).queue, CardQueue::Suspended);
    assert_eq!(read(c, ids[3]).queue, CardQueue::BuriedByUser);
    assert_eq!(
        run(
            c,
            Request::UnburyDeck {
                deck: DeckId(-1),
                mode: UnburyMode::All
            }
        )
        .value
        .count,
        0
    );
}
#[test]
fn home_unbury_and_query_exclude_cards_currently_in_an_external_filtered_deck() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let selected = [ids[0], ids[1]];
    c.run(
        OpSpec::named("external burial fixture", OpSource::Maintenance),
        move |ctx| {
            for (id, name) in [(42, "Home"), (43, "Home\u{1f}Sub"), (44, "External")] {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.id = DeckId(id);
                deck.name = name.into();
                if id == 44 {
                    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
                }
                ctx.scope().put(&deck)?;
            }
            for (index, id) in selected.into_iter().enumerate() {
                let mut card = ctx.scope().card(id)?.unwrap();
                card.deck_id = DeckId(if index == 0 { 44 } else { 43 });
                card.original_deck_id = DeckId(if index == 0 { 42 } else { 0 });
                card.queue = if index == 0 {
                    CardQueue::BuriedByScheduler
                } else {
                    CardQueue::BuriedByUser
                };
                ctx.scope().put(&card)?;
            }
            Ok(())
        },
    )
    .unwrap();
    h.clock.step_ms(1000);
    assert_eq!(
        ops::buried_kinds(c, DeckId(42)).unwrap(),
        ops::BuriedKinds {
            manual: true,
            siblings: false
        }
    );
    let before = dump(c);
    let status = c.peek_undo(UndoScope::Global).unwrap();
    let no = run(
        c,
        Request::UnburyDeck {
            deck: DeckId(42),
            mode: UnburyMode::Siblings,
        },
    );
    assert_eq!(no.value.count, 0);
    assert!(no.undo.is_none());
    assert!(no.changes.is_empty());
    assert_eq!(dump(c), before);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    let external = c
        .reader(ReaderRole::Query)
        .unwrap()
        .row_image(Table::Cards, &RowKey::Id(ids[0].0))
        .unwrap();
    assert_eq!(
        run(
            c,
            Request::UnburyDeck {
                deck: DeckId(42),
                mode: UnburyMode::All
            }
        )
        .value
        .count,
        1
    );
    assert_eq!(read(c, ids[1]).queue, CardQueue::New);
    assert_eq!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .row_image(Table::Cards, &RowKey::Id(ids[0].0))
            .unwrap(),
        external
    );
    assert_eq!(ops::buried_kinds(c, DeckId(42)).unwrap(), ops::BuriedKinds::default());
    exact_undo_redo(c, before, "Unbury/Unsuspend");
}
#[test]
fn subtree_unbury_counts_a_filtered_descendant_with_a_home_ancestor_once() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let id = ids[0];
    c.run(
        OpSpec::named("filtered descendant fixture", OpSource::Maintenance),
        move |ctx| {
            for (id, name) in [(42, "Home"), (43, "Home\u{1f}Filtered")] {
                let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
                deck.id = DeckId(id);
                deck.name = name.into();
                if id == 43 {
                    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
                }
                ctx.scope().put(&deck)?;
            }
            let mut card = ctx.scope().card(id)?.unwrap();
            card.deck_id = DeckId(43);
            card.original_deck_id = DeckId(42);
            card.original_due = 1_894_701_900;
            card.due = -100_000;
            card.card_type = CardType::Learning;
            card.queue = CardQueue::BuriedByUser;
            ctx.scope().put(&card)?;
            Ok(())
        },
    )
    .unwrap();
    h.clock.step_ms(1000);
    let before = dump(c);
    assert_eq!(
        run(
            c,
            Request::UnburyDeck {
                deck: DeckId(42),
                mode: UnburyMode::All
            }
        )
        .value
        .count,
        1
    );
    let restored = read(c, id);
    assert_eq!(restored.queue, CardQueue::InterdayLearning);
    assert_eq!((restored.deck_id, restored.original_deck_id), (DeckId(43), DeckId(42)));
    assert_eq!((restored.due, restored.original_due), (-100_000, 1_894_701_900));
    assert_eq!(dump(c)["decks"], before["decks"]);
    exact_undo_redo(c, before, "Unbury/Unsuspend");
}
#[test]
fn set_due_preflights_all_home_decks_and_empty_selection_writes_no_context() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let id = ids[3];
    c.run(OpSpec::named("missing home", OpSource::Maintenance), move |ctx| {
        let mut c = ctx.scope().card(id)?.unwrap();
        c.deck_id = DeckId(999);
        ctx.scope().put(&c)?;
        Ok(())
    })
    .unwrap();
    let before = dump(c);
    let status = c.peek_undo(UndoScope::Global).unwrap();
    assert!(matches!(
        ops::run(
            c,
            Request::SetDue {
                cards: ids,
                days: "3".into(),
                context: Some(Context::Reviewer)
            },
            &Default::default()
        ),
        Err(EngineError::NotFound {
            table: Table::Decks,
            ..
        })
    ));
    assert_eq!(dump(c), before);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), status);
    let no = run(
        c,
        Request::SetDue {
            cards: vec![CardId(-1)],
            days: "5-2!".into(),
            context: Some(Context::Reviewer),
        },
    );
    assert!(no.undo.is_none());
    assert_eq!(dump(c), before);
}

#[test]
fn cancelled_session_mutation_leaves_rows_history_and_head_unchanged() {
    let h = session_support::Harness::new(3);
    let (id, head) = h.start();
    let before = dump(&h.collection);
    let status = h.collection.peek_undo(UndoScope::Session(id)).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    cancel.cancel();
    assert!(matches!(
        h.review.sched_op(
            Request::Bury(Targets::Cards(vec![head.items[0].card_id])),
            Some(id),
            &meguri_engine::tags::RunOptions {
                cancel: Some(cancel),
                progress: None
            }
        ),
        Err(EngineError::Cancelled)
    ));
    assert_eq!(dump(&h.collection), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Session(id)).unwrap(), status);
    assert_eq!(h.review.head(id, 3).unwrap(), head);
    h.review.end(id).unwrap();
}
#[test]
fn later_grade_now_sibling_reads_the_burial_written_by_the_earlier_answer() {
    let (h, ids, _) = setup();
    let c = &h.collection;
    let second = ids[1];
    c.run(OpSpec::named("active sibling", OpSource::Maintenance), move |ctx| {
        let mut card = ctx.scope().card(second)?.unwrap();
        card.queue = CardQueue::New;
        ctx.scope().put(&card)?;
        Ok(())
    })
    .unwrap();
    let before = dump(c);
    run(
        c,
        Request::GradeNow {
            cards: vec![ids[0], ids[1]],
            rating: Rating::Good,
        },
    );
    let rows = c.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        rows.deck(DeckId(1)).unwrap().unwrap().common.new_studied,
        1,
        "the second sibling was buried before its answer, so contributes no new counter"
    );
    assert_eq!(rows.card(ids[1]).unwrap().unwrap().queue, CardQueue::Learning);
    drop(rows);
    exact_undo_redo(c, before, "Grade Now");
}
