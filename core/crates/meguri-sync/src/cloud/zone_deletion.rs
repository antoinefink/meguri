//! One retained P4 DeleteZones pass. IDs are descriptive; the original command,
//! guard and checked Active read are the only deletion authority.
use super::{
    policy::Command,
    publication::{RecordRead, ZoneDeletionGuard},
};
use std::sync::{Mutex, PoisonError};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FailureFacts {
    pub code: Option<i64>,
    pub retry_after_ms: Option<i64>,
    pub encrypted_data_reset: bool,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ZoneResult {
    pub zone: String,
    pub failure: Option<FailureFacts>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Diagnostic {
    Read {
        found: bool,
        allowed: usize,
        failure: Option<FailureFacts>,
        terminal: Option<FailureFacts>,
    },
    Deleted {
        results: Vec<ZoneResult>,
        terminal: Option<FailureFacts>,
    },
}

pub struct Pass {
    pub(crate) command: Command,
    pub(crate) guard: ZoneDeletionGuard,
    pub(crate) phase: Mutex<Phase>,
}
pub(crate) enum Phase {
    ReadReady,
    ReadQueuing,
    ReadQueued,
    Checking,
    DeleteReady { read: RecordRead, zones: Vec<String> },
    DeleteQueuing,
    DeleteQueued { zones: Vec<String> },
    Completing,
    Finished,
}
impl Pass {
    pub(crate) fn finished(&self) -> bool {
        matches!(
            *self.phase.lock().unwrap_or_else(PoisonError::into_inner),
            Phase::Finished
        )
    }
    /// Only unstarted work can be abandoned. Accepted operations must join their
    /// original terminal before their retained Host/Collection can be released.
    pub fn finish(&self) -> bool {
        let mut phase = self.phase.lock().unwrap_or_else(PoisonError::into_inner);
        match *phase {
            Phase::ReadReady | Phase::DeleteReady { .. } | Phase::Finished => {
                *phase = Phase::Finished;
                true
            }
            _ => false,
        }
    }
}
