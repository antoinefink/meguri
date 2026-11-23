//! `sync/meguri-provider` MP7 over FFI: a schema-classified note type save
//! while Meguri Sync is ready runs as one job (round, save with the losses
//! the user confirmed, upload), and Upload… finishes one whose upload is
//! pending. The job is advanced by its caller, one phase at a time: wait for
//! the phase's job to end, then advance again.
use super::normal::{ProviderRoundRefusal, ProviderRoundResult, access_for, round_result};
use super::{ProviderConnection, ProviderCredentials, ProviderOwner};
use crate::deck_import::{DeckImportApplyOutcome, DeckImportConfirmation, DeckImportOptions, DeckImportSession};
use crate::mutations::MutationSummary;
use crate::notetype_change::{NotetypeChangeRequest, NotetypeChanged};
use crate::notetype_structure::{NotetypeRemoval, NotetypeRemoved, NotetypeRestore, NotetypeStructureDraft,
    NotetypeStructureSaved};
use crate::{CoreError, callback, jobs};
use meguri_engine::notetypes::{change::{self, ChangeNotetype}, remove::{self, RemoveNotetype},
    restore::{self, RestoreNotetype}, structure::{self, StructureDraft}};
use meguri_host::{DeckImportAnswer, MeguriAccessSource, MeguriBootstrapError, MeguriStructureOutcome, MeguriStructureRefusal,
    MeguriStructureStage, MeguriStructureStep, MeguriStructureStop, MeguriStructureTask,
    MeguriStructureUploadOutcome, MeguriStructureWork, MeguriUploadPending};
use meguri_model::ids::NotetypeId;
use std::sync::{Arc, Mutex, PoisonError};

/// One of the four operations that delete content (`notetypes/draft-save`
/// D8), with what its sheet or question showed.
#[derive(Clone, uniffi::Enum)]
pub enum ProviderStructureOp {
    Save { draft: NotetypeStructureDraft },
    Change { request: NotetypeChangeRequest },
    Remove { request: NotetypeRemoval },
    Restore { request: NotetypeRestore },
    /// MP7.1: a deck package import whose confirmation showed the sync
    /// paragraph. `confirmation` is the one a previous apply returned; none
    /// asks the plan's structural reasons first.
    Import { session: Arc<DeckImportSession>, options: DeckImportOptions, confirmation: Option<Arc<DeckImportConfirmation>> },
}
/// What the op did, once it committed.
#[derive(Clone, uniffi::Enum)]
pub enum ProviderStructureSaved {
    Structure { saved: NotetypeStructureSaved },
    Changed { changed: NotetypeChanged },
    Removed { removed: NotetypeRemoved },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderStructureStage { Round, Save, Facts, ServerCopy, Upload, MediaPrepare, Media }
/// The job did not start: nothing was saved and nothing is pending.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderStructureRefusal {
    /// A round, a service change or another structure save is running.
    Busy,
    /// Meguri Sync is not the ready service: the save runs alone (D5).
    NotReady,
    /// An earlier structure upload is pending: Upload… comes first.
    UploadPending,
    /// Upload… with no pending structure upload.
    NothingPending,
    Round { refusal: ProviderRoundRefusal },
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderUploadPending { Unreachable, Busy, Failed, Cancelled, LocalChanged, Storage, SignIn }
/// The save is committed; what became of its upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum ProviderStructureUpload {
    /// The server holds the collection. With `media_complete` the binding
    /// is ready again; otherwise setup's media step continues (MP6.5).
    Uploaded { media_complete: bool },
    /// The op changed no structure: an ordinary round sends it.
    NotNeeded,
    /// The needs-you line with Upload… (MP7 step 5).
    Pending { reason: ProviderUploadPending },
    /// MP6.3's collection choice; the upload stays pending meanwhile.
    ChooseCollection,
    /// MP6.9 settles the upload first.
    AcceptanceUnknown,
    /// A retained setup operation owns the next step.
    SetupInProgress,
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderStructureOutcome {
    /// Nothing was saved and nothing is pending: the round found that the
    /// server requires a full sync (MP6.3's choice; the draft stays).
    FullSyncRequired,
    /// Nothing was saved: the round failed, was cancelled or refused, or
    /// lost its acknowledgement. `round` reports it as any round.
    RoundStopped { round: Arc<ProviderRoundResult> },
    /// Nothing was saved: cancelled before the save.
    Cancelled,
    /// Nothing was saved: the job's record could not be written.
    Unavailable,
    /// MP7.1 step 3: the import's apply refused or asked to confirm, and
    /// wrote nothing. Nothing is pending; `import_outcome` has the typed
    /// refusal or the confirmation.
    ImportNotCommitted,
    Saved { upload: ProviderStructureUpload },
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderStructureStep {
    /// Wait for this job to end, then advance again.
    Running { stage: ProviderStructureStage, handle: jobs::JobHandle },
    Finished { outcome: ProviderStructureOutcome },
}
#[derive(Clone, uniffi::Enum)]
pub enum ProviderStructureStart {
    Started { job: Arc<ProviderStructureJob> },
    Refused { refusal: ProviderStructureRefusal },
}
type Saved = Arc<Mutex<Option<ProviderStructureSaved>>>;
#[derive(uniffi::Object)]
pub struct ProviderStructureJob { owner: Arc<ProviderOwner>, task: Arc<MeguriStructureTask>, saved: Saved,
    /// The import's own answer, for a job whose op is an import.
    import: Option<DeckImportAnswer> }

fn refusal(value: MeguriStructureRefusal) -> ProviderStructureRefusal {
    use MeguriStructureRefusal as R;
    match value {
        R::Busy => ProviderStructureRefusal::Busy,
        R::NotReady => ProviderStructureRefusal::NotReady,
        R::UploadPending => ProviderStructureRefusal::UploadPending,
        R::NothingPending => ProviderStructureRefusal::NothingPending,
        R::Round(refusal) => ProviderStructureRefusal::Round { refusal: refusal.into() },
        R::Engine(meguri_engine::EngineError::Busy { .. }) => ProviderStructureRefusal::Busy,
        R::Engine(_) => ProviderStructureRefusal::Unavailable,
    }
}
fn upload(value: MeguriStructureUploadOutcome) -> ProviderStructureUpload {
    use MeguriStructureUploadOutcome as U;
    match value {
        U::Uploaded { media_complete } => ProviderStructureUpload::Uploaded { media_complete },
        U::NotNeeded => ProviderStructureUpload::NotNeeded,
        U::Pending(reason) => ProviderStructureUpload::Pending { reason: match reason {
            MeguriUploadPending::Unreachable => ProviderUploadPending::Unreachable,
            MeguriUploadPending::Busy => ProviderUploadPending::Busy,
            MeguriUploadPending::Failed => ProviderUploadPending::Failed,
            MeguriUploadPending::Cancelled => ProviderUploadPending::Cancelled,
            MeguriUploadPending::LocalChanged => ProviderUploadPending::LocalChanged,
            MeguriUploadPending::Storage => ProviderUploadPending::Storage,
            MeguriUploadPending::SignIn => ProviderUploadPending::SignIn,
        } },
        U::ChooseCollection => ProviderStructureUpload::ChooseCollection,
        U::AcceptanceUnknown => ProviderStructureUpload::AcceptanceUnknown,
        U::SetupInProgress => ProviderStructureUpload::SetupInProgress,
    }
}
/// The op as the job's save phase. Its inputs are checked before the job
/// starts; its typed result is kept for `saved`.
fn work(op: ProviderStructureOp, host: &meguri_host::CoreHost)
    -> Result<(MeguriStructureWork, Saved, Option<DeckImportAnswer>), CoreError> {
    let saved = Saved::default();
    let op = match op {
        ProviderStructureOp::Import { session, options, confirmation } => {
            // A closed session imports nothing: the caller's own error, before a round runs.
            let session = session.shared().ok_or_else(|| CoreError::InvalidArgument { message: "the import session is closed".into() })?;
            let (work, answer) = host.deck_import_work(session, options.into(), confirmation.map(|confirmation| confirmation.token()));
            return Ok((work, saved, Some(answer)));
        }
        other => other,
    };
    let output = saved.clone();
    let keep = move |value| *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
    let work: MeguriStructureWork = match op {
        ProviderStructureOp::Save { draft } => {
            let draft = StructureDraft::try_from(draft)?;
            Box::new(move |collection, job| {
                let result = structure::apply(collection, draft, job.cancel.clone(), job.progress.clone())?;
                keep(ProviderStructureSaved::Structure { saved: result.into() });
                Ok(())
            })
        }
        ProviderStructureOp::Change { request } => {
            // The collection the info query read, and no other.
            let planned = request.generation;
            let current = host.collection()?.generation().0;
            if current != planned { return Err(CoreError::StaleGeneration { planned, current }); }
            let request = ChangeNotetype::try_from(request)?;
            Box::new(move |collection, job| {
                // The collection the job runs on is the one the info query read.
                let current = collection.generation().0;
                if current != planned { return Err(meguri_engine::EngineError::StaleGeneration {
                    planned: meguri_engine::Generation(planned), current: meguri_engine::Generation(current) }); }
                let changed = change::change(collection, request, job.cancel.clone(), job.progress.clone())?;
                keep(ProviderStructureSaved::Changed { changed: NotetypeChanged {
                    summary: MutationSummary::of(&changed), notes: changed.value.notes } });
                Ok(())
            })
        }
        ProviderStructureOp::Remove { request } => {
            let input = RemoveNotetype { notetype: NotetypeId(request.notetype_id),
                confirmed_notes: request.confirmed_notes, confirmed_cards: request.confirmed_cards };
            Box::new(move |collection, job| {
                let removed = remove::remove(collection, input, job.cancel.clone(), job.progress.clone())?;
                keep(ProviderStructureSaved::Removed { removed: NotetypeRemoved {
                    summary: MutationSummary::of(&removed), id: removed.value.id.0, removed: removed.value.removed,
                    notes_removed: removed.value.notes_removed, cards_removed: removed.value.cards_removed } });
                Ok(())
            })
        }
        ProviderStructureOp::Restore { request } => {
            let input = RestoreNotetype { notetype: NotetypeId(request.notetype_id), kind: request.kind.map(Into::into),
                confirmed: request.confirmed.try_into()?,
                digest: request.digest.map(crate::notetype_structure::digest).transpose()? };
            Box::new(move |collection, job| {
                let result = restore::restore(collection, input, job.cancel.clone(), job.progress.clone())?;
                keep(ProviderStructureSaved::Structure { saved: result.into() });
                Ok(())
            })
        }
        ProviderStructureOp::Import { .. } => unreachable!("handled above"),
    };
    Ok((work, saved, None))
}
impl ProviderConnection {
    fn structure_access(&self, credentials: ProviderCredentials) -> MeguriAccessSource {
        let (owner, inner) = (self.owner.clone(), self.inner.clone());
        Arc::new(move || access_for(&owner, &inner, credentials.clone()).map_err(|_| MeguriBootstrapError::ConnectionChanged))
    }
    fn structure_start(&self, start: Result<Arc<MeguriStructureTask>, MeguriStructureRefusal>, saved: Saved,
        import: Option<DeckImportAnswer>) -> ProviderStructureStart {
        match start {
            Ok(task) => ProviderStructureStart::Started { job: Arc::new(ProviderStructureJob { owner: self.owner.clone(), task, saved, import }) },
            Err(error) => ProviderStructureStart::Refused { refusal: refusal(error) },
        }
    }
}
#[uniffi::export]
impl ProviderConnection {
    /// MP7's job for one accepted sheet or question. `reachable` false is
    /// D4's unreachable case: no round, the save, and the upload left
    /// pending. Invalid input is this call's error; nothing started.
    pub fn start_structure_save(&self, credentials: ProviderCredentials, op: ProviderStructureOp, reachable: bool)
        -> Result<ProviderStructureStart, CoreError> {
        callback::enter("ProviderConnection.start_structure_save");
        self.owner.validate()?;
        let (work, saved, import) = work(op, &self.owner.host)?;
        let start = self.owner.host.start_meguri_structure_save(self.structure_access(credentials), work, reachable);
        Ok(self.structure_start(start, saved, import))
    }
    /// MP7 step 5's Upload…: no round and no further question.
    pub fn start_structure_upload(&self, credentials: ProviderCredentials) -> Result<ProviderStructureStart, CoreError> {
        callback::enter("ProviderConnection.start_structure_upload");
        self.owner.validate()?;
        let start = self.owner.host.start_meguri_structure_upload(self.structure_access(credentials));
        Ok(self.structure_start(start, Saved::default(), None))
    }
}
#[uniffi::export]
impl ProviderStructureJob {
    /// The running phase, or the outcome. The op's own refusal or failure
    /// (`LossesChanged`, `Stale`, a failed safety backup, a cancelled plan)
    /// is this call's error, as the local save's job reports it: nothing was
    /// saved and nothing is pending.
    pub fn advance(&self) -> Result<ProviderStructureStep, CoreError> {
        callback::enter("ProviderStructureJob.advance");
        let step = self.owner.host.advance_meguri_structure(&self.task)
            .map_err(|_| CoreError::Unavailable { reason: "Meguri structure job owner changed".into() })?;
        Ok(match step {
            MeguriStructureStep::Running { stage, job } => ProviderStructureStep::Running { handle: jobs::wrap(job), stage: match stage {
                MeguriStructureStage::Round => ProviderStructureStage::Round,
                MeguriStructureStage::Save => ProviderStructureStage::Save,
                MeguriStructureStage::Facts => ProviderStructureStage::Facts,
                MeguriStructureStage::ServerCopy => ProviderStructureStage::ServerCopy,
                MeguriStructureStage::Upload => ProviderStructureStage::Upload,
                MeguriStructureStage::MediaPrepare => ProviderStructureStage::MediaPrepare,
                MeguriStructureStage::Media => ProviderStructureStage::Media,
            } },
            MeguriStructureStep::Finished(outcome) => ProviderStructureStep::Finished { outcome: match &*outcome {
                // The import's own refusal or confirmation is typed: `import_outcome` has it.
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::Refused(_)) if self.import_answered() =>
                    ProviderStructureOutcome::ImportNotCommitted,
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::Refused(error)) => return Err(error.clone().into()),
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::FullSyncRequired) => ProviderStructureOutcome::FullSyncRequired,
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::Round { task, .. }) =>
                    ProviderStructureOutcome::RoundStopped { round: round_result(self.owner.clone(), task.clone()) },
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::Cancelled) => ProviderStructureOutcome::Cancelled,
                MeguriStructureOutcome::NotSaved(MeguriStructureStop::Unavailable) => ProviderStructureOutcome::Unavailable,
                MeguriStructureOutcome::Saved(value) => ProviderStructureOutcome::Saved { upload: upload(*value) },
            } },
        })
    }
    /// Honoured where MP7 allows: before the save it ends the job with
    /// nothing saved; after it, the upload is left pending. An upload that
    /// was dispatched is never taken back.
    pub fn cancel(&self) -> Result<(), CoreError> {
        callback::enter("ProviderStructureJob.cancel");
        self.task.cancel();
        Ok(())
    }
    /// The op's result once it committed.
    pub fn saved(&self) -> Result<Option<ProviderStructureSaved>, CoreError> {
        callback::enter("ProviderStructureJob.saved");
        Ok(self.saved.lock().unwrap_or_else(PoisonError::into_inner).clone())
    }
    /// For an import (MP7.1): what its apply answered, once it ran. The
    /// result of a committed import (with `media_pending` after
    /// `MediaCommitPending`), the confirmation to show, or the refusal.
    /// None while the job has not reached its import, and for any other op.
    pub fn import_outcome(&self) -> Result<Option<DeckImportApplyOutcome>, CoreError> {
        callback::enter("ProviderStructureJob.import_outcome");
        let answer = self.import.as_ref().and_then(|answer| answer.lock().unwrap_or_else(PoisonError::into_inner).clone());
        Ok(answer.map(crate::deck_import::apply_outcome))
    }
}
impl ProviderStructureJob {
    fn import_answered(&self) -> bool {
        self.import.as_ref().is_some_and(|answer| answer.lock().unwrap_or_else(PoisonError::into_inner).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn connection(root: &std::path::Path) -> ProviderConnection {
        use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
        let directory = root.join("collection"); std::fs::create_dir_all(&directory).unwrap();
        let config = meguri_host::HostConfig::new(directory, root.join("backups"), root.join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)), Arc::new(SeededRandom::new(89)),
            Arc::new(meguri_storage::space::StatvfsSpace));
        meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
        let host = Arc::new(meguri_host::CoreHost::open(config).unwrap());
        let generation = host.cloud_connection_generation().unwrap();
        let inner = host.configure_meguri_connection("https://structure.synthetic.example/").unwrap();
        ProviderConnection { owner: Arc::new(ProviderOwner { host, generation }), inner }
    }
    fn credentials(connection: &ProviderConnection) -> ProviderCredentials {
        ProviderCredentials { connection: super::super::information(&connection.inner),
            username: "synthetic".into(), hostkey: "synthetic-key".into() }
    }
    // Without a ready Meguri Sync binding the save runs alone (D5): the job
    // refuses before its op, and nothing is pending.
    #[test]
    fn without_a_ready_binding_the_job_refuses_and_runs_no_op() {
        let root = tempfile::tempdir().unwrap(); let connection = connection(root.path());
        let remove = ProviderStructureOp::Remove { request: NotetypeRemoval { notetype_id: 1, confirmed_notes: 0, confirmed_cards: 0 } };
        for reachable in [true, false] {
            assert!(matches!(connection.start_structure_save(credentials(&connection), remove.clone(), reachable).unwrap(),
                ProviderStructureStart::Refused { refusal: ProviderStructureRefusal::NotReady }));
        }
        assert!(matches!(connection.start_structure_upload(credentials(&connection)).unwrap(),
            ProviderStructureStart::Refused { refusal: ProviderStructureRefusal::NothingPending }));
        assert!(!connection.owner.structure_upload_pending().unwrap());
        // A refused job leaves the lane free.
        assert!(matches!(connection.start_structure_upload(credentials(&connection)).unwrap(),
            ProviderStructureStart::Refused { refusal: ProviderStructureRefusal::NothingPending }));
        // Invalid input is the call's own error, before anything starts.
        let restore = ProviderStructureOp::Restore { request: NotetypeRestore { notetype_id: 1, kind: None,
            confirmed: Default::default(), digest: Some(vec![1, 2, 3]) } };
        assert!(matches!(connection.start_structure_save(credentials(&connection), restore, true),
            Err(CoreError::InvalidArgument { .. })));
        connection.owner.host.close().unwrap();
    }
    /// MP7.1: without a ready binding an import is not this job's (it runs
    /// alone, `package/apkg-import` I14). The refusal leaves the session
    /// open; a closed session is the caller's error, before anything starts.
    #[test]
    fn an_import_without_a_ready_binding_is_refused_and_keeps_its_session() {
        use crate::deck_import::{DeckImportOptions, DeckImportSession, DeckImportUpdate};
        use meguri_package::{CollectionPackageWriteContext, Finalisation, apkg, write_collection_package_from_live};
        let root = tempfile::tempdir().unwrap(); let connection = connection(root.path());
        let host = &connection.owner.host;
        let context = host.deck_package_context();
        let package = root.path().join("deck.apkg");
        {
            let collection = host.collection().unwrap();
            let lease = collection.reader(meguri_storage::ReaderRole::Maintenance).unwrap();
            write_collection_package_from_live(&lease, &package, &CollectionPackageWriteContext {
                collection_directory: &context.collection_directory, purpose: meguri_storage::atomic_file::Purpose::Export,
                finalisation: Finalisation::Replace, clock: context.clock.as_ref(), random: context.random.as_ref(),
                space: context.space.as_ref(), is_cancelled: &|| false }).unwrap();
        }
        let inspected = apkg::inspect(&package, "deck.apkg", &context, &meguri_engine::CancelToken::new(),
            &meguri_engine::ProgressSink::new()).unwrap();
        let session = DeckImportSession::register(inspected);
        let options = DeckImportOptions { merge_notetypes: false, update_notes: DeckImportUpdate::IfNewer,
            update_notetypes: DeckImportUpdate::IfNewer, with_scheduling: false, with_deck_configs: false };
        let op = ProviderStructureOp::Import { session: session.clone(), options, confirmation: None };
        for reachable in [true, false] {
            assert!(matches!(connection.start_structure_save(credentials(&connection), op.clone(), reachable).unwrap(),
                ProviderStructureStart::Refused { refusal: ProviderStructureRefusal::NotReady }));
        }
        assert!(session.shared().is_some(), "the refused job left the session open");
        assert!(!connection.owner.structure_upload_pending().unwrap());
        session.close().unwrap();
        assert!(matches!(connection.start_structure_save(credentials(&connection), op, true),
            Err(CoreError::InvalidArgument { .. })));
        drop(session);
        connection.owner.host.close().unwrap();
    }
}
