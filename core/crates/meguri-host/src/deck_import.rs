//! Deck package import (`package/apkg-import`) at the host: what a session
//! needs from this host, and `sync/meguri-provider#MP7.1`'s write step, the
//! import's apply inside the structure job.
use crate::{CoreHost, MeguriStructureWork};
use meguri_engine::EngineError;
use meguri_package::apkg::{self, ApkgImportError, ConfirmationToken, ImportOptions, ImportResult,
    ImportSession};
use std::sync::{Arc, Mutex, PoisonError};

/// What the import's apply answered inside the job: its typed result or
/// refusal, kept for the sheet. Empty until the write step ran.
pub type DeckImportAnswer = Arc<Mutex<Option<Result<ImportResult, ApkgImportError>>>>;

impl CoreHost {
    /// MP7.1 step 3: the apply of `session` as the structure job's write
    /// step. It asks the apply to confirm structure, so a plan with a
    /// structural reason that `token` does not cover writes nothing and the
    /// job ends as after `LossesChanged`. What the job is told:
    /// - a commit: `Ok`, and the job reads from `scm` whether an upload follows;
    /// - `MediaCommitPending`: the engine's error of that name, so the
    ///   upload is left pending (MP7.1 step 4);
    /// - any refusal before the commit: an error, with nothing written.
    pub fn deck_import_work(&self, session: Arc<ImportSession>, options: ImportOptions,
        token: Option<ConfirmationToken>) -> (MeguriStructureWork, DeckImportAnswer) {
        let answer = DeckImportAnswer::default();
        let (engine, output) = (self.engine().clone(), answer.clone());
        let work: MeguriStructureWork = Box::new(move |collection, job| {
            // The collection the job was admitted on, and no other.
            let current = engine.collection()?.generation();
            if current != collection.generation() {
                return Err(EngineError::StaleGeneration { planned: collection.generation(), current });
            }
            let outcome = apkg::apply_confirming(&engine, &session, options, token.as_ref(), true, &job.cancel, &job.progress);
            let told = match &outcome {
                Ok(_) => Ok(()),
                Err(ApkgImportError::MediaCommitPending(result)) => Err(EngineError::MediaCommitPending {
                    seq: meguri_engine::Seq(result.seq), message: "deck package import".into() }),
                // Committed, its result lost: never told as a refusal.
                Err(ApkgImportError::Io { detail }) if detail == apkg::COMMITTED_WITHOUT_RESULT => Err(EngineError::CommitCompleted {
                    seq: meguri_engine::Seq(0), message: "deck package import".into() }),
                Err(ApkgImportError::Cancelled) => Err(EngineError::Cancelled),
                Err(ApkgImportError::Engine(error)) => Err((**error).clone()),
                Err(_) => Err(EngineError::InvalidArgument { message: "the import was refused".into() }),
            };
            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome);
            told
        });
        (work, answer)
    }
}
