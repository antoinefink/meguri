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
