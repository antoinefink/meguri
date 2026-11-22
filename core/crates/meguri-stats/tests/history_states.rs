//! `stats/history-reads` H2 and H14 (U289): Cards Over Time and Started,
//! kept per deck key in the `stats_history` component. There is no oracle:
//! hand-built logs with known answers, direct counts of the collection, and
//! the component against a cold replay after random changes (H20). Names
//! and values are made up.

use std::collections::{BTreeMap, HashMap};
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meguri_engine::derived_host::Invalidation;
use meguri_engine::{CancelToken, Readiness};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_stats::graphs::{History, Scope, Unreadable};
use meguri_stats::history::{
    self, CardStatesOverTime, HISTORY_COMPONENT, HistoryData, HistoryRequest, HistoryResult, Progress, StateCounts,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{Cell, ConfigValue, ReaderRole, RowKey, Table};

use crate::graphs_common::{CRAM, Fixture, LANG, NOTE, NOW_MS, VERBS, card, revlog, revlog_cells};

const WAIT: Duration = Duration::from_secs(120);
const DAY: i64 = 86_400;

fn request(scope: Scope, history: History) -> HistoryRequest {
    HistoryRequest {
        scope,
        history,
        cancel: CancelToken::new(),
    }
}

fn read(host: &meguri_testkit::bench::fixtures::CoreHost, scope: Scope, history: History) -> HistoryResult {
    let collection = host.collection().unwrap();
    history::card_states(host.stats(), &collection, host.derived(), &request(scope, history), &mut |_| {}).unwrap()
}

fn data(result: HistoryResult) -> Box<HistoryData> {
    match result {
        HistoryResult::Data(data) => data,
        other => panic!("not data: {other:?}"),
    }
}

fn warm(f: &Fixture, scope: Scope, history: History) -> Box<HistoryData> {
    data(read(&f.host, scope, history))
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

/// A row: `ease`, `ivl`, `factor` and kind as given.
fn row(id: i64, card: i64, ease: u8, ivl: i32, factor: u32, kind: RevlogKind) -> RevlogEntry {
    RevlogEntry {
        button: ease,
        interval: ivl,
        ease_factor: factor,
        kind,
        ..revlog(id, card)
    }
}

fn shaped(id: i64, deck: i64, card_type: CardType, queue: CardQueue, ivl: u32) -> Card {
    Card {
        card_type,
        queue,
        interval: ivl,
        ..card(id, deck)
    }
}

/// A whole second of day `day` (H1) as a review-log or card id, `k`
/// milliseconds in: an hour before the day ends.
fn at(n: i64, day: i64, k: i64) -> i64 {
    (n + day * DAY - 3_600) * 1_000 + k
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum S {
    New,
    Learning,
    Relearning,
    Young,
    Mature,
}

/// The one state a one-card series is in on `day`.
fn one_state(series: &CardStatesOverTime, day: i32) -> Option<S> {
    let counts = series.on(day);
    let states = [
        (counts.new, S::New),
        (counts.learning, S::Learning),
        (counts.relearning, S::Relearning),
        (counts.young, S::Young),
        (counts.mature, S::Mature),
    ];
    let all = [counts.new, counts.learning, counts.relearning, counts.young, counts.mature];
    assert!(all.iter().all(|c| *c == 0 || *c == 1) && counts.total() <= 1, "day {day}: {counts:?}");
    states.iter().find(|(count, _)| *count == 1).map(|(_, state)| *state)
}

/// One card per deck under `Solo`: its rows, and the state expected on each
/// day from −14 to 0 (by the first day of each run) and its first review.
struct Case {
    name: &'static str,
    card: Card,
    rows: Vec<RevlogEntry>,
    states: Vec<(i32, Option<S>)>,
    started: Option<i32>,
}

fn expected_on(case: &Case, day: i32) -> Option<S> {
    case.states.iter().rev().find(|(from, _)| *from <= day).and_then(|(_, state)| *state)
}

fn cases(n: i64, deck: impl Fn(&str) -> i64) -> Vec<Case> {
    use RevlogKind::{Filtered, Manual, Rescheduled, Review};
    use S::{Learning, Mature, New, Relearning, Young};
    const LEARN: RevlogKind = RevlogKind::Learning;
    const RELEARN: RevlogKind = RevlogKind::Relearning;
    let mut cases = Vec::new();
    let mut case = |name: &'static str, card: Card, rows: Vec<RevlogEntry>, states: Vec<(i32, Option<S>)>, started: Option<i32>| {
        let mut card = card;
        card.deck_id = DeckId(deck(name));
        cases.push(Case {
            name,
            card,
            rows,
            states,
            started,
        });
    };
    let id = at(n, -12, 1);
    case(
        "sequence",
        shaped(id, 0, CardType::Review, CardQueue::Review, 30),
        vec![
            row(at(n, -10, 10), id, 3, -600, 2500, LEARN),
            row(at(n, -9, 11), id, 3, 1, 2500, LEARN),
            row(at(n, -5, 12), id, 3, 25, 2500, Review),
            row(at(n, -3, 13), id, 1, -600, 2500, RELEARN),
            row(at(n, -1, 14), id, 3, 30, 2500, RELEARN),
        ],
        vec![(-14, None), (-12, Some(New)), (-10, Some(Learning)), (-9, Some(Young)), (-5, Some(Mature)), (-3, Some(Relearning)), (-1, Some(Mature))],
        Some(-10),
    );
    let id = at(n, -8, 2);
    case(
        "reset",
        shaped(id, 0, CardType::Learning, CardQueue::Learning, 0),
        vec![
            row(at(n, -7, 20), id, 3, 5, 2500, Review),
            row(at(n, -4, 21), id, 0, 0, 0, Manual),
            row(at(n, -2, 22), id, 3, -600, 2500, LEARN),
        ],
        vec![(-14, None), (-8, Some(New)), (-7, Some(Young)), (-4, Some(New)), (-2, Some(Learning))],
        // A Reset never starts a card again.
        Some(-7),
    );
    let id = at(n, -8, 3);
    case(
        "cram",
        shaped(id, 0, CardType::Review, CardQueue::Review, 25),
        vec![
            row(at(n, -6, 30), id, 3, 5, 2500, Review),
            row(at(n, -4, 31), id, 3, 30, 0, Filtered),
            row(at(n, -2, 32), id, 3, 25, 2500, Review),
        ],
        vec![(-14, None), (-8, Some(New)), (-6, Some(Young)), (-2, Some(Mature))],
        Some(-6),
    );
    let id = at(n, -8, 4);
    case(
        "review-ivl-0",
        shaped(id, 0, CardType::Review, CardQueue::Review, 10),
        vec![
            row(at(n, -6, 40), id, 3, 30, 2500, Review),
            row(at(n, -4, 41), id, 3, 0, 2500, Review),
            row(at(n, -2, 42), id, 3, 10, 2500, Review),
        ],
        vec![(-14, None), (-8, Some(New)), (-6, Some(Mature)), (-4, Some(Relearning)), (-2, Some(Young))],
        Some(-6),
    );
    case(
        "new-no-row",
        shaped(at(n, -5, 5), 0, CardType::New, CardQueue::New, 0),
        Vec::new(),
        vec![(-14, None), (-5, Some(New))],
        None,
    );
    case(
        "imported-no-row",
        shaped(at(n, -9, 6), 0, CardType::Review, CardQueue::Review, 40),
        Vec::new(),
        vec![(-14, None), (-9, Some(Mature))],
        None,
    );
    let id = at(n, -3, 7);
    case(
        "row-before-creation",
        shaped(id, 0, CardType::Review, CardQueue::Review, 3),
        vec![row(at(n, -7, 70), id, 3, 3, 2500, Review)],
        vec![(-14, None), (-7, Some(Young))],
        Some(-7),
    );
    let id = at(n, -8, 8);
    case(
        "two-rows-one-day",
        shaped(id, 0, CardType::Review, CardQueue::Review, 3),
        vec![
            row(at(n, -5, 80), id, 3, 25, 2500, Review),
            row(at(n, -5, 81), id, 1, -600, 2500, RELEARN),
            row(at(n, -2, 82), id, 3, 3, 2500, RELEARN),
        ],
        vec![(-14, None), (-8, Some(New)), (-5, Some(Relearning)), (-2, Some(Young))],
        Some(-5),
    );
    let id = at(n, -8, 9);
    case(
        "suspended",
        shaped(id, 0, CardType::Review, CardQueue::Suspended, 30),
        vec![row(at(n, -6, 90), id, 3, 30, 2500, Review)],
        vec![(-14, None)],
        // Started keeps suspended cards.
        Some(-6),
    );
    let id = at(n, -6, 10);
    case(
        "buried",
        shaped(id, 0, CardType::Review, CardQueue::BuriedByUser, 5),
        vec![row(at(n, -4, 100), id, 3, 5, 2500, Review)],
        vec![(-14, None), (-6, Some(New)), (-4, Some(Young))],
        Some(-4),
    );
    let id = at(n, -5, 11);
    case(
        "cram-only",
        shaped(id, 0, CardType::New, CardQueue::New, 0),
        vec![row(at(n, -3, 110), id, 3, 0, 0, Filtered)],
        vec![(-14, None), (-5, Some(New))],
        // A cram row never starts a card.
        None,
    );
    let id = at(n, -8, 12);
    case(
        "unrated-rows",
        shaped(id, 0, CardType::Review, CardQueue::Review, 5),
        vec![
            row(at(n, -6, 120), id, 3, 30, 2500, Review),
            row(at(n, -4, 121), id, 0, 0, 2500, Rescheduled),
            row(at(n, -3, 122), id, 0, 12, 2500, Manual),
            row(at(n, -2, 123), id, 3, 5, 2500, Review),
        ],
        // An unrated row with no interval changes nothing; a Set Due Date
        // row's interval gives the state.
        vec![(-14, None), (-8, Some(New)), (-6, Some(Mature)), (-3, Some(Young))],
        Some(-6),
    );
    let id = at(n, -2, 13);
    case(
        "row-after-n",
        shaped(id, 0, CardType::Review, CardQueue::Review, 5),
        vec![row((n + 3_600) * 1_000, id, 3, 5, 2500, Review)],
        // A row after `N` counts on day 0.
        vec![(-14, None), (-2, Some(New)), (0, Some(Young))],
        Some(0),
    );
    cases
}

#[test]
fn h2_table_and_sequences_give_the_expected_state_on_each_day() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let solo = add_deck(&f, "Solo");
    let names = [
        "sequence",
        "reset",
        "cram",
        "review-ivl-0",
        "new-no-row",
        "imported-no-row",
        "row-before-creation",
        "two-rows-one-day",
        "suspended",
        "buried",
        "cram-only",
        "unrated-rows",
        "row-after-n",
        "deleted",
    ];
    let decks: HashMap<&str, i64> = names.iter().map(|name| (*name, add_deck(&f, &format!("Solo\u{1f}{name}")))).collect();
    let cases = cases(n, |name| decks[name]);
    for case in &cases {
        let (card, rows) = (case.card.clone(), case.rows.clone());
        f.write(move |scope| {
            scope.put(&card)?;
            for row in &rows {
                scope.insert_revlog(row)?;
            }
            Ok(())
        });
    }
    // A deleted card: its rows stay and count nowhere.
    let deleted = shaped(at(n, -6, 14), decks["deleted"], CardType::Review, CardQueue::Review, 30);
    f.write(move |scope| {
        scope.put(&deleted)?;
        scope.insert_revlog(&row(at(n, -4, 140), deleted.id.0, 3, 30, 2500, RevlogKind::Review))?;
        Ok(())
    });
    f.write(move |scope| {
        scope.remove_card(CardId(at(n, -6, 14)))?;
        Ok(())
    });
    for history in [History::AllHistory, History::Last12Months] {
        let mut sum: BTreeMap<i32, StateCounts> = BTreeMap::new();
        for case in &cases {
            let got = warm(&f, Scope::Deck(DeckId(decks[case.name])), history);
            assert!(got.from_component, "{}", case.name);
            for day in -14..=0 {
                assert_eq!(one_state(&got.card_states, day), expected_on(case, day), "{} day {day}", case.name);
                let counts = got.card_states.on(day);
                let total = sum.entry(day).or_default();
                *total = total.plus(counts);
            }
            assert_eq!(got.card_states.left_out, u64::from(case.name == "suspended"), "{}", case.name);
            let started: Vec<(i32, u64)> = got.started.days.iter().map(|(d, c)| (*d, *c)).collect();
            assert_eq!(started, case.started.map(|day| (day, 1)).into_iter().collect::<Vec<_>>(), "{}", case.name);
            assert_eq!(got.started.first_day, case.started, "{}", case.name);
            let entered = case.states.iter().find(|(_, state)| state.is_some()).map(|(day, _)| *day);
            assert_eq!(got.card_states.first_day, entered, "{}", case.name);
        }
        // The parent deck's series is the sum of its subdecks' (H14.1).
        let parent = warm(&f, Scope::Deck(DeckId(solo)), history);
        for day in -14..=0 {
            assert_eq!(parent.card_states.on(day), sum[&day], "Solo day {day}");
        }
        assert_eq!(parent.card_states.left_out, 1);
        assert_eq!(parent.started.days.values().sum::<u64>(), cases.iter().filter(|c| c.started.is_some()).count() as u64);
    }
}

#[test]
fn h2_4_start_holds_the_counts_before_the_window_and_first_day_is_all_history() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let deck = add_deck(&f, "Old");
    let id = at(n, -400, 1);
    let old = shaped(id, deck, CardType::Review, CardQueue::Review, 30);
    f.write(move |scope| {
        scope.put(&old)?;
        scope.insert_revlog(&row(at(n, -380, 1), id, 3, 5, 2500, RevlogKind::Review))?;
        scope.insert_revlog(&row(at(n, -10, 2), id, 3, 30, 2500, RevlogKind::Review))?;
        Ok(())
    });
    let all = warm(&f, Scope::Deck(DeckId(deck)), History::AllHistory);
    let year = warm(&f, Scope::Deck(DeckId(deck)), History::Last12Months);
    assert_eq!(all.card_states.start, StateCounts::default());
    assert_eq!(all.card_states.deltas.len(), 3);
    assert_eq!(all.card_states.first_day, Some(-400));
    assert_eq!(year.card_states.first_day, Some(-400));
    // The end of day −367: young since day −380.
    assert_eq!(year.card_states.start, StateCounts { young: 1, ..StateCounts::default() });
    assert_eq!(year.card_states.start, all.card_states.on(-367));
    assert_eq!(year.card_states.deltas.keys().copied().collect::<Vec<_>>(), vec![-10]);
    for day in -366..=0 {
        assert_eq!(year.card_states.on(day), all.card_states.on(day), "day {day}");
    }
    assert!(year.started.days.is_empty());
    assert_eq!(year.started.first_day, Some(-380));
    assert_eq!(all.started.days.iter().collect::<Vec<_>>(), vec![(&-380, &1)]);
}

/// A seeded random collection on top of the fixture: cards in every deck,
/// filtered ones among them, some suspended or buried, some imported
/// without a log, with rows of every kind on random days.
fn random_collection(f: &Fixture, seed: u64, cards: usize) -> i64 {
    let random = SeededRandom::new(seed);
    let pick = |n: u64| random.next_u64() % n;
    let n = f.next_day_at();
    let solo = add_deck(f, "Solo");
    let decks = [LANG, VERBS, solo];
    let mut all = Vec::new();
    for at_card in 0..cards {
        let created = -(pick(500) as i64);
        let id = at(n, created, at_card as i64);
        let home = decks[pick(3) as usize];
        let card_type = [CardType::New, CardType::Learning, CardType::Review, CardType::Relearning][pick(4) as usize];
        let queue = match pick(10) {
            0 => CardQueue::Suspended,
            1 => CardQueue::BuriedByUser,
            2 => CardQueue::BuriedByScheduler,
            _ => CardQueue::Review,
        };
        let mut c = shaped(id, home, card_type, queue, pick(60) as u32);
        if pick(6) == 0 {
            c.deck_id = DeckId(CRAM);
            c.original_deck_id = DeckId(home);
        }
        let mut rows = Vec::new();
        for k in 0..pick(9) {
            // Some rows before the card's creation, a few after `N`.
            let day = (created + pick(40) as i64 - 10).min(0);
            let id = if pick(30) == 0 { (n + 600 + k as i64) * 1_000 + at_card as i64 } else { at(n, day, 1_000 * k as i64 + at_card as i64) };
            let (ease, ivl, factor, kind) = match pick(8) {
                0 => (0, 0, 0, RevlogKind::Manual),
                1 => (3, pick(40) as i32, 0, RevlogKind::Filtered),
                2 => (0, pick(30) as i32 + 1, 2500, RevlogKind::Manual),
                3 => (1, -600, 2500, RevlogKind::Relearning),
                4 => (3, -600, 2500, RevlogKind::Learning),
                5 => (0, 0, 2500, RevlogKind::Rescheduled),
                _ => ((pick(4) + 1) as u8, pick(50) as i32 - 5, 2500, RevlogKind::Review),
            };
            rows.push(row(id, c.id.0, ease, ivl, factor, kind));
        }
        all.push((c, rows));
    }
    f.write(move |scope| {
        for (c, rows) in &all {
            scope.put(c)?;
            for r in rows {
                scope.insert_revlog(r)?;
            }
        }
        Ok(())
    });
    solo
}

/// A card as stored: `(id, did, odid, type, queue, ivl)`.
type Stored = (i64, i64, i64, i64, i64, i64);
/// A row as stored: `(id, ease, ivl, factor, type)`.
type StoredRow = (i64, i64, i64, i64, i64);

fn integer(value: &SqlValue) -> i64 {
    match value {
        SqlValue::Integer(v) => *v,
        other => panic!("{other:?}"),
    }
}

fn stored(f: &Fixture) -> (Vec<Stored>, HashMap<i64, Vec<StoredRow>>) {
    let collection = f.collection();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let cards = snapshot
        .execute_raw("SELECT id, did, odid, type, queue, ivl FROM cards", &[])
        .unwrap()
        .rows
        .iter()
        .map(|r| (integer(&r[0]), integer(&r[1]), integer(&r[2]), integer(&r[3]), integer(&r[4]), integer(&r[5])))
        .collect();
    let mut rows: HashMap<i64, Vec<StoredRow>> = HashMap::new();
    for r in snapshot
        .execute_raw("SELECT cid, id, ease, ivl, factor, type FROM revlog WHERE id >= 0 ORDER BY id", &[])
        .unwrap()
        .rows
    {
        rows.entry(integer(&r[0]))
            .or_default()
            .push((integer(&r[1]), integer(&r[2]), integer(&r[3]), integer(&r[4]), integer(&r[5])));
    }
    (cards, rows)
}

/// C4.1's type bucket.
fn bucket(card_type: i64, ivl: i64) -> S {
    match card_type {
        0 => S::New,
        1 => S::Learning,
        3 => S::Relearning,
        _ if ivl < 21 => S::Young,
        _ => S::Mature,
    }
}

/// B3's day of a row, folded at 0.
fn row_day(id: i64, n: i64) -> i64 {
    let t = id.div_euclid(1000);
    ((t - n) / DAY).min(0)
}

/// C10's creation day, folded at 0.
fn card_day(id: i64, n: i64) -> i64 {
    let c = id / 1000;
    (-((n - c).div_euclid(DAY))).min(0)
}

/// A direct evaluation of H2.2 for one card on one day, written apart
/// from the core's segments.
fn state_on(card: &Stored, rows: &[StoredRow], n: i64, day: i64) -> Option<S> {
    let (id, _, _, card_type, _, ivl) = *card;
    let current = bucket(card_type, ivl);
    let created = card_day(id, n);
    let Some(last) = rows.last() else {
        return (day >= created).then_some(current);
    };
    let entered = created.min(row_day(rows[0].0, n));
    if day < entered {
        return None;
    }
    if day >= row_day(last.0, n) {
        return Some(current);
    }
    let mut state = S::New;
    for (row_id, ease, row_ivl, factor, kind) in rows {
        if row_day(*row_id, n) > day {
            break;
        }
        let kind = kind & 0xff;
        state = if kind == 4 && *factor == 0 {
            S::New
        } else if kind == 3 && *factor == 0 {
            state
        } else if *row_ivl >= 1 {
            if *row_ivl < 21 { S::Young } else { S::Mature }
        } else if *ease >= 1 {
            if matches!(state, S::New | S::Learning) { S::Learning } else { S::Relearning }
        } else {
            state
        };
    }
    Some(state)
}

fn counts_of(states: impl Iterator<Item = S>) -> StateCounts {
    let mut counts = StateCounts::default();
    for state in states {
        match state {
            S::New => counts.new += 1,
            S::Learning => counts.learning += 1,
            S::Relearning => counts.relearning += 1,
            S::Young => counts.young += 1,
            S::Mature => counts.mature += 1,
        }
    }
    counts
}

/// The scopes the random tests read, each with its decks (G2.1).
fn scopes(solo: i64) -> Vec<(Scope, Option<Vec<i64>>)> {
    vec![
        (Scope::Collection, None),
        (Scope::Deck(DeckId(LANG)), Some(vec![LANG, VERBS])),
        (Scope::Deck(DeckId(VERBS)), Some(vec![VERBS])),
        (Scope::Deck(DeckId(CRAM)), Some(vec![CRAM])),
        (Scope::Deck(DeckId(solo)), Some(vec![solo])),
    ]
}

fn in_scope(card: &Stored, decks: &Option<Vec<i64>>) -> bool {
    decks
        .as_ref()
        .is_none_or(|decks| decks.contains(&card.1) || (card.2 != 0 && decks.contains(&card.2)))
}

#[test]
fn h2_the_last_day_equals_the_cards_by_type_bucket_without_the_suspended() {
    let f = Fixture::new();
    let solo = random_collection(&f, 7, 400);
    let (cards, _) = stored(&f);
    for (scope, decks) in scopes(solo) {
        let scoped: Vec<&Stored> = cards.iter().filter(|c| in_scope(c, &decks)).collect();
        let expected = counts_of(scoped.iter().filter(|c| c.4 != -1).map(|c| bucket(c.3, c.5)));
        for history in [History::AllHistory, History::Last12Months] {
            let got = warm(&f, scope.clone(), history);
            assert_eq!(got.card_states.on(0), expected, "{scope:?} {history:?}");
            assert_eq!(got.card_states.left_out, scoped.iter().filter(|c| c.4 == -1).count() as u64, "{scope:?}");
        }
    }
}

#[test]
fn h2_deltas_from_start_give_a_direct_count_of_every_day() {
    let f = Fixture::new();
    let solo = random_collection(&f, 11, 300);
    let n = f.next_day_at();
    let (cards, rows) = stored(&f);
    let none = Vec::new();
    for (scope, decks) in scopes(solo) {
        let all = warm(&f, scope.clone(), History::AllHistory);
        let year = warm(&f, scope.clone(), History::Last12Months);
        let scoped: Vec<&Stored> = cards.iter().filter(|c| c.4 != -1 && in_scope(c, &decks)).collect();
        let direct = |day: i64| {
            counts_of(
                scoped
                    .iter()
                    .filter_map(|c| state_on(c, rows.get(&c.0).unwrap_or(&none), n, day)),
            )
        };
        for day in -560..=0 {
            assert_eq!(all.card_states.on(day), direct(i64::from(day)), "{scope:?} all, day {day}");
        }
        assert_eq!(year.card_states.start, direct(-367), "{scope:?}");
        for day in -366..=0 {
            assert_eq!(year.card_states.on(day), direct(i64::from(day)), "{scope:?} year, day {day}");
        }
        // The fixture's own cards are older than the random ones.
        let oldest = scoped
            .iter()
            .map(|c| {
                let first_row = rows.get(&c.0).and_then(|rows| rows.first()).map(|row| row_day(row.0, n));
                first_row.map_or(card_day(c.0, n), |day| day.min(card_day(c.0, n)))
            })
            .min();
        if let Some(oldest) = oldest {
            assert_eq!(all.card_states.on(i32::try_from(oldest - 1).unwrap()), StateCounts::default(), "{scope:?}");
            assert_eq!(all.card_states.on(i32::try_from(oldest).unwrap()), direct(oldest), "{scope:?}");
        }
        assert_eq!(all.card_states.first_day.map(i64::from), oldest, "{scope:?}");
        // Every delta day holds a change.
        assert!(all.card_states.deltas.values().all(|d| *d != StateCounts::default()));
    }
}

#[test]
fn h2_3_started_counts_each_cards_first_graded_row() {
    let f = Fixture::new();
    let solo = random_collection(&f, 13, 300);
    let n = f.next_day_at();
    let (cards, rows) = stored(&f);
    for (scope, decks) in scopes(solo) {
        let mut expected: BTreeMap<i32, u64> = BTreeMap::new();
        for c in cards.iter().filter(|c| in_scope(c, &decks)) {
            let first = rows
                .get(&c.0)
                .into_iter()
                .flatten()
                .find(|(_, ease, _, factor, kind)| *ease >= 1 && !(kind & 0xff == 3 && *factor == 0));
            if let Some((id, ..)) = first {
                *expected.entry(row_day(*id, n) as i32).or_default() += 1;
            }
        }
        let all = warm(&f, scope.clone(), History::AllHistory);
        assert_eq!(all.started.days, expected, "{scope:?}");
        assert_eq!(all.started.first_day, expected.keys().next().copied(), "{scope:?}");
        let year = warm(&f, scope.clone(), History::Last12Months);
        let window: BTreeMap<i32, u64> = expected.iter().filter(|(d, _)| **d >= -366).map(|(d, c)| (*d, *c)).collect();
        assert_eq!(year.started.days, window, "{scope:?}");
        assert_eq!(year.started.first_day, all.started.first_day);
    }
}

#[test]
fn h2_1_burying_and_the_rollovers_unbury_change_no_past_day() {
    let f = Fixture::new();
    let solo = random_collection(&f, 17, 200);
    let before: Vec<Box<HistoryData>> = scopes(solo).into_iter().map(|(s, _)| warm(&f, s, History::AllHistory)).collect();
    let (cards, _) = stored(&f);
    let review: Vec<i64> = cards.iter().filter(|c| c.4 == 2).map(|c| c.0).take(40).collect();
    f.write(move |scope| {
        for id in &review {
            let mut c = meguri_storage::ReadRows::card(&*scope, CardId(*id))?.unwrap();
            c.queue = CardQueue::BuriedByUser;
            scope.put(&c)?;
        }
        Ok(())
    });
    let buried: Vec<Box<HistoryData>> = scopes(solo).into_iter().map(|(s, _)| warm(&f, s, History::AllHistory)).collect();
    for (a, b) in before.iter().zip(&buried) {
        assert_eq!(a.card_states, b.card_states);
        assert_eq!(a.started, b.started);
    }
    // The next day: the day clock unburies, and every day moves back one.
    f.clock.step_ms(DAY * 1_000);
    let next: Vec<Box<HistoryData>> = scopes(solo).into_iter().map(|(s, _)| warm(&f, s, History::AllHistory)).collect();
    for (a, b) in buried.iter().zip(&next) {
        assert!(b.from_component);
        assert_eq!(b.context.next_day_at, a.context.next_day_at + DAY);
        // Day 0 moved to −1 holds no row after the old `N`.
        for day in -560..=-1 {
            assert_eq!(b.card_states.on(day - 1), a.card_states.on(day), "day {day}");
        }
        assert_eq!(b.card_states.on(0), a.card_states.on(0));
    }
}

/// H14.4: the component's parts equal a cold replay of the same snapshot
/// and context, for every scope and history.
fn compare(f: &Fixture, solo: i64, step: &str) {
    compare_from(f, solo, step, true);
}

fn compare_from(f: &Fixture, solo: i64, step: &str, kept: bool) {
    let collection = f.collection();
    let derived = f.host.derived();
    for (scope, _) in scopes(solo) {
        for history in [History::Last12Months, History::AllHistory] {
            let request = request(scope.clone(), history);
            let context = history::page_context(&collection, derived).unwrap();
            let warm = history::card_states_in(f.host.stats(), &collection, derived, &context, &request, &mut |_| {}).unwrap();
            if let HistoryResult::Data(data) = &warm {
                assert_eq!(data.from_component, kept, "{step}: {scope:?} {history:?}");
            }
            let cold = history::card_states_cold(&collection, &context, &request).unwrap();
            assert!(matches!(cold, HistoryResult::Data(_) | HistoryResult::UnreadableRow(_)), "{step}: {cold:?}");
            match (warm, cold) {
                (HistoryResult::Data(w), HistoryResult::Data(c)) => {
                    assert_eq!(w.card_states, c.card_states, "{step}: {scope:?} {history:?}");
                    assert_eq!(w.started, c.started, "{step}: {scope:?} {history:?}");
                }
                (w, c) => assert_eq!(w, c, "{step}: {scope:?} {history:?}"),
            }
        }
    }
}

/// One random change of H20's mix.
struct Mix {
    random: SeededRandom,
    next_id: i64,
    cards: Vec<i64>,
    removed: Vec<i64>,
    bad_rows: Vec<i64>,
    decks: Vec<i64>,
    offset_hours: i32,
}

impl Mix {
    fn pick(&self, n: usize) -> usize {
        (self.random.next_u64() % n as u64) as usize
    }

    fn some_card(&self) -> Option<i64> {
        (!self.cards.is_empty()).then(|| self.cards[self.pick(self.cards.len())])
    }

    fn row_id(&mut self, f: &Fixture, days_back: i64) -> i64 {
        self.next_id += 1;
        at(f.next_day_at(), -days_back, 500 + self.next_id % 400)
    }

    fn step(&mut self, f: &mut Fixture) -> &'static str {
        match self.pick(25) {
            0 | 1 => {
                // An answer: one row today, the card's new interval.
                let Some(id) = self.some_card() else { return "none" };
                let row_id = self.row_id(f, 0);
                let ivl = self.pick(60) as u32;
                let ease = (self.pick(4) + 1) as u8;
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    c.card_type = if ease == 1 { CardType::Relearning } else { CardType::Review };
                    c.interval = ivl;
                    scope.put(&c)?;
                    scope.insert_revlog(&row(row_id, id, ease, if ease == 1 { -600 } else { ivl as i32 }, 2500, RevlogKind::Review))?;
                    Ok(())
                });
                "answer"
            }
            2 => {
                // Its undo: the newest row goes, the card returns.
                let Some(id) = self.some_card() else { return "none" };
                f.write(move |scope| {
                    let rows = meguri_storage::ReadRows::revlog_ids_of_card(&*scope, CardId(id))?;
                    if let Some(last) = rows.last() {
                        scope.remove_revlog(RevlogId(*last))?;
                    }
                    let mut c = current(scope, id);
                    c.interval = c.interval.saturating_sub(3);
                    scope.put(&c)?;
                    Ok(())
                });
                "undo"
            }
            3 => {
                let Some(id) = self.some_card() else { return "none" };
                let row_id = self.row_id(f, 0);
                let days = self.pick(40) as i32 + 1;
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    c.card_type = CardType::Review;
                    c.interval = days as u32;
                    scope.put(&c)?;
                    scope.insert_revlog(&row(row_id, id, 0, days, 2500, RevlogKind::Manual))?;
                    Ok(())
                });
                "set due date"
            }
            4 => {
                let Some(id) = self.some_card() else { return "none" };
                let row_id = self.row_id(f, 0);
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    c.card_type = CardType::New;
                    c.queue = CardQueue::New;
                    c.interval = 0;
                    scope.put(&c)?;
                    scope.insert_revlog(&row(row_id, id, 0, 0, 0, RevlogKind::Manual))?;
                    Ok(())
                });
                "reset"
            }
            5 => {
                let Some(id) = self.some_card() else { return "none" };
                let queue = [CardQueue::Suspended, CardQueue::Review, CardQueue::BuriedByUser][self.pick(3)];
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    c.queue = queue;
                    scope.put(&c)?;
                    Ok(())
                });
                "suspend, unsuspend or bury"
            }
            6 => {
                let Some(id) = self.some_card() else { return "none" };
                let deck = self.decks[self.pick(self.decks.len())];
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    c.deck_id = DeckId(deck);
                    c.original_deck_id = DeckId(0);
                    scope.put(&c)?;
                    Ok(())
                });
                "deck move"
            }
            7 => {
                let Some(id) = self.some_card() else { return "none" };
                f.write(move |scope| {
                    let mut c = current(scope, id);
                    if c.original_deck_id.0 == 0 {
                        c.original_deck_id = c.deck_id;
                        c.deck_id = DeckId(CRAM);
                    } else {
                        c.deck_id = c.original_deck_id;
                        c.original_deck_id = DeckId(0);
                    }
                    scope.put(&c)?;
                    Ok(())
                });
                "filtered build or empty"
            }
            8 => {
                let Some(id) = self.some_card() else { return "none" };
                f.write(move |scope| {
                    scope.remove_card(CardId(id))?;
                    Ok(())
                });
                self.cards.retain(|c| *c != id);
                self.removed.push(id);
                "delete"
            }
            9 => {
                let Some(id) = self.removed.pop() else { return "none" };
                let deck = self.decks[self.pick(self.decks.len())];
                f.write(move |scope| {
                    scope.put(&card(id, deck))?;
                    Ok(())
                });
                self.cards.push(id);
                "undo of a delete"
            }
            10 => {
                // An import: a card with its schedule and no log.
                self.next_id += 1;
                let id = at(f.next_day_at(), -(self.pick(300) as i64), 500 + self.next_id % 400);
                let deck = self.decks[self.pick(self.decks.len())];
                let ivl = self.pick(90) as u32;
                f.write(move |scope| {
                    scope.put(&shaped(id, deck, CardType::Review, CardQueue::Review, ivl))?;
                    Ok(())
                });
                self.cards.push(id);
                "import"
            }
            11 => {
                // A sync bringing an older row.
                let Some(id) = self.some_card() else { return "none" };
                let back = self.pick(400) as i64;
                let row_id = self.row_id(f, back);
                let ivl = self.pick(40) as i32 - 2;
                f.write_raw(move |scope| {
                    scope.insert_revlog(&row(row_id, id, 3, ivl, 2500, RevlogKind::Review))?;
                    Ok(())
                });
                "sync with an older row"
            }
            12 => {
                f.clock.step_ms(DAY * 1_000);
                "rollover"
            }
            13 => {
                // A sync after a rollover, bringing the previous day's row.
                f.clock.step_ms(DAY * 1_000);
                let Some(id) = self.some_card() else { return "none" };
                let row_id = self.row_id(f, 1);
                f.write_raw(move |scope| {
                    scope.insert_revlog(&row(row_id, id, 3, 12, 2500, RevlogKind::Review))?;
                    Ok(())
                });
                "sync after a rollover"
            }
            14 => {
                f.host.close().unwrap();
                f.clock.step_ms(3 * DAY * 1_000);
                f.reopen_closed();
                "several rollovers while closed"
            }
            15 => {
                // A re-key: the card under a new id, its rows' `cid`
                // rewritten in place.
                let Some(old) = self.some_card() else { return "none" };
                self.next_id += 1;
                let new = old + 1_000_000 + self.next_id;
                f.write_raw(move |scope| {
                    let mut c = current(scope, old);
                    let rows = meguri_storage::ReadRows::revlog_ids_of_card(&*scope, CardId(old))?;
                    scope.remove_card(CardId(old))?;
                    c.id = CardId(new);
                    scope.put(&c)?;
                    for id in rows {
                        let mut image = meguri_storage::ReadRows::row_image(&*scope, Table::Revlog, &RowKey::Id(id))?.unwrap();
                        image[1] = Cell::Integer(new);
                        scope.restore(Table::Revlog, &RowKey::Id(id), Some(&image))?;
                    }
                    Ok(())
                });
                self.cards.retain(|c| *c != old);
                self.cards.push(new);
                "re-key"
            }
            16 => {
                let hour = self.pick(24) as i64;
                f.write(move |scope| {
                    scope.set_config("rollover", &ConfigValue::Integer(i128::from(hour)))?;
                    Ok(())
                });
                "rollover hour change"
            }
            17 => {
                self.offset_hours = (self.offset_hours + 1 + self.pick(5) as i32) % 12;
                f.clock.set_utc_offset(UtcOffset::from_seconds(self.offset_hours * 3_600).unwrap());
                "time zone change"
            }
            18 => {
                meguri_stats::graphs::release_stats_caches(f.host.stats(), f.host.derived());
                "memory warning"
            }
            19 => {
                f.reopen();
                "close and reopen"
            }
            20 => {
                if let Some(id) = self.bad_rows.pop() {
                    f.write(move |scope| {
                        scope.remove_revlog(RevlogId(id))?;
                        Ok(())
                    });
                    return "unreadable row removed";
                }
                let Some(cid) = self.some_card() else { return "none" };
                let back = self.pick(500) as i64;
                let id = self.row_id(f, back);
                f.write_raw(move |scope| {
                    let cells = revlog_cells(id, cid, Cell::Integer(3), Cell::Integer(1), Cell::Integer(-1), Cell::Integer(1), Cell::Integer(1));
                    scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
                    Ok(())
                });
                self.bad_rows.push(id);
                "unreadable row"
            }
            21 => {
                // A rebuild with commits landing while it runs: they are
                // kept and applied after the swap (D7).
                f.host.derived().invalidate(Invalidation::CheckDatabase);
                for _ in 0..3 {
                    let Some(id) = self.some_card() else { return "none" };
                    let row_id = self.row_id(f, 0);
                    f.write(move |scope| {
                        let mut c = current(scope, id);
                        c.card_type = CardType::Review;
                        c.interval = 25;
                        scope.put(&c)?;
                        scope.insert_revlog(&row(row_id, id, 3, 25, 2500, RevlogKind::Review))?;
                        Ok(())
                    });
                }
                "answers during a rebuild"
            }
            22 => {
                // A row with a negative id, which no part reads (G3).
                let Some(id) = self.some_card() else { return "none" };
                self.next_id += 1;
                let row_id = -1_000 - self.next_id;
                f.write_raw(move |scope| {
                    scope.insert_revlog(&row(row_id, id, 3, 40, 2500, RevlogKind::Review))?;
                    Ok(())
                });
                "negative-id row"
            }
            23 => {
                // A sync bringing rows before their card, in an earlier
                // commit.
                self.next_id += 1;
                let id = at(f.next_day_at(), -(self.pick(200) as i64), 500 + self.next_id % 400);
                let (first, second) = (self.row_id(f, 30), self.row_id(f, 5));
                f.write_raw(move |scope| {
                    scope.insert_revlog(&row(first, id, 3, 4, 2500, RevlogKind::Review))?;
                    scope.insert_revlog(&row(second, id, 3, 30, 2500, RevlogKind::Review))?;
                    Ok(())
                });
                let deck = self.decks[self.pick(self.decks.len())];
                f.write_raw(move |scope| {
                    scope.put(&shaped(id, deck, CardType::Review, CardQueue::Review, 30))?;
                    Ok(())
                });
                self.cards.push(id);
                "rows before their card"
            }
            _ => {
                f.write(|scope| {
                    scope.set_config("firstDayOfWeek", &ConfigValue::Integer(1))?;
                    Ok(())
                });
                "a change the component ignores"
            }
        }
    }
}

fn current(scope: &mut meguri_storage::WriteScope<'_>, id: i64) -> Card {
    meguri_storage::ReadRows::card(&*scope, CardId(id))
        .unwrap()
        .unwrap_or_else(|| card(id, LANG))
}

fn mix_run(seed: u64, steps: usize) {
    let mut f = Fixture::new();
    let solo = random_collection(&f, seed, 150);
    let (cards, _) = stored(&f);
    let mut mix = Mix {
        random: SeededRandom::new(seed ^ 0x5eed),
        next_id: 0,
        cards: cards.iter().map(|c| c.0).collect(),
        removed: Vec::new(),
        bad_rows: Vec::new(),
        decks: vec![LANG, VERBS, solo],
        offset_hours: 0,
    };
    compare(&f, solo, "built");
    let mut events = f.host.engine().stream().subscribe();
    for at_step in 0..steps {
        let name = mix.step(&mut f);
        let step = format!("step {at_step} ({name})");
        compare(&f, solo, &step);
        // Every other change is followed commit by commit: the component
        // never went stale for it (no "rebuild" capture).
        if REBUILDS.contains(&name) {
            events = f.host.engine().stream().subscribe();
        } else {
            assert_eq!(stale_events(&events), 0, "{step} rebuilt the component");
        }
    }
}

/// The steps after which the component is rebuilt or reopened.
const REBUILDS: [&str; 5] = [
    "rollover hour change",
    "time zone change",
    "several rollovers while closed",
    "close and reopen",
    "answers during a rebuild",
];

/// The `Stale` readiness events of the component received so far.
fn stale_events(events: &meguri_engine::Subscription) -> usize {
    std::iter::from_fn(|| events.try_recv())
        .filter(|event| match &event.kind {
            meguri_engine::EventKind::Extension(payload) => payload
                .as_any()
                .downcast_ref::<meguri_engine::DerivedReadiness>()
                .is_some_and(|r| r.component == HISTORY_COMPONENT && r.readiness == Readiness::Stale),
            _ => false,
        })
        .count()
}

#[test]
fn h1_rows_and_cards_on_the_day_boundaries_fall_on_b3_and_c10_days() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let deck = add_deck(&f, "Edges");
    let end = n - DAY;
    // C10: a card created on the last second of day −1 is on day −1, one a
    // second later on day 0; ids −1 and −1500 truncate to seconds 0 and −1.
    let cards = [
        (end * 1_000, -1),
        (end * 1_000 + 999, -1),
        (end * 1_000 + 1_000, 0),
        (-1, -(n.div_euclid(DAY)) as i32),
        (-1_500, -((n + 1).div_euclid(DAY)) as i32),
    ];
    for (id, _) in cards {
        f.write(move |scope| {
            scope.put(&shaped(id, deck, CardType::New, CardQueue::New, 0))?;
            Ok(())
        });
    }
    let got = warm(&f, Scope::Deck(DeckId(deck)), History::AllHistory);
    let mut expected: BTreeMap<i32, StateCounts> = BTreeMap::new();
    for (_, day) in cards {
        expected.entry(day).or_default().new += 1;
    }
    assert_eq!(got.card_states.deltas, expected);
    // B3: a row on the last second of day −2 is on day −2 (its id's
    // milliseconds dropped), the next second on day −1.
    let other = add_deck(&f, "Edges2");
    let id = at(n, -9, 0);
    let rows = [((end - DAY) * 1_000 + 999, -2), ((end - DAY + 1) * 1_000, -1)];
    f.write(move |scope| {
        scope.put(&shaped(id, other, CardType::Review, CardQueue::Review, 5))?;
        for (row_id, _) in rows {
            scope.insert_revlog(&row(row_id, id, 3, 5, 2500, RevlogKind::Review))?;
        }
        Ok(())
    });
    let got = warm(&f, Scope::Deck(DeckId(other)), History::AllHistory);
    // New from −9, young from the first row's day; its first review then.
    assert_eq!(one_state(&got.card_states, -3), Some(S::New));
    assert_eq!(one_state(&got.card_states, -2), Some(S::Young));
    assert_eq!(got.started.days.keys().copied().collect::<Vec<_>>(), vec![-2]);
}

#[test]
fn h14_4_the_kept_series_equals_a_cold_replay_after_random_changes() {
    for seed in [1, 2, 3] {
        mix_run(seed, 70);
    }
}

#[test]
fn h14_2_the_component_waits_for_the_first_request_that_needs_it() {
    let f = Fixture::new();
    assert!(f.host.derived().wait_idle(WAIT));
    let readiness = f.host.derived().readiness(HISTORY_COMPONENT).expect("registered").readiness;
    assert_eq!(readiness, Readiness::Stale, "held at bind");
    let first = warm(&f, Scope::Collection, History::AllHistory);
    assert!(first.from_component);
    assert_eq!(
        f.host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness,
        Readiness::Ready
    );
    // A clean close keeps it: the next open reads it without a build.
    let mut f = f;
    f.reopen();
    assert!(f.host.derived().wait_idle(WAIT));
    assert_eq!(f.host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness, Readiness::Ready);
    let again = warm(&f, Scope::Collection, History::AllHistory);
    assert!(again.from_component);
    assert_eq!(again.card_states, first.card_states);
    // The kept records are one per readable existing card.
    let collection = f.collection();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    f.host.derived().flush().unwrap();
    let snapshot = lease.snapshot().unwrap();
    let kept = history::kept_cards(f.host.derived(), &snapshot).unwrap().expect("current");
    let count = integer(&snapshot.execute_raw("SELECT count(*) FROM cards", &[]).unwrap().rows[0][0]);
    assert_eq!(kept.len() as i64, count);
}

#[test]
fn h14_3_a_phase_change_rebuilds_on_the_next_request() {
    let f = Fixture::new();
    let solo = random_collection(&f, 23, 120);
    compare(&f, solo, "built");
    f.write(|scope| {
        scope.set_config("rollover", &ConfigValue::Integer(9))?;
        Ok(())
    });
    compare(&f, solo, "another rollover hour");
    f.clock.set_utc_offset(UtcOffset::from_seconds(5 * 3_600 + 1_800).unwrap());
    compare(&f, solo, "another offset");
}

#[test]
fn h15_an_unreadable_row_outside_the_window_fails_only_its_scopes() {
    let f = Fixture::new();
    let n = f.next_day_at();
    let id = at(n, -500, 1);
    let lang = shaped(at(n, -600, 1), LANG, CardType::Review, CardQueue::Review, 30);
    let cid = lang.id.0;
    f.write(move |scope| {
        scope.put(&lang)?;
        Ok(())
    });
    f.write_raw(move |scope| {
        let cells = revlog_cells(id, cid, Cell::Integer(3), Cell::Integer(1), Cell::Integer(-1), Cell::Integer(1), Cell::Integer(1));
        scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
        Ok(())
    });
    let bad = Unreadable {
        table: "revlog",
        column: "factor",
        key: id,
    };
    for history in [History::Last12Months, History::AllHistory] {
        for scope in [Scope::Collection, Scope::Deck(DeckId(LANG))] {
            assert_eq!(read(&f.host, scope, history), HistoryResult::UnreadableRow(bad), "{history:?}");
        }
        assert!(matches!(read(&f.host, Scope::Deck(DeckId(VERBS)), history), HistoryResult::Data(_)));
    }
    assert_eq!(read(&f.host, Scope::Deck(DeckId(424_242)), History::AllHistory), HistoryResult::UnknownDeck);
    assert_eq!(read(&f.host, Scope::Search("deck:Lang".into()), History::AllHistory), HistoryResult::UnsupportedScope);
}

/// Every table of the collection, row by row.
fn dump(host: &meguri_testkit::bench::fixtures::CoreHost) -> Vec<Vec<Vec<SqlValue>>> {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    ["col", "notes", "cards", "revlog", "decks", "deck_config", "config", "tags", "graves", "notetypes"]
        .iter()
        .map(|table| snapshot.execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[]).unwrap().rows)
        .collect()
}

#[test]
fn h17_builds_and_reads_write_nothing_to_the_collection() {
    let f = Fixture::new();
    let solo = random_collection(&f, 29, 100);
    f.next_day_at();
    let before = dump(&f.host);
    for (scope, _) in scopes(solo) {
        warm(&f, scope, History::AllHistory);
    }
    f.host.derived().invalidate(Invalidation::CheckDatabase);
    for (scope, _) in scopes(solo) {
        warm(&f, scope, History::Last12Months);
    }
    assert_eq!(dump(&f.host), before);
}

/// A collection of `cards` cards in `Lang` with `rows` review rows spread
/// over them, written before the host opens it.
struct Big {
    _dir: tempfile::TempDir,
    host: meguri_testkit::bench::fixtures::CoreHost,
}

fn big(cards: i64, rows: i64) -> Big {
    let dir = tempfile::tempdir().unwrap();
    let path = crate::graphs_common::fixture_path(dir.path());
    fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2"),
        &path,
    )
    .unwrap();
    let clock = meguri_model::clock::FixedClock::new(NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    let opened = meguri_storage::open_collection(&path, &meguri_storage::OpenOptions::new(&clock, &random)).unwrap();
    let first = NOW_MS - 900 * DAY * 1_000;
    for statement in [
        "DELETE FROM revlog".to_owned(),
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {cards}) \
             INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, \
             odid, flags, data) SELECT {first} + i, {NOTE}, {LANG}, 0, 0, -1, 2, 2, 100, i % 60, 2500, 1, 0, 0, 0, 0, 0, \
             '{{}}' FROM n"
        ),
        format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {rows}) \
             INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) SELECT {first} + i * 60000, \
             {first} + 1 + (i % {cards}), -1, 1 + i % 4, i % 50 - 5, 1, 2500, 5000, i % 3 FROM n"
        ),
    ] {
        opened.writer().execute_raw(&statement, &[]).unwrap();
    }
    drop(opened);
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    let host = meguri_testkit::bench::fixtures::open_engine_collection(&path, clock).unwrap();
    Big { _dir: dir, host }
}

#[test]
fn h15_a_build_reports_fractions_that_never_go_back() {
    let big = big(30_000, 240_000);
    let seen = Arc::new(Mutex::new(Vec::new()));
    let collection = big.host.collection().unwrap();
    let request = request(Scope::Collection, History::AllHistory);
    let sink = Arc::clone(&seen);
    let result = history::card_states(big.host.stats(), &collection, big.host.derived(), &request, &mut |progress| {
        sink.lock().unwrap().push(progress)
    })
    .unwrap();
    assert!(data(result).from_component);
    let seen = seen.lock().unwrap();
    let mut last = 0.0;
    for progress in seen.iter() {
        if let Progress::Fraction(fraction) = progress {
            assert!((0.0..=1.0).contains(fraction), "{seen:?}");
            assert!(*fraction >= last, "{seen:?}");
            last = *fraction;
        }
    }
    assert!(!seen.is_empty(), "a build this size reports progress");
}

#[test]
fn h16_a_cancelled_read_returns_within_50_ms_and_the_build_goes_on() {
    let big = big(30_000, 240_000);
    let host = Arc::new(big.host);
    let cancel = CancelToken::new();
    let waiting = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let (release, go) = std::sync::mpsc::channel::<()>();
    let reader = {
        let (host, cancel, waiting) = (Arc::clone(&host), Arc::clone(&cancel), Arc::clone(&waiting));
        std::thread::spawn(move || {
            let collection = host.collection().unwrap();
            let request = HistoryRequest {
                scope: Scope::Collection,
                history: History::AllHistory,
                cancel,
            };
            // The first report holds the read until the test has cancelled
            // it, so it is still waiting then.
            let mut held = Some(go);
            let result = history::card_states(host.stats(), &collection, host.derived(), &request, &mut |_| {
                waiting.store(true, std::sync::atomic::Ordering::SeqCst);
                if let Some(go) = held.take() {
                    go.recv().unwrap();
                }
            })
            .unwrap();
            (result, meguri_engine::monotonic::now())
        })
    };
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !waiting.load(std::sync::atomic::Ordering::SeqCst) {
        assert!(meguri_engine::monotonic::now() < deadline, "the read never waited for the build");
        std::thread::sleep(Duration::from_millis(1));
    }
    let cancelled_at = meguri_engine::monotonic::now();
    cancel.cancel();
    release.send(()).unwrap();
    let (result, returned_at) = reader.join().unwrap();
    assert_eq!(result, HistoryResult::Cancelled);
    let took = returned_at.saturating_sub(cancelled_at);
    assert!(took <= Duration::from_millis(50), "returned {took:?} after the cancel");
    // The build was not cancelled with it.
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness != Readiness::Ready {
        assert!(!host.derived().has_failed(HISTORY_COMPONENT));
        assert!(meguri_engine::monotonic::now() < deadline, "the build stopped");
        std::thread::sleep(Duration::from_millis(5));
    }
    assert!(data(read(&host, Scope::Collection, History::AllHistory)).from_component);
}

#[test]
fn h18_a_store_that_cannot_be_written_leaves_the_parts_to_the_snapshot() {
    let f = Fixture::new();
    let solo = random_collection(&f, 31, 80);
    compare(&f, solo, "built");
    let expected = warm(&f, Scope::Collection, History::AllHistory);
    // Another connection holds the store's write lock: the next commit's
    // transaction fails, and so does marking the component stale, so the
    // store lets go of its file (D6).
    let held = meguri_storage::rusqlite::Connection::open(f.path().with_file_name("meguri-derived.db")).unwrap();
    held.busy_timeout(WAIT).unwrap();
    held.execute_batch("BEGIN IMMEDIATE").unwrap();
    f.write(|scope| {
        scope.set_config("firstDayOfWeek", &ConfigValue::Integer(6))?;
        Ok(())
    });
    let (cards, _) = stored(&f);
    let id = cards.iter().find(|c| c.4 == 2).unwrap().0;
    f.write(move |scope| {
        let mut c = current(scope, id);
        c.interval = 99;
        scope.put(&c)?;
        Ok(())
    });
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !f.host.derived().has_failed(HISTORY_COMPONENT) {
        assert!(meguri_engine::monotonic::now() < deadline, "the store never let go of its file");
        std::thread::sleep(Duration::from_millis(5));
    }
    drop(held);
    compare_from(&f, solo, "unbound", false);
    let got = warm(&f, Scope::Collection, History::AllHistory);
    assert!(!got.from_component);
    assert_eq!(got.card_states.on(0).total(), expected.card_states.on(0).total());
}

#[test]
fn h14_4_commits_during_a_build_are_kept_and_applied_after_the_swap() {
    let big = big(30_000, 240_000);
    let host = &big.host;
    assert!(data(read(host, Scope::Collection, History::AllHistory)).from_component);
    let collection = host.collection().unwrap();
    let card = NOW_MS - 900 * DAY * 1_000 + 7;
    host.derived().invalidate(Invalidation::CheckDatabase);
    // Building with rows done: its snapshot is taken and it has not swapped.
    let deadline = meguri_engine::monotonic::now() + WAIT;
    while !matches!(
        host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness,
        Readiness::Building { completed, .. } if completed > 0
    ) {
        assert!(meguri_engine::monotonic::now() < deadline, "the build never started");
        std::thread::sleep(Duration::from_millis(1));
    }
    collection
        .run(meguri_engine::OpSpec::named("answer", meguri_model::changes::OpSource::UserUndoable), move |ctx| {
            let mut c = meguri_storage::ReadRows::card(&*ctx.scope(), CardId(card))?.unwrap();
            c.card_type = CardType::Relearning;
            ctx.scope().put(&c)?;
            ctx.scope().insert_revlog(&row(NOW_MS - 1_000, card, 1, -600, 2500, RevlogKind::Relearning))?;
            Ok(())
        })
        .unwrap();
    assert!(
        matches!(host.derived().readiness(HISTORY_COMPONENT).unwrap().readiness, Readiness::Building { .. }),
        "the commit landed after the swap: this run does not test kept deltas"
    );
    for history in [History::AllHistory, History::Last12Months] {
        let request = request(Scope::Collection, history);
        let context = history::page_context(&collection, host.derived()).unwrap();
        let warm = data(history::card_states_in(host.stats(), &collection, host.derived(), &context, &request, &mut |_| {}).unwrap());
        assert!(warm.from_component);
        let cold = data(history::card_states_cold(&collection, &context, &request).unwrap());
        assert_eq!(warm.card_states, cold.card_states, "{history:?}");
        assert_eq!(warm.started, cold.started, "{history:?}");
        assert_eq!(warm.card_states.on(0).relearning, cold.card_states.on(0).relearning);
        assert!(warm.card_states.on(0).relearning >= 1);
    }
}
