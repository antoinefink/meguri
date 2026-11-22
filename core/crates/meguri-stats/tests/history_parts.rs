//! `stats/history-reads` H6 to H8 (U288): the round 11 extras inside the
//! Reviews, Hourly Breakdown, Answer Buttons, Future Due and Card Counts
//! records. They have no oracle and are tested by construction (H20): each
//! extra against the Anki members of the same record, or a direct count of
//! the rows or cards a test writes. Names and values are made up.

use std::collections::BTreeMap;

use meguri_engine::CancelToken;
use meguri_stats::graphs::records::answer_buttons::AnswerButtonsRecord;
use meguri_stats::graphs::records::card_counts::{CardCountsRecord, StateCounts};
use meguri_stats::graphs::records::hourly::{HourlyRecord, PassFail, RetentionHours};
use meguri_stats::graphs::records::retention::{RetentionCounts, RetentionRecord};
use meguri_stats::graphs::records::reviews::ReviewsRecord;
use meguri_stats::graphs::records::FutureDueRecord;
use meguri_stats::graphs::{Columns, Graph, GraphData, GraphRequest, GraphResult, History, Scope};
use meguri_storage::{Cell, RowKey, Table};

use crate::graphs_common::{Fixture, LANG, NOTE, revlog_cells};

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

/// The id of a row at whole second `t`.
fn at_second(t: i64) -> i64 {
    t * 1_000
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
    let reviews: ReviewsRecord = take(&request(&f, History::AllHistory, &[Graph::Reviews]), 0);
    assert_eq!(reviews.count_by_answer, BTreeMap::from([(-10, [0, 0, 2, 0]), (0, [2, 1, 1, 1])]));
    assert_eq!(reviews.time_by_answer, BTreeMap::from([(-10, [0, 0, 30, 0]), (0, [3_000, 3_000, 4_000, 5_000])]));
    assert_eq!(reviews.unrated, 3);
    // Per day, By Answer is By Type less that day's unrated rows.
    let unrated_rows = BTreeMap::from([(-1, (1, 500)), (0, (2, 13_000))]);
    for (day, kinds) in &reviews.count {
        let total: i64 = kinds.values().iter().sum();
        let time: i64 = reviews.time[day].values().iter().sum();
        let (less, less_time) = unrated_rows.get(day).copied().unwrap_or((0, 0));
        let answers = reviews.count_by_answer.get(day).copied().unwrap_or_default();
        let answer_time = reviews.time_by_answer.get(day).copied().unwrap_or_default();
        assert_eq!(answers.iter().sum::<i64>(), total - less, "day {day}");
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
    let year: ReviewsRecord = take(&request(&f, History::Last12Months, &[Graph::Reviews]), 0);
    let all: ReviewsRecord = take(&request(&f, History::AllHistory, &[Graph::Reviews]), 0);
    assert_eq!((year.unrated, all.unrated), (1, 2));
    assert_eq!(year.time_by_answer, BTreeMap::from([(0, [0, 0, 0, 6_000_000_000])]));
    assert_eq!(year.count_by_answer, BTreeMap::from([(0, [0, 0, 0, 2])]));
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

#[test]
fn h7_remembered_by_hour_sums_to_the_retention_counts_of_the_same_rows() {
    let f = empty();
    let n = f.next_day_at();
    let rows = mixed_rows(n);
    put_rows(&f, &rows);
    let (hourly, _, retention) = hourly_and_buttons(&f, History::AllHistory);
    let by_hour = &hourly.retention_by_hour;
    // No row is at or after `N`, so a range from its bound on holds the
    // rows of the period with the same start (V4.2, V5.2).
    assert_eq!(kept(&by_hour.one_month), of_period(&retention.month));
    assert_eq!(kept(&by_hour.one_year), of_period(&retention.year));
    assert_eq!(kept(by_hour.all_time.as_ref().unwrap()), of_period(&retention.all_time.unwrap()));
    // The other two ranges, and every hour, against a direct count by
    // V4.1's rule.
    for (hours, days) in [(&by_hour.three_months, 90), (&by_hour.six_months, 182), (&by_hour.one_year, 365)] {
        let mut expected = [PassFail::default(); 24];
        for r in rows.iter().filter(|r| r.id / 1_000 >= n - days * DAY) {
            let cram = r.kind == 3 && r.factor == 0;
            let long = r.last_ivl >= 1 || r.last_ivl <= -86_400;
            if r.ease >= 1 && !cram && (r.kind == 1 || long) {
                let hour = ((r.id / 1_000).rem_euclid(DAY) / 3_600) as usize;
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
    let hour = (((n - 3_600).rem_euclid(DAY)) / 3_600) as usize;
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
