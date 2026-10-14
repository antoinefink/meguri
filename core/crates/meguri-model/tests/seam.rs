//! The time and randomness seam (R1875): tests and the scenario runner inject
//! fixed instants and seeds, and the core sees exactly those values.

use meguri_model::clock::{Clock, FixedClock, UtcOffset};
use meguri_model::rng::{RandomSource, SeededRandom};

/// 2030-01-15T10:00:00Z in milliseconds since the Unix epoch.
const JAN_15_2030_10H_UTC_MS: i64 = 1_894_701_600_000;
/// +05:45 (Nepal), in seconds east of UTC.
const PLUS_05_45_SECS: i32 = 20_700;

#[test]
fn fixed_clock_returns_the_injected_instant_and_offset() {
    let offset = UtcOffset::from_seconds(PLUS_05_45_SECS).expect("+05:45 is a valid offset");
    let clock: &dyn Clock = &FixedClock::new(JAN_15_2030_10H_UTC_MS, offset);

    assert_eq!(clock.now_ms(), JAN_15_2030_10H_UTC_MS);
    assert_eq!(clock.now_ms(), JAN_15_2030_10H_UTC_MS, "a fixed clock never advances");
    assert_eq!(clock.utc_offset_at(clock.now_ms()).seconds(), PLUS_05_45_SECS);
    assert_eq!(clock.utc_offset_at(0).seconds(), PLUS_05_45_SECS);
    assert_eq!(clock.utc_offset().seconds(), PLUS_05_45_SECS);
    assert_eq!(clock.zone_id(), "UTC");
}

#[test]
fn the_host_names_the_time_zone() {
    let offset = UtcOffset::from_seconds(PLUS_05_45_SECS).unwrap();
    let clock: &dyn Clock = &FixedClock::new(JAN_15_2030_10H_UTC_MS, offset).in_zone("Asia/Kathmandu");
    assert_eq!(clock.zone_id(), "Asia/Kathmandu");
    assert_eq!(clock.now_ms(), JAN_15_2030_10H_UTC_MS);
}

#[test]
fn seconds_round_down_also_before_1970() {
    use meguri_model::time::{EpochMillis, EpochSecs};
    let clock: &dyn Clock = &FixedClock::new(-1, UtcOffset::UTC);
    assert_eq!(EpochSecs::from_clock(clock), EpochSecs(-1));
    assert_eq!(EpochMillis(1_999).whole_secs(), EpochSecs(1));
}

#[test]
fn utc_offsets_cover_west_and_east_and_reject_a_whole_day() {
    assert_eq!(UtcOffset::UTC.seconds(), 0);
    assert_eq!(
        UtcOffset::from_seconds(-12 * 3600).map(UtcOffset::seconds),
        Some(-43_200)
    );
    assert_eq!(UtcOffset::from_seconds(14 * 3600).map(UtcOffset::seconds), Some(50_400));
    assert_eq!(UtcOffset::from_seconds(86_399).map(UtcOffset::seconds), Some(86_399));
    assert_eq!(UtcOffset::from_seconds(86_400), None);
    assert_eq!(UtcOffset::from_seconds(-86_400), None);
}

/// SplitMix64 reference outputs for seed 1234567 (Steele, Lea and Flood, 2014;
/// the values published with the Rosetta Code SplitMix64 task).
const SPLITMIX64_SEED_1234567: [u64; 5] = [
    6_457_827_717_110_365_317,
    3_203_168_211_198_807_973,
    9_817_491_932_198_370_423,
    4_593_380_528_125_082_431,
    16_408_922_859_458_223_821,
];

#[test]
fn seeded_random_yields_the_reference_sequence_for_its_seed() {
    let source: &dyn RandomSource = &SeededRandom::new(1_234_567);
    let drawn: Vec<u64> = (0..5).map(|_| source.next_u64()).collect();
    assert_eq!(drawn, SPLITMIX64_SEED_1234567);
}

#[test]
fn the_same_seed_replays_the_same_sequence() {
    let first = SeededRandom::new(42);
    let second = SeededRandom::new(42);
    let a: Vec<u64> = (0..1000).map(|_| first.next_u64()).collect();
    let b: Vec<u64> = (0..1000).map(|_| second.next_u64()).collect();
    assert_eq!(a, b);

    let other = SeededRandom::new(43);
    let c: Vec<u64> = (0..1000).map(|_| other.next_u64()).collect();
    assert_ne!(a, c, "a different seed gives a different sequence");
}
