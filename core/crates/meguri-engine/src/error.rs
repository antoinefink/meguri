//! The engine's error. Area units add a variant only when callers must react
//! to it differently from these; meguri-ffi maps them onto `CoreError`.

use std::fmt;

use meguri_model::ids::NotetypeId;
use meguri_storage::{OpenError, StorageError, Table};

use crate::lifecycle::Generation;
use crate::notetypes::NotetypeRefusal;
use crate::undo::reasons::DisabledReason;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EngineError {
    /// The file export must restart against fresh source authority.
    ExportSourceChanged,
    /// Correct an unsafe or unreadable media item before retrying.
    ExportSourceInvalid,
    /// The collection or media exceeds the file format's supported limits.
    ExportFormatLimit,
    /// Selected ids exceed the export time bound; run Check Database.
    ExportInvalidIds,
    /// Selected records or references need Check Database.
    ExportNeedsCheckDatabase,
    /// Normal sync aborts its merge with the original typed preparation fault.
    SyncNote(meguri_model::note::SyncNoteFault),
    DeckOptionsInvalid(Vec<meguri_sched::options::Issue>),
    SchedOp(crate::sched::ops::Refusal),
    /// Find and Replace refused (`browser/find-replace` F11): nothing was
    /// written.
    FindReplace(crate::bulk::find_replace::Refusal),
    /// Imported type-in replacement template is malformed (render/type-answer A4.5).
    InvalidTypeAnswerReplacement,
    SessionActive,
    CollectionCorrupt,
    InvalidIgnoreBeforeDate {
        preset: meguri_model::ids::DeckConfigId,
    },
    SuspendDeadline {
        phase: String,
    },
    NotFound {
        table: Table,
        key: String,
    },
    DeckRefused(crate::decks::Refusal),
    /// An editor attempted to save a note deleted since it was opened.
    NoteNotFound {
        id: meguri_model::ids::NoteId,
    },
    /// A precondition of the op failed (`engine/op-model` O7): the row it
    /// was planned on changed. Nothing was written.
    Stale {
        table: Table,
        key: String,
    },
    /// The op, handle or snapshot belongs to an older collection generation
    /// (`engine/lifecycle` L3).
    StaleGeneration {
        planned: Generation,
        current: Generation,
    },
    /// The collection cannot take this op now (`engine/lifecycle` L4, L6).
    CollectionUnavailable {
        reason: String,
    },
    /// Another long operation holds the slot this one needs (R171).
    Busy {
        running_op: String,
    },
    /// No step to undo or redo, or the core refuses the top one
    /// (`engine/undo` M4, M5): nothing was written.
    UndoUnavailable {
        reason: DisabledReason,
    },
    /// The op's or job's cancel token was cancelled before it committed.
    Cancelled,
    /// The storage guard (`notetypes/schema-changes` G6): the transaction
    /// changed this note type's field or template count and would leave
    /// `scm <= ls`. A bug in the op; nothing was written.
    SchemaNotMarked {
        notetype_id: NotetypeId,
    },
    /// A note type save refused (`notetypes/validation` V2, V3): nothing was
    /// written.
    NotetypeRefused(NotetypeRefusal),
    /// A note type structure op refused (`notetypes/structure-changes`
    /// S1.2, S12, `notetypes/restore-to-default` D7): nothing was written.
    NotetypeStructure(crate::notetypes::structure::StructureRefusal),
    /// The open gate refused or failed (`storage/open-and-connections` C8).
    Open(OpenError),
    Storage(StorageError),
    SafetyBackupFailed(Box<crate::ports::SafetyBackupFailure>),
    Media(meguri_storage::media::MediaError),
    /// The write committed and its changes were published. Never repeat it.
    CommitCompleted {
        seq: crate::Seq,
        message: String,
    },
    /// The note operation committed. Recover media; never repeat the op.
    MediaCommitPending {
        seq: crate::Seq,
        message: String,
    },
    /// The op panicked; its transaction was rolled back.
    Panicked {
        message: String,
    },
    /// The caller passed something the engine rejects.
    InvalidArgument {
        message: String,
    },
    /// Compiled out for this platform or build.
    Unsupported {
        feature: String,
    },
    /// A failure the caller cannot act on.
    Internal {
        message: String,
    },
}

impl EngineError {
    pub(crate) fn from_panic(payload: &(dyn std::any::Any + Send)) -> EngineError {
        EngineError::Panicked {
            message: panic_message(payload),
        }
    }

    pub(crate) fn internal(message: impl Into<String>) -> EngineError {
        EngineError::Internal {
            message: message.into(),
        }
    }

    pub(crate) fn unavailable(reason: impl Into<String>) -> EngineError {
        EngineError::CollectionUnavailable { reason: reason.into() }
    }
}

/// The text of a caught panic's payload.
pub(crate) fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    if let Some(s) = payload.downcast_ref::<&str>() {
        (*s).to_owned()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "panic with a non-string payload".to_owned()
    }
}

impl From<StorageError> for EngineError {
    fn from(error: StorageError) -> EngineError {
        EngineError::Storage(error)
    }
}

impl From<OpenError> for EngineError {
    fn from(error: OpenError) -> EngineError {
        EngineError::Open(error)
    }
}

impl fmt::Display for EngineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            EngineError::ExportSourceChanged => f.write_str("export source changed"),
            EngineError::ExportInvalidIds => f.write_str("export ids need Check Database"),
            EngineError::ExportNeedsCheckDatabase => f.write_str("export records need Check Database"),
            EngineError::ExportSourceInvalid => f.write_str("export source item refused"),
            EngineError::ExportFormatLimit => f.write_str("export format limit"),
            EngineError::SyncNote(_) => f.write_str("incoming collection contract"),
            EngineError::DeckOptionsInvalid(issues) => write!(f, "invalid deck options: {issues:?}"),
            EngineError::SchedOp(reason) => write!(f, "scheduling operation refused: {reason:?}"),
            EngineError::FindReplace(refusal) => write!(f, "find and replace refused: {refusal}"),
            EngineError::InvalidTypeAnswerReplacement => f.write_str("invalid_type_answer_replacement"),
            EngineError::SessionActive => f.write_str("review session is active"),
            EngineError::InvalidIgnoreBeforeDate { preset } => {
                write!(f, "invalid ignore-before date for preset {}", preset.0)
            }
            EngineError::SuspendDeadline { phase } => write!(f, "suspend deadline exceeded: {phase}"),
            EngineError::NotFound { table, key } => write!(f, "not found: {table} {key}"),
            EngineError::DeckRefused(reason) => f.write_str(reason.kind()),
            EngineError::NoteNotFound { id } => write!(f, "note {} no longer exists", id.0),
            EngineError::Stale { table, key } => write!(f, "stale: row {key} of {table} changed since it was read"),
            EngineError::StaleGeneration { planned, current } => write!(
                f,
                "stale generation: planned on generation {}, the collection is at {}",
                planned.0, current.0
            ),
            EngineError::CollectionCorrupt => f.write_str("collection corrupt"),
            EngineError::CollectionUnavailable { reason } => write!(f, "collection unavailable: {reason}"),
            EngineError::Busy { running_op } => write!(f, "busy: {running_op} is running"),
            EngineError::UndoUnavailable { reason } => write!(f, "undo unavailable: {reason}"),
            EngineError::Cancelled => f.write_str("cancelled"),
            EngineError::SchemaNotMarked { notetype_id } => write!(
                f,
                "schema not marked: note type {} changed its field or template count with scm <= ls",
                notetype_id.0
            ),
            EngineError::NotetypeRefused(refusal) => write!(f, "note type refused: {refusal}"),
            EngineError::NotetypeStructure(refusal) => write!(f, "note type structure change refused: {refusal}"),
            EngineError::Open(error) => write!(f, "open: {error}"),
            EngineError::Storage(error) => write!(f, "storage: {error}"),
            EngineError::SafetyBackupFailed(failure) => write!(f, "safety backup failed: {failure:?}"),
            EngineError::Media(error) => write!(f, "media: {error}"),
            EngineError::CommitCompleted { seq, message } => {
                write!(f, "operation {} committed; post-commit failure: {message}", seq.0)
            }
            EngineError::MediaCommitPending { seq, message } => {
                write!(f, "operation {} committed; media recovery pending: {message}", seq.0)
            }
            EngineError::Panicked { message } => write!(f, "panic: {message}"),
            EngineError::InvalidArgument { message } => write!(f, "invalid argument: {message}"),
            EngineError::Unsupported { feature } => write!(f, "unsupported: {feature}"),
            EngineError::Internal { message } => write!(f, "internal error: {message}"),
        }
    }
}

impl std::error::Error for EngineError {}
