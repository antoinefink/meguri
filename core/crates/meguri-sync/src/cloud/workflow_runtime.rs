use super::*;
use crate::cloud::policy::PolicyWork;
use crate::cloud::{
    row_payload::{self, View},
    workflow::*,
};
use std::collections::{BTreeMap, BTreeSet};
impl Runtime {
    pub(super) fn record_exchange_drops_locked(s: &mut State, dropped: Vec<(String, u64)>) {
        if let Some((_, exchange)) = &mut s.actions.exchange {
            for (name, revision) in dropped {
                if s.actions
                    .selected_revs
                    .get(&name)
                    .is_some_and(|selected| *selected <= revision)
                {
                    exchange.record_drop(name);
                }
            }
        }
    }
    #[cfg(feature = "test-hooks")]
    pub fn set_collection_record_limit_for_testing(&self, limit: usize) -> Result<(), RuntimeError> {
        if !(1..=250).contains(&limit) {
            return Err(Error::InvalidData.into());
        }
        self.state().actions.record_limit = limit;
        Ok(())
    }
    pub fn attach_sources(
        &self,
        random: Arc<dyn meguri_model::rng::RandomSource>,
        review: std::sync::Weak<meguri_engine::session::ReviewService>,
    ) {
        *self.random.lock().unwrap_or_else(PoisonError::into_inner) = Some(random);
        *self.review.lock().unwrap_or_else(PoisonError::into_inner) = review;
    }
    pub fn trace(&self) -> Vec<TraceEvent> {
        self.state().trace.clone()
    }
    pub fn start_sdk_instance(&self, instance: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if !s.whole_complete || s.scan_required || !s.bound.as_ref().is_some_and(|b| b.verified && b.usable) {
            return Err(RuntimeError::IdentityUnverified);
        }
        let account = String::from_utf8(
            s.bound
                .as_ref()
                .ok_or(Error::InvalidData)?
                .binding
                .facts
                .account
                .0
                .clone(),
        )
        .map_err(|_| Error::InvalidData)?;
        if !s
            .trace
            .iter()
            .any(|e| matches!(e,TraceEvent::EngineStarted{instance:i,..} if i==instance))
        {
            s.trace.push(TraceEvent::EngineStarted {
                instance: instance.into(),
                account,
            });
        }
        Ok(())
    }
    pub(super) fn instance_locked(s: &State) -> Result<String, RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let binding = b.binding.id.0.iter().map(|v| format!("{v:02x}")).collect::<String>();
        Ok(format!(
            "{binding}-{}-{}-{}",
            b.open_counter, s.incarnation, b.file.inode
        ))
    }
    pub fn instance(&self) -> Result<String, RuntimeError> {
        Self::instance_locked(&self.state())
    }
    pub(super) fn caller_locked(s: &State, instance: &str) -> Result<(), RuntimeError> {
        let host_instance = Self::instance_locked(s)?;
        let sdk_valid = s
            .adapter
            .sdk
            .as_ref()
            .is_some_and(|sdk| sdk.id == instance && sdk.guard == Self::adapter_guard_locked(s) && !sdk.stopping);
        if !sdk_valid && host_instance != instance {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !b.usable || !b.verified || b.binding.facts.state != BindingState::Active {
            return Err(RuntimeError::IdentityUnverified);
        }
        Ok(())
    }
    /// SDKs call this before changing their own pending state for a delivery.
    /// Every mutating core API repeats the check under its commit's state lock.
    pub fn validate_caller(&self, instance: &str) -> Result<(), RuntimeError> {
        Self::caller_locked(&self.state(), instance)
    }
    pub fn set_automatic(&self, instance: &str, on: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        s.actions.automatic = on;
        Ok(())
    }
    pub(super) fn active_review(&self) -> Option<u64> {
        self.review
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .upgrade()
            .and_then(|r| r.active_session())
            .map(|s| s.0)
    }
    /// Called off the writer after real ReviewService inputs complete.
    pub fn poll_review(&self, instance: &str) -> Result<Option<u64>, RuntimeError> {
        self.poll_review_with_work(instance, &mut PolicyWork::default())
    }
    pub fn poll_review_with_work(&self, instance: &str, work: &mut PolicyWork) -> Result<Option<u64>, RuntimeError> {
        let _ = self.adapter_now();
        let active = self.active_review();
        let mut s = self.state();
        if Self::caller_locked(&s, instance).is_err() {
            return Ok(None);
        }
        if let Some(old) = s.last_session {
            if active != Some(old) {
                work.effect()?;
                s.actions.observations.push(Observation::SessionEnded { session: old });
                s.last_session = active;
                s.cloud.detached.clear();
                s.cloud.incoming_held.clear();
                s.cloud.deferred.clear();
                self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
                return Ok(Some(old));
            }
        } else if active.is_some() {
            work.effect()?;
            s.last_session = active;
        }
        work.check()?;
        Ok(None)
    }
    pub(super) fn workflow_gate(&self) -> Result<(), RuntimeError> {
        let s = self.state();
        Self::gate_locked(&s)
    }
    pub(super) fn gate_locked(s: &State) -> Result<(), RuntimeError> {
        Self::capture_gate_locked(s)?;
        if s.adapter.known_loss.is_some()
            || s.adapter.durable.collection_loss.is_some()
            || s.adapter.durable.epoch_verification.is_some()
            || (!s.root_confirmed && !Self::publication_allows_send_locked(s))
        {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        Ok(())
    }
    // A manual exchange must capture before its worker can fetch root proof.
    // This grants selection only; physical sends still use gate_locked.
    fn capture_gate_locked(s: &State) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !b.usable || !b.verified {
            return Err(RuntimeError::IdentityUnverified);
        }
        if Self::publication_holds_binding_locked(s) {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        if !s.whole_complete
            || s.scan_required
            || s.scan_running
            || s.tracking_stopped
            || s.adapter.paused
            || s.incoming.running
        {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        Ok(())
    }
    /// Full-image diagnostic readout. Production selection uses stored headers.
    pub fn entity_facts(&self, name: &str) -> Result<Option<EntityFacts>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .entity_dump(b.binding.id, name)?)
        })
    }
    #[cfg(feature = "test-hooks")]
    pub fn payload_proof_for_testing(&self, name: &str) -> Result<Option<PayloadTrackingProof>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            let Some(h) = s
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .entity_header(b.binding.id, name)?
            else {
                return Ok(None);
            };
            let digest = h.local_digest();
            let e = h.facts;
            Ok(Some(PayloadTrackingProof {
                name: e.name,
                local_rev: e.local_rev,
                acked_rev: e.acked_rev,
                server_erev: e.server_erev,
                dirty: e.mark.is_some(),
                flights: e.in_flight.len(),
                payload_digest: digest,
                acked_payload_digest: e.acked_payload_digest,
                system_fields: e.acked_system_fields,
                envelope: e.acked_envelope.0,
            }))
        })
    }
    /// Full-image diagnostic page, including canonical retained flight payloads.
    pub fn entity_facts_page(&self, kind: codec::Kind, after: Option<&[u8]>) -> Result<Vec<EntityFacts>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            store
                .entity_keys_page(b.binding.id, kind, after)?
                .into_iter()
                .map(|key| {
                    let h = store
                        .entity_key_header(b.binding.id, kind, &key)?
                        .ok_or(Error::InvalidData)?;
                    Ok(store
                        .entity_dump(b.binding.id, &h.facts.name)?
                        .ok_or(Error::InvalidData)?)
                })
                .collect()
        })
    }
    pub(super) fn sendables_locked(s: &State) -> Result<PendingSnapshot, RuntimeError> {
        if Self::gate_locked(s).is_err() {
            return Ok(PendingSnapshot::empty());
        }
        Self::pending_selection_locked(s)
    }
    fn pending_selection_locked(s: &State) -> Result<PendingSnapshot, RuntimeError> {
        Self::capture_gate_locked(s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let mut pending = store.dirty_names(b.binding.id)?;
        pending.0.retain(|name, _| {
            !s.cloud.deferred.contains(name) && !s.cloud.incoming_held.contains(name) && !s.policy.parked.contains(name)
        });
        if !s.media.paused
            && !s.adapter.paused
            && let Some((sdk, work)) = &s.actions.media_upload
            && s.adapter
                .sdk
                .as_ref()
                .is_some_and(|current| current.id == *sdk && !current.stopping)
            && let super::super::media::Request::Upload { name, .. } = &work.request
            && !s.policy.parked.contains(name)
        {
            pending.0.insert(name.clone(), PendingChange::Save);
        }
        let allowed = s.root_confirmed
            && !s.media.paused
            && s.media.enabled != Some(false)
            && !Self::publication_allows_send_locked(s);
        for name in store.media_deletions(b.binding.id, s.media.observed_now_ms, allowed)? {
            if !s.policy.parked.contains(&name) {
                pending.0.insert(name, PendingChange::Delete);
            }
        }
        Ok(pending)
    }
    pub(super) fn emit_pending_locked(
        &self,
        s: &mut State,
        kind: UpdateKind,
        mut force: BTreeSet<String>,
    ) -> Result<PendingUpdate, RuntimeError> {
        s.media.observed_now_ms = self.pending_clock.load(std::sync::atomic::Ordering::Relaxed);
        if kind == UpdateKind::Full {
            s.cloud.deferred.clear();
            s.policy.parked.clear();
            s.policy.held_images.clear();
        }
        // Callers with the state guard hold tracking access only for row reads.
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let allowed = s.root_confirmed
                && s.media.enabled != Some(false)
                && !s.media.paused
                && !Self::publication_allows_send_locked(s);
            if let Some(b) = s.bound.as_ref().map(|b| b.binding.id) {
                s.store.as_mut().ok_or(Error::Database)?.clear_invalid_media_deletions(
                    b,
                    s.media.observed_now_ms,
                    allowed,
                )?;
            }
            Ok(())
        })?;
        let snapshot = with_tracking_access(|| -> Result<_, RuntimeError> {
            let snapshot = Self::sendables_locked(s)?;
            if kind == UpdateKind::Full
                && let Some(sdk) = s.adapter.sdk.as_ref().filter(|sdk| !sdk.stopping)
                && let Some(bound) = &s.bound
            {
                // A full update can keep an already queued save only while it
                // represents the same work. An edit after handoff needs a new
                // pending addition even while the SDK still holds that name.
                let mut handed = BTreeMap::new();
                for ((owner, _), records) in &s.actions.request_flights {
                    if owner != &sdk.id {
                        continue;
                    }
                    for (name, flight) in records {
                        if snapshot.0.get(name) == Some(&PendingChange::Save) {
                            handed
                                .entry(name)
                                .and_modify(|rev: &mut u64| *rev = (*rev).max(flight.local_rev))
                                .or_insert(flight.local_rev);
                        }
                    }
                }
                let store = s.store.as_ref().ok_or(Error::Database)?;
                for (name, revision) in handed {
                    if store
                        .entity_header(bound.binding.id, name)?
                        .is_some_and(|header| header.facts.local_rev > revision)
                    {
                        force.insert(name.clone());
                    }
                }
            }
            Ok(snapshot)
        })?;
        Ok(s.actions.update(kind, snapshot, force))
    }
    pub fn pending_transition(
        &self,
        instance: &str,
        kind: UpdateKind,
        snapshot: PendingSnapshot,
    ) -> Result<PendingUpdate, RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if kind == UpdateKind::Full {
            s.cloud.deferred.clear();
        }
        let mut snapshot = snapshot;
        snapshot
            .0
            .retain(|name, _| !s.cloud.incoming_held.contains(name) && !s.cloud.deferred.contains(name));
        Ok(s.actions.update(kind, snapshot, BTreeSet::new()))
    }
    pub fn tracking_sendables(&self) -> BTreeMap<String, PendingChange> {
        self.state().actions.desired.clone()
    }
    pub fn take_pending_updates(&self, instance: &str) -> Result<Vec<PendingUpdate>, RuntimeError> {
        let mut s = self.state();
        if Self::caller_locked(&s, instance).is_err() {
            return Ok(Vec::new());
        }
        Ok(s.actions.take())
    }
    pub fn observations(&self) -> Vec<Observation> {
        self.state().actions.observations.clone()
    }
    pub fn emitted_update_ids(&self) -> Vec<u64> {
        self.state().actions.emitted.clone()
    }
    pub fn sdk_update_applied(&self, instance: &str, id: u64, added: &[String]) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let action = s.actions.handed_updates.get(&id).ok_or(Error::InvalidData)?;
        if added.iter().any(|n| !action.adds.contains_key(n)) {
            return Err(Error::InvalidData.into());
        }
        s.actions.handed_updates.remove(&id);
        for name in added {
            s.actions.observations.push(Observation::PendingAdded {
                name: name.clone(),
                action_id: id,
            });
        }
        Ok(())
    }
    pub fn sdk_saved_removed(&self, instance: &str, request_id: u64, names: &[String]) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if s.actions.handler.is_some() {
            return Err(Error::InvalidData.into());
        }
        let expected = s.actions.saved_callbacks.get(&request_id).cloned().unwrap_or_default();
        if names.iter().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(Error::InvalidData.into());
        }
        s.actions.saved_callbacks.remove(&request_id);
        for name in names {
            s.actions.observations.push(Observation::SavedPendingRemoved {
                name: name.clone(),
                request_id,
            });
        }
        Ok(())
    }
    pub fn accepted_root_epoch(&self) -> Result<EpochId, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            let ack = s
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .runtime_binding(b.binding.id)?
                .root_ack
                .ok_or(Error::InvalidData)?;
            if ack.open != b.open_counter {
                return Err(Error::InvalidData.into());
            }
            Ok(ack.epoch)
        })
    }
    /// Bounded admission for collection conflict metadata, before handler entry.
    /// Headers prove original handed membership without hydrating any image.
    /// Full receipt validation remains in `sent_results_with_conflicts`.
    pub fn preflight_sent_conflicts(
        &self,
        sdk: &str,
        collection: &Collection,
        request_id: u64,
        names: &[String],
    ) -> Result<(), RuntimeError> {
        let evidence = collection.open_evidence()?;
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !collection.is_current()
            || b.evidence.generation != evidence.generation
            || b.file.pair() != evidence.native_file
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let handed = s
                .actions
                .request_flights
                .get(&(sdk.to_owned(), request_id))
                .ok_or(Error::InvalidData)?;
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for name in names {
                let flight = handed.get(name).ok_or(Error::InvalidData)?;
                let header = store.entity_header(b.binding.id, name)?.ok_or(Error::InvalidData)?;
                let local = &header.facts;
                if local.origin != flight.origin
                    || !local.in_flight.iter().any(|f| {
                        f.local_rev == flight.local_rev
                            && f.erev == flight.erev
                            && f.payload_digest == flight.payload_digest
                            && f.tombstone == flight.tombstone
                            && (!f.tombstone
                                || local.kind == codec::Kind::MediaName
                                || f.tombstone_envelope_digest == Some(flight.envelope_digest))
                    })
                {
                    return Err(Error::InvalidData.into());
                }
            }
            Ok(())
        })
    }
    pub fn enter_sent_handler(&self, instance: &str, request_id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        s.actions.begin_handler(request_id)?;
        #[cfg(feature = "test-hooks")]
        s.actions.smoke_acks.conflicts.clear();
        Ok(())
    }
    pub fn return_sent_handler(&self, instance: &str) -> Result<u64, RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let request = s.actions.end_handler()?;
        #[cfg(feature = "test-hooks")]
        s.actions.smoke_acks.conflicts.clear();
        let key = (instance.to_owned(), request);
        s.actions.request_flights.remove(&key);
        s.policy.flights.remove(&key);
        if s.actions.payload_uploads.contains_key(&key) {
            s.actions.payload_handlers_returned.insert(key.clone());
        }
        let sends = s.actions.payload_upload_sends.get(&key);
        // Automatic SDK sends have no explicit SendCall. Handler return is
        // their drain point even when a failed record remains outstanding.
        let release = sends.is_none_or(|ids| {
            s.actions
                .calls
                .iter()
                .filter(|call| ids.contains(&call.id))
                .all(|call| call.returned)
        });
        if release {
            s.actions.payload_uploads.remove(&key);
            s.actions.payload_upload_sends.remove(&key);
            s.actions.payload_handlers_returned.remove(&key);
        }
        Ok(request)
    }
    /// V2.4 settles intents before a scan. A publisher's new binding cannot
    /// seed structural rows until its own controls have reached Zones (V15.2).
    pub fn prepare_binding(
        &self,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        self.refresh_identity()?;
        let status = self.status()?;
        if status.scan_completed_this_open {
            return Ok(());
        }
        let publishing_bound = self.publication_op()?.is_some_and(|op| {
            op.step == crate::cloud::publication::PublishStep::Bound
                && status.binding.as_ref().is_some_and(|b| op.binding_after == Some(b.id))
        });
        if !publishing_bound {
            return self.reconcile_binding(collection, media, timeout).map(|_| ());
        }
        self.settle_before_transition(collection, timeout)?;
        let guard = {
            let s = self.state();
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if !b.usable || !b.verified {
                return Err(RuntimeError::IdentityUnverified);
            }
            if s.scan_running || s.invalidation_exhausted {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            }
        };
        let defer = self.with_scan(&guard, collection, |s| {
            let op = s.store.as_ref().ok_or(Error::Database)?.publication_op()?;
            let Some(op) = op.filter(|op| {
                op.binding_after == Some(guard.binding) && op.step == crate::cloud::publication::PublishStep::Bound
            }) else {
                return Ok(false);
            };
            Self::require_supported_publication(&op)?;
            Ok(true)
        })?;
        if !defer {
            self.reconcile_binding(collection, media, timeout)?;
        }
        Ok(())
    }

    pub fn reconcile_binding(
        &self,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        timeout: Duration,
    ) -> Result<NoteScanProof, RuntimeError> {
        self.settle_before_transition(collection, timeout)?;
        #[cfg(feature = "test-hooks")]
        let mut scan_pause = self.scan_pause.lock().unwrap_or_else(PoisonError::into_inner).take();
        let guard = {
            let mut s = self.state();
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if !b.usable || !b.verified {
                return Err(RuntimeError::IdentityUnverified);
            }
            if s.scan_running || s.invalidation_exhausted {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            let g = ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            };
            s.scan_running = true;
            s.whole_complete = false;
            g
        };
        let publishing = self.publication_op()?.is_some_and(|op| {
            op.binding_after == Some(guard.binding) && op.step == crate::cloud::publication::PublishStep::Zones
        });
        let instance = self.instance()?;
        let result = (|| {
            // V2.4 settles existing intents before this scan, but a durable
            // Ready delivery can still be queued after a fetch restart. Scan
            // only local facts alongside it; ApplyReady owns its envelope,
            // cursor and acknowledged base, and must settle before any send.
            self.with_scan(&guard, collection, |_| Ok(()))?;
            #[cfg(feature = "test-hooks")]
            Self::pause_scan(&mut scan_pause, ScanPausePoint::BeforeSnapshot)?;
            let lease = collection.reader(ReaderRole::Cloud)?;
            let snapshot = lease.snapshot()?;
            let receipt = match collection.wait_wal_covered(snapshot.wal_identity(), snapshot.seq(), timeout)? {
                WalCoverage::Covered(r)
                    if r.identity() == snapshot.wal_identity()
                        && r.through() >= snapshot.seq()
                        && snapshot.generation() == guard.generation
                        && r.identity().native_file() == guard.file.pair() =>
                {
                    r
                }
                _ => return Err(RuntimeError::StaleIncarnation),
            };
            let view = row_payload::ReadView(&*snapshot);
            let col = snapshot.col()?;
            let scan_cursor = NoteScanCursor {
                seq: snapshot.seq().0,
                generation: snapshot.generation().0,
                covered_through: receipt.through().0,
                native_file: guard.file,
                modified: col.modified.0,
                schema_modified: col.schema_modified.0,
            };
            let mut recovered = Vec::new();
            let mut counts = Vec::new();
            let mut retained_tombstones = Vec::new();
            for kind in row_payload::KINDS.into_iter().filter(|k| *k != codec::Kind::MediaName) {
                let table = row_payload::table(kind).ok_or(Error::InvalidData)?;
                // Include tracked absences; a local-only scan cannot prove completeness.
                let mut after = None;
                let mut tombstones = 0;
                loop {
                    let page = self.with_scan(&guard, collection, |s| {
                        Ok(s.store.as_ref().ok_or(Error::Database)?.entity_keys_page(
                            guard.binding,
                            kind,
                            after.as_deref(),
                        )?)
                    })?;
                    if page.is_empty() {
                        break;
                    }
                    for key in page {
                        if view
                            .row(table, &super::super::workflow_store::row_key(kind, &key)?)?
                            .is_none()
                        {
                            let e = self.with_scan(&guard, collection, |s| {
                                Ok(s.store
                                    .as_ref()
                                    .ok_or(Error::Database)?
                                    .entity_image(guard.binding, kind, &key)?
                                    .ok_or(Error::InvalidData)?)
                            })?;
                            e.validate()?;
                            if kind == codec::Kind::Revlog && !e.sent && e.server_erev == 0 && e.in_flight.is_empty() {
                                let removed = self.with_scan(&guard, collection, |s| {
                                    Ok(s.store.as_mut().ok_or(Error::Database)?.remove_unsent_scan_revlog(
                                        guard.binding,
                                        &key,
                                        guard.open,
                                        &scan_cursor,
                                    )?)
                                })?;
                                if !removed {
                                    return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                                }
                                after = Some(key);
                                continue;
                            }
                            let deleted = e.tombstone_at.is_some()
                                || (e.server_erev > 0
                                    && e.acked_tombstone()
                                    && e.mark.is_none()
                                    && e.local_rev == e.acked_rev);
                            if !matches!(kind, codec::Kind::Note | codec::Kind::Card) || !deleted {
                                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                            }
                            // K11.4/K17.1: absence is already recorded. Preserve
                            // its pending revision, base, ACK and handed flights.
                            tombstones += 1;
                        }
                        after = Some(key);
                    }
                }
                if matches!(kind, codec::Kind::Note | codec::Kind::Card) {
                    retained_tombstones.push((kind, tombstones));
                }
                let mut cursor = None;
                let mut count = 0;
                loop {
                    let page = view.page(table, cursor.as_ref())?;
                    if page.is_empty() {
                        break;
                    }
                    for row in page {
                        let key = row_payload::key(kind, &row)?;
                        cursor = Some(RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?);
                        // Copy only one tracking body per projection, then release guards.
                        let mut existing = self.with_scan(&guard, collection, |s| {
                            Ok(s.store
                                .as_ref()
                                .ok_or(Error::Database)?
                                .entity_image(guard.binding, kind, &key)?)
                        })?;
                        let newly_observed = existing.is_none();
                        if newly_observed
                            && !publishing
                            && !matches!(
                                kind,
                                codec::Kind::Note | codec::Kind::Card | codec::Kind::Revlog | codec::Kind::Config
                            )
                        {
                            return Err(Error::InvalidData.into());
                        }
                        let p = row_payload::project(&view, kind, &row, |id| {
                            let tracked = {
                                let s = self.state();
                                with_tracking_access(|| {
                                    s.store.as_ref().ok_or(Error::Database)?.entity_image(
                                        guard.binding,
                                        codec::Kind::Notetype,
                                        &codec::id_key(id),
                                    )
                                })
                            }?;
                            if let Some(e) = tracked {
                                match e.acked_payload.unwrap_or(e.payload) {
                                    codec::Payload::Notetype(n) => Ok(n),
                                    _ => Err(Error::InvalidData),
                                }
                            } else if publishing {
                                let raw = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
                                row_payload::notetype(&view, &raw, 1)
                            } else {
                                Err(Error::InvalidData)
                            }
                        })?;
                        let mut e = if let Some(e) = existing.take() {
                            e
                        } else {
                            let origin = self.publication_random_id()?;
                            self.with_scan(&guard, collection, |s| {
                                let store = s.store.as_mut().ok_or(Error::Database)?;
                                Ok(if publishing {
                                    store.insert_publication_entity(
                                        guard.binding,
                                        p.clone(),
                                        origin,
                                        guard.open,
                                        snapshot.seq().0,
                                        snapshot.generation().0,
                                    )?
                                } else {
                                    store.insert_workflow_entity(
                                        guard.binding,
                                        p.clone(),
                                        origin,
                                        (guard.open, snapshot.seq().0, snapshot.generation().0),
                                    )?
                                })
                            })?
                        };
                        if let (codec::Payload::Deck(old), codec::Payload::Deck(new)) = (&e.payload, &p)
                            && (old.kind != new.kind || old.parent_id != new.parent_id)
                        {
                            return Err(RuntimeError::Incomplete(Incomplete::StructuralLayout));
                        }
                        if !matches!(
                            kind,
                            codec::Kind::Note
                                | codec::Kind::Card
                                | codec::Kind::Revlog
                                | codec::Kind::Deck
                                | codec::Kind::Preset
                                | codec::Kind::Notetype
                                | codec::Kind::Config
                        ) && e.payload != p
                        {
                            return Err(RuntimeError::Incomplete(Incomplete::StructuralLayout));
                        }
                        if e.payload != p || (newly_observed && !publishing) {
                            recovered.push(e.name.clone());
                        }
                        e.payload = p;
                        self.with_scan(&guard, collection, |s| {
                            Ok(s.store.as_mut().ok_or(Error::Database)?.workflow_merge(
                                guard.binding,
                                guard.open,
                                snapshot.seq().0,
                                snapshot.generation().0,
                                e,
                            )?)
                        })?;
                        count += 1;
                    }
                }
                counts.push((kind, count));
            }
            self.consume_media_feed(collection, media)?;
            let feed = media.store().media_feed_snapshot().map_err(|_| Error::InvalidData)?;
            if !feed.status.complete {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            let media_count = self.with_scan(&guard, collection, |s| {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                let mut after = None;
                let mut count = 0u64;
                loop {
                    let page = store.entity_page(guard.binding, codec::Kind::MediaName, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    count += page.len() as u64;
                    after = page.last().map(|e| e.key.clone());
                }
                Ok(count)
            })?;
            #[cfg(feature = "test-hooks")]
            Self::pause_scan(&mut scan_pause, ScanPausePoint::BeforeMerge)?;
            let col = snapshot.col()?;
            let proof = NoteScanProof {
                snapshot_seq: snapshot.seq(),
                snapshot_generation: snapshot.generation(),
                snapshot_native_file: guard.file,
                receipt,
                recovered_names: recovered,
            };
            self.with_scan(&guard, collection, |s| {
                s.store.as_mut().ok_or(Error::Database)?.finish_runtime_note_scan(
                    guard.binding,
                    guard.open,
                    NoteScanCursor {
                        seq: snapshot.seq().0,
                        generation: snapshot.generation().0,
                        covered_through: receipt.through().0,
                        native_file: guard.file,
                        modified: col.modified.0,
                        schema_modified: col.schema_modified.0,
                    },
                )?;
                let mut coverage_counts = counts.iter().map(|(k, n)| (*k, *n as u64)).collect::<Vec<_>>();
                coverage_counts.push((codec::Kind::MediaName, media_count));
                s.store.as_mut().ok_or(Error::Database)?.finish_workflow_scan(
                    guard.binding,
                    CoverageLedger {
                        open: guard.open,
                        cursor: NoteScanCursor {
                            seq: snapshot.seq().0,
                            generation: snapshot.generation().0,
                            covered_through: receipt.through().0,
                            native_file: guard.file,
                            modified: col.modified.0,
                            schema_modified: col.schema_modified.0,
                        },
                        counts: coverage_counts,
                        retained_tombstones,
                        media_feed_id: feed.status.feed_id.0,
                        media_cursor: feed.status.counter,
                    },
                )?;
                s.trace.push(TraceEvent::ScanCommitted {
                    instance: instance.clone(),
                    native_file: guard.file,
                    generation: guard.generation.0,
                    covered_seq: receipt.through().0,
                    recovered_names: proof.recovered_names.clone(),
                });
                s.note_scan = Some(proof.clone());
                s.whole_complete = true;
                s.scan_required = false;
                s.tracking_stopped = false;
                for (kind, row_count) in counts {
                    s.actions
                        .observations
                        .push(Observation::ReconcileKindCompleted { kind, row_count });
                }
                s.actions.observations.push(Observation::ReconcileKindCompleted {
                    kind: codec::Kind::MediaName,
                    row_count: media_count as usize,
                });
                s.actions.observations.push(Observation::MediaFeedDrained {
                    entry_count: feed.items.len(),
                    feed_id: feed.status.feed_id.0,
                    cursor: feed.status.counter,
                });
                Ok(())
            })?;
            Ok(proof)
        })();
        let result = result.and_then(|proof| {
            let file = self.inputs.files.identity(collection.path())?;
            let publication =
                meguri_engine::device_state::DeviceState::for_collection(collection.path()).publication()?;
            Ok((proof, file, publication))
        });
        let mut s = self.state();
        if s.incarnation == guard.incarnation {
            s.scan_running = false;
            let result = result.and_then(|(proof, file, publication)| {
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                if file != guard.file || file.created_nanos >= 1_000_000_000 {
                    return Err(RuntimeError::FileIdentityUnavailable);
                }
                if (b.binding.id, b.open_counter, b.evidence.generation, b.file)
                    != (guard.binding, guard.open, guard.generation, guard.file)
                    || !b.usable
                    || !b.verified
                    || s.invalidation != guard.invalidation
                    || s.invalidation_exhausted
                    || !collection.is_current()
                    || collection.generation() != guard.generation
                    || !publication.as_ref().is_some_and(|p| {
                        p.identity == b.binding.facts.publication
                            && p.decision == meguri_engine::device_state::PublicationDecision::Clear
                    })
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                if Self::gate_locked(&s).is_ok() {
                    s.cloud.deferred.clear();
                    let snapshot = with_tracking_access(|| Self::sendables_locked(&s))?;
                    s.actions.update(UpdateKind::Full, snapshot, BTreeSet::new());
                }
                Ok(proof)
            });
            if result.is_err() {
                s.scan_required = true;
                s.whole_complete = false;
            }
            result
        } else {
            Err(RuntimeError::StaleIncarnation)
        }
    }
    /// A completed SDK fetch at the current no-state point, with no root-zone
    /// delivery, permits the V26.1 record-ID fallback. It is not confirmation.
    pub fn sdk_fetch_completed_without_root(&self, instance: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if !s.whole_complete || s.scan_required {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        s.root_fetch_qualified = true;
        Ok(())
    }
    pub fn root_read_due(&self) -> Result<bool, RuntimeError> {
        let s = self.state();
        Ok(!Self::publication_allows_send_locked(&s)
            && s.root_fetch_qualified
            && s.whole_complete
            && s.bound.as_ref().is_some_and(|b| b.verified && b.usable))
    }
    pub fn accept_root_read(
        &self,
        instance: &str,
        request_id: u64,
        envelope: &[u8],
        system_fields: &[u8],
        lost_zone: bool,
    ) -> Result<bool, RuntimeError> {
        self.accept_active_proof(instance, request_id, envelope, system_fields, lost_zone, (false, None))
    }
    pub(super) fn accept_fetch_root_read(
        &self,
        read: &super::super::adapter::FetchRead,
        envelope: &[u8],
        system_fields: &[u8],
    ) -> Result<bool, RuntimeError> {
        self.accept_active_proof(
            &read.sdk,
            read.fetch.unwrap_or_default(),
            envelope,
            system_fields,
            read.lost_collection,
            (false, Some(read)),
        )
    }
    fn accept_active_proof(
        &self,
        instance: &str,
        request_id: u64,
        envelope: &[u8],
        system_fields: &[u8],
        lost_zone: bool,
        source: (bool, Option<&super::super::adapter::FetchRead>),
    ) -> Result<bool, RuntimeError> {
        let (completed_fetch, read) = source;
        let _ = self.adapter_now();
        let active = super::super::control_codec::Active::decode(envelope).map_err(|_| Error::InvalidData)?;
        let mut s = self.state();
        if Self::caller_locked(&s, instance).is_err() {
            return Ok(false);
        }
        if let Some(read) = read {
            Self::fetch_read_locked(&s, read)?;
        }
        if !completed_fetch && !read.is_some_and(|r| r.known_loss) {
            s.trace.push(TraceEvent::RootReadRecorded {
                instance: instance.into(),
                request_id,
            });
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let id = b.binding.id;
        if Self::publication_allows_send_locked(&s) {
            return Ok(false);
        }
        if !b.verified
            || !b.usable
            || s.adapter.known_loss.is_some()
            || (s.adapter.durable.collection_loss.is_some() && !lost_zone)
            || (s.adapter.durable.epoch_verification.is_some() && read.is_none())
            || !s.whole_complete
            || s.scan_required
            || (!s.root_fetch_qualified && !lost_zone && !completed_fetch)
        {
            return Ok(false);
        }
        let reason = if active.epoch != b.binding.facts.epoch.0 {
            Some(IsolationReason::EpochSwitched)
        } else if lost_zone {
            Some(IsolationReason::ZoneMissing)
        } else {
            None
        };
        if let Some(reason) = reason {
            let (binding, durable) = with_tracking_access(|| {
                let store = s.store.as_mut().ok_or(Error::Database)?;
                let durable = store.isolate_runtime_binding_with_active(
                    id,
                    reason,
                    Saved {
                        name: "active".into(),
                        envelope: envelope.to_vec(),
                        system_fields: system_fields.to_vec(),
                    },
                )?;
                let binding = store.binding(id)?.ok_or(Error::BindingNotFound)?;
                Ok::<_, Error>((binding, durable))
            })?;
            s.adapter.durable = durable;
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.usable = false;
            b.verified = false;
            b.binding = binding;
            s.publish_authority();
            if read.is_some_and(|read| read.lost_collection) {
                s.adapter.pending_collection_loss = None;
            }
            return Ok(false);
        }
        let open = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.open_counter;
        let durable = with_tracking_access(|| {
            s.store.as_mut().ok_or(Error::Database)?.save_root_ack(
                id,
                RootAcknowledgement {
                    open,
                    request_id,
                    epoch: EpochId(active.epoch),
                    envelope: OpaqueBytes(envelope.to_vec()),
                    system_fields: OpaqueBytes(system_fields.to_vec()),
                },
            )
        })?;
        s.adapter.durable = durable;
        s.root_reads.insert(request_id);
        let first = !s.root_confirmed;
        s.root_confirmed = true;
        if first {
            s.trace.push(TraceEvent::RootConfirmed {
                instance: instance.into(),
            });
            self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
        }
        Ok(first)
    }
    pub fn accept_root_fetch(
        &self,
        instance: &str,
        applied_active: Option<&Saved>,
        completed: bool,
        request_id: u64,
    ) -> Result<bool, RuntimeError> {
        if !completed {
            return Ok(false);
        }
        let Some(active) = applied_active else { return Ok(false) };
        if active.name != "active" {
            return Err(Error::InvalidData.into());
        }
        self.accept_active_proof(
            instance,
            request_id,
            &active.envelope,
            &active.system_fields,
            false,
            (true, None),
        )
    }
    pub(super) fn project_send_row(
        &self,
        guard: &ScanGuard,
        collection: &Collection,
        view: &impl View,
        e: &EntityFacts,
        publishing: bool,
        row: &[Cell],
    ) -> Result<codec::Payload, Error> {
        row_payload::project(view, e.kind, row, |id| {
            let tracked = self
                .with_scan(guard, collection, |s| {
                    let name = codec::entity_name(
                        &s.bound
                            .as_ref()
                            .ok_or(RuntimeError::StaleIncarnation)?
                            .binding
                            .facts
                            .record_key
                            .0,
                        codec::Kind::Notetype,
                        &codec::id_key(id),
                    )
                    .map_err(|_| Error::InvalidData)?;
                    Ok(s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .entity_facts(guard.binding, &name)?)
                })
                .map_err(|_| Error::InvalidData)?;
            if let Some(e) = tracked
                && let codec::Payload::Notetype(n) = if e.mark.is_some() {
                    e.payload
                } else {
                    e.acked_payload.unwrap_or(e.payload)
                }
            {
                return Ok(n);
            }
            if publishing {
                let row = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
                return row_payload::notetype(view, &row, 1);
            }
            Err(Error::InvalidData)
        })
    }
    pub fn build_collection_batch(
        &self,
        instance: &str,
        collection: &Collection,
        requested: &BTreeSet<String>,
    ) -> Result<BuiltBatch, RuntimeError> {
        let empty = || BuiltBatch {
            id: 0,
            saves: Vec::new(),
        };
        {
            let s = self.state();
            if Self::caller_locked(&s, instance).is_err() || Self::gate_locked(&s).is_err() {
                return Ok(empty());
            }
        }
        self.poll_review(instance)?;
        self.consume_media_feed(collection, &self.media_service(collection)?)?;
        self.drain_incoming(collection, Duration::from_secs(30))?;
        if self.workflow_gate().is_err() {
            return Ok(empty());
        }
        let guard = {
            let s = self.state();
            Self::caller_locked(&s, instance)?;
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            }
        };
        // Named entity rows only. This is tracking data, never a collection
        // snapshot or a lease taken on the writer.
        let named = self.with_scan(&guard, collection, |s| {
            let mut named: BTreeMap<(Table, RowKey), BTreeSet<String>> = BTreeMap::new();
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for name in requested {
                if let Some(h) = store.entity_header(guard.binding, name)? {
                    let e = h.facts;
                    if e.kind != codec::Kind::Tag
                        && let Some(table) = row_payload::table(e.kind)
                    {
                        named
                            .entry((table, super::super::workflow_store::row_key(e.kind, &e.key)?))
                            .or_default()
                            .insert(name.clone());
                    }
                    if matches!(e.kind, codec::Kind::Notetype | codec::Kind::Tag) {
                        // Preserve layout-part keys and the stored tag spelling.
                        // Ordinary Note/Card key capture reads metadata only.
                        let full = store.entity_facts(guard.binding, name)?.ok_or(Error::InvalidData)?;
                        for (table, key, _) in row_payload::payload_rows(&full.payload)? {
                            named.entry((table, key)).or_default().insert(name.clone());
                        }
                    }
                }
            }
            Ok(named)
        })?;
        let mut lease = collection.reader(ReaderRole::Cloud)?;
        let (snapshot, hold) = lease.capture_cloud(collection, named.keys().cloned().collect())?;
        #[cfg(feature = "test-hooks")]
        if let Some(hook) = self.take_cloud_capture_hook_for_test() {
            hook(self);
        }
        let (mut runs, facts, blocked, publishing, ready, binding) = self.with_scan(&guard, collection, |s| {
            // Recover every retained Pending overlap, including a capture
            // whose fallible handoff never registered in this runtime.
            Self::rebuild_cloud_associations_locked(s, collection)?;
            let mut facts = BTreeMap::new();
            let mut blocked = s.cloud.incoming_held.clone();
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for name in requested {
                if store.incoming_blocks_name(guard.binding, name)? {
                    blocked.insert(name.clone());
                }
                if let Some(e) = store.entity_header(guard.binding, name)? {
                    facts.insert(name.clone(), e.facts);
                }
            }
            let ready = Self::gate_locked(s).is_ok()
                && !store.has_rekey_intents(guard.binding)?
                && s.cloud.incoming_seq <= snapshot.seq().0
                && s.last_session == snapshot.active_session().map(|session| session.0);
            Ok((
                s.cloud.detached.clone(),
                facts,
                blocked,
                Self::publication_allows_send_locked(s),
                ready,
                s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone(),
            ))
        })?;
        if !ready {
            return Ok(empty());
        }
        runs.push(hold.run);
        // P7's entire WAL wait is bounded by one second and runs without the
        // writer, runtime state or tracking lock. Only a real matching receipt
        // permits any flight or record to leave this call.
        let receipt =
            match collection.wait_wal_covered(snapshot.wal_identity(), snapshot.seq(), Duration::from_secs(1))? {
                WalCoverage::Covered(receipt)
                    if receipt.identity() == snapshot.wal_identity()
                        && receipt.through() >= snapshot.seq()
                        && snapshot.generation() == guard.generation
                        && snapshot.wal_identity().native_file() == guard.file.pair() =>
                {
                    receipt
                }
                _ => return Ok(empty()),
            };
        let mut deferred = BTreeSet::new();
        // The first pass retains descriptors only. Every full image and codec
        // buffer dies before the next requested name. The pinned snapshot,
        // held runs and WAL receipt cover both passes and every flight commit.
        type PreparedCandidate = (Flight, Vec<u8>, usize, Vec<String>, Option<Vec<u8>>);
        let prepare = |name: &String,
                       e: &EntityFacts,
                       deferred: &mut BTreeSet<String>|
         -> Result<Option<PreparedCandidate>, RuntimeError> {
            let (payload, image_seq, held) = 'image: {
                if blocked.contains(name)
                    || (e.observer_open == guard.open
                        && e.observed_generation == snapshot.generation().0
                        && e.observed_seq > snapshot.seq().0)
                {
                    deferred.insert(name.clone());
                    return Ok(None);
                }
                if (e.acked_tombstone() && e.kind != codec::Kind::MediaName)
                    || e.mark.is_none()
                    || (!publishing
                        && !matches!(
                            e.kind,
                            codec::Kind::Note
                                | codec::Kind::Card
                                | codec::Kind::Revlog
                                | codec::Kind::Deck
                                | codec::Kind::Config
                                | codec::Kind::MediaName
                        ))
                {
                    return Ok(None);
                }
                if e.kind == codec::Kind::MediaName {
                    let payload = self.with_scan(&guard, collection, |s| {
                        let store = s.store.as_ref().ok_or(Error::Database)?;
                        store
                            .entity_header(guard.binding, name)?
                            .ok_or(Error::InvalidData)?
                            .local_payload(&store.connection, guard.binding)
                            .map_err(RuntimeError::from)
                    })?;
                    break 'image (payload, snapshot.seq().0, false);
                }
                let view = super::super::held_view::HeldView::new(&*snapshot, &runs, snapshot.seq());
                let table = row_payload::table(e.kind).ok_or(Error::InvalidData)?;
                let key = super::super::workflow_store::row_key(e.kind, &e.key)?;
                let Some(row) = view.row(table, &key)? else {
                    let (image_seq, held) = view.image();
                    if !held && e.tombstone_at.is_some() && !e.acked_tombstone() {
                        let payload = self.with_scan(&guard, collection, |s| {
                            let store = s.store.as_ref().ok_or(Error::Database)?;
                            store
                                .entity_header(guard.binding, name)?
                                .ok_or(Error::InvalidData)?
                                .local_payload(&store.connection, guard.binding)
                                .map_err(RuntimeError::from)
                        })?;
                        break 'image (payload, image_seq, false);
                    } else {
                        // Defer held creates. Deleted rows use the captured image.
                        deferred.insert(name.clone());
                    }
                    return Ok(None);
                };
                let payload = self.project_send_row(&guard, collection, &view, e, publishing, &row);
                let payload = match payload {
                    Ok(payload) => payload,
                    Err(Error::InvalidData) => {
                        deferred.insert(name.clone());
                        return Ok(None);
                    }
                    Err(error) => return Err(error.into()),
                };
                let (image_seq, held) = view.image();
                if payload.row_digest().map_err(|_| Error::InvalidData)? == e.acked_row_digest && e.server_erev > 0 {
                    if held {
                        deferred.insert(name.clone());
                    }
                    return Ok(None);
                }
                break 'image (payload, image_seq, held);
            };
            let erev = e.server_erev.checked_add(1).ok_or(Error::InvalidData)?;
            let tombstone_at = if e.kind == codec::Kind::MediaName {
                self.with_scan(&guard, collection, |s| {
                    Ok(s.store.as_ref().ok_or(Error::Database)?.media_ledger(guard.binding)?)
                })?
                .names
                .get(std::str::from_utf8(&e.key).map_err(|_| Error::InvalidData)?)
                .and_then(|n| n.tombstone_at)
            } else if !held {
                e.tombstone_at
            } else {
                None
            };
            let row_tombstone = tombstone_at.is_some() && e.kind != codec::Kind::MediaName;
            let payload = if tombstone_at.is_some() && !row_tombstone {
                self.with_scan(&guard, collection, |s| {
                    Ok(s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .entity_facts(guard.binding, name)?
                        .ok_or(Error::InvalidData)?
                        .acked_payload
                        .ok_or(Error::InvalidData)?)
                })?
            } else {
                payload
            };
            let row_digest = if row_tombstone {
                codec::sha256(&[])
            } else {
                payload.row_digest().map_err(|_| Error::InvalidData)?
            };
            let encoded_payload = if row_tombstone {
                Vec::new()
            } else {
                payload.encode().map_err(|_| Error::InvalidData)?
            };
            let entity = codec::Entity::build(
                codec::EntityFields {
                    kind: e.kind,
                    key: e.key.clone(),
                    origin: e.origin,
                    erev,
                    generation: None,
                    tombstone: tombstone_at.is_some(),
                    deleted_at_ms: tombstone_at.unwrap_or(0),
                    writer: binding.id.0,
                    base_sha256: if e.server_erev > 0 {
                        Some(e.acked_payload_digest)
                    } else {
                        None
                    },
                },
                &encoded_payload,
            )
            .map_err(|_| Error::InvalidData)?;
            let asset = entity.payload.is_none().then_some(encoded_payload);
            let envelope = entity.encode().map_err(|_| Error::InvalidData)?;
            let size = envelope.len().checked_add(2048).ok_or(Error::InvalidData)?;
            let references = if row_tombstone {
                Vec::new()
            } else {
                self.with_scan(&guard, collection, |s| {
                    Ok(s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .collection_references(guard.binding, &payload)?)
                })?
            };
            let flight = Flight {
                local_rev: e.local_rev,
                payload_digest: entity.payload_sha256,
                row_digest,
                payload: payload.into(),
                erev,
                image_seq,
                held,
                tombstone: tombstone_at.is_some(),
                tombstone_envelope_digest: row_tombstone.then(|| codec::sha256(&envelope)),
            };
            Ok(Some((flight, envelope, size, references, asset)))
        };
        let mut descriptors = BTreeMap::new();
        for (name, e) in &facts {
            if let Some((flight, envelope, size, references, asset)) = prepare(name, e, &mut deferred)? {
                // No payload, encoded asset or full image survives this loop.
                let _ = (flight, envelope, asset);
                descriptors.insert(name.clone(), (size, references));
            }
        }
        let selected = self.with_scan(&guard, collection, |s| {
            Self::caller_locked(s, instance)?;
            if Self::gate_locked(s).is_err() || s.cloud.incoming_seq > snapshot.seq().0 {
                return Ok(Vec::new());
            }
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut waiting = BTreeSet::new();
            loop {
                let mut changed = false;
                for (name, (_, references)) in &descriptors {
                    if deferred.contains(name) {
                        continue;
                    }
                    for reference in references {
                        if deferred.contains(reference)
                            || s.cloud.deferred.contains(reference)
                            || s.cloud.incoming_held.contains(reference)
                            || store.incoming_blocks_name(guard.binding, reference)?
                        {
                            changed |= deferred.insert(name.clone());
                            break;
                        }
                        if waiting.contains(reference)
                            || !store.entity_header(guard.binding, reference)?.is_some_and(|h| {
                                let e = h.facts;
                                e.server_erev > 0
                                    && e.acked_payload.is_some()
                                    && !(matches!(e.kind, codec::Kind::Note | codec::Kind::Card) && e.acked_tombstone())
                            })
                        {
                            changed |= waiting.insert(name.clone());
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            let mut selected = Vec::new();
            let mut bytes = 0usize;
            for (name, (size, _)) in &descriptors {
                if deferred.contains(name)
                    || waiting.contains(name)
                    || store.incoming_blocks_name(guard.binding, name)?
                {
                    continue;
                }
                if selected.len() >= s.actions.record_limit {
                    break;
                }
                if *size > s.actions.byte_limit {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                if bytes.checked_add(*size).ok_or(Error::InvalidData)? > s.actions.byte_limit {
                    break;
                }
                bytes += size;
                selected.push(name.clone());
            }
            Ok(selected)
        })?;
        let mut saves = Vec::new();
        let mut flights = BTreeMap::new();
        let mut uploads = Vec::new();
        let mut images = Vec::new();
        // Incremental staging and persistence: one full image at a time,
        // independently of record count, asset count and total asset bytes.
        for name in selected {
            let captured = facts.get(&name).ok_or(Error::InvalidData)?;
            let Some((flight, envelope, _, _, asset)) = prepare(&name, captured, &mut deferred)? else {
                continue;
            };
            let upload = asset
                .map(|asset| super::super::payload_asset::upload(&self.directory, guard.binding, &asset))
                .transpose()?;
            // The asset Vec has been released before persistence encodes the
            // flight into its binding-scoped blob. Files alone cross records.
            let handed = HandedSave {
                origin: captured.origin,
                local_rev: flight.local_rev,
                erev: flight.erev,
                payload_digest: flight.payload_digest,
                tombstone: flight.tombstone,
                envelope_digest: codec::sha256(&envelope),
                image_seq: flight.image_seq,
                held: flight.held,
            };
            let image = (
                name.clone(),
                flight.erev,
                flight.payload_digest,
                flight.image_seq,
                flight.held,
            );
            let identity = flight.identity();
            let committed = self.with_scan(&guard, collection, |s| {
                Self::caller_locked(s, instance)?;
                if Self::gate_locked(s).is_err() || s.cloud.incoming_seq > snapshot.seq().0 {
                    return Ok(false);
                }
                let store = s.store.as_mut().ok_or(Error::Database)?;
                let e = store
                    .entity_header(guard.binding, &name)?
                    .ok_or(Error::InvalidData)?
                    .facts;
                if e.server_erev != captured.server_erev
                    || e.acked_payload_digest != captured.acked_payload_digest
                    || e.acked_system_fields != captured.acked_system_fields
                    || store.incoming_blocks_name(guard.binding, &name)?
                {
                    return Ok(false);
                }
                if store.persist_flight(guard.binding, &name, flight)? {
                    s.cloud.remember_flight(guard.binding, name.clone(), identity);
                }
                Ok(true)
            })?;
            if !committed {
                continue;
            }
            let asset_path = upload.as_ref().map(|p| p.to_string_lossy().into_owned());
            if let Some(upload) = upload {
                uploads.push(upload);
            }
            flights.insert(name.clone(), handed);
            images.push((image, identity));
            saves.push(SaveInput {
                name,
                envelope,
                system_fields: (captured.server_erev > 0).then(|| captured.acked_system_fields.clone()),
                asset_path,
            });
        }
        #[cfg(feature = "test-hooks")]
        let smoke_images = saves
            .iter()
            .filter_map(|input| {
                let e = facts.get(&input.name)?;
                matches!(e.kind, codec::Kind::Card | codec::Kind::Revlog).then_some(e)
            })
            .map(|e| self.smoke_project_image(&guard, collection, &*snapshot, &runs, snapshot.seq(), e))
            .collect::<Result<Vec<_>, _>>();
        self.with_scan(&guard, collection, |s| {
            Self::caller_locked(s, instance)?;
            if Self::gate_locked(s).is_err() || s.cloud.incoming_seq > snapshot.seq().0 {
                return Ok(empty());
            }
            // Recheck conditional authority after all incremental commits.
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for input in &saves {
                let e = store
                    .entity_header(guard.binding, &input.name)?
                    .ok_or(Error::InvalidData)?
                    .facts;
                let captured = facts.get(&input.name).ok_or(Error::InvalidData)?;
                if e.server_erev != captured.server_erev
                    || e.acked_payload_digest != captured.acked_payload_digest
                    || e.acked_system_fields != captured.acked_system_fields
                    || store.incoming_blocks_name(guard.binding, &input.name)?
                {
                    return Ok(empty());
                }
            }
            for ((name, erev, digest, seq, held), _) in &images {
                s.cloud
                    .handed_images
                    .entry((name.clone(), *erev, *digest))
                    .and_modify(|v| *v = (*v).max(*seq))
                    .or_insert(*seq);
                if *held {
                    s.policy.held_images.insert(name.clone());
                } else {
                    s.policy.held_images.remove(name);
                }
            }
            if s.last_capture
                .as_ref()
                .is_none_or(|proof| proof.observed_seq <= snapshot.seq())
            {
                s.cloud.deferred.extend(deferred);
            }
            let pending = Self::sendables_locked(s)?;
            for ((name, ..), identity) in images {
                s.cloud.mark_flight_handed(guard.binding, &name, identity);
            }
            s.actions.update(UpdateKind::Changes, pending, BTreeSet::new());
            let _covered = &receipt;
            s.actions.batch += 1;
            let id = s.actions.batch;
            if !saves.is_empty() {
                s.trace.push(TraceEvent::CollectionBatchBuilt {
                    instance: instance.into(),
                    names: saves.iter().map(|v| v.name.clone()).collect(),
                });
                s.actions.observations.push(Observation::BatchBuilt {
                    batch_id: id,
                    names: saves.iter().map(|e| e.name.clone()).collect(),
                });
                s.actions.request_flights.insert((instance.to_owned(), id), flights);
                if !uploads.is_empty() {
                    let sends: BTreeSet<_> = s
                        .actions
                        .calls
                        .iter()
                        .filter(|call| !call.returned)
                        .map(|call| call.id)
                        .collect();
                    if !sends.is_empty() {
                        s.actions.payload_upload_sends.insert((instance.to_owned(), id), sends);
                    }
                    s.actions.payload_uploads.insert((instance.to_owned(), id), uploads);
                }
                s.policy.flights.insert(
                    (instance.to_owned(), id),
                    saves.iter().map(|v| v.name.clone()).collect(),
                );
            }
            #[cfg(feature = "test-hooks")]
            {
                // The proof uses the same pinned snapshot/held runs as this
                // batch, then checks the durable flights and exact envelopes.
                let proof = smoke_images
                    .and_then(|images| {
                        self.smoke_built_batch_locked(
                            s,
                            instance,
                            id,
                            (
                                snapshot.generation().0,
                                snapshot.seq().0,
                                snapshot.active_session().map(|v| v.0),
                            ),
                            &saves,
                            images,
                        )
                    })
                    .ok();
                if s.actions.smoke_batches.len() == 256
                    && let Some(key) = s.actions.smoke_batches.keys().next().cloned()
                {
                    s.actions.smoke_batches.remove(&key);
                }
                s.actions.smoke_batches.insert((instance.to_owned(), id), proof);
            }
            Ok(BuiltBatch { id, saves })
        })
    }
    fn decode_saved_record_locked(
        s: &State,
        instance: &str,
        request: u64,
        record: &Saved,
    ) -> Result<(codec::Received, FlightImage), RuntimeError> {
        let binding = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let handed = s
            .actions
            .request_flights
            .get(&(instance.to_owned(), request))
            .and_then(|records| records.get(&record.name))
            .ok_or(Error::InvalidData)?;
        if record.system_fields.is_empty() || codec::sha256(&record.envelope) != handed.envelope_digest {
            return Err(Error::InvalidData.into());
        }
        let header = store
            .entity_header(binding.id, &record.name)?
            .ok_or(Error::InvalidData)?;
        let facts = &header.facts;
        let index = facts
            .in_flight
            .iter()
            .position(|flight| {
                flight.local_rev == handed.local_rev
                    && flight.erev == handed.erev
                    && flight.payload_digest == handed.payload_digest
                    && flight.tombstone == handed.tombstone
            })
            .ok_or(Error::InvalidData)?;
        // The durable flight owns the old image even when a newer row has
        // been captured. No returned SDK file participates in this proof.
        let flight = &facts.in_flight[index];
        let bytes = if flight.tombstone && facts.kind != codec::Kind::MediaName {
            Vec::new()
        } else {
            header.flight_bytes(&store.connection, binding.id, index)?
        };
        let received = codec::receive(
            &binding.facts.record_key.0,
            &record.name,
            &record.envelope,
            codec::AssetInput::Bytes(&bytes),
        )
        .map_err(|_| Error::InvalidData)?;
        if received.entity.fields.writer != binding.id.0
            || received.entity.fields.origin != handed.origin
            || received.entity.fields.erev != handed.erev
            || received.entity.payload_sha256 != handed.payload_digest
            || received.entity.fields.generation.is_some()
            || received.entity.fields.tombstone != handed.tombstone
        {
            return Err(Error::InvalidData.into());
        }
        // Identical bytes can be handed again later. The request, not the
        // durable digest alone, says which image this callback accepted.
        Ok((
            received,
            FlightImage {
                seq: handed.image_seq,
                held: handed.held,
            },
        ))
    }
    /// Resolve Engine's retained overlap against the current binding, never
    /// cached record names or the rest of a step's touched rows. No journals
    /// or collection images are read here; callers hold tracking access.
    fn rebuild_cloud_associations_locked(s: &mut State, collection: &Collection) -> Result<(), RuntimeError> {
        if !collection.is_current() || s.cloud.generation != Some(collection.generation()) {
            return Err(RuntimeError::StaleIncarnation);
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let mut associations = BTreeMap::new();
        for (id, keys) in collection.cloud_pending_keys()? {
            let mut named = BTreeMap::new();
            for (table, key) in keys {
                let kind = if matches!(table, Table::Fields | Table::Templates) {
                    Some(codec::Kind::Notetype)
                } else {
                    row_payload::KINDS
                        .into_iter()
                        .find(|kind| row_payload::table(*kind) == Some(table))
                };
                let mut name = None;
                if let Some(kind) = kind {
                    let entity_key = match &key {
                        RowKey::Id(id) => Some(codec::id_key(*id).to_vec()),
                        RowKey::Pair(id, _) if matches!(table, Table::Fields | Table::Templates) => {
                            Some(codec::id_key(*id).to_vec())
                        }
                        RowKey::Text(text) if table == Table::Tags => Some(
                            meguri_text::fold::folded(std::str::from_utf8(text).map_err(|_| Error::InvalidData)?)
                                .into_bytes(),
                        ),
                        RowKey::Text(text) => Some(text.clone()),
                        _ => None,
                    };
                    if let Some(entity_key) = entity_key
                        && let Some(header) = store.entity_key_header(binding, kind, &entity_key)?
                    {
                        if matches!(table, Table::Fields | Table::Templates) {
                            let entity = store
                                .entity_facts(binding, &header.facts.name)?
                                .ok_or(Error::InvalidData)?;
                            if row_payload::payload_rows(&entity.payload)?.iter().any(|(t, k, _)| {
                                *t == table && super::super::held_view::compare(table, &key, k).is_eq()
                            }) {
                                name = Some(entity.name);
                            }
                        } else {
                            name = Some(header.facts.name);
                        }
                    }
                }
                named.insert((table, key), name);
            }
            associations.insert(id, named);
        }
        s.cloud.flagged = associations;
        Ok(())
    }
    /// Called only after a durable ACK/settle, while tracking access is held.
    /// Each named key must independently be acked through this step, or clean.
    pub(super) fn settle_cloud_uploaded_locked(s: &mut State, collection: &Collection) -> Result<(), RuntimeError> {
        Self::rebuild_cloud_associations_locked(s, collection)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let mut uploaded = Vec::new();
        for (id, keys) in &s.cloud.flagged {
            let mut complete = !keys.is_empty();
            for name in keys.values() {
                let Some(name) = name else {
                    complete = false;
                    continue;
                };
                let e = store.entity_header(b, name)?;
                let acked = s.cloud.acked_images.get(name).is_some_and(|seq| *seq >= id.0);
                let clean = e.is_some_and(|h| h.facts.server_erev > 0 && h.facts.mark.is_none());
                complete &= acked || clean;
            }
            if complete {
                uploaded.push(*id);
            }
        }
        collection.mark_cloud_uploaded(&uploaded)?;
        for id in uploaded {
            s.cloud.flagged.remove(&id);
        }
        Ok(())
    }
    /// Shared durable ACK and opening-local upload bookkeeping. The caller
    /// independently proves either an exact Saved result or a historical flight.
    fn acknowledge_sent_locked(
        s: &mut State,
        collection: &Collection,
        saved: &Saved,
        received: codec::Received,
        now_ms: i64,
        force: &mut BTreeSet<String>,
        references: Option<&mut media::cleanup::SavedReferences>,
    ) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let image_key = (
            saved.name.clone(),
            received.entity.fields.erev,
            received.entity.payload_sha256,
        );
        let image = s.cloud.handed_images.get(&image_key).copied();
        let store = s.store.as_mut().ok_or(Error::Database)?;
        let pending = if let Some(references) = references {
            store.acknowledge_with_references(b, saved, received, now_ms, Some(references))?
        } else {
            store.acknowledge(b, saved, received, now_ms)?
        };
        if pending {
            force.insert(saved.name.clone());
        }
        if let Some(image) = image {
            s.cloud
                .acked_images
                .entry(saved.name.clone())
                .and_modify(|seq| *seq = (*seq).max(image))
                .or_insert(image);
        }
        s.cloud
            .handed_images
            .retain(|(name, erev, _), _| name != &image_key.0 || *erev > image_key.1);
        s.cloud.retire_flights(&saved.name, image_key.1);
        Self::settle_cloud_uploaded_locked(s, collection)?;
        Ok(())
    }
    pub fn sent_results(
        &self,
        instance: &str,
        collection: &Collection,
        request_id: u64,
        saved: &[Saved],
        failed: &[(String, SendError)],
        batch_count: usize,
    ) -> Result<(), RuntimeError> {
        self.sent_results_with_conflicts(instance, collection, request_id, saved, &[], failed, batch_count)
            .map(|_| ())
    }

    /// One original SDK receipt, validated in full before any record can ACK.
    /// Returns actual Saved ACKs plus own historical-flight conflict ACKs;
    /// inbox intake and refusals contribute zero.
    #[allow(clippy::too_many_arguments)]
    pub fn sent_results_with_conflicts(
        &self,
        instance: &str,
        collection: &Collection,
        request_id: u64,
        saved: &[Saved],
        conflicts: &[Saved],
        failed: &[(String, SendError)],
        batch_count: usize,
    ) -> Result<u64, RuntimeError> {
        #[cfg(feature = "test-hooks")]
        super::workflow_store::set_saved_ack_body_reads(None);
        let _ = self.adapter_now();
        let at_ms = self.adapter_now()?;
        let evidence = collection.open_evidence()?;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if !conflicts.is_empty() {
            Self::sdk_locked(&s, instance)?;
        }
        // Receipt authority comes from the current caller, file, handler and
        // persisted flight below. A scan may block new sends after this save landed.
        if s.bound
            .as_ref()
            .is_none_or(|b| b.evidence.generation != evidence.generation || b.file.pair() != evidence.native_file)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if s.actions.handler != Some(request_id) {
            return Err(Error::InvalidData.into());
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        // A missing-record result is authority only for this original handed
        // request, never a caller-supplied record name or a later incarnation.
        for (name, error) in failed {
            if *error == SendError::UnknownItem
                && !s
                    .policy
                    .flights
                    .get(&(instance.to_owned(), request_id))
                    .is_some_and(|names| names.contains(name))
            {
                return Err(Error::InvalidData.into());
            }
        }
        let mut force = BTreeSet::new();
        let mut saved_count = 0;
        let mut acknowledged = Vec::new();
        with_tracking_access(|| -> Result<(), RuntimeError> {
            // Validate the whole receipt before committing any ACK, but retain
            // no decoded payload across records. Malformed tails cannot partly
            // apply a receipt, and a large batch cannot retain all ACK images.
            let mut names = BTreeSet::new();
            for record in saved {
                if !names.insert(&record.name) {
                    return Err(Error::InvalidData.into());
                }
                Self::decode_saved_record_locked(&s, instance, request_id, record)?;
            }
            for server in conflicts {
                if !names.insert(&server.name) {
                    return Err(Error::InvalidData.into());
                }
                Self::decode_save_conflict_locked(&s, instance, request_id, server)?;
            }
            for (name, _) in failed {
                if !names.insert(name)
                    || !s
                        .actions
                        .request_flights
                        .get(&(instance.to_owned(), request_id))
                        .is_some_and(|records| records.contains_key(name))
                {
                    return Err(Error::InvalidData.into());
                }
            }
            #[cfg(feature = "test-hooks")]
            let saved_ack_body_reads_before = super::workflow_store::body_reads();
            let mut references = media::cleanup::SavedReferences::default();
            for saved in saved {
                let (received, image) = Self::decode_saved_record_locked(&s, instance, request_id, saved)?;
                Self::acknowledge_sent_locked(
                    &mut s,
                    collection,
                    saved,
                    received,
                    at_ms,
                    &mut force,
                    Some(&mut references),
                )?;
                if s.actions.exchange.is_some() {
                    acknowledged.push((&saved.name, image));
                }
                saved_count += 1;
            }
            #[cfg(feature = "test-hooks")]
            super::workflow_store::set_saved_ack_body_reads(Some(
                super::workflow_store::body_reads().saturating_sub(saved_ack_body_reads_before),
            ));
            // Historical-flight ACKs and inbox intake retain their standalone
            // behavior; no reference snapshot crosses into those mutating paths.
            drop(references);
            for server in conflicts {
                if let Some((received, image)) = Self::decode_save_conflict_locked(&s, instance, request_id, server)? {
                    Self::acknowledge_sent_locked(&mut s, collection, server, received, at_ms, &mut force, None)?;
                    if let Some(image) = image {
                        acknowledged.push((&server.name, image));
                        #[cfg(feature = "test-hooks")]
                        if s.actions.smoke_acks.enabled {
                            s.actions.smoke_acks.conflicts.push(SmokeHistoricalAck {
                                sdk: instance.to_owned(),
                                name: server.name.clone(),
                                envelope_digest: codec::sha256(&server.envelope),
                                image_seq: image.seq,
                                held: image.held,
                            });
                        }
                    }
                    saved_count += 1;
                } else {
                    let key = s
                        .bound
                        .as_ref()
                        .ok_or(RuntimeError::StaleIncarnation)?
                        .binding
                        .facts
                        .record_key
                        .0;
                    let (mut entries, refused) =
                        super::super::incoming::classify(&key, std::slice::from_ref(server), at_ms);
                    for entry in &mut entries {
                        entry.source = super::super::InboxSource::SaveConflict;
                    }
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .receive_asset_save_conflict(b, &entries, &refused)?;
                    force.insert(server.name.clone());
                }
            }
            Ok(())
        })?;
        let mut absent_revisions = BTreeMap::new();
        let missing_result = with_tracking_access(|| -> Result<(), RuntimeError> {
            for (name, error) in failed {
                if *error == SendError::UnknownItem {
                    let handed = s
                        .actions
                        .request_flights
                        .get(&(instance.to_owned(), request_id))
                        .and_then(|records| records.get(name))
                        .copied()
                        .ok_or(Error::InvalidData)?;
                    let disposition = s
                        .store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .record_unknown_item(b, name, &handed, at_ms)?;
                    match disposition {
                        MissingSaveDisposition::Outstanding => {}
                        MissingSaveDisposition::TerminalAbsence { local_rev } => {
                            absent_revisions.insert(name.clone(), local_rev);
                        }
                    }
                }
            }
            Ok(())
        });
        if let Err(e) = missing_result {
            Self::invalidate_locked(&mut s);
            return Err(e);
        }
        if failed.iter().any(|(_, e)| *e == SendError::LimitExceeded) && batch_count > 1 {
            s.actions.record_limit = (s.actions.record_limit / 2).max(1);
            s.actions.byte_limit = (s.actions.byte_limit / 2).max(1);
        }
        if let Some(names) = s.policy.flights.get_mut(&(instance.to_owned(), request_id)) {
            for name in saved
                .iter()
                .chain(conflicts)
                .map(|v| &v.name)
                .chain(failed.iter().map(|(n, _)| n))
            {
                names.remove(name);
            }
        }
        if let Some(records) = s.actions.request_flights.get_mut(&(instance.to_owned(), request_id)) {
            for name in saved
                .iter()
                .chain(conflicts)
                .map(|r| &r.name)
                .chain(failed.iter().map(|(n, _)| n))
            {
                records.remove(name);
            }
        }
        s.actions.request_flights.retain(|_, records| !records.is_empty());
        for (name, error) in failed {
            if *error == SendError::Other || (*error == SendError::LimitExceeded && batch_count <= 1) {
                s.policy.parked.insert(name.clone());
            }
            if *error == SendError::BatchRequestFailed || (*error == SendError::LimitExceeded && batch_count > 1) {
                force.insert(name.clone());
            }
        }
        let terminal_names = absent_revisions
            .into_iter()
            .filter(|(name, revision)| {
                s.actions
                    .selected_revs
                    .get(name)
                    .is_some_and(|selected| selected <= revision)
            })
            .map(|(name, _)| name)
            .collect::<Vec<_>>();
        if let Some((_, exchange)) = &mut s.actions.exchange {
            for (name, image) in acknowledged {
                exchange.record_ack(name, image);
            }
            // Storage proved absence for this handed revision. Keep it out of
            // the ACK set and leave every other selected revision outstanding.
            for name in terminal_names {
                exchange.outstanding_names.remove(&name);
                if !exchange.acked_names.contains(&name) && !exchange.dropped_names.contains(&name) {
                    exchange.absent_names.insert(name);
                }
            }
        }
        s.actions
            .saved_callbacks
            .insert(request_id, saved.iter().map(|s| s.name.clone()).collect());
        // Ordinary per-record failures park their records without discarding
        // valid root proof or ACKs. Zone loss still requires the native zone
        // lifecycle path. All failed records remain durably pending.
        if !saved.is_empty() || !conflicts.is_empty() || !failed.is_empty() {
            self.emit_pending_locked(&mut s, UpdateKind::Changes, force)?;
        }
        let lost = failed.iter().any(|(_, error)| *error == SendError::ZoneMissing);
        let zone = s
            .bound
            .as_ref()
            .map(|b| super::super::adapter::collection_zone(b.binding.facts.epoch));
        let native = s
            .adapter
            .sdk
            .as_ref()
            .is_some_and(|sdk| sdk.id == instance && !sdk.stopping);
        drop(s);
        if lost && native {
            self.adapter_observe_zone_losses(
                instance,
                &[super::super::adapter::ZoneLoss {
                    zone: zone.ok_or(RuntimeError::StaleIncarnation)?,
                    cause: super::super::adapter::ZoneLossCause::Deleted,
                }],
            )?;
        }
        Ok(saved_count)
    }
    pub fn sync_now(&self, instance: &str, collection: &Collection) -> Result<u64, RuntimeError> {
        self.sync_now_with_work(instance, collection, &mut PolicyWork::default())
    }
    pub fn sync_now_with_work(
        &self,
        instance: &str,
        collection: &Collection,
        work: &mut PolicyWork,
    ) -> Result<u64, RuntimeError> {
        let _ = self.adapter_now();
        self.poll_review_with_work(instance, work)?;
        let media = self.media_service_with_work(collection, work)?;
        self.consume_media_feed_with_work(collection, &media, work)?;
        if self.active_review().is_some() {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        Self::capture_gate_locked(&self.state())?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let boundary = snapshot.seq().0;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        Self::capture_gate_locked(&s)?;
        if !collection.is_current()
            || s.bound
                .as_ref()
                .is_none_or(|b| b.evidence.generation != snapshot.generation())
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if let Some((id, _)) = &s.actions.exchange {
            return Ok(*id);
        }
        self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
        let next_durable = s
            .adapter
            .durable
            .exchanges
            .last_key_value()
            .map(|(id, _)| id.checked_add(1).ok_or(Error::InvalidData))
            .transpose()?
            .unwrap_or(1);
        let next_closed = s
            .adapter
            .closed_exchanges
            .last_key_value()
            .map(|(id, _)| id.checked_add(1).ok_or(Error::InvalidData))
            .transpose()?
            .unwrap_or(1);
        let id = s.actions.next.max(next_durable).max(next_closed);
        // Pending updates remain gated by root proof. Capture the durable local
        // selection independently so the first fetch cannot lose preproof edits.
        let bound = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let b = bound.binding.id;
        let open = bound.open_counter;
        let names = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.policy_pending_names(b))?;
        let revs = with_tracking_access(|| -> Result<BTreeMap<String, u64>, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut revs = BTreeMap::new();
            for name in &names {
                let Some(header) = store.entity_header(b, name)? else {
                    continue;
                };
                let e = header.facts;
                if e.observer_open == open
                    && e.observed_generation == snapshot.generation().0
                    && e.observed_seq > boundary
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                revs.insert(name.clone(), e.local_rev);
            }
            Ok(revs)
        })?;
        let names = revs.keys().cloned().collect::<BTreeSet<_>>();
        s.adapter.exchange_owner = Some((id, instance.into(), s.incarnation));
        s.actions.selected_revs = revs;
        s.actions.exchange_phase = Some(FetchPhase::BeforeSend);
        s.actions.exchange = Some((
            id,
            ExchangeResult {
                boundary_seq: boundary,
                selected_names: names.clone(),
                acked_names: BTreeSet::new(),
                held_back_names: BTreeSet::new(),
                outstanding_names: names,
                blocked_names: BTreeSet::new(),
                dropped_names: BTreeSet::new(),
                absent_names: BTreeSet::new(),
            },
        ));
        Ok(id)
    }
    pub fn exchange_fetch_due(&self) -> Option<FetchPhase> {
        self.state().actions.exchange_phase
    }
    pub fn exchange_fetch_completed(
        &self,
        instance: &str,
        phase: FetchPhase,
        seen: &BTreeSet<String>,
    ) -> Result<(), RuntimeError> {
        // Coverage needs only the validated tracking header's server revision
        // and name; keep payload bodies out of this paged membership check.
        for kind in row_payload::KINDS {
            let mut after = None;
            loop {
                let page = {
                    let s = self.state();
                    Self::caller_locked(&s, instance)?;
                    if s.actions.exchange_phase != Some(phase) {
                        return Err(Error::InvalidData.into());
                    }
                    let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
                    with_tracking_access(|| {
                        s.store
                            .as_ref()
                            .ok_or(Error::Database)?
                            .entity_keys_page(binding, kind, after.as_deref())
                    })?
                };
                if page.is_empty() {
                    break;
                }
                for key in page {
                    let header = {
                        let s = self.state();
                        Self::caller_locked(&s, instance)?;
                        if s.actions.exchange_phase != Some(phase) {
                            return Err(Error::InvalidData.into());
                        }
                        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
                        with_tracking_access(|| {
                            s.store
                                .as_ref()
                                .ok_or(Error::Database)?
                                .entity_key_header(binding, kind, &key)
                        })?
                        .ok_or(Error::InvalidData)?
                    };
                    if header.facts.server_erev > 0 && !seen.contains(&header.facts.name) {
                        return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                    }
                    after = Some(key);
                }
            }
        }
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if s.actions.exchange_phase != Some(phase) {
            return Err(Error::InvalidData.into());
        }
        s.actions.exchange_phase = None;
        if phase == FetchPhase::AfterSend
            && let Some((id, r)) = s.actions.exchange.take()
        {
            s.actions.exchanges.insert(id, r);
        }
        Ok(())
    }
    pub fn exchange_sends_finished(&self, instance: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        if s.actions.exchange.is_some() {
            s.actions.exchange_phase = Some(FetchPhase::AfterSend);
        }
        Ok(())
    }
    pub fn wants_send(&self) -> bool {
        let s = self.state();
        s.actions.automatic || s.actions.exchange.is_some() || Self::publication_automatic_locked(&s)
    }
    pub fn start_send_call(&self, instance: &str) -> Result<Option<SendCall>, RuntimeError> {
        self.start_send_call_mode(instance, false, &mut PolicyWork::default())
    }
    pub fn start_manual_send_call(&self, instance: &str) -> Result<Option<SendCall>, RuntimeError> {
        self.start_manual_send_call_with_work(instance, &mut PolicyWork::default())
    }
    pub(crate) fn start_manual_send_call_with_work(
        &self,
        instance: &str,
        work: &mut PolicyWork,
    ) -> Result<Option<SendCall>, RuntimeError> {
        self.start_send_call_mode(instance, true, work)
    }
    fn start_send_call_mode(
        &self,
        instance: &str,
        manual: bool,
        work: &mut PolicyWork,
    ) -> Result<Option<SendCall>, RuntimeError> {
        let mut s = self.state();
        work.check()?;
        Self::caller_locked(&s, instance)?;
        if Self::gate_locked(&s).is_err()
            || s.actions.exchange_phase.is_some()
            || s.actions.handler.is_some()
            || (!manual
                && !s.actions.automatic
                && s.actions.exchange.is_none()
                && !Self::publication_automatic_locked(&s))
        {
            return Ok(None);
        }
        work.effect()?;
        s.adapter.sdk_retrying = false;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::PolicySendRetryCleared);
        let exchange = s.actions.exchange.as_ref().map(|(id, _)| *id);
        if let Some(id) = exchange
            && s.actions.calls.iter().filter(|c| c.exchange == Some(id)).count() >= 8
        {
            return Ok(None);
        }
        let call = SendCall {
            id: s.actions.calls.len() as u64 + 1,
            exchange,
            automatic: !manual && exchange.is_none(),
            request_ids: Vec::new(),
            returned: false,
        };
        s.actions.calls.push(call.clone());
        Ok(Some(call))
    }
    #[cfg(feature = "test-hooks")]
    pub fn sdk_retrying_for_testing(&self) -> bool {
        self.state().adapter.sdk_retrying
    }
    pub fn send_call_request(&self, instance: &str, id: u64, request: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let c = s
            .actions
            .calls
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(Error::InvalidData)?;
        c.request_ids.push(request);
        let key = (instance.to_owned(), request);
        if s.actions.payload_uploads.contains_key(&key) {
            s.actions.payload_upload_sends.entry(key).or_default().insert(id);
        }
        Ok(())
    }
    pub fn finish_send_call(&self, instance: &str, id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let call = s
            .actions
            .calls
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(Error::InvalidData)?;
        call.returned = true;
        let uploads: Vec<_> = s
            .actions
            .payload_upload_sends
            .iter()
            .filter(|((owner, batch), sends)| {
                owner == instance
                    && sends.contains(&id)
                    && s.actions.payload_handlers_returned.contains(&(owner.clone(), *batch))
                    && s.actions
                        .calls
                        .iter()
                        .filter(|call| sends.contains(&call.id))
                        .all(|call| call.returned)
            })
            .map(|(key, _)| key.clone())
            .collect();
        for key in uploads {
            s.actions.payload_uploads.remove(&key);
            s.actions.payload_upload_sends.remove(&key);
            s.actions.payload_handlers_returned.remove(&key);
        }
        Ok(())
    }
    pub fn send_calls(&self) -> Vec<SendCall> {
        self.state().actions.calls.clone()
    }
    pub fn exchange_result(&self, id: u64) -> Option<ExchangeResult> {
        let s = self.state();
        s.actions.exchanges.get(&id).cloned().or_else(|| {
            s.actions
                .exchange
                .as_ref()
                .filter(|(i, _)| *i == id)
                .map(|(_, r)| r.clone())
        })
    }
}
