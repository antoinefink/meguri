//! Read-only image fetch job; the native picker validates the returned image
//! before using the generation-bound media importer.
use std::sync::{Arc, Mutex};
use crate::{CoreError, JobHandle, callback, jobs, runtime};

#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EditorFetchedImage {
    pub name: String,
    pub bytes: Vec<u8>,
}

#[derive(Debug, uniffi::Object)]
pub struct EditorImageFetchJob {
    result: Arc<Mutex<Option<EditorFetchedImage>>>,
}

#[uniffi::export]
impl EditorImageFetchJob {
    /// Consume once, after the matching job has drained.
    pub fn result(&self) -> Result<Option<EditorFetchedImage>, CoreError> {
        callback::enter("EditorImageFetchJob::result");
        Ok(self.result.lock().unwrap().take())
    }
}

#[derive(Debug, Clone, uniffi::Record)]
pub struct EditorImageFetchStart {
    pub handle: JobHandle,
    pub result: Arc<EditorImageFetchJob>,
}

#[uniffi::export]
pub fn start_editor_image_fetch(url: String) -> Result<EditorImageFetchStart, CoreError> {
    callback::enter("start_editor_image_fetch");
    let result = Arc::new(Mutex::new(None));
    let output = result.clone();
    let handle = jobs::start("editor image fetch", 0, move |ctx| {
        let fetched = meguri_sync::remote_media::fetch(runtime::engine()?.threads(), ctx.cancel.engine_token(), &url)
            .map_err(|error| match error {
                meguri_sync::SyncError::Cancelled => CoreError::Cancelled,
                meguri_sync::SyncError::Status { code, .. } => CoreError::Unavailable {
                    reason: format!("The server answered with status {code}."),
                },
                error => CoreError::Unavailable { reason: error.to_string() },
            })?;
        ctx.cancel.check()?;
        *output.lock().unwrap() = Some(EditorFetchedImage { name: fetched.name, bytes: fetched.bytes });
        Ok(())
    })?;
    Ok(EditorImageFetchStart { handle, result: Arc::new(EditorImageFetchJob { result }) })
}
