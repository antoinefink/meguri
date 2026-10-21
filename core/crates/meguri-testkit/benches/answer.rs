//! U12: actual production answer round trip, prediction and undo excluded.
//! Prepare fixtures in another process through tools/mg; default is 14,400 cards.
use criterion::Criterion;
use meguri_engine::{
    UndoScope,
    sched::answer::{self, AnswerCache},
};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::CardId,
    rng::SeededRandom,
};
use meguri_sched::{answer::Rating, states::fsrs_glue::FsrsCache};
use meguri_storage::{ReadRows, ReaderRole, Table, space::StatvfsSpace};
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
fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(20)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let mut suite = Suite::wall_clock("answer");
    for size in std::env::var("MEGURI_BENCH_SIZES")
        .unwrap_or_else(|_| "14400".into())
        .split(',')
    {
        let size = size.trim();
        if size.is_empty() {
            continue;
        }
        let fixture = fixtures::profile_collection(size).expect("prepared fixture");
        assert!(fixture.reused, "prepare fixtures before the measured process");
        let copy = fixture.working_copy().unwrap();
        let aux = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(aux.path().join("backups")).unwrap();
        let host = CoreHost::open(HostConfig::new(
            copy.path.parent().unwrap().into(),
            aux.path().join("backups"),
            aux.path().join("widget.snapshot"),
            Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(1)),
            Arc::new(StatvfsSpace),
        ))
        .unwrap();
        let collection = host.collection().unwrap();
        let ids = {
            let r = collection.reader(ReaderRole::Query).unwrap();
            r.ids_after(Table::Cards, None, 10000).unwrap()
        };
        let mut models = FsrsCache::default();
        let id = ids
            .into_iter()
            .map(CardId)
            .find(|id| answer::predict(&collection, *id, &mut models, None).is_ok())
            .expect("a schedulable fixture card");
        let cache = AnswerCache::build(&collection).unwrap();
        for _ in 0..20 {
            let p = answer::predict(&collection, id, &mut models, cache.clone()).unwrap();
            answer::answer(&collection, p.answer(Rating::Good, 4000)).unwrap();
            collection.undo(UndoScope::Global).unwrap();
        }
        let mut measurement = Measurement::start("sched.answer_commit", Some(size));
        criterion.bench_function(&format!("answer_commit/{size}"), |b| {
            b.iter_custom(|n| {
                let mut total = Duration::ZERO;
                for _ in 0..n {
                    let p = answer::predict(&collection, id, &mut models, cache.clone()).unwrap();
                    let request = p.answer(Rating::Good, 4000);
                    total += measurement.run(1, |_| {
                        let result = answer::answer(&collection, request.clone()).unwrap();
                        black_box(result)
                    });
                    // The history restores a real mutation; setup and undo never
                    // enter the duration samples or criterion's returned time.
                    collection.undo(UndoScope::Global).unwrap();
                }
                total
            })
        });
        suite.push(measurement.finish());
        host.close().unwrap();
    }
    criterion.final_summary();
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let p95 = record.percentile(Percentile::P95);
        eprintln!(
            "{} fixture {:?}: p95 {p95:?} (submission + production writer commit; prediction/undo excluded)",
            record.name, record.fixture
        );
        if record.fixture.as_deref() == Some("14400") && p95 > Duration::from_millis(5) {
            status = ExitCode::FAILURE;
        }
    }
    status
}
