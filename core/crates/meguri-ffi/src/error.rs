//! The error every export returns (KTD9). Swift sees it as the generated
//! `CoreError` enum, and CoreBridge maps it onto `BridgeError`. Area units add
//! a variant only when Swift must react to it differently from these.

use std::fmt;

use crate::{error_types::*, lifecycle::CollectionOpenFailure, review_session::ReviewDisabledReason};
use meguri_engine::EngineError;

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Error)]
pub enum CoreError {
    DeckOptionsInvalid {
        issues: Vec<crate::deck_options::OptionIssue>,
    },
    SchedOpRefused {
        refusal: crate::sched_ops::SchedOpRefusal,
    },
    Stale {
        table: CoreTable,
        key: String,
    },
    StaleGeneration {
        planned: u64,
        current: u64,
    },
    UndoUnavailable {
        reason: ReviewDisabledReason,
    },
    NeedsSchemaConfirmation {
        reasons: Vec<SchemaChangeReason>,
    },
    NotetypeRefused {
        refusal: NotetypeRefusal,
    },
    DeckRefused {
        refusal: DeckRefusal,
    },
    NoteNotFound {
        id: i64,
    },
    NotFound {
        table: CoreTable,
        key: String,
    },
    InsufficientSpace {
        required: u64,
        available: u64,
    },
    Open {
        failure: CollectionOpenFailure,
    },
    SuspendDeadline {
        phase: String,
    },
    SessionActive,
    /// Invalid imported type-in replacement; no partial rendering is returned.
    InvalidTypeAnswerReplacement,
    /// The operation's `CancelToken` was cancelled before it finished.
    Cancelled,
    /// Another long operation holds the slot this one needs (R171).
    Busy {
        running_op: String,
    },
    /// The collection cannot take this call now, for example during an
    /// exclusive phase (KTD17).
    Unavailable {
        reason: String,
    },
    /// Compiled out for this platform or build: the iOS stubs of the server
    /// exports (KTD9) and the AnkiWeb switch (KTD21).
    Unsupported {
        feature: String,
    },
    /// The caller passed a value the core rejects.
    InvalidArgument {
        message: String,
    },
    /// A failure the caller cannot act on: a bug, or a caught panic.
    Internal {
        message: String,
    },
    /// The note write committed; recover attachments without repeating it.
    MediaCommitPending {
        seq: u64,
        message: String,
    },
    /// The write committed. Refresh its state without repeating it.
    CommitCompleted {
        seq: u64,
        message: String,
    },
}

impl fmt::Display for CoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            CoreError::DeckOptionsInvalid { .. } => write!(f, "{self:?}"),
            CoreError::SchedOpRefused { .. } => write!(f, "{self:?}"),
            CoreError::Stale { .. } => write!(f, "{self:?}"),
            CoreError::StaleGeneration { .. } => write!(f, "{self:?}"),
            CoreError::UndoUnavailable { .. } => write!(f, "{self:?}"),
            CoreError::NeedsSchemaConfirmation { .. } => write!(f, "{self:?}"),
            CoreError::NotetypeRefused { .. } => write!(f, "{self:?}"),
            CoreError::DeckRefused { .. } => write!(f, "{self:?}"),
            CoreError::NoteNotFound { .. } => write!(f, "{self:?}"),
            CoreError::NotFound { .. } => write!(f, "{self:?}"),
            CoreError::InsufficientSpace { .. } => write!(f, "{self:?}"),
            CoreError::Open { .. } => write!(f, "{self:?}"),
            CoreError::SuspendDeadline { .. } => write!(f, "{self:?}"),
            CoreError::SessionActive => f.write_str("review session is active"),
            CoreError::InvalidTypeAnswerReplacement => f.write_str("invalid_type_answer_replacement"),
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

/// Structured engine errors preserve actionable values across the bridge.
impl From<EngineError> for CoreError {
    fn from(error: EngineError) -> CoreError {
        match error {
            EngineError::DeckOptionsInvalid(issues) => CoreError::DeckOptionsInvalid {
                issues: issues.into_iter().map(Into::into).collect(),
            },
            EngineError::SchedOp(reason) => CoreError::SchedOpRefused { refusal: reason.into() },
            EngineError::InvalidTypeAnswerReplacement => CoreError::InvalidTypeAnswerReplacement,
            EngineError::Cancelled => CoreError::Cancelled,
            EngineError::Busy { running_op } => CoreError::Busy { running_op },
            EngineError::SessionActive => CoreError::SessionActive,
            EngineError::CollectionUnavailable { reason } => CoreError::Unavailable { reason },
            EngineError::SuspendDeadline { phase } => CoreError::SuspendDeadline { phase },
            EngineError::MediaCommitPending { seq, message } => CoreError::MediaCommitPending { seq: seq.0, message },
            EngineError::CommitCompleted { seq, message } => CoreError::CommitCompleted { seq: seq.0, message },
            EngineError::Stale { table, key } => CoreError::Stale {
                table: table.into(),
                key,
            },
            EngineError::StaleGeneration { planned, current } => CoreError::StaleGeneration {
                planned: planned.0,
                current: current.0,
            },
            EngineError::UndoUnavailable { reason } => CoreError::UndoUnavailable { reason: reason.into() },
            EngineError::NeedsSchemaConfirmation { reasons } => CoreError::NeedsSchemaConfirmation {
                reasons: reasons.into_iter().map(Into::into).collect(),
            },
            EngineError::NotetypeRefused(refusal) => CoreError::NotetypeRefused {
                refusal: refusal.into(),
            },
            EngineError::DeckRefused(refusal) => CoreError::DeckRefused {
                refusal: refusal.into(),
            },
            EngineError::NoteNotFound { id } => CoreError::NoteNotFound { id: id.0 },
            EngineError::NotFound { table, key } => CoreError::NotFound {
                table: table.into(),
                key,
            },
            EngineError::Storage(meguri_storage::StorageError::InsufficientSpace { required, available }) => {
                CoreError::InsufficientSpace { required, available }
            }
            EngineError::Open(failure) => CoreError::Open {
                failure: failure.into(),
            },
            EngineError::InvalidIgnoreBeforeDate { .. } => CoreError::InvalidArgument {
                message: error.to_string(),
            },
            EngineError::Unsupported { feature } => CoreError::Unsupported { feature },
            EngineError::InvalidArgument { message } => CoreError::InvalidArgument { message },
            EngineError::Internal { message } => CoreError::Internal { message },
            EngineError::SchemaNotMarked { .. }
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
            CoreError::DeckOptionsInvalid { issues } => {
                EngineError::DeckOptionsInvalid(issues.into_iter().map(Into::into).collect())
            }
            CoreError::SchedOpRefused { refusal } => EngineError::SchedOp(refusal.into()),
            CoreError::Stale { table, key } => EngineError::Stale {
                table: table.into(),
                key,
            },
            CoreError::StaleGeneration { planned, current } => EngineError::StaleGeneration {
                planned: meguri_engine::Generation(planned),
                current: meguri_engine::Generation(current),
            },
            CoreError::UndoUnavailable { reason } => EngineError::UndoUnavailable { reason: reason.into() },
            CoreError::NeedsSchemaConfirmation { reasons } => EngineError::NeedsSchemaConfirmation {
                reasons: reasons.into_iter().map(Into::into).collect(),
            },
            CoreError::NotetypeRefused { refusal } => EngineError::NotetypeRefused(refusal.into()),
            CoreError::DeckRefused { refusal } => EngineError::DeckRefused(refusal.into()),
            CoreError::NoteNotFound { id } => EngineError::NoteNotFound {
                id: meguri_model::ids::NoteId(id),
            },
            CoreError::NotFound { table, key } => EngineError::NotFound {
                table: table.into(),
                key,
            },
            CoreError::InsufficientSpace { required, available } => {
                EngineError::Storage(meguri_storage::StorageError::InsufficientSpace { required, available })
            }
            CoreError::Open { failure } => EngineError::Open(failure.into()),
            CoreError::SuspendDeadline { phase } => EngineError::SuspendDeadline { phase },
            CoreError::SessionActive => EngineError::SessionActive,

            CoreError::InvalidTypeAnswerReplacement => EngineError::InvalidTypeAnswerReplacement,
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

impl From<meguri_storage::StorageError> for CoreError {
    fn from(error: meguri_storage::StorageError) -> Self {
        EngineError::from(error).into()
    }
}
