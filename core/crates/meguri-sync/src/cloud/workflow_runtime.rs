use super::*;
use crate::cloud::{
    row_payload::{self, View},
    workflow::*,
};
use std::collections::{BTreeMap, BTreeSet};
impl Runtime {
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
        let active = self.active_review();
        let mut s = self.state();
        if Self::caller_locked(&s, instance).is_err() {
            return Ok(None);
        }
        if let Some(old) = s.last_session {
            if active != Some(old) {
                s.actions.observations.push(Observation::SessionEnded { session: old });
                s.last_session = active;
                s.cloud.detached.clear();
                s.cloud.incoming_held.clear();
                s.cloud.deferred.clear();
                self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
                return Ok(Some(old));
            }
        } else {
            s.last_session = active;
        }
        Ok(None)
    }
    pub(super) fn workflow_gate(&self) -> Result<(), RuntimeError> {
        let s = self.state();
        Self::gate_locked(&s)
    }
    pub(super) fn gate_locked(s: &State) -> Result<(), RuntimeError> {
        Self::capture_gate_locked(s)?;
        if !s.root_confirmed && !Self::publication_allows_send_locked(s) {
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
    pub fn entity_facts(&self, name: &str) -> Result<Option<EntityFacts>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .entity_facts(b.binding.id, name)?)
        })
    }
    pub fn entity_facts_page(&self, kind: codec::Kind, after: Option<&[u8]>) -> Result<Vec<EntityFacts>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .entity_page(b.binding.id, kind, after)?)
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
        Ok(pending)
    }
    pub(super) fn emit_pending_locked(
        &self,
        s: &mut State,
        kind: UpdateKind,
        force: BTreeSet<String>,
    ) -> Result<PendingUpdate, RuntimeError> {
        if kind == UpdateKind::Full {
            s.cloud.deferred.clear();
            s.policy.parked.clear();
            s.policy.held_images.clear();
        }
        // Callers with the state guard hold tracking access only for row reads.
        let snapshot = with_tracking_access(|| Self::sendables_locked(s))?;
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
    pub fn enter_sent_handler(&self, instance: &str, request_id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        Ok(s.actions.begin_handler(request_id)?)
    }
    pub fn return_sent_handler(&self, instance: &str) -> Result<u64, RuntimeError> {
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        Ok(s.actions.end_handler()?)
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
            let mut recovered = Vec::new();
            let mut counts = Vec::new();
            for kind in row_payload::KINDS.into_iter().filter(|k| *k != codec::Kind::MediaName) {
                let table = row_payload::table(kind).ok_or(Error::InvalidData)?;
                // Include tracked absences; a local-only scan cannot prove completeness.
                let mut after = None;
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
                            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                        }
                        after = Some(key);
                    }
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
                        let content = self.with_scan(&guard, collection, |s| {
                            Ok(s.store
                                .as_ref()
                                .ok_or(Error::Database)?
                                .entity_content(guard.binding, kind, &key)?)
                        })?;
                        let mut existing = content
                            .as_deref()
                            .map(|content| super::super::workflow_store::decode_entity(kind, content))
                            .transpose()?;
                        let newly_observed = existing.is_none();
                        if newly_observed && !publishing && !matches!(kind, codec::Kind::Note | codec::Kind::Card) {
                            return Err(Error::InvalidData.into());
                        }
                        let p = row_payload::project(&view, kind, &row, |id| {
                            let tracked = {
                                let s = self.state();
                                with_tracking_access(|| {
                                    s.store.as_ref().ok_or(Error::Database)?.entity_content(
                                        guard.binding,
                                        codec::Kind::Notetype,
                                        &codec::id_key(id),
                                    )
                                })
                            }?;
                            if let Some(content) = tracked {
                                let e = super::super::workflow_store::decode_entity(codec::Kind::Notetype, &content)?;
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
        self.accept_active_proof(instance, request_id, envelope, system_fields, lost_zone, false)
    }
    fn accept_active_proof(
        &self,
        instance: &str,
        request_id: u64,
        envelope: &[u8],
        system_fields: &[u8],
        lost_zone: bool,
        completed_fetch: bool,
    ) -> Result<bool, RuntimeError> {
        let active = super::super::control_codec::Active::decode(envelope).map_err(|_| Error::InvalidData)?;
        let mut s = self.state();
        if Self::caller_locked(&s, instance).is_err() {
            return Ok(false);
        }
        if !completed_fetch {
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
            with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .isolate_runtime_binding(id, reason)
            })?;
            let binding = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.binding(id))?
                .ok_or(Error::BindingNotFound)?;
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.usable = false;
            b.verified = false;
            b.binding = binding;
            return Ok(false);
        }
        let open = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.open_counter;
        with_tracking_access(|| {
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
            true,
        )
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
            if Self::caller_locked(&s, instance).is_err() {
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
                if let Some(e) = store.entity_facts(guard.binding, name)? {
                    if e.kind == codec::Kind::MediaName {
                        continue;
                    }
                    for (table, key, _) in row_payload::payload_rows(&e.payload)? {
                        named.entry((table, key)).or_default().insert(name.clone());
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
                if let Some(e) = store.entity_facts(guard.binding, name)? {
                    facts.insert(name.clone(), e);
                }
            }
            let ready = Self::gate_locked(s).is_ok()
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
        let mut images = BTreeMap::new();
        // All collection reads, lazy journal reads and projection/encoding are
        // off the writer and outside state/tracking guards. Incoming merge L
        // continues to be read directly from its write scope.
        for (name, e) in &facts {
            if blocked.contains(name)
                || (e.observer_open == guard.open
                    && e.observed_generation == snapshot.generation().0
                    && e.observed_seq > snapshot.seq().0)
            {
                deferred.insert(name.clone());
                continue;
            }
            if e.mark.is_none()
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
                continue;
            }
            if e.kind == codec::Kind::MediaName {
                images.insert(name.clone(), (e.payload.clone(), snapshot.seq().0, false));
                continue;
            }
            let view = super::super::held_view::HeldView::new(&*snapshot, &runs, snapshot.seq());
            let table = row_payload::table(e.kind).ok_or(Error::InvalidData)?;
            let key = super::super::workflow_store::row_key(e.kind, &e.key)?;
            let Some(row) = view.row(table, &key)? else {
                // A held creation is absent, never a deletion/tombstone.
                deferred.insert(name.clone());
                continue;
            };
            let payload = row_payload::project(&view, e.kind, &row, |id| {
                let tracked = self
                    .with_scan(&guard, collection, |s| {
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
                    && let codec::Payload::Notetype(n) = e.acked_payload.unwrap_or(e.payload)
                {
                    return Ok(n);
                }
                if publishing {
                    let row = view.row(Table::Notetypes, &RowKey::Id(id))?.ok_or(Error::InvalidData)?;
                    return row_payload::notetype(&view, &row, 1);
                }
                Err(Error::InvalidData)
            });
            let payload = match payload {
                Ok(payload) => payload,
                Err(Error::InvalidData) => {
                    deferred.insert(name.clone());
                    continue;
                }
                Err(error) => return Err(error.into()),
            };
            let (image_seq, held) = view.image();
            if payload.row_digest().map_err(|_| Error::InvalidData)? == e.acked_row_digest && e.server_erev > 0 {
                if held {
                    deferred.insert(name.clone());
                }
                continue;
            }
            images.insert(name.clone(), (payload, image_seq, held));
        }
        let mut prepared = BTreeMap::new();
        for (name, (payload, image_seq, held)) in images {
            let e = facts.get(&name).ok_or(Error::InvalidData)?;
            let erev = e.server_erev.checked_add(1).ok_or(Error::InvalidData)?;
            let tombstone_at = if e.kind == codec::Kind::MediaName {
                self.with_scan(&guard, collection, |s| {
                    Ok(s.store.as_ref().ok_or(Error::Database)?.media_ledger(guard.binding)?)
                })?
                .names
                .get(std::str::from_utf8(&e.key).map_err(|_| Error::InvalidData)?)
                .and_then(|n| n.tombstone_at)
            } else {
                None
            };
            let payload = if tombstone_at.is_some() {
                e.acked_payload.clone().ok_or(Error::InvalidData)?
            } else {
                payload
            };
            let encoded_payload = payload.encode().map_err(|_| Error::InvalidData)?;
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
            if entity.payload.is_none() {
                return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
            }
            let envelope = entity.encode().map_err(|_| Error::InvalidData)?;
            let size = envelope.len().checked_add(2048).ok_or(Error::InvalidData)?;
            let mut references = super::super::publication_store::references(&payload)?
                .into_iter()
                .map(|(kind, key)| {
                    codec::entity_name(&binding.facts.record_key.0, kind, &key).map_err(|_| Error::InvalidData)
                })
                .collect::<Result<Vec<_>, _>>()?;
            if let codec::Payload::Note(note) = &payload {
                let fields = std::str::from_utf8(&note.flds).map_err(|_| Error::InvalidData)?;
                for local_name in meguri_text::media_refs::local_references(fields) {
                    let case = local_name.to_lowercase();
                    let media_name =
                        codec::entity_name(&binding.facts.record_key.0, codec::Kind::MediaName, case.as_bytes())
                            .map_err(|_| Error::InvalidData)?;
                    let hold = self.with_scan(&guard, collection, |s| {
                        let store = s.store.as_ref().ok_or(Error::Database)?;
                        let ledger = store.media_ledger(guard.binding)?;
                        Ok(ledger.names.get(&case).is_none_or(|n| n.withheld.is_none())
                            && store
                                .entity_facts(guard.binding, &media_name)?
                                .is_some_and(|m| m.server_erev == 0 || m.acked_media_tombstone()))
                    })?;
                    if hold {
                        references.push(media_name);
                    }
                }
            }
            let flight = Flight {
                local_rev: e.local_rev,
                payload_digest: entity.payload_sha256,
                row_digest: payload.row_digest().map_err(|_| Error::InvalidData)?,
                payload,
                erev,
                image_seq,
                held,
            };
            prepared.insert(name, (flight, envelope, size, references));
        }
        self.with_scan(&guard, collection, |s| {
            Self::caller_locked(s, instance)?;
            if Self::gate_locked(s).is_err() || s.cloud.incoming_seq > snapshot.seq().0 {
                return Ok(empty());
            }
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
            let store = s.store.as_mut().ok_or(Error::Database)?;
            // This transport uses ordinary non-atomic batches. Unacked
            // references go first; dependents wait until their real ACK.
            // These waits stay pending; only image deferrals persist below.
            let mut waiting = BTreeSet::new();
            loop {
                let mut changed = false;
                for (name, (_, _, _, references)) in &prepared {
                    if deferred.contains(name) {
                        continue;
                    }
                    for reference in references {
                        if deferred.contains(reference)
                            || s.cloud.deferred.contains(reference)
                            || s.cloud.incoming_held.contains(reference)
                            || store.incoming_blocks_name(b.id, reference)?
                        {
                            changed |= deferred.insert(name.clone());
                            break;
                        }
                        if waiting.contains(reference)
                            || !store
                                .entity_facts(b.id, reference)?
                                .is_some_and(|e| e.server_erev > 0 && e.acked_payload.is_some())
                        {
                            changed |= waiting.insert(name.clone());
                        }
                    }
                }
                if !changed {
                    break;
                }
            }
            let mut saves = Vec::new();
            let mut flights = BTreeMap::new();
            let mut bytes = 0usize;
            for (name, (flight, envelope, size, _)) in prepared {
                if deferred.contains(&name) || waiting.contains(&name) || store.incoming_blocks_name(b.id, &name)? {
                    continue;
                }
                if saves.len() >= s.actions.record_limit {
                    break;
                }
                let e = store.entity_facts(b.id, &name)?.ok_or(Error::InvalidData)?;
                let captured = facts.get(&name).ok_or(Error::InvalidData)?;
                // Another saved callback may have advanced the conditional
                // tag while this snapshot was being encoded. Rebuild later.
                if e.server_erev != captured.server_erev
                    || e.acked_payload_digest != captured.acked_payload_digest
                    || e.acked_system_fields != captured.acked_system_fields
                {
                    continue;
                }
                if size > s.actions.byte_limit {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                if bytes + size > s.actions.byte_limit {
                    break;
                }
                bytes += size;
                let image = (name.clone(), flight.erev, flight.payload_digest);
                let image_seq = flight.image_seq;
                let held_image = flight.held;
                let codec::DecodeOutcome::Supported(entity) =
                    codec::Entity::decode(&envelope).map_err(|_| Error::InvalidData)?
                else {
                    return Err(Error::InvalidData.into());
                };
                flights.insert(
                    name.clone(),
                    HandedSave {
                        origin: entity.fields.origin,
                        local_rev: flight.local_rev,
                        erev: entity.fields.erev,
                        payload_digest: entity.payload_sha256,
                        tombstone: entity.fields.tombstone,
                    },
                );
                store.persist_flight(b.id, &name, flight)?;
                s.cloud
                    .handed_images
                    .entry(image)
                    .and_modify(|seq| *seq = (*seq).max(image_seq))
                    .or_insert(image_seq);
                if held_image {
                    s.policy.held_images.insert(name.clone());
                } else {
                    s.policy.held_images.remove(&name);
                }
                saves.push(SaveInput {
                    name,
                    envelope,
                    system_fields: if e.server_erev > 0 {
                        Some(e.acked_system_fields)
                    } else {
                        None
                    },
                });
            }
            // A full update after S already released old deferrals. Do not
            // reinstall them from a snapshot preceding that local operation.
            if s.last_capture
                .as_ref()
                .is_none_or(|proof| proof.observed_seq <= snapshot.seq())
            {
                s.cloud.deferred.extend(deferred);
            }
            let pending = Self::sendables_locked(s)?;
            s.actions.update(UpdateKind::Changes, pending, BTreeSet::new());
            // Retain the actual receipt and pinned view through flight commit.
            let _covered = &receipt;
            s.actions.batch += 1;
            let id = s.actions.batch;
            if !saves.is_empty() {
                let names = saves.iter().map(|v| v.name.clone()).collect();
                s.trace.push(TraceEvent::CollectionBatchBuilt {
                    instance: instance.into(),
                    names,
                });
                s.actions.observations.push(Observation::BatchBuilt {
                    batch_id: id,
                    names: saves.iter().map(|e| e.name.clone()).collect(),
                });
            }
            if !saves.is_empty() {
                s.actions.request_flights.insert((instance.to_owned(), id), flights);
                s.policy.flights.insert(
                    (instance.to_owned(), id),
                    saves.iter().map(|v| v.name.clone()).collect(),
                );
            }
            Ok(BuiltBatch { id, saves })
        })
    }
    fn decode_saved(&self, instance: &str, records: &[Saved]) -> Result<Vec<codec::Received>, RuntimeError> {
        let key = {
            let s = self.state();
            Self::caller_locked(&s, instance)?;
            s.bound
                .as_ref()
                .ok_or(RuntimeError::StaleIncarnation)?
                .binding
                .facts
                .record_key
                .0
        };
        records
            .iter()
            .map(|record| {
                codec::receive(&key, &record.name, &record.envelope, codec::AssetInput::Absent)
                    .map_err(|_| RuntimeError::Tracking(Error::InvalidData))
            })
            .collect()
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
                        && let Some(content) = store.entity_content(binding, kind, &entity_key)?
                    {
                        let entity = super::super::workflow_store::decode_entity(kind, &content)?;
                        if row_payload::payload_rows(&entity.payload)?
                            .iter()
                            .any(|(t, k, _)| *t == table && super::super::held_view::compare(table, &key, k).is_eq())
                        {
                            name = Some(entity.name);
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
                let e = store.entity_facts(b, name)?;
                let acked = s.cloud.acked_images.get(name).is_some_and(|seq| *seq >= id.0);
                let clean = e.is_some_and(|e| e.server_erev > 0 && e.mark.is_none());
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
    pub fn sent_results(
        &self,
        instance: &str,
        collection: &Collection,
        request_id: u64,
        saved: &[Saved],
        failed: &[(String, SendError)],
        batch_count: usize,
    ) -> Result<(), RuntimeError> {
        let decoded = self.decode_saved(instance, saved)?;
        let at_ms = self.adapter_now()?;
        let evidence = collection.open_evidence()?;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
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
        with_tracking_access(|| -> Result<(), RuntimeError> {
            for (saved, received) in saved.iter().zip(decoded) {
                let image_key = (
                    saved.name.clone(),
                    received.entity.fields.erev,
                    received.entity.payload_sha256,
                );
                let image = s.cloud.handed_images.get(&image_key).copied();
                let store = s.store.as_mut().ok_or(Error::Database)?;
                if store.acknowledge(b, saved, received)? {
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
                Self::settle_cloud_uploaded_locked(&mut s, collection)?;
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
            for name in saved.iter().map(|v| &v.name).chain(failed.iter().map(|(n, _)| n)) {
                names.remove(name);
            }
        }
        if let Some(records) = s.actions.request_flights.get_mut(&(instance.to_owned(), request_id)) {
            for name in saved.iter().map(|r| &r.name).chain(failed.iter().map(|(n, _)| n)) {
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
        let acknowledged = with_tracking_access(|| -> Result<Vec<String>, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut done = Vec::new();
            for saved in saved {
                if let Some(selected) = s.actions.selected_revs.get(&saved.name) {
                    let e = store.entity_facts(b, &saved.name)?.ok_or(Error::InvalidData)?;
                    if e.acked_rev >= *selected {
                        done.push(saved.name.clone());
                    }
                }
            }
            Ok(done)
        })?;
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
            for name in acknowledged {
                exchange.acked_names.insert(name.clone());
                exchange.outstanding_names.remove(&name);
            }
            // Storage proved absence for this handed revision. Keep it out of
            // the ACK set and leave every other selected revision outstanding.
            for name in terminal_names {
                exchange.outstanding_names.remove(&name);
            }
        }
        s.actions
            .saved_callbacks
            .insert(request_id, saved.iter().map(|s| s.name.clone()).collect());
        // Non-atomic batches can contain both Saved and an unsupported failure.
        // Preserve real acknowledgements, exchange accounting and handler
        // receipts before stopping. Failed records remain durably pending.
        if failed
            .iter()
            .any(|(_, error)| matches!(error, SendError::ZoneMissing | SendError::Other))
        {
            Self::invalidate_locked(&mut s);
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        if !saved.is_empty() || !failed.is_empty() {
            self.emit_pending_locked(&mut s, UpdateKind::Changes, force)?;
        }
        Ok(())
    }
    pub fn sync_now(&self, instance: &str, collection: &Collection) -> Result<u64, RuntimeError> {
        self.poll_review(instance)?;
        self.consume_media_feed(collection, &self.media_service(collection)?)?;
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
        let names = with_tracking_access(|| Self::pending_selection_locked(&s))?
            .0
            .into_keys()
            .collect::<BTreeSet<_>>();
        let bound = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let b = bound.binding.id;
        let open = bound.open_counter;
        let revs = with_tracking_access(|| -> Result<BTreeMap<String, u64>, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut revs = BTreeMap::new();
            for name in &names {
                let e = store.entity_facts(b, name)?.ok_or(Error::InvalidData)?;
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
        s.adapter.exchange_owner = Some((id, instance.into(), s.incarnation));
        s.actions.selected_revs = revs;
        s.actions.exchange_phase = Some(FetchPhase::BeforeSend);
        s.actions.exchange = Some((
            id,
            ExchangeResult {
                boundary_seq: boundary,
                selected_names: names.clone(),
                acked_names: BTreeSet::new(),
                outstanding_names: names,
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
        // Full baseline discovery stays off the writer. Copy one bounded body
        // under the guards, then decode and compare after releasing both.
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
                    let content = {
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
                                .entity_content(binding, kind, &key)
                        })?
                        .ok_or(Error::InvalidData)?
                    };
                    let entity = super::super::workflow_store::decode_entity(kind, &content)?;
                    if entity.server_erev > 0 && !seen.contains(&entity.name) {
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
        self.start_send_call_mode(instance, false)
    }
    pub fn start_manual_send_call(&self, instance: &str) -> Result<Option<SendCall>, RuntimeError> {
        self.start_send_call_mode(instance, true)
    }
    fn start_send_call_mode(&self, instance: &str, manual: bool) -> Result<Option<SendCall>, RuntimeError> {
        let mut s = self.state();
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
        s.adapter.sdk_retrying = false;
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
        let requests = call.request_ids.clone();
        for request in requests {
            s.policy.flights.remove(&(instance.to_owned(), request));
            s.actions.request_flights.remove(&(instance.to_owned(), request));
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
