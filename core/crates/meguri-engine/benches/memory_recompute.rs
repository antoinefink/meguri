//! U46/R1849: full recompute and commit, every card processed. The shared
//! generated fixture is copied, then new/no-history cards receive a graded
//! SM-2 row so the 100k budget measures 100k processed cards. Undo restores
//! the exact starting workload outside each timed operation.
use criterion::Criterion;
use meguri_engine::{
    Engine, EngineConfig, OpenConfig, UndoScope,
    sched::memory::{Target, recompute},
    tags::RunOptions,
};
use meguri_model::{
    changes::OpSource,
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_testkit::{
    bench::{
        fixtures,
        peak_alloc::PeakAlloc,
        report::{self, Measurement, Suite},
        timing::Percentile,
    },
    generate::DEFAULT_NOW_MS,
};
use std::{process::ExitCode, sync::Arc, time::Duration};
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn main() -> ExitCode {
    let mut criterion = Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_secs(1))
        .measurement_time(Duration::from_secs(3))
        .configure_from_args();
    let fixture = fixtures::profile_collection("100k").expect("100k fixture");
    let copy = fixture.working_copy().expect("private working copy");
    let opened = fixtures::open(&copy.path).expect("storage open");
    // Preparation only. The measured operation uses product typed reads,
    // the ordinary writer journal, change stream and undo machinery.
    opened
        .writer()
        .execute_raw("UPDATE cards SET type=2,queue=2,data='{}'", &[])
        .unwrap();
    opened.writer().execute_raw("INSERT INTO revlog(id,cid,usn,ease,ivl,lastIvl,factor,time,type) SELECT ?1+row_number() OVER(ORDER BY c.id),c.id,-1,3,20,10,2500,1000,1 FROM cards c WHERE NOT EXISTS(SELECT 1 FROM revlog r WHERE r.cid=c.id)",
        &[meguri_storage::raw::SqlValue::Integer(DEFAULT_NOW_MS)]).unwrap();
    opened.close().unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let collection = engine
        .open_raw(
            &copy.path,
            OpenConfig::new(
                Arc::new(FixedClock::new(DEFAULT_NOW_MS, UtcOffset::UTC)),
                Arc::new(SeededRandom::new(1)),
            ),
        )
        .unwrap();
    let mut measurement = Measurement::start("fsrs.recomputeMemoryStates", Some("100k"));
    criterion.bench_function("memory_recompute/100k", |bench| {
        bench.iter_custom(|iterations| {
            let mut total = Duration::ZERO;
            for _ in 0..iterations {
                total += measurement.run(1, |_| {
                    let result = recompute(
                        &collection,
                        Target::All,
                        OpSource::UserUndoable,
                        "Deck Options",
                        &RunOptions::default(),
                    )
                    .unwrap();
                    assert_eq!(result.value.processed, 100_000);
                    assert_eq!(result.value.changed, 100_000);
                });
                collection.undo(UndoScope::Global).unwrap();
            }
            total
        })
    });
    engine.close().unwrap();
    criterion.final_summary();
    let mut suite = Suite::wall_clock("memory-states");
    suite.push(measurement.finish());
    suite
        .write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR"))))
        .unwrap();
    let p95 = suite.records()[0].percentile(Percentile::P95);
    eprintln!("memory recompute100k p95 {p95:?}");
    if cfg!(target_vendor = "apple") && p95 > Duration::from_secs(3) {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}
