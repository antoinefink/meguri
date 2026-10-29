//! The shared host/SDK ordinary receive and collection-apply path.
use super::*;
use crate::cloud::{incoming::*, incoming_merge, row_payload, workflow::Saved};
use std::collections::{BTreeMap, BTreeSet};

// Ordinary replay can use a recovery row's old owner without selecting it or
// opening its SDK gate. The scan pins the actual current engine opening.
#[derive(Clone)]
struct IncomingGuard {
    scan: ScanGuard,
    recovery: Option<RecoveryGuard>,
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
        self.0.lock().unwrap_or_else(PoisonError::into_inner).incoming.running = false;
        self.1.notify_all();
    }
}

impl Runtime {
    fn finish_incoming(&self, running: Running) -> Result<(), RuntimeError> {
        drop(running);
        let mut s = self.state();
        self.emit_pending_locked(&mut s, workflow::UpdateKind::Full, BTreeSet::new())?;
        Ok(())
    }
    pub(super) fn wait_incoming_idle(&self) {
        let mut s = self.state();
        while s.incoming.running {
            s = self.incoming_idle.wait(s).unwrap_or_else(PoisonError::into_inner);
        }
    }
    pub(super) fn settle_before_transition(
        &self,
        collection: &Collection,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        self.wait_incoming_idle();
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
                    RecoveryKind::Adopt(p) => (
                        &p.binding_before,
                        &p.account,
                        p.file_before,
                        p.before.publication.identity,
                    ),
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
                }
            };
            s.incoming.running = true;
            guard
        };
        let running = Running(self.state.clone(), self.incoming_idle.clone());
        self.with_incoming(&guard, collection, |_| Ok(()))?;
        self.replay_ordinary(collection, &guard, timeout)?;
        drop(running);
        if guard.recovery.is_some() {
            // The current usable owner's own crash intents still precede its
            // scan after the recovery row's old owner has finished.
            self.settle_before_transition(collection, timeout)?;
        }
        Ok(())
    }

    fn check_incoming_locked(s: &State, guard: &IncomingGuard) -> Result<(), RuntimeError> {
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
        } else if !b.usable || !b.verified {
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
            return self.with_scan(&guard.scan, collection, work);
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
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.entity_facts(binding, name)?))
    }
    pub fn incoming_runs(&self) -> Vec<ApplyRun> {
        self.state().incoming.runs.clone()
    }
    pub fn incoming_wal_receipts(&self) -> Vec<WalReceipt> {
        self.state().incoming.receipts.clone()
    }

    /// Non-SDK transport boundary. It persists the very same facts as adapter_fetched.
    pub fn receive_collection_records(&self, instance: &str, records: &[Saved]) -> Result<(), RuntimeError> {
        let at = self.adapter_now()?;
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
        let entities: Vec<_> = records.iter().filter(|r| r.name != "epoch").cloned().collect();
        let (entries, refused) = classify(&key, &entities, at);
        meguri_model::faultpoint!("cloud.fetch.before_inbox").map_err(Error::from)?;
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .receive_records(b, &entries, &refused)
        });
        if result.is_err() {
            Self::invalidate_locked(&mut s);
        }
        result?;
        s.incoming.retry_granted = true;
        drop(s);
        meguri_model::faultpoint!("cloud.fetch.after_inbox").map_err(Error::from)?;
        Ok(())
    }

    /// Bounded work grant, called by an actual fetch/event trigger, never a timer.
    /// Collection is borrowed per call; Runtime owns no collection handle.
    pub fn drain_incoming(&self, collection: &Collection, timeout: Duration) -> Result<usize, RuntimeError> {
        self.refresh_identity()?;
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
            s.incoming.running = true;
            g
        };
        let guard = IncomingGuard {
            scan: guard,
            recovery: None,
        };
        let _running = Running(self.state.clone(), self.incoming_idle.clone());
        self.with_incoming(&guard, collection, |_| Ok(()))?;
        let replayed = self.replay_ordinary(collection, &guard, timeout)?;
        let (entries, retry_granted, failed_this_open) = self.with_incoming(&guard, collection, |s| {
            Ok((
                s.store.as_ref().ok_or(Error::Database)?.inbox(guard.binding)?,
                s.incoming.retry_granted,
                s.incoming.failed_this_open.clone(),
            ))
        })?;
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
            .filter(|e| matches!(e.entry.reason, InboxReason::Ready | InboxReason::Reference))
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
        if entries.is_empty() {
            if let Some(entry) = retry {
                entries.push(entry);
            }
        }
        if entries.is_empty() {
            if replayed > 0 {
                self.finish_incoming(_running)?;
            }
            return Ok(0);
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
        let count = self.apply_ordinary_entries(collection, &guard, entries, None, timeout)?;
        self.finish_incoming(_running)?;
        Ok(count)
    }

    fn apply_ordinary_entries(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        entries: Vec<StoredInboxEntry>,
        replay_of: Option<i64>,
        timeout: Duration,
    ) -> Result<usize, RuntimeError> {
        collection.check_rollover()?;
        self.with_incoming(guard, collection, |_| Ok(()))?;
        let shared = self.state.clone();
        let file = guard.file;
        let generation = guard.generation;
        let binding = guard.binding;
        let writer_guard = guard.clone();
        let attempted: Vec<_> = entries.iter().map(|e| e.id).collect();
        let result = collection.run(
            meguri_engine::OpSpec::named("cloud_incoming_apply", meguri_engine::OpSource::CloudIncoming),
            move |ctx| {
                let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
                if ctx.generation() != generation {
                    return Err(engine_error("incoming generation"));
                }
                with_tracking_access(|| {
                    Self::check_incoming_locked(&s, &writer_guard)?;
                    apply(ctx, &mut s, binding, file, generation, entries, replay_of)
                })
                .map_err(engine_error)
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
                self.with_incoming(guard, collection, |s| {
                    Ok(s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .rollback_apply(binding, &attempted, cancel)?)
                })?;
                return Err(error.into());
            }
        };
        let Some(intent_id) = result.value else {
            return Ok(0);
        };
        self.with_incoming(guard, collection, |s| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_apply_seq(binding, intent_id, result.seq.0)?;
            let intent = s
                .store
                .as_ref()
                .ok_or(Error::Database)?
                .apply_intents(binding)?
                .into_iter()
                .find(|i| i.id == intent_id)
                .ok_or(Error::InvalidData)?;
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
        self.settle_ordinary(collection, guard, intent_id, timeout)?;
        Ok(attempted.len())
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
        let (current, seq, identity) = self.read_intent_rows(collection, &i)?;
        // The snapshot/read lease has been released; no caller waits under it,
        // the writer, runtime state or the tracking gate. Seq belongs to this
        // opening, never the intent's prior-process result counter.
        let receipt = match collection.wait_wal_covered(identity, seq, timeout)? {
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
        meguri_model::faultpoint!("cloud.apply.after_flush").map_err(Error::from)?;
        self.with_incoming(guard, collection, |s| {
            if collection.last_seq() != seq {
                return Err(EngineError::Busy {
                    running_op: "cloud incoming".into(),
                }
                .into());
            }
            s.store.as_mut().ok_or(Error::Database)?.settle_apply(
                &i,
                guard.open,
                seq.0,
                guard.generation.0,
                &current,
                guard.recovery.is_none(),
            )?;
            s.incoming.receipts.push(receipt);
            Ok(())
        })
    }

    fn read_intent_rows(
        &self,
        collection: &Collection,
        i: &ApplyIntent,
    ) -> Result<
        (
            BTreeMap<String, Option<codec::Payload>>,
            Seq,
            meguri_engine::WalIdentity,
        ),
        RuntimeError,
    > {
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
    ) -> Result<usize, RuntimeError> {
        let intents = self.with_incoming(guard, collection, |s| {
            Ok(s.store.as_ref().ok_or(Error::Database)?.apply_intents(guard.binding)?)
        })?;
        let count = intents.len();
        for i in intents {
            collection.check_rollover()?;
            if i.native_file != guard.file {
                return Err(RuntimeError::FileIdentityUnavailable);
            }
            let (current, current_seq, _) = self.read_intent_rows(collection, &i)?;
            let all_after = i
                .keys
                .iter()
                .all(|k| digest(current.get(&k.name).and_then(Option::as_ref)).ok() == Some(k.after_digest));
            if all_after {
                self.settle_ordinary(collection, guard, i.id, timeout)?;
            } else {
                // Retain the original inbox and acknowledged base. The next
                // writer derives L afresh, so a local edit after a crash wins
                // only its changed parts rather than being overwritten.
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
                let entries = self.with_incoming(guard, collection, |s| {
                    let entries = s.store.as_ref().ok_or(Error::Database)?.inbox(guard.binding)?;
                    let mut by_id: BTreeMap<_, _> = entries.into_iter().map(|entry| (entry.id, entry)).collect();
                    i.inbox_ids
                        .iter()
                        .map(|id| by_id.remove(id).ok_or(Error::InvalidData.into()))
                        .collect()
                })?;
                self.apply_ordinary_entries(collection, guard, entries, Some(i.id), timeout)?;
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
        Ok(count)
    }
}

fn layouts(store: &TrackingStore, b: BindingId) -> Result<BTreeMap<i64, codec::Notetype>, Error> {
    let mut layouts = BTreeMap::new();
    let mut after = None;
    loop {
        let page = store.entity_page(b, codec::Kind::Notetype, after.as_deref())?;
        if page.is_empty() {
            break;
        }
        for e in page {
            after = Some(e.key);
            if let Some(codec::Payload::Notetype(n)) = e.acked_payload {
                layouts.insert(n.id, n);
            }
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
    binding: BindingId,
    file: NativeFileIdentity,
    generation: Generation,
    entries: Vec<StoredInboxEntry>,
    replay_of: Option<i64>,
) -> Result<Option<i64>, RuntimeError> {
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
        let InboxValue::Record { envelope, .. } = &entry.entry.value else {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        };
        let received = codec::receive(
            &record_key,
            &entry.entry.record_name,
            &envelope.0,
            codec::AssetInput::Absent,
        )
        .map_err(|_| Error::InvalidData)?;
        let remote = &received.entity;
        if remote.fields.tombstone
            || remote.fields.generation.is_some()
            || matches!(remote.fields.kind, codec::Kind::Notetype | codec::Kind::MediaName)
        {
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        }
        let server = received.payload.ok_or(Error::InvalidData)?;
        if !references(ctx.scope(), &server, &layouts)? {
            store.park_incoming(binding, entry.id, InboxReason::Reference)?;
            continue;
        }
        let facts = store.entity_facts(binding, &entry.entry.record_name)?;
        let local = project(ctx.scope(), server.kind(), &remote.fields.key, &layouts)?;
        if facts
            .as_ref()
            .is_some_and(|e| e.origin != remote.fields.origin && e.server_erev == 0)
            && (remote.fields.kind as u8) <= 6
            && !same_object(local.as_ref(), &server)
        {
            // K14 needs an atomic re-key before ordinary merging.
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
            continue;
        }
        let own = facts
            .as_ref()
            .filter(|e| e.origin == remote.fields.origin && remote.fields.writer == binding.0)
            .and_then(|e| e.in_flight.iter().find(|f| f.payload_digest == remote.payload_sha256))
            .map(|f| f.local_rev);
        let base = facts.as_ref().and_then(|e| {
            remote
                .fields
                .base_sha256
                .and_then(|d| e.in_flight.iter().find(|f| f.payload_digest == d).map(|f| &f.payload))
                .or(e.base_payload.as_ref())
                .or(e.acked_payload.as_ref())
        });
        if local.is_none() && facts.is_some() {
            // A local deletion needs K17's edit-versus-delete handling; it is
            // not permission to resurrect the row as a clean download.
            store.park_incoming(binding, entry.id, InboxReason::Structure)?;
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
            if let codec::Payload::Note(n) = p {
                if n.flds.as_slice()
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
        }
        if let Some(losing) = losing {
            preserve_conflict(
                store,
                binding,
                &entry.entry,
                &remote.fields,
                &losing,
                result.as_ref().ok_or(Error::InvalidData)?,
                ctx.now().0,
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
                deck_descendants(ctx, store, binding, old, new, &record_key, &layouts, &mut keys)?;
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
                prior.before_payload = None;
                prior.own_flight = key.own_flight;
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
        key.after_payload = after;
    }
    let id = match store.persist_apply_intent(
        &ApplyIntent {
            id: 0,
            binding,
            inbox_ids: ids,
            keys,
            result_seq: None,
            native_file: file,
            generation: generation.0,
        },
        replay_of,
    ) {
        Ok(id) => id,
        Err(error) => {
            s.tracking_stopped = true;
            s.scan_required = true;
            return Err(error.into());
        }
    };
    meguri_model::faultpoint!("cloud.apply.after_intent").map_err(Error::from)?;
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
        _ => false,
    }
}

fn deck_descendants(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    store: &TrackingStore,
    binding: BindingId,
    old: &codec::Deck,
    new: &codec::Deck,
    record_key: &[u8; 32],
    layouts: &BTreeMap<i64, codec::Notetype>,
    keys: &mut Vec<ApplyKey>,
) -> Result<(), RuntimeError> {
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
    rows: &impl ReadRows,
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
    entry: &InboxEntry,
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
    let _ = entry;
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
            field_names: None,
            template_names: None,
            note_deck: None,
            staged_path: None,
            created_at_ms: at,
            resolved_at_ms: None,
        },
    )?;
    meguri_model::faultpoint!("cloud.apply.after_copy").map_err(Error::from)?;
    Ok(())
}
