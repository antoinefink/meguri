//! F3/F4 through production queues, sessions, committed answers and reloads.
mod common;
mod session_support;
use meguri_engine::{Collection, OpSource, OpSpec, SessionId, UndoScope,
    sched::{answer, queue::QueueProvider},
    session::{AnswerOutcome, Head, ReviewService, ReviewTerminalState, StartResult}};
use meguri_model::{card::{Card, CardQueue, CardType}, card_data::MemoryState, clock::Clock,
    ids::{CardId, DeckConfigId, DeckId}, proto::{ProtoBlob, deck::{DeckKind, DeckKindMessage, FilteredDeck}},
    revlog::RevlogKind, time::EpochMillis};
use meguri_sched::{answer::Rating, states::{engine::State, fsrs_glue::FsrsCache}};
use meguri_storage::{ReadRows, ReaderRole, Record};
use session_support::Harness;

fn card(c: &Collection, id: CardId) -> Card {
    c.reader(ReaderRole::Query).unwrap().card(id).unwrap().unwrap()
}
fn start(review: &ReviewService, deck: DeckId) -> (SessionId, Head) {
    match review.start(deck, None).unwrap() {
        StartResult::Started { session_id, head, .. } => (session_id, head),
        other => panic!("filtered session refused: {other:?}"),
    }
}
fn answer_head(review: &ReviewService, session: SessionId, head: &Head, rating: Rating, at: i64) -> Head {
    let item = &head.items[0];
    match review.answer(session, item.card_id, rating, item.state_token, EpochMillis(at), 1234).unwrap() {
        AnswerOutcome::Accepted { head_diff, .. } => head_diff.head,
        other => panic!("filtered answer refused: {other:?}"),
    }
}
fn home_review(h: &Harness, due: i32) -> Card {
    let id = h.cards[0];
    let last_review_secs = (h.clock.now_ms() / 1000) - 7 * 86400;
    h.collection.run(OpSpec::named("home review fixture", OpSource::Maintenance), move |ctx| {
        let mut home = ctx.scope().deck(DeckId(1))?.unwrap();
        let mut kind = (*home.kind).clone();
        let DeckKind::Normal(n) = &mut kind.kind else { unreachable!() };
        n.desired_retention = Some(0.97);
        home.kind = ProtoBlob::new(kind);
        ctx.scope().put(&home)?;
        let mut card = ctx.scope().card(id)?.unwrap();
        card.card_type = CardType::Review;
        card.queue = CardQueue::Review;
        card.interval = 21;
        card.due = due;
        card.ease_factor = 2350;
        card.reps = 8;
        card.lapses = 2;
        card.data.memory_state = Some(MemoryState { stability: 17.0, difficulty: 4.0 });
        card.data.last_review_secs = Some(last_review_secs);
        ctx.scope().put(&card)?;
        Ok(())
    }).unwrap();
    card(&h.collection, id)
}
fn borrow(h: &Harness, reschedule: bool, again: u32) -> DeckId {
    let deck = meguri_engine::decks::add(&h.collection, "Filtered regression".into()).unwrap().value.id;
    let ids = h.cards.clone();
    h.collection.run(OpSpec::named("import filtered membership fixture", OpSource::Maintenance), move |ctx| {
        let mut row = ctx.scope().deck(deck)?.unwrap();
        let mut definition = FilteredDeck::default();
        definition.reschedule = reschedule;
        definition.preview_again_secs = again;
        definition.preview_hard_secs = 600;
        definition.preview_good_secs = 0;
        row.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(definition)));
        ctx.scope().put(&row)?;
        for (index, id) in ids.into_iter().enumerate() {
            let mut card = ctx.scope().card(id)?.unwrap();
            card.original_deck_id = card.deck_id;
            card.original_due = card.due;
            card.deck_id = deck;
            if card.due > 0 { card.due = -100000 + index as i32; }
            if !reschedule { card.queue = CardQueue::Review; }
            ctx.scope().put(&card)?;
        }
        Ok(())
    }).unwrap();
    deck
}
fn assert_schedule(actual: &Card, expected: &Card) {
    let mut normalized = actual.clone();
    normalized.deck_id = expected.deck_id;
    normalized.original_deck_id = expected.original_deck_id;
    normalized.original_due = expected.original_due;
    normalized.queue = expected.queue;
    normalized.due = expected.due;
    normalized.mtime = expected.mtime;
    normalized.usn = expected.usn;
    assert_eq!(normalized.to_cells(), expected.to_cells(), "preview changed home scheduling/history");
}

#[test]
fn preview_repeat_reopens_finishes_and_undo_restores_exact_assignment_and_logs() {
    for due in [0, 103] {
        let h = Harness::new(1);
        let home = home_review(&h, due);
        let deck = borrow(&h, false, 60);
        let borrowed = card(&h.collection, home.id);
        let queues = QueueProvider::default();
        assert!(queues.queue(&h.collection, Some(DeckId(1))).unwrap().value.presentation().is_empty());
        assert_eq!(queues.deck_counts(&h.collection).unwrap().counts(deck).review, 1);
        let (session, head) = start(&h.review, deck);
        assert_eq!(head.items[0].card_id, home.id);
        assert!(matches!(head.items[0].next_states.current, State::Preview { .. }));
        let next = answer_head(&h.review, session, &head, Rating::Again, h.clock.now_ms());
        assert_eq!(next.items[0].card_id, home.id);
        h.settle();
        let repeat = card(&h.collection, home.id);
        assert_eq!((repeat.deck_id, repeat.original_deck_id, repeat.original_due, repeat.queue),
            (deck, DeckId(1), due, CardQueue::Preview));
        assert!(repeat.due >= (h.clock.now_ms() / 1000) as i32 + 60);
        assert_schedule(&repeat, &home);
        {
            let log_id = meguri_model::ids::RevlogId(h.logs()[0]);
            let r = h.collection.reader(ReaderRole::Query).unwrap();
            let log = r.revlog_entry(log_id).unwrap().unwrap();
            assert_eq!((log.kind, log.ease_factor, log.button), (RevlogKind::Filtered, 0, 1));
        }
        // Undo the timed answer too: all stored cells and history return.
        let restored = h.review.undo(UndoScope::Global).unwrap();
        assert_eq!(restored.items[0].card_id, home.id);
        assert_eq!(card(&h.collection, home.id).to_cells(), borrowed.to_cells());
        assert!(h.logs().is_empty());
        answer_head(&h.review, session, &restored, Rating::Again, h.clock.now_ms());
        h.settle();
        let repeat = card(&h.collection, home.id);
        h.review.end(session).unwrap();
        h.engine.close().unwrap();
        let (engine, c, review) = session_support::open(&h.dir.path().join("collection.anki2"), h.clock.clone());
        let (session, head) = start(&review, deck);
        assert_eq!(head.items[0].card_id, home.id, "queue 4 must survive reopening");
        assert_eq!(head.counts.learning, 1);
        let done = answer_head(&review, session, &head, Rating::Easy, h.clock.now_ms());
        assert!(done.items.is_empty());
        assert_eq!(done.terminal_state, ReviewTerminalState::Finished);
        review.flush_for_background().unwrap();
        let finished = card(&c, home.id);
        assert_eq!((finished.deck_id, finished.original_deck_id, finished.original_due, finished.queue, finished.due),
            (DeckId(1), DeckId(0), 0, CardQueue::Review, due));
        assert_schedule(&finished, &home);
        assert_eq!(c.reader(ReaderRole::Query).unwrap().ids_after(meguri_storage::Table::Revlog, None, 10).unwrap().len(), 2);
        let undo = review.undo(UndoScope::Global).unwrap();
        assert_eq!(undo.items[0].card_id, home.id);
        assert_eq!(card(&c, home.id).to_cells(), repeat.to_cells());
        assert_eq!(c.reader(ReaderRole::Query).unwrap().ids_after(meguri_storage::Table::Revlog, None, 10).unwrap().len(), 1);
        answer_head(&review, session, &undo, Rating::Good, h.clock.now_ms());
        review.flush_for_background().unwrap();
        assert_eq!(card(&c, home.id).due, due, "zero Good delay also finishes");
        review.end(session).unwrap();
        engine.close().unwrap();
    }
}

#[test]
fn early_due_and_late_scheduled_answers_use_home_fsrs_then_return_or_relearn() {
    for due in [10, 0, -3] {
        for rating in [Rating::Good, Rating::Again] {
            let h = Harness::new(1);
            let home = home_review(&h, due);
            let normal = answer::predict(&h.collection, home.id, &mut FsrsCache::default(), None).unwrap();
            let normal_chosen = meguri_sched::ops::grade_now::select(&normal.states, rating);
            let deck = borrow(&h, true, 60);
            let (session, head) = start(&h.review, deck);
            let states = &head.items[0].next_states;
            let State::Review(current) = states.current else { panic!("review current") };
            assert_eq!(current.elapsed_days, (21 - due).max(0) as u32);
            let chosen = meguri_sched::ops::grade_now::select(states, rating);
            assert_eq!(chosen.state, normal_chosen.state);
            assert_eq!(chosen.delay, normal_chosen.delay);
            assert_eq!(chosen.revlog.kind, if due > 0 { 3 } else { 1 });
            assert!(chosen.revlog.factor > 0);
            let borrowed = card(&h.collection, home.id);
            answer_head(&h.review, session, &head, rating, h.clock.now_ms());
            h.settle();
            let written = card(&h.collection, home.id);
            assert_eq!(written.reps, home.reps + 1);
            assert_ne!(written.data.last_review_secs, home.data.last_review_secs);
            let log_id = meguri_model::ids::RevlogId(h.logs()[0]);
            let reader = h.collection.reader(ReaderRole::Query).unwrap();
            let log = reader.revlog_entry(log_id).unwrap().unwrap();
            assert_eq!(log.kind, if due > 0 { RevlogKind::Filtered } else { RevlogKind::Review });
            assert_eq!(log.ease_factor, chosen.revlog.factor);
            assert_eq!(reader.deck(deck).unwrap().unwrap().common.review_studied, 1);
            assert_eq!(reader.deck(DeckId(1)).unwrap().unwrap().common.review_studied, 0);
            drop(reader);
            if rating == Rating::Good {
                assert_eq!((written.deck_id, written.original_deck_id, written.original_due, written.queue),
                    (DeckId(1), DeckId(0), 0, CardQueue::Review));
                assert_eq!(written.due, written.interval as i32);
            } else {
                assert_eq!((written.deck_id, written.original_deck_id, written.original_due, written.queue),
                    (deck, DeckId(1), written.due, CardQueue::Learning));
                assert_eq!(written.card_type, CardType::Relearning);
                assert_eq!(written.lapses, home.lapses + 1);
                let next = h.review.head(session, 3).unwrap();
                assert_eq!(next.items[0].card_id, home.id);
                answer_head(&h.review, session, &next, Rating::Easy, h.clock.now_ms());
                h.settle();
                assert_eq!(card(&h.collection, home.id).original_deck_id, DeckId(0));
                h.review.undo(UndoScope::Global).unwrap();
            }
            h.review.undo(UndoScope::Global).unwrap();
            assert_eq!(card(&h.collection, home.id).to_cells(), borrowed.to_cells());
            assert!(h.logs().is_empty());
            h.review.end(session).unwrap();
        }
    }
}

#[test]
fn preview_repeat_outside_learn_ahead_waits_after_reload_then_progresses() {
    let h = Harness::new(1);
    home_review(&h, 103);
    let deck = borrow(&h, false, 3600);
    let (session, head) = start(&h.review, deck);
    let waiting = answer_head(&h.review, session, &head, Rating::Again, h.clock.now_ms());
    assert!(waiting.items.is_empty());
    assert!(matches!(waiting.terminal_state, ReviewTerminalState::WaitingUntil { .. }));
    h.settle();
    let repeat = card(&h.collection, h.cards[0]);
    h.review.end(session).unwrap();
    h.engine.close().unwrap();
    let (engine, c, review) = session_support::open(&h.dir.path().join("collection.anki2"), h.clock.clone());
    let (session, waiting) = start(&review, deck);
    assert!(waiting.items.is_empty());
    let ReviewTerminalState::WaitingUntil { instant } = waiting.terminal_state else { panic!("repeat disappeared on reload") };
    assert!(instant <= i64::from(repeat.due));
    assert_eq!(QueueProvider::default().deck_study_state(&c, Some(deck)).unwrap().next_learning_due, Some(i64::from(repeat.due)));
    h.clock.step_ms((instant - (h.clock.now_ms() / 1000)) * 1000);
    let ready = review.head(session, 3).unwrap();
    assert_eq!(ready.items[0].card_id, h.cards[0]);
    let done = answer_head(&review, session, &ready, Rating::Easy, h.clock.now_ms());
    assert!(done.items.is_empty());
    review.flush_for_background().unwrap();
    assert_eq!(card(&c, h.cards[0]).due, 103);
    review.end(session).unwrap();
    engine.close().unwrap();
}

#[test]
fn filtered_new_positions_ignore_home_limits_and_learning_graduates_normally() {
    let mut h = Harness::new(3);
    h.cards.reverse();
    let first = h.cards[0];
    let saved_position = card(&h.collection, first).due;
    h.collection.run(OpSpec::named("exhaust normal limits", OpSource::Maintenance), |ctx| {
        let mut preset = ctx.scope().deck_config(DeckConfigId(1))?.unwrap();
        let mut config = (*preset.config).clone();
        config.new_per_day = 0;
        config.reviews_per_day = 0;
        preset.config = ProtoBlob::new(config);
        ctx.scope().put(&preset)?;
        Ok(())
    }).unwrap();
    let deck = borrow(&h, true, 60);
    let (session, head) = start(&h.review, deck);
    assert_eq!(head.counts.new, 3);
    assert_eq!(head.items[0].card_id, first);
    assert_eq!(head.items[0].next_states.current, State::New { position: saved_position as u32 });
    let next = answer_head(&h.review, session, &head, Rating::Good, h.clock.now_ms());
    assert_eq!(next.counts.new, 2);
    h.settle();
    let learning = card(&h.collection, first);
    assert_eq!(learning.card_type, CardType::Learning);
    assert_eq!((learning.deck_id, learning.original_deck_id, learning.original_due), (deck, DeckId(1), learning.due));
    assert_eq!(learning.data.new_position, Some(saved_position as u32));
    // Intraday learning has priority only when due; complete the other new cards first.
    let mut head = next;
    for _ in 0..3 {
        if head.items.is_empty() { break; }
        head = answer_head(&h.review, session, &head, Rating::Easy, h.clock.now_ms());
    }
    h.settle();
    assert!(head.items.is_empty());
    for id in &h.cards {
        let returned = card(&h.collection, *id);
        assert_eq!((returned.deck_id, returned.original_deck_id, returned.original_due, returned.queue),
            (DeckId(1), DeckId(0), 0, CardQueue::Review));
    }
    h.review.end(session).unwrap();
}

#[test]
fn changed_membership_or_definition_rejects_the_presented_answer_without_history() {
    for change in ["empty", "rebuild", "definition"] {
        let h = Harness::new(1);
        home_review(&h, 103);
        let deck = borrow(&h, false, 60);
        let (session, head) = start(&h.review, deck);
        let shown = head.items[0].clone();
        let card_id = shown.card_id;
        h.collection.run(OpSpec::named("external filtered operation", OpSource::UserUndoable), move |ctx| {
            if change == "definition" {
                let mut row = ctx.scope().deck(deck)?.unwrap();
                let mut kind = (*row.kind).clone();
                let DeckKind::Filtered(d) = &mut kind.kind else { unreachable!() };
                d.preview_again_secs = 120;
                row.kind = ProtoBlob::new(kind);
                ctx.scope().put(&row)?;
            } else {
                let mut row = ctx.scope().card(card_id)?.unwrap();
                if change == "empty" {
                    meguri_sched::model::card_sched::return_from_filtered(&mut row);
                } else {
                    row.due = -99990;
                    row.original_due = 104;
                }
                ctx.scope().put(&row)?;
            }
            Ok(())
        }).unwrap();
        let before = card(&h.collection, shown.card_id).to_cells();
        let outcome = h.review.answer(session, shown.card_id, Rating::Again, shown.state_token,
            EpochMillis(h.clock.now_ms()), 100).unwrap();
        assert!(matches!(outcome, AnswerOutcome::Stale { .. } | AnswerOutcome::NotAtHead { .. }));
        h.settle();
        assert!(h.logs().is_empty());
        assert_eq!(card(&h.collection, shown.card_id).to_cells(), before);
        h.review.end(session).unwrap();
    }
}
