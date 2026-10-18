//! FSRS answer states of a review card (specs/sched/fsrs-transitions.md
//! T1-T6).

use fsrs::FSRS;
use meguri_model::card_data::MemoryState;
use meguri_sched::states::fuzz;
use meguri_sched::states::review::{self, ReviewCard};

/// T6 (oracle, conformance/goldens/smoke/fsrs-new-card.json step 6).
fn t6_card() -> ReviewCard {
    ReviewCard {
        memory_state: MemoryState {
            stability: 2.3065,
            difficulty: 2.111,
        },
        days_elapsed: 3,
        interval: 2,
        desired_retention: 0.9,
        maximum_interval: 36500,
        fuzz_seed: fuzz::seed(1_894_698_000_000, 2),
    }
}

#[test]
fn a_review_card_gets_the_oracle_answer_states_of_t6() {
    let model = FSRS::new(&[]).unwrap();
    let a = review::answers(&model, &t6_card()).unwrap();
    let row = |x: review::Answer| {
        (
            x.days,
            x.memory_state.stability.to_bits(),
            x.memory_state.difficulty.to_bits(),
        )
    };
    let bits = |days: u32, s: f32, d: f32| (days, s.to_bits(), d.to_bits());
    assert_eq!(row(a.again), bits(1, 0.6369821, 7.3921676));
    assert_eq!(row(a.hard), bits(10, 9.240417, 4.7481427));
    assert_eq!(row(a.good), bits(15, 13.836124, 2.1041174));
    assert_eq!(row(a.easy), bits(26, 23.900333, 1.0));
    // At desired retention 0.9 the unrounded intervals are the new
    // stabilities (T6), so fuzz moved Hard, Good and Easy up.
    for x in [a.hard, a.good, a.easy] {
        assert!(
            (x.unrounded - x.memory_state.stability).abs() <= 1e-3 * x.memory_state.stability,
            "{x:?}"
        );
    }
}

#[test]
fn good_lands_above_hard_and_easy_above_good_below_the_maximum() {
    let model = FSRS::new(&[]).unwrap();
    for (s, d, elapsed, interval) in [
        (0.5, 9.0, 0, 1),
        (3.0, 5.0, 3, 3),
        (40.0, 2.0, 50, 40),
        (400.0, 8.0, 0, 400),
    ] {
        for reps in 0..40 {
            let card = ReviewCard {
                memory_state: MemoryState {
                    stability: s,
                    difficulty: d,
                },
                days_elapsed: elapsed,
                interval,
                desired_retention: 0.9,
                maximum_interval: 36500,
                fuzz_seed: fuzz::seed(1_700_000_000_000, reps),
            };
            let a = review::answers(&model, &card).unwrap();
            assert!(a.again.days >= 1);
            assert!(a.hard.days >= 1, "{a:?}");
            assert!(a.good.days > a.hard.days, "{a:?}");
            assert!(a.easy.days > a.good.days, "{a:?}");
        }
    }
}

#[test]
fn the_maximum_interval_holds_hard_good_and_easy_but_not_again() {
    let model = FSRS::new(&[]).unwrap();
    let card = ReviewCard {
        memory_state: MemoryState {
            stability: 100.0,
            difficulty: 5.0,
        },
        days_elapsed: 100,
        interval: 100,
        maximum_interval: 1,
        ..t6_card()
    };
    let a = review::answers(&model, &card).unwrap();
    assert_eq!((a.hard.days, a.good.days, a.easy.days), (1, 1, 1));
    // T4: Again's review part has no maximum.
    assert_eq!(a.again.days, a.again.unrounded.round().max(1.0) as u32);
}
