use super::*;
use crate::cloud::{rekey_store::Rekey, row_payload::View};
use meguri_storage::{Journal, RowImage};

impl Runtime {
    pub(super) fn select_rekey(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        entries: &[StoredInboxEntry],
    ) -> Result<Option<usize>, RuntimeError> {
        let (record_key, layouts) = self.with_incoming(guard, collection, |s| {
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
        for (index, entry) in entries.iter().enumerate() {
            let InboxValue::Record {
                envelope, staged_asset, ..
            } = &entry.entry.value
            else {
                continue;
            };
            let received = match crate::cloud::payload_asset::receive_owned(
                &self.directory,
                guard.binding,
                &record_key,
                &entry.entry.record_name,
                &envelope.0,
                staged_asset.as_deref(),
            ) {
                Ok(r) => r,
                Err(codec::ReceiveError::RetryableAsset(_)) => continue,
                Err(_) => return Err(Error::InvalidData.into()),
            };
            if received.entity.fields.kind as u8 > 6 || received.entity.fields.generation.is_some() {
                continue;
            }
            let header = self.with_incoming(guard, collection, |s| {
                Ok(s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .entity_header(guard.binding, &entry.entry.record_name)?)
            })?;
            if let Some(header) = header {
                let local = project(
                    &*snapshot,
                    received.entity.fields.kind,
                    &received.entity.fields.key,
                    &layouts,
                )?;
                if needed(
                    &header.facts,
                    &received.entity,
                    &envelope.0,
                    local.as_ref(),
                    received.payload.as_ref(),
                    guard.binding,
                ) {
                    return Ok(Some(index));
                }
            }
        }
        Ok(None)
    }
}

fn needed(
    facts: &crate::cloud::workflow::EntityFacts,
    remote: &codec::Entity,
    envelope: &[u8],
    local: Option<&codec::Payload>,
    server: Option<&codec::Payload>,
    binding: BindingId,
) -> bool {
    if remote.fields.kind as u8 > 6
        || remote.fields.generation.is_some()
        || local.is_none()
        || facts.kind != remote.fields.kind
        || facts.key != remote.fields.key
    {
        return false;
    }
    if facts.acked_tombstone() {
        return true;
    }
    if remote.fields.tombstone && facts.own_row_tombstone_flight(binding.0, remote, envelope).is_some() {
        return true;
    }
    facts.server_erev == 0
        && facts.origin != remote.fields.origin
        && server.is_none_or(|server| !same_object(local, server))
}

pub(super) fn apply_rekey(
    ctx: &mut meguri_engine::OpContext<'_, '_>,
    s: &mut State,
    guard: &IncomingGuard,
    directory: &Path,
    entries: Vec<StoredInboxEntry>,
) -> Result<Option<i64>, RuntimeError> {
    let [entry]: [StoredInboxEntry; 1] = entries.try_into().map_err(|_| Error::InvalidData)?;
    let store = s.store.as_mut().ok_or(Error::Database)?;
    let binding = store.binding(guard.binding)?.ok_or(Error::BindingNotFound)?;
    let before_layouts = layouts(store, guard.binding)?;
    let InboxValue::Record {
        envelope, staged_asset, ..
    } = &entry.entry.value
    else {
        return Err(Error::InvalidData.into());
    };
    let received = crate::cloud::payload_asset::receive_owned(
        directory,
        guard.binding,
        &binding.facts.record_key.0,
        &entry.entry.record_name,
        &envelope.0,
        staged_asset.as_deref(),
    )
    .map_err(|_| Error::InvalidData)?;
    let remote = &received.entity;
    let header = store
        .entity_header(guard.binding, &entry.entry.record_name)?
        .ok_or(Error::InvalidData)?;
    let local = project(ctx.scope(), remote.fields.kind, &remote.fields.key, &before_layouts)?;
    if !needed(
        &header.facts,
        remote,
        &envelope.0,
        local.as_ref(),
        received.payload.as_ref(),
        guard.binding,
    ) {
        return Ok(None); // A queued interactive op changed the planning view.
    }
    drop(local);
    let old = i64::from_be_bytes(
        remote
            .fields
            .key
            .as_slice()
            .try_into()
            .map_err(|_| Error::InvalidData)?,
    );
    let kind = remote.fields.kind;
    let table = row_payload::table(kind).ok_or(Error::InvalidData)?;
    let (scope, cancel) = ctx.cloud_rekey_scope()?;
    let stopped = || cancel.as_ref().is_some_and(|t| t.is_cancelled());
    let mut after = old;
    let new = loop {
        if stopped() {
            return Err(EngineError::Cancelled.into());
        }
        let next = match scope.cloud_rekey_candidate(table, after, stopped) {
            Ok(id) => id,
            Err(_) if stopped() => return Err(EngineError::Cancelled.into()),
            Err(e) => return Err(e.into()),
        };
        if !store.rekey_holds(guard.binding, kind, next)? {
            break next;
        }
        after = next;
    };
    let moved = scope.cloud_rekey_id(table, old, new, stopped);
    if stopped() {
        return Err(EngineError::Cancelled.into());
    }
    moved?;
    let journal = scope.journal().ok_or(Error::InvalidData)?;
    let before = Before { rows: &*scope, journal };
    let mut after_layouts = before_layouts.clone();
    for delta in journal.entries().filter(|d| d.table() == Table::Notetypes) {
        let RowKey::Id(id) = delta.key() else {
            return Err(Error::InvalidData.into());
        };
        if let Some(row) = journal.after(delta)? {
            let generation = before_layouts
                .get(id)
                .or_else(|| {
                    (kind == codec::Kind::Notetype && *id == new)
                        .then(|| before_layouts.get(&old))
                        .flatten()
                })
                .ok_or(Error::InvalidData)?
                .struct_gen;
            after_layouts.insert(
                *id,
                row_payload::notetype(&row_payload::ReadView(&*scope), &row, generation)?,
            );
        }
    }
    let mut keys = BTreeSet::new();
    for delta in journal.entries() {
        let mut table = delta.table();
        let mut key = delta.key().clone();
        if matches!(table, Table::Fields | Table::Templates) {
            let RowKey::Pair(id, _) = key else {
                return Err(Error::InvalidData.into());
            };
            table = Table::Notetypes;
            key = RowKey::Id(id);
        }
        if let Some(kind) = row_payload::KINDS
            .into_iter()
            .find(|k| row_payload::table(*k) == Some(table))
        {
            let bytes = match key {
                RowKey::Id(id) => codec::id_key(id).to_vec(),
                RowKey::Text(t) => t,
                _ => return Err(Error::InvalidData.into()),
            };
            keys.insert((kind, bytes));
        }
    }
    let new_key = codec::id_key(new).to_vec();
    let mut intent_keys = Vec::new();
    for (kind, key) in keys {
        let name = codec::entity_name(&binding.facts.record_key.0, kind, &key).map_err(|_| Error::InvalidData)?;
        let before_digest = digest(image(&before, kind, &key, &before_layouts)?.as_ref())?;
        let after_digest = digest(image(&row_payload::ReadView(&*scope), kind, &key, &after_layouts)?.as_ref())?;
        intent_keys.push(ApplyKey {
            name,
            kind,
            key,
            role: ApplyKeyRole::SideEffect,
            before_digest,
            after_digest,
            before_payload: None,
            after_payload: None,
            own_flight: None,
        });
    }
    let planned_at_ms = scope.now().0;
    let mut created = Vec::new();
    for key in intent_keys
        .iter()
        .filter(|k| k.before_digest.is_none() && k.after_digest.is_some() && k.key != new_key)
    {
        let mut origin = [0; 16];
        origin[..8].copy_from_slice(&ctx.random().next_u64().to_le_bytes());
        origin[8..].copy_from_slice(&ctx.random().next_u64().to_le_bytes());
        created.push((key.name.clone(), origin));
    }
    let rekey = Rekey {
        struct_gen: before_layouts
            .get(&old)
            .filter(|_| kind == codec::Kind::Notetype)
            .map(|n| n.struct_gen),
        created,
        format: 1,
        binding: guard.binding,
        kind,
        old: remote.fields.key.clone(),
        new: new_key.clone(),
        old_name: entry.entry.record_name.clone(),
        new_name: codec::entity_name(&binding.facts.record_key.0, kind, &new_key).map_err(|_| Error::InvalidData)?,
        origin: header.facts.origin,
        local_rev: header.facts.local_rev,
        inbox: entry.id,
        server_digest: codec::sha256(&envelope.0),
        preserve_old: header.facts.acked_tombstone()
            || header
                .facts
                .own_row_tombstone_flight(guard.binding.0, remote, &envelope.0)
                .is_some(),
    };
    let intent = ApplyIntent {
        media: None,
        id: 0,
        binding: guard.binding,
        inbox_ids: vec![entry.id],
        keys: intent_keys,
        result_seq: None,
        originating_open: Some(guard.open),
        planned_at_ms,
        native_file: guard.file,
        generation: guard.generation.0,
    };
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| store.persist_rekey(&intent, &rekey)))
        .unwrap_or(Err(Error::Database))
    {
        Ok(id) => Ok(Some(id)),
        Err(error) => {
            s.tracking_stopped = true;
            s.scan_required = true;
            Err(error.into())
        }
    }
}

pub(super) fn image(
    view: &impl View,
    kind: codec::Kind,
    key: &[u8],
    layouts: &BTreeMap<i64, codec::Notetype>,
) -> Result<Option<codec::Payload>, Error> {
    let table = row_payload::table(kind).ok_or(Error::InvalidData)?;
    let key = crate::cloud::workflow_store::row_key(kind, key)?;
    view.row(table, &key)?
        .map(|row| {
            if kind == codec::Kind::Notetype {
                let id = row_payload::int(&row, 0)?;
                let generation = layouts.get(&id).ok_or(Error::InvalidData)?.struct_gen;
                return row_payload::notetype(view, &row, generation).map(codec::Payload::Notetype);
            }
            row_payload::project(view, kind, &row, |id| {
                layouts.get(&id).cloned().ok_or(Error::InvalidData)
            })
        })
        .transpose()
}
struct Before<'a, T: ReadRows + ?Sized> {
    rows: &'a T,
    journal: &'a Journal,
}
impl<T: ReadRows + ?Sized> View for Before<'_, T> {
    fn row(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, Error> {
        if let Some(delta) = self.journal.entries().find(|d| d.table() == table && d.key() == key) {
            return Ok(self.journal.before(delta)?);
        }
        Ok(self.rows.row_image(table, key)?)
    }
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, Error> {
        self.page_before(table, after, None)
    }
    fn parts(&self, table: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
        self.page_before(table, after.map(|ord| RowKey::Pair(id, ord)).as_ref(), Some(id))
    }
    fn history(&self, id: i64) -> Result<i64, Error> {
        let mut bound = None;
        let mut latest = loop {
            let candidate = match bound {
                None => self.rows.max_revlog_id(meguri_model::ids::CardId(id))?,
                Some(before) => self.rows.max_revlog_id_before(meguri_model::ids::CardId(id), before)?,
            };
            if candidate == 0
                || !self
                    .journal
                    .entries()
                    .any(|d| d.table() == Table::Revlog && d.key() == &RowKey::Id(candidate))
            {
                break candidate;
            }
            bound = Some(candidate);
        };
        for delta in self.journal.entries().filter(|d| d.table() == Table::Revlog) {
            if let Some(row) = self.journal.before(delta)?
                && row_payload::int(&row, 1)? == id
            {
                latest = latest.max(row_payload::int(&row, 0)?);
            }
        }
        Ok(latest)
    }
}
impl<T: ReadRows + ?Sized> Before<'_, T> {
    fn page_before(&self, table: Table, after: Option<&RowKey>, parent: Option<i64>) -> Result<Vec<RowImage>, Error> {
        let mut cursor = after.cloned();
        loop {
            let live = if let Some(parent) = parent {
                let ord = match cursor {
                    Some(RowKey::Pair(_, ord)) => Some(ord),
                    _ => None,
                };
                row_payload::ReadView(self.rows).parts(table, parent, ord)?
            } else {
                self.rows.row_page(table, cursor.as_ref())?
            };
            let end = live
                .last()
                .map(|row| RowKey::from_stored(table, row))
                .transpose()
                .map_err(|_| Error::InvalidData)?;
            let mut keys = live
                .iter()
                .map(|row| RowKey::from_stored(table, row).map_err(|_| Error::InvalidData))
                .collect::<Result<Vec<_>, _>>()?;
            keys.extend(
                self.journal
                    .entries()
                    .filter(|d| d.table() == table)
                    .map(|d| d.key().clone()),
            );
            keys.retain(|key| {
                cursor
                    .as_ref()
                    .is_none_or(|a| crate::cloud::held_view::compare(table, key, a).is_gt())
                    && end
                        .as_ref()
                        .is_none_or(|e| !crate::cloud::held_view::compare(table, key, e).is_gt())
                    && parent.is_none_or(|id| matches!(key, RowKey::Pair(p,_) if *p == id))
            });
            keys.sort_by(|a, b| crate::cloud::held_view::compare(table, a, b));
            keys.dedup();
            let mut rows = Vec::new();
            for key in keys {
                if let Some(row) = self.row(table, &key)? {
                    rows.push(row);
                }
                if rows.len() == 250 {
                    break;
                }
            }
            if !rows.is_empty() || end.is_none() {
                return Ok(rows);
            }
            cursor = end;
        }
    }
}

impl Runtime {
    fn identity_layouts(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        r: &Rekey,
    ) -> Result<BTreeMap<i64, codec::Notetype>, RuntimeError> {
        let mut layouts = self.with_incoming(guard, collection, |s| {
            Ok(layouts(s.store.as_ref().ok_or(Error::Database)?, guard.binding)?)
        })?;
        if let Some(generation) = r.struct_gen {
            let old = i64::from_be_bytes(r.old.as_slice().try_into().map_err(|_| Error::InvalidData)?);
            let new = i64::from_be_bytes(r.new.as_slice().try_into().map_err(|_| Error::InvalidData)?);
            let mut moved = layouts.get(&old).cloned().ok_or(Error::InvalidData)?;
            moved.id = new;
            moved.struct_gen = generation;
            layouts.insert(new, moved);
        }
        Ok(layouts)
    }
    fn current_identity_layouts(
        rows: &(impl ReadRows + ?Sized),
        layouts: &mut BTreeMap<i64, codec::Notetype>,
    ) -> Result<(), Error> {
        for (id, layout) in layouts {
            if let Some(row) = rows.row_image(Table::Notetypes, &RowKey::Id(*id))? {
                *layout = row_payload::notetype(&row_payload::ReadView(rows), &row, layout.struct_gen)?;
            }
        }
        Ok(())
    }
    fn identity_proof(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        i: &ApplyIntent,
        r: &Rekey,
    ) -> Result<(bool, bool, Seq, meguri_engine::WalIdentity), RuntimeError> {
        let mut layouts = self.identity_layouts(collection, guard, r)?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        Self::current_identity_layouts(&*snapshot, &mut layouts)?;
        let (mut after, mut before) = (true, true);
        for key in &i.keys {
            let payload = image(&row_payload::ReadView(&*snapshot), key.kind, &key.key, &layouts)?;
            let actual = digest(payload.as_ref())?;
            after &= actual == key.after_digest;
            before &= actual == key.before_digest;
        }
        Ok((after, before, snapshot.seq(), snapshot.wal_identity()))
    }
    pub(super) fn replay_identity(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        i: &ApplyIntent,
        r: &Rekey,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        let (after, before, _, _) = self.identity_proof(collection, guard, i, r)?;
        if after {
            return self.settle_identity(collection, guard, i, r, timeout);
        }
        if !before {
            return Err(Error::RecoveryRefused.into());
        }
        self.with_incoming(guard, collection, |s| {
            Ok(s.store.as_mut().ok_or(Error::Database)?.clear_rolled_back_rekey(i)?)
        })
    }
    pub(super) fn settle_identity(
        &self,
        collection: &Collection,
        guard: &IncomingGuard,
        i: &ApplyIntent,
        r: &Rekey,
        timeout: Duration,
    ) -> Result<(), RuntimeError> {
        let (after, _, seq, identity) = self.identity_proof(collection, guard, i, r)?;
        if !after || i.native_file != guard.file {
            return Err(Error::RecoveryRefused.into());
        }
        // No reader, runtime or tracking guard is held over the real flush.
        let deadline = meguri_engine::monotonic::now().saturating_add(timeout);
        collection.flush_wal(deadline.saturating_sub(meguri_engine::monotonic::now()))?;
        let receipt = match collection.wait_wal_covered(
            identity,
            seq,
            deadline.saturating_sub(meguri_engine::monotonic::now()),
        )? {
            WalCoverage::Covered(receipt)
                if receipt.identity() == identity
                    && receipt.through() >= seq
                    && identity.native_file() == guard.file.pair()
                    && identity.generation() == guard.generation =>
            {
                receipt
            }
            _ => return Err(Error::RecoveryRefused.into()),
        };
        self.with_incoming(guard, collection, |s| {
            s.incoming.receipts.push(receipt);
            Ok(())
        })?;
        meguri_model::faultpoint!("cloud.apply.after_flush").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::ApplyAfterFlush);
        // Reopen the snapshot after flushing; require the proven writer seq.
        let mut layouts = self.identity_layouts(collection, guard, r)?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        if snapshot.seq() != seq {
            return Err(EngineError::Busy {
                running_op: "cloud identity operation".into(),
            }
            .into());
        }
        Self::current_identity_layouts(&*snapshot, &mut layouts)?;
        self.with_incoming(guard, collection, |s| {
            if collection.last_seq() != seq {
                return Err(Error::RecoveryRefused.into());
            }
            s.store.as_mut().ok_or(Error::Database)?.settle_rekey(
                i,
                r,
                (guard.open, seq.0, guard.generation.0),
                |key| {
                    let payload = image(&row_payload::ReadView(&*snapshot), key.kind, &key.key, &layouts)?;
                    if digest(payload.as_ref())? != key.after_digest {
                        return Err(Error::RecoveryRefused);
                    }
                    Ok(payload)
                },
                || {
                    if collection.last_seq() == seq {
                        Ok(())
                    } else {
                        Err(Error::RecoveryRefused)
                    }
                },
            )?;
            s.cloud.acked_images.remove(&r.old_name);
            s.cloud.handed_images.retain(|(name, _, _), _| name != &r.old_name);
            Ok(())
        })?;
        drop(snapshot);
        drop(lease);
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::IntentSettlementComplete);
        Ok(())
    }
}
