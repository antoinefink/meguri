//! Internal typed work on the retained Host's existing registered pool.
use super::*;
use crate::cloud_policy::policy_jobs::{Cell, Completion};
use meguri_engine::Generation;

macro_rules! typed_result {
    ($result:ident, $start:ident, $value:ty) => {
        #[derive(uniffi::Object)]
        pub struct $result {
            cell: Arc<Cell<$value>>,
        }
        #[uniffi::export]
        impl $result {
            pub fn result(&self) -> Result<Option<$value>, CoreError> {
                callback::enter(concat!(stringify!($result), ".result"));
                self.cell
                    .0
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .as_ref()
                    .map(|stored| stored.result.clone())
                    .transpose()
            }
        }
        #[derive(Clone, uniffi::Record)]
        pub struct $start {
            pub handle: crate::jobs::JobHandle,
            pub result: Arc<$result>,
        }
    };
}
typed_result!(
    CloudPublicationWorkResult,
    CloudPublicationWorkJob,
    CloudPublicationWork
);
#[derive(Clone, uniffi::Record)]
pub struct CloudAdoptionResumed {
    pub resumed: bool,
}
typed_result!(CloudAdoptionResumeResult, CloudAdoptionResumeJob, CloudAdoptionResumed);
// A successful void reply must remain distinct from a pending typed result.
#[derive(Clone, uniffi::Record)]
pub struct CloudAdoptionApplied {
    pub applied: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudAdoptionDiagnosticTask {
    ReadActive,
    ReadEpoch,
    ReadMediaRoot,
    Fetch,
    FetchPayload,
    Validate,
    RecheckActive,
    Activate,
}
impl From<a::AdoptionDiagnosticTask> for CloudAdoptionDiagnosticTask {
    fn from(value: a::AdoptionDiagnosticTask) -> Self {
        match value {
            a::AdoptionDiagnosticTask::ReadActive => Self::ReadActive,
            a::AdoptionDiagnosticTask::ReadEpoch => Self::ReadEpoch,
            a::AdoptionDiagnosticTask::ReadMediaRoot => Self::ReadMediaRoot,
            a::AdoptionDiagnosticTask::Fetch => Self::Fetch,
            a::AdoptionDiagnosticTask::FetchPayload => Self::FetchPayload,
            a::AdoptionDiagnosticTask::Validate => Self::Validate,
            a::AdoptionDiagnosticTask::RecheckActive => Self::RecheckActive,
            a::AdoptionDiagnosticTask::Activate => Self::Activate,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudAdoptionDiagnosticStage {
    Operation,
    Owner,
    Claim,
    Authority,
    ActiveValidation,
    EpochShape,
    EpochValidation,
    EpochWrite,
    MediaRootShape,
    MediaRootValidation,
    MediaRootWrite,
    MediaRootEpoch,
    MediaRootStageCreate,
    MediaRootSpace,
    PageValidation,
    PageRecordValidation,
    PageEpochShape,
    PageEpochData,
    PageEpochSystemFields,
    PageSpace,
    PayloadValidation,
    PayloadRead,
    PayloadSpace,
    PayloadWrite,
    ReceivedValidation,
    RecordEncode,
    RecordSpace,
    RecordWrite,
    RowStage,
    FinalSpace,
    PageAdvance,
    PageFinish,
    NextAuthority,
    Transport,
}
impl From<a::AdoptionDiagnosticStage> for CloudAdoptionDiagnosticStage {
    fn from(value: a::AdoptionDiagnosticStage) -> Self {
        match value {
            a::AdoptionDiagnosticStage::Operation => Self::Operation,
            a::AdoptionDiagnosticStage::Owner => Self::Owner,
            a::AdoptionDiagnosticStage::Claim => Self::Claim,
            a::AdoptionDiagnosticStage::Authority => Self::Authority,
            a::AdoptionDiagnosticStage::ActiveValidation => Self::ActiveValidation,
            a::AdoptionDiagnosticStage::EpochShape => Self::EpochShape,
            a::AdoptionDiagnosticStage::EpochValidation => Self::EpochValidation,
            a::AdoptionDiagnosticStage::EpochWrite => Self::EpochWrite,
            a::AdoptionDiagnosticStage::MediaRootShape => Self::MediaRootShape,
            a::AdoptionDiagnosticStage::MediaRootValidation => Self::MediaRootValidation,
            a::AdoptionDiagnosticStage::MediaRootWrite => Self::MediaRootWrite,
            a::AdoptionDiagnosticStage::MediaRootEpoch => Self::MediaRootEpoch,
            a::AdoptionDiagnosticStage::MediaRootStageCreate => Self::MediaRootStageCreate,
            a::AdoptionDiagnosticStage::MediaRootSpace => Self::MediaRootSpace,
            a::AdoptionDiagnosticStage::PageValidation => Self::PageValidation,
            a::AdoptionDiagnosticStage::PageRecordValidation => Self::PageRecordValidation,
            a::AdoptionDiagnosticStage::PageEpochShape => Self::PageEpochShape,
            a::AdoptionDiagnosticStage::PageEpochData => Self::PageEpochData,
            a::AdoptionDiagnosticStage::PageEpochSystemFields => Self::PageEpochSystemFields,
            a::AdoptionDiagnosticStage::PageSpace => Self::PageSpace,
            a::AdoptionDiagnosticStage::PayloadValidation => Self::PayloadValidation,
            a::AdoptionDiagnosticStage::PayloadRead => Self::PayloadRead,
            a::AdoptionDiagnosticStage::PayloadSpace => Self::PayloadSpace,
            a::AdoptionDiagnosticStage::PayloadWrite => Self::PayloadWrite,
            a::AdoptionDiagnosticStage::ReceivedValidation => Self::ReceivedValidation,
            a::AdoptionDiagnosticStage::RecordEncode => Self::RecordEncode,
            a::AdoptionDiagnosticStage::RecordSpace => Self::RecordSpace,
            a::AdoptionDiagnosticStage::RecordWrite => Self::RecordWrite,
            a::AdoptionDiagnosticStage::RowStage => Self::RowStage,
            a::AdoptionDiagnosticStage::FinalSpace => Self::FinalSpace,
            a::AdoptionDiagnosticStage::PageAdvance => Self::PageAdvance,
            a::AdoptionDiagnosticStage::PageFinish => Self::PageFinish,
            a::AdoptionDiagnosticStage::NextAuthority => Self::NextAuthority,
            a::AdoptionDiagnosticStage::Transport => Self::Transport,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudAdoptionFailureCategory {
    Validation,
    Tracking,
    Engine,
    Cancelled,
    AuthorityChanged,
    AccountUnavailable,
    Publication,
    Incomplete,
    FileIdentityUnavailable,
    Transport,
}
impl From<a::AdoptionFailureCategory> for CloudAdoptionFailureCategory {
    fn from(value: a::AdoptionFailureCategory) -> Self {
        match value {
            a::AdoptionFailureCategory::Validation => Self::Validation,
            a::AdoptionFailureCategory::Tracking => Self::Tracking,
            a::AdoptionFailureCategory::Engine => Self::Engine,
            a::AdoptionFailureCategory::Cancelled => Self::Cancelled,
            a::AdoptionFailureCategory::AuthorityChanged => Self::AuthorityChanged,
            a::AdoptionFailureCategory::AccountUnavailable => Self::AccountUnavailable,
            a::AdoptionFailureCategory::Publication => Self::Publication,
            a::AdoptionFailureCategory::Incomplete => Self::Incomplete,
            a::AdoptionFailureCategory::FileIdentityUnavailable => Self::FileIdentityUnavailable,
            a::AdoptionFailureCategory::Transport => Self::Transport,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Record)]
pub struct CloudAdoptionFailureDiagnostic {
    pub task: CloudAdoptionDiagnosticTask,
    pub last_entered_stage: CloudAdoptionDiagnosticStage,
    pub primary_category: Option<CloudAdoptionFailureCategory>,
    pub cleanup_category: Option<CloudAdoptionFailureCategory>,
}
impl From<a::AdoptionFailureDiagnostic> for CloudAdoptionFailureDiagnostic {
    fn from(value: a::AdoptionFailureDiagnostic) -> Self {
        Self {
            task: value.task.into(),
            last_entered_stage: value.last_entered_stage.into(),
            primary_category: value.primary_category.map(Into::into),
            cleanup_category: value.cleanup_category.map(Into::into),
        }
    }
}
#[derive(uniffi::Object)]
pub struct CloudAdoptionAppliedResult {
    cell: Arc<Cell<CloudAdoptionApplied>>,
    diagnostic: Arc<std::sync::OnceLock<Option<CloudAdoptionFailureDiagnostic>>>,
}
#[uniffi::export]
impl CloudAdoptionAppliedResult {
    pub fn result(&self) -> Result<Option<CloudAdoptionApplied>, CoreError> {
        callback::enter("CloudAdoptionAppliedResult.result");
        self.cell
            .0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_ref()
            .map(|stored| stored.result.clone())
            .transpose()
    }
    /// None means no captured failure (also while pending). Never consumes the
    /// typed outcome, including when result() throws. No later job can write it.
    pub fn failure_diagnostic(&self) -> Result<Option<CloudAdoptionFailureDiagnostic>, CoreError> {
        callback::enter("CloudAdoptionAppliedResult.failure_diagnostic");
        if self.cell.0.lock().unwrap_or_else(PoisonError::into_inner).is_none() {
            return Ok(None);
        }
        Ok(self.diagnostic.get().cloned().flatten())
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudAdoptionResultJob {
    pub handle: crate::jobs::JobHandle,
    pub result: Arc<CloudAdoptionAppliedResult>,
}

impl CloudConnection {
    fn start_connection<T: Send + 'static>(
        self: Arc<Self>,
        name: &'static str,
        generation: u64,
        work: impl FnOnce(Arc<Self>, &mut c::policy::PolicyWork) -> Result<T, CoreError> + Send + 'static,
    ) -> Result<(crate::jobs::JobHandle, Arc<Cell<T>>), CoreError> {
        self.owner.check_generation(generation)?;
        let cell = Arc::new(Cell(Mutex::new(None)));
        let completion = Completion(cell.clone());
        let owner = self.clone();
        let handle = self
            .owner
            .host
            .start_cloud_connection_job(Generation(generation), name, move |job| {
                let mut authority = c::policy::PolicyWork::new(&job.cancel)
                    .with_generation(owner.owner.host.engine().downgrade(), Generation(generation));
                let value = job
                    .cancel
                    .check()
                    .map_err(CoreError::from)
                    .and_then(|()| owner.owner.check_generation(generation))
                    .and_then(|()| work(owner, &mut authority))
                    .and_then(|value| {
                        authority.check().map_err(error)?;
                        Ok(value)
                    });
                // Runtime latches at its actual pre-effect boundary. Do not sample
                // cancellation or active-owner state over its original outcome.
                completion.finish(value)
            })
            .map_err(|error| match error {
                meguri_engine::EngineError::StaleGeneration { .. } => failure(CloudConnectionFailure::AuthorityChanged),
                _ => failure(CloudConnectionFailure::Closed),
            })?;
        Ok((crate::jobs::wrap(handle), cell))
    }
}
#[uniffi::export]
impl CloudConnection {
    pub fn start_next_publication_work(self: Arc<Self>) -> Result<CloudPublicationWorkJob, CoreError> {
        callback::enter("CloudConnection.start_next_publication_work");
        let generation = self.owner.current_generation()?;
        let (handle, cell) = self.start_connection(
            "CloudConnection.next_publication_work",
            generation,
            move |owner, work| owner.next_publication_work_with_work(generation, work),
        )?;
        Ok(CloudPublicationWorkJob {
            handle,
            result: Arc::new(CloudPublicationWorkResult { cell }),
        })
    }
    pub fn start_resume_adoption(
        self: Arc<Self>,
        operation: Arc<CloudRecoveryOperation>,
    ) -> Result<CloudAdoptionResumeJob, CoreError> {
        callback::enter("CloudConnection.start_resume_adoption");
        self.owns(&operation.owner)?;
        if !operation.adoption {
            return Err(failure(CloudConnectionFailure::NotOffered));
        }
        let (handle, cell) = self.start_connection(
            "CloudConnection.resume_adoption",
            operation.generation,
            move |owner, work| {
                owner
                    .owner
                    .host
                    .resume_cloud_adoption_with_work(operation.id, work)
                    .map(|resumed| CloudAdoptionResumed { resumed })
                    .map_err(error)
            },
        )?;
        Ok(CloudAdoptionResumeJob {
            handle,
            result: Arc::new(CloudAdoptionResumeResult { cell }),
        })
    }
    pub fn start_adoption_result(
        self: Arc<Self>,
        handle: Arc<CloudAdoptionWork>,
        result: CloudAdoptionResult,
    ) -> Result<CloudAdoptionResultJob, CoreError> {
        callback::enter("CloudConnection.start_adoption_result");
        self.owns(&handle.owner)?;
        let diagnostic = Arc::new(std::sync::OnceLock::new());
        let frozen = diagnostic.clone();
        let (handle, cell) = self.start_connection(
            "CloudConnection.adoption_result",
            handle.generation,
            move |owner, work| {
                let mut recorder = a::AdoptionDiagnosticRecorder::new(&handle.work.task);
                let outcome = owner
                    .adoption_result_with_work(handle, result, work, &mut recorder)
                    .map(|()| CloudAdoptionApplied { applied: true });
                // Freeze before start_connection publishes the existing typed
                // completion. This never changes its outcome or final check.
                let _ = frozen.set(recorder.snapshot().map(Into::into));
                outcome
            },
        )?;
        Ok(CloudAdoptionResultJob {
            handle,
            result: Arc::new(CloudAdoptionAppliedResult { cell, diagnostic }),
        })
    }
}
