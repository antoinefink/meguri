mod common;
use meguri_model::{
    deck_config::DeckConfig,
    ids::DeckConfigId,
    proto::{ProtoBlob, deck_config::DeckConfigMessage},
};
use meguri_sched::{
    model::preset::PresetView,
    states::{
        engine::{State, StateInput, next_states},
        fsrs_glue::FsrsCache,
    },
};
fn preset() -> PresetView {
    preset_with(|_| {})
}
fn preset_with(edit: impl FnOnce(&mut DeckConfigMessage)) -> PresetView {
    let mut fields = DeckConfigMessage::builtin_defaults();
    edit(&mut fields);
    PresetView::new(&DeckConfig {
        id: DeckConfigId(1),
        name: String::new(),
        mtime: Default::default(),
        usn: Default::default(),
        config: ProtoBlob::new(fields),
    })
}
#[test]
fn default_new_card_uses_steps_and_graduates_on_easy() {
    let card = common::card();
    let preset = preset();
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: None,
        last_review_time: None,
        desired_retention: 0.9,
        today: 0,
        next_day_at: 86400,
        now: 0,
        short_term_with_steps: false,
        learn_ahead: 1200,
    };
    let states = next_states(&mut FsrsCache::default(), &input, None).unwrap();
    assert_eq!(states.current, State::New { position: 1 });
    assert!(matches!(states.again.state, State::Learning(_)));
    assert!(matches!(states.easy.state, State::Review(_)));
    assert_eq!(states.again.label.text(), "<1m");
    assert_eq!(states.again.factor, 0);
    assert_eq!(states.again.revlog.last_interval, 0);
}

#[test]
fn elapsed_argument_keeps_the_pinned_wrapping_conversions() {
    use meguri_sched::states::engine::elapsed_days;
    assert_eq!(elapsed_days(86400, Some(0)), 1);
    assert_eq!(elapsed_days(86400, Some(1)), 0);
    assert_eq!(elapsed_days(0, None), 0);
    assert_eq!(elapsed_days(0, Some(1)), 0);
    assert_eq!(elapsed_days((u32::MAX as i64 + 4) * 86400, Some(0)), 3);
    assert_eq!(elapsed_days(i64::MAX, Some(-1)), 0);
}

#[test]
fn resolved_absent_memory_and_imported_interval_wraps_do_not_refuse_or_panic() {
    use meguri_model::card::{CardQueue, CardType};
    use meguri_sched::states::engine::StateError;
    let mut card = common::card();
    card.card_type = CardType::Review;
    card.queue = CardQueue::Review;
    card.interval = u32::MAX;
    card.due = -2;
    card.ease_factor = 2500;
    let preset = preset();
    let mut cache = FsrsCache::default();
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: None,
        last_review_time: None,
        desired_retention: 0.9,
        today: 0,
        next_day_at: 86400,
        now: 0,
        short_term_with_steps: false,
        learn_ahead: 1200,
    };
    let states = next_states(&mut cache, &input, None).unwrap();
    let State::Review(current) = states.current else {
        panic!("review")
    };
    assert_eq!(current.elapsed_days, 1);
    assert_eq!(current.memory_state, None);
    assert_eq!(states.good.revlog.last_interval, -1);
    assert_eq!(states.good.revlog.kind, 1);
    assert!(states.good.state.memory_state().is_some());
    let mut filtered = card.clone();
    filtered.original_deck_id.0 = 1;
    let input = StateInput {
        card: &filtered,
        ..input
    };
    assert!(matches!(
        next_states(&mut cache, &input, None),
        Err(StateError::InFilteredDeck)
    ));
}

#[test]
fn chooser_only_replaces_day_picks_and_previews_use_the_same_answers() {
    use meguri_sched::states::interval_limits::{Delay, FuzzedDayChooser};
    struct Upper(Vec<(f32, u32, u32)>);
    impl FuzzedDayChooser for Upper {
        fn choose(&mut self, x: f32, min: u32, max: u32) -> Option<u32> {
            self.0.push((x, min, max));
            Some(max)
        }
    }
    let card = common::card();
    let preset = preset();
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: None,
        last_review_time: None,
        desired_retention: 0.9,
        today: 0,
        next_day_at: 86400,
        now: 0,
        short_term_with_steps: false,
        learn_ahead: 1200,
    };
    let mut chooser = Upper(vec![]);
    let states = next_states(&mut FsrsCache::default(), &input, Some(&mut chooser)).unwrap();
    assert_eq!(chooser.0.len(), 2); // Only Easy leaves the two default learning steps: Good prerequisite, then Easy.
    assert_eq!(states.easy.delay, Delay::Days(36500));
    assert_eq!(states.easy.revlog.interval, 36500);
    assert_eq!(states.easy.label.text(), "100y");
    assert_eq!(states.easy.factor, 2500);
    assert_eq!(states.again.delay, Delay::Seconds(60));
    assert_eq!(states.again.left, 2);
    assert_eq!(states.again.revlog.interval, -60);
}

#[test]
fn new_cards_use_the_full_virtual_step_count_and_ignore_imported_memory() {
    use meguri_model::card_data::MemoryState;
    let card = common::card();
    let preset = preset_with(|p| p.learn_steps = vec![1.0; 1001]);
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: Some(MemoryState {
            stability: f32::NAN,
            difficulty: f32::NAN,
        }),
        last_review_time: None,
        desired_retention: 0.9,
        today: 0,
        next_day_at: 86400,
        now: 0,
        short_term_with_steps: false,
        learn_ahead: 0,
    };
    let states = next_states(&mut FsrsCache::default(), &input, None).unwrap();
    assert_eq!(states.again.left, 1001);
    assert_eq!(states.hard.left, 1001);
    assert_eq!(states.good.left, 1000);
    assert_eq!(states.current.memory_state(), None);
}

#[test]
fn review_previews_keep_ease_leech_and_difficulty_separate() {
    use meguri_model::{
        card::{CardQueue, CardType},
        card_data::MemoryState,
    };
    use meguri_sched::states::{engine::StateError, interval_limits::Delay};
    let mut card = common::card();
    card.card_type = CardType::Review;
    card.queue = CardQueue::Review;
    card.interval = 10;
    card.ease_factor = 1400;
    card.lapses = 7;
    let preset = preset();
    let input = StateInput {
        card: &card,
        preset: &preset,
        memory_state: Some(MemoryState {
            stability: 10.0,
            difficulty: 5.0,
        }),
        last_review_time: Some(0),
        desired_retention: 0.9,
        today: 9,
        next_day_at: 10 * 86400,
        now: 9 * 86400,
        short_term_with_steps: false,
        learn_ahead: 1200,
    };
    let states = next_states(&mut FsrsCache::default(), &input, None).unwrap();
    assert_eq!(
        [
            states.again.factor,
            states.hard.factor,
            states.good.factor,
            states.easy.factor
        ],
        [1300, 1300, 1400, 1550]
    );
    let State::Relearning { review, .. } = states.again.state else {
        panic!("relearning")
    };
    assert!(review.leeched);
    assert_eq!(review.lapses, 8);
    assert_eq!(states.again.revlog.interval, -600);
    assert_eq!(states.again.revlog.last_interval, 10);
    assert_eq!(states.again.revlog.kind, 1);
    let difficulty = states.good.state.memory_state().unwrap().difficulty;
    assert_eq!(
        states.good.revlog.factor,
        (1000.0 * ((difficulty - 1.0) / 9.0 + 0.1)).round() as u32
    );
    let Delay::Days(days) = states.good.delay else {
        panic!("days")
    };
    assert_eq!(states.good.revlog.interval, days as i32);
    let bad = preset_with(|p| p.set_fsrs_params_6(vec![0.5; 34]));
    let input = StateInput { preset: &bad, ..input };
    assert!(matches!(
        next_states(&mut FsrsCache::default(), &input, None),
        Err(StateError::UnsupportedParams)
    ));
}
