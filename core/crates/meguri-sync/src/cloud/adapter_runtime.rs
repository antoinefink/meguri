use super::*;
use crate::cloud::{adapter::*, workflow::*};
use std::collections::{BTreeMap, BTreeSet};
impl Runtime {
    pub fn attach_adapter_clock(&self, clock: Arc<dyn meguri_model::clock::Clock>) {
        *self.adapter_clock.lock().unwrap_or_else(PoisonError::into_inner) = Some(clock);
    }
    pub(super) fn adapter_now(&self) -> Result<i64, RuntimeError> {
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        Ok(clock.now_ms())
    }
    pub(super) fn adapter_guard_locked(s: &State) -> Guard {
        Guard {
            generation: s.bound.as_ref().map_or(0, |b| b.evidence.generation.0),
            binding: s.bound.as_ref().map_or_else(Vec::new, |b| b.binding.id.0.to_vec()),
            token: s.adapter.token.saturating_add(s.invalidation),
        }
    }
    pub fn adapter_guard(&self) -> Guard {
        Self::adapter_guard_locked(&self.state())
    }
    pub fn register_sdk(&self, automatic: bool) -> Result<(String, Option<Vec<u8>>, String), RuntimeError> {
        let mut s = self.state();
        if s.adapter.sdk.is_some() {
            return Err(Error::InvalidData.into());
        }
        let host = Self::instance_locked(&s)?;
        Self::caller_locked(&s, &host)?;
        if !s.whole_complete || s.scan_required || s.tracking_stopped || s.adapter.paused {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
        let (serialization, durable) = with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            store.adapter_install()?;
            Ok((
                store.engine_state(b.id)?.map(|v| v.serialization.0),
                store.adapter_durable(b.id)?,
            ))
        })?;
        s.adapter.durable = durable;
        s.adapter.next_sdk = s.adapter.next_sdk.checked_add(1).ok_or(Error::InvalidData)?;
        let id = format!("sdk:{host}:{}", s.adapter.next_sdk);
        if s.adapter.next_sdk > 1 {
            s.root_confirmed = false;
        }
        s.adapter.no_state = serialization.is_none();
        s.adapter.no_state_point += 1;
        s.adapter.fetch = None;
        s.adapter.scheduled = None;
        s.actions.automatic = automatic;
        s.actions.handler = None;
        #[cfg(feature = "test-hooks")]
        meguri_model::faultpoint!("cloud.adapter.register_sdk.before_pending").map_err(Error::from)?;
        self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
        // Failed pending discovery must leave the SDK slot available for retry.
        s.adapter.sdk = Some(SDKAuthority {
            id: id.clone(),
            guard: Self::adapter_guard_locked(&s),
            stopping: false,
        });
        Ok((id, serialization, collection_zone(b.facts.epoch)))
    }
    pub fn sdk_root_confirmed(&self) -> bool {
        self.state().root_confirmed
    }
    pub fn validate_sdk(&self, sdk: &str) -> Result<(), RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)
    }
    fn sdk_locked(s: &State, sdk: &str) -> Result<(), RuntimeError> {
        if !s
            .adapter
            .sdk
            .as_ref()
            .is_some_and(|a| a.id == sdk && !a.stopping && a.guard == Self::adapter_guard_locked(s))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Self::caller_locked(s, sdk)
    }
    /// Re-creation invalidates the SDK authority without changing the policy guard.
    pub fn retire_sdk(&self, sdk: &str, policy_stop: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let authority = s
            .adapter
            .sdk
            .as_mut()
            .filter(|a| a.id == sdk)
            .ok_or(RuntimeError::StaleIncarnation)?;
        authority.stopping = true;
        if policy_stop {
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        }
        Ok(())
    }
    pub fn release_sdk(&self, sdk: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.adapter.sdk.as_ref().is_some_and(|a| a.id == sdk && a.stopping) {
            s.adapter.sdk = None;
            s.actions.handler = None;
            s.adapter.fetch = None;
            s.adapter.scheduled = None;
        }
        Ok(())
    }
    pub fn adapter_pause(&self, paused: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.adapter.paused != paused {
            s.adapter.paused = paused;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        }
        Ok(())
    }
    pub fn adapter_stop_syncing(&self) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .isolate_runtime_binding(id, IsolationReason::Stopped)
        })?;
        if let Some(b) = s.bound.as_mut() {
            b.usable = false;
            b.verified = false;
            b.binding.facts.state = BindingState::Isolated(IsolationReason::Stopped);
        }
        Ok(())
    }
    pub fn adapter_account(&self, sdk: Option<&str>, identity: IdentityStatus) -> Result<(), RuntimeError> {
        self.wait_incoming_idle();
        let mut s = self.state();
        if let Some(sdk) = sdk {
            Self::sdk_locked(&s, sdk)?;
        }
        Self::adapter_account_locked(self, &mut s, identity, sdk.is_some())
    }
    fn adapter_account_locked(
        &self,
        s: &mut State,
        identity: IdentityStatus,
        sdk_event: bool,
    ) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let same = matches!(&identity, IdentityStatus::Available(a) if a == &b.binding.facts.account) && b.verified;
        if !same {
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        }
        let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
        let isolation = match identity {
            IdentityStatus::Available(a) if a == b.binding.facts.account => {
                b.verified = b.usable;
                None
            }
            IdentityStatus::Available(_) => Some(IsolationReason::AccountChanged),
            IdentityStatus::SignedOut => Some(IsolationReason::SignedOut),
            IdentityStatus::SwitchedAccounts => Some(IsolationReason::AccountChanged),
            _ => {
                b.verified = false;
                None
            }
        };
        if let Some(reason) = isolation {
            b.verified = false;
            b.usable = false;
            b.binding.facts.state = BindingState::Isolated(reason);
            let id = b.binding.id;
            with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .isolate_runtime_binding(id, reason)
            })?;
        }
        if same && sdk_event {
            s.adapter.no_state = true;
            s.adapter.no_state_point += 1;
            self.emit_pending_locked(s, UpdateKind::Full, BTreeSet::new())?;
        }
        Ok(())
    }
    pub(super) fn adapter_snapshot_locked(s: &State) -> Result<Snapshot, RuntimeError> {
        let Some(b) = s.bound.as_ref() else {
            return s.adapter.closed_snapshot.clone().ok_or(RuntimeError::StaleIncarnation);
        };
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut entities = Vec::new();
            for kind in crate::cloud::row_payload::KINDS {
                let mut after = None;
                loop {
                    let page = store.entity_page(b.binding.id, kind, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|e| e.key.clone());
                    entities.extend(page);
                }
            }
            let runtime = store.stored_runtime_state(b.binding.id)?;
            let durable = store.adapter_durable(b.binding.id)?;
            let inbox = store.inbox(b.binding.id)?;
            let indexed_inbox = inbox.iter().map(|e| (e.id, &e.entry)).collect::<Vec<_>>();
            let tracking_content = crate::cloud::store::encode(&(
                store.binding(b.binding.id)?.map(|b| (b.id, b.created_at_ms, b.facts)),
                &runtime,
                &entities,
                indexed_inbox,
                &durable,
                store
                    .engine_state(b.binding.id)?
                    .map(|e| (e.serialization, e.saved_at_ms)),
            ))?
            .into_bytes();
            Ok(Snapshot {
                binding: b.binding.id.0.to_vec(),
                tracking_content,
                serialization: store.engine_state(b.binding.id)?.map(|s| s.serialization.0),
                entities,
                inbox: inbox.into_iter().map(|e| e.entry).collect(),
                root: runtime.and_then(|v| v.root_ack),
                tracking_stopped: s.tracking_stopped,
                absence_pass_completed: durable.absence.as_ref().is_some_and(|a| a.completed),
            })
        })
    }
    pub fn retain_adapter_inspection(&self) {
        self.state().adapter.retain_inspection = true;
    }
    pub fn adapter_snapshot(&self) -> Result<Snapshot, RuntimeError> {
        Self::adapter_snapshot_locked(&self.state())
    }
    fn adapter_commit_locked(
        s: &mut State,
        entries: &[InboxEntry],
        refused: &[super::super::incoming::RefusedEntry],
        serialization: Option<&[u8]>,
        at_ms: i64,
        fail: bool,
    ) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| {
            s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                b,
                entries,
                refused,
                serialization,
                &s.adapter.durable,
                at_ms,
                fail,
            )
        });
        if result.is_err() {
            Self::invalidate_locked(s);
        }
        result.map_err(Into::into)
    }
    pub fn adapter_state_update(&self, sdk: &str, serialization: &[u8]) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        #[cfg(feature = "test-hooks")]
        let fail = std::mem::take(&mut s.adapter.fail_state);
        #[cfg(not(feature = "test-hooks"))]
        let fail = false;
        Self::adapter_commit_locked(&mut s, &[], &[], Some(serialization), at_ms, fail)
    }
    pub fn adapter_fetched(
        &self,
        sdk: &str,
        zone: &str,
        records: &[Saved],
        deleted: &[String],
    ) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let (key, epoch) = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            let b = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
            (b.facts.record_key.0, b.facts.epoch.0)
        };
        if zone == "meguri-root" {
            // A root proof is accepted at its drained fetch, never from an old callback.
            for r in records {
                if r.name != "active" {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                crate::cloud::control_codec::Active::decode(&r.envelope).map_err(|_| Error::InvalidData)?;
            }
        } else if zone != collection_zone(EpochId(epoch)) {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        let (mut entries, refused) = if zone != "meguri-root" {
            // The collection's epoch header is a control record, never an entity.
            let entities: Vec<_> = records.iter().filter(|r| r.name != "epoch").cloned().collect();
            super::super::incoming::classify(&key, &entities, at_ms)
        } else {
            (Vec::new(), Vec::new())
        };
        if zone != "meguri-root" {
            for name in deleted {
                entries.push(InboxEntry {
                    record_type: RecordType::Entity,
                    record_name: name.clone(),
                    value: InboxValue::Deletion,
                    reason: InboxReason::Ready,
                    source: InboxSource::Fetch,
                    absence_pass: None,
                    retry_count: 0,
                    fetched_at_ms: at_ms,
                });
            }
        }
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        #[cfg(feature = "test-hooks")]
        let fail = std::mem::take(&mut s.adapter.fail_inbox);
        #[cfg(not(feature = "test-hooks"))]
        let fail = false;
        let previous = s.adapter.durable.clone();
        if zone == "meguri-root" {
            if deleted.iter().any(|name| name == "active") {
                s.adapter.durable.active = None;
                s.adapter.durable.active_deleted = true;
            } else if !s.adapter.durable.active_deleted
                && let Some(record) = records.last()
            {
                s.adapter.durable.active = Some(record.clone());
            }
        }
        if let Err(error) = Self::adapter_commit_locked(&mut s, &entries, &refused, None, at_ms, fail) {
            s.adapter.durable = previous;
            return Err(error);
        }
        if zone != "meguri-root" {
            s.incoming.retry_granted = true;
        }
        if s.adapter.durable.active_deleted {
            s.root_confirmed = false;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            if let Some(b) = s.bound.as_mut() {
                b.verified = false;
                b.usable = false;
                b.binding.facts.state = BindingState::Isolated(IsolationReason::ActiveMissing);
            }
        }
        Ok(())
    }
    pub fn adapter_begin_fetch(&self, sdk: &str, zones: Vec<String>) -> Result<u64, RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if s.adapter.fetch.is_some() {
            return Err(Error::InvalidData.into());
        }
        s.adapter.next_fetch += 1;
        let id = s.adapter.next_fetch;
        s.adapter.fetch = Some(FetchCall {
            id,
            scope: zones,
            ..Default::default()
        });
        Ok(id)
    }
    pub fn adapter_will_fetch(&self, sdk: &str, group: String, manual: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let boundary = FetchBoundary {
            group,
            manual,
            ..Default::default()
        };
        if let Some(call) = s.adapter.fetch.as_mut() {
            if call.boundary.take().is_some() {
                s.adapter.missing_did += 1;
            }
            s.adapter.fetch.as_mut().ok_or(Error::InvalidData)?.boundary = Some(boundary);
        } else {
            s.adapter.scheduled = Some(boundary);
        }
        Ok(())
    }
    pub fn adapter_fetch_zone(&self, sdk: &str, zone: String, will: bool, failed: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let point = s.adapter.no_state_point;
        let boundary = if let Some(call) = s.adapter.fetch.as_mut() {
            call.boundary.as_mut()
        } else {
            s.adapter.scheduled.as_mut()
        };
        if let Some(boundary) = boundary {
            if will {
                boundary.zones.insert(zone, (point, false));
            } else if let Some((started, done)) = boundary.zones.get_mut(&zone) {
                *done = !failed && *started == point;
            }
        }
        Ok(())
    }
    pub fn adapter_did_fetch(&self, sdk: &str, group: &str, manual: bool) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let boundary = if s.adapter.fetch.is_some() {
            s.adapter.fetch.as_mut().and_then(|c| c.boundary.take())
        } else {
            s.adapter.scheduled.take()
        };
        let Some(boundary) = boundary else { return Ok(()) };
        if boundary.group != group || boundary.manual != manual {
            return Ok(());
        }
        let scope = s.adapter.fetch.as_ref().map(|c| c.scope.clone());
        let drains = if let Some(scope) = scope {
            scope
                .into_iter()
                .filter_map(|zone| match boundary.zones.get(&zone) {
                    Some((_, true)) => Some(FetchDrain { zone, fetched: true }),
                    None if !s.adapter.no_state => Some(FetchDrain { zone, fetched: false }),
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            boundary
                .zones
                .into_iter()
                .filter_map(|(zone, (_, done))| done.then_some(FetchDrain { zone, fetched: true }))
                .collect()
        };
        if drains.iter().any(|d| d.zone == "meguri-root") {
            let root = if s.adapter.durable.active_deleted {
                None
            } else {
                s.adapter.durable.active.clone().or(with_tracking_access(|| {
                    s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .stored_runtime_state(s.bound.as_ref().ok_or(Error::Database)?.binding.id)
                })?
                .and_then(|v| v.root_ack)
                .map(|r| Saved {
                    name: "active".into(),
                    envelope: r.envelope.0,
                    system_fields: r.system_fields.0,
                }))
            };
            let request = s.adapter.fetch.as_ref().map_or(0, |c| c.id);
            drop(s);
            if let Some(root) = root {
                self.accept_root_fetch(sdk, Some(&root), true, request)?;
            }
            s = self.state();
            Self::sdk_locked(&s, sdk)?;
        }
        let old = s.adapter.durable.clone();
        s.adapter.durable.drains.extend(drains.clone());
        if let Err(e) = Self::adapter_commit_locked(&mut s, &[], &[], None, at_ms, false) {
            s.adapter.durable = old;
            return Err(e);
        }
        if let Some(call) = s.adapter.fetch.as_mut() {
            call.completed = true;
            call.drains.extend(drains.into_iter().map(|d| d.zone));
        }
        Ok(())
    }
    pub fn adapter_finish_fetch(&self, sdk: &str, id: u64) -> Result<bool, RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let call = s.adapter.fetch.take().ok_or(Error::InvalidData)?;
        if call.id != id {
            return Err(Error::InvalidData.into());
        }
        if call.boundary.is_some() {
            s.adapter.missing_did += 1;
        }
        Ok(call.completed && call.scope.iter().all(|z| call.drains.contains(z)))
    }
    pub fn adapter_fetch_drains(&self) -> Vec<FetchDrain> {
        self.state().adapter.durable.drains.clone()
    }
    pub fn adapter_missing_did(&self) -> u64 {
        self.state().adapter.missing_did
    }
    pub fn adapter_exchange_fetch(&self, sdk: &str, complete: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if complete {
            s.actions.exchange_phase = None;
        }
        Ok(())
    }
    pub fn adapter_complete_exchange(
        &self,
        sdk: &str,
        id: u64,
        fetched: bool,
        terminal: Option<CollectionOutcome>,
    ) -> Result<CompletedExchange, RuntimeError> {
        let mut s = self.state();
        if let Some(completed) = s.adapter.durable.exchanges.get(&id) {
            return Ok(completed.clone());
        }
        if let Some((owner, completed)) = s.adapter.closed_exchanges.get(&id)
            && owner == sdk
        {
            return Ok(completed.clone());
        }
        if s.adapter.exchange_owner.as_ref() != Some(&(id, sdk.into(), s.incarnation)) {
            return Err(RuntimeError::StaleIncarnation);
        }
        let (active_id, result) = s.actions.exchange.clone().ok_or(Error::InvalidData)?;
        if active_id != id {
            return Err(Error::InvalidData.into());
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let live = Self::sdk_locked(&s, sdk).is_ok();
        let inbox_empty = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.inbox(binding))?.is_empty();
        let outcome = if s
            .bound
            .as_ref()
            .is_some_and(|b| matches!(b.binding.facts.state, BindingState::Isolated(_)))
        {
            CollectionOutcome::Failed
        } else if !live {
            CollectionOutcome::Cancelled
        } else if let Some(outcome @ (CollectionOutcome::Failed | CollectionOutcome::Cancelled)) = terminal {
            outcome
        } else if !fetched {
            CollectionOutcome::WaitingNoAccount
        } else if result.outstanding_names.is_empty() && inbox_empty {
            CollectionOutcome::Succeeded
        } else {
            CollectionOutcome::Incomplete
        };
        let completed = CompletedExchange {
            id,
            collection: outcome,
            result,
        };
        let mut durable = s.adapter.durable.clone();
        durable.exchanges.insert(id, completed.clone());
        if let Err(error) = with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .adapter_record_exchange(binding, &durable)
        }) {
            Self::invalidate_locked(&mut s);
            return Err(error.into());
        }
        s.adapter.durable = durable;
        s.actions.exchange = None;
        s.actions.exchange_phase = None;
        s.actions.selected_revs.clear();
        s.adapter.exchange_owner = None;
        Ok(completed)
    }
    pub fn adapter_exchange_result(&self, id: u64) -> Option<CompletedExchange> {
        self.state().adapter.durable.exchanges.get(&id).cloned()
    }
    pub fn adapter_exchange_active(&self) -> bool {
        self.state().actions.exchange.is_some()
    }
    /// Compare after actual send return (A23 false), without a full-policy trigger.
    pub fn adapter_repair_pending(
        &self,
        sdk: &str,
        pending: BTreeMap<String, PendingChange>,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let desired = s.actions.desired.clone();
        let missing = desired
            .iter()
            .filter(|(n, v)| pending.get(*n) != Some(*v))
            .map(|(n, _)| n.clone())
            .collect::<BTreeSet<_>>();
        if !missing.is_empty() {
            s.actions.update(UpdateKind::Changes, PendingSnapshot(desired), missing);
        }
        Ok(())
    }
}
impl Runtime {
    pub fn adapter_send_due(&self, sdk: &str) -> Result<bool, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let Some((id, exchange)) = &s.actions.exchange else {
            return Ok(false);
        };
        Ok(!s.adapter.sdk_retrying
            && !exchange.outstanding_names.is_empty()
            && s.actions.calls.iter().filter(|c| c.exchange == Some(*id)).count() < 8)
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Preparation only: a synthetic previously confirmed service baseline.
    /// All later edits, review commits, scans, flights and acks use normal paths.
    pub fn prepare_adapter_baseline_for_testing(
        &self,
        collection: &Collection,
        clock: &dyn meguri_model::clock::Clock,
        random: &dyn meguri_model::rng::RandomSource,
        saved_state: Option<Vec<u8>>,
    ) -> Result<(Vec<Saved>, Saved, Saved), RuntimeError> {
        use crate::cloud::row_payload::{self, View};
        if self.inputs.scope.container.as_str() != "synthetic-u114" {
            return Err(Error::InvalidData.into());
        }
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let view = row_payload::ReadView(&*snapshot);
        let mut layouts = BTreeMap::new();
        let mut after = None;
        loop {
            let page = view.page(Table::Notetypes, after.as_ref())?;
            if page.is_empty() {
                break;
            }
            for row in page {
                let id = row_payload::int(&row, 0)?;
                after = Some(RowKey::Id(id));
                layouts.insert(id, row_payload::notetype(&view, &row, 1)?);
            }
        }
        let record_key = [114; 32];
        let epoch = [114; 16];
        let mut records = Vec::new();
        for kind in row_payload::KINDS {
            let Some(table) = row_payload::table(kind) else {
                continue;
            };
            let mut after = None;
            loop {
                let page = view.page(table, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?);
                    let key = row_payload::key(kind, &row)?;
                    let payload = row_payload::project(&view, kind, &row, |id| {
                        layouts.get(&id).cloned().ok_or(Error::InvalidData)
                    })?;
                    let name = codec::entity_name(&record_key, kind, &key).map_err(|_| Error::InvalidData)?;
                    let envelope = codec::Entity::build(
                        codec::EntityFields {
                            kind,
                            key,
                            origin: [31; 16],
                            erev: 1,
                            generation: None,
                            tombstone: false,
                            deleted_at_ms: 0,
                            writer: [41; 16],
                            base_sha256: None,
                        },
                        &payload.encode().map_err(|_| Error::InvalidData)?,
                    )
                    .map_err(|_| Error::InvalidData)?
                    .encode()
                    .map_err(|_| Error::InvalidData)?;
                    records.push(Saved {
                        name,
                        envelope,
                        system_fields: b"synthetic-confirmed-system-fields".to_vec(),
                    });
                }
            }
        }
        let root = Saved {
            name: "active".into(),
            envelope: crate::cloud::control_codec::Active {
                epoch,
                activated_at_ms: clock.now_ms(),
                previous: None,
                replaced_through_ms: None,
            }
            .encode()
            .map_err(|_| Error::InvalidData)?,
            system_fields: b"synthetic-root-system-fields".to_vec(),
        };
        let incoming_payload = codec::Payload::Config(codec::Config {
            key: b"synthetic-incoming".to_vec(),
            mtime_secs: clock.now_ms() / 1000,
            val: b"true".to_vec(),
        });
        let incoming_key = row_payload::payload_key(&incoming_payload)?;
        let incoming = Saved {
            name: codec::entity_name(&record_key, codec::Kind::Config, &incoming_key)
                .map_err(|_| Error::InvalidData)?,
            envelope: codec::Entity::build(
                codec::EntityFields {
                    kind: codec::Kind::Config,
                    key: incoming_key,
                    origin: [51; 16],
                    erev: 2,
                    generation: None,
                    tombstone: false,
                    deleted_at_ms: 0,
                    writer: [61; 16],
                    base_sha256: None,
                },
                &incoming_payload.encode().map_err(|_| Error::InvalidData)?,
            )
            .map_err(|_| Error::InvalidData)?
            .encode()
            .map_err(|_| Error::InvalidData)?,
            system_fields: b"synthetic-incoming-system-fields".to_vec(),
        };
        let publication = meguri_engine::device_state::DeviceState::for_collection(collection.path())
            .publication()?
            .ok_or(Error::InvalidData)?
            .identity;
        {
            let mut s = self.state();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_mut().ok_or(Error::Database)?;
                if store.selected_binding()?.is_some() {
                    return Err(Error::InvalidData.into());
                }
                let binding = store.create_binding(
                    BindingFacts {
                        account: OpaqueBytes(b"synthetic-account-A".to_vec()),
                        epoch: EpochId(epoch),
                        record_key: RecordKey(record_key),
                        publication,
                        state: BindingState::Active,
                    },
                    clock,
                    random,
                )?;
                for record in &records {
                    store.prepare_entity_baseline_for_testing(
                        binding.id,
                        &record.name,
                        &record.envelope,
                        OpaqueBytes(record.system_fields.clone()),
                    )?;
                }
                store.adapter_install()?;
                store.adapter_commit(
                    binding.id,
                    &[],
                    &[],
                    saved_state.as_deref(),
                    &Durable::default(),
                    clock.now_ms(),
                    false,
                )?;
                Ok(())
            })?;
        }
        drop(snapshot);
        drop(lease);
        self.bind(collection)?;
        self.adapter_account(
            None,
            IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
        )?;
        Ok((records, root, incoming))
    }
    pub fn arm_adapter_faults_for_testing(&self, inbox: bool, state: bool) {
        let mut s = self.state();
        s.adapter.fail_inbox = inbox;
        s.adapter.fail_state = state;
    }
}

impl Runtime {
    pub fn adapter_tracking_failure(&self, sdk: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        Self::invalidate_locked(&mut s);
        Ok(())
    }
}

impl Runtime {
    pub fn adapter_sdk_retrying(&self, sdk: &str, retained_error: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        s.adapter.sdk_retrying |= retained_error;
        Ok(())
    }
}

impl Runtime {
    pub fn adapter_begin_identity_read(
        &self,
        _transition: bool,
        origin_sdk: Option<&str>,
    ) -> Result<u64, RuntimeError> {
        let mut s = self.state();
        if let Some(sdk) = origin_sdk {
            Self::sdk_locked(&s, sdk)?;
        }
        if s.bound.is_none() {
            return Err(RuntimeError::StaleIncarnation);
        }
        if origin_sdk.is_some() {
            s.adapter.no_state = true;
            s.adapter.no_state_point += 1;
            self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
        }
        s.adapter.next_identity = s.adapter.next_identity.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.adapter.next_identity;
        // A pending lookup is not evidence of a different account. Keep the
        // verified owner accepting records and their following cursor (V9.1).
        s.adapter.identity_read = Some((
            id,
            Self::adapter_guard_locked(&s),
            s.incarnation,
            origin_sdk.map(str::to_owned),
        ));
        Ok(id)
    }
    pub fn adapter_complete_identity_read(&self, id: u64, identity: IdentityStatus) -> Result<bool, RuntimeError> {
        let mut s = self.state();
        let Some((request, guard, incarnation, origin_sdk)) = s.adapter.identity_read.clone() else {
            return Ok(false);
        };
        if request != id {
            return Ok(false);
        }
        s.adapter.identity_read = None;
        if s.bound.is_none()
            || guard != Self::adapter_guard_locked(&s)
            || incarnation != s.incarnation
            || origin_sdk
                .as_deref()
                .is_some_and(|sdk| Self::sdk_locked(&s, sdk).is_err())
        {
            return Ok(false);
        }
        // A failed later read preserves an already verified process owner (V9.1).
        if !matches!(identity, IdentityStatus::ReadFailed) {
            self.adapter_account_locked(&mut s, identity, false)?;
        }
        Ok(true)
    }
}

impl Runtime {
    pub(super) fn adapter_close_exchange_locked(s: &mut State) {
        if let Some((id, result)) = s.actions.exchange.take()
            && let Some((owned_id, owner, _)) = s.adapter.exchange_owner.take()
            && id == owned_id
        {
            // P12.7: close has no durable row or successor-generation write.
            s.adapter.closed_exchanges.insert(
                id,
                (
                    owner,
                    CompletedExchange {
                        id,
                        collection: CollectionOutcome::Cancelled,
                        result,
                    },
                ),
            );
        }
        s.actions.exchange_phase = None;
        s.actions.selected_revs.clear();
    }
}
