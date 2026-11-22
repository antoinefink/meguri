//! `stats/history-reads` H3.4, H4, H6 to H9 and H11 to H13 (U288) and
//! `stats/statistics-page` S2: the round 11 extras inside the Hourly
//! Breakdown, Answer Buttons, Future Due and Card Counts records, and the
//! page's parts computed per request (By Answer, retention per day, the
//! oldest day, every year's activity, the strip's numbers, the cards
//! behind a day). They have no oracle and are tested by construction
//! (H20): each against the Anki members of a record, another Meguri read
//! with the same rule (the Today page's numbers, the activity heatmap), or
//! a direct count of the rows or cards a test writes. Names and values are
//! made up.

use std::collections::{BTreeMap, BTreeSet};

use meguri_engine::CancelToken;
use meguri_model::clock::UtcOffset;
use meguri_model::ids::DeckId;
use meguri_model::proto::deck::DeckKind;
use meguri_stats::graphs::encoding::{Decoder, Encoder, MemberKind};
use meguri_stats::graphs::records::FutureDueRecord;
use meguri_stats::graphs::records::answer_buttons::AnswerButtonsRecord;
use meguri_stats::graphs::records::card_counts::{CardCountsRecord, StateCounts};
use meguri_stats::graphs::records::hourly::{HourlyRecord, PassFail, RetentionHours};
use meguri_stats::graphs::records::retention::{RetentionCounts, RetentionRecord};
use meguri_stats::graphs::records::reviews::ReviewsRecord;
use meguri_stats::graphs::{Columns, Context, Graph, GraphData, GraphRequest, GraphResult, History, Scope, ScopeError};
use meguri_stats::page::{self, OldestDay, PageEnd, PageHeader, PageMessage, PageRequest, Part, PartOutcome, PartValue};
use meguri_stats::parts::activity::ActivityDaysPart;
use meguri_stats::parts::by_answer::ByAnswerPart;
use meguri_stats::parts::day_cards::{DayCards, DayCardsRequest, DayKind, stats_day_cards};
use meguri_stats::parts::page_day;
use meguri_stats::parts::retention_by_day::{RetentionByDayPart, RetentionDay};
use meguri_stats::parts::studied_today::StudiedTodayPart;
use meguri_storage::{Cell, ConfigValue, ReadRows, RowKey, Table};

use crate::graphs_common::{CRAM, Fixture, LANG, NOTE, VERBS, revlog_cells};

const DAY: i64 = 86_400;
/// A card id no card has: a deleted card's rows count in the Collection
/// scope only.
const GONE: i64 = 4_242;

/// A review-log row as stored: Review, `lastIvl` 30, `ease` 3, `factor`
/// 2500 and `time` 0 unless changed.
#[derive(Clone, Copy, Debug)]
struct Row {
    id: i64,
    ease: i64,
    last_ivl: i64,
    factor: i64,
    time: i64,
    kind: i64,
}

fn row(id: i64) -> Row {
    Row {
        id,
        ease: 3,
        last_ivl: 30,
        factor: 2_500,
        time: 0,
        kind: 1,
    }
}

impl Row {
    fn ease(self, ease: i64) -> Row {
        Row { ease, ..self }
    }
    fn last(self, last_ivl: i64) -> Row {
        Row { last_ivl, ..self }
    }
    fn time(self, time: i64) -> Row {
        Row { time, ..self }
    }
    fn kind(self, kind: i64) -> Row {
        Row { kind, ..self }
    }
    fn factor(self, factor: i64) -> Row {
        Row { factor, ..self }
    }
}

/// The fixture without its cards or rows.
fn empty() -> Fixture {
    Fixture::with_raw(&["DELETE FROM cards"])
}

/// Writes `rows` on a deleted card.
fn put_rows(f: &Fixture, rows: &[Row]) {
    let cells: Vec<(i64, Vec<Cell>)> = rows
        .iter()
        .map(|r| {
            let int = Cell::Integer;
            (r.id, revlog_cells(r.id, GONE, int(r.ease), int(r.last_ivl), int(r.factor), int(r.time), int(r.kind)))
        })
        .collect();
    f.write_raw(move |scope| {
        for (id, row) in cells {
            scope.restore(Table::Revlog, &RowKey::Id(id), Some(&row))?;
        }
        Ok(())
    });
}

fn request(f: &Fixture, history: History, graphs: &[Graph]) -> Box<GraphData> {
    let request = GraphRequest {
        scope: Scope::Collection,
        history,
        graphs: graphs.to_vec(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    match f.host.stats().request(&f.collection(), f.host.derived(), &request).unwrap() {
        GraphResult::Data(data) => data,
        other => panic!("not data: {other:?}"),
    }
}

fn take<T: Clone + 'static>(data: &GraphData, at: usize) -> T {
    data.records[at].as_any().downcast_ref::<T>().expect("the record of its graph").clone()
}

/// The fixture's clock is UTC (offset 0), so a row's V5.1 hour is its
/// second of the UTC day over 3,600.
fn utc_hour(t: i64) -> usize {
    (t.rem_euclid(DAY) / 3_600) as usize
}

/// The id of a row at whole second `t`.
fn at_second(t: i64) -> i64 {
    t * 1_000
}

/// The parts a page request returns, by part, with the header they carry.
fn page_parts(f: &Fixture, scope: Scope, history: History, parts: &[Part]) -> (BTreeMap<Part, PartOutcome>, PageHeader) {
    page_parts_with(f, scope, history, parts, Columns::Cached)
}

fn page_parts_with(
    f: &Fixture,
    scope: Scope,
    history: History,
    parts: &[Part],
    columns: Columns,
) -> (BTreeMap<Part, PartOutcome>, PageHeader) {
    let request = PageRequest {
        scope,
        history,
        parts: parts.to_vec(),
        refresh: 1,
        cancel: CancelToken::new(),
        columns,
    };
    let mut outcomes = BTreeMap::new();
    let mut header = None;
    let end = page::request(f.host.stats(), &f.collection(), f.host.derived(), &request, &mut |message| {
        if let PageMessage::Part(part) = message {
            header.get_or_insert_with(|| (*part.header).clone());
            outcomes.insert(part.part, part.outcome);
        }
    })
    .unwrap();
    assert_eq!(end, PageEnd::Complete);
    let header = header.unwrap_or_else(|| panic!("no part of {parts:?} returned"));
    (outcomes, header)
}

fn part_value(f: &Fixture, scope: Scope, history: History, part: Part) -> PartValue {
    match page_parts(f, scope, history, &[part]).0.remove(&part) {
        Some(PartOutcome::Data(value)) => value,
        other => panic!("{part:?}: {other:?}"),
    }
}

fn by_answer(f: &Fixture, history: History) -> (ReviewsRecord, ByAnswerPart) {
    let reviews: ReviewsRecord = take(&request(f, history, &[Graph::Reviews]), 0);
    match part_value(f, Scope::Collection, history, Part::ByAnswer) {
        PartValue::ByAnswer(answers) => (reviews, answers),
        other => panic!("{other:?}"),
    }
}

// H6. Answers per day.

#[test]
fn h6_answers_per_day_add_up_to_the_counted_rows_less_the_unrated_ones() {
    let f = empty();
    let n = f.next_day_at();
    let today = n - 3_600;
    let yesterday = n - DAY - 3_600;
    let rows = [
        // Today: each answer once, Again twice, and two unrated rows.
        row(at_second(today)).ease(1).time(1_000),
        row(at_second(today) + 1).ease(1).time(2_000).kind(0),
        row(at_second(today) + 2).ease(2).time(3_000).kind(2),
        row(at_second(today) + 3).ease(3).time(4_000).last(5),
        row(at_second(today) + 4).ease(4).time(5_000).kind(3),
        row(at_second(today) + 5).ease(0).time(6_000),
        row(at_second(today) + 6).ease(5).time(7_000),
        // Manual and Rescheduled rows count nowhere in Reviews, rated or not.
        row(at_second(today) + 7).ease(3).time(8_000).kind(4),
        row(at_second(today) + 8).ease(0).time(9_000).kind(5),
        // Yesterday: only an unrated row, so no By Answer key.
        row(at_second(yesterday)).ease(0).time(500),
        // Ten days ago: Good twice.
        row(at_second(n - 10 * DAY)).time(10),
        row(at_second(n - 10 * DAY) + 1).time(20),
    ];
    put_rows(&f, &rows);
    let (reviews, answers) = by_answer(&f, History::AllHistory);
    assert_eq!(answers.count_by_answer, BTreeMap::from([(-10, [0, 0, 2, 0]), (0, [2, 1, 1, 1])]));
    assert_eq!(answers.time_by_answer, BTreeMap::from([(-10, [0, 0, 30, 0]), (0, [3_000, 3_000, 4_000, 5_000])]));
    assert_eq!(answers.unrated, 3);
    // Per day, By Answer is By Type less that day's unrated rows.
    let unrated_rows = BTreeMap::from([(-1, (1, 500)), (0, (2, 13_000))]);
    for (day, kinds) in &reviews.count {
        let total: i64 = kinds.values().iter().sum();
        let time: i64 = reviews.time[day].values().iter().sum();
        let (less, less_time) = unrated_rows.get(day).copied().unwrap_or((0, 0));
        let counts = answers.count_by_answer.get(day).copied().unwrap_or_default();
        let answer_time = answers.time_by_answer.get(day).copied().unwrap_or_default();
        assert_eq!(counts.iter().sum::<i64>(), total - less, "day {day}");
        assert_eq!(answer_time.iter().sum::<i64>(), time - less_time, "day {day}");
    }
}

#[test]
fn h6_unrated_counts_only_the_window_and_by_answer_times_never_wrap() {
    let f = empty();
    let n = f.next_day_at();
    let rows = [
        row(at_second(n - 400 * DAY)).ease(0),
        row(at_second(n - 3_600)).ease(0),
        // Two Easy rows whose times pass 2^32 ms together (V9's rule).
        row(at_second(n - 3_600) + 1).ease(4).time(3_000_000_000),
        row(at_second(n - 3_600) + 2).ease(4).time(3_000_000_000),
    ];
    put_rows(&f, &rows);
    let (_, year) = by_answer(&f, History::Last12Months);
    let (_, all) = by_answer(&f, History::AllHistory);
    assert_eq!((year.unrated, all.unrated), (1, 2));
    assert_eq!(year.time_by_answer, BTreeMap::from([(0, [0, 0, 0, 6_000_000_000])]));
    assert_eq!(year.count_by_answer, BTreeMap::from([(0, [0, 0, 0, 2])]));
}

#[test]
fn h6_by_answer_takes_the_reviews_records_day_keys_cut_to_32_bits() {
    let f = empty();
    let n = f.next_day_at();
    // Today, a row whose day index passes 32 bits, and the largest id: every
    // row is Good, so By Answer has exactly By Type's days.
    let rows = [row(at_second(n - 3_600)), row(1_791_856_800_000_000), row(1_791_856_800_000_000_000), row(i64::MAX)];
    put_rows(&f, &rows);
    let (reviews, answers) = by_answer(&f, History::AllHistory);
    assert!(answers.count_by_answer.keys().eq(reviews.count.keys()));
    for (day, kinds) in &reviews.count {
        assert_eq!(answers.count_by_answer[day][2], kinds.values().iter().sum::<i64>(), "day {day}");
    }
}

#[test]
fn h6_twenty_years_of_by_answer_fit_g6s_limit_as_a_part_of_their_own() {
    // G6's case for the `reviews` record (7,305 consecutive days, 150 rated
    // rows a day, stored times of 5,000 to 60,000 ms), By Answer's members:
    // in the record they passed the limit; as their own part they fit.
    let mut part = ByAnswerPart::default();
    for day in -7_304..=0 {
        let spread = i64::from(day).rem_euclid(97);
        let time = |k: i64| [40, 40, 40, 30][k as usize] * (5_000 + (spread * 571 + k * 9_001) % 55_001);
        part.count_by_answer.insert(day, [40, 40, 40, 30]);
        part.time_by_answer.insert(day, [time(0), time(1), time(2), time(3)]);
    }
    part.unrated = 12;
    let mut out = Encoder::new();
    part.encode(&mut out);
    let bytes = out.into_bytes();
    assert!(bytes.len() <= 262_144, "{}", bytes.len());
    // Member order: the map's four counts then four times, then `unrated`.
    let mut decoder = Decoder::new(&bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers; 8]).unwrap();
    assert_eq!(decoder.integer().unwrap(), 12);
    assert!(decoder.is_done());
    let mut decoded = ByAnswerPart {
        unrated: 12,
        ..ByAnswerPart::default()
    };
    for (at, day) in map.keys.iter().enumerate() {
        let member = |m: usize| map.integers[m][at];
        let day = i32::try_from(*day).unwrap();
        decoded.count_by_answer.insert(day, [0, 1, 2, 3].map(member));
        decoded.time_by_answer.insert(day, [4, 5, 6, 7].map(member));
    }
    assert_eq!(decoded, part);
}

// H7. The six-month range and remembered by hour.

fn hourly_and_buttons(f: &Fixture, history: History) -> (HourlyRecord, AnswerButtonsRecord, RetentionRecord) {
    let data = request(f, history, &[Graph::Hourly, Graph::AnswerButtons, Graph::Retention]);
    (take(&data, 0), take(&data, 1), take(&data, 2))
}

#[test]
fn h7_six_months_holds_the_rows_from_its_bound_on() {
    let f = empty();
    let n = f.next_day_at();
    let bound = n - 182 * DAY;
    // Just before the bound (whole second `bound` − 1), at it, and 10 days
    // after it; the first is in `one_year` only.
    let rows = [row(at_second(bound) - 1).ease(1), row(at_second(bound)).ease(2), row(at_second(bound + 10 * DAY))];
    put_rows(&f, &rows);
    for history in [History::Last12Months, History::AllHistory] {
        let (hourly, buttons, _) = hourly_and_buttons(&f, history);
        let total = |hours: &[meguri_stats::graphs::records::hourly::HourCounts; 24]| -> (u64, u64) {
            hours.iter().fold((0, 0), |(t, c), h| (t + h.total, c + h.correct))
        };
        assert_eq!(total(&hourly.six_months), (2, 2), "{history:?}");
        assert_eq!(total(&hourly.one_year), (3, 2), "{history:?}");
        assert_eq!(total(&hourly.three_months), (0, 0), "{history:?}");
        assert_eq!(buttons.six_months.mature, [0, 1, 1, 0], "{history:?}");
        assert_eq!(buttons.one_year.mature, [1, 1, 1, 0], "{history:?}");
        assert_eq!(buttons.three_months.mature, [0, 0, 0, 0], "{history:?}");
    }
}

fn kept(hours: &RetentionHours) -> PassFail {
    hours.iter().fold(PassFail::default(), |sum, hour| PassFail {
        passed: sum.passed + hour.passed,
        failed: sum.failed + hour.failed,
    })
}

fn of_period(counts: &RetentionCounts) -> PassFail {
    PassFail {
        passed: counts.young_passed + counts.mature_passed,
        failed: counts.young_failed + counts.mature_failed,
    }
}

/// Rows of every kind and rating over 400 days, none at or after `N`, at
/// different hours of the day.
fn mixed_rows(n: i64) -> Vec<Row> {
    let mut rows = Vec::new();
    for back in [0_i64, 1, 6, 29, 30, 31, 89, 90, 120, 181, 182, 200, 364, 365, 399] {
        for (k, (kind, ease, last_ivl, factor)) in [
            (1, 3, 30, 2_500),
            (1, 1, 10, 2_500),
            (0, 3, 1, 2_500),
            (0, 1, -600, 2_500),
            (2, 1, -86_400, 2_500),
            (3, 3, 10, 0),
            (3, 1, 10, 2_500),
            (4, 3, 15, 2_500),
            (1, 0, 10, 2_500),
            (1, 5, 25, 2_500),
        ]
        .into_iter()
        .enumerate()
        {
            let t = n - back * DAY - 1 - (k as i64) * 4_000;
            rows.push(Row {
                id: at_second(t),
                ease,
                last_ivl,
                factor,
                time: 0,
                kind,
            });
        }
    }
    rows
}

fn retention_by_day(f: &Fixture, scope: Scope, history: History) -> RetentionByDayPart {
    match part_value(f, scope, history, Part::RetentionByDay) {
        PartValue::RetentionByDay(part) => part,
        other => panic!("{other:?}"),
    }
}

/// H4's counts summed over the days in `days`.
fn days_sum(part: &RetentionByDayPart, days: impl std::ops::RangeBounds<i32>) -> RetentionCounts {
    part.days.range(days).fold(RetentionCounts::default(), |sum, (_, day)| RetentionCounts {
        young_passed: sum.young_passed + day.young_passed,
        young_failed: sum.young_failed + day.young_failed,
        mature_passed: sum.mature_passed + day.mature_passed,
        mature_failed: sum.mature_failed + day.mature_failed,
    })
}

#[test]
fn h7_remembered_by_hour_sums_to_the_retention_by_day_counts_of_the_same_rows() {
    let f = empty();
    let n = f.next_day_at();
    let rows = mixed_rows(n);
    put_rows(&f, &rows);
    let (hourly, _, _) = hourly_and_buttons(&f, History::AllHistory);
    let by_day = retention_by_day(&f, Scope::Collection, History::AllHistory);
    let by_hour = &hourly.retention_by_hour;
    // No row is at or after `N` or on a day's last second, so a range from
    // its bound `N − d × 86,400` on holds the rows of days −(d − 1) to 0
    // (H1, V5.2).
    for (hours, days) in [
        (&by_hour.one_month, 30),
        (&by_hour.three_months, 90),
        (&by_hour.six_months, 182),
        (&by_hour.one_year, 365),
    ] {
        assert_eq!(kept(hours), of_period(&days_sum(&by_day, -(days - 1)..=0)), "{days} days");
    }
    assert_eq!(kept(by_hour.all_time.as_ref().unwrap()), of_period(&days_sum(&by_day, ..)));
    // Every hour of the ranges, against a direct count by V4.1's rule.
    for (hours, days) in [(&by_hour.three_months, 90), (&by_hour.six_months, 182), (&by_hour.one_year, 365)] {
        let mut expected = [PassFail::default(); 24];
        for r in rows.iter().filter(|r| r.id / 1_000 >= n - days * DAY) {
            let cram = r.kind == 3 && r.factor == 0;
            let long = r.last_ivl >= 1 || r.last_ivl <= -86_400;
            if r.ease >= 1 && !cram && (r.kind == 1 || long) {
                let hour = utc_hour(r.id / 1_000);
                if r.ease == 1 {
                    expected[hour].failed += 1;
                } else {
                    expected[hour].passed += 1;
                }
            }
        }
        assert_eq!(hours, &expected, "{days} days");
    }
}

#[test]
fn h7_remembered_by_hour_has_an_all_time_range_only_with_all_history() {
    let f = empty();
    let n = f.next_day_at();
    put_rows(&f, &[row(at_second(n - 3_600)), row(at_second(n - 3_600) + 1).ease(1)]);
    let (year, _, _) = hourly_and_buttons(&f, History::Last12Months);
    let (all, _, _) = hourly_and_buttons(&f, History::AllHistory);
    assert_eq!(year.retention_by_hour.all_time, None);
    let hour = utc_hour(n - 3_600);
    for hours in [&year.retention_by_hour.one_month, all.retention_by_hour.all_time.as_ref().unwrap()] {
        assert_eq!(hours[hour], PassFail { passed: 1, failed: 1 });
        assert_eq!(kept(hours), PassFail { passed: 1, failed: 1 });
    }
}

// H8. Reviews due young and mature; suspended cards by state.

/// A card of `Lang` as stored: (id, type, queue, due, ivl).
type Card = (i64, i64, i64, i64, i64);

fn put_cards(f: &Fixture, cards: &[Card]) {
    let rows: Vec<(i64, Vec<Cell>)> = cards
        .iter()
        .map(|&(id, card_type, queue, due, ivl)| {
            let int = Cell::Integer;
            let cells = vec![
                int(id),
                int(NOTE),
                int(LANG),
                int(0),
                int(0),
                int(-1),
                int(card_type),
                int(queue),
                int(due),
                int(ivl),
                int(2_500),
                int(1),
                int(0),
                int(0),
                int(0),
                int(0),
                int(0),
                Cell::text("{}"),
            ];
            (id, cells)
        })
        .collect();
    f.write_raw(move |scope| {
        for (id, cells) in rows {
            scope.restore(Table::Cards, &RowKey::Id(id), Some(&cells))?;
        }
        Ok(())
    });
}

#[test]
fn h8_due_young_and_mature_split_the_future_due_map_at_21_days() {
    let f = empty();
    let today = i64::from(request(&f, History::AllHistory, &[]).context.today);
    let n = f.next_day_at();
    put_cards(&f, &[
        // Day 2: a young and a mature review card.
        (1, 2, 2, today + 2, 20),
        (2, 2, 2, today + 2, 21),
        // Day 2: a learning card (`ivl` 0, young) and a relearning one
        // (`ivl` 30, mature), due as epoch seconds.
        (3, 1, 1, n + 2 * DAY + 60, 0),
        (4, 3, 1, n + 2 * DAY + 120, 30),
        // Overdue by 3 days: mature.
        (5, 2, 2, today - 3, 40),
        // Not counted: new, suspended, and buried for today.
        (6, 0, 0, 1, 0),
        (7, 2, -1, today + 2, 5),
        (8, 2, -2, today, 5),
        // Buried, due tomorrow: counted (C3.3), young.
        (9, 2, -3, today + 1, 5),
    ]);
    let due: FutureDueRecord = take(&request(&f, History::Last12Months, &[Graph::FutureDue]), 0);
    assert_eq!(due.due_days, BTreeMap::from([(-3, 1), (1, 1), (2, 4)]));
    assert_eq!(due.future_due_young, BTreeMap::from([(1, 1), (2, 2)]));
    for (day, cards) in &due.due_days {
        let young = due.future_due_young.get(day).copied().unwrap_or(0);
        assert!(young <= *cards, "day {day}");
    }
    assert!(due.future_due_young.keys().all(|day| due.due_days.contains_key(day)));
    // The Anki members are as before.
    assert!(due.have_backlog);
    assert_eq!(due.due_tomorrow, 1);
}

#[test]
fn h8_suspended_cards_by_their_type_bucket() {
    let f = empty();
    put_cards(&f, &[
        // Suspended: one of each type bucket, two young.
        (1, 0, -1, 1, 0),
        (2, 1, -1, 0, 0),
        (3, 3, -1, 0, 4),
        (4, 2, -1, 10, 20),
        (5, 2, -1, 10, 3),
        (6, 2, -1, 10, 21),
        // Active and buried cards are not suspended.
        (7, 2, 2, 10, 21),
        (8, 0, -2, 2, 0),
        (9, 2, -3, 10, 5),
    ]);
    let counts: CardCountsRecord = take(&request(&f, History::Last12Months, &[Graph::CardCounts]), 0);
    let suspended = counts.suspended_by_state;
    assert_eq!(
        suspended,
        StateCounts {
            new: 1,
            learning: 1,
            relearning: 1,
            young: 2,
            mature: 1,
            ..StateCounts::default()
        }
    );
    assert_eq!(counts.excluding_inactive.suspended, 6);
    // What H2's last day equals: every card but the suspended ones, by type,
    // buried cards in their state.
    let remaining = |all: u32, less: u32| all - less;
    let inc = counts.including_inactive;
    assert_eq!(
        [
            remaining(inc.new, suspended.new),
            remaining(inc.learning, suspended.learning),
            remaining(inc.relearning, suspended.relearning),
            remaining(inc.young, suspended.young),
            remaining(inc.mature, suspended.mature),
        ],
        [1, 0, 0, 1, 1]
    );
}

// Cards in any deck, and rows on them.

/// Writes a card of the fixture's note as stored: (id, did, odid, type,
/// queue, ivl).
fn put_deck_cards(f: &Fixture, cards: &[(i64, i64, i64, i64, i64, i64)]) {
    let rows: Vec<(i64, Vec<Cell>)> = cards
        .iter()
        .map(|&(id, did, odid, card_type, queue, ivl)| {
            let int = Cell::Integer;
            let cells = vec![
                int(id),
                int(NOTE),
                int(did),
                int(0),
                int(0),
                int(-1),
                int(card_type),
                int(queue),
                int(5),
                int(ivl),
                int(2_500),
                int(1),
                int(0),
                int(0),
                int(0),
                int(odid),
                int(0),
                Cell::text("{}"),
            ];
            (id, cells)
        })
        .collect();
    f.write_raw(move |scope| {
        for (id, cells) in rows {
            scope.restore(Table::Cards, &RowKey::Id(id), Some(&cells))?;
        }
        Ok(())
    });
}

/// Writes `rows` on card `cid` (which may name no card).
fn put_rows_on(f: &Fixture, cid: i64, rows: &[Row]) {
    let cells: Vec<(i64, Vec<Cell>)> = rows
        .iter()
        .map(|r| {
            let int = Cell::Integer;
            (r.id, revlog_cells(r.id, cid, int(r.ease), int(r.last_ivl), int(r.factor), int(r.time), int(r.kind)))
        })
        .collect();
    f.write_raw(move |scope| {
        for (id, row) in cells {
            scope.restore(Table::Revlog, &RowKey::Id(id), Some(&row))?;
        }
        Ok(())
    });
}

/// Sets a normal deck's own desired retention (`sched/presets` V7).
fn set_deck_retention(f: &Fixture, deck: i64, retention: f32) {
    f.write(move |scope| {
        let mut stored = scope.deck(DeckId(deck))?.expect("the fixture's deck");
        match &mut stored.kind.kind {
            DeckKind::Normal(normal) => normal.desired_retention = Some(retention),
            DeckKind::Filtered(_) => panic!("a normal deck"),
        }
        scope.put(&stored)?;
        Ok(())
    });
}

// H4 and H3.4. Retention and Forgotten per day, with the desired retention.

fn period_and_days(by_day: &RetentionByDayPart, retention: &RetentionRecord) -> Vec<(RetentionCounts, RetentionCounts)> {
    vec![
        (retention.today, days_sum(by_day, 0..=0)),
        (retention.yesterday, days_sum(by_day, -1..=-1)),
        (retention.week, days_sum(by_day, -6..=0)),
        (retention.month, days_sum(by_day, -29..=0)),
        (retention.year, days_sum(by_day, -364..=0)),
        (retention.all_time.unwrap(), days_sum(by_day, ..)),
    ]
}

#[test]
fn h4_retention_by_day_sums_to_each_v4_period_and_differs_only_by_the_edge_rows() {
    let f = empty();
    let n = f.next_day_at();
    // No row at or after `N`, and none on a day's last second.
    put_rows(&f, &mixed_rows(n));
    let retention: RetentionRecord = take(&request(&f, History::AllHistory, &[Graph::Retention]), 0);
    let by_day = retention_by_day(&f, Scope::Collection, History::AllHistory);
    for (period, days) in period_and_days(&by_day, &retention) {
        assert_eq!(period, days);
    }
    // Forgotten on a day is its fails.
    for day in by_day.days.values() {
        assert_eq!(day.forgotten(), day.young_failed + day.mature_failed);
    }
    // Rows on the edges: at `N − 86,400` (V4.2's today, H1's day −1), at `N`
    // and after it (in no V4.2 period, on day 0).
    put_rows(&f, &[
        row(at_second(n - DAY)).last(5),
        row(at_second(n)).ease(1),
        row(at_second(n + 3_600)),
    ]);
    let retention: RetentionRecord = take(&request(&f, History::AllHistory, &[Graph::Retention]), 0);
    let by_day = retention_by_day(&f, Scope::Collection, History::AllHistory);
    let pairs = period_and_days(&by_day, &retention);
    let (today, day_0) = pairs[0];
    assert_eq!(today.young_passed, day_0.young_passed + 1);
    assert_eq!(today.mature_failed + 1, day_0.mature_failed);
    assert_eq!(today.mature_passed + 1, day_0.mature_passed);
    let (yesterday, day_1) = pairs[1];
    assert_eq!(yesterday.young_passed + 1, day_1.young_passed);
    assert_eq!((yesterday.mature_passed, yesterday.mature_failed), (day_1.mature_passed, day_1.mature_failed));
    let (all, days) = pairs[5];
    assert_eq!(all.mature_failed + 1, days.mature_failed);
    assert_eq!(all.mature_passed + 1, days.mature_passed);
    assert_eq!(all.young_passed, days.young_passed);
}

#[test]
fn h4_retention_by_day_reads_only_the_window_and_follows_the_deck_rule() {
    let f = empty();
    let n = f.next_day_at();
    // A card in `Lang`, one in `Lang::Verbs`, one in `Cram` from `Lang`.
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30), (3, CRAM, LANG, 2, 2, 30)]);
    put_rows_on(&f, 1, &[row(at_second(n - 3_600)), row(at_second(n - 400 * DAY))]);
    put_rows_on(&f, 2, &[row(at_second(n - 3_600) + 1).ease(1)]);
    put_rows_on(&f, 3, &[row(at_second(n - 2 * DAY))]);
    put_rows(&f, &[row(at_second(n - 3_600) + 2).last(3)]);
    let day = |young_passed, young_failed, mature_passed, mature_failed| RetentionCounts {
        young_passed,
        young_failed,
        mature_passed,
        mature_failed,
    };
    let all = retention_by_day(&f, Scope::Collection, History::AllHistory);
    let year = retention_by_day(&f, Scope::Collection, History::Last12Months);
    assert_eq!(days_sum(&all, -400..=-400), day(0, 0, 1, 0));
    assert_eq!(days_sum(&year, ..), day(1, 0, 2, 1));
    assert!(year.days.keys().all(|day| *day > -367));
    // Deck scopes (G2.1): `Lang` holds its subdeck and the card in `Cram`;
    // the deleted card's row is the Collection scope's only.
    let lang = retention_by_day(&f, Scope::Deck(DeckId(LANG)), History::Last12Months);
    assert_eq!(days_sum(&lang, ..), day(0, 0, 2, 1));
    let verbs = retention_by_day(&f, Scope::Deck(DeckId(VERBS)), History::Last12Months);
    assert_eq!(days_sum(&verbs, ..), day(0, 0, 0, 1));
    let cram = retention_by_day(&f, Scope::Deck(DeckId(CRAM)), History::Last12Months);
    assert_eq!(days_sum(&cram, ..), day(0, 0, 1, 0));
}

#[test]
fn h3_4_the_desired_retention_weights_follow_each_rows_home_deck() {
    let f = empty();
    let n = f.next_day_at();
    set_deck_retention(&f, LANG, 0.85);
    set_deck_retention(&f, VERBS, 0.8);
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30), (3, CRAM, LANG, 2, 2, 30)]);
    put_rows_on(&f, 1, &[row(at_second(n - 3_600)), row(at_second(n - 3_600) + 1).ease(1)]);
    put_rows_on(&f, 2, &[row(at_second(n - 3_600) + 2)]);
    // A card in a filtered deck uses its home deck's value (V7).
    put_rows_on(&f, 3, &[row(at_second(n - DAY - 3_600))]);
    // A deleted card's row counts, with no retention in force (H4.2); an
    // unrated row and a cram row are not H4.1's rows.
    put_rows(&f, &[
        row(at_second(n - 3_600) + 3),
        row(at_second(n - 3_600) + 4).ease(0),
        row(at_second(n - 3_600) + 5).kind(3).factor(0),
    ]);
    let weights = |part: &RetentionByDayPart, at: i32| -> (f64, u64, u64) {
        let day: RetentionDay = part.days[&at];
        (day.desired_sum, day.desired_rows, day.young_passed + day.young_failed + day.mature_passed + day.mature_failed)
    };
    let (lang, verbs) = (f64::from(0.85_f32), f64::from(0.8_f32));
    let all = retention_by_day(&f, Scope::Collection, History::Last12Months);
    assert_eq!(weights(&all, 0), (lang + lang + verbs, 3, 4));
    assert_eq!(weights(&all, -1), (lang, 1, 1));
    assert_eq!(all.retentions, [0.8_f32, 0.85_f32]);
    assert!(all.several);
    let only_verbs = retention_by_day(&f, Scope::Deck(DeckId(VERBS)), History::Last12Months);
    assert_eq!(weights(&only_verbs, 0), (verbs, 1, 1));
    assert_eq!(only_verbs.retentions, [0.8_f32]);
    assert!(!only_verbs.several);
    // A scope with cards and no counted row still gives its retentions, for
    // the line's mean (H3.4).
    let f = empty();
    set_deck_retention(&f, VERBS, 0.75);
    put_deck_cards(&f, &[(2, VERBS, 0, 0, 0, 0)]);
    let quiet = retention_by_day(&f, Scope::Collection, History::AllHistory);
    assert!(quiet.days.is_empty());
    assert_eq!(quiet.retentions, [0.75_f32]);
}

/// Builds the column caches and waits until they serve the requests, so
/// `Columns::Cached` reads them (and their overlay after later commits).
fn build_columns(f: &Fixture) {
    use meguri_engine::Readiness;
    meguri_stats::graphs::stats_home_idle(f.host.derived());
    f.host.derived().request_refresh(meguri_stats::graphs::COMPONENT);
    let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(120);
    while f.host.derived().readiness(meguri_stats::graphs::COMPONENT).unwrap().readiness != Readiness::Ready {
        assert!(meguri_engine::monotonic::now() < deadline, "the column caches never became ready");
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    f.host.derived().flush().unwrap();
    assert!(f.data(Scope::Collection, History::AllHistory).scope.from_caches());
}

// H9. The oldest day.

fn header(f: &Fixture, scope: Scope, history: History, columns: Columns) -> PageHeader {
    page_parts_with(f, scope, history, &[Part::CardCounts], columns).1
}

#[test]
fn h9_oldest_day_is_the_scopes_first_row_whatever_the_window() {
    let f = empty();
    let n = f.next_day_at();
    for columns in [Columns::Cached, Columns::Snapshot] {
        assert_eq!(header(&f, Scope::Collection, History::AllHistory, columns).oldest_day, OldestDay::NoRows);
    }
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30)]);
    put_rows_on(&f, 1, &[row(at_second(n - 500 * DAY - 3_600)), row(at_second(n - 3 * DAY))]);
    put_rows_on(&f, 2, &[row(at_second(n - 30 * DAY)).ease(0)]);
    // A deleted card's row, and rows with a negative id, which no part reads.
    put_rows(&f, &[row(at_second(n - 800 * DAY - 3_600)), row(-5), row(-86_400_000_000)]);
    build_columns(&f);
    let cases = [
        (Scope::Collection, -800),
        (Scope::Deck(DeckId(LANG)), -500),
        (Scope::Deck(DeckId(VERBS)), -30),
    ];
    for (scope, day) in cases {
        for history in [History::Last12Months, History::AllHistory] {
            for columns in [Columns::Cached, Columns::Snapshot] {
                let oldest = header(&f, scope.clone(), history, columns).oldest_day;
                assert_eq!(oldest, OldestDay::Day(day), "{scope:?} {history:?} {columns:?}");
            }
        }
    }
    assert_eq!(header(&f, Scope::Deck(DeckId(CRAM)), History::AllHistory, Columns::Cached).oldest_day, OldestDay::NoRows);
}

// H11. Activity, every year.

fn activity(f: &Fixture, scope: Scope, history: History) -> ActivityDaysPart {
    match part_value(f, scope, history, Part::ActivityDays) {
        PartValue::ActivityDays(part) => part,
        other => panic!("{other:?}"),
    }
}

#[test]
fn h11_equals_the_heatmaps_371_days_and_extends_to_every_year() {
    let f = empty();
    let n = f.next_day_at();
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30)]);
    let mut lang = Vec::new();
    let mut verbs = Vec::new();
    let mut gone = Vec::new();
    for (k, back) in [0_i64, 0, 1, 6, 52, 369, 370, 371, 400, 800, 1_500].into_iter().enumerate() {
        let id = at_second(n - back * DAY - 600) - k as i64;
        // AH2: every rating and type counts, `ease` 0 never.
        let shapes = [row(id), row(id - 100).ease(1).kind(0), row(id - 200).ease(2).kind(4), row(id - 300).ease(0)];
        match k % 3 {
            0 => lang.extend(shapes),
            1 => verbs.extend(shapes),
            _ => gone.extend(shapes),
        }
    }
    // A row after `N`: on day 0 (AH3, H1).
    lang.push(row(at_second(n + 7_200)));
    put_rows_on(&f, 1, &lang);
    put_rows_on(&f, 2, &verbs);
    put_rows(&f, &gone);
    let collection = f.collection();
    for (scope, deck) in [(Scope::Collection, None), (Scope::Deck(DeckId(VERBS)), Some(DeckId(VERBS)))] {
        let heatmap = meguri_engine::activity::activity_heatmap(&collection, deck).unwrap();
        for history in [History::Last12Months, History::AllHistory] {
            let part = activity(&f, scope.clone(), history);
            for (k, count) in heatmap.counts.iter().enumerate() {
                let day = -(k as i32);
                assert_eq!(part.days.get(&day).copied().unwrap_or(0), u64::from(*count), "{scope:?} {history:?} day {day}");
            }
            assert!(part.days.values().all(|count| *count > 0));
            // Every year: the rows older than the heatmap's window, by a
            // direct count.
            let rows: &[&[Row]] = match deck {
                None => &[&lang, &verbs, &gone],
                Some(_) => &[&verbs],
            };
            let mut older: BTreeMap<i32, u64> = BTreeMap::new();
            for r in rows.iter().flat_map(|rows| rows.iter()).filter(|r| r.ease >= 1) {
                let day = page_day(r.id, n);
                if day < -370 {
                    *older.entry(day).or_default() += 1;
                }
            }
            assert!(!older.is_empty());
            assert_eq!(part.days.range(..-370).map(|(d, c)| (*d, *c)).collect::<BTreeMap<_, _>>(), older);
        }
    }
}

#[test]
fn h11_days_are_anki_days_across_a_year_end_the_rollover_hour_and_a_time_zone_change() {
    let f = empty();
    // The fixture's clock: 2030-01-15T10:00:00Z, UTC, rollover hour 4, so
    // day 0 runs from 2030-01-15 04:00 to 2030-01-16 04:00.
    let noon = |days_before_today: i64| at_second(1_894_708_800 - days_before_today * DAY);
    assert_eq!(f.next_day_at(), 1_894_766_400);
    // Noon on 2029-12-30 to 2030-01-02: four days in a row over the year's
    // end, with no gap.
    let year_end: Vec<Row> = (13..=16).map(|back| row(noon(back))).collect();
    // 2030-01-14 02:00Z, before that day's rollover: day −2 at hour 4.
    let early = row(at_second(1_894_586_400));
    // 2030-01-14 23:30Z: day −1 in UTC.
    let late = row(at_second(1_894_663_800));
    put_rows(&f, &[year_end.as_slice(), &[early, late]].concat());
    let days = |f: &Fixture| activity(f, Scope::Collection, History::AllHistory).days;
    assert_eq!(days(&f), BTreeMap::from([(-16, 1), (-15, 1), (-14, 1), (-13, 1), (-2, 1), (-1, 1)]));
    // Rollover at midnight: 02:00 is on its own calendar day, −1.
    f.write(|scope| {
        scope.set_config("rollover", &ConfigValue::Integer(0))?;
        Ok(())
    });
    assert_eq!(days(&f), BTreeMap::from([(-16, 1), (-15, 1), (-14, 1), (-13, 1), (-1, 2)]));
    f.write(|scope| {
        scope.set_config("rollover", &ConfigValue::Integer(4))?;
        Ok(())
    });
    // Nine hours east: 23:30Z is 08:30 on 2030-01-15 local, today; the
    // other rows keep their days.
    f.clock.set_utc_offset(UtcOffset::from_seconds(9 * 3_600).unwrap());
    let (outcomes, header) = page_parts(&f, Scope::Collection, History::AllHistory, &[Part::ActivityDays]);
    assert_eq!(header.context.next_day_at, 1_894_734_000);
    match &outcomes[&Part::ActivityDays] {
        PartOutcome::Data(PartValue::ActivityDays(part)) => {
            assert_eq!(part.days, BTreeMap::from([(-16, 1), (-15, 1), (-14, 1), (-13, 1), (-1, 1), (0, 1)]));
        }
        other => panic!("{other:?}"),
    }
}

// H12. The strip's numbers.

fn studied(f: &Fixture, scope: Scope) -> StudiedTodayPart {
    match part_value(f, scope, History::Last12Months, Part::StudiedToday) {
        PartValue::StudiedToday(part) => part,
        other => panic!("{other:?}"),
    }
}

#[test]
fn h12_all_decks_equals_the_today_pages_numbers_odd_values_included() {
    let f = empty();
    let n = f.next_day_at();
    let s = (n - DAY) * 1_000;
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30), (3, CRAM, LANG, 2, 2, 30)]);
    put_rows_on(&f, 1, &[
        // B2: a row at `S` is out, one after it in.
        row(s).time(1),
        row(s + 1).time(1_000),
        // Types 4 and 5 never count; a stored type of 260 (low byte 4) or 9
        // does, and so do odd times, as stored.
        row(s + 2).kind(4).time(2_000),
        row(s + 3).kind(5).time(3_000),
        row(s + 4).kind(260).time(4_000),
        row(s + 5).kind(9).ease(0).time(-50),
        row(s + 6).time(5_000_000_000),
    ]);
    put_rows_on(&f, 2, &[row(s + 7).time(7_000)]);
    put_rows_on(&f, 3, &[row(s + 8).time(8_000)]);
    put_rows(&f, &[row(s + 9).time(9_000), row(at_second(n + 3_600)).time(10_000)]);
    let numbers = meguri_engine::studied::studied_today_numbers(&f.collection()).unwrap();
    let all = studied(&f, Scope::Collection);
    assert_eq!((all.answers, all.cards, all.time_ms), (numbers.answers, numbers.cards, numbers.time_ms));
    assert_eq!(all, StudiedTodayPart {
        answers: 8,
        cards: 4,
        time_ms: 1_000 + 4_000 - 50 + 5_000_000_000 + 7_000 + 8_000 + 9_000 + 10_000,
    });
    // A deck: T12's rules over its rows only (G2.1: its subdeck, the card in
    // `Cram` from it).
    let lang = studied(&f, Scope::Deck(DeckId(LANG)));
    assert_eq!(lang, StudiedTodayPart {
        answers: 6,
        cards: 3,
        time_ms: 1_000 + 4_000 - 50 + 5_000_000_000 + 7_000 + 8_000,
    });
    assert_eq!(studied(&f, Scope::Deck(DeckId(VERBS))), StudiedTodayPart {
        answers: 1,
        cards: 1,
        time_ms: 7_000,
    });
    assert_eq!(studied(&f, Scope::Deck(DeckId(CRAM))), StudiedTodayPart {
        answers: 1,
        cards: 1,
        time_ms: 8_000,
    });
    // A Search scope is not offered: `unsupported_scope` (H1).
    let (outcomes, _) = page_parts(&f, Scope::Search("deck:Lang".into()), History::Last12Months, &[Part::StudiedToday]);
    assert!(matches!(outcomes[&Part::StudiedToday], PartOutcome::Failed(page::PartFailure::UnsupportedScope)));
}

// H13. The cards behind a day.

fn day_cards(f: &Fixture, context: &Context, scope: Scope, day: i32, kind: DayKind, columns: Columns) -> DayCards {
    let request = DayCardsRequest {
        scope,
        context: context.clone(),
        day,
        kind,
        cancel: CancelToken::new(),
        columns,
    };
    stats_day_cards(f.host.stats(), &f.collection(), f.host.derived(), &request).unwrap()
}

/// The rows a test wrote, with their card.
type Written = Vec<(i64, Row)>;

/// A plain scan of `written` for H13's kinds: the cards of `cards` with a
/// row of `kind` on `day`.
fn scan(written: &Written, cards: &BTreeSet<i64>, n: i64, day: i32, kind: DayKind) -> DayCards {
    let found: BTreeSet<i64> = written
        .iter()
        .filter(|(cid, r)| {
            let counted = match kind {
                // V1: every kind but Manual (4) and Rescheduled (5).
                DayKind::Reviewed => !matches!(r.kind, 4 | 5),
                DayKind::Answered => r.ease >= 1,
                DayKind::Forgotten => {
                    let cram = r.kind == 3 && r.factor == 0;
                    let long = r.last_ivl >= 1 || r.last_ivl <= -86_400;
                    r.ease == 1 && !cram && (r.kind == 1 || long)
                }
            };
            cards.contains(cid) && counted && page_day(r.id, n) == day
        })
        .map(|(cid, _)| *cid)
        .collect();
    if found.is_empty() { DayCards::NoCards } else { DayCards::Cards(found.into_iter().collect()) }
}

#[test]
fn h13_each_kinds_cards_are_exactly_those_a_plain_scan_finds() {
    let f = empty();
    let n = f.next_day_at();
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30), (3, CRAM, LANG, 2, 2, 30), (4, LANG, 0, 2, 2, 30)]);
    let mut written: Written = Vec::new();
    let shapes = |id: i64| {
        [
            row(id),
            row(id + 1).ease(1),
            row(id + 2).ease(1).last(-600).kind(0),
            row(id + 3).ease(1).kind(3).factor(0),
            row(id + 4).ease(0).kind(4),
            row(id + 5).ease(3).kind(5),
            row(id + 6).ease(1).last(-86_400).kind(2),
        ]
    };
    for (k, back) in [0_i64, 0, 1, 1, 3, 40, 400].into_iter().enumerate() {
        let card = 1 + (k as i64) % 4;
        // Some rows on a day's last second, and one after `N`.
        let base = if k == 2 { at_second(n - back * DAY) - 6 } else { at_second(n - back * DAY - 900) + 10 * k as i64 };
        for (at, shape) in shapes(base).into_iter().enumerate() {
            if (at + k) % 3 != 1 {
                written.push((card, shape));
            }
        }
    }
    written.push((1, row(at_second(n + 600))));
    written.push((GONE, row(at_second(n - 900) + 99).ease(1)));
    for card in [1, 2, 3, 4, GONE] {
        let rows: Vec<Row> = written.iter().filter(|(cid, _)| *cid == card).map(|(_, r)| *r).collect();
        put_rows_on(&f, card, &rows);
    }
    build_columns(&f);
    let context = header(&f, Scope::Collection, History::AllHistory, Columns::Cached).context;
    let every: BTreeSet<i64> = [1, 2, 3, 4].into();
    let scopes = [
        (Scope::Collection, every.clone()),
        (Scope::Deck(DeckId(LANG)), every.clone()),
        (Scope::Deck(DeckId(VERBS)), [2].into()),
        (Scope::Deck(DeckId(CRAM)), [3].into()),
    ];
    let mut listed = 0;
    for (scope, cards) in &scopes {
        for day in [1, 0, -1, -2, -3, -40, -399, -400, -401] {
            for kind in [DayKind::Reviewed, DayKind::Answered, DayKind::Forgotten] {
                let want = scan(&written, cards, n, day, kind);
                for columns in [Columns::Cached, Columns::Snapshot] {
                    let got = day_cards(&f, &context, scope.clone(), day, kind, columns);
                    assert_eq!(got, want, "{scope:?} day {day} {kind:?} {columns:?}");
                }
                listed += usize::from(matches!(want, DayCards::Cards(_)));
            }
        }
    }
    assert!(listed > 20, "{listed}");
}

#[test]
fn h13_a_commit_after_the_shown_result_never_refuses_the_read() {
    let f = empty();
    let n = f.next_day_at();
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30), (2, VERBS, 0, 2, 2, 30)]);
    put_rows_on(&f, 1, &[row(at_second(n - 900))]);
    build_columns(&f);
    let shown = header(&f, Scope::Collection, History::Last12Months, Columns::Cached).context;
    let before = f.dump();
    assert_eq!(day_cards(&f, &shown, Scope::Collection, 0, DayKind::Reviewed, Columns::Cached), DayCards::Cards(vec![1]));
    // A read writes nothing (H17).
    assert_eq!(f.dump(), before);
    // A background sync brings a row of the same day: the cards as they are
    // now, on the day the user picked.
    put_rows_on(&f, 2, &[row(at_second(n - 800))]);
    for columns in [Columns::Cached, Columns::Snapshot] {
        assert_eq!(day_cards(&f, &shown, Scope::Collection, 0, DayKind::Reviewed, columns), DayCards::Cards(vec![1, 2]));
    }
    // A card deleted since is not listed.
    f.write(|scope| {
        scope.remove_card(meguri_model::ids::CardId(1))?;
        Ok(())
    });
    assert_eq!(day_cards(&f, &shown, Scope::Collection, 0, DayKind::Reviewed, Columns::Cached), DayCards::Cards(vec![2]));
    assert_eq!(day_cards(&f, &shown, Scope::Collection, -5, DayKind::Reviewed, Columns::Cached), DayCards::NoCards);
    // A Search scope, a deck that names no deck, a cancelled read.
    assert_eq!(
        day_cards(&f, &shown, Scope::Search("deck:Lang".into()), 0, DayKind::Answered, Columns::Cached),
        DayCards::UnsupportedScope
    );
    assert_eq!(
        day_cards(&f, &shown, Scope::Deck(DeckId(77)), 0, DayKind::Answered, Columns::Cached),
        DayCards::ScopeError(ScopeError::UnknownDeck)
    );
    let cancelled = DayCardsRequest {
        scope: Scope::Collection,
        context: shown.clone(),
        day: 0,
        kind: DayKind::Reviewed,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    cancelled.cancel.cancel();
    let got = stats_day_cards(f.host.stats(), &f.collection(), f.host.derived(), &cancelled).unwrap();
    assert_eq!(got, DayCards::Cancelled);
}

#[test]
fn h13_an_unreadable_row_on_the_day_is_reported_and_one_on_another_day_is_not() {
    let f = empty();
    let n = f.next_day_at();
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30)]);
    put_rows_on(&f, 1, &[row(at_second(n - 900)), row(at_second(n - 3 * DAY))]);
    build_columns(&f);
    // The result on screen came before the unreadable row.
    let shown = header(&f, Scope::Collection, History::Last12Months, Columns::Cached).context;
    let bad = at_second(n - 10 * DAY);
    let cells = revlog_cells(bad, 1, Cell::Integer(300), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
    f.write_raw(move |scope| {
        scope.restore(Table::Revlog, &RowKey::Id(bad), Some(&cells))?;
        Ok(())
    });
    for columns in [Columns::Cached, Columns::Snapshot] {
        assert_eq!(day_cards(&f, &shown, Scope::Collection, -3, DayKind::Reviewed, columns), DayCards::Cards(vec![1]));
        assert!(matches!(
            day_cards(&f, &shown, Scope::Collection, -10, DayKind::Reviewed, columns),
            DayCards::UnreadableRow(row) if row.key == bad
        ));
    }
}

// H9 into H3.2: Knowledge's month samples reach back to the oldest day.

#[test]
fn h9_knowledge_month_samples_reach_back_to_the_headers_oldest_day() {
    use meguri_stats::history::{self, KnowledgeRequest, KnowledgeResult};
    let f = empty();
    let n = f.next_day_at();
    put_deck_cards(&f, &[(1, LANG, 0, 2, 2, 30)]);
    // The scope's oldest row is unrated (no memory item), 700 days ago; the
    // first graded review comes 300 days later.
    put_rows_on(&f, 1, &[
        row(at_second(n - 700 * DAY - 3_600)).ease(0).kind(4),
        row(at_second(n - 400 * DAY - 3_600)).kind(0).last(-600),
        row(at_second(n - 390 * DAY - 3_600)),
    ]);
    let (outcomes, header) = page_parts(&f, Scope::Deck(DeckId(LANG)), History::AllHistory, &[Part::KnowledgeOverTime]);
    assert_eq!(header.oldest_day, OldestDay::Day(-700));
    let PartOutcome::Data(PartValue::Knowledge(part)) = &outcomes[&Part::KnowledgeOverTime] else {
        panic!("{outcomes:?}");
    };
    let cold = |oldest_day: Option<i32>| {
        let request = KnowledgeRequest {
            scope: Scope::Deck(DeckId(LANG)),
            history: History::AllHistory,
            oldest_day,
            cancel: CancelToken::new(),
        };
        match history::knowledge_cold(&f.collection(), &header.context, &request).unwrap() {
            KnowledgeResult::Data(data) => data.knowledge,
            other => panic!("{other:?}"),
        }
    };
    // The page passes H9's day: its samples are a cold read's with it.
    assert_eq!(*part, cold(Some(-700)));
    // The first sample is the end of the oldest day's month, before any
    // estimate; without the day the leading empty months are dropped.
    let first = part.samples[0].day;
    assert!((-700..-669).contains(&first), "{first}");
    assert_eq!(part.samples[0].studied, 0);
    assert!(cold(None).samples[0].day > first);
}
