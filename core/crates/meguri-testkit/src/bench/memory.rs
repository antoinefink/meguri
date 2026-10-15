//! Process memory for benches (R1864, U241): one definition everywhere.
//! On Apple hosts it is the process's `phys_footprint`, the number the
//! system's memory limits and Xcode's gauges use; in Docker (Linux) it is
//! the resident set. A [`MemorySampler`] reads it on a background thread
//! while a bench runs and keeps the highest reading.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::thread::JoinHandle;
use std::time::Duration;

/// What [`process_footprint_bytes`] measures here, as the perf result's
/// `runner.memory_measure` names it.
#[cfg(target_vendor = "apple")]
pub const MEASURE: &str = "phys_footprint";
#[cfg(not(target_vendor = "apple"))]
pub const MEASURE: &str = "peak_rss";

/// The process's memory now, in bytes: `phys_footprint` on Apple
/// platforms.
#[cfg(target_vendor = "apple")]
pub fn process_footprint_bytes() -> Option<u64> {
    let mut info = std::mem::MaybeUninit::<libc::rusage_info_v4>::zeroed();
    // SAFETY: proc_pid_rusage writes a rusage_info_v4 for the flavor
    // RUSAGE_INFO_V4 into the buffer, which is that size.
    let rc = unsafe {
        libc::proc_pid_rusage(
            libc::getpid(),
            libc::RUSAGE_INFO_V4,
            info.as_mut_ptr().cast::<libc::rusage_info_t>(),
        )
    };
    if rc != 0 {
        return None;
    }
    // SAFETY: the call succeeded, so the kernel filled the struct.
    Some(unsafe { info.assume_init() }.ri_phys_footprint)
}

/// The process's memory now, in bytes: the resident set (`VmRSS`) on Linux.
#[cfg(not(target_vendor = "apple"))]
pub fn process_footprint_bytes() -> Option<u64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: u64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .trim()
        .trim_end_matches("kB")
        .trim()
        .parse()
        .ok()?;
    Some(kb * 1024)
}

/// Reads [`process_footprint_bytes`] every `interval` on its own thread
/// until stopped, and keeps the highest reading (also taken at start and
/// at stop).
#[derive(Debug)]
pub struct MemorySampler {
    peak: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
}

/// The sampling period benches use.
pub const DEFAULT_INTERVAL: Duration = Duration::from_millis(5);

impl MemorySampler {
    pub fn start(interval: Duration) -> MemorySampler {
        let peak = Arc::new(AtomicU64::new(process_footprint_bytes().unwrap_or(0)));
        let stop = Arc::new(AtomicBool::new(false));
        let thread = {
            let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
            std::thread::Builder::new()
                .name("bench-memory-sampler".to_owned())
                .spawn(move || {
                    while !stop.load(Ordering::Relaxed) {
                        if let Some(now) = process_footprint_bytes() {
                            peak.fetch_max(now, Ordering::Relaxed);
                        }
                        std::thread::sleep(interval);
                    }
                })
                .expect("the sampler thread starts")
        };
        MemorySampler {
            peak,
            stop,
            thread: Some(thread),
        }
    }

    /// The highest reading so far, in bytes.
    pub fn peak_bytes(&self) -> u64 {
        self.peak.load(Ordering::Relaxed)
    }

    /// Stops sampling and returns the highest reading, in bytes.
    pub fn stop(mut self) -> u64 {
        self.halt();
        self.peak_bytes()
    }

    fn halt(&mut self) {
        if let Some(now) = process_footprint_bytes() {
            self.peak.fetch_max(now, Ordering::Relaxed);
        }
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for MemorySampler {
    fn drop(&mut self) {
        self.halt();
    }
}
