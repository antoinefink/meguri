//! Deterministic SDK stand-in. Policy, capture, actions and acknowledgements run
//! in the production runtime in the shared CoreHost child.
use super::{
    child_driver::{ChildDriver, Input},
    devices::{IdentityReads, PreboundDevice, Result, failure},
    *,
};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::workflow::{
    self, BuiltBatch, EntityFacts, ExchangeResult, FetchPhase, Saved, SendCall, SendError,
};
use std::collections::{BTreeMap, BTreeSet};
pub use workflow::{Observation, PendingChange, PendingSnapshot, PendingUpdate, UpdateKind};
#[derive(Clone, Debug)]
pub struct BatchAttempt {
    pub mutations: Vec<Mutation>,
    pub request_ids: Vec<u64>,
}
#[derive(Clone, Debug)]
pub struct ResponseBatch {
    pub instance: String,
    pub mutations: Vec<Mutation>,
    pub response: Response,
}
#[derive(Clone, Debug)]
pub struct RootReadProof {
    pub instance: String,
    pub request_id: u64,
    pub persisted_epoch: meguri_sync::cloud::EpochId,
    pub accepted_record: Record,
}
/// SDK pending changes deduplicate by name, including save/delete replacement.
/// The SDK retains successful names until the complete sent handler returns.
pub(crate) struct SDKFiles {
    pub scope: Scope,
    pub collection_zone: String,
}
pub struct SDKRig {
    pub(crate) driver: ChildDriver,
    pub(crate) files: SDKFiles,
    pub(crate) instance: String,
    pending: BTreeMap<String, PendingChange>,
    database_pending: BTreeMap<String, PendingChange>,
    applied: Vec<PendingUpdate>,
    observations: Vec<Observation>,
    emitted: Vec<u64>,
    sent: BTreeMap<u64, ResponseBatch>,
    calls: BTreeMap<u64, u64>,
    waiting: Option<u64>,
    handler: Option<u64>,
    pub(crate) root: Option<RootReadProof>,
}
impl SDKRig {
    pub fn open(files: &PreboundDevice, automatic: bool, now: i64, seed: u64) -> Result<Self> {
        Self::open_with_identity(
            files,
            IdentityReads::Available(files.scope.account.clone()),
            automatic,
            now,
            seed,
        )
    }
    pub(crate) fn open_with_identity(
        files: &PreboundDevice,
        identity: IdentityReads,
        automatic: bool,
        now: i64,
        seed: u64,
    ) -> Result<Self> {
        let mut driver = ChildDriver::spawn(files, identity, automatic, now, seed)?;
        let instance = driver.call(Input::Instance)?;
        Ok(Self::from_driver(
            driver,
            files.scope.clone(),
            instance,
            files.collection_zone.clone(),
        ))
    }
    pub(crate) fn from_driver(driver: ChildDriver, scope: Scope, instance: String, collection_zone: String) -> Self {
        Self {
            driver,
            files: SDKFiles { scope, collection_zone },
            instance,
            pending: BTreeMap::new(),
            database_pending: BTreeMap::new(),
            applied: Vec::new(),
            observations: Vec::new(),
            emitted: Vec::new(),
            sent: BTreeMap::new(),
            calls: BTreeMap::new(),
            waiting: None,
            handler: None,
            root: None,
        }
    }
    pub(crate) fn install_instance(&mut self, instance: String, zone: String) {
        self.instance = instance;
        self.files.collection_zone = zone;
        self.pending.clear();
        self.database_pending.clear();
        self.waiting = None;
        self.handler = None;
        self.root = None;
    }
    pub(crate) fn database_pending(&self) -> BTreeMap<String, PendingChange> {
        self.database_pending.clone()
    }
    pub fn driver(&mut self) -> &mut ChildDriver {
        &mut self.driver
    }
    pub fn instance(&self) -> String {
        self.instance.clone()
    }
    pub(crate) fn refresh(&mut self) -> Result<()> {
        self.observations = self.driver.call(Input::Observations)?;
        self.emitted = self.driver.call(Input::Emitted)?;
        Ok(())
    }
    pub fn observations(&self) -> &[Observation] {
        &self.observations
    }
    pub fn emitted_update_ids(&self) -> &[u64] {
        &self.emitted
    }
    pub fn applied_update_ids(&self) -> Vec<u64> {
        self.applied.iter().map(|u| u.id).collect()
    }
    pub fn applied_updates(&self) -> &[PendingUpdate] {
        &self.applied
    }
    pub fn sdk_pending(&self) -> BTreeMap<String, PendingChange> {
        self.pending.clone()
    }
    pub fn core_tracking_sendables(&mut self) -> Result<BTreeMap<String, PendingChange>> {
        self.driver.call(Input::Sendables)
    }
    pub fn entity_facts(&mut self, name: &str) -> Result<EntityFacts> {
        self.driver.call(Input::Entity(name.into()))
    }
    pub fn tracking_pending_names(&mut self) -> Result<BTreeSet<String>> {
        let facts: Vec<EntityFacts> = self.driver.call(Input::AllEntities)?;
        Ok(facts.into_iter().filter(|e| e.mark.is_some()).map(|e| e.name).collect())
    }
    pub fn core_pending_transition(&mut self, kind: UpdateKind, snapshot: PendingSnapshot) -> Result<PendingUpdate> {
        let u = self
            .driver
            .call(Input::Transition(self.instance.clone(), kind, snapshot))?;
        self.refresh()?;
        Ok(u)
    }
    fn updates(&mut self) -> Result<()> {
        let updates: Vec<PendingUpdate> = self.driver.call(Input::Updates(self.instance.clone()))?;
        for u in updates {
            for name in &u.removes {
                self.pending.remove(name);
            }
            let mut added = Vec::new();
            for (name, change) in &u.adds {
                if self.pending.insert(name.clone(), *change) != Some(*change) {
                    added.push(name.clone());
                }
            }
            self.driver
                .call::<()>(Input::AppliedUpdate(self.instance.clone(), u.id, added))?;
            self.applied.push(u);
        }
        self.refresh()
    }
    pub(crate) fn caller(&self) -> Caller {
        Caller {
            instance: Some(self.instance.clone()),
            ..Caller::default()
        }
    }
    pub(crate) fn qualify_no_state(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let mut remaining = steps;
        self.qualify_budget(service, &mut remaining)
    }
    pub(crate) fn qualify_budget(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        let mut token = None;
        while *remaining > 0 {
            *remaining -= 1;
            let r = service.request(&self.files.scope, self.caller(), Operation::FetchDatabase { token })?;
            let Ok(Reply::DatabaseChanges(page)) = r.result else {
                return Err(failure());
            };
            // Persist actual zone announcements through the runtime, independently
            // of root facts or the Service inspection API.
            let zones = page
                .changes
                .iter()
                .filter(|change| change.removed.is_none())
                .map(|change| change.zone.clone())
                .collect();
            self.driver.call::<()>(Input::ZonesSeen(zones))?;
            if !page.more {
                self.driver.call::<()>(Input::Qualify(self.instance.clone()))?;
                return Ok(());
            }
            token = Some(page.token);
        }
        Err(failure())
    }
    fn fetch_exchange(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        let phase: Option<FetchPhase> = self.driver.call(Input::FetchDue)?;
        let Some(phase) = phase else { return Ok(()) };
        let mut seen = BTreeSet::new();
        for root_zone in [true, false] {
            let zone = if root_zone {
                "meguri-root".to_owned()
            } else {
                self.files.collection_zone.clone()
            };
            let mut token = None;
            let mut active = None;
            loop {
                *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
                let response = service.request(
                    &self.files.scope,
                    self.caller(),
                    Operation::FetchZone {
                        zone: zone.clone(),
                        token,
                    },
                )?;
                let Ok(Reply::ZoneChanges(page)) = response.result else {
                    return Err(failure());
                };
                let mut entities = Vec::new();
                for change in page.changes {
                    match change {
                        RecordChange::Saved(record)
                            if root_zone && record.name == "active" && record.record_type == "MGActive" =>
                        {
                            let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
                                return Err(failure());
                            };
                            active = Some(Saved {
                                name: record.name.clone(),
                                envelope: e.clone(),
                                system_fields: record.system_fields.bytes().to_vec(),
                            });
                        }
                        RecordChange::Saved(record) if !root_zone && record.record_type == "MGEntity" => {
                            let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
                                return Err(failure());
                            };
                            seen.insert(record.name.clone());
                            entities.push(Saved {
                                name: record.name.clone(),
                                envelope: e.clone(),
                                system_fields: record.system_fields.bytes().to_vec(),
                            });
                        }
                        RecordChange::Saved(record) if !root_zone && record.record_type == "MGEpoch" => {}
                        _ => return Err(failure()),
                    }
                }
                if !entities.is_empty() {
                    self.driver
                        .call::<()>(Input::Fetched(self.instance.clone(), entities))?;
                }
                if !page.more {
                    if root_zone {
                        let active = active.ok_or_else(failure)?;
                        self.driver.call::<bool>(Input::RootFetch(
                            self.instance.clone(),
                            Some(active),
                            true,
                            response.request_id,
                        ))?;
                    }
                    break;
                }
                token = Some(page.token);
            }
        }
        self.driver
            .call::<()>(Input::FetchCompleted(self.instance.clone(), phase, seen))?;
        self.refresh()
    }
    pub fn reconcile_and_confirm_root(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        if steps < 4 {
            return Err(failure());
        }
        self.driver.call::<()>(Input::Identity(None))?;
        self.driver.call::<()>(Input::Reconcile)?;
        self.driver.call::<()>(Input::StartSDK(self.instance.clone()))?;
        self.confirm_root_read(service, &mut (steps - 3))
    }
    pub(crate) fn confirm_root_read(
        &mut self,
        service: &mut Service<SeededRandom>,
        remaining: &mut usize,
    ) -> Result<()> {
        self.qualify_budget(service, remaining)?;
        *remaining = remaining.checked_sub(1).ok_or_else(failure)?;
        let due: bool = self.driver.call(Input::ReadDue)?;
        if !due {
            return Err(failure());
        }
        let r = service.request(
            &self.files.scope,
            self.caller(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )?;
        let Ok(Reply::Record(record)) = r.result else {
            return Err(failure());
        };
        let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
            return Err(failure());
        };
        let confirmed: bool = self.driver.call(Input::RootRead(
            self.instance.clone(),
            r.request_id,
            e.clone(),
            record.system_fields.bytes().to_vec(),
            false,
        ))?;
        if !confirmed {
            return Err(failure());
        }
        let persisted_epoch = self.driver.call(Input::RootEpoch)?;
        self.root = Some(RootReadProof {
            instance: self.instance.clone(),
            request_id: r.request_id,
            persisted_epoch,
            accepted_record: record,
        });
        self.updates()?;
        Ok(())
    }
    pub fn accepted_root_read(&self) -> Result<RootReadProof> {
        self.root.clone().ok_or_else(failure)
    }
    fn build(&mut self) -> Result<(BuiltBatch, Vec<Mutation>)> {
        self.driver.call::<()>(Input::Poll(self.instance.clone()))?;
        self.updates()?;
        let requested = self
            .pending
            .iter()
            .filter(|(_, v)| **v == PendingChange::Save)
            .map(|(n, _)| n.clone())
            .collect();
        let b: BuiltBatch = self.driver.call(Input::Build(self.instance.clone(), requested))?;
        let mutations = b
            .saves
            .iter()
            .map(|s| Mutation::Save {
                record: RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone()),
                if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
            })
            .collect();
        self.refresh()?;
        Ok((b, mutations))
    }
    fn send(&mut self, service: &mut Service<SeededRandom>, mutations: Vec<Mutation>) -> Result<ResponseBatch> {
        let response = service.request(
            &self.files.scope,
            self.caller(),
            Operation::Modify {
                zone: self.files.collection_zone.clone(),
                atomic: false,
                changes: mutations.clone(),
            },
        )?;
        let batch = ResponseBatch {
            instance: self.instance.clone(),
            mutations,
            response,
        };
        self.sent.insert(batch.response.request_id, batch.clone());
        self.waiting = Some(batch.response.request_id);
        Ok(batch)
    }
    pub fn request_collection_batch(&mut self, service: &mut Service<SeededRandom>) -> Result<BatchAttempt> {
        let (_, mutations) = self.build()?;
        if mutations.is_empty() {
            return Ok(BatchAttempt {
                mutations,
                request_ids: Vec::new(),
            });
        }
        let b = self.send(service, mutations)?;
        let attempt = BatchAttempt {
            mutations: b.mutations.clone(),
            request_ids: vec![b.response.request_id],
        };
        self.deliver_sent_event(&b.response)?;
        self.return_sent_handler()?;
        self.updates()?;
        Ok(attempt)
    }
    pub fn drive_until_response(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<ResponseBatch> {
        if steps < 3 || self.handler.is_some() || self.waiting.is_some() {
            return Err(failure());
        }
        let mut remaining = steps - 3;
        self.fetch_exchange(service, &mut remaining)?;
        let (_, mutations) = self.build()?;
        if mutations.is_empty() {
            return Err(failure());
        }
        let call: Option<SendCall> = self.driver.call(Input::StartCall(self.instance.clone()))?;
        let call = call.ok_or_else(failure)?;
        let b = self.send(service, mutations)?;
        self.driver
            .call::<()>(Input::RecordCall(self.instance.clone(), call.id, b.response.request_id))?;
        self.calls.insert(b.response.request_id, call.id);
        self.refresh()?;
        Ok(b)
    }
    pub fn drive_ready(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let mut remaining = steps;
        self.drive_budget(service, &mut remaining)
    }
    pub(crate) fn drive_budget(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        while *remaining > 0 {
            *remaining -= 1;
            if self.handler.is_some() {
                self.refresh()?;
                return Ok(());
            }
            if let Some(id) = self.waiting {
                let r = self.sent.get(&id).ok_or_else(failure)?.response.clone();
                self.deliver_sent_event(&r)?;
                self.return_sent_handler()?;
                continue;
            }
            self.fetch_exchange(service, remaining)?;
            self.driver.call::<()>(Input::Poll(self.instance.clone()))?;
            self.updates()?;
            let wants_send: bool = self.driver.call(Input::WantsSend)?;
            if !wants_send {
                self.refresh()?;
                return Ok(());
            }
            let (_, mutations) = self.build()?;
            if mutations.is_empty() {
                self.driver.call::<()>(Input::SendsFinished(self.instance.clone()))?;
                self.fetch_exchange(service, remaining)?;
                self.refresh()?;
                return Ok(());
            }
            let call: Option<SendCall> = self.driver.call(Input::StartCall(self.instance.clone()))?;
            let Some(call) = call else {
                self.refresh()?;
                return Ok(());
            };
            let b = self.send(service, mutations)?;
            self.driver
                .call::<()>(Input::RecordCall(self.instance.clone(), call.id, b.response.request_id))?;
            self.calls.insert(b.response.request_id, call.id);
        }
        Err(failure())
    }
    pub fn sdk_drop_nonretry_failures(&mut self, response: &Response) -> Result<()> {
        let b = self.sent.get(&response.request_id).ok_or_else(failure)?;
        self.driver.call::<()>(Input::Validate(b.instance.clone()))?;
        match &response.result {
            Err(_) => {
                for m in &b.mutations {
                    match m {
                        Mutation::Save { record, .. } => {
                            self.pending.remove(&record.name);
                        }
                        Mutation::Delete { name } => {
                            self.pending.remove(name);
                        }
                    }
                }
            }
            Ok(Reply::Modified(items)) => {
                for item in items {
                    if item.result.is_err() {
                        self.pending.remove(&item.name);
                    }
                }
            }
            _ => return Err(failure()),
        }
        Ok(())
    }
    pub fn deliver_sent_event(&mut self, response: &Response) -> Result<()> {
        if self.handler.is_some() {
            return Err(failure());
        }
        let batch = self.sent.get(&response.request_id).ok_or_else(failure)?.clone();
        self.sdk_drop_nonretry_failures(response)?;
        self.driver
            .call::<()>(Input::EnterHandler(batch.instance.clone(), response.request_id))?;
        self.handler = Some(response.request_id);
        let mut saved = Vec::new();
        let mut failed = Vec::new();
        let error = |c: ErrorCode| match c {
            ErrorCode::LimitExceeded => SendError::LimitExceeded,
            ErrorCode::BatchRequestFailed => SendError::BatchRequestFailed,
            ErrorCode::ZoneNotFound => SendError::ZoneMissing,
            _ => SendError::Other,
        };
        match &response.result {
            Ok(Reply::Modified(items)) => {
                for item in items {
                    match &item.result {
                        Ok(MutationSuccess::Saved(record)) => {
                            let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
                                return Err(failure());
                            };
                            saved.push(Saved {
                                name: record.name.clone(),
                                envelope: e.clone(),
                                system_fields: record.system_fields.bytes().to_vec(),
                            });
                        }
                        Err(e) => failed.push((item.name.clone(), error(e.code))),
                        _ => {}
                    }
                }
            }
            Err(e) => {
                for m in &batch.mutations {
                    if let Mutation::Save { record, .. } = m {
                        failed.push((record.name.clone(), error(e.code)));
                    }
                }
            }
            _ => return Err(failure()),
        }
        self.driver.call::<()>(Input::Sent(
            batch.instance.clone(),
            response.request_id,
            saved,
            failed,
            batch.mutations.len(),
        ))?;
        self.refresh()?;
        Ok(())
    }
    pub fn enter_sent_handler_for_ordering(&mut self) -> Result<()> {
        if self.handler.is_some() {
            return Err(failure());
        }
        self.driver.call::<()>(Input::EnterHandler(self.instance.clone(), 0))?;
        self.driver
            .call::<()>(Input::Sent(self.instance.clone(), 0, Vec::new(), Vec::new(), 0))?;
        self.handler = Some(0);
        self.refresh()
    }
    pub fn return_sent_handler(&mut self) -> Result<()> {
        let id = self.handler.ok_or_else(failure)?;
        let instance = self
            .sent
            .get(&id)
            .map(|b| b.instance.clone())
            .unwrap_or_else(|| self.instance.clone());
        let returned: u64 = self.driver.call(Input::ReturnHandler(instance.clone()))?;
        self.handler = None;
        if id != returned {
            return Err(failure());
        }
        if let Some(batch) = self.sent.get(&id) {
            let mut removed = Vec::new();
            if let Ok(Reply::Modified(items)) = &batch.response.result {
                for item in items {
                    if item.result.is_ok() {
                        self.pending.remove(&item.name);
                        removed.push(item.name.clone());
                    }
                }
            }
            self.driver
                .call::<()>(Input::SavedRemoved(instance.clone(), id, removed))?;
        }
        if self.waiting == Some(id) {
            self.waiting = None;
        }
        self.updates()?;
        if let Some(call) = self.calls.remove(&id) {
            self.driver.call::<()>(Input::FinishCall(instance, call))?;
        }
        self.refresh()
    }
    pub fn sync_now(&mut self) -> Result<u64> {
        let id = self.driver.call(Input::SyncNow(self.instance.clone()))?;
        self.updates()?;
        Ok(id)
    }
    pub fn send_changes_calls(&mut self) -> Vec<SendCall> {
        self.driver.call(Input::Calls).expect("production send calls")
    }
    pub fn exchange_result(&mut self, id: u64) -> Result<ExchangeResult> {
        self.driver.call(Input::Exchange(id))
    }
    pub fn sent_request(&self, id: u64) -> Result<ResponseBatch> {
        self.sent.get(&id).cloned().ok_or_else(failure)
    }
    pub fn advance_clock_ms(&mut self, ms: i64) -> Result<()> {
        self.driver.call(Input::Advance(ms))
    }
    pub fn close_clean(&mut self) -> Result<()> {
        self.refresh()?;
        self.driver.close()
    }
}
