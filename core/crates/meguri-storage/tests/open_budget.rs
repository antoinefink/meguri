//! The open budget (R1823, `storage/open-and-connections` C2.6): a warm open
//! of a schema-18 collection with 150,000 cards and nothing to upgrade takes
//! at most 150 ms. The gate reads only the `col` row, one `config` row and
//! `sqlite_master`, so the time must not grow with the collection; the large
//! file is there to show that. U12's benchmark measures the same on the
//! target devices; this is the host's number with the bundled SQLite.

mod common;

use std::time::Duration;

use common::{copy_fixture, exec, int, open};
use meguri_storage::Writer;
use meguri_testkit::wallclock::Stopwatch;

const NOTES: i64 = 100_000;
const CARDS: i64 = 150_000;
const REVIEWS: i64 = 450_000;
const BUDGET: Duration = Duration::from_millis(150);
const RUNS: usize = 40;

/// `count` rows numbered 1 to `count` (`i`) into `table`, in one statement.
fn fill(w: &Writer, table: &str, count: i64, select: &str) {
    exec(
        w,
        &format!(
            "WITH RECURSIVE n(i) AS (SELECT 1 UNION ALL SELECT i + 1 FROM n WHERE i < {count})
             INSERT INTO {table} SELECT {select} FROM n"
        ),
    );
}

/// The schema-18 fixture grown to `NOTES` notes, `CARDS` cards and
/// `REVIEWS` review entries, closed cleanly.
fn large_collection(path: &std::path::Path) {
    let opened = open(path).unwrap();
    let w = opened.writer();
    let mid = int(&exec(w, "SELECT min(id) FROM notetypes"));
    let note = int(&exec(w, "SELECT max(id) FROM notes"));
    let card = int(&exec(w, "SELECT max(id) FROM cards"));
    let review = int(&exec(w, "SELECT max(id) FROM revlog"));
    fill(
        w,
        "notes",
        NOTES,
        &format!(
            "{note} + i, 'g' || i, {mid}, 1894701600, -1, 'vocab',
             'front ' || i || char(31) || 'back ' || i, 'front ' || i, i, 0, ''"
        ),
    );
    fill(
        w,
        "cards",
        CARDS,
        &format!(
            "{card} + i, {note} + (i - 1) % {NOTES} + 1, 1, (i - 1) / {NOTES}, 1894701600, -1,
             2, 2, i % 400, 10, 2500, 3, 0, 0, 0, 0, 0, ''"
        ),
    );
    fill(
        w,
        "revlog",
        REVIEWS,
        &format!("{review} + i, {card} + (i - 1) % {CARDS} + 1, -1, 3, 10, 4, 2500, 6000, 1"),
    );
    assert!(int(&exec(w, "SELECT count(*) FROM cards")) > CARDS);
    opened.close().unwrap();
}

#[test]
fn a_warm_open_of_a_150k_card_collection_takes_at_most_150_ms() {
    let (_dir, path) = copy_fixture("schema-18.anki2");
    large_collection(&path);

    // Warm: one open first, so the file's pages are in the OS cache.
    open(&path).unwrap().close().unwrap();
    let mut samples: Vec<Duration> = (0..RUNS)
        .map(|_| {
            let watch = Stopwatch::start();
            let opened = open(&path).unwrap();
            let took = watch.elapsed();
            assert_eq!(opened.upgraded_from(), None);
            opened.close().unwrap();
            took
        })
        .collect();
    samples.sort();
    let (p50, p95, max) = (samples[RUNS / 2], samples[RUNS * 95 / 100], samples[RUNS - 1]);
    eprintln!(
        "open gate, {CARDS} cards, {RUNS} warm opens: p50 {p50:?}, p95 {p95:?}, max {max:?}, file {} bytes",
        std::fs::metadata(&path).unwrap().len()
    );
    assert!(p95 <= BUDGET, "p95 {p95:?} is over the {BUDGET:?} budget");
}
