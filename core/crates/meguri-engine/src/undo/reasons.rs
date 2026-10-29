//! Why a step cannot be undone, why the history was cleared, and what each
//! op source does to the history (`specs/engine/undo.md` M2, M3, M6).

use std::fmt::{self, Write as _};

use meguri_model::changes::OpSource;

/// The answer `peek` gives when no step is available, and the reason an undo
/// or redo is refused (M3, the table of reasons). The UI has its own words
/// for each (R365, R366, R462, R1796); logs and errors show the variant's
/// name in snake case (`cleared_by_rollover`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DisabledReason {
    /// There is nothing to undo: a new session, a history undone to its
    /// bottom, or a session scope that finds another session's step (or a
    /// step of no session) on top.
    None,
    /// Day rollover emptied the history.
    ClearedByRollover,
    /// The incoming changes of a normal sync dropped the steps below (M9.4).
    ClearedBySync,
    /// A one-way sync, a restore or a `.colpkg` import replaced the
    /// collection.
    ClearedByOneWay,
    /// An op that records no step changed rows (a non-undoable user op, the
    /// optimizer, maintenance), or Check Database ran.
    ClearedByNonUndoableOp,
    /// A local-server write or a change from outside cut the history, or an
    /// undo's check found a row of its step changed (M4.2, M7).
    ChangedExternally,
    /// A sync uploaded the step on top (M9.2).
    AlreadyUploaded,
    /// The step on top is in the snapshot of a sync still running (M9.1).
    UploadPending,
}

impl fmt::Display for DisabledReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Each capital of the variant's name starts a lower-case word.
        let variant = format!("{self:?}");
        for (at, letter) in variant.char_indices() {
            if letter.is_ascii_uppercase() && at > 0 {
                f.write_char('_')?;
            }
            f.write_char(letter.to_ascii_lowercase())?;
        }
        Ok(())
    }
}

/// Why the whole history is cleared without an op (M6).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClearCause {
    /// The day changed (U40, `sched/day-clock` D9), even when the rollover
    /// op wrote nothing.
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
    /// CloudKit cuts at the newest targeted step, detaching a hit held run (M9.8).
    CloudTargetedDrop,
    /// Neither recorded nor clearing: the day clock's writes
    /// (`sched/day-clock` D10). Its rollover op clears the history itself
    /// when the day changes (M6, D9).
    Kept,
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
        OpSource::CloudIncoming | OpSource::CloudRekey => SourceEffect::CloudTargetedDrop,
        OpSource::Rollover => SourceEffect::Kept,
    }
}

#[cfg(test)]
mod tests {
    use super::DisabledReason;

    #[test]
    fn a_reason_shows_as_its_name_in_snake_case() {
        assert_eq!(DisabledReason::None.to_string(), "none");
        assert_eq!(
            DisabledReason::ClearedByNonUndoableOp.to_string(),
            "cleared_by_non_undoable_op"
        );
        assert_eq!(DisabledReason::UploadPending.to_string(), "upload_pending");
    }
}
