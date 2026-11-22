//! `stats/graphs-cards` C1 to C11 (U101): the seven card records, computed
//! by the production request over raw card rows on the schema-18 fixture.
//! The conformance cases rebuild a scenario's cards relative to the
//! request's `today` and `N` and expect its golden's values
//! (`conformance/goldens/stats/graphs/...`); floats compare with G14's
//! tolerance. Names and values are made up.

use std::collections::{BTreeMap, BTreeSet};
use std::time::Duration;

use meguri_engine::{CancelToken, Readiness};
use meguri_model::ids::DeckId;
use meguri_search::SearchMode;
use meguri_stats::graphs::encoding::{Decoder, MemberKind};
use meguri_stats::graphs::records::card_counts::{CountLabel, LabelText, StateCounts, label_text};
use meguri_stats::graphs::records::{
    AddedRecord, CardCountsRecord, DifficultyRecord, FutureDueRecord, IntervalsRecord, RetrievabilityRecord,
    StabilityRecord,
};
use meguri_stats::graphs::{
    COMPONENT, Columns, Context, Graph, GraphData, GraphRecord, GraphRequest, GraphResult, History, Scope,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Cell, ConfigValue, ReaderRole, RowKey, Table};

use crate::graphs_common::{CRAM, Fixture, LANG, NOTE, VERBS, join};

/// The retrievability clock of C13: now_s 1,897,293,600.
const R_NOW_MS: i64 = 1_897_293_600_000;
const NOW_S: i64 = 1_897_293_600;
const DAY: i64 = 86_400;

const CARD_GRAPHS: [Graph; 7] = [
    Graph::FutureDue,
    Graph::CardCounts,
    Graph::Intervals,
    Graph::Stability,
    Graph::Difficulty,
    Graph::Retrievability,
    Graph::Added,
];

/// A card row written as stored (the `cards` table's columns).
#[derive(Clone, Debug)]
struct Raw {
    id: i64,
    nid: i64,
    did: i64,
    odid: i64,
    card_type: i64,
    queue: i64,
    due: i64,
    odue: i64,
    ivl: i64,
    data: String,
}

/// A new card of `Lang`.
fn raw(id: i64) -> Raw {
    Raw {
        id,
        nid: NOTE,
        did: LANG,
        odid: 0,
        card_type: 0,
        queue: 0,
        due: 1,
        odue: 0,
        ivl: 0,
        data: "{}".into(),
    }
}

impl Raw {
    fn state(self, card_type: i64, queue: i64) -> Raw {
        Raw { card_type, queue, ..self }
    }

    fn review(self, ivl: i64, due: i64) -> Raw {
        Raw {
            ivl,
            due,
            ..self.state(2, 2)
        }
    }

    fn due(self, due: i64) -> Raw {
        Raw { due, ..self }
    }

    fn ivl(self, ivl: i64) -> Raw {
        Raw { ivl, ..self }
    }

    fn data(self, data: &str) -> Raw {
        Raw {
            data: data.into(),
            ..self
        }
    }

    fn note(self, nid: i64) -> Raw {
        Raw { nid, ..self }
    }

    fn deck(self, did: i64) -> Raw {
        Raw { did, ..self }
    }

    fn filtered(self, did: i64, odid: i64, odue: i64) -> Raw {
        Raw { did, odid, odue, ..self }
    }

    fn cells(&self) -> Vec<Cell> {
        let int = Cell::Integer;
        vec![
            int(self.id),
            int(self.nid),
            int(self.did),
            int(0),
            int(0),
            int(-1),
            int(self.card_type),
            int(self.queue),
            int(self.due),
            int(self.ivl),
            int(2500),
            int(1),
            int(0),
            int(0),
            int(self.odue),
            int(self.odid),
            int(0),
            Cell::text(self.data.clone()),
        ]
    }
}

/// The fixture with no card, at `now_ms`.
fn empty_at(now_ms: i64) -> Fixture {
    let f = Fixture::with_raw(&["DELETE FROM cards"]);
    f.clock.set_now_ms(now_ms);
    f
}

fn empty() -> Fixture {
    empty_at(R_NOW_MS)
}

fn put(f: &Fixture, cards: &[Raw]) {
    let rows: Vec<(i64, Vec<Cell>)> = cards.iter().map(|card| (card.id, card.cells())).collect();
    f.write_raw(move |scope| {
        for (id, cells) in rows {
            scope.restore(Table::Cards, &RowKey::Id(id), Some(&cells))?;
        }
        Ok(())
    });
}

fn request(f: &Fixture, scope: Scope, history: History, graphs: &[Graph]) -> Box<GraphData> {
    let request = GraphRequest {
        scope: scope.clone(),
        history,
        graphs: graphs.to_vec(),
        refresh: 1,
        context: None,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    match f.host.stats().request(&f.collection(), f.host.derived(), &request).unwrap() {
        GraphResult::Data(data) => data,
        other => panic!("{scope:?}: {other:?}"),
    }
}

fn context(f: &Fixture) -> Context {
    request(f, Scope::Collection, History::AllHistory, &[]).context
}

/// One graph's record over `scope`.
fn record<T: Clone + 'static>(f: &Fixture, scope: Scope, graph: Graph) -> T {
    let data = request(f, scope, History::Last12Months, &[graph]);
    assert_eq!(data.records[0].graph(), graph);
    data.records[0].as_any().downcast_ref::<T>().expect("the graph's record").clone()
}

fn collection<T: Clone + 'static>(f: &Fixture, graph: Graph) -> T {
    record(f, Scope::Collection, graph)
}

fn map<K: Ord + Copy, const N: usize>(pairs: [(K, u32); N]) -> BTreeMap<K, u32> {
    pairs.into_iter().collect()
}

/// G14's tolerance: 1e-6 relative.
#[track_caller]
fn close(actual: f32, expected: f32) {
    let tolerance = 1e-6 * f64::from(expected).abs();
    assert!(
        (f64::from(actual) - f64::from(expected)).abs() <= tolerance,
        "{actual} is not {expected}"
    );
}

fn add_deck(f: &Fixture, native: &str) -> i64 {
    let deck = meguri_engine::decks::new_deck(native.to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    f.write(move |scope| {
        tx.send(scope.add_deck(&deck)?.0).unwrap();
        Ok(())
    });
    rx.recv().unwrap()
}

fn set_config(f: &Fixture, key: &'static str, value: ConfigValue) {
    f.write(move |scope| {
        scope.set_config(key, &value)?;
        Ok(())
    });
}

fn wait_columns(f: &Fixture) {
    f.host.derived().request_refresh(COMPONENT);
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(120);
    while f.host.derived().readiness(COMPONENT).map(|r| r.readiness) != Some(Readiness::Ready) {
        assert!(meguri_engine::monotonic::now() < deadline, "the stats columns never became ready");
        std::thread::sleep(Duration::from_millis(5));
    }
}

// C3. Future Due.

#[test]
fn c3_ae57_buried_cards_and_the_backlog() {
    // Golden future-due/ae57-buried-backlog.
    let f = empty();
    let t = i64::from(context(&f).today);
    let mut cards = Vec::new();
    for day in 1..=3 {
        for at in 0..10 {
            cards.push(raw(100 + day * 10 + at).review(2, t - day));
        }
    }
    for at in 0..5 {
        cards.push(raw(200 + at).review(2, t).state(2, if at % 2 == 0 { -2 } else { -3 }));
    }
    for at in 0..3 {
        cards.push(raw(300 + at).review(2, t + 1).state(2, -2));
    }
    put(&f, &cards);
    for scope in [Scope::Collection, Scope::Deck(DeckId(LANG))] {
        let due: FutureDueRecord = record(&f, scope, Graph::FutureDue);
        assert_eq!(due.due_days, map([(-3, 10), (-2, 10), (-1, 10), (1, 3)]));
        assert!(due.have_backlog);
        assert_eq!(due.daily_load, 19);
        assert_eq!(due.due_tomorrow, 3);
    }
}

#[test]
fn c3_2_due_kinds_follow_the_value_not_the_queue() {
    // Golden future-due/due-kinds.
    let f = empty();
    let Context { next_day_at: n, today, .. } = context(&f);
    let t = i64::from(today);
    put(&f, &[
        raw(1).state(1, 1).due(n + 1_800),
        raw(2).state(1, 1).due(n + 90_000),
        raw(3).state(1, 1).due(n - 1),
        raw(4).state(1, 1).due(n - DAY),
        raw(5).state(1, 3).due(t),
        raw(6).review(3, t + 3),
        raw(7).state(1, 1).due(1_000_000_000),
    ]);
    let due: FutureDueRecord = collection(&f, Graph::FutureDue);
    let far = (1_000_000_000 - t) as i32;
    assert_eq!(due.due_days, map([(-1, 1), (0, 3), (1, 1), (3, 1), (far, 1)]));
    assert!(due.have_backlog);
    assert_eq!(due.daily_load, 6);
    assert_eq!(due.due_tomorrow, 1);
}

#[test]
fn c2_the_home_due_of_filtered_and_stray_cards() {
    // Golden future-due/filtered-odue (`Cram` is filtered, `Lang` home).
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &[
        raw(1).review(5, -100_000).filtered(CRAM, LANG, t + 5),
        raw(2).state(1, 3).due(-100_001).filtered(CRAM, LANG, 0),
        raw(3).review(5, t + 2).filtered(LANG, 0, t + 7),
        raw(4).review(5, t + 3).filtered(LANG, -1, t + 9),
    ]);
    let minus_t = -(t as i32);
    for scope in [Scope::Collection, Scope::Deck(DeckId(LANG))] {
        let due: FutureDueRecord = record(&f, scope, Graph::FutureDue);
        assert_eq!(due.due_days, map([(minus_t, 1), (2, 1), (3, 1), (5, 1)]));
        assert_eq!((due.have_backlog, due.daily_load), (true, 1));
    }
    let due: FutureDueRecord = record(&f, Scope::Deck(DeckId(CRAM)), Graph::FutureDue);
    assert_eq!(due.due_days, map([(minus_t, 1), (5, 1)]));
    assert_eq!(due.daily_load, 1);
}

#[test]
fn c3_1_and_c3_3_types_and_queues() {
    // Golden future-due/types-and-queues.
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &[
        raw(1).review(5, t + 2).state(2, -1),
        raw(2).state(0, -1),
        raw(3).state(0, -2),
        raw(4).review(1, t + 4).state(2, -2),
        raw(5).review(1, t).state(2, -3),
    ]);
    let due: FutureDueRecord = collection(&f, Graph::FutureDue);
    assert_eq!(due.due_days, map([(4, 1)]));
    assert!(!due.have_backlog);
    // The buried card due today is in the load, not in the map.
    assert_eq!(due.daily_load, 2);
    assert_eq!(due.due_tomorrow, 0);
}

#[test]
fn c3_4_daily_load_truncates_an_f32_sum() {
    // Golden future-due/daily-load.
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &(0..5).map(|ivl| raw(10 + ivl).review(ivl, t + 1)).collect::<Vec<_>>());
    let due: FutureDueRecord = collection(&f, Graph::FutureDue);
    assert_eq!(due.due_days, map([(1, 5)]));
    assert_eq!((due.daily_load, due.have_backlog, due.due_tomorrow), (3, false, 5));
}

#[test]
fn c3_2_due_extremes() {
    // Golden future-due/due-extremes (at `today` 30 the wrap is 2147483618).
    let f = empty();
    let Context { next_day_at: n, today, .. } = context(&f);
    put(&f, &[raw(1).review(0, n + 259_200), raw(2).review(5, -2_147_483_648)]);
    let due: FutureDueRecord = collection(&f, Graph::FutureDue);
    let wrapped = i32::MIN.wrapping_sub(today as i32);
    assert_eq!(due.due_days, map([(3, 1), (wrapped, 1)]));
    assert_eq!((due.daily_load, due.have_backlog), (1, false));
}

// C4. Card Counts.

/// The cards of `card-counts/states` (golden), with three notes.
fn state_cards(f: &Fixture, deck: i64) -> Vec<Raw> {
    let n = context(f).next_day_at;
    vec![
        raw(1).note(11),
        raw(2).state(1, 1).due(n + 600).note(11),
        raw(3).state(3, 1).ivl(3).due(n + 600).note(12),
        raw(4).review(20, 5).note(12),
        raw(5).review(21, 5).note(13),
        raw(6).review(30, 5).state(2, -1).note(13),
        raw(7).state(0, -2).note(13),
        raw(8).state(1, -3).due(n + 600).note(13),
    ]
    .into_iter()
    .map(|card| card.deck(deck))
    .collect()
}

fn counts(values: [u32; 7]) -> StateCounts {
    let [new, learning, relearning, young, mature, suspended, buried] = values;
    StateCounts {
        new,
        learning,
        relearning,
        young,
        mature,
        suspended,
        buried,
    }
}

#[test]
fn c4_1_both_count_sets_and_c4_2_notes() {
    let f = empty();
    put(&f, &state_cards(&f, LANG));
    let counted: CardCountsRecord = collection(&f, Graph::CardCounts);
    assert_eq!(counted.excluding_inactive, counts([1, 1, 1, 1, 1, 1, 2]));
    assert_eq!(counted.including_inactive, counts([2, 2, 1, 1, 2, 0, 0]));
    assert_eq!(counted.notes, 3);
    // Both totals are the scope's cards.
    for set in [counted.excluding_inactive, counted.including_inactive] {
        assert_eq!(set.values().iter().sum::<u32>(), 8);
    }
}

#[test]
fn c4_1_a_filtered_card_counts_in_its_deck_its_home_deck_and_the_collection() {
    // Golden card-counts/filtered-deck.
    let f = empty();
    put(&f, &[raw(1).review(30, 5).filtered(CRAM, LANG, 5)]);
    for scope in [Scope::Deck(DeckId(CRAM)), Scope::Deck(DeckId(LANG)), Scope::Collection] {
        let counted: CardCountsRecord = record(&f, scope.clone(), Graph::CardCounts);
        assert_eq!(counted.excluding_inactive, counts([0, 0, 0, 0, 1, 0, 0]), "{scope:?}");
        assert_eq!(counted.including_inactive, counts([0, 0, 0, 0, 1, 0, 0]), "{scope:?}");
    }
    let counted: CardCountsRecord = record(&f, Scope::Deck(DeckId(VERBS)), Graph::CardCounts);
    assert_eq!(counted.excluding_inactive, StateCounts::default());
}

#[test]
fn c4_3_the_label_texts_of_both_settings() {
    let suffix = " AND -(\"is:buried\" OR \"is:suspended\")";
    let separate = [
        (CountLabel::New, "\"is:new\""),
        (CountLabel::Learning, "(-\"is:review\" AND \"is:learn\")"),
        (CountLabel::Relearning, "(\"is:review\" AND \"is:learn\")"),
        (CountLabel::Young, "(\"is:review\" AND -\"is:learn\") AND \"prop:ivl<21\""),
        (CountLabel::Mature, "(\"is:review\" AND -\"is:learn\") AND \"prop:ivl>=21\""),
    ];
    let f = empty();
    let counted: CardCountsRecord = collection(&f, Graph::CardCounts);
    let expected: Vec<LabelText> = separate
        .iter()
        .map(|(label, text)| LabelText {
            label: *label,
            text: format!("{text}{suffix}"),
        })
        .chain([
            LabelText {
                label: CountLabel::Suspended,
                text: "\"is:suspended\"".into(),
            },
            LabelText {
                label: CountLabel::Buried,
                text: "\"is:buried\"".into(),
            },
        ])
        .collect();
    assert_eq!(counted.labels_separate, expected);
    let together: Vec<LabelText> = separate
        .iter()
        .map(|(label, text)| LabelText {
            label: *label,
            text: (*text).into(),
        })
        .collect();
    assert_eq!(counted.labels_together, together);
    assert_eq!(label_text(CountLabel::Suspended, false), None);
    assert_eq!(label_text(CountLabel::Buried, false), None);
}

#[test]
fn ae60_each_label_finds_exactly_the_cards_it_counts() {
    let f = empty();
    let japanese = add_deck(&f, "Japanese");
    let mut cards = state_cards(&f, japanese);
    // Outside the scope: a card of each kind that must not be found.
    cards.extend(state_cards(&f, LANG).into_iter().map(|card| Raw { id: card.id + 100, ..card }));
    put(&f, &cards);
    // The label each Japanese card counts under, separating and not.
    let separate = [
        CountLabel::New,
        CountLabel::Learning,
        CountLabel::Relearning,
        CountLabel::Young,
        CountLabel::Mature,
        CountLabel::Suspended,
        CountLabel::Buried,
        CountLabel::Buried,
    ];
    let together = [
        CountLabel::New,
        CountLabel::Learning,
        CountLabel::Relearning,
        CountLabel::Young,
        CountLabel::Mature,
        CountLabel::Mature,
        CountLabel::New,
        CountLabel::Learning,
    ];
    for (setting, rows) in [(true, separate), (false, together)] {
        set_config(&f, "cardCountsSeparateInactive", ConfigValue::Bool(setting));
        let data = request(&f, Scope::Deck(DeckId(japanese)), History::Last12Months, &[Graph::CardCounts]);
        assert_eq!(data.preferences.card_counts_separate_inactive, setting);
        let counted = data.records[0].as_any().downcast_ref::<CardCountsRecord>().unwrap();
        let labels = if setting { &counted.labels_separate } else { &counted.labels_together };
        for label in labels {
            let text = join(&data.browse_scope, &label.text).expect("a clickable scope");
            if setting && label.label == CountLabel::Young {
                // AE60's bytes.
                assert_eq!(
                    text,
                    "(deck:\"Japanese\") AND (\"is:review\" AND -\"is:learn\") AND \"prop:ivl<21\" AND -(\"is:buried\" OR \"is:suspended\")"
                );
            }
            let found: BTreeSet<i64> = meguri_engine::browse::search::search_ids(f.host.engine(), &text, SearchMode::Cards)
                .unwrap()
                .into_iter()
                .collect();
            let expected: BTreeSet<i64> = rows
                .iter()
                .zip(1..)
                .filter(|(row, _)| **row == label.label)
                .map(|(_, id)| id)
                .collect();
            assert_eq!(found, expected, "{setting} {:?}: {text}", label.label);
        }
    }
}

// C5. Intervals.

#[test]
fn c5_review_and_relearning_cards_in_every_queue() {
    // Golden intervals/types-and-queues.
    let f = empty();
    put(&f, &[
        raw(1).review(0, 5),
        raw(2).review(1, 5),
        raw(3).review(21, 5),
        raw(4).review(36_500, 5),
        raw(5).review(21, 5).state(2, -1),
        raw(6).state(3, 1).ivl(1),
        raw(7).ivl(5),
        raw(8).state(1, 1).ivl(5),
    ]);
    let intervals: IntervalsRecord = collection(&f, Graph::Intervals);
    assert_eq!(intervals.by_interval, map([(0, 1), (1, 2), (21, 2), (36_500, 1)]));
}

// C6 to C9. Memory states.

#[test]
fn c7_stability_rounding_and_c6_the_cards_left_out() {
    // Golden fsrs/stability-rounding.
    let f = empty();
    let mut cards: Vec<Raw> = ["0.4", "0.5", "2.5", "20.49", "-3", "1e40"]
        .iter()
        .zip(1..)
        .map(|(s, id)| raw(id).review(3, 5).data(&format!("{{\"s\":{s},\"d\":5.0}}")))
        .collect();
    cards.push(raw(20).data("{\"s\":2.5,\"d\":5.0}"));
    cards.push(raw(21).review(3, 5).data("{\"s\":7.0}"));
    // Not left out: a new and a learning card with no state.
    cards.push(raw(22));
    cards.push(raw(23).state(1, 1));
    put(&f, &cards);
    let stability: StabilityRecord = collection(&f, Graph::Stability);
    assert_eq!(stability.by_days, map([(0, 2), (1, 1), (3, 2), (20, 1), (u32::MAX, 1)]));
    assert_eq!(stability.left_out, 1);
    let difficulty: DifficultyRecord = collection(&f, Graph::Difficulty);
    let retrievability: RetrievabilityRecord = collection(&f, Graph::Retrievability);
    assert_eq!((difficulty.left_out, retrievability.left_out), (1, 1));
}

const EIGHT: [&str; 8] = ["1", "1.09", "2.08", "4.6", "5.5", "10", "11", "0.5"];

fn with_difficulties(first: i64, deck: i64, values: &[&str]) -> Vec<Raw> {
    values
        .iter()
        .zip(first..)
        .map(|(d, id)| raw(id).review(3, 5).deck(deck).data(&format!("{{\"s\":5.0,\"d\":{d}}}")))
        .collect()
}

#[test]
fn c8_difficulty_bins_and_median() {
    // Golden fsrs/difficulty-bins.
    let f = empty();
    put(&f, &with_difficulties(1, LANG, &EIGHT));
    let difficulty: DifficultyRecord = collection(&f, Graph::Difficulty);
    assert_eq!(
        difficulty.by_percent,
        map([(0, 2), (1, 1), (11, 1), (39, 1), (50, 1), (99, 1), (111, 1)])
    );
    assert_eq!(difficulty.median, 26.0);
    assert_eq!(difficulty.left_out, 0);
}

#[test]
fn c8_infinite_difficulties() {
    // Golden fsrs/difficulty-infinite.
    let f = empty();
    let a = add_deck(&f, "Alpha");
    let b = add_deck(&f, "Beta");
    let mut first = EIGHT.to_vec();
    first.push("1e40");
    let mut cards = with_difficulties(1, a, &first);
    cards.extend(with_difficulties(20, b, &["-1e40", "5.5", "10"]));
    put(&f, &cards);
    let in_deck = |deck: i64| record::<DifficultyRecord>(&f, Scope::Deck(DeckId(deck)), Graph::Difficulty);
    let alpha = in_deck(a);
    assert_eq!(
        alpha.by_percent,
        map([(0, 2), (1, 1), (11, 1), (39, 1), (50, 1), (99, 1), (111, 1), (u32::MAX, 1)])
    );
    assert_eq!(alpha.median, 39.999996);
    let beta = in_deck(b);
    assert_eq!(beta.by_percent, map([(0, 1), (50, 1), (99, 1)]));
    assert_eq!(beta.median, 50.0);
    let all: DifficultyRecord = collection(&f, Graph::Difficulty);
    assert_eq!(
        all.by_percent,
        map([(0, 3), (1, 1), (11, 1), (39, 1), (50, 2), (99, 2), (111, 1), (u32::MAX, 1)])
    );
    assert_eq!(all.median, 45.0);
}

/// The first seven rows of C9.6 at the retrievability clock, one note each.
fn elapsed_cards(t: i64) -> Vec<Raw> {
    vec![
        raw(1).review(10, t + 1).data(&format!("{{\"s\":2.5,\"d\":5.0,\"lrt\":{}}}", NOW_S - 864_000)),
        raw(2).review(20, t + 5).data("{\"s\":5.0,\"d\":5.0}"),
        raw(3).review(10, t + 1).data(&format!("{{\"s\":5.0,\"d\":5.0,\"lrt\":{}}}", NOW_S + 10)),
        raw(4).state(3, 1).ivl(0).due(NOW_S + 600).data("{\"s\":5.0,\"d\":5.0}"),
        raw(5).state(3, 1).ivl(2).due(NOW_S - 3_600).data("{\"s\":4.0,\"d\":6.0}"),
        raw(6).review(40, t + 5).data("{\"s\":5.0,\"d\":5.0}"),
        raw(7).due(5).data("{\"s\":5.0,\"d\":5.0}"),
    ]
    .into_iter()
    .map(|card| {
        let nid = 50 + card.id;
        card.note(nid)
    })
    .collect()
}

#[test]
fn c9_1_to_c9_3_elapsed_time_and_bins() {
    // Golden fsrs/retrievability-elapsed.
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &elapsed_cards(t));
    let recall: RetrievabilityRecord = collection(&f, Graph::Retrievability);
    assert_eq!(recall.by_percent, map([(2, 2), (3, 1), (61, 1), (71, 1), (76, 1), (99, 1)]));
    close(recall.average, 45.299477);
    close(recall.sum_by_card, 3.1709635);
    close(recall.sum_by_note, 3.1709635);
    assert_eq!(recall.left_out, 0);
}

#[test]
fn c9_4_sums_by_card_and_by_note() {
    // Golden fsrs/retrievability-sums: note A holds the first card of C9.6
    // and a card without a memory state, note B the second card.
    let f = empty();
    let t = i64::from(context(&f).today);
    let rows = elapsed_cards(t);
    put(&f, &[
        rows[0].clone().note(7),
        raw(9).review(3, t).note(7),
        rows[1].clone().note(8),
    ]);
    let recall: RetrievabilityRecord = collection(&f, Graph::Retrievability);
    assert_eq!(recall.by_percent, map([(71, 1), (76, 1)]));
    close(recall.sum_by_card, 1.4844089);
    close(recall.average, 74.22044);
    close(recall.sum_by_note, 1.1252699);
    assert_eq!(recall.left_out, 1);
}

#[test]
fn c9_4_a_card_whose_note_row_is_missing_stays_in_the_sums() {
    // Golden fsrs/retrievability-orphan: the notes table is never read.
    let f = empty();
    let t = i64::from(context(&f).today);
    let rows = elapsed_cards(t);
    put(&f, &[rows[0].clone().note(424_242), rows[1].clone().note(NOTE)]);
    for scope in [Scope::Collection, Scope::Deck(DeckId(LANG))] {
        let recall: RetrievabilityRecord = record(&f, scope, Graph::Retrievability);
        close(recall.sum_by_card, 1.4844089);
        close(recall.sum_by_note, 1.4844089);
        close(recall.average, 74.22044);
    }
}

#[test]
fn c9_2_the_card_decay() {
    // Golden fsrs/retrievability-decay, every card in one scope.
    let f = empty();
    let t = i64::from(context(&f).today);
    let decays = ["\"decay\":0.1542,", "\"decay\":-0.5,", "", "\"decay\":null,", "\"decay\":\"0.2\",", "\"decay\":0,"];
    let mut cards = Vec::new();
    for (at, decay) in decays.iter().enumerate() {
        let at = at as i64;
        cards.push(raw(10 + at).note(10 + at).review(10, t).data(&format!(
            "{{{decay}\"s\":5.0,\"d\":5.0,\"lrt\":{}}}",
            NOW_S - 864_000
        )));
        cards.push(raw(20 + at).note(20 + at).review(20, t + 5).data(&format!("{{{decay}\"s\":5.0,\"d\":5.0}}")));
    }
    put(&f, &cards);
    let recall: RetrievabilityRecord = collection(&f, Graph::Retrievability);
    assert_eq!(
        recall.by_percent,
        map([(65, 1), (76, 3), (78, 1), (80, 1), (82, 3), (84, 1), (99, 2)])
    );
    close(recall.average, 82.265785);
    close(recall.sum_by_card, 9.871895);
    // Anki's note order is unspecified (its golden has 9.871896); here one
    // card per note in id order gives the same sum bit for bit.
    assert_eq!(recall.sum_by_note.to_bits(), recall.sum_by_card.to_bits());
}

#[test]
fn c1_and_c9_4_f32_sums_follow_card_and_note_id_order() {
    // R 1 (decay 0) on the lowest card id, then two R of about 4.2e-8
    // (decay 3, s 0.001, 8 days). In id order each small value is lost
    // against 1.0; added first they survive. The notes run the other way,
    // so `sum_by_note` (ascending note id) takes the small values first.
    let f = empty();
    let lrt = NOW_S - 8 * DAY;
    put(&f, &[
        raw(1).note(30).review(10, 5).data(&format!("{{\"decay\":0,\"s\":5.0,\"d\":5.0,\"lrt\":{lrt}}}")),
        raw(2).note(20).review(10, 5).data(&format!("{{\"decay\":3,\"s\":0.001,\"d\":5.0,\"lrt\":{lrt}}}")),
        raw(3).note(10).review(10, 5).data(&format!("{{\"decay\":3,\"s\":0.001,\"d\":5.0,\"lrt\":{lrt}}}")),
    ]);
    let small = fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: 0.001,
            difficulty: 5.0,
        },
        (8 * DAY) as f32 / 86_400.0,
        3.0,
    );
    let id_order = 1.0f32 + small + small;
    let small_first = small + small + 1.0f32;
    assert_ne!(id_order.to_bits(), small_first.to_bits(), "the values must be order-sensitive");
    let recall: RetrievabilityRecord = collection(&f, Graph::Retrievability);
    assert_eq!(recall.sum_by_card.to_bits(), id_order.to_bits());
    assert_eq!(recall.sum_by_note.to_bits(), small_first.to_bits());
}

#[test]
fn c9_1_odd_elapsed_values() {
    // Golden fsrs/retrievability-elapsed-odd, but the filtered card with
    // `odue` 0, whose elapsed days (`today` + 20) depend on the
    // collection's day number: its R is checked against the fsrs function.
    let f = empty();
    let t = i64::from(context(&f).today);
    let state = "\"s\":5.0,\"d\":5.0";
    put(&f, &[
        raw(1).review(20, 35).filtered(CRAM, LANG, 0).data(&format!("{{{state}}}")),
        raw(2).review(20, t + 5).filtered(LANG, -1, t + 10).data(&format!("{{{state}}}")),
        raw(3).state(3, 1).ivl(2).due(25).data(&format!("{{{state}}}")),
        raw(4).review(0, 1_896_429_600).state(2, -1).data(&format!("{{{state}}}")),
        raw(5).review(10, t).data(&format!("{{{state},\"lrt\":-9223372036854775808}}")),
    ]);
    let in_scope = |scope: Scope| record::<RetrievabilityRecord>(&f, scope, Graph::Retrievability);
    let filtered = fsrs::current_retrievability(
        fsrs::MemoryState {
            stability: 5.0,
            difficulty: 5.0,
        },
        ((t + 20) * DAY) as u32 as f32 / 86_400.0,
        0.5,
    );
    let filtered_key = (filtered * 100.0).floor() as u32;
    let cram = in_scope(Scope::Deck(DeckId(CRAM)));
    assert_eq!(cram.by_percent, map([(filtered_key, 1)]));
    assert_eq!(cram.sum_by_card, filtered);
    let all = in_scope(Scope::Collection);
    let mut expected = map([(3, 2), (76, 1), (99, 1)]);
    *expected.entry(filtered_key).or_insert(0) += 1;
    assert_eq!(all.by_percent, expected);
    // The stray `odid` takes `due` (15 days), the day-number relearning card
    // now_s + 172,775 s, the suspended review 0 s, the extreme `lrt` now_s.
    let others = fsrs_sum(&[(15 * DAY) as u32, (NOW_S + 172_775) as u32, 0, NOW_S as u32]);
    close(all.sum_by_card, filtered + others);
}

/// The f32 sum of R at stability 5 for each elapsed second count, in order.
fn fsrs_sum(elapsed: &[u32]) -> f32 {
    elapsed
        .iter()
        .map(|e| {
            fsrs::current_retrievability(
                fsrs::MemoryState {
                    stability: 5.0,
                    difficulty: 5.0,
                },
                *e as f32 / 86_400.0,
                0.5,
            )
        })
        .sum()
}

#[test]
fn c9_5_r_is_recomputed_for_each_request_with_the_caches_warm() {
    // As fsrs/retrievability-now: the keys move after 10 days, and the two
    // histories then agree.
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &[
        raw(1).review(10, t).data(&format!("{{\"s\":30.0,\"d\":5.0,\"lrt\":{}}}", NOW_S - 2 * DAY)),
        raw(2).review(10, t).data(&format!("{{\"s\":60.0,\"d\":5.0,\"lrt\":{}}}", NOW_S - 3 * DAY)),
    ]);
    wait_columns(&f);
    let before = request(&f, Scope::Collection, History::Last12Months, &[Graph::Retrievability]);
    assert!(before.scope.from_caches());
    let keys = |data: &GraphData| {
        data.records[0]
            .as_any()
            .downcast_ref::<RetrievabilityRecord>()
            .unwrap()
            .clone()
    };
    let first = keys(&before);
    f.clock.set_now_ms(R_NOW_MS + 10 * DAY * 1000);
    let year = request(&f, Scope::Collection, History::Last12Months, &[Graph::Retrievability]);
    let all = request(&f, Scope::Collection, History::AllHistory, &[Graph::Retrievability]);
    assert!(year.scope.from_caches());
    let (later, later_all) = (keys(&year), keys(&all));
    assert_ne!(first.by_percent, later.by_percent);
    assert!(later.sum_by_card < first.sum_by_card);
    assert_eq!(later, later_all);
    assert_eq!(later.sum_by_card, fsrs_with(&[(30.0, 12 * DAY), (60.0, 13 * DAY)]));
}

fn fsrs_with(cards: &[(f32, i64)]) -> f32 {
    cards
        .iter()
        .map(|(stability, elapsed)| {
            fsrs::current_retrievability(
                fsrs::MemoryState {
                    stability: *stability,
                    difficulty: 5.0,
                },
                *elapsed as u32 as f32 / 86_400.0,
                0.5,
            )
        })
        .sum()
}

#[test]
fn c9_6_a_card_reviewed_400_days_ago_agrees_with_prop_r() {
    let f = empty();
    let t = i64::from(context(&f).today);
    assert!(t >= 400, "the fixture's day number {t}");
    let ivl = 30;
    put(&f, &[raw(1).review(ivl, t - 400 + ivl).data("{\"s\":40.0,\"d\":5.0}")]);
    let year: RetrievabilityRecord = record(&f, Scope::Collection, Graph::Retrievability);
    let all = request(&f, Scope::Collection, History::AllHistory, &[Graph::Retrievability]);
    let all = all.records[0].as_any().downcast_ref::<RetrievabilityRecord>().unwrap().clone();
    assert_eq!(year, all);
    // `prop:r`'s value, from the search's own SQL function.
    let collection = f.collection();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let rows = snapshot
        .execute_raw(&format!("SELECT meguri_retrievability(data, due, ivl, {t}, {NOW_S}) FROM cards"), &[])
        .unwrap()
        .rows;
    let SqlValue::Real(prop_r) = rows[0][0] else {
        panic!("{rows:?}");
    };
    assert_eq!(year.sum_by_card, prop_r as f32);
}

// C10. Added.

#[test]
fn c10_creation_days_from_n_whatever_the_history() {
    // Goldens added/day-edges and added/day-edges-odd, relative to `N`.
    let f = empty();
    let n = context(&f).next_day_at;
    let ids = [
        n * 1000,
        (n - DAY + 1) * 1000,
        (n - DAY) * 1000 + 999,
        (n - DAY) * 1000,
        (n + 1) * 1000,
        (n - 400 * DAY) * 1000,
        n * 1000 - 1,
        -1,
        -1_500,
    ];
    put(&f, &ids.iter().map(|id| raw(*id)).collect::<Vec<_>>());
    let before_epoch = -((n / DAY) as i32);
    for history in [History::Last12Months, History::AllHistory] {
        let data = request(&f, Scope::Collection, history, &[Graph::Added]);
        let added = data.records[0].as_any().downcast_ref::<AddedRecord>().unwrap();
        assert_eq!(added.by_day, map([(before_epoch, 2), (-400, 1), (-1, 2), (0, 3), (1, 1)]));
    }
}

// C11. Records.

#[test]
fn c11_every_card_record_in_one_request_and_an_empty_scope() {
    // The setup of card-counts/empty-scope (no card in the scope). Its
    // golden pins C12's JSON shape, which is U104's observation; this
    // checks the records under it.
    let f = empty();
    let t = i64::from(context(&f).today);
    put(&f, &[raw(1).review(3, t + 1).data("{\"s\":5.0,\"d\":5.0}")]);
    let data = request(&f, Scope::Search("tag:absent".into()), History::Last12Months, &CARD_GRAPHS);
    let graphs: Vec<Graph> = data.records.iter().map(|record| record.graph()).collect();
    assert_eq!(graphs, CARD_GRAPHS);
    let of = |at: usize| data.records[at].as_any();
    assert_eq!(of(0).downcast_ref::<FutureDueRecord>().unwrap(), &FutureDueRecord::default());
    let counted = of(1).downcast_ref::<CardCountsRecord>().unwrap();
    assert_eq!((counted.excluding_inactive, counted.including_inactive, counted.notes), (StateCounts::default(), StateCounts::default(), 0));
    assert_eq!(of(2).downcast_ref::<IntervalsRecord>().unwrap(), &IntervalsRecord::default());
    assert_eq!(of(3).downcast_ref::<StabilityRecord>().unwrap(), &StabilityRecord::default());
    assert_eq!(of(4).downcast_ref::<DifficultyRecord>().unwrap(), &DifficultyRecord::default());
    assert_eq!(of(5).downcast_ref::<RetrievabilityRecord>().unwrap(), &RetrievabilityRecord::default());
    assert_eq!(of(6).downcast_ref::<AddedRecord>().unwrap(), &AddedRecord::default());
    let counted: CardCountsRecord = collection(&f, Graph::CardCounts);
    assert_eq!(counted.excluding_inactive, counts([0, 0, 0, 1, 0, 0, 0]));
}

#[test]
fn c6_3_the_fsrs_key_changes_no_card_record() {
    // Golden fsrs/fsrs-off.
    let f = empty();
    let t = i64::from(context(&f).today);
    let state = format!("{{\"s\":5.0,\"d\":5.0,\"lrt\":{}}}", NOW_S - 10 * DAY);
    put(&f, &[raw(1).review(10, t).data(&state).note(1), raw(2).review(10, t).data(&state).note(2)]);
    let mut seen = Vec::new();
    for fsrs in [true, false] {
        set_config(&f, "fsrs", ConfigValue::Bool(fsrs));
        let data = request(&f, Scope::Collection, History::Last12Months, &CARD_GRAPHS);
        assert_eq!(data.preferences.fsrs, fsrs);
        let encoded: Vec<Vec<u8>> = data.records.iter().map(|record| encode(record.as_ref())).collect();
        seen.push(encoded);
        let recall = data.records[5].as_any().downcast_ref::<RetrievabilityRecord>().unwrap();
        assert_eq!(recall.by_percent, map([(82, 2)]));
        close(recall.average, 82.50286);
        close(recall.sum_by_card, 1.6500572);
        let difficulty = data.records[4].as_any().downcast_ref::<DifficultyRecord>().unwrap();
        assert_eq!(difficulty.by_percent, map([(44, 2)]));
        close(difficulty.median, 44.444447);
    }
    assert_eq!(seen[0], seen[1]);
}

fn encode(record: &dyn GraphRecord) -> Vec<u8> {
    let mut out = meguri_stats::graphs::encoding::Encoder::new();
    record.encode(&mut out);
    out.into_bytes()
}

fn decoded_counts(decoder: &mut Decoder<'_>) -> BTreeMap<i64, i64> {
    let map = decoder.sparse_map(&[MemberKind::Integers]).unwrap();
    map.keys.into_iter().zip(map.integers[0].iter().copied()).collect()
}

fn widened<K: Copy + Into<i64>>(map: &BTreeMap<K, u32>) -> BTreeMap<i64, i64> {
    map.iter().map(|(key, count)| ((*key).into(), i64::from(*count))).collect()
}

#[test]
fn g6_each_card_record_encodes_and_decodes() {
    let f = empty();
    let t = i64::from(context(&f).today);
    let mut cards = state_cards(&f, LANG);
    cards.extend(elapsed_cards(t).into_iter().map(|card| Raw { id: card.id + 20, ..card }));
    cards.push(raw(40).review(36_500, t - 3));
    put(&f, &cards);
    let data = request(&f, Scope::Collection, History::AllHistory, &CARD_GRAPHS);
    let bytes: Vec<Vec<u8>> = data.records.iter().map(|record| encode(record.as_ref())).collect();
    let of = |at: usize| data.records[at].as_any();

    let due = of(0).downcast_ref::<FutureDueRecord>().unwrap();
    let mut d = Decoder::new(&bytes[0]);
    assert_eq!(decoded_counts(&mut d), widened(&due.due_days));
    assert_eq!(
        [d.integer().unwrap(), d.integer().unwrap(), d.integer().unwrap()],
        [i64::from(due.have_backlog), i64::from(due.daily_load), i64::from(due.due_tomorrow)]
    );
    assert!(d.is_done() && due.have_backlog);

    let counted = of(1).downcast_ref::<CardCountsRecord>().unwrap();
    let mut d = Decoder::new(&bytes[1]);
    for set in [counted.excluding_inactive, counted.including_inactive] {
        for value in set.values() {
            assert_eq!(d.integer().unwrap(), i64::from(value));
        }
    }
    assert_eq!(d.integer().unwrap(), i64::from(counted.notes));
    for labels in [&counted.labels_separate, &counted.labels_together] {
        let read = d.list(|d| Ok((d.integer()?, d.text()?))).unwrap();
        let written: Vec<(i64, String)> = labels.iter().map(|l| (l.label as i64, l.text.clone())).collect();
        assert_eq!(read, written);
    }
    assert!(d.is_done());

    let intervals = of(2).downcast_ref::<IntervalsRecord>().unwrap();
    let mut d = Decoder::new(&bytes[2]);
    assert_eq!(decoded_counts(&mut d), widened(&intervals.by_interval));
    assert!(d.is_done() && intervals.by_interval.contains_key(&36_500));

    let stability = of(3).downcast_ref::<StabilityRecord>().unwrap();
    let mut d = Decoder::new(&bytes[3]);
    assert_eq!(decoded_counts(&mut d), widened(&stability.by_days));
    assert_eq!(d.integer().unwrap(), i64::from(stability.left_out));
    assert!(d.is_done());

    let difficulty = of(4).downcast_ref::<DifficultyRecord>().unwrap();
    let mut d = Decoder::new(&bytes[4]);
    assert_eq!(decoded_counts(&mut d), widened(&difficulty.by_percent));
    assert_eq!(d.f32().unwrap().to_bits(), difficulty.median.to_bits());
    assert_eq!(d.integer().unwrap(), i64::from(difficulty.left_out));
    assert!(d.is_done());

    let recall = of(5).downcast_ref::<RetrievabilityRecord>().unwrap();
    let mut d = Decoder::new(&bytes[5]);
    assert_eq!(decoded_counts(&mut d), widened(&recall.by_percent));
    for value in [recall.average, recall.sum_by_card, recall.sum_by_note] {
        assert_eq!(d.f32().unwrap().to_bits(), value.to_bits());
    }
    assert_eq!(d.integer().unwrap(), i64::from(recall.left_out));
    assert!(d.is_done());

    let added = of(6).downcast_ref::<AddedRecord>().unwrap();
    let mut d = Decoder::new(&bytes[6]);
    assert_eq!(decoded_counts(&mut d), widened(&added.by_day));
    assert!(d.is_done());
}
