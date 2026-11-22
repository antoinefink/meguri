//! `stats/history-reads` H10 (U291): By Deck, the page scope's cards
//! partitioned by home deck, in the deck tree's order. No oracle: tested by
//! construction (H20) against a direct count of the rows a test writes, the
//! Reviews and Retention records of the same scope, and the table's own
//! sums (a parent's row equals its deck's own row plus its subdecks' rows).
//! Names and values are made up.

use std::collections::{BTreeMap, HashMap};

use meguri_engine::CancelToken;
use meguri_model::ids::DeckId;
use meguri_stats::graphs::records::retention::RetentionRecord;
use meguri_stats::graphs::records::reviews::ReviewsRecord;
use meguri_stats::graphs::{Columns, History, Scope};
use meguri_stats::page::{self, PageEnd, PageMessage, PageRequest, Part, PartOutcome, PartValue};
use meguri_stats::parts::by_deck::{ByDeckPart, DeckRow, DeckValues, DeckWindows};
use meguri_storage::{Cell, RowKey, Table};

use crate::graphs_common::{CRAM, Fixture, LANG, NOTE, VERBS, revlog_cells};

const DAY: i64 = 86_400;
/// A card id no card has.
const GONE: i64 = 4_242;
/// A deck id no deck has: a damaged card's home deck.
const NOWHERE: i64 = 999_999;

/// A row as stored: (id, ease, lastIvl, factor, time, type).
type Row = (i64, i64, i64, i64, i64, i64);

fn add_deck(f: &Fixture, native: &str) -> i64 {
    let deck = meguri_engine::decks::new_deck(native.to_owned());
    let (tx, rx) = std::sync::mpsc::channel();
    f.write(move |scope| {
        tx.send(scope.add_deck(&deck)?.0).unwrap();
        Ok(())
    });
    rx.recv().unwrap()
}

/// Cards as stored: (id, did, odid).
fn put_cards(f: &Fixture, cards: &[(i64, i64, i64)]) {
    let rows: Vec<(i64, Vec<Cell>)> = cards
        .iter()
        .map(|&(id, did, odid)| {
            let int = Cell::Integer;
            let cells = vec![
                int(id),
                int(NOTE),
                int(did),
                int(0),
                int(0),
                int(-1),
                int(2),
                int(2),
                int(5),
                int(30),
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

fn put_rows(f: &Fixture, rows: &[(i64, Row)]) {
    let cells: Vec<(i64, Vec<Cell>)> = rows
        .iter()
        .map(|&(cid, (id, ease, last, factor, time, kind))| {
            let int = Cell::Integer;
            (id, revlog_cells(id, cid, int(ease), int(last), int(factor), int(time), int(kind)))
        })
        .collect();
    f.write_raw(move |scope| {
        for (id, row) in cells {
            scope.restore(Table::Revlog, &RowKey::Id(id), Some(&row))?;
        }
        Ok(())
    });
}

fn parts(f: &Fixture, scope: Scope, history: History, columns: Columns, wanted: &[Part]) -> BTreeMap<Part, PartValue> {
    let request = PageRequest {
        scope,
        history,
        parts: wanted.to_vec(),
        refresh: 1,
        cancel: CancelToken::new(),
        columns,
    };
    let mut values = BTreeMap::new();
    let end = page::request(f.host.stats(), &f.collection(), f.host.derived(), &request, &mut |message| {
        if let PageMessage::Part(part) = message {
            match part.outcome {
                PartOutcome::Data(value) => {
                    values.insert(part.part, value);
                }
                PartOutcome::Failed(failure) => panic!("{:?}: {failure:?}", part.part),
            }
        }
    })
    .unwrap();
    assert_eq!(end, PageEnd::Complete);
    values
}

fn by_deck(f: &Fixture, scope: Scope, history: History) -> ByDeckPart {
    match parts(f, scope, history, Columns::Cached, &[Part::ByDeck]).remove(&Part::ByDeck) {
        Some(PartValue::ByDeck(part)) => part,
        other => panic!("{other:?}"),
    }
}

/// The decks of a part's rows, own-cards rows marked.
fn decks(part: &ByDeckPart) -> Vec<(i64, bool)> {
    part.rows.iter().map(|row| (row.deck.0, row.own_cards)).collect()
}

fn sum(values: impl Iterator<Item = DeckValues>) -> DeckValues {
    values.fold(DeckValues::default(), |a, b| DeckValues {
        reviews: a.reviews + b.reviews,
        time_ms: a.time_ms + b.time_ms,
        passed: a.passed + b.passed,
        failed: a.failed + b.failed,
    })
}

fn window_values(windows: &DeckWindows) -> Vec<DeckValues> {
    windows.values().copied().collect()
}

/// The fixture's decks, three of ours, the cards in them, and rows over
/// two years on every card and on a deleted one.
struct Study {
    f: Fixture,
    n: i64,
    beta: i64,
    sub: i64,
    empty: i64,
    ten: i64,
    four: i64,
    /// Each card's home deck.
    home: HashMap<i64, i64>,
    rows: Vec<(i64, Row)>,
}

fn study() -> Study {
    let f = Fixture::with_raw(&["DELETE FROM cards"]);
    let n = f.next_day_at();
    let beta = add_deck(&f, "Beta");
    let sub = add_deck(&f, "Beta\u{1f}Sub");
    let empty = add_deck(&f, "Empty");
    let ten = add_deck(&f, "10. y");
    let four = add_deck(&f, "4. x");
    // (card, did, odid): two in `Lang`, one in `Lang::Verbs`, one in `Cram`
    // from `Lang`, one each in `Beta`, `Beta::Sub`, `Default`, `10. y`,
    // `4. x`, and one whose home deck names no deck.
    let cards = [
        (1, LANG, 0),
        (2, LANG, 0),
        (3, VERBS, 0),
        (4, CRAM, LANG),
        (5, beta, 0),
        (6, sub, 0),
        (7, 1, 0),
        (8, ten, 0),
        (9, four, 0),
        (10, NOWHERE, 0),
    ];
    put_cards(&f, &cards);
    let home = cards.iter().map(|&(id, did, odid)| (id, if odid != 0 { odid } else { did })).collect();
    let mut rows = Vec::new();
    let backs = [0_i64, 1, 29, 30, 31, 89, 120, 181, 182, 300, 364, 365, 366, 500, 700];
    for (k, back) in backs.into_iter().enumerate() {
        for card in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, GONE] {
            let t = n - back * DAY - 3_600 - card;
            let id = t * 1_000;
            let shape = match (k + card as usize) % 6 {
                // A Good review, an Again review, a learning step, cram, a
                // rated Manual row, an unrated Rescheduled row.
                0 => (id, 3, 30, 2_500, 4_000, 1),
                1 => (id, 1, 5, 2_500, 9_000, 1),
                2 => (id, 3, -600, 2_500, 2_000, 0),
                3 => (id, 3, 10, 0, 1_000, 3),
                4 => (id, 2, 15, 2_500, 3_000, 4),
                _ => (id, 0, 0, 0, 500, 5),
            };
            rows.push((card, shape));
        }
    }
    // The bound of `one_month` (H1): a row at `N − 30 × 86,400` is in it,
    // one a second before is not.
    rows.push((1, ((n - 30 * DAY) * 1_000, 3, 30, 2_500, 70, 1)));
    rows.push((2, ((n - 30 * DAY - 1) * 1_000, 3, 30, 2_500, 80, 1)));
    put_rows(&f, &rows);
    Study {
        f,
        n,
        beta,
        sub,
        empty,
        ten,
        four,
        home,
        rows,
    }
}

/// A direct count of H10.2 over the `rows` of the cards whose home deck
/// (by `home`) is in `decks`, in the window of `days` before `n` (none for
/// all time).
fn count(rows: &[(i64, Row)], home: &HashMap<i64, i64>, n: i64, decks: &[i64], days: Option<i64>) -> DeckValues {
    let mut values = DeckValues::default();
    for (card, (id, ease, last, factor, time, kind)) in rows {
        let Some(home) = home.get(card) else { continue };
        if !decks.contains(home) || days.is_some_and(|days| id / 1_000 < n - days * DAY) {
            continue;
        }
        // V1: Reviews counts every kind but Manual and Rescheduled.
        if !matches!(kind, 4 | 5) {
            values.reviews += 1;
            values.time_ms += time;
        }
        // V4.1: rated, not cram, Review or a long last interval.
        let cram = *kind == 3 && *factor == 0;
        if *ease >= 1 && !cram && (*kind == 1 || *last >= 1 || *last <= -86_400) {
            if *ease == 1 {
                values.failed += 1;
            } else {
                values.passed += 1;
            }
        }
    }
    values
}

impl Study {
    fn expected(&self, decks: &[i64], days: Option<i64>) -> DeckValues {
        count(&self.rows, &self.home, self.n, decks, days)
    }

    /// The deleted card's rows, counted as a row's would be.
    fn deleted(&self, days: Option<i64>) -> DeckValues {
        count(&self.rows, &HashMap::from([(GONE, -1)]), self.n, &[-1], days)
    }

    fn expected_windows(&self, decks: &[i64], history: History) -> Vec<DeckValues> {
        let mut out: Vec<DeckValues> = [30, 90, 182, 365].map(|days| self.expected(decks, Some(days))).to_vec();
        if history == History::AllHistory {
            out.push(self.expected(decks, None));
        }
        out
    }

    fn cards_in(&self, decks: &[i64]) -> u64 {
        self.home.values().filter(|home| decks.contains(home)).count() as u64
    }
}

#[test]
fn h10_all_decks_has_a_row_per_top_level_normal_deck_in_tree_order() {
    let s = study();
    for history in [History::Last12Months, History::AllHistory] {
        let part = by_deck(&s.f, Scope::Collection, history);
        // C1's unicase order; `Cram` is filtered and `Empty` has no card.
        assert_eq!(decks(&part), [(s.ten, false), (s.four, false), (s.beta, false), (1, false), (LANG, false)], "{history:?}");
        let lang = [LANG, VERBS, CRAM];
        let expect: [(&[i64], i64); 5] = [(&[s.ten], s.ten), (&[s.four], s.four), (&[s.beta, s.sub], s.beta), (&[1], 1), (&lang, LANG)];
        for (row, (decks, deck)) in part.rows.iter().zip(expect) {
            assert_eq!(row.deck.0, deck);
            assert_eq!(row.cards, s.cards_in(decks), "{deck}");
            assert_eq!(window_values(&row.windows), s.expected_windows(decks, history), "{deck} {history:?}");
            assert_eq!(row.windows.all_time.is_some(), history == History::AllHistory);
        }
        assert!(!part.rows.iter().any(|row| row.deck.0 == s.empty || row.deck.0 == CRAM));
    }
}

#[test]
fn h10_the_rows_add_up_to_the_scope_less_deleted_cards_and_cards_with_no_home_deck() {
    let s = study();
    let f = &s.f;
    let values = parts(f, Scope::Collection, History::AllHistory, Columns::Cached, &[Part::ByDeck, Part::Reviews, Part::Retention]);
    let PartValue::ByDeck(part) = &values[&Part::ByDeck] else { panic!() };
    let record = |part: Part| match &values[&part] {
        PartValue::Record(record) => record.clone(),
        other => panic!("{other:?}"),
    };
    let reviews = record(Part::Reviews);
    let reviews = reviews.as_any().downcast_ref::<ReviewsRecord>().unwrap();
    let retention = record(Part::Retention);
    let retention = retention.as_any().downcast_ref::<RetentionRecord>().unwrap();
    // Every existing card in exactly one row but the one whose home deck
    // names no deck.
    let cards: u64 = part.rows.iter().map(|row| row.cards).sum();
    assert_eq!(cards, s.home.len() as u64 - 1);
    // All time: the Reviews record's count less the rows of the deleted and
    // the card with no home deck.
    let all = sum(part.rows.iter().map(|row| row.windows.all_time.unwrap()));
    let outside = s.expected(&[NOWHERE], None);
    let deleted = s.deleted(None);
    let counted: i64 = reviews.count.values().map(|kinds| kinds.values().iter().sum::<i64>()).sum();
    let time: i64 = reviews.time.values().map(|kinds| kinds.values().iter().sum::<i64>()).sum();
    assert_eq!(all.reviews + outside.reviews + deleted.reviews, counted as u64);
    assert_eq!(all.time_ms + outside.time_ms + deleted.time_ms, time);
    // A year (no row at or after `N`): the Retention record's.
    let year = sum(part.rows.iter().map(|row| row.windows.one_year));
    let outside = s.expected(&[NOWHERE], Some(365));
    let deleted = s.deleted(Some(365));
    let r = retention.year;
    assert_eq!(year.passed + outside.passed + deleted.passed, r.young_passed + r.mature_passed);
    assert_eq!(year.failed + outside.failed + deleted.failed, r.young_failed + r.mature_failed);
}

#[test]
fn h10_a_parents_row_is_its_own_cards_row_plus_its_subdecks_rows() {
    let s = study();
    for history in [History::Last12Months, History::AllHistory] {
        let all = by_deck(&s.f, Scope::Collection, history);
        let row_of = |deck: i64| -> DeckRow { *all.rows.iter().find(|row| row.deck.0 == deck).unwrap() };
        for (parent, children) in [(LANG, vec![VERBS]), (s.beta, vec![s.sub])] {
            let part = by_deck(&s.f, Scope::Deck(DeckId(parent)), history);
            // The own-cards row first, then each direct normal subdeck.
            let mut want = vec![(parent, true)];
            want.extend(children.iter().map(|child| (*child, false)));
            assert_eq!(decks(&part), want, "{parent} {history:?}");
            let parent_row = row_of(parent);
            assert_eq!(part.rows.iter().map(|row| row.cards).sum::<u64>(), parent_row.cards);
            let windows: Vec<Vec<DeckValues>> = part.rows.iter().map(|row| window_values(&row.windows)).collect();
            let summed: Vec<DeckValues> =
                (0..windows[0].len()).map(|at| sum(windows.iter().map(|values| values[at]))).collect();
            assert_eq!(summed, window_values(&parent_row.windows), "{parent} {history:?}");
        }
        // `Lang`'s own row holds the card in `Cram` (its home deck), not the
        // filtered deck; `Lang::Verbs` alone is its own row.
        let lang = by_deck(&s.f, Scope::Deck(DeckId(LANG)), history);
        assert_eq!(lang.rows[0].cards, 3);
        assert_eq!(window_values(&lang.rows[0].windows), s.expected_windows(&[LANG, CRAM], history));
        let verbs = by_deck(&s.f, Scope::Deck(DeckId(VERBS)), history);
        assert_eq!(decks(&verbs), [(VERBS, true)]);
        // A deck with no card has no row; a filtered deck has no row of its
        // own cards (none has it as home deck) and no subdeck.
        assert!(by_deck(&s.f, Scope::Deck(DeckId(s.empty)), history).rows.is_empty());
        assert!(by_deck(&s.f, Scope::Deck(DeckId(CRAM)), history).rows.is_empty());
    }
}

#[test]
fn h10_a_deck_without_own_cards_has_no_own_row_and_caches_change_nothing() {
    let s = study();
    let f = &s.f;
    let gamma = add_deck(f, "Gamma");
    let kid = add_deck(f, "Gamma\u{1f}Kid");
    put_cards(f, &[(20, kid, 0)]);
    let n = f.next_day_at();
    put_rows(f, &[(20, ((n - 3_600) * 1_000, 3, 30, 2_500, 1_000, 1))]);
    let part = by_deck(f, Scope::Deck(DeckId(gamma)), History::Last12Months);
    assert_eq!(decks(&part), [(kid, false)]);
    assert_eq!(part.rows[0].windows.one_month, DeckValues {
        reviews: 1,
        time_ms: 1_000,
        passed: 1,
        failed: 0
    });
    // The same values from the column caches (with their overlay after a
    // later commit) and from the snapshot.
    meguri_stats::graphs::stats_home_idle(f.host.derived());
    f.host.derived().request_refresh(meguri_stats::graphs::COMPONENT);
    let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(120);
    while f.host.derived().readiness(meguri_stats::graphs::COMPONENT).unwrap().readiness != meguri_engine::Readiness::Ready {
        assert!(meguri_engine::monotonic::now() < deadline);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    f.host.derived().flush().unwrap();
    put_rows(f, &[(20, ((n - 3_000) * 1_000, 1, 30, 2_500, 2_000, 1))]);
    // The graph request does not wait for the derived writer; the page's does.
    f.host.derived().flush().unwrap();
    assert!(f.data(Scope::Collection, History::AllHistory).scope.from_caches());
    for scope in [Scope::Collection, Scope::Deck(DeckId(LANG)), Scope::Deck(DeckId(gamma))] {
        for history in [History::Last12Months, History::AllHistory] {
            let cached = parts(f, scope.clone(), history, Columns::Cached, &[Part::ByDeck]);
            let fresh = parts(f, scope.clone(), history, Columns::Snapshot, &[Part::ByDeck]);
            assert_eq!(format!("{:?}", cached[&Part::ByDeck]), format!("{:?}", fresh[&Part::ByDeck]), "{scope:?} {history:?}");
        }
    }
    // A Search scope is not offered (H1).
    let request = PageRequest {
        scope: Scope::Search("deck:Lang".into()),
        history: History::AllHistory,
        parts: vec![Part::ByDeck],
        refresh: 1,
        cancel: CancelToken::new(),
        columns: Columns::Cached,
    };
    let mut failed = false;
    page::request(f.host.stats(), &f.collection(), f.host.derived(), &request, &mut |message| {
        if let PageMessage::Part(part) = message {
            failed = matches!(part.outcome, PartOutcome::Failed(page::PartFailure::UnsupportedScope));
        }
    })
    .unwrap();
    assert!(failed);
}
