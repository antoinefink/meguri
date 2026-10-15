//! Per-operation timing (R1864). A suite hands [`OpSamples::run`] to
//! criterion's `iter_custom`: each call of the operation is timed on its own
//! and recorded in an HDR histogram, and the sum goes back to criterion. A
//! stall in one operation therefore shows in the high percentiles instead of
//! vanishing into a batch mean.
//!
//! Every routine call is recorded, criterion's warm-up passes included. A
//! bench that needs a warm state (a cached file, a built index) prepares it
//! before handing the operation to criterion.

use std::time::Duration;

use hdrhistogram::Histogram;

use crate::wallclock::Stopwatch;

/// The percentiles a result reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Percentile {
    P50,
    P95,
    P99,
    Max,
}

impl Percentile {
    /// The rows a wall-clock result has, in order.
    pub const REPORTED: [Percentile; 4] = [Percentile::P50, Percentile::P95, Percentile::P99, Percentile::Max];

    pub fn as_str(self) -> &'static str {
        match self {
            Percentile::P50 => "p50",
            Percentile::P95 => "p95",
            Percentile::P99 => "p99",
            Percentile::Max => "max",
        }
    }

    fn per_hundred(self) -> u64 {
        match self {
            Percentile::P50 => 50,
            Percentile::P95 => 95,
            Percentile::P99 => 99,
            Percentile::Max => 100,
        }
    }

    /// The 1-based rank of this percentile among `count` sorted samples: the
    /// smallest sample with fewer than (100 - N)% of the samples above it.
    /// With one slow sample in a hundred, the p99 is that sample: a
    /// percentile never hides the slow tail it is meant to show.
    pub fn rank(self, count: u64) -> u64 {
        if count == 0 {
            return 0;
        }
        (count * self.per_hundred() / 100 + 1).min(count)
    }
}

/// Nanosecond samples of one operation, from 1 ns to one hour, at three
/// significant digits (a value reads back at most 0.1% above what was
/// recorded).
#[derive(Clone, Debug)]
pub struct OpSamples {
    histogram: Histogram<u64>,
    total: Duration,
}

impl Default for OpSamples {
    fn default() -> Self {
        OpSamples::new()
    }
}

const HOUR_NS: u64 = 3_600_000_000_000;

impl OpSamples {
    pub fn new() -> OpSamples {
        OpSamples {
            histogram: Histogram::new_with_bounds(1, HOUR_NS, 3).expect("valid histogram bounds"),
            total: Duration::ZERO,
        }
    }

    /// Records one operation's time (clamped to the histogram's range).
    pub fn record(&mut self, elapsed: Duration) {
        let ns = u64::try_from(elapsed.as_nanos()).unwrap_or(u64::MAX).clamp(1, HOUR_NS);
        self.histogram.record(ns).expect("a clamped value is in range");
        self.total += elapsed;
    }

    /// Runs `op` `iters` times (its argument is the call's index in this
    /// batch), timing each call on its own. What `op` returns is dropped
    /// after its timing stops, so closing or cleaning up costs nothing here.
    /// Returns the summed time, which is what `iter_custom` expects.
    pub fn run<T>(&mut self, iters: u64, mut op: impl FnMut(u64) -> T) -> Duration {
        let mut batch = Duration::ZERO;
        for i in 0..iters {
            let watch = Stopwatch::start();
            let out = op(i);
            let elapsed = watch.elapsed();
            drop(out);
            self.record(elapsed);
            batch += elapsed;
        }
        batch
    }

    /// Operations recorded.
    pub fn count(&self) -> u64 {
        self.histogram.len()
    }

    /// The summed time of every recorded operation.
    pub fn total(&self) -> Duration {
        self.total
    }

    /// The percentile, by [`Percentile::rank`]; zero without samples.
    pub fn percentile(&self, percentile: Percentile) -> Duration {
        let rank = percentile.rank(self.count());
        if rank == 0 {
            return Duration::ZERO;
        }
        let mut seen = 0;
        for value in self.histogram.iter_recorded() {
            seen += value.count_since_last_iteration();
            if seen >= rank {
                return Duration::from_nanos(self.histogram.highest_equivalent(value.value_iterated_to()));
            }
        }
        Duration::from_nanos(self.histogram.max())
    }
}
