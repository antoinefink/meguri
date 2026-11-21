//! Test-only observations of the real writer-thread WAL syncs and op timings.

use std::time::Duration;

use crate::Seq;

#[derive(Clone, Debug)]
pub struct WalSync {
    pub started: Duration,
    pub completed: Duration,
    pub through: Seq,
    pub succeeded: bool,
}

#[derive(Clone, Debug)]
pub struct WritePhase {
    pub started: Duration,
    pub completed: Duration,
}

#[derive(Clone, Debug, Default)]
pub struct DurabilityEvents {
    pub syncs: Vec<WalSync>,
    pub writes: Vec<WritePhase>,
}

/// The log of one recording. It keeps only the attempts that start after
/// recording starts: a sync that was already running belongs to earlier
/// commits, even when the writer logs it after the test's call returned.
#[derive(Debug)]
pub(crate) struct Recording {
    since: Duration,
    events: DurabilityEvents,
}

impl Recording {
    pub(crate) fn new(since: Duration) -> Self {
        Self {
            since,
            events: DurabilityEvents::default(),
        }
    }

    pub(crate) fn sync(&mut self, sync: WalSync) {
        if sync.started >= self.since {
            self.events.syncs.push(sync);
        }
    }

    pub(crate) fn write(&mut self, phase: WritePhase) {
        if phase.started >= self.since {
            self.events.writes.push(phase);
        }
    }

    pub(crate) fn events(&self) -> &DurabilityEvents {
        &self.events
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sync(started: u64) -> WalSync {
        WalSync {
            started: Duration::from_millis(started),
            completed: Duration::from_millis(started + 1),
            through: Seq(1),
            succeeded: true,
        }
    }

    fn write(started: u64) -> WritePhase {
        WritePhase {
            started: Duration::from_millis(started),
            completed: Duration::from_millis(started + 1),
        }
    }

    #[test]
    fn a_recording_leaves_out_attempts_that_started_before_it() {
        let mut recording = Recording::new(Duration::from_millis(10));
        // Logged after the start, as a sync already running then would be.
        recording.sync(sync(9));
        recording.write(write(9));
        recording.sync(sync(10));
        recording.write(write(11));
        let events = recording.events();
        let synced: Vec<_> = events.syncs.iter().map(|s| s.started).collect();
        let written: Vec<_> = events.writes.iter().map(|w| w.started).collect();
        assert_eq!(synced, [Duration::from_millis(10)]);
        assert_eq!(written, [Duration::from_millis(11)]);
    }
}
