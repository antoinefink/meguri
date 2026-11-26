//! Multi-answer C8 production sessions, with every rating probed through U44.
mod common;
mod session_support;
use meguri_engine::{
    UndoScope,
    session::{AnswerOutcome, Head},
};
use meguri_model::{clock::Clock, time::EpochMillis};
use meguri_sched::answer::Rating;
use session_support::Harness;
fn choose(h: &Harness, id: meguri_engine::SessionId, head: &Head, rating: Rating) -> Head {
    let item = &head.items[0];
    let expected = item.predicted_next[rating as usize - 1];
    let AnswerOutcome::Accepted { head_diff, .. } = h
        .review
        .answer(
            id,
            item.card_id,
            rating,
            item.state_token,
            EpochMillis(h.clock.now_ms()),
            200,
        )
        .unwrap()
    else {
        panic!("prediction answer rejected")
    };
    assert_eq!(head_diff.head.items.first().map(|h| h.card_id), expected);
    h.settle();
    assert_eq!(
        h.review.head(id, 3).unwrap().items.first().map(|h| h.card_id),
        expected,
        "real U44 commit must retain optimistic prediction"
    );
    head_diff.head
}
#[test]
fn c8_all_four_actual_choices_in_ten_thousand_random_multi_answer_sessions() {
    let mut tasks = Vec::with_capacity(4);
    let mut launch_error = None;
    for (begin, end) in [(0, 2500), (2500, 5000), (5000, 7500), (7500, 10000)] {
        let launched = std::thread::Builder::new()
            .name(format!("c8-sessions-{begin}-{end}"))
            .spawn(move || run_session_range(begin..end));
        match launched {
            Ok(task) => tasks.push(task),
            Err(cause) => {
                launch_error = Some(cause);
                break;
            }
        }
    }

    // Collect every result before allowing a thread failure to escape.
    let mut first_panic = None;
    for task in tasks {
        let outcome = task.join();
        if first_panic.is_none() {
            first_panic = outcome.err();
        }
    }
    if let Some(payload) = first_panic {
        std::panic::resume_unwind(payload);
    }
    if let Some(cause) = launch_error {
        panic!("could not launch C8 session range: {cause}");
    }
}
fn run_session_range(range: std::ops::Range<usize>) {
    // Construct the collection and its service within the owning test thread.
    let h = Harness::new(18);
    let mut random = 0x4d65_6775_7269_u64;
    let mut preceding_draws = range.start * 12;
    while preceding_draws != 0 {
        random ^= random << 13;
        random ^= random >> 7;
        random ^= random << 17;
        preceding_draws -= 1;
    }
    for _session in range {
        h.clock.set_now_ms(session_support::NOW);
        let (id, _) = h.start();
        let mut steps = 0;
        while steps < 12 {
            let head = h.review.head(id, 3).unwrap();
            if head.items.is_empty() {
                break;
            }
            // Each counterfactual executes the actual production answer and
            // committed U25 undo, so this is more than a one-step pure test.
            for rating in [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy] {
                let before = h.review.head(id, 3).unwrap();
                choose(&h, id, &before, rating);
                let restored = h.review.undo(UndoScope::Session(id)).unwrap();
                assert_eq!(restored.items[0].card_id, before.items[0].card_id);
                // The presentation returns before U25; settle the real undo
                // before the next counterfactual uses its committed row.
                h.settle();
            }
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let rating = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy][random as usize % 4];
            choose(&h, id, &h.review.head(id, 3).unwrap(), rating);
            h.clock.step_ms(((random >> 8) % 80 + 1) as i64 * 1000);
            steps += 1;
        }
        // Eighteen single-card notes, no siblings: at most twelve chosen
        // answers can graduate cards. Counterfactuals are committed-undone.
        assert_eq!(steps, 12, "every random session must consume twelve draws");
        h.review.end(id).unwrap();
        // Reset through the real operation journals, never rewrite expected
        // states from a case id or bypass the service with an answer adapter.
        while h.collection.peek_undo(UndoScope::Global).unwrap().name().is_some() {
            h.collection.undo(UndoScope::Global).unwrap();
        }
    }
}
#[test]
fn c9_rapid_same_card_postimages_commit_in_order_without_extra_cache_contribution() {
    let h = Harness::new(1);
    let (id, mut head) = h.start();
    let card = head.items[0].card_id;
    for _ in 0..8 {
        let AnswerOutcome::Accepted { head_diff, .. } = h
            .review
            .answer(
                id,
                card,
                Rating::Again,
                head.items[0].state_token,
                EpochMillis(h.clock.now_ms()),
                10,
            )
            .unwrap()
        else {
            panic!("same-card answer rejected")
        };
        assert_eq!(head_diff.head.items[0].card_id, card);
        head = head_diff.head;
    }
    h.settle();
    assert_eq!(h.logs().len(), 8);
    use meguri_storage::{ReadRows, ReaderRole};
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .card(card)
            .unwrap()
            .unwrap()
            .reps,
        8
    );
}

#[test]
fn malformed_membership_refuses_session_prediction_and_answer_without_journal_or_write() {
    use meguri_engine::{OpSource, OpSpec, decks::filtered::{self, Definition, Term}, sched::queue::QueueProvider};
    use meguri_model::ids::DeckId;
    use meguri_sched::states::fsrs_glue::FsrsCache;
    use meguri_storage::{ReadRows, ReaderRole, Record, RowKey, StampMode, Table};
    use std::{fs, sync::Arc};

    for mode in [None, Some(false), Some(true)] {
        for head_first in [false, true] {
            let h = Harness::new(1);
            let card = h.cards[0];
            let deck = if let Some(reschedule) = mode {
                filtered::save(&h.collection, None, Definition {
                    name: "Malformed membership".into(),
                    terms: vec![Term { search: format!("cid:{}", card.0), limit: 1, order: 5 }],
                    reschedule,
                    ..Definition::default()
                }, false).unwrap().value
            } else {
                DeckId(1)
            };
            let meguri_engine::session::StartResult::Started { session_id, head, .. } =
                h.review.start(deck, None).unwrap() else { panic!("valid fixture refused") };
            let shown = &head.items[0];
            h.settle();

            // Delay only the observer: admission must inspect current storage,
            // even while the old valid card remains in the shown queue.
            let gate = common::Gate::new();
            let held = gate.clone();
            h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
                if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
            })));
            h.collection.run(OpSpec::named("malformed session membership", OpSource::Maintenance)
                .stamp(StampMode::Raw), move |ctx| {
                    // Match U44's negative-home fixture, including an existing
                    // normal row at -1; an unchecked home lookup succeeds.
                    let mut home = ctx.scope().deck(DeckId(1))?.unwrap();
                    home.id = if mode.is_some() { DeckId(-1) } else { DeckId(2) };
                    home.name = "Other home".into();
                    ctx.scope().put(&home)?;
                    let mut row = ctx.scope().card(card)?.unwrap();
                    row.original_deck_id = home.id;
                    ctx.scope().put(&row)?;
                    Ok(())
                }).unwrap();
            gate.wait_entered();
            // Establish D8 before measuring answer-owned writes; the queue
            // label query below uses the normal maintenance prelude.
            h.collection.check_rollover().unwrap();

            let images = || {
                let r = h.collection.reader(ReaderRole::Query).unwrap();
                (r.card(card).unwrap().unwrap().to_cells(),
                 r.deck_rows().unwrap().iter().map(Record::to_cells).collect::<Vec<_>>(),
                 r.row_image(Table::Col, &RowKey::Id(1)).unwrap())
            };
            let before = images();
            let seq = h.collection.last_seq();
            let journal = h.collection.path().parent().unwrap().join("meguri-pending-answers.jsonl");
            let journal_before = fs::read(&journal).unwrap();
            let polled = head_first.then(|| h.review.head(session_id, 3));
            let answer = h.review.answer(session_id, card, Rating::Easy, shown.state_token,
                EpochMillis(h.clock.now_ms()), 100);
            let peek = h.review.peek_first_card(deck);
            let queue = QueueProvider::default().queue_with_head_states(
                &h.collection, Some(deck), &mut FsrsCache::default());
            h.review.observe_hook_for_testing(None);
            gate.open();
            h.settle();

            if let Some(polled) = polled { assert!(polled.is_err(), "{mode:?}"); }
            assert!(answer.is_err(), "malformed answer admitted: {mode:?}: {answer:?}");
            assert!(peek.is_err(), "malformed peek shown: {mode:?}");
            assert!(queue.is_err(), "malformed queue head predicted: {mode:?}");
            assert!(h.logs().is_empty());
            assert_eq!(images(), before);
            assert_eq!(h.collection.last_seq(), seq);
            assert_eq!(fs::read(&journal).unwrap(), journal_before);
            h.review.end(session_id).unwrap();
        }
    }
}

#[test]
fn ordinary_pending_overlay_cannot_hide_changed_stored_membership() {
    use meguri_engine::{OpSource, OpSpec, decks::filtered::{self, Definition, Term}};
    use meguri_model::ids::DeckId;
    use meguri_storage::{ReadRows, ReaderRole, Record, StampMode};
    use std::{fs, sync::Arc};

    for change in ["malformed", "borrowed", "moved"] {
        let h = Harness::new(1);
        let card = h.cards[0];
        // Review a normal descendant: membership is the card's own deck,
        // rather than necessarily the selected session deck.
        h.collection.run(OpSpec::named("descendant membership fixture", OpSource::Maintenance), move |ctx| {
            let mut child = ctx.scope().deck(DeckId(1))?.unwrap();
            child.id = DeckId(2);
            child.name = format!("{}\u{1f}Child", child.name);
            ctx.scope().put(&child)?;
            child.id = DeckId(3);
            child.name = "Other home".into();
            ctx.scope().put(&child)?;
            let mut row = ctx.scope().card(card)?.unwrap();
            row.deck_id = DeckId(2);
            ctx.scope().put(&row)?;
            Ok(())
        }).unwrap();
        let borrowed = filtered::save(&h.collection, None, Definition {
            name: "Borrowed pending card".into(),
            terms: vec![Term { search: "cid:0".into(), limit: 1, order: 5 }],
            ..Definition::default()
        }, true).unwrap().value;
        let (session, head) = h.start();
        h.settle();
        let gate = common::Gate::new();
        let held = gate.clone();
        h.review.observe_hook_for_testing(Some(Arc::new(move |payload| {
            if payload.card_ids().any(|id| id == card) { held.enter_and_wait(); }
        })));
        // Occupy the service worker before it can apply Again, keeping the
        // writer free for the maintenance commit and its delayed observer.
        h.collection.run(OpSpec::named("hold ordinary review observer", OpSource::Maintenance)
            .stamp(StampMode::Raw), move |ctx| {
                let mut row = ctx.scope().card(card)?.unwrap();
                row.flags = 3;
                ctx.scope().put(&row)?;
                Ok(())
            }).unwrap();
        gate.wait_entered();
        let AnswerOutcome::Accepted { head_diff, .. } = h.review.answer(
            session, card, Rating::Again, head.items[0].state_token,
            EpochMillis(h.clock.now_ms()), 100).unwrap()
        else { panic!("valid descendant pending answer refused"); };
        let repeat = &head_diff.head.items[0];
        assert_eq!(repeat.card_id, card);
        h.collection.run(OpSpec::named("change pending card membership", OpSource::Maintenance)
            .stamp(StampMode::Raw), move |ctx| {
                let mut row = ctx.scope().card(card)?.unwrap();
                match change {
                    "malformed" => row.original_deck_id = DeckId(3),
                    "borrowed" => {
                        row.deck_id = borrowed;
                        row.original_deck_id = DeckId(2);
                    }
                    _ => row.deck_id = DeckId(3),
                }
                ctx.scope().put(&row)?;
                Ok(())
            }).unwrap();
        let image = || h.collection.reader(ReaderRole::Query).unwrap()
            .card(card).unwrap().unwrap().to_cells();
        let before = image();
        let seq = h.collection.last_seq();
        let journal = h.dir.path().join("meguri-pending-answers.jsonl");
        // Installing a hook is an ordered file-worker barrier: include the
        // first intent before comparing bytes, without draining its answer.
        h.review.journal_hook_for_testing(Arc::new(|_| {})).unwrap();
        let journal_before = fs::read(&journal).unwrap();
        let polled = h.review.head(session, 3);
        let repeated = h.review.answer(session, card, Rating::Again, repeat.state_token,
            EpochMillis(h.clock.now_ms()), 100);
        h.review.journal_hook_for_testing(Arc::new(|_| {})).unwrap();
        let journal_after = fs::read(&journal).unwrap();
        let after = image();
        let seq_after = h.collection.last_seq();
        h.review.observe_hook_for_testing(None);
        gate.open();
        h.settle();

        assert!(polled.is_err(), "pending prediction hid {change} membership");
        assert!(repeated.is_err(), "second answer admitted: {change}: {repeated:?}");
        assert_eq!(journal_after, journal_before, "{change}");
        assert_eq!(after, before, "{change}");
        assert_eq!(seq_after, seq, "{change}");
        assert!(h.logs().is_empty(), "{change}");
        assert_eq!(image(), before, "{change}");
        h.review.end(session).unwrap();
    }
}
