//! Why a step cannot be undone, why the history was cleared, and what each
//! op source does to the history (`specs/engine/undo.md` M2, M3, M6).

use std::fmt;

use meguri_model::changes::OpSource;

/// Why `peek` reports no available step, or why an undo or redo was refused
/// (M3). The UI words each one (R365, R366, R462, R1796).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisabledReason {
    /// Nothing to undo: session start, everything undone, or a session scope
    /// whose top step is not its own.
    None,
    ClearedByRollover,
    ClearedBySync,
    /// The collection was replaced: one-way sync, restore, `.colpkg` import.
    ClearedByOneWay,
    ClearedByNonUndoableOp,
    /// A local-server write or an outside change truncated the history, or
    /// an undo found its rows changed.
    ChangedExternally,
    AlreadyUploaded,
    /// The step is in the snapshot of a running sync.
    UploadPending,
}

impl DisabledReason {
    pub fn as_str(self) -> &'static str {
        match self {
            DisabledReason::None => "none",
            DisabledReason::ClearedByRollover => "cleared_by_rollover",
            DisabledReason::ClearedBySync => "cleared_by_sync",
            DisabledReason::ClearedByOneWay => "cleared_by_one_way",
            DisabledReason::ClearedByNonUndoableOp => "cleared_by_non_undoable_op",
            DisabledReason::ChangedExternally => "changed_externally",
            DisabledReason::AlreadyUploaded => "already_uploaded",
            DisabledReason::UploadPending => "upload_pending",
        }
    }
}

impl fmt::Display for DisabledReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Why the whole history is cleared without an op (M6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClearCause {
    /// The day changed (U40), even when rollover wrote nothing.
    Rollover,
    /// Check Database ran (U128).
    CheckDatabase,
    /// Another process changed the collection file (U32).
    ExternalChange,
}

impl ClearCause {
    pub fn reason(self) -> DisabledReason {
        match self {
            ClearCause::Rollover => DisabledReason::ClearedByRollover,
            ClearCause::CheckDatabase => DisabledReason::ClearedByNonUndoableOp,
            ClearCause::ExternalChange => DisabledReason::ChangedExternally,
        }
    }
}

/// What a committed op of a source does to the history when it changed
/// something (M2). Skip-undo is per op, not per source.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SourceEffect {
    /// Recorded as a new step; redo cleared.
    Recorded,
    /// Every step and redo step dropped, with this reason.
    ClearAll(DisabledReason),
    /// Never recorded; truncates at the deepest intersecting step (M7).
    TruncateIntersecting,
    /// The sync merge rule (M9.4, KTD7).
    SyncMerge,
}

/// The undo effect of every op source (M2, R2020).
pub fn source_effect(source: OpSource) -> SourceEffect {
    match source {
        OpSource::UserUndoable => SourceEffect::Recorded,
        OpSource::UserNonUndoable | OpSource::Optimizer | OpSource::Maintenance => {
            SourceEffect::ClearAll(DisabledReason::ClearedByNonUndoableOp)
        }
        OpSource::LocalApi => SourceEffect::TruncateIntersecting,
        OpSource::SyncIncoming => SourceEffect::SyncMerge,
        OpSource::Rollover => SourceEffect::ClearAll(DisabledReason::ClearedByRollover),
    }
}
