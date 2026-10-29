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
    BoundBeforeDecision,
    BoundAfterDecision,
    BeforeScan,
    ScanCommitted,
    BeforeEpochRead,
    BeforeActiveSave,
    BeforeZoneDeletion,
    ZoneDeletionReply,
    ResolutionMoveBeforeRewrite,
    MediaTrashBeforeInstall,
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
    scan_committed: bool,
    transport_queued: bool,
    started_host_instance: Option<String>,
    needs_no_state_listing: bool,
    own_sent: BTreeMap<u64, ResponseBatch>,
    trace: Vec<cloud::workflow::TraceEvent>,
    observations: Vec<Observation>,
    binding_inputs: Vec<BindingInput>,
    cleanup_pending: bool,
    cleanup_zones: VecDeque<String>,
    cleanup_reply_pending: bool,
}
impl PublicationRig {
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
            own_sent: BTreeMap::new(),
            trace: Vec::new(),
            observations: Vec::new(),
            binding_inputs: Vec::new(),
            cleanup_pending: false,
            cleanup_zones: VecDeque::new(),
            cleanup_reply_pending: false,
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
    pub fn facts(&self) -> Result<PublicationFacts> {
        self.sdk.borrow_mut().driver.call(Input::PublicationFacts)
    }
    pub fn recovery_state(&self) -> Result<RecoveryState> {
        self.sdk.borrow_mut().driver.call(Input::RecoveryState)
    }
    pub fn collection_zone(&self) -> Result<String> {
        Ok(cloud::adapter::collection_zone(
            self.facts()?.current.ok_or_else(failure)?.facts.epoch,
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
    pub fn hold(&mut self, point: Point) -> Result<()> {
        self.holds.insert(point);
        Ok(())
    }
    pub fn is_held(&self, point: Point) -> bool {
        self.held == Some(point)
    }
    pub fn release(&mut self, point: Point) -> Result<()> {
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
            .facts()?
            .recovery_ops
            .into_iter()
            .find(|p| p.id == work.op)
            .ok_or_else(failure)?
            .epoch;
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
        self.sdk.get_mut().driver.call(Input::AdoptionReply(work, reply))
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
        if self.held == Some(Point::ZoneDeletionReply) {
            return self.sdk.get_mut().driver.queued_request();
        }
        let Some(PublicationWork::Own(request)) = &self.work else {
            return Err(failure());
        };
        if self.held != Some(Point::BeforeActiveSave) {
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
    pub fn drive_ready(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let mut remaining = steps;
        self.drive_budget(service, &mut remaining)
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
                    OwnKind::ReadOwnEpoch => Some(Point::BeforeEpochRead),
                    OwnKind::SaveActive => Some(Point::BeforeActiveSave),
                    _ => None,
                },
                PublicationWork::Adoption(work) if work.task == cloud::adoption::AdoptionTask::ReadActive => {
                    Some(Point::AdoptionStagingRecorded)
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
                    self.sdk.get_mut().drive_budget(service, remaining)?;
                    self.work = None;
                    if self
                        .facts()?
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
                    let facts = self.facts()?;
                    if !facts.recovery_ops.is_empty() {
                        return self.refresh();
                    }
                    if facts
                        .current
                        .as_ref()
                        .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
                    {
                        let scan: super::super::child_driver::ScanState =
                            self.sdk.get_mut().driver.call(Input::ScanState)?;
                        if scan.scan_required {
                            if self.gate(Point::BeforeScan) {
                                return self.refresh();
                            }
                            self.sdk.get_mut().driver.call::<()>(Input::Reconcile)?;
                        }
                        self.ensure_sdk()?;
                        if self.sdk.get_mut().root.is_none()
                            && !self.sdk.get_mut().driver.call::<bool>(Input::RootConfirmed)?
                        {
                            self.sdk.get_mut().confirm_root_read(service, remaining)?;
                            self.needs_no_state_listing = false;
                        }
                        self.sdk.get_mut().drive_budget(service, remaining)?;
                    }
                    return self.refresh();
                }
            }
            self.work = None;
        }
        Err(failure())
    }
    pub fn sync_now_and_drain(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        self.sdk.get_mut().driver.call::<()>(Input::PublicationTrigger)?;
        if self.facts()?.recovery_ops.is_empty() {
            self.ensure_sdk()?;
            self.sdk.get_mut().sync_now()?;
        }
        let mut remaining = steps;
        self.drive_budget(service, &mut remaining)?;
        if self.held.is_some() {
            return Ok(());
        }
        let facts = self.facts()?;
        if facts.recovery_ops.is_empty()
            && facts
                .current
                .as_ref()
                .is_some_and(|b| b.facts.state == cloud::BindingState::Active)
        {
            self.sdk.get_mut().driver.call::<()>(Input::CollectionDrained)?;
            if self.facts()?.zones.values().any(|z| z.deletion_due) {
                self.cleanup_pending = true;
                self.drive_budget(service, &mut remaining)?;
            }
        }
        Ok(())
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
    pub fn collection_rows(&mut self) -> Result<Vec<cloud::entity_codec::Payload>> {
        self.call(Input::CollectionRows)
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
