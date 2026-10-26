//! Job progress (KTD9, R1858): the engine's progress sink exported to Swift.
//! The sink throttles (at most one update per [`MIN_INTERVAL`], phase changes
//! at once); Swift keeps only the newest (`bufferingNewest(1)`). Reports made
//! before Swift attaches a listener are dropped: the next one carries the
//! newer state. Collection activation retains its last typed snapshot and
//! drains its lock-free foreign delivery before terminal publication. U37 adds the phase and the ETA to [`ProgressUpdate`].

use std::sync::{Arc, mpsc};

use crate::collection_entry::CollectionImportMedia;

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
    /// Exact current P16 preparation facts, retained in subsequent entry updates.
    pub collection_media_prepared: Option<CollectionImportMedia>,
}

/// Implemented in Swift. Ordinary jobs call on the reporting thread; collection
/// activation uses its serial delivery worker. Swift only yields into an
/// `AsyncStream` (KTD9).
#[uniffi::export(foreign)]
pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, update: ProgressUpdate);
}

/// The progress channel of one job, handed to Swift in its `JobHandle`.
#[derive(Debug, uniffi::Object)]
pub struct JobProgress {
    job_id: u64,
    sink: ProgressSink,
    collection_entry: Option<Arc<CollectionEntryRelay>>,
}

#[uniffi::export]
impl JobProgress {
    /// Attaches the listener that receives this job's throttled updates,
    /// replacing any earlier one.
    pub fn set_listener(&self, listener: Arc<dyn ProgressListener>) -> Result<(), CoreError> {
        callback::enter("JobProgress::set_listener");
        if let Some(relay) = &self.collection_entry {
            relay.set_listener(self.job_id, listener);
            return Ok(());
        }
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
        Arc::new(JobProgress {
            job_id,
            sink,
            collection_entry: None,
        })
    }

    pub(crate) fn wrap_collection_entry(
        job_id: u64,
        sink: ProgressSink,
        relay: Arc<CollectionEntryRelay>,
    ) -> Arc<Self> {
        Arc::new(Self {
            job_id,
            sink,
            collection_entry: Some(relay),
        })
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
            collection_media_prepared: None,
        };
        callback::invoke("ProgressListener::on_progress", || self.listener.on_progress(update));
    }
}

/// Only collection activation uses this serial relay. The replacement worker
/// snapshots typed data while holding its own locks; it never calls Swift.
/// Delivery and listener replay run on this relay's worker without core locks.
/// Drain precedes result publication and JobFinished, including unwinding.
#[derive(Debug)]
pub(crate) struct CollectionEntryRelay {
    sender: mpsc::Sender<EntryDelivery>,
}
enum EntryDelivery {
    Progress(Progress),
    Media(CollectionImportMedia),
    Listener(u64, Arc<dyn ProgressListener>, mpsc::SyncSender<()>),
    Drain(mpsc::SyncSender<()>),
}
impl CollectionEntryRelay {
    pub(crate) fn new() -> Result<Arc<Self>, CoreError> {
        let (sender, receiver) = mpsc::channel();
        std::thread::Builder::new()
            .name("collection-entry-progress".into())
            .spawn(move || {
                let mut listener: Option<Arc<dyn ProgressListener>> = None;
                let mut latest: Option<Progress> = None;
                let mut job_id = None;
                let mut media: Option<CollectionImportMedia> = None;
                for delivery in receiver {
                    match delivery {
                        EntryDelivery::Progress(progress) => {
                            latest = Some(progress);
                        }
                        EntryDelivery::Media(counts) => {
                            media = Some(counts);
                        }
                        EntryDelivery::Listener(id, new, ack) => {
                            job_id = Some(id);
                            listener = Some(new);
                            if let (Some(listener), Some(progress)) = (&listener, &latest) {
                                deliver_entry(listener.as_ref(), id, progress, media.clone());
                            }
                            let _ = ack.send(());
                            continue;
                        }
                        EntryDelivery::Drain(ack) => {
                            let _ = ack.send(());
                            continue;
                        }
                    }
                    if let (Some(listener), Some(progress), Some(id)) = (&listener, &latest, job_id) {
                        deliver_entry(listener.as_ref(), id, progress, media.clone());
                    }
                }
            })
            .map_err(|_| CoreError::Internal {
                message: "collection entry progress delivery unavailable".into(),
            })?;
        Ok(Arc::new(Self { sender }))
    }
    fn set_listener(&self, job_id: u64, listener: Arc<dyn ProgressListener>) {
        let (ack, done) = mpsc::sync_channel(0);
        // Replay completes before Swift registers even an already finished job.
        if self.sender.send(EntryDelivery::Listener(job_id, listener, ack)).is_ok() {
            let _ = done.recv();
        }
    }
    fn drain(&self) {
        let (ack, done) = mpsc::sync_channel(0);
        if self.sender.send(EntryDelivery::Drain(ack)).is_ok() {
            let _ = done.recv();
        }
    }
    pub(crate) fn drain_guard(self: &Arc<Self>) -> CollectionEntryDrain {
        CollectionEntryDrain(self.clone())
    }
}
impl meguri_engine::ProgressListener for CollectionEntryRelay {
    fn on_progress(&self, progress: &Progress) {
        let _ = self.sender.send(EntryDelivery::Progress(progress.clone()));
    }
    fn on_collection_media_prepared(&self, counts: &meguri_storage::media::ImportMediaCounts) {
        let _ = self.sender.send(EntryDelivery::Media(counts.clone().into()));
    }
}
fn deliver_entry(
    listener: &dyn ProgressListener,
    job_id: u64,
    progress: &Progress,
    media: Option<CollectionImportMedia>,
) {
    let update = ProgressUpdate {
        job_id,
        phase: match &progress.phase {
            meguri_engine::ProgressPhase::Planning => JobProgressPhase::Planning,
            meguri_engine::ProgressPhase::Committing => JobProgressPhase::Committing,
            meguri_engine::ProgressPhase::Named(name) => JobProgressPhase::Named { name: (*name).into() },
        },
        eta_millis: progress.eta.map(|d| u64::try_from(d.as_millis()).unwrap_or(u64::MAX)),
        completed: progress.completed,
        total: progress.total,
        collection_media_prepared: media,
    };
    callback::invoke("ProgressListener::on_progress", || listener.on_progress(update));
}
pub(crate) struct CollectionEntryDrain(Arc<CollectionEntryRelay>);
impl Drop for CollectionEntryDrain {
    fn drop(&mut self) {
        self.0.drain();
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
                total: Some(3_000),
                collection_media_prepared: None,
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
                total: None,
                collection_media_prepared: None,
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
    #[test]
    fn entry_media_snapshot_replays_before_terminal_and_survives_commit_coalescing() {
        let relay = CollectionEntryRelay::new().unwrap();
        let sink = ProgressSink::new();
        sink.set_listener(relay.clone());
        sink.set_phase(meguri_engine::ProgressPhase::Named("media"));
        let mut counts = meguri_storage::media::ImportMediaCounts {
            incoming: 5,
            decoded_bytes: 123,
            reused: 2,
            added: 3,
            trashed: 1,
        };
        sink.report_collection_media_prepared(&counts);
        counts.added = 999;
        assert_eq!(counts.added, 999);
        // Registration can lag the complete backend job. It must synchronously
        // replay the owned snapshot before Swift registers a terminal event.
        relay.drain();
        let progress = JobProgress::wrap_collection_entry(71, sink.clone(), relay.clone());
        let recorder = Arc::new(Recorder::default());
        progress.set_listener(recorder.clone()).unwrap();
        let first = recorder.0.lock().unwrap()[0].clone();
        assert_eq!(first.job_id, 71);
        assert_eq!(first.collection_media_prepared.as_ref().unwrap().added, 3);
        assert_eq!(first.collection_media_prepared.as_ref().unwrap().decoded_bytes, 123);
        sink.set_phase(meguri_engine::ProgressPhase::Committing);
        relay.drain();
        let updates = recorder.0.lock().unwrap();
        assert_eq!(updates.last().unwrap().phase, JobProgressPhase::Committing);
        assert_eq!(
            updates.last().unwrap().collection_media_prepared,
            first.collection_media_prepared
        );
    }

    #[test]
    fn entry_callbacks_run_off_replacement_thread_and_can_reenter_unlocked_sink() {
        struct EntryReentrant {
            sink: std::sync::Weak<ProgressSink>,
            producer: std::thread::ThreadId,
            called: Mutex<bool>,
        }
        impl ProgressListener for EntryReentrant {
            fn on_progress(&self, _update: ProgressUpdate) {
                assert_ne!(std::thread::current().id(), self.producer);
                // Would deadlock if a callback held the sink mutex.
                let _ = self.sink.upgrade().unwrap().phase();
                *self.called.lock().unwrap() = true;
            }
        }
        let relay = CollectionEntryRelay::new().unwrap();
        let sink = Arc::new(ProgressSink::new());
        sink.set_listener(relay.clone());
        let listener = Arc::new(EntryReentrant {
            sink: Arc::downgrade(&sink),
            producer: std::thread::current().id(),
            called: Mutex::new(false),
        });
        let progress = JobProgress::wrap_collection_entry(72, sink.as_ref().clone(), relay.clone());
        progress.set_listener(listener.clone()).unwrap();
        sink.set_phase(meguri_engine::ProgressPhase::Committing);
        relay.drain();
        assert!(*listener.called.lock().unwrap());
    }
}
