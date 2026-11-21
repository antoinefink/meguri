//! `sync/meguri-provider#MP7`: a note type structure save while Meguri Sync is
//! ready, as one job: a normal round, the save with the losses the user
//! confirmed, the upload. One retained phase at a time over the actual engine
//! job registry; the caller waits for the phase's job and asks again. No
//! scheduler lives here.
//!
//! What is durable, and what a kill leaves:
//! - Before the save: nothing, or the armed record with `scm <= ls`, which
//!   the next open, round or job withdraws. Nothing was saved.
//! - After the save: the record with `scm > ls`. The upload is pending, shown
//!   as needs you, until a completed upload or collection choice clears it,
//!   or until the save is undone.
//! - The job's exclusive admission is process state held by its task: it ends
//!   with the task or the process.
use crate::{CoreHost, ManualTransferTask, MeguriAccess, MeguriBootstrapDirection, MeguriBootstrapError,
    MeguriCollectionState, MeguriRoundResult, MeguriRoundStart, MeguriRoundTask};
use meguri_engine::{CancelToken, Collection, EngineError, Generation, JobContext, JobHandle, JobKind,
    device_state::DeviceState, schema_gate};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::{Job,
    oneway::{MediaPreview, Preview, TransferError},
    policy::{Acceptance, Operation, Phase, RemoteFacts},
    provider::{MeguriReadiness, MeguriReadyRefusal, MeguriRoundRefusal, MeguriStructureUpload, StructureBaseline}};
use std::sync::{Arc, Mutex, PoisonError, atomic::{AtomicBool, Ordering}};

/// Fresh access for each phase: a phase consumes its key and transport.
pub type MeguriAccessSource = Arc<dyn Fn() -> Result<MeguriAccess, MeguriBootstrapError> + Send + Sync>;
/// One of the four confirmed-losses ops (`notetypes/draft-save` D8), on the
/// collection the job was started on. It keeps its typed result itself.
pub type MeguriStructureWork = Box<dyn FnOnce(&Collection, &JobContext) -> Result<(), EngineError> + Send>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriStructureStage { Round, Save, Facts, ServerCopy, Upload, MediaPrepare, Media }
/// The job did not start: nothing was saved and nothing is pending.
#[derive(Debug)]
pub enum MeguriStructureRefusal {
    /// A round, a handoff or another structure job owns the lane.
    Busy,
    /// Meguri Sync is not the ready service: the save runs alone (D5).
    NotReady,
    /// An earlier structure upload is pending: Upload… comes first.
    UploadPending,
    /// Upload… with no pending structure upload.
    NothingPending,
    Round(MeguriRoundRefusal),
    Engine(EngineError),
}
/// The job ended before saving: nothing saved, nothing pending, admission released.
pub enum MeguriStructureStop {
    /// The op's own refusal or failure: `LossesChanged`, `Stale`, a failed
    /// safety backup, a cancelled plan. The question shows again from it.
    Refused(EngineError),
    /// The round found that the server requires a full sync: MP6.3's choice.
    FullSyncRequired,
    /// A failed, cancelled, refused or lost-acknowledgement round. `task`
    /// owns its finalization retry.
    Round { result: Arc<MeguriRoundResult>, task: Arc<MeguriRoundTask> },
    Cancelled,
    /// The record could not be written before the save.
    Unavailable,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriUploadPending {
    /// The server was not reached, or the caller found it unreachable.
    Unreachable,
    /// A review is in progress, or another owner holds the collection.
    Busy,
    /// The upload was prepared or sent and failed; nothing was accepted.
    Failed,
    Cancelled,
    /// This device changed while the upload was prepared.
    LocalChanged,
    /// Provider metadata could not be written.
    Storage,
}
/// The save is committed. What became of its upload.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriStructureUploadOutcome {
    /// The server holds the collection. `media_complete`: the binding is
    /// ready again; otherwise setup's media step continues (MP6.5).
    Uploaded { media_complete: bool },
    /// The op changed no structure: an ordinary round sends it.
    NotNeeded,
    /// Recorded as pending: the needs-you line with Upload….
    Pending(MeguriUploadPending),
    /// The server's facts are not the proved baseline, or none is proved:
    /// MP6.3's collection choice. The pending record stays.
    ChooseCollection,
    /// The upload may have reached the server: MP6.9 settles it first.
    AcceptanceUnknown,
    /// A retained setup operation owns the next step (finish, media).
    SetupInProgress,
}
pub enum MeguriStructureOutcome {
    NotSaved(MeguriStructureStop),
    Saved(MeguriStructureUploadOutcome),
}
#[derive(Clone)]
pub enum MeguriStructureStep {
    /// Wait for this job's end, then advance again.
    Running { stage: MeguriStructureStage, job: JobHandle },
    Finished(Arc<MeguriStructureOutcome>),
}

type SaveCell = Arc<Mutex<Option<Result<(), EngineError>>>>;
enum Stage {
    Round(Arc<MeguriRoundTask>),
    Save { job: JobHandle, result: SaveCell },
    Facts(ManualTransferTask<RemoteFacts>),
    ServerCopy(ManualTransferTask<Preview>),
    Upload(ManualTransferTask<Operation>),
    MediaPrepare(ManualTransferTask<MediaPreview>),
    Media(ManualTransferTask<Operation>),
}
impl Stage {
    fn job(&self) -> (MeguriStructureStage, JobHandle) {
        use MeguriStructureStage as S;
        match self {
            Self::Round(task) => (S::Round, task.handle()),
            Self::Save { job, .. } => (S::Save, job.clone()),
            Self::Facts(task) => (S::Facts, task.job.clone()),
            Self::ServerCopy(task) => (S::ServerCopy, task.job.clone()),
            Self::Upload(task) => (S::Upload, task.job.clone()),
            Self::MediaPrepare(task) => (S::MediaPrepare, task.job.clone()),
            Self::Media(task) => (S::Media, task.job.clone()),
        }
    }
}
struct TaskState {
    stage: Option<Stage>,
    /// The exclusive admission (MP7 step 1); dropped when the job ends.
    admission: Option<Arc<()>>,
    work: Option<MeguriStructureWork>,
    /// The server's `usn` as read before this job's server copy.
    read_usn: Option<i32>,
    finished: Option<Arc<MeguriStructureOutcome>>,
}
pub struct MeguriStructureTask {
    owner: Arc<()>,
    access: MeguriAccessSource,
    collection: Collection,
    generation: Generation,
    /// False when the caller found the server unreachable (D4): no round, no upload.
    reachable: bool,
    cancelled: AtomicBool,
    state: Mutex<TaskState>,
}
impl MeguriStructureTask {
    /// Honoured where MP7 allows: a cancelled round or plan saves nothing;
    /// after the save it only leaves the upload pending. An upload that was
    /// dispatched keeps its own point of no return (MP4).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        let state = self.state.lock().unwrap_or_else(PoisonError::into_inner);
        match &state.stage {
            Some(Stage::Round(task)) => task.cancel(false),
            Some(stage) => stage.job().1.cancel.cancel(),
            None => {}
        }
    }
    fn is_cancelled(&self) -> bool { self.cancelled.load(Ordering::SeqCst) }
}

use MeguriStructureOutcome as O;
use MeguriStructureUploadOutcome as U;
enum Next { Stage(Stage), Done(MeguriStructureOutcome) }
fn pending(reason: MeguriUploadPending) -> Next { Next::Done(O::Saved(U::Pending(reason))) }
fn transfer_pending(error: &TransferError) -> MeguriUploadPending {
    match error {
        TransferError::Cancelled | TransferError::Interrupted => MeguriUploadPending::Cancelled,
        TransferError::Stale(meguri_sync::policy::Boundary::LocalChanged) => MeguriUploadPending::LocalChanged,
        TransferError::Server(error) if matches!(error.kind, meguri_sync::ErrorKind::Offline
            | meguri_sync::ErrorKind::Network { .. }) => MeguriUploadPending::Unreachable,
        _ => MeguriUploadPending::Failed,
    }
}
fn bootstrap_pending(error: &MeguriBootstrapError) -> MeguriUploadPending {
    match error {
        MeguriBootstrapError::Busy | MeguriBootstrapError::Engine(EngineError::Busy { .. }) => MeguriUploadPending::Busy,
        MeguriBootstrapError::Transfer(error) => transfer_pending(error),
        MeguriBootstrapError::Ready(MeguriReadyRefusal::Storage(_)) => MeguriUploadPending::Storage,
        _ => MeguriUploadPending::Failed,
    }
}
/// A retained operation that sent nothing and replaced nothing.
fn unsent(op: &Operation) -> bool {
    matches!(op.acceptance, Acceptance::NotSent | Acceptance::Refused)
        && op.phase != Phase::Activating && op.activation_identity.is_none() && !op.collection_complete
}

impl CoreHost {
    fn structure_record(&self) -> Result<Option<MeguriStructureUpload>, EngineError> {
        Ok(self.provider.snapshot()?.meguri_structure)
    }
    /// Whether `record` is pending: the open collection is the one it names
    /// and still reads `scm > ls` (MP7 steps 5 and 6).
    fn structure_pending(&self, record: &MeguriStructureUpload) -> Result<bool, EngineError> {
        let Ok(collection) = self.collection() else { return Ok(false) };
        let settled = DeviceState::for_collection(collection.path()).settled_publication()?;
        if settled.map(|publication| publication.identity) != Some(record.publication()) { return Ok(false); }
        let col = collection.reader(ReaderRole::Query)?.col()?;
        Ok(schema_gate::is_modified(&col))
    }
    /// MP7 step 5's state, read only: a structure upload is pending for the
    /// open collection. The needs-you line shows while this holds.
    pub fn meguri_structure_upload_pending(&self) -> Result<bool, EngineError> {
        match self.structure_record()? {
            Some(record) => self.structure_pending(&record),
            None => Ok(false),
        }
    }
    /// The same answer, withdrawing a record that names nothing pending while
    /// its binding is still in place: the save never committed (a kill, a
    /// refusal) or was undone (MP7 step 6). A record whose binding the upload
    /// already held is left for the bootstrap that completes it.
    pub(crate) fn settle_meguri_structure_upload(&self) -> Result<bool, EngineError> {
        let doc = self.provider.snapshot()?;
        let Some(record) = doc.meguri_structure else { return Ok(false) };
        if self.structure_pending(&record)? { return Ok(true); }
        if doc.meguri_binding.is_some() && doc.handoff.is_none() && self.collection().is_ok() {
            self.clear_structure_record(&record)?;
        }
        Ok(false)
    }
    fn clear_structure_record(&self, record: &MeguriStructureUpload) -> Result<(), EngineError> {
        let generation = self.cloud_connection_generation()?;
        let cleared = self.publish_provider_metadata(generation, |publication|
            Ok(self.provider.clear_meguri_structure_upload(record, generation.0, publication)))?;
        cleared.map_err(|_| EngineError::CollectionUnavailable { reason: "Meguri structure record changed".into() })
    }
    fn structure_admission(&self) -> Result<(Arc<()>, Collection), MeguriStructureRefusal> {
        use MeguriStructureRefusal as R;
        let admission = self.admit_meguri_structure_job().map_err(|_| R::Busy)?;
        let collection = self.collection().map_err(R::Engine)?;
        Ok((admission, collection))
    }
    fn structure_task(&self, access: MeguriAccessSource, collection: Collection, admission: Arc<()>, reachable: bool,
        work: Option<MeguriStructureWork>, stage: Option<Stage>, finished: Option<MeguriStructureOutcome>) -> Arc<MeguriStructureTask> {
        let finished = finished.map(Arc::new);
        Arc::new(MeguriStructureTask { owner: self.meguri_rounds_owner(), access, generation: collection.generation(), collection,
            reachable, cancelled: AtomicBool::new(false),
            state: Mutex::new(TaskState { stage, admission: finished.is_none().then_some(admission), work, read_usn: None, finished }) })
    }

    /// MP7 steps 1 to 4 for one confirmed op. `reachable` false is D4's
    /// unreachable case: no round, the save, and the upload left pending.
    pub fn start_meguri_structure_save(&self, access: MeguriAccessSource, work: MeguriStructureWork, reachable: bool)
        -> Result<Arc<MeguriStructureTask>, MeguriStructureRefusal> {
        use MeguriStructureRefusal as R;
        let (admission, collection) = self.structure_admission()?;
        if self.settle_meguri_structure_upload().map_err(R::Engine)? { return Err(R::UploadPending); }
        if !matches!(self.meguri_readiness(), Ok(MeguriReadiness::Ready)) { return Err(R::NotReady); }
        if !reachable {
            // No round ran: the binding is the document's, and the baseline is
            // whatever the last completed bootstrap or round left (step 4).
            let binding = self.provider.snapshot().map_err(|error| R::Engine(error.into()))?.meguri_binding.ok_or(R::NotReady)?;
            let task = self.structure_task(access, collection, admission, false, Some(work), None, None);
            let next = self.arm_and_save(&task, &mut task.state.lock().unwrap_or_else(PoisonError::into_inner), &binding);
            self.install(&task, next);
            return Ok(task);
        }
        let source = access.clone();
        let access = source().map_err(|_| R::NotReady)?;
        match self.start_meguri_collection_for(access, Some(&admission)) {
            MeguriRoundStart::Started(round) =>
                Ok(self.structure_task(source, collection, admission, true, Some(work), Some(Stage::Round(round)), None)),
            MeguriRoundStart::Refused(refusal) => Err(R::Round(refusal)),
        }
    }
    /// MP7 step 5's Upload…: admission, a read of the server's facts and the
    /// upload, with no round and no further question.
    pub fn start_meguri_structure_upload(&self, access: MeguriAccessSource)
        -> Result<Arc<MeguriStructureTask>, MeguriStructureRefusal> {
        use MeguriStructureRefusal as R;
        let (admission, collection) = self.structure_admission()?;
        if !self.settle_meguri_structure_upload().map_err(R::Engine)? { return Err(R::NothingPending); }
        let task = self.structure_task(access, collection, admission, true, None, None, None);
        let next = self.begin_upload(&task);
        self.install(&task, next);
        Ok(task)
    }
    fn install(&self, task: &MeguriStructureTask, next: Next) {
        let mut state = task.state.lock().unwrap_or_else(PoisonError::into_inner);
        self.settle(&mut state, next);
    }
    fn settle(&self, state: &mut TaskState, next: Next) {
        match next {
            Next::Stage(stage) => state.stage = Some(stage),
            Next::Done(outcome) => {
                state.stage = None;
                state.work = None;
                state.finished = Some(Arc::new(outcome));
                // Step 1's admission ends with the job, whatever its outcome.
                state.admission = None;
                self.refresh_provider_admission();
            }
        }
    }
    /// The job's current phase, or its outcome. A phase whose job has ended
    /// is finalized here, locally, and the next one started.
    pub fn advance_meguri_structure(&self, task: &MeguriStructureTask) -> Result<MeguriStructureStep, MeguriStructureRefusal> {
        if !Arc::ptr_eq(&task.owner, &self.meguri_rounds_owner()) { return Err(MeguriStructureRefusal::Busy); }
        let mut state = task.state.lock().unwrap_or_else(PoisonError::into_inner);
        loop {
            if let Some(outcome) = &state.finished { return Ok(MeguriStructureStep::Finished(outcome.clone())); }
            let Some(stage) = state.stage.take() else {
                return Err(MeguriStructureRefusal::Engine(EngineError::Internal { message: "structure job has no phase".into() }));
            };
            let (name, job) = stage.job();
            if self.engine().jobs().running().iter().any(|running| running.id == job.id) {
                state.stage = Some(stage);
                return Ok(MeguriStructureStep::Running { stage: name, job });
            }
            let next = self.finish_stage(task, &mut state, stage);
            // A round whose result is not joined yet is waited for again.
            let waiting = matches!(&next, Next::Stage(Stage::Round(_)));
            self.settle(&mut state, next);
            if waiting && let Some(stage) = &state.stage {
                let (stage, job) = stage.job();
                return Ok(MeguriStructureStep::Running { stage, job });
            }
        }
    }
    fn finish_stage(&self, task: &MeguriStructureTask, state: &mut TaskState, stage: Stage) -> Next {
        match stage {
            Stage::Round(round) => self.after_round(task, state, round),
            Stage::Save { result, .. } => {
                let result = result.lock().unwrap_or_else(PoisonError::into_inner).take();
                self.after_save(task, result)
            }
            Stage::Facts(facts) => self.after_facts(task, state, facts.take_result()),
            Stage::ServerCopy(copy) => self.after_server_copy(task, state, copy.take_result()),
            Stage::Upload(upload) => self.after_upload(task, upload.take_result()),
            Stage::MediaPrepare(prepared) => match (prepared.take_result(), (task.access)()) {
                (Some(Ok(preview)), Ok(access)) => match self.run_meguri_bootstrap_media(access, preview.confirmation) {
                    Ok(media) => Next::Stage(Stage::Media(media)),
                    Err(_) => Next::Done(O::Saved(U::Uploaded { media_complete: false })),
                },
                _ => Next::Done(O::Saved(U::Uploaded { media_complete: false })),
            },
            Stage::Media(media) => {
                let complete = match (media.take_result(), (task.access)()) {
                    (Some(Ok(op)), Ok(access)) => self.complete_meguri_bootstrap(access.connection(), access.username(), &op.id)
                        .is_ok_and(|accounting| accounting.is_complete()),
                    _ => false,
                };
                Next::Done(O::Saved(U::Uploaded { media_complete: complete }))
            }
        }
    }
    /// Step 2's result. Only a completed round goes on to the save.
    fn after_round(&self, task: &MeguriStructureTask, state: &mut TaskState, round: Arc<MeguriRoundTask>) -> Next {
        let stop = |stop| Next::Done(O::NotSaved(stop));
        let result = match self.meguri_round_result(&round) {
            Ok(Some(result)) => result,
            // Not joined yet: the caller waits for the same job again.
            Ok(None) => return Next::Stage(Stage::Round(round)),
            Err(_) => return stop(MeguriStructureStop::Unavailable),
        };
        let completed = result.recovery.is_none() && result.collection.as_ref().is_some_and(|collection|
            matches!(collection.state, MeguriCollectionState::NoChanges | MeguriCollectionState::Merged)
                && collection.refusal.is_none() && collection.error.is_none() && !collection.acceptance_unknown);
        if !completed {
            let full_sync = result.recovery.is_none() && result.collection.as_ref()
                .is_some_and(|collection| collection.state == MeguriCollectionState::FullSyncRequired);
            return stop(if full_sync { MeguriStructureStop::FullSyncRequired }
                else { MeguriStructureStop::Round { result, task: round } });
        }
        if task.is_cancelled() { return stop(MeguriStructureStop::Cancelled); }
        let binding = round.admitted_binding().clone();
        self.arm_and_save(task, state, &binding)
    }
    /// Writes the record, then starts the op. From here a kill leaves either
    /// nothing saved (the record is withdrawn) or the save with its record.
    fn arm_and_save(&self, task: &MeguriStructureTask, state: &mut TaskState, binding: &meguri_sync::provider::MeguriBinding) -> Next {
        let stop = |stop| Next::Done(O::NotSaved(stop));
        let Some(work) = state.work.take() else { return stop(MeguriStructureStop::Unavailable) };
        let baseline = match task.collection.reader(ReaderRole::Query).and_then(|reader| Ok(reader.col()?)) {
            // Proved only by a collection that synced and holds no earlier
            // structure change: its `ls` and `scm` are the server's.
            Ok(col) => (col.last_sync.0 != 0 && !schema_gate::is_modified(&col)).then_some(StructureBaseline {
                modified: col.last_sync.0, schema_modified: col.schema_modified.0 }),
            Err(error) => return stop(MeguriStructureStop::Refused(error)),
        };
        let generation = task.generation;
        let armed = self.publish_provider_metadata(generation, |publication|
            Ok(self.provider.arm_meguri_structure_upload(binding, baseline, generation.0, publication)));
        let record = match armed {
            Ok(Ok(record)) => record,
            _ => return stop(MeguriStructureStop::Unavailable),
        };
        let result: SaveCell = Arc::default();
        let output = result.clone();
        let collection = task.collection.clone();
        match self.engine().start_job(JobKind::BulkOp, move |job| {
            let outcome = work(&collection, job);
            *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(outcome.clone());
            outcome
        }) {
            Ok(job) => Next::Stage(Stage::Save { job, result }),
            Err(error) => {
                // Nothing ran: the record goes now, or with the next settle.
                let _ = self.clear_structure_record(&record);
                stop(MeguriStructureStop::Refused(error))
            }
        }
    }
    /// Step 3's result. A refusal ends the job with nothing saved and
    /// nothing pending; a save goes on to its upload.
    fn after_save(&self, task: &MeguriStructureTask, result: Option<Result<(), EngineError>>) -> Next {
        match result {
            Some(Ok(())) => {}
            refused => {
                // Nothing was written, so the record is removed. When that
                // write fails, the next settle removes it (`scm <= ls`).
                if let Ok(Some(record)) = self.structure_record() { let _ = self.clear_structure_record(&record); }
                return Next::Done(O::NotSaved(match refused {
                    Some(Err(error)) => MeguriStructureStop::Refused(error),
                    _ => MeguriStructureStop::Cancelled,
                }));
            }
        }
        match self.settle_meguri_structure_upload() {
            // The op committed no structure change: nothing to upload.
            Ok(false) => Next::Done(O::Saved(U::NotNeeded)),
            Ok(true) if !task.reachable => pending(MeguriUploadPending::Unreachable),
            Ok(true) if task.is_cancelled() => pending(MeguriUploadPending::Cancelled),
            Ok(true) => self.begin_upload(task),
            Err(_) => pending(MeguriUploadPending::Storage),
        }
    }
    /// Step 4, and step 5's Upload…: settles what an earlier attempt left,
    /// then reads the server's facts.
    fn begin_upload(&self, task: &MeguriStructureTask) -> Next {
        let Ok(access) = (task.access)() else { return pending(MeguriUploadPending::Failed) };
        match self.manual_transfer_history().active() {
            Ok(None) => {}
            Ok(Some(op)) if op.acceptance == Acceptance::PossiblyAccepted => return Next::Done(O::Saved(U::AcceptanceUnknown)),
            Ok(Some(op)) if unsent(&op) => {
                if self.discard_meguri_bootstrap_preparation(access.connection(), &op).is_err() {
                    return Next::Done(O::Saved(U::SetupInProgress));
                }
            }
            Ok(Some(_)) => return Next::Done(O::Saved(U::SetupInProgress)),
            Err(_) => return pending(MeguriUploadPending::Storage),
        }
        // An ended attempt leaves ordinary admission closed until it is read again.
        self.refresh_provider_admission();
        let Ok(Some(record)) = self.structure_record() else { return pending(MeguriUploadPending::Storage) };
        if record.baseline().is_none() {
            return match self.hold_structure_binding(&record) {
                Ok(()) => Next::Done(O::Saved(U::ChooseCollection)),
                Err(reason) => pending(reason),
            };
        }
        match self.read_server_facts(access) {
            Ok(facts) => Next::Stage(Stage::Facts(facts)),
            Err(error) => pending(bootstrap_pending(&error)),
        }
    }
    /// One `meta` read; it changes nothing on either side.
    fn read_server_facts(&self, access: MeguriAccess) -> Result<ManualTransferTask<RemoteFacts>, MeguriBootstrapError> {
        let engine = self.engine().clone();
        let (clock, random) = self.meguri_round_sources();
        let (key, options, transport) = access.into_round_inputs();
        Ok(self.manual_worker(None, move |_, context, _| {
            let options = options.cancel(context.cancel.clone());
            let job = match transport {
                Some(transport) => Job::with_transport(&options, Some(key), random.as_ref(), transport),
                None => Job::new(&options, Some(key), random.as_ref()),
            };
            let mut job = job.map_err(|error| TransferError::Server(
                meguri_sync::JobError::from_transport(error, None, meguri_sync::ServerKind::SelfHosted)))?;
            let read = meguri_sync::transport::runtime::block_on(engine.threads(), &CancelToken::default(), async {
                Ok(meguri_sync::status::probe(&mut job, clock.as_ref()).await)
            }).map_err(|_| TransferError::Storage)?;
            Ok(RemoteFacts::from(&read?.server))
        }, |_| Ok(()))?)
    }
    /// The binding the record names stops being ready, so that the retained
    /// bootstrap path can run. Already held by an earlier attempt: nothing to do.
    fn hold_structure_binding(&self, record: &MeguriStructureUpload) -> Result<(), MeguriUploadPending> {
        let doc = self.provider.snapshot().map_err(|_| MeguriUploadPending::Storage)?;
        let Some(binding) = doc.meguri_binding else { return Ok(()) };
        if binding.connection() != record.binding().connection() || binding.account() != record.binding().account()
            || binding.operation() != record.binding().operation() {
            return Err(MeguriUploadPending::Failed);
        }
        let generation = self.cloud_connection_generation().map_err(|_| MeguriUploadPending::Storage)?;
        match self.publish_provider_metadata(generation, |publication|
            Ok(self.provider.hold_meguri_binding_for_full_sync(&binding, generation.0, publication))) {
            Ok(Ok(())) => Ok(()),
            Err(EngineError::Busy { .. }) => Err(MeguriUploadPending::Busy),
            _ => Err(MeguriUploadPending::Storage),
        }
    }
    /// Step 4's comparison: only a server that still holds the proved
    /// baseline is replaced with no question.
    fn after_facts(&self, task: &MeguriStructureTask, state: &mut TaskState, facts: Option<Result<RemoteFacts, TransferError>>) -> Next {
        let facts = match facts {
            Some(Ok(facts)) => facts,
            Some(Err(error)) => return pending(match transfer_pending(&error) {
                // A read that failed sent nothing; the server was not reached.
                MeguriUploadPending::Failed => MeguriUploadPending::Unreachable,
                other => other,
            }),
            None => return pending(MeguriUploadPending::Cancelled),
        };
        let Ok(Some(record)) = self.structure_record() else { return pending(MeguriUploadPending::Storage) };
        let same = record.baseline().is_some_and(|baseline|
            baseline.modified == facts.modified && baseline.schema_modified == facts.schema_modified);
        if let Err(reason) = self.hold_structure_binding(&record) { return pending(reason); }
        if !same { return Next::Done(O::Saved(U::ChooseCollection)); }
        if task.is_cancelled() { return pending(MeguriUploadPending::Cancelled); }
        let Ok(access) = (task.access)() else { return pending(MeguriUploadPending::Failed) };
        state.read_usn = Some(facts.usn);
        match self.prepare_meguri_bootstrap_admitted(access, MeguriBootstrapDirection::UseDeviceCollection) {
            Ok(copy) => Next::Stage(Stage::ServerCopy(copy)),
            Err(error) => pending(bootstrap_pending(&error)),
        }
    }
    /// The preparation retained the server copy and took the safety backup
    /// (MP4). Serving that copy moved the server's stamps once; anything else
    /// between the read and the copy is another client.
    fn after_server_copy(&self, task: &MeguriStructureTask, state: &mut TaskState, copy: Option<Result<Preview, TransferError>>) -> Next {
        let preview = match copy {
            Some(Ok(preview)) => preview,
            failed => {
                self.discard_unsent(task);
                return pending(match failed { Some(Err(error)) => transfer_pending(&error), _ => MeguriUploadPending::Cancelled });
            }
        };
        let Ok(Some(record)) = self.structure_record() else { self.discard_unsent(task); return pending(MeguriUploadPending::Storage) };
        let served = preview.operation.remote.as_ref().filter(|remote|
            state.read_usn.and_then(|usn| usn.checked_add(1)) == Some(remote.usn)
                && record.baseline().is_some_and(|baseline| baseline.schema_modified == remote.schema_modified));
        let Some(served) = served else {
            self.discard_unsent(task);
            return Next::Done(O::Saved(U::ChooseCollection));
        };
        // The same content under the stamps of the copy: a later Upload…
        // compares with these.
        let baseline = StructureBaseline { modified: served.modified, schema_modified: served.schema_modified };
        let generation = task.generation;
        let advanced = self.publish_provider_metadata(generation, |publication|
            Ok(self.provider.advance_meguri_structure_baseline(&record, baseline, generation.0, publication)));
        if !matches!(advanced, Ok(Ok(_))) { self.discard_unsent(task); return pending(MeguriUploadPending::Storage); }
        if task.is_cancelled() { self.discard_unsent(task); return pending(MeguriUploadPending::Cancelled); }
        let Ok(access) = (task.access)() else { self.discard_unsent(task); return pending(MeguriUploadPending::Failed) };
        match self.confirm_meguri_bootstrap(access, preview) {
            Ok(upload) => Next::Stage(Stage::Upload(upload)),
            Err(error) => { self.discard_unsent(task); pending(bootstrap_pending(&error)) }
        }
    }
    fn after_upload(&self, task: &MeguriStructureTask, upload: Option<Result<Operation, TransferError>>) -> Next {
        match upload {
            Some(Ok(op)) => match (task.access)().and_then(|access| self.prepare_meguri_bootstrap_media(access, op.id.clone())) {
                Ok(media) => Next::Stage(Stage::MediaPrepare(media)),
                Err(_) => Next::Done(O::Saved(U::Uploaded { media_complete: false })),
            },
            failed => {
                let reason = match &failed { Some(Err(error)) => transfer_pending(error), _ => MeguriUploadPending::Cancelled };
                match self.manual_transfer_history().active() {
                    Ok(Some(op)) if op.acceptance == Acceptance::PossiblyAccepted => Next::Done(O::Saved(U::AcceptanceUnknown)),
                    Ok(Some(op)) if unsent(&op) => { self.discard_unsent(task); pending(reason) }
                    Ok(Some(_)) => Next::Done(O::Saved(U::SetupInProgress)),
                    Ok(None) => pending(reason),
                    Err(_) => pending(MeguriUploadPending::Storage),
                }
            }
        }
    }
    /// Ends a retained preparation that sent nothing, so that the next
    /// Upload… starts clean. One that may have been sent is never touched.
    fn discard_unsent(&self, task: &MeguriStructureTask) {
        if let (Ok(Some(op)), Ok(access)) = (self.manual_transfer_history().active(), (task.access)()) {
            if unsent(&op) { let _ = self.discard_meguri_bootstrap_preparation(access.connection(), &op); }
        }
    }
}
