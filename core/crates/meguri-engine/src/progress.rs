//! Progress sinks (`engine/jobs` J5, R1858): phase, counts and ETA, delivered
//! to one listener at most once per [`MIN_INTERVAL`], with every phase change
//! delivered at once. Progress is the only throttled channel (KTD9); it never
//! goes through the change stream.

use std::fmt;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::Duration;

use crate::monotonic::{self, MonotonicClock};

/// At most ten updates a second reach the listener (R1858).
pub const MIN_INTERVAL: Duration = Duration::from_millis(100);

/// Finite, content-free routine sync evidence, retained even before attachment.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoutineSyncMilestone {
    CollectionStarted, ServerAcknowledged, ApplyStarted, ApplyCompleted,
    MediaCheckStarted, MediaTransferStarted,
}

/// What the reporting work is doing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ProgressPhase {
    /// Cancellable work before any write (plan/commit, `engine/op-model` O6).
    Planning,
    /// The write phase: past the cancel point, so the UI stops offering
    /// Cancel (O5).
    Committing,
    /// A stage a job defines for itself (a sync's upload, a backup's
    /// compression).
    Named(&'static str),
}

impl ProgressPhase {
    /// Whether Cancel can still stop the work in this phase.
    pub fn cancellable(&self) -> bool {
        !matches!(self, ProgressPhase::Committing)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct ObjectCounts {
    pub sent: u64,
    pub received: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Progress {
    pub sequence: u64,
    pub routine_sync: Vec<RoutineSyncMilestone>,
    pub sync_files_left: Option<u64>,
    /// Normal-sync object counts, retained across phase changes. Transport
    /// byte progress uses its own job sink and never overwrites these values.
    pub objects: Option<ObjectCounts>,
    pub phase: ProgressPhase,
    pub completed: u64,
    /// `None` while the amount of work is unknown.
    pub total: Option<u64>,
    /// Time left at the phase's average rate so far.
    pub eta: Option<Duration>,
}

/// Receives a sink's throttled updates, on the reporting thread, with no lock
/// of the sink held. The Swift implementation only yields into an
/// `AsyncStream` (KTD9).
pub trait ProgressListener: Send + Sync {
    fn on_progress(&self, progress: &Progress);
    /// Fresh P16 merge facts, delivered before collection activation. Native
    /// consumers remain behind the public media-entry guard until adapted.
    fn on_collection_media_prepared(&self, _counts: &meguri_storage::media::ImportMediaCounts) {}
}

/// The progress channel of one op or job. Clones share it.
#[derive(Clone)]
pub struct ProgressSink {
    inner: Arc<Inner>,
}

struct Inner {
    clock: MonotonicClock,
    state: Mutex<State>,
}

struct State {
    sequence: u64,
    listener: Option<Arc<dyn ProgressListener>>,
    routine_sync: Vec<RoutineSyncMilestone>,
    sync_files_left: Option<u64>,
    phase: ProgressPhase,
    phase_started: Duration,
    completed: u64,
    total: Option<u64>,
    objects: Option<ObjectCounts>,
    last_delivery: Option<Duration>,
}

impl fmt::Debug for ProgressSink {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let state = self.lock();
        f.debug_struct("ProgressSink")
            .field("phase", &state.phase)
            .field("completed", &state.completed)
            .field("total", &state.total)
            .finish_non_exhaustive()
    }
}

impl Default for ProgressSink {
    fn default() -> Self {
        ProgressSink::new()
    }
}

impl ProgressSink {
    /// A sink in the `Planning` phase with no listener.
    pub fn new() -> ProgressSink {
        ProgressSink::with_clock(monotonic::now)
    }

    /// A sink paced by `clock` (tests pass a fake one).
    pub fn with_clock(clock: MonotonicClock) -> ProgressSink {
        let now = clock();
        ProgressSink {
            inner: Arc::new(Inner {
                clock,
                state: Mutex::new(State {
                    sequence: 0,
                    listener: None,
                    routine_sync: Vec::new(),
                    sync_files_left: None,
                    phase: ProgressPhase::Planning,
                    phase_started: now,
                    completed: 0,
                    total: None,
                    last_delivery: None,
                    objects: None,
                }),
            }),
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.inner.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Attaches the listener of every later update, replacing any earlier
    /// one. Routine sync replays its retained evidence; other jobs wait for
    /// the next report or phase change.
    pub fn set_listener(&self, listener: Arc<dyn ProgressListener>) {
        let now = (self.inner.clock)();
        let replay = {
            let mut state = self.lock();
            state.listener = Some(listener);
            if state.routine_sync.is_empty() { None } else { state.deliver(now) }
        };
        if let Some((listener, progress)) = replay { listener.on_progress(&progress); }
    }

    /// Unthrottled operation facts. Clone the listener under the sink lock,
    /// then deliver on the reporting worker with no sink or core lock held.
    pub fn report_collection_media_prepared(&self, counts: &meguri_storage::media::ImportMediaCounts) {
        let listener = self.lock().listener.clone();
        if let Some(listener) = listener {
            listener.on_collection_media_prepared(counts);
        }
    }

    /// Each code is recorded once, so the retained evidence is bounded at six.
    /// Delivery bypasses count throttling; every later update carries the evidence.
    pub fn observe_routine_sync(&self, milestone: RoutineSyncMilestone) {
        let now = (self.inner.clock)();
        let delivery = {
            let mut state = self.lock();
            if state.routine_sync.contains(&milestone) { return; }
            state.routine_sync.push(milestone);
            state.deliver(now)
        };
        if let Some((listener, progress)) = delivery { listener.on_progress(&progress); }
    }

    /// Publish routine media activity, evidence and counts as one snapshot.
    /// Knowledge and phase changes bypass throttling; generic checked counts
    /// retain the existing count reporter's meaning.
    pub fn report_routine_media(&self, transferring: bool, transfer_started: bool, checked: u64, left: Option<u64>) {
        let now = (self.inner.clock)();
        let delivery = {
            let mut state = self.lock();
            let phase = ProgressPhase::Named(if transferring { "meguri-media-transfer" } else { "meguri-media-checking" });
            let left = if transferring { left } else { None };
            let phase_changed = state.phase != phase;
            let knowledge_changed = state.sync_files_left.is_some() != left.is_some();
            let new_transfer = transfer_started && !state.routine_sync.contains(&RoutineSyncMilestone::MediaTransferStarted);
            if new_transfer { state.routine_sync.push(RoutineSyncMilestone::MediaTransferStarted); }
            if phase_changed { state.phase_started = now; }
            state.phase = phase;
            state.sync_files_left = left;
            state.completed = checked;
            state.total = None;
            let due = state.last_delivery.is_none_or(|last| now.saturating_sub(last) >= MIN_INTERVAL);
            if phase_changed || knowledge_changed || new_transfer || due { state.deliver(now) } else { None }
        };
        if let Some((listener, progress)) = delivery { listener.on_progress(&progress); }
    }

    pub fn phase(&self) -> ProgressPhase {
        self.lock().phase.clone()
    }

    /// Moves to `phase` with its counts reset; delivered at once when a
    /// listener is attached, and with the next report otherwise. Setting the
    /// current phase again does nothing.
    pub fn set_phase(&self, phase: ProgressPhase) {
        let now = (self.inner.clock)();
        let delivery = {
            let mut state = self.lock();
            if state.phase == phase {
                return;
            }
            state.phase = phase;
            state.phase_started = now;
            state.completed = 0;
            state.total = None;
            state.deliver(now)
        };
        if let Some((listener, progress)) = delivery {
            listener.on_progress(&progress);
        }
    }

    /// H16/N11: object counts use the same 100 ms delivery throttle as all
    /// job progress; neither a byte counter nor a phase reset is a decrease.
    pub fn report_objects(&self, sent: u64, received: u64) {
        {
            let mut state = self.lock();
            state.objects = Some(ObjectCounts { sent, received });
        }
        self.report(sent.saturating_add(received), None);
    }

    /// Reports the counts of the current phase. Cheap when throttled.
    pub fn report(&self, completed: u64, total: Option<u64>) {
        let now = (self.inner.clock)();
        let delivery = {
            let mut state = self.lock();
            state.completed = completed;
            state.total = total;
            match state.last_delivery {
                Some(last) if now.saturating_sub(last) < MIN_INTERVAL => None,
                _ => state.deliver(now),
            }
        };
        if let Some((listener, progress)) = delivery {
            listener.on_progress(&progress);
        }
    }
}

impl State {
    /// The update to deliver now, if a listener is attached.
    fn deliver(&mut self, now: Duration) -> Option<(Arc<dyn ProgressListener>, Progress)> {
        let listener = self.listener.clone()?;
        self.last_delivery = Some(now);
        self.sequence = self.sequence.saturating_add(1);
        Some((
            listener,
            Progress {
                sequence: self.sequence,
                routine_sync: self.routine_sync.clone(),
                sync_files_left: self.sync_files_left,
                objects: self.objects,
                phase: self.phase.clone(),
                completed: self.completed,
                total: self.total,
                eta: self.eta(now),
            },
        ))
    }

    fn eta(&self, now: Duration) -> Option<Duration> {
        let total = self.total?;
        if self.completed == 0 || self.completed > total {
            return None;
        }
        let elapsed = now.saturating_sub(self.phase_started);
        let remaining = total - self.completed;
        let nanos = elapsed.as_nanos() * u128::from(remaining) / u128::from(self.completed);
        Some(Duration::from_nanos(u64::try_from(nanos).unwrap_or(u64::MAX)))
    }
}
