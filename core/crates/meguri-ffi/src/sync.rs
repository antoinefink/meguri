//! Typed cloud bridge. Each object retains its original host and tracking owner;
//! an inspection after close cannot acquire or mutate a successor host.
use crate::{CoreError, callback, runtime};
use meguri_sync::cloud::{self as c, adapter as a, runtime as r, workflow as w};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex, PoisonError},
};

pub(crate) fn cloud_error(error: impl std::fmt::Display) -> CoreError {
    CoreError::Unavailable {
        reason: error.to_string(),
    }
}
#[derive(Clone, Debug, Default, uniffi::Record)]
pub struct CloudRecord {
    pub name: String,
    pub envelope: Vec<u8>,
    pub system_fields: Option<Vec<u8>>,
    pub record_type: Option<String>,
    pub zone: Option<String>,
    pub asset_path: Option<String>,
}
impl CloudRecord {
    pub(crate) fn fetched(self) -> w::FetchedRecord {
        let asset_path = self.asset_path.as_ref().map(std::path::PathBuf::from);
        w::FetchedRecord {
            record: self.saved(),
            asset_path,
        }
    }
    fn saved(self) -> w::Saved {
        w::Saved {
            name: self.name,
            envelope: self.envelope,
            system_fields: self.system_fields.unwrap_or_default(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudGuard {
    pub generation: u64,
    pub binding: Vec<u8>,
    pub token: u64,
}
impl From<a::Guard> for CloudGuard {
    fn from(v: a::Guard) -> Self {
        Self {
            generation: v.generation,
            binding: v.binding,
            token: v.token,
        }
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudPendingChange {
    Save,
    Delete,
}
impl From<CloudPendingChange> for w::PendingChange {
    fn from(v: CloudPendingChange) -> Self {
        match v {
            CloudPendingChange::Save => Self::Save,
            CloudPendingChange::Delete => Self::Delete,
        }
    }
}
impl From<w::PendingChange> for CloudPendingChange {
    fn from(v: w::PendingChange) -> Self {
        match v {
            w::PendingChange::Save => Self::Save,
            w::PendingChange::Delete => Self::Delete,
        }
    }
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudUpdateKind {
    Full,
    Changes,
}
impl From<CloudUpdateKind> for w::UpdateKind {
    fn from(v: CloudUpdateKind) -> Self {
        match v {
            CloudUpdateKind::Full => Self::Full,
            CloudUpdateKind::Changes => Self::Changes,
        }
    }
}
impl From<w::UpdateKind> for CloudUpdateKind {
    fn from(v: w::UpdateKind) -> Self {
        match v {
            w::UpdateKind::Full => Self::Full,
            w::UpdateKind::Changes => Self::Changes,
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudPendingUpdate {
    pub id: u64,
    pub kind: CloudUpdateKind,
    pub adds: HashMap<String, CloudPendingChange>,
    pub removes: Vec<String>,
    pub forced_additions: Vec<String>,
}
impl From<w::PendingUpdate> for CloudPendingUpdate {
    fn from(v: w::PendingUpdate) -> Self {
        Self {
            id: v.id,
            kind: v.kind.into(),
            adds: v.adds.into_iter().map(|(n, v)| (n, v.into())).collect(),
            removes: v.removes.into_iter().collect(),
            forced_additions: v.forced_additions.into_iter().collect(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudFlight {
    pub local_rev: u64,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDirtyMark {
    pub source: u8,
    pub change_seq: u64,
    pub payload_digest: Vec<u8>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudEntityFacts {
    pub name: String,
    pub local_rev: u64,
    pub acked_rev: u64,
    pub mark: Option<CloudDirtyMark>,
    pub in_flight: Vec<CloudFlight>,
    pub payload_digest: Vec<u8>,
    pub acked_payload_digest: Vec<u8>,
    pub acked_system_fields: Vec<u8>,
}
impl TryFrom<w::EntityFacts> for CloudEntityFacts {
    type Error = CoreError;
    fn try_from(v: w::EntityFacts) -> Result<Self, CoreError> {
        Ok(Self {
            name: v.name,
            local_rev: v.local_rev,
            acked_rev: v.acked_rev,
            mark: v.mark.map(|m| CloudDirtyMark {
                source: match m.source {
                    r::DirtySource::Observer => 0,
                    r::DirtySource::Snapshot => 1,
                },
                change_seq: m.change_seq,
                payload_digest: m.payload_digest.to_vec(),
            }),
            in_flight: v
                .in_flight
                .into_iter()
                .map(|f| CloudFlight { local_rev: f.local_rev })
                .collect(),
            payload_digest: v.payload.payload_digest().map_err(cloud_error)?.to_vec(),
            acked_payload_digest: v.acked_payload_digest.to_vec(),
            acked_system_fields: v.acked_system_fields,
        })
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudDurableSnapshot {
    pub binding: Vec<u8>,
    pub serialization: Option<Vec<u8>>,
    pub entities: Vec<CloudEntityFacts>,
    pub inbox_names: Vec<String>,
    pub tracking_content: Vec<u8>,
    pub root_envelope: Option<Vec<u8>>,
    pub root_system_fields: Option<Vec<u8>>,
    pub tracking_stopped: bool,
    pub absence_pass_completed: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudFetchDrain {
    pub zone: String,
    pub fetched: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudWorkflowDiagnostics {
    pub missing_did_fetch_events: u64,
    pub root_reads: Vec<crate::cloud_fetch_reads::CloudRootReadDiagnostic>,
    pub zone_deletions: Vec<crate::cloud_zone_deletion::CloudZoneDeletionDiagnostic>,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudWorkflowStatus {
    pub syncing_collection: bool,
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudCollectionOutcome {
    Succeeded,
    WaitingNoAccount,
    Incomplete,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudExchangeResult {
    pub id: u64,
    pub collection: CloudCollectionOutcome,
    pub selected_names: Vec<String>,
    pub acked_names: Vec<String>,
    pub outstanding_names: Vec<String>,
}
impl From<a::CompletedExchange> for CloudExchangeResult {
    fn from(v: a::CompletedExchange) -> Self {
        Self {
            id: v.id,
            collection: match v.collection {
                a::CollectionOutcome::Succeeded => CloudCollectionOutcome::Succeeded,
                a::CollectionOutcome::WaitingNoAccount => CloudCollectionOutcome::WaitingNoAccount,
                a::CollectionOutcome::Incomplete => CloudCollectionOutcome::Incomplete,
                a::CollectionOutcome::Failed => CloudCollectionOutcome::Failed,
                a::CollectionOutcome::Cancelled => CloudCollectionOutcome::Cancelled,
            },
            selected_names: v.result.selected_names.into_iter().collect(),
            acked_names: v.result.acked_names.into_iter().collect(),
            outstanding_names: v.result.outstanding_names.into_iter().collect(),
        }
    }
}
#[derive(Clone, Debug, uniffi::Enum)]
pub enum CloudWorkflowObservation {
    SessionEnded {
        session: u64,
    },
    CorePendingEmitted {
        kind: CloudUpdateKind,
        action_id: u64,
    },
    FullComparisonSkipped {
        action_id: u64,
    },
    BatchBuilt {
        names: Vec<String>,
        batch_id: u64,
    },
    PendingAdded {
        name: String,
        action_id: u64,
    },
    HandlerReturned {
        request_id: u64,
    },
    SavedPendingRemoved {
        name: String,
        request_id: u64,
    },
    ReconcileKindCompleted {
        kind: u8,
        row_count: u64,
    },
    MediaFeedDrained {
        entry_count: u64,
        feed_id: Vec<u8>,
        cursor: u64,
    },
}
impl From<w::Observation> for CloudWorkflowObservation {
    fn from(v: w::Observation) -> Self {
        match v {
            w::Observation::SessionEnded { session } => Self::SessionEnded { session },
            w::Observation::CorePendingEmitted { kind, action_id } => Self::CorePendingEmitted {
                kind: kind.into(),
                action_id,
            },
            w::Observation::FullComparisonSkipped { action_id } => Self::FullComparisonSkipped { action_id },
            w::Observation::BatchBuilt { names, batch_id } => Self::BatchBuilt {
                names: names.into_iter().collect(),
                batch_id,
            },
            w::Observation::PendingAdded { name, action_id } => Self::PendingAdded { name, action_id },
            w::Observation::HandlerReturned { request_id } => Self::HandlerReturned { request_id },
            w::Observation::SavedPendingRemoved { name, request_id } => Self::SavedPendingRemoved { name, request_id },
            w::Observation::ReconcileKindCompleted { kind, row_count } => Self::ReconcileKindCompleted {
                kind: kind as u8,
                row_count: row_count as u64,
            },
            w::Observation::MediaFeedDrained {
                entry_count,
                feed_id,
                cursor,
            } => Self::MediaFeedDrained {
                entry_count: entry_count as u64,
                feed_id: feed_id.to_vec(),
                cursor,
            },
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudEngineRegistration {
    pub instance_id: String,
    pub serialization: Option<Vec<u8>>,
    pub collection_zone: String,
    pub needs_root_fetch: bool,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudEngineBatch {
    pub id: u64,
    pub saves: Vec<CloudRecord>,
    pub deletes: Vec<String>,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudEngineErrorCode {
    NotAuthenticated,
    AccountTemporarilyUnavailable,
    NetworkFailure,
    NetworkUnavailable,
    RequestRateLimited,
    ServiceUnavailable,
    ZoneBusy,
    LimitExceeded,
    BatchRequestFailed,
    ZoneNotFound,
    EncryptedDataReset,
    UserDeletedZone,
    Other,
    UnknownItem,
    ServerRecordChanged,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudZoneLossCause {
    Deleted,
    Purged,
    EncryptedDataReset,
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudZoneLoss {
    pub zone: String,
    pub cause: CloudZoneLossCause,
}
impl CloudZoneLoss {
    pub(crate) fn into_loss_cause(self) -> c::adapter::ZoneLossCause {
        c::adapter::ZoneLoss::from(self).cause
    }
}
impl From<CloudZoneLoss> for c::adapter::ZoneLoss {
    fn from(loss: CloudZoneLoss) -> Self {
        Self {
            zone: loss.zone,
            cause: match loss.cause {
                CloudZoneLossCause::Deleted => c::adapter::ZoneLossCause::Deleted,
                CloudZoneLossCause::Purged => c::adapter::ZoneLossCause::Purged,
                CloudZoneLossCause::EncryptedDataReset => c::adapter::ZoneLossCause::EncryptedDataReset,
            },
        }
    }
}
#[derive(Clone, Debug, uniffi::Record)]
pub struct CloudRecordFailure {
    pub name: String,
    pub code: CloudEngineErrorCode,
    pub server_record: Option<CloudRecord>,
    pub zone: Option<String>,
    pub native_code: Option<i64>,
    pub retry_after_ms: Option<i64>,
}
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudAccountStatus {
    Available,
    NoAccount,
    TemporarilyUnavailable,
    Restricted,
    CouldNotDetermine,
    SwitchedAccounts,
    Configuration,
}
fn identity(status: CloudAccountStatus, value: Option<Vec<u8>>) -> r::IdentityStatus {
    match status {
        CloudAccountStatus::Available => value
            .map(|v| r::IdentityStatus::Available(c::OpaqueBytes(v)))
            .unwrap_or(r::IdentityStatus::ReadFailed),
        CloudAccountStatus::NoAccount => r::IdentityStatus::SignedOut,
        CloudAccountStatus::TemporarilyUnavailable => r::IdentityStatus::TemporarilyUnavailable,
        CloudAccountStatus::Restricted => r::IdentityStatus::Restricted,
        CloudAccountStatus::CouldNotDetermine => r::IdentityStatus::ReadFailed,
        CloudAccountStatus::Configuration => r::IdentityStatus::Configuration,
        CloudAccountStatus::SwitchedAccounts => r::IdentityStatus::SwitchedAccounts,
    }
}
#[derive(uniffi::Object)]
pub struct CloudWorkflow {
    pub(crate) host: Arc<meguri_host::CoreHost>,
    pub(crate) core: Arc<r::Runtime>,
    identity: Option<Arc<CachedIdentity>>,
}
#[uniffi::export]
impl CloudWorkflow {
    pub fn connection(&self) -> Result<Arc<crate::cloud_connection::CloudConnection>, CoreError> {
        callback::enter("CloudWorkflow.connection");
        crate::cloud_connection::CloudConnection::for_workflow(self)
    }
    #[uniffi::constructor]
    pub fn new() -> Result<Arc<Self>, CoreError> {
        callback::enter("CloudWorkflow.new");
        let host = runtime::host()?;
        let core = host.cloud_runtime().cloned().ok_or_else(|| CoreError::Unavailable {
            reason: "cloud platform inputs were not configured before open".into(),
        })?;
        core.retain_adapter_inspection();
        Ok(Arc::new(Self {
            host,
            core,
            identity: PLATFORM
                .lock()
                .unwrap_or_else(PoisonError::into_inner)
                .as_ref()
                .map(|platform| platform.identity.clone()),
        }))
    }
    pub fn instance(&self) -> Result<String, CoreError> {
        callback::enter("CloudWorkflow.instance");
        self.core.instance().map_err(cloud_error)
    }
    pub fn current_guard(&self) -> Result<CloudGuard, CoreError> {
        callback::enter("CloudWorkflow.current_guard");
        Ok(self.core.adapter_guard().into())
    }
    pub fn entity_facts(&self, name: String) -> Result<Option<CloudEntityFacts>, CoreError> {
        callback::enter("CloudWorkflow.entity_facts");
        self.core
            .entity_facts(&name)
            .map_err(cloud_error)?
            .map(TryInto::try_into)
            .transpose()
    }
    pub fn await_tracking(&self, generation: u64, seq: u64) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.await_tracking");
        let c = self.host.collection()?;
        if c.generation().0 != generation {
            return Err(CoreError::StaleGeneration {
                planned: generation,
                current: c.generation().0,
            });
        }
        if !self
            .core
            .capture_proof(meguri_engine::Seq(seq), meguri_engine::Generation(generation))
            .is_some_and(|p| p.persisted)
        {
            return Err(cloud_error("tracking did not persist the requested commit"));
        }
        Ok(())
    }
    pub fn tracking_sendables(&self) -> Result<HashMap<String, CloudPendingChange>, CoreError> {
        callback::enter("CloudWorkflow.tracking_sendables");
        Ok(self
            .core
            .tracking_sendables()
            .into_iter()
            .map(|(n, v)| (n, v.into()))
            .collect())
    }
    pub fn pending_transition(
        &self,
        instance: String,
        kind: CloudUpdateKind,
        snapshot: HashMap<String, CloudPendingChange>,
    ) -> Result<CloudPendingUpdate, CoreError> {
        callback::enter("CloudWorkflow.pending_transition");
        self.core
            .pending_transition(
                &instance,
                kind.into(),
                w::PendingSnapshot(snapshot.into_iter().map(|(n, v)| (n, v.into())).collect()),
            )
            .map(Into::into)
            .map_err(cloud_error)
    }
    pub fn emitted_update_ids(&self) -> Result<Vec<u64>, CoreError> {
        callback::enter("CloudWorkflow.emitted_update_ids");
        Ok(self.core.emitted_update_ids())
    }
    pub fn observations(&self) -> Result<Vec<CloudWorkflowObservation>, CoreError> {
        callback::enter("CloudWorkflow.observations");
        Ok(self.core.observations().into_iter().map(Into::into).collect())
    }
    pub fn poll_review(&self) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.poll_review");
        self.core
            .poll_review(&self.core.instance().map_err(cloud_error)?)
            .map(|_| ())
            .map_err(cloud_error)
    }
    pub fn set_paused(&self, paused: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.set_paused");
        self.core.adapter_pause(paused).map_err(cloud_error)
    }
    pub fn stop_syncing(&self) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.stop_syncing");
        self.host.stop_using_icloud().map_err(cloud_error)
    }
    pub fn durable_snapshot(&self) -> Result<CloudDurableSnapshot, CoreError> {
        callback::enter("CloudWorkflow.durable_snapshot");
        let v = self.core.adapter_snapshot().map_err(cloud_error)?;
        Ok(CloudDurableSnapshot {
            binding: v.binding,
            tracking_content: v.tracking_content,
            serialization: v.serialization,
            entities: v
                .entities
                .into_iter()
                .map(TryInto::try_into)
                .collect::<Result<_, _>>()?,
            inbox_names: v.inbox.into_iter().map(|e| e.record_name).collect(),
            root_envelope: v.root.as_ref().map(|r| r.envelope.0.clone()),
            root_system_fields: v.root.map(|r| r.system_fields.0),
            tracking_stopped: v.tracking_stopped,
            absence_pass_completed: v.absence_pass_completed,
        })
    }
    pub fn fetch_drains(&self) -> Result<Vec<CloudFetchDrain>, CoreError> {
        callback::enter("CloudWorkflow.fetch_drains");
        Ok(self
            .core
            .adapter_fetch_drains()
            .into_iter()
            .map(|v| CloudFetchDrain {
                zone: v.zone,
                fetched: v.fetched,
            })
            .collect())
    }
    pub fn diagnostics(&self) -> Result<CloudWorkflowDiagnostics, CoreError> {
        callback::enter("CloudWorkflow.diagnostics");
        Ok(CloudWorkflowDiagnostics {
            missing_did_fetch_events: self.core.adapter_missing_did(),
            root_reads: self
                .core
                .adapter_root_read_diagnostics()
                .into_iter()
                .map(Into::into)
                .collect(),
            zone_deletions: self
                .core
                .zone_deletion_diagnostics()
                .into_iter()
                .map(Into::into)
                .collect(),
        })
    }
    pub fn status(&self) -> Result<CloudWorkflowStatus, CoreError> {
        callback::enter("CloudWorkflow.status");
        Ok(CloudWorkflowStatus {
            syncing_collection: self.core.adapter_exchange_active(),
        })
    }
    pub fn exchange_result(&self, id: u64) -> Result<Option<CloudExchangeResult>, CoreError> {
        callback::enter("CloudWorkflow.exchange_result");
        Ok(self.core.adapter_exchange_result(id).map(Into::into))
    }
    pub fn begin_identity_read(&self, transition: bool, origin_sdk: Option<String>) -> Result<u64, CoreError> {
        callback::enter("CloudWorkflow.begin_identity_read");
        self.core
            .adapter_begin_identity_read(transition, origin_sdk.as_deref())
            .map_err(cloud_error)
    }
    pub fn complete_identity_read(
        &self,
        id: u64,
        status: CloudAccountStatus,
        value: Option<Vec<u8>>,
    ) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.complete_identity_read");
        let observed = identity(status, value);
        let accepted = self
            .core
            .adapter_complete_identity_read(id, observed.clone())
            .map_err(cloud_error)?;
        if accepted
            && let r::IdentityStatus::Available(account) = &observed
            && self
                .core
                .status()
                .map_err(cloud_error)?
                .binding
                .as_ref()
                .is_some_and(|b| b.facts.account == *account && b.facts.state == c::BindingState::Active)
        {
            self.core
                .adapter_accept_zone_losses(id, &self.host.collection()?)
                .map_err(cloud_error)?;
        }
        if accepted
            && !matches!(observed, r::IdentityStatus::ReadFailed)
            && let Some(cache) = &self.identity
        {
            cache.set(observed);
        }
        Ok(accepted)
    }
    pub fn prepare_runtime(&self) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.prepare_runtime");
        self.host.prepare_cloud_runtime(std::time::Duration::from_secs(1)).map_err(cloud_error)
    }
    pub fn register_sdk(&self, automatic: bool) -> Result<CloudEngineRegistration, CoreError> {
        callback::enter("CloudWorkflow.register_sdk");
        let (instance_id, serialization, collection_zone) = self.core.register_sdk(automatic).map_err(cloud_error)?;
        Ok(CloudEngineRegistration {
            instance_id,
            serialization,
            collection_zone,
            needs_root_fetch: self.core.adapter_epoch_crt_known() && !self.core.sdk_root_confirmed(),
        })
    }
    pub fn validate_prepared_batch(&self, sdk: String, zone: String, batch: u64) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.validate_prepared_batch");
        self.core.validate_prepared_batch(&sdk, &zone, batch).map_err(crate::cloud_connection::error)
    }
    pub fn validate_network_sdk(&self, sdk: String, media: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.validate_network_sdk");
        if media {
            self.core.validate_network_media_sdk(&sdk)
        } else {
            self.core.validate_network_sdk(&sdk)
        }
        .map_err(crate::cloud_connection::error)
    }
    pub fn validate_sdk(&self, sdk: String) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.validate_sdk");
        self.core.validate_sdk(&sdk).map_err(crate::cloud_connection::error)
    }
    pub fn retire_sdk(&self, sdk: String, policy_stop: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.retire_sdk");
        self.core.retire_sdk(&sdk, policy_stop).map_err(cloud_error)
    }
    pub fn release_sdk(&self, sdk: String) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.release_sdk");
        self.core.release_sdk(&sdk).map_err(cloud_error)
    }
    pub fn account_changed(
        &self,
        sdk: Option<String>,
        status: CloudAccountStatus,
        value: Option<Vec<u8>>,
    ) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.account_changed");
        let observed = identity(status, value);
        self.core
            .adapter_account(sdk.as_deref(), observed.clone())
            .map_err(cloud_error)?;
        if let Some(cache) = &self.identity {
            cache.set(observed);
        }
        Ok(())
    }
    pub fn take_pending_updates(&self, sdk: String) -> Result<Vec<CloudPendingUpdate>, CoreError> {
        callback::enter("CloudWorkflow.take_pending_updates");
        self.core.validate_sdk(&sdk).map_err(cloud_error)?;
        self.core
            .take_pending_updates(&sdk)
            .map(|v| v.into_iter().map(Into::into).collect())
            .map_err(cloud_error)
    }
    pub fn update_applied(&self, sdk: String, id: u64, added: Vec<String>) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.update_applied");
        self.core.sdk_update_applied(&sdk, id, &added).map_err(cloud_error)
    }
    pub fn build_batch(
        &self,
        sdk: String,
        zone: String,
        pending: HashMap<String, CloudPendingChange>,
    ) -> Result<Option<CloudEngineBatch>, CoreError> {
        callback::enter("CloudWorkflow.build_batch");
        self.core.validate_sdk(&sdk).map_err(cloud_error)?;
        if zone == "meguri-media" {
            let requested = pending
                .into_iter()
                .map(|(name, action)| (name, action.into()))
                .collect();
            let Some((id, work)) = self
                .core
                .build_cloud_media_batch(&sdk, &self.host.collection()?, &requested)
                .map_err(cloud_error)?
            else {
                return Ok(None);
            };
            let (saves, deletes) = match work.request {
                c::media::Request::Upload { name, envelope, staged } => (
                    vec![CloudRecord {
                        name,
                        envelope,
                        system_fields: None,
                        record_type: Some("MGBlob".into()),
                        zone: Some(zone),
                        asset_path: Some(staged.to_string_lossy().into_owned()),
                    }],
                    Vec::new(),
                ),
                c::media::Request::Delete { name } => (Vec::new(), vec![name]),
                _ => return Err(cloud_error("unsupported media batch")),
            };
            return Ok(Some(CloudEngineBatch { id, saves, deletes }));
        }
        let epoch = self
            .core
            .status()
            .map_err(cloud_error)?
            .binding
            .ok_or_else(|| cloud_error("binding unavailable"))?
            .facts
            .epoch;
        let allowed = c::adapter::collection_zone(epoch);
        if zone != allowed {
            return Ok(None);
        }
        let names = pending
            .into_iter()
            .filter_map(|(n, v)| matches!(v, CloudPendingChange::Save).then_some(n))
            .collect();
        let b = self
            .core
            .build_collection_batch(&sdk, &self.host.collection()?, &names)
            .map_err(cloud_error)?;
        if b.saves.is_empty() {
            return Ok(None);
        }
        Ok(Some(CloudEngineBatch {
            id: b.id,
            saves: b
                .saves
                .into_iter()
                .map(|r| CloudRecord {
                    name: r.name,
                    envelope: r.envelope,
                    system_fields: r.system_fields,
                    record_type: Some("MGEntity".into()),
                    zone: Some(zone.clone()),
                    asset_path: r.asset_path,
                })
                .collect(),
            deletes: Vec::new(),
        }))
    }
    #[allow(clippy::too_many_arguments)]
    pub fn sent(
        &self,
        sdk: String,
        batch_id: u64,
        saved: Vec<CloudRecord>,
        failed: Vec<CloudRecordFailure>,
        deleted: Vec<String>,
        failed_delete: Vec<CloudRecordFailure>,
        batch_count: u32,
    ) -> Result<u64, CoreError> {
        callback::enter("CloudWorkflow.sent");
        self.core.validate_sdk(&sdk).map_err(cloud_error)?;
        // Record metadata cannot turn a collection callback into media work.
        // The retained backend batch decides the path, below.
        let media_metadata = saved
            .iter()
            .any(|r| r.record_type.as_deref() == Some("MGBlob") || r.zone.as_deref() == Some("meguri-media"));
        if media_metadata
            && saved
                .iter()
                .any(|r| r.record_type.as_deref() != Some("MGBlob") || r.zone.as_deref() != Some("meguri-media"))
        {
            return Err(cloud_error("mixed media Saved callback"));
        }
        if !media_metadata {
            let epoch = self
                .core
                .status()
                .map_err(cloud_error)?
                .binding
                .ok_or_else(|| cloud_error("binding unavailable"))?
                .facts
                .epoch;
            let zone = c::adapter::collection_zone(epoch);
            if saved.iter().any(|record| {
                record.record_type.as_deref().is_some_and(|kind| kind != "MGEntity")
                    || record.zone.as_deref().is_some_and(|actual| actual != zone.as_str())
            }) {
                return Err(cloud_error("collection Saved metadata mismatch"));
            }
        }
        let mut names = BTreeSet::new();
        if saved
            .iter()
            .map(|record| &record.name)
            .chain(failed.iter().map(|failure| &failure.name))
            .chain(deleted.iter())
            .chain(failed_delete.iter().map(|failure| &failure.name))
            .any(|name| !names.insert(name))
        {
            return Err(cloud_error("duplicate sent receipt name"));
        }
        if failed_delete
            .iter()
            .any(|f| f.zone.as_deref() != Some("meguri-media") || f.server_record.is_some())
        {
            return Err(cloud_error("invalid failed deletion metadata"));
        }
        let mut conflicts = Vec::new();
        let mut blob_conflicts = Vec::new();
        for failure in &failed {
            match (&failure.code, &failure.server_record) {
                (CloudEngineErrorCode::ServerRecordChanged, Some(record)) => {
                    if record.record_type.as_deref() == Some("MGBlob") {
                        if failure.zone.as_deref() != Some("meguri-media")
                            || record.zone.as_deref() != Some("meguri-media")
                            || record.name != failure.name
                            || record.asset_path.is_some()
                            || record.envelope.is_empty()
                        {
                            return Err(cloud_error("invalid immutable blob conflict metadata"));
                        }
                        blob_conflicts.push(record.clone().saved());
                        continue;
                    }
                    let binding = self
                        .core
                        .status()
                        .map_err(cloud_error)?
                        .binding
                        .ok_or_else(|| cloud_error("binding unavailable"))?;
                    let zone = c::adapter::collection_zone(binding.facts.epoch);
                    if media_metadata
                        || record.name != failure.name
                        || record.record_type.as_deref() != Some("MGEntity")
                        || record.zone.as_deref() != Some(zone.as_str())
                        || record.system_fields.as_ref().is_none_or(Vec::is_empty)
                        || record.envelope.is_empty()
                        || record.envelope.len() > 1_000_000
                        || record.asset_path.is_some()
                    {
                        return Err(cloud_error("collection conflict metadata mismatch"));
                    }
                    conflicts.push(record.clone().saved());
                }
                (CloudEngineErrorCode::ServerRecordChanged, None) | (_, Some(_)) => {
                    return Err(cloud_error("unexpected or missing conflict metadata"));
                }
                (_, None) => {}
            }
        }
        if !conflicts.is_empty() {
            self.core
                .preflight_sent_conflicts(
                    &sdk,
                    &self.host.collection()?,
                    batch_id,
                    &conflicts.iter().map(|record| record.name.clone()).collect::<Vec<_>>(),
                )
                .map_err(cloud_error)?;
        }
        let bound_zone = c::adapter::collection_zone(
            self.core
                .status()
                .map_err(cloud_error)?
                .binding
                .ok_or_else(|| cloud_error("binding unavailable"))?
                .facts
                .epoch,
        );
        let mut losses = Vec::new();
        for failure in failed.iter().chain(&failed_delete) {
            let cause = match failure.code {
                CloudEngineErrorCode::ZoneNotFound => Some(c::adapter::ZoneLossCause::Deleted),
                CloudEngineErrorCode::UserDeletedZone => Some(c::adapter::ZoneLossCause::Purged),
                CloudEngineErrorCode::EncryptedDataReset => Some(c::adapter::ZoneLossCause::EncryptedDataReset),
                _ => None,
            };
            if let Some(cause) = cause {
                let zone = failure.zone.clone().unwrap_or_else(|| bound_zone.clone());
                if zone != bound_zone && zone != "meguri-media" {
                    return Err(cloud_error("foreign zone loss receipt"));
                }
                losses.push(c::adapter::ZoneLoss { zone, cause });
            }
        }
        self.core.enter_sent_handler(&sdk, batch_id).map_err(cloud_error)?;
        let sdk_retrying = failed.iter().chain(&failed_delete).any(|f| {
            matches!(
                f.code,
                CloudEngineErrorCode::NotAuthenticated
                    | CloudEngineErrorCode::AccountTemporarilyUnavailable
                    | CloudEngineErrorCode::NetworkFailure
                    | CloudEngineErrorCode::NetworkUnavailable
                    | CloudEngineErrorCode::RequestRateLimited
                    | CloudEngineErrorCode::ServiceUnavailable
                    | CloudEngineErrorCode::ZoneBusy
            )
        });
        self.core
            .adapter_sdk_retrying(&sdk, sdk_retrying)
            .map_err(cloud_error)?;
        let saved = saved.into_iter().map(CloudRecord::saved).collect::<Vec<_>>();
        let retrying = failed
            .iter()
            .chain(&failed_delete)
            .filter(|f| {
                !matches!(
                    f.code,
                    CloudEngineErrorCode::LimitExceeded
                        | CloudEngineErrorCode::BatchRequestFailed
                        | CloudEngineErrorCode::UnknownItem
                        | CloudEngineErrorCode::ZoneNotFound
                        | CloudEngineErrorCode::UserDeletedZone
                        | CloudEngineErrorCode::EncryptedDataReset
                        | CloudEngineErrorCode::Other
                        | CloudEngineErrorCode::ServerRecordChanged
                )
            })
            .map(|f| f.name.clone())
            .collect::<Vec<_>>();
        let failed_delete = failed_delete
            .into_iter()
            .filter_map(|f| {
                let code = match f.code {
                    CloudEngineErrorCode::LimitExceeded => w::SendError::LimitExceeded,
                    CloudEngineErrorCode::BatchRequestFailed => w::SendError::BatchRequestFailed,
                    CloudEngineErrorCode::UnknownItem => w::SendError::UnknownItem,
                    CloudEngineErrorCode::ZoneNotFound
                    | CloudEngineErrorCode::UserDeletedZone
                    | CloudEngineErrorCode::EncryptedDataReset => w::SendError::ZoneMissing,
                    CloudEngineErrorCode::Other | CloudEngineErrorCode::ServerRecordChanged => w::SendError::Other,
                    _ => return None,
                };
                Some((f.name, code))
            })
            .collect::<Vec<_>>();
        let failed = failed
            .into_iter()
            .filter_map(|f| {
                let code = match f.code {
                    CloudEngineErrorCode::LimitExceeded => w::SendError::LimitExceeded,
                    CloudEngineErrorCode::BatchRequestFailed => w::SendError::BatchRequestFailed,
                    CloudEngineErrorCode::UnknownItem => w::SendError::UnknownItem,
                    CloudEngineErrorCode::ZoneNotFound
                    | CloudEngineErrorCode::UserDeletedZone
                    | CloudEngineErrorCode::EncryptedDataReset => w::SendError::ZoneMissing,
                    CloudEngineErrorCode::Other => w::SendError::Other,
                    _ => return None,
                };
                Some((f.name, code))
            })
            .collect::<Vec<_>>();
        if self
            .core
            .cloud_media_sdk_sent(
                &sdk,
                &self.host.collection()?,
                batch_id,
                &saved,
                &blob_conflicts,
                &failed,
                &deleted,
                &failed_delete,
                &retrying,
            )
            .map_err(cloud_error)?
        {
            if !losses.is_empty() {
                self.core
                    .adapter_observe_zone_losses(&sdk, &losses)
                    .map_err(cloud_error)?;
            }
            return Ok(saved.len() as u64);
        }
        if media_metadata || !blob_conflicts.is_empty() || !deleted.is_empty() || !failed_delete.is_empty() {
            return Err(cloud_error("media receipt has no original SDK work"));
        }
        let count = self
            .core
            .sent_results_with_conflicts(
                &sdk,
                &self.host.collection()?,
                batch_id,
                &saved,
                &conflicts,
                &failed,
                batch_count as usize,
            )
            .map_err(cloud_error)?;
        if !losses.is_empty() {
            self.core
                .adapter_observe_zone_losses(&sdk, &losses)
                .map_err(cloud_error)?;
        }
        Ok(count)
    }
    pub fn handler_returned(&self, sdk: String) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.handler_returned");
        self.core.return_sent_handler(&sdk).map(|_| ()).map_err(cloud_error)
    }
    /// Actual SDK bookkeeping only, after the original sent handler returned.
    pub fn sdk_saved_removed(&self, sdk: String, request_id: u64, names: Vec<String>) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.sdk_saved_removed");
        self.core.validate_sdk(&sdk).map_err(cloud_error)?;
        self.core
            .sdk_saved_removed(&sdk, request_id, &names)
            .map_err(cloud_error)
    }
    pub fn tracking_failure(&self, sdk: String) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.tracking_failure");
        self.core.adapter_tracking_failure(&sdk).map_err(cloud_error)
    }
    pub fn fetched_collection_zone_deletions(&self, sdk: String, names: Vec<String>) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.fetched_collection_zone_deletions");
        self.core
            .adapter_fetched_collection_zone_deletions(&sdk, &names)
            .map_err(cloud_error)
    }
    pub fn fetched_zone_losses(&self, sdk: String, losses: Vec<CloudZoneLoss>) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.fetched_zone_losses");
        self.core
            .adapter_observe_zone_losses(&sdk, &losses.into_iter().map(Into::into).collect::<Vec<_>>())
            .map_err(cloud_error)
    }
    pub fn state_update(&self, sdk: String, state: Vec<u8>) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.state_update");
        self.core.adapter_state_update(&sdk, &state).map_err(cloud_error)
    }
    pub fn fetched(
        &self,
        sdk: String,
        zone: String,
        records: Vec<CloudRecord>,
        deleted: Vec<String>,
    ) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.fetched");
        self.core.validate_sdk(&sdk).map_err(cloud_error)?;
        if records.iter().any(|r| {
            let expected = if zone == "meguri-root" {
                "MGActive"
            } else if r.name == "epoch" {
                "MGEpoch"
            } else if r.name.starts_with('g') {
                "MGGeneration"
            } else {
                "MGEntity"
            };
            r.zone.as_ref().is_some_and(|z| z != &zone)
                || r.record_type.as_deref().is_some_and(|kind| kind != expected)
                || (expected != "MGEntity" && r.asset_path.is_some())
        }) {
            self.core.adapter_tracking_failure(&sdk).map_err(cloud_error)?;
            return Err(cloud_error("unsupported fetched record metadata"));
        }
        self.core
            .adapter_fetched_assets(
                &sdk,
                &zone,
                &records.into_iter().map(CloudRecord::fetched).collect::<Vec<_>>(),
                &deleted,
            )
            .map_err(cloud_error)?;
        // adapter_fetched has validated the SDK and exact zone and committed
        // the inbox. Its guards are gone before the host admits a writer/WAL
        // wait. The serialized native event completes before later cursor events.
        if zone != "meguri-root" {
            loop {
                match self
                    .host
                    .apply_native_cloud_incoming(&sdk, std::time::Duration::from_secs(1))
                    .map_err(cloud_error)?
                {
                    c::incoming::IncomingDisposition::Progress(0) | c::incoming::IncomingDisposition::Deferred(_) => {
                        break;
                    }
                    c::incoming::IncomingDisposition::Progress(_) => {}
                }
            }
        }
        Ok(())
    }
    pub fn begin_fetch(&self, sdk: String, zones: Vec<String>) -> Result<u64, CoreError> {
        callback::enter("CloudWorkflow.begin_fetch");
        self.core.adapter_begin_fetch(&sdk, zones).map_err(cloud_error)
    }
    pub fn will_fetch(&self, sdk: String, group: String, manual: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.will_fetch");
        self.core.adapter_will_fetch(&sdk, group, manual).map_err(cloud_error)
    }
    pub fn fetch_zone(&self, sdk: String, zone: String, will: bool, failed: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.fetch_zone");
        self.core
            .adapter_fetch_zone(&sdk, zone, will, failed)
            .map_err(cloud_error)
    }
    pub fn did_fetch(&self, sdk: String, group: String, manual: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.did_fetch");
        self.core.adapter_did_fetch(&sdk, &group, manual).map_err(cloud_error)
    }
    pub fn finish_fetch(&self, sdk: String, id: u64) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.finish_fetch");
        self.core.adapter_finish_fetch(&sdk, id).map_err(cloud_error)
    }
    pub fn abort_fetch(&self, sdk: String, id: u64) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.abort_fetch");
        self.core.adapter_abort_fetch(&sdk, id).map_err(cloud_error)
    }
    pub fn begin_exchange(&self, sdk: String) -> Result<u64, CoreError> {
        callback::enter("CloudWorkflow.begin_exchange");
        self.core.sync_now(&sdk, &self.host.collection()?).map_err(cloud_error)
    }
    pub fn exchange_fetched(&self, sdk: String, complete: bool) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.exchange_fetched");
        self.core.adapter_exchange_fetch(&sdk, complete).map_err(cloud_error)
    }
    pub fn complete_exchange(
        &self,
        sdk: String,
        id: u64,
        fetched: bool,
        cancelled: bool,
        failed: bool,
    ) -> Result<CloudExchangeResult, CoreError> {
        callback::enter("CloudWorkflow.complete_exchange");
        self.core
            .adapter_complete_exchange(
                &sdk,
                id,
                fetched,
                if failed {
                    Some(a::CollectionOutcome::Failed)
                } else if cancelled {
                    Some(a::CollectionOutcome::Cancelled)
                } else {
                    None
                },
            )
            .map(Into::into)
            .map_err(cloud_error)
    }
    pub fn send_due(&self, sdk: String) -> Result<bool, CoreError> {
        callback::enter("CloudWorkflow.send_due");
        self.core.adapter_send_due(&sdk).map_err(cloud_error)
    }
    pub fn begin_send(&self, sdk: String) -> Result<Option<u64>, CoreError> {
        callback::enter("CloudWorkflow.begin_send");
        self.core
            .start_manual_send_call(&sdk)
            .map(|v| v.map(|c| c.id))
            .map_err(cloud_error)
    }
    pub fn finish_send(
        &self,
        sdk: String,
        id: u64,
        pending: HashMap<String, CloudPendingChange>,
    ) -> Result<(), CoreError> {
        callback::enter("CloudWorkflow.finish_send");
        self.core.finish_send_call(&sdk, id).map_err(cloud_error)?;
        self.core
            .adapter_repair_pending(
                &sdk,
                pending
                    .into_iter()
                    .map(|(n, v)| (n, v.into()))
                    .collect::<BTreeMap<_, _>>(),
            )
            .map_err(cloud_error)
    }
}

struct CachedIdentity(Mutex<Option<r::IdentityStatus>>);
impl CachedIdentity {
    fn set(&self, value: r::IdentityStatus) {
        *self.0.lock().unwrap_or_else(PoisonError::into_inner) = Some(value);
    }
}
impl r::IdentityProvider for CachedIdentity {
    fn read_identity(&self, _: &c::BuildScope) -> r::IdentityStatus {
        self.0
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .unwrap_or(r::IdentityStatus::ReadFailed)
    }
}
struct NativeFiles;
impl r::FileIdentityProvider for NativeFiles {
    fn identity(&self, path: &std::path::Path) -> Result<r::NativeFileIdentity, r::RuntimeError> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let m = std::fs::metadata(path).map_err(|_| r::RuntimeError::FileIdentityUnavailable)?;
            let created = m
                .created()
                .map_err(|_| r::RuntimeError::FileIdentityUnavailable)?
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| r::RuntimeError::FileIdentityUnavailable)?;
            Ok(r::NativeFileIdentity {
                device: m.dev(),
                inode: m.ino(),
                created_seconds: i64::try_from(created.as_secs())
                    .map_err(|_| r::RuntimeError::FileIdentityUnavailable)?,
                created_nanos: created.subsec_nanos(),
            })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(r::RuntimeError::FileIdentityUnavailable)
        }
    }
}
/// Returns exactly one full-width key, or no value on OS failure. No core
/// reentry, UI or async work; unlike clock/capacity reads this runs without locks.
#[uniffi::export(foreign)]
pub trait CloudKeyByteProvider: Send + Sync {
    fn draw_key(&self) -> Option<Vec<u8>>;
}

struct ForeignKeyBytes(Arc<dyn CloudKeyByteProvider>);
impl r::KeyByteSource for ForeignKeyBytes {
    fn draw_key(&self) -> Result<[u8; 32], r::KeySourceError> {
        callback::invoke("CloudKeyByteProvider.draw_key", || self.0.draw_key())
            .and_then(|bytes| bytes.try_into().ok())
            .ok_or(r::KeySourceError::Unavailable)
    }
}

struct CloudPlatform {
    inputs: r::CloudInputs,
    identity: Arc<CachedIdentity>,
    key_bytes: Arc<dyn r::KeyByteSource>,
}
static PLATFORM: Mutex<Option<CloudPlatform>> = Mutex::new(None);
#[derive(Clone, Copy, Debug, uniffi::Enum)]
pub enum CloudEnvironment {
    Development,
    Production,
}
#[uniffi::export]
pub fn configure_cloud_platform(
    container: String,
    environment: CloudEnvironment,
    status: CloudAccountStatus,
    value: Option<Vec<u8>>,
    key_bytes: Arc<dyn CloudKeyByteProvider>,
) -> Result<(), CoreError> {
    callback::enter("configure_cloud_platform");
    if runtime::host().is_ok() {
        return Err(CoreError::Busy {
            running_op: "cloud composition requires a closed host".into(),
        });
    }
    let completed = Arc::new(CachedIdentity(Mutex::new(Some(identity(status, value)))));
    let platform = CloudPlatform {
        inputs: r::CloudInputs {
            scope: c::BuildScope {
                container: c::ContainerId::new(container).map_err(cloud_error)?,
                environment: match environment {
                    CloudEnvironment::Development => c::Environment::Development,
                    CloudEnvironment::Production => c::Environment::Production,
                },
            },
            identity: completed.clone(),
            files: Arc::new(NativeFiles),
        },
        identity: completed,
        key_bytes: Arc::new(ForeignKeyBytes(key_bytes)),
    };
    *PLATFORM.lock().unwrap_or_else(PoisonError::into_inner) = Some(platform);
    Ok(())
}
pub(crate) fn platform_inputs() -> Option<(r::CloudInputs, Option<Arc<dyn r::KeyByteSource>>)> {
    let configured = PLATFORM
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .as_ref()
        .map(|platform| (platform.inputs.clone(), Some(platform.key_bytes.clone())));
    #[cfg(feature = "test-hooks")]
    {
        configured.or_else(|| {
            Some((
                r::CloudInputs {
                    scope: c::BuildScope {
                        container: c::ContainerId::new("synthetic-u114").expect("literal"),
                        environment: c::Environment::Development,
                    },
                    identity: Arc::new(CachedIdentity(Mutex::new(None))),
                    files: Arc::new(NativeFiles),
                },
                None,
            ))
        })
    }
    #[cfg(not(feature = "test-hooks"))]
    {
        configured
    }
}

#[cfg(test)]
#[path = "sync_tests.rs"]
mod tests;
