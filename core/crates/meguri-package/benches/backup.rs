//! B8 / R1848 / R1852: production streamed backup plus retention, U12 harness.
//! MG discovers this Cargo target; fixtures are prepared by the existing job.
use criterion::Criterion;
use meguri_engine::{
    CancelToken, backup_limits, browser_config, device_state::DeviceState, maintenance::MaintenanceTrigger,
};
use meguri_model::clock::{Clock, StepClock, UtcOffset};
use meguri_package::backup::naming;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::bench::{
    fixtures,
    peak_alloc::PeakAlloc,
    report::{self, Measurement, Suite},
    timing::Percentile,
};
use meguri_testkit::generate::DEFAULT_NOW_MS;
use std::{fs, process::ExitCode, sync::Arc, time::Duration};

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

// Temporary generated-only Quit diagnosis. Remove with the two writer probes
// after interpreting the unchanged complete-call measurements.
mod temporary_quit_timing {
    use meguri_model::faultpoint::{self, Fault};
    use std::{
        sync::{
            OnceLock,
            atomic::{AtomicBool, AtomicU64, Ordering},
        },
        time::{Duration, Instant},
    };

    static ORIGIN: OnceLock<Instant> = OnceLock::new();
    static ACTIVE: AtomicBool = AtomicBool::new(false);
    static START: AtomicU64 = AtomicU64::new(0);
    static STAGES: [AtomicU64; 6] = [const { AtomicU64::new(0) }; 6];
    const HOOKS: [(&str, fn()); 6] = [
        ("package.backup.before_snapshot_live", || record(0)),
        ("package.backup.after_snapshot_live", || record(1)),
        ("storage.atomic.after_write", || record(2)),
        ("storage.atomic.after_sync", || record(3)),
        ("storage.atomic.after_verify", || record(4)),
        ("storage.backup.before_rename", || record(5)),
    ];

    fn now_ns() -> u64 {
        u64::try_from(ORIGIN.get().unwrap().elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    fn record(stage: usize) {
        if !ACTIVE.load(Ordering::Relaxed) {
            return;
        }
        // Later AtomicFile callbacks can belong to device-state metadata.
        if (2..=4).contains(&stage) && STAGES[5].load(Ordering::Relaxed) != 0 {
            return;
        }
        let elapsed = now_ns().saturating_sub(START.load(Ordering::Relaxed));
        let _ = STAGES[stage].compare_exchange(0, elapsed.saturating_add(1), Ordering::Relaxed, Ordering::Relaxed);
    }

    pub fn arm() {
        ORIGIN.get_or_init(Instant::now);
        for stage in &STAGES {
            stage.store(0, Ordering::Relaxed);
        }
        for (name, callback) in HOOKS {
            faultpoint::arm(name, Fault::Callback(callback));
        }
    }

    pub fn begin() {
        START.store(now_ns(), Ordering::Relaxed);
        ACTIVE.store(true, Ordering::Relaxed);
    }

    pub fn end() {
        ACTIVE.store(false, Ordering::Relaxed);
    }

    pub fn disarm() {
        for (name, _) in HOOKS {
            faultpoint::disarm(name);
        }
    }

    pub fn stages() -> [Option<Duration>; 6] {
        std::array::from_fn(|index| {
            STAGES[index]
                .load(Ordering::Relaxed)
                .checked_sub(1)
                .map(Duration::from_nanos)
        })
    }
}

fn main() -> ExitCode {
    let mut criterion = Criterion::default().configure_from_args();
    let mut suite = Suite::wall_clock("backup");
    let sizes = std::env::var("MEGURI_BENCH_SIZES").unwrap_or_else(|_| "150k".into());
    let mut group = criterion.benchmark_group("backup");
    group.sample_size(10);
    for size in sizes.split(',').map(str::trim).filter(|s| !s.is_empty()) {
        let fixture = fixtures::profile_collection(size).expect("prepared generated fixture");
        assert!(fixture.reused, "prepare fixtures before the benchmark process");
        let copy = fixture.working_copy().expect("isolated working copy");
        let backup_directory = tempfile::tempdir().expect("isolated backup directory");
        let backups = backup_directory.path().join("backups");
        fs::create_dir(&backups).unwrap();
        // The full hourly history is thinned once before measurement. Subsequent
        // samples advance one day, keeping a bounded, realistic retained set.
        let wall_now = DEFAULT_NOW_MS.div_euclid(1000);
        for hour in 1..=400 * 24 {
            let name = naming::name_from_wall_seconds(wall_now - hour * 3600).unwrap();
            fs::write(backups.join(name), []).unwrap();
        }
        let clock = Arc::new(StepClock::new(DEFAULT_NOW_MS, UtcOffset::UTC, "UTC"));
        let host = fixtures::open_engine_collection_with_backups(&copy.path, &backups, clock.clone()).unwrap();
        let collection = host.collection().unwrap();
        let provider = host.backup();
        provider
            .before_colpkg_import(&collection, &CancelToken::default())
            .expect("warm streamed backup and retention");
        let mut measurement = Measurement::start("package.backup", Some(size));
        group.bench_function(format!("streamed_with_retention/{size}"), |b| {
            b.iter_custom(|iters| {
                measurement.run(iters, |_| {
                    clock.step_ms(86_400_000);
                    provider
                        .before_colpkg_import(&collection, &CancelToken::default())
                        .expect("durable backup with retention")
                })
            })
        });
        suite.push(measurement.finish());
        if size == "150k" {
            // Use the public settings operation so OS entry times cannot defer
            // these Quit backups. Keep the real writer and retention limits.
            let mut limits = backup_limits::read(&*collection.reader(ReaderRole::Query).unwrap()).unwrap();
            limits.minimum_interval_mins = 0;
            backup_limits::save(&collection, limits).unwrap();
            let state = DeviceState::for_collection(collection.path());
            let mut quit = Measurement::start("package.backup.quit", Some(size));
            group.bench_function(format!("quit_with_pending_change/{size}"), |b| {
                b.iter_custom(|iters| {
                    let mut total = Duration::ZERO;
                    for _ in 0..iters {
                        // Prepare a real committed change before each sample,
                        // including Criterion warm-up. Never write col.mod.
                        let previous = state.last_backup().unwrap().unwrap();
                        clock.step_ms(86_400_000);
                        let search = format!("deck:quit-benchmark-{}", clock.now_ms());
                        assert!(
                            browser_config::set_default_search_text(&collection, &search)
                                .unwrap()
                                .value
                        );
                        let modified = collection.reader(ReaderRole::Query).unwrap().col().unwrap().modified;
                        assert_ne!(modified.0, previous.snapshot_mod);
                        let mut outcome = None;
                        temporary_quit_timing::arm();
                        let elapsed = quit.run(1, |_| {
                            temporary_quit_timing::begin();
                            outcome = Some(host.engine().run_due_maintenance(MaintenanceTrigger::Quit));
                            temporary_quit_timing::end();
                        });
                        total += elapsed;
                        temporary_quit_timing::disarm();
                        // Read publication before any existing outcome assertion
                        // can panic. Emit generated facts only, never names/errors.
                        let publication = state.last_backup();
                        let published = publication.as_ref().ok().and_then(Option::as_ref);
                        let expected = (modified.0, clock.now_ms());
                        eprintln!(
                            "temporary generated Quit diagnostic: elapsed={elapsed:?} stages_order=\
                             [before_snapshot_live,after_snapshot_live,after_write,after_sync,after_verify,before_rename] \
                             stages={:?} call_ok={} returned_deadline={:?} previous_mod_instant={:?} \
                             published_mod_instant={:?} expected_mod_instant={expected:?} \
                             publication_read_ok={} durable_name_changed={:?} \
                             snapshot_record_matches={:?} published_file_exists={:?}",
                            temporary_quit_timing::stages(),
                            outcome.as_ref().is_some_and(Result::is_ok),
                            outcome.as_ref().and_then(|result| result.as_ref().ok()),
                            (previous.snapshot_mod, previous.instant),
                            published.map(|record| (record.snapshot_mod, record.instant)),
                            publication.is_ok(),
                            published.map(|record| record.name != previous.name),
                            published.map(|record| (record.snapshot_mod, record.instant) == expected),
                            published.map(|record| backups.join(&record.name).exists()),
                        );
                        // Quit may report writer failure through logging. Prove
                        // publication as well as a successful entry-point return.
                        assert_eq!(outcome.unwrap().unwrap(), None);
                        let published = publication.unwrap().unwrap();
                        assert_ne!(published.name, previous.name);
                        assert_eq!(published.snapshot_mod, modified.0);
                        assert_eq!(published.instant, clock.now_ms());
                        let path = backups.join(&published.name);
                        assert!(fs::symlink_metadata(path).unwrap().is_file());
                        assert!(fs::metadata(backups.join(&published.name)).unwrap().len() > 0);
                    }
                    total
                })
            });
            suite.push(quit.finish());
        }
        host.close().unwrap();
    }
    group.finish();
    criterion.final_summary();
    if let Err(error) = suite.write(&report::results_dir(std::path::Path::new(env!("CARGO_TARGET_TMPDIR")))) {
        eprintln!("backup benchmark report: {error}");
        return ExitCode::FAILURE;
    }
    let mut status = ExitCode::SUCCESS;
    for record in suite.records() {
        let maximum = record.percentile(Percentile::Max);
        eprintln!(
            "{} {}: max {maximum:?}, peak {} bytes",
            record.name,
            record.fixture.as_deref().unwrap_or("-"),
            record.memory_peak_bytes
        );
        if record.fixture.as_deref() == Some("150k") && maximum > Duration::from_secs(2) {
            eprintln!("backup exceeds the existing 150k <=2s contract");
            status = ExitCode::FAILURE;
        }
        if record.fixture.as_deref() == Some("500k") && record.memory_peak_bytes > 300 * 1024 * 1024 {
            eprintln!("backup exceeds R1852's 300 MB footprint");
            status = ExitCode::FAILURE;
        }
    }
    status
}
