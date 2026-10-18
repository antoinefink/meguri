//! Per-collection media coordination (`media/media-journal` J4–J5).
mod events;
mod lock;
mod service;

pub use events::MediaArrived;
pub use lock::{MediaJobKind, MediaJobLease};
pub use service::MediaService;
pub(crate) use service::{MediaState, PendingMedia};

pub(crate) fn error(error: meguri_storage::media::MediaError) -> crate::EngineError {
    crate::EngineError::Media(error)
}
