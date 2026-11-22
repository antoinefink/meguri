//! `stats/graphs-common` G9: the `stats_columns` component. Its build waits
//! for `homeIdle` (G9.2), its files persist at close and load at the next
//! open (G9.3), it follows every change set (G9.4), a memory warning
//! releases it (G9.5), and every result equals the same request computed
//! from the snapshot, after any sequence of changes (G9.6). Names and
//! values are made up.

use std::time::{Duration, Instant};

use meguri_engine::Readiness;
use meguri_model::card::{CardQueue, CardType};
use meguri_model::ids::{CardId, DeckId, RevlogId};
use meguri_model::revlog::RevlogKind;
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_stats::graphs::{COMPONENT, Columns, GraphResult, History, Scope, StatsCard, Unreadable};
use meguri_storage::{Cell, RowKey, Table};

use crate::graphs_common::{CRAM, Fixture, LANG, NOW_MS, VERBS, card, insert_card, revlog, revlog_cells};

const WAIT: Duration = Duration::from_secs(60);

/// What a request gave, in a form two requests compare by.
#[derive(Debug, PartialEq)]
enum Outcome {
    Data {
        cards: Vec<StatsCard>,
        rows: Vec<(i64, Option<i64>, meguri_stats::graphs::rows::StatsRow)>,
        cached: bool,
    },
    Unreadable(Unreadable),
    Other(String),
}

fn outcome(result: GraphResult) -> Outcome {
    match result {
        GraphResult::Data(data) => Outcome::Data {
            cards: data.scope.cards().collect(),
            rows: data.scope.rows().map(|r| (r.row.id, r.card, r.row)).collect(),
            cached: data.scope.from_caches(),
        },
        GraphResult::UnreadableRow(bad) => Outcome::Unreadable(bad),
        other => Outcome::Other(format!("{other:?}")),
    }
}

fn without_source(outcome: Outcome) -> Outcome {
    match outcome {
        Outcome::Data { cards, rows, .. } => Outcome::Data {
            cards,
            rows,
            cached: false,
        },
        other => other,
    }
}

fn readiness(f: &Fixture) -> Readiness {
    f.host.derived().readiness(COMPONENT).expect("registered").readiness
}

fn wait_ready(f: &Fixture) {
    wait_ready_after(f, "");
}

fn wait_ready_after(f: &Fixture, step: &str) {
    let deadline = Instant::now() + WAIT;
    while readiness(f) != Readiness::Ready {
        assert!(Instant::now() < deadline, "{step}: stats_columns never became ready: {:?}", readiness(f));
        std::thread::sleep(Duration::from_millis(5));
    }
    f.host.derived().flush().unwrap();
}

fn scopes() -> Vec<Scope> {
    vec![
        Scope::Collection,
        Scope::Deck(DeckId(LANG)),
        Scope::Deck(DeckId(VERBS)),
        Scope::Deck(DeckId(CRAM)),
        Scope::Search("deck:Lang or is:suspended".into()),
    ]
}

/// G9.6: the cached result equals the snapshot's, for every scope and
/// history; with `cached`, the caches served the request.
fn compare(f: &Fixture, cached: bool, step: &str) {
    f.host.derived().flush().unwrap();
    for scope in scopes() {
        for history in [History::Last12Months, History::AllHistory] {
            let from_caches = outcome(f.request_with(scope.clone(), history, Columns::Cached));
            let from_snapshot = outcome(f.request_with(scope.clone(), history, Columns::Snapshot));
            if cached && let Outcome::Data { cached: served, .. } = &from_caches {
                assert!(served, "{step}: {scope:?} {history:?} was not served by the caches");
            }
            assert_eq!(
                without_source(from_caches),
                without_source(from_snapshot),
                "{step}: {scope:?} {history:?}"
            );
        }
    }
}

#[test]
fn g9_2_the_build_waits_for_home_idle_and_a_request_computes_meanwhile() {
    let f = Fixture::new();
    assert!(f.host.derived().wait_idle(WAIT));
    assert_eq!(readiness(&f), Readiness::Stale);
    // Still held: the request reads its snapshot.
    let before = outcome(f.request(Scope::Collection, History::AllHistory));
    assert!(matches!(before, Outcome::Data { cached: false, .. }));
    // A signal no component names changes nothing.
    f.host.derived().signal("somethingElse");
    assert!(f.host.derived().wait_idle(WAIT));
    assert_eq!(readiness(&f), Readiness::Stale);
    meguri_stats::graphs::stats_home_idle(f.host.derived());
    wait_ready(&f);
    let after = outcome(f.request(Scope::Collection, History::AllHistory));
    assert!(matches!(after, Outcome::Data { cached: true, .. }));
    assert_eq!(without_source(after), without_source(before));
}

#[test]
fn g9_2_request_refresh_starts_the_build_at_once() {
    let f = Fixture::new();
    f.host.derived().request_refresh(COMPONENT);
    wait_ready(&f);
    compare(&f, true, "built");
}

#[test]
fn g9_3_a_close_persists_the_overlay_and_the_next_open_maps_it_without_a_build() {
    let mut f = Fixture::new();
    f.host.derived().request_refresh(COMPONENT);
    wait_ready(&f);
    f.write(|scope| {
        scope.put(&card(100, LANG))?;
        for at in 0..4 {
            scope.insert_revlog(&revlog(NOW_MS - at * 1000, 100))?;
        }
        Ok(())
    });
    compare(&f, true, "overlay");
    let expected = without_source(outcome(f.request(Scope::Collection, History::AllHistory)));
    f.reopen();
    // No signal, so a rebuild would stay held: Ready means the files were
    // fresh and mapped as they are.
    assert!(f.host.derived().wait_idle(WAIT));
    assert_eq!(readiness(&f), Readiness::Ready);
    let reopened = outcome(f.request(Scope::Collection, History::AllHistory));
    assert!(matches!(reopened, Outcome::Data { cached: true, .. }));
    assert_eq!(without_source(reopened), expected);
    compare(&f, true, "reopened");
    // Only the live base's two files are left.
    let files = std::fs::read_dir(f.path().with_file_name("meguri-derived.files").join(COMPONENT))
        .unwrap()
        .count();
    assert_eq!(files, 2);
}

#[test]
fn g9_5_a_memory_warning_releases_and_the_next_request_reloads_the_same() {
    let f = Fixture::new();
    f.host.derived().request_refresh(COMPONENT);
    wait_ready(&f);
    // An empty overlay first, then a non-empty one.
    meguri_stats::graphs::release_stats_caches(f.host.stats(), f.host.derived());
    compare(&f, true, "released empty");
    f.write(|scope| {
        scope.put(&card(100, VERBS))?;
        scope.insert_revlog(&revlog(NOW_MS - 1_000, 100))?;
        scope.remove_card(CardId(super::graphs_common::CRAM_CARD))?;
        Ok(())
    });
    compare(&f, true, "overlay");
    meguri_stats::graphs::release_stats_caches(f.host.stats(), f.host.derived());
    assert_eq!(readiness(&f), Readiness::Ready);
    compare(&f, true, "released with an overlay");
}

/// One random change of G9.6's mix.
struct Mix {
    random: SeededRandom,
    next_card: i64,
    next_row: i64,
    cards: Vec<i64>,
    removed: Vec<i64>,
    bad_rows: Vec<i64>,
    decks: Vec<i64>,
}

impl Mix {
    fn pick(&self, n: usize) -> usize {
        (self.random.next_u64() % n as u64) as usize
    }

    fn card_of(&self) -> Option<i64> {
        (!self.cards.is_empty()).then(|| self.cards[self.pick(self.cards.len())])
    }

    fn row_id(&mut self) -> i64 {
        self.next_row += 1;
        // Some rows older than the 12-month window.
        let days = if self.pick(4) == 0 { 400 } else { self.pick(300) as i64 };
        NOW_MS - days * 86_400_000 - self.next_row
    }

    /// Runs one change; returns its name.
    fn step(&mut self, f: &mut Fixture) -> &'static str {
        match self.pick(14) {
            0 | 1 => {
                self.next_card += 1;
                let id = self.next_card;
                let deck = self.decks[self.pick(self.decks.len())];
                let filtered = self.pick(5) == 0;
                let rows: Vec<i64> = (0..self.pick(4)).map(|_| self.row_id()).collect();
                let new = self.pick(3) == 0;
                f.write(move |scope| {
                    let mut c = card(id, if filtered { CRAM } else { deck });
                    if filtered {
                        c.original_deck_id = DeckId(deck);
                    }
                    if new {
                        c.card_type = CardType::New;
                        c.queue = CardQueue::New;
                    }
                    scope.put(&c)?;
                    for row in rows {
                        scope.insert_revlog(&revlog(row, id))?;
                    }
                    Ok(())
                });
                self.cards.push(id);
                "add"
            }
            2 | 3 => {
                let Some(id) = self.card_of() else { return "none" };
                let row = self.row_id();
                let ivl = self.pick(100) as u32;
                f.write(move |scope| {
                    let mut c = scope_card(scope, id);
                    c.interval = ivl;
                    c.card_type = CardType::Review;
                    c.queue = CardQueue::Review;
                    scope.put(&c)?;
                    scope.insert_revlog(&revlog(row, id))?;
                    Ok(())
                });
                "answer"
            }
            4 => {
                let Some(id) = self.card_of() else { return "none" };
                f.write(move |scope| {
                    // The undo of an answer: its row goes, the card returns.
                    let rows = meguri_storage::ReadRows::revlog_ids_of_card(&*scope, CardId(id))?;
                    if let Some(last) = rows.last() {
                        scope.remove_revlog(RevlogId(*last))?;
                    }
                    let mut c = scope_card(scope, id);
                    c.interval = c.interval.saturating_sub(1);
                    scope.put(&c)?;
                    Ok(())
                });
                "undo"
            }
            5 => {
                let Some(id) = self.card_of() else { return "none" };
                f.write(move |scope| {
                    scope.remove_card(CardId(id))?;
                    Ok(())
                });
                self.cards.retain(|c| *c != id);
                self.removed.push(id);
                "delete"
            }
            6 => {
                let Some(id) = self.removed.pop() else { return "none" };
                let deck = self.decks[self.pick(self.decks.len())];
                f.write(move |scope| {
                    scope.put(&card(id, deck))?;
                    Ok(())
                });
                self.cards.push(id);
                "restore"
            }
            7 => {
                let Some(id) = self.card_of() else { return "none" };
                let deck = self.decks[self.pick(self.decks.len())];
                let queue = [CardQueue::Suspended, CardQueue::BuriedByUser, CardQueue::Review][self.pick(3)];
                f.write(move |scope| {
                    let mut c = scope_card(scope, id);
                    c.deck_id = DeckId(deck);
                    c.original_deck_id = DeckId(0);
                    c.queue = queue;
                    scope.put(&c)?;
                    Ok(())
                });
                "move, suspend or bury"
            }
            8 => {
                let Some(id) = self.card_of() else { return "none" };
                let row = self.row_id();
                f.write(move |scope| {
                    // Forget: new again, with a Manual row.
                    let mut c = scope_card(scope, id);
                    c.card_type = CardType::New;
                    c.queue = CardQueue::New;
                    c.interval = 0;
                    scope.put(&c)?;
                    let mut entry = revlog(row, id);
                    entry.kind = RevlogKind::Manual;
                    entry.button = 0;
                    scope.insert_revlog(&entry)?;
                    Ok(())
                });
                "forget"
            }
            9 => {
                // A re-key (`CloudRekey`): the card under a new id, its rows'
                // `cid` rewritten in place.
                let Some(old) = self.card_of() else { return "none" };
                self.next_card += 1;
                let new = self.next_card;
                f.write_raw(move |scope| {
                    let mut c = scope_card(scope, old);
                    let rows = meguri_storage::ReadRows::revlog_ids_of_card(&*scope, CardId(old))?;
                    scope.remove_card(CardId(old))?;
                    c.id = CardId(new);
                    scope.put(&c)?;
                    for id in rows {
                        let mut image = scope_row(scope, id);
                        image[1] = Cell::Integer(new);
                        scope.restore(Table::Revlog, &RowKey::Id(id), Some(&image))?;
                    }
                    Ok(())
                });
                self.cards.retain(|c| *c != old);
                self.cards.push(new);
                "re-key"
            }
            10 => {
                // A row replaced by a readable one with the same id: removed,
                // then added again (no op rewrites a row in place but a
                // re-key's `cid`).
                let Some(cid) = self.card_of() else { return "none" };
                let Some(id) = self.bad_rows.pop().or_else(|| Some(self.row_id())) else { return "none" };
                f.write(move |scope| {
                    scope.remove_revlog(RevlogId(id))?;
                    Ok(())
                });
                f.write(move |scope| {
                    scope.insert_revlog(&revlog(id, cid))?;
                    Ok(())
                });
                "row replaced"
            }
            11 => {
                let Some(cid) = self.card_of() else { return "none" };
                let id = self.row_id();
                f.write_raw(move |scope| {
                    let cells = revlog_cells(id, cid, Cell::Integer(3), Cell::Integer(1), Cell::Integer(-1), Cell::Integer(1), Cell::Integer(1));
                    scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
                    Ok(())
                });
                self.bad_rows.push(id);
                "unreadable factor"
            }
            12 => {
                if let Some(id) = self.bad_rows.pop() {
                    f.write(move |scope| {
                        scope.remove_revlog(RevlogId(id))?;
                        Ok(())
                    });
                    return "unreadable row removed";
                }
                let Some(cid) = self.card_of() else { return "none" };
                let id = self.row_id();
                f.write_raw(move |scope| {
                    let cells = revlog_cells(id, cid, Cell::Integer(300), Cell::Integer(1), Cell::Integer(0), Cell::Integer(1), Cell::Integer(1));
                    scope.restore(Table::Revlog, &RowKey::Id(id), Some(&cells))?;
                    Ok(())
                });
                self.bad_rows.push(id);
                "unreadable row"
            }
            _ => match self.pick(3) {
                0 => {
                    meguri_stats::graphs::release_stats_caches(f.host.stats(), f.host.derived());
                    "memory warning"
                }
                1 => {
                    f.reopen();
                    // A clean close leaves the files fresh: no build needed.
                    // The app signals Home's idleness after each open, which
                    // releases any later rebuild (G9.2).
                    meguri_stats::graphs::stats_home_idle(f.host.derived());
                    "close and reopen"
                }
                _ => {
                    f.write(|scope| {
                        scope.set_config("firstDayOfWeek", &meguri_storage::ConfigValue::Integer(1))?;
                        Ok(())
                    });
                    "a change the copies ignore"
                }
            },
        }
    }
}

fn scope_card(scope: &mut meguri_storage::WriteScope<'_>, id: i64) -> meguri_model::card::Card {
    meguri_storage::ReadRows::card(&*scope, CardId(id)).unwrap().unwrap_or_else(|| card(id, LANG))
}

fn scope_row(scope: &mut meguri_storage::WriteScope<'_>, id: i64) -> Vec<Cell> {
    meguri_storage::ReadRows::row_image(&*scope, Table::Revlog, &RowKey::Id(id)).unwrap().unwrap()
}

/// One run of G9.6's mix from `seed` over `f`.
fn mix_run(mut f: Fixture, seed: u64, bad_rows: Vec<i64>) {
    let mut mix = Mix {
        random: SeededRandom::new(seed),
        next_card: 10_000,
        next_row: 0,
        cards: vec![super::graphs_common::CRAM_CARD],
        removed: Vec::new(),
        bad_rows,
        decks: vec![LANG, VERBS, 1],
    };
    // Changes that land while the build runs.
    f.host.derived().request_refresh(COMPONENT);
    for _ in 0..6 {
        mix.step(&mut f);
    }
    wait_ready(&f);
    compare(&f, true, &format!("seed {seed}: after the build"));
    for at in 0..40 {
        f.clock.set_now_ms(NOW_MS + at * 1_000);
        let name = mix.step(&mut f);
        wait_ready_after(&f, &format!("seed {seed}, step {at}: {name}"));
        compare(&f, true, &format!("seed {seed}, step {at}: {name}"));
    }
}

#[test]
fn g9_6_the_caches_equal_the_snapshot_after_random_changes() {
    for seed in [3, 17, 2026] {
        mix_run(Fixture::new(), seed, Vec::new());
    }
}

#[test]
fn g9_6_with_unreadable_rows_written_raw_before_the_build() {
    // An unreadable card in Default and an unreadable row of a Lang card:
    // the Collection and Lang scopes fail alike on both paths until the row
    // is removed; the card stays (no op may write or remove it).
    let old = NOW_MS - 30 * 86_400_000;
    let f = Fixture::with_raw(&[
        &insert_card(9_000, 1, 7, 2, 3),
        &format!(
            "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) \
             VALUES ({old}, {}, -1, 300, 1, 1, 0, 1, 1)",
            super::graphs_common::CRAM_CARD
        ),
    ]);
    mix_run(f, 99, vec![old]);
}
