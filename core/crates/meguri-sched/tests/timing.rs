//! The pure day clock against the worked examples of
//! `specs/sched/day-clock.md` (D2, D3, D3.3, D4, D7, D11). Instants are
//! written as the local time and offset the spec gives and converted here
//! with a civil-date formula of this file's own.

use meguri_model::clock::{Clock, UtcOffset};
use meguri_sched::timing::{
    self, DayInputs, NewCollectionTiming, SchedDay, collapse_time, creation_offset, last_unburied, local_offset,
    minutes_west, needs_unbury, new_collection_timing, rollover_hour, sched_day, unburied_queue,
};

/// Days from 1970-01-01 to a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The instant of `"YYYY-MM-DD HH:MM:SS"` local time at `offset` seconds
/// east of UTC.
fn at(local: &str, offset: i32) -> i64 {
    let n = |range: std::ops::Range<usize>| local[range].parse::<i64>().unwrap();
    let days = days_from_civil(n(0..4), n(5..7), n(8..10));
    days * 86_400 + n(11..13) * 3600 + n(14..16) * 60 + n(17..19) - i64::from(offset)
}

const CET: i32 = 3600;
const CEST: i32 = 7200;

fn offset(seconds: i32) -> UtcOffset {
    UtcOffset::from_seconds(seconds).unwrap()
}

fn day(crt: i64, creation_offset: Option<i32>, current: i32, hour: u8, now: i64) -> SchedDay {
    sched_day(&DayInputs {
        crt,
        creation_offset,
        offset: offset(current),
        rollover_hour: hour,
        now,
    })
}

#[test]
fn the_civil_helper_agrees_with_the_spec_instants() {
    assert_eq!(at("2030-01-15 04:00:00", CET), 1_894_676_400);
    assert_eq!(at("2030-01-15 04:00:00", 9 * 3600), 1_894_647_600);
    assert_eq!(at("2030-01-14 04:00:00", -8 * 3600), 1_894_622_400);
    assert_eq!(at("2030-03-30 04:00:00", CEST), 1_901_066_400);
}

#[test]
fn d3_1_paris_with_a_creation_offset() {
    let crt = 1_894_676_400;
    for (now, hour, today, next) in [
        ("2030-01-15 10:00:00", 4, 0, "2030-01-16 04:00:00"),
        ("2030-01-16 03:59:59", 4, 0, "2030-01-16 04:00:00"),
        ("2030-01-16 04:00:00", 4, 1, "2030-01-17 04:00:00"),
        ("2030-01-20 10:00:00", 12, 4, "2030-01-20 12:00:00"),
        ("2030-01-20 12:00:00", 12, 5, "2030-01-21 12:00:00"),
        // Before crt the day is clamped to 0.
        ("2030-01-12 10:00:00", 4, 0, "2030-01-13 04:00:00"),
    ] {
        assert_eq!(
            day(crt, Some(-60), CET, hour, at(now, CET)),
            SchedDay {
                today,
                next_day_at: at(next, CET)
            },
            "{now} at hour {hour}"
        );
    }
}

#[test]
fn ae84_kathmandu_one_second_before_at_and_after_the_boundary() {
    let kathmandu = 5 * 3600 + 45 * 60;
    let crt = at("2030-01-15 04:00:00", kathmandu);
    let next = at("2030-01-21 04:00:00", kathmandu);
    assert_eq!(
        day(crt, Some(-345), kathmandu, 4, at("2030-01-20 03:59:59", kathmandu)),
        SchedDay {
            today: 4,
            next_day_at: at("2030-01-20 04:00:00", kathmandu)
        }
    );
    for now in ["2030-01-20 04:00:00", "2030-01-20 04:00:01"] {
        assert_eq!(
            day(crt, Some(-345), kathmandu, 4, at(now, kathmandu)),
            SchedDay {
                today: 5,
                next_day_at: next
            },
            "{now}"
        );
    }
}

#[test]
fn r1900_created_in_one_zone_studied_in_another() {
    let la = -8 * 3600;
    let tokyo = 9 * 3600;
    // Created in Tokyo, studied in Los Angeles.
    let crt = 1_894_647_600;
    assert_eq!(day(crt, Some(-540), la, 4, at("2030-01-15 10:00:00", la)).today, 0);
    assert_eq!(day(crt, Some(-540), la, 4, at("2030-01-16 03:59:59", la)).today, 0);
    assert_eq!(day(crt, Some(-540), la, 4, at("2030-01-16 04:00:00", la)).today, 1);
    // Created in Los Angeles, studied in Tokyo: N is a calendar day after C.
    let crt = 1_894_622_400;
    assert_eq!(day(crt, Some(480), tokyo, 4, at("2030-01-15 10:00:00", tokyo)).today, 1);
    assert_eq!(day(crt, Some(480), tokyo, 4, at("2030-01-16 04:00:00", tokyo)).today, 2);
}

#[test]
fn only_the_calendar_date_of_crt_counts() {
    // crt 2030-01-14 23:30 +01:00 under creationOffset 1439 (-23:59) is
    // 2030-01-13: a day more than under -60.
    let crt = at("2030-01-14 23:30:00", CET);
    let now = at("2030-01-20 10:00:00", CET);
    assert_eq!(day(crt, Some(-60), CET, 4, now).today, 6);
    assert_eq!(day(crt, Some(1439), CET, 4, now).today, 7);
}

#[test]
fn d3_2_hours_zero_and_twenty_three() {
    let crt = 1_894_676_400;
    // Hour 0: always rolled over, the next boundary is the next midnight.
    assert_eq!(
        day(crt, Some(-60), CET, 0, at("2030-01-18 00:00:00", CET)),
        SchedDay {
            today: 3,
            next_day_at: at("2030-01-19 00:00:00", CET)
        }
    );
    assert_eq!(
        day(crt, Some(-60), CET, 0, at("2030-01-17 23:59:59", CET)),
        SchedDay {
            today: 2,
            next_day_at: at("2030-01-18 00:00:00", CET)
        }
    );
    // Hour 23: the day starts at 23:00.
    assert_eq!(
        day(crt, Some(-60), CET, 23, at("2030-01-18 22:59:59", CET)),
        SchedDay {
            today: 2,
            next_day_at: at("2030-01-18 23:00:00", CET)
        }
    );
    assert_eq!(
        day(crt, Some(-60), CET, 23, at("2030-01-18 23:00:00", CET)),
        SchedDay {
            today: 3,
            next_day_at: at("2030-01-19 23:00:00", CET)
        }
    );
}

#[test]
fn d3_3_twenty_three_and_twenty_five_hour_days_each_count_as_one() {
    // Paris, rollover 4, created 2030-03-20 04:00 +01:00 (day 0).
    let crt = at("2030-03-20 04:00:00", CET);
    // Day 10 starts 2030-03-30 04:00 +01:00, day 11 23 hours later.
    let day10 = at("2030-03-30 04:00:00", CET);
    let day11 = at("2030-03-31 04:00:00", CEST);
    assert_eq!(day11 - day10, 23 * 3600);
    assert_eq!(day(crt, Some(-60), CET, 4, day10).today, 10);
    assert_eq!(day(crt, Some(-60), CEST, 4, day11 - 1).today, 10);
    assert_eq!(day(crt, Some(-60), CEST, 4, day11).today, 11);
    // A computation made before the change gives a boundary under the old
    // offset: 05:00 local after the change.
    assert_eq!(
        day(crt, Some(-60), CET, 4, at("2030-03-30 12:00:00", CET)).next_day_at,
        at("2030-03-31 04:00:00", CET)
    );
    // After it, 04:00 +02:00.
    assert_eq!(
        day(crt, Some(-60), CEST, 4, at("2030-03-31 12:00:00", CEST)).next_day_at,
        at("2030-04-01 04:00:00", CEST)
    );
    // Day 220 starts 2030-10-26 04:00 +02:00, day 221 25 hours later.
    let day220 = at("2030-10-26 04:00:00", CEST);
    let day221 = at("2030-10-27 04:00:00", CET);
    assert_eq!(day221 - day220, 25 * 3600);
    assert_eq!(day(crt, Some(-60), CEST, 4, day220).today, 220);
    assert_eq!(day(crt, Some(-60), CET, 4, day221 - 1).today, 220);
    assert_eq!(day(crt, Some(-60), CET, 4, day221).today, 221);
}

#[test]
fn d3_3_a_rollover_hour_inside_a_skipped_hour() {
    // Paris, rollover 2, 2030-03-31: 02:00 local does not exist.
    let crt = at("2030-03-25 04:00:00", CET);
    let before = day(crt, Some(-60), CET, 2, at("2030-03-31 01:59:59", CET));
    assert_eq!(
        before,
        SchedDay {
            today: 5,
            next_day_at: at("2030-03-31 02:00:00", CET)
        }
    );
    // That instant is 03:00 +02:00, where 02:00 +02:00 is already past.
    assert_eq!(
        day(crt, Some(-60), CEST, 2, before.next_day_at),
        SchedDay {
            today: 6,
            next_day_at: at("2030-04-01 02:00:00", CEST)
        }
    );
}

#[test]
fn d3_3_a_rollover_hour_inside_a_repeated_hour() {
    // New York, rollover 1, 2030-11-03: 01:00 local happens twice.
    let edt = -4 * 3600;
    let est = -5 * 3600;
    let crt = at("2030-10-28 04:00:00", edt);
    let first = at("2030-11-03 01:00:00", edt);
    assert_eq!(day(crt, Some(240), edt, 1, first - 1).today, 5);
    let started = day(crt, Some(240), edt, 1, first);
    assert_eq!(started.today, 6);
    let next = at("2030-11-04 01:00:00", est);
    assert_eq!(next - first, 25 * 3600);
    // An hour later the clocks repeat 01:00, now at -05:00: still day 6.
    assert_eq!(
        day(crt, Some(240), est, 1, first + 3600),
        SchedDay {
            today: 6,
            next_day_at: next
        }
    );
}

#[test]
fn d4_legacy_timing_paris_examples() {
    let crt = 1_894_755_600;
    assert_eq!(crt, at("2030-01-16 02:00:00", CET));
    for (now, hour, today, next) in [
        ("2030-01-14 03:00:00", 4, 4_294_967_294, "2030-01-14 04:00:00"),
        ("2030-01-15 05:00:00", 4, 0, "2030-01-16 04:00:00"),
        ("2030-01-16 04:00:00", 4, 0, "2030-01-16 04:00:00"),
        ("2030-01-16 04:00:01", 4, 0, "2030-01-17 04:00:00"),
        ("2030-01-20 04:00:00", 4, 4, "2030-01-20 04:00:00"),
        ("2030-01-22 12:00:00", 12, 6, "2030-01-22 12:00:00"),
        ("2030-01-25 00:00:00", 0, 9, "2030-01-25 00:00:00"),
    ] {
        assert_eq!(
            day(crt, None, CET, hour, at(now, CET)),
            SchedDay {
                today,
                next_day_at: at(next, CET)
            },
            "{now} at hour {hour}"
        );
    }
}

#[test]
fn d4_a_day_or_more_before_crt_wraps_to_an_unsigned_number() {
    let crt = at("2030-01-16 02:00:00", CET);
    // A is 2030-01-16 04:00; exactly one day before it is -1.
    assert_eq!(day(crt, None, CET, 4, at("2030-01-15 04:00:00", CET)).today, u32::MAX);
    assert_eq!(day(crt, None, CET, 4, at("2030-01-15 04:00:01", CET)).today, 0);
}

#[test]
fn d4_the_two_timings_differ_when_crt_falls_on_another_date_under_the_current_offset() {
    let crt = at("2030-01-14 23:30:00", CET);
    let now = at("2030-06-01 10:00:00", CEST);
    assert_eq!(day(crt, None, CEST, 4, now).today, 137);
    assert_eq!(day(crt, Some(-60), CEST, 4, now).today, 138);
}

#[test]
fn d2_rollover_readings() {
    for (stored, hour) in [
        (Some(0), Some(0)),
        (Some(4), Some(4)),
        (Some(23), Some(23)),
        (Some(24), Some(23)),
        (Some(255), Some(23)),
        (Some(256), None),
        (Some(-1), None),
        (None, None),
    ] {
        assert_eq!(rollover_hour(stored), hour, "{stored:?}");
    }
    assert_eq!(timing::DEFAULT_ROLLOVER_HOUR, 4);
}

#[test]
fn d2_offset_readings() {
    for (stored, offset) in [
        (Some(-60), Some(-60)),
        (Some(1439), Some(1439)),
        (Some(-1439), Some(-1439)),
        (Some(1440), None),
        (Some(-1440), None),
        (None, None),
    ] {
        assert_eq!(creation_offset(stored), offset, "{stored:?}");
        assert_eq!(local_offset(stored), offset.unwrap_or(0), "{stored:?}");
    }
}

#[test]
fn d2_last_unburied_and_collapse_time_readings() {
    assert_eq!(last_unburied(Some(17)), 17);
    assert_eq!(last_unburied(Some(4_294_967_295)), u32::MAX);
    assert_eq!(last_unburied(Some(4_294_967_296)), 0);
    assert_eq!(last_unburied(Some(-1)), 0);
    assert_eq!(last_unburied(None), 0);
    assert_eq!(collapse_time(Some(600)), 600);
    assert_eq!(collapse_time(Some(-1)), 1200);
    assert_eq!(collapse_time(None), 1200);
}

#[test]
fn d1_offsets_in_minutes_west() {
    assert_eq!(minutes_west(offset(3600)), -60);
    assert_eq!(minutes_west(offset(-8 * 3600)), 480);
    assert_eq!(minutes_west(offset(5 * 3600 + 45 * 60)), -345);
    assert_eq!(minutes_west(offset(-(3 * 3600 + 30 * 60))), 210);
    assert_eq!(minutes_west(UtcOffset::UTC), 0);
}

#[test]
fn d7_the_unbury_condition() {
    // With today 10: 9 and 18 unbury, 10 and 17 do not.
    assert!(needs_unbury(9, 10));
    assert!(needs_unbury(18, 10));
    assert!(!needs_unbury(10, 10));
    assert!(!needs_unbury(17, 10));
    // No overflow at the top of the range.
    assert!(!needs_unbury(u32::MAX, u32::MAX));
    assert!(needs_unbury(0, u32::MAX));
}

#[test]
fn d7_the_queue_a_buried_card_returns_to() {
    assert_eq!(unburied_queue(0, 1), Some(0));
    assert_eq!(unburied_queue(2, 5), Some(2));
    assert_eq!(unburied_queue(1, 1_895_202_000), Some(1));
    assert_eq!(unburied_queue(3, 1_895_202_000), Some(1));
    assert_eq!(unburied_queue(1, 6), Some(3));
    assert_eq!(unburied_queue(3, 6), Some(3));
    assert_eq!(unburied_queue(1, 1_000_000_000), Some(3));
    assert_eq!(unburied_queue(1, 1_000_000_001), Some(1));
}

/// A clock in a zone with one offset change, enough for D11's table.
#[derive(Debug)]
struct Zone {
    now_ms: i64,
    /// Offsets in effect from each instant on, in order.
    changes: Vec<(i64, i32)>,
}

impl Clock for Zone {
    fn now_ms(&self) -> i64 {
        self.now_ms
    }

    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset {
        let seconds = self
            .changes
            .iter()
            .rev()
            .find(|(from, _)| at_ms.div_euclid(1000) >= *from)
            .map_or(self.changes[0].1, |(_, o)| *o);
        offset(seconds)
    }

    fn zone_id(&self) -> String {
        "Test/Zone".to_owned()
    }
}

fn fixed(offset: i32, now: i64) -> Zone {
    Zone {
        now_ms: now * 1000,
        changes: vec![(i64::MIN, offset)],
    }
}

#[test]
fn d11_new_collection_timing() {
    // Paris in January, created before 04:00: the previous day's 04:00.
    let t = at("2030-01-15 03:30:00", CET);
    assert_eq!(
        new_collection_timing(t, &fixed(CET, t)),
        NewCollectionTiming {
            crt: 1_894_590_000,
            creation_offset: -60
        }
    );
    // UTC, created at 04:00 exactly: that instant.
    let t = at("2030-01-15 04:00:00", 0);
    assert_eq!(
        new_collection_timing(t, &fixed(0, t)),
        NewCollectionTiming {
            crt: 1_894_680_000,
            creation_offset: 0
        }
    );
    // Tokyo at 10:00: the same day's 04:00.
    let t = at("2030-01-15 10:00:00", 9 * 3600);
    assert_eq!(
        new_collection_timing(t, &fixed(9 * 3600, t)),
        NewCollectionTiming {
            crt: 1_894_647_600,
            creation_offset: -540
        }
    );
}

#[test]
fn d11_the_creation_offset_is_the_one_in_effect_at_crt() {
    // Paris moves to +02:00 at 2030-03-31 01:00Z and back at 2030-10-27 01:00Z.
    let paris = |now| Zone {
        now_ms: now * 1000,
        changes: vec![
            (i64::MIN, CET),
            (at("2030-03-31 01:00:00", 0), CEST),
            (at("2030-10-27 01:00:00", 0), CET),
        ],
    };
    let t = at("2030-03-31 03:30:00", CEST);
    assert_eq!(
        new_collection_timing(t, &paris(t)),
        NewCollectionTiming {
            crt: 1_901_066_400,
            creation_offset: -60
        }
    );
    let t = at("2030-10-27 03:30:00", CET);
    assert_eq!(
        new_collection_timing(t, &paris(t)),
        NewCollectionTiming {
            crt: 1_919_214_000,
            creation_offset: -120
        }
    );
}
