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
    pub fn start_sdk_instance(&self) -> Result<(), RuntimeError> {
        let instance = self.instance()?;
        let mut s = self.state();
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
            .any(|e| matches!(e,TraceEvent::EngineStarted{instance:i,..} if *i==instance))
        {
            s.trace.push(TraceEvent::EngineStarted { instance, account });
        }
        Ok(())
    }
    pub fn instance(&self) -> Result<String, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        Ok(format!("{}-{}-{}", b.open_counter, s.incarnation, b.file.inode))
    }
    pub fn set_automatic(&self, on: bool) {
        self.state().actions.automatic = on;
    }
    fn active_review(&self) -> Option<u64> {
        self.review
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .upgrade()
            .and_then(|r| r.active_session())
            .map(|s| s.0)
    }
    /// Called off the writer after real ReviewService inputs complete.
    pub fn poll_review(&self) -> Result<(), RuntimeError> {
        let active = self.active_review();
        let mut s = self.state();
        s.review_blocked = active.is_some();
        if let Some(old) = s.last_session {
            if active != Some(old) {
                s.actions.observations.push(Observation::SessionEnded { session: old });
                s.last_session = active;
                self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
            }
        } else {
            s.last_session = active;
        }
        Ok(())
    }
    pub(super) fn workflow_gate(&self) -> Result<(), RuntimeError> {
        if self.active_review().is_some() {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        let s = self.state();
        Self::gate_locked(&s)
    }
    fn gate_locked(s: &State) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !b.usable || !b.verified {
            return Err(RuntimeError::IdentityUnverified);
        }
        if !s.whole_complete
            || s.scan_required
            || s.scan_running
            || s.tracking_stopped
            || !s.root_confirmed
            || s.last_session.is_some()
            || s.review_blocked
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
    fn sendables_locked(s: &State) -> Result<PendingSnapshot, RuntimeError> {
        if Self::gate_locked(s).is_err() {
            return Ok(PendingSnapshot::empty());
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let store = s.store.as_ref().ok_or(Error::Database)?;
        let mut names = BTreeMap::new();
        for kind in [
            codec::Kind::Note,
            codec::Kind::Card,
            codec::Kind::Revlog,
            codec::Kind::Deck,
            codec::Kind::Config,
        ] {
            let mut after = None;
            loop {
                let page = store.entity_page(b.binding.id, kind, after.as_deref())?;
                if page.is_empty() {
                    break;
                }
                for e in page {
                    after = Some(e.key.clone());
                    if e.mark.is_some() {
                        names.insert(e.name, PendingChange::Save);
                    }
                }
            }
        }
        Ok(PendingSnapshot(names))
    }
    pub(super) fn emit_pending_locked(
        &self,
        s: &mut State,
        kind: UpdateKind,
        force: BTreeSet<String>,
    ) -> Result<PendingUpdate, RuntimeError> {
        // Callers with the state guard hold tracking access only for row reads.
        let snapshot = with_tracking_access(|| Self::sendables_locked(s))?;
        Ok(s.actions.update(kind, snapshot, force))
    }
    pub fn pending_transition(&self, kind: UpdateKind, snapshot: PendingSnapshot) -> PendingUpdate {
        self.state().actions.update(kind, snapshot, BTreeSet::new())
    }
    pub fn tracking_sendables(&self) -> BTreeMap<String, PendingChange> {
        self.state().actions.desired.clone()
    }
    pub fn take_pending_updates(&self) -> Vec<PendingUpdate> {
        self.state().actions.take()
    }
    pub fn observations(&self) -> Vec<Observation> {
        self.state().actions.observations.clone()
    }
    pub fn emitted_update_ids(&self) -> Vec<u64> {
        self.state().actions.emitted.clone()
    }
    pub fn sdk_update_applied(&self, id: u64, added: &[String]) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let action = s.actions.handed_updates.remove(&id).ok_or(Error::InvalidData)?;
        if added.iter().any(|n| !action.adds.contains_key(n)) {
            return Err(Error::InvalidData.into());
        }
        for name in added {
            s.actions.observations.push(Observation::PendingAdded {
                name: name.clone(),
                action_id: id,
            });
        }
        Ok(())
    }
    pub fn sdk_saved_removed(&self, request_id: u64, names: &[String]) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.actions.handler.is_some() {
            return Err(Error::InvalidData.into());
        }
        let expected = s.actions.saved_callbacks.remove(&request_id).unwrap_or_default();
        if names.iter().cloned().collect::<BTreeSet<_>>() != expected {
            return Err(Error::InvalidData.into());
        }
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
    pub fn enter_sent_handler(&self, request_id: u64) -> Result<(), RuntimeError> {
        Ok(self.state().actions.begin_handler(request_id)?)
    }
    pub fn return_sent_handler(&self) -> Result<u64, RuntimeError> {
        Ok(self.state().actions.end_handler()?)
    }
    pub fn reconcile_binding(
        &self,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
        timeout: Duration,
    ) -> Result<NoteScanProof, RuntimeError> {
        let guard = {
            let mut s = self.state();
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if !b.usable || !b.verified {
                return Err(RuntimeError::IdentityUnverified);
            }
            if s.scan_running || s.tracking_stopped {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            let g = ScanGuard {
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
        let instance = self.instance()?;
        let result = (|| {
            self.with_scan(&guard, collection, |s| {
                if !s
                    .store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .inbox(guard.binding)?
                    .is_empty()
                {
                    return Err(Error::InvalidData.into());
                }
                Ok(())
            })?;
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
                                .entity_content(guard.binding, kind, &key)?
                                .ok_or(Error::InvalidData)?)
                        })?;
                        let mut e = super::super::workflow_store::decode_entity(kind, &content)?;
                        let p = row_payload::project(&view, kind, &row, |id| {
                            let content = {
                                let s = self.state();
                                with_tracking_access(|| {
                                    s.store
                                        .as_ref()
                                        .ok_or(Error::Database)?
                                        .entity_content(guard.binding, codec::Kind::Notetype, &codec::id_key(id))?
                                        .ok_or(Error::InvalidData)
                                })
                            }?;
                            let e = super::super::workflow_store::decode_entity(codec::Kind::Notetype, &content)?;
                            match e.acked_payload {
                                Some(codec::Payload::Notetype(n)) => Ok(n),
                                _ => Err(Error::InvalidData),
                            }
                        })?;
                        if let (codec::Payload::Deck(old), codec::Payload::Deck(new)) = (&e.payload, &p) {
                            if old.name != new.name || old.kind != new.kind || old.parent_id != new.parent_id {
                                return Err(RuntimeError::Incomplete(Incomplete::StructuralLayout));
                            }
                        }
                        if !matches!(
                            kind,
                            codec::Kind::Note
                                | codec::Kind::Card
                                | codec::Kind::Revlog
                                | codec::Kind::Deck
                                | codec::Kind::Config
                        ) && e.acked_payload.as_ref() != Some(&p)
                        {
                            return Err(RuntimeError::Incomplete(Incomplete::StructuralLayout));
                        }
                        if e.payload != p {
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
            // Only the empty physical/J6 case is supported by this workflow.
            let feed = media.store().media_feed_snapshot().map_err(|_| Error::InvalidData)?;
            if !feed.status.complete || !feed.items.is_empty() || media.tracked_names()?.names.len() != 0 {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            let mut entries = std::fs::read_dir(media.store().media_dir()).map_err(Error::from)?;
            if entries.next().transpose().map_err(Error::from)?.is_some() {
                return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
            }
            self.with_scan(&guard, collection, |s| {
                if !s
                    .store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .entity_page(guard.binding, codec::Kind::MediaName, None)?
                    .is_empty()
                {
                    return Err(Error::InvalidData.into());
                }
                Ok(())
            })?;
            let end_feed = media.store().media_feed_status().map_err(|_| Error::InvalidData)?;
            if feed.status != end_feed {
                return Err(RuntimeError::StaleIncarnation);
            }
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
                coverage_counts.push((codec::Kind::MediaName, 0));
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
                for (kind, row_count) in counts {
                    s.actions
                        .observations
                        .push(Observation::ReconcileKindCompleted { kind, row_count });
                }
                s.actions.observations.push(Observation::ReconcileKindCompleted {
                    kind: codec::Kind::MediaName,
                    row_count: 0,
                });
                s.actions.observations.push(Observation::MediaFeedDrained {
                    entry_count: 0,
                    feed_id: feed.status.feed_id.0,
                    cursor: feed.status.counter,
                });
                Ok(())
            })?;
            Ok(proof)
        })();
        let mut s = self.state();
        if s.incarnation == guard.incarnation {
            s.scan_running = false;
            if result.is_err() {
                s.scan_required = true;
                s.whole_complete = false;
            }
        }
        result
    }
    /// A completed SDK fetch at the current no-state point, with no root-zone
    /// delivery, permits the V26.1 record-ID fallback. It is not confirmation.
    pub fn sdk_fetch_completed_without_root(&self, instance: &str) -> Result<(), RuntimeError> {
        if self.instance()? != instance {
            return Err(RuntimeError::StaleIncarnation);
        }
        let mut s = self.state();
        if !s.whole_complete || s.scan_required {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        s.root_fetch_qualified = true;
        Ok(())
    }
    pub fn root_read_due(&self) -> Result<bool, RuntimeError> {
        let s = self.state();
        Ok(s.root_fetch_qualified && s.whole_complete && s.bound.as_ref().is_some_and(|b| b.verified && b.usable))
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
        if self.instance()? != instance {
            return Ok(false);
        }
        let active = super::super::control_codec::Active::decode(envelope).map_err(|_| Error::InvalidData)?;
        let mut s = self.state();
        if !completed_fetch {
            s.trace.push(TraceEvent::RootReadRecorded {
                instance: instance.into(),
                request_id,
            });
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let id = b.binding.id;
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
        if !completed || self.instance()? != instance {
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
        collection: &Collection,
        requested: &BTreeSet<String>,
    ) -> Result<BuiltBatch, RuntimeError> {
        self.poll_review()?;
        if self.workflow_gate().is_err() {
            return Ok(BuiltBatch {
                id: 0,
                saves: Vec::new(),
            });
        }
        let guard = {
            let s = self.state();
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            ScanGuard {
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            }
        };
        self.with_scan(&guard, collection, |_| Ok(()))?;
        let mut s = self.state();
        Self::gate_locked(&s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
        let mut saves = Vec::new();
        let mut bytes = 0;
        let limit = s.actions.record_limit;
        let byte_limit = s.actions.byte_limit;
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            for name in requested {
                if saves.len() >= limit {
                    break;
                }
                let Some(e) = store.entity_facts(b.id, name)? else {
                    continue;
                };
                if e.mark.is_none()
                    || !matches!(
                        e.kind,
                        codec::Kind::Note
                            | codec::Kind::Card
                            | codec::Kind::Revlog
                            | codec::Kind::Deck
                            | codec::Kind::Config
                    )
                {
                    continue;
                }
                let erev = e.server_erev.checked_add(1).ok_or(Error::InvalidData)?;
                let payload = e.payload.encode().map_err(|_| Error::InvalidData)?;
                let entity = codec::Entity::build(
                    codec::EntityFields {
                        kind: e.kind,
                        key: e.key.clone(),
                        origin: e.origin,
                        erev,
                        generation: None,
                        tombstone: false,
                        deleted_at_ms: 0,
                        writer: b.id.0,
                        base_sha256: if e.server_erev > 0 {
                            Some(e.acked_payload_digest)
                        } else {
                            None
                        },
                    },
                    &payload,
                )
                .map_err(|_| Error::InvalidData)?;
                if entity.payload.is_none() {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                let envelope = entity.encode().map_err(|_| Error::InvalidData)?;
                let size = envelope.len().checked_add(2048).ok_or(Error::InvalidData)?;
                if size > byte_limit {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                if bytes + size > byte_limit {
                    break;
                }
                bytes += size;
                store.persist_flight(
                    b.id,
                    name,
                    Flight {
                        local_rev: e.local_rev,
                        payload_digest: entity.payload_sha256,
                        row_digest: e.payload.row_digest().map_err(|_| Error::InvalidData)?,
                        payload: e.payload.clone(),
                        erev,
                    },
                )?;
                saves.push(SaveInput {
                    name: name.clone(),
                    envelope,
                    system_fields: if e.server_erev > 0 {
                        Some(e.acked_system_fields)
                    } else {
                        None
                    },
                });
            }
            Ok(())
        })?;
        if !saves.is_empty() {
            let instance = format!(
                "{}-{}-{}",
                s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.open_counter,
                s.incarnation,
                s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.file.inode
            );
            s.trace.push(TraceEvent::CollectionBatchBuilt {
                instance,
                names: saves.iter().map(|v| v.name.clone()).collect(),
            });
        }
        s.actions.batch += 1;
        let id = s.actions.batch;
        if !saves.is_empty() {
            s.actions.observations.push(Observation::BatchBuilt {
                batch_id: id,
                names: saves.iter().map(|e| e.name.clone()).collect(),
            });
        }
        drop(s);
        self.with_scan(&guard, collection, |state| Self::gate_locked(state))?;
        Ok(BuiltBatch { id, saves })
    }
    pub fn sent_results(
        &self,
        request_id: u64,
        saved: &[Saved],
        failed: &[(String, SendError)],
        batch_count: usize,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.actions.handler != Some(request_id) {
            return Err(Error::InvalidData.into());
        }
        if failed
            .iter()
            .any(|(_, error)| matches!(error, SendError::ZoneMissing | SendError::Other))
        {
            s.scan_required = true;
            s.tracking_stopped = true;
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let mut force = BTreeSet::new();
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            for saved in saved {
                if store.acknowledge(b, saved)? {
                    force.insert(saved.name.clone());
                }
            }
            Ok(())
        })?;
        if failed.iter().any(|(_, e)| *e == SendError::LimitExceeded) && batch_count > 1 {
            s.actions.record_limit = (s.actions.record_limit / 2).max(1);
            s.actions.byte_limit = (s.actions.byte_limit / 2).max(1);
        }
        for (name, error) in failed {
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
        if let Some((_, exchange)) = &mut s.actions.exchange {
            for name in acknowledged {
                exchange.acked_names.insert(name.clone());
                exchange.outstanding_names.remove(&name);
            }
        }
        s.actions
            .saved_callbacks
            .insert(request_id, saved.iter().map(|s| s.name.clone()).collect());
        if !saved.is_empty() || !failed.is_empty() {
            self.emit_pending_locked(&mut s, UpdateKind::Changes, force)?;
        }
        Ok(())
    }
    pub fn sync_now(&self, collection: &Collection) -> Result<u64, RuntimeError> {
        self.poll_review()?;
        self.workflow_gate()?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let boundary = snapshot.seq().0;
        let mut s = self.state();
        if s.actions.exchange.is_some() {
            return Err(Error::InvalidData.into());
        }
        self.emit_pending_locked(&mut s, UpdateKind::Full, BTreeSet::new())?;
        let id = s.actions.next;
        let names = s.actions.desired.keys().cloned().collect::<BTreeSet<_>>();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let revs = with_tracking_access(|| -> Result<BTreeMap<String, u64>, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut revs = BTreeMap::new();
            for name in &names {
                let e = store.entity_facts(b, name)?.ok_or(Error::InvalidData)?;
                if e.observed_generation == snapshot.generation().0 && e.observed_seq > boundary {
                    return Err(RuntimeError::StaleIncarnation);
                }
                revs.insert(name.clone(), e.local_rev);
            }
            Ok(revs)
        })?;
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
    pub fn accept_fetched_baseline(&self, records: &[Saved]) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
        let result = with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for record in records {
                let received = codec::receive(
                    &b.facts.record_key.0,
                    &record.name,
                    &record.envelope,
                    codec::AssetInput::Absent,
                )
                .map_err(|_| Error::InvalidData)?;
                let e = store
                    .entity_facts(b.id, &record.name)?
                    .ok_or(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation))?;
                if received.entity.fields.origin != e.origin
                    || received.entity.fields.erev != e.server_erev
                    || received.entity.payload_sha256 != e.acked_payload_digest
                    || record.system_fields != e.acked_system_fields
                {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
            }
            Ok(())
        });
        if result.is_err() {
            s.scan_required = true;
            s.tracking_stopped = true;
        }
        result
    }
    pub fn exchange_fetch_completed(&self, phase: FetchPhase, seen: &BTreeSet<String>) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.actions.exchange_phase != Some(phase) {
            return Err(Error::InvalidData.into());
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            for kind in row_payload::KINDS {
                let mut after = None;
                loop {
                    let page = store.entity_page(binding, kind, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    for e in page {
                        after = Some(e.key);
                        if e.server_erev > 0 && !seen.contains(&e.name) {
                            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                        }
                    }
                }
            }
            Ok(())
        })?;
        s.actions.exchange_phase = None;
        if phase == FetchPhase::AfterSend {
            if let Some((id, r)) = s.actions.exchange.take() {
                s.actions.exchanges.insert(id, r);
            }
        }
        Ok(())
    }
    pub fn exchange_sends_finished(&self) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.actions.exchange.is_some() {
            s.actions.exchange_phase = Some(FetchPhase::AfterSend);
        }
        Ok(())
    }
    pub fn wants_send(&self) -> bool {
        let s = self.state();
        s.actions.automatic || s.actions.exchange.is_some()
    }
    pub fn start_send_call(&self) -> Result<Option<SendCall>, RuntimeError> {
        let mut s = self.state();
        if s.actions.exchange_phase.is_some()
            || s.actions.handler.is_some()
            || (!s.actions.automatic && s.actions.exchange.is_none())
        {
            return Ok(None);
        }
        let exchange = s.actions.exchange.as_ref().map(|(id, _)| *id);
        if let Some(id) = exchange {
            if s.actions.calls.iter().filter(|c| c.exchange == Some(id)).count() >= 8 {
                return Ok(None);
            }
        }
        let call = SendCall {
            id: s.actions.calls.len() as u64 + 1,
            exchange,
            automatic: exchange.is_none(),
            request_ids: Vec::new(),
            returned: false,
        };
        s.actions.calls.push(call.clone());
        Ok(Some(call))
    }
    pub fn send_call_request(&self, id: u64, request: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let c = s
            .actions
            .calls
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(Error::InvalidData)?;
        c.request_ids.push(request);
        Ok(())
    }
    pub fn finish_send_call(&self, id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let call = s
            .actions
            .calls
            .iter_mut()
            .find(|c| c.id == id)
            .ok_or(Error::InvalidData)?;
        call.returned = true;
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
