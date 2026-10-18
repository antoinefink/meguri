use meguri_engine::{Collection, EngineError, OpenStep, SuspendParticipant, media::MediaService};
use meguri_model::clock::Clock;
use meguri_storage::media::MediaStore;
use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

pub(crate) struct MediaPort {
    clock: Arc<dyn Clock>,
    store: Mutex<Option<MediaStore>>,
}
impl MediaPort {
    pub(crate) fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            clock,
            store: Mutex::new(None),
        }
    }
    pub(crate) fn close_until(&self, deadline: Duration) -> Result<(), EngineError> {
        let store = self.store.lock().unwrap_or_else(PoisonError::into_inner).clone();
        if let Some(store) = store {
            loop {
                if meguri_engine::monotonic::now() >= deadline {
                    return Err(EngineError::SuspendDeadline { phase: "media".into() });
                }
                if store.try_close().map_err(EngineError::Media)? {
                    break;
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        }
        Ok(())
    }
}
impl OpenStep for MediaPort {
    fn name(&self) -> &str {
        "media"
    }
    fn run(&self, collection: &Collection) -> Result<(), EngineError> {
        let service = MediaService::open(collection, self.clock.clone())?;
        // Keep the store before recovery: failure cleanup must close it too.
        *self.store.lock().unwrap_or_else(PoisonError::into_inner) = Some(service.store().clone());
        service.recover()?;
        Ok(())
    }
}
impl SuspendParticipant for MediaPort {
    fn name(&self) -> &str {
        "media"
    }
    fn flush(&self) -> Result<(), EngineError> {
        Ok(())
    }
    fn release_connections_until(&self, deadline: Duration) -> Result<(), EngineError> {
        self.close_until(deadline)
    }
    fn resume(&self) -> Result<(), EngineError> {
        if let Some(store) = self.store.lock().unwrap_or_else(PoisonError::into_inner).as_ref() {
            store.resume().map_err(EngineError::Media)?;
        }
        Ok(())
    }
    fn resume_with_collection(&self, collection: &Collection) -> Result<(), EngineError> {
        self.resume()?;
        if let Err(error) = MediaService::open(collection, self.clock.clone()).and_then(|service| service.recover()) {
            self.close_until(meguri_engine::monotonic::now() + Duration::from_secs(5))?;
            return Err(error);
        }
        Ok(())
    }
}
