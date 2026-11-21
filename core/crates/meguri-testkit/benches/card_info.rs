//! U92: Card Info's read on a working copy of a generated collection,
//! through meguri-stats' entry point (`stats/card-info` C1: 500 review-log
//! rows within R1841's 100 ms; the Browse and Note Types plan's section 3).
//!
//! - `card_info.most_rows`: the card with the most review-log rows.
//! - `card_info.rows_500`: that card filled to 500 rows, Review answers
//!   one day apart after its last row, before the host opens.
//! - `card_info.debug_json`: the 500-row card's snapshot and its dump.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

use std::hint::black_box;
use std::process::ExitCode;
use std::sync::Arc;
use std::time::Duration;

use criterion::Criterion;
use meguri_host::{CoreHost, HostConfig};
use meguri_model::clock::{FixedClock, UtcOffset};
use meguri_model::ids::CardId;
use meguri_model::rng::SeededRandom;
use meguri_stats::CardInfoOutcome;
use meguri_storage::raw::SqlValue;
use meguri_storage::space::StatvfsSpace;
use meguri_storage::{OpenOptions, open_collection};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::bench::timing::Percentile;
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const ROWS: i64 = 500;

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn integer(value: &SqlValue) -> i64 {
    match value {
        SqlValue::Integer(value) => *value,
        other => panic!("expected an integer, got {other:?}"),
    }
}

/// The card with the most rows and its count; then that card filled to
/// `ROWS` rows, written on the copy before the host opens it.
fn prepare(path: &std::path::Path) -> (CardId, i64) {
    let clock = FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC);
    let random = SeededRandom::new(1);
    let opened = open_collection(path, &OpenOptions::new(&clock, &random)).unwrap();
    let writer = opened.writer();
    let top = writer
        .execute_raw(
            "SELECT cid, count(*), max(id) FROM revlog GROUP BY cid ORDER BY 2 DESC, 1 LIMIT 1",
            &[],
        )
        .unwrap();
    let row = &top.rows[0];
    let (card, count, last) = (integer(&row[0]), integer(&row[1]), integer(&row[2]));
    for k in 1..=(ROWS - count).max(0) {
        writer
            .execute_raw(
                "INSERT INTO revlog (id, cid, usn, ease, ivl, lastIvl, factor, time, type) VALUES (?1, ?2, -1, 3, ?3, ?3, 2500, 3000, 1)",
                &[
                    SqlValue::Integer(last + k * 86_400_000),
                    SqlValue::Integer(card),
                    SqlValue::Integer(k),
                ],
            )
            .unwrap();
    }
    (CardId(card), count)
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("card-info");
    let mut row_counts = Vec::new();
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        // The natural card is measured on a second copy left as generated.
        let natural = fixture.working_copy().unwrap();
        let (card, natural_rows) = {
            let clock = FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC);
            let random = SeededRandom::new(1);
            let opened = open_collection(&natural.path, &OpenOptions::new(&clock, &random)).unwrap();
            let top = opened
                .writer()
                .execute_raw("SELECT cid, count(*) FROM revlog GROUP BY cid ORDER BY 2 DESC, 1 LIMIT 1", &[])
                .unwrap();
            (CardId(integer(&top.rows[0][0])), integer(&top.rows[0][1]))
        };
        let (filled, _) = prepare(&copy.path);
        assert_eq!(filled, card);
        row_counts.push((size.clone(), natural_rows));
        for (path, cases) in [
            (&natural.path, &["card_info.most_rows"][..]),
            (&copy.path, &["card_info.rows_500", "card_info.debug_json"][..]),
        ] {
            let auxiliary = tempfile::tempdir().unwrap();
            std::fs::create_dir_all(auxiliary.path().join("backups")).unwrap();
            let host = CoreHost::open(HostConfig::new(
                path.parent().unwrap().into(),
                auxiliary.path().join("backups"),
                auxiliary.path().join("app-group/widget.snapshot"),
                Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
                Arc::new(SeededRandom::new(1)),
                Arc::new(StatvfsSpace),
            ))
            .unwrap();
            let collection = host.collection().unwrap();
            for name in cases {
                let op = || match meguri_stats::card_info(&collection, card).unwrap() {
                    CardInfoOutcome::Found(info) => {
                        if *name == "card_info.debug_json" {
                            black_box(info.debug_json());
                        }
                        black_box(info);
                    }
                    CardInfoOutcome::Unavailable(kind) => panic!("card info unavailable: {kind:?}"),
                };
                // Warm the pages and the statements outside the samples.
                op();
                let mut measurement = Measurement::start(name, Some(&size));
                criterion.bench_function(&format!("{name}/{size}"), |b| b.iter_custom(|n| measurement.run(n, |_| op())));
                if measurement.samples().count() > 0 {
                    suite.push(measurement.finish());
                }
            }
            drop(collection);
            host.close().unwrap();
        }
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    for record in suite.records() {
        let natural = row_counts
            .iter()
            .find(|(size, _)| Some(size.as_str()) == record.fixture.as_deref())
            .map_or(0, |(_, rows)| *rows);
        eprintln!(
            "{} fixture {:?}: p95 {:?} (most rows of one card: {natural}; filled to {ROWS})",
            record.name,
            record.fixture,
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
