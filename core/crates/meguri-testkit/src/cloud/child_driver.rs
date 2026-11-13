//! One bounded IPC driver shared by the cloud integration targets.
use super::{
    Scope,
    devices::{self, IdentityReads, PreboundDevice, Result, failure},
};
use meguri_engine::{Generation, Seq, SessionId, WalCoverage};
use meguri_model::{
    changes::OpSource,
    clock::{Clock, StepClock, UtcOffset},
    ids::{CardId, DeckId, NoteId},
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table, space::SpaceProvider};
use meguri_sync::cloud::{
    self as core, entity_codec as codec,
    runtime::{self, IdentityProvider, IdentityStatus},
    workflow::*,
};
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::Sha256;
use std::{
    collections::{BTreeMap, BTreeSet},
    io::{Read, Write},
    os::fd::{AsRawFd, FromRawFd},
    os::unix::{
        net::UnixStream,
        process::{CommandExt, ExitStatusExt},
    },
    process::{Child, Command, Stdio},
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};
const FRAME: usize = 16 * 1024 * 1024;
const IPC_TIMEOUT: Duration = Duration::from_secs(30);
static TRASH_WRITE: Mutex<Option<(std::path::PathBuf, Vec<u8>, bool)>> = Mutex::new(None);
fn write_during_trash() {
    use std::os::unix::fs::MetadataExt;
    meguri_model::faultpoint::disarm("media.trash.before_move");
    let (path, bytes, in_place) = TRASH_WRITE.lock().unwrap().take().unwrap();
    if in_place {
        let before = std::fs::metadata(&path).unwrap();
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .truncate(true)
            .open(&path)
            .unwrap();
        file.write_all(&bytes).unwrap();
        file.sync_all().unwrap();
        let after = file.metadata().unwrap();
        assert_eq!((before.dev(), before.ino()), (after.dev(), after.ino()));
    } else {
        let mut replacement = tempfile::NamedTempFile::new_in(path.parent().unwrap()).unwrap();
        replacement.write_all(&bytes).unwrap();
        replacement.as_file().sync_all().unwrap();
        replacement.persist(&path).unwrap();
    }
}
pub use core::incoming::{ApplyIntent, ApplyKeyRole};
use core::testing::IncomingStage;
pub use core::testing::{ApplyInterruption, TrackingFault};
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RolloverObservation {
    pub seq: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IncomingObservation {
    pub seq: u64,
    pub native_file: runtime::NativeFileIdentity,
    pub generation: u64,
    pub col_mod: i64,
    pub undo_depth: usize,
    pub change_events: Vec<u64>,
    pub engine_instance: Option<String>,
    pub engine_state: Option<Vec<u8>>,
    pub last_fetch_engine_state: Option<Vec<u8>>,
    pub runs: Vec<core::incoming::ApplyRun>,
    pub wal_receipts: Vec<Receipt>,
    pub rollovers: Vec<RolloverObservation>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AddedNote {
    pub note: codec::Note,
    pub cards: Vec<codec::Card>,
    pub seq: u64,
    pub generation: u64,
    pub native_file: runtime::NativeFileIdentity,
}
/// A granted bounded worker can be paused at an observed real boundary.
#[derive(Clone, Debug)]
pub enum IncomingProgress {
    Completed(usize),
    Paused(IncomingStage),
}
/// First and highest readings at one fixed stage, each (cumulative peak, current bytes).
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct PayloadMemoryStageSamples {
    pub stage: IncomingStage,
    pub first: (usize, usize),
    pub high_water: (usize, usize),
}
#[derive(Debug)]
struct WorkPaused;
impl std::fmt::Display for WorkPaused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cloud child worker paused")
    }
}
impl std::error::Error for WorkPaused {}
pub(crate) fn is_work_paused(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.is::<WorkPaused>()
}

#[derive(Debug)]
struct ResetRecoveryUnavailable;
impl std::fmt::Display for ResetRecoveryUnavailable {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("cloud child reset recovery unavailable")
    }
}
impl std::error::Error for ResetRecoveryUnavailable {}
/// Exact production guard, preserved through the payload-free child channel.
pub fn is_reset_recovery_unavailable(error: &(dyn std::error::Error + Send + Sync + 'static)) -> bool {
    error.is::<ResetRecoveryUnavailable>()
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingInboxEntry {
    pub id: i64,
    /// SHA-256 of the JSON serialization of the complete stored InboxValue.
    pub value_sha256: [u8; 32],
    pub record_name: String,
    /// Zero only for a deletion or an envelope with no readable revision.
    pub erev: u64,
    pub reason: core::InboxReason,
    pub source: core::InboxSource,
    pub retry_count: u32,
}
fn inbox_value_sha256(value: &core::InboxValue) -> Result<[u8; 32]> {
    struct HashWriter(Sha256);
    impl Write for HashWriter {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            sha2::Digest::update(&mut self.0, bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let mut writer = HashWriter(<Sha256 as sha2::Digest>::new());
    serde_json::to_writer(&mut writer, value)?;
    Ok(sha2::Digest::finalize(writer.0).into())
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct IncomingRefusedEntry {
    pub record_name: String,
    pub erev: u64,
    pub version_known: bool,
    pub origin: Option<[u8; 16]>,
    pub reason: String,
}
pub use core::workflow::PayloadTrackingProof as PayloadProof;
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Edit {
    pub note: codec::Note,
    pub seq: u64,
    pub generation: u64,
    pub native_file: runtime::NativeFileIdentity,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Receipt {
    pub through: u64,
    pub generation: u64,
    pub native_file: runtime::NativeFileIdentity,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanProof {
    pub snapshot_seq: u64,
    pub snapshot_generation: u64,
    pub snapshot_native_file: runtime::NativeFileIdentity,
    pub receipt: Receipt,
    pub recovered_names: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CaptureProof {
    pub observed_seq: u64,
    pub generation: u64,
    pub attempted: bool,
    pub persisted: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ScanState {
    pub scan_required: bool,
    pub scan_running: bool,
    pub scan_completed_this_open: bool,
    pub tracking_stopped: bool,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewAnswer {
    pub revlog_id: i64,
    pub op_id: u64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReviewHold {
    pub op_ids: BTreeSet<u64>,
    pub entity_names: BTreeSet<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AdoptionCompletion {
    pub identity_changes: usize,
    pub slot_available: bool,
    pub stored_usable: bool,
    pub identity_verified: bool,
    pub proof_matches: bool,
    pub replacement_recorded: bool,
}
#[derive(Clone, Debug)]
pub struct Killed {
    pub signal: Option<i32>,
    pub clean_close: bool,
}
#[derive(Serialize, Deserialize)]
pub(crate) struct Boot {
    directory: std::path::PathBuf,
    scope: Scope,
    stable: Option<(i64, core::BindingId)>,
    identity: IdentityReads,
    automatic: bool,
    now: i64,
    seed: u64,
}
/// Parent-staged Service bytes are disposable sources, never durable ownership.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FetchedAssetRecord {
    pub record: Saved,
    pub asset_path: Option<std::path::PathBuf>,
}
/// Only the owned Fetched callback projects envelopes out of Saved. Other
/// download/result commands keep their existing inline envelope transport.
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FetchedEnvelopeSource {
    path: std::path::PathBuf,
    ranges: Vec<(u64, u64)>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct FetchedCallback {
    pub instance: String,
    pub records: Vec<FetchedAssetRecord>,
    pub epochs: Vec<(usize, super::Record)>,
    pub scope: Scope,
    pub envelopes: Option<FetchedEnvelopeSource>,
    pub zone: String,
    pub serialization: Vec<u8>,
    pub exchange: Option<u64>,
}
/// One-shot damage to the original driver's next nonempty fetched envelope spool.
#[derive(Clone, Copy)]
pub enum FetchedEnvelopeFault {
    Missing,
    Short,
}
/// Observations of the original command's disposable sources, without exposing paths.
#[derive(Clone)]
pub struct FetchedEnvelopeFaultObservation {
    pub exchange: Option<u64>,
    pub exchange_active_before_fault: bool,
    pub staged_sources: usize,
    pub sources_after_fault: usize,
    paths: Vec<std::path::PathBuf>,
}
impl FetchedEnvelopeFaultObservation {
    pub fn existing_sources(&self) -> usize {
        self.paths.iter().filter(|path| path.exists()).count()
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) enum Input {
    Boot(Boot),
    Instance,
    Validate(String),
    SignOut,
    Reopen,
    BodyReads,
    PayloadMemoryReset,
    PayloadMemoryPeak,
    PayloadMemoryStages,
    PayloadRecordLimit(usize),
    EditGeneratedPayload(i64, usize, u64),
    PayloadProof(String),
    StoredState,
    Identity(Option<String>),
    Reconcile,
    MediaWork,
    MediaQueue(core::media::Work),
    MediaBuild(BTreeMap<String, PendingChange>),
    MediaAdmit(u64),
    MediaSent(
        String,
        u64,
        Vec<Saved>,
        Vec<Saved>,
        Vec<(String, SendError)>,
        Vec<String>,
        Vec<(String, SendError)>,
        Vec<String>,
    ),
    MediaResume,
    MediaAdapterPause(bool),
    MediaTrash(String),
    MediaConflicts,
    MediaBlobs,
    MediaReply(core::media::Work, core::media::Reply),
    MediaValidate(core::media::Work),
    FailMediaIo,
    WriteDuringMediaTrash(String, Vec<u8>, bool),
    StartSDK(String),
    Facts,
    LastAppliedActive,
    ScanState,
    ScanProof,
    Capture(u64, u64),
    Wal(u64),
    ArmLoss(i64),
    ArmCaptureFailure(usize),
    ArmAdoptionActivationCrash,
    ArmAdoptionAccountChange(Option<String>),
    AdoptionCompletion(core::adoption::AdoptOp),
    Edit(i64, Vec<String>),
    PeekUndoReason,
    Undo,
    RemoveRevlog(i64),
    AddNote(i64, i64, Vec<String>),
    DeleteNote(i64),
    BuryCard(i64),
    WriteCardData(i64, String),
    RekeyDeckFixture(i64, bool),
    RekeyConfigValue(String, i64),
    GeneratedCardData(i64, usize),
    RekeyRowsProof(i64, i64, i64),
    IncomingObservation(core::BindingId),
    IncomingSubmission,
    InterruptIncoming(ApplyInterruption),
    TrackingFault(String, TrackingFault),
    LatchHold(IncomingStage),
    LatchRelease(IncomingStage),
    ContinueWork,
    Payloads,
    CheckObservation,
    Entity(String),
    AllEntities,
    Poll(String),
    Trace,
    Observations,
    Emitted,
    Updates(String),
    Transition(String, UpdateKind, PendingSnapshot),
    Sendables,
    Build(String, BTreeSet<String>),
    EnterHandler(String, u64),
    ReturnHandler(String),
    Sent(String, u64, Vec<Saved>, Vec<Saved>, Vec<(String, SendError)>, usize),
    AppliedUpdate(String, u64, Vec<String>),
    SavedRemoved(String, u64, Vec<String>),
    Qualify(String),
    RootRead(String, u64, Vec<u8>, Vec<u8>, bool),
    RootFetch(String, Option<Saved>, bool, u64),
    ReadDue,
    RootEpoch,
    CollectionZone,
    SyncNow(String),
    FetchDue,
    FetchLifecycle,
    FetchValidate(String),
    FetchBegin(String, Vec<String>),
    FetchWill(String, String, bool),
    FetchZone(String, String, bool, bool),
    FetchDid(String, String, bool),
    FetchFinish(String, u64),
    FetchFailed(String, u64),
    FetchApply(String, Option<u64>),
    FetchCompleted(String, FetchPhase, BTreeSet<String>),
    Fetched(FetchedCallback),
    IncomingIntents(core::BindingId),
    IncomingInbox(core::BindingId),
    IncomingRefused(core::BindingId),
    BindingEntity(core::BindingId, String),
    ServiceEpoch(String, u64, Scope, String, super::Record),
    EpochReadability,
    DeliverCollection(Vec<Saved>),
    DeliverCollectionAssets(Vec<super::Record>),
    DeliverCollectionDownload(FetchedAssetRecord),
    AssetReadPass(core::incoming::AssetReadKind),
    NextAssetRead(core::incoming::AssetReadPass),
    AdmitAssetRead(core::incoming::AssetReadRequest, bool),
    CancelAssetRead(core::incoming::AssetReadRequest),
    CancelAssetPass(core::incoming::AssetReadPass),
    AssetReadResult(core::incoming::AssetReadRequest, Option<FetchedAssetRecord>),
    AssetReadOwnership(core::incoming::AssetReadRequest),
    AssetSDK,
    AssetSuspendResume,
    AssetConflictBodyReads,
    EntityBodyReads,
    SavedAckBodyReads,
    MediaPolicyBlobCandidateVisits,
    ResetMediaPolicyBlobCandidateVisits,
    PolicyMediaFacts,
    AssetBodyReadControl(core::BindingId, String),
    AssetConflict(String, u64, Saved),
    DrainIncoming(usize),
    PauseAutomaticWAL(bool),
    CloudHold(u64),
    SendsFinished(String),
    WantsSend,
    StartCall(String),
    RecordCall(String, u64, u64),
    FinishCall(String, u64),
    Calls,
    Exchange(u64),
    FetchedExchange(u64),
    Begin(i64),
    Start(u64),
    Show(u64, i64),
    Good(u64, i64),
    End(u64),
    ActiveReviewSession,
    Hold(u64),
    Card(i64),
    Revlog(i64),
    Advance(i64),
    Close,
    BackupPath(String),
    BackupNotices,
    RegisterSDK(Option<String>),
    RootConfirmed,
    PreviewBegin,
    PreviewActive(u64, core::publication::RecordRead),
    PreviewEpoch(u64, core::publication::RecordRead),
    PreviewCancel,
    PublicationConfirm(core::publication::Preview),
    AdoptionConfirm(core::publication::Preview),
    AdoptionReply(core::adoption::AdoptionWork, core::adoption::AdoptionReply),
    AdoptionLocal(core::adoption::AdoptionWork),
    Capacity(Option<u64>),
    PadAdoptionCandidate,
    PublicationNext,
    PublicationLocal([u8; 16], core::publication::LocalWork),
    PublicationExecute(core::publication::OwnRequest),
    PublicationCancel([u8; 16]),
    PublicationTrigger,
    PublicationFacts,
    AuthorityStatus,
    PublicationControl,
    SelectedControl,
    PublicationReplyEvents,
    CollectionRows,
    MediaRows,
    CollectionHeader,
    BackupNow,
    RestoreBackup(String),
    BackupRows(String),
    BackupRetention(u32, u32, u32),
    PutMedia(String, Vec<u8>),
    MediaFiles,
    RecoveryState,
    BeginZoneListing(String),
    ZoneListingResult(Option<Vec<String>>),
    CollectionDrained,
    DeletionGuard,
    DeletionCheck(core::publication::RecordRead),
    DeleteZone(String),
    KeyResult(std::result::Result<[u8; 32], core::runtime::KeySourceError>),
    TransportAccepted,
    TransportResult(super::Response),
}
#[derive(Serialize, Deserialize)]
enum ChildMessage {
    Reply(std::result::Result<serde_json::Value, ChildFailure>),
    DrawKey,
    Submit(super::Caller, super::Operation),
    Paused(IncomingStage),
}

// This closed catalog is the entire error channel: no source error or payload
// is serialized. Capacity failures retain only their two typed byte counts.
#[derive(Clone, Debug, Serialize, Deserialize)]
enum ChildFailure {
    Unknown,
    SDKSyncNow(Box<ChildFailure>),
    SDKUpdates(Box<ChildFailure>),
    SDKAppliedUpdate(Box<ChildFailure>),
    SDKQualify(Box<ChildFailure>),
    FetchRootAccept(Box<ChildFailure>),
    FetchCompleted(Box<ChildFailure>),
    FetchEnvelopeHydration(Box<ChildFailure>),
    FetchCursorProjection(Box<ChildFailure>),
    FetchNativeIntake(Box<ChildFailure>),
    FetchStateUpdate(Box<ChildFailure>),
    FetchCursorHook(Box<ChildFailure>),
    FetchLegacyIntake(Box<ChildFailure>),
    FetchLegacyAssetRejected,
    FetchCoreApply(Box<ChildFailure>),
    FetchCompletion(Box<ChildFailure>, Box<ChildFailure>),
    RuntimeTracking(StoreFailure),
    RuntimeEngine(EngineFailure),
    RuntimeStaleIncarnation,
    RuntimeIdentityUnverified,
    RuntimeIncompleteWholeBindingReconciliation,
    RuntimeIncompleteNewOrRemovedNote,
    RuntimeIncompleteStructuralLayout,
    RuntimeIncompleteRootConfirmationAndSending,
    RuntimeFileIdentityUnavailable,
    RuntimePublication,
    RuntimeResetRecoveryUnavailable,
    Engine(EngineFailure),
    Store(StoreFailure),
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum EngineFailure {
    InsufficientSpace { required: u64, available: u64 },
    StaleGeneration,
    CollectionUnavailable,
    Busy,
    Open,
    Storage,
    Internal,
    InvalidArgument,
    ReviewJournalNativeIdentity,
    Other,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
enum StoreFailure {
    Io,
    Database,
    Constraint,
    InvalidIdentifier,
    InvalidData,
    BindingNotFound,
    IsolatedBinding,
    RecoveryRequired,
    RecoveryRefused,
    RuntimeSchemaUnsupported,
}
impl ChildFailure {
    fn classify(error: Box<dyn std::error::Error + Send + Sync>) -> Self {
        if let Some(error) = error.downcast_ref::<Self>() {
            return error.clone();
        }
        if let Some(error) = error.downcast_ref::<runtime::RuntimeError>() {
            use runtime::{Incomplete, RuntimeError};
            return match error {
                RuntimeError::Tracking(error) => Self::RuntimeTracking(StoreFailure::classify(error)),
                RuntimeError::Engine(error) => Self::RuntimeEngine(EngineFailure::classify(error)),
                RuntimeError::StaleIncarnation => Self::RuntimeStaleIncarnation,
                RuntimeError::IdentityUnverified => Self::RuntimeIdentityUnverified,
                RuntimeError::Incomplete(incomplete) => match incomplete {
                    Incomplete::WholeBindingReconciliation => Self::RuntimeIncompleteWholeBindingReconciliation,
                    Incomplete::NewOrRemovedNote => Self::RuntimeIncompleteNewOrRemovedNote,
                    Incomplete::StructuralLayout => Self::RuntimeIncompleteStructuralLayout,
                    Incomplete::RootConfirmationAndSending => Self::RuntimeIncompleteRootConfirmationAndSending,
                },
                RuntimeError::FileIdentityUnavailable => Self::RuntimeFileIdentityUnavailable,
                RuntimeError::Publication(core::publication::PublicationError::ResetRecoveryUnavailable) => {
                    Self::RuntimeResetRecoveryUnavailable
                }
                RuntimeError::Publication(_) => Self::RuntimePublication,
            };
        }
        if let Some(error) = error.downcast_ref::<meguri_engine::EngineError>() {
            return Self::Engine(EngineFailure::classify(error));
        }
        if let Some(error) = error.downcast_ref::<core::Error>() {
            return Self::Store(StoreFailure::classify(error));
        }
        Self::Unknown
    }
    fn into_error(self) -> Box<dyn std::error::Error + Send + Sync> {
        if let Self::RuntimeEngine(EngineFailure::InsufficientSpace { required, available })
        | Self::Engine(EngineFailure::InsufficientSpace { required, available }) = self
        {
            return Box::new(meguri_storage::StorageError::InsufficientSpace { required, available });
        }
        if matches!(self, Self::RuntimeResetRecoveryUnavailable) {
            return Box::new(ResetRecoveryUnavailable);
        }
        if matches!(self, Self::Unknown) {
            return failure();
        }
        // Debug formats only our finite, payload-free catalog, never an error.
        Box::new(std::io::Error::other(format!(
            "cloud device operation failed: {self:?}"
        )))
    }
}
impl std::fmt::Display for ChildFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "cloud device operation failed: {self:?}")
    }
}
impl std::error::Error for ChildFailure {}
impl EngineFailure {
    fn classify(error: &meguri_engine::EngineError) -> Self {
        use meguri_engine::EngineError;
        match error {
            EngineError::StaleGeneration { .. } => Self::StaleGeneration,
            EngineError::CollectionUnavailable { .. } => Self::CollectionUnavailable,
            EngineError::Busy { .. } => Self::Busy,
            EngineError::Open(_) => Self::Open,
            EngineError::Storage(meguri_storage::StorageError::InsufficientSpace { required, available }) => {
                Self::InsufficientSpace {
                    required: *required,
                    available: *available,
                }
            }
            EngineError::Storage(_) => Self::Storage,
            EngineError::Internal { .. } => Self::Internal,
            EngineError::InvalidArgument { message }
                if message == "review journal: unexpected native collection identity; evidence retained" =>
            {
                Self::ReviewJournalNativeIdentity
            }
            EngineError::InvalidArgument { .. } => Self::InvalidArgument,
            _ => Self::Other,
        }
    }
}
impl StoreFailure {
    fn classify(error: &core::Error) -> Self {
        match error {
            core::Error::Io(_) => Self::Io,
            core::Error::Database => Self::Database,
            core::Error::Constraint => Self::Constraint,
            core::Error::InvalidIdentifier => Self::InvalidIdentifier,
            core::Error::InvalidData => Self::InvalidData,
            core::Error::BindingNotFound => Self::BindingNotFound,
            core::Error::IsolatedBinding => Self::IsolatedBinding,
            core::Error::RecoveryRequired => Self::RecoveryRequired,
            core::Error::RecoveryRefused => Self::RecoveryRefused,
            core::Error::RuntimeSchemaUnsupported => Self::RuntimeSchemaUnsupported,
        }
    }
}

fn write_frame<T: Serialize>(s: &mut UnixStream, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > FRAME {
        return Err(failure());
    }
    s.write_all(&u32::try_from(bytes.len())?.to_be_bytes())?;
    s.write_all(&bytes)?;
    Ok(())
}
fn read_frame<T: DeserializeOwned>(s: &mut UnixStream) -> Result<T> {
    let mut header = [0; 4];
    s.read_exact(&mut header)?;
    read_frame_body(s, header)
}
fn read_idle_command<T: DeserializeOwned>(s: &mut UnixStream) -> Result<T> {
    let timeout = s.read_timeout()?;
    s.set_read_timeout(None)?;
    let mut header = [0; 4];
    let first = s.read_exact(&mut header[..1]);
    // Restore before propagating EOF/errors or reading any remaining frame bytes.
    s.set_read_timeout(timeout)?;
    first?;
    s.read_exact(&mut header[1..])?;
    read_frame_body(s, header)
}
fn read_frame_body<T: DeserializeOwned>(s: &mut UnixStream, header: [u8; 4]) -> Result<T> {
    let n = u32::from_be_bytes(header) as usize;
    if n > FRAME {
        return Err(failure());
    }
    let mut bytes = vec![0; n];
    s.read_exact(&mut bytes)?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub struct ChildDriver {
    child: Option<Child>,
    stream: UnixStream,
    keys: Option<Arc<dyn runtime::KeyByteSource>>,
    queued: Option<super::coordinator::HeldRequest>,
    queued_response: Option<super::Response>,
    pending_work: Option<Vec<u8>>,
    suspended_work: Vec<(Vec<u8>, Option<IncomingStage>)>,
    completed_work: Vec<(Vec<u8>, std::result::Result<serde_json::Value, ChildFailure>)>,
    paused_work: Option<IncomingStage>,
    fetched_sources: BTreeMap<Vec<u8>, Vec<tempfile::NamedTempFile>>,
    fetched_envelope_fault: Option<FetchedEnvelopeFault>,
    fetched_envelope_fault_observation: Option<FetchedEnvelopeFaultObservation>,
    asset_commands: BTreeMap<Vec<u8>, Vec<u8>>,
}
impl ChildDriver {
    pub(crate) fn check_observation(&mut self) -> Result<super::checks::Observation> {
        self.call(Input::CheckObservation)
    }
    pub fn fault_next_fetched_envelope_for_testing(&mut self, fault: FetchedEnvelopeFault) {
        self.fetched_envelope_fault = Some(fault);
        self.fetched_envelope_fault_observation = None;
    }
    pub fn fetched_envelope_fault_observation_for_testing(&self) -> Option<FetchedEnvelopeFaultObservation> {
        self.fetched_envelope_fault_observation.clone()
    }
    pub fn fetched_source_count_for_testing(&self) -> usize {
        self.fetched_sources.values().map(Vec::len).sum()
    }
    pub fn fetched_exchange_for_testing(
        &mut self,
        id: u64,
    ) -> Result<(bool, Option<core::adapter::CompletedExchange>)> {
        self.call(Input::FetchedExchange(id))
    }
    pub(crate) fn stage_fetched(
        &mut self,
        instance: String,
        records: Vec<super::Record>,
        scope: Scope,
        zone: String,
        serialization: Vec<u8>,
        exchange: Option<u64>,
    ) -> Result<FetchedCallback> {
        let mut files = Vec::new();
        let mut inputs = Vec::new();
        let mut epochs = Vec::new();
        let mut envelopes = None;
        let mut ranges = Vec::new();
        let mut offset = 0_u64;
        for (index, record) in records.into_iter().enumerate() {
            // Keep control transport metadata until the child validates it.
            if record.name == "epoch" || record.record_type == "MGEpoch" {
                epochs.push((index, record));
                continue;
            }
            let Some(super::Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                return Err(Box::new(std::io::Error::other("fetched callback: envelope shape")));
            };
            let length = u64::try_from(envelope.len())?;
            if length > super::Limits::default().non_asset_per_record {
                return Err(Box::new(std::io::Error::other("fetched callback: envelope length")));
            }
            if envelopes.is_none() {
                envelopes = Some(
                    tempfile::NamedTempFile::new()
                        .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: envelope create"))?,
                );
            }
            envelopes
                .as_mut()
                .ok_or_else(failure)?
                .write_all(envelope)
                .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: envelope write"))?;
            ranges.push((offset, length));
            offset = offset.checked_add(length).ok_or_else(failure)?;
            let asset_path = match record.fields.get("pa") {
                Some(super::Field::Asset(asset)) => {
                    let mut file = tempfile::NamedTempFile::new()
                        .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: asset create"))?;
                    file.write_all(asset.bytes())
                        .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: asset write"))?;
                    file.as_file()
                        .sync_all()
                        .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: asset sync"))?;
                    let path = file.path().to_path_buf();
                    files.push(file);
                    Some(path)
                }
                None => None,
                _ => return Err(Box::new(std::io::Error::other("fetched callback: asset shape"))),
            };
            inputs.push(FetchedAssetRecord {
                record: Saved {
                    name: record.name,
                    envelope: Vec::new(),
                    system_fields: record.system_fields.bytes().to_vec(),
                },
                asset_path,
            });
        }
        // These are disposable, unbuffered transport writes. Runtime still owns
        // all durable intake; the spool needs no per-record fsync.
        let envelopes = envelopes.map(|file| {
            let source = FetchedEnvelopeSource {
                path: file.path().to_path_buf(),
                ranges,
            };
            files.push(file);
            source
        });
        let callback = FetchedCallback {
            instance,
            records: inputs,
            epochs,
            scope,
            envelopes,
            zone,
            serialization,
            exchange,
        };
        let key = serde_json::to_vec(&Input::Fetched(callback.clone()))?;
        self.fetched_sources.insert(key.clone(), files);
        if callback.envelopes.is_some()
            && let Some(fault) = self.fetched_envelope_fault.take()
        {
            let exchange_active_before_fault = match callback.exchange {
                Some(id) => self.fetched_exchange_for_testing(id)?.0,
                None => false,
            };
            // The last owned file is this callback's envelope spool. Damage it
            // only after staging, leaving the command and every asset untouched.
            let files = self.fetched_sources.get(&key).ok_or_else(failure)?;
            let spool = files.last().ok_or_else(failure)?;
            match fault {
                FetchedEnvelopeFault::Missing => std::fs::remove_file(spool.path()),
                FetchedEnvelopeFault::Short => spool.as_file().set_len(offset.checked_sub(1).ok_or_else(failure)?),
            }
            .map_err(|error| std::io::Error::new(error.kind(), "fetched callback: envelope fault"))?;
            let paths: Vec<_> = files.iter().map(|file| file.path().to_path_buf()).collect();
            self.fetched_envelope_fault_observation = Some(FetchedEnvelopeFaultObservation {
                exchange: callback.exchange,
                exchange_active_before_fault,
                staged_sources: paths.len(),
                sources_after_fault: paths.iter().filter(|path| path.exists()).count(),
                paths,
            });
        }
        Ok(callback)
    }
    pub fn apply_intents(&mut self, binding: core::BindingId) -> Result<Vec<ApplyIntent>> {
        self.call(Input::IncomingIntents(binding))
    }
    pub fn inbox_entries(&mut self, binding: core::BindingId) -> Result<Vec<IncomingInboxEntry>> {
        self.call(Input::IncomingInbox(binding))
    }
    pub fn refused_entries(&mut self, binding: core::BindingId) -> Result<Vec<IncomingRefusedEntry>> {
        self.call(Input::IncomingRefused(binding))
    }
    pub fn binding_entity(&mut self, binding: core::BindingId, name: &str) -> Result<Option<EntityFacts>> {
        self.call(Input::BindingEntity(binding, name.into()))
    }
    pub fn deliver_collection_records(&mut self, records: &[super::Record]) -> Result<()> {
        if records.iter().any(|record| record.fields.contains_key("pa")) {
            return self.call(Input::DeliverCollectionAssets(records.to_vec()));
        }
        let records = records
            .iter()
            .map(|r| {
                let Some(super::Field::EncryptedBytes(bytes)) = r.fields.get("e") else {
                    return Err(failure());
                };
                Ok(Saved {
                    name: r.name.clone(),
                    envelope: bytes.clone(),
                    system_fields: r.system_fields.bytes().to_vec(),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        self.call(Input::DeliverCollection(records))
    }
    /// Deliver unchanged Service metadata with this download's optional file.
    /// The parent owns the source through the final reply, including pauses;
    /// only Runtime's normal intake may classify or durably copy its bytes.
    pub fn deliver_collection_asset_download(
        &mut self,
        record: &super::Record,
        asset_path: Option<&std::path::Path>,
    ) -> Result<()> {
        let Some(super::Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
            return Err(failure());
        };
        self.asset_input(Input::DeliverCollectionDownload(FetchedAssetRecord {
            record: Saved {
                name: record.name.clone(),
                envelope: envelope.clone(),
                system_fields: record.system_fields.bytes().to_vec(),
            },
            asset_path: asset_path.map(std::path::Path::to_path_buf),
        }))
    }
    // Keep both the original transport command and its files through its final
    // reply. A repeated helper call resumes that command with the same paths.
    fn asset_input<T: DeserializeOwned>(&mut self, mut input: Input) -> Result<T> {
        let caller_key = serde_json::to_vec(&input)?;
        let original = if let Some(original) = self.asset_commands.get(&caller_key) {
            input = serde_json::from_slice(original)?;
            original.clone()
        } else {
            let mut files = Vec::new();
            let record = match &mut input {
                Input::DeliverCollectionDownload(record) => Some(record),
                Input::AssetReadResult(_, record) => record.as_mut(),
                _ => return Err(failure()),
            };
            if let Some(record) = record
                && let Some(path) = &record.asset_path
            {
                let mut file = tempfile::NamedTempFile::new()?;
                std::io::copy(&mut std::fs::File::open(path)?, &mut file)?;
                file.as_file().sync_all()?;
                record.asset_path = Some(file.path().to_path_buf());
                files.push(file);
            }
            let original = serde_json::to_vec(&input)?;
            self.fetched_sources.insert(original.clone(), files);
            self.asset_commands.insert(caller_key.clone(), original.clone());
            original
        };
        let result = self.call(input);
        if !self.fetched_sources.contains_key(&original) {
            self.asset_commands.remove(&caller_key);
        }
        result
    }
    pub fn asset_read_requests(
        &mut self,
        kind: core::incoming::AssetReadKind,
    ) -> Result<Vec<core::incoming::AssetReadRequest>> {
        // Legacy single-entry fixtures still cross the real three boundaries.
        // This helper admits only the selected request, never a bulk vector.
        let pass = self.begin_asset_read_pass(kind)?;
        let request = self.next_asset_read(&pass)?;
        if let Some(request) = &request
            && !self.admit_asset_read(request, true)?
        {
            return Err(failure());
        }
        self.call::<()>(Input::CancelAssetPass(pass))?;
        Ok(request.into_iter().collect())
    }
    pub fn begin_asset_read_pass(
        &mut self,
        kind: core::incoming::AssetReadKind,
    ) -> Result<core::incoming::AssetReadPass> {
        self.call(Input::AssetReadPass(kind))
    }
    pub fn next_asset_read(
        &mut self,
        pass: &core::incoming::AssetReadPass,
    ) -> Result<Option<core::incoming::AssetReadRequest>> {
        self.call(Input::NextAssetRead(pass.clone()))
    }
    pub fn admit_asset_read(&mut self, request: &core::incoming::AssetReadRequest, accept: bool) -> Result<bool> {
        self.call(Input::AdmitAssetRead(request.clone(), accept))
    }
    pub fn cancel_asset_read(&mut self, request: &core::incoming::AssetReadRequest) -> Result<bool> {
        self.call(Input::CancelAssetRead(request.clone()))
    }
    /// Observe the actual core result owner and accepted transport queue entry.
    pub fn asset_read_ownership(&mut self, request: &core::incoming::AssetReadRequest) -> Result<(bool, bool)> {
        self.call(Input::AssetReadOwnership(request.clone()))
    }
    pub fn asset_read_result(
        &mut self,
        request: &core::incoming::AssetReadRequest,
        record: Option<&super::Record>,
        asset_path: Option<&std::path::Path>,
    ) -> Result<bool> {
        let result = record
            .map(|record| -> Result<_> {
                let Some(super::Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                    return Err(failure());
                };
                Ok(FetchedAssetRecord {
                    record: Saved {
                        name: record.name.clone(),
                        envelope: envelope.clone(),
                        system_fields: record.system_fields.bytes().to_vec(),
                    },
                    asset_path: asset_path.map(std::path::Path::to_path_buf),
                })
            })
            .transpose()?;
        self.asset_input(Input::AssetReadResult(request.clone(), result))
    }
    pub fn suspend_resume_asset_sdk(&mut self) -> Result<(String, String)> {
        self.call(Input::AssetSuspendResume)
    }
    /// Enter/return the real Runtime handler without supplying a sent result.
    pub fn enter_asset_read_sent_handler(&mut self, sdk: &str, request_id: u64) -> Result<()> {
        self.call(Input::EnterHandler(sdk.to_owned(), request_id))
    }
    pub fn return_asset_read_sent_handler(&mut self, sdk: &str) -> Result<u64> {
        self.call(Input::ReturnHandler(sdk.to_owned()))
    }
    pub fn hold_asset_read_prepared(&mut self) -> Result<()> {
        self.hold_incoming_stage(IncomingStage::AssetReadPrepared)
    }
    pub fn release_asset_read_prepared(&mut self) -> Result<()> {
        self.release_incoming_stage(IncomingStage::AssetReadPrepared)
    }
    pub fn hold_asset_read_finishing(&mut self) -> Result<()> {
        self.hold_incoming_stage(IncomingStage::AssetReadFinishing)
    }
    pub fn release_asset_read_finishing(&mut self) -> Result<()> {
        self.release_incoming_stage(IncomingStage::AssetReadFinishing)
    }
    pub fn asset_conflict_body_reads(&mut self) -> Result<usize> {
        self.call(Input::AssetConflictBodyReads)
    }
    pub fn entity_body_reads(&mut self) -> Result<usize> {
        self.call(Input::EntityBodyReads)
    }
    pub fn saved_ack_body_reads(&mut self) -> Result<Option<usize>> {
        self.call(Input::SavedAckBodyReads)
    }
    pub fn media_policy_blob_candidate_visits(&mut self) -> Result<usize> {
        self.call(Input::MediaPolicyBlobCandidateVisits)
    }
    pub fn reset_media_policy_blob_candidate_visits(&mut self) -> Result<()> {
        self.call(Input::ResetMediaPolicyBlobCandidateVisits)
    }
    /// Actual Runtime projection for the ready collection, with its real generation.
    pub fn policy_media_facts(&mut self) -> Result<core::policy::MediaFacts> {
        self.call(Input::PolicyMediaFacts)
    }
    pub fn asset_body_read_control(&mut self, binding: core::BindingId, name: &str) -> Result<usize> {
        self.call(Input::AssetBodyReadControl(binding, name.to_owned()))
    }
    pub fn recreate_asset_sdk(&mut self) -> Result<()> {
        let old: String = self.call(Input::AssetSDK)?;
        let _: (String, Option<Vec<u8>>, String) = self.call(Input::RegisterSDK(Some(old)))?;
        Ok(())
    }
    /// Submit a local save to Service and forward its conflict response to the
    /// child while retaining the flight created by the production send path.
    pub fn deliver_real_asset_save_conflict(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
        name: &str,
    ) -> Result<()> {
        self.deliver_real_asset_save_conflict_inner(service, scope, name, false)?;
        Ok(())
    }
    /// Captures full handoff facts only for small fixtures that fit IPC frames.
    pub fn deliver_real_asset_save_conflict_with_small_handoff(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
        name: &str,
    ) -> Result<EntityFacts> {
        self.deliver_real_asset_save_conflict_inner(service, scope, name, true)?
            .ok_or_else(failure)
    }
    fn deliver_real_asset_save_conflict_inner(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
        name: &str,
        capture_handoff: bool,
    ) -> Result<Option<EntityFacts>> {
        let sdk: String = self.call(Input::AssetSDK)?;
        let batch: BuiltBatch = self.call(Input::Build(sdk.clone(), BTreeSet::from([name.to_owned()])))?;
        let handoff = if capture_handoff {
            Some(self.call(Input::Entity(name.to_owned()))?)
        } else {
            None
        };
        let save = batch.saves.iter().find(|save| save.name == name).ok_or_else(failure)?;
        let mut record = super::RecordInput::encrypted(name, "MGEntity", save.envelope.clone());
        if let Some(path) = &save.asset_path {
            record.fields.insert(
                "pa".into(),
                super::Field::Asset(super::Asset::synthetic(std::fs::read(path)?)),
            );
        }
        let zone: String = self.call(Input::CollectionZone)?;
        let response = service.request(
            scope,
            Default::default(),
            super::Operation::Modify {
                zone,
                atomic: false,
                changes: vec![super::Mutation::Save {
                    record,
                    if_unchanged: save.system_fields.clone().map(super::SystemFields::from_bytes),
                }],
            },
        )?;
        let Ok(super::Reply::Modified(items)) = response.result else {
            return Err(failure());
        };
        let Some(item) = items.into_iter().next() else {
            return Err(failure());
        };
        let Err(error) = item.result else { return Err(failure()) };
        if error.code != super::ErrorCode::ServerRecordChanged {
            return Err(failure());
        }
        let server = error.server_record.ok_or_else(failure)?;
        let Some(super::Field::EncryptedBytes(envelope)) = server.fields.get("e") else {
            return Err(failure());
        };
        self.call::<()>(Input::EnterHandler(sdk.clone(), batch.id))?;
        self.call::<()>(Input::AssetConflict(
            sdk.clone(),
            batch.id,
            Saved {
                name: server.name.clone(),
                envelope: envelope.clone(),
                system_fields: server.system_fields.bytes().to_vec(),
            },
        ))?;
        let _: u64 = self.call(Input::ReturnHandler(sdk))?;
        Ok(handoff)
    }
    pub fn drain_incoming(&mut self, steps: usize) -> Result<IncomingProgress> {
        match self.call(Input::DrainIncoming(steps)) {
            Ok(count) => Ok(IncomingProgress::Completed(count)),
            Err(error) if is_work_paused(error.as_ref()) => {
                Ok(IncomingProgress::Paused(self.paused_work.ok_or_else(failure)?))
            }
            Err(error) => Err(error),
        }
    }
    pub fn undo(&mut self) -> Result<()> {
        self.call(Input::Undo)
    }
    pub fn peek_undo_reason(&mut self) -> Result<Option<String>> {
        self.call(Input::PeekUndoReason)
    }
    pub fn remove_revlog(&mut self, id: i64) -> Result<(u64, u64)> {
        self.call(Input::RemoveRevlog(id))
    }
    pub fn add_note(&mut self, mid: i64, did: i64, fields: &[&str]) -> Result<AddedNote> {
        self.call(Input::AddNote(
            mid,
            did,
            fields.iter().map(|s| (*s).to_owned()).collect(),
        ))
    }
    pub fn delete_note(&mut self, id: i64) -> Result<()> {
        self.call(Input::DeleteNote(id))
    }
    pub fn bury_card(&mut self, id: i64) -> Result<()> {
        self.call(Input::BuryCard(id))
    }
    pub fn incoming_observation(&mut self, binding: core::BindingId) -> Result<IncomingObservation> {
        self.call(Input::IncomingObservation(binding))
    }
    pub fn interrupt_next_incoming(&mut self, interruption: ApplyInterruption) -> Result<()> {
        self.call(Input::InterruptIncoming(interruption))
    }
    pub fn arm_tracking_fault(&mut self, name: &str, fault: TrackingFault) -> Result<()> {
        self.call(Input::TrackingFault(name.into(), fault))
    }
    pub fn is_incoming_stage_held(&self, stage: IncomingStage) -> bool {
        self.paused_work == Some(stage) || self.suspended_work.iter().any(|(_, paused)| *paused == Some(stage))
    }
    pub fn hold_incoming_stage(&mut self, stage: IncomingStage) -> Result<()> {
        self.call(Input::LatchHold(stage))
    }
    pub fn release_incoming_stage(&mut self, stage: IncomingStage) -> Result<()> {
        self.call::<()>(Input::LatchRelease(stage))?;
        if self.paused_work == Some(stage) {
            self.paused_work = None;
        }
        for (_, paused) in &mut self.suspended_work {
            if *paused == Some(stage) {
                *paused = None;
            }
        }
        Ok(())
    }
    pub(crate) fn has_incoming_reply(&self, input: &Input) -> Result<bool> {
        let key = serde_json::to_vec(input)?;
        Ok(self.pending_work.as_ref() == Some(&key)
            || self.suspended_work.iter().any(|(pending, _)| *pending == key)
            || self.completed_work.iter().any(|(completed, _)| *completed == key))
    }
    pub(crate) fn resume_incoming_work(&mut self) -> Result<()> {
        if self.pending_work.is_none() {
            return Ok(());
        }
        write_frame(&mut self.stream, &Input::ContinueWork)?;
        match self.next_message()? {
            ChildMessage::Paused(stage) => {
                self.paused_work = Some(stage);
                Err(Box::new(WorkPaused))
            }
            ChildMessage::Reply(reply) => {
                self.paused_work = None;
                let key = self.pending_work.take().ok_or_else(failure)?;
                self.fetched_sources.remove(&key);
                self.completed_work.push((key, reply));
                if let Some((key, paused)) = self.suspended_work.pop() {
                    self.pending_work = Some(key);
                    self.paused_work = paused;
                }
                // The retained caller continuation consumes this reply once,
                // including its original error, through call().
                Ok(())
            }
            _ => Err(failure()),
        }
    }
    pub(crate) fn complete_pending_media_io_failure(&mut self) -> Result<()> {
        let key = self.pending_work.clone().ok_or_else(failure)?;
        self.resume_incoming_work()?;
        let index = self
            .completed_work
            .iter()
            .position(|(completed, _)| *completed == key)
            .ok_or_else(failure)?;
        let (_, reply) = self.completed_work.remove(index);
        match reply {
            Err(ChildFailure::RuntimeTracking(StoreFailure::Io)) => Ok(()),
            Err(error) => Err(error.into_error()),
            Ok(_) => Err(failure()),
        }
    }
    pub fn complete_pending_media_reply(&mut self) -> Result<()> {
        let key = self.pending_work.clone().ok_or_else(failure)?;
        if !matches!(serde_json::from_slice::<Input>(&key)?, Input::MediaReply(..)) {
            return Err(failure());
        }
        self.resume_incoming_work()?;
        let index = self
            .completed_work
            .iter()
            .position(|(completed, _)| *completed == key)
            .ok_or_else(failure)?;
        let (_, result) = self.completed_work.remove(index);
        result.map_err(ChildFailure::into_error)?;
        Ok(())
    }
    pub fn pause_media_adapter(&mut self, paused: bool) -> Result<()> {
        self.call(Input::MediaAdapterPause(paused))
    }
    pub fn write_during_media_trash(&mut self, name: &str, bytes: &[u8], in_place: bool) -> Result<()> {
        self.call(Input::WriteDuringMediaTrash(name.into(), bytes.to_vec(), in_place))
    }
    pub fn trash_media(&mut self, name: &str) -> Result<()> {
        self.call(Input::MediaTrash(name.into()))
    }
    pub fn media_conflicts(&mut self) -> Result<Vec<core::ConflictCopy>> {
        self.call(Input::MediaConflicts)
    }
    pub fn media_blobs(&mut self) -> Result<BTreeMap<String, core::media::BlobState>> {
        self.call(Input::MediaBlobs)
    }
    pub fn pause_automatic_wal_flush_for_testing(&mut self, paused: bool) -> Result<()> {
        self.call(Input::PauseAutomaticWAL(paused))
    }
    pub fn read_cloud_review_hold(&mut self, session: u64) -> Result<ReviewHold> {
        self.call(Input::CloudHold(session))
    }
    pub(crate) fn spawn(
        files: &PreboundDevice,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
    ) -> Result<Self> {
        Self::spawn_boot(
            Boot {
                directory: files.directory.clone(),
                scope: files.scope.clone(),
                stable: Some((files.note.id, files.binding.id)),
                identity,
                automatic,
                now,
                seed,
            },
            None,
        )
    }
    pub(crate) fn spawn_unbound(
        directory: &std::path::Path,
        scope: &Scope,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
        keys: Arc<dyn runtime::KeyByteSource>,
    ) -> Result<Self> {
        Self::spawn_boot(
            Boot {
                directory: directory.to_owned(),
                scope: scope.clone(),
                stable: None,
                identity,
                automatic,
                now,
                seed,
            },
            Some(keys),
        )
    }
    fn spawn_boot(boot: Boot, keys: Option<Arc<dyn runtime::KeyByteSource>>) -> Result<Self> {
        let (parent, child_socket) = UnixStream::pair()?;
        parent.set_read_timeout(Some(IPC_TIMEOUT))?;
        parent.set_write_timeout(Some(IPC_TIMEOUT))?;
        let fd = child_socket.as_raw_fd();
        let mut command = Command::new(std::env::current_exe()?);
        command
            .args(["--exact", "cloud_child_entry", "--nocapture"])
            .env("MEGURI_CLOUD_CHILD_FD", fd.to_string())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::inherit());
        // Only the selected socket survives exec; all other descriptors retain CLOEXEC.
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        let child = command.spawn()?;
        drop(child_socket);
        let mut driver = Self {
            child: Some(child),
            stream: parent,
            keys,
            queued: None,
            queued_response: None,
            pending_work: None,
            suspended_work: Vec::new(),
            completed_work: Vec::new(),
            paused_work: None,
            fetched_sources: BTreeMap::new(),
            fetched_envelope_fault: None,
            fetched_envelope_fault_observation: None,
            asset_commands: BTreeMap::new(),
        };
        driver.call::<()>(Input::Boot(boot))?;
        Ok(driver)
    }
    fn next_message(&mut self) -> Result<ChildMessage> {
        loop {
            match read_frame(&mut self.stream)? {
                ChildMessage::DrawKey => {
                    let draw = self.keys.as_ref().ok_or_else(failure)?.draw_key();
                    write_frame(&mut self.stream, &Input::KeyResult(draw))?;
                }
                message => return Ok(message),
            }
        }
    }
    pub(crate) fn call<T: DeserializeOwned>(&mut self, input: Input) -> Result<T> {
        let label = match &input {
            Input::Reconcile => "Reconcile",
            Input::Build(..) => "Build",
            Input::Fetched(..) => "Fetched",
            Input::ServiceEpoch(..) => "ServiceEpoch",
            Input::EpochReadability => "EpochReadability",
            Input::DeliverCollection(..) => "DeliverCollection",
            Input::DeliverCollectionAssets(..) => "DeliverCollectionAssets",
            Input::DeliverCollectionDownload(..) => "DeliverCollectionDownload",
            Input::DrainIncoming(..) => "DrainIncoming",
            Input::AssetReadResult(..) => "AssetReadResult",
            Input::MediaWork => "MediaWork",
            Input::MediaQueue(..) => "MediaQueue",
            Input::MediaBuild(..) => "MediaBuild",
            Input::MediaAdmit(..) => "MediaAdmit",
            Input::MediaReply(..) => "MediaReply",
            Input::MediaSent(..) => "MediaSent",
            Input::CollectionDrained => "CollectionDrained",
            Input::AdoptionConfirm(..) => "AdoptionConfirm",
            Input::AdoptionReply(..) => "AdoptionReply",
            Input::AdoptionLocal(..) => "AdoptionLocal",
            Input::AdoptionCompletion(..) => "AdoptionCompletion",
            Input::PublicationConfirm(..) => "PublicationConfirm",
            Input::PublicationNext => "PublicationNext",
            Input::PublicationLocal(..) => "PublicationLocal",
            Input::PublicationExecute(..) => "PublicationExecute",
            Input::PublicationCancel(..) => "PublicationCancel",
            Input::PublicationTrigger => "PublicationTrigger",
            Input::PublicationFacts => "PublicationFacts",
            Input::AuthorityStatus => "AuthorityStatus",
            Input::PublicationControl => "PublicationControl",
            Input::PublicationReplyEvents => "PublicationReplyEvents",
            Input::SelectedControl => "SelectedControl",
            Input::PreviewBegin => "PreviewBegin",
            Input::PreviewActive(..) => "PreviewActive",
            Input::PreviewEpoch(..) => "PreviewEpoch",
            Input::PreviewCancel => "PreviewCancel",
            Input::RegisterSDK(..) => "RegisterSDK",
            Input::RootFetch(..) => "RootFetch",
            Input::RootRead(..) => "RootRead",
            Input::LastAppliedActive => "LastAppliedActive",
            Input::SyncNow(..) => "SyncNow",
            Input::Poll(..) => "Poll",
            Input::FetchLifecycle => "FetchLifecycle",
            Input::FetchValidate(..) => "FetchValidate",
            Input::FetchBegin(..) => "FetchBegin",
            Input::FetchWill(..) => "FetchWill",
            Input::FetchZone(..) => "FetchZone",
            Input::FetchDid(..) => "FetchDid",
            Input::FetchFinish(..) => "FetchFinish",
            Input::FetchFailed(..) => "FetchFailed",
            Input::FetchApply(..) => "FetchApply",
            Input::FetchCompleted(..) => "FetchCompleted",
            Input::Updates(..) => "Updates",
            Input::AppliedUpdate(..) => "AppliedUpdate",
            Input::SendsFinished(..) => "SendsFinished",
            Input::StartCall(..) => "StartCall",
            Input::FinishCall(..) => "FinishCall",
            _ => "Other",
        };
        let result = self.call_inner(input);
        if result.is_err() {
            eprintln!("cloud-child input_failed={label}");
        }
        result
    }
    fn call_inner<T: DeserializeOwned>(&mut self, input: Input) -> Result<T> {
        let key = serde_json::to_vec(&input)?;
        // Reap a released worker before admitting another incoming command.
        // Keep its reply under its original key; queries never advance it.
        if matches!(
            input,
            Input::Fetched(..)
                | Input::FetchApply(..)
                | Input::ServiceEpoch(..)
                | Input::DeliverCollection(..)
                | Input::DeliverCollectionAssets(..)
                | Input::DeliverCollectionDownload(..)
                | Input::AssetReadResult(..)
                | Input::DrainIncoming(..)
                | Input::Reconcile
                | Input::Build(..)
        ) {
            while self.pending_work.is_some() && self.pending_work.as_ref() != Some(&key) && self.paused_work.is_none()
            {
                self.resume_incoming_work()?;
            }
        }
        // Consume an older matching reply before advancing a repeated command.
        if self.pending_work.as_ref() == Some(&key)
            && !self.completed_work.iter().any(|(completed, _)| *completed == key)
        {
            self.resume_incoming_work()?;
        }
        if let Some(index) = self.completed_work.iter().position(|(completed, _)| *completed == key) {
            let (_, result) = self.completed_work.remove(index);
            return Ok(serde_json::from_value(result.map_err(ChildFailure::into_error)?)?);
        }
        if self.suspended_work.iter().any(|(pending, _)| *pending == key) {
            return Err(Box::new(WorkPaused));
        }
        write_frame(&mut self.stream, &input)?;
        match self.next_message()? {
            ChildMessage::Reply(reply) => {
                self.fetched_sources.remove(&key);
                Ok(serde_json::from_value(reply.map_err(ChildFailure::into_error)?)?)
            }
            ChildMessage::Paused(stage) => {
                if let Some(pending) = self.pending_work.take() {
                    self.suspended_work.push((pending, self.paused_work));
                }
                self.pending_work = Some(key);
                self.paused_work = Some(stage);
                Err(Box::new(WorkPaused))
            }
            _ => Err(failure()),
        }
    }
    /// Enter the real submission guard and enqueue its original request over IPC.
    /// The child waits for the actual reply, servicing inspection commands meanwhile.
    pub(crate) fn queued_request(&self) -> Result<super::coordinator::HeldRequest> {
        self.queued.clone().ok_or_else(failure)
    }
    pub(crate) fn enqueue(&mut self, input: Input) -> Result<super::coordinator::HeldRequest> {
        if self.queued.is_some() {
            return Err(failure());
        }
        write_frame(&mut self.stream, &input)?;
        let ChildMessage::Submit(caller, operation) = self.next_message()? else {
            return Err(failure());
        };
        let request = super::coordinator::HeldRequest { caller, operation };
        self.queued = Some(request.clone());
        write_frame(&mut self.stream, &Input::TransportAccepted)?;
        Ok(request)
    }
    pub(crate) fn execute<T: DeserializeOwned>(
        &mut self,
        input: Input,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        write_frame(&mut self.stream, &input)?;
        self.transport_replies(service, scope)
    }
    /// Hold only delivery: the original guarded request has already reached Service.
    pub(crate) fn hold_reply(
        &mut self,
        input: Input,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<()> {
        let request = self.enqueue(input)?;
        self.queued_response = Some(service.request(scope, request.caller, request.operation)?);
        Ok(())
    }
    /// Replace only the result of one already-enqueued reset root read. Retain
    /// its Service request id and deliver it once through the existing resume.
    pub(crate) fn capture_reset_media_root_reply(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
        result: std::result::Result<super::Reply, super::ServiceError>,
    ) -> Result<super::Response> {
        let request = self.queued_request()?;
        if self.queued_response.is_some()
            || request.caller.own_operation.is_none()
            || request.caller.instance.is_some()
            || !matches!(&request.operation, super::Operation::Read { zone, name,
                fields: super::RequestedFields::All } if zone == "meguri-media" && name == "root")
        {
            return Err(failure());
        }
        let original = service.request(scope, request.caller, request.operation)?;
        self.queued_response = Some(super::Response {
            request_id: original.request_id,
            result,
        });
        Ok(original)
    }
    fn transport_replies<T: DeserializeOwned>(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        loop {
            match self.next_message()? {
                ChildMessage::Reply(result) => {
                    return Ok(serde_json::from_value(result.map_err(ChildFailure::into_error)?)?);
                }
                ChildMessage::Submit(caller, operation) => {
                    write_frame(&mut self.stream, &Input::TransportAccepted)?;
                    let response = service.request(scope, caller, operation)?;
                    write_frame(&mut self.stream, &Input::TransportResult(response))?;
                }
                ChildMessage::DrawKey => unreachable!(),
                ChildMessage::Paused(_) => return Err(failure()),
            }
        }
    }
    pub(crate) fn resume<T: DeserializeOwned>(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
    ) -> Result<T> {
        let request = self.queued.take().ok_or_else(failure)?;
        let response = match self.queued_response.take() {
            Some(response) => response,
            None => service.request(scope, request.caller, request.operation)?,
        };
        write_frame(&mut self.stream, &Input::TransportResult(response))?;
        self.transport_replies(service, scope)
    }
    pub fn reconcile_binding(&mut self) -> Result<()> {
        self.call(Input::Reconcile)
    }
    pub fn qualify_without_root(&mut self, instance: String) -> Result<()> {
        self.call(Input::Qualify(instance))
    }
    pub fn publication_reply_events(
        &mut self,
    ) -> Result<(
        Option<core::selected_observation::PublicationEvent>,
        Option<core::selected_observation::PublicationEvent>,
    )> {
        self.call(Input::PublicationReplyEvents)
    }
    pub fn selected_control(&mut self) -> Result<core::selected_observation::SelectedControlObservation> {
        self.call(Input::SelectedControl)
    }
    pub fn stored_runtime_state(&mut self) -> Result<runtime::StoredRuntimeState> {
        self.call(Input::StoredState)
    }
    /// Fixed catalog boundary only; this test-only crate cannot arm arbitrary code.
    pub fn arm_adoption_activation_crash(&mut self) -> Result<()> {
        self.call(Input::ArmAdoptionActivationCrash)
    }
    /// Change only the platform identity input after the last safety check.
    /// None represents sign-out; the real production port must return Unknown.
    pub fn arm_adoption_account_change(&mut self, account: Option<String>) -> Result<core::adoption::AdoptOp> {
        self.call(Input::ArmAdoptionAccountChange(account))
    }
    pub fn adoption_completion(&mut self, op: &core::adoption::AdoptOp) -> Result<AdoptionCompletion> {
        self.call(Input::AdoptionCompletion(op.clone()))
    }
    pub fn set_available_capacity(&mut self, bytes: Option<u64>) -> Result<()> {
        self.call(Input::Capacity(bytes))
    }
    /// One sparse file in the current, durable adoption row's owned candidate.
    /// Returns the actual logical size of the whole candidate after padding.
    pub fn pad_adoption_candidate(&mut self) -> Result<u64> {
        self.call(Input::PadAdoptionCandidate)
    }
    /// Called after the command's EOF. Require a real abort, never a rig hold.
    pub fn reap_adoption_activation_crash(&mut self) -> Result<()> {
        let mut child = self.child.take().ok_or_else(failure)?;
        let status = child.wait()?;
        if status.signal() != Some(libc::SIGABRT) {
            return Err(failure());
        }
        Ok(())
    }
    /// Read actual adapter receipts without driving or qualifying a fetch.
    pub fn fetch_lifecycle_for_testing(&mut self) -> Result<(Vec<core::adapter::FetchDrain>, u64)> {
        self.call(Input::FetchLifecycle)
    }
    pub fn set_account_for_testing(&mut self, account: String) -> Result<()> {
        self.call(Input::Identity(Some(account)))
    }
    pub fn sign_out_for_testing(&mut self) -> Result<()> {
        self.call(Input::SignOut)
    }
    pub fn generated_card_data(&mut self, id: i64, bytes: usize) -> Result<()> {
        self.call(Input::GeneratedCardData(id, bytes))
    }
    /// Hash complete physical rows, normalizing only the two moved references.
    pub fn rekey_rows_proof(&mut self, note: i64, card: i64, original: i64) -> Result<([u8; 32], [u8; 32])> {
        self.call(Input::RekeyRowsProof(note, card, original))
    }
    /// Synthetic deck/config fixture, written through the journalled Local API.
    pub fn rekey_deck_fixture(&mut self, id: i64, create_deck: bool) -> Result<()> {
        self.call(Input::RekeyDeckFixture(id, create_deck))
    }
    pub fn rekey_config_value(&mut self, key: String, value: i64) -> Result<CaptureProof> {
        self.call(Input::RekeyConfigValue(key, value))
    }
    /// Build and hand off an ordinary save, send it to Service, then lose its reply.
    pub fn lose_named_save_reply(
        &mut self,
        service: &mut super::Service<meguri_model::rng::SeededRandom>,
        scope: &Scope,
        name: &str,
    ) -> Result<super::Record> {
        let sdk: String = self.call(Input::AssetSDK)?;
        let batch: BuiltBatch = self.call(Input::Build(sdk, BTreeSet::from([name.to_owned()])))?;
        let save = batch.saves.iter().find(|save| save.name == name).ok_or_else(failure)?;
        if save.asset_path.is_some() {
            return Err(failure());
        }
        let zone: String = self.call(Input::CollectionZone)?;
        let response = service.request(
            scope,
            Default::default(),
            super::Operation::Modify {
                zone,
                atomic: false,
                changes: vec![super::Mutation::Save {
                    record: super::RecordInput::encrypted(name, "MGEntity", save.envelope.clone()),
                    if_unchanged: save.system_fields.clone().map(super::SystemFields::from_bytes),
                }],
            },
        )?;
        let Ok(super::Reply::Modified(items)) = response.result else {
            return Err(failure());
        };
        let Some(item) = items.into_iter().next() else {
            return Err(failure());
        };
        let Ok(super::MutationSuccess::Saved(record)) = item.result else {
            return Err(failure());
        };
        Ok(record)
    }
    /// Journal a real Card data edit through the normal host writer.
    pub fn write_card_data(&mut self, id: i64, data: String) -> Result<()> {
        self.call(Input::WriteCardData(id, data))
    }
    pub fn reopen_for_testing(&mut self) -> Result<String> {
        self.call(Input::Reopen)
    }
    pub fn build_for_instance(&mut self, instance: String, names: BTreeSet<String>) -> Result<BuiltBatch> {
        self.call(Input::Build(instance, names))
    }
    /// Consume the retained builder reply without starting a build or transport.
    pub fn complete_pending_build(&mut self) -> Result<BuiltBatch> {
        let key = self.pending_work.clone().ok_or_else(failure)?;
        if !matches!(serde_json::from_slice::<Input>(&key)?, Input::Build(..)) {
            return Err(failure());
        }
        self.resume_incoming_work()?;
        let index = self
            .completed_work
            .iter()
            .position(|(completed, _)| *completed == key)
            .ok_or_else(failure)?;
        let (_, result) = self.completed_work.remove(index);
        Ok(serde_json::from_value(result.map_err(ChildFailure::into_error)?)?)
    }
    pub fn accept_root_for_instance(
        &mut self,
        instance: String,
        id: u64,
        envelope: Vec<u8>,
        fields: Vec<u8>,
    ) -> Result<bool> {
        self.call(Input::RootRead(instance, id, envelope, fields, false))
    }
    /// The child allocator must be installed by the integration test binary.
    pub fn payload_memory_reset(&mut self) -> Result<usize> {
        self.call(Input::PayloadMemoryReset)
    }
    pub fn payload_memory_peak(&mut self) -> Result<(usize, usize)> {
        self.call(Input::PayloadMemoryPeak)
    }
    pub fn payload_memory_stages(&mut self) -> Result<[Option<PayloadMemoryStageSamples>; 6]> {
        self.call(Input::PayloadMemoryStages)
    }
    pub fn payload_record_limit(&mut self, limit: usize) -> Result<()> {
        self.call(Input::PayloadRecordLimit(limit))
    }
    pub fn edit_generated_payload(&mut self, id: i64, bytes: usize, seed: u64) -> Result<PayloadProof> {
        self.call(Input::EditGeneratedPayload(id, bytes, seed))
    }
    pub fn payload_proof(&mut self, name: &str) -> Result<PayloadProof> {
        self.call(Input::PayloadProof(name.into()))
    }
    pub fn last_capture_body_reads(&mut self) -> Result<usize> {
        self.call(Input::BodyReads)
    }
    pub fn edit_note_fields(&mut self, id: i64, fields: &[&str]) -> Result<Edit> {
        self.call(Input::Edit(id, fields.iter().map(|s| (*s).to_owned()).collect()))
    }
    pub fn arm_capture_failure_for_testing(&mut self, after_writes: usize) -> Result<()> {
        self.call(Input::ArmCaptureFailure(after_writes))
    }
    pub fn capture_proof(&mut self, seq: u64, generation: u64) -> Result<CaptureProof> {
        self.call(Input::Capture(seq, generation))
    }
    pub fn wal_proof(&mut self, seq: u64) -> Result<Receipt> {
        self.call(Input::Wal(seq))
    }
    pub fn scan_state(&mut self) -> Result<ScanState> {
        self.call(Input::ScanState)
    }
    pub fn scan_proof(&mut self) -> Result<ScanProof> {
        self.call(Input::ScanProof)
    }
    pub fn review_begin(&mut self, note: i64) -> Result<u64> {
        self.call(Input::Begin(note))
    }
    pub fn review_start(&mut self, session: u64) -> Result<codec::Card> {
        self.call(Input::Start(session))
    }
    pub fn review_show_answer(&mut self, s: u64, c: i64) -> Result<()> {
        self.call(Input::Show(s, c))
    }
    pub fn review_answer_good(&mut self, s: u64, c: i64) -> Result<ReviewAnswer> {
        self.call(Input::Good(s, c))
    }
    pub fn review_end_and_drain(&mut self, s: u64) -> Result<()> {
        self.call(Input::End(s))
    }
    pub fn review_active_session(&mut self) -> Result<Option<u64>> {
        self.call(Input::ActiveReviewSession)
    }
    pub fn read_review_hold(&mut self, s: u64) -> Result<ReviewHold> {
        self.call(Input::Hold(s))
    }
    pub fn read_card(&mut self, id: i64) -> Result<codec::Card> {
        self.call(Input::Card(id))
    }
    pub fn read_revlog(&mut self, id: i64) -> Result<codec::Revlog> {
        self.call(Input::Revlog(id))
    }
    pub fn collection_payloads(&mut self) -> Result<BTreeMap<String, codec::Payload>> {
        self.call(Input::Payloads)
    }
    pub(crate) fn kill_sigkill(&mut self) -> Result<Killed> {
        let mut child = self.child.take().ok_or_else(failure)?;
        child.kill()?;
        let status = child.wait()?;
        Ok(Killed {
            signal: status.signal(),
            clean_close: false,
        })
    }
    pub(crate) fn close(&mut self) -> Result<()> {
        if self.child.is_none() {
            return Ok(());
        }
        if self.pending_work.is_some() {
            for stage in [
                IncomingStage::AssetReadPrepared,
                IncomingStage::AssetReadFinishing,
                IncomingStage::FetchBeforeInbox,
                IncomingStage::FetchAfterInbox,
                IncomingStage::FetchAfterCursor,
                IncomingStage::ApplyAfterIntent,
                IncomingStage::ApplyAfterCommit,
                IncomingStage::ApplyAfterFlush,
                IncomingStage::IncomingRollbackComplete,
                IncomingStage::BeforeCollectionSave,
                IncomingStage::IntentSettlementComplete,
                IncomingStage::ResolutionBeforeMove,
                IncomingStage::ResolutionMoveBeforeRewrite,
                IncomingStage::ResolutionRewriteCommitted,
                IncomingStage::MediaTrashBeforeInstall,
                IncomingStage::MediaReplyBeforeApply,
                IncomingStage::MediaReplyBeforeTracking,
                IncomingStage::MediaSettlementBeforeCommit,
            ] {
                self.release_incoming_stage(stage)?;
            }
            // Consume the original worker reply before the independent Close
            // reply; a previously failed operation does not skip host cleanup.
            while self.pending_work.is_some() {
                self.resume_incoming_work()?;
            }
        }
        self.call::<()>(Input::Close)?;
        let mut child = self.child.take().ok_or_else(failure)?;
        let pid = child.id();
        struct Cleanup(u32, bool);
        impl Drop for Cleanup {
            fn drop(&mut self) {
                if self.1 {
                    unsafe {
                        libc::kill(self.0 as i32, libc::SIGKILL);
                    }
                }
            }
        }
        let mut cleanup = Cleanup(pid, true);
        let (tx, rx) = std::sync::mpsc::channel();
        let reaper = std::thread::spawn(move || {
            let result = child.wait();
            let _ = tx.send(result);
        });
        let status = match rx.recv_timeout(IPC_TIMEOUT) {
            Ok(status) => status?,
            Err(_) => {
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                let _ = rx.recv_timeout(IPC_TIMEOUT);
                reaper.join().map_err(|_| failure())?;
                return Err(failure());
            }
        };
        cleanup.1 = false;
        reaper.join().map_err(|_| failure())?;
        if !status.success() {
            return Err(failure());
        }
        Ok(())
    }
}
impl Drop for ChildDriver {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}
struct Account(Mutex<IdentityStatus>);
#[derive(Default)]
struct MutableSpace(Mutex<Option<u64>>);
impl SpaceProvider for MutableSpace {
    fn available_bytes(&self, directory: &std::path::Path) -> std::result::Result<u64, meguri_storage::StorageError> {
        match *self.0.lock().unwrap() {
            Some(bytes) => Ok(bytes),
            None => meguri_storage::space::StatvfsSpace.available_bytes(directory),
        }
    }
}
static ADOPTION_IDENTITY_CHANGES: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &core::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
struct Device {
    host: meguri_host::CoreHost,
    capacity: Arc<MutableSpace>,
    options: meguri_host::HostConfig,
    automatic: bool,
    account: Arc<Account>,
    clock: Arc<StepClock>,
    stable: Option<(i64, core::BindingId)>,
    deletion: Option<core::publication::ZoneDeletionGuard>,
    zone_listing: Option<core::runtime::ZoneListingRequest>,
    session_started: BTreeMap<u64, i64>,
    closed: bool,
    incoming_worker: Option<incoming_driver::Work>,
    incoming_latches: Arc<incoming_driver::Latches>,
    asset_conflict_body_reads: usize,
    asset_read_queue: Arc<Mutex<Vec<core::incoming::AssetReadRequest>>>,
    incoming_events: meguri_engine::changes::stream::Subscription,
    change_events: Vec<u64>,
    rollovers: Vec<RolloverObservation>,
    last_fetch_engine_state: Arc<Mutex<Option<Vec<u8>>>>,
    incoming_submission: Arc<Mutex<Option<super::coordinator::HeldRequest>>>,
    publication_reply_events: (
        Option<core::selected_observation::PublicationEvent>,
        Option<core::selected_observation::PublicationEvent>,
    ),
}
impl Device {
    fn new(b: Boot, stream: &UnixStream) -> Result<Self> {
        let clock = Arc::new(StepClock::new(b.now, UtcOffset::UTC, "UTC"));
        let account = Arc::new(Account(Mutex::new(match b.identity {
            IdentityReads::Available(a) => IdentityStatus::Available(core::OpaqueBytes(a.into_bytes())),
            IdentityReads::FailAllSession => IdentityStatus::ReadFailed,
            IdentityReads::TemporarilyUnavailable => IdentityStatus::TemporarilyUnavailable,
        })));
        let mut options = devices::options(&b.directory, b.now, b.seed);
        let capacity = Arc::new(MutableSpace::default());
        options.disk_space_provider = capacity.clone();
        options.clock = clock.clone();
        options.cloud = Some(runtime::CloudInputs {
            scope: devices::build_scope(&b.scope)?,
            identity: account.clone(),
            files: Arc::new(devices::NativeFiles),
        });
        if b.stable.is_none() {
            options.cloud_key_bytes = Some(Arc::new(ChildKeys(Mutex::new(stream.try_clone()?))));
        }
        let host = meguri_host::CoreHost::open(options.clone())?;
        let incoming_events = host.engine().stream().subscribe();
        let incoming_latches = Arc::new(incoming_driver::Latches::default());
        if let Some(runtime) = host.cloud_runtime() {
            let latches = incoming_latches.clone();
            runtime.set_incoming_stage_hook_for_testing(move |stage| latches.hit(stage));
        }
        Ok(Self {
            host,
            capacity,
            options,
            automatic: b.automatic,
            account,
            clock,
            stable: b.stable,
            deletion: None,
            zone_listing: None,
            session_started: BTreeMap::new(),
            closed: false,
            incoming_worker: None,
            incoming_latches,
            asset_conflict_body_reads: 0,
            asset_read_queue: Arc::new(Mutex::new(Vec::new())),
            incoming_events,
            change_events: Vec::new(),
            rollovers: Vec::new(),
            last_fetch_engine_state: Arc::new(Mutex::new(None)),
            incoming_submission: Arc::new(Mutex::new(None)),
            publication_reply_events: (None, None),
        })
    }
    fn runtime(&self) -> Result<&runtime::Runtime> {
        self.host.cloud_runtime().map(|r| r.as_ref()).ok_or_else(failure)
    }
    fn all_facts(&self) -> Result<Vec<EntityFacts>> {
        let mut out = Vec::new();
        let mut total = 0;
        for kind in core::row_payload::KINDS {
            let mut after = None;
            loop {
                let page = self.runtime()?.entity_facts_page(kind, after.as_deref())?;
                if page.is_empty() {
                    break;
                }
                for e in page {
                    after = Some(e.key.clone());
                    total += e.payload.encode()?.len();
                    if out.len() >= 2048 || total > 8 * 1024 * 1024 {
                        return Err(failure());
                    }
                    out.push(e);
                }
            }
        }
        Ok(out)
    }
    fn payloads(&self) -> Result<BTreeMap<String, codec::Payload>> {
        let r = self.runtime()?;
        let mut layouts = BTreeMap::new();
        for e in self.all_facts()? {
            if let Some(codec::Payload::Notetype(n)) = e.acked_payload {
                layouts.insert(n.id, n);
            }
        }
        let binding = r.status()?.binding.ok_or_else(failure)?;
        let c = self.host.collection()?;
        let lease = c.reader(ReaderRole::Query)?;
        let snapshot = lease.snapshot()?;
        devices::collect(&*snapshot, &binding.facts.record_key.0, &layouts)
    }
    fn card(&self, id: i64) -> Result<codec::Card> {
        self.payloads()?
            .into_values()
            .find_map(|p| match p {
                codec::Payload::Card(c) if c.id == id => Some(c),
                _ => None,
            })
            .ok_or_else(failure)
    }
    fn revlog(&self, id: i64) -> Result<codec::Revlog> {
        self.payloads()?
            .into_values()
            .find_map(|p| match p {
                codec::Payload::Revlog(c) if c.id == id => Some(c),
                _ => None,
            })
            .ok_or_else(failure)
    }
    fn receipt(&self) -> Result<Receipt> {
        let c = self.host.collection()?;
        let (identity, seq, generation, native) = {
            let lease = c.reader(ReaderRole::Cloud)?;
            let snap = lease.snapshot()?;
            (
                snap.wal_identity(),
                snap.seq(),
                snap.generation(),
                runtime::FileIdentityProvider::identity(&devices::NativeFiles, c.path())?,
            )
        };
        match c.wait_wal_covered(identity, seq, IPC_TIMEOUT)? {
            WalCoverage::Covered(r) if r.identity() == identity && r.through() >= seq => Ok(Receipt {
                through: r.through().0,
                generation: generation.0,
                native_file: native,
            }),
            _ => Err(failure()),
        }
    }
    fn facts(&self) -> Result<devices::DeviceFacts> {
        let r = self.runtime()?;
        let status = r.status()?;
        let binding = status.binding.ok_or_else(failure)?;
        let (id, stable_binding) = self.stable.ok_or_else(failure)?;
        let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Note, &codec::id_key(id))?;
        let e = r.entity_facts(&name)?.ok_or_else(failure)?;
        let payloads = self.payloads()?;
        let Some(codec::Payload::Note(note)) = payloads.get(&name) else {
            return Err(failure());
        };
        let pending_names = self
            .all_facts()?
            .into_iter()
            .filter(|e| e.mark.is_some())
            .map(|e| e.name)
            .collect();
        Ok(devices::DeviceFacts {
            note: note.clone(),
            local_rev: e.local_rev,
            change_counter: status.change_counter,
            last_observed_digest: Some(e.payload.row_digest()?),
            mark: e.mark.map(|m| devices::DirtyMark {
                binding: stable_binding,
                source: m.source,
                change_seq: m.change_seq,
                payload_digest: m.payload_digest,
            }),
            pending_names,
            binding_state: binding.facts.state,
            scan_drained: status.scan_drained,
            sidecar_clean: false,
        })
    }
    fn payload_proof(&self, name: &str) -> Result<PayloadProof> {
        self.runtime()?.payload_proof_for_testing(name)?.ok_or_else(failure)
    }
    fn handle(&mut self, input: Input) -> Result<serde_json::Value> {
        fn value<T: Serialize>(v: T) -> Result<serde_json::Value> {
            Ok(serde_json::to_value(v)?)
        }
        match input {
            Input::Capacity(bytes) => {
                *self.capacity.0.lock().unwrap() = bytes;
                value(())
            }
            Input::PublicationControl => value(self.runtime()?.publication_control_for_testing()?),
            Input::PublicationReplyEvents => value(self.publication_reply_events.clone()),
            Input::SelectedControl => value(self.runtime()?.selected_control_for_testing(&self.host.collection()?)?),
            Input::PayloadMemoryReset => {
                use crate::bench::peak_alloc::PeakAlloc;
                if !PeakAlloc::is_installed() {
                    return Err(failure());
                }
                PeakAlloc::reset_peak();
                self.incoming_latches.reset_payload_memory();
                value(PeakAlloc::current())
            }
            Input::PayloadMemoryPeak => value((
                crate::bench::peak_alloc::PeakAlloc::peak(),
                crate::bench::peak_alloc::PeakAlloc::current(),
            )),
            Input::PayloadMemoryStages => value(self.incoming_latches.payload_memory_stages()),
            Input::PayloadRecordLimit(limit) => {
                self.runtime()?.set_collection_record_limit_for_testing(limit)?;
                value(())
            }
            Input::PayloadProof(name) => value(self.payload_proof(&name)?),
            Input::EditGeneratedPayload(id, bytes, seed) => {
                use meguri_model::rng::RandomSource;
                let rng = meguri_model::rng::SeededRandom::new(seed);
                let mut back = String::with_capacity(bytes);
                while back.len() < bytes {
                    for byte in rng.next_u64().to_le_bytes() {
                        if back.len() == bytes {
                            break;
                        }
                        back.push((b'a' + byte % 26) as char);
                    }
                }
                let c = self.host.collection()?;
                let result = meguri_engine::notes::update(
                    &c,
                    meguri_engine::notes::UpdateNote {
                        id: NoteId(id),
                        fields: vec!["Synthetic front".into(), back],
                        tags: Vec::new(),
                    },
                    OpSource::UserUndoable,
                )?;
                let proof = self
                    .runtime()?
                    .capture_proof(result.seq, result.generation)
                    .ok_or_else(failure)?;
                if !proof.persisted {
                    return Err(failure());
                }
                let binding = self.runtime()?.status()?.binding.ok_or_else(failure)?;
                let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Note, &codec::id_key(id))?;
                value(self.payload_proof(&name)?)
            }
            Input::PeekUndoReason => value(
                self.host
                    .collection()?
                    .peek_undo(meguri_engine::undo::UndoScope::Global)?
                    .reason()
                    .map(|reason| reason.to_string()),
            ),
            Input::RemoveRevlog(id) => {
                let result = self.host.collection()?.run(
                    meguri_engine::OpSpec::named("fixture remove review history", OpSource::LocalApi),
                    move |ctx| {
                        ctx.scope().remove_revlog(meguri_model::ids::RevlogId(id))?;
                        Ok(())
                    },
                )?;
                value((result.seq.0, result.generation.0))
            }
            Input::Undo => {
                self.host.review().restore_controlled(
                    meguri_engine::undo::UndoScope::Global,
                    false,
                    meguri_engine::CancelToken::new(),
                    meguri_engine::ProgressSink::new(),
                )?;
                value(())
            }
            Input::AddNote(mid, did, fields) => {
                let c = self.host.collection()?;
                let added = meguri_engine::notes::add(
                    &c,
                    meguri_engine::notes::AddNote {
                        notetype_id: meguri_model::ids::NotetypeId(mid),
                        deck_id: DeckId(did),
                        fields,
                        tags: Vec::new(),
                        guid: None,
                        update_add_defaults: false,
                    },
                    OpSource::UserUndoable,
                )?;
                let rows = self.collection_rows()?;
                let note = rows
                    .iter()
                    .find_map(|p| match p {
                        codec::Payload::Note(n) if n.id == added.value.id.0 => Some(n.clone()),
                        _ => None,
                    })
                    .ok_or_else(failure)?;
                let cards = rows
                    .into_iter()
                    .filter_map(|p| match p {
                        codec::Payload::Card(c) if added.value.cards.iter().any(|id| id.0 == c.id) => Some(c),
                        _ => None,
                    })
                    .collect();
                value(AddedNote {
                    note,
                    cards,
                    seq: added.seq.0,
                    generation: added.generation.0,
                    native_file: runtime::FileIdentityProvider::identity(&devices::NativeFiles, c.path())?,
                })
            }
            Input::DeleteNote(id) => {
                meguri_engine::notes::delete(&self.host.collection()?, vec![NoteId(id)], OpSource::UserUndoable)?;
                value(())
            }
            Input::GeneratedCardData(id, bytes) => {
                let data = format!("{{\"fixture\":\"{}\"}}", "r".repeat(bytes));
                self.host.collection()?.run(
                    meguri_engine::OpSpec::named("synthetic large card", OpSource::UserUndoable),
                    move |ctx| {
                        ctx.scope()
                            .put_card_data(CardId(id), &meguri_model::card_data::CardData::parse(&data))?;
                        Ok(())
                    },
                )?;
                value(())
            }
            Input::RekeyRowsProof(note, card, original) => {
                use meguri_storage::Cell;
                use sha2::Digest;
                fn hash(row: Vec<Cell>) -> [u8; 32] {
                    let mut h = Sha256::new();
                    for cell in row {
                        match cell {
                            Cell::Null => h.update([0]),
                            Cell::Integer(v) => {
                                h.update([1]);
                                h.update(v.to_be_bytes());
                            }
                            Cell::Real(v) => {
                                h.update([2]);
                                h.update(v.to_bits().to_be_bytes());
                            }
                            Cell::Text(v) => {
                                h.update([3]);
                                h.update((v.len() as u64).to_be_bytes());
                                h.update(v);
                            }
                            Cell::Blob(v) => {
                                h.update([4]);
                                h.update((v.len() as u64).to_be_bytes());
                                h.update(v);
                            }
                        }
                    }
                    h.finalize().into()
                }
                let c = self.host.collection()?;
                let lease = c.reader(meguri_storage::ReaderRole::Cloud)?;
                let snapshot = lease.snapshot()?;
                let note_hash = {
                    let mut row = snapshot
                        .row_image(Table::Notes, &RowKey::Id(note))?
                        .ok_or_else(failure)?;
                    row[0] = Cell::Integer(original);
                    hash(row)
                };
                let mut row = snapshot
                    .row_image(Table::Cards, &RowKey::Id(card))?
                    .ok_or_else(failure)?;
                if row[1] != Cell::Integer(note) {
                    return Err(failure());
                }
                row[1] = Cell::Integer(original);
                value((note_hash, hash(row)))
            }
            Input::RekeyConfigValue(key, number) => {
                let result = self.host.collection()?.run(
                    meguri_engine::OpSpec::named("synthetic config edit", OpSource::LocalApi),
                    move |ctx| {
                        ctx.scope()
                            .set_config(&key, &meguri_storage::ConfigValue::from(number))?;
                        Ok(())
                    },
                )?;
                let proof = self
                    .runtime()?
                    .capture_proof(result.seq, result.generation)
                    .ok_or_else(failure)?;
                value(CaptureProof {
                    observed_seq: proof.observed_seq.0,
                    generation: proof.generation.0,
                    attempted: proof.attempted,
                    persisted: proof.persisted,
                })
            }
            Input::RekeyDeckFixture(id, create_deck) => {
                self.host.collection()?.run(
                    meguri_engine::OpSpec::named("synthetic rekey deck", OpSource::LocalApi),
                    move |ctx| {
                        let scope = ctx.scope();
                        scope.set_config(&format!("_deck_{id}_fixture"), &meguri_storage::ConfigValue::from(id))?;
                        if create_deck {
                            let mut deck = scope.deck(DeckId(1))?.ok_or(meguri_engine::EngineError::Internal {
                                message: "fixture deck missing".into(),
                            })?;
                            deck.id = DeckId(id);
                            deck.name = "Synthetic collision deck".into();
                            scope.put(&deck)?;
                        }
                        Ok(())
                    },
                )?;
                value(())
            }
            Input::WriteCardData(id, data) => {
                self.host.collection()?.run(
                    meguri_engine::OpSpec::named("fixture card data edit", OpSource::UserUndoable),
                    move |ctx| {
                        ctx.scope()
                            .put_card_data(CardId(id), &meguri_model::card_data::CardData::parse(&data))?;
                        Ok(())
                    },
                )?;
                value(())
            }
            Input::BuryCard(id) => {
                meguri_engine::sched::ops::run(
                    &self.host.collection()?,
                    meguri_engine::sched::ops::Request::Bury(meguri_engine::sched::ops::Targets::Cards(vec![CardId(
                        id,
                    )])),
                    &meguri_engine::tags::RunOptions::default(),
                )?;
                value(())
            }
            Input::IncomingObservation(binding) => value(incoming_driver::observe(self, binding)?),
            Input::IncomingSubmission => value(self.incoming_submission.lock().unwrap().clone().ok_or_else(failure)?),
            Input::InterruptIncoming(interruption) => {
                self.runtime()?.interrupt_next_incoming_for_testing(interruption);
                value(())
            }
            Input::TrackingFault(name, fault) => {
                core::testing::arm_tracking_fault(&name, fault)?;
                value(())
            }
            Input::LatchHold(stage) => {
                self.incoming_latches.hold(stage);
                value(())
            }
            Input::LatchRelease(stage) => {
                self.incoming_latches.release(stage);
                value(())
            }
            Input::ContinueWork => Err(failure()),
            Input::ArmAdoptionAccountChange(account) => {
                let op = self.runtime()?.adoption_op()?.ok_or_else(failure)?;
                if !matches!(&*self.account.0.lock().unwrap(), IdentityStatus::Available(current) if current == &op.account)
                {
                    return Err(failure());
                }
                let next = match account {
                    Some(account) => IdentityStatus::Available(core::OpaqueBytes(account.into_bytes())),
                    None => IdentityStatus::SignedOut,
                };
                let account = self.account.clone();
                ADOPTION_IDENTITY_CHANGES.store(0, std::sync::atomic::Ordering::SeqCst);
                self.runtime()?.set_current_account_hook_for_test(move || {
                    *account.0.lock().unwrap() = next;
                    ADOPTION_IDENTITY_CHANGES.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                });
                value(op)
            }
            Input::AdoptionCompletion(op) => {
                let status = self.runtime()?.status()?;
                let collection = self.host.collection()?;
                let file = runtime::FileIdentityProvider::identity(&devices::NativeFiles, collection.path())?;
                let proof_matches = collection.cloud_adopt_activation()?.is_some_and(|proof| {
                    let operation = meguri_engine::device_state::PublicationId::from_bytes(op.id);
                    let matches = match (op.file_before, &op.before) {
                        (Some(_), Some(before)) => proof.matches(operation, &before.publication, &op.candidate),
                        (None, before) => proof.matches_initial_prior(
                            operation,
                            before.as_ref().map(|b| &b.publication),
                            &op.candidate,
                        ),
                        _ => false,
                    };
                    matches && proof.native_file() == (file.device, file.inode)
                });
                let slot_available = self
                    .host
                    .engine()
                    .acquire_slot(meguri_engine::LongOp::CloudAdopt)
                    .is_ok();
                let replacement_recorded = match &op.before {
                    Some(before) => meguri_engine::device_state::DeviceState::for_collection(collection.path())
                        .replacement_evidence_for(before.publication.identity)?
                        .is_some_and(|evidence| evidence.candidate == op.candidate),
                    None => false,
                };
                value(AdoptionCompletion {
                    identity_changes: ADOPTION_IDENTITY_CHANGES.load(std::sync::atomic::Ordering::SeqCst),
                    slot_available,
                    stored_usable: status.stored_usable,
                    identity_verified: status.identity_verified,
                    proof_matches,
                    replacement_recorded,
                })
            }
            Input::ArmAdoptionActivationCrash => {
                self.runtime()?.arm_adoption_activation_crash_for_testing();
                value(())
            }
            Input::Boot(_)
            | Input::KeyResult(_)
            | Input::TransportAccepted
            | Input::TransportResult(_)
            | Input::PublicationExecute(_)
            | Input::DeleteZone(_) => Err(failure()),
            Input::Validate(i) => {
                self.runtime()?.validate_caller(&i)?;
                value(())
            }
            Input::SignOut => {
                *self.account.0.lock().unwrap() = IdentityStatus::SignedOut;
                self.runtime()?.refresh_identity()?;
                value(())
            }
            Input::Reopen => {
                self.host.close()?;
                self.host = meguri_host::CoreHost::open(self.options.clone())?;
                self.incoming_events = self.host.engine().stream().subscribe();
                let latches = self.incoming_latches.clone();
                self.runtime()?
                    .set_incoming_stage_hook_for_testing(move |stage| latches.hit(stage));
                self.runtime()?.refresh_identity()?;
                value(self.runtime()?.instance()?)
            }
            Input::StoredState => value(self.runtime()?.stored_state_for_testing()?),
            Input::BodyReads => value(self.runtime()?.last_capture_body_reads_for_testing()),
            Input::Instance => value(self.runtime()?.instance()?),
            Input::Identity(account) => {
                if let Some(a) = account {
                    *self.account.0.lock().unwrap() = IdentityStatus::Available(core::OpaqueBytes(a.into_bytes()));
                }
                self.runtime()?.refresh_identity()?;
                value(())
            }
            Input::Reconcile => {
                let c = self.host.collection()?;
                let media = self.host.media()?;
                if self.stable.is_none() {
                    media.store().register_full(|_, _| true).map_err(|_| failure())?;
                }
                self.runtime()?.reconcile_binding(&c, &media, IPC_TIMEOUT)?;
                value(())
            }
            Input::StartSDK(i) => {
                self.runtime()?.set_automatic(&i, self.automatic)?;
                self.runtime()?.start_sdk_instance(&i)?;
                value(())
            }
            Input::Facts => value(self.facts()?),
            Input::EpochReadability => {
                let collection = self.host.collection()?;
                let generation = {
                    let lease = collection.reader(ReaderRole::Cloud)?;
                    let snapshot = lease.snapshot()?;
                    snapshot.generation().0
                };
                let runtime = self.runtime()?;
                let projection = runtime.policy_projection(core::policy::Lifecycle::Ready, generation)?;
                value((
                    runtime.root_unreadable_for_testing()?,
                    projection.facts.summary.attention.get(&core::policy::Attention::RootUnreadable).copied().unwrap_or(0),
                    projection.facts.summary.crt_known,
                    projection.root_confirmed,
                ))
            }
            Input::LastAppliedActive => {
                let snapshot = self.runtime()?.adapter_snapshot()?;
                let content: serde_json::Value = serde_json::from_slice(&snapshot.tracking_content)?;
                // Snapshot tuple member 4 is persisted adapter::Durable, not
                // snapshot.root (the current-open root acknowledgement).
                let tuple = content.as_array().filter(|v| v.len() == 6).ok_or_else(failure)?;
                let active = tuple[4]
                    .as_object()
                    .and_then(|durable| durable.get("active"))
                    .ok_or_else(failure)?;
                let active: Option<Saved> = serde_json::from_value(active.clone())?;
                value(active)
            }
            Input::ScanState => {
                let s = self.runtime()?.status()?;
                value(ScanState {
                    scan_required: s.scan_required,
                    scan_running: s.scan_running,
                    scan_completed_this_open: s.scan_completed_this_open,
                    tracking_stopped: s.tracking_stopped,
                })
            }
            Input::ScanProof => {
                let p = self.runtime()?.note_scan_proof().ok_or_else(failure)?;
                value(ScanProof {
                    snapshot_seq: p.snapshot_seq.0,
                    snapshot_generation: p.snapshot_generation.0,
                    snapshot_native_file: p.snapshot_native_file,
                    receipt: Receipt {
                        through: p.receipt.through().0,
                        generation: p.snapshot_generation.0,
                        native_file: p.snapshot_native_file,
                    },
                    recovered_names: p.recovered_names,
                })
            }
            Input::Capture(seq, generation) => {
                let p = self
                    .runtime()?
                    .capture_proof(Seq(seq), Generation(generation))
                    .ok_or_else(failure)?;
                value(CaptureProof {
                    observed_seq: p.observed_seq.0,
                    generation: p.generation.0,
                    attempted: p.attempted,
                    persisted: p.persisted,
                })
            }
            Input::Wal(seq) => {
                let r = self.receipt()?;
                if r.through < seq {
                    return Err(failure());
                }
                value(r)
            }
            Input::ArmCaptureFailure(after_writes) => {
                self.runtime()?.arm_capture_failure_for_testing(after_writes)?;
                value(())
            }
            Input::ArmLoss(id) => {
                self.runtime()?.arm_capture_loss_for_testing(id)?;
                value(())
            }
            Input::Edit(id, fields) => {
                let c = self.host.collection()?;
                let tags = {
                    let lease = c.reader(ReaderRole::Query)?;
                    lease.note(NoteId(id))?.ok_or_else(failure)?.tags
                };
                let result = meguri_engine::notes::update(
                    &c,
                    meguri_engine::notes::UpdateNote {
                        id: NoteId(id),
                        fields,
                        tags: tags.split_whitespace().map(str::to_owned).collect(),
                    },
                    OpSource::UserUndoable,
                )?;
                let note = self
                    .collection_rows()?
                    .into_iter()
                    .find_map(|p| match p {
                        codec::Payload::Note(n) if n.id == id => Some(n),
                        _ => None,
                    })
                    .ok_or_else(failure)?;
                value(Edit {
                    note,
                    seq: result.seq.0,
                    generation: result.generation.0,
                    native_file: runtime::FileIdentityProvider::identity(&devices::NativeFiles, c.path())?,
                })
            }
            Input::Payloads => value(self.payloads()?),
            Input::CheckObservation => value(super::checks::observe_host(&self.host)?),
            Input::Entity(n) => value(self.runtime()?.entity_facts(&n)?.ok_or_else(failure)?),
            Input::AllEntities => value(self.all_facts()?),
            Input::Poll(i) => {
                self.runtime()?.poll_review(&i)?;
                value(())
            }
            Input::Trace => value(self.runtime()?.trace()),
            Input::Observations => value(self.runtime()?.observations()),
            Input::Emitted => value(self.runtime()?.emitted_update_ids()),
            Input::MediaBlobs => value(self.runtime()?.media_blobs_for_testing()?),
            Input::MediaWork => {
                let runtime = self.runtime()?;
                if let Some(sdk) = runtime.incoming_sdk_instance_for_testing() {
                    runtime.prepare_cloud_media_sdk(&sdk)?;
                }
                value(self.host.next_cloud_media_work()?)
            }
            Input::MediaQueue(work) => {
                let runtime = self.runtime()?;
                let Some(sdk) = runtime.incoming_sdk_instance_for_testing() else {
                    return value(false);
                };
                runtime.queue_cloud_media_upload(&sdk, &self.host.collection()?, &work)?;
                value(true)
            }
            Input::MediaBuild(requested) => {
                let runtime = self.runtime()?;
                let Some(sdk) = runtime.incoming_sdk_instance_for_testing() else {
                    return value(None::<(u64, core::media::Work)>);
                };
                value(runtime.build_cloud_media_batch(&sdk, &self.host.collection()?, &requested)?)
            }
            Input::MediaAdmit(id) => {
                let runtime = self.runtime()?;
                value(runtime.validate_cloud_media_batch(
                    &runtime.incoming_sdk_instance_for_testing().ok_or_else(failure)?,
                    &self.host.collection()?,
                    id,
                )?)
            }
            Input::MediaSent(sdk, id, saved, blob_conflicts, failed, deleted, failed_delete, retrying) => {
                if !self.runtime()?.cloud_media_sdk_sent(
                    &sdk,
                    &self.host.collection()?,
                    id,
                    &saved,
                    &blob_conflicts,
                    &failed,
                    &deleted,
                    &failed_delete,
                    &retrying,
                )? {
                    return Err(failure());
                }
                value(())
            }
            Input::MediaAdapterPause(paused) => {
                self.runtime()?.adapter_pause(paused)?;
                value(())
            }
            Input::MediaTrash(name) => {
                let media = self.host.media()?;
                media.store().trash(&[name]).map_err(|_| failure())?;
                self.runtime()?.consume_media_feed(&self.host.collection()?, &media)?;
                value(())
            }
            Input::MediaConflicts => {
                let copies = self.runtime()?.media_conflicts_for_testing()?;
                value(copies)
            }
            Input::MediaResume => {
                self.runtime()?.resume_cloud_media();
                value(())
            }
            Input::MediaReply(work, reply) => {
                self.host.cloud_media_reply(&work, reply)?;
                value(())
            }
            Input::MediaValidate(work) => {
                self.runtime()?
                    .validate_cloud_media_work(&self.host.collection()?, &work)?;
                value(())
            }
            Input::WriteDuringMediaTrash(name, bytes, in_place) => {
                let path = self.host.media()?.store().media_dir().join(name);
                *TRASH_WRITE.lock().unwrap() = Some((path, bytes, in_place));
                meguri_model::faultpoint::arm(
                    "media.trash.before_move",
                    meguri_model::faultpoint::Fault::Callback(write_during_trash),
                );
                value(())
            }
            Input::FailMediaIo => {
                self.runtime()?.fail_next_media_io_for_testing();
                value(())
            }
            Input::Updates(i) => value(
                self.runtime()?
                    .take_pending_updates(&i)
                    .map_err(|error| ChildFailure::SDKUpdates(Box::new(ChildFailure::classify(Box::new(error)))))?,
            ),
            Input::Transition(i, k, p) => value(self.runtime()?.pending_transition(&i, k, p)?),
            Input::Sendables => value(self.runtime()?.tracking_sendables()),
            Input::Build(i, names) => value(self.runtime()?.build_collection_batch(
                &i,
                &self.host.collection()?,
                &names,
            )?),
            Input::EnterHandler(i, id) => {
                self.runtime()?.enter_sent_handler(&i, id)?;
                value(())
            }
            Input::ReturnHandler(i) => value(self.runtime()?.return_sent_handler(&i)?),
            Input::Sent(i, id, saved, conflicts, failed, n) => {
                self.runtime()?.sent_results_with_conflicts(
                    &i,
                    &self.host.collection()?,
                    id,
                    &saved,
                    &conflicts,
                    &failed,
                    n,
                )?;
                value(())
            }
            Input::AppliedUpdate(i, id, names) => {
                self.runtime()?.sdk_update_applied(&i, id, &names).map_err(|error| {
                    ChildFailure::SDKAppliedUpdate(Box::new(ChildFailure::classify(Box::new(error))))
                })?;
                value(())
            }
            Input::SavedRemoved(i, id, names) => {
                self.runtime()?.sdk_saved_removed(&i, id, &names)?;
                value(())
            }
            Input::Qualify(i) => {
                self.runtime()?
                    .sdk_fetch_completed_without_root(&i)
                    .map_err(|error| ChildFailure::SDKQualify(Box::new(ChildFailure::classify(Box::new(error)))))?;
                value(())
            }
            Input::RootRead(i, id, e, tags, lost) => value(self.runtime()?.accept_root_read(&i, id, &e, &tags, lost)?),
            Input::RootFetch(i, e, completed, id) => value(
                self.runtime()?
                    .accept_root_fetch(&i, e.as_ref(), completed, id)
                    .map_err(|error| {
                        ChildFailure::FetchRootAccept(Box::new(ChildFailure::classify(Box::new(error))))
                    })?,
            ),
            Input::ReadDue => value(self.runtime()?.root_read_due()?),
            Input::RootEpoch => value(self.runtime()?.accepted_root_epoch()?),
            Input::CollectionZone => {
                let binding = self.runtime()?.status()?.binding.ok_or_else(failure)?;
                value(core::adapter::collection_zone(binding.facts.epoch))
            }
            Input::SyncNow(i) => value(
                self.runtime()?
                    .sync_now(&i, &self.host.collection()?)
                    .map_err(|error| ChildFailure::SDKSyncNow(Box::new(ChildFailure::classify(Box::new(error)))))?,
            ),
            Input::FetchDue => value(self.runtime()?.exchange_fetch_due()),
            Input::FetchLifecycle => value((
                self.runtime()?.adapter_fetch_drains(),
                self.runtime()?.adapter_missing_did(),
            )),
            Input::FetchValidate(sdk) => {
                self.runtime()?.validate_sdk(&sdk)?;
                value(())
            }
            Input::FetchBegin(sdk, zones) => {
                let runtime = self.runtime()?;
                // Prebound legacy fixtures have no native SDK. They retain their
                // old full-fetch path and cannot produce a native cleanup drain.
                if runtime.incoming_sdk_instance_for_testing().is_none() {
                    value(None::<u64>)
                } else {
                    value(Some(runtime.adapter_begin_fetch(&sdk, zones)?))
                }
            }
            Input::FetchWill(sdk, group, manual) => {
                self.runtime()?.adapter_will_fetch(&sdk, group, manual)?;
                value(())
            }
            Input::FetchZone(sdk, zone, will, failed) => {
                self.runtime()?.adapter_fetch_zone(&sdk, zone, will, failed)?;
                value(())
            }
            Input::FetchDid(sdk, group, manual) => {
                self.runtime()?.adapter_did_fetch(&sdk, &group, manual)?;
                value(())
            }
            Input::FetchFinish(sdk, id) => value(self.runtime()?.adapter_finish_fetch(&sdk, id)?),
            Input::FetchFailed(sdk, id) => {
                self.runtime()?.adapter_complete_exchange(
                    &sdk,
                    id,
                    false,
                    Some(core::adapter::CollectionOutcome::Failed),
                )?;
                value(())
            }
            Input::FetchCompleted(i, phase, seen) => {
                self.runtime()?
                    .exchange_fetch_completed(&i, phase, &seen)
                    .map_err(|error| ChildFailure::FetchCompleted(Box::new(ChildFailure::classify(Box::new(error)))))?;
                value(())
            }
            Input::AssetSuspendResume => {
                let before = self
                    .runtime()?
                    .incoming_sdk_instance_for_testing()
                    .ok_or_else(failure)?;
                if !matches!(
                    self.host.engine().suspend(IPC_TIMEOUT)?,
                    meguri_engine::suspend::SuspendOutcome::Suspended
                ) {
                    return Err(failure());
                }
                self.host.engine().resume()?;
                self.runtime()?.refresh_identity()?;
                let after = self
                    .runtime()?
                    .incoming_sdk_instance_for_testing()
                    .ok_or_else(failure)?;
                value((before, after))
            }
            Input::AssetConflictBodyReads => value(self.asset_conflict_body_reads),
            Input::EntityBodyReads => value(core::testing::entity_body_reads()),
            Input::SavedAckBodyReads => value(core::testing::saved_ack_body_reads()),
            Input::MediaPolicyBlobCandidateVisits => value(core::testing::media_policy_blob_candidate_visits()),
            Input::ResetMediaPolicyBlobCandidateVisits => {
                core::testing::reset_media_policy_blob_candidate_visits();
                value(())
            }
            Input::PolicyMediaFacts => {
                let collection = self.host.collection()?;
                let generation = {
                    let lease = collection.reader(ReaderRole::Cloud)?;
                    let snapshot = lease.snapshot()?;
                    snapshot.generation().0
                };
                value(
                    self.runtime()?
                        .policy_projection(core::policy::Lifecycle::Ready, generation)?
                        .facts
                        .summary
                        .media,
                )
            }
            Input::AssetBodyReadControl(b, name) => {
                let before = core::testing::entity_body_reads();
                let _ = self.runtime()?.binding_entity(b, &name)?.ok_or_else(failure)?;
                value(core::testing::entity_body_reads().saturating_sub(before))
            }
            Input::AssetSDK => value(
                self.runtime()?
                    .incoming_sdk_instance_for_testing()
                    .ok_or_else(failure)?,
            ),
            Input::AssetReadPass(kind) => {
                let sdk = self
                    .runtime()?
                    .incoming_sdk_instance_for_testing()
                    .ok_or_else(failure)?;
                value(
                    self.runtime()?
                        .begin_asset_read_pass(&sdk, &self.host.collection()?, kind)?,
                )
            }
            Input::NextAssetRead(pass) => value(self.runtime()?.next_asset_read(&self.host.collection()?, &pass)?),
            Input::CancelAssetRead(request) => value(self.runtime()?.cancel_asset_read(&request)),
            Input::AssetReadOwnership(request) => value((
                self.runtime()?.asset_read_owned_for_testing(&request),
                self.asset_read_queue
                    .lock()
                    .unwrap_or_else(PoisonError::into_inner)
                    .contains(&request),
            )),
            Input::CancelAssetPass(pass) => {
                self.runtime()?.cancel_asset_read_pass(&pass);
                value(())
            }
            Input::AdmitAssetRead(request, accept) => value(self.runtime()?.submit_asset_read_queue(
                &self.host.collection()?,
                &request,
                || {
                    if !accept {
                        return false;
                    }
                    let mut queue = self
                        .asset_read_queue
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner);
                    if queue.contains(&request) {
                        return false;
                    }
                    queue.push(request.clone());
                    true
                },
            )?),
            Input::AssetConflict(sdk, id, server) => {
                let before = core::testing::entity_body_reads();
                self.runtime()?
                    .sent_asset_save_conflict(&sdk, &self.host.collection()?, id, &server)?;
                self.asset_conflict_body_reads = core::testing::entity_body_reads().saturating_sub(before);
                value(())
            }
            Input::IncomingIntents(b) => value(self.runtime()?.apply_intents(b)?),
            Input::IncomingInbox(b) => value(
                self.runtime()?
                    .inbox_entries(b)?
                    .into_iter()
                    .map(|stored| {
                        Ok(IncomingInboxEntry {
                            id: stored.id,
                            record_name: stored.entry.record_name,
                            value_sha256: inbox_value_sha256(&stored.entry.value)?,
                            erev: match stored.entry.value {
                                core::InboxValue::Record { erev, .. } => erev,
                                core::InboxValue::Deletion => 0,
                            },
                            reason: stored.entry.reason,
                            source: stored.entry.source,
                            retry_count: stored.entry.retry_count,
                        })
                    })
                    .collect::<Result<Vec<_>>>()?,
            ),
            Input::IncomingRefused(b) => value(
                self.runtime()?
                    .refused_entries(b)?
                    .into_iter()
                    .map(|e| IncomingRefusedEntry {
                        record_name: e.record_name,
                        erev: e.erev.unwrap_or(0),
                        version_known: e.erev.is_some(),
                        origin: e.origin,
                        reason: e.reason,
                    })
                    .collect::<Vec<_>>(),
            ),
            Input::BindingEntity(b, name) => value(self.runtime()?.binding_entity(b, &name)?),
            Input::DeliverCollection(records) => {
                self.runtime()?
                    .receive_collection_records(&self.runtime()?.instance()?, &records)?;
                value(())
            }
            Input::DrainIncoming(steps) => {
                if steps == 0 {
                    value(0usize)
                } else {
                    value(self.host.apply_cloud_incoming(IPC_TIMEOUT)?)
                }
            }
            Input::PauseAutomaticWAL(paused) => {
                self.host.collection()?.pause_automatic_wal_flush_for_testing(paused);
                value(())
            }
            Input::Fetched(..)
            | Input::ServiceEpoch(..)
            | Input::FetchApply(..)
            | Input::DeliverCollectionAssets(..)
            | Input::DeliverCollectionDownload(..)
            | Input::AssetReadResult(..) => Err(failure()),
            Input::SendsFinished(i) => {
                self.runtime()?.exchange_sends_finished(&i)?;
                value(())
            }
            Input::WantsSend => value(self.runtime()?.wants_send()),
            Input::StartCall(i) => value(self.runtime()?.start_send_call(&i)?),
            Input::RecordCall(i, id, request) => {
                self.runtime()?.send_call_request(&i, id, request)?;
                value(())
            }
            Input::FinishCall(i, id) => {
                self.runtime()?.finish_send_call(&i, id)?;
                value(())
            }
            Input::Calls => value(self.runtime()?.send_calls()),
            Input::Exchange(id) => value(self.runtime()?.exchange_result(id).ok_or_else(failure)?),
            Input::FetchedExchange(id) => value((
                self.runtime()?.adapter_exchange_active(),
                self.runtime()?.adapter_exchange_result(id),
            )),
            Input::Begin(note) => {
                let c = self.host.collection()?;
                let (id, deck) = {
                    let lease = c.reader(ReaderRole::Query)?;
                    let mut after = None;
                    let mut found = None;
                    loop {
                        let page = lease.row_page(Table::Cards, after.as_ref())?;
                        if page.is_empty() {
                            break;
                        }
                        for row in page {
                            let id = core::row_payload::int(&row, 0)?;
                            after = Some(RowKey::Id(id));
                            if core::row_payload::int(&row, 1)? == note {
                                found = Some((id, core::row_payload::int(&row, 2)?));
                                break;
                            }
                        }
                        if found.is_some() {
                            break;
                        }
                    }
                    found.ok_or_else(failure)?
                };
                let meguri_engine::session::StartResult::Started { session_id, .. } =
                    self.host.review().start(DeckId(deck), Some(CardId(id)))?
                else {
                    return Err(failure());
                };
                self.session_started.insert(session_id.0, self.clock.now_ms());
                self.runtime()?.poll_review(&self.runtime()?.instance()?)?;
                value(session_id.0)
            }
            Input::Start(s) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().ok_or_else(failure)?;
                self.host.review().render_head(
                    SessionId(s),
                    &[item.render_key],
                    meguri_engine::rendering::Platform::Mac,
                )?;
                value(self.card(item.card_id.0)?)
            }
            Input::Show(s, id) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().filter(|h| h.card_id.0 == id).ok_or_else(failure)?;
                self.host.review().render_head(
                    SessionId(s),
                    &[item.render_key],
                    meguri_engine::rendering::Platform::Mac,
                )?;
                value(())
            }
            Input::Good(s, id) => {
                let head = self.host.review().head(SessionId(s), 1)?;
                let item = head.items.first().filter(|h| h.card_id.0 == id).ok_or_else(failure)?;
                let at = self.clock.now_ms();
                let elapsed = u64::try_from(at - *self.session_started.get(&s).ok_or_else(failure)?)?;
                let answer = self.host.review().answer(
                    SessionId(s),
                    CardId(id),
                    meguri_sched::answer::Rating::Good,
                    item.state_token,
                    meguri_model::time::EpochMillis(at),
                    elapsed,
                )?;
                if !matches!(answer, meguri_engine::session::AnswerOutcome::Accepted { .. }) {
                    return Err(failure());
                }
                self.host.review().application_barrier_for_testing()?;
                let card = self.card(id)?;
                let hold = self.host.collection()?.held_back_run(Some(SessionId(s)))?;
                let op_id = hold.steps.first().ok_or_else(failure)?.id.0;
                value(ReviewAnswer {
                    revlog_id: card.last_review_id,
                    op_id,
                })
            }
            Input::End(s) => {
                self.host.review().end(SessionId(s))?;
                self.host.review().application_barrier_for_testing()?;
                self.runtime()?.poll_review(&self.runtime()?.instance()?)?;
                value(())
            }
            Input::ActiveReviewSession => value(self.host.review().active_session().map(|session| session.0)),
            Input::Hold(s) => {
                let c = self.host.collection()?;
                let hold = c.held_back_run(Some(SessionId(s)))?;
                let key = self.runtime()?.status()?.binding.ok_or_else(failure)?.facts.record_key;
                let mut entity_names = BTreeSet::new();
                for row in hold.rows() {
                    let row = row?;
                    if let Some(kind) = core::row_payload::KINDS
                        .into_iter()
                        .find(|k| core::row_payload::table(*k) == Some(row.table))
                    {
                        let k = match row.key {
                            RowKey::Id(id) => codec::id_key(id).to_vec(),
                            RowKey::Text(t) => t,
                            _ => continue,
                        };
                        entity_names.insert(codec::entity_name(&key.0, kind, &k)?);
                    }
                }
                value(ReviewHold {
                    op_ids: hold.steps.iter().map(|s| s.id.0).collect(),
                    entity_names,
                })
            }
            Input::CloudHold(s) => {
                let c = self.host.collection()?;
                let hold = c.cloud_held_run(Some(SessionId(s)))?;
                let key = self.runtime()?.status()?.binding.ok_or_else(failure)?.facts.record_key;
                let mut entity_names = BTreeSet::new();
                for row in hold.rows() {
                    let row = row?;
                    if let Some(kind) = core::row_payload::KINDS
                        .into_iter()
                        .find(|k| core::row_payload::table(*k) == Some(row.table))
                    {
                        let k = match row.key {
                            RowKey::Id(id) => codec::id_key(id).to_vec(),
                            RowKey::Text(t) => t,
                            _ => continue,
                        };
                        entity_names.insert(codec::entity_name(&key.0, kind, &k)?);
                    }
                }
                value(ReviewHold {
                    op_ids: hold.steps.iter().map(|s| s.id.0).collect(),
                    entity_names,
                })
            }
            Input::Card(id) => value(self.card(id)?),
            Input::Revlog(id) => value(self.revlog(id)?),
            Input::Advance(ms) => {
                self.clock.step_ms(ms);
                value(())
            }
            input @ (Input::BackupPath(..)
            | Input::BackupNotices
            | Input::RegisterSDK(..)
            | Input::RootConfirmed
            | Input::PreviewBegin
            | Input::PreviewActive(..)
            | Input::PreviewEpoch(..)
            | Input::PreviewCancel
            | Input::PublicationConfirm(..)
            | Input::AdoptionConfirm(..)
            | Input::AdoptionReply(..)
            | Input::AdoptionLocal(..)
            | Input::PadAdoptionCandidate
            | Input::PublicationNext
            | Input::PublicationLocal(..)
            | Input::PublicationCancel(..)
            | Input::PublicationTrigger
            | Input::PublicationFacts
            | Input::AuthorityStatus
            | Input::CollectionRows
            | Input::MediaRows
            | Input::CollectionHeader
            | Input::BackupNow
            | Input::RestoreBackup(..)
            | Input::BackupRows(..)
            | Input::BackupRetention(..)
            | Input::PutMedia(..)
            | Input::MediaFiles
            | Input::RecoveryState
            | Input::BeginZoneListing(..)
            | Input::ZoneListingResult(..)
            | Input::CollectionDrained
            | Input::DeletionGuard
            | Input::DeletionCheck(..)) => publication_driver::handle(self, input),
            Input::Close => {
                self.host.close()?;
                self.closed = true;
                value(())
            }
        }
    }
}
/// Infrastructure entry only. A normal invocation performs no product check.
/// The parent selects this exact test in a subprocess and supplies a private fd.
pub fn child_entry() -> Result<()> {
    let Ok(fd) = std::env::var("MEGURI_CLOUD_CHILD_FD") else {
        return Ok(());
    };
    let fd: i32 = fd.parse()?;
    if fd < 3 {
        return Err(failure());
    }
    let mut stream = unsafe { UnixStream::from_raw_fd(fd) };
    stream.set_read_timeout(Some(IPC_TIMEOUT))?;
    stream.set_write_timeout(Some(IPC_TIMEOUT))?;
    let Input::Boot(boot) = read_frame(&mut stream)? else {
        return Err(failure());
    };
    let mut device = match Device::new(boot, &stream) {
        Ok(d) => {
            write_frame(&mut stream, &ChildMessage::Reply(Ok(serde_json::Value::Null)))?;
            d
        }
        Err(error) => {
            write_frame(&mut stream, &ChildMessage::Reply(Err(ChildFailure::classify(error))))?;
            return Err(failure());
        }
    };
    // The parent may be busy with a sibling until a new command begins.
    // EOF and partial-frame timeouts still unwind and close the host.
    for _ in 0..100_000 {
        let input: Input = read_idle_command(&mut stream)?;
        let Some(input) = incoming_driver::dispatch(&mut device, input, &mut stream)? else {
            continue;
        };
        let close = matches!(input, Input::Close);
        let response = handle_transport_command(&mut device, input, &mut stream).map_err(ChildFailure::classify);
        // A Close served during a reply wait has already sent its own reply.
        // Do not send a second reply for the abandoned transport command.
        if device.closed && !close {
            return Ok(());
        }
        write_frame(&mut stream, &ChildMessage::Reply(response))?;
        if close {
            return Ok(());
        }
    }
    Err(failure())
}

// Key IPC is driven by the production provider call, never by Boot/preview.
struct ChildKeys(Mutex<UnixStream>);
impl runtime::KeyByteSource for ChildKeys {
    fn draw_key(&self) -> std::result::Result<[u8; 32], runtime::KeySourceError> {
        let mut stream = self.0.lock().map_err(|_| runtime::KeySourceError::Unavailable)?;
        let mut exchange = || -> Result<_> {
            write_frame(&mut stream, &ChildMessage::DrawKey)?;
            match read_frame(&mut stream)? {
                Input::KeyResult(result) => Ok(result),
                _ => Err(failure()),
            }
        };
        exchange().unwrap_or(Err(runtime::KeySourceError::Unavailable))
    }
}
fn transport_enqueue(stream: &mut UnixStream, caller: super::Caller, operation: super::Operation) -> Result<()> {
    write_frame(stream, &ChildMessage::Submit(caller, operation))?;
    match read_frame(stream)? {
        Input::TransportAccepted => Ok(()),
        _ => Err(failure()),
    }
}
fn transport_reply(device: &mut Device, stream: &mut UnixStream) -> Result<super::Response> {
    loop {
        match read_frame(stream)? {
            Input::TransportResult(response) => return Ok(response),
            input => {
                let close = matches!(input, Input::Close);
                let response = device.handle(input).map_err(ChildFailure::classify);
                write_frame(stream, &ChildMessage::Reply(response))?;
                if close {
                    return Err(failure());
                }
            }
        }
    }
}
struct ChildTransport<'a> {
    device: &'a mut Device,
    stream: &'a mut UnixStream,
}
impl super::coordinator::PublicationTransport for ChildTransport<'_> {
    type Enqueued = ();
    fn enqueue(&mut self, caller: super::Caller, operation: super::Operation) -> Result<()> {
        transport_enqueue(self.stream, caller, operation)
    }
    fn reply(&mut self, (): ()) -> Result<super::Response> {
        transport_reply(self.device, self.stream)
    }
}
fn complete_loss_identity_read(runtime: &runtime::Runtime, inputs: &runtime::CloudInputs) -> Result<()> {
    // Use the operation's original Runtime and configured provider, even if a
    // nested command replaced the host while its actual reply was held.
    let read = runtime.adapter_begin_identity_read(false, None)?;
    let identity = inputs.identity.read_identity(&inputs.scope);
    if !runtime.adapter_complete_identity_read(read, identity)? {
        return Err(runtime::RuntimeError::IdentityUnverified.into());
    }
    // Acceptance alone is not same-account proof. The original result reducer
    // still checks freshness, identity and the operation's unchanged authority.
    Ok(())
}
fn handle_transport_command(device: &mut Device, input: Input, stream: &mut UnixStream) -> Result<serde_json::Value> {
    match input {
        Input::PublicationExecute(request) => {
            let runtime = device.host.cloud_runtime().cloned().ok_or_else(failure)?;
            let inputs = device.options.cloud.clone().ok_or_else(failure)?;
            let collection = device.host.collection()?;
            let (result, responses) = super::coordinator::execute_publication_transport(
                &request,
                &runtime,
                &collection,
                &mut ChildTransport { device, stream },
            )?;
            if let Some(event) = core::selected_observation::PublicationEvent::from_reply(&request, &result) {
                match event.kind {
                    core::publication::OwnKind::SaveCompleteEpoch => device.publication_reply_events.0 = Some(event),
                    core::publication::OwnKind::SaveActive => device.publication_reply_events.1 = Some(event),
                    _ => {}
                }
            }
            use core::publication::{OwnKind, OwnResult, RecordRead};
            if matches!(&result, OwnResult::ZoneLosses(_))
                || (request.kind == OwnKind::ReadOwnEpoch
                    && matches!(&result, OwnResult::Read(RecordRead::Missing | RecordRead::ZoneMissing)))
            {
                complete_loss_identity_read(&runtime, &inputs)?;
            }
            runtime.publication_result(&request, result, &collection)?;
            Ok(serde_json::to_value(responses)?)
        }
        Input::DeleteZone(zone) => {
            let runtime = device.host.cloud_runtime().cloned().ok_or_else(failure)?;
            let inputs = device.options.cloud.clone().ok_or_else(failure)?;
            let collection = device.host.collection()?;
            let guard = device.deletion.clone().ok_or_else(failure)?;
            runtime.submit_zone_deletion(&guard, &collection, || {
                transport_enqueue(
                    stream,
                    super::Caller {
                        own_operation: Some("publication-cleanup".into()),
                        ..Default::default()
                    },
                    super::Operation::DeleteZone {
                        zone: zone.clone(),
                        cause: super::RemovalCause::Deleted,
                    },
                )
            })??;
            let response = transport_reply(device, stream)?;
            // Keep the actual Service failure: ZoneNotFound alone removes the
            // retained row; its reset bit additionally isolates this binding.
            // The original guard/collection and single transport reply still own
            // completion, including when a nested command invalidated authority.
            let failure = response.result.as_ref().err().map(|error| {
                use super::ErrorCode;
                core::zone_deletion::FailureFacts {
                    code: Some(match error.code {
                        ErrorCode::UnknownItem => 11,
                        ErrorCode::ServerRecordChanged => 14,
                        ErrorCode::InvalidArguments => 12,
                        ErrorCode::LimitExceeded => 27,
                        ErrorCode::QuotaExceeded => 25,
                        ErrorCode::BatchRequestFailed => 22,
                        ErrorCode::ZoneNotFound => 26,
                        ErrorCode::UserDeletedZone => 28,
                        ErrorCode::ChangeTokenExpired => 21,
                    }),
                    retry_after_ms: None, // Service has no retry-after field.
                    encrypted_data_reset: error.encrypted_data_reset,
                }
            });
            if failure
                .as_ref()
                .is_some_and(|f| f.code == Some(26) && f.encrypted_data_reset)
            {
                complete_loss_identity_read(&runtime, &inputs)?;
            }
            runtime.zone_deletion_batch_result(
                &guard,
                &collection,
                &[core::zone_deletion::ZoneResult { zone, failure }],
            )?;
            Ok(serde_json::to_value(response)?)
        }
        input => device.handle(input),
    }
}
#[path = "publication_driver.rs"]
mod publication_driver;

#[path = "incoming_driver.rs"]
mod incoming_driver;

#[cfg(test)]
#[path = "child_fetched_sources_tests.rs"]
mod fetched_sources_tests;

#[cfg(test)]
mod idle_command_tests {
    use super::*;
    use std::io::ErrorKind;

    #[test]
    fn partial_frames_and_eof_remain_errors_after_idle_admission() {
        for eof in [false, true] {
            let partials: &[&[u8]] = if eof {
                &[&[], &[0], &[0, 0, 0, 4, b'n']]
            } else {
                &[&[0], &[0, 0, 0, 4, b'n']]
            };
            for bytes in partials {
                let (mut child, mut parent) = UnixStream::pair().unwrap();
                child.set_read_timeout(Some(Duration::from_millis(50))).unwrap();
                let timeout = child.read_timeout().unwrap();
                parent.write_all(bytes).unwrap();
                if eof {
                    parent.shutdown(std::net::Shutdown::Write).unwrap();
                }
                let error = read_idle_command::<serde_json::Value>(&mut child).unwrap_err();
                let kind = error.downcast_ref::<std::io::Error>().unwrap().kind();
                if eof {
                    assert_eq!(kind, ErrorKind::UnexpectedEof);
                } else {
                    assert!(matches!(kind, ErrorKind::WouldBlock | ErrorKind::TimedOut));
                }
                assert_eq!(child.read_timeout().unwrap(), timeout);
            }
        }

        let (mut child, mut parent) = UnixStream::pair().unwrap();
        child.set_read_timeout(Some(IPC_TIMEOUT)).unwrap();
        write_frame(&mut parent, &serde_json::Value::Null).unwrap();
        assert_eq!(
            read_idle_command::<serde_json::Value>(&mut child).unwrap(),
            serde_json::Value::Null
        );
        assert_eq!(child.read_timeout().unwrap(), Some(IPC_TIMEOUT));
    }
}
