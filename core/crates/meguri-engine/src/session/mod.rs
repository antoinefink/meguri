//! Production C5 service; E2 stays an independent read-only admission path.
pub(crate) mod filtered_lifecycle;
mod frozen;
pub mod held;
pub(crate) mod journal;
mod service;
#[cfg(feature = "test-hooks")]
pub use journal::JournalStage;
#[cfg(feature = "test-hooks")]
pub use service::RenderStage;
pub use service::{
    AnswerOutcome, CurrentChange, Head, HeadDiff, HeadItem, HistoryRestore, HomePreset, Marks, PeekResult, Refusal,
    ReviewService, ReviewTerminalState, SessionEvent, SessionStartHook, StartResult, UnqueuedSession,
};
pub(crate) use service::{Application, ProductionRestore};

pub(crate) fn wal_covered(shared: &crate::executor::ExecShared) -> Result<(), crate::EngineError> {
    let journal = shared
        .review_journal
        .lock()
        .unwrap()
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(journal) = journal {
        journal.covered(shared.generation(), shared.last_seq())?;
    }
    Ok(())
}
pub(crate) fn retire_witnesses(
    shared: &crate::executor::ExecShared,
    writer: &meguri_storage::Writer,
) -> Result<(), crate::EngineError> {
    let journal = shared
        .review_journal
        .lock()
        .unwrap()
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(journal) = journal {
        journal.check(shared.generation())?;
        if journal.has_committed() {
            writer.sync_wal()?;
            journal.covered(shared.generation(), shared.last_seq())?;
        }
    }
    Ok(())
}
pub(crate) fn retire_incarnation(shared: &crate::executor::ExecShared) -> Result<(), crate::EngineError> {
    let journal = shared
        .review_journal
        .lock()
        .unwrap()
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(journal) = journal {
        journal.barrier()?;
        journal.retire()?;
    }
    *shared.review_journal.lock().unwrap() = None;
    Ok(())
}

/// Recovery can own either an already quiesced live binding or no provider at
/// all. Never edit its file while a retained worker still owns that binding.
pub(crate) fn prepare_recovery_replacement(
    shared: &crate::executor::ExecShared,
) -> Result<Option<journal::RecoveryJournal>, crate::EngineError> {
    let bound = shared
        .review_journal
        .lock()
        .unwrap()
        .as_ref()
        .and_then(std::sync::Weak::upgrade);
    if let Some(bound) = bound {
        bound.check_identity(journal::recovery_identity(&shared.path)?)?;
        retire_incarnation(shared)?;
        Ok(None)
    } else {
        journal::RecoveryJournal::prepare(&shared.path)
    }
}
