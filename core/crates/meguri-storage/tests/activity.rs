//! The activity heatmap's day counts (`stats/activity-heatmap` AH2 to AH5):
//! which rows count, on which day, per scope; the statements' plans never
//! scan the review log; and the counts stay right and quick on a review log
//! of a million rows.
mod common;

use std::path::Path;
use std::time::Duration;

use common::{empty_collection, open, plain};
use meguri_model::ids::DeckId;
use meguri_model::rng::{RandomSource, SeededRandom};
use meguri_storage::queries::activity::{
    ACTIVITY_DAYS, ActivityScope, COLLECTION_SQL, deck_sql, window_start,
};
use meguri_storage::rusqlite::{Connection, params};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::wallclock::Stopwatch;

/// 2026-10-13 04:00 Europe/Paris: `stats/day-buckets` B3's `N`.
const N: i64 = 1_791_856_800;
const DAY: i64 = 86_400;

fn insert_card(conn: &Connection, id: i64, did: i64, odid: i64) {
    conn.execute(
        "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) \
         VALUES (?1, ?1, ?2, 0, 0, 0, 2, 2, ?3, 1, 2500, 1, 0, 0, 0, ?4, 0, '')",
        params![id, did, id % 500, odid],
    )
    .unwrap();
}

fn insert_review(conn: &Connection, id: i64, cid: i64, ease: i64, kind: i64) {
    conn.execute(
        "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?1, ?2, 0, ?3, 1, 1, 2500, 4000, ?4)",
        params![id, cid, ease, kind],
    )
    .unwrap();
}

fn counts(path: &Path, scope: ActivityScope<'_>, next_day_at: i64) -> Vec<u32> {
    let collection = open(path).unwrap();
    let reader = collection.reader_factory().open(ReaderRole::Query).unwrap();
    reader.activity_day_counts(scope, next_day_at).unwrap()
}

/// `k` values as (day, count) pairs, zeros left out.
fn days(counts: &[u32]) -> Vec<(usize, u32)> {
    counts.iter().enumerate().filter(|(_, n)| **n > 0).map(|(k, n)| (k, *n)).collect()
}

#[test]
fn rows_land_on_anki_days_back_from_the_boundary_and_only_rated_rows_count() {
    let (_dir, path) = empty_collection();
    {
        let conn = plain(&path);
        insert_card(&conn, 1, 10, 0);
        let ms = |secs: i64| secs * 1000;
        // AH3's table: the boundary itself is today; one day before it is day 1.
        insert_review(&conn, ms(N), 1, 3, 1);
        insert_review(&conn, ms(N - 1), 1, 3, 1);
        insert_review(&conn, ms(N - DAY) + 999, 1, 3, 1);
        insert_review(&conn, ms(N - DAY - 1), 1, 3, 1);
        // After the boundary (clock skew): today.
        insert_review(&conn, ms(N + 3_600), 1, 3, 1);
        insert_review(&conn, ms(N + 90_000), 1, 3, 1);
        // AH2: answers of every type count; unrated rows never do.
        for (offset, ease, kind) in [(10, 3, 0), (11, 1, 2), (12, 3, 3), (13, 2, 4), (14, 0, 4), (15, 0, 5), (16, 0, 1)] {
            insert_review(&conn, ms(N - 2 * DAY - offset), 1, ease, kind);
        }
        // The window's edges: day 370 is in it, day 371 is not.
        insert_review(&conn, window_start(N), 1, 4, 1);
        insert_review(&conn, window_start(N) - 1, 1, 4, 1);
        // A deleted card's row counts for the collection only.
        insert_review(&conn, ms(N - 5 * DAY), 999, 3, 1);
    }
    let all = counts(&path, ActivityScope::Collection, N);
    assert_eq!(all.len(), ACTIVITY_DAYS as usize);
    assert_eq!(days(&all), [(0, 4), (1, 2), (2, 4), (5, 1), (370, 1)]);
    let deck = counts(&path, ActivityScope::HomeDecks(&[DeckId(10)]), N);
    assert_eq!(days(&deck), [(0, 4), (1, 2), (2, 4), (370, 1)]);
    // A deck nobody's card is in, and no deck at all, count nothing.
    assert!(days(&counts(&path, ActivityScope::HomeDecks(&[DeckId(11)]), N)).is_empty());
    assert!(days(&counts(&path, ActivityScope::HomeDecks(&[]), N)).is_empty());
    // A later boundary moves every row one day back.
    let next = counts(&path, ActivityScope::Collection, N + DAY);
    assert_eq!(days(&next), [(0, 2), (1, 2), (2, 2), (3, 4), (6, 1)]);
}

#[test]
fn deck_scope_follows_the_home_deck_of_cards_as_they_are_now() {
    let (_dir, path) = empty_collection();
    {
        let conn = plain(&path);
        insert_card(&conn, 1, 10, 0); // in its home deck
        insert_card(&conn, 2, 20, 0); // a subdeck
        insert_card(&conn, 3, 99, 10); // in a filtered deck, home 10
        insert_card(&conn, 4, 10, 30); // sitting in 10 with another home: not 10's
        insert_card(&conn, 5, 30, 0);
        for cid in 1..=5 {
            insert_review(&conn, N * 1000 - cid, cid, 3, 1);
        }
    }
    let scope = |decks: &[DeckId]| days(&counts(&path, ActivityScope::HomeDecks(decks), N));
    assert_eq!(scope(&[DeckId(10)]), [(0, 2)]);
    assert_eq!(scope(&[DeckId(10), DeckId(20)]), [(0, 3)]);
    assert_eq!(scope(&[DeckId(30)]), [(0, 2)]);
    assert!(scope(&[DeckId(99)]).is_empty(), "a filtered deck holds no home cards");
    // Moving a card moves its rows at once.
    plain(&path).execute("UPDATE cards SET did = 30 WHERE id = 1", []).unwrap();
    assert_eq!(scope(&[DeckId(10)]), [(0, 1)]);
    assert_eq!(scope(&[DeckId(30)]), [(0, 3)]);
}

/// The plan's detail lines, with the values bound as the query binds them.
fn plan(conn: &Connection, sql: &str) -> Vec<String> {
    let mut statement = conn.prepare(&format!("EXPLAIN QUERY PLAN {sql}")).unwrap();
    let n = statement.parameter_count();
    let values: Vec<meguri_storage::rusqlite::types::Value> = (1..=n)
        .map(|i| match i {
            3 => 10.into(),
            2 => window_start(N).into(),
            _ => N.into(),
        })
        .collect();
    statement
        .query_map(meguri_storage::rusqlite::params_from_iter(values), |row| row.get::<_, String>(3))
        .unwrap()
        .collect::<Result<_, _>>()
        .unwrap()
}

/// The review log is never scanned: it is read by rowid range or through
/// `ix_revlog_cid` with the rowid bound.
fn assert_bounded(lines: &[String], context: &str) {
    for line in lines {
        let scan = line.starts_with("SCAN revlog") || line.starts_with("SCAN r ") || line == "SCAN r";
        assert!(!scan, "{context}: {line} in {lines:#?}");
    }
    let searches: Vec<&String> = lines.iter().filter(|l| l.contains("SEARCH revlog") || l.contains("SEARCH r ")).collect();
    assert!(!searches.is_empty(), "{context}: the review log is searched: {lines:#?}");
    // Every read of the review log keeps the window's rowid bound: a search by card
    // alone would read each card's whole history.
    for line in searches {
        assert!(line.contains("rowid>?"), "{context}: {line} drops the window bound in {lines:#?}");
    }
}

/// Both halves of the deck scope's card set come through their index.
fn assert_plans(conn: &Connection, context: &str) {
    let collection = plan(conn, COLLECTION_SQL);
    assert_bounded(&collection, context);
    assert!(
        collection.iter().any(|l| l.contains("INTEGER PRIMARY KEY (rowid>?)")),
        "{context}: a rowid range: {collection:#?}"
    );
    let deck = plan(conn, &deck_sql(1));
    assert_bounded(&deck, context);
    assert!(deck.iter().any(|l| l.contains("ix_cards_sched")), "{context}: {deck:#?}");
    assert!(!deck.iter().any(|l| l.starts_with("SCAN c")), "{context}: {deck:#?}");
    assert!(deck.iter().any(|l| l.contains("idx_cards_odid")), "{context}: {deck:#?}");
}

#[test]
fn neither_statement_scans_the_review_log() {
    let (_dir, path) = empty_collection();
    let conn = plain(&path);
    assert_plans(&conn, "fresh file");
    // Anki's own files carry ANALYZE statistics (storage/schema-18 S5): three
    // years of a small collection's history, then statistics. (The name
    // indexes need `unicase`, which this plain connection lacks.)
    for card in 1..=300 {
        insert_card(&conn, card, 10 + card % 20, 0);
    }
    for row in 0..3_000 {
        insert_review(&conn, (N - row * 3 * 365 * DAY / 3_000) * 1000 - row, 1 + row % 300, 3, 1);
    }
    conn.execute_batch("ANALYZE revlog; ANALYZE cards").unwrap();
    assert_plans(&conn, "analyzed file");
}

/// Draws below `n` from the model's deterministic source, so the expected
/// counts can be computed from the same sequence.
fn below(source: &SeededRandom, n: u64) -> u64 {
    source.next_u64() % n
}

const CARDS: i64 = 100_000;
const REVIEWS: i64 = 1_000_000;
/// The large deck: 40% of the cards, a fifth of them through a filtered deck.
const LARGE: i64 = 2;
/// A small deck of 500 cards.
const SMALL: i64 = 7;

fn home_and_current(card: i64) -> (i64, i64, i64) {
    match card % 100 {
        0..=7 => (LARGE, 50, LARGE),   // in filtered deck 50, home LARGE
        8..=39 => (LARGE, LARGE, 0),
        40 if card % 2 == 0 => (SMALL, SMALL, 0),
        h => (100 + h % 9, 100 + h % 9, 0),
    }
}

#[test]
fn a_million_reviews_count_correctly_and_quickly() {
    let (_dir, path) = empty_collection();
    let mix = SeededRandom::new(20_261_120);
    let mut expected_all = vec![0u32; ACTIVITY_DAYS as usize];
    let mut expected_large = vec![0u32; ACTIVITY_DAYS as usize];
    let mut expected_small = vec![0u32; ACTIVITY_DAYS as usize];
    let built = Stopwatch::start();
    {
        let mut conn = plain(&path);
        let tx = conn.transaction().unwrap();
        {
            let mut cards = tx
                .prepare(
                    "INSERT INTO cards (id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, left, odue, odid, flags, data) \
                     VALUES (?1, ?1, ?2, 0, 0, 0, 2, 2, ?3, 1, 2500, 1, 0, 0, 0, ?4, 0, '')",
                )
                .unwrap();
            for card in 1..=CARDS {
                let (_, did, odid) = home_and_current(card);
                cards.execute(params![card, did, card % 500, odid]).unwrap();
            }
            let mut reviews = tx
                .prepare("INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?1, ?2, 0, ?3, 1, 1, 2500, 4000, ?4)")
                .unwrap();
            // Four years of history up to a few hours after the boundary;
            // ids made unique by a per-row offset, as a busy log has them.
            let span_ms = 4 * 365 * DAY * 1000;
            let end = N * 1000 + 6 * 3_600_000;
            let mut used = std::collections::HashSet::with_capacity(REVIEWS as usize);
            let mut inserted = 0;
            while inserted < REVIEWS {
                let id = end - below(&mix, span_ms as u64) as i64;
                if !used.insert(id) {
                    continue;
                }
                inserted += 1;
                // A tenth of the cards hold no row (deleted cards' rows: ids past CARDS).
                let card = 1 + below(&mix, (CARDS + CARDS / 10) as u64) as i64;
                let ease = if below(&mix, 20) == 0 { 0 } else { 1 + below(&mix, 4) as i64 };
                let kind = if ease == 0 { 4 } else { below(&mix, 3) as i64 };
                reviews.execute(params![id, card, ease, kind]).unwrap();
                if ease == 0 || id < window_start(N) {
                    continue;
                }
                let k = ((N - id / 1000) / DAY).max(0) as usize;
                expected_all[k] += 1;
                if card <= CARDS {
                    match home_and_current(card).0 {
                        LARGE => expected_large[k] += 1,
                        SMALL => expected_small[k] += 1,
                        _ => {}
                    }
                }
            }
        }
        tx.commit().unwrap();
    }
    eprintln!("built {CARDS} cards and {REVIEWS} reviews in {:?}", built.elapsed());

    let collection = open(&path).unwrap();
    let reader = collection.reader_factory().open(ReaderRole::Query).unwrap();
    let timed = |scope: ActivityScope<'_>| {
        // The first run reads the pages in; report the warm one too.
        let cold = Stopwatch::start();
        let first = reader.activity_day_counts(scope, N).unwrap();
        let cold = cold.elapsed();
        let warm = Stopwatch::start();
        let second = reader.activity_day_counts(scope, N).unwrap();
        let warm = warm.elapsed();
        assert_eq!(first, second);
        (first, cold, warm)
    };
    let (all, all_cold, all_warm) = timed(ActivityScope::Collection);
    let (large, large_cold, large_warm) = timed(ActivityScope::HomeDecks(&[DeckId(LARGE)]));
    let (small, small_cold, small_warm) = timed(ActivityScope::HomeDecks(&[DeckId(SMALL)]));
    let window = expected_all.iter().map(|n| u64::from(*n)).sum::<u64>();
    eprintln!(
        "activity heatmap, {REVIEWS} reviews ({window} counted in the window): collection {all_cold:?} cold / {all_warm:?} warm; \
         large deck ({} rows) {large_cold:?} / {large_warm:?}; small deck ({} rows) {small_cold:?} / {small_warm:?}",
        expected_large.iter().sum::<u32>(),
        expected_small.iter().sum::<u32>(),
    );
    assert_eq!(all, expected_all);
    assert_eq!(large, expected_large);
    assert_eq!(small, expected_small);
    // Generous: a loaded CI machine must not fail this; a full scan or a
    // query per day would still be caught by the plan test above.
    let bound = Duration::from_secs(5);
    for (name, took) in [("collection", all_cold), ("large deck", large_cold), ("small deck", small_cold)] {
        assert!(took < bound, "{name} took {took:?}");
    }

    // With Anki's statistics in the file the plans stay bounded.
    drop(reader);
    drop(collection);
    let conn = plain(&path);
    conn.execute_batch("ANALYZE revlog; ANALYZE cards").unwrap();
    assert_plans(&conn, "analyzed million-row file");
    eprintln!("analyzed plans: collection {:#?}, deck {:#?}", plan(&conn, COLLECTION_SQL), plan(&conn, &deck_sql(1)));
}
