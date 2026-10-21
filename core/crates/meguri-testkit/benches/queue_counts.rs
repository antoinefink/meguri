//! U41/U12. Use the harness's pre-generated MEGURI_BENCH_SIZES fixtures
//! (default 10k,150k); --sizes 14400,150k includes the required queue size.
//! Generation in this process would corrupt RSS measurements.
use criterion::Criterion;
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::DeckId,
    rng::SeededRandom,
};
use meguri_sched::states::fsrs_glue::FsrsCache;
use meguri_storage::{ReadRows, ReaderRole, space::StatvfsSpace};
use meguri_testkit::{
    bench::{
        fixtures,
        peak_alloc::PeakAlloc,
        report::{self, Measurement, Suite},
        timing::Percentile,
    },
    generate::DEFAULT_NOW_MS,
};
use std::{hint::black_box, process::ExitCode, sync::Arc, time::Duration};
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn sizes() -> Vec<String> {
    std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "10k,150k".to_owned())
        .split(',')
        .map(|s| s.trim().to_ascii_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("queue-counts");
    for size in sizes() {
        let fixture = fixtures::profile_collection(&size).expect("pre-generated fixture");
        assert!(
            fixture.reused,
            "prepare this fixture through tools/mg fixtures before starting the benchmark process"
        );
        let copy = fixture.working_copy().unwrap();
        let directory = copy.path.parent().unwrap();
        let auxiliary = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(auxiliary.path().join("backups")).unwrap();
        let host = CoreHost::open(HostConfig::new(
            directory.into(),
            auxiliary.path().join("backups"),
            auxiliary.path().join("app-group/widget.snapshot"),
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
            Arc::new(StatvfsSpace),
        ))
        .unwrap();
        let c = host.collection().unwrap();
        {
            // Choose the largest existing normal study subtree, without
            // rewriting the original generated collection's topology/cards.
            let decks = {
                let r = c.reader(ReaderRole::Query).unwrap();
                r.deck_rows().unwrap()
            };
            let deck = decks
                .into_iter()
                .filter(|d| matches!(d.kind.kind, meguri_model::proto::deck::DeckKind::Normal(_)))
                .map(|d| {
                    let count = host.queues().queue(&c, Some(d.id)).unwrap().value.presentation().len();
                    (count, d.id)
                })
                .max_by_key(|(n, _)| *n)
                .map_or(DeckId(1), |(_, id)| id);
            let mut cache = FsrsCache::default();
            let warm = host
                .queues()
                .queue_with_head_states(&c, Some(deck), &mut cache)
                .unwrap();
            assert!(warm.1.is_some(), "benchmark must exercise a real U45 head");
            let mut measurement = Measurement::start("sched.queue_build_and_head_states", Some(&size));
            criterion.bench_function(&format!("queue_and_head/{size}"), |b| {
                b.iter_custom(|n| {
                    measurement.run(n, |_| {
                        black_box(
                            host.queues()
                                .queue_with_head_states(&c, Some(deck), &mut cache)
                                .unwrap(),
                        )
                    })
                })
            });
            suite.push(measurement.finish());
        }
        {
            // Use the same host provider's explicit grouped refresh, keeping
            // optional cache hits out of the measured workload.
            black_box(host.queues().refresh_deck_counts(&c).unwrap());
            let mut measurement = Measurement::start("sched.deck_tree_refresh", Some(&size));
            criterion.bench_function(&format!("deck_counts/{size}"), |b| {
                b.iter_custom(|n| measurement.run(n, |_| black_box(host.queues().refresh_deck_counts(&c).unwrap())))
            });
            suite.push(measurement.finish());
        }
        host.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    for record in suite.records() {
        eprintln!(
            "{} fixture {} p95 {:?}",
            record.name,
            record.fixture.as_deref().unwrap_or("none"),
            record.percentile(Percentile::P95)
        );
    }
    ExitCode::SUCCESS
}
