//! The process's engine (U24) and its event dispatcher: one of each per
//! process, created together on first use with no collection open. The
//! engine's change stream feeds the dispatcher, its job registry runs every
//! job, and its thread registry names the dispatcher thread. U37 opens the
//! collection on it through `CoreHost::open` (U38).

use std::sync::{Arc, OnceLock};

use meguri_engine::{Engine, EngineConfig};

use crate::error::CoreError;
use crate::events::Dispatcher;

struct Runtime {
    engine: Engine,
    dispatcher: Arc<Dispatcher>,
}

fn runtime() -> Result<&'static Runtime, CoreError> {
    static RUNTIME: OnceLock<Result<Runtime, CoreError>> = OnceLock::new();
    RUNTIME
        .get_or_init(|| {
            let engine = Engine::new(EngineConfig::default()).map_err(CoreError::from)?;
            // Subscribed before anything can reach the engine, so every event
            // (the first job's JobFinished included) waits in the
            // subscription until Swift attaches its listener
            // (`engine/change-stream` S3).
            let dispatcher = Dispatcher::start(engine.stream(), engine.threads())?;
            Ok(Runtime { engine, dispatcher })
        })
        .as_ref()
        .map_err(Clone::clone)
}

pub(crate) fn engine() -> Result<&'static Engine, CoreError> {
    runtime().map(|runtime| &runtime.engine)
}

pub(crate) fn dispatcher() -> Result<&'static Arc<Dispatcher>, CoreError> {
    runtime().map(|runtime| &runtime.dispatcher)
}
