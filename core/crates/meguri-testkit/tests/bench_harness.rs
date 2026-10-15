//! The benchmark harness (U12, R1864): per-operation samples in criterion's
//! `iter_custom`, percentiles that keep the slow tail, the memory sampler,
//! the counting allocator, the shared generated collections and the perf
//! result JSON (`tools/perf/perf-result.schema.json`).

use std::hint::black_box;
use std::time::Duration;

use criterion::Criterion;
use meguri_testkit::bench::fixtures;
use meguri_testkit::bench::memory::{self, MemorySampler};
use meguri_testkit::bench::peak_alloc::PeakAlloc;
use meguri_testkit::bench::report::{Measurement, Suite};
use meguri_testkit::bench::timing::{OpSamples, Percentile};
use serde_json::Value;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

/// A criterion short enough for a test: a tenth of a second of warm-up,
/// half a second of measurement.
fn quick_criterion() -> Criterion {
    Criterion::default()
        .sample_size(10)
        .warm_up_time(Duration::from_millis(100))
        .measurement_time(Duration::from_millis(500))
}

fn micros(n: u64) -> Duration {
    Duration::from_micros(n)
}

fn ms(n: u64) -> Duration {
    Duration::from_millis(n)
}

/// HDR keeps three significant digits: a recorded value reads back within
/// 0.1% above it.
fn about(read: Duration, recorded: Duration) -> bool {
    read >= recorded && read.as_nanos() <= recorded.as_nanos() * 1001 / 1000
}

#[test]
fn one_slow_sample_in_a_hundred_is_the_p99() {
    let mut samples = OpSamples::new();
    for i in 0..100 {
        samples.record(if i == 37 { ms(20) } else { micros(100) });
    }
    assert_eq!(samples.count(), 100);
    assert!(about(samples.percentile(Percentile::P99), Duration::from_millis(20)));
    assert!(about(samples.percentile(Percentile::Max), Duration::from_millis(20)));
    assert!(about(samples.percentile(Percentile::P95), micros(100)));
    assert!(about(samples.percentile(Percentile::P50), micros(100)));

    // Ten in a thousand: still the p99; nine in a thousand: no longer.
    let mut ten = OpSamples::new();
    let mut nine = OpSamples::new();
    for i in 0..1000 {
        let slow = i % 100 == 0;
        ten.record(if slow { ms(20) } else { micros(100) });
        nine.record(if slow && i > 0 { ms(20) } else { micros(100) });
    }
    assert!(about(ten.percentile(Percentile::P99), Duration::from_millis(20)));
    assert!(about(nine.percentile(Percentile::P99), micros(100)));
}

#[test]
fn a_criterion_bench_with_one_20_ms_stall_in_100_operations_reports_it_at_p99() {
    let mut criterion = quick_criterion();
    let mut samples = OpSamples::new();
    let mut op = 0u64;
    criterion.bench_function("stall", |b| {
        b.iter_custom(|iters| {
            samples.run(iters, |_| {
                let stall = op.is_multiple_of(100);
                op += 1;
                if stall {
                    std::thread::sleep(Duration::from_millis(20));
                }
                black_box(op.wrapping_mul(31))
            })
        })
    });
    assert!(samples.count() >= 100, "criterion ran {} operations", samples.count());
    assert_eq!(samples.count(), op, "every operation is one sample");
    let p99 = samples.percentile(Percentile::P99);
    assert!(p99 >= Duration::from_millis(20), "p99 {p99:?} misses the stall");
    let p50 = samples.percentile(Percentile::P50);
    assert!(p50 < Duration::from_millis(1), "p50 {p50:?}");
    // The mean of a batch of 100 would be about 0.2 ms: per-operation
    // samples are what make the stall visible.
    let mean = samples.total() / u32::try_from(samples.count()).unwrap();
    assert!(mean < Duration::from_millis(1), "mean {mean:?}");
}

#[test]
fn the_counting_allocator_reports_the_peak_since_its_reset() {
    PeakAlloc::reset_peak();
    let before = PeakAlloc::current();
    let block = vec![7u8; 8 << 20];
    black_box(&block);
    let peak = PeakAlloc::peak();
    drop(block);
    assert!(PeakAlloc::is_installed());
    assert!(peak >= before + (8 << 20), "peak {peak}, before {before}");
    assert!(PeakAlloc::current() < before + (8 << 20));
    PeakAlloc::reset_peak();
    assert!(PeakAlloc::peak() < peak);
}

#[test]
fn the_memory_sampler_sees_touched_pages_while_they_live() {
    let base = memory::process_footprint_bytes().expect("this platform reports a footprint");
    let sampler = MemorySampler::start(Duration::from_millis(2));
    let mut block = vec![0u8; 64 << 20];
    for page in block.iter_mut().step_by(4096) {
        *page = 1;
    }
    black_box(&block);
    std::thread::sleep(Duration::from_millis(50));
    drop(block);
    let peak = sampler.stop();
    assert!(peak >= base + (60 << 20), "peak {peak}, base {base}");
}

fn rows<'a>(doc: &'a Value, name: &str, metric: &str) -> Vec<&'a Value> {
    doc["results"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|r| r["name"] == name && r["metric"] == metric)
        .collect()
}

#[test]
fn an_open_bench_on_the_10k_collection_reports_percentiles_and_peak_memory() {
    let root = tempfile::tempdir().unwrap();
    let collection = fixtures::profile_collection_in(root.path(), "10k").unwrap();
    assert!(!collection.reused);
    assert_eq!(collection.manifest.cards_requested, 10_000);
    let again = fixtures::profile_collection_in(root.path(), "10k").unwrap();
    assert!(again.reused, "the second request reuses the generated collection");
    assert_eq!(again.manifest, collection.manifest);

    let copy = collection.working_copy().unwrap();
    let mut criterion = quick_criterion();
    let mut measurement = Measurement::start("collection.open", Some("10k"));
    criterion.bench_function("open/10k", |b| {
        b.iter_custom(|iters| measurement.run(iters, |_| fixtures::open(&copy.path).unwrap()))
    });
    let mut suite = Suite::wall_clock("open");
    suite.push(measurement.finish());

    let out = tempfile::tempdir().unwrap();
    let path = suite.write(out.path()).unwrap();
    assert_eq!(path, out.path().join("open.json"));
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
    assert_eq!(doc["schema_version"], 1);
    assert_eq!(doc["suite"], "open");
    assert_eq!(doc["kind"], "wall-clock");
    assert_eq!(doc["configuration"], "bench");
    assert_eq!(doc["runner"]["memory_measure"], memory::MEASURE);

    let durations = rows(&doc, "collection.open", "duration");
    let percentiles: Vec<&str> = durations.iter().map(|r| r["percentile"].as_str().unwrap()).collect();
    assert_eq!(percentiles, ["p50", "p95", "p99", "max"]);
    let mut previous = 0.0;
    for row in &durations {
        assert_eq!(row["unit"], "ms");
        assert_eq!(row["fixture"], "10k");
        assert!(row["samples"].as_u64().unwrap() >= 10);
        let value = row["value"].as_f64().unwrap();
        assert!(value > 0.0 && value >= previous, "{row}");
        previous = value;
    }

    let memory = rows(&doc, "collection.open", "memory");
    assert_eq!(memory.len(), 1);
    let memory = memory[0];
    assert_eq!(memory["unit"], "MB");
    assert_eq!(memory["percentile"], "max");
    assert!(memory["value"].as_f64().unwrap() > 1.0, "{memory}");
    let breakdown = memory["breakdown"].as_object().unwrap();
    assert!(breakdown["rust_alloc_peak"].as_f64().unwrap() > 0.0);
    assert!(breakdown["sqlite_highwater"].as_f64().unwrap() > 0.0);
}
