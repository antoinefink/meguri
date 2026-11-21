//! `stats/card-info` C14.2: the tables of C4 to C7, the snapshot's rules on
//! a schema-18 fixture copy, C11's kinds, C8's bytes and C1's no-write
//! property. Values and names are made up.

use std::fs;
use std::path::Path;

use meguri_model::{
    changes::{CardChange, ChangeKind, ChangeSet, ConfigKeyChange, DeckChange, OpSource},
    clock::{Clock, FixedClock, UtcOffset},
    deck_config::DeckConfig,
    ids::{CardId, DeckConfigId, DeckId, NoteId, RevlogId},
    revlog::RevlogKind,
    rng::SeededRandom,
};
use meguri_stats::card_info::{
    self, CardInfo, CardInfoError, CurvePoint, ErrorKind, Input, MemorySource, RangeKind, StatsKey,
    curve::{CurveReview, build as curve},
    texts::{difficulty_text, ease_text, stability_text},
};
use meguri_stats::text::{
    SpanForm, custom_data::custom_data_text, date, date_time, js_number, time_span, two_decimal_percent, whole_percent,
};
use meguri_storage::{OpenOptions, OpenedCollection, ReadRows, ReaderRole, open_collection, raw::SqlValue};
use meguri_testkit::exec::clock::ScenarioClock;
use tempfile::TempDir;

/// 2030-01-15T10:00:00Z.
const NOW: i64 = 1_894_701_600;
/// The fixture's review card (note type Pair, deck Lang, preset Default),
/// its note and its sibling (a new card of the second template).
const CARD: i64 = 1_788_249_600_238;
const NOTE: i64 = 1_788_249_600_238;
const SIBLING: i64 = 1_788_249_600_239;
/// A card of the fixture's cloze note type.
const CLOZE_CARD: i64 = 1_788_249_600_243;
const LANG: i64 = 1_788_249_600_212;
const VERBS: i64 = 1_788_249_600_214;
const CRAM: i64 = 1_788_249_600_254;
const SLOW: i64 = 1_788_249_600_216;

fn utc() -> FixedClock {
    FixedClock::new(NOW * 1000, UtcOffset::UTC)
}

fn input(card: i64) -> Input {
    Input {
        card_id: CardId(card),
        now: NOW,
        days_elapsed: 100,
        next_day_at: NOW + 18 * 3600,
    }
}

struct Db {
    _dir: TempDir,
    path: std::path::PathBuf,
    db: OpenedCollection,
}

impl Db {
    fn new() -> Db {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("collection.anki2");
        fs::copy(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
            &path,
        )
        .unwrap();
        let clock = utc();
        let random = SeededRandom::new(1);
        let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
        let db = Db { _dir: dir, path, db };
        db.exec("DELETE FROM revlog", &[]);
        db
    }

    fn exec(&self, sql: &str, params: &[SqlValue]) {
        self.db.writer().execute_raw(sql, params).unwrap();
    }

    /// The card's scheduling columns and data.
    fn card(&self, kind: i64, queue: i64, due: i64, ivl: i64, data: &str) {
        self.exec(
            "UPDATE cards SET type = ?1, queue = ?2, due = ?3, ivl = ?4, data = ?5, odue = 0, odid = 0 WHERE id = ?6",
            &[
                SqlValue::Integer(kind),
                SqlValue::Integer(queue),
                SqlValue::Integer(due),
                SqlValue::Integer(ivl),
                SqlValue::Text(data.into()),
                SqlValue::Integer(CARD),
            ],
        );
    }

    /// A review-log row of the card: seconds, ease, ivl, lastIvl, factor,
    /// time, type.
    fn row(&self, (secs, ease, ivl, last, factor, time, kind): (i64, i64, i64, i64, i64, i64, i64)) {
        self.exec(
            "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?1, ?2, -1, ?3, ?4, ?5, ?6, ?7, ?8)",
            &[
                SqlValue::Integer(secs * 1000),
                SqlValue::Integer(CARD),
                SqlValue::Integer(ease),
                SqlValue::Integer(ivl),
                SqlValue::Integer(last),
                SqlValue::Integer(factor),
                SqlValue::Integer(time),
                SqlValue::Integer(kind),
            ],
        );
    }

    fn preset(&self, id: i64, edit: impl FnOnce(&mut DeckConfig)) {
        let reader = self.db.open_reader(ReaderRole::Query).unwrap();
        let mut preset = reader.deck_config(DeckConfigId(id)).unwrap().unwrap();
        edit(&mut preset);
        self.exec(
            "UPDATE deck_config SET config = ?1 WHERE id = ?2",
            &[SqlValue::Blob(preset.config.to_bytes().into_owned()), SqlValue::Integer(id)],
        );
    }

    fn info_with(&self, card: i64, clock: &dyn Clock) -> Result<CardInfo, CardInfoError> {
        let reader = self.db.open_reader(ReaderRole::Query).unwrap();
        card_info::build(&reader, clock, &input(card))
    }

    fn info(&self) -> CardInfo {
        self.info_with(CARD, &utc()).unwrap()
    }

    fn kind(&self, card: i64) -> ErrorKind {
        match self.info_with(card, &utc()) {
            Err(CardInfoError::Unavailable(kind)) => kind,
            other => panic!("expected an unavailable kind, got {other:?}"),
        }
    }
}

fn row(info: &CardInfo, key: StatsKey) -> Option<&str> {
    info.rows.iter().find(|row| row.key == key).map(|row| row.value.as_str())
}

fn widen(x: f32) -> f64 {
    f64::from(x)
}

// C4.2.
#[test]
fn c4_2_stability_texts() {
    for (s, text) in [
        (0.01f32, "14 minutes"),
        (0.5, "12 hours"),
        (1.4, "1 day"),
        (1.5, "2 days"),
        (2.3065, "2 days"),
        (30.4, "30 days"),
        (30.5, "1 month"),
        (31.0, "1.02 months"),
        (31.2, "1.03 months (31 days)"),
        (39.451553, "1.3 months (39 days)"),
        (45.0, "1.48 months (45 days)"),
        (92.9232, "3.06 months (93 days)"),
        (365.0, "1 year (365 days)"),
        (1000.0, "2.74 years (1,000 days)"),
        (-1.0, "-1 day"),
        (-0.000004, "-0 seconds"),
        (f32::INFINITY, "∞ years (∞ days)"),
        (f32::NEG_INFINITY, "-∞ years"),
    ] {
        assert_eq!(stability_text(widen(s)), text, "{s}");
    }
}

// C4.3.
#[test]
fn c4_3_interval_difficulty_and_retrievability_texts() {
    for (ivl, text) in [
        (1u32, "1 day"),
        (15, "15 days"),
        (30, "30 days"),
        (31, "1.02 months"),
        (45, "1.48 months"),
        (365, "1 year"),
        (400, "1.1 years"),
        (1000, "2.74 years"),
    ] {
        assert_eq!(time_span(f64::from(ivl) * 86_400.0, SpanForm::Long, false, false), text);
    }
    for (d, text) in [
        (1.0f32, "0%"),
        (2.118, "12%"),
        (5.5, "50%"),
        (7.6402645, "74%"),
        (10.0, "100%"),
        (f32::INFINITY, "Infinity%"),
        (f32::NEG_INFINITY, "-Infinity%"),
        (1e30, "1.1111111278305179e+31%"),
    ] {
        assert_eq!(difficulty_text(d), text, "{d}");
    }
    for (r, text) in [(0.9f32, "90%"), (0.905, "90%"), (0.9403443, "94%"), (0.995, "100%"), (0.004, "0%")] {
        assert_eq!(whole_percent(widen(r) * 100.0), text, "{r}");
    }
    // An infinite stability keeps retrievability at 1 whatever the days.
    let r = fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: f32::INFINITY,
            difficulty: 5.0,
        },
        400.0,
        0.5,
    );
    assert_eq!(whole_percent(widen(r) * 100.0), "100%");
}

// C5.2.
#[test]
fn c5_2_time_span_table() {
    for (seconds, long, short) in [
        (0.0, "0 seconds", "0s"),
        (0.004, "0 seconds", "0s"),
        (0.005, "0.01 seconds", "0.01s"),
        (1.0, "1 second", "1s"),
        (1.004, "1 second", "1s"),
        (1.005, "1.01 seconds", "1.01s"),
        (4.5, "4.5 seconds", "4.5s"),
        (59.995, "60 seconds", "60s"),
        (60.0, "1 minute", "1m"),
        (70.0, "1.17 minutes", "1.17m"),
        (600.0, "10 minutes", "10m"),
        (3_599.0, "59.98 minutes", "59.98m"),
        (5_400.0, "1.5 hours", "1.5h"),
        (86_399.0, "24 hours", "24h"),
        (129_600.0, "1.5 days", "1.5d"),
        (-86_400.0, "-1 day", "-1d"),
        (-0.004, "-0 seconds", "-0s"),
        (f64::INFINITY, "∞ years", "∞y"),
        (f64::NEG_INFINITY, "-∞ years", "-∞y"),
    ] {
        assert_eq!(time_span(seconds, SpanForm::Long, false, false), long, "{seconds}");
        assert_eq!(time_span(seconds, SpanForm::Short, false, false), short, "{seconds}");
    }
    // Grouping, the months suffix and the rounded form's halves toward +∞.
    assert_eq!(time_span(86_400.0 * 1_234.5, SpanForm::Long, false, true), "1,234.5 days");
    assert_eq!(time_span(2_628_000.0 * 2.0, SpanForm::Short, false, false), "2mo");
    assert_eq!(time_span(-0.4 * 3_600.0, SpanForm::Long, true, false), "-24 minutes");
    assert_eq!(time_span(-0.4, SpanForm::Long, true, false), "-0 seconds");
    assert_eq!(time_span(-1.5, SpanForm::Long, true, false), "-1 second");
    assert_eq!(time_span(2.5, SpanForm::Long, true, false), "3 seconds");
    // History examples (C6.2): 125 ms, 61,000 ms.
    assert_eq!(time_span(widen(125.0f32 / 1000.0), SpanForm::Short, false, false), "0.13s");
    assert_eq!(time_span(widen(61_000.0f32 / 1000.0), SpanForm::Short, false, false), "1.02m");
    assert_eq!(time_span(3.5, SpanForm::Short, false, false), "3.5s");
}

// C5.4.
#[test]
fn c5_4_percents_from_the_exact_binary_value() {
    assert_eq!(two_decimal_percent(8.855), "8.86%");
    assert_eq!(two_decimal_percent(1.005), "1.00%");
    assert_eq!(two_decimal_percent(99.995), "100.00%");
    assert_eq!(two_decimal_percent(90.12), "90.12%");
    assert_eq!(two_decimal_percent(100.0), "100.00%");
    // Exact ties round away from zero.
    assert_eq!(two_decimal_percent(0.125), "0.13%");
    assert_eq!(two_decimal_percent(2.375), "2.38%");
    assert_eq!(two_decimal_percent(-0.125), "-0.13%");
    assert_eq!(two_decimal_percent(-0.001), "-0.00%");
    assert_eq!(whole_percent(-0.4), "-0%");
    assert_eq!(whole_percent(-0.0), "0%");
    assert_eq!(whole_percent(0.5), "1%");
    assert_eq!(whole_percent(-2.5), "-3%");
    for (x, text) in [
        (f64::NAN, "NaN%"),
        (f64::INFINITY, "Infinity%"),
        (f64::NEG_INFINITY, "-Infinity%"),
        (1e21, "1e+21%"),
        (-1e21, "-1e+21%"),
    ] {
        assert_eq!(whole_percent(x), text);
        assert_eq!(two_decimal_percent(x), text);
    }
}

#[test]
fn javascript_number_form() {
    for (x, text) in [
        (1.5, "1.5"),
        (-0.0, "0"),
        (1e21, "1e+21"),
        (1e-7, "1e-7"),
        (1.5e-7, "1.5e-7"),
        (0.000001, "0.000001"),
        (123_456_789_012_345_680_000.0, "123456789012345680000"),
        (f64::INFINITY, "Infinity"),
        (-2.5e30, "-2.5e+30"),
    ] {
        assert_eq!(js_number(x), text);
    }
}

// C5.5.
#[test]
fn c5_5_custom_data_table() {
    for (cd, text) in [
        (r#"{"a":1,"b":"x"}"#, "a=1 b=x"),
        (r#"{"2":1,"1":2,"z":3}"#, "1=2 2=1 z=3"),
        (r#"{"v":null,"t":true,"f":1.50}"#, "v=null t=true f=1.5"),
        (r#"{"n":[1,2],"o":{"p":1}}"#, "n=1,2 o=[object Object]"),
        (r#"{"a":1,"b":2,"a":3}"#, "a=3 b=2"),
        (r#"{"a":1e21}"#, "a=1e+21"),
        (r#"{"a":1e400,"b":-1e400,"c":1e-400}"#, "a=Infinity b=-Infinity c=0"),
        ("\"😀\"", "0=\u{FFFD} 1=\u{FFFD}"),
        (r#"{"a":"\ud800x","😀":1}"#, "a=\u{FFFD}x 😀=1"),
        ("{}", ""),
        ("5", ""),
        ("true", ""),
        ("xyz", "xyz"),
        ("null", "null"),
        ("[[1,2],3]", "0=1,2 1=3"),
        ("[1,2]", "0=1 1=2"),
        ("[null,[null,1]]", "0=null 1=,1"),
        ("[]", ""),
        (r#"{"01":1,"4294967295":2,"4294967294":3,"-1":4}"#, "4294967294=3 01=1 4294967295=2 -1=4"),
        (r#"{"a":-0,"b":1e-7}"#, "a=0 b=1e-7"),
        ("{\"a\":1,}", "{\"a\":1,}"),
        ("01", "01"),
    ] {
        assert_eq!(custom_data_text(cd), text, "{cd}");
    }
    let nested = |depth: usize| format!("{}1{}", "[".repeat(depth), "]".repeat(depth));
    assert_eq!(custom_data_text(&nested(128)), "0=1");
    assert_eq!(custom_data_text(&nested(129)), nested(129));
}

// C6.4.
#[test]
fn c6_4_ease_column() {
    for (factor, text) in [
        (0u32, ""),
        (1, "D:-10%"),
        (95, "D:-1%"),
        (96, "D:-0%"),
        (100, "D:0%"),
        (224, "D:12%"),
        (1_005, "D:91%"),
        (1_055, "D:96%"),
        (1_100, "D:100%"),
        (1_101, "110%"),
        (1_105, "111%"),
        (1_300, "130%"),
        (2_350, "235%"),
        (2_500, "250%"),
        (2_505, "251%"),
        (4_000, "400%"),
    ] {
        assert_eq!(ease_text(factor), text, "{factor}");
    }
}

// C5.1 in every zone of R1899, across DST changes, at local midnight and at
// the rollover hour.
#[test]
fn c5_1_dates_across_the_zone_matrix() {
    #[rustfmt::skip]
    let cases: &[(&str, &[(i64, &str)])] = &[
        ("UTC", &[(1894748400, "2030-01-15 @ 23:00"), (1894751999, "2030-01-15 @ 23:59"), (1894752000, "2030-01-16 @ 00:00"), (1894766340, "2030-01-16 @ 03:59"), (1894766400, "2030-01-16 @ 04:00")]),
        ("Europe/Paris", &[(1894748400, "2030-01-16 @ 00:00"), (1894748399, "2030-01-15 @ 23:59"), (1894762740, "2030-01-16 @ 03:59"), (1894762800, "2030-01-16 @ 04:00")]),
        ("America/New_York", &[(1894748400, "2030-01-15 @ 18:00"), (1894769999, "2030-01-15 @ 23:59"), (1894770000, "2030-01-16 @ 00:00"), (1894784340, "2030-01-16 @ 03:59"), (1894784400, "2030-01-16 @ 04:00")]),
        ("America/Los_Angeles", &[(1894748400, "2030-01-15 @ 15:00"), (1894780799, "2030-01-15 @ 23:59"), (1894780800, "2030-01-16 @ 00:00"), (1894795140, "2030-01-16 @ 03:59"), (1894795200, "2030-01-16 @ 04:00")]),
        ("Asia/Tokyo", &[(1894748400, "2030-01-16 @ 08:00"), (1894719599, "2030-01-15 @ 23:59"), (1894719600, "2030-01-16 @ 00:00"), (1894733940, "2030-01-16 @ 03:59"), (1894734000, "2030-01-16 @ 04:00")]),
        ("Asia/Kolkata", &[(1894748400, "2030-01-16 @ 04:30"), (1894732199, "2030-01-15 @ 23:59"), (1894732200, "2030-01-16 @ 00:00"), (1894746540, "2030-01-16 @ 03:59"), (1894746600, "2030-01-16 @ 04:00")]),
        ("Asia/Kathmandu", &[(1894748400, "2030-01-16 @ 04:45"), (1894731299, "2030-01-15 @ 23:59"), (1894731300, "2030-01-16 @ 00:00"), (1894745640, "2030-01-16 @ 03:59"), (1894745700, "2030-01-16 @ 04:00")]),
        ("Pacific/Chatham", &[(1894748400, "2030-01-16 @ 12:45"), (1894702499, "2030-01-15 @ 23:59"), (1894702500, "2030-01-16 @ 00:00"), (1894716840, "2030-01-16 @ 03:59"), (1894716900, "2030-01-16 @ 04:00")]),
        ("Australia/Lord_Howe", &[(1894748400, "2030-01-16 @ 10:00"), (1894712399, "2030-01-15 @ 23:59"), (1894712400, "2030-01-16 @ 00:00"), (1894726740, "2030-01-16 @ 03:59"), (1894726800, "2030-01-16 @ 04:00")]),
        ("Pacific/Kiritimati", &[(1894748400, "2030-01-16 @ 13:00"), (1894701599, "2030-01-15 @ 23:59"), (1894701600, "2030-01-16 @ 00:00"), (1894715940, "2030-01-16 @ 03:59"), (1894716000, "2030-01-16 @ 04:00")]),
        ("America/St_Johns", &[(1894748400, "2030-01-15 @ 19:30"), (1894764599, "2030-01-15 @ 23:59"), (1894764600, "2030-01-16 @ 00:00"), (1894778940, "2030-01-16 @ 03:59"), (1894779000, "2030-01-16 @ 04:00")]),
        ("Europe/Paris", &[(1901149199, "2030-03-31 @ 01:59"), (1901149200, "2030-03-31 @ 03:00"), (1919293199, "2030-10-27 @ 02:59"), (1919293200, "2030-10-27 @ 02:00")]),
        ("America/New_York", &[(1899356399, "2030-03-10 @ 01:59"), (1899356400, "2030-03-10 @ 03:00"), (1919915999, "2030-11-03 @ 01:59"), (1919916000, "2030-11-03 @ 01:00")]),
        ("America/Los_Angeles", &[(1899367199, "2030-03-10 @ 01:59"), (1899367200, "2030-03-10 @ 03:00"), (1919926799, "2030-11-03 @ 01:59"), (1919926800, "2030-11-03 @ 01:00")]),
        ("Australia/Lord_Howe", &[(1901717999, "2030-04-07 @ 01:59"), (1901718000, "2030-04-07 @ 01:30"), (1917444599, "2030-10-06 @ 01:59"), (1917444600, "2030-10-06 @ 02:30")]),
    ];
    for (zone, points) in cases {
        let clock = ScenarioClock::new(zone, "2030-01-15T10:00:00").unwrap();
        for (instant, text) in *points {
            assert_eq!(date_time(*instant, &clock), *text, "{zone} {instant}");
            assert_eq!(date(*instant, &clock), text[..10], "{zone} {instant}");
        }
    }
}

#[test]
fn c5_1_early_years_and_the_javascript_date_range() {
    let clock = utc();
    assert_eq!(date_time(-31_542_607_560, &clock), "970-06-15 @ 12:34");
    assert_eq!(date(-62_167_219_200, &clock), "0-01-01");
    assert_eq!(date_time(-62_167_219_260, &clock), "-1-12-31 @ 23:59");
    assert_eq!(date(8_640_000_000_000, &clock), "275760-09-13");
    assert_eq!(date(-8_640_000_000_000, &clock), "-271821-04-20");
    assert_eq!(date(8_640_000_000_001, &clock), "NaN-aN-aN");
    assert_eq!(date_time(-8_640_000_000_001, &clock), "NaN-aN-aN @ aN:aN");
    assert_eq!(date(i64::MIN, &clock), "NaN-aN-aN");
    assert_eq!(date_time(i64::MAX, &clock), "NaN-aN-aN @ aN:aN");
    // C5.1's example instant, with seconds dropped, not rounded.
    let paris = ScenarioClock::new("Europe/Paris", "2030-01-15T10:00:00").unwrap();
    assert_eq!(date_time(1_894_748_400, &paris), "2030-01-16 @ 00:00");
    assert_eq!(date_time(1_894_748_459, &paris), "2030-01-16 @ 00:00");
}

fn review(days_ago: f64, interval_days: f64, stability: f64) -> CurveReview {
    CurveReview {
        instant: NOW - (days_ago * 86_400.0).round() as i64,
        interval_secs: (interval_days * 86_400.0).round() as i64,
        stability,
    }
}

// C7.3.
#[test]
fn c7_3_offered_ranges() {
    use RangeKind::{AllTime, FirstMonth, FirstWeek, FirstYear};
    for (reviews, maximum, offered) in [
        (vec![review(1.0 / 24.0, 600.0 / 86_400.0, 1.0)], 0.0451, vec![FirstWeek]),
        (vec![review(3.0, 1.0, 1.0), review(1.0, 2.0, 2.0)], 5.0, vec![FirstWeek]),
        (vec![review(4.0, 1.0, 1.0), review(2.0 / 24.0, 3.0, 3.0)], 8.4167, vec![FirstWeek, FirstMonth]),
        (vec![review(10.0, 1.0, 1.0)], 10.5, vec![FirstWeek, FirstMonth]),
        (vec![review(20.0, 5.0, 5.0), review(5.0, 15.0, 15.0)], 37.5, vec![FirstWeek, FirstMonth, FirstYear]),
        (
            vec![review(400.0, 30.0, 30.0), review(100.0, 90.0, 90.0)],
            445.0,
            vec![FirstWeek, FirstMonth, FirstYear, AllTime],
        ),
    ] {
        let built = curve(&reviews, NOW, 0.5, 0.9).unwrap();
        assert!((built.maximum_days - maximum).abs() < 1e-4, "{maximum}: {}", built.maximum_days);
        let kinds: Vec<_> = built.ranges.iter().map(|range| range.kind).collect();
        assert_eq!(kinds, offered);
        assert_eq!(built.default_range, *offered.last().unwrap());
    }
    assert!(curve(&[], NOW, 0.5, 0.9).is_none());
}

fn ascending(points: &[CurvePoint]) {
    for pair in points.windows(2) {
        assert!(pair[0].instant <= pair[1].instant, "{} > {}", pair[0].instant, pair[1].instant);
    }
}

// C7.5, with C7.3's fifth row.
#[test]
fn c7_5_points_of_the_fifth_example() {
    let reviews = [review(20.0, 5.0, 5.0), review(5.0, 15.0, 15.0)];
    let built = curve(&reviews, NOW, 0.5, 0.9).unwrap();
    let week = &built.ranges[0];
    assert_eq!(week.span_days, 7.0);
    ascending(&week.points);
    let first = &week.points[0];
    assert_eq!((first.days_since_first, first.retrievability, first.future), (0.0, 100.0, false));
    assert_eq!(week.points.last().unwrap().days_since_first, 7.0);
    assert!(week.points.iter().all(|point| !point.future));
    // Samples come from k × step, step = 7 / 1,000 days, never summed.
    for (k, point) in week.points[1..week.points.len() - 1].iter().enumerate() {
        let k = (k + 1) as f64;
        assert_eq!(point.instant, reviews[0].instant as f64 + (k * 0.007) * 86_400.0);
    }
    assert_eq!(week.points.len(), 1 + 999 + 1);
    let month = &built.ranges[1];
    ascending(&month.points);
    let t1 = reviews[1].instant as f64;
    let at_review: Vec<_> = month.points.iter().filter(|point| point.instant == t1).collect();
    assert_eq!(at_review.len(), 2);
    assert!(at_review[0].retrievability < 100.0 && at_review[0].stability == 5.0);
    assert_eq!((at_review[1].retrievability, at_review[1].stability), (100.0, 15.0));
    let now = NOW as f64;
    let at_now: Vec<_> = month.points.iter().filter(|point| point.instant == now).collect();
    assert_eq!(at_now.len(), 2);
    assert!(!at_now[0].future && at_now[1].future);
    assert_eq!(at_now[0].retrievability, at_now[1].retrievability);
    assert!(month.points.iter().all(|p| p.instant == now || p.future == (p.instant > now)));
    assert_eq!(month.points.last().unwrap().days_since_first, 30.0);
    // Tooltip texts (C7.6).
    let drop = at_review[0];
    assert_eq!(drop.elapsed_text, "15 days");
    assert_eq!(drop.stability_text, "5 days");
    assert_eq!(drop.retrievability_text, two_decimal_percent(drop.retrievability));
    assert_eq!(built.desired_retention_text, "90%");
    // y_min and the retention line.
    let lowest = month.points.iter().map(|p| p.retrievability).fold(f64::INFINITY, f64::min);
    assert_eq!(month.y_min, (100.0 - 1.2 * (100.0 - lowest)).max(0.0));
    assert_eq!(month.shows_retention, widen(0.9) * 100.0 > month.y_min);
}

#[test]
fn c7_4_retrievability_formula_with_the_stored_decay() {
    let reviews = [review(10.0, 10.0, 10.0)];
    let built = curve(&reviews, NOW, 0.05, 0.9).unwrap();
    let point = built.ranges[0].points.last().unwrap();
    let decay = widen(0.05);
    let expected = ((point.days_since_review / 10.0) * (0.9f64.powf(1.0 / -decay) - 1.0) + 1.0).powf(-decay) * 100.0;
    assert_eq!(point.retrievability, expected);
}

#[test]
fn c7_5_same_instant_reviews_count_as_the_newest() {
    let older = CurveReview {
        instant: NOW - 86_400,
        interval_secs: 86_400,
        stability: 1.0,
    };
    let newer = CurveReview { stability: 3.0, ..older };
    let built = curve(&[older, newer], NOW, 0.5, 0.9).unwrap();
    let points = &built.ranges[0].points;
    assert_eq!(points.iter().filter(|p| p.instant == older.instant as f64).count(), 1);
    assert!(points.iter().all(|p| p.stability == 3.0));
}

#[test]
fn c7_5_a_review_and_now_at_the_range_end() {
    // M = 7 exactly: reviews 7 days and 0 days ago, the newest interval 0.
    let reviews = [review(7.0, 1.0, 2.0), review(0.0, 0.0, 4.0)];
    let built = curve(&reviews, NOW, 0.5, 0.9).unwrap();
    let week = &built.ranges[0];
    assert_eq!(week.span_days, 7.0);
    let at_end: Vec<_> = week.points.iter().filter(|p| p.instant == NOW as f64).collect();
    // The review's two points, then the now pair; no third range-end point.
    assert_eq!(at_end.len(), 4);
    assert_eq!(at_end.iter().map(|p| p.future).collect::<Vec<_>>(), [false, false, false, true]);
    assert_eq!(at_end[1].retrievability, 100.0);
    assert_eq!(week.points.last().unwrap().instant, NOW as f64);
}

#[test]
fn c7_5_reviews_after_now_and_span_zero() {
    // Every curve review after now (a fast clock): no point is past, no pair.
    let ahead = CurveReview {
        instant: NOW + 3_600,
        interval_secs: 86_400,
        stability: 2.0,
    };
    let built = curve(&[ahead], NOW, 0.5, 0.9).unwrap();
    let first = &built.ranges[0].points;
    assert_eq!((first[0].instant, first[0].retrievability), (ahead.instant as f64, 100.0));
    assert!(first.len() > 2);
    assert!(built.ranges.iter().flat_map(|r| &r.points).all(|p| p.future));
    // Span 0: one learning answer with interval 0 in the same second.
    let now = CurveReview {
        instant: NOW,
        interval_secs: 0,
        stability: 0.5,
    };
    let built = curve(&[now], NOW, 0.5, 0.9).unwrap();
    assert_eq!(built.maximum_days, 0.0);
    let points = &built.ranges[0].points;
    assert_eq!(points.len(), 3);
    assert_eq!(points.iter().map(|p| p.future).collect::<Vec<_>>(), [false, false, true]);
    assert!(points.iter().all(|p| p.instant == NOW as f64));
}

// C1: building the snapshot writes nothing (card row, col.mod, WAL).
#[test]
fn c1_building_the_snapshot_writes_nothing() {
    let db = Db::new();
    db.card(2, 2, 105, 10, r#"{"pos":2}"#);
    db.row((NOW - 30 * 86_400, 3, -600, 0, 2500, 4_000, 0));
    db.row((NOW - 20 * 86_400, 3, 10, -600, 2500, 3_000, 1));
    let mut wal = db.path.clone().into_os_string();
    wal.push("-wal");
    let state = || {
        let rows = db
            .db
            .writer()
            .execute_raw(
                "SELECT (SELECT mod FROM col), data, mod, usn FROM cards WHERE id = ?1",
                &[SqlValue::Integer(CARD)],
            )
            .unwrap()
            .rows;
        (rows, fs::read(&db.path).unwrap(), fs::read(&wal).ok())
    };
    let before = state();
    let info = db.info();
    assert_eq!(info.raw.memory_state_source, Some(MemorySource::Computed));
    let _ = info.debug_json();
    assert_eq!(state(), before);
}

#[test]
fn c4_1_rows_of_a_new_card_and_no_ease_row() {
    let db = Db::new();
    db.card(0, 0, 7, 0, "{}");
    let info = db.info();
    let keys: Vec<_> = info.rows.iter().map(|row| row.key).collect();
    assert_eq!(
        keys,
        [
            StatsKey::Added,
            StatsKey::Position,
            StatsKey::Reviews,
            StatsKey::Lapses,
            StatsKey::CardType,
            StatsKey::NoteType,
            StatsKey::Deck,
            StatsKey::Preset,
            StatsKey::CardId,
            StatsKey::NoteId,
        ]
    );
    assert_eq!(row(&info, StatsKey::Position), Some("7"));
    assert_eq!(row(&info, StatsKey::CardId), Some("1788249600238"));
    assert_eq!(row(&info, StatsKey::NoteId), Some(NOTE.to_string().as_str()));
    assert_eq!(row(&info, StatsKey::Added), Some("2026-09-01"));
    assert!(info.history.is_empty() && info.curve.is_none());
    assert_eq!((info.raw.due_date, info.raw.memory_state), (None, None));
}

#[test]
fn c4_1_rows_of_a_review_card_in_order() {
    let db = Db::new();
    db.card(2, 2, 105, 10, r#"{"pos":2,"s":45.0,"d":5.5,"lrt":1893837600,"cd":"{\"a\":1}"}"#);
    db.exec("UPDATE cards SET factor = 1305 WHERE id = ?1", &[SqlValue::Integer(CARD)]);
    db.row((NOW - 30 * 86_400, 3, -600, 0, 2500, 3_000, 0));
    db.row((NOW - 20 * 86_400, 3, 10, -600, 2500, 6_000, 1));
    db.row((NOW - 19 * 86_400, 0, 0, 0, 0, 0, 4));
    let info = db.info();
    let keys: Vec<_> = info.rows.iter().map(|row| row.key.as_str()).collect();
    assert_eq!(
        keys,
        [
            "added",
            "first_review",
            "latest_review",
            "due",
            "position",
            "interval",
            "stability",
            "difficulty",
            "retrievability",
            "reviews",
            "lapses",
            "average_time",
            "total_time",
            "card_type",
            "note_type",
            "deck",
            "preset",
            "card_id",
            "note_id",
            "custom_data",
        ]
    );
    assert_eq!(row(&info, StatsKey::Stability), Some("1.48 months (45 days)"));
    assert_eq!(row(&info, StatsKey::Difficulty), Some("50%"));
    assert_eq!(row(&info, StatsKey::AverageTime), Some("4.5 seconds"));
    assert_eq!(row(&info, StatsKey::TotalTime), Some("9 seconds"));
    assert_eq!(row(&info, StatsKey::Interval), Some("10 days"));
    assert_eq!(row(&info, StatsKey::CustomData), Some("a=1"));
    assert_eq!(row(&info, StatsKey::FirstReview), Some("2029-12-16"));
    // Due from a day number keeps now's time of day: 5 days ahead.
    assert_eq!(info.raw.due_date, Some(NOW + 5 * 86_400));
    assert_eq!(row(&info, StatsKey::Due), Some("2030-01-20"));
    assert!(info.dependencies.due_from_day_number);
    // No row shows the ease, whatever the factor (FSRS always runs).
    assert!(info.rows.iter().all(|row| !row.value.contains("130.5")));
    assert_eq!(info.raw.ease, 1305);
    // The history, newest first, with its texts.
    let kinds: Vec<_> = info.history.iter().map(|e| e.texts.kind.as_str()).collect();
    assert_eq!(kinds, ["Manual", "Review", "Learn"]);
    let learn = &info.history[2];
    assert_eq!(
        (
            learn.texts.interval.as_str(),
            learn.texts.ease.as_str(),
            learn.texts.time.as_str(),
            learn.texts.rating.as_str(),
            learn.texts.date_time.as_str(),
        ),
        ("10 minutes", "250%", "3s", "3", "2029-12-16 @ 10:00")
    );
    assert_eq!((info.history[0].texts.interval.as_str(), info.history[0].texts.rating.as_str()), ("0 seconds", "0"));
}

#[test]
fn c4_3_times_of_rated_and_unrated_rows() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    db.row((NOW - 30 * 86_400, 0, 0, 0, 0, 5_000, 4));
    let info = db.info();
    assert_eq!((info.raw.total_secs, info.raw.average_secs), (0.0, 0.0));
    assert_eq!(row(&info, StatsKey::TotalTime), None);
    db.row((NOW - 20 * 86_400, 3, 10, 0, 2500, 70_000, 1));
    let info = db.info();
    assert_eq!(row(&info, StatsKey::AverageTime), Some("1.25 minutes"));
    assert_eq!(info.raw.total_secs, 75.0);
}

#[test]
fn c4_3_filtered_deck_with_its_home_deck() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    db.exec(
        "UPDATE cards SET did = ?1, odid = ?2, odue = 103 WHERE id = ?3",
        &[SqlValue::Integer(CRAM), SqlValue::Integer(VERBS), SqlValue::Integer(CARD)],
    );
    let info = db.info();
    assert_eq!(row(&info, StatsKey::Deck), Some("Cram (Lang::Verbs)"));
    assert_eq!(info.raw.original_deck.as_deref(), Some("Lang::Verbs"));
    assert_eq!(info.raw.preset, "Slow");
    assert_eq!(info.raw.due_date, Some(NOW + 3 * 86_400));
    assert_eq!(
        (info.dependencies.deck, info.dependencies.home_deck),
        (DeckId(CRAM), DeckId(VERBS))
    );
    // odid equal to did names no home deck.
    db.exec(
        "UPDATE cards SET did = ?1, odid = ?1 WHERE id = ?2",
        &[SqlValue::Integer(LANG), SqlValue::Integer(CARD)],
    );
    assert_eq!(db.info().raw.original_deck, None);
}

#[test]
fn c3_3_epoch_due_and_wrapped_position() {
    let db = Db::new();
    db.card(1, 1, 1_894_702_000, 0, r#"{"pos":3000000000}"#);
    let info = db.info();
    assert_eq!(info.raw.due_date, Some(1_894_702_000));
    assert_eq!(info.raw.due_position, Some(-1_294_967_296));
    assert_eq!(row(&info, StatsKey::Position), Some("-1294967296"));
    assert_eq!(row(&info, StatsKey::Due), Some("2030-01-15"));
    assert!(!info.dependencies.due_from_day_number);
}

#[test]
fn c1_a_cd_that_is_not_a_string_reads_as_no_keys() {
    for data in [
        r#"{"pos":3,"s":10,"d":5,"dr":0.9,"lrt":1894000000,"cd":5}"#,
        r#"{"pos":3,"s":10,"d":5,"dr":0.9,"lrt":1894000000,"cd":null}"#,
        r#"[3,10,5,0.9,null,1894000000,5]"#,
    ] {
        let db = Db::new();
        db.card(2, 2, 105, 10, data);
        db.row((NOW - 30 * 86_400, 3, -600, 0, 2500, 3_000, 0));
        db.row((NOW - 20 * 86_400, 3, 10, -600, 2500, 3_000, 1));
        let info = db.info();
        assert_eq!(info.raw.due_position, None, "{data}");
        assert_eq!(info.raw.desired_retention, None);
        assert_eq!(info.raw.custom_data, "");
        assert_eq!(info.raw.memory_state_source, Some(MemorySource::Computed));
        assert_eq!(row(&info, StatsKey::CustomData), None);
    }
    // `data` that is not TEXT reads as empty (`model/row-records` R4).
    let db = Db::new();
    db.card(0, 0, 7, 0, "{}");
    db.exec(
        "UPDATE cards SET data = CAST(?1 AS BLOB) WHERE id = ?2",
        &[SqlValue::Text(r#"{"pos":42,"cd":"{}"}"#.into()), SqlValue::Integer(CARD)],
    );
    let info = db.info();
    assert_eq!((info.raw.due_position, info.raw.custom_data.as_str()), (Some(7), ""));
    // A stored `{}` is kept and shows an empty Custom Data row.
    let db = Db::new();
    db.card(2, 2, 105, 10, r#"{"cd":"{}"}"#);
    let info = db.info();
    assert_eq!(info.raw.custom_data, "{}");
    assert_eq!(row(&info, StatsKey::CustomData), Some(""));
}

fn retrievability(info: &CardInfo) -> f32 {
    info.raw.fsrs_retrievability.unwrap()
}

fn expected_r(stability: f32, elapsed: u32, decay: f32) -> f32 {
    fsrs::current_retrievability(
        fsrs::MemoryState {
            stability,
            difficulty: 5.0,
        },
        elapsed as f32 / 86_400.0,
        decay,
    )
}

#[test]
fn c3_5_last_review_rules() {
    let db = Db::new();
    db.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0}"#);
    db.row((NOW - 10 * 86_400, 3, 10, 0, 2500, 3_000, 1));
    // A later cram row does not count; a type 259 row with factor 0 does.
    db.row((NOW - 5 * 86_400, 3, 0, 0, 0, 3_000, 3));
    assert_eq!(retrievability(&db.info()), expected_r(20.0, 10 * 86_400, 0.5));
    db.row((NOW - 2 * 86_400, 3, 0, 0, 0, 3_000, 259));
    let info = db.info();
    assert_eq!(retrievability(&info), expected_r(20.0, 2 * 86_400, 0.5));
    // The history reads that row as a cram row's kind (C2).
    assert_eq!(info.history[0].kind, RevlogKind::Filtered);
    // lrt wins; an lrt after now wraps to a retrievability near 0; i64::MIN
    // gives the elapsed time of lrt 0.
    db.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0,"lrt":1894701500,"decay":0.2}"#);
    assert_eq!(retrievability(&db.info()), expected_r(20.0, 100, 0.2));
    db.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0,"lrt":1894701700}"#);
    let ahead = retrievability(&db.info());
    assert_eq!(ahead, expected_r(20.0, 4_294_967_196, 0.5));
    assert!(ahead < 0.05);
    db.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0,"lrt":-9223372036854775808}"#);
    let minimum = retrievability(&db.info());
    db.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0,"lrt":0}"#);
    assert_eq!(minimum, retrievability(&db.info()));
    assert_eq!(minimum, expected_r(20.0, NOW as u32, 0.5));
}

#[test]
fn c3_5_a_computed_state_uses_the_stored_decay_of_its_list() {
    let db = Db::new();
    let mut params = vec![
        0.212, 1.2931, 2.3065, 8.2956, 6.4133, 0.8334, 3.0194, 0.001, 1.8722, 0.1666, 0.796, 1.4835, 0.0614, 0.2629,
        1.6483, 0.6014, 1.8729, 0.5425, 0.0912, 0.0658,
    ];
    params.push(0.05);
    db.preset(1, |preset| preset.config.set_fsrs_params_6(params));
    db.card(2, 2, 105, 10, "{}");
    db.row((NOW - 30 * 86_400, 3, -600, 0, 2500, 3_000, 0));
    db.row((NOW - 20 * 86_400, 3, 10, -600, 2500, 3_000, 1));
    let info = db.info();
    let state = info.raw.memory_state.unwrap();
    assert_eq!(info.raw.fsrs_params.len(), 21);
    let expected = fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: state.stability,
            difficulty: state.difficulty,
        },
        (20 * 86_400) as f32 / 86_400.0,
        0.05,
    );
    assert_eq!(retrievability(&info), expected);
}

#[test]
fn c6_3_per_entry_states_and_the_next_state_rule() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    db.row((NOW - 40 * 86_400, 3, -600, 0, 2500, 3_000, 0));
    db.row((NOW - 35 * 86_400, 3, 5, -600, 2500, 3_000, 1));
    // Between two kept rows: an unrated Review row and a Rescheduled row.
    db.row((NOW - 33 * 86_400, 0, 7, 5, 2500, 0, 1));
    db.row((NOW - 32 * 86_400, 0, 9, 5, 2500, 0, 5));
    db.row((NOW - 30 * 86_400, 3, 15, 5, 2500, 3_000, 1));
    // After the last kept row: a Set Due Date row.
    db.row((NOW - 29 * 86_400, 0, 20, 15, 2500, 0, 4));
    let info = db.info();
    let states: Vec<_> = info.history.iter().rev().map(|e| e.memory_state.unwrap()).collect();
    assert_eq!(states[2], states[4]);
    assert_eq!(states[3], states[4]);
    assert_eq!(states[5], states[4]);
    assert_ne!(states[1], states[4]);
    assert_eq!(
        info.history[0].texts.stability,
        time_span(widen(states[4].stability) * 86_400.0, SpanForm::Long, true, false)
    );
    // The curve skips the Manual and Rescheduled rows; the unrated Review
    // row is a curve review (C7.2).
    let curve = info.curve.unwrap();
    let month = curve.ranges.iter().find(|r| r.kind == RangeKind::FirstMonth).unwrap();
    let starts = month.points.iter().filter(|p| p.retrievability == 100.0).count();
    assert_eq!(starts, 4);
    // A Reset marker leaves no item: no state anywhere and no curve.
    db.row((NOW - 28 * 86_400, 0, 0, 20, 0, 0, 4));
    let info = db.info();
    assert!(info.history.iter().all(|e| e.memory_state.is_none()));
    assert_eq!(info.raw.memory_state, None);
    assert!(info.curve.is_none());
}

#[test]
fn c11_each_unavailable_kind() {
    let db = Db::new();
    assert_eq!(db.kind(42), ErrorKind::NotFound);
    let unreadable_row = Db::new();
    unreadable_row.row((NOW - 86_400, 300, 1, 0, 2500, 0, 1));
    assert_eq!(unreadable_row.kind(CARD), ErrorKind::UnreadableRecord);
    let negative_factor = Db::new();
    negative_factor.row((NOW - 86_400, 3, 1, 0, -1, 0, 1));
    assert_eq!(negative_factor.kind(CARD), ErrorKind::UnreadableRecord);
    let unreadable_card = Db::new();
    unreadable_card.exec("UPDATE cards SET type = 7 WHERE id = ?1", &[SqlValue::Integer(CARD)]);
    assert_eq!(unreadable_card.kind(CARD), ErrorKind::UnreadableRecord);
    // An unreadable record comes before a missing one.
    unreadable_row.exec("DELETE FROM notes WHERE id = ?1", &[SqlValue::Integer(NOTE)]);
    assert_eq!(unreadable_row.kind(CARD), ErrorKind::UnreadableRecord);
    let note = Db::new();
    note.exec("DELETE FROM notes WHERE id = ?1", &[SqlValue::Integer(NOTE)]);
    assert_eq!(note.kind(CARD), ErrorKind::MissingNote);
    let notetype = Db::new();
    notetype.exec("DELETE FROM notetypes WHERE id = 1788249600186", &[]);
    assert_eq!(notetype.kind(CARD), ErrorKind::MissingNotetype);
    let template = Db::new();
    template.exec("DELETE FROM templates WHERE ntid = 1788249600186 AND ord = 1", &[]);
    assert_eq!(template.kind(SIBLING), ErrorKind::MissingTemplate);
    let cloze = Db::new();
    assert!(!cloze.info_with(CLOZE_CARD, &utc()).unwrap().raw.card_type.is_empty());
    cloze.exec("DELETE FROM templates WHERE ntid = 1788249600189", &[]);
    assert_eq!(cloze.kind(CLOZE_CARD), ErrorKind::MissingTemplate);
    let deck = Db::new();
    deck.exec("UPDATE cards SET did = 99 WHERE id = ?1", &[SqlValue::Integer(CARD)]);
    assert_eq!(deck.kind(CARD), ErrorKind::MissingDeck);
    let home = Db::new();
    home.exec(
        "UPDATE cards SET did = ?1, odid = 98 WHERE id = ?2",
        &[SqlValue::Integer(CRAM), SqlValue::Integer(CARD)],
    );
    assert_eq!(home.kind(CARD), ErrorKind::MissingHomeDeck);
    let filtered = Db::new();
    filtered.exec(
        "UPDATE cards SET did = ?1, odid = ?1 WHERE id = ?2",
        &[SqlValue::Integer(CRAM), SqlValue::Integer(CARD)],
    );
    assert_eq!(filtered.kind(CARD), ErrorKind::FilteredHomeDeck);
    let preset = Db::new();
    preset.exec(
        "UPDATE cards SET did = ?1 WHERE id = ?2",
        &[SqlValue::Integer(VERBS), SqlValue::Integer(CARD)],
    );
    preset.exec("DELETE FROM deck_config WHERE id = ?1", &[SqlValue::Integer(SLOW)]);
    assert_eq!(preset.kind(CARD), ErrorKind::MissingPreset);
    // The cutoff and the list are checked for every card, a new one included.
    let cutoff = Db::new();
    cutoff.card(0, 0, 3, 0, "{}");
    cutoff.preset(1, |preset| preset.config.ignore_revlogs_before_date = "2026-13-01".into());
    assert_eq!(cutoff.kind(CARD), ErrorKind::InvalidCutoff);
    let list = Db::new();
    list.card(0, 0, 3, 0, "{}");
    list.preset(1, |preset| preset.config.set_fsrs_params_6(vec![0.5; 18]));
    assert_eq!(list.kind(CARD), ErrorKind::FsrsFailed);
    let nan = Db::new();
    nan.card(2, 2, 105, 10, r#"{"s":20.0,"d":5.0}"#);
    let mut params = vec![1.0f32; 21];
    params[4] = f32::NAN;
    nan.preset(1, |preset| preset.config.set_fsrs_params_6(params));
    assert_eq!(nan.kind(CARD), ErrorKind::FsrsFailed);
}

#[test]
fn c10_dependencies_and_the_change_test() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    db.row((NOW - 30 * 86_400, 3, -600, 0, 2500, 3_000, 0));
    let info = db.info();
    let deps = &info.dependencies;
    assert_eq!((deps.card, deps.note), (CardId(CARD), NoteId(NOTE)));
    assert_eq!((deps.deck, deps.home_deck), (DeckId(LANG), DeckId(LANG)));
    assert_eq!(deps.preset, DeckConfigId(1));
    assert_eq!(deps.revlog, [RevlogId((NOW - 30 * 86_400) * 1000)]);
    let reader = db.db.open_reader(ReaderRole::Query).unwrap();
    let mut changes = ChangeSet::empty(OpSource::UserUndoable);
    assert!(!deps.affected(&changes, &reader).unwrap());
    changes.cards.push(CardChange {
        id: CardId(CARD + 1),
        note_id: NoteId(NOTE),
        kind: ChangeKind::Updated,
        memory_state_only: false,
    });
    assert!(!deps.affected(&changes, &reader).unwrap());
    changes.decks.push(DeckChange {
        id: DeckId(LANG),
        kind: ChangeKind::Updated,
    });
    assert!(deps.affected(&changes, &reader).unwrap());
    let mut rollover = ChangeSet::empty(OpSource::UserUndoable);
    rollover.config_keys.push(ConfigKeyChange {
        key: "rollover".into(),
        kind: ChangeKind::Updated,
    });
    assert!(deps.affected(&rollover, &reader).unwrap());
    let mut offset = ChangeSet::empty(OpSource::UserUndoable);
    offset.config_keys.push(ConfigKeyChange {
        key: "creationOffset".into(),
        kind: ChangeKind::Updated,
    });
    assert!(deps.affected(&offset, &reader).unwrap());
    let mut other = ChangeSet::empty(OpSource::UserUndoable);
    other.config_keys.push(ConfigKeyChange {
        key: "curDeck".into(),
        kind: ChangeKind::Updated,
    });
    assert!(!deps.affected(&other, &reader).unwrap());
    // An added row of this card, then one of another card.
    db.row((NOW - 10 * 86_400, 3, 5, 0, 2500, 3_000, 1));
    let mut added = ChangeSet::empty(OpSource::UserUndoable);
    added.revlog_added.push(RevlogId((NOW - 10 * 86_400) * 1000));
    let reader = db.db.open_reader(ReaderRole::Query).unwrap();
    assert!(deps.affected(&added, &reader).unwrap());
    assert!(!deps.affected_with(&added, |_| Some(CardId(CARD + 1))));
    let mut removed = ChangeSet::empty(OpSource::UserUndoable);
    removed.revlog_removed.push(deps.revlog[0]);
    assert!(deps.affected_with(&removed, |_| None));
}

// C8: the dump's members, order and bytes.
#[test]
fn c8_debug_dump_bytes() {
    let db = Db::new();
    db.card(2, 2, 105, 10, r#"{"pos":2,"s":45.0,"d":5.5,"lrt":1893837600}"#);
    db.row((NOW - 20 * 86_400, 3, 10, -600, 2500, 6_000, 1));
    let info = db.info();
    let dump = info.debug_json();
    let f32_text = |value: f32| {
        let mut text = String::new();
        meguri_model::json_text::push_f32(&mut text, value);
        text
    };
    let r = f32_text(info.raw.fsrs_retrievability.unwrap());
    // One graded interday row: an incomplete history whose starting state
    // is the SM-2 conversion (`sched/fsrs-items` I5).
    let state = info.history[0].memory_state.unwrap();
    let (s, d) = (f32_text(state.stability), f32_text(state.difficulty));
    let head = format!(
        r#"{{
  "stats": {{
    "card_id": {CARD},
    "note_id": {NOTE},
    "custom_data": "",
    "added": 1788249600,
    "first_review": 1892973600,
    "latest_review": 1892973600,
    "due_date": 1895133600,
    "due_position": 2,
    "interval": 10,
    "ease": 2650,
    "reviews": 2,
    "lapses": 0,
    "average_secs": 6.0,
    "total_secs": 6.0,
    "desired_retention": null,
    "fsrs_retrievability": {r},
    "memory_state": {{
      "stability": 45.0,
      "difficulty": 5.5
    }},
    "fsrs_params": [],
    "memory_state_source": "stored"
  }},
  "card": {{
    "id": {CARD},
    "nid": {NOTE},
    "did": {LANG},
    "ord": 0,
    "mod": 1788681600,
    "usn": -1,
    "type": 2,
    "queue": 2,
    "due": 105,
    "ivl": 10,
    "factor": 2650,
    "reps": 2,
    "lapses": 0,
    "left": 0,
    "odue": 0,
    "odid": 0,
    "flags": 0,
    "data": "{{\"pos\":2,\"s\":45.0,\"d\":5.5,\"lrt\":1893837600}}"
  }},
  "revlog": [
    {{
      "row": {{
        "id": 1892973600000,
        "cid": {CARD},
        "usn": -1,
        "ease": 3,
        "ivl": 10,
        "lastIvl": -600,
        "factor": 2500,
        "time": 6000,
        "type": 1
      }},
      "info": {{
        "time": 1892973600,
        "review_kind": 1,
        "button_chosen": 3,
        "interval": 864000,
        "last_interval": 600,
        "ease": 2500,
        "taken_secs": 6.0,
        "memory_state": {{
          "stability": {s},
          "difficulty": {d}
        }}
      }}
    }}
  ],
  "deck": {{
"#
    );
    assert!(dump.starts_with(&head), "{dump}");
    assert!(dump.ends_with("\n  \"rollover\": 4\n}"), "{dump}");
    assert!(!dump.contains("\"homeDeck\""));
    let preset_at = dump.find("\n  \"preset\": {\n").expect("a preset member");
    assert!(dump.find("\n  \"deck\": {\n").unwrap() < preset_at);
    // No name the user typed: the deck's and the preset's names are left out.
    for name in ["\"name\"", "\"desc\"", "Lang", "Default", "Pair", "Forward"] {
        assert!(!dump.contains(name), "{name}");
    }
    // Every line is indented by two spaces per level.
    let mut depth = 0usize;
    for line in dump.lines() {
        let closing = line.trim_start().starts_with(['}', ']']);
        let indent = line.len() - line.trim_start().len();
        assert_eq!(indent, 2 * (depth - usize::from(closing)), "{line}");
        if closing {
            depth -= 1;
        }
        if line.ends_with('{') || line.ends_with('[') {
            depth += 1;
        }
    }
    assert_eq!(depth, 0);
    // A filtered card has its home deck, and the search terms stay.
    db.exec(
        "UPDATE cards SET did = ?1, odid = ?2, odue = 103 WHERE id = ?3",
        &[SqlValue::Integer(CRAM), SqlValue::Integer(LANG), SqlValue::Integer(CARD)],
    );
    let dump = db.info().debug_json();
    assert!(dump.contains("\n  \"homeDeck\": {\n"));
    assert!(dump.contains("deck:Lang"));
    // A rollover nested past 128 levels is written as a string, not walked.
    let deep = format!("{}4{}", "[".repeat(200), "]".repeat(200));
    db.exec(
        "UPDATE config SET val = CAST(?1 AS BLOB) WHERE KEY = 'rollover'",
        &[SqlValue::Text(deep.clone())],
    );
    let dump = db.info().debug_json();
    assert!(dump.ends_with(&format!("\n  \"rollover\": \"{deep}\"\n}}")), "{dump}");
}

// C6.3 with an incomplete history: a row before the first kept row has no
// state, and the first kept row has the SM-2 starting state (I5).
#[test]
fn c6_3_incomplete_history_starts_from_the_sm2_state() {
    let db = Db::new();
    db.card(2, 2, 105, 20, "{}");
    db.row((NOW - 40 * 86_400, 0, 3, 0, 2500, 0, 4));
    db.row((NOW - 30 * 86_400, 3, 10, 3, 2500, 3_000, 1));
    db.row((NOW - 10 * 86_400, 3, 20, 10, 2500, 3_000, 1));
    let info = db.info();
    let ascending: Vec<_> = info.history.iter().rev().map(|e| e.memory_state).collect();
    assert_eq!(ascending[0], None);
    let reader = db.db.open_reader(ReaderRole::Query).unwrap();
    let preset = meguri_sched::model::preset::PresetView::for_config(&reader, DeckConfigId(1)).unwrap();
    let start = fsrs::FSRS::new(preset.parameters())
        .unwrap()
        .memory_state_from_sm2(2.5, 10.0, preset.fields().historical_retention)
        .unwrap();
    let first = ascending[1].unwrap();
    assert_eq!((first.stability, first.difficulty), (start.stability, start.difficulty));
    assert_ne!(ascending[2], ascending[1]);
    assert_eq!(info.raw.memory_state, ascending[2]);
}

#[test]
fn c4_3_one_rated_row_of_70_seconds_and_a_hidden_retrievability() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    db.row((NOW - 20 * 86_400, 3, 10, 0, 2500, 70_000, 1));
    let info = db.info();
    assert_eq!(row(&info, StatsKey::AverageTime), Some("1.17 minutes"));
    assert_eq!(row(&info, StatsKey::TotalTime), Some("1.17 minutes"));
    // A retrievability of 0 shows no row; stability and difficulty stay.
    db.card(2, 2, 105, 10, r#"{"s":1e-45,"d":5.0,"lrt":0}"#);
    let info = db.info();
    assert_eq!(info.raw.fsrs_retrievability, Some(0.0));
    assert_eq!(row(&info, StatsKey::Retrievability), None);
    assert!(row(&info, StatsKey::Stability).is_some() && row(&info, StatsKey::Difficulty).is_some());
}

#[test]
fn c7_5_a_review_at_the_range_end_before_now() {
    // M = 10.5 for one answer 10 days ago with a 1-day interval; First Week
    // ends at day 7, so a review on day 7 keeps its two points and the range
    // end adds none; now (day 10) is outside.
    let reviews = [review(10.0, 1.0, 1.0), review(3.0, 1.0, 2.0)];
    let built = curve(&reviews, NOW, 0.5, 0.9).unwrap();
    let week = &built.ranges[0];
    let end = reviews[0].instant as f64 + 7.0 * 86_400.0;
    assert_eq!(reviews[1].instant as f64, end);
    let at_end: Vec<_> = week.points.iter().filter(|p| p.instant == end).collect();
    assert_eq!(at_end.len(), 2);
    assert_eq!(at_end[1].retrievability, 100.0);
    assert_eq!(week.points.last().unwrap().instant, end);
    assert!(week.points.iter().all(|p| !p.future));
}

#[test]
fn c7_5_far_off_review_instants_keep_the_read_bounded() {
    for instant in [NOW - 2_000_000_000_000, i64::MIN / 1000, 0] {
        let reviews = [CurveReview {
            instant,
            interval_secs: 86_400,
            stability: 3.0,
        }];
        let built = curve(&reviews, NOW, 0.5, 0.9).unwrap();
        for range in &built.ranges {
            assert!(range.points.len() <= 100_005, "{instant}: {}", range.points.len());
            ascending(&range.points);
        }
    }
}

// C8: the deck and preset members parse back to their schema-11 objects
// without the left-out names.
#[test]
fn c8_dump_parses_back_to_the_records() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    let dump = db.info().debug_json();
    let parsed = meguri_testkit::observe::parse_json(&dump).unwrap();
    let reader = db.db.open_reader(ReaderRole::Query).unwrap();
    let deck = reader.deck(DeckId(LANG)).unwrap().unwrap();
    let mut expected = meguri_testkit::observe::parse_json(&meguri_model::schema11::deck_to_json(&deck)).unwrap();
    for name in ["name", "desc", "usn"] {
        expected.as_object_mut().unwrap().remove(name);
    }
    assert_eq!(parsed["deck"], expected);
    let preset = reader.deck_config(DeckConfigId(1)).unwrap().unwrap();
    let mut expected =
        meguri_testkit::observe::parse_json(&meguri_model::schema11::deck_config_to_json(&preset)).unwrap();
    expected.as_object_mut().unwrap().remove("name");
    assert_eq!(parsed["preset"], expected);
    let members: Vec<&str> = dump
        .lines()
        .filter(|line| line.starts_with("  \""))
        .map(|line| line.trim_start().split('"').nth(1).unwrap())
        .collect();
    assert_eq!(members, ["stats", "card", "revlog", "deck", "preset", "rollover"]);
}

// C8: a rollover stored as an INTEGER or a REAL is written as that number;
// a preset whose stored extra JSON is nested past the reader's bound keeps
// its name out of the dump.
#[test]
fn c8_dump_rollover_storage_classes_and_a_deep_preset() {
    let db = Db::new();
    db.card(2, 2, 105, 10, "{}");
    for (sql, text) in [
        ("UPDATE config SET val = 6 WHERE KEY = 'rollover'", "6"),
        ("UPDATE config SET val = 2.5 WHERE KEY = 'rollover'", "2.5"),
    ] {
        db.exec(sql, &[]);
        let dump = db.info().debug_json();
        assert!(dump.ends_with(&format!("\n  \"rollover\": {text}\n}}")), "{dump}");
    }
    let reader = db.db.open_reader(ReaderRole::Query).unwrap();
    let preset = reader.deck_config(DeckConfigId(1)).unwrap().unwrap();
    let mut bytes = preset.config.to_bytes().into_owned();
    let deep = format!("{{\"deep\":{}0{}}}", "[".repeat(200), "]".repeat(200));
    // Field 255 (`other`), length-delimited.
    bytes.extend([0xFA, 0x0F]);
    let mut length = deep.len();
    while length >= 0x80 {
        bytes.push((length as u8 & 0x7F) | 0x80);
        length >>= 7;
    }
    bytes.push(length as u8);
    bytes.extend(deep.as_bytes());
    db.exec(
        "UPDATE deck_config SET config = ?1 WHERE id = 1",
        &[SqlValue::Blob(bytes)],
    );
    let dump = db.info().debug_json();
    assert!(!dump.contains("Default"), "{dump}");
    // The deep member is kept as one string inside the filtered object.
    assert!(dump.contains("\"deep\": "), "{dump}");
    assert!(dump.contains("\n  \"preset\": {\n"));
}
