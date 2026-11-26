//! U1 writer operations crossing production U2 shown-card admission.
mod common;
mod session_support;

use meguri_engine::{
    CancelToken, OpSource, OpSpec, ProgressSink,
    bulk::{CardTarget, change_deck::{self, DeckTarget}},
    decks::filtered::{self, Definition, Term},
    session::{AnswerOutcome, Head, ReviewTerminalState, StartResult},
};
use meguri_model::{clock::Clock, ids::DeckId, time::EpochMillis, usn::Usn};
use meguri_sched::answer::Rating;
use meguri_storage::{ReadRows, ReaderRole, Record, StampMode};
use session_support::Harness;
use std::sync::Arc;

fn definition(h: &Harness, index: usize, reschedule: bool) -> Definition {
    Definition {
        name: format!("Lifecycle {index}"),
        terms: vec![Term { search: format!("cid:{}", h.cards[index].0), limit: 1, order: 5 }],
        reschedule,
        ..Definition::default()
    }
}

fn start(h: &Harness, deck: DeckId) -> (meguri_engine::SessionId, Head) {
    match h.review.start(deck, None).unwrap() {
        StartResult::Started { session_id, head, .. } => (session_id, head),
        other => panic!("start refused: {other:?}"),
    }
}

fn assert_finished(head: &Head) {
    assert!(head.items.is_empty());
    assert_eq!(head.counts, meguri_sched::queue::Counts::default());
    assert_eq!(head.terminal_state, ReviewTerminalState::Finished);
}

#[test]
fn delayed_empty_move_or_delete_never_projects_or_admits_a_home_card_in_filtered_session() {
    for reschedule in [false, true] {
        for operation in ["empty", "move", "delete"] {
            for head_first in [false, true] {
                let h = Harness::new(1);
                let deck = filtered::save(&h.collection, None, definition(&h, 0, reschedule), false)
                    .unwrap().value;
                let (session, head) = start(&h, deck);
                let shown = &head.items[0];
                let card = shown.card_id;
                h.settle();
                let gate = common::Gate::new();
                let held = gate.clone();
                h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
                    if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
                })));
                match operation {
                    "empty" => { filtered::empty(&h.collection, deck).unwrap(); }
                    "move" => {
                        change_deck::run(&h.collection, &CardTarget::Cards(vec![card]),
                            &DeckTarget::Deck(DeckId(1)), CancelToken::new(), ProgressSink::new()).unwrap();
                    }
                    _ => {
                        h.collection.run(OpSpec::named("delete selected filtered deck", OpSource::UserUndoable),
                            move |ctx| meguri_engine::decks::delete_in(ctx.scope(), &[deck])).unwrap();
                        assert!(h.collection.reader(ReaderRole::Query).unwrap().deck(deck).unwrap().is_none());
                    }
                }
                gate.wait_entered();
                let home = h.collection.reader(ReaderRole::Query).unwrap().card(card).unwrap().unwrap();
                // On the buggy read path this poll mints a fresh ordinary token;
                // try that exact token rather than only retrying the old one.
                let polled = head_first.then(|| h.review.head(session, 3));
                let token = polled.as_ref().and_then(|r| r.as_ref().ok())
                    .and_then(|h| h.items.first()).map_or(shown.state_token, |h| h.state_token);
                let outcome = h.review.answer(session, card, Rating::Easy, token,
                    EpochMillis(h.clock.now_ms()), 100);
                h.review.observe_hook_for_testing(None);
                gate.open();

                assert_eq!((home.deck_id, home.original_deck_id), (DeckId(1), DeckId(0)));
                if let Some(polled) = polled { assert_finished(&polled.unwrap()); }
                match outcome.unwrap() {
                    AnswerOutcome::NotAtHead { fresh_head } | AnswerOutcome::Stale { fresh_head, .. } => {
                        assert_finished(&fresh_head);
                    }
                    other => panic!("home card accepted: {operation} head_first={head_first} reschedule={reschedule}: {other:?}"),
                }
                h.settle();
                assert!(h.logs().is_empty(), "wrong-deck answer wrote a review log");
                assert_eq!(h.collection.reader(ReaderRole::Query).unwrap()
                    .card(card).unwrap().unwrap().to_cells(), home.to_cells());
                assert_finished(&h.review.head(session, 3).unwrap());
                h.review.end(session).unwrap();
            }
        }
    }
}

#[test]
fn delayed_empty_or_move_clears_presented_and_waiting_preview_repeats() {
    for (empty, delay) in [(false, 60), (false, 3600), (true, 60), (true, 3600)] {
        let h = Harness::new(1);
        let mut def = definition(&h, 0, false);
        def.again_seconds = delay;
        let deck = filtered::save(&h.collection, None, def, false).unwrap().value;
        let (session, head) = start(&h, deck);
        let shown = &head.items[0];
        assert!(matches!(h.review.answer(session, shown.card_id, Rating::Again, shown.state_token,
            EpochMillis(h.clock.now_ms()), 100).unwrap(), AnswerOutcome::Accepted { .. }));
        h.settle();
        let repeat = h.review.head(session, 3).unwrap();
        assert_eq!(h.logs().len(), 1);
        if delay == 3600 {
            assert!(matches!(repeat.terminal_state, ReviewTerminalState::WaitingUntil { .. }));
        } else {
            assert_eq!(repeat.items[0].card_id, shown.card_id);
        }
        let gate = common::Gate::new();
        let held = gate.clone();
        let card = shown.card_id;
        h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
            if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
        })));
        if empty {
            filtered::empty(&h.collection, deck).unwrap();
        } else {
            change_deck::run(&h.collection, &CardTarget::Cards(vec![card]),
                &DeckTarget::Deck(DeckId(1)), CancelToken::new(), ProgressSink::new()).unwrap();
        }
        gate.wait_entered();
        let polled = h.review.head(session, 3);
        let outcome = h.review.answer(session, card, Rating::Easy, shown.state_token,
            EpochMillis(h.clock.now_ms()), 100);
        h.review.observe_hook_for_testing(None);
        gate.open();
        assert_finished(&polled.unwrap());
        assert!(matches!(outcome.unwrap(), AnswerOutcome::NotAtHead { .. }));
        h.settle();
        assert_eq!(h.logs().len(), 1, "removing membership must leave only the prior preview log");
        assert_finished(&h.review.head(session, 3).unwrap());
        h.review.end(session).unwrap();
    }
}

#[test]
fn delayed_empty_does_not_reinsert_a_pending_preview_overlay() {
    let h = Harness::new(1);
    let deck = filtered::save(&h.collection, None, definition(&h, 0, false), false).unwrap().value;
    let (session, head) = start(&h, deck);
    h.settle();
    let gate = common::Gate::new();
    let held = gate.clone();
    let card = head.items[0].card_id;
    h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
        if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
    })));
    // Occupy only the observer worker, leaving the writer free to Empty after
    // accepting Again but before that pending answer can reach the writer.
    h.collection.run(OpSpec::named("hold review observer", OpSource::Maintenance)
        .stamp(StampMode::Raw), move |ctx| {
            let mut row = ctx.scope().card(card)?.unwrap();
            row.flags = 3;
            ctx.scope().put(&row)?;
            Ok(())
        }).unwrap();
    gate.wait_entered();
    let pending = h.review.answer(session, card, Rating::Again, head.items[0].state_token,
        EpochMillis(h.clock.now_ms()), 100);
    filtered::empty(&h.collection, deck).unwrap();
    let polled = h.review.head(session, 3);
    let outcome = h.review.answer(session, card, Rating::Easy, head.items[0].state_token,
        EpochMillis(h.clock.now_ms()), 100);
    h.review.observe_hook_for_testing(None);
    gate.open();
    assert!(matches!(pending.unwrap(), AnswerOutcome::Accepted { .. }));
    assert_finished(&polled.unwrap());
    assert!(matches!(outcome.unwrap(), AnswerOutcome::NotAtHead { .. }));
    h.settle();
    assert!(h.logs().is_empty());
    assert_finished(&h.review.head(session, 3).unwrap());
    h.review.end(session).unwrap();
}

#[test]
fn same_second_real_lifecycle_refuses_old_showing_before_observer_and_fresh_head_accepts() {
    for reschedule in [false, true] {
        for operation in ["save", "rebuild", "empty-rebuild"] {
            let h = Harness::new(1);
            let def = definition(&h, 0, reschedule);
            let deck = filtered::save(&h.collection, None, def.clone(), false).unwrap().value;
            let (session, head) = start(&h, deck);
            let shown = head.items[0].clone();
            let before = h.collection.reader(ReaderRole::Query).unwrap()
                .card(shown.card_id).unwrap().unwrap().to_cells();
            let now = h.clock.now_ms();
            h.settle();

            // Hold the asynchronous review observer, not the writer. Admission
            // must see the actual lifecycle commit even before queue rebuilding.
            let gate = common::Gate::new();
            let held = gate.clone();
            let card = shown.card_id;
            h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
                if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
            })));
            match operation {
                "save" => { filtered::save(&h.collection, Some(deck), def, false).unwrap(); }
                "rebuild" => { filtered::rebuild(&h.collection, deck).unwrap(); }
                _ => {
                    filtered::empty(&h.collection, deck).unwrap();
                    filtered::rebuild(&h.collection, deck).unwrap();
                }
            }
            gate.wait_entered();
            assert_eq!(h.clock.now_ms(), now);
            assert_eq!(h.collection.reader(ReaderRole::Query).unwrap()
                .card(card).unwrap().unwrap().to_cells(), before, "{operation} {reschedule}");

            let outcome = h.review.answer(session, card, Rating::Easy, shown.state_token,
                EpochMillis(now), 100).unwrap();
            h.review.observe_hook_for_testing(None);
            gate.open();
            let AnswerOutcome::Stale { fresh_head, .. } = outcome else {
                panic!("old showing accepted after {operation} {reschedule}: {outcome:?}");
            };
            assert!(h.logs().is_empty());
            let fresh = &fresh_head.items[0];
            assert_eq!(fresh.card_id, card);
            assert_ne!(fresh.state_token, shown.state_token);
            assert!(matches!(h.review.answer(session, card, Rating::Easy, fresh.state_token,
                EpochMillis(now), 100).unwrap(), AnswerOutcome::Accepted { .. }));
            h.settle();
            assert_eq!(h.logs().len(), 1);
            h.review.end(session).unwrap();
        }
    }
}

#[test]
fn unrelated_lifecycle_metadata_and_failed_build_keep_filtered_showing_answerable() {
    let h = Harness::new(2);
    let def = definition(&h, 0, false);
    let deck = filtered::save(&h.collection, None, def.clone(), false).unwrap().value;
    let other = filtered::save(&h.collection, None, definition(&h, 1, false), false).unwrap().value;
    let (session, head) = start(&h, deck);
    let shown = head.items[0].clone();
    filtered::rebuild(&h.collection, other).unwrap();
    let mut failed = def;
    failed.terms[0].search = "cid:1".into();
    assert!(filtered::save(&h.collection, Some(deck), failed, false).is_err());
    let card = shown.card_id;
    h.collection.run(OpSpec::named("unrelated review metadata", OpSource::Maintenance)
        .stamp(StampMode::Raw), move |ctx| {
            let mut row = ctx.scope().card(card)?.unwrap();
            row.flags = 3;
            row.usn = Usn(12);
            ctx.scope().put(&row)?;
            let mut note = ctx.scope().note(row.note_id)?.unwrap();
            note.fields.push_str(" edited");
            ctx.scope().put(&note)?;
            Ok(())
        }).unwrap();
    assert_eq!(h.review.head(session, 3).unwrap().items[0].state_token, shown.state_token);
    assert!(matches!(h.review.answer(session, card, Rating::Easy, shown.state_token,
        EpochMillis(h.clock.now_ms()), 100).unwrap(), AnswerOutcome::Accepted { .. }));
    h.settle();
    assert_eq!(h.logs().len(), 1);
    h.review.end(session).unwrap();
}

#[test]
fn lifecycle_revision_allows_optimistic_filtered_answer_chain_behind_writer() {
    let h = Harness::new(1);
    let deck = filtered::save(&h.collection, None, definition(&h, 0, false), false).unwrap().value;
    let (session, head) = start(&h, deck);
    h.settle();
    let gate = common::Gate::new();
    let held = gate.clone();
    let blocked = h.collection.submit(
        OpSpec::new(meguri_engine::executor::catalog::BULK_OP, OpSource::Maintenance)
            .declare_long_write(std::time::Duration::from_secs(3)),
        move |_| { held.enter_and_wait(); Ok(()) },
    );
    gate.wait_entered();
    let shown = &head.items[0];
    let first = h.review.answer(session, shown.card_id, Rating::Again, shown.state_token,
        EpochMillis(h.clock.now_ms()), 100).unwrap();
    let AnswerOutcome::Accepted { head_diff, .. } = first else {
        gate.open();
        panic!("first optimistic answer refused: {first:?}");
    };
    let polled = h.review.head(session, 3).unwrap();
    assert_eq!(polled, head_diff.head);
    let next = &polled.items[0];
    assert_eq!(next.card_id, shown.card_id);
    let second = h.review.answer(session, next.card_id, Rating::Easy, next.state_token,
        EpochMillis(h.clock.now_ms()), 100).unwrap();
    let logs_before_commit = h.logs();
    gate.open();
    blocked.wait().unwrap();
    assert!(matches!(second, AnswerOutcome::Accepted { .. }));
    assert!(logs_before_commit.is_empty());
    h.settle();
    assert_eq!(h.logs().len(), 2);
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap()
        .card(shown.card_id).unwrap().unwrap().original_deck_id, DeckId(0));
    h.review.end(session).unwrap();
}
