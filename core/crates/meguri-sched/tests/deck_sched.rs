use meguri_model::{
    card::CardQueue,
    proto::deck::{DayLimit, DeckCommon, NormalDeck},
};
use meguri_sched::model::deck_sched::{DeckSched, clear_today_limit};

#[test]
fn limits_and_counters_are_day_sensitive_without_rewriting_the_source() {
    let mut deck = NormalDeck::default();
    deck.new_limit = Some(5);
    deck.new_limit_today = Some(DayLimit::new(50, 12));
    deck.desired_retention = Some(0.5);
    let mut common = DeckCommon::default();
    common.last_day_studied = 12;
    common.new_studied = -3;
    common.review_studied = 2;
    let view = DeckSched::new(Some(&deck), &common);
    assert_eq!(view.new_limit(12, 20), Some(50));
    assert_eq!(view.new_limit(13, 20), Some(5));
    assert_eq!(view.retention(0.9), 0.5);
    assert_eq!(view.studied(12), (-3, 2));
    assert_eq!(view.studied(13), (0, 0));
    assert_eq!(common.last_day_studied, 12);
    assert_eq!(DeckSched::new(None, &common).new_limit(12, 20), None);
}

#[test]
fn clearing_today_limits_keeps_expired_values_but_removes_zero_and_future_days() {
    for (day, expected) in [(12, Some(11)), (9, Some(9)), (0, None), (14, None)] {
        let mut limit = Some(DayLimit::new(50, day));
        clear_today_limit(&mut limit, 12);
        assert_eq!(limit.as_ref().map(|v| v.today), expected);
        if let Some(limit) = limit {
            assert_eq!(limit.limit, 50);
        }
    }
    let mut none = None;
    clear_today_limit(&mut none, 0);
    assert!(none.is_none());
}

#[test]
fn answers_reset_all_counters_lazily_and_cap_time_before_incrementing() {
    let mut common = DeckCommon::default();
    common.last_day_studied = 12;
    common.learning_studied = 5;
    for (queue, time) in [
        (CardQueue::New, 5000),
        (CardQueue::Review, 7000),
        (CardQueue::Learning, 100000),
        (CardQueue::InterdayLearning, 1000),
    ] {
        DeckSched::record_answer(&mut common, 12, queue, time, 60);
    }
    assert_eq!(
        (
            common.new_studied,
            common.review_studied,
            common.learning_studied,
            common.milliseconds_studied
        ),
        (1, 2, 5, 73000)
    );
    DeckSched::record_answer(&mut common, 13, CardQueue::New, 2000, 60);
    assert_eq!(
        (
            common.new_studied,
            common.review_studied,
            common.learning_studied,
            common.milliseconds_studied
        ),
        (1, 0, 0, 2000)
    );
}
