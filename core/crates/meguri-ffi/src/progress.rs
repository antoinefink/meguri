//! Job progress (KTD9, R1858). Progress is the only throttled channel: Rust
//! delivers at most one update per [`MIN_INTERVAL`] to the job's listener, and
//! Swift keeps only the newest (`bufferingNewest(1)`). Updates reported
//! before Swift attaches a listener, or inside the interval, are dropped: the
//! next report carries the newer state.

use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::callback;
use crate::error::CoreError;

/// At most ten updates a second reach Swift (R1858).
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ProgressUpdate {
    pub job_id: u64,
    pub completed: u64,
    /// `None` while the amount of work is unknown.
    pub total: Option<u64>,
}

/// Implemented in Swift. Called on the reporting job's thread; the Swift
/// implementation only yields into an `AsyncStream` (KTD9).
#[uniffi::export(foreign)]
pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, update: ProgressUpdate);
}

/// The progress channel of one job, handed to Swift in its `JobHandle`.
#[derive(uniffi::Object)]
pub struct JobProgress {
    job_id: u64,
    clock: fn() -> Duration,
    state: Mutex<State>,
}

struct State {
    listener: Option<Arc<dyn ProgressListener>>,
    throttle: Throttle,
}

impl std::fmt::Debug for JobProgress {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobProgress")
            .field("job_id", &self.job_id)
            .finish_non_exhaustive()
    }
}

#[uniffi::export]
impl JobProgress {
    /// Attaches the listener that receives this job's throttled updates,
    /// replacing any earlier one.
    pub fn set_listener(&self, listener: Arc<dyn ProgressListener>) -> Result<(), CoreError> {
        callback::enter("JobProgress::set_listener");
        self.lock().listener = Some(listener);
        Ok(())
    }
}

impl JobProgress {
    pub fn new(job_id: u64) -> Arc<JobProgress> {
        JobProgress::with_clock(job_id, clock::monotonic)
    }

    fn with_clock(job_id: u64, clock: fn() -> Duration) -> Arc<JobProgress> {
        Arc::new(JobProgress {
            job_id,
            clock,
            state: Mutex::new(State {
                listener: None,
                throttle: Throttle::new(MIN_INTERVAL),
            }),
        })
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }

    /// Reports progress from the job. Cheap when throttled; the listener is
    /// called without any lock held.
    pub fn report(&self, completed: u64, total: Option<u64>) {
        let listener = {
            let mut state = self.lock();
            let Some(listener) = state.listener.clone() else {
                return;
            };
            if !state.throttle.admit((self.clock)()) {
                return;
            }
            listener
        };
        let update = ProgressUpdate {
            job_id: self.job_id,
            completed,
            total,
        };
        callback::invoke("ProgressListener::on_progress", || listener.on_progress(update));
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Leading-edge throttle: admits a report when at least `interval` has passed
/// since the last admitted one, so any one-second window holds at most
/// `1s / interval + 1` admitted reports.
#[derive(Debug)]
struct Throttle {
    interval: Duration,
    last: Option<Duration>,
}

impl Throttle {
    fn new(interval: Duration) -> Throttle {
        Throttle { interval, last: None }
    }

    fn admit(&mut self, now: Duration) -> bool {
        match self.last {
            Some(last) if now.saturating_sub(last) < self.interval => false,
            _ => {
                self.last = Some(now);
                true
            }
        }
    }
}

/// The monotonic time progress pacing uses. It is never collection time, so
/// it does not go through `meguri_model::clock` (R1875 covers the instants the
/// core stores or schedules with).
mod clock {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    #[allow(clippy::disallowed_methods)]
    pub(super) fn monotonic() -> Duration {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[derive(Default)]
    struct Recorder(Mutex<Vec<ProgressUpdate>>);
    impl ProgressListener for Recorder {
        fn on_progress(&self, update: ProgressUpdate) {
            self.0.lock().unwrap().push(update);
        }
    }

    thread_local! {
        static NOW_MS: Cell<u64> = const { Cell::new(0) };
    }
    fn fake_clock() -> Duration {
        Duration::from_millis(NOW_MS.get())
    }

    #[test]
    fn a_thousand_reports_a_second_reach_the_listener_at_most_eleven_times_in_any_second() {
        let progress = JobProgress::with_clock(7, fake_clock);
        let recorder = Arc::new(Recorder::default());
        progress.set_listener(recorder.clone()).unwrap();
        let mut delivered_at = Vec::new();
        for ms in 0..3_000u64 {
            NOW_MS.set(ms);
            let before = recorder.0.lock().unwrap().len();
            progress.report(ms, Some(3_000));
            if recorder.0.lock().unwrap().len() > before {
                delivered_at.push(ms);
            }
        }
        for &start in &delivered_at {
            let in_window = delivered_at
                .iter()
                .filter(|&&t| t >= start && t <= start + 1_000)
                .count();
            assert!(
                in_window <= 11,
                "{in_window} updates in the second starting at {start} ms"
            );
        }
        // One update every 100 ms: 0, 100, ..., 2900.
        assert_eq!(delivered_at.len(), 30);
        let updates = recorder.0.lock().unwrap();
        assert_eq!(
            updates[0],
            ProgressUpdate {
                job_id: 7,
                completed: 0,
                total: Some(3_000)
            }
        );
        assert_eq!(updates[29].completed, 2_900);
    }

    #[test]
    fn reports_before_a_listener_is_attached_are_dropped() {
        let progress = JobProgress::with_clock(1, fake_clock);
        NOW_MS.set(0);
        progress.report(1, None);
        let recorder = Arc::new(Recorder::default());
        progress.set_listener(recorder.clone()).unwrap();
        NOW_MS.set(1);
        progress.report(2, None);
        assert_eq!(
            *recorder.0.lock().unwrap(),
            vec![ProgressUpdate {
                job_id: 1,
                completed: 2,
                total: None
            }]
        );
    }

    #[test]
    fn the_listener_runs_without_the_progress_lock_held() {
        struct Reentrant(Arc<JobProgress>, Mutex<u32>);
        impl ProgressListener for Reentrant {
            fn on_progress(&self, _update: ProgressUpdate) {
                // Would deadlock if report() still held the state lock.
                drop(self.0.lock());
                *self.1.lock().unwrap() += 1;
            }
        }
        let progress = JobProgress::with_clock(1, fake_clock);
        let listener = Arc::new(Reentrant(Arc::clone(&progress), Mutex::new(0)));
        progress.set_listener(listener.clone()).unwrap();
        progress.report(1, None);
        assert_eq!(*listener.1.lock().unwrap(), 1);
    }

    #[test]
    fn the_system_clock_is_monotonic() {
        let a = clock::monotonic();
        let b = clock::monotonic();
        assert!(b >= a);
    }
}
