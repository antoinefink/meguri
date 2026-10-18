use meguri_sched::states::steps::{StepDelay, Steps, seconds};

#[test]
fn minute_conversion_is_f32_truncating_and_saturating() {
    for (minutes, expected) in [
        (1.0, 60),
        (0.75, 45),
        (0.5, 30),
        (19.983334, 1199),
        (1440.0, 86400),
        (0.001, 0),
        (-5.0, 0),
        (f32::NAN, 0),
        (f32::INFINITY, u32::MAX),
    ] {
        assert_eq!(seconds(minutes), expected);
    }
}

#[test]
fn remaining_counts_and_each_step_transition_follow_the_contract_table() {
    for (left, index) in [(3, 0), (2, 1), (1, 2), (1002, 1), (2003, 0), (0, 2), (5, 0)] {
        let steps = Steps::new(&[1.0, 10.0, 60.0], left);
        assert_eq!(steps.index(), index);
        assert_eq!(
            steps.again(),
            Some(StepDelay {
                seconds: 60,
                remaining: 3
            })
        );
        assert_eq!(steps.hard().unwrap().remaining, left % 1000);
        assert_eq!(steps.current(), [60, 600, 3600][index]);
        assert_eq!(
            steps.good(),
            if index == 2 {
                None
            } else {
                Some(StepDelay {
                    seconds: [600, 3600][index],
                    remaining: 2 - index as u32,
                })
            }
        );
    }
    let empty = Steps::new(&[], 0);
    assert_eq!(empty.current(), 0);
    assert_eq!(empty.again(), None);
    assert_eq!(empty.hard(), None);
    assert_eq!(empty.good(), None);
}

#[test]
fn hard_uses_integer_averages_then_rounds_first_step_delays_above_one_day() {
    for (minutes, expected) in [
        (10.0, 900),
        (960.0, 86400),
        (1380.0, 86400),
        (1440.0, 172800),
        (1500.0, 172800),
        (2880.0, 259200),
        (2820.0, 259200),
    ] {
        assert_eq!(Steps::new(&[minutes], 1).hard().unwrap().seconds, expected);
    }
    for (first, second, expected) in [
        (0.5, 2.0, 75),
        (1.0, 10.0, 330),
        (10.0, 1440.0, 43500),
        (1440.0, 4320.0, 172800),
        (1200.0, 2940.0, 86400),
    ] {
        assert_eq!(Steps::new(&[first, second], 2).hard().unwrap().seconds, expected);
    }
    assert_eq!(Steps::new(&[1.0, 1500.0], 1).hard().unwrap().seconds, 90000);
    // The first-step multiplication/sum saturates before division, per L4.
    assert_eq!(Steps::new(&[f32::INFINITY], 1).hard().unwrap().seconds, 2_147_472_000);
    assert_eq!(
        Steps::new(&[f32::INFINITY, f32::INFINITY], 2).hard().unwrap().seconds,
        2_147_472_000
    );
}
