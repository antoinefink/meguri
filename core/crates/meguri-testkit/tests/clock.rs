//! The scenario clock (specs/conformance/dsl.md D5, D7.1). Expected instants
//! and offsets are the oracle's `timing` observations in the smoke goldens, or
//! epoch values computed outside this crate.

use meguri_model::clock::Clock;
use meguri_testkit::exec::clock::ScenarioClock;
use meguri_testkit::scenario::Duration;

fn d(text: &str) -> Duration {
    text.parse().unwrap()
}

#[test]
fn start_and_advance_follow_the_oracle_in_europe_paris() {
    // smoke/timing-advance: now and utc_offset_secs at each observe step.
    let clock = ScenarioClock::new("Europe/Paris", "2030-01-15T10:00:00").unwrap();
    let mut seen = vec![clock.now_secs()];
    for step in ["20m", "23h40m", "4d", "21d"] {
        clock.advance(d(step));
        seen.push(clock.now_secs());
    }
    assert_eq!(
        seen,
        [
            1_894_698_000,
            1_894_699_200,
            1_894_784_400,
            1_895_130_000,
            1_896_944_400
        ]
    );
    assert_eq!(clock.now_ms(), 1_896_944_400_000);
    assert_eq!(clock.utc_offset_at(clock.now_ms()).seconds(), 3600);
}

#[test]
fn set_clock_uses_wall_time_in_the_scenario_zone() {
    // smoke/timing-tz-asia-kathmandu and smoke/timing-tz-utc.
    let kathmandu = ScenarioClock::new("Asia/Kathmandu", "2030-01-15T10:00:00").unwrap();
    assert_eq!(kathmandu.now_secs(), 1_894_680_900);
    kathmandu.set("2030-01-16T03:59:59").unwrap();
    assert_eq!(kathmandu.now_secs(), 1_894_745_699);
    kathmandu.advance(d("1s"));
    assert_eq!(kathmandu.now_secs(), 1_894_745_700);
    assert_eq!(kathmandu.utc_offset_at(kathmandu.now_ms()).seconds(), 20_700);

    let utc = ScenarioClock::new("UTC", "2030-01-15T10:00:00").unwrap();
    assert_eq!(utc.now_secs(), 1_894_701_600);
    utc.set("2030-01-16T03:59:59").unwrap();
    assert_eq!(utc.now_secs(), 1_894_766_399);
}

#[test]
fn explicit_offsets_dst_gaps_and_repeated_hours() {
    let paris = ScenarioClock::new("Europe/Paris", "2030-01-15T10:00:00Z").unwrap();
    assert_eq!(paris.now_secs(), 1_894_701_600);
    paris.set("2030-07-01T12:00:00+02:00").unwrap();
    assert_eq!(paris.now_secs(), 1_909_130_400);
    assert_eq!(paris.utc_offset_at(paris.now_ms()).seconds(), 7200);
    // 02:30 on 2030-03-31 does not exist in Paris.
    assert!(paris.set("2030-03-31T02:30:00").is_err());
    // 02:30 on 2030-10-27 happens twice: the first one (+02:00) is meant.
    paris.set("2030-10-27T02:30:00").unwrap();
    assert_eq!(paris.now_secs(), 1_919_291_400);
    assert_eq!(paris.utc_offset_at(1_919_295_000_000).seconds(), 3600);

    for bad in [
        "2030-01-15T10:00:00.5",
        "2030-01-15 10:00:00",
        "2030-02-30T10:00:00",
        "tomorrow",
    ] {
        assert!(ScenarioClock::new("Europe/Paris", bad).is_err(), "accepted {bad}");
    }
    assert!(ScenarioClock::new("Mars/Olympus", "2030-01-15T10:00:00").is_err());
}

#[test]
fn durations_are_whole_seconds_with_86400_second_days() {
    assert_eq!(d("1d").seconds(), 86_400);
    assert_eq!(d("23h40m").seconds(), 85_200);
    assert_eq!(d("90s").seconds(), 90);
    assert_eq!(d("1d1d").seconds(), 172_800);
    for bad in ["", "1", "1w", "-1d", "1.5h", "d", " 1d"] {
        assert!(bad.parse::<Duration>().is_err(), "accepted {bad:?}");
    }
    // A day is 86,400 s across the October DST change.
    let paris = ScenarioClock::new("Europe/Paris", "2030-10-26T12:00:00").unwrap();
    let before = paris.now_secs();
    paris.advance(d("1d"));
    assert_eq!(paris.now_secs() - before, 86_400);
}
