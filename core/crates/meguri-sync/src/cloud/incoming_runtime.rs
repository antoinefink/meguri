//! The shared host/SDK ordinary receive and collection-apply path.
use super::*;
use crate::cloud::{incoming::*, incoming_merge, row_payload, workflow::Saved};
use std::collections::{BTreeMap, BTreeSet};

#[path = "rekey_runtime.rs"]
mod rekey_impl;

type IntentRows = (
    BTreeMap<String, Option<codec::Payload>>,
    Seq,
    meguri_engine::WalIdentity,
);

// Ordinary replay can use a recovery row's old owner without selecting it or
// opening its SDK gate. The scan pins the actual current engine opening.
#[derive(Clone)]
struct IncomingGuard {
    scan: ScanGuard,
    recovery: Option<RecoveryGuard>,
    sdk: Option<String>,
}
#[derive(Clone)]
struct RecoveryGuard {
    anchor: ScanGuard,
    owner: Binding,
    op: crate::cloud::adoption::RecoveryKind,
    decision: crate::cloud::publication::DecisionSnapshot,
}
impl std::ops::Deref for IncomingGuard {
    type Target = ScanGuard;
    fn deref(&self) -> &ScanGuard {
        &self.scan
    }
}

struct Running(Arc<Mutex<State>>, Arc<std::sync::Condvar>);
impl Drop for Running {
    fn drop(&mut self) {
        StateGuard::new(self.0.lock().unwrap_or_else(PoisonError::into_inner))
            .incoming
            .running = false;
        self.1.notify_all();
    }
}

impl Runtime {
    fn finish_incoming(&self, running: Running) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        drop(running);
        let mut s = self.state();
        let cleanup = with_tracking_access(|| -> Result<(), Error> {
            let binding = s.bound.as_ref().ok_or(Error::BindingNotFound)?.binding.id;
            let live = s.store.as_ref().ok_or(Error::Database)?.inbox(binding)?;
            super::super::payload_asset::sweep(&self.directory, binding, &live)
        });
        if let Err(error) = cleanup {
            Self::invalidate_locked(&mut s);
            return Err(error.into());
        }
        self.emit_pending_locked(&mut s, workflow::UpdateKind::Full, BTreeSet::new())?;
        Ok(())
    }
    pub(super) fn wait_incoming_idle(&self) {
        let mut s = self.state();
        while s.incoming.running {
            s = s.wait(&self.incoming_idle);
        }
    }
    pub(super) fn settle_before_transition(
        &self,
        collection: &Collection,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        self.wait_incoming_idle();
        let status = self.status()?;
        if status.binding.is_some() && status.stored_usable && status.identity_verified {
            self.replay_media_intents(collection, &self.media_service(collection)?)?;
        }
        let has_work = {
            let s = self.state();
            with_tracking_access(|| -> Result<bool, RuntimeError> {
                let Some(store) = s.store.as_ref() else {
                    return Ok(false);
                };
                let publish = store.publication_op()?;
                let adopt = store.adoption_op()?;
                let before = publish
                    .as_ref()
                    .and_then(|p| p.binding_before.as_ref())
                    .or_else(|| adopt.as_ref().and_then(|p| p.binding_before.as_ref()));
                if let Some(before) = before
                    && !store.apply_intents(before.id)?.is_empty()
                {
                    return Ok(true);
                }
                let Some(b) = s.bound.as_ref().filter(|b| b.usable && b.verified) else {
                    return Ok(false);
                };
                Ok(!store.apply_intents(b.binding.id)?.is_empty())
            })?
        };
        if !has_work {
            return Ok(());
        }
        // Read providers outside state/tracking guards. An isolated owner is
        // permitted only by this build's existing durable adopt/publish row.
        let account = self.publication_account()?;
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        let guard = {
            let mut s = self.state();
            if s.incoming.running {
                return Err(EngineError::Busy {
                    running_op: "cloud incoming".into(),
                }
                .into());
            }
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            let anchor = ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file: b.file,
                incarnation: s.incarnation,
            };
            let recovery = with_tracking_access(|| -> Result<_, RuntimeError> {
                use crate::cloud::adoption::RecoveryKind;
                let store = s.store.as_ref().ok_or(Error::Database)?;
                let publish = store.publication_op()?;
                let adopt = store.adoption_op()?;
                if publish.is_some() && adopt.is_some() {
                    return Err(Error::InvalidData.into());
                }
                let op = publish
                    .map(|p| RecoveryKind::Publish(Box::new(p)))
                    .or_else(|| adopt.map(|p| RecoveryKind::Adopt(Box::new(p))));
                let Some(op) = op else { return Ok(None) };
                let (before, op_account, op_file, publication) = match &op {
                    RecoveryKind::Publish(p) => {
                        (&p.binding_before, &p.account, p.confirmed_file, p.confirmed_publication)
                    }
                    RecoveryKind::Adopt(p) => {
                        // Initial adoption has no old file whose intents could
                        // be replayed. A prior binding must never be discarded.
                        if p.file_before.is_none() && p.binding_before.is_none() {
                            return Ok(None);
                        }
                        (
                            &p.binding_before,
                            &p.account,
                            p.file_before.ok_or(Error::InvalidData)?,
                            p.before.as_ref().ok_or(Error::InvalidData)?.publication.identity,
                        )
                    }
                };
                let Some(before) = before else { return Ok(None) };
                let owner = store.binding(before.id)?.ok_or(Error::BindingNotFound)?;
                // No intents means there is nothing to project from the old
                // file. In particular queued inbox entries are never applied.
                if store.apply_intents(owner.id)?.is_empty() {
                    return Ok(None);
                }
                if owner.scope != self.inputs.scope
                    || owner.facts.account != *op_account
                    || account != *op_account
                    || owner.facts.publication != publication
                    || decision.publication.identity != publication
                    || file != op_file
                    || store.runtime_binding(owner.id)?.file != Some(file)
                    || matches!(
                        owner.facts.state,
                        BindingState::Isolated(
                            IsolationReason::FileMismatch
                                | IsolationReason::ReplacedLocally
                                | IsolationReason::EncryptedDataReset
                        )
                    )
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                Ok(Some(RecoveryGuard {
                    anchor: anchor.clone(),
                    owner,
                    op,
                    decision: decision.clone(),
                }))
            })?;
            let guard = if let Some(recovery) = recovery {
                let open = with_tracking_access(|| {
                    s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .runtime_binding(recovery.owner.id)
                })?
                .open_counter;
                IncomingGuard {
                    scan: ScanGuard {
                        binding: recovery.owner.id,
                        open,
                        ..anchor
                    },
                    recovery: Some(recovery),
                    sdk: None,
                }
            } else {
                // A recovery row with no intents must not grant a full receive.
                if !b.usable || !b.verified {
                    return Ok(());
                }
                if account != b.binding.facts.account {
                    return Err(RuntimeError::StaleIncarnation);
                }
                IncomingGuard {
                    scan: anchor,
                    recovery: None,
                    sdk: None,
                }
            };
            s.incoming.running = true;
            guard
        };
        let running = Running(self.state.clone(), self.incoming_idle.clone());
        self.with_incoming(&guard, collection, |_| Ok(()))?;
        self.replay_ordinary(collection, &guard, timeout)?.into_result()?;
        drop(running);
        if guard.recovery.is_some() {
            // The current usable owner's own crash intents still precede its
            // scan after the recovery row's old owner has finished.
            self.settle_before_transition(collection, timeout)?;
        }
        Ok(())
    }

    fn check_incoming_locked(s: &State, guard: &IncomingGuard) -> Result<(), RuntimeError> {
        if let Some(sdk) = &guard.sdk {
            Self::sdk_locked(s, sdk)?;
        }
        let anchor = guard.recovery.as_ref().map_or(&guard.scan, |r| &r.anchor);
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if s.incarnation != anchor.incarnation
            || s.invalidation != anchor.invalidation
            || s.invalidation_exhausted
            || s.tracking_stopped
            || (b.binding.id, b.open_counter, b.file, b.evidence.generation)
                != (anchor.binding, anchor.open, anchor.file, anchor.generation)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if let Some(r) = &guard.recovery {
            use crate::cloud::adoption::RecoveryKind;
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let matches = match &r.op {
                RecoveryKind::Publish(p) => {
                    store.publication_op()?.as_ref() == Some(p.as_ref()) && store.adoption_op()?.is_none()
                }
                RecoveryKind::Adopt(p) => {
                    store.adoption_op()?.as_ref() == Some(p.as_ref()) && store.publication_op()?.is_none()
                }
            };
            if !matches
                || store.binding(r.owner.id)?.as_ref() != Some(&r.owner)
                || store.runtime_binding(r.owner.id)?.file != Some(guard.file)
                || store.runtime_binding(r.owner.id)?.open_counter != guard.open
            {
                return Err(RuntimeError::StaleIncarnation);
            }
        } else if !b.usable
            || !b.verified
            || s.adapter.known_loss.is_some()
            || s.adapter.durable.collection_loss.is_some()
        {
            return Err(RuntimeError::IdentityUnverified);
        }
        Ok(())
    }

    fn with_incoming<T>(
        &self,
        guard: &IncomingGuard,
        collection: &Collection,
        work: impl FnOnce(&mut State) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        let Some(r) = &guard.recovery else {
            return self.with_scan(&guard.scan, collection, |s| {
                Self::check_incoming_locked(s, guard)?;
                work(s)
            });
        };
        // Never call account/file providers under a writer or tracking lock.
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        if account != r.owner.facts.account
            || file != guard.file
            || file.created_nanos >= 1_000_000_000
            || decision != r.decision
            || !collection.is_current()
            || collection.generation() != guard.generation
            || collection.open_evidence()?.native_file != file.pair()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let mut s = self.state();
        with_tracking_access(|| {
            Self::check_incoming_locked(&s, guard)?;
            work(&mut s)
        })
    }
    pub fn apply_intents(&self, binding: BindingId) -> Result<Vec<ApplyIntent>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.apply_intents(binding)?))
    }
    pub fn inbox_entries(&self, binding: BindingId) -> Result<Vec<StoredInboxEntry>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.inbox(binding)?))
    }
    #[cfg(feature = "test-hooks")]
    pub fn set_inbox_reason_for_testing(
        &self,
        binding: BindingId,
        id: i64,
        reason: InboxReason,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        with_tracking_access(|| {
            Ok(s.store
                .as_mut()
                .ok_or(Error::Database)?
                .park_incoming(binding, id, reason)?)
        })
    }
    pub fn refused_entries(&self, binding: BindingId) -> Result<Vec<RefusedEntry>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.refused_entries(binding)?))
    }
    pub fn binding_entity(
        &self,
        binding: BindingId,
        name: &str,
    ) -> Result<Option<workflow::EntityFacts>, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.entity_dump(binding, name)?))
    }
    pub fn incoming_runs(&self) -> Vec<ApplyRun> {
        self.state().incoming.runs.clone()
    }
    pub fn incoming_wal_receipts(&self) -> Vec<WalReceipt> {
        self.state().incoming.receipts.clone()
    }

    /// Fixed-role reader support. Inputs come from the caller's held native
    /// snapshot, never a command or journal. No work is advanced by this read.
    #[cfg(feature = "test-hooks")]
    pub fn original_note_observation_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        identity: meguri_engine::WalIdentity,
        seq: Seq,
        digest: [u8; 32],
    ) -> Result<(workflow::EntityFacts, Option<u64>), RuntimeError> {
        self.original_note_observation_with_recheck_for_testing(collection, id, identity, seq, digest, || {})
    }

    /// The bounded test callback runs after the actual read, with neither
    /// State nor tracking ownership. It supplies no observation or authority.
    #[cfg(feature = "test-hooks")]
    pub fn original_note_observation_with_recheck_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        identity: meguri_engine::WalIdentity,
        seq: Seq,
        digest: [u8; 32],
        after_read: impl FnOnce(),
    ) -> Result<(workflow::EntityFacts, Option<u64>), RuntimeError> {
        // Record only predicates already reached by the original short circuit.
        fn mismatch(mask: &mut u16, bit: u32, value: bool) -> bool {
            if value {
                *mask |= 1 << bit;
            }
            value
        }
        let mut phase = "initial.account";
        let mut mismatch_mask = 0u16;
        let observation = (|| {
            let account = self.publication_account()?;
            phase = "initial.decision";
            let decision = self.publication_decision()?;
            phase = "initial.openEvidence";
            let evidence = collection.open_evidence()?;
            let guard = {
                let s = self.state();
                phase = "initial.binding";
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                phase = "initial.accountTracking";
                if mismatch(&mut mismatch_mask, 0, account != b.binding.facts.account)
                    || mismatch(&mut mismatch_mask, 1, s.tracking_stopped)
                {
                    return Err(RuntimeError::StaleIncarnation);
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
            phase = "initial.withScanAuthority";
            let (result, binding) = self.with_scan(&guard, collection, |s| {
                phase = "initial.capturedWAL";
                if mismatch(&mut mismatch_mask, 0, identity.generation() != guard.generation)
                    || mismatch(&mut mismatch_mask, 1, identity.native_file() != guard.file.pair())
                    || mismatch(&mut mismatch_mask, 2, collection.last_seq() != seq)
                    || mismatch(&mut mismatch_mask, 3, s.tracking_stopped)
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                phase = "initial.store";
                let store = s.store.as_ref().ok_or(Error::Database)?;
                phase = "initial.bound";
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                phase = "initial.runtimeBindingRead";
                let runtime = store.runtime_binding(guard.binding)?;
                phase = "initial.bindingOpen";
                if mismatch(
                    &mut mismatch_mask,
                    0,
                    decision.publication.identity != b.binding.facts.publication,
                ) || mismatch(
                    &mut mismatch_mask,
                    1,
                    decision.publication.decision != meguri_engine::device_state::PublicationDecision::Clear,
                ) || mismatch(&mut mismatch_mask, 2, evidence != b.evidence)
                    || mismatch(
                        &mut mismatch_mask,
                        3,
                        store.binding(guard.binding)?.as_ref() != Some(&b.binding),
                    )
                    || mismatch(&mut mismatch_mask, 4, runtime.file != Some(guard.file))
                    || mismatch(&mut mismatch_mask, 5, runtime.open_counter != guard.open)
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                phase = "entity.name";
                let name = codec::entity_name(&b.binding.facts.record_key.0, codec::Kind::Note, &codec::id_key(id))
                    .map_err(|_| Error::InvalidData)?;
                phase = "entity.read";
                let entity = store.entity_dump(guard.binding, &name)?;
                phase = "entity.missing";
                let entity = entity.ok_or(Error::InvalidData)?;
                phase = "entity.exact";
                if mismatch(&mut mismatch_mask, 0, entity.kind != codec::Kind::Note)
                    || mismatch(&mut mismatch_mask, 1, entity.key != codec::id_key(id))
                    || mismatch(&mut mismatch_mask, 2, entity.tombstone_at.is_some())
                    || mismatch(&mut mismatch_mask, 3, entity.observer_open != guard.open)
                    || mismatch(&mut mismatch_mask, 4, entity.observed_generation != guard.generation.0)
                    || mismatch(&mut mismatch_mask, 5, entity.observed_seq > seq.0)
                    || mismatch(
                        &mut mismatch_mask,
                        6,
                        entity.payload.payload_digest().map_err(|_| Error::InvalidData)? != digest,
                    )
                {
                    return Err(Error::InvalidData.into());
                }
                phase = "tracking.applyIntents";
                let intents = store.apply_intents(guard.binding)?;
                phase = "tracking.inbox";
                let inbox = store.inbox(guard.binding)?;
                let applied = s.incoming.runs.iter().rev().find(|run| {
                    !s.incoming.running
                        && run.native_file == guard.file
                        && run.generation == guard.generation.0
                        && run.seq == entity.observed_seq
                        && run.seq <= seq.0
                        && !run.entry_ids.is_empty()
                        && run.changed_keys.contains(&name)
                        && !intents.iter().any(|i| i.id == run.intent_id)
                        && !inbox.iter().any(|e| run.entry_ids.contains(&e.id))
                        && s.incoming
                            .receipts
                            .iter()
                            .any(|r| r.identity() == identity && r.through().0 >= run.seq)
                });
                Ok(((entity, applied.map(|r| r.seq)), b.binding.clone()))
            })?;
            after_read();
            // Provider reads precede the final State/tracking acquisition. The
            // second with_scan rereads the actual file and publication, then pins
            // the same invalidation/incarnation/open/generation under State.
            phase = "final.account";
            let final_account = self.publication_account()?;
            phase = "final.decision";
            let final_decision = self.publication_decision()?;
            phase = "final.providers";
            if mismatch(&mut mismatch_mask, 0, final_account != account)
                || mismatch(&mut mismatch_mask, 1, final_decision != decision)
                || mismatch(&mut mismatch_mask, 2, !collection.is_current())
                || mismatch(&mut mismatch_mask, 3, collection.open_evidence()? != evidence)
                || mismatch(&mut mismatch_mask, 4, collection.last_seq() != seq)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            phase = "final.withScanAuthority";
            self.with_scan(&guard, collection, |s| {
                phase = "final.store";
                let store = s.store.as_ref().ok_or(Error::Database)?;
                phase = "final.bound";
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                phase = "final.runtimeBindingRead";
                let runtime = store.runtime_binding(guard.binding)?;
                phase = "final.bindingOpen";
                if mismatch(&mut mismatch_mask, 0, s.tracking_stopped)
                    || mismatch(&mut mismatch_mask, 1, b.binding != binding)
                    || mismatch(
                        &mut mismatch_mask,
                        2,
                        store.binding(guard.binding)?.as_ref() != Some(&binding),
                    )
                    || mismatch(&mut mismatch_mask, 3, runtime.file != Some(guard.file))
                    || mismatch(&mut mismatch_mask, 4, runtime.open_counter != guard.open)
                    || mismatch(&mut mismatch_mask, 5, collection.last_seq() != seq)
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                // Scan progress, feed delivery and ACK bookkeeping are not read
                // authority and need not remain frozen across this observation.
                Ok(result)
            })
        })();
        observation.inspect_err(|error: &RuntimeError| {
            let (kind, variant) = error.refusal_tags_for_testing();
            eprintln!(
                "cloudOriginal.refused phase={phase} errorKind={kind} variant={variant} mismatchMask={mismatch_mask}"
            );
        })
    }

    /// Non-SDK transport boundary. It persists the very same facts as adapter_fetched.
    pub fn receive_collection_records(&self, instance: &str, records: &[Saved]) -> Result<(), RuntimeError> {
        let at = self.adapter_now()?;
        let (binding, guard) = {
            let s = self.state();
            let guard = Self::control_intake_guard_locked(&s, instance)?;
            (s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone(), guard)
        };
        let entities: Vec<_> = records.iter().filter(|r| r.name != "epoch").cloned().collect();
        let (entries, refused) = classify(&binding.facts.record_key.0, &entities, at);
        let fetched_epoch = super::super::epoch_crt_store::fetched(&binding, records);
        let epoch_readability = super::super::root_readability_store::epoch_observation(&binding, records, &[])?;
        meguri_model::faultpoint!("cloud.fetch.before_inbox").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(super::super::testing::IncomingStage::FetchBeforeInbox);
        self.with_scan_file(&guard, &self.directory.join("collection.anki2"), |s| {
            if Self::control_intake_guard_locked(s, instance)? != guard {
                return Err(RuntimeError::StaleIncarnation);
            }
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            let result = with_tracking_access(|| {
                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .receive_records(b, &entries, &refused, fetched_epoch.as_ref(), epoch_readability)?;
                    Self::refresh_epoch_crt_locked(s)
                }))
                .unwrap_or(Err(RuntimeError::Tracking(Error::Database)))
            });
            if result.is_err() {
                Self::invalidate_locked(s);
            }
            result?;
            s.incoming.retry_granted = true;
            Ok(())
        })?;
        meguri_model::faultpoint!("cloud.fetch.after_inbox").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(super::super::testing::IncomingStage::FetchAfterInbox);
        Ok(())
    }

    /// Process-local diagnostics retain the writer error without serializing or
    /// logging collection values. Native event handling does not throw it.
    pub fn incoming_deferred_error(&self) -> Option<EngineError> {
        self.state().incoming.last_deferred_error.clone()
    }

    /// Bounded work grant, called by an actual fetch/event trigger, never a timer.
    /// Collection is borrowed per call; Runtime owns no collection handle.
    pub fn drain_incoming(&self, collection: &Collection, timeout: Duration) -> Result<usize, RuntimeError> {
        self.drain_incoming_bounded(collection, timeout, None)?.into_result()
    }

    /// Native continuation consumes no new receive/retry grant. Every bounded
    /// operation pins and rechecks the exact SDK as well as collection authority.
    pub fn drain_native_incoming(
        &self,
        collection: &Collection,
        sdk: &str,
        timeout: Duration,
    ) -> Result<IncomingDisposition, RuntimeError> {
        self.drain_incoming_bounded(collection, timeout, Some(sdk))
    }

    fn drain_incoming_bounded(
        &self,
        collection: &Collection,
        timeout: Duration,
        sdk: Option<&str>,
    ) -> Result<IncomingDisposition, RuntimeError> {
        self.drain_incoming_bounded_with_work(
            collection,
            timeout,
            sdk,
            &mut crate::cloud::policy::PolicyWork::default(),
        )
    }
    pub(crate) fn drain_native_incoming_with_work(
        &self,
        collection: &Collection,
        sdk: &str,
        timeout: Duration,
        work: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<IncomingDisposition, RuntimeError> {
        self.drain_incoming_bounded_with_work(collection, timeout, Some(sdk), work)
    }
    fn drain_incoming_bounded_with_work(
        &self,
        collection: &Collection,
        timeout: Duration,
        sdk: Option<&str>,
        work: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<IncomingDisposition, RuntimeError> {
        self.refresh_identity_with_work(work)?;
        if let Ok(instance) = self.instance() {
            self.poll_review_with_work(&instance, work)?;
        }
        let guard = {
            let mut s = self.state();
            if s.incoming.running {
                return Err(EngineError::Busy {
                    running_op: "cloud incoming".into(),
                }
                .into());
            }
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if !b.verified || !b.usable {
                return Err(RuntimeError::IdentityUnverified);
            }
            let held_for_transition = with_tracking_access(|| -> Result<bool, Error> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                Ok(store
                    .publication_op()?
                    .as_ref()
                    .and_then(|p| p.binding_before.as_ref())
                    .is_some_and(|p| p.id == b.binding.id)
                    || store
                        .adoption_op()?
                        .as_ref()
                        .and_then(|p| p.binding_before.as_ref())
                        .is_some_and(|p| p.id == b.binding.id))
            })?;
            if held_for_transition {
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
            work.effect()?;
            s.incoming.running = true;
            g
        };
        let guard = IncomingGuard {
            scan: guard,
            recovery: None,
            sdk: sdk.map(str::to_owned),
        };
        let _running = Running(self.state.clone(), self.incoming_idle.clone());
        self.replay_media_intents(collection, &self.media_service(collection)?)?;
        if self.state().bound.as_ref().is_some_and(Bound::crt_equal) {
            self.drain_media_metadata(collection, &self.media_service(collection)?)?;
        }
        self.with_incoming(&guard, collection, |_| Ok(()))?;
        let replayed = match self.replay_ordinary(collection, &guard, timeout)? {
            IncomingDisposition::Progress(count) => count,
            deferred @ IncomingDisposition::Deferred(_) => return Ok(deferred),
        };
        // A bounded op can park every selected entry without applying a row.
        // Continue selecting here until an apply completes or no eligible work
        // remains; zero applied entries is never a native continuation signal.
        let mut parked = false;
        loop {
            if !self.state().bound.as_ref().is_some_and(Bound::crt_equal) {
                return Ok(IncomingDisposition::Progress(0));
            }
            let (entries, retry_granted, failed_this_open) = self.with_incoming(&guard, collection, |s| {
                Ok((
                    s.store.as_ref().ok_or(Error::Database)?.inbox(guard.binding)?,
                    s.incoming.retry_granted,
                    s.incoming.failed_this_open.clone(),
                ))
            })?;
            let references = self.eligible_incoming_references(collection, &guard, &entries)?;
            let retry = entries
                .iter()
                .find(|e| {
                    retry_granted
                        && matches!(e.entry.reason, InboxReason::ApplyFailed { .. })
                        && (e.entry.retry_count < 5 || !failed_this_open.contains(&e.id))
                })
                .cloned();
            let mut entries: Vec<_> = entries
                .into_iter()
                .filter(|e| (matches!(e.entry.reason, InboxReason::Ready) || references.contains(&e.id))
                    && !matches!(&e.entry.value, InboxValue::Record { envelope, .. } if matches!(codec::Entity::decode(&envelope.0), Ok(codec::DecodeOutcome::Supported(v)) if v.fields.kind == codec::Kind::MediaName)))
                .collect();
            entries.sort_by_key(|e| {
                let entity = match &e.entry.value {
                    InboxValue::Record { envelope, .. } => codec::Entity::decode(&envelope.0).ok(),
                    _ => None,
                };
                if let Some(codec::DecodeOutcome::Supported(v)) = entity {
                    let order = match v.fields.kind {
                        codec::Kind::Preset => 0,
                        codec::Kind::Deck => 1,
                        codec::Kind::Notetype => 2,
                        codec::Kind::Note => 3,
                        codec::Kind::Card => 4,
                        codec::Kind::Revlog => 5,
                        codec::Kind::Tag => 6,
                        codec::Kind::Config => 7,
                        _ => 8,
                    };
                    (v.fields.tombstone, order, e.entry.record_name.clone(), v.fields.erev)
                } else {
                    (true, 9, e.entry.record_name.clone(), 0)
                }
            });
            entries.truncate(500);
            if entries.is_empty()
                && let Some(entry) = retry
            {
                entries.push(entry);
            }
            if entries.is_empty() {
                if replayed > 0 || parked {
                    self.finish_incoming(_running)?;
                }
                return Ok(IncomingDisposition::Progress(0));
            }
            self.with_incoming(&guard, collection, |s| {
                s.incoming.retry_granted = false;
                for e in &entries {
                    if matches!(e.entry.reason, InboxReason::ApplyFailed { .. }) {
                        s.incoming.failed_this_open.insert(e.id);
                    }
                }
                Ok(())
            })?;
            // Select at most one version of each name. A protected older intent is
            // replayed above before a newer version can be considered here.
            let mut names = BTreeSet::new();
            entries.retain(|e| names.insert(e.entry.record_name.clone()));
            let disposition = self.apply_ordinary_entries(collection, &guard, entries, None, timeout)?;
            if matches!(disposition, IncomingDisposition::Progress(0)) {
                parked = true;
                continue;
            }
            if matches!(disposition, IncomingDisposition::Progress(_)) {
                self.finish_incoming(_running)?;
            }
            return Ok(disposition);
        }
    }

    fn eligible_incoming_references(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        entries: &[StoredInboxEntry],
    ) -> Result<BTreeSet<i64>, RuntimeError> {
        if !entries.iter().any(|e| matches!(e.entry.reason, InboxReason::Reference)) {
            return Ok(BTreeSet::new());
        }
        let (key, layouts) = self.with_incoming(guard, collection, |s| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            Ok((
                store
                    .binding(guard.binding)?
                    .ok_or(Error::BindingNotFound)?
                    .facts
                    .record_key
                    .0,
                layouts(store, guard.binding)?,
            ))
        })?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut eligible = BTreeSet::new();
        for entry in entries
            .iter()
            .filter(|e| matches!(e.entry.reason, InboxReason::Reference))
        {
            let InboxValue::Record {
                envelope, staged_asset, ..
            } = &entry.entry.value
            else {
                continue;
            };
            let received = match super::super::payload_asset::receive_owned(
                &self.directory,
                guard.binding,
                &key,
                &entry.entry.record_name,
                &envelope.0,
                staged_asset.as_deref(),
            ) {
                Ok(received) => received,
                Err(codec::ReceiveError::RetryableAsset(_)) => continue,
                Err(_) => return Err(Error::InvalidData.into()),
            };
            if let Some(payload) = received.payload
                && references(&*snapshot, &payload, &layouts)?
            {
                eligible.insert(entry.id);
            }
        }
        // Eligibility is advisory. The journalled writer checks references and
        // the exact SDK/collection authority again before any apply or commit.
        Ok(eligible)
    }

    fn apply_ordinary_entries(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        entries: Vec<StoredInboxEntry>,
        replay_of: Option<i64>,
        timeout: Duration,
    ) -> Result<IncomingDisposition, RuntimeError> {
        collection.check_rollover()?;
        self.with_incoming(guard, collection, |_| Ok(()))?;
        let (entries, rekey) = if replay_of.is_none() {
            if let Some(index) = self.select_rekey(collection, guard, &entries)? {
                (vec![entries.into_iter().nth(index).ok_or(Error::InvalidData)?], true)
            } else {
                (entries, false)
            }
        } else {
            (entries, false)
        };
        let shared = self.state.clone();
        #[cfg(feature = "test-hooks")]
        let hooks = self.incoming_hooks.clone();
        let file = guard.file;
        let generation = guard.generation;
        let binding = guard.binding;
        let writer_guard = guard.clone();
        let directory = self.directory.clone();
        let attempted: Vec<_> = entries.iter().map(|e| e.id).collect();
        // The writer transports EngineError only. Retain typed tracking/guard
        // failures before that conversion so they can never become Deferred.
        let fatal = Arc::new(Mutex::new(None));
        let writer_fatal = fatal.clone();
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks.hit(super::super::testing::IncomingStage::ApplyBeforeWriter);
        let result = collection.run(
            meguri_engine::OpSpec::named(
                "cloud_incoming_apply",
                if rekey {
                    meguri_engine::OpSource::CloudRekey
                } else {
                    meguri_engine::OpSource::CloudIncoming
                },
            ),
            move |ctx| {
                if ctx.generation() != generation {
                    return Err(engine_error("incoming generation"));
                }
                let capture = |error: RuntimeError| {
                    let mapped = match &error {
                        RuntimeError::Engine(error) => error.clone(),
                        _ => engine_error(&error),
                    };
                    if !matches!(error, RuntimeError::Engine(_)) {
                        *writer_fatal.lock().unwrap_or_else(PoisonError::into_inner) = Some(error);
                    }
                    mapped
                };
                let applied = {
                    let crt = ctx.scope().col()?.created.0;
                    let mut s = StateGuard::new(shared.lock().unwrap_or_else(PoisonError::into_inner));
                    with_tracking_access(|| {
                        Self::check_incoming_locked(&s, &writer_guard)?;
                        if !Self::writer_crt_equal_locked(&s, binding, crt)? {
                            return Ok(None);
                        }
                        if rekey {
                            rekey_impl::apply_rekey(ctx, &mut s, &writer_guard, &directory, entries)
                        } else {
                            apply(ctx, &mut s, &writer_guard, &directory, entries, replay_of)
                        }
                    })
                    .map_err(capture)?
                };
                // Still inside the journalled writer, before collection COMMIT,
                // but neither runtime state nor TRACKING_ACCESS is held.
                if applied.is_some() {
                    meguri_model::faultpoint!("cloud.apply.after_intent")
                        .map_err(Error::from)
                        .map_err(engine_error)?;
                    #[cfg(feature = "test-hooks")]
                    {
                        hooks.hit(super::super::testing::IncomingStage::ApplyAfterIntent);
                        hooks.interrupt(ctx)?;
                    }
                }
                // A callback may have invalidated the opening while paused.
                let crt = ctx.scope().col()?.created.0;
                let s = StateGuard::new(shared.lock().unwrap_or_else(PoisonError::into_inner));
                let equal = with_tracking_access(|| {
                    Self::check_incoming_locked(&s, &writer_guard)?;
                    Self::writer_crt_equal_locked(&s, binding, crt)
                }).map_err(capture)?;
                if applied.is_some() && !equal {
                    return Err(EngineError::Cancelled);
                }
                let targets: BTreeSet<_> = ctx
                    .scope()
                    .journal()
                    .into_iter()
                    .flat_map(|journal| journal.entries())
                    .filter(|entry| entry.table() != Table::Col)
                    .map(|entry| (entry.table(), entry.key().clone()))
                    .collect();
                Ok((applied, targets))
            },
        );
        let result = match result {
            Ok(result) => result,
            Err(EngineError::CommitCompleted { seq, .. }) => {
                // The observer already records this real seq when a post-commit
                // error prevented delivery of OpResult. Keep the intent for replay.
                let _ = seq;
                return Err(engine_error("incoming commit completed").into());
            }
            Err(error) => {
                if self.state().tracking_stopped {
                    return Err(error.into());
                }
                let cancel = matches!(error, EngineError::Cancelled);
                let rollback = self.with_incoming(guard, collection, |s| {
                    let rollback = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                        let store = s.store.as_mut().ok_or(Error::Database)?;
                        if rekey {
                            store.rollback_apply_kind(binding, &attempted, cancel, ApplyFailure::Rekey)
                        } else {
                            store.rollback_apply(binding, &attempted, cancel)
                        }
                    }))
                    .unwrap_or(Err(Error::Database));
                    if rollback.is_err() {
                        // Close the gate under the existing tracking lock.
                        // Full invalidation runs after that lock is released.
                        s.tracking_stopped = true;
                        s.scan_required = true;
                        s.whole_complete = false;
                    }
                    Ok(rollback)
                })?;
                if rollback.is_err() {
                    Self::invalidate_locked(&mut self.state());
                }
                rollback?;
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(super::super::testing::IncomingStage::IncomingRollbackComplete);
                if let Some(fatal) = fatal.lock().unwrap_or_else(PoisonError::into_inner).take() {
                    // Existing non-native callers keep the writer's error
                    // conversion; native callers retain the typed fatal cause.
                    return Err(if guard.sdk.is_some() { fatal } else { error.into() });
                }
                self.with_incoming(guard, collection, |s| {
                    s.incoming.last_deferred_error = Some(error.clone());
                    Ok(())
                })?;
                return Ok(IncomingDisposition::Deferred(error));
            }
        };
        let (Some(intent_id), targets) = result.value else {
            return Ok(IncomingDisposition::Progress(0));
        };
        if let Ok(instance) = self.instance() {
            self.poll_review(&instance)?;
        }
        let active = self.active_review();
        self.with_incoming(guard, collection, |s| {
            s.cloud.incoming_seq = result.seq.0;
            if active.is_some()
                && let Some(run) = result.cloud_detached_run
                && run.steps.iter().any(|step| step.session.map(|s| s.0) == active)
            {
                // Retain the actual op result before any fallible tracking
                // write. Only the corresponding real session end releases it.
                s.last_session = active;
                s.cloud.detached.push(run);
            }
            let intent = s
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .apply_intents(binding)?
                .into_iter()
                .find(|i| i.id == intent_id)
                .ok_or(Error::InvalidData)?;
            if active.is_some() && s.last_session == active {
                for key in &intent.keys {
                    let table = row_payload::table(key.kind).ok_or(Error::InvalidData)?;
                    let row_key = super::super::workflow_store::row_key(key.kind, &key.key)?;
                    if targets.contains(&(table, row_key.clone()))
                        && s.cloud.detached.iter().any(|run| run.touches(table, &row_key))
                    {
                        s.cloud.incoming_held.insert(key.name.clone());
                    }
                }
            }
            if let Some(rekey) =
                crate::cloud::rekey_store::get(&s.store.as_ref().ok_or(Error::Database)?.connection, intent_id)?
                && s.cloud.incoming_held.contains(&rekey.old_name)
            {
                s.cloud.incoming_held.insert(rekey.new_name);
            }
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_apply_seq(binding, intent_id, result.seq.0)?;
            s.incoming.runs.push(ApplyRun {
                intent_id,
                seq: result.seq.0,
                native_file: file,
                generation: generation.0,
                entry_ids: intent.inbox_ids.clone(),
                changed_keys: intent
                    .keys
                    .iter()
                    .filter(|k| k.before_digest != k.after_digest)
                    .map(|k| k.name.clone())
                    .collect(),
            });
            Ok(())
        })?;
        meguri_model::faultpoint!("cloud.apply.after_commit").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(super::super::testing::IncomingStage::ApplyAfterCommit);
        self.settle_ordinary(collection, guard, intent_id, timeout)?;
        if rekey {
            let entries = self.with_incoming(guard, collection, |s| {
                Ok(s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .inbox(binding)?
                    .into_iter()
                    .filter(|e| attempted.contains(&e.id))
                    .collect::<Vec<_>>())
            })?;
            return self.apply_ordinary_entries(collection, guard, entries, None, timeout);
        }
        Ok(IncomingDisposition::Progress(attempted.len()))
    }

    fn settle_ordinary(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        id: i64,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        let i = self.with_incoming(guard, collection, |s| {
            s.store
                .as_ref()
                .ok_or(Error::Database)?
                .apply_intents(guard.binding)?
                .into_iter()
                .find(|i| i.id == id)
                .ok_or(Error::InvalidData.into())
        })?;
        if let Some(rekey) = self.with_incoming(guard, collection, |s| {
            Ok(crate::cloud::rekey_store::get(
                &s.store.as_ref().ok_or(Error::Database)?.connection,
                i.id,
            )?)
        })? {
            return self.settle_identity(collection, guard, &i, &rekey, timeout);
        }
        let (current, seq, identity) = self.read_intent_rows(collection, &i)?;
        // The snapshot/read lease has been released; no caller waits under it,
        // the writer, runtime state or the tracking gate. Seq belongs to this
        // opening, never the intent's prior-process result counter.
        let deadline = meguri_engine::monotonic::now().saturating_add(timeout);
        collection.flush_wal(deadline.saturating_sub(meguri_engine::monotonic::now()))?;
        let receipt = match collection.wait_wal_covered(
            identity,
            seq,
            deadline.saturating_sub(meguri_engine::monotonic::now()),
        )? {
            WalCoverage::Covered(r)
                if r.identity() == identity
                    && r.through() >= seq
                    && identity.native_file() == guard.file.pair()
                    && identity.generation() == guard.generation =>
            {
                r
            }
            _ => return Err(engine_error("incoming WAL not covered").into()),
        };
        self.with_incoming(guard, collection, |s| {
            s.incoming.receipts.push(receipt);
            Ok(())
        })?;
        meguri_model::faultpoint!("cloud.apply.after_flush").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(super::super::testing::IncomingStage::ApplyAfterFlush);
        self.with_incoming(guard, collection, |s| {
            if collection.last_seq() != seq {
                return Err(EngineError::Busy {
                    running_op: "cloud incoming".into(),
                }
                .into());
            }
            let store = s.store.as_mut().ok_or(Error::Database)?;
            let mut own = BTreeMap::new();
            for key in &i.keys {
                if let Some(rev) = key.own_flight {
                    let flights = store
                        .entity_header(i.binding, &key.name)?
                        .ok_or(Error::InvalidData)?
                        .facts
                        .in_flight;
                    own.insert(key.name.clone(), (rev, flights));
                }
            }
            store.settle_apply(
                &i,
                guard.open,
                seq.0,
                guard.generation.0,
                &current,
                guard.recovery.is_none(),
            )?;
            for key in &i.keys {
                if key.role != ApplyKeyRole::Entry {
                    continue;
                }
                if store.incoming_blocks_name(i.binding, &key.name)? {
                    // Rekey is pending; this entry grants no ACK or image update.
                    continue;
                }
                s.cloud.acked_images.remove(&key.name);
                if let Some((rev, flights)) = own.get(&key.name) {
                    let e = store
                        .entity_header(i.binding, &key.name)?
                        .ok_or(Error::InvalidData)?
                        .facts;
                    let landed = flights.iter().find(|flight| {
                        flight.local_rev == *rev
                            && flight.erev == e.server_erev
                            && flight.payload_digest == e.acked_payload_digest
                            && flight
                                .tombstone_envelope_digest
                                .is_none_or(|digest| digest == codec::sha256(&e.acked_envelope.0))
                    });
                    if let Some(image) = landed.and_then(|flight| s.cloud.flight_image(i.binding, &key.name, flight))
                        && guard.recovery.is_none()
                        && let Some((_, exchange)) = &mut s.actions.exchange
                    {
                        exchange.record_ack(&key.name, image);
                        #[cfg(feature = "test-hooks")]
                        if s.actions.smoke_acks.enabled
                            && let Some(sdk) = &guard.sdk
                        {
                            let acknowledgements = &mut s.actions.smoke_acks;
                            if acknowledgements.fetched.len() == 4096 {
                                acknowledgements.overflow = true;
                            } else {
                                acknowledgements
                                    .fetched
                                    .push(super::super::workflow::SmokeHistoricalAck {
                                        sdk: sdk.clone(),
                                        name: key.name.clone(),
                                        envelope_digest: codec::sha256(&e.acked_envelope.0),
                                        image_seq: image.seq,
                                        held: image.held,
                                    });
                            }
                        }
                    }
                    if let Some(image) = flights
                        .iter()
                        .filter(|f| f.local_rev == *rev && f.payload_digest == e.acked_payload_digest)
                        .filter_map(|f| {
                            s.cloud
                                .handed_images
                                .get(&(key.name.clone(), f.erev, f.payload_digest))
                                .copied()
                        })
                        .max()
                    {
                        s.cloud.acked_images.insert(key.name.clone(), image);
                    }
                    s.cloud
                        .handed_images
                        .retain(|(name, erev, _), _| name != &key.name || *erev > e.server_erev);
                    s.cloud.retire_flights(&key.name, e.server_erev);
                } else {
                    s.cloud.handed_images.retain(|(name, _, _), _| name != &key.name);
                    s.cloud.open_flights.remove(&key.name);
                }
            }
            if guard.recovery.is_none() {
                Self::settle_cloud_uploaded_locked(s, collection)?;
            }
            Ok(())
        })?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(super::super::testing::IncomingStage::IntentSettlementComplete);
        Ok(())
    }

    fn read_intent_rows(&self, collection: &Collection, i: &ApplyIntent) -> Result<IntentRows, RuntimeError> {
        let layouts = {
            let s = self.state();
            with_tracking_access(|| layouts(s.store.as_ref().ok_or(Error::Database)?, i.binding))?
        };
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut current = BTreeMap::new();
        for k in &i.keys {
            current.insert(k.name.clone(), project(&*snapshot, k.kind, &k.key, &layouts)?);
        }
        Ok((current, snapshot.seq(), snapshot.wal_identity()))
    }

    fn replay_ordinary(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        timeout: Duration,
    ) -> Result<IncomingDisposition, RuntimeError> {
        let intents = self.with_incoming(guard, collection, |s| {
            Ok(s.store.as_ref().ok_or(Error::Database)?.apply_intents(guard.binding)?)
        })?;
        let count = intents.len();
        for i in intents {
            if i.media.is_some() {
                continue;
            }
            collection.check_rollover()?;
            if i.native_file != guard.file {
                return Err(RuntimeError::FileIdentityUnavailable);
            }
            if let Some(rekey) = self.with_incoming(guard, collection, |s| {
                Ok(crate::cloud::rekey_store::get(
                    &s.store.as_ref().ok_or(Error::Database)?.connection,
                    i.id,
                )?)
            })? {
                if let deferred @ IncomingDisposition::Deferred(_) =
                    self.replay_identity(collection, guard, &i, &rekey, timeout)?
                {
                    return Ok(deferred);
                }
                continue;
            }
            let (current, current_seq, _) = self.read_intent_rows(collection, &i)?;
            let all_after = i
                .keys
                .iter()
                .all(|k| digest(current.get(&k.name).and_then(Option::as_ref)).ok() == Some(k.after_digest));
            let same_open_commit = self.with_incoming(guard, collection, |s| {
                Ok(i.originating_open == Some(guard.open)
                    && i.result_seq.is_some_and(|seq| {
                        current_seq.0 > seq
                            && s.incoming.runs.iter().any(|run| {
                                run.intent_id == i.id
                                    && run.seq == seq
                                    && run.native_file == guard.file
                                    && run.generation == guard.generation.0
                                    && run.entry_ids == i.inbox_ids
                            })
                    }))
            })?;
            let restored_deletion = if !all_after && same_open_commit {
                let tombstones = self.with_incoming(guard, collection, |s| {
                    Ok(s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .inbox(guard.binding)?
                        .into_iter()
                        .filter(|e| i.inbox_ids.contains(&e.id))
                        .filter_map(|e| match e.entry.value {
                            InboxValue::Record { envelope, .. } => match codec::Entity::decode(&envelope.0) {
                                Ok(codec::DecodeOutcome::Supported(v))
                                    if v.fields.tombstone
                                        && matches!(v.fields.kind, codec::Kind::Note | codec::Kind::Card) =>
                                {
                                    Some(e.entry.record_name)
                                }
                                _ => None,
                            },
                            _ => None,
                        })
                        .collect::<BTreeSet<_>>())
                })?;
                // This opening delivered the entire op. Each mismatch needs
                // its own newer matching observer image; a restored deletion
                // cannot supply evidence for an unrelated locally edited key.
                self.with_incoming(guard, collection, |s| {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    let mut restored = false;
                    for k in &i.keys {
                        let local = current.get(&k.name).and_then(Option::as_ref);
                        if digest(local)? == k.after_digest {
                            continue;
                        }
                        if local.is_none() {
                            return Ok(false);
                        }
                        if k.after_digest.is_none() {
                            if tombstones.is_empty()
                                || !matches!(k.kind, codec::Kind::Note | codec::Kind::Card)
                                || !(k.role == ApplyKeyRole::SideEffect || tombstones.contains(&k.name))
                            {
                                return Ok(false);
                            }
                            restored = true;
                        }
                        let Some(e) = store.entity_facts(i.binding, &k.name)? else {
                            return Ok(false);
                        };
                        if e.observer_open != guard.open
                            || e.observed_generation != guard.generation.0
                            || e.observed_seq <= i.result_seq.ok_or(Error::InvalidData)?
                            || e.observed_seq > current_seq.0
                            || e.tombstone_at.is_some()
                            || digest(Some(&e.payload))? != digest(local)?
                        {
                            return Ok(false);
                        }
                    }
                    Ok(restored)
                })?
            } else {
                false
            };
            if all_after || restored_deletion {
                // A committed deletion followed by Undo needs fresh WAL coverage
                // and parked rekey evidence, never a replay of the deletion.
                self.settle_ordinary(collection, guard, i.id, timeout)?;
            } else {
                if !self.state().bound.as_ref().is_some_and(Bound::crt_equal) {
                    continue;
                }
                // Retain the original inbox and acknowledged base. The next
                // writer derives L afresh, so a local edit after a crash wins
                // only its changed parts rather than being overwritten.
                let all_before = i
                    .keys
                    .iter()
                    .all(|k| digest(current.get(&k.name).and_then(Option::as_ref)).ok() == Some(k.before_digest));
                if !all_before {
                    self.with_incoming(guard, collection, |s| {
                        Ok(s.store.as_mut().ok_or(Error::Database)?.reconcile_apply_keys(
                            &i,
                            guard.open,
                            current_seq.0,
                            guard.generation.0,
                            &current,
                            guard.recovery.is_none(),
                        )?)
                    })?;
                }
                let entries = self.with_incoming(guard, collection, |s| {
                    let entries = s.store.as_ref().ok_or(Error::Database)?.inbox(guard.binding)?;
                    let mut by_id: BTreeMap<_, _> = entries.into_iter().map(|entry| (entry.id, entry)).collect();
                    i.inbox_ids
                        .iter()
                        .map(|id| by_id.remove(id).ok_or(Error::InvalidData.into()))
                        .collect()
                })?;
                if let deferred @ IncomingDisposition::Deferred(_) =
                    self.apply_ordinary_entries(collection, guard, entries, Some(i.id), timeout)?
                {
                    return Ok(deferred);
                }
                self.with_incoming(guard, collection, |s| {
                    if s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .apply_intents(guard.binding)?
                        .iter()
                        .any(|pending| pending.id == i.id)
                    {
                        return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
                    }
                    Ok(())
                })?;
            }
        }
        Ok(IncomingDisposition::Progress(count))
    }
}

fn layouts(store: &TrackingStore, b: BindingId) -> Result<BTreeMap<i64, codec::Notetype>, Error> {
    let mut layouts = BTreeMap::new();
    let mut after = None;
    loop {
        let page = store.entity_keys_page(b, codec::Kind::Notetype, after.as_deref())?;
        if page.is_empty() {
            break;
        }
        for key in page {
            let header = store
                .entity_key_header(b, codec::Kind::Notetype, &key)?
                .ok_or(Error::InvalidData)?;
            if let codec::Payload::Notetype(n) = header.local_payload(&store.connection, b)? {
                layouts.insert(n.id, n);
            } else {
                return Err(Error::InvalidData);
            }
            after = Some(key);
        }
    }
    Ok(layouts)
}
fn project(
    rows: &(impl ReadRows + ?Sized),
    kind: codec::Kind,
    key: &[u8],
    layouts: &BTreeMap<i64, codec::Notetype>,
) -> Result<Option<codec::Payload>, Error> {
    let table = row_payload::table(kind).ok_or(Error::InvalidData)?;
    let key = crate::cloud::workflow_store::row_key(kind, key)?;
    rows.row_image(table, &key)?
        .map(|row| {
            row_payload::project(&row_payload::ReadView(rows), kind, &row, |id| {
                layouts.get(&id).cloned().ok_or(Error::InvalidData)
            })
        })
        .transpose()
}

fn apply(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    s: &mut State,
    guard: &IncomingGuard,
    directory: &Path,
    entries: Vec<StoredInboxEntry>,
    replay_of: Option<i64>,
) -> Result<Option<i64>, RuntimeError> {
    let binding = guard.binding;
    let store = s.store.as_mut().ok_or(Error::Database)?;
    let layouts = layouts(store, binding)?;
    let record_key = store
        .binding(binding)?
        .ok_or(Error::BindingNotFound)?
        .facts
        .record_key
        .0;
    let mut keys = Vec::new();
    let mut ids = Vec::new();
    for entry in entries {
        let InboxValue::Record {
            envelope, staged_asset, ..
        } = &entry.entry.value
        else {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        };
        let received = match super::super::payload_asset::receive_owned(
            directory,
            binding,
            &record_key,
            &entry.entry.record_name,
            &envelope.0,
            staged_asset.as_deref(),
        ) {
            Ok(received) => received,
            Err(codec::ReceiveError::RetryableAsset(_)) => {
                store.park_incoming(binding, entry.id, InboxReason::Asset)?;
                continue;
            }
            Err(_) => return Err(Error::InvalidData.into()),
        };
        let remote = &received.entity;
        if remote.fields.tombstone
            && remote.fields.generation.is_none()
            && matches!(remote.fields.kind, codec::Kind::Note | codec::Kind::Card)
        {
            if let Some(mut deletion_keys) = plan_row_deletion(ctx, store, binding, &entry, remote, &layouts)? {
                keys.append(&mut deletion_keys);
                ids.push(entry.id);
            }
            continue;
        }
        if remote.fields.tombstone
            || remote.fields.generation.is_some()
            || matches!(remote.fields.kind, codec::Kind::Notetype | codec::Kind::MediaName)
        {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        }
        let server = received.payload.ok_or(Error::InvalidData)?;
        let local = project(ctx.scope(), server.kind(), &remote.fields.key, &layouts)?;
        // An exact fetched own save needs a durable digest/flight intent and
        // actual WAL coverage, but no duplicate copy of the unchanged local
        // 40 MiB image in intent JSON. Its ACK image is the retained flight.
        if let Some(h) = store.entity_header(binding, &entry.entry.record_name)?
            && h.facts.origin == remote.fields.origin
            && remote.fields.writer == binding.0
            && h.facts.kind == remote.fields.kind
            && h.facts.key == remote.fields.key
            && h.facts.tombstone_at.is_none()
            && local.is_some()
            && let Some(f) = h
                .facts
                .in_flight
                .iter()
                .find(|f| !f.tombstone && f.erev == remote.fields.erev && f.payload_digest == remote.payload_sha256)
        {
            if !references(ctx.scope(), &server, &layouts)? {
                store.park_incoming(binding, entry.id, InboxReason::Reference)?;
                continue;
            }
            let digest = digest(local.as_ref())?;
            keys.push(ApplyKey {
                name: entry.entry.record_name.clone(),
                kind: server.kind(),
                key: remote.fields.key.clone(),
                role: ApplyKeyRole::Entry,
                before_digest: digest,
                after_digest: digest,
                before_payload: None,
                after_payload: None,
                own_flight: Some(f.local_rev),
            });
            ids.push(entry.id);
            continue;
        }
        let facts = store.entity_facts(binding, &entry.entry.record_name)?;
        if facts
            .as_ref()
            .is_some_and(|e| e.origin != remote.fields.origin && e.server_erev == 0)
            && (remote.fields.kind as u8) <= 6
            && !same_object(local.as_ref(), &server)
            && !facts
                .as_ref()
                .is_some_and(|e| local.is_none() && e.tombstone_at.is_some())
        {
            // K14 needs an atomic re-key before ordinary merging.
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        }
        let changed_origin = facts
            .as_ref()
            .is_some_and(|e| e.origin != remote.fields.origin && e.server_erev == 0);
        let own = facts
            .as_ref()
            .filter(|e| e.origin == remote.fields.origin && remote.fields.writer == binding.0)
            .and_then(|e| e.in_flight.iter().find(|f| f.payload_digest == remote.payload_sha256))
            .map(|f| f.local_rev);
        let flight_base = facts
            .as_ref()
            .and_then(|e| {
                remote
                    .fields
                    .base_sha256
                    .and_then(|d| e.in_flight.iter().find(|f| f.payload_digest == d))
            })
            .map(|f| super::super::tracking_payload::flight_payload(&store.connection, binding, f))
            .transpose()?;
        let base = if changed_origin {
            None
        } else {
            flight_base.as_ref().or_else(|| {
                facts
                    .as_ref()
                    .and_then(|e| e.base_payload.as_ref().or(e.acked_payload.as_ref()))
            })
        };
        if local.is_none()
            && facts
                .as_ref()
                .is_some_and(|e| e.tombstone_at.is_some() && e.origin == remote.fields.origin)
        {
            // K17.1: a real local deletion wins a same-origin server edit.
            let e = facts.as_ref().ok_or(Error::InvalidData)?;
            if e.origin != remote.fields.origin || e.acked_tombstone() {
                store.park_incoming(binding, entry.id, InboxReason::Structure)?;
                continue;
            }
            let at = ctx.now().0;
            if own.is_none() {
                preserve_deleted_note(store, binding, ctx.scope(), &remote.fields, &server, at)?;
            }
            keys.push(ApplyKey {
                name: entry.entry.record_name.clone(),
                kind: server.kind(),
                key: remote.fields.key.clone(),
                role: ApplyKeyRole::Entry,
                before_digest: None,
                after_digest: None,
                before_payload: Some(e.payload.clone()),
                after_payload: None,
                own_flight: own,
            });
            ids.push(entry.id);
            continue;
        }
        if local.is_none()
            && facts
                .as_ref()
                .is_some_and(|e| e.server_erev > 0 || e.tombstone_at.is_none())
        {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        }
        if !references(ctx.scope(), &server, &layouts)? {
            store.park_incoming(binding, entry.id, InboxReason::Reference)?;
            continue;
        }
        let comparison_base = facts.as_ref().and_then(|e| {
            remote
                .fields
                .base_sha256
                .and_then(|d| e.in_flight.iter().find(|f| f.payload_digest == d))
        });
        let clean = if let Some(f) = comparison_base {
            digest(local.as_ref())? == Some(f.row_digest)
        } else {
            facts
                .as_ref()
                .is_some_and(|e| digest(local.as_ref()).ok().flatten() == Some(e.acked_row_digest))
        };
        let (mut result, losing) = if own.is_some() {
            (local.clone(), None)
        } else if clean || local.is_none() {
            (Some(server.clone()), None)
        } else {
            let (p, c) = incoming_merge::merge(base, local.as_ref().ok_or(Error::InvalidData)?, &server)?;
            (Some(p), c)
        };
        if let Some(p) = result.as_mut() {
            if let codec::Payload::Deck(deck) = p {
                // Stored path follows this device's current parent spelling.
                let leaf = deck.name.rsplit(|b| *b == 31).next().unwrap_or_default().to_vec();
                if deck.parent_id == 0 {
                    deck.name = leaf;
                } else {
                    let parent = ctx
                        .scope()
                        .row_image(Table::Decks, &RowKey::Id(deck.parent_id))?
                        .ok_or(Error::InvalidData)?;
                    let Some(Cell::Text(parent)) = parent.get(1) else {
                        return Err(Error::InvalidData.into());
                    };
                    deck.name = parent.clone();
                    deck.name.push(31);
                    deck.name.extend_from_slice(&leaf);
                }
            }
            if *p != server && Some(&*p) != local.as_ref() {
                incoming_merge::stamp_local(p, ctx.now().0 / 1000);
            }
            if let codec::Payload::Note(n) = p
                && n.flds.as_slice()
                    != match &server {
                        codec::Payload::Note(n) => n.flds.as_slice(),
                        _ => &[],
                    }
            {
                let nt = ctx
                    .scope()
                    .notetype(meguri_model::ids::NotetypeId(n.mid))?
                    .ok_or(Error::InvalidData)?;
                let fields: Vec<_> = std::str::from_utf8(&n.flds)
                    .map_err(|_| Error::InvalidData)?
                    .split('\u{1f}')
                    .collect();
                n.sfld = codec::Stored::Text(
                    meguri_text::checksum::sort_text_of(&fields, nt.config.sort_field_idx as usize)
                        .as_bytes()
                        .to_vec(),
                );
                n.csum = i64::from(meguri_text::checksum::note_checksum(
                    fields.first().copied().unwrap_or(""),
                ));
            }
        }
        if let Some(losing) = losing {
            let at = ctx.now().0;
            preserve_conflict(
                store,
                binding,
                ctx.scope(),
                &remote.fields,
                &losing,
                result.as_ref().ok_or(Error::InvalidData)?,
                at,
            )?;
        }
        let key = ApplyKey {
            name: entry.entry.record_name.clone(),
            kind: remote.fields.kind,
            key: remote.fields.key.clone(),
            role: ApplyKeyRole::Entry,
            before_digest: digest(local.as_ref())?,
            after_digest: digest(result.as_ref())?,
            before_payload: None,
            after_payload: result.clone(),
            own_flight: own,
        };
        if own.is_none() && key.before_digest != key.after_digest {
            if let (Some(codec::Payload::Deck(old)), Some(codec::Payload::Deck(new))) = (&local, &result) {
                deck_descendants(
                    ctx,
                    store,
                    binding,
                    DeckChange { old, new },
                    &record_key,
                    &layouts,
                    &mut keys,
                )?;
            }
            for (table, row_key, row) in row_payload::payload_rows(result.as_ref().ok_or(Error::InvalidData)?)? {
                ctx.scope().restore(table, &row_key, Some(&row))?;
            }
        }
        keys.push(key);
        ids.push(entry.id);
    }
    if ids.is_empty() {
        return Ok(None);
    }
    let mut unique: BTreeMap<String, ApplyKey> = BTreeMap::new();
    for key in keys {
        if let Some(prior) = unique.get_mut(&key.name) {
            if key.role == ApplyKeyRole::Entry {
                prior.role = ApplyKeyRole::Entry;
                prior.before_payload = key.before_payload;
                prior.own_flight = key.own_flight;
            } else if prior.role == ApplyKeyRole::Entry && key.after_digest.is_none() {
                // A live inbox entry can be removed by a later note tombstone.
                // Retain its actual pre-delete image as well as its entry role.
                prior.before_payload = key.before_payload.or_else(|| prior.after_payload.clone());
            }
        } else {
            unique.insert(key.name.clone(), key);
        }
    }
    let mut keys: Vec<_> = unique.into_values().collect();
    // Derive final actual post-images while rollback is still possible. In
    // particular a card's history includes revlogs written by this same op.
    for key in &mut keys {
        let after = project(ctx.scope(), key.kind, &key.key, &layouts)?;
        key.after_digest = digest(after.as_ref())?;
        key.after_payload =
            if key.own_flight.is_some() && key.before_payload.is_none() && key.before_digest == key.after_digest {
                None
            } else {
                after
            };
    }
    let planned_at_ms = ctx.now().0;
    let id = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        store.persist_apply_intent(
            &ApplyIntent {
                media: None,
                id: 0,
                binding,
                inbox_ids: ids,
                keys,
                result_seq: None,
                originating_open: Some(guard.open),
                planned_at_ms,
                native_file: guard.file,
                generation: guard.generation.0,
            },
            replay_of,
        )
    }))
    .unwrap_or(Err(Error::Database))
    {
        Ok(id) => id,
        Err(error) => {
            s.tracking_stopped = true;
            s.scan_required = true;
            return Err(error.into());
        }
    };
    Ok(Some(id))
}

fn same_object(local: Option<&codec::Payload>, server: &codec::Payload) -> bool {
    match (local, server) {
        (Some(codec::Payload::Note(l)), codec::Payload::Note(s)) => l.guid == s.guid,
        (Some(codec::Payload::Card(l)), codec::Payload::Card(s)) => (l.nid, l.ord) == (s.nid, s.ord),
        (Some(codec::Payload::Revlog(l)), codec::Payload::Revlog(s)) => l.cid == s.cid,
        (Some(codec::Payload::Deck(l)), codec::Payload::Deck(s)) => std::str::from_utf8(&l.name)
            .ok()
            .zip(std::str::from_utf8(&s.name).ok())
            .is_some_and(|(l, s)| meguri_text::fold::compare_folded(l, s) == std::cmp::Ordering::Equal),
        (Some(codec::Payload::Preset(l)), codec::Payload::Preset(s)) => l.name == s.name,
        (Some(codec::Payload::Notetype(l)), codec::Payload::Notetype(s)) => {
            l.fields.iter().map(|f| &f.name).eq(s.fields.iter().map(|f| &f.name))
                && l.templates
                    .iter()
                    .map(|t| &t.name)
                    .eq(s.templates.iter().map(|t| &t.name))
        }
        _ => false,
    }
}

struct DeckChange<'a> {
    old: &'a codec::Deck,
    new: &'a codec::Deck,
}

fn deck_descendants(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    store: &TrackingStore,
    binding: BindingId,
    change: DeckChange<'_>,
    record_key: &[u8; 32],
    layouts: &BTreeMap<i64, codec::Notetype>,
    keys: &mut Vec<ApplyKey>,
) -> Result<(), RuntimeError> {
    let DeckChange { old, new } = change;
    if old.name == new.name {
        return Ok(());
    }
    let mut prefix = old.name.clone();
    prefix.push(31);
    let mut after = None;
    let mut changes = Vec::new();
    loop {
        let page = ctx.scope().row_page(Table::Decks, after.as_ref())?;
        if page.is_empty() {
            break;
        }
        for row in page {
            let id = row_payload::int(&row, 0)?;
            after = Some(RowKey::Id(id));
            let Some(Cell::Text(name)) = row.get(1) else {
                return Err(Error::InvalidData.into());
            };
            if !name.starts_with(&prefix) {
                continue;
            }
            if id == new.parent_id {
                return Err(Error::InvalidData.into());
            }
            let before = row_payload::project(&row_payload::ReadView(&*ctx.scope()), codec::Kind::Deck, &row, |id| {
                layouts.get(&id).cloned().ok_or(Error::InvalidData)
            })?;
            let mut changed = before.clone();
            let codec::Payload::Deck(deck) = &mut changed else {
                return Err(Error::InvalidData.into());
            };
            let mut path = new.name.clone();
            path.push(31);
            path.extend_from_slice(&name[prefix.len()..]);
            deck.name = path;
            deck.mtime_secs = ctx.now().0 / 1000;
            let key = codec::id_key(id).to_vec();
            let name = codec::entity_name(record_key, codec::Kind::Deck, &key).map_err(|_| Error::InvalidData)?;
            let clean = store
                .entity_facts(binding, &name)?
                .is_some_and(|e| e.server_erev > 0 && digest(Some(&before)).ok().flatten() == Some(e.acked_row_digest));
            changes.push((
                changed.clone(),
                ApplyKey {
                    name,
                    kind: codec::Kind::Deck,
                    key,
                    role: ApplyKeyRole::SideEffect,
                    before_digest: digest(Some(&before))?,
                    after_digest: digest(Some(&changed))?,
                    before_payload: if clean { Some(before) } else { None },
                    after_payload: Some(changed),
                    own_flight: None,
                },
            ));
        }
    }
    for (payload, key) in changes {
        for (table, row_key, row) in row_payload::payload_rows(&payload)? {
            ctx.scope().restore(table, &row_key, Some(&row))?;
        }
        keys.push(key);
    }
    Ok(())
}

fn references(
    rows: &(impl ReadRows + ?Sized),
    p: &codec::Payload,
    layouts: &BTreeMap<i64, codec::Notetype>,
) -> Result<bool, Error> {
    let present = |t, id| {
        rows.row_image(t, &RowKey::Id(id))
            .map(|r| r.is_some())
            .map_err(Error::from)
    };
    Ok(match p {
        codec::Payload::Note(n) => {
            layouts.get(&n.mid).is_some_and(|nt| {
                nt.struct_gen == n.nt_struct_gen && nt.fields.len() == n.flds.split(|b| *b == 31).count()
            }) && present(Table::Notetypes, n.mid)?
        }
        codec::Payload::Card(c) => {
            present(Table::Notes, c.nid)?
                && present(Table::Decks, c.did)?
                && (c.odid == 0 || present(Table::Decks, c.odid)?)
        }
        codec::Payload::Revlog(r) => present(Table::Cards, r.cid)?,
        codec::Payload::Deck(d) => d.parent_id == 0 || present(Table::Decks, d.parent_id)?,
        _ => true,
    })
}

fn preserve_conflict(
    store: &mut TrackingStore,
    binding: BindingId,
    rows: &impl ReadRows,
    fields: &codec::EntityFields,
    losing: &codec::Payload,
    winning: &codec::Payload,
    at: i64,
) -> Result<(), RuntimeError> {
    let discarded_digest = losing.payload_digest().map_err(|_| Error::InvalidData)?;
    let retained_digest = winning.payload_digest().map_err(|_| Error::InvalidData)?;
    let mut identity = binding.0.to_vec();
    identity.extend_from_slice(&discarded_digest);
    identity.extend_from_slice(&retained_digest);
    let hash = codec::sha256(&identity);
    let mut id = [0; 16];
    id.copy_from_slice(&hash[..16]);
    let envelope = codec::Entity::build(fields.clone(), &losing.encode().map_err(|_| Error::InvalidData)?)
        .map_err(|_| Error::InvalidData)?
        .encode()
        .map_err(|_| Error::InvalidData)?;
    let (kind, reason) = match losing {
        codec::Payload::Note(_) => (ConflictKind::Note, ConflictReason::FieldConflict),
        codec::Payload::Deck(_) => (ConflictKind::Deck, ConflictReason::BlobConflict),
        codec::Payload::Preset(_) => (ConflictKind::Preset, ConflictReason::BlobConflict),
        _ => return Err(Error::InvalidData.into()),
    };
    // Capture K10.3 names and placement from the writer before replacing rows.
    let (field_names, template_names, note_deck) = if let codec::Payload::Note(note) = losing {
        let (field_names, template_names) = match rows.notetype(meguri_model::ids::NotetypeId(note.mid))? {
            Some(notetype) => (
                Some(notetype.fields.into_iter().map(|field| field.name).collect()),
                Some(notetype.templates.into_iter().map(|template| template.name).collect()),
            ),
            None => (None, None),
        };
        let note_deck = rows
            .cards_of_note(meguri_model::ids::NoteId(note.id))?
            .first()
            .map(|card| card.deck_id.0);
        (field_names, template_names, note_deck)
    } else {
        (None, None, None)
    };
    store.store_conflict(
        binding,
        &ConflictCopy {
            id: CopyId(id),
            kind,
            key: OpaqueBytes(fields.key.clone()),
            reason,
            losing_envelope: OpaqueBytes(envelope),
            losing_payload_digest: PayloadDigest(discarded_digest),
            winning_payload_digest: PayloadDigest(retained_digest),
            field_names,
            template_names,
            note_deck,
            staged_path: None,
            created_at_ms: at,
            resolved_at_ms: None,
        },
    )?;
    meguri_model::faultpoint!("cloud.apply.after_copy").map_err(Error::from)?;
    Ok(())
}

/// K17 ordinary deletion uses this op's live rows, never a held image or a
/// reconstructed base. Unknown identities remain blocked for collision work.
fn plan_row_deletion(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    store: &mut TrackingStore,
    binding: BindingId,
    entry: &StoredInboxEntry,
    remote: &codec::Entity,
    layouts: &BTreeMap<i64, codec::Notetype>,
) -> Result<Option<Vec<ApplyKey>>, RuntimeError> {
    let Some(facts) = store.entity_facts(binding, &entry.entry.record_name)? else {
        store.park_incoming(binding, entry.id, InboxReason::Structure)?;
        return Ok(None);
    };
    if facts.origin != remote.fields.origin {
        store.park_incoming(binding, entry.id, InboxReason::Structure)?;
        return Ok(None);
    }
    let local = project(ctx.scope(), remote.fields.kind, &remote.fields.key, layouts)?;
    let InboxValue::Record { envelope, .. } = &entry.entry.value else {
        return Err(Error::InvalidData.into());
    };
    let own_flight = facts
        .own_row_tombstone_flight(binding.0, remote, &envelope.0)
        .map(|f| f.local_rev);
    if remote.fields.writer == binding.0 {
        // K13.2: a fetched own save never writes collection rows. An Undo
        // needs K17 rekey support even when it restored the old ACK digest.
        // Keep both the exact flight and fetched evidence until that exists.
        if own_flight.is_none() || local.is_some() || remote.fields.erev <= facts.server_erev {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            return Ok(None);
        }
        return Ok(Some(vec![ApplyKey {
            name: entry.entry.record_name.clone(),
            kind: remote.fields.kind,
            key: remote.fields.key.clone(),
            role: ApplyKeyRole::Entry,
            before_digest: None,
            after_digest: None,
            before_payload: Some(facts.payload),
            after_payload: None,
            own_flight,
        }]));
    }
    let at = ctx.now().0;
    if let Some(local) = &local
        && local.row_digest().map_err(|_| Error::InvalidData)? != facts.acked_row_digest
    {
        preserve_deleted_note(store, binding, ctx.scope(), &remote.fields, local, at)?;
    }
    let mut keys = Vec::new();
    let key = super::super::workflow_store::row_key(remote.fields.kind, &remote.fields.key)?;
    let RowKey::Id(id) = key else {
        return Err(Error::InvalidData.into());
    };
    if remote.fields.kind == codec::Kind::Note {
        for card in ctx.scope().cards_of_note(meguri_model::ids::NoteId(id))? {
            let key = codec::id_key(card.id.0).to_vec();
            let before = project(ctx.scope(), codec::Kind::Card, &key, layouts)?.ok_or(Error::InvalidData)?;
            let name = codec::entity_name(
                &store
                    .binding(binding)?
                    .ok_or(Error::BindingNotFound)?
                    .facts
                    .record_key
                    .0,
                codec::Kind::Card,
                &key,
            )
            .map_err(|_| Error::InvalidData)?;
            let clean = store.entity_facts(binding, &name)?.is_none_or(|e| e.mark.is_none());
            keys.push(ApplyKey {
                name,
                kind: codec::Kind::Card,
                key,
                role: ApplyKeyRole::SideEffect,
                before_digest: digest(Some(&before))?,
                after_digest: None,
                before_payload: if clean { Some(before) } else { None },
                after_payload: None,
                own_flight: None,
            });
        }
        ctx.scope().remove_note_and_cards(meguri_model::ids::NoteId(id))?;
    } else {
        ctx.scope().remove_card(meguri_model::ids::CardId(id))?;
        ctx.scope().add_grave(
            id,
            meguri_model::grave::GraveKind::Card,
            meguri_model::usn::Usn::PENDING,
        )?;
    }
    keys.push(ApplyKey {
        name: entry.entry.record_name.clone(),
        kind: remote.fields.kind,
        key: remote.fields.key.clone(),
        role: ApplyKeyRole::Entry,
        before_digest: digest(local.as_ref())?,
        after_digest: None,
        before_payload: Some(local.unwrap_or(facts.payload)),
        after_payload: None,
        own_flight,
    });
    Ok(Some(keys))
}

fn preserve_deleted_note(
    store: &mut TrackingStore,
    binding: BindingId,
    rows: &impl ReadRows,
    fields: &codec::EntityFields,
    losing: &codec::Payload,
    at: i64,
) -> Result<(), RuntimeError> {
    let codec::Payload::Note(note) = losing else {
        return Ok(());
    };
    let discarded = losing.payload_digest().map_err(|_| Error::InvalidData)?;
    let retained = codec::sha256(&[]);
    let mut identity = binding.0.to_vec();
    identity.extend_from_slice(&discarded);
    identity.extend_from_slice(&retained);
    let mut id = [0; 16];
    id.copy_from_slice(&codec::sha256(&identity)[..16]);
    let mut copy_fields = fields.clone();
    copy_fields.tombstone = false;
    copy_fields.deleted_at_ms = 0;
    let envelope = codec::Entity::build(copy_fields, &losing.encode().map_err(|_| Error::InvalidData)?)
        .map_err(|_| Error::InvalidData)?
        .encode()
        .map_err(|_| Error::InvalidData)?;
    let notetype = rows.notetype(meguri_model::ids::NotetypeId(note.mid))?;
    let note_deck = rows
        .cards_of_note(meguri_model::ids::NoteId(note.id))?
        .first()
        .map(|c| c.deck_id.0);
    store.store_conflict(
        binding,
        &ConflictCopy {
            id: CopyId(id),
            kind: ConflictKind::Note,
            key: OpaqueBytes(fields.key.clone()),
            reason: ConflictReason::EditVsDelete,
            losing_envelope: OpaqueBytes(envelope),
            losing_payload_digest: PayloadDigest(discarded),
            winning_payload_digest: PayloadDigest(retained),
            field_names: notetype
                .as_ref()
                .map(|n| n.fields.iter().map(|f| f.name.clone()).collect()),
            template_names: notetype
                .as_ref()
                .map(|n| n.templates.iter().map(|t| t.name.clone()).collect()),
            note_deck,
            staged_path: None,
            created_at_ms: at,
            resolved_at_ms: None,
        },
    )?;
    meguri_model::faultpoint!("cloud.apply.after_copy").map_err(Error::from)?;
    Ok(())
}

#[cfg(test)]
mod same_object_tests {
    use super::{codec as c, same_object};
    use meguri_model::proto::{BlobMessage, deck, deck_config, notetype};

    fn payloads() -> [c::Payload; 6] {
        let mut normal = deck::NormalDeck::default();
        normal.config_id = 1;
        let mut template = notetype::TemplateConfig::default();
        template.q_format = "{{Front}}".into();
        template.a_format = "{{Back}}".into();
        [
            c::Payload::Note(c::Note {
                id: 100,
                guid: b"guid".to_vec(),
                mid: 1,
                flds: b"front\x1fback".to_vec(),
                sfld: c::Stored::Text(b"front".to_vec()),
                nt_struct_gen: 1,
                ..Default::default()
            }),
            c::Payload::Card(c::Card {
                id: 100,
                nid: 1,
                did: 1,
                data: c::Stored::Text(b"{}".to_vec()),
                nt_struct_gen: 1,
                ..Default::default()
            }),
            c::Payload::Revlog(c::Revlog {
                id: 100,
                cid: 1,
                ease: 3,
                ..Default::default()
            }),
            c::Payload::Deck(c::Deck {
                id: 100,
                name: b"Deck".to_vec(),
                common: deck::DeckCommon::default().encode(),
                kind: deck::DeckKindMessage::new(deck::DeckKind::Normal(normal)).encode(),
                ..Default::default()
            }),
            c::Payload::Preset(c::Preset {
                id: 100,
                name: b"Preset".to_vec(),
                config: deck_config::DeckConfigMessage::default().encode(),
                ..Default::default()
            }),
            c::Payload::Notetype(c::Notetype {
                id: 100,
                name: b"Type".to_vec(),
                struct_gen: 1,
                config: notetype::NotetypeConfig::default().encode(),
                fields: ["Front", "Back"]
                    .into_iter()
                    .enumerate()
                    .map(|(ord, name)| c::Field {
                        ord: ord as i64,
                        name: name.as_bytes().to_vec(),
                        config: notetype::FieldConfig::default().encode(),
                    })
                    .collect(),
                templates: ["Forward", "Reverse"]
                    .into_iter()
                    .enumerate()
                    .map(|(ord, name)| c::Template {
                        ord: ord as i64,
                        name: name.as_bytes().to_vec(),
                        config: template.encode(),
                        ..Default::default()
                    })
                    .collect(),
                ..Default::default()
            }),
        ]
    }

    // Preconditions for the pure predicate: canonical wire and valid native rows/blobs.
    fn canonical(payload: c::Payload) -> c::Payload {
        use meguri_storage::Record;
        let bytes = payload.encode().unwrap();
        let decoded = c::Payload::decode(payload.kind(), &bytes).unwrap();
        assert_eq!(decoded, payload);
        assert_eq!(decoded.encode().unwrap(), bytes);
        for (table, _, row) in crate::cloud::row_payload::payload_rows(&decoded).unwrap() {
            use meguri_storage::Table;
            match table {
                Table::Notes => {
                    meguri_model::note::Note::from_cells(&row).unwrap();
                }
                Table::Cards => {
                    meguri_model::card::Card::from_cells(&row).unwrap();
                }
                Table::Revlog => {
                    meguri_model::revlog::RevlogEntry::from_cells(&row).unwrap();
                }
                Table::Decks => {
                    meguri_model::deck::Deck::from_cells(&row).unwrap();
                }
                Table::DeckConfig => {
                    meguri_model::deck_config::DeckConfig::from_cells(&row).unwrap();
                }
                _ => {}
            }
        }
        if let c::Payload::Notetype(nt) = &decoded {
            std::str::from_utf8(&nt.name).unwrap();
            notetype::NotetypeConfig::decode(&nt.config).unwrap();
            for f in &nt.fields {
                std::str::from_utf8(&f.name).unwrap();
                notetype::FieldConfig::decode(&f.config).unwrap();
            }
            for t in &nt.templates {
                std::str::from_utf8(&t.name).unwrap();
                notetype::TemplateConfig::decode(&t.config).unwrap();
            }
        }
        decoded
    }

    fn compare(local: &c::Payload, server: c::Payload, expected: bool) {
        let local = canonical(local.clone());
        let server = canonical(server);
        assert_eq!(same_object(Some(&local), &server), expected);
        assert_eq!(same_object(Some(&server), &local), expected);
    }

    #[test]
    fn note_equal_guid_ignores_other_content_but_different_guid_is_a_collision() {
        let local = canonical(payloads()[0].clone());
        let c::Payload::Note(mut server) = local.clone() else {
            unreachable!()
        };
        server.id += 1;
        server.mid = 2;
        server.modified = 50;
        server.flds = b"different\x1fcontent".to_vec();
        server.sfld = c::Stored::Integer(12);
        server.tags = b" tag ".to_vec();
        server.data = b"opaque".to_vec();
        compare(&local, c::Payload::Note(server.clone()), true);
        server.guid = b"other-guid".to_vec();
        compare(&local, c::Payload::Note(server), false);
    }

    #[test]
    fn card_requires_both_nid_and_ord_and_ignores_other_columns() {
        let local = canonical(payloads()[1].clone());
        let c::Payload::Card(mut server) = local.clone() else {
            unreachable!()
        };
        server.id += 1;
        server.did = 2;
        server.modified = 50;
        server.due = c::Stored::Text(b"opaque".to_vec());
        server.ivl = 31;
        server.reps = 17;
        server.data = c::Stored::Blob(vec![0xff]);
        server.last_review_id = 123;
        compare(&local, c::Payload::Card(server.clone()), true);
        let mut changed = server.clone();
        changed.nid += 1;
        compare(&local, c::Payload::Card(changed), false);
        server.ord += 1;
        compare(&local, c::Payload::Card(server), false);
    }

    #[test]
    fn revlog_equal_cid_ignores_other_columns_but_different_cid_is_a_collision() {
        let local = canonical(payloads()[2].clone());
        let c::Payload::Revlog(mut server) = local.clone() else {
            unreachable!()
        };
        server.id += 1;
        server.ease = 2;
        server.ivl = c::Stored::Real(-6.5);
        server.last_ivl = c::Stored::Integer(31);
        server.factor = 2500;
        server.time = c::Stored::Integer(123);
        server.review_type = 1;
        compare(&local, c::Payload::Revlog(server.clone()), true);
        server.cid += 1;
        compare(&local, c::Payload::Revlog(server), false);
    }

    #[test]
    fn deck_name_uses_pinned_unicase_with_valid_utf8() {
        for (a, b, equal) in [
            ("Deck", "dEcK", true),
            ("Straße", "STRASSE", true),
            ("\u{fb03}", "iff", true),
            ("\u{fb03}", "ffi", false),
            ("Deck", "Other", false),
        ] {
            let c::Payload::Deck(mut l) = payloads()[3].clone() else {
                unreachable!()
            };
            l.name = a.as_bytes().to_vec();
            let mut s = l.clone();
            s.id += 1;
            s.name = b.as_bytes().to_vec();
            s.mtime_secs = 50;
            let mut common = deck::DeckCommon::default();
            common.study_collapsed = true;
            s.common = common.encode();
            compare(&c::Payload::Deck(l), c::Payload::Deck(s), equal);
        }
    }

    #[test]
    fn preset_name_is_exact_and_ignores_id_and_config() {
        let local = canonical(payloads()[4].clone());
        let c::Payload::Preset(mut server) = local.clone() else {
            unreachable!()
        };
        server.id += 1;
        server.mtime_secs = 50;
        // Unknown protobuf fact is valid and differs from the local config.
        server.config.extend_from_slice(&[0x98, 6, 7]);
        compare(&local, c::Payload::Preset(server.clone()), true);
        server.name = b"preset".to_vec();
        compare(&local, c::Payload::Preset(server.clone()), false);
        server.name = b"Other".to_vec();
        compare(&local, c::Payload::Preset(server), false);
    }

    #[test]
    fn notetype_requires_both_ordered_name_lists_and_ignores_config_and_text() {
        let local = canonical(payloads()[5].clone());
        let c::Payload::Notetype(mut server) = local.clone() else {
            unreachable!()
        };
        server.id += 1;
        server.name = b"Renamed type".to_vec();
        server.mtime_secs = 50;
        server.config.extend_from_slice(&[0x98, 6, 7]);
        server.fields[0].config.extend_from_slice(&[0x98, 6, 9]);
        let mut template = notetype::TemplateConfig::default();
        template.q_format = "{{Back}} changed".into();
        template.a_format = "{{Front}} changed".into();
        server.templates[0].config = template.encode();
        server.templates[0].mtime_secs = 51;
        compare(&local, c::Payload::Notetype(server.clone()), true);
        for fields in [true, false] {
            let mut changed = server.clone();
            if fields {
                changed.fields[0].name = b"Other".to_vec();
            } else {
                changed.templates[0].name = b"Other".to_vec();
            }
            compare(&local, c::Payload::Notetype(changed), false);
            let mut reordered = server.clone();
            // Keep canonical ord order while reversing the ordered names.
            if fields {
                let first = reordered.fields[0].name.clone();
                reordered.fields[0].name = reordered.fields[1].name.clone();
                reordered.fields[1].name = first;
            } else {
                let first = reordered.templates[0].name.clone();
                reordered.templates[0].name = reordered.templates[1].name.clone();
                reordered.templates[1].name = first;
            }
            compare(&local, c::Payload::Notetype(reordered), false);
            let mut shorter = server.clone();
            if fields {
                shorter.fields.pop();
            } else {
                shorter.templates.pop();
            }
            compare(&local, c::Payload::Notetype(shorter), false);
        }
    }

    #[test]
    fn missing_local_and_every_mismatched_kind_pair_are_false() {
        let payloads = payloads().map(canonical);
        for server in &payloads {
            assert!(!same_object(None, server));
            for local in &payloads {
                if local.kind() != server.kind() {
                    assert!(!same_object(Some(local), server));
                }
            }
        }
    }
}
