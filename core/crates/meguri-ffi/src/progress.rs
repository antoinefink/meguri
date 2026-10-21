//! Job progress (KTD9, R1858): the engine's progress sink exported to Swift.
//! The sink throttles (at most one update per [`MIN_INTERVAL`], phase changes
//! at once); Swift keeps only the newest (`bufferingNewest(1)`). Reports made
//! before Swift attaches a listener are dropped: the next one carries the
//! newer state. U37 adds the phase and the ETA to [`ProgressUpdate`].

use std::sync::Arc;

use meguri_engine::{Progress, ProgressSink};

use crate::callback;
use crate::error::CoreError;

/// At most ten updates a second reach Swift (R1858).
pub const MIN_INTERVAL: std::time::Duration = meguri_engine::progress::MIN_INTERVAL;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum JobProgressPhase {
    Planning,
    Committing,
    Named { name: String },
}

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct ProgressUpdate {
    pub job_id: u64,
    pub phase: JobProgressPhase,
    pub eta_millis: Option<u64>,
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
#[derive(Debug, uniffi::Object)]
pub struct JobProgress {
    job_id: u64,
    sink: ProgressSink,
}

#[uniffi::export]
impl JobProgress {
    /// Attaches the listener that receives this job's throttled updates,
    /// replacing any earlier one.
    pub fn set_listener(&self, listener: Arc<dyn ProgressListener>) -> Result<(), CoreError> {
        callback::enter("JobProgress::set_listener");
        self.sink.set_listener(Arc::new(Relay {
            job_id: self.job_id,
            listener,
        }));
        Ok(())
    }
}

impl JobProgress {
    pub fn new(job_id: u64) -> Arc<JobProgress> {
        JobProgress::wrap(job_id, ProgressSink::new())
    }

    /// The exported face of a job's engine sink.
    pub fn wrap(job_id: u64, sink: ProgressSink) -> Arc<JobProgress> {
        Arc::new(JobProgress { job_id, sink })
    }

    pub(crate) fn engine_sink(&self) -> ProgressSink {
        self.sink.clone()
    }

    pub fn job_id(&self) -> u64 {
        self.job_id
    }

    /// Reports progress from the job. Cheap when throttled; the listener is
    /// called without any lock held.
    pub fn report(&self, completed: u64, total: Option<u64>) {
        self.sink.report(completed, total);
    }
}

/// Hands the engine sink's updates to the Swift listener.
struct Relay {
    job_id: u64,
    listener: Arc<dyn ProgressListener>,
}

impl meguri_engine::ProgressListener for Relay {
    fn on_progress(&self, progress: &Progress) {
        let update = ProgressUpdate {
            job_id: self.job_id,
            phase: match &progress.phase {
                meguri_engine::ProgressPhase::Planning => JobProgressPhase::Planning,
                meguri_engine::ProgressPhase::Committing => JobProgressPhase::Committing,
                meguri_engine::ProgressPhase::Named(name) => JobProgressPhase::Named { name: (*name).into() },
            },
            eta_millis: progress.eta.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
            completed: progress.completed,
            total: progress.total,
        };
        callback::invoke("ProgressListener::on_progress", || self.listener.on_progress(update));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;
    use std::sync::Mutex;
    use std::time::Duration;

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
        NOW_MS.set(0);
        let progress = JobProgress::wrap(7, ProgressSink::with_clock(fake_clock));
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
                phase: JobProgressPhase::Planning,
                eta_millis: None,
                completed: 0,
                total: Some(3_000)
            }
        );
        assert_eq!(updates[29].completed, 2_900);
    }

    #[test]
    fn committing_phase_reaches_the_exported_progress_listener_unchanged() {
        let sink = ProgressSink::new();
        let progress = JobProgress::wrap(37, sink.clone());
        let recorder = Arc::new(Recorder::default());
        progress.set_listener(recorder.clone()).unwrap();
        sink.set_phase(meguri_engine::ProgressPhase::Committing);
        let updates = recorder.0.lock().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].phase, JobProgressPhase::Committing);
        assert_eq!(updates[0].job_id, 37);
    }

    #[test]
    fn reports_before_a_listener_is_attached_are_dropped() {
        NOW_MS.set(0);
        let progress = JobProgress::wrap(1, ProgressSink::with_clock(fake_clock));
        progress.report(1, None);
        let recorder = Arc::new(Recorder::default());
        progress.set_listener(recorder.clone()).unwrap();
        NOW_MS.set(1);
        progress.report(2, None);
        assert_eq!(
            *recorder.0.lock().unwrap(),
            vec![ProgressUpdate {
                job_id: 1,
                phase: JobProgressPhase::Planning,
                eta_millis: None,
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
                // Would deadlock if report() still held the sink's lock.
                self.0.report(99, None);
                *self.1.lock().unwrap() += 1;
            }
        }
        NOW_MS.set(0);
        let progress = JobProgress::wrap(1, ProgressSink::with_clock(fake_clock));
        let listener = Arc::new(Reentrant(Arc::clone(&progress), Mutex::new(0)));
        progress.set_listener(listener.clone()).unwrap();
        progress.report(1, None);
        assert_eq!(*listener.1.lock().unwrap(), 1);
    }
}
