//! Interval fuzz against the examples of specs/sched/fuzz.md: worked values
//! for Z3-Z5 and oracle values for Z6.

use meguri_sched::states::fuzz;

#[test]
fn learning_step_integer_fuzz_matches_recorded_answers_and_previous_answer_seeds() {
    for (id, reps, seconds, expected) in [
        (1894701600000u64, 0, 600, 739),
        (1894698000000, 0, 600, 696),
        (1894698000000, 2, 600, 715),
        (1894701600002, 0, 30, 33),
        (1894701600003, 4294967295, 30, 31),
        (1894701600004, 1, 30, 34),
        (1894701600007, 2, 7200, 7360),
        (1894701600008, 3, 72000, 72267),
    ] {
        assert_eq!(fuzz::learning_delay(seconds, fuzz::seed(id, reps)), expected);
    }
    for seconds in 0..=4 {
        assert_eq!(fuzz::learning_delay(seconds, 42), seconds);
    }
    for seed in 0..1000 {
        assert!((600..750).contains(&fuzz::learning_delay(600, seed)));
        assert_eq!(fuzz::learning_delay(u32::MAX, seed), u32::MAX);
        assert!((u32::MAX - 3..=u32::MAX).contains(&fuzz::learning_delay(u32::MAX - 3, seed)));
    }
}

#[test]
fn fractional_threshold_and_collapsed_range_widening_remain_exact() {
    assert_eq!(fuzz::range(2.5f32.next_down(), 1, 100), (2, 2));
    assert_eq!(fuzz::range(2.5, 1, 100), (2, 4));
    assert_eq!(fuzz::range(3.0, 3, 100), (3, 4));
    assert_eq!(fuzz::range(3.0, 3, 3), (3, 3));
}

#[test]
fn the_seed_is_the_card_id_plus_the_review_count_with_wrap_around() {
    assert_eq!(fuzz::seed(1_894_698_000_000, 2), 1_894_698_000_002);
    assert_eq!(fuzz::seed(u64::MAX, 2), 1);
}

#[test]
fn the_factor_is_a_draw_in_zero_to_one_that_depends_only_on_the_seed() {
    for seed in [0, 1, 2, 1_894_698_000_002, u64::MAX] {
        let f = fuzz::factor(seed);
        assert!((0.0..1.0).contains(&f), "factor {f} of seed {seed}");
        assert_eq!(f.to_bits(), fuzz::factor(seed).to_bits());
    }
    assert_ne!(fuzz::factor(1).to_bits(), fuzz::factor(2).to_bits());
}

#[test]
fn the_delta_follows_z3() {
    assert_eq!(fuzz::delta(2.4), 0.0);
    assert_eq!(fuzz::delta(0.0), 0.0);
    // Z3's examples are exact arithmetic; f32 differs only in the last bits.
    for (x, expected) in [(2.5, 1.0), (7.0, 1.675), (20.0, 2.975), (100.0, 6.975)] {
        let got = fuzz::delta(x);
        assert!((got - expected).abs() < 1e-5, "delta({x}) = {got}, expected {expected}");
    }
}

#[test]
fn the_range_and_pick_follow_the_z4_table() {
    // x, m, M, (lower, upper), result at f = 0, 0.5, 0.99.
    type Row = (f32, u32, u32, (u32, u32), [u32; 3]);
    let table: [Row; 7] = [
        (2.4, 1, 36500, (2, 2), [2, 2, 2]),
        (3.0, 1, 36500, (2, 4), [2, 3, 4]),
        (10.0, 1, 36500, (8, 12), [8, 10, 12]),
        (25.0, 1, 36500, (22, 28), [22, 25, 28]),
        (4.0, 5, 36500, (5, 6), [5, 6, 6]),
        (200.0, 1, 150, (141, 150), [141, 146, 150]),
        (1.0, 4, 4, (4, 4), [4, 4, 4]),
    ];
    for (x, m, max, range, picks) in table {
        assert_eq!(fuzz::range(x, m, max), range, "range of {x} in [{m}, {max}]");
        for (f, expected) in [0.0, 0.5, 0.99].into_iter().zip(picks) {
            assert_eq!(
                fuzz::fuzzed_interval(x, m, max, f),
                expected,
                "{x} in [{m}, {max}] at factor {f}"
            );
        }
    }
}

#[test]
fn the_no_regression_minimum_follows_z5() {
    assert_eq!(fuzz::no_regression_minimum(9.6, 9, 36500), 10);
    assert_eq!(fuzz::no_regression_minimum(8.2, 9, 36500), 9);
    assert_eq!(fuzz::no_regression_minimum(5.0, 9, 36500), 0);
}

#[test]
fn stored_card_deltas_equal_the_oracle_examples_of_z6() {
    // conformance/goldens/smoke/fuzz-table-small.json, maximum 36,500.
    let intervals = [1, 2, 3, 7, 20, 100, 1000];
    let rows: [(u64, u64, [i64; 7]); 3] = [
        (1, 0, [0, 0, 1, 2, 2, 5, 34]),
        (1, 1, [0, 0, -1, -2, -3, -6, -44]),
        (1_894_680_000_001, 2, [0, 0, 0, 1, 1, 2, 13]),
    ];
    for (card_id, reps, deltas) in rows {
        for (interval, expected) in intervals.into_iter().zip(deltas) {
            assert_eq!(
                fuzz::stored_card_delta(card_id, reps, interval, 36500),
                expected,
                "card {card_id}, reps {reps}, interval {interval}"
            );
        }
    }
}
