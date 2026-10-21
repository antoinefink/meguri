//! Production C5 service; E2 stays an independent read-only admission path.
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
    ReviewService, SessionEvent, SessionStartHook, StartResult,
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
