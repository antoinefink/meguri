//! Deterministic SDK stand-in. Policy, capture, actions and acknowledgements run
//! in the production runtime in the shared CoreHost child.
use super::{
    child_driver::{ChildDriver, FetchedCallback, Input},
    devices::{IdentityReads, PreboundDevice, Result, failure},
    *,
};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    self,
    workflow::{self, BuiltBatch, EntityFacts, ExchangeResult, FetchPhase, Saved, SendCall, SendError},
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
// This is the fake SDK's own serialization of actual Service page progress.
// Full-fetch fixtures intentionally redeliver from None each exchange; saved
// tokens are observations, never a claim of native CloudKit resume coverage.
#[derive(Serialize, Deserialize)]
struct SDKSerialization {
    collection_zone: String,
    collection_cursor: Token,
}
pub(crate) fn collection_cursor_projection(bytes: &[u8]) -> Result<Option<Vec<u8>>> {
    let state: SDKSerialization = serde_json::from_slice(bytes)?;
    Ok(Some(serde_json::to_vec(&(
        state.collection_zone,
        state.collection_cursor,
    ))?))
}
struct FetchWork {
    phase: FetchPhase,
    instance: String,
    scope: Scope,
    collection_zone: String,
    exchange: Option<u64>,
    call: Option<u64>,
    group: String,
    manual: bool,
    zone_started: bool,
    lifecycle_finished: bool,
    applying: bool,
    root_zone: bool,
    token: Option<Token>,
    seen: BTreeSet<String>,
    active: Option<Saved>,
    page: Option<FetchPage>,
}
struct FetchPage {
    callback: Option<FetchedCallback>,
    token: Token,
    more: bool,
    request_id: u64,
}

pub use workflow::{Observation, PendingChange, PendingSnapshot, PendingUpdate, UpdateKind};
#[derive(Clone, Debug)]
pub struct BatchAttempt {
    pub mutations: Vec<Mutation>,
    pub request_ids: Vec<u64>,
}
#[derive(Clone, Debug)]
pub struct ResponseBatch {
    pub instance: String,
    /// Core authority; response.request_id remains the external Service receipt.
    pub backend_batch_id: u64,
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
    media_requests: BTreeSet<u64>,
    calls: BTreeMap<u64, u64>,
    waiting: Option<u64>,
    handler: Option<u64>,
    pub(crate) root: Option<RootReadProof>,
    fetch: Option<FetchWork>,
    exchange: Option<u64>,
    fetch_errors: Vec<Box<dyn std::error::Error + Send + Sync>>,
    building: Option<BTreeSet<String>>,
}
impl SDKRig {
    /// Reopen actual adopted files without supplying a binding or baseline.
    /// Call reconcile_register_and_confirm_root before using the SDK transport.
    pub fn open_existing_collection(
        directory: &std::path::Path,
        scope: &Scope,
        now: i64,
        seed: u64,
        keys: std::sync::Arc<dyn meguri_sync::cloud::runtime::KeyByteSource>,
    ) -> Result<Self> {
        let driver = ChildDriver::spawn_unbound(
            directory,
            scope,
            IdentityReads::Available(scope.account.clone()),
            true,
            now,
            seed,
            keys,
        )?;
        Ok(Self::from_driver(driver, scope.clone(), String::new(), String::new()))
    }
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
            media_requests: BTreeSet::new(),
            calls: BTreeMap::new(),
            waiting: None,
            handler: None,
            root: None,
            fetch: None,
            exchange: None,
            fetch_errors: Vec::new(),
            building: None,
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
        // A held reply still belongs to its retained SDK and call, even if the
        // current registration changes before that reply returns.
        self.exchange = None;
        self.building = None;
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
    pub(super) fn updates(&mut self) -> Result<bool> {
        let updates: Vec<PendingUpdate> = self.driver.call(Input::Updates(self.instance.clone()))?;
        let changed = updates.iter().any(|u| !u.adds.is_empty() || !u.removes.is_empty());
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
        self.refresh()?;
        Ok(changed)
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
            self.driver.call::<()>(Input::BeginZoneListing(self.instance.clone()))?;
            let r = match service.request(&self.files.scope, self.caller(), Operation::FetchDatabase { token }) {
                Ok(response) => response,
                Err(error) => {
                    self.driver.call::<()>(Input::ZoneListingResult(None))?;
                    return Err(error.into());
                }
            };
            let page = match r.result {
                Ok(Reply::DatabaseChanges(page)) => page,
                Ok(_) => {
                    self.driver.call::<()>(Input::ZoneListingResult(None))?;
                    return Err(Box::new(std::io::Error::other("qualify: Service reply shape")));
                }
                Err(_) => {
                    self.driver.call::<()>(Input::ZoneListingResult(None))?;
                    return Err(Box::new(std::io::Error::other("qualify: Service error reply")));
                }
            };
            // Persist actual zone announcements through the runtime, independently
            // of root facts or the Service inspection API.
            let zones = page
                .changes
                .iter()
                .filter(|change| change.removed.is_none())
                .map(|change| change.zone.clone())
                .collect();
            self.driver.call::<()>(Input::ZoneListingResult(Some(zones)))?;
            if !page.more {
                self.driver.call::<()>(Input::Qualify(self.instance.clone()))?;
                return Ok(());
            }
            token = Some(page.token);
        }
        Err(Box::new(std::io::Error::other("qualify: step budget exhausted")))
    }
    fn fetch_exchange(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        let mut fetch = match self.fetch.take() {
            Some(fetch) => fetch,
            None => {
                let phase: Option<FetchPhase> = self.driver.call(Input::FetchDue)?;
                let Some(phase) = phase else { return Ok(()) };
                let instance = self.instance.clone();
                let collection_zone = self.files.collection_zone.clone();
                let call: Option<u64> = self.driver.call(Input::FetchBegin(
                    instance.clone(),
                    vec!["meguri-root".into(), collection_zone.clone()],
                ))?;
                FetchWork {
                    phase,
                    instance,
                    scope: self.files.scope.clone(),
                    collection_zone,
                    exchange: self.exchange,
                    call,
                    group: format!("testkit-fetch-{}", call.unwrap_or(0)),
                    manual: self.exchange.is_some(),
                    zone_started: false,
                    lifecycle_finished: false,
                    applying: false,
                    root_zone: true,
                    token: None,
                    seen: BTreeSet::new(),
                    active: None,
                    page: None,
                }
            }
        };
        let result = self.fetch_pages(service, remaining, &mut fetch);
        if result
            .as_ref()
            .is_err_and(|error| super::child_driver::is_work_paused(error.as_ref()))
        {
            self.fetch = Some(fetch);
        } else if result.is_err() {
            self.fail_fetch(&fetch);
            self.exchange = None;
        }
        result
    }
    fn fetched_callback(&mut self, fetch: &FetchWork, records: Vec<Record>, token: &Token) -> Result<FetchedCallback> {
        let zone = fetch.collection_zone.clone();
        let serialization = serde_json::to_vec(&SDKSerialization {
            collection_zone: zone.clone(),
            collection_cursor: token.clone(),
        })?;
        self.driver
            .stage_fetched(fetch.instance.clone(), records, zone, serialization, fetch.exchange)
    }
    /// Finish only this SDK's original callback before any new trigger. Direct
    /// grants keep their replies in ChildDriver for their own callers.
    pub(crate) fn resume_fetch_callback(&mut self) -> Result<()> {
        let Some(mut fetch) = self.fetch.take() else {
            return Ok(());
        };
        let input = if fetch.applying {
            Input::FetchApply(fetch.instance.clone(), fetch.exchange)
        } else if let Some(page) = fetch.page.as_ref().filter(|_| !fetch.root_zone) {
            Input::Fetched(page.callback.as_ref().ok_or_else(failure)?.clone())
        } else {
            self.fetch = Some(fetch);
            return Ok(());
        };
        if !self.driver.has_incoming_reply(&input)? {
            self.fetch = Some(fetch);
            return Ok(());
        }
        let result = (|| {
            if fetch.applying {
                self.complete_fetch(&mut fetch)
            } else {
                self.driver.call::<()>(input)?;
                if self.consume_fetch_page(&mut fetch)? {
                    self.complete_fetch(&mut fetch)
                } else {
                    Ok(())
                }
            }
        })();
        match result {
            Err(error) if super::child_driver::is_work_paused(error.as_ref()) => {
                self.fetch = Some(fetch);
                Err(error)
            }
            Err(error) => {
                self.fail_fetch(&fetch);
                self.fetch_errors.push(error);
                self.exchange = None;
                Ok(())
            }
            Ok(()) if fetch.page.is_none() && !fetch.applying && fetch.token.is_some() => {
                self.fetch = Some(fetch);
                Ok(())
            }
            Ok(()) => Ok(()),
        }
    }
    fn fetch_zone(fetch: &FetchWork) -> String {
        if fetch.root_zone {
            "meguri-root".into()
        } else {
            fetch.collection_zone.clone()
        }
    }
    // Every final page must return from its durable intake callback before its
    // zone closes. The same path consumes a normally delivered or held reply.
    fn consume_fetch_page(&mut self, fetch: &mut FetchWork) -> Result<bool> {
        let page = fetch.page.take().ok_or_else(failure)?;
        if page.more {
            fetch.token = Some(page.token);
            return Ok(false);
        }
        if fetch.call.is_some() {
            self.driver.call::<()>(Input::FetchZone(
                fetch.instance.clone(),
                Self::fetch_zone(fetch),
                false,
                false,
            ))?;
        }
        fetch.zone_started = false;
        fetch.token = None;
        if fetch.root_zone {
            fetch.root_zone = false;
            Ok(false)
        } else {
            Ok(true)
        }
    }
    fn complete_fetch(&mut self, fetch: &mut FetchWork) -> Result<()> {
        if !fetch.lifecycle_finished {
            if let Some(id) = fetch.call {
                self.driver.call::<()>(Input::FetchDid(
                    fetch.instance.clone(),
                    fetch.group.clone(),
                    fetch.manual,
                ))?;
                // Consume the original call exactly once, including a false result.
                fetch.lifecycle_finished = true;
                let completed: bool = self.driver.call(Input::FetchFinish(fetch.instance.clone(), id))?;
                if !completed {
                    return Err(Box::new(std::io::Error::other("fetch lifecycle: incomplete")));
                }
            } else {
                fetch.lifecycle_finished = true;
            }
        }
        if fetch.call.is_some() {
            // Native delivery/persistence returns before ApplyReady. In particular,
            // media downloads may be held until the collection drain above.
            fetch.applying = true;
            self.driver
                .call::<()>(Input::FetchApply(fetch.instance.clone(), fetch.exchange))?;
            fetch.applying = false;
        }
        self.driver.call::<()>(Input::FetchCompleted(
            fetch.instance.clone(),
            fetch.phase,
            fetch.seen.clone(),
        ))?;
        if fetch.phase == FetchPhase::AfterSend {
            self.exchange = None;
        }
        self.refresh()
    }
    fn fail_fetch(&mut self, fetch: &FetchWork) {
        if let Some(id) = fetch.call.filter(|_| !fetch.lifecycle_finished) {
            if fetch.zone_started
                && self
                    .driver
                    .call::<()>(Input::FetchZone(
                        fetch.instance.clone(),
                        Self::fetch_zone(fetch),
                        false,
                        true,
                    ))
                    .is_err()
            {
                eprintln!("cloud-child fetch_failure_stage=zone_failed");
            }
            if self
                .driver
                .call::<bool>(Input::FetchFinish(fetch.instance.clone(), id))
                .is_err()
            {
                eprintln!("cloud-child fetch_failure_stage=finish_failed");
            }
        }
        if let Some(exchange) = fetch.exchange
            && self
                .driver
                .call::<()>(Input::FetchFailed(fetch.instance.clone(), exchange))
                .is_err()
        {
            eprintln!("cloud-child fetch_failure_stage=exchange_failed");
        }
    }
    /// Original asynchronous SDK callback errors, consumed once by the caller.
    pub fn take_fetch_errors(&mut self) -> Vec<Box<dyn std::error::Error + Send + Sync>> {
        std::mem::take(&mut self.fetch_errors)
    }
    fn fetch_pages(
        &mut self,
        service: &mut Service<SeededRandom>,
        remaining: &mut usize,
        fetch: &mut FetchWork,
    ) -> Result<()> {
        loop {
            if fetch.applying {
                return self.complete_fetch(fetch);
            }
            let zone = Self::fetch_zone(fetch);
            if fetch.page.is_none() {
                *remaining = remaining
                    .checked_sub(1)
                    .ok_or_else(|| std::io::Error::other("fetch pages: step budget exhausted"))?;
                if !fetch.zone_started {
                    if fetch.call.is_some() {
                        if fetch.root_zone {
                            self.driver.call::<()>(Input::FetchWill(
                                fetch.instance.clone(),
                                fetch.group.clone(),
                                fetch.manual,
                            ))?;
                        }
                        self.driver
                            .call::<()>(Input::FetchZone(fetch.instance.clone(), zone.clone(), true, false))?;
                    }
                    fetch.zone_started = true;
                }
                // A resumed page may outlive its SDK or account. Check the
                // original authority before another actual Service enqueue.
                if fetch.call.is_some() {
                    self.driver.call::<()>(Input::FetchValidate(fetch.instance.clone()))?;
                } else {
                    self.driver.call::<()>(Input::Validate(fetch.instance.clone()))?;
                }
                let response = service.request(
                    &fetch.scope,
                    Caller {
                        instance: Some(fetch.instance.clone()),
                        ..self.caller()
                    },
                    Operation::FetchZone {
                        zone: zone.clone(),
                        token: fetch.token.clone(),
                    },
                )?;
                let page = match response.result {
                    Ok(Reply::ZoneChanges(page)) => page,
                    Ok(_) => {
                        return Err(Box::new(std::io::Error::other(if fetch.root_zone {
                            "root fetch: Service reply shape"
                        } else {
                            "collection fetch: Service reply shape"
                        })));
                    }
                    Err(_) => {
                        return Err(Box::new(std::io::Error::other(if fetch.root_zone {
                            "root fetch: Service error reply"
                        } else {
                            "collection fetch: Service error reply"
                        })));
                    }
                };
                let mut entities = Vec::new();
                for change in page.changes {
                    match change {
                        RecordChange::Saved(record)
                            if fetch.root_zone && record.name == "active" && record.record_type == "MGActive" =>
                        {
                            let Some(Field::EncryptedBytes(e)) = record.fields.get("e") else {
                                return Err(Box::new(std::io::Error::other("root fetch: envelope shape")));
                            };
                            fetch.active = Some(Saved {
                                name: record.name.clone(),
                                envelope: e.clone(),
                                system_fields: record.system_fields.bytes().to_vec(),
                            });
                        }
                        RecordChange::Saved(record) if !fetch.root_zone && record.record_type == "MGEntity" => {
                            if !matches!(record.fields.get("e"), Some(Field::EncryptedBytes(_))) {
                                return Err(Box::new(std::io::Error::other("collection fetch: envelope shape")));
                            }
                            fetch.seen.insert(record.name.clone());
                            entities.push(record);
                        }
                        RecordChange::Saved(record) if !fetch.root_zone && record.record_type == "MGEpoch" => {}
                        _ => {
                            return Err(Box::new(std::io::Error::other(if fetch.root_zone {
                                "root fetch: change shape"
                            } else {
                                "collection fetch: change shape"
                            })));
                        }
                    }
                }
                // Retain the original page and callback command across a child
                // pause. Neither Service nor the receive handler is resubmitted.
                let callback = if fetch.root_zone {
                    None
                } else {
                    Some(self.fetched_callback(fetch, entities, &page.token)?)
                };
                fetch.page = Some(FetchPage {
                    callback,
                    token: page.token,
                    more: page.more,
                    request_id: response.request_id,
                });
            }
            let page = fetch
                .page
                .as_ref()
                .ok_or_else(|| std::io::Error::other("fetch pages: missing page"))?;
            if fetch.root_zone {
                if !page.more {
                    self.driver.call::<bool>(Input::RootFetch(
                        fetch.instance.clone(),
                        Some(
                            fetch
                                .active
                                .clone()
                                .ok_or_else(|| std::io::Error::other("root fetch: missing active"))?,
                        ),
                        true,
                        page.request_id,
                    ))?;
                }
            } else {
                let input = Input::Fetched(
                    page.callback
                        .as_ref()
                        .ok_or_else(|| std::io::Error::other("collection fetch: missing callback"))?
                        .clone(),
                );
                self.driver.call::<()>(input)?;
            }
            if self.consume_fetch_page(fetch)? {
                return self.complete_fetch(fetch);
            }
        }
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
            .building
            .get_or_insert_with(|| {
                self.pending
                    .iter()
                    .filter(|(_, change)| **change == PendingChange::Save)
                    .map(|(name, _)| name.clone())
                    .collect()
            })
            .clone();
        let b: BuiltBatch = self.driver.call(Input::Build(self.instance.clone(), requested))?;
        self.building = None;
        let mutations = b
            .saves
            .iter()
            .map(|s| {
                let mut record = RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone());
                if let Some(path) = &s.asset_path {
                    record
                        .fields
                        .insert("pa".into(), Field::Asset(Asset::synthetic(std::fs::read(path)?)));
                }
                Ok(Mutation::Save {
                    record,
                    if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
                })
            })
            .collect::<Result<Vec<_>>>()?;
        // Building can emit removals for held/deferred images even when it
        // returns no saves. Apply the real ordered updates after handler return
        // before reporting SDK pending state or selecting another batch.
        self.updates()?;
        Ok((b, mutations))
    }
    fn send(
        &mut self,
        service: &mut Service<SeededRandom>,
        built: &BuiltBatch,
        mutations: Vec<Mutation>,
    ) -> Result<ResponseBatch> {
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
            backend_batch_id: built.id,
            mutations,
            response,
        };
        self.sent.insert(batch.response.request_id, batch.clone());
        self.waiting = Some(batch.response.request_id);
        Ok(batch)
    }
    /// Return the Runtime selection unchanged. The send path validates that
    /// retained value again before submitting its Service operation.
    pub fn select_media_batch(&mut self) -> Result<Option<(u64, cloud::media::Work)>> {
        self.updates()?;
        let selected = self.driver.call(Input::MediaBuild(self.pending.clone()))?;
        self.updates()?;
        Ok(selected)
    }
    pub fn request_selected_media_response(
        &mut self,
        service: &mut Service<SeededRandom>,
        selected: (u64, cloud::media::Work),
    ) -> Result<ResponseBatch> {
        let (id, work) = selected;
        let original: cloud::media::Work = match self.driver.call(Input::MediaAdmit(id)) {
            Ok(original) => original,
            Err(error) => {
                self.updates()?;
                return Err(error);
            }
        };
        if original != work {
            return Err(failure());
        }
        let mutation = match original.request {
            cloud::media::Request::Delete { name } => Mutation::Delete { name },
            cloud::media::Request::Upload { name, envelope, staged } => {
                use std::io::Read;
                let mut bytes = Vec::new();
                std::fs::File::open(staged)?
                    .take(cloud::media::MAX_ASSET_BYTES + 1)
                    .read_to_end(&mut bytes)?;
                if bytes.len() as u64 > cloud::media::MAX_ASSET_BYTES {
                    return Err(failure());
                }
                let mut record = RecordInput::encrypted(name, "MGBlob", envelope);
                record.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes)));
                Mutation::Save {
                    record,
                    if_unchanged: None,
                }
            }
            _ => return Err(failure()),
        };
        let mutations = vec![mutation];
        let response = service.request(
            &self.files.scope,
            self.caller(),
            Operation::Modify {
                zone: "meguri-media".into(),
                atomic: false,
                changes: mutations.clone(),
            },
        )?;
        let batch = ResponseBatch {
            instance: self.instance.clone(),
            backend_batch_id: id,
            mutations,
            response,
        };
        self.media_requests.insert(batch.response.request_id);
        self.sent.insert(batch.response.request_id, batch.clone());
        self.waiting = Some(batch.response.request_id);
        Ok(batch)
    }
    pub(crate) fn send_pending_media(&mut self, service: &mut Service<SeededRandom>) -> Result<bool> {
        let Some(selected) = self.select_media_batch()? else {
            self.updates()?;
            return Ok(false);
        };
        let batch = self.request_selected_media_response(service, selected)?;
        self.deliver_sent_event(&batch.response)?;
        self.return_sent_handler()?;
        self.updates()?;
        Ok(true)
    }
    pub fn request_collection_batch(&mut self, service: &mut Service<SeededRandom>) -> Result<BatchAttempt> {
        let (built, mutations) = self.build()?;
        if mutations.is_empty() {
            return Ok(BatchAttempt {
                mutations,
                request_ids: Vec::new(),
            });
        }
        let b = self.send(service, &built, mutations)?;
        let attempt = BatchAttempt {
            mutations: b.mutations.clone(),
            request_ids: vec![b.response.request_id],
        };
        self.deliver_sent_event(&b.response)?;
        self.return_sent_handler()?;
        self.updates()?;
        Ok(attempt)
    }
    /// Send the SDK's next real batch without fetching or delivering its reply.
    /// A paused build resumes its original command on the next call.
    pub fn request_collection_batch_response(&mut self, service: &mut Service<SeededRandom>) -> Result<ResponseBatch> {
        if self.handler.is_some() || self.waiting.is_some() {
            return Err(failure());
        }
        let (built, mutations) = self.build()?;
        if mutations.is_empty() {
            return Err(failure());
        }
        let call: Option<SendCall> = self.driver.call(Input::StartCall(self.instance.clone()))?;
        let call = call.ok_or_else(failure)?;
        let batch = self.send(service, &built, mutations)?;
        self.driver.call::<()>(Input::RecordCall(
            self.instance.clone(),
            call.id,
            batch.backend_batch_id,
        ))?;
        self.calls.insert(batch.response.request_id, call.id);
        self.refresh()?;
        Ok(batch)
    }
    /// Real handed request whose SDK reply is lost. End the physical send,
    /// without a fabricated result callback or ACK; durable flights survive.
    pub fn lose_collection_batch_reply(&mut self, service: &mut Service<SeededRandom>) -> Result<ResponseBatch> {
        if self.handler.is_some() || self.waiting.is_some() {
            return Err(failure());
        }
        let (built, mutations) = self.build()?;
        if mutations.is_empty() {
            return Err(failure());
        }
        let call: Option<SendCall> = self.driver.call(Input::StartCall(self.instance.clone()))?;
        let call = call.ok_or_else(failure)?;
        let batch = self.send(service, &built, mutations)?;
        self.driver
            .call::<()>(Input::RecordCall(self.instance.clone(), call.id, built.id))?;
        self.driver
            .call::<()>(Input::FinishCall(self.instance.clone(), call.id))?;
        self.waiting = None;
        self.sent.remove(&batch.response.request_id);
        self.refresh()?;
        Ok(batch)
    }
    pub fn reopen_after_lost_replies(&mut self) -> Result<()> {
        let instance = self.driver.reopen_for_testing()?;
        self.install_instance(instance, self.files.collection_zone.clone());
        Ok(())
    }
    /// Register after the reopened host's reconciliation, before receiving assets.
    pub fn reconcile_register_and_confirm_root(
        &mut self,
        service: &mut Service<SeededRandom>,
        steps: usize,
    ) -> Result<()> {
        if steps < 4 {
            return Err(failure());
        }
        self.driver.call::<()>(Input::Identity(None))?;
        self.driver.call::<()>(Input::Reconcile)?;
        let (instance, _serialization, zone): (String, Option<Vec<u8>>, String) =
            self.driver.call(Input::RegisterSDK(None))?;
        self.install_instance(instance, zone);
        // This rig deliberately performs full fetches, even with saved SDK state.
        // confirm_root_read also completes the actual no-state zone listing.
        self.confirm_root_read(service, &mut (steps - 3))
    }
    /// Fetch actual Service evidence without proceeding to a new save.
    pub fn fetch_recovery_exchange(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        self.sync_now()?;
        let mut remaining = steps;
        self.fetch_exchange(service, &mut remaining)
    }
    pub fn drive_until_response(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<ResponseBatch> {
        if steps < 3 || self.handler.is_some() || self.waiting.is_some() {
            return Err(failure());
        }
        let mut remaining = steps - 3;
        self.fetch_exchange(service, &mut remaining)?;
        let (built, mutations) = self.build()?;
        if mutations.is_empty() {
            return Err(failure());
        }
        let call: Option<SendCall> = self.driver.call(Input::StartCall(self.instance.clone()))?;
        let call = call.ok_or_else(failure)?;
        let b = self.send(service, &built, mutations)?;
        self.driver
            .call::<()>(Input::RecordCall(self.instance.clone(), call.id, b.backend_batch_id))?;
        self.calls.insert(b.response.request_id, call.id);
        self.refresh()?;
        Ok(b)
    }
    pub fn drive_ready(&mut self, service: &mut Service<SeededRandom>, steps: usize) -> Result<()> {
        let mut remaining = steps;
        self.drive_budget(service, &mut remaining)
    }
    pub(crate) fn drive_budget(&mut self, service: &mut Service<SeededRandom>, remaining: &mut usize) -> Result<()> {
        self.drive_budget_inner(service, remaining, false)
    }
    pub(super) fn drive_media_budget(
        &mut self,
        service: &mut Service<SeededRandom>,
        remaining: &mut usize,
    ) -> Result<()> {
        self.drive_budget_inner(service, remaining, true)
    }
    fn drive_budget_inner(
        &mut self,
        service: &mut Service<SeededRandom>,
        remaining: &mut usize,
        media: bool,
    ) -> Result<()> {
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
                if media && self.drive_cloud_media(service, remaining)? {
                    continue;
                }
                self.refresh()?;
                return Ok(());
            }
            let (built, mutations) = self.build()?;
            if mutations.is_empty() {
                if media && self.drive_cloud_media(service, remaining)? {
                    continue;
                }
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
            let b = self.send(service, &built, mutations)?;
            self.driver
                .call::<()>(Input::RecordCall(self.instance.clone(), call.id, b.backend_batch_id))?;
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
            .call::<()>(Input::EnterHandler(batch.instance.clone(), batch.backend_batch_id))?;
        self.handler = Some(response.request_id);
        let mut saved = Vec::new();
        let mut conflicts = Vec::new();
        let mut failed = Vec::new();
        let mut deleted = Vec::new();
        let mut blob_conflicts = Vec::new();
        let error = |c: ErrorCode| match c {
            ErrorCode::LimitExceeded => SendError::LimitExceeded,
            ErrorCode::BatchRequestFailed => SendError::BatchRequestFailed,
            ErrorCode::ZoneNotFound => SendError::ZoneMissing,
            ErrorCode::UnknownItem => SendError::UnknownItem,
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
                        Err(e) if e.code == ErrorCode::ServerRecordChanged => {
                            let record = e.server_record.as_deref().ok_or_else(failure)?;
                            let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                                return Err(failure());
                            };
                            if self.media_requests.contains(&response.request_id)
                                && record.record_type == "MGBlob"
                                && record.name == item.name
                            {
                                blob_conflicts.push(Saved {
                                    name: record.name.clone(),
                                    envelope: envelope.clone(),
                                    system_fields: record.system_fields.bytes().to_vec(),
                                });
                                continue;
                            }
                            if record.name != item.name || record.record_type != "MGEntity" {
                                return Err(failure());
                            }
                            conflicts.push(Saved {
                                name: record.name.clone(),
                                envelope: envelope.clone(),
                                system_fields: record.system_fields.bytes().to_vec(),
                            });
                        }
                        Ok(MutationSuccess::Deleted) => deleted.push(item.name.clone()),
                        Err(e) => failed.push((item.name.clone(), error(e.code))),
                    }
                }
            }
            Err(e) => {
                if e.code == ErrorCode::ServerRecordChanged {
                    return Err(failure());
                }
                for m in &batch.mutations {
                    let name = match m {
                        Mutation::Save { record, .. } => record.name.clone(),
                        Mutation::Delete { name } => name.clone(),
                    };
                    failed.push((name, error(e.code)));
                }
            }
            _ => return Err(failure()),
        }
        let media = self.media_requests.contains(&response.request_id);
        if media {
            let delete_names: BTreeSet<_> = batch
                .mutations
                .iter()
                .filter_map(|m| {
                    if let Mutation::Delete { name } = m {
                        Some(name.clone())
                    } else {
                        None
                    }
                })
                .collect();
            let (failed_delete, failed_save): (Vec<_>, Vec<_>) =
                failed.into_iter().partition(|(name, _)| delete_names.contains(name));
            self.driver.call::<()>(Input::MediaSent(
                batch.instance.clone(),
                batch.backend_batch_id,
                saved,
                blob_conflicts,
                failed_save,
                deleted,
                failed_delete,
                Vec::new(),
            ))?;
            self.refresh()?;
            return Ok(());
        }
        self.driver.call::<()>(Input::Sent(
            batch.instance.clone(),
            batch.backend_batch_id,
            saved,
            conflicts,
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
        self.driver.call::<()>(Input::Sent(
            self.instance.clone(),
            0,
            Vec::new(),
            Vec::new(),
            Vec::new(),
            0,
        ))?;
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
        let backend = self.sent.get(&id).map_or(id, |batch| batch.backend_batch_id);
        if backend != returned {
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
                .call::<()>(Input::SavedRemoved(instance.clone(), backend, removed))?;
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
        self.exchange = Some(id);
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
