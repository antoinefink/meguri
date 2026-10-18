mod common;
use meguri_model::{
    card::{CardQueue, CardType},
    ids::CardId,
};
use meguri_sched::states::learning::current;

#[test]
fn intraday_elapsed_reconstructs_the_previous_integer_fuzz_with_u32_count_wrap() {
    let now = 1_894_701_600;
    for (id, reps, minutes, ahead, elapsed) in [
        (1894701600000, 1, 0.05, 0, 3),
        (1894701600002, 1, 0.5, 0, 33),
        (1894701600003, 0, 0.5, 0, 31),
        (1894701600009, 1, 0.5, 100, 4294967229u32),
        (1894701600007, 3, 120.0, 0, 7360),
    ] {
        let mut card = common::card();
        card.id = CardId(id);
        card.card_type = CardType::Learning;
        card.queue = CardQueue::Learning;
        card.reps = reps;
        card.remaining_steps = 1001;
        card.due = now as i32 + ahead;
        let state = current(&card, &[minutes], 0, now, None);
        assert_eq!(state.remaining_steps, 1);
        assert_eq!(state.elapsed_secs, elapsed);
        assert!(state.memory_state.is_none());
    }
}

#[test]
fn interday_elapsed_uses_wrapping_signed_day_arithmetic_and_inactive_queues_report_zero() {
    let mut card = common::card();
    card.card_type = CardType::Learning;
    card.queue = CardQueue::InterdayLearning;
    card.remaining_steps = 1;
    for (minutes, due, elapsed) in [
        (1440.0, 50, 86400),
        (1440.0, 48, 259200),
        (1440.0, 52, 4294880896u32),
        (4320.0, 50, 259200),
    ] {
        card.due = due;
        let state = current(&card, &[minutes], 50, 1_894_701_600, None);
        assert_eq!(state.elapsed_secs, elapsed);
    }
    card.queue = CardQueue::BuriedByScheduler;
    assert_eq!(current(&card, &[10.0], 50, 1_894_701_600, None).elapsed_secs, 0);
}
