//! The perf result JSON of a wall-clock suite
//! (`tools/perf/perf-result.schema.json`, U241's gate input).
//!
//! A suite measures each operation with a [`Measurement`]: per-operation
//! samples ([`OpSamples`]) plus the process memory sampled while it runs.
//! Each record becomes one `duration` row per reported percentile (ms) and
//! one `memory` row (the peak, MB = 2^20 bytes), whose breakdown holds the
//! counting allocator's peak and SQLite's high-water mark. Those two are
//! never the memory number: SQLite allocates outside Rust's allocator.

use std::path::{Path, PathBuf};
use std::time::Duration;

use serde_json::{Value, json};

use super::memory::{self, DEFAULT_INTERVAL, MemorySampler};
use super::peak_alloc::PeakAlloc;
use super::timing::{OpSamples, Percentile};

const MIB: f64 = 1024.0 * 1024.0;

/// SQLite's allocation high-water mark in bytes; `reset` starts a new one.
fn sqlite_highwater(reset: bool) -> u64 {
    // SAFETY: a plain read (and optional reset) of SQLite's global memory
    // statistics; it takes no pointer.
    let bytes = unsafe { meguri_storage::rusqlite::ffi::sqlite3_memory_highwater(i32::from(reset)) };
    u64::try_from(bytes).unwrap_or(0)
}

/// One operation being measured.
#[derive(Debug)]
pub struct Measurement {
    name: String,
    fixture: Option<String>,
    samples: OpSamples,
    sampler: MemorySampler,
}

impl Measurement {
    /// Starts measuring `name` (on `fixture`): resets the allocator's peak
    /// and SQLite's high-water mark and starts the memory sampler.
    pub fn start(name: &str, fixture: Option<&str>) -> Measurement {
        PeakAlloc::reset_peak();
        sqlite_highwater(true);
        Measurement {
            name: name.to_owned(),
            fixture: fixture.map(str::to_owned),
            samples: OpSamples::new(),
            sampler: MemorySampler::start(DEFAULT_INTERVAL),
        }
    }

    /// [`OpSamples::run`]: hand this to criterion's `iter_custom`.
    pub fn run<T>(&mut self, iters: u64, op: impl FnMut(u64) -> T) -> Duration {
        self.samples.run(iters, op)
    }

    /// Like [`Measurement::run`] for an operation whose measured part ends
    /// before the call returns (a search's first page, before the rest of
    /// its events are drained): `op` times that part itself and returns it.
    pub fn run_timed(&mut self, iters: u64, mut op: impl FnMut(u64) -> Duration) -> Duration {
        let mut batch = Duration::ZERO;
        for i in 0..iters {
            let elapsed = op(i);
            self.samples.record(elapsed);
            batch += elapsed;
        }
        batch
    }

    pub fn samples(&self) -> &OpSamples {
        &self.samples
    }

    /// Stops the sampler and reads the peaks.
    pub fn finish(self) -> BenchRecord {
        let memory_peak_bytes = self.sampler.stop();
        BenchRecord {
            name: self.name,
            fixture: self.fixture,
            samples: self.samples,
            memory_peak_bytes,
            rust_alloc_peak_bytes: PeakAlloc::is_installed().then(|| PeakAlloc::peak() as u64),
            sqlite_highwater_bytes: sqlite_highwater(false),
        }
    }
}

/// A finished measurement.
#[derive(Debug, Clone)]
pub struct BenchRecord {
    pub name: String,
    pub fixture: Option<String>,
    pub samples: OpSamples,
    /// The highest process footprint seen while it ran ([`memory`]).
    pub memory_peak_bytes: u64,
    /// The counting allocator's peak, when the bench installed it.
    pub rust_alloc_peak_bytes: Option<u64>,
    pub sqlite_highwater_bytes: u64,
}

impl BenchRecord {
    pub fn percentile(&self, percentile: Percentile) -> Duration {
        self.samples.percentile(percentile)
    }

    fn rows(&self) -> Vec<Value> {
        let base = |metric: &str, unit: &str, percentile: &str, value: f64| {
            let mut row = json!({
                "name": self.name,
                "metric": metric,
                "unit": unit,
                "percentile": percentile,
                "value": value,
                "samples": self.samples.count(),
            });
            if let Some(fixture) = &self.fixture {
                row["fixture"] = json!(fixture);
            }
            row
        };
        let mut rows: Vec<Value> = Percentile::REPORTED
            .iter()
            .map(|&p| base("duration", "ms", p.as_str(), self.percentile(p).as_nanos() as f64 / 1e6))
            .collect();
        let peak = self.memory_peak_bytes as f64 / MIB;
        let mut memory = base("memory", "MB", Percentile::Max.as_str(), peak);
        let mut breakdown = json!({ "sqlite_highwater": self.sqlite_highwater_bytes as f64 / MIB });
        if let Some(rust) = self.rust_alloc_peak_bytes {
            breakdown["rust_alloc_peak"] = json!(rust as f64 / MIB);
        }
        memory["breakdown"] = breakdown;
        rows.push(memory);
        rows
    }
}

/// The records of one bench target, written as one perf result.
#[derive(Debug, Clone)]
pub struct Suite {
    name: String,
    records: Vec<BenchRecord>,
}

impl Suite {
    /// A criterion suite named after its bench target.
    pub fn wall_clock(name: &str) -> Suite {
        Suite {
            name: name.to_owned(),
            records: Vec::new(),
        }
    }

    pub fn push(&mut self, record: BenchRecord) {
        self.records.push(record);
    }

    pub fn records(&self) -> &[BenchRecord] {
        &self.records
    }

    pub fn to_json(&self) -> Value {
        json!({
            "schema_version": 1,
            "suite": self.name,
            "kind": "wall-clock",
            "configuration": "bench",
            "runner": runner(),
            "results": self.records.iter().flat_map(BenchRecord::rows).collect::<Vec<_>>(),
        })
    }

    /// Writes `<dir>/<suite>.json` and returns its path.
    pub fn write(&self, dir: &Path) -> std::io::Result<PathBuf> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("{}.json", self.name));
        let text = serde_json::to_string_pretty(&self.to_json()).expect("a result serializes");
        std::fs::write(&path, text + "\n")?;
        Ok(path)
    }
}

/// Where benches write their results: `$MEGURI_BENCH_RESULTS` (set by
/// `tools/mg bench`), else `<target>/bench-results` beside `fallback`.
pub fn results_dir(fallback: &Path) -> PathBuf {
    match std::env::var_os("MEGURI_BENCH_RESULTS") {
        Some(dir) if !dir.is_empty() => PathBuf::from(dir),
        _ => fallback.join("bench-results"),
    }
}

/// The machine, as the perf result's `runner` (U241 keys baselines by id).
fn runner() -> Value {
    let arch = std::env::consts::ARCH;
    #[cfg(target_vendor = "apple")]
    {
        let cpu = apple::sysctl_string("machdep.cpu.brand_string").unwrap_or_else(|| "unknown".to_owned());
        let os = apple::sysctl_string("kern.osproductversion").unwrap_or_else(|| "unknown".to_owned());
        let os_build = apple::sysctl_string("kern.osversion").unwrap_or_else(|| "unknown".to_owned());
        json!({
            "kind": "mac",
            "id": format!("{cpu}/macOS {os}"),
            "cpu": cpu,
            "os": format!("macOS {os}"),
            "os_build": os_build,
            "arch": arch,
            "memory_measure": memory::MEASURE,
        })
    }
    #[cfg(not(target_vendor = "apple"))]
    {
        json!({
            "kind": "docker",
            "id": format!("docker/{}-{arch}", std::env::consts::OS),
            "os": std::env::consts::OS,
            "arch": arch,
            "memory_measure": memory::MEASURE,
        })
    }
}

#[cfg(target_vendor = "apple")]
mod apple {
    use std::ffi::CString;

    pub fn sysctl_string(name: &str) -> Option<String> {
        let name = CString::new(name).ok()?;
        let mut len: libc::size_t = 0;
        // SAFETY: a size query: no output buffer.
        let rc = unsafe { libc::sysctlbyname(name.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) };
        if rc != 0 || len == 0 {
            return None;
        }
        let mut buf = vec![0u8; len];
        // SAFETY: buf holds len bytes, as the size query reported.
        let rc = unsafe {
            libc::sysctlbyname(
                name.as_ptr(),
                buf.as_mut_ptr().cast(),
                &mut len,
                std::ptr::null_mut(),
                0,
            )
        };
        if rc != 0 {
            return None;
        }
        buf.truncate(len);
        while buf.last() == Some(&0) {
            buf.pop();
        }
        String::from_utf8(buf).ok()
    }
}
