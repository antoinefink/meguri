//! Wall-clock time for runner reports and budgets. This is the testkit's only
//! read of the system clock; nothing read here reaches an executor or the core,
//! which take time from the scenario clock (R1875).

/// Measures elapsed wall time.
#[derive(Debug, Clone, Copy)]
pub struct Stopwatch {
    started: std::time::Instant,
}

impl Stopwatch {
    #[allow(clippy::disallowed_methods)] // runner timing only, never fed to code under test
    pub fn start() -> Stopwatch {
        Stopwatch {
            started: std::time::Instant::now(),
        }
    }

    /// Whole milliseconds since `start`.
    pub fn elapsed_ms(&self) -> u64 {
        u64::try_from(self.started.elapsed().as_millis()).unwrap_or(u64::MAX)
    }

    /// The time since `start`, for budgets below a millisecond's resolution.
    pub fn elapsed(&self) -> std::time::Duration {
        self.started.elapsed()
    }
}
