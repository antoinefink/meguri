//! R1846 / R1852 for a deck package import (`package/apkg-import` I18.3):
//! the generated collection of a profile, written as a latest-format
//! package without media, imported whole into a new empty collection.
//! Media copy is outside R1846's budget and outside this suite.
use criterion::Criterion;
use meguri_engine::{CancelToken, ProgressSink};
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::rng::SeededRandom;
use meguri_package::apkg::{self, ApkgContext, ImportOptions, UpdateCondition};
use meguri_package::{CollectionPackageWriteContext, Finalisation, write_collection_package_from_live};
use meguri_storage::ReaderRole;
use meguri_storage::atomic_file::Purpose;
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::bench::{
    fixtures,
    peak_alloc::PeakAlloc,
    report::{self, Measurement, Suite},
    timing::Percentile,
};
use meguri_testkit::generate::DEFAULT_NOW_MS;
use std::{process::ExitCode, sync::Arc, time::Duration};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

const OPTIONS: ImportOptions = ImportOptions {
    merge_notetypes: false,
    update_notes: UpdateCondition::IfNewer,
    update_notetypes: UpdateCondition::IfNewer,
    with_scheduling: true,
    with_deck_configs: true,
};

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("import");
    let sizes = std::env::var("MEGURI_BENCH_SIZES").unwrap_or_else(|_| "10k".into());
    let mut group = criterion.benchmark_group("import");
    group.sample_size(10);
    for size in sizes.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let fixture = fixtures::profile_collection(size).expect("prepared generated fixture");
        assert!(fixture.reused, "prepare fixtures before the benchmark process");
        let copy = fixture.working_copy().expect("isolated working copy");
        let scratch = tempfile::tempdir().expect("scratch directory");
        let clock = Arc::new(StepClock::new(DEFAULT_NOW_MS, UtcOffset::UTC, "UTC"));
        let random = Arc::new(SeededRandom::new(7));
        // The package: the whole generated collection, written once.
        let package = scratch.path().join("generated.apkg");
        {
            let source =
                fixtures::open_engine_collection_with_backups(&copy.path, &scratch.path().join("b0"), clock.clone())
                    .unwrap();
            let collection = source.collection().unwrap();
            let lease = collection.reader(ReaderRole::Maintenance).unwrap();
            write_collection_package_from_live(
                &lease,
                &package,
                &CollectionPackageWriteContext {
                    collection_directory: copy.path.parent().unwrap(),
                    purpose: Purpose::Export,
                    finalisation: Finalisation::Replace,
                    clock: clock.as_ref(),
                    random: random.as_ref(),
                    space: &StatvfsSpace,
                    is_cancelled: &|| false,
                },
            )
            .unwrap();
            drop(lease);
            source.close().unwrap();
        }
        let mut inspect = Measurement::start("package.apkg_import.inspect", Some(size));
        let mut apply = Measurement::start("package.apkg_import.apply", Some(size));
        let mut round = 0_u32;
        group.bench_function(format!("whole_collection/{size}"), |b| {
            b.iter_custom(|iters| {
                let mut total = Duration::ZERO;
                for _ in 0..iters {
                    // A new empty collection for every sample.
                    round += 1;
                    let root = scratch.path().join(format!("target-{round}"));
                    let directory = root.join("collection");
                    std::fs::create_dir_all(&directory).unwrap();
                    let path = directory.join("collection.anki2");
                    meguri_engine::bootstrap::create(&path, clock.as_ref(), random.as_ref()).unwrap();
                    let host =
                        fixtures::open_engine_collection_with_backups(&path, &root.join("backups"), clock.clone())
                            .unwrap();
                    let context = ApkgContext {
                        collection_directory: directory.clone(),
                        clock: clock.clone(),
                        random: random.clone(),
                        space: Arc::new(StatvfsSpace),
                    };
                    let mut session = None;
                    total += inspect.run(1, |_| {
                        session = Some(
                            apkg::inspect(&package, "generated.apkg", &context, &CancelToken::new(), &ProgressSink::new())
                                .expect("the generated package inspects"),
                        );
                    });
                    let session = session.unwrap();
                    let mut imported = 0;
                    total += apply.run(1, |_| {
                        imported = apkg::apply(
                            host.engine(),
                            &session,
                            OPTIONS,
                            None,
                            &CancelToken::new(),
                            &ProgressSink::new(),
                        )
                        .expect("the generated package imports")
                        .counts
                        .notes_added;
                    });
                    assert_eq!(imported, session.facts().notes);
                    drop(session);
                    host.close().unwrap();
                    std::fs::remove_dir_all(&root).unwrap();
                }
                total
            })
        });
        suite.push(inspect.finish());
        suite.push(apply.finish());
    }
    group.finish();
    criterion.final_summary();
    if let Err(error) = suite.write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")))) {
        eprintln!("import benchmark report: {error}");
        return ExitCode::FAILURE;
    }
    let mut status = ExitCode::SUCCESS;
    let mut whole = Duration::ZERO;
    for record in suite.records() {
        let p95 = record.percentile(Percentile::P95);
        eprintln!(
            "{} {}: p95 {p95:?}, max {:?}, peak {} bytes",
            record.name,
            record.fixture.as_deref().unwrap_or("-"),
            record.percentile(Percentile::Max),
            record.memory_peak_bytes
        );
        if record.fixture.as_deref() == Some("10k") {
            whole += p95;
            if record.memory_peak_bytes > 300 * 1024 * 1024 {
                eprintln!("import exceeds R1852's 300 MB");
                status = ExitCode::FAILURE;
            }
        }
    }
    if whole > Duration::from_secs(3) {
        eprintln!("import of the 10k profile exceeds R1846's 3 s");
        status = ExitCode::FAILURE;
    }
    status
}
