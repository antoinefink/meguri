//! U289: `stats_history` following commits (`stats/history-reads` H14.3,
//! H19: under 5 ms for a change set of up to 10 cards, under 100 ms after a
//! sync of 1,000 rows). Each sample is the commit and its wait for the
//! derived writer, so it holds the op's own write and the capture on the
//! collection writer too: an upper bound of the update's cost.
//!
//! - `stats_history_apply.answer_or_undo`: an answer (one row, the card's
//!   new interval), then its undo (the row removed, the card back), in
//!   turn.
//! - `stats_history_apply.sync_1000`: a `SyncIncoming` commit of 1,000 rows
//!   on 500 cards, the next 500 of the collection's review cards each time;
//!   the rows are removed again outside the timed part, so every sample
//!   starts from the same histories.
//!
//! Sizes come from `$MEGURI_BENCH_SIZES` (default `10k,150k`), made first by
//! `tools/mg bench`.

#[path = "support/stats_history.rs"]
mod support;

use std::process::ExitCode;
use std::time::Duration;

use criterion::Criterion;
use meguri_engine::OpSpec;
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::usn::Usn;
use meguri_stats::graphs::{History, Scope};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole, StampMode};
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{self, Measurement, Suite};
use meguri_testkit::generate::DEFAULT_NOW_MS;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn entry(id: i64, card: i64, ivl: i32) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(id),
        card_id: CardId(card),
        usn: Usn(-1),
        button: 3,
        interval: ivl,
        last_interval: 1,
        ease_factor: 2500,
        answer_millis: 5_000,
        kind: RevlogKind::Review,
    }
}

fn card_ids(host: &meguri_host::CoreHost, limit: usize) -> Vec<i64> {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let snapshot = lease.snapshot().unwrap();
    snapshot
        .execute_raw(&format!("SELECT id FROM cards WHERE queue = 2 ORDER BY id LIMIT {limit}"), &[])
        .unwrap()
        .rows
        .into_iter()
        .map(|row| match row[0] {
            SqlValue::Integer(id) => id,
            ref other => panic!("{other:?}"),
        })
        .collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(100)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(10))
        .configure_from_args();
    let mut suite = Suite::wall_clock("stats-history-apply");
    for size in support::sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        let host = support::open_host(&copy.path, auxiliary.path());
        support::read(&host, &Scope::Collection, History::AllHistory);
        let cards = card_ids(&host, 200_000);
        let collection = host.collection().unwrap();
        // Rows after every row the fixture holds, so no id repeats.
        let mut next = DEFAULT_NOW_MS + 3_600_000;
        let mut answered: Option<(i64, i64, u32)> = None;
        let mut turn = 0usize;
        let name = "stats_history_apply.answer_or_undo";
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run(n, |_| {
                    match answered.take() {
                        None => {
                            turn += 1;
                            next += 1;
                            let (card, row) = (cards[turn % cards.len()], next);
                            let before = collection
                                .run(OpSpec::named("answer", OpSource::UserUndoable), move |ctx| {
                                    let mut c = ReadRows::card(&*ctx.scope(), CardId(card))?.unwrap();
                                    let before = c.interval;
                                    c.interval = before + 7;
                                    ctx.scope().put(&c)?;
                                    ctx.scope().insert_revlog(&entry(row, card, (before + 7) as i32))?;
                                    Ok(before)
                                })
                                .unwrap()
                                .value;
                            answered = Some((card, row, before));
                        }
                        Some((card, row, before)) => {
                            collection
                                .run(OpSpec::named("set_due_date", OpSource::UserUndoable), move |ctx| {
                                    ctx.scope().remove_revlog(RevlogId(row))?;
                                    let mut c = ReadRows::card(&*ctx.scope(), CardId(card))?.unwrap();
                                    c.interval = before;
                                    ctx.scope().put(&c)?;
                                    Ok(())
                                })
                                .unwrap();
                        }
                    }
                    host.derived().flush().unwrap();
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
        let name = "stats_history_apply.sync_1000";
        let mut measurement = Measurement::start(name, Some(&size));
        criterion.bench_function(&format!("{name}/{size}"), |b| {
            b.iter_custom(|n| {
                measurement.run_timed(n, |_| {
                    let first = next + 1;
                    next += 1_000;
                    let from = turn * 500;
                    turn += 1;
                    let cards: Vec<i64> = (0..500).map(|at| cards[(from + at) % cards.len()]).collect();
                    #[allow(clippy::disallowed_methods)]
                    let started = std::time::Instant::now();
                    collection
                        .run(
                            OpSpec::named("sync_incoming", OpSource::SyncIncoming).stamp(StampMode::Raw),
                            move |ctx| {
                                for at in 0..1_000i64 {
                                    let card = cards[usize::try_from(at).unwrap() % cards.len()];
                                    ctx.scope().insert_revlog(&entry(first + at, card, 12))?;
                                }
                                Ok(())
                            },
                        )
                        .unwrap();
                    host.derived().flush().unwrap();
                    let took = started.elapsed();
                    collection
                        .run(
                            OpSpec::named("sync_incoming", OpSource::SyncIncoming).stamp(StampMode::Raw),
                            move |ctx| {
                                for at in 0..1_000i64 {
                                    ctx.scope().remove_revlog(RevlogId(first + at))?;
                                }
                                Ok(())
                            },
                        )
                        .unwrap();
                    host.derived().flush().unwrap();
                    took
                })
            })
        });
        if measurement.samples().count() > 0 {
            suite.push(measurement.finish());
        }
        // Still current, and served by the component.
        support::read(&host, &Scope::Collection, History::AllHistory);
        drop(collection);
        host.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    support::print(&suite);
    ExitCode::SUCCESS
}
