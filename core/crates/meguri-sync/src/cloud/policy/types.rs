use crate::cloud::{BindingId, BindingState, OpaqueBytes};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DeviceSettings {
    pub enabled: bool,
    pub paused: bool,
    pub automatic: bool,
    pub media: bool,
}
impl DeviceSettings {
    pub fn default_connected() -> Self {
        Self {
            enabled: true,
            paused: false,
            automatic: true,
            media: true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Lifecycle {
    Ready,
    #[default]
    NoCollection,
    Opening,
    Upgrading,
    ExclusiveBusy,
    Failed,
    Corrupt,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccountStatus {
    Available,
    NoAccount,
    Restricted,
    Temporary,
    Configuration,
    #[default]
    Undetermined,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Account {
    pub status: AccountStatus,
    pub identity: Option<OpaqueBytes>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PublishStep {
    Bound,
    Zones,
    Sent,
    Complete,
    Done,
    Cancelling,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingView {
    pub id: BindingId,
    pub state: BindingState,
    pub account: OpaqueBytes,
    pub usable: bool,
    pub verified: bool,
    pub publish_step: Option<PublishStep>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Fence {
    #[default]
    Clear,
    DecisionRequired,
    UnprovenAdoption,
    ChooseCollection,
    LocalOnly,
    EnvironmentChanged,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Scan {
    None,
    #[default]
    Required,
    Running,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Attention {
    Isolated,
    RootUnreadable,
    ChooseCollection,
    Conflicts,
    ConflictsUnreadable,
    Refused,
    ReferenceWaiting,
    Oversize,
    Unreadable,
    Unsyncable,
    ClaimStuck,
    GenerationIncomplete,
    AssetUnavailable,
    ApplyFailed,
    RekeyFailed,
    CrtMismatch,
    CollectionCorrupt,
    MediaZoneMissing,
    MediaChanged,
    MediaMissing,
    RecoveryOpWaiting,
    ExchangeIncomplete,
    QuotaFull,
    LocalStorageFull,
    TrackingError,
    UnsupportedSchema,
    CloudError,
}
/// Provider evidence is optional. None means unavailable, never a successful zero.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct MediaFacts {
    pub upload_files: Option<u64>,
    pub upload_bytes: Option<u64>,
    pub download_files: Option<u64>,
    pub download_bytes: Option<u64>,
    pub unknown_blobs: Option<u64>,
    pub missing_names: Option<u64>,
    pub uploads_running: Option<u64>,
    pub downloads_running: Option<u64>,
    pub root_acked: Option<bool>,
    pub zone_missing: Option<bool>,
    pub no_space: Option<bool>,
    /// Provider computes this from a full 60 s throughput window; no extrapolation here.
    pub remaining_download_ms: Option<u64>,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Summary {
    /// Complete accounting, independently of whether anything is sendable.
    pub collection_known: bool,
    pub attention_known: bool,
    pub pending_unheld: u64,
    pub sendable: u64,
    pub parked: u64,
    pub held: u64,
    pub in_flight: u64,
    pub ready: u64,
    pub incoming_waiting: u64,
    pub asset_retryable: u64,
    pub apply_failed: u64,
    pub apply_in_progress: bool,
    pub unsettled_intents: u64,
    pub scan: Scan,
    pub crt_mismatch: bool,
    pub zone_deletions_due: u64,
    pub attention: BTreeMap<Attention, u64>,
    pub media: MediaFacts,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct CoreFacts {
    pub lifecycle: Lifecycle,
    pub generation: u64,
    pub binding: Option<BindingView>,
    pub session: Option<u64>,
    pub fence: Fence,
    pub summary: Summary,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Engine {
    pub instance: String,
    pub binding: BindingId,
    pub stopping: bool,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Zone {
    Root,
    Collection,
    Media,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum OperationKind {
    Send,
    Fetch,
    Read,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct Operation {
    pub zone: Zone,
    pub kind: OperationKind,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Outcome {
    Ok,
    Failed,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Activity {
    Started,
    Ended { outcome: Outcome, saved: u64 },
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OperationError {
    NoAccount,
    Configuration,
    Offline,
    Throttled,
    QuotaFull,
    LocalStorageFull,
    TrackingError,
    ConflictRecovery,
    UnsupportedSchema,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ErrorDetail {
    pub class: OperationError,
    pub code: i64,
    pub retry_after_ms: Option<i64>,
    pub operation: Operation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Source {
    Command,
    Key,
    AnkiConnect,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExchangeOutcome {
    Succeeded {
        selected: u64,
        held_back: u64,
        newer_pending: u64,
        incoming_waiting: u64,
        media: MediaFacts,
    },
    Incomplete {
        outstanding: u64,
        blocked: u64,
    },
    Waiting {
        class: OperationError,
        retry_after_ms: Option<i64>,
    },
    Failed {
        class: OperationError,
    },
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeRecord {
    pub exchange: u64,
    pub ended_ms: i64,
    pub source: Source,
    pub outcome: ExchangeOutcome,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evidence {
    pub last_completed: Option<ExchangeRecord>,
    pub last_attempt: Option<ExchangeRecord>,
    pub quiet_ms: Option<i64>,
}
impl Evidence {
    pub fn as_of_ms(&self) -> Option<i64> {
        self.quiet_ms
            .into_iter()
            .chain(self.last_completed.as_ref().map(|r| r.ended_ms))
            .max()
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unavailable {
    CollectionUnavailable,
    ReviewActive,
    NotConnected,
    Paused,
    ICloudUnavailable,
    DecisionRequired,
    RecoveryRequired,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnavailableReason {
    NoAccount,
    Restricted,
    Temporary,
    SetupNeeded,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PausedReason {
    DecisionRequired,
    UserPaused,
    AutomaticOff,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusState {
    LocalOnly,
    ICloudUnavailable(UnavailableReason),
    Paused(PausedReason),
    NeedsAttention,
    Offline,
    SyncingCollection,
    SyncingMedia,
    ChangesPending,
    UpToDate {
        as_of_ms: i64,
    },
    /// No qualified timestamp or an incomplete provider projection. Never display as synced.
    AwaitingEvidence,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Status {
    pub state: StatusState,
    pub pending: u64,
    pub held_back: u64,
    pub in_flight: u64,
    pub incoming_waiting: u64,
    pub attention: BTreeMap<Attention, u64>,
    pub media: MediaFacts,
    pub media_paused: bool,
    pub error: Option<ErrorDetail>,
    pub account: Account,
    pub account_matches_binding: bool,
    pub evidence: Evidence,
    pub sync_now: Result<(), Unavailable>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChangeCause {
    Local,
    Cloud,
    Scan,
    Probe,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Input {
    pub now_ms: i64,
    pub facts: Option<CoreFacts>,
    pub event: Event,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Event {
    Lifecycle {
        state: Lifecycle,
        generation: u64,
    },
    CollectionReplaced {
        generation: u64,
    },
    SessionStarted(u64),
    SessionEnded(u64),
    Binding(Option<BindingView>),
    PublicationFence(Fence),
    TrackingChanged(ChangeCause),
    TrackingWriteFailed {
        no_space: bool,
    },
    Account(Account),
    Network {
        reachable: bool,
    },
    DeviceSettings(DeviceSettings),
    EngineStarted {
        instance: String,
        binding: BindingId,
    },
    EngineStopped {
        instance: String,
    },
    EngineActivity {
        instance: String,
        operation: Operation,
        phase: Activity,
    },
    EngineError {
        instance: String,
        operation: Operation,
        error: OperationError,
        code: i64,
        retry_after_ms: Option<i64>,
    },
    FetchDrained {
        zone: Zone,
        fetched: bool,
    },
    RootConfirmed {
        instance: String,
    },
    VolumeSpaceAvailable,
    Foreground,
    Background {
        deadline_ms: i64,
    },
    BackgroundExpiring,
    QuitRequested,
    FlushTimerFired,
    SyncNowRequested(Source),
    SyncNowCancelled {
        exchange: u64,
    },
    ExchangeEnded {
        exchange: u64,
        outcome: ExchangeOutcome,
    },
    VisibleMedia {
        names: Vec<String>,
    },
    /// Core-only acknowledgement of a successful evidence commit.
    EvidenceCommitted(Evidence),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Update {
    Full,
    Changes,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StopReason {
    GateClosed,
    TrackingFailed,
    BindingChanged,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Guard {
    pub generation: u64,
    pub binding: Option<BindingId>,
    pub token: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Action {
    pub guard: Guard,
    pub kind: ActionKind,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ActionKind {
    StartEngine { binding: BindingId, automatic: bool },
    StopEngine { reason: StopReason },
    ConfigureAutomatic { on: bool },
    UpdatePending(Update),
    RequestFetch { zones: Vec<Zone> },
    RequestSend { zones: Vec<Zone> },
    ReadBlobs { include_missing: bool },
    RetryAssetEntries,
    RunCleanup,
    DeleteZones,
    ArmFlushTimer { at_ms: i64 },
    CancelFlushTimer,
    RunExchange { exchange: u64, source: Source },
    CancelExchange { exchange: u64 },
    BackgroundFlush { until_ms: i64 },
    CancelBackgroundFlush,
    QuitFlush { until_ms: i64 },
    QuitNow,
    PauseMedia,
    ResumeMedia,
    PrioritizeMedia { names: Vec<String> },
    ApplyReady { retry_failed: bool },
    RecordExchange(ExchangeRecord),
    RecordQuietPoint { at_ms: i64 },
    ProbeTracking,
    StatusChanged(Status),
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Exchange {
    pub id: u64,
    pub source: Source,
    pub guard: Guard,
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Cycle {
    pub failures: u32,
    pub active: bool,
    pub cloud_error: bool,
    pub error: Option<ErrorDetail>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct State {
    pub facts: CoreFacts,
    pub settings: DeviceSettings,
    pub account: Account,
    pub reachable: bool,
    pub foreground: bool,
    pub engine: Option<Engine>,
    pub root_confirmed: bool,
    pub token: u64,
    pub flush_at_ms: Option<i64>,
    pub exchange: Option<Exchange>,
    pub next_exchange: u64,
    pub background_until_ms: Option<i64>,
    pub quit_until_ms: Option<i64>,
    pub drained: [bool; 2],
    pub drain_serial: u64,
    pub quiet_serial: u64,
    pub tracking_stop: Option<bool>,
    pub local_restart_used: bool,
    pub probe_in_progress: bool,
    pub quota_full: bool,
    pub transient_auth: bool,
    pub last_error: Option<ErrorDetail>,
    pub cycles: BTreeMap<Operation, Cycle>,
    pub evidence: Evidence,
    pub last_cleanup_ms: Option<i64>,
    pub last_status: Option<Status>,
}
impl Default for State {
    fn default() -> Self {
        Self {
            facts: CoreFacts::default(),
            settings: DeviceSettings::default(),
            account: Account::default(),
            reachable: false,
            foreground: true,
            engine: None,
            root_confirmed: false,
            token: 0,
            flush_at_ms: None,
            exchange: None,
            next_exchange: 0,
            background_until_ms: None,
            quit_until_ms: None,
            drained: [false; 2],
            drain_serial: 0,
            quiet_serial: 0,
            tracking_stop: None,
            local_restart_used: false,
            probe_in_progress: false,
            quota_full: false,
            transient_auth: false,
            last_error: None,
            cycles: BTreeMap::new(),
            evidence: Evidence::default(),
            last_cleanup_ms: None,
            last_status: None,
        }
    }
}
