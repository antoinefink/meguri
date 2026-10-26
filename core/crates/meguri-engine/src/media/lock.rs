use crate::EngineError;
use std::sync::{Arc, Mutex};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaJobKind {
    Sync,
    CheckMedia,
    CollectionImport,
}
impl MediaJobKind {
    fn name(self) -> &'static str {
        match self {
            Self::Sync => "media sync",
            Self::CheckMedia => "Check Media",
            Self::CollectionImport => "collection media import",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub(super) struct MediaJobLock(Arc<Mutex<Option<MediaJobKind>>>);

/// Owns only admission to a long media job; no journal mutex is held.
#[derive(Debug)]
pub struct MediaJobLease(MediaJobLock);

impl MediaJobLock {
    pub fn try_acquire(&self, kind: MediaJobKind) -> Result<MediaJobLease, EngineError> {
        let mut state = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(running) = *state {
            return Err(EngineError::Busy {
                running_op: running.name().into(),
            });
        }
        *state = Some(kind);
        Ok(MediaJobLease(self.clone()))
    }
}
impl Drop for MediaJobLease {
    fn drop(&mut self) {
        *self.0.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner) = None;
    }
}
