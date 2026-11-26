//! U1 writer operations crossing production U2 shown-card admission.
mod common;
mod session_support;

use meguri_engine::{
    OpSource, OpSpec,
    decks::filtered::{self, Definition, Term},
    session::{AnswerOutcome, Head, StartResult},
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
    let next = &head_diff.head.items[0];
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
