//! `stats/graphs-revlog` V1, V2, V4 to V7 and V9 (U99): the Reviews,
//! Retention, Hourly Breakdown and Answer Buttons records, on a schema-18
//! fixture copy whose own review-log rows are removed, then the oracle
//! goldens of V10's scenarios for those four parts. The Calendar record (V3)
//! is not built: the Statistics plan leaves it out. The round 11 extras
//! (`stats/history-reads` H6, H7) are U288's. Names and values are made up.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use meguri_engine::CancelToken;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::DeckId;
use meguri_stats::graphs::encoding::{Decoder, Encoder, MemberKind};
use meguri_stats::graphs::records::answer_buttons::{AnswerButtonsRecord, ButtonCounts};
use meguri_stats::graphs::records::hourly::{HourCounts, HourlyRecord, Hours};
use meguri_stats::graphs::records::retention::{RetentionCounts, RetentionRecord};
use meguri_stats::graphs::records::reviews::{ReviewKinds, ReviewsRecord};
use meguri_stats::graphs::{Columns, Graph, GraphRecord, GraphRequest, GraphResult, History, Scope};
use meguri_testkit::bench::fixtures::CoreHost;
use serde_json::{Map, Value, json};
use tempfile::TempDir;

use crate::graphs_common::{LANG, fixture_path, insert_card};

/// 2026-10-12 12:00 +02:00 in Europe/Paris, the V10 default.
const PARIS_NOON_MS: i64 = 1_791_799_200_000;
/// `N` for that clock with rollover 4: 2026-10-13 04:00 +02:00.
const PARIS_N: i64 = 1_791_856_800;
const PARIS: i32 = 7_200;
/// A card id no card has: a deleted card's rows.
const GONE: i64 = 1_000;
const DAY: i64 = 86_400;

/// A collection with only the rows a test writes, and a clock.
struct Study {
    _dir: TempDir,
    host: CoreHost,
}

impl Study {
    fn new(now_ms: i64, offset: i32, zone: &str, statements: &[String]) -> Study {
        let dir = tempfile::tempdir().unwrap();
        let path = fixture_path(dir.path());
        fs::copy(conformance().join("fixtures/storage/schema-18.anki2"), &path).unwrap();
        {
            let clock = meguri_model::clock::FixedClock::new(now_ms, UtcOffset::UTC);
            let random = meguri_model::rng::SeededRandom::new(1);
            let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
            opened.writer().execute_raw("DELETE FROM revlog", &[]).unwrap();
            for statement in statements {
                opened.writer().execute_raw(statement, &[]).unwrap();
            }
        }
        let clock = Arc::new(StepClock::new(now_ms, UtcOffset::from_seconds(offset).unwrap(), zone));
        let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock).unwrap();
        Study { _dir: dir, host }
    }

    fn paris(rows: &[Row]) -> Study {
        Study::new(PARIS_NOON_MS, PARIS, "Europe/Paris", &[insert(rows)])
    }

    fn parts(&self, scope: Scope, history: History) -> Parts {
        let request = GraphRequest {
            scope,
            history,
            graphs: vec![Graph::Reviews, Graph::Retention, Graph::Hourly, Graph::AnswerButtons],
            refresh: 1,
            context: None,
            cancel: CancelToken::new(),
            columns: Columns::Cached,
        };
        let collection = self.host.collection().unwrap();
        let data = match self.host.stats().request(&collection, self.host.derived(), &request).unwrap() {
            GraphResult::Data(data) => data,
            other => panic!("not data: {other:?}"),
        };
        fn take<T: Clone + 'static>(record: &meguri_stats::graphs::Record) -> T {
            record.as_any().downcast_ref::<T>().expect("the record of its graph").clone()
        }
        Parts {
            next_day_at: data.context.next_day_at,
            reviews: take(&data.records[0]),
            retention: take(&data.records[1]),
            hourly: take(&data.records[2]),
            buttons: take(&data.records[3]),
        }
    }

    fn all(&self) -> Parts {
        self.parts(Scope::Collection, History::AllHistory)
    }

    fn year(&self) -> Parts {
        self.parts(Scope::Collection, History::Last12Months)
    }
}

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

#[derive(Clone, Debug)]
struct Parts {
    next_day_at: i64,
    reviews: ReviewsRecord,
    retention: RetentionRecord,
    hourly: HourlyRecord,
    buttons: AnswerButtonsRecord,
}

/// A review-log row: V10's defaults are type 1, `lastIvl` 30, `ease` 3,
/// `factor` 2500, `time` 0, on a deleted card.
#[derive(Clone, Copy, Debug)]
struct Row {
    id: i64,
    cid: i64,
    ease: i64,
    last_ivl: i64,
    factor: i64,
    time: i64,
    kind: i64,
}

fn row(id: i64) -> Row {
    Row {
        id,
        cid: GONE,
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
    fn factor(self, factor: i64) -> Row {
        Row { factor, ..self }
    }
    fn time(self, time: i64) -> Row {
        Row { time, ..self }
    }
    fn kind(self, kind: i64) -> Row {
        Row { kind, ..self }
    }
    fn card(self, cid: i64) -> Row {
        Row { cid, ..self }
    }
}

fn insert(rows: &[Row]) -> String {
    if rows.is_empty() {
        return "SELECT 1".to_owned();
    }
    let values: Vec<String> = rows
        .iter()
        .map(|r| format!("({}, {}, -1, {}, 1, {}, {}, {}, {})", r.id, r.cid, r.ease, r.last_ivl, r.factor, r.time, r.kind))
        .collect();
    format!("INSERT INTO revlog VALUES {}", values.join(", "))
}

/// Rows a millisecond apart from `start`, one per maker.
fn at(start: i64, makers: &[fn(Row) -> Row]) -> Vec<Row> {
    makers.iter().enumerate().map(|(n, make)| make(row(start + n as i64))).collect()
}

fn kinds(learn: i64, relearn: i64, young: i64, mature: i64, filtered: i64) -> ReviewKinds {
    ReviewKinds {
        learn,
        relearn,
        young,
        mature,
        filtered,
    }
}

fn counts(young_passed: u64, young_failed: u64, mature_passed: u64, mature_failed: u64) -> RetentionCounts {
    RetentionCounts {
        young_passed,
        young_failed,
        mature_passed,
        mature_failed,
    }
}

fn mature_count(record: &ReviewsRecord) -> BTreeMap<i32, i64> {
    record.count.iter().map(|(day, kinds)| (*day, kinds.mature)).collect()
}

fn hour(total: u64, correct: u64) -> HourCounts {
    HourCounts { total, correct }
}

/// The non-empty hours of a range, as (hour, total, correct).
fn busy(hours: &Hours) -> Vec<(usize, u64, u64)> {
    hours
        .iter()
        .enumerate()
        .filter(|(_, cell)| cell.total > 0 || cell.correct > 0)
        .map(|(at, cell)| (at, cell.total, cell.correct))
        .collect()
}

fn ranges(record: &HourlyRecord) -> Vec<&Hours> {
    [&record.one_month, &record.three_months, &record.one_year]
        .into_iter()
        .chain(record.all_time.as_ref())
        .collect()
}

fn button_ranges(record: &AnswerButtonsRecord) -> Vec<ButtonCounts> {
    [record.one_month, record.three_months, record.one_year]
        .into_iter()
        .chain(record.all_time)
        .collect()
}

fn encode(record: &dyn GraphRecord) -> Vec<u8> {
    let mut out = Encoder::new();
    record.encode(&mut out);
    out.into_bytes()
}

#[test]
fn v2_1_day_index_table_with_its_32_bit_key() {
    let ids = [
        1_760_234_399_999,
        1_760_234_400_000,
        1_760_234_401_000,
        1_791_770_400_000,
        1_791_770_401_000,
        1_791_856_800_000,
        1_791_943_199_999,
        1_791_943_200_000,
        1_791_856_800_000_000,
        1_791_856_800_000_000_000,
        i64::MAX,
    ];
    let rows: Vec<Row> = ids.iter().map(|id| row(*id)).collect();
    let study = Study::paris(&rows);
    let all = study.all();
    assert_eq!(all.next_day_at, PARIS_N);
    let expected = BTreeMap::from([
        (-735_773_886, 1),
        (-622_211_972, 1),
        (-366, 2),
        (-365, 1),
        (-1, 1),
        (0, 3),
        (1, 1),
        (20_718_344, 1),
    ]);
    assert_eq!(mature_count(&all.reviews), expected);
    // H6's By Answer takes the same cut keys (every row is Good).
    let good: BTreeMap<i32, i64> = all.reviews.count_by_answer.iter().map(|(day, answers)| (*day, answers[2])).collect();
    assert_eq!(good, expected);
    let mut year = expected;
    year.insert(-366, 1);
    assert_eq!(mature_count(&study.year().reviews), year);
}

#[test]
fn v1_v2_kinds_split_with_times() {
    // V10 `reviews/kinds-split`.
    let rows = [
        row(PARIS_NOON_MS).kind(0).time(1_000),
        row(PARIS_NOON_MS + 1).kind(2).ease(1).time(2_000),
        row(PARIS_NOON_MS + 2).last(20).time(3_000),
        row(PARIS_NOON_MS + 3).last(21).time(4_000),
        row(PARIS_NOON_MS + 4).last(-600).time(5_000),
        row(PARIS_NOON_MS + 5).kind(3).factor(0).time(6_000),
        row(PARIS_NOON_MS + 6).kind(3).ease(0).time(7_000),
        row(PARIS_NOON_MS + 7).kind(4).ease(0).time(8_000),
        row(PARIS_NOON_MS + 8).kind(5).ease(0).time(500),
        row(PARIS_NOON_MS + 9).ease(0).time(9_000),
    ];
    let study = Study::paris(&rows);
    for parts in [study.all(), study.year()] {
        assert_eq!(parts.reviews.count, BTreeMap::from([(0, kinds(1, 1, 2, 2, 2))]));
        assert_eq!(parts.reviews.time, BTreeMap::from([(0, kinds(1_000, 2_000, 8_000, 13_000, 13_000))]));
        for hours in ranges(&parts.hourly) {
            assert_eq!(busy(hours), [(12, 6, 4)]);
        }
    }
}

#[test]
fn v1_kind_values_as_g4_reads_them() {
    // 260 reads as Manual (left out of Reviews, in Retention with a long
    // last interval); -1 and 6 read as Learn.
    let rows = [
        row(PARIS_NOON_MS).kind(260).last(15),
        row(PARIS_NOON_MS + 1).kind(-1),
        row(PARIS_NOON_MS + 2).kind(6).ease(1),
    ];
    let parts = Study::paris(&rows).all();
    assert_eq!(parts.reviews.count, BTreeMap::from([(0, kinds(2, 0, 0, 0, 0))]));
    assert_eq!(parts.retention.today, counts(1, 0, 1, 1));
    assert_eq!(busy(&parts.hourly.one_month), [(12, 2, 1)]);
    assert_eq!(parts.buttons.one_month.learning, [1, 0, 1, 0]);
}

#[test]
fn v2_1_day_edges_and_window_start() {
    // V10 `reviews/day-edges`.
    let ids = [
        1_791_770_399_999,
        1_791_770_400_000,
        1_791_770_400_500,
        1_791_770_401_000,
        1_791_856_800_000,
        1_791_943_199_999,
        1_791_943_200_000,
    ];
    let edges = Study::paris(&ids.map(row));
    assert_eq!(mature_count(&edges.year().reviews), BTreeMap::from([(-1, 3), (0, 3), (1, 1)]));
    // V10 `reviews/window-start`.
    let start = Study::paris(&[1_760_234_399_999, 1_760_234_400_000, 1_760_234_401_000].map(row));
    assert_eq!(mature_count(&start.year().reviews), BTreeMap::from([(-366, 1), (-365, 1)]));
    assert_eq!(mature_count(&start.all().reviews), BTreeMap::from([(-366, 2), (-365, 1)]));
}

#[test]
fn review_time_sums_no_wrap() {
    // V9 `stats-review-time-no-wrap`, the rows of V10 `reviews/time-wrap`:
    // Anki's 32-bit sums wrap, Meguri's 64-bit sums do not.
    let today = PARIS_NOON_MS;
    let rows = [
        row(today).kind(0).time(2_200_000_000),
        row(today + 1).kind(0).time(2_200_000_000),
        row(today + 2).last(10).time(3_000_000_000),
        row(today + 3).time(3_000_000_000),
        row(today + 4).time(3_000_000_000),
        row(today - DAY * 1_000).time(3_000_000_000),
        // Beyond the scenario: young times past 2^32 ms on one day too.
        row(today + 5).last(10).time(4_294_967_295),
    ];
    let parts = Study::paris(&rows).year();
    assert_eq!(parts.reviews.count, BTreeMap::from([(-1, kinds(0, 0, 0, 1, 0)), (0, kinds(2, 0, 2, 2, 0))]));
    assert_eq!(
        parts.reviews.time,
        BTreeMap::from([
            (-1, kinds(0, 0, 0, 3_000_000_000, 0)),
            (0, kinds(4_400_000_000, 0, 7_294_967_295, 6_000_000_000, 0)),
        ])
    );
}

#[test]
fn v4_1_retention_row_filter() {
    // V10 `retention/row-filter`.
    let rows = [
        row(PARIS_NOON_MS).last(10).ease(1),
        row(PARIS_NOON_MS + 1),
        row(PARIS_NOON_MS + 2).kind(0).last(1),
        row(PARIS_NOON_MS + 3).kind(0).last(-600),
        row(PARIS_NOON_MS + 4).kind(2).last(-86_400).ease(1),
        row(PARIS_NOON_MS + 5).kind(2).last(-86_399).ease(1),
        row(PARIS_NOON_MS + 6).kind(3).factor(0).last(10),
        row(PARIS_NOON_MS + 7).kind(3).last(10),
        row(PARIS_NOON_MS + 8).last(10).ease(0),
        row(PARIS_NOON_MS + 9).kind(4).last(15),
        row(PARIS_NOON_MS + 10).last(25).ease(5),
    ];
    let study = Study::paris(&rows);
    let expected = counts(3, 2, 2, 0);
    let all = study.all().retention;
    for period in [all.today, all.week, all.month, all.year, all.all_time.unwrap()] {
        assert_eq!(period, expected);
    }
    assert_eq!(all.yesterday, RetentionCounts::default());
    let year = study.year().retention;
    assert_eq!(year.today, expected);
    assert_eq!(year.all_time, None);
}

#[test]
fn v4_2_retention_periods() {
    // V10 `retention/periods`.
    let n = PARIS_N * 1_000;
    let mut ids = vec![n - 1, n];
    for k in [1, 2, 7, 30, 365] {
        ids.push(n - k * DAY * 1_000);
        ids.push(n - k * DAY * 1_000 - 1);
    }
    let rows: Vec<Row> = ids.into_iter().map(row).collect();
    let retention = Study::paris(&rows).all().retention;
    let passed = |c: RetentionCounts| c.mature_passed;
    assert_eq!(
        [
            passed(retention.today),
            passed(retention.yesterday),
            passed(retention.week),
            passed(retention.month),
            passed(retention.year),
            passed(retention.all_time.unwrap()),
        ],
        [2, 2, 6, 8, 10, 11]
    );
}

#[test]
fn v7_edges_compared() {
    // (t, day, in `month`, in `today`, in `one_month`).
    let table = [
        (PARIS_N - 30 * DAY, -30, true, false, true),
        (PARIS_N - 30 * DAY - 1, -30, false, false, false),
        (PARIS_N - DAY - 1, -1, true, false, true),
        (PARIS_N - DAY, -1, true, true, true),
        (PARIS_N - DAY + 1, 0, true, true, true),
        (PARIS_N, 0, false, false, true),
        (PARIS_N + DAY, 1, false, false, true),
    ];
    for (t, day, month, today, one_month) in table {
        let parts = Study::paris(&[row(t * 1_000)]).year();
        assert_eq!(mature_count(&parts.reviews), BTreeMap::from([(day, 1)]), "{t}");
        assert_eq!(parts.retention.month.mature_passed, u64::from(month), "{t}");
        assert_eq!(parts.retention.today.mature_passed, u64::from(today), "{t}");
        assert_eq!(parts.hourly.one_month.iter().map(|h| h.total).sum::<u64>(), u64::from(one_month), "{t}");
        assert_eq!(parts.buttons.one_month.mature[2], u64::from(one_month), "{t}");
    }
}

#[test]
fn ae59_history_parts_and_no_all_time_member_with_twelve_months() {
    // V10 `retention/ae59-history-parts`.
    let rows = [row(PARIS_NOON_MS), row(PARIS_NOON_MS - 400 * DAY * 1_000)];
    let study = Study::paris(&rows);
    let all = study.all();
    assert_eq!(all.retention.all_time.unwrap().mature_passed, 2);
    assert_eq!(all.retention.year.mature_passed, 1);
    assert_eq!(busy(all.hourly.all_time.as_ref().unwrap()), [(12, 2, 2)]);
    assert_eq!(all.buttons.all_time.unwrap().mature, [0, 0, 2, 0]);
    let year = study.year();
    assert_eq!(year.retention.year.mature_passed, 1);
    assert_eq!(year.retention.all_time, None);
    assert_eq!(year.hourly.all_time, None);
    assert_eq!(year.buttons.all_time, None);
    // The encodings carry five periods and three ranges, or six and four.
    let lengths = |parts: &Parts| -> [i64; 3] {
        [
            Decoder::new(&encode(&parts.retention)).integer().unwrap(),
            Decoder::new(&encode(&parts.hourly)).integer().unwrap(),
            Decoder::new(&encode(&parts.buttons)).integer().unwrap(),
        ]
    };
    assert_eq!(lengths(&year), [5, 3, 3]);
    assert_eq!(lengths(&all), [6, 4, 4]);
}

#[test]
fn v5_hourly_row_kinds() {
    // V10 `hourly/row-kinds`: today at 10:15.
    let start = PARIS_NOON_MS - 105 * 60 * 1_000;
    let rows = [
        row(start).kind(0),
        row(start + 1).ease(1),
        row(start + 2).kind(2).ease(2),
        row(start + 3).kind(3),
        row(start + 4).kind(4).ease(0),
        row(start + 5).kind(5).ease(0),
        row(start + 6).ease(0),
        row(start + 7).ease(5),
    ];
    let parts = Study::paris(&rows).all();
    for hours in ranges(&parts.hourly) {
        assert_eq!(busy(hours), [(10, 5, 3)]);
    }
}

#[test]
fn v5_1_hours_at_the_offset_now() {
    // V10 `hourly/current-offset`: now 2026-06-10 12:00 +02:00.
    let rows = [row(1_768_465_800_000), row(1_780_299_000_000).ease(1)];
    let study = Study::new(1_781_085_600_000, PARIS, "Europe/Paris", &[insert(&rows)]);
    let hourly = study.all().hourly;
    assert_eq!(busy(&hourly.one_month), [(9, 1, 0)]);
    assert_eq!(busy(&hourly.three_months), [(9, 1, 0)]);
    assert_eq!(busy(&hourly.one_year), [(9, 1, 0), (10, 1, 1)]);
    assert_eq!(busy(hourly.all_time.as_ref().unwrap()), [(9, 1, 0), (10, 1, 1)]);
    // V10 `hourly/kathmandu-edges`: +05:45, now 2026-10-12 12:00.
    let ids = [1_791_778_499_999, 1_791_778_500_000, 1_791_742_499_999, 1_791_742_500_000];
    let study = Study::new(1_791_785_700_000, 20_700, "Asia/Kathmandu", &[insert(&ids.map(row))]);
    for hours in ranges(&study.all().hourly) {
        assert_eq!(busy(hours), [(0, 1, 1), (9, 1, 1), (10, 1, 1), (23, 1, 1)]);
    }
}

#[test]
fn hourly_epoch_rows() {
    // V9 `stats-hourly-epoch-rows`: west of UTC, rows just after the epoch
    // go to hour 0, at t = 0 too, where Anki's request fails. With V10
    // `hourly/near-epoch`'s row at t + offset = -1,800.
    let rows = [row(0), row(12_600_000)];
    let study = Study::new(1_791_820_800_000, -14_400, "America/New_York", &[insert(&rows)]);
    let hourly = study.all().hourly;
    assert_eq!(busy(hourly.all_time.as_ref().unwrap()), [(0, 2, 2)]);
    for hours in [&hourly.one_month, &hourly.three_months, &hourly.one_year] {
        assert!(busy(hours).is_empty());
    }
}

#[test]
fn v5_2_v6_2_range_edges() {
    // V10 `hourly/range-edges`.
    let n = PARIS_N * 1_000;
    let mut rows = Vec::new();
    for k in [30, 90, 365] {
        rows.push(row(n - k * DAY * 1_000));
        rows.push(row(n - k * DAY * 1_000 - 1));
    }
    rows.push(row(n + 3_600_000).ease(1));
    let parts = Study::paris(&rows).all();
    let hours: Vec<Vec<(usize, u64, u64)>> = ranges(&parts.hourly).into_iter().map(busy).collect();
    assert_eq!(hours, [
        vec![(4, 1, 1), (5, 1, 0)],
        vec![(3, 1, 1), (4, 2, 2), (5, 1, 0)],
        vec![(3, 2, 2), (4, 3, 3), (5, 1, 0)],
        vec![(3, 3, 3), (4, 3, 3), (5, 1, 0)],
    ]);
    let mature: Vec<[u64; 4]> = button_ranges(&parts.buttons).iter().map(|b| b.mature).collect();
    assert_eq!(mature, [[1, 0, 1, 0], [1, 0, 3, 0], [1, 0, 5, 0], [1, 0, 6, 0]]);
    assert_eq!(parts.retention.month.mature_passed, 1);
    assert_eq!(parts.retention.year.mature_passed, 5);
    assert_eq!(parts.retention.all_time.unwrap().mature_passed, 6);
}

#[test]
fn v6_1_button_groups() {
    // V10 `buttons/groups`.
    let makers: [fn(Row) -> Row; 13] = [
        |r| r.kind(0).ease(1),
        |r| r.kind(0).ease(2),
        |r| r.kind(0).ease(3),
        |r| r.kind(0).ease(4),
        |r| r.kind(2).ease(1),
        |r| r.kind(3).factor(0),
        |r| r.last(20).ease(2),
        |r| r.last(-600),
        |r| r.last(21).ease(4),
        |r| r.ease(0),
        |r| r.ease(5),
        |r| r.kind(4),
        |r| r.kind(5).ease(1),
    ];
    let rows = at(PARIS_NOON_MS, &makers);
    let study = Study::paris(&rows);
    for parts in [study.all(), study.year()] {
        for range in button_ranges(&parts.buttons) {
            assert_eq!(range, ButtonCounts {
                learning: [2, 1, 2, 1],
                young: [0, 1, 1, 0],
                mature: [0, 0, 0, 1],
            });
        }
    }
}

#[test]
fn g2_scope_parts() {
    // V10 `buttons/scope-parts`: a card in `Lang` with a Good and an Again
    // row today, and a deleted card's Good row.
    let card = 1_788_249_700_001;
    let rows = [row(PARIS_NOON_MS).card(card), row(PARIS_NOON_MS + 1).card(card).ease(1), row(PARIS_NOON_MS + 2)];
    let study = Study::new(PARIS_NOON_MS, PARIS, "Europe/Paris", &[insert_card(card, LANG, 2, 2, 100), insert(&rows)]);
    let check = |parts: Parts, mature: [u64; 4], passed: u64, failed: u64, total: u64, correct: u64| {
        assert_eq!(parts.buttons.one_month.mature, mature);
        assert_eq!(parts.retention.today, counts(0, 0, passed, failed));
        assert_eq!(parts.hourly.one_month[12], hour(total, correct));
    };
    check(study.year(), [1, 0, 2, 0], 2, 1, 3, 2);
    check(study.parts(Scope::Deck(DeckId(LANG)), History::Last12Months), [1, 0, 1, 0], 1, 1, 2, 1);
    check(study.parts(Scope::Search("deck:Lang".into()), History::Last12Months), [1, 0, 1, 0], 1, 1, 2, 1);
}

#[test]
fn v8_no_counted_row() {
    // Only rows no part counts: Manual and Rescheduled rows with no rating.
    let rows = [row(PARIS_NOON_MS).kind(4).ease(0), row(PARIS_NOON_MS + 1).kind(5).ease(0)];
    for study in [Study::paris(&[]), Study::paris(&rows)] {
        let parts = study.all();
        assert!(parts.reviews.count.is_empty() && parts.reviews.time.is_empty());
        let mut empty = RetentionRecord::default();
        empty.all_time = Some(RetentionCounts::default());
        assert_eq!(parts.retention, empty);
        assert!(ranges(&parts.hourly).iter().all(|hours| busy(hours).is_empty()));
        assert!(button_ranges(&parts.buttons).iter().all(|range| *range == ButtonCounts::default()));
        // An empty map is its run count, 0: By Type's, By Answer's (H6),
        // then `unrated` 0.
        assert_eq!(encode(&parts.reviews), [0, 0, 0]);
    }
}

#[test]
fn g6_encodings_round_trip() {
    let rows = [
        row(PARIS_NOON_MS).kind(0).time(1_000),
        row(PARIS_NOON_MS - 3 * DAY * 1_000).ease(1).time(70_000),
        row(PARIS_NOON_MS - 4 * DAY * 1_000).last(5).time(9_999),
        row(PARIS_NOON_MS - 9 * DAY * 1_000).kind(3).time(5),
    ];
    let parts = Study::paris(&rows).all();
    // Reviews: one map, ten members.
    let bytes = encode(&parts.reviews);
    let mut decoder = Decoder::new(&bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers; 10]).unwrap();
    // H6's By Answer map and `unrated`.
    let answers = decoder.sparse_map(&[MemberKind::Integers; 8]).unwrap();
    assert_eq!(decoder.integer().unwrap(), parts.reviews.unrated as i64);
    assert!(decoder.is_done());
    let by_answer: Vec<(i64, [i64; 4], [i64; 4])> = (0..answers.keys.len())
        .map(|at| {
            let member = |m: usize| answers.integers[m][at];
            (answers.keys[at], [0, 1, 2, 3].map(member), [4, 5, 6, 7].map(member))
        })
        .collect();
    let want: Vec<(i64, [i64; 4], [i64; 4])> = parts
        .reviews
        .count_by_answer
        .iter()
        .map(|(day, counts)| (i64::from(*day), *counts, parts.reviews.time_by_answer[day]))
        .collect();
    assert_eq!(by_answer, want);
    assert_eq!(map.keys, [-9, -4, -3, 0]);
    let decoded: Vec<(ReviewKinds, ReviewKinds)> = (0..map.keys.len())
        .map(|at| {
            let member = |m: usize| map.integers[m][at];
            (
                kinds(member(0), member(1), member(2), member(3), member(4)),
                kinds(member(5), member(6), member(7), member(8), member(9)),
            )
        })
        .collect();
    let expected: Vec<(ReviewKinds, ReviewKinds)> =
        parts.reviews.count.values().copied().zip(parts.reviews.time.values().copied()).collect();
    assert_eq!(decoded, expected);
    // Retention: the periods' four counts.
    let bytes = encode(&parts.retention);
    let mut decoder = Decoder::new(&bytes);
    let periods = decoder.list(|d| (0..4).map(|_| d.integer()).collect::<Result<Vec<_>, _>>()).unwrap();
    assert!(decoder.is_done());
    let r = &parts.retention;
    let want: Vec<Vec<i64>> = [r.today, r.yesterday, r.week, r.month, r.year, r.all_time.unwrap()]
        .iter()
        .map(|c| c.values().iter().map(|v| *v as i64).collect())
        .collect();
    assert_eq!(periods, want);
    // Hourly: per range 24 pairs.
    let bytes = encode(&parts.hourly);
    let mut decoder = Decoder::new(&bytes);
    let decoded = decoder
        .list(|d| d.list(|d| Ok(hour(d.integer()? as u64, d.integer()? as u64))))
        .unwrap();
    // H7: `six_months`, then `retention_by_hour`'s five ranges.
    let six = decoder.list(|d| Ok(hour(d.integer()? as u64, d.integer()? as u64))).unwrap();
    let kept = decoder
        .list(|d| d.list(|d| Ok((d.integer()? as u64, d.integer()? as u64))))
        .unwrap();
    assert!(decoder.is_done());
    let want: Vec<Vec<HourCounts>> = ranges(&parts.hourly).into_iter().map(|h| h.to_vec()).collect();
    assert_eq!(decoded, want);
    assert_eq!(six, parts.hourly.six_months.to_vec());
    let by_hour = &parts.hourly.retention_by_hour;
    let want: Vec<Vec<(u64, u64)>> = [&by_hour.one_month, &by_hour.three_months, &by_hour.six_months, &by_hour.one_year]
        .into_iter()
        .chain(by_hour.all_time.as_ref())
        .map(|hours| hours.iter().map(|h| (h.passed, h.failed)).collect())
        .collect();
    assert_eq!(kept, want);
    // Answer Buttons: per range three lists of four.
    let bytes = encode(&parts.buttons);
    let mut decoder = Decoder::new(&bytes);
    let decoded = decoder
        .list(|d| (0..3).map(|_| d.list(|d| d.integer())).collect::<Result<Vec<_>, _>>())
        .unwrap();
    // H7: the `six_months` range.
    let six: Vec<Vec<i64>> = (0..3).map(|_| decoder.list(|d| d.integer())).collect::<Result<_, _>>().unwrap();
    assert!(decoder.is_done());
    let b = parts.buttons.six_months;
    let want_six: Vec<Vec<i64>> = [b.learning, b.young, b.mature].iter().map(|p| p.iter().map(|v| *v as i64).collect()).collect();
    assert_eq!(six, want_six);
    let want: Vec<Vec<Vec<i64>>> = button_ranges(&parts.buttons)
        .iter()
        .map(|b| [b.learning, b.young, b.mature].iter().map(|p| p.iter().map(|v| *v as i64).collect()).collect())
        .collect();
    assert_eq!(decoded, want);
}

/// The whole record, H6's extras included.
fn decode_reviews(bytes: &[u8]) -> ReviewsRecord {
    let mut decoder = Decoder::new(bytes);
    let map = decoder.sparse_map(&[MemberKind::Integers; 10]).unwrap();
    let answers = decoder.sparse_map(&[MemberKind::Integers; 8]).unwrap();
    let mut record = ReviewsRecord::default();
    record.unrated = decoder.integer().unwrap() as u64;
    assert!(decoder.is_done());
    for (at, day) in map.keys.iter().enumerate() {
        let member = |m: usize| map.integers[m][at];
        let day = i32::try_from(*day).unwrap();
        record.count.insert(day, kinds(member(0), member(1), member(2), member(3), member(4)));
        record.time.insert(day, kinds(member(5), member(6), member(7), member(8), member(9)));
    }
    for (at, day) in answers.keys.iter().enumerate() {
        let member = |m: usize| answers.integers[m][at];
        let day = i32::try_from(*day).unwrap();
        record.count_by_answer.insert(day, [0, 1, 2, 3].map(member));
        record.time_by_answer.insert(day, [4, 5, 6, 7].map(member));
    }
    record
}

#[test]
fn g6_twenty_years_of_daily_study_fit_the_limit() {
    // G6's test: 7,305 consecutive days, rows of every kind on each, 150
    // counted rows a day, stored times of 5,000 to 60,000 ms. Only Anki's
    // members: with H6's By Answer maps filled the record passes the limit
    // (about 294,000 bytes), a spec question recorded in
    // docs/process/verification-status.md.
    let mut record = ReviewsRecord::default();
    for day in -7_304..=0 {
        let spread = i64::from(day).rem_euclid(97);
        let count = kinds(30, 30, 30, 30, 30);
        let time = |k: i64| 30 * (5_000 + (spread * 571 + k * 9_001) % 55_001);
        record.count.insert(day, count);
        record.time.insert(day, kinds(time(1), time(2), time(3), time(4), time(5)));
    }
    let bytes = encode(&record);
    assert!(bytes.len() <= 262_144, "{}", bytes.len());
    assert_eq!(decode_reviews(&bytes), record);
}

#[test]
fn g6_a_record_past_the_limit_is_returned_whole() {
    // G6's test: one row of each kind on every day from 1970-01-01, each
    // with a stored time of 4,000,000,000 ms.
    let days = PARIS_NOON_MS / 1_000 / DAY;
    let rows = format!(
        "WITH RECURSIVE day(d) AS (SELECT 0 UNION ALL SELECT d + 1 FROM day WHERE d < {days}), \
         kind(k, type, last) AS (VALUES (0, 0, 30), (1, 2, 30), (2, 1, 5), (3, 1, 30), (4, 3, 30)) \
         INSERT INTO revlog SELECT (d * 86400 + 43200) * 1000 + k, {GONE}, -1, 3, 1, last, 2500, 4000000000, type \
         FROM day CROSS JOIN kind"
    );
    let study = Study::new(PARIS_NOON_MS, PARIS, "Europe/Paris", &[rows]);
    let reviews = study.all().reviews;
    assert_eq!(reviews.count.len() as i64, days + 1);
    assert!(reviews.count.values().all(|k| *k == kinds(1, 1, 1, 1, 1)));
    let bytes = encode(&reviews);
    assert!(bytes.len() > 262_144, "{}", bytes.len());
    assert_eq!(decode_reviews(&bytes), reviews);
}

// V10's oracle goldens for U99's four parts, serialised as V8 says. Each
// listed scenario writes its rows with `db_exec` on deleted cards (or on
// the fixture's first card, in deck `Lang`), may set `crt` and
// `creationOffset` and reopen, and observes Collection, Deck `Lang` or
// Search `deck:Lang` requests. `reviews/time-wrap` is left out: its golden
// is Anki's wrapped sum, which Meguri does not reproduce (V9,
// `review_time_sums_no_wrap`).

fn kinds_json(kinds: &ReviewKinds) -> Value {
    let named = [
        ("learn", kinds.learn),
        ("relearn", kinds.relearn),
        ("young", kinds.young),
        ("mature", kinds.mature),
        ("filtered", kinds.filtered),
    ];
    Value::Object(named.into_iter().filter(|(_, v)| *v != 0).map(|(k, v)| (k.to_owned(), json!(v))).collect())
}

fn reviews_json(record: &ReviewsRecord) -> Value {
    if record.count.is_empty() {
        return json!({});
    }
    let map = |days: &BTreeMap<i32, ReviewKinds>| -> Value {
        Value::Object(days.iter().map(|(day, kinds)| (day.to_string(), kinds_json(kinds))).collect())
    };
    json!({"count": map(&record.count), "time": map(&record.time)})
}

fn retention_json(record: &RetentionRecord) -> Value {
    let period = |c: &RetentionCounts| -> Value {
        let named = [
            ("young_passed", c.young_passed),
            ("young_failed", c.young_failed),
            ("mature_passed", c.mature_passed),
            ("mature_failed", c.mature_failed),
        ];
        Value::Object(named.into_iter().filter(|(_, v)| *v != 0).map(|(k, v)| (k.to_owned(), json!(v))).collect())
    };
    let mut out = Map::new();
    for (name, counts) in [
        ("today", &record.today),
        ("yesterday", &record.yesterday),
        ("week", &record.week),
        ("month", &record.month),
        ("year", &record.year),
    ] {
        out.insert(name.into(), period(counts));
    }
    if let Some(all) = &record.all_time {
        out.insert("all_time".into(), period(all));
    }
    Value::Object(out)
}

fn hourly_json(record: &HourlyRecord) -> Value {
    let range = |hours: &Hours| -> Value {
        hours
            .iter()
            .map(|h| {
                let mut cell = Map::new();
                if h.total != 0 {
                    cell.insert("total".into(), json!(h.total));
                }
                if h.correct != 0 {
                    cell.insert("correct".into(), json!(h.correct));
                }
                Value::Object(cell)
            })
            .collect()
    };
    let mut out = Map::new();
    out.insert("one_month".into(), range(&record.one_month));
    out.insert("three_months".into(), range(&record.three_months));
    out.insert("one_year".into(), range(&record.one_year));
    if let Some(all) = &record.all_time {
        out.insert("all_time".into(), range(all));
    }
    Value::Object(out)
}

fn buttons_json(record: &AnswerButtonsRecord) -> Value {
    let range = |b: &ButtonCounts| json!({"learning": b.learning, "young": b.young, "mature": b.mature});
    let mut out = Map::new();
    out.insert("one_month".into(), range(&record.one_month));
    out.insert("three_months".into(), range(&record.three_months));
    out.insert("one_year".into(), range(&record.one_year));
    if let Some(all) = &record.all_time {
        out.insert("all_time".into(), range(all));
    }
    Value::Object(out)
}

/// The UTC offset at each scenario clock's start.
fn offset_at(zone: &str, start: &str) -> i32 {
    match (zone, start) {
        ("Europe/Paris", "2026-10-12T12:00:00" | "2026-06-10T12:00:00" | "2026-03-29T12:00:00") => 7_200,
        ("America/St_Johns", "2026-10-12T12:00:00") => -9_000,
        ("America/New_York", "2026-10-12T12:00:00") => -14_400,
        ("Asia/Kathmandu", "2026-10-12T12:00:00") => 20_700,
        // After the 02:00 change to +11:00.
        ("Australia/Lord_Howe", "2026-10-04T12:00:00") => 39_600,
        other => panic!("no offset for {other:?}"),
    }
}

/// Epoch milliseconds of a local `YYYY-MM-DDTHH:MM:SS` at `offset`.
fn local_ms(start: &str, offset: i32) -> i64 {
    let number = |range: std::ops::Range<usize>| start[range].parse::<i64>().unwrap();
    let (year, month, day) = (number(0..4), number(5..7), number(8..10));
    let seconds = number(11..13) * 3_600 + number(14..16) * 60 + number(17..19);
    // Days from 1970-01-01 in the proleptic Gregorian calendar.
    let (y, m) = if month <= 2 { (year - 1, month + 9) } else { (year, month - 3) };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * m + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    (days * DAY + seconds - i64::from(offset)) * 1_000
}

fn scope_of(value: &Value) -> Scope {
    match value {
        Value::String(s) if s == "collection" => Scope::Collection,
        Value::Object(o) if o.get("deck") == Some(&json!("Lang")) => Scope::Deck(DeckId(LANG)),
        Value::Object(o) => Scope::Search(o["search"].as_str().unwrap().to_owned()),
        other => panic!("{other}"),
    }
}

fn check_golden(name: &str) {
    let scenario = fs::read_to_string(conformance().join(format!("scenarios/stats/graphs/{name}.yaml"))).unwrap();
    let golden: Value =
        serde_json::from_str(&fs::read_to_string(conformance().join(format!("goldens/stats/graphs/{name}.json"))).unwrap())
            .unwrap();
    let config: Value = scenario
        .lines()
        .find_map(|line| line.strip_prefix("  config: "))
        .map(|text| serde_json::from_str(text).unwrap())
        .unwrap();
    let mut statements = Vec::new();
    for (key, value) in config.as_object().unwrap() {
        match key.as_str() {
            // The fixture's own value.
            "rollover" => assert_eq!(value, &json!(4), "{name}"),
            "creationOffset" => statements.push(format!(
                "INSERT OR REPLACE INTO config VALUES ('creationOffset', 0, 0, CAST('{value}' AS BLOB))"
            )),
            other => panic!("{name}: config {other}"),
        }
    }
    let clock = scenario.lines().find_map(|line| line.strip_prefix("clock: {tz: ")).unwrap();
    let (zone, rest) = clock.split_once(", start: \"").unwrap();
    let start = rest.split('"').next().unwrap();
    let steps: Vec<Value> = scenario
        .lines()
        .filter_map(|line| line.strip_prefix("  - "))
        .map(|step| serde_json::from_str(step).unwrap())
        .collect();
    // Every row is written before the one stats observation, the last step;
    // a reopen or a timing observation before it changes nothing here, since
    // the collection opens after every statement ran.
    let (observe, writes) = steps.split_last().unwrap();
    for step in writes {
        if let Some(sql) = step["db_exec"]["sql"].as_str() {
            statements.push(sql.to_owned());
        } else {
            assert!(step.get("reopen").is_some() || step["observe"] == json!(["timing"]), "{name}: {step}");
        }
    }
    let offset = offset_at(zone, start);
    let study = Study::new(local_ms(start, offset), offset, zone, &statements);
    let requests = observe["observe"][0]["stats"].as_array().unwrap();
    let observed = golden["observations"]["steps"][steps.len() - 1]["stats"].as_array().unwrap();
    assert_eq!(requests.len(), observed.len(), "{name}");
    for (request, expected) in requests.iter().zip(observed) {
        let history = match request["history"].as_str().unwrap() {
            "year" => History::Last12Months,
            "all" => History::AllHistory,
            other => panic!("{other}"),
        };
        let parts = study.parts(scope_of(&request["scope"]), history);
        let names: Vec<&str> = request["parts"].as_array().unwrap().iter().map(|p| p.as_str().unwrap()).collect();
        let mut keys: Vec<&str> = expected.as_object().unwrap().keys().map(String::as_str).collect();
        keys.sort_unstable();
        let mut wanted = names.clone();
        wanted.sort_unstable();
        assert_eq!(keys, wanted, "{name} {request}");
        for part in names {
            let mine = match part {
                "reviews" => reviews_json(&parts.reviews),
                "true_retention" => retention_json(&parts.retention),
                "hours" => hourly_json(&parts.hourly),
                "buttons" => buttons_json(&parts.buttons),
                other => panic!("{other}"),
            };
            assert_eq!(mine, expected[part], "{name} {request} {part}");
        }
    }
}

macro_rules! goldens {
    ($($test:ident => $name:literal,)*) => {
        $(
            #[test]
            fn $test() {
                check_golden($name);
            }
        )*
    };
}

goldens! {
    golden_reviews_kinds_split => "reviews/kinds-split",
    golden_reviews_day_edges => "reviews/day-edges",
    golden_reviews_window_start => "reviews/window-start",
    golden_calendar_ae58_reviews => "calendar/ae58-paris-spring-forward",
    golden_calendar_history_years_reviews => "calendar/history-years",
    golden_retention_row_filter => "retention/row-filter",
    golden_retention_periods => "retention/periods",
    golden_retention_ae59 => "retention/ae59-history-parts",
    golden_hourly_row_kinds => "hourly/row-kinds",
    golden_hourly_current_offset => "hourly/current-offset",
    golden_hourly_kathmandu_edges => "hourly/kathmandu-edges",
    golden_hourly_near_epoch => "hourly/near-epoch",
    golden_hourly_range_edges => "hourly/range-edges",
    golden_buttons_groups => "buttons/groups",
    golden_buttons_scope_parts => "buttons/scope-parts",
    golden_tz_st_johns => "buttons/tz-matrix/st-johns",
    golden_tz_st_johns_scopes => "buttons/tz-matrix/st-johns-scopes",
    golden_tz_kathmandu => "buttons/tz-matrix/kathmandu",
    golden_tz_kathmandu_scopes => "buttons/tz-matrix/kathmandu-scopes",
    golden_tz_lord_howe_dst => "buttons/tz-matrix/lord-howe-dst",
    golden_tz_lord_howe_dst_scopes => "buttons/tz-matrix/lord-howe-dst-scopes",
    golden_tz_other_creation_zone => "buttons/tz-matrix/other-creation-zone",
    golden_tz_other_creation_zone_scopes => "buttons/tz-matrix/other-creation-zone-scopes",
}
