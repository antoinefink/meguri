//! Multi-answer C8 production sessions, with every rating probed through U44.
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
    let h = Harness::new(18);
    let mut random = 0x4d65_6775_7269_u64;
    for _session in 0..10_000 {
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
            }
            random ^= random << 13;
            random ^= random >> 7;
            random ^= random << 17;
            let rating = [Rating::Again, Rating::Hard, Rating::Good, Rating::Easy][random as usize % 4];
            choose(&h, id, &h.review.head(id, 3).unwrap(), rating);
            h.clock.step_ms(((random >> 8) % 80 + 1) as i64 * 1000);
            steps += 1;
        }
        assert!(steps >= 2, "a random session must exercise progression");
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
