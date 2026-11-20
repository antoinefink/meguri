#[path = "media_driver.rs"]
mod media_driver;
use super::super::{
    adapter::{Observation, ResponseBatch, SDKRig},
    child_driver::{ChildDriver, Input},
    devices::{IdentityReads, Result, failure},
    *,
};
pub use cloud::publication::{ConnectDecision, PublishStep};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{self as cloud, publication::*, runtime::KeyByteSource};
use serde::{Deserialize, Serialize};
use std::{
    cell::RefCell,
    collections::{BTreeMap, BTreeSet, VecDeque},
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Point {
    Confirmed,
    AdoptionStagingRecorded,
    /// The real Activate task, after validation recorded Ready and the active recheck returned.
    AdoptionBeforeActivate,
    /// Hold the original selected continuation, after a complete fetched page.
    AdoptionBeforeNextFetch,
    /// Hold the original Core-selected external payload read after metadata intake.
    AdoptionBeforeFetchPayload,
    BoundBeforeDecision,
    BoundAfterDecision,
    BeforeScan,
    ScanCommitted,
    BeforeCompleteEpochSave,
    BeforeEpochRead,
    BeforeActiveSave,
    /// Before dispatch of the real reset-owned root read.
    BeforeResetMediaRootRead,
    /// Before dispatch of the real conditional root save, including initial publication.
    BeforeMediaRootSave,
    BeforeZoneDeletion,
    AfterCollectionCleanup,
    ZoneDeletionReply,
    ResolutionBeforeMove,
    ResolutionRewriteCommitted,
    ResolutionMoveBeforeRewrite,
    MediaTrashBeforeInstall,
    MediaReplyBeforeApply,
    MediaReplyBeforeTracking,
    MediaSettlementBeforeCommit,
    FetchBeforeInbox,
    FetchAfterInbox,
    FetchAfterCursor,
    ApplyAfterIntent,
    ApplyAfterCommit,
    ApplyAfterFlush,
    IncomingRollbackComplete,
    BeforeCollectionSave,
    IntentSettlementComplete,
}
impl Point {
    fn incoming_stage(self) -> Option<cloud::testing::IncomingStage> {
        use cloud::testing::IncomingStage as Stage;
        Some(match self {
            Self::ResolutionBeforeMove => Stage::ResolutionBeforeMove,
            Self::ResolutionMoveBeforeRewrite => Stage::ResolutionMoveBeforeRewrite,
            Self::ResolutionRewriteCommitted => Stage::ResolutionRewriteCommitted,
            Self::MediaTrashBeforeInstall => Stage::MediaTrashBeforeInstall,
            Self::MediaReplyBeforeApply => Stage::MediaReplyBeforeApply,
            Self::MediaReplyBeforeTracking => Stage::MediaReplyBeforeTracking,
            Self::MediaSettlementBeforeCommit => Stage::MediaSettlementBeforeCommit,
            Self::FetchBeforeInbox => Stage::FetchBeforeInbox,
            Self::FetchAfterInbox => Stage::FetchAfterInbox,
            Self::FetchAfterCursor => Stage::FetchAfterCursor,
            Self::ApplyAfterIntent => Stage::ApplyAfterIntent,
            Self::ApplyAfterCommit => Stage::ApplyAfterCommit,
            Self::ApplyAfterFlush => Stage::ApplyAfterFlush,
            Self::IncomingRollbackComplete => Stage::IncomingRollbackComplete,
            Self::BeforeCollectionSave => Stage::BeforeCollectionSave,
            Self::IntentSettlementComplete => Stage::IntentSettlementComplete,
            _ => return None,
        })
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum BackupNotice {
    OutsideReplacementNoBackup,
    ReplacementBackupRetained {
        name: String,
    },
    ReplacementBackupExpired {
        name: String,
    },
    /// Legacy/foreign replacement evidence carries no owned safety receipt.
    ReplacementBackupUnknown,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct ConnectionPreview {
    pub local: Counts,
    pub cloud: Option<Counts>,
    pub decision: ConnectDecision,
    pub unpublished_by_kind: BTreeMap<cloud::entity_codec::Kind, u64>,
    pub backup_notices: Vec<BackupNotice>,
    production: Preview,
}
impl std::fmt::Debug for ConnectionPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ConnectionPreview")
    }
}
#[derive(Clone, Serialize, Deserialize)]
pub struct HeldRequest {
    pub caller: Caller,
    pub operation: Operation,
}
impl std::fmt::Debug for HeldRequest {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("HeldRequest")
    }
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BackupResult {
    pub name: String,
    pub deleted: Vec<String>,
}
#[derive(Clone, Copy, Debug, Serialize, Deserialize)]
pub struct CollectionHeader {
    pub crt: i64,
    pub ver: i64,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct RecoveryState {
    pub publication_fence_decision: Option<DecisionSnapshot>,
}
impl std::fmt::Debug for RecoveryState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("RecoveryState")
    }
}
#[derive(Serialize, Deserialize)]
pub(crate) struct RestoredWire {
    name: String,
    safety_backup: Option<String>,
    generation: u64,
    cloud_publication_pending: bool,
    deadline: Option<i64>,
}
impl From<meguri_package::restore::RestoredBackup> for RestoredWire {
    fn from(value: meguri_package::restore::RestoredBackup) -> Self {
        Self {
            name: value.name,
            safety_backup: value.safety_backup,
            generation: value.generation.0,
            cloud_publication_pending: value.cloud_publication_pending,
            deadline: value.deadline.map(|d| d.0),
        }
    }
}
impl From<RestoredWire> for meguri_package::restore::RestoredBackup {
    fn from(value: RestoredWire) -> Self {
        Self {
            name: value.name,
            safety_backup: value.safety_backup,
            generation: meguri_engine::Generation(value.generation),
            cloud_publication_pending: value.cloud_publication_pending,
            deadline: value.deadline.map(meguri_model::time::EpochMillis),
        }
    }
}
#[derive(Clone, Debug)]
pub struct BindingInput {
    pub publish_step: Option<PublishStep>,
}
pub struct PublicationRig {
    sdk: RefCell<SDKRig>,
    scope: Scope,
    holds: BTreeSet<Point>,
    held: Option<Point>,
    work: Option<PublicationWork>,
    adoption_metadata_first: bool,
    scan_committed: bool,
    transport_queued: bool,
    started_host_instance: Option<String>,
    needs_no_state_listing: bool,
    explicit_sync_pending: bool,
    own_sent: BTreeMap<u64, ResponseBatch>,
    trace: Vec<cloud::workflow::TraceEvent>,
    observations: Vec<Observation>,
    binding_inputs: Vec<BindingInput>,
    cleanup_pending: bool,
    cleanup_zones: VecDeque<String>,
    cleanup_reply_pending: bool,
    last_sync_phase: &'static str,
}
impl PublicationRig {
    /// The online child must already be reaped. Recover A with cloud disabled,
    /// then retire its writer/journal witnesses through the real exclusive swap.
    /// This fixture grants no cloud authority and records no local replacement.
    pub fn replace_native_file_offline(
        directory: &Path,
        scope: &Scope,
        now: i64,
        seed: u64,
        expected: &PublicationFacts,
    ) -> Result<cloud::runtime::NativeFileIdentity> {
        use super::super::devices::{NativeFiles, build_scope, options};
        use cloud::runtime::FileIdentityProvider;
        use meguri_engine::{EngineError, LongOp, device_state::DeviceState};
        use sha2::{Digest as _, Sha256};
        use std::sync::Mutex;

        let path = directory.join("collection.anki2");
        let device = DeviceState::for_collection(&path);
        let decision = device.publication_decision()?.ok_or_else(failure)?;
        assert!(decision == expected.decision);
        let evidence = device.replacement_evidence_for(decision.publication.identity)?;
        let semantic_sidecar = || -> Result<serde_json::Value> {
            let mut document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(directory.join("meguri-sidecar.json"))?)?;
            assert!(
                document
                    .as_object_mut()
                    .unwrap()
                    .remove("clean_shutdown")
                    .unwrap()
                    .is_boolean()
            );
            Ok(document)
        };
        let sidecar_before = semantic_sidecar()?;
        let tracking = || -> Result<_> {
            let store = match cloud::TrackingStore::open(directory, build_scope(scope)?)? {
                cloud::OpenOutcome::Opened(store) => store,
                _ => return Err(failure()),
            };
            let mut bindings = Vec::new();
            for binding in &expected.bindings {
                bindings.push((store.binding(binding.id)?, store.stored_runtime_state(binding.id)?));
            }
            Ok((store.publication_op()?, bindings))
        };
        let tracking_before = tracking()?;
        let op = tracking_before.0.as_ref().ok_or_else(failure)?;
        assert!(expected.recovery_ops.len() == 1);
        assert!(expected.recovery_ops[0] == cloud::adoption::RecoveryOpFacts::from(op.clone()));
        for (binding, (stored, _)) in expected.bindings.iter().zip(&tracking_before.1) {
            assert!(stored.as_ref() == Some(binding));
        }
        let native_a = NativeFiles.identity(&path)?;
        assert_eq!(native_a, expected.native_file);
        let current = expected.current.as_ref().ok_or_else(failure)?;
        assert!(
            tracking_before
                .1
                .iter()
                .any(|(binding, runtime)| binding.as_ref() == Some(current)
                    && runtime.as_ref().is_some_and(|r| r.file == Some(native_a)))
        );

        let assert_unchanged = || -> Result<()> {
            assert!(semantic_sidecar()? == sidecar_before);
            assert!(device.publication_decision()?.as_ref() == Some(&decision));
            assert!(device.replacement_evidence_for(decision.publication.identity)? == evidence);
            assert!(tracking()? == tracking_before);
            Ok(())
        };
        let mut config = options(directory, now, seed);
        config.publication_binding = decision.publication.binding.clone();
        assert!(config.cloud.is_none() && config.cloud_key_bytes.is_none());
        let host = meguri_host::CoreHost::open(config)?;
        assert!(host.cloud_runtime().is_none());
        assert_eq!(NativeFiles.identity(&path)?, native_a);
        assert_unchanged()?;
        let copied = Arc::new(Mutex::new(None));
        {
            let slot = host.engine().acquire_slot(LongOp::Restore)?;
            let mut exclusive = slot.enter_exclusive(true)?;
            let copied = copied.clone();
            exclusive.replace(move |live| {
                let replacement = live.with_file_name("external-copy.anki2");
                std::fs::copy(live, &replacement).map_err(|e| EngineError::Storage(e.into()))?;
                let bytes = std::fs::read(live).map_err(|e| EngineError::Storage(e.into()))?;
                let candidate = std::fs::read(&replacement).map_err(|e| EngineError::Storage(e.into()))?;
                assert!(bytes == candidate);
                let native_b = NativeFiles.identity(&replacement).map_err(|_| EngineError::Internal {
                    message: "fixture native identity unavailable".into(),
                })?;
                assert_ne!((native_a.device, native_a.inode), (native_b.device, native_b.inode));
                std::fs::rename(&replacement, live).map_err(|e| EngineError::Storage(e.into()))?;
                assert_eq!(NativeFiles.identity(live).unwrap(), native_b);
                let digest: [u8; 32] = Sha256::digest(&bytes).into();
                *copied.lock().unwrap() = Some((native_b, digest));
                Ok(())
            })?;
        }
        let (native_b, digest) = copied.lock().unwrap().take().ok_or_else(failure)?;
        for after_close in [false, true] {
            if after_close {
                host.close()?;
            }
            assert_eq!(NativeFiles.identity(&path)?, native_b);
            let actual: [u8; 32] = Sha256::digest(std::fs::read(&path)?).into();
            assert!(actual == digest);
            assert_unchanged()?;
        }
        Ok(native_b)
    }
    pub fn open(
        directory: &Path,
        scope: &Scope,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
        keys: Arc<dyn KeyByteSource>,
    ) -> Result<Self> {
        let driver = ChildDriver::spawn_unbound(directory, scope, identity, automatic, now, seed, keys)?;
        let mut rig = Self {
            sdk: RefCell::new(SDKRig::from_driver(driver, scope.clone(), String::new(), String::new())),
            scope: scope.clone(),
            holds: BTreeSet::new(),
            held: None,
            work: None,
            scan_committed: false,
            transport_queued: false,
            started_host_instance: None,
            needs_no_state_listing: false,
            explicit_sync_pending: false,
            own_sent: BTreeMap::new(),
            trace: Vec::new(),
            observations: Vec::new(),
            binding_inputs: Vec::new(),
            cleanup_pending: false,
            cleanup_zones: VecDeque::new(),
            cleanup_reply_pending: false,
            last_sync_phase: "idle",
            adoption_metadata_first: false,
        };
        // Refresh only real account/local facts. Unbound discovery registers no SDK.
        rig.sdk.get_mut().driver.call::<()>(Input::Identity(None))?;
        rig.refresh()?;
        Ok(rig)
    }
    pub fn scope(&self) -> &Scope {
        &self.scope
    }
    pub fn driver(&mut self) -> &mut ChildDriver {
        &mut self.sdk.get_mut().driver
    }
    pub fn wants_send(&mut self) -> Result<bool> {
        self.sdk.get_mut().driver.wants_send()
    }
    pub fn send_changes_calls(&mut self) -> Result<Vec<cloud::workflow::SendCall>> {
        self.sdk.get_mut().driver.send_changes_calls()
    }
    pub fn take_fetch_errors(&mut self) -> Vec<Box<dyn std::error::Error + Send + Sync>> {
        self.sdk.get_mut().take_fetch_errors()
    }
    pub fn facts(&self) -> Result<PublicationFacts> {
        self.sdk.borrow_mut().driver.call(Input::PublicationFacts)
    }
    pub fn collection_header(&self) -> Result<CollectionHeader> {
        self.sdk.borrow_mut().driver.call(Input::CollectionHeader)
    }
    /// Actual runtime status, without binding, account or collection payloads.
    pub fn authority_status(&self) -> Result<(bool, bool)> {
        self.sdk.borrow_mut().driver.call(Input::AuthorityStatus)
    }
    fn control(&self) -> Result<cloud::testing::PublicationControl> {
        self.sdk.borrow_mut().driver.call(Input::PublicationControl)
    }
    pub fn recovery_state(&self) -> Result<RecoveryState> {
        self.sdk.borrow_mut().driver.call(Input::RecoveryState)
    }
    pub fn collection_zone(&self) -> Result<String> {
        Ok(cloud::adapter::collection_zone(
            self.control()?.current.ok_or_else(failure)?.facts.epoch,
        ))
    }
    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }
    pub fn trace(&self) -> &[cloud::workflow::TraceEvent] {
        &self.trace
    }
    pub fn binding_inputs(&self) -> &[BindingInput] {
        &self.binding_inputs
    }
    pub fn sdk_database_pending(&self) -> BTreeMap<String, adapter::PendingChange> {
        self.sdk.borrow().database_pending()
    }
    pub fn sdk_pending(&self) -> BTreeMap<String, adapter::PendingChange> {
        self.sdk.borrow().sdk_pending()
    }
    pub fn hold(&mut self, point: Point) -> Result<()> {
        if let Some(stage) = point.incoming_stage() {
            return self.sdk.get_mut().driver.hold_incoming_stage(stage);
        }
        self.holds.insert(point);
        Ok(())
    }
    pub fn is_held(&self, point: Point) -> bool {
        if let Some(stage) = point.incoming_stage() {
            return self.sdk.borrow().driver.is_incoming_stage_held(stage);
        }
        self.held == Some(point)
    }
    pub fn release(&mut self, point: Point) -> Result<()> {
        if let Some(stage) = point.incoming_stage() {
            return self.sdk.get_mut().driver.release_incoming_stage(stage);
        }
        self.holds.remove(&point);
        if self.held == Some(point) {
            self.held = None;
        }
        Ok(())
    }
    fn gate(&mut self, point: Point) -> bool {
        if self.holds.contains(&point) {
            self.held = Some(point);
            true
        } else {
            false
        }
    }
    fn refresh(&mut self) -> Result<()> {
        self.sdk.get_mut().refresh()?;
        self.observations = self.sdk.get_mut().observations().to_vec();
        self.trace = self.sdk.get_mut().driver.call(Input::Trace)?;
        self.binding_inputs = self
            .trace
            .iter()
            .filter_map(|e| match e {
                cloud::workflow::TraceEvent::PublicationBinding { publish_step, .. } => Some(BindingInput {
                    publish_step: *publish_step,
                }),
                _ => None,
            })
            .collect();
        Ok(())
    }
    fn read(
        &mut self,
        service: &mut Service<SeededRandom>,
        zone: String,
        name: &str,
        fields: RequestedFields,
    ) -> Result<RecordRead> {
        let response = service.request(
            &self.scope,
            Caller::default(),
            Operation::Read {
                zone,
                name: name.into(),
                fields,
            },
        )?;
        read_result(&response)
    }
    pub fn preview(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<ConnectionPreview> {
        self.preview_with_active_fields(service, RequestedFields::All, steps)
    }
    pub fn preview_with_active_fields(
        &mut self,
        service: &mut Service<SeededRandom>,
        fields: RequestedFields,
        steps: usize,
    ) -> Result<ConnectionPreview> {
        let mut work: PreviewWork = self.sdk.get_mut().driver.call(Input::PreviewBegin)?;
        for _ in 0..steps {
            work = match work {
                PreviewWork::ReadActive { id } => {
                    let read = self.read(service, "meguri-root".into(), "active", fields.clone())?;
                    self.sdk.get_mut().driver.call(Input::PreviewActive(id, read))?
                }
                PreviewWork::ReadEpoch { id, epoch } => {
                    let read = self.read(
                        service,
                        cloud::adapter::collection_zone(epoch),
                        "epoch",
                        RequestedFields::All,
                    )?;
                    self.sdk.get_mut().driver.call(Input::PreviewEpoch(id, read))?
                }
                PreviewWork::Ready(production) => {
                    let notices = self.sdk.get_mut().driver.call(Input::BackupNotices)?;
                    let preview = ConnectionPreview {
                        local: production.local.clone(),
                        cloud: production.cloud.clone(),
                        decision: production.decision.clone(),
                        unpublished_by_kind: production.unpublished_by_kind.clone(),
                        backup_notices: notices,
                        production: *production,
                    };
                    self.refresh()?;
                    return Ok(preview);
                }
            };
        }
        Err(failure())
    }
    pub fn cancel_preview(&mut self) -> Result<()> {
        self.sdk.get_mut().driver.call(Input::PreviewCancel)
    }
    pub fn confirm_publication(&mut self, preview: &ConnectionPreview) -> Result<[u8; 16]> {
        let id = self
            .sdk
            .get_mut()
            .driver
            .call(Input::PublicationConfirm(preview.production.clone()))?;
        self.work = None;
        self.refresh()?;
        Ok(id)
    }
    pub fn stop_using_icloud(&mut self) -> Result<()> {
        self.sdk.get_mut().driver.call::<()>(Input::StopICloud)?;
        self.work = None;
        self.refresh()
    }
    /// `sync/cloudkit-recovery#V27.2`: the row the core picks for this preview.
    pub fn setup_decision(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
    ) -> Result<cloud::setup::SetupDecision> {
        self.sdk
            .get_mut()
            .driver
            .call(Input::SetupDecision(preview.production.clone(), history))
    }
    /// The decided row's recovery call, made with no confirmation. `None`
    /// is a resumed binding; otherwise the recovery op's id.
    pub fn start_setup(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
    ) -> Result<Option<[u8; 16]>> {
        self.start_setup_call(preview, history, None)
    }
    /// One named no-question start, called whatever the decision is.
    pub fn force_setup_start(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
        start: cloud::setup::SilentStart,
    ) -> Result<Option<[u8; 16]>> {
        self.start_setup_call(preview, history, Some(start))
    }
    fn start_setup_call(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
        start: Option<cloud::setup::SilentStart>,
    ) -> Result<Option<[u8; 16]>> {
        let started = self
            .sdk
            .get_mut()
            .driver
            .call(Input::SetupStart(preview.production.clone(), history, start))?;
        self.work = None;
        self.refresh()?;
        Ok(started)
    }
    /// V27.4's "Keep the iCloud collection" for a preview whose binding could
    /// resume, under a supplied history. The recovery op's id.
    pub fn keep_cloud_over_resumable(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
    ) -> Result<[u8; 16]> {
        let id = self
            .sdk
            .get_mut()
            .driver
            .call(Input::SetupKeepCloud(preview.production.clone(), history))?;
        self.work = None;
        self.refresh()?;
        Ok(id)
    }
    /// V27.4's "Keep this device's collection" under a supplied history;
    /// `confirm_publication` is this call with the history unknown.
    pub fn keep_device_over_active(
        &mut self,
        preview: &ConnectionPreview,
        history: cloud::setup::OtherServiceHistory,
    ) -> Result<[u8; 16]> {
        let id = self
            .sdk
            .get_mut()
            .driver
            .call(Input::SetupKeepDevice(preview.production.clone(), history))?;
        self.work = None;
        self.refresh()?;
        Ok(id)
    }
    pub fn adopt_cloud(&mut self, preview: &ConnectionPreview) -> Result<[u8; 16]> {
        let id = self
            .sdk
            .get_mut()
            .driver
            .call(Input::AdoptionConfirm(preview.production.clone()))?;
        self.work = None;
        self.refresh()?;
        Ok(id)
    }
    fn deliver_adoption(
        &mut self,
        service: &mut Service<SeededRandom>,
        work: cloud::adoption::AdoptionWork,
    ) -> Result<()> {
        use cloud::adoption::{AdoptionReply, AdoptionTask as T, StagingRecord};
        if matches!(work.task, T::Validate | T::Activate) {
            return self.sdk.get_mut().driver.call(Input::AdoptionLocal(work));
        }
        let epoch = self
            .control()?
            .recovery_ops
            .into_iter()
            .find(|p| p.id == work.op)
            .ok_or_else(failure)?
            .epoch;
        // These exact parent-owned files survive the synchronous child call,
        // including any IPC pause, until production adoption has copied them.
        let mut payload_files = Vec::new();
        let reply = match &work.task {
            T::ReadActive | T::RecheckActive => {
                AdoptionReply::Read(self.read(service, "meguri-root".into(), "active", RequestedFields::All)?)
            }
            T::ReadEpoch => AdoptionReply::Read(self.read(
                service,
                cloud::adapter::collection_zone(epoch),
                "epoch",
                RequestedFields::All,
            )?),
            T::ReadMediaRoot => {
                AdoptionReply::Read(self.read(service, "meguri-media".into(), "root", RequestedFields::All)?)
            }
            T::FetchPayload {
                name,
                encrypted,
                system_fields,
            } => {
                let response = service.request(
                    &self.scope,
                    Caller::default(),
                    Operation::Read {
                        zone: cloud::adapter::collection_zone(epoch),
                        name: name.clone(),
                        fields: RequestedFields::All,
                    },
                )?;
                match response.result {
                    Ok(Reply::Record(record))
                        if record.name == *name
                            && record.record_type == "MGEntity"
                            && record.system_fields.bytes() == system_fields.0
                            && matches!(record.fields.get("e"), Some(Field::EncryptedBytes(bytes)) if *bytes == encrypted.0) =>
                    {
                        if let Some(Field::Asset(asset)) = record.fields.get("pa") {
                            use std::io::Write;
                            let mut file = tempfile::NamedTempFile::new()?;
                            file.write_all(asset.bytes())?;
                            file.as_file().sync_all()?;
                            let path = file.path().to_path_buf();
                            payload_files.push(file);
                            AdoptionReply::Payload {
                                record: StagingRecord {
                                    name: name.clone(),
                                    record_type: "MGEntity".into(),
                                    record: ControlRecord {
                                        encrypted: Some(encrypted.0.clone()),
                                        system_fields: system_fields.clone(),
                                    },
                                    payload_file: Some(path),
                                },
                            }
                        } else {
                            AdoptionReply::Failed
                        }
                    }
                    _ => AdoptionReply::Failed,
                }
            }
            T::Fetch { cursor } => {
                let token = cursor.as_ref().map(|c| serde_json::from_slice(&c.0)).transpose()?;
                let response = service.request(
                    &self.scope,
                    Caller::default(),
                    Operation::FetchZone {
                        zone: cloud::adapter::collection_zone(epoch),
                        token,
                    },
                )?;
                let Ok(Reply::ZoneChanges(page)) = response.result else {
                    return Err(failure());
                };
                let mut records = Vec::new();
                let mut deleted = Vec::new();
                for change in page.changes {
                    match change {
                        RecordChange::Saved(record) => {
                            let payload_file = match record.fields.get("pa") {
                                Some(Field::Asset(_))
                                    if record.record_type == "MGEntity" && self.adoption_metadata_first =>
                                {
                                    None
                                }
                                Some(Field::Asset(asset)) if record.record_type == "MGEntity" => {
                                    use std::io::Write;
                                    let mut file = tempfile::NamedTempFile::new()?;
                                    file.write_all(asset.bytes())?;
                                    file.as_file().sync_all()?;
                                    let path = file.path().to_path_buf();
                                    payload_files.push(file);
                                    Some(path)
                                }
                                None => None,
                                _ => return Err(failure()),
                            };
                            if record
                                .fields
                                .iter()
                                .any(|(key, field)| key != "pa" && matches!(field, Field::Asset(_)))
                            {
                                return Err(failure());
                            }
                            let encrypted = match record.fields.get("e") {
                                Some(Field::EncryptedBytes(e)) => Some(e.clone()),
                                _ => None,
                            };
                            records.push(StagingRecord {
                                name: record.name,
                                record_type: record.record_type,
                                record: ControlRecord {
                                    encrypted,
                                    system_fields: cloud::OpaqueBytes(record.system_fields.bytes().to_vec()),
                                },
                                payload_file,
                            });
                        }
                        RecordChange::Deleted { name, .. } => deleted.push(name),
                    }
                }
                let next = if page.more {
                    Some(cloud::OpaqueBytes(serde_json::to_vec(&page.token)?))
                } else {
                    None
                };
                AdoptionReply::Page { records, deleted, next }
            }
            _ => return Err(failure()),
        };
        let result = self.sdk.get_mut().driver.call(Input::AdoptionReply(work, reply));
        drop(payload_files);
        result
    }
    /// Project actual Service metadata first; resolve each payload from its
    /// current immutable Service version when Core asks for it.
    pub fn use_metadata_first_adoption(&mut self) {
        self.adoption_metadata_first = true;
    }
    pub fn verify_identity(&mut self, account: String) -> Result<()> {
        self.sdk.get_mut().driver.call::<()>(Input::Identity(Some(account)))?;
        self.refresh()
    }
    pub fn cancel_recovery_op(&mut self, id: [u8; 16]) -> Result<()> {
        self.sdk.get_mut().driver.call::<()>(Input::PublicationCancel(id))?;
        self.work = None;
        self.held = None;
        self.scan_committed = false;
        self.refresh()
    }
    fn ensure_sdk(&mut self) -> Result<()> {
        let host: String = self.sdk.get_mut().driver.call(Input::Instance)?;
        if self.started_host_instance.as_ref() != Some(&host) {
            let old = self
                .started_host_instance
                .as_ref()
                .map(|_| self.sdk.get_mut().instance());
            let (instance, serialization, zone): (String, Option<Vec<u8>>, String) =
                self.sdk.get_mut().driver.call(Input::RegisterSDK(old))?;
            self.needs_no_state_listing = serialization.is_none();
            self.sdk.get_mut().install_instance(instance, zone);
            self.started_host_instance = Some(host);
        }
        Ok(())
    }
    fn prepare_sdk(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<bool> {
        if self.held.is_some() {
            return Ok(false);
        }
        let scan: super::super::child_driver::ScanState = self.sdk.get_mut().driver.call(Input::ScanState)?;
        if scan.tracking_stopped && !self.explicit_sync_pending {
            return Ok(false);
        }
        if scan.scan_required || self.sdk.get_mut().driver.has_incoming_reply(&Input::Reconcile)? {
            *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
            if self.gate(Point::BeforeScan) {
                return Ok(false);
            }
            self.sdk.get_mut().driver.call::<()>(Input::Reconcile)?;
        }
        self.ensure_sdk()?;
        if self.sdk.get_mut().root.is_none() && !self.sdk.get_mut().driver.call::<bool>(Input::RootConfirmed)? {
            self.sdk.get_mut().confirm_root_read(service, remaining)?;
            self.needs_no_state_listing = false;
        }
        Ok(true)
    }
    fn submit_explicit_sync(&mut self) -> Result<()> {
        if self.explicit_sync_pending {
            self.sdk.get_mut().sync_now()?;
            self.explicit_sync_pending = false;
        }
        Ok(())
    }
    fn remember_replies(&mut self, request: &OwnRequest, replies: Vec<Response>) {
        for response in replies {
            let mutations = match &request.operation {
                OwnOperation::Save {
                    name,
                    record_type,
                    encrypted,
                    if_unchanged,
                    ..
                } => vec![Mutation::Save {
                    record: RecordInput::encrypted(name, record_type, encrypted.clone()),
                    if_unchanged: if_unchanged.as_ref().map(|s| SystemFields::from_bytes(s.0.clone())),
                }],
                _ => Vec::new(),
            };
            self.own_sent.insert(
                response.request_id,
                ResponseBatch {
                    instance: String::new(),
                    // Publication-owned operations have no SDK collection batch.
                    backend_batch_id: 0,
                    mutations,
                    response,
                },
            );
        }
    }
    pub fn sent_request(&self, id: u64) -> Result<ResponseBatch> {
        self.own_sent
            .get(&id)
            .cloned()
            .map(Ok)
            .unwrap_or_else(|| self.sdk.borrow().sent_request(id))
    }
    pub fn held_request(&mut self) -> Result<HeldRequest> {
        if self.is_held(Point::BeforeCollectionSave) {
            return self.sdk.get_mut().driver.call(Input::IncomingSubmission);
        }
        if self.held == Some(Point::ZoneDeletionReply) {
            return self.sdk.get_mut().driver.queued_request();
        }
        let Some(PublicationWork::Own(request)) = &self.work else {
            return Err(failure());
        };
        if !matches!(
            self.held,
            Some(
                Point::BeforeCompleteEpochSave
                    | Point::BeforeActiveSave
                    | Point::BeforeResetMediaRootRead
                    | Point::BeforeMediaRootSave
            )
        ) {
            return Err(failure());
        }
        if self.transport_queued {
            return self.sdk.get_mut().driver.queued_request();
        }
        let queued = self
            .sdk
            .get_mut()
            .driver
            .enqueue(Input::PublicationExecute(*request.clone()))?;
        self.transport_queued = true;
        Ok(queued)
    }
    /// Consume one captured reset-root reply with the original production
    /// request, then stop before scheduling any subsequent recovery operation.
    pub fn deliver_reset_media_root_reply(
        &mut self,
        service: &mut Service<SeededRandom>,
        result: std::result::Result<Reply, ServiceError>,
    ) -> Result<Response> {
        let Some(PublicationWork::Own(request)) = self.work.clone() else {
            return Err(failure());
        };
        if self.held != Some(Point::BeforeResetMediaRootRead) || request.kind != OwnKind::ReadResetMediaRoot {
            return Err(failure());
        }
        self.held_request()?;
        let original = self
            .sdk
            .get_mut()
            .driver
            .capture_reset_media_root_reply(service, &self.scope, result)?;
        let replies = self.sdk.get_mut().driver.resume(service, &self.scope)?;
        self.remember_replies(&request, replies);
        self.transport_queued = false;
        self.work = None;
        self.release(Point::BeforeResetMediaRootRead)?;
        self.refresh()?;
        Ok(original)
    }
    pub fn drive_ready(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let result = (|| {
            self.sdk.get_mut().driver.resume_incoming_work()?;
            self.sdk.get_mut().resume_fetch_callback()?;
            if !self.sdk.get_mut().complete_retained_media_callback()? {
                return self.refresh();
            }
            let scan: super::super::child_driver::ScanState = self.sdk.get_mut().driver.call(Input::ScanState)?;
            if scan.tracking_stopped {
                return self.refresh();
            }
            let mut remaining = steps;
            let resuming_explicit = self.explicit_sync_pending;
            self.drive_budget(service, &mut remaining)?;
            if resuming_explicit && !self.explicit_sync_pending {
                self.finish_collection_drain(service, &mut remaining)?;
            }
            Ok(())
        })();
        self.finish_worker_drive(result)
    }
    fn finish_worker_drive(&mut self, result: Result<()>) -> Result<()> {
        match result {
            Err(error) if super::super::child_driver::is_work_paused(error.as_ref()) => {
                self.last_sync_phase = "finish_worker_drive_refresh";
                self.refresh()
            }
            result => result,
        }
    }
    fn drive_budget(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        while *remaining > 0 {
            *remaining -= 1;
            if self.held.is_some() {
                return self.refresh();
            }
            if self.scan_committed {
                if self.gate(Point::ScanCommitted) {
                    return self.refresh();
                }
                self.scan_committed = false;
            }
            if self.cleanup_pending {
                if self.gate(Point::BeforeZoneDeletion) {
                    return self.refresh();
                }
                self.cleanup_pending = false;
                let mut recheck = self.cleanup_reply_pending;
                if self.cleanup_reply_pending {
                    self.cleanup_reply_pending = false;
                    self.sdk.get_mut().driver.resume::<Response>(service, &self.scope)?;
                } else {
                    // The candidate list is finite for this drained cleanup.
                    self.sdk.get_mut().driver.call::<()>(Input::DeletionGuard)?;
                    let read = self.read(service, "meguri-root".into(), "active", RequestedFields::All)?;
                    let zones: Vec<String> = self.sdk.get_mut().driver.call(Input::DeletionCheck(read))?;
                    self.cleanup_zones = zones.into();
                }
                while let Some(zone) = self.cleanup_zones.pop_front() {
                    *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
                    if recheck {
                        let read = self.read(service, "meguri-root".into(), "active", RequestedFields::All)?;
                        let authorized: Vec<String> = self.sdk.get_mut().driver.call(Input::DeletionCheck(read))?;
                        if !authorized.contains(&zone) {
                            continue;
                        }
                    }
                    recheck = true;
                    if self.holds.contains(&Point::ZoneDeletionReply) {
                        self.sdk
                            .get_mut()
                            .driver
                            .hold_reply(Input::DeleteZone(zone), service, &self.scope)?;
                        self.cleanup_pending = true;
                        self.cleanup_reply_pending = true;
                        self.gate(Point::ZoneDeletionReply);
                        return self.refresh();
                    }
                    self.sdk
                        .get_mut()
                        .driver
                        .execute::<Response>(Input::DeleteZone(zone), service, &self.scope)?;
                }
                continue;
            }
            if self.work.is_none() {
                self.work = Some(self.sdk.get_mut().driver.call(Input::PublicationNext)?);
            }
            let work = self.work.clone().ok_or_else(failure)?;
            let point = match &work {
                PublicationWork::Local {
                    work: LocalWork::Bind, ..
                } => Some(Point::Confirmed),
                PublicationWork::Local {
                    work: LocalWork::WriteDecision,
                    ..
                } => Some(Point::BoundBeforeDecision),
                PublicationWork::Own(r) => match r.kind {
                    OwnKind::CreateZones => Some(Point::BoundAfterDecision),
                    OwnKind::SaveCompleteEpoch => Some(Point::BeforeCompleteEpochSave),
                    OwnKind::ReadOwnEpoch => Some(Point::BeforeEpochRead),
                    OwnKind::SaveActive => Some(Point::BeforeActiveSave),
                    OwnKind::ReadResetMediaRoot => Some(Point::BeforeResetMediaRootRead),
                    OwnKind::SaveMediaRoot => Some(Point::BeforeMediaRootSave),
                    _ => None,
                },
                PublicationWork::Adoption(work) if work.task == cloud::adoption::AdoptionTask::ReadActive => {
                    Some(Point::AdoptionStagingRecorded)
                }
                PublicationWork::Adoption(work) if work.task == cloud::adoption::AdoptionTask::Activate => {
                    Some(Point::AdoptionBeforeActivate)
                }
                PublicationWork::Adoption(work)
                    if matches!(work.task, cloud::adoption::AdoptionTask::Fetch { cursor: Some(_) }) =>
                {
                    Some(Point::AdoptionBeforeNextFetch)
                }
                PublicationWork::Adoption(work)
                    if matches!(work.task, cloud::adoption::AdoptionTask::FetchPayload { .. }) =>
                {
                    Some(Point::AdoptionBeforeFetchPayload)
                }
                PublicationWork::Scan => Some(Point::BeforeScan),
                _ => None,
            };
            if let Some(point) = point
                && self.gate(point)
            {
                return self.refresh();
            }
            match work {
                PublicationWork::Adoption(work) => self.deliver_adoption(service, work)?,
                PublicationWork::Local { op, work } => self
                    .sdk
                    .get_mut()
                    .driver
                    .call::<()>(Input::PublicationLocal(op, work))?,
                PublicationWork::Own(request) => {
                    let replies = if self.transport_queued {
                        self.transport_queued = false;
                        self.sdk.get_mut().driver.resume(service, &self.scope)?
                    } else {
                        self.sdk.get_mut().driver.execute(
                            Input::PublicationExecute(*request.clone()),
                            service,
                            &self.scope,
                        )?
                    };
                    self.remember_replies(&request, replies);
                }
                PublicationWork::Scan => {
                    self.sdk.get_mut().driver.call::<()>(Input::Reconcile)?;
                    self.scan_committed = true;
                }
                PublicationWork::SendEntities => {
                    self.ensure_sdk()?;
                    if self.needs_no_state_listing {
                        self.sdk.get_mut().qualify_budget(service, remaining)?;
                        self.needs_no_state_listing = false;
                    }
                    self.sdk.get_mut().drive_media_budget(service, remaining)?;
                    self.work = None;
                    if self
                        .control()?
                        .recovery_ops
                        .iter()
                        .any(|op| op.step == PublishStep::Zones)
                    {
                        return self.refresh();
                    }
                    continue;
                }
                PublicationWork::Idle => {
                    self.work = None;
                    let facts = self.control()?;
                    if !facts.recovery_ops.is_empty() {
                        return self.refresh();
                    }
                    if facts
                        .current
                        .as_ref()
                        .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
                    {
                        if !self.prepare_sdk(service, remaining)? {
                            return self.refresh();
                        }
                        self.submit_explicit_sync()?;
                        self.sdk.get_mut().drive_media_budget(service, remaining)?;
                    }
                    return self.refresh();
                }
            }
            self.work = None;
        }
        Err(failure())
    }
    /// Fixed parent phase labels only; leaves the original error available to callers.
    pub fn last_sync_phase(&self) -> &'static str {
        self.last_sync_phase
    }
    pub fn sync_now_and_drain(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let result = (|| {
            self.last_sync_phase = "resume_incoming_work";
            self.sdk.get_mut().driver.resume_incoming_work()?;
            self.last_sync_phase = "resume_fetch_callback";
            self.sdk.get_mut().resume_fetch_callback()?;
            self.last_sync_phase = "complete_retained_media_callback";
            if !self.sdk.get_mut().complete_retained_media_callback()? {
                return self.refresh();
            }
            self.last_sync_phase = "publication_trigger";
            self.sdk.get_mut().driver.call::<()>(Input::PublicationTrigger)?;
            self.last_sync_phase = "media_resume";
            self.sdk.get_mut().driver.call::<()>(Input::MediaResume)?;
            let mut remaining = steps;
            self.last_sync_phase = "read_publication_control";
            if self.control()?.recovery_ops.is_empty() {
                self.explicit_sync_pending = true;
                self.last_sync_phase = "prepare_sdk";
                if !self.prepare_sdk(service, &mut remaining)? {
                    return self.refresh();
                }
                self.last_sync_phase = "submit_explicit_sync";
                self.submit_explicit_sync()?;
            }
            self.last_sync_phase = "drive_budget";
            self.drive_budget(service, &mut remaining)?;
            self.last_sync_phase = "finish_collection_drain";
            self.finish_collection_drain(service, &mut remaining)?;
            if self.held.is_some() {
                return self.refresh();
            }
            self.sdk.get_mut().driver.call::<()>(Input::MediaResume)?;
            self.last_sync_phase = "drive_cloud_media";
            self.sdk.get_mut().drive_media_budget(service, &mut remaining)
        })();
        self.finish_worker_drive(result)
    }
    fn finish_collection_drain(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        if self.held.is_some() {
            return Ok(());
        }
        let facts = self.control()?;
        if facts.recovery_ops.is_empty()
            && facts
                .current
                .as_ref()
                .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
        {
            self.sdk.get_mut().driver.call::<()>(Input::CollectionDrained)?;
            if self.gate(Point::AfterCollectionCleanup) {
                return Ok(());
            }
            if self.control()?.zones.values().any(|z| z.deletion_due) {
                self.cleanup_pending = true;
                self.drive_budget(service, remaining)?;
            }
        }
        Ok(())
    }
    pub fn fetch_without_sending(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        self.sdk.get_mut().fetch_recovery_exchange(service, steps)
    }
    pub fn select_media_batch(&mut self) -> Result<Option<(u64, cloud::media::Work)>> {
        self.sdk.get_mut().select_media_batch()
    }
    pub fn send_selected_media(
        &mut self,
        service: &mut Service<SeededRandom>,
        selected: (u64, cloud::media::Work),
    ) -> Result<ResponseBatch> {
        self.sdk.get_mut().request_selected_media_response(service, selected)
    }
    pub fn deliver_media_response(&mut self, response: &Response) -> Result<()> {
        self.sdk.get_mut().deliver_sent_event(response)?;
        self.sdk.get_mut().return_sent_handler()
    }
    /// The seeded ordinary runner starts only after actual publication/adoption.
    /// Recovery/account/lifecycle programs require their own future grants.
    pub(crate) fn begin_ordinary(&mut self) -> Result<()> {
        let control = self.control()?;
        if self.held.is_some()
            || self.work.is_some()
            || !control.recovery_ops.is_empty()
            || !control
                .current
                .as_ref()
                .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
            || self.needs_no_state_listing
            || self.started_host_instance.is_none()
        {
            return Err(failure());
        }
        self.sdk.get_mut().ordinary_start()
    }
    pub(crate) fn ordinary_action(&self) -> Option<adapter::OrdinaryAction> {
        self.sdk.borrow().ordinary_action()
    }
    pub(crate) fn ordinary_step(&mut self, service: &mut Service<SeededRandom>) -> Result<adapter::OrdinaryReceipt> {
        self.sdk.get_mut().ordinary_step(service)
    }
    pub(crate) fn ordinary_callback_scope(&self) -> Option<adapter::FetchScope> {
        self.sdk.borrow().ordinary_callback_scope()
    }
    pub(crate) fn ordinary_idle(&self) -> bool {
        self.sdk.borrow().ordinary_idle()
    }
    pub(crate) fn ordinary_has_callback(&self) -> bool {
        self.sdk.borrow().ordinary_held_callback()
    }
    pub(crate) fn ordinary_counts(&mut self) -> Result<(usize, usize, usize)> {
        let entities: Vec<cloud::workflow::EntityFacts> = self.sdk.get_mut().driver.call(Input::AllEntities)?;
        let pending = entities
            .iter()
            .filter(|e| e.mark.is_some() || e.local_rev > e.acked_rev)
            .count();
        let flights = entities.iter().map(|e| e.in_flight.len()).sum();
        let control = self.control()?;
        let binding = control.current.ok_or_else(failure)?;
        let inbox = self.sdk.get_mut().driver.inbox_entries(binding.id)?.len();
        Ok((pending, flights, inbox))
    }
    pub fn advance_clock_ms(&mut self, ms: i64) -> Result<()> {
        self.sdk.get_mut().advance_clock_ms(ms)
    }
    pub fn kill(&mut self) -> Result<super::super::child_driver::Killed> {
        self.sdk.get_mut().driver.kill_sigkill()
    }
    pub fn close_clean(&mut self) -> Result<()> {
        self.sdk.get_mut().close_clean()
    }
}
pub(crate) fn read_result(response: &Response) -> Result<RecordRead> {
    Ok(match &response.result {
        Ok(Reply::Record(record)) => RecordRead::Found(super::publication_control_record(record)?),
        Err(error) if error.code == ErrorCode::UnknownItem => RecordRead::Missing,
        Err(error) if error.code == ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
        _ => RecordRead::Failed,
    })
}
impl ChildDriver {
    /// SQL-backed collection rows only, matching the backup projection.
    pub fn collection_rows(&mut self) -> Result<Vec<cloud::entity_codec::Payload>> {
        self.call(Input::CollectionRows)
    }
    /// Read-only MediaName projection from the independent journal and files.
    pub fn media_rows(&mut self) -> Result<Vec<cloud::entity_codec::Payload>> {
        self.call(Input::MediaRows)
    }
    pub fn collection_header(&mut self) -> Result<CollectionHeader> {
        self.call(Input::CollectionHeader)
    }
    pub fn backup_now(&mut self) -> Result<BackupResult> {
        self.call(Input::BackupNow)
    }
    pub fn restore_backup(&mut self, backup: &BackupResult) -> Result<meguri_package::restore::RestoredBackup> {
        Ok(self
            .call::<RestoredWire>(Input::RestoreBackup(backup.name.clone()))?
            .into())
    }
    pub fn backup_path(&mut self, name: &str) -> Result<PathBuf> {
        self.call(Input::BackupPath(name.into()))
    }
    pub fn backup_collection_rows(&mut self, name: &str) -> Result<Vec<cloud::entity_codec::Payload>> {
        self.call(Input::BackupRows(name.into()))
    }
    pub fn set_backup_retention(&mut self, daily: u32, weekly: u32, monthly: u32) -> Result<()> {
        self.call(Input::BackupRetention(daily, weekly, monthly))
    }
    pub fn put_media(&mut self, name: &str, bytes: &[u8]) -> Result<String> {
        self.call(Input::PutMedia(name.into(), bytes.to_vec()))
    }
    pub fn media_files(&mut self) -> Result<BTreeMap<String, Vec<u8>>> {
        self.call(Input::MediaFiles)
    }
    pub fn wants_send(&mut self) -> Result<bool> {
        self.call(Input::WantsSend)
    }
    pub fn send_changes_calls(&mut self) -> Result<Vec<cloud::workflow::SendCall>> {
        self.call(Input::Calls)
    }
}
