//! Composition only: Runtime owns V14; existing providers own capacity/backup.
use super::*;
use meguri_sync::cloud::{
    adoption::{AdoptionReply, AdoptionWork},
    publication::Preview,
    runtime::RuntimeError,
};
impl CoreHost {
    pub fn confirm_cloud_adoption(&self, preview: &Preview) -> Result<[u8; 16], RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.confirm_cloud_adoption(
            preview,
            &self.collection()?,
            self.engine(),
        )
    }
    pub fn cloud_adoption_reply(&self, work: &AdoptionWork, reply: AdoptionReply) -> Result<(), RuntimeError> {
        self.cloud_runtime().ok_or_else(unavailable)?.cloud_adoption_reply(
            work,
            reply,
            &self.collection()?,
            self.backup_inputs.space.as_ref(),
        )
    }
    pub fn perform_cloud_adoption_local(&self, work: &AdoptionWork) -> Result<(), RuntimeError> {
        self.cloud_runtime()
            .ok_or_else(unavailable)?
            .perform_cloud_adoption_local(
                work,
                &self.collection()?,
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
