//! The error every export returns (KTD9). Swift sees it as the generated
//! `CoreError` enum, and CoreBridge maps it onto `BridgeError`. Area units add
//! a variant only when Swift must react to it differently from these.

use std::fmt;

use meguri_engine::EngineError;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Error)]
pub enum CoreError {
    /// The operation's `CancelToken` was cancelled before it finished.
    Cancelled,
    /// Another long operation holds the slot this one needs (R171).
    Busy { running_op: String },
    /// The collection cannot take this call now, for example during an
    /// exclusive phase (KTD17).
    Unavailable { reason: String },
    /// Compiled out for this platform or build: the iOS stubs of the server
    /// exports (KTD9) and the AnkiWeb switch (KTD21).
    Unsupported { feature: String },
    /// The caller passed a value the core rejects.
    InvalidArgument { message: String },
    /// A failure the caller cannot act on: a bug, or a caught panic.
    Internal { message: String },
    /// The note write committed; recover attachments without repeating it.
    MediaCommitPending { seq: u64, message: String },
    /// The write committed. Refresh its state without repeating it.
    CommitCompleted { seq: u64, message: String },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::Cancelled => f.write_str("cancelled"),
            CoreError::Busy { running_op } => write!(f, "busy: {running_op} is running"),
            CoreError::Unavailable { reason } => write!(f, "unavailable: {reason}"),
            CoreError::Unsupported { feature } => write!(f, "unsupported: {feature}"),
            CoreError::InvalidArgument { message } => write!(f, "invalid argument: {message}"),
            CoreError::Internal { message } => write!(f, "internal error: {message}"),
            CoreError::MediaCommitPending { seq, message } => {
                write!(f, "operation {seq} committed; media recovery pending: {message}")
            }
            CoreError::CommitCompleted { seq, message } => {
                write!(f, "operation {seq} committed; post-commit failure: {message}")
            }
        }
    }
}

impl std::error::Error for CoreError {}

/// The engine's errors as Swift sees them. U37 adds variants for the op
/// results Swift reacts to (`Stale`, `StaleGeneration`, `UndoUnavailable`,
/// `NeedsSchemaConfirmation`, `NotetypeRefused`); until then they cross as
/// `Unavailable`, `InvalidArgument` or `Internal`.
impl From<EngineError> for CoreError {
    fn from(error: EngineError) -> CoreError {
        match error {
            EngineError::Cancelled => CoreError::Cancelled,
            EngineError::Busy { running_op } => CoreError::Busy { running_op },
            EngineError::SessionActive => CoreError::Unavailable {
                reason: "SessionActive".into(),
            },
            EngineError::CollectionUnavailable { reason } => CoreError::Unavailable { reason },
            EngineError::SuspendDeadline { .. } => CoreError::Unavailable {
                reason: error.to_string(),
            },
            EngineError::MediaCommitPending { seq, message } => CoreError::MediaCommitPending { seq: seq.0, message },
            EngineError::CommitCompleted { seq, message } => CoreError::CommitCompleted { seq: seq.0, message },
            EngineError::StaleGeneration { .. }
            | EngineError::UndoUnavailable { .. }
            | EngineError::NeedsSchemaConfirmation { .. } => CoreError::Unavailable {
                reason: error.to_string(),
            },
            EngineError::InvalidIgnoreBeforeDate { .. }
            | EngineError::NotFound { .. }
            | EngineError::DeckRefused(_)
            | EngineError::NotetypeRefused(_)
            | EngineError::NoteNotFound { .. } => CoreError::InvalidArgument {
                message: error.to_string(),
            },
            EngineError::Unsupported { feature } => CoreError::Unsupported { feature },
            EngineError::InvalidArgument { message } => CoreError::InvalidArgument { message },
            EngineError::Internal { message } => CoreError::Internal { message },
            EngineError::Stale { .. }
            | EngineError::SchemaNotMarked { .. }
            | EngineError::Open(_)
            | EngineError::Storage(_)
            | EngineError::Media(_)
            | EngineError::Panicked { .. } => CoreError::Internal {
                message: error.to_string(),
            },
        }
    }
}

/// An export's error inside engine work (a job started over FFI), so it
/// comes back unchanged in the job's outcome.
impl From<CoreError> for EngineError {
    fn from(error: CoreError) -> EngineError {
        match error {
            CoreError::Cancelled => EngineError::Cancelled,
            CoreError::Busy { running_op } => EngineError::Busy { running_op },
            CoreError::Unavailable { reason } => EngineError::CollectionUnavailable { reason },
            CoreError::Unsupported { feature } => EngineError::Unsupported { feature },
            CoreError::InvalidArgument { message } => EngineError::InvalidArgument { message },
            CoreError::Internal { message } => EngineError::Internal { message },
            CoreError::MediaCommitPending { seq, message } => EngineError::MediaCommitPending {
                seq: meguri_engine::Seq(seq),
                message,
            },
            CoreError::CommitCompleted { seq, message } => EngineError::CommitCompleted {
                seq: meguri_engine::Seq(seq),
                message,
            },
        }
    }
}

impl CoreError {
    /// The message of a caught panic payload, for `Internal` errors.
    #[cfg(feature = "test-hooks")]
    pub(crate) fn from_panic(payload: &(dyn std::any::Any + Send)) -> CoreError {
        let message = if let Some(s) = payload.downcast_ref::<&str>() {
            (*s).to_owned()
        } else if let Some(s) = payload.downcast_ref::<String>() {
            s.clone()
        } else {
            "panic with a non-string payload".to_owned()
        };
        CoreError::Internal {
            message: format!("panic: {message}"),
        }
    }
}
