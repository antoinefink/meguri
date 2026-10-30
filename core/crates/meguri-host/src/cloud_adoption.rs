//! Composition only: Runtime owns V14; existing providers own capacity/backup.
use super::*;
use meguri_sync::cloud::{
    adoption::{AdoptionReply, AdoptionWork},
    publication::Preview,
    runtime::RuntimeError,
};
impl CoreHost {
    pub fn cloud_connection_directory(&self) -> &std::path::Path {
        &self.backup_inputs.collection_directory
    }
    /// Generation authority also exists while this retained Host owns no file.
    pub fn cloud_connection_generation(&self) -> Result<meguri_engine::Generation, EngineError> {
        if !matches!(
            *self.closed.lock().unwrap_or_else(PoisonError::into_inner),
            CloseState::Active
        ) {
            return Err(EngineError::CollectionUnavailable {
                reason: "cloud connection host closed".into(),
            });
        }
        Ok(self.engine().generation())
    }
    /// A retained native connection must never follow this process engine into
    /// a successor host after close. Drop the state guard before any core work.
    pub fn cloud_connection_collection(&self) -> Result<meguri_engine::Collection, EngineError> {
        self.cloud_connection_generation()?;
        self.collection()
    }
    pub fn begin_cloud_connection_preview(&self) -> Result<meguri_sync::cloud::publication::PreviewWork, RuntimeError> {
        self.cloud_connection_generation()?;
        let runtime = self.cloud_runtime().ok_or_else(unavailable)?;
        if matches!(self.engine().state(), meguri_engine::LifecycleState::NoCollection) {
            runtime.begin_connection_without_collection()
        } else {
            runtime.begin_connection_preview(&self.collection()?, &self.media()?)
        }
    }
    pub fn confirm_cloud_adoption(&self, preview: &Preview) -> Result<[u8; 16], RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.confirm_cloud_adoption(
            preview,
            &self.cloud_connection_collection()?,
            self.engine(),
        )
    }
    pub fn cloud_adoption_reply(&self, work: &AdoptionWork, reply: AdoptionReply) -> Result<(), RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.cloud_adoption_reply(
            work,
            reply,
            &self.cloud_connection_collection()?,
            self.backup_inputs.space.as_ref(),
        )
    }
    pub fn perform_cloud_adoption_local(&self, work: &AdoptionWork) -> Result<(), RuntimeError> {
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .perform_cloud_adoption_local(
                work,
                &self.cloud_connection_collection()?,
                &self.media()?,
                self.backup_inputs.space.as_ref(),
                self.backup.as_ref(),
            )
    }
}
fn unavailable() -> RuntimeError {
    EngineError::Unsupported {
        feature: "cloud runtime unavailable".into(),
    }
    .into()
}
