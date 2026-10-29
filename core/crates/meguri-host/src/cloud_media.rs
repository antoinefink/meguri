//! Typed host forwarding; Runtime owns media state and file decisions.
use crate::CoreHost;
use meguri_sync::cloud::{
    media::{Reply, Work},
    runtime::RuntimeError,
};
impl CoreHost {
    pub fn next_cloud_media_work(&self) -> Result<Option<Work>, RuntimeError> {
        let runtime = self.cloud_runtime().ok_or_else(|| {
            RuntimeError::Engine(meguri_engine::EngineError::Unsupported {
                feature: "cloud runtime unavailable".into(),
            })
        })?;
        runtime.cloud_media_work_with_space(&self.collection()?, &self.media()?, self.media_space_provider())
    }
    pub fn cloud_media_reply(&self, work: &Work, reply: Reply) -> Result<(), RuntimeError> {
        let runtime = self.cloud_runtime().ok_or_else(|| {
            RuntimeError::Engine(meguri_engine::EngineError::Unsupported {
                feature: "cloud runtime unavailable".into(),
            })
        })?;
        runtime.cloud_media_reply(&self.collection()?, &self.media()?, work, reply)
    }
}
