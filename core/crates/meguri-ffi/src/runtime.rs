//! The process's engine (U24): one per process, created on first use with no
//! collection open. Its change stream feeds the event dispatcher, its job
//! registry runs every job, and its thread registry names the dispatcher
//! thread. U37 opens the collection on it through `CoreHost::open` (U38).

use std::sync::OnceLock;

use meguri_engine::{Engine, EngineConfig};

use crate::error::CoreError;

pub(crate) fn engine() -> Result<&'static Engine, CoreError> {
    static ENGINE: OnceLock<Result<Engine, CoreError>> = OnceLock::new();
    ENGINE
        .get_or_init(|| Engine::new(EngineConfig::default()).map_err(CoreError::from))
        .as_ref()
        .map_err(Clone::clone)
}
