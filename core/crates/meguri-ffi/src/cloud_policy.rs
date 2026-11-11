//! Retained facade over CoreHost's real policy Driver. DTOs carry no authority.
//! Native queue callbacks only admit prepared work, outside all core locks.
use crate::{CoreError, callback, sync::CloudWorkflow};
use meguri_sync::cloud::policy as p;
use std::sync::{Arc, Mutex, PoisonError, Weak};
use std::time::Duration;

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyFailure {
    Closed,
    WrongOwner,
    GenerationChanged,
    StaleCommand,
    NotCoreAction,
    NotNativeAction,
    CoreCapabilityUnavailable,
    UnqualifiedCompletion,
}
fn failure(failure: CloudPolicyFailure) -> CoreError {
    CoreError::CloudPolicy { failure }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyZone {
    Root,
    Collection,
    Media,
}
impl From<p::Zone> for CloudPolicyZone {
    fn from(v: p::Zone) -> Self {
        match v {
            p::Zone::Root => Self::Root,
            p::Zone::Collection => Self::Collection,
            p::Zone::Media => Self::Media,
        }
    }
}
impl From<CloudPolicyZone> for p::Zone {
    fn from(v: CloudPolicyZone) -> Self {
        match v {
            CloudPolicyZone::Root => Self::Root,
            CloudPolicyZone::Collection => Self::Collection,
            CloudPolicyZone::Media => Self::Media,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyOperationKind {
    Send,
    Fetch,
    Read,
}
impl From<p::OperationKind> for CloudPolicyOperationKind {
    fn from(v: p::OperationKind) -> Self {
        match v {
            p::OperationKind::Send => Self::Send,
            p::OperationKind::Fetch => Self::Fetch,
            p::OperationKind::Read => Self::Read,
        }
    }
}
impl From<CloudPolicyOperationKind> for p::OperationKind {
    fn from(v: CloudPolicyOperationKind) -> Self {
        match v {
            CloudPolicyOperationKind::Send => Self::Send,
            CloudPolicyOperationKind::Fetch => Self::Fetch,
            CloudPolicyOperationKind::Read => Self::Read,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyOperationError {
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
impl From<p::OperationError> for CloudPolicyOperationError {
    fn from(v: p::OperationError) -> Self {
        match v {
            p::OperationError::NoAccount => Self::NoAccount,
            p::OperationError::Configuration => Self::Configuration,
            p::OperationError::Offline => Self::Offline,
            p::OperationError::Throttled => Self::Throttled,
            p::OperationError::QuotaFull => Self::QuotaFull,
            p::OperationError::LocalStorageFull => Self::LocalStorageFull,
            p::OperationError::TrackingError => Self::TrackingError,
            p::OperationError::ConflictRecovery => Self::ConflictRecovery,
            p::OperationError::UnsupportedSchema => Self::UnsupportedSchema,
            p::OperationError::Other => Self::Other,
        }
    }
}
impl From<CloudPolicyOperationError> for p::OperationError {
    fn from(v: CloudPolicyOperationError) -> Self {
        match v {
            CloudPolicyOperationError::NoAccount => Self::NoAccount,
            CloudPolicyOperationError::Configuration => Self::Configuration,
            CloudPolicyOperationError::Offline => Self::Offline,
            CloudPolicyOperationError::Throttled => Self::Throttled,
            CloudPolicyOperationError::QuotaFull => Self::QuotaFull,
            CloudPolicyOperationError::LocalStorageFull => Self::LocalStorageFull,
            CloudPolicyOperationError::TrackingError => Self::TrackingError,
            CloudPolicyOperationError::ConflictRecovery => Self::ConflictRecovery,
            CloudPolicyOperationError::UnsupportedSchema => Self::UnsupportedSchema,
            CloudPolicyOperationError::Other => Self::Other,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicySource {
    Command,
    Key,
    AnkiConnect,
}
impl From<p::Source> for CloudPolicySource {
    fn from(v: p::Source) -> Self {
        match v {
            p::Source::Command => Self::Command,
            p::Source::Key => Self::Key,
            p::Source::AnkiConnect => Self::AnkiConnect,
        }
    }
}
impl From<CloudPolicySource> for p::Source {
    fn from(v: CloudPolicySource) -> Self {
        match v {
            CloudPolicySource::Command => Self::Command,
            CloudPolicySource::Key => Self::Key,
            CloudPolicySource::AnkiConnect => Self::AnkiConnect,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyChangeCause {
    Local,
    Cloud,
    Scan,
    Probe,
}
impl From<p::ChangeCause> for CloudPolicyChangeCause {
    fn from(v: p::ChangeCause) -> Self {
        match v {
            p::ChangeCause::Local => Self::Local,
            p::ChangeCause::Cloud => Self::Cloud,
            p::ChangeCause::Scan => Self::Scan,
            p::ChangeCause::Probe => Self::Probe,
        }
    }
}
impl From<CloudPolicyChangeCause> for p::ChangeCause {
    fn from(v: CloudPolicyChangeCause) -> Self {
        match v {
            CloudPolicyChangeCause::Local => Self::Local,
            CloudPolicyChangeCause::Cloud => Self::Cloud,
            CloudPolicyChangeCause::Scan => Self::Scan,
            CloudPolicyChangeCause::Probe => Self::Probe,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyUnavailable {
    CollectionUnavailable,
    ReviewActive,
    NotConnected,
    Paused,
    ICloudUnavailable,
    DecisionRequired,
    RecoveryRequired,
}
impl From<p::Unavailable> for CloudPolicyUnavailable {
    fn from(v: p::Unavailable) -> Self {
        match v {
            p::Unavailable::CollectionUnavailable => Self::CollectionUnavailable,
            p::Unavailable::ReviewActive => Self::ReviewActive,
            p::Unavailable::NotConnected => Self::NotConnected,
            p::Unavailable::Paused => Self::Paused,
            p::Unavailable::ICloudUnavailable => Self::ICloudUnavailable,
            p::Unavailable::DecisionRequired => Self::DecisionRequired,
            p::Unavailable::RecoveryRequired => Self::RecoveryRequired,
        }
    }
}
impl From<CloudPolicyUnavailable> for p::Unavailable {
    fn from(v: CloudPolicyUnavailable) -> Self {
        match v {
            CloudPolicyUnavailable::CollectionUnavailable => Self::CollectionUnavailable,
            CloudPolicyUnavailable::ReviewActive => Self::ReviewActive,
            CloudPolicyUnavailable::NotConnected => Self::NotConnected,
            CloudPolicyUnavailable::Paused => Self::Paused,
            CloudPolicyUnavailable::ICloudUnavailable => Self::ICloudUnavailable,
            CloudPolicyUnavailable::DecisionRequired => Self::DecisionRequired,
            CloudPolicyUnavailable::RecoveryRequired => Self::RecoveryRequired,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyUnavailableReason {
    NoAccount,
    Restricted,
    Temporary,
    SetupNeeded,
}
impl From<p::UnavailableReason> for CloudPolicyUnavailableReason {
    fn from(v: p::UnavailableReason) -> Self {
        match v {
            p::UnavailableReason::NoAccount => Self::NoAccount,
            p::UnavailableReason::Restricted => Self::Restricted,
            p::UnavailableReason::Temporary => Self::Temporary,
            p::UnavailableReason::SetupNeeded => Self::SetupNeeded,
        }
    }
}
impl From<CloudPolicyUnavailableReason> for p::UnavailableReason {
    fn from(v: CloudPolicyUnavailableReason) -> Self {
        match v {
            CloudPolicyUnavailableReason::NoAccount => Self::NoAccount,
            CloudPolicyUnavailableReason::Restricted => Self::Restricted,
            CloudPolicyUnavailableReason::Temporary => Self::Temporary,
            CloudPolicyUnavailableReason::SetupNeeded => Self::SetupNeeded,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyPausedReason {
    DecisionRequired,
    UserPaused,
    AutomaticOff,
}
impl From<p::PausedReason> for CloudPolicyPausedReason {
    fn from(v: p::PausedReason) -> Self {
        match v {
            p::PausedReason::DecisionRequired => Self::DecisionRequired,
            p::PausedReason::UserPaused => Self::UserPaused,
            p::PausedReason::AutomaticOff => Self::AutomaticOff,
        }
    }
}
impl From<CloudPolicyPausedReason> for p::PausedReason {
    fn from(v: CloudPolicyPausedReason) -> Self {
        match v {
            CloudPolicyPausedReason::DecisionRequired => Self::DecisionRequired,
            CloudPolicyPausedReason::UserPaused => Self::UserPaused,
            CloudPolicyPausedReason::AutomaticOff => Self::AutomaticOff,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyAccountStatus {
    Available,
    NoAccount,
    Restricted,
    Temporary,
    Configuration,
    Undetermined,
}
impl From<p::AccountStatus> for CloudPolicyAccountStatus {
    fn from(v: p::AccountStatus) -> Self {
        match v {
            p::AccountStatus::Available => Self::Available,
            p::AccountStatus::NoAccount => Self::NoAccount,
            p::AccountStatus::Restricted => Self::Restricted,
            p::AccountStatus::Temporary => Self::Temporary,
            p::AccountStatus::Configuration => Self::Configuration,
            p::AccountStatus::Undetermined => Self::Undetermined,
        }
    }
}
impl From<CloudPolicyAccountStatus> for p::AccountStatus {
    fn from(v: CloudPolicyAccountStatus) -> Self {
        match v {
            CloudPolicyAccountStatus::Available => Self::Available,
            CloudPolicyAccountStatus::NoAccount => Self::NoAccount,
            CloudPolicyAccountStatus::Restricted => Self::Restricted,
            CloudPolicyAccountStatus::Temporary => Self::Temporary,
            CloudPolicyAccountStatus::Configuration => Self::Configuration,
            CloudPolicyAccountStatus::Undetermined => Self::Undetermined,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyStopReason {
    GateClosed,
    TrackingFailed,
    BindingChanged,
}
impl From<p::StopReason> for CloudPolicyStopReason {
    fn from(v: p::StopReason) -> Self {
        match v {
            p::StopReason::GateClosed => Self::GateClosed,
            p::StopReason::TrackingFailed => Self::TrackingFailed,
            p::StopReason::BindingChanged => Self::BindingChanged,
        }
    }
}
impl From<CloudPolicyStopReason> for p::StopReason {
    fn from(v: CloudPolicyStopReason) -> Self {
        match v {
            CloudPolicyStopReason::GateClosed => Self::GateClosed,
            CloudPolicyStopReason::TrackingFailed => Self::TrackingFailed,
            CloudPolicyStopReason::BindingChanged => Self::BindingChanged,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyUpdate {
    Full,
    Changes,
}
impl From<p::Update> for CloudPolicyUpdate {
    fn from(v: p::Update) -> Self {
        match v {
            p::Update::Full => Self::Full,
            p::Update::Changes => Self::Changes,
        }
    }
}
impl From<CloudPolicyUpdate> for p::Update {
    fn from(v: CloudPolicyUpdate) -> Self {
        match v {
            CloudPolicyUpdate::Full => Self::Full,
            CloudPolicyUpdate::Changes => Self::Changes,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyAttention {
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
impl From<p::Attention> for CloudPolicyAttention {
    fn from(v: p::Attention) -> Self {
        match v {
            p::Attention::Isolated => Self::Isolated,
            p::Attention::RootUnreadable => Self::RootUnreadable,
            p::Attention::ChooseCollection => Self::ChooseCollection,
            p::Attention::Conflicts => Self::Conflicts,
            p::Attention::ConflictsUnreadable => Self::ConflictsUnreadable,
            p::Attention::Refused => Self::Refused,
            p::Attention::ReferenceWaiting => Self::ReferenceWaiting,
            p::Attention::Oversize => Self::Oversize,
            p::Attention::Unreadable => Self::Unreadable,
            p::Attention::Unsyncable => Self::Unsyncable,
            p::Attention::ClaimStuck => Self::ClaimStuck,
            p::Attention::GenerationIncomplete => Self::GenerationIncomplete,
            p::Attention::AssetUnavailable => Self::AssetUnavailable,
            p::Attention::ApplyFailed => Self::ApplyFailed,
            p::Attention::RekeyFailed => Self::RekeyFailed,
            p::Attention::CrtMismatch => Self::CrtMismatch,
            p::Attention::CollectionCorrupt => Self::CollectionCorrupt,
            p::Attention::MediaZoneMissing => Self::MediaZoneMissing,
            p::Attention::MediaChanged => Self::MediaChanged,
            p::Attention::MediaMissing => Self::MediaMissing,
            p::Attention::RecoveryOpWaiting => Self::RecoveryOpWaiting,
            p::Attention::ExchangeIncomplete => Self::ExchangeIncomplete,
            p::Attention::QuotaFull => Self::QuotaFull,
            p::Attention::LocalStorageFull => Self::LocalStorageFull,
            p::Attention::TrackingError => Self::TrackingError,
            p::Attention::UnsupportedSchema => Self::UnsupportedSchema,
            p::Attention::CloudError => Self::CloudError,
        }
    }
}
impl From<CloudPolicyAttention> for p::Attention {
    fn from(v: CloudPolicyAttention) -> Self {
        match v {
            CloudPolicyAttention::Isolated => Self::Isolated,
            CloudPolicyAttention::RootUnreadable => Self::RootUnreadable,
            CloudPolicyAttention::ChooseCollection => Self::ChooseCollection,
            CloudPolicyAttention::Conflicts => Self::Conflicts,
            CloudPolicyAttention::ConflictsUnreadable => Self::ConflictsUnreadable,
            CloudPolicyAttention::Refused => Self::Refused,
            CloudPolicyAttention::ReferenceWaiting => Self::ReferenceWaiting,
            CloudPolicyAttention::Oversize => Self::Oversize,
            CloudPolicyAttention::Unreadable => Self::Unreadable,
            CloudPolicyAttention::Unsyncable => Self::Unsyncable,
            CloudPolicyAttention::ClaimStuck => Self::ClaimStuck,
            CloudPolicyAttention::GenerationIncomplete => Self::GenerationIncomplete,
            CloudPolicyAttention::AssetUnavailable => Self::AssetUnavailable,
            CloudPolicyAttention::ApplyFailed => Self::ApplyFailed,
            CloudPolicyAttention::RekeyFailed => Self::RekeyFailed,
            CloudPolicyAttention::CrtMismatch => Self::CrtMismatch,
            CloudPolicyAttention::CollectionCorrupt => Self::CollectionCorrupt,
            CloudPolicyAttention::MediaZoneMissing => Self::MediaZoneMissing,
            CloudPolicyAttention::MediaChanged => Self::MediaChanged,
            CloudPolicyAttention::MediaMissing => Self::MediaMissing,
            CloudPolicyAttention::RecoveryOpWaiting => Self::RecoveryOpWaiting,
            CloudPolicyAttention::ExchangeIncomplete => Self::ExchangeIncomplete,
            CloudPolicyAttention::QuotaFull => Self::QuotaFull,
            CloudPolicyAttention::LocalStorageFull => Self::LocalStorageFull,
            CloudPolicyAttention::TrackingError => Self::TrackingError,
            CloudPolicyAttention::UnsupportedSchema => Self::UnsupportedSchema,
            CloudPolicyAttention::CloudError => Self::CloudError,
        }
    }
}

#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyDeviceSettings {
    pub enabled: bool,
    pub paused: bool,
    pub automatic: bool,
    pub media: bool,
}
impl From<CloudPolicyDeviceSettings> for p::DeviceSettings {
    fn from(v: CloudPolicyDeviceSettings) -> Self {
        Self {
            enabled: v.enabled,
            paused: v.paused,
            automatic: v.automatic,
            media: v.media,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyOperation {
    pub zone: CloudPolicyZone,
    pub kind: CloudPolicyOperationKind,
}
impl From<CloudPolicyOperation> for p::Operation {
    fn from(v: CloudPolicyOperation) -> Self {
        Self {
            zone: v.zone.into(),
            kind: v.kind.into(),
        }
    }
}
impl From<p::Operation> for CloudPolicyOperation {
    fn from(v: p::Operation) -> Self {
        Self {
            zone: v.zone.into(),
            kind: v.kind.into(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyActivity {
    Started,
    Ended { succeeded: bool, saved: u64 },
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyEvent {
    Refresh {
        cause: CloudPolicyChangeCause,
    },
    TrackingWriteFailed {
        no_space: bool,
    },
    Settings {
        settings: CloudPolicyDeviceSettings,
    },
    Network {
        reachable: bool,
    },
    Foreground,
    Background {
        deadline_ms: i64,
    },
    BackgroundExpiring,
    QuitRequested,
    FlushTimerFired,
    SyncNowRequested {
        source: CloudPolicySource,
    },
    SyncNowCancelled {
        exchange: u64,
    },
    VolumeSpaceAvailable,
    VisibleMedia {
        names: Vec<String>,
    },
    Operation {
        instance: String,
        operation: CloudPolicyOperation,
        phase: CloudPolicyActivity,
    },
    OperationError {
        instance: String,
        operation: CloudPolicyOperation,
        class: CloudPolicyOperationError,
        code: i64,
        retry_after_ms: Option<i64>,
    },
}
impl From<CloudPolicyEvent> for p::DeviceEvent {
    fn from(v: CloudPolicyEvent) -> Self {
        match v {
            CloudPolicyEvent::Refresh { cause } => Self::Refresh(cause.into()),
            CloudPolicyEvent::TrackingWriteFailed { no_space } => Self::TrackingWriteFailed { no_space },
            CloudPolicyEvent::Settings { settings } => Self::Settings(settings.into()),
            CloudPolicyEvent::Network { reachable } => Self::Network { reachable },
            CloudPolicyEvent::Foreground => Self::Foreground,
            CloudPolicyEvent::Background { deadline_ms } => Self::Background { deadline_ms },
            CloudPolicyEvent::BackgroundExpiring => Self::BackgroundExpiring,
            CloudPolicyEvent::QuitRequested => Self::QuitRequested,
            CloudPolicyEvent::FlushTimerFired => Self::FlushTimerFired,
            CloudPolicyEvent::SyncNowRequested { source } => Self::SyncNowRequested(source.into()),
            CloudPolicyEvent::SyncNowCancelled { exchange } => Self::SyncNowCancelled { exchange },
            CloudPolicyEvent::VolumeSpaceAvailable => Self::VolumeSpaceAvailable,
            CloudPolicyEvent::VisibleMedia { names } => Self::VisibleMedia { names },
            CloudPolicyEvent::Operation {
                instance,
                operation,
                phase,
            } => Self::Operation {
                instance,
                operation: operation.into(),
                phase: match phase {
                    CloudPolicyActivity::Started => p::Activity::Started,
                    CloudPolicyActivity::Ended { succeeded, saved } => p::Activity::Ended {
                        outcome: if succeeded { p::Outcome::Ok } else { p::Outcome::Failed },
                        saved,
                    },
                },
            },
            CloudPolicyEvent::OperationError {
                instance,
                operation,
                class,
                code,
                retry_after_ms,
            } => Self::OperationError {
                instance,
                operation: operation.into(),
                class: class.into(),
                code,
                retry_after_ms,
            },
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyMediaFacts {
    pub upload_files: Option<u64>,
    pub upload_bytes: Option<u64>,
    pub download_files: Option<u64>,
    pub download_bytes: Option<u64>,
    pub unknown_blobs: Option<u64>,
    pub missing_names: Option<u64>,
    pub uploads_running: Option<u64>,
    pub downloads_running: Option<u64>,
    pub remaining_download_ms: Option<u64>,
    pub root_acked: Option<bool>,
    pub zone_missing: Option<bool>,
    pub no_space: Option<bool>,
}
impl From<p::MediaFacts> for CloudPolicyMediaFacts {
    fn from(v: p::MediaFacts) -> Self {
        Self {
            upload_files: v.upload_files,
            upload_bytes: v.upload_bytes,
            download_files: v.download_files,
            download_bytes: v.download_bytes,
            unknown_blobs: v.unknown_blobs,
            missing_names: v.missing_names,
            uploads_running: v.uploads_running,
            downloads_running: v.downloads_running,
            remaining_download_ms: v.remaining_download_ms,
            root_acked: v.root_acked,
            zone_missing: v.zone_missing,
            no_space: v.no_space,
        }
    }
}

#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyExchangeOutcome {
    Succeeded {
        selected: u64,
        held_back: u64,
        newer_pending: u64,
        incoming_waiting: u64,
        media: CloudPolicyMediaFacts,
    },
    Incomplete {
        outstanding: u64,
        blocked: u64,
    },
    Waiting {
        class: CloudPolicyOperationError,
        retry_after_ms: Option<i64>,
    },
    Failed {
        class: CloudPolicyOperationError,
    },
    Cancelled,
}
impl From<p::ExchangeOutcome> for CloudPolicyExchangeOutcome {
    fn from(v: p::ExchangeOutcome) -> Self {
        match v {
            p::ExchangeOutcome::Succeeded {
                selected,
                held_back,
                newer_pending,
                incoming_waiting,
                media,
            } => Self::Succeeded {
                selected,
                held_back,
                newer_pending,
                incoming_waiting,
                media: media.into(),
            },
            p::ExchangeOutcome::Incomplete { outstanding, blocked } => Self::Incomplete { outstanding, blocked },
            p::ExchangeOutcome::Waiting { class, retry_after_ms } => Self::Waiting {
                class: class.into(),
                retry_after_ms,
            },
            p::ExchangeOutcome::Failed { class } => Self::Failed { class: class.into() },
            p::ExchangeOutcome::Cancelled => Self::Cancelled,
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyEarlyEnd {
    Waiting {
        class: CloudPolicyOperationError,
        retry_after_ms: Option<i64>,
    },
    Failed {
        class: CloudPolicyOperationError,
    },
    Cancelled,
}
impl From<CloudPolicyEarlyEnd> for p::ExchangeOutcome {
    fn from(v: CloudPolicyEarlyEnd) -> Self {
        match v {
            CloudPolicyEarlyEnd::Waiting { class, retry_after_ms } => Self::Waiting {
                class: class.into(),
                retry_after_ms,
            },
            CloudPolicyEarlyEnd::Failed { class } => Self::Failed { class: class.into() },
            CloudPolicyEarlyEnd::Cancelled => Self::Cancelled,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyExchangeRecord {
    pub exchange: u64,
    pub ended_ms: i64,
    pub source: CloudPolicySource,
    pub outcome: CloudPolicyExchangeOutcome,
}
impl From<p::ExchangeRecord> for CloudPolicyExchangeRecord {
    fn from(v: p::ExchangeRecord) -> Self {
        Self {
            exchange: v.exchange,
            ended_ms: v.ended_ms,
            source: v.source.into(),
            outcome: v.outcome.into(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyStatusState {
    LocalOnly,
    ICloudUnavailable { reason: CloudPolicyUnavailableReason },
    Paused { reason: CloudPolicyPausedReason },
    NeedsAttention,
    Offline,
    SyncingCollection,
    SyncingMedia,
    ChangesPending,
    UpToDate { as_of_ms: i64 },
    AwaitingEvidence,
}
impl From<p::StatusState> for CloudPolicyStatusState {
    fn from(v: p::StatusState) -> Self {
        match v {
            p::StatusState::ICloudUnavailable(reason) => Self::ICloudUnavailable { reason: reason.into() },
            p::StatusState::Paused(reason) => Self::Paused { reason: reason.into() },
            p::StatusState::UpToDate { as_of_ms } => Self::UpToDate { as_of_ms },
            p::StatusState::LocalOnly => Self::LocalOnly,
            p::StatusState::NeedsAttention => Self::NeedsAttention,
            p::StatusState::Offline => Self::Offline,
            p::StatusState::SyncingCollection => Self::SyncingCollection,
            p::StatusState::SyncingMedia => Self::SyncingMedia,
            p::StatusState::ChangesPending => Self::ChangesPending,
            p::StatusState::AwaitingEvidence => Self::AwaitingEvidence,
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicySyncNowAvailability {
    Available,
    Unavailable { reason: CloudPolicyUnavailable },
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyAttentionCount {
    pub kind: CloudPolicyAttention,
    pub count: u64,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyErrorDetail {
    pub class: CloudPolicyOperationError,
    pub code: i64,
    pub retry_after_ms: Option<i64>,
    pub operation: CloudPolicyOperation,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CloudPolicyEnableControl {
    ResumePause,
    DecisionRequired,
    Connect,
    Pause,
    AwaitingResumeOffer,
}
impl From<p::EnableControl> for CloudPolicyEnableControl {
    fn from(v: p::EnableControl) -> Self {
        match v {
            p::EnableControl::ResumePause => Self::ResumePause,
            p::EnableControl::DecisionRequired => Self::DecisionRequired,
            p::EnableControl::Connect => Self::Connect,
            p::EnableControl::Pause => Self::Pause,
            p::EnableControl::AwaitingResumeOffer => Self::AwaitingResumeOffer,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyControls {
    pub enable: CloudPolicyEnableControl,
    pub binding_usable: bool,
    pub binding_active: bool,
    pub binding_stopped: bool,
    pub decision_required: bool,
    pub user_paused: bool,
    pub resume_binding_offered: Option<bool>,
    pub automatic_eligible: bool,
}
impl From<p::Controls> for CloudPolicyControls {
    fn from(v: p::Controls) -> Self {
        Self {
            enable: v.enable.into(),
            binding_usable: v.binding_usable,
            binding_active: v.binding_active,
            binding_stopped: v.binding_stopped,
            decision_required: v.decision_required,
            user_paused: v.user_paused,
            resume_binding_offered: v.resume_binding_offered,
            automatic_eligible: v.automatic_eligible,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPolicyStatus {
    pub state: CloudPolicyStatusState,
    pub pending: u64,
    pub held_back: u64,
    pub in_flight: u64,
    pub incoming_waiting: u64,
    pub attention: Vec<CloudPolicyAttentionCount>,
    pub media: CloudPolicyMediaFacts,
    pub media_paused: bool,
    pub error: Option<CloudPolicyErrorDetail>,
    pub account_status: CloudPolicyAccountStatus,
    pub account_matches_binding: bool,
    pub as_of_ms: Option<i64>,
    pub last_completed: Option<CloudPolicyExchangeRecord>,
    pub last_attempt: Option<CloudPolicyExchangeRecord>,
    pub sync_now: CloudPolicySyncNowAvailability,
    pub controls: CloudPolicyControls,
}
impl From<p::Status> for CloudPolicyStatus {
    fn from(v: p::Status) -> Self {
        Self {
            state: v.state.into(),
            controls: v.controls.into(),
            pending: v.pending,
            held_back: v.held_back,
            in_flight: v.in_flight,
            incoming_waiting: v.incoming_waiting,
            attention: v
                .attention
                .into_iter()
                .map(|(kind, count)| CloudPolicyAttentionCount {
                    kind: kind.into(),
                    count,
                })
                .collect(),
            media: v.media.into(),
            media_paused: v.media_paused,
            error: v.error.map(|e| CloudPolicyErrorDetail {
                class: e.class.into(),
                code: e.code,
                retry_after_ms: e.retry_after_ms,
                operation: e.operation.into(),
            }),
            account_status: v.account.status.into(),
            account_matches_binding: v.account_matches_binding,
            as_of_ms: v.evidence.as_of_ms(),
            last_completed: v.evidence.last_completed.map(Into::into),
            last_attempt: v.evidence.last_attempt.map(Into::into),
            sync_now: match v.sync_now {
                Ok(()) => CloudPolicySyncNowAvailability::Available,
                Err(reason) => CloudPolicySyncNowAvailability::Unavailable { reason: reason.into() },
            },
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudPolicyAction {
    StartEngine { automatic: bool },
    StopEngine { reason: CloudPolicyStopReason },
    ConfigureAutomatic { on: bool },
    UpdatePending { update: CloudPolicyUpdate },
    RequestFetch { zones: Vec<CloudPolicyZone> },
    RequestSend { zones: Vec<CloudPolicyZone> },
    ReadBlobs { include_missing: bool },
    RetryAssetEntries,
    RunCleanup,
    DeleteZones,
    ArmFlushTimer { at_ms: i64 },
    CancelFlushTimer,
    RunExchange { exchange: u64, source: CloudPolicySource },
    CancelExchange { exchange: u64 },
    BackgroundFlush { until_ms: i64 },
    CancelBackgroundFlush,
    QuitFlush { until_ms: i64 },
    QuitNow,
    PauseMedia,
    ResumeMedia,
    PrioritizeMedia { names: Vec<String> },
    ApplyReady { retry_failed: bool },
    RecordExchange { record: CloudPolicyExchangeRecord },
    RecordQuietPoint { at_ms: i64 },
    ProbeTracking,
    StatusChanged { status: Box<CloudPolicyStatus> },
}
impl From<&p::ActionKind> for CloudPolicyAction {
    fn from(v: &p::ActionKind) -> Self {
        match v.clone() {
            p::ActionKind::StartEngine { automatic, .. } => Self::StartEngine { automatic },
            p::ActionKind::StopEngine { reason } => Self::StopEngine { reason: reason.into() },
            p::ActionKind::ConfigureAutomatic { on } => Self::ConfigureAutomatic { on },
            p::ActionKind::UpdatePending(update) => Self::UpdatePending { update: update.into() },
            p::ActionKind::RequestFetch { zones } => Self::RequestFetch {
                zones: zones.into_iter().map(Into::into).collect(),
            },
            p::ActionKind::RequestSend { zones } => Self::RequestSend {
                zones: zones.into_iter().map(Into::into).collect(),
            },
            p::ActionKind::ReadBlobs { include_missing } => Self::ReadBlobs { include_missing },
            p::ActionKind::ArmFlushTimer { at_ms } => Self::ArmFlushTimer { at_ms },
            p::ActionKind::RunExchange { exchange, source } => Self::RunExchange {
                exchange,
                source: source.into(),
            },
            p::ActionKind::CancelExchange { exchange } => Self::CancelExchange { exchange },
            p::ActionKind::BackgroundFlush { until_ms } => Self::BackgroundFlush { until_ms },
            p::ActionKind::QuitFlush { until_ms } => Self::QuitFlush { until_ms },
            p::ActionKind::PrioritizeMedia { names } => Self::PrioritizeMedia { names },
            p::ActionKind::ApplyReady { retry_failed } => Self::ApplyReady { retry_failed },
            p::ActionKind::RecordExchange(record) => Self::RecordExchange { record: record.into() },
            p::ActionKind::RecordQuietPoint { at_ms } => Self::RecordQuietPoint { at_ms },
            p::ActionKind::StatusChanged(status) => Self::StatusChanged {
                status: Box::new((*status).into()),
            },
            p::ActionKind::RetryAssetEntries => Self::RetryAssetEntries,
            p::ActionKind::RunCleanup => Self::RunCleanup,
            p::ActionKind::DeleteZones => Self::DeleteZones,
            p::ActionKind::CancelFlushTimer => Self::CancelFlushTimer,
            p::ActionKind::CancelBackgroundFlush => Self::CancelBackgroundFlush,
            p::ActionKind::QuitNow => Self::QuitNow,
            p::ActionKind::PauseMedia => Self::PauseMedia,
            p::ActionKind::ResumeMedia => Self::ResumeMedia,
            p::ActionKind::ProbeTracking => Self::ProbeTracking,
        }
    }
}

#[derive(uniffi::Object)]
pub struct CloudPolicy {
    host: Arc<meguri_host::CoreHost>,
    generation: u64,
}
#[derive(uniffi::Object)]
pub struct CloudPolicyCommand {
    owner: Arc<CloudPolicy>,
    // The ORIGINAL command, including its private issued backend_guard.
    command: p::Command,
}
#[cfg(any(test, feature = "test-hooks"))]
impl CloudPolicyCommand {
    #[cfg(feature = "test-hooks")]
    pub(crate) fn original_exchange_witness_inner(&self) -> Result<Vec<u8>, CoreError> {
        self.owner.owns(self)?;
        let collection = self.owner.collection()?;
        self.owner
            .driver()?
            .original_exchange_witness_for_testing(&self.command, &collection)
            .map(|v| v.to_vec())
            .map_err(|_| failure(CloudPolicyFailure::StaleCommand))
    }
    #[cfg(feature = "test-hooks")]
    pub(crate) fn original_exchange_capture_inner(
        &self,
        runtime_exchange: u64,
    ) -> Result<(u64, Vec<String>), CoreError> {
        self.owner.owns(self)?;
        self.owner.collection()?;
        self.owner
            .driver()?
            .original_exchange_capture_for_testing(&self.command, runtime_exchange)
            .map_err(|_| failure(CloudPolicyFailure::StaleCommand))
    }
    pub(crate) fn exchange_observation_for_testing(&self) -> Result<p::ExchangeObservation, CoreError> {
        self.owner.owns(self)?;
        self.owner.collection()?;
        self.owner
            .driver()?
            .exchange_observation_for_testing(&self.command)
            .map_err(|error| crate::sync::cloud_error(error.to_string()))
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudPolicyIssuedCommand {
    pub handle: Arc<CloudPolicyCommand>,
    pub action: CloudPolicyAction,
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudPolicyOutput {
    Commands {
        commands: Vec<CloudPolicyIssuedCommand>,
    },
    CoreActionFailed {
        error: CoreError,
        commands: Vec<CloudPolicyIssuedCommand>,
    },
    ApplyDeferred {
        error: CoreError,
    },
    SyncNowUnavailable {
        reason: CloudPolicyUnavailable,
    },
}
#[derive(Clone, uniffi::Enum)]
pub enum CloudPolicyAdmission {
    Admitted { accepted: bool },
    NoOperation,
    PolicyOutput { output: CloudPolicyOutput },
}
#[derive(Clone, uniffi::Record)]
pub struct CloudPolicyCapture {
    pub runtime_exchange: Option<u64>,
    pub output: CloudPolicyOutput,
}
/// Synchronous admission of ALREADY PREPARED native control work. Never await,
/// perform UI/provider work, or reenter core here. Await SDK replies after return.
#[uniffi::export(foreign)]
pub trait CloudPolicyQueue: Send + Sync {
    fn enqueue(&self) -> bool;
}

// One opaque facade per live original Host/generation; weak entries never keep
// a Host alive. The Host owns the Driver; this registry never creates a Driver.
static POLICIES: Mutex<Vec<Weak<CloudPolicy>>> = Mutex::new(Vec::new());
impl CloudPolicy {
    pub(crate) fn for_workflow(workflow: &CloudWorkflow) -> Result<Arc<Self>, CoreError> {
        let generation = workflow.host.cloud_connection_generation()?.0;
        if workflow.host.cloud_policy_driver().is_none() {
            return Err(failure(CloudPolicyFailure::CoreCapabilityUnavailable));
        }
        let mut policies = POLICIES.lock().unwrap_or_else(PoisonError::into_inner);
        policies.retain(|p| p.strong_count() > 0);
        if let Some(policy) = policies
            .iter()
            .filter_map(Weak::upgrade)
            .find(|p| Arc::ptr_eq(&p.host, &workflow.host) && p.generation == generation)
        {
            return Ok(policy);
        }
        let policy = Arc::new(Self {
            host: workflow.host.clone(),
            generation,
        });
        policies.push(Arc::downgrade(&policy));
        Ok(policy)
    }
    fn validate_owner(&self) -> Result<(), CoreError> {
        let generation = self
            .host
            .cloud_connection_generation()
            .map_err(|_| failure(CloudPolicyFailure::Closed))?;
        if generation.0 != self.generation {
            return Err(failure(CloudPolicyFailure::GenerationChanged));
        }
        Ok(())
    }
    fn collection(&self) -> Result<meguri_engine::Collection, CoreError> {
        self.validate_owner()?;
        let collection = self
            .host
            .cloud_connection_collection()
            .map_err(|_| failure(CloudPolicyFailure::Closed))?;
        if collection.generation().0 != self.generation || !collection.is_current() {
            return Err(failure(CloudPolicyFailure::GenerationChanged));
        }
        Ok(collection)
    }
    fn driver(&self) -> Result<&p::Driver, CoreError> {
        self.host
            .cloud_policy_driver()
            .ok_or_else(|| failure(CloudPolicyFailure::Closed))
    }
    fn issued(self: &Arc<Self>, commands: Vec<p::Command>) -> Vec<CloudPolicyIssuedCommand> {
        commands
            .into_iter()
            .map(|command| CloudPolicyIssuedCommand {
                action: (&command.action.kind).into(),
                handle: Arc::new(CloudPolicyCommand {
                    owner: self.clone(),
                    command,
                }),
            })
            .collect()
    }
    pub(crate) fn output(
        self: &Arc<Self>,
        result: Result<Vec<p::Command>, p::DriverError>,
    ) -> Result<CloudPolicyOutput, CoreError> {
        match result {
            Ok(commands) => Ok(CloudPolicyOutput::Commands {
                commands: self.issued(commands),
            }),
            Err(p::DriverError::CoreActionFailed { error, commands }) => Ok(CloudPolicyOutput::CoreActionFailed {
                error: crate::cloud_connection::error(error),
                commands: self.issued(commands),
            }),
            Err(p::DriverError::ApplyDeferred(error)) => Ok(CloudPolicyOutput::ApplyDeferred { error: error.into() }),
            Err(p::DriverError::Unavailable(reason)) => {
                Ok(CloudPolicyOutput::SyncNowUnavailable { reason: reason.into() })
            }
            Err(p::DriverError::Runtime(error)) => Err(crate::cloud_connection::error(error)),
            Err(p::DriverError::StaleCommand) => Err(failure(CloudPolicyFailure::StaleCommand)),
            Err(p::DriverError::NotCoreAction) => Err(failure(CloudPolicyFailure::NotCoreAction)),
            Err(p::DriverError::CoreCapabilityUnavailable(_)) => {
                Err(failure(CloudPolicyFailure::CoreCapabilityUnavailable))
            }
            Err(p::DriverError::UnqualifiedCompletion) => Err(failure(CloudPolicyFailure::UnqualifiedCompletion)),
        }
    }
    fn owns(&self, command: &CloudPolicyCommand) -> Result<(), CoreError> {
        if !std::ptr::eq(self, Arc::as_ptr(&command.owner)) {
            return Err(failure(CloudPolicyFailure::WrongOwner));
        }
        self.validate_owner()?;
        if command.command.action.guard.generation != self.generation {
            return Err(failure(CloudPolicyFailure::GenerationChanged));
        }
        Ok(())
    }
}
#[uniffi::export]
impl CloudWorkflow {
    pub fn policy(&self) -> Result<Arc<CloudPolicy>, CoreError> {
        callback::enter("CloudWorkflow.policy");
        CloudPolicy::for_workflow(self)
    }
}
#[uniffi::export]
impl CloudPolicy {
    /// Validate only the retained original Host's closed state and generation.
    /// This metadata check remains available while ordinary jobs are suspended.
    pub fn validate_original_owner(&self) -> Result<(), CoreError> {
        callback::enter("CloudPolicy.validate_original_owner");
        self.validate_owner()
    }
    pub fn input(self: Arc<Self>, event: CloudPolicyEvent) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.input");
        self.input_with_work(event, &mut p::PolicyWork::default())
    }
    /// Samples the original Runtime's platform provider through its guarded
    /// identity-read workflow. No UI account string is accepted.
    pub fn refresh_account(self: Arc<Self>) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.refresh_account");
        self.refresh_account_with_work(&mut p::PolicyWork::default())
    }
    /// Project an already accepted read from this original Host/Runtime. The
    /// request is a lookup key, never frontend-supplied identity authority.
    pub fn project_identity_read(self: Arc<Self>, request: u64) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.project_identity_read");
        self.project_identity_read_with_work(request, &mut p::PolicyWork::default())
    }
    /// Read-only: lifecycle/session/tracking changes enter through Refresh,
    /// never by supplying CoreFacts or generation in a DTO.
    pub fn status(&self) -> Result<CloudPolicyStatus, CoreError> {
        callback::enter("CloudPolicy.status");
        self.status_with_work(&mut p::PolicyWork::default())
    }
    pub fn execute_core(self: Arc<Self>, command: Arc<CloudPolicyCommand>) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.execute_core");
        self.execute_core_with_work(command, &mut p::PolicyWork::default())
    }
    /// Revalidation is the last step before the synchronous native enqueue,
    /// with no await/provider call/lock held across the foreign callback.
    /// This acknowledges admission only; it does not report execution/success.
    pub fn submit_native(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        queue: Arc<dyn CloudPolicyQueue>,
    ) -> Result<CloudPolicyAdmission, CoreError> {
        callback::enter("CloudPolicy.submit_native");
        self.submit_native_with_work(command, queue, &mut p::PolicyWork::default())
    }
    pub fn capture_exchange(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
    ) -> Result<CloudPolicyCapture, CoreError> {
        callback::enter("CloudPolicy.capture_exchange");
        self.capture_exchange_with_work(command, &mut p::PolicyWork::default())
    }
    pub fn complete_exchange(self: Arc<Self>, exchange: u64) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.complete_exchange");
        self.complete_exchange_with_work(exchange, &mut p::PolicyWork::default())
    }
    pub fn end_before_capture(
        self: Arc<Self>,
        exchange: u64,
        outcome: CloudPolicyEarlyEnd,
    ) -> Result<CloudPolicyOutput, CoreError> {
        callback::enter("CloudPolicy.end_before_capture");
        self.end_before_capture_with_work(exchange, outcome, &mut p::PolicyWork::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_projection_preserves_counts_optional_media_and_qualified_evidence() {
        let mut raw = p::State::default().status();
        raw.state = p::StatusState::AwaitingEvidence;
        raw.pending = 7;
        raw.held_back = 11;
        raw.in_flight = 13;
        raw.incoming_waiting = 17;
        raw.media.upload_files = Some(19);
        raw.media.download_files = None;
        raw.attention.insert(p::Attention::ApplyFailed, 23);
        raw.evidence.quiet_ms = Some(29);
        raw.sync_now = Err(p::Unavailable::ReviewActive);
        raw.controls.enable = p::EnableControl::Pause;
        raw.controls.binding_usable = true;
        raw.controls.binding_active = true;
        raw.controls.automatic_eligible = true;
        let dto = CloudPolicyStatus::from(raw);
        assert!(matches!(dto.state, CloudPolicyStatusState::AwaitingEvidence));
        assert_eq!(
            (dto.pending, dto.held_back, dto.in_flight, dto.incoming_waiting),
            (7, 11, 13, 17)
        );
        assert_eq!(dto.controls.enable, CloudPolicyEnableControl::Pause);
        assert!(dto.controls.binding_usable && dto.controls.binding_active);
        assert!(dto.controls.automatic_eligible);
        assert_eq!(dto.controls.resume_binding_offered, None);
        assert_eq!(dto.media.upload_files, Some(19));
        assert!(dto.media.download_files.is_none());
        assert_eq!(dto.as_of_ms, Some(29));
        assert!(
            dto.attention
                .iter()
                .any(|a| a.kind == CloudPolicyAttention::ApplyFailed && a.count == 23)
        );
        assert!(matches!(
            dto.sync_now,
            CloudPolicySyncNowAvailability::Unavailable {
                reason: CloudPolicyUnavailable::ReviewActive
            }
        ));
    }
}

#[path = "cloud_policy_execution.rs"]
mod execution;
pub use execution::{CloudPolicyExecution, CloudPolicyOperationFacts, CloudPolicyReadiness};

pub(crate) use execution::operation_error_class;

#[path = "cloud_policy_jobs.rs"]
pub(crate) mod policy_jobs;
pub use policy_jobs::*;

impl CloudPolicy {
    fn input_with_work(
        self: Arc<Self>,
        event: CloudPolicyEvent,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        self.validate_owner()?;
        let result = self.host.cloud_policy_input_with_work(event.into(), work);
        self.output(result)
    }
    fn refresh_account_with_work(self: Arc<Self>, work: &mut p::PolicyWork) -> Result<CloudPolicyOutput, CoreError> {
        self.validate_owner()?;
        let sampled = self
            .host
            .cloud_runtime()
            .ok_or_else(|| failure(CloudPolicyFailure::Closed))?
            .policy_account_identity_with_work(work);
        self.validate_owner()?;
        let (request, identity) = sampled.map_err(|error| match error {
            meguri_sync::cloud::runtime::RuntimeError::StaleIncarnation => failure(CloudPolicyFailure::StaleCommand),
            error => crate::cloud_connection::error(error),
        })?;
        let result = self
            .host
            .cloud_policy_input_with_work(p::DeviceEvent::IdentityRead { request, identity }, work);
        self.output(result)
    }
    fn project_identity_read_with_work(
        self: Arc<Self>,
        request: u64,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        self.validate_owner()?;
        let result = self
            .host
            .cloud_policy_input_with_work(p::DeviceEvent::AcceptedIdentityRead { request }, work);
        // Preserve Closed/GenerationChanged precedence over stale proof.
        self.validate_owner()?;
        let result = result.map_err(|error| match error {
            p::DriverError::Runtime(meguri_sync::cloud::runtime::RuntimeError::StaleIncarnation) => {
                p::DriverError::StaleCommand
            }
            error => error,
        });
        self.output(result)
    }
    fn status_with_work(&self, work: &mut p::PolicyWork) -> Result<CloudPolicyStatus, CoreError> {
        self.validate_owner()?;
        let status = self
            .driver()?
            .status_with_work(work)
            .map_err(execution::execution_error)?;
        work.check().map_err(crate::cloud_connection::error)?;
        self.validate_owner()?;
        Ok(status.into())
    }
    fn execute_core_with_work(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        self.owns(&command)?;
        let result = self
            .host
            .execute_cloud_policy_core_with_work(&command.command, Duration::from_secs(1), work);
        self.output(result)
    }
    fn submit_native_with_work(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        queue: Arc<dyn CloudPolicyQueue>,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyAdmission, CoreError> {
        self.owns(&command)?;
        match &command.command.action.kind {
            // These actions use dedicated retained execution/result ports.
            // General queue admission cannot substitute for those capabilities.
            p::ActionKind::RunCleanup
            | p::ActionKind::ReadBlobs { .. }
            | p::ActionKind::RetryAssetEntries
            | p::ActionKind::ResumeMedia
            | p::ActionKind::DeleteZones => {
                return Err(failure(CloudPolicyFailure::CoreCapabilityUnavailable));
            }
            p::ActionKind::UpdatePending(_)
            | p::ActionKind::ApplyReady { .. }
            | p::ActionKind::RecordExchange(_)
            | p::ActionKind::RecordQuietPoint { .. }
            | p::ActionKind::ProbeTracking
            | p::ActionKind::StatusChanged(_) => {
                return Err(failure(CloudPolicyFailure::NotNativeAction));
            }
            _ => {}
        }
        match self.driver()?.native_disposition_with_work(&command.command, work) {
            Ok(p::NativeDisposition::NoOperation) => return Ok(CloudPolicyAdmission::NoOperation),
            Ok(p::NativeDisposition::Enqueue) => {}
            Err(error) => {
                return Ok(CloudPolicyAdmission::PolicyOutput {
                    output: self.output(Err(error))?,
                });
            }
        }
        work.effect().map_err(crate::cloud_connection::error)?;
        Ok(CloudPolicyAdmission::Admitted {
            accepted: callback::invoke("CloudPolicyQueue.enqueue", || queue.enqueue()),
        })
    }
    fn capture_exchange_with_work(
        self: Arc<Self>,
        command: Arc<CloudPolicyCommand>,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyCapture, CoreError> {
        self.owns(&command)?;
        let collection = self.collection()?;
        match self
            .driver()?
            .capture_exchange_with_work(&command.command, &collection, work)
        {
            Ok(id) => Ok(CloudPolicyCapture {
                runtime_exchange: Some(id),
                output: CloudPolicyOutput::Commands { commands: Vec::new() },
            }),
            Err(error) => Ok(CloudPolicyCapture {
                runtime_exchange: None,
                output: self.output(Err(error))?,
            }),
        }
    }
    fn complete_exchange_with_work(
        self: Arc<Self>,
        exchange: u64,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        self.collection()?;
        let engine = self.host.engine();
        self.output(
            self.driver()?
                .complete_exchange_with_work(exchange, engine.state(), engine.generation(), work),
        )
    }
    fn end_before_capture_with_work(
        self: Arc<Self>,
        exchange: u64,
        outcome: CloudPolicyEarlyEnd,
        work: &mut p::PolicyWork,
    ) -> Result<CloudPolicyOutput, CoreError> {
        self.collection()?;
        let engine = self.host.engine();
        self.output(self.driver()?.end_before_capture_with_work(
            exchange,
            outcome.into(),
            engine.state(),
            engine.generation(),
            work,
        ))
    }
}
