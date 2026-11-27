//! One retained routine phase at a time, over the actual engine job registry.
//! Result polling performs only local guarded finalization. No scheduler lives here.
use crate::{CoreHost, MeguriAccess};
use meguri_engine::{EngineError, EventKind, JobHandle, JobKind, Subscription};
use meguri_sync::{ErrorKind, Job, JobError, StatusMemory, normal,
    media::{self, BoundMediaTerminal, MediaFailure, MediaSyncResult, RoundMediaAccounting},
    provider::{MeguriRoundAdmission, MeguriRoundRefusal, MeguriReadyRefusal}};
use std::sync::{Arc, Mutex, PoisonError};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriRoundStage { Collection, Media }
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriCollectionState { NoChanges, Merged, FullSyncRequired, Failed }
#[derive(Clone)]
pub struct MeguriCollectionResult {
    pub state: MeguriCollectionState,
    pub error: Option<JobError>,
    pub refusal: Option<MeguriRoundRefusal>,
    pub acceptance_unknown: bool,
    pub skipped: u64,
}
#[derive(Clone)]
pub struct MeguriMediaResult {
    pub effects: MediaSyncResult,
    pub failure: Option<MediaFailure>,
    pub refusal: Option<MeguriRoundRefusal>,
    pub accounting: Option<RoundMediaAccounting>,
    pub accounting_error: Option<JobError>,
    pub complete: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeguriRoundRecovery { BindingChanged, PublicationChanged, Storage, Unavailable }
#[derive(Clone)]
pub struct MeguriRoundResult {
    pub collection: Option<MeguriCollectionResult>, pub media: Option<MeguriMediaResult>,
    pub recovery: Option<MeguriRoundRecovery>,
}
pub enum MeguriRoundStart { Started(Arc<MeguriRoundTask>), Refused(MeguriRoundRefusal) }

pub(crate) struct RoundRegistry {
    owner: Arc<()>, state: Mutex<RegistryState>, memory: Arc<Mutex<StatusMemory>>,
}
#[derive(Default)]
struct RegistryState {
    handoff: bool, active: Option<Active>,
    /// MP7 step 1: the structure save job's exclusive admission. While it is
    /// held only that job's own round starts; a provider handoff waits. The
    /// job's task owns the only strong reference, so the admission ends with
    /// the task, however it ends.
    structure: std::sync::Weak<()>,
}
enum Active { Reserved(Arc<()>), Task(Arc<MeguriRoundTask>) }
impl Default for RoundRegistry {
    fn default() -> Self { Self { owner: Arc::new(()), state: Mutex::default(), memory: Arc::default() } }
}
struct Reservation<'a> { registry: &'a RoundRegistry, id: Arc<()>, installed: bool }
impl Drop for Reservation<'_> {
    fn drop(&mut self) {
        if self.installed { return; }
        let mut state = self.registry.state.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(&state.active, Some(Active::Reserved(id)) if Arc::ptr_eq(id, &self.id)) { state.active = None; }
    }
}
impl Reservation<'_> {
    fn install(mut self, task: Arc<MeguriRoundTask>) {
        let mut state = self.registry.state.lock().unwrap_or_else(PoisonError::into_inner);
        // A handoff that raced registration closes the just-created real token.
        if state.handoff { task.job.cancel.interrupt(); }
        state.active = Some(Active::Task(task));
        self.installed = true;
    }
}
pub struct MeguriRoundTask {
    owner: Arc<()>, job: JobHandle, admission: MeguriRoundAdmission,
    stage: MeguriRoundStage, finished: Mutex<Option<Subscription>>,
    state: Mutex<TaskState>,
    /// Its owner (the structure save job) went away before reading the
    /// result: the next reservation finalizes it.
    abandoned: std::sync::atomic::AtomicBool,
}
struct TaskState { source: Source, terminal: Option<Terminal>, published: Option<Arc<MeguriRoundResult>> }
enum Source {
    Normal(Option<normal::Task>),
    Media { terminal: Arc<Mutex<Option<Result<BoundMediaTerminal, JobError>>>>, service: meguri_engine::media::MediaService },
}
enum Terminal {
    Normal { result: Result<normal::Outcome, JobError>, finish_sent: bool },
    Media(Option<Result<BoundMediaTerminal, JobError>>),
}
impl MeguriRoundTask {
    pub fn handle(&self) -> JobHandle { self.job.clone() }
    pub(crate) fn abandon(&self) {
        self.abandoned.store(true, std::sync::atomic::Ordering::SeqCst);
        self.cancel(true);
    }
    pub fn stage(&self) -> MeguriRoundStage { self.stage }
    pub fn cancel(&self, interrupted: bool) {
        if interrupted { self.job.cancel.interrupt(); } else { self.job.cancel.cancel(); }
    }
    /// Waits only the original JobFinished, whose registry removes the worker
    /// after all file work and its engine slot have drained.
    pub fn wait(&self) {
        let mut events = self.finished.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(events) = events.take() {
            while let Some(event) = events.recv() {
                if matches!(event.kind, EventKind::JobFinished { id, .. } if id == self.job.id) { break; }
            }
        }
    }
}
fn engine_refusal(error: EngineError) -> MeguriRoundRefusal {
    match error {
        EngineError::Busy { .. } => MeguriRoundRefusal::Busy,
        EngineError::Storage(_) | EngineError::Media(_) => MeguriRoundRefusal::Storage,
        _ => MeguriRoundRefusal::Interrupted,
    }
}
fn ready_recovery(error: MeguriReadyRefusal) -> MeguriRoundRecovery {
    match error {
        MeguriReadyRefusal::Storage(_) => MeguriRoundRecovery::Storage,
        MeguriReadyRefusal::PublicationChanged => MeguriRoundRecovery::PublicationChanged,
        MeguriReadyRefusal::MediaEpochChanged => MeguriRoundRecovery::BindingChanged,
        MeguriReadyRefusal::Media(_) => MeguriRoundRecovery::Storage,
        _ => MeguriRoundRecovery::BindingChanged,
    }
}
fn stopped(task: &MeguriRoundTask) -> JobError {
    JobError::new(if task.job.cancel.stop_cause() == Some(meguri_engine::StopCause::User) {
        ErrorKind::Cancelled
    } else { ErrorKind::Interrupted }, None, "Meguri original phase ended before a result")
}
fn collection_result(result: &Result<normal::Outcome, JobError>, sent: bool,
    refusal: Option<MeguriRoundRefusal>) -> MeguriCollectionResult {
    use meguri_sync::CheckResult;
    let (state, error, skipped) = match result {
        Ok(value) if matches!(value.check.result, CheckResult::OneWay { .. })
            || value.merge.as_ref().is_some_and(|merge| merge.one_way) => (MeguriCollectionState::FullSyncRequired, None, 0),
        Ok(value) if value.merge.is_some() => (MeguriCollectionState::Merged, None,
            value.merge.as_ref().map_or(0, |merge| merge.skipped.len() as u64)),
        Ok(value) if matches!(value.check.result, CheckResult::NoChanges) => (MeguriCollectionState::NoChanges, None, 0),
        Ok(_) => (MeguriCollectionState::Failed, Some(JobError::new(ErrorKind::Internal, None, "normal terminal has no merge")), 0),
        Err(error) => (if matches!(error.kind, ErrorKind::SanityCheckFailed { .. } | ErrorKind::NotetypeSchemaChanged { .. }) {
            MeguriCollectionState::FullSyncRequired
        } else { MeguriCollectionState::Failed }, Some(error.clone()), 0),
    };
    MeguriCollectionResult { state, error, refusal, acceptance_unknown: sent && result.is_err(), skipped }
}
impl CoreHost {
    fn reserve_meguri_round(&self, structure: Option<&Arc<()>>) -> Result<Reservation<'_>, MeguriRoundRefusal> {
        self.finalize_abandoned_round();
        let reservation = {
            let mut state = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
            let admitted = match (state.structure.upgrade(), structure) {
                (None, None) => true,
                (Some(held), Some(own)) => Arc::ptr_eq(&held, own),
                _ => false,
            };
            if state.handoff || state.active.is_some() || !admitted { return Err(MeguriRoundRefusal::Busy); }
            let id = Arc::new(());
            state.active = Some(Active::Reserved(id.clone()));
            Reservation { registry: &self.meguri_rounds, id, installed: false }
        };
        // Registry ownership never spans lifecycle, engine registry, provider
        // reads, worker registration or any other potentially blocking work.
        if self.engine().slot_holder().is_some() || self.engine().jobs().running().iter().any(|job|
            job.name == JobKind::NormalSync.name() || job.name == JobKind::MediaSync.name()) {
            return Err(MeguriRoundRefusal::Busy);
        }
        Ok(reservation)
    }
    fn routine_admission(&self, access: &MeguriAccess) -> Result<(meguri_engine::Collection, MeguriRoundAdmission), MeguriRoundRefusal> {
        let collection = self.collection().map_err(engine_refusal)?;
        let media = self.media().map_err(engine_refusal)?;
        let lease = media.try_job(meguri_engine::media::MediaJobKind::Sync).map_err(engine_refusal)?;
        let admission = self.provider.admit_meguri_round(collection.clone(), self.original_resource_authority(&collection),
            self.manual_transfer_history().clone(), access.connection(), access.username(), media.store().clone());
        drop(lease);
        let admission = admission.map_err(|refusal| {
            if refusal == MeguriRoundRefusal::BindingChanged { self.provider.close_admission(); }
            refusal
        })?;
        Ok((collection, admission))
    }
    pub fn start_meguri_collection(&self, access: MeguriAccess) -> MeguriRoundStart {
        self.start_meguri_collection_for(access, None)
    }
    /// `structure` is the MP7 job's own admission; no other caller has one.
    pub(crate) fn start_meguri_collection_for(&self, access: MeguriAccess, structure: Option<&Arc<()>>) -> MeguriRoundStart {
        let start = || -> Result<Arc<MeguriRoundTask>, MeguriRoundRefusal> {
            let reservation = self.reserve_meguri_round(structure)?;
            // MP7 step 5: no round runs while a structure upload is pending;
            // a save that was undone withdraws the record here.
            if self.settle_meguri_structure_upload().map_err(engine_refusal)? { return Err(MeguriRoundRefusal::NotReady); }
            let (collection, admission) = self.routine_admission(&access)?;
            let finished = self.engine().stream().subscribe();
            let (clock, random) = self.meguri_round_sources();
            let (key, options, transport) = access.into_round_inputs();
            let normal = normal::start_admitted(self.engine(), collection, options, key, clock, random,
                self.meguri_rounds.memory.clone(), transport, admission.clone()).map_err(engine_refusal)?;
            let task = Arc::new(MeguriRoundTask { owner: self.meguri_rounds.owner.clone(), job: normal.handle.clone(),
                admission, stage: MeguriRoundStage::Collection, finished: Mutex::new(Some(finished)),
                state: Mutex::new(TaskState { source: Source::Normal(Some(normal)), terminal: None, published: None }), abandoned: Default::default() });
            reservation.install(task.clone());
            Ok(task)
        };
        match start() { Ok(task) => MeguriRoundStart::Started(task), Err(refusal) => MeguriRoundStart::Refused(refusal) }
    }
    pub fn start_meguri_media(&self, access: MeguriAccess) -> MeguriRoundStart {
        let start = || -> Result<Arc<MeguriRoundTask>, MeguriRoundRefusal> {
            let reservation = self.reserve_meguri_round(None)?;
            if self.settle_meguri_structure_upload().map_err(engine_refusal)? { return Err(MeguriRoundRefusal::NotReady); }
            let (collection, admission) = self.routine_admission(&access)?;
            let service = self.media().map_err(engine_refusal)?;
            let terminal = Arc::new(Mutex::new(None));
            let output = terminal.clone();
            let media = service.clone();
            let generation = collection.generation();
            let engine = self.engine().clone();
            let admitted = admission.clone();
            let (_, random) = self.meguri_round_sources();
            let (key, options, transport) = access.into_round_inputs();
            let finished = engine.stream().subscribe();
            let job = self.engine().start_job(JobKind::MediaSync, move |ctx| {
                ctx.progress.set_phase(meguri_engine::ProgressPhase::Named("meguri-media-checking"));
                ctx.progress.observe_routine_sync(meguri_engine::progress::RoutineSyncMilestone::MediaCheckStarted);
                let options = options.cancel(ctx.cancel.clone());
                let job = match transport { Some(transport) => Job::with_transport(&options, Some(key), random.as_ref(), transport),
                    None => Job::new(&options, Some(key), random.as_ref()) };
                let mut job = match job { Ok(job) => job, Err(error) => {
                    *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(Err(
                        JobError::from_transport(error, None, meguri_sync::ServerKind::SelfHosted)));
                    return Err(EngineError::Internal { message: "Meguri media transport initialization".into() });
                } };
                let result = media::reconcile_bound(engine.threads(), &mut job, &media, generation.0, admitted,
                    &mut |snapshot| {
                        use media::MediaPhase;
                        let transferring = snapshot.transfer_started
                            && matches!(snapshot.phase, MediaPhase::Downloading | MediaPhase::Uploading);
                        // Only an active download has a complete known remainder.
                        // Listing/checking counters and upload batches are not totals.
                        ctx.progress.report_routine_media(transferring, snapshot.transfer_started, snapshot.totals.checked,
                            (transferring && snapshot.phase == MediaPhase::Downloading).then_some(snapshot.files_left));
                    });
                let failed = result.outcome().is_err();
                *output.lock().unwrap_or_else(PoisonError::into_inner) = Some(Ok(result));
                if failed { Err(EngineError::Internal { message: "Meguri media round failed".into() }) } else { Ok(()) }
            }).map_err(engine_refusal)?;
            let task = Arc::new(MeguriRoundTask { owner: self.meguri_rounds.owner.clone(), job, admission,
                stage: MeguriRoundStage::Media, finished: Mutex::new(Some(finished)),
                state: Mutex::new(TaskState { source: Source::Media { terminal, service }, terminal: None, published: None }), abandoned: Default::default() });
            reservation.install(task.clone());
            Ok(task)
        };
        match start() { Ok(task) => MeguriRoundStart::Started(task), Err(refusal) => MeguriRoundStart::Refused(refusal) }
    }
    /// Call after the matching completion, or poll without blocking. A foreign
    /// task never consumes a current result. Failed finalization stays retained.
    pub fn meguri_round_result(&self, task: &MeguriRoundTask) -> Result<Option<Arc<MeguriRoundResult>>, MeguriRoundRefusal> {
        if !Arc::ptr_eq(&task.owner, &self.meguri_rounds.owner) { return Err(MeguriRoundRefusal::BindingChanged); }
        if self.engine().jobs().running().iter().any(|job| job.id == task.job.id) { return Ok(None); }
        let mut state = task.state.lock().unwrap_or_else(PoisonError::into_inner);
        if let Some(result) = &state.published { return Ok(Some(result.clone())); }
        if state.terminal.is_none() {
            state.terminal = Some(match &mut state.source {
                Source::Normal(normal) => {
                    let normal = normal.take().expect("original normal result consumed once");
                    let finish_sent = normal.finish_sent();
                    Terminal::Normal { result: normal.wait(), finish_sent }
                }
                Source::Media { terminal, .. } => Terminal::Media(terminal.lock().unwrap_or_else(PoisonError::into_inner).take()),
            });
        }
        let result = Arc::new(self.finalize_meguri_round(task, &state));
        state.published = Some(result.clone());
        if result.recovery.is_none() {
            // Retain the actual terminal/receipt, relinquishing only its
            // original Sync exclusion after local finalization is settled.
            if let Some(Terminal::Media(Some(Ok(terminal)))) = &mut state.terminal {
                terminal.release_finalized_lease();
            }
            let mut registry = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
            if matches!(&registry.active, Some(Active::Task(current)) if std::ptr::eq(current.as_ref(), task)) {
                registry.active = None;
            }
        } else { self.provider.close_admission(); }
        Ok(Some(result))
    }
    /// Explicit local retry. Never consumes another job or repeats requests.
    pub fn retry_meguri_round_finalization(&self, task: &MeguriRoundTask) -> Result<Option<Arc<MeguriRoundResult>>, MeguriRoundRefusal> {
        if !Arc::ptr_eq(&task.owner, &self.meguri_rounds.owner) { return Err(MeguriRoundRefusal::BindingChanged); }
        {
            let mut state = task.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.published.as_ref().is_none_or(|result| result.recovery.is_none()) { return Ok(state.published.clone()); }
            state.published = None;
        }
        self.meguri_round_result(task)
    }
    fn finalize_meguri_round(&self, task: &MeguriRoundTask, state: &TaskState) -> MeguriRoundResult {
        let mut result = MeguriRoundResult { collection: None, media: None, recovery: None };
        let generation = task.job_generation();
        match state.terminal.as_ref().expect("retained terminal") {
            Terminal::Normal { result: original, finish_sent } => {
                let mut collection = collection_result(original, *finish_sent, task.admission.refusal());
                if collection.state == MeguriCollectionState::FullSyncRequired {
                    self.provider.close_admission();
                    let hold = self.publish_provider_metadata(generation, |publication| {
                        if publication.as_ref() != Some(task.admission.publication()) {
                            return Ok(Err(MeguriReadyRefusal::PublicationChanged));
                        }
                        Ok(self.provider.finalize_meguri_round_hold(&task.admission, generation.0, publication))
                    });
                    result.recovery = match hold { Ok(Ok(())) => None, Ok(Err(error)) => Some(ready_recovery(error)),
                        Err(_) => Some(MeguriRoundRecovery::Unavailable) };
                    if result.recovery.is_none() { self.refresh_provider_admission(); }
                } else if matches!(collection.state, MeguriCollectionState::NoChanges | MeguriCollectionState::Merged)
                    && let Err(refusal) = task.admission.check() {
                    collection.state = MeguriCollectionState::Failed;
                    collection.refusal = Some(refusal);
                }
                result.collection = Some(collection);
            }
            Terminal::Media(terminal) => {
                let mut media = match terminal {
                    Some(Ok(terminal)) => {
                        let (effects, failure) = match terminal.outcome() {
                            Ok(effects) => (effects.clone(), None),
                            Err(failure) => (failure.snapshot.totals.clone(), Some((**failure).clone())),
                        };
                        let (accounting, accounting_error) = match terminal.accounting() {
                            Ok(value) => (Some(value.clone()), None), Err(error) => (None, Some(error.clone())),
                        };
                        MeguriMediaResult { effects, failure, refusal: task.admission.refusal().or_else(|| terminal.binding_changed().then_some(MeguriRoundRefusal::BindingChanged)), accounting, accounting_error, complete: false }
                    }
                    Some(Err(error)) => MeguriMediaResult { effects: Default::default(), failure: Some(MediaFailure {
                        error: error.clone(), snapshot: Default::default() }), refusal: task.admission.refusal(),
                        accounting: None, accounting_error: None, complete: false },
                    None => MeguriMediaResult { effects: Default::default(), failure: Some(MediaFailure {
                        error: stopped(task), snapshot: Default::default() }), refusal: task.admission.refusal(),
                        accounting: None, accounting_error: None, complete: false },
                };
                if let Some(Ok(terminal)) = terminal {
                    if let (Some(receipt), Source::Media { service, .. }) = (terminal.receipt(), &state.source) {
                        if receipt.reset_occurred() {
                            let update = self.publish_provider_metadata(generation, |publication| {
                                if publication.as_ref() != Some(task.admission.publication()) {
                                    return Ok(Err(MeguriReadyRefusal::PublicationChanged));
                                }
                                Ok(self.provider.finalize_meguri_round_epoch(task.admission.binding(), receipt, service.store(), generation.0, publication))
                            });
                            result.recovery = match update { Ok(Ok(())) => None, Ok(Err(error)) => Some(ready_recovery(error)),
                                Err(_) => Some(MeguriRoundRecovery::Unavailable) };
                        } else if media.failure.is_none() {
                            // No provider write is needed. Closed original
                            // resources remain a typed refusal, not a fabricated
                            // metadata recovery or a reason to keep a join open.
                            media.refusal = task.admission.check().err();
                            if media.refusal.is_none() {
                                if let Err(error) = self.provider.finalize_meguri_round_epoch(task.admission.binding(), receipt,
                                    service.store(), generation.0, Some(task.admission.publication().clone())) {
                                    match error {
                                        MeguriReadyRefusal::MediaEpochChanged => media.refusal = Some(MeguriRoundRefusal::BindingChanged),
                                        _ => media.accounting_error = Some(JobError::new(ErrorKind::LocalStorage, None, "Meguri terminal baseline unavailable")),
                                    }
                                }
                            }
                        }
                        if result.recovery.is_none() {
                            if receipt.reset_occurred() { self.refresh_provider_admission(); }
                            if media.failure.is_none() && media.refusal.is_none() {
                                media.refusal = task.admission.check_finalized_media(receipt).err();
                            }
                        }
                    } else if media.failure.is_none() { media.refusal = Some(MeguriRoundRefusal::BindingChanged); }
                }
                media.complete = result.recovery.is_none() && media.failure.is_none() && media.refusal.is_none()
                    && media.accounting_error.is_none() && media.accounting.as_ref().is_some_and(|a|
                        a.excluded == 0 && a.pending == 0 && a.awaiting == 0);
                result.media = Some(media);
            }
        }
        result
    }
    /// Refused, with nothing closed, while the structure save job holds the lane.
    pub(crate) fn close_meguri_round_admission(&self) -> Result<(), EngineError> {
        {
            let mut state = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
            if state.structure.strong_count() != 0 {
                return Err(EngineError::Busy { running_op: "Meguri note type structure save".into() });
            }
            state.handoff = true;
            if let Some(Active::Task(task)) = &state.active { task.cancel(true); }
        }
        // Native iCloud handoff keeps its own special closure provenance.
        // Only the Meguri owner is revoked here; begin_handoff closes both
        // services through the existing durable transition immediately after.
        if self.provider.snapshot().is_ok_and(|doc| doc.selection == meguri_sync::provider::Selection::Meguri) {
            self.provider.close_admission();
        }
        Ok(())
    }
    pub(crate) fn require_meguri_round_join(&self) -> Result<(), EngineError> {
        let state = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.active.is_some() {
            return Err(EngineError::Busy { running_op: "Meguri original round drain or finalization".into() });
        }
        if state.structure.strong_count() != 0 {
            return Err(EngineError::Busy { running_op: "Meguri note type structure save".into() });
        }
        Ok(())
    }
    /// MP7 step 1. Refused while a round, a handoff or another job owns the lane.
    pub(crate) fn admit_meguri_structure_job(&self) -> Result<Arc<()>, MeguriRoundRefusal> {
        self.finalize_abandoned_round();
        let mut state = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
        if state.handoff || state.active.is_some() || state.structure.strong_count() != 0 { return Err(MeguriRoundRefusal::Busy); }
        let token = Arc::new(());
        state.structure = Arc::downgrade(&token);
        Ok(token)
    }
    /// A round whose owner went away is finalized as its owner would have:
    /// its entry is released unless it still runs or its finalization failed.
    fn finalize_abandoned_round(&self) {
        let abandoned = {
            let state = self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner);
            match &state.active {
                Some(Active::Task(task)) if task.abandoned.load(std::sync::atomic::Ordering::SeqCst) => Some(task.clone()),
                _ => None,
            }
        };
        let Some(task) = abandoned else { return };
        // A finalization that failed earlier is tried again here, since no
        // owner is left to ask for it; while it fails the entry stays.
        if self.meguri_round_result(&task).is_ok_and(|result| result.is_some_and(|result| result.recovery.is_some())) {
            let _ = self.retry_meguri_round_finalization(&task);
        }
    }
    pub(crate) fn meguri_rounds_owner(&self) -> Arc<()> { self.meguri_rounds.owner.clone() }
    pub(crate) fn meguri_structure_job_active(&self) -> bool {
        self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner).structure.strong_count() != 0
    }
    pub(crate) fn reopen_meguri_round_admission(&self) {
        self.meguri_rounds.state.lock().unwrap_or_else(PoisonError::into_inner).handoff = false;
    }
}
impl MeguriRoundTask {
    pub(crate) fn admitted_binding(&self) -> &meguri_sync::provider::MeguriBinding { self.admission.binding() }
    fn job_generation(&self) -> meguri_engine::Generation { self.admission.generation() }
}
