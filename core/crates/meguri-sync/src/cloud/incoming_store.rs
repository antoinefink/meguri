//! Durable K13 inbox ordering and the collection/acknowledgement bridge.
use super::{
    incoming::*,
    store::{decode, encode, validate_entry},
    workflow::EntityFacts,
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

/// Base-format DDL belongs to the caller's creation/migration transaction.
pub(super) fn install(c: &Transaction<'_>) -> Result<(), Error> {
    c.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_apply_intents(
        id INTEGER PRIMARY KEY AUTOINCREMENT, binding BLOB NOT NULL REFERENCES bindings(id),
        content TEXT NOT NULL CHECK(json_valid(content)));
        CREATE INDEX IF NOT EXISTS cloud_apply_by_binding ON cloud_apply_intents(binding);
        CREATE TABLE IF NOT EXISTS cloud_refused(binding BLOB NOT NULL REFERENCES bindings(id),
        name TEXT NOT NULL, content TEXT NOT NULL CHECK(json_valid(content)), PRIMARY KEY(binding,name));",
    )?;
    Ok(())
}

fn table_present(c: &Connection, name: &str, required: bool) -> Result<bool, Error> {
    let kind: Option<String> = c
        .query_row("SELECT type FROM sqlite_master WHERE name=?1", [name], |r| r.get(0))
        .optional()?;
    match kind.as_deref() {
        Some("table") => Ok(true),
        None if !required => Ok(false),
        _ => Err(Error::InvalidData),
    }
}

fn decode_intent(id: i64, binding: BindingId, text: &str) -> Result<ApplyIntent, Error> {
    let mut intent: ApplyIntent = decode(text)?;
    // The pre-V3 writer serialized zero before SQLite assigned the row id.
    // Only an intent still awaiting its result sequence may use that form.
    let pending_legacy_id = intent.id == 0 && intent.result_seq.is_none();
    if id <= 0 || intent.binding != binding || (intent.id != id && !pending_legacy_id) {
        return Err(Error::InvalidData);
    }
    intent.id = id;
    Ok(intent)
}

/// V3 requires both tables. Older bases may lack either table, but any WIP
/// incoming facts already present must be readable before the source opens.
pub(super) fn validate(c: &Connection, required: bool) -> Result<(), Error> {
    let has_intents = table_present(c, "cloud_apply_intents", required)?;
    let has_refused = table_present(c, "cloud_refused", required)?;
    let index: Option<(String, String)> = c
        .query_row(
            "SELECT type,tbl_name FROM sqlite_master WHERE name='cloud_apply_by_binding'",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .optional()?;
    if let Some((kind, table)) = index
        && (kind != "index" || table != "cloud_apply_intents")
    {
        return Err(Error::InvalidData);
    }
    // Check storage ownership only. Do not read another build's runtime
    // payloads, infer server facts, or interpret intentionally refused bytes.
    let require_owner = |binding: BindingId| -> Result<(), Error> {
        let exists: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE id=?1)",
            [binding.0.as_slice()],
            |r| r.get(0),
        )?;
        if exists { Ok(()) } else { Err(Error::InvalidData) }
    };
    if has_intents {
        let mut q = c.prepare("SELECT id,binding,content FROM cloud_apply_intents")?;
        let mut rows = q.query([])?;
        while let Some(row) = rows.next()? {
            let binding = store::binding_id(row.get(1)?)?;
            require_owner(binding)?;
            let intent = decode_intent(row.get(0)?, binding, &row.get::<_, String>(2)?)?;
            for id in &intent.inbox_ids {
                let exists: bool = c.query_row(
                    "SELECT EXISTS(SELECT 1 FROM inbox WHERE id=?1 AND binding=?2)",
                    params![id, binding.0.as_slice()],
                    |r| r.get(0),
                )?;
                if !exists {
                    return Err(Error::InvalidData);
                }
            }
        }
    }
    if has_refused {
        let mut q = c.prepare("SELECT binding,name,content FROM cloud_refused")?;
        let mut rows = q.query([])?;
        while let Some(row) = rows.next()? {
            require_owner(store::binding_id(row.get(0)?)?)?;
            let name: String = row.get(1)?;
            let refusal: RefusedEntry = decode(&row.get::<_, String>(2)?)?;
            if name != refusal.record_name {
                return Err(Error::InvalidData);
            }
        }
    }
    Ok(())
}

pub(super) fn facts(c: &Connection, b: BindingId, name: &str) -> Result<Option<EntityFacts>, Error> {
    facts_for(c, b, name, true)
}

// Receive compares identity/version metadata only; applying still needs bodies.
fn facts_for(c: &Connection, b: BindingId, name: &str, bodies: bool) -> Result<Option<EntityFacts>, Error> {
    for (table, notes) in [("cloud_runtime_notes", true), ("cloud_runtime_entities", false)] {
        let exists: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if !exists {
            continue;
        }
        let sql = format!("SELECT content FROM {table} WHERE binding=?1 AND json_extract(content,'$.name')=?2");
        let text: Option<String> = c
            .query_row(&sql, params![b.0.as_slice(), name], |r| r.get(0))
            .optional()?;
        if let Some(text) = text {
            return Ok(Some(if !bodies {
                let kind = if notes {
                    entity_codec::Kind::Note
                } else {
                    decode::<EntityFacts>(&text)?.kind
                };
                super::tracking_payload::header(kind, &text)?.facts
            } else if notes {
                super::workflow_store::note_facts(super::tracking_payload::load_note(c, b, &text)?)?
            } else {
                let e: EntityFacts = decode(&text)?;
                super::tracking_payload::load_entity(c, b, e.kind, &text)?
            }));
        }
    }
    Ok(None)
}

fn version(e: &InboxEntry) -> Option<([u8; 16], u64)> {
    let InboxValue::Record { envelope, .. } = &e.value else {
        return None;
    };
    match entity_codec::Entity::decode(&envelope.0).ok()? {
        entity_codec::DecodeOutcome::Supported(e) => Some((e.fields.origin, e.fields.erev)),
        _ => None,
    }
}

pub(super) fn intents(c: &Connection, b: BindingId) -> Result<Vec<ApplyIntent>, Error> {
    let mut q = c.prepare("SELECT id,content FROM cloud_apply_intents WHERE binding=?1 ORDER BY id")?;
    let rows = q.query_map([b.0.as_slice()], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
    rows.map(|r| {
        let (id, text) = r?;
        decode_intent(id, b, &text)
    })
    .collect()
}

/// Classified native intake shares ordering, pass attribution and intent protection.
pub(super) fn receive(
    tx: &Transaction<'_>,
    b: BindingId,
    entries: &[InboxEntry],
    refused: &[RefusedEntry],
) -> Result<(), Error> {
    receive_selected(tx, b, entries, refused, None)
}

// Equal-version replacement authority is private to the exact guarded read.
fn receive_selected(
    tx: &Transaction<'_>,
    b: BindingId,
    entries: &[InboxEntry],
    refused: &[RefusedEntry],
    settle: Option<i64>,
) -> Result<(), Error> {
    let active_pass = super::adapter_store::running_pass(tx, b)?;
    // K13.1 records all delivered names, including inputs rejected below.
    // These marks commit with the received entries before cursor persistence.
    if let Some(pass_id) = active_pass {
        for record_name in entries
            .iter()
            .map(|e| &e.record_name)
            .chain(refused.iter().map(|e| &e.record_name))
        {
            super::adapter_store::mark_seen(tx, b, record_name, pass_id)?;
        }
    }
    let protected: std::collections::BTreeSet<i64> = intents(tx, b)?.into_iter().flat_map(|i| i.inbox_ids).collect();
    for refusal in refused {
        let local = facts_for(tx, b, &refusal.record_name, false)?;
        if let (Some(origin), Some(erev)) = (refusal.origin, refusal.erev) {
            if local
                .as_ref()
                .is_some_and(|e| e.origin == origin && e.server_erev >= erev)
            {
                continue;
            }
            let prior: Option<String> = tx
                .query_row(
                    "SELECT content FROM cloud_refused WHERE binding=?1 AND name=?2",
                    params![b.0.as_slice(), refusal.record_name],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(prior) = prior {
                let prior: RefusedEntry = decode(&prior)?;
                if prior.origin == Some(origin) && prior.erev.is_some_and(|v| v > erev) {
                    continue;
                }
            }
            let mut q =
                tx.prepare("SELECT id,entry FROM inbox WHERE binding=?1 AND json_extract(entry,'$.record_name')=?2")?;
            let queued = q
                .query_map(params![b.0.as_slice(), refusal.record_name], |r| {
                    Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                })?
                .map(|r| {
                    let (id, text) = r?;
                    Ok((id, decode::<InboxEntry>(&text)?))
                })
                .collect::<Result<Vec<_>, Error>>()?;
            drop(q);
            if queued
                .iter()
                .any(|(_, e)| version(e).is_some_and(|(o, v)| o == origin && v > erev))
            {
                continue;
            }
            for (id, e) in queued {
                if !protected.contains(&id) && version(&e).is_some_and(|(o, v)| o == origin && v <= erev) {
                    tx.execute("DELETE FROM inbox WHERE id=?1", [id])?;
                }
            }
        }
        tx.execute(
            "INSERT INTO cloud_refused(binding,name,content) VALUES(?1,?2,?3)
            ON CONFLICT(binding,name) DO UPDATE SET content=excluded.content",
            params![b.0.as_slice(), refusal.record_name, encode(refusal)?],
        )?;
    }
    for original in entries {
        let mut inserted = original.clone();
        if settle.is_none() {
            inserted.absence_pass = active_pass;
        }
        let e = &inserted;
        validate_entry(e)?;
        let local = facts_for(tx, b, &e.record_name, false)?;
        let mut q = tx.prepare(
            "SELECT id,entry FROM inbox WHERE binding=?1 AND json_extract(entry,'$.record_name')=?2 ORDER BY id",
        )?;
        let queued = q
            .query_map(params![b.0.as_slice(), e.record_name], |r| {
                Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
            })?
            .map(|r| {
                let (id, text) = r?;
                Ok((id, decode::<InboxEntry>(&text)?))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        drop(q);
        if matches!(e.value, InboxValue::Deletion) && local.is_none() && queued.is_empty() {
            continue;
        }
        if let Some((origin, erev)) = version(e) {
            if let Some(local) = &local {
                if local.server_erev > 0 && local.origin != origin {
                    let InboxValue::Record { envelope, .. } = &e.value else {
                        return Err(Error::InvalidData);
                    };
                    let refusal = RefusedEntry {
                        record_name: e.record_name.clone(),
                        reason: "origin_mismatch".into(),
                        envelope: envelope.clone(),
                        origin: Some(origin),
                        erev: Some(erev),
                        seen_at_ms: e.fetched_at_ms,
                    };
                    tx.execute("INSERT INTO cloud_refused(binding,name,content) VALUES(?1,?2,?3) ON CONFLICT(binding,name) DO UPDATE SET content=excluded.content",
                        params![b.0.as_slice(), e.record_name, encode(&refusal)?])?;
                    continue;
                }
                if local.origin == origin && erev <= local.server_erev {
                    continue;
                }
            }
            if queued
                .iter()
                .any(|(id, old)| Some(*id) != settle && version(old).is_some_and(|(o, v)| o == origin && v >= erev))
            {
                continue;
            }
            let old: Option<String> = tx
                .query_row(
                    "SELECT content FROM cloud_refused WHERE binding=?1 AND name=?2",
                    params![b.0.as_slice(), e.record_name],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(old) = old {
                let old: RefusedEntry = decode(&old)?;
                if old.origin == Some(origin) && old.erev.is_some_and(|v| v >= erev) {
                    continue;
                }
                if old.origin != Some(origin) && erev <= local.as_ref().map_or(0, |e| e.server_erev) {
                    continue;
                }
            }
        }
        if version(e).is_some() || matches!(e.value, InboxValue::Deletion) {
            tx.execute(
                "DELETE FROM cloud_refused WHERE binding=?1 AND name=?2",
                params![b.0.as_slice(), e.record_name],
            )?;
        }
        if let Some(id) = settle {
            // The caller checked this entry and its exact version in this transaction.
            tx.execute(
                "UPDATE inbox SET entry=?3 WHERE binding=?1 AND id=?2",
                params![b.0.as_slice(), id, encode(e)?],
            )?;
            continue;
        }
        for (id, _) in queued {
            if !protected.contains(&id) && (version(e).is_some() || matches!(e.value, InboxValue::Deletion)) {
                tx.execute("DELETE FROM inbox WHERE id=?1", [id])?;
            }
        }
        tx.execute(
            "INSERT INTO inbox(binding,entry) VALUES(?1,?2) ON CONFLICT(binding,entry) DO NOTHING",
            params![b.0.as_slice(), encode(e)?],
        )?;
    }
    Ok(())
}

impl TrackingStore {
    pub(super) fn incoming_blocks_name(&self, b: BindingId, name: &str) -> Result<bool, Error> {
        if self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM inbox WHERE binding=?1 AND json_extract(entry,'$.record_name')=?2)",
            params![b.0.as_slice(), name],
            |r| r.get::<_, bool>(0),
        )? || self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_refused WHERE binding=?1 AND name=?2)",
            params![b.0.as_slice(), name],
            |r| r.get::<_, bool>(0),
        )? {
            return Ok(true);
        }
        Ok(intents(&self.connection, b)?
            .iter()
            .any(|i| i.keys.iter().any(|k| k.name == name)))
    }
    pub(super) fn receive_records(
        &mut self,
        b: BindingId,
        entries: &[InboxEntry],
        refused: &[RefusedEntry],
        fetched_epoch: Option<&super::control_codec::Epoch>,
    ) -> Result<(), Error> {
        if self.binding(b)?.ok_or(Error::BindingNotFound)?.facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        let tx = self.connection.transaction()?;
        receive(&tx, b, entries, refused)?;
        if let Some(epoch) = fetched_epoch {
            super::epoch_crt_store::put(&tx, b, epoch, super::epoch_crt_store::Authority::Fetched)?;
        }
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.tracking.commit.inbox")?;
        meguri_model::faultpoint!("cloud.tracking.commit.inbox")?;
        tx.commit()?;
        Ok(())
    }
    pub fn apply_intents(&self, b: BindingId) -> Result<Vec<ApplyIntent>, Error> {
        self.require_binding(b)?;
        intents(&self.connection, b)
    }
    pub fn refused_entries(&self, b: BindingId) -> Result<Vec<RefusedEntry>, Error> {
        self.require_binding(b)?;
        let mut q = self
            .connection
            .prepare("SELECT content FROM cloud_refused WHERE binding=?1 ORDER BY name")?;
        q.query_map([b.0.as_slice()], |r| r.get::<_, String>(0))?
            .map(|r| decode(&r?))
            .collect()
    }
    pub(super) fn persist_apply_intent(&mut self, i: &ApplyIntent, replay_of: Option<i64>) -> Result<i64, Error> {
        let tx = self.connection.transaction()?;
        for id in &i.inbox_ids {
            let exists: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM inbox WHERE id=?1 AND binding=?2)",
                params![id, i.binding.0.as_slice()],
                |r| r.get(0),
            )?;
            if !exists {
                return Err(Error::InvalidData);
            }
        }
        tx.execute(
            "INSERT INTO cloud_apply_intents(binding,content) VALUES(?1,?2)",
            params![i.binding.0.as_slice(), encode(i)?],
        )?;
        let id = tx.last_insert_rowid();
        let mut stored = i.clone();
        stored.id = id;
        tx.execute(
            "UPDATE cloud_apply_intents SET content=?2 WHERE id=?1",
            params![id, encode(&stored)?],
        )?;
        if let Some(prior) = replay_of {
            let old = intents(&tx, i.binding)?
                .into_iter()
                .find(|old| old.id == prior)
                .ok_or(Error::InvalidData)?;
            if old.inbox_ids.iter().collect::<std::collections::BTreeSet<_>>()
                != i.inbox_ids.iter().collect::<std::collections::BTreeSet<_>>()
            {
                return Err(Error::InvalidData);
            }
            // Keep the prior intent across reconcile and collection rollback;
            // replacement is atomic with persistence of the actual new intent.
            tx.execute(
                "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
                params![i.binding.0.as_slice(), prior],
            )?;
        }
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.tracking.commit.intent")?;
        meguri_model::faultpoint!("cloud.tracking.commit.intent")?;
        tx.commit()?;
        Ok(id)
    }
    pub(super) fn record_apply_seq(&mut self, b: BindingId, id: i64, seq: u64) -> Result<(), Error> {
        let mut i = intents(&self.connection, b)?
            .into_iter()
            .find(|i| i.id == id)
            .ok_or(Error::InvalidData)?;
        if i.result_seq.is_some_and(|s| s != seq) {
            return Err(Error::InvalidData);
        }
        i.result_seq = Some(seq);
        self.connection.execute(
            "UPDATE cloud_apply_intents SET content=?2 WHERE id=?1",
            params![id, encode(&i)?],
        )?;
        Ok(())
    }
    pub(super) fn park_incoming(&mut self, b: BindingId, id: i64, reason: InboxReason) -> Result<(), Error> {
        let text: String = self.connection.query_row(
            "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
            params![b.0.as_slice(), id],
            |r| r.get(0),
        )?;
        let mut e: InboxEntry = decode(&text)?;
        e.reason = reason;
        self.connection
            .execute("UPDATE inbox SET entry=?2 WHERE id=?1", params![id, encode(&e)?])?;
        Ok(())
    }
    pub(super) fn rollback_apply(&mut self, b: BindingId, ids: &[i64], cancel: bool) -> Result<(), Error> {
        self.rollback_apply_kind(b, ids, cancel, ApplyFailure::Apply)
    }
    pub(super) fn rollback_apply_kind(
        &mut self,
        b: BindingId,
        ids: &[i64],
        cancel: bool,
        failure: ApplyFailure,
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        for id in ids {
            let text: String = tx.query_row(
                "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                params![b.0.as_slice(), id],
                |r| r.get(0),
            )?;
            let mut e: InboxEntry = decode(&text)?;
            e.reason = if cancel {
                InboxReason::Ready
            } else {
                e.retry_count = e.retry_count.saturating_add(1);
                InboxReason::ApplyFailed {
                    failure,
                    error: OpaqueBytes(if failure == ApplyFailure::Rekey {
                        b"collection_rekey".to_vec()
                    } else {
                        b"collection_apply".to_vec()
                    }),
                }
            };
            tx.execute("UPDATE inbox SET entry=?2 WHERE id=?1", params![id, encode(&e)?])?;
        }
        for i in intents(&tx, b)? {
            if i.result_seq.is_none() && i.inbox_ids.iter().any(|id| ids.contains(id)) {
                tx.execute("DELETE FROM cloud_rekey_intents WHERE id=?1", [i.id])?;
                tx.execute("DELETE FROM cloud_apply_intents WHERE id=?1", [i.id])?;
            }
        }
        super::tracking_payload::collect_retired(&tx, b)?;
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.tracking.commit.rollback")?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn reconcile_apply_keys(
        &mut self,
        i: &ApplyIntent,
        open: u64,
        seq: u64,
        generation: u64,
        current: &std::collections::BTreeMap<String, Option<entity_codec::Payload>>,
        current_observer_open: bool,
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(i.binding)?;
        let tx = self.connection.transaction()?;
        for key in &i.keys {
            let Some(mut e) = facts(&tx, i.binding, &key.name)? else {
                continue;
            };
            let Some(payload) = current.get(&key.name).and_then(Option::as_ref) else {
                continue;
            };
            if current_observer_open
                && e.observer_open == open
                && e.observed_generation == generation
                && e.observed_seq > seq
            {
                continue;
            }
            let clean = e.mark.is_none();
            e.observe(
                payload.clone(),
                runtime::DirtySource::Snapshot,
                &mut state.change_counter,
            )?;
            if clean && key.role == ApplyKeyRole::SideEffect && e.mark.is_some() {
                e.base_payload = key.before_payload.clone().or(e.acked_payload.clone());
            }
            e.observer_open = open;
            e.observed_seq = seq;
            e.observed_generation = generation;
            super::workflow_store::put(&tx, i.binding, &e)?;
        }
        super::runtime_store::put_binding(&tx, i.binding, &state)?;
        tx.commit()?;
        Ok(())
    }
    /// Called only after real coverage and a current collection read, off the writer.
    pub(super) fn settle_apply(
        &mut self,
        i: &ApplyIntent,
        open: u64,
        seq: u64,
        generation: u64,
        current: &std::collections::BTreeMap<String, Option<entity_codec::Payload>>,
        current_observer_open: bool,
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(i.binding)?;
        let record_key = self
            .binding(i.binding)?
            .ok_or(Error::BindingNotFound)?
            .facts
            .record_key
            .0;
        let payload_source = (self.directory.as_path(), &record_key);
        let tx = self.connection.transaction()?;
        let mut parked = std::collections::BTreeSet::new();
        for key in &i.keys {
            if settle_own_live(
                &tx,
                i,
                key,
                current,
                (open, seq, generation),
                payload_source,
                &mut state,
            )? {
                continue;
            }
            let prior = facts(&tx, i.binding, &key.name)?;
            let observed = current.get(&key.name).ok_or(Error::InvalidData)?.clone();
            // A newer observer may already have run after the covered snapshot.
            let local = prior
                .as_ref()
                .filter(|e| {
                    current_observer_open
                        && e.observer_open == open
                        && e.observed_generation == generation
                        && e.observed_seq > seq
                })
                .map(|e| {
                    if e.tombstone_at.is_some() {
                        None
                    } else {
                        Some(e.payload.clone())
                    }
                })
                .unwrap_or(observed);
            let Some(local) = local else {
                settle_absent_key(&tx, i, key, prior, &mut state, (open, seq, generation), payload_source)?;
                continue;
            };
            let mut e = if let Some(e) = prior {
                e
            } else {
                workflow::EntityFacts {
                    name: key.name.clone(),
                    kind: key.kind,
                    key: key.key.clone(),
                    origin: [0; 16],
                    sent: key.role == ApplyKeyRole::Entry,
                    local_rev: 1,
                    acked_rev: 0,
                    tombstone_at: None,
                    payload: local.clone(),
                    base_payload: None,
                    mark: None,
                    server_erev: 0,
                    acked_system_fields: Vec::new(),
                    acked_row_digest: [0; 32],
                    acked_payload_digest: [0; 32],
                    acked_payload: None,
                    acked_envelope: OpaqueBytes::default(),
                    in_flight: Vec::new(),
                    observer_open: open,
                    observed_seq: seq,
                    observed_generation: generation,
                }
            };
            let was_clean = e.mark.is_none();
            if key.role == ApplyKeyRole::Entry && e.payload != local {
                e.local_rev = super::runtime_store::next(e.local_rev)?;
            }
            e.payload = local;
            e.tombstone_at = None;
            if key.role == ApplyKeyRole::Entry {
                let id = i
                    .inbox_ids
                    .iter()
                    .find(|id| {
                        tx.query_row(
                            "SELECT json_extract(entry,'$.record_name') FROM inbox WHERE id=?1",
                            [id],
                            |r| r.get::<_, String>(0),
                        )
                        .ok()
                        .as_ref()
                            == Some(&key.name)
                    })
                    .ok_or(Error::InvalidData)?;
                let text: String = tx.query_row("SELECT entry FROM inbox WHERE id=?1", [id], |r| r.get(0))?;
                let mut entry: InboxEntry = decode(&text)?;
                let InboxValue::Record {
                    envelope,
                    system_fields,
                    staged_asset,
                    ..
                } = entry.value.clone()
                else {
                    return Err(Error::InvalidData);
                };
                let entity_codec::DecodeOutcome::Supported(server) =
                    entity_codec::Entity::decode(&envelope.0).map_err(|_| Error::InvalidData)?
                else {
                    return Err(Error::InvalidData);
                };
                if server.fields.tombstone && matches!(key.kind, entity_codec::Kind::Note | entity_codec::Kind::Card) {
                    if server.fields.origin != e.origin
                        || server.fields.kind != e.kind
                        || server.fields.key != e.key
                        || server.payload.as_deref() != Some(&[])
                    {
                        return Err(Error::InvalidData);
                    }
                    // The covered read/newer observer supplied a live row after
                    // this intent. Keep it pending and retain the tombstone for
                    // rekey work, without decoding its empty body or advancing ACK.
                    if was_clean {
                        e.base_payload = key.before_payload.clone().or(e.acked_payload.clone());
                    }
                    if e.local_rev <= e.acked_rev {
                        e.local_rev = super::runtime_store::next(e.local_rev)?;
                    }
                    state.change_counter = super::runtime_store::next(state.change_counter)?;
                    e.mark = Some(runtime::DirtyMark {
                        source: runtime::DirtySource::Observer,
                        change_seq: state.change_counter,
                        payload_digest: e.payload.payload_digest().map_err(|_| Error::InvalidData)?,
                    });
                    if !current_observer_open || e.observer_open != open || e.observed_seq <= seq {
                        e.observer_open = open;
                        e.observed_seq = seq;
                        e.observed_generation = generation;
                    }
                    super::workflow_store::put(&tx, i.binding, &e)?;
                    entry.reason = InboxReason::Structure;
                    tx.execute("UPDATE inbox SET entry=?2 WHERE id=?1", params![id, encode(&entry)?])?;
                    parked.insert(*id);
                    continue;
                }
                super::adapter_store::transfer_entry(&tx, i.binding, &key.name, entry.absence_pass)?;
                let payload = super::payload_asset::receive_owned(
                    payload_source.0,
                    i.binding,
                    payload_source.1,
                    &key.name,
                    &envelope.0,
                    staged_asset.as_deref(),
                )
                .map_err(|_| Error::InvalidData)?
                .payload
                .ok_or(Error::InvalidData)?;
                e.origin = server.fields.origin;
                e.server_erev = server.fields.erev;
                e.acked_system_fields = system_fields.0;
                e.acked_row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
                e.acked_payload_digest = server.payload_sha256;
                e.acked_payload = Some(payload.clone());
                e.acked_envelope = envelope;
                if let Some(rev) = key.own_flight {
                    e.acked_rev = rev;
                    e.in_flight.retain(|f| f.local_rev > rev);
                } else {
                    e.acked_rev = e.local_rev;
                    e.in_flight.clear();
                }
                e.base_payload = Some(payload);
            } else if was_clean {
                e.base_payload = key.before_payload.clone().or(e.acked_payload.clone());
            }
            if key.role == ApplyKeyRole::Entry
                && e.payload.row_digest().map_err(|_| Error::InvalidData)? == e.acked_row_digest
            {
                e.mark = None;
                e.base_payload = None;
            } else {
                state.change_counter = super::runtime_store::next(state.change_counter)?;
                if key.role == ApplyKeyRole::SideEffect {
                    e.local_rev = super::runtime_store::next(e.local_rev)?;
                }
                e.mark = Some(runtime::DirtyMark {
                    source: runtime::DirtySource::Observer,
                    change_seq: state.change_counter,
                    payload_digest: e.payload.payload_digest().map_err(|_| Error::InvalidData)?,
                });
            }
            if !current_observer_open || e.observer_open != open || e.observed_seq <= seq {
                e.observer_open = open;
                e.observed_seq = seq;
                e.observed_generation = generation;
            }
            super::workflow_store::put(&tx, i.binding, &e)?;
        }
        super::runtime_store::put_binding(&tx, i.binding, &state)?;
        for id in &i.inbox_ids {
            if parked.contains(id) {
                continue;
            }
            tx.execute(
                "DELETE FROM inbox WHERE binding=?1 AND id=?2",
                params![i.binding.0.as_slice(), id],
            )?;
        }
        tx.execute(
            "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), i.id],
        )?;
        super::tracking_payload::collect_retired(&tx, i.binding)?;
        meguri_model::faultpoint!("cloud.tracking.commit.ack")?;
        tx.commit()?;
        Ok(())
    }
}

/// Settle a digest-only own-save intent against the exact durable flight. All
/// authority and current-row checks precede the transactional reference ACK.
fn settle_own_live(
    tx: &Transaction<'_>,
    intent: &ApplyIntent,
    key: &ApplyKey,
    current: &std::collections::BTreeMap<String, Option<entity_codec::Payload>>,
    observed: (u64, u64, u64),
    payload_source: (&std::path::Path, &[u8; 32]),
    state: &mut runtime::StoredRuntimeState,
) -> Result<bool, Error> {
    let Some(rev) = key.own_flight else {
        return Ok(false);
    };
    if key.role != ApplyKeyRole::Entry || key.after_digest.is_none() {
        return Ok(false);
    }
    let Some(local) = current.get(&key.name).and_then(Option::as_ref) else {
        return Ok(false);
    };
    let Some(text) = (if key.kind == entity_codec::Kind::Note {
        let id = i64::from_be_bytes(key.key.as_slice().try_into().map_err(|_| Error::InvalidData)?);
        tx.query_row(
            "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![intent.binding.0.as_slice(), id],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    } else {
        tx.query_row(
            "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![intent.binding.0.as_slice(), key.kind as i64, &key.key],
            |r| r.get::<_, String>(0),
        )
        .optional()?
    }) else {
        return Ok(false);
    };
    let mut h = super::tracking_payload::header(key.kind, &text)?;
    if h.facts.tombstone_at.is_some()
        || local.row_digest().map_err(|_| Error::InvalidData)? != h.images.local_row_digest
    {
        return Ok(false);
    }
    let mut found = None;
    for id in &intent.inbox_ids {
        let text: String = tx.query_row(
            "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
            params![intent.binding.0.as_slice(), id],
            |r| r.get(0),
        )?;
        let e: InboxEntry = decode(&text)?;
        if e.record_name == key.name {
            found = Some(e);
            break;
        }
    }
    let entry = found.ok_or(Error::InvalidData)?;
    let InboxValue::Record {
        envelope,
        system_fields,
        staged_asset,
        ..
    } = &entry.value
    else {
        return Err(Error::InvalidData);
    };
    let received = super::payload_asset::receive_owned(
        payload_source.0,
        intent.binding,
        payload_source.1,
        &key.name,
        &envelope.0,
        staged_asset.as_deref(),
    )
    .map_err(|_| Error::InvalidData)?;
    let remote = &received.entity;
    if remote.fields.writer != intent.binding.0
        || remote.fields.origin != h.facts.origin
        || remote.fields.kind != key.kind
        || remote.fields.key != key.key
        || remote.fields.tombstone
        || remote.fields.generation.is_some()
        || received.payload.is_none()
    {
        return Err(Error::InvalidData);
    }
    let index = h
        .facts
        .in_flight
        .iter()
        .position(|f| {
            f.local_rev == rev
                && !f.tombstone
                && f.erev == remote.fields.erev
                && f.payload_digest == remote.payload_sha256
        })
        .ok_or(Error::InvalidData)?;
    let saved = workflow::Saved {
        name: key.name.clone(),
        envelope: envelope.0.clone(),
        system_fields: system_fields.0.clone(),
    };
    h.ack(index, &saved)?;
    if h.images.local_row_digest != h.facts.acked_row_digest {
        // A real edit back to the old ACK may have cleared the observer mark.
        // This newly acknowledged flight changes the base, so refresh pending
        // evidence in the same transaction without changing the local revision.
        state.change_counter = super::runtime_store::next(state.change_counter)?;
        h.facts.mark = Some(runtime::DirtyMark {
            source: runtime::DirtySource::Observer,
            change_seq: state.change_counter,
            payload_digest: h.local_digest(),
        });
    }
    if h.facts.observer_open != observed.0 || h.facts.observed_seq <= observed.1 {
        h.facts.observer_open = observed.0;
        h.facts.observed_seq = observed.1;
        h.facts.observed_generation = observed.2;
    }
    super::tracking_payload::put_header(tx, intent.binding, &h)?;
    super::adapter_store::transfer_entry(tx, intent.binding, &key.name, entry.absence_pass)?;
    Ok(true)
}

fn settle_absent_key(
    tx: &Transaction<'_>,
    intent: &ApplyIntent,
    key: &ApplyKey,
    prior: Option<workflow::EntityFacts>,
    state: &mut runtime::StoredRuntimeState,
    observed: (u64, u64, u64),
    payload_source: (&std::path::Path, &[u8; 32]),
) -> Result<(), Error> {
    if !matches!(key.kind, entity_codec::Kind::Note | entity_codec::Kind::Card) {
        return Err(Error::InvalidData);
    }
    let mut e = if key.role == ApplyKeyRole::SideEffect {
        let mut e = prior.ok_or(Error::InvalidData)?;
        let before = key.before_payload.clone().unwrap_or_else(|| e.payload.clone());
        // This is the exact intent before-image when the entity was clean.
        // A dirty entity keeps its earlier pending base instead.
        if !e.sent && e.server_erev == 0 && e.in_flight.is_empty() {
            return super::workflow_store::remove_entity(tx, intent.binding, &e);
        }
        e.remove(before, intent.planned_at_ms, &mut state.change_counter)?;
        e
    } else {
        let mut found = None;
        for id in &intent.inbox_ids {
            let text: String = tx.query_row(
                "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                params![intent.binding.0.as_slice(), id],
                |r| r.get(0),
            )?;
            let entry: InboxEntry = decode(&text)?;
            if entry.record_name == key.name {
                found = Some(entry);
                break;
            }
        }
        let entry = found.ok_or(Error::InvalidData)?;
        super::adapter_store::transfer_entry(tx, intent.binding, &key.name, entry.absence_pass)?;
        let InboxValue::Record {
            envelope,
            system_fields,
            staged_asset,
            ..
        } = entry.value
        else {
            return Err(Error::InvalidData);
        };
        let entity_codec::DecodeOutcome::Supported(server) =
            entity_codec::Entity::decode(&envelope.0).map_err(|_| Error::InvalidData)?
        else {
            return Err(Error::InvalidData);
        };
        let tombstone = server.fields.tombstone;
        if server.fields.kind != key.kind || server.fields.key != key.key {
            return Err(Error::InvalidData);
        }
        let mut e = match prior {
            Some(e) => e,
            None if !tombstone && key.kind == entity_codec::Kind::Card && key.after_digest.is_none() => {
                // This card was downloaded and removed by its note's tombstone
                // in the same op. Its real server entry is the first ACK/base.
                let payload = super::payload_asset::receive_owned(
                    payload_source.0,
                    intent.binding,
                    payload_source.1,
                    &key.name,
                    &envelope.0,
                    staged_asset.as_deref(),
                )
                .map_err(|_| Error::InvalidData)?
                .payload
                .ok_or(Error::InvalidData)?;
                workflow::EntityFacts {
                    name: key.name.clone(),
                    kind: key.kind,
                    key: key.key.clone(),
                    origin: server.fields.origin,
                    sent: key.role == ApplyKeyRole::Entry,
                    local_rev: 1,
                    acked_rev: 0,
                    tombstone_at: None,
                    payload,
                    base_payload: None,
                    mark: None,
                    server_erev: 0,
                    acked_system_fields: Vec::new(),
                    acked_row_digest: [0; 32],
                    acked_payload_digest: [0; 32],
                    acked_payload: None,
                    acked_envelope: OpaqueBytes::default(),
                    in_flight: Vec::new(),
                    observer_open: observed.0,
                    observed_seq: observed.1,
                    observed_generation: observed.2,
                }
            }
            None => return Err(Error::InvalidData),
        };
        let before = key.before_payload.clone().unwrap_or_else(|| e.payload.clone());
        if server.fields.origin != e.origin || server.fields.kind != e.kind || server.fields.key != e.key {
            return Err(Error::InvalidData);
        }
        let own = if tombstone && key.own_flight.is_some() {
            let flight = e
                .own_row_tombstone_flight(intent.binding.0, &server, &envelope.0)
                .cloned()
                .ok_or(Error::InvalidData)?;
            if Some(flight.local_rev) != key.own_flight {
                return Err(Error::InvalidData);
            }
            Some(flight)
        } else {
            None
        };
        let newer = own.as_ref().is_some_and(|f| e.local_rev > f.local_rev);
        let acked = if tombstone {
            own.as_ref()
                .map(|f| super::tracking_payload::flight_payload(tx, intent.binding, f))
                .transpose()?
                .unwrap_or_else(|| before.clone())
        } else {
            super::payload_asset::receive_owned(
                payload_source.0,
                intent.binding,
                payload_source.1,
                &key.name,
                &envelope.0,
                staged_asset.as_deref(),
            )
            .map_err(|_| Error::InvalidData)?
            .payload
            .ok_or(Error::InvalidData)?
        };
        e.server_erev = server.fields.erev;
        e.acked_system_fields = system_fields.0;
        e.acked_row_digest = if tombstone {
            entity_codec::sha256(&[])
        } else {
            acked.row_digest().map_err(|_| Error::InvalidData)?
        };
        e.acked_payload_digest = server.payload_sha256;
        e.acked_payload = Some(acked.clone());
        e.acked_envelope = envelope;
        if !newer {
            e.payload = before;
        }
        if let Some(rev) = key.own_flight {
            e.acked_rev = rev;
            e.in_flight.retain(|f| f.local_rev > rev);
        } else {
            e.acked_rev = e.local_rev;
            e.in_flight.clear();
        }
        if tombstone {
            if !newer {
                e.tombstone_at = Some(server.fields.deleted_at_ms);
                e.mark = None;
                e.base_payload = None;
            }
        } else {
            // The incoming live version supplies the conditional base, but
            // cannot resurrect the explicitly deleted local object (K17.1).
            e.base_payload = Some(acked);
            e.tombstone_at = Some(e.tombstone_at.unwrap_or(intent.planned_at_ms));
            e.local_rev = super::runtime_store::next(e.local_rev)?;
            state.change_counter = super::runtime_store::next(state.change_counter)?;
            e.mark = Some(runtime::DirtyMark {
                source: runtime::DirtySource::Observer,
                change_seq: state.change_counter,
                payload_digest: e.payload.payload_digest().map_err(|_| Error::InvalidData)?,
            });
        }
        e
    };
    if e.observer_open != observed.0 || e.observed_seq <= observed.1 {
        e.observer_open = observed.0;
        e.observed_seq = observed.1;
        e.observed_generation = observed.2;
    }
    super::workflow_store::put(tx, intent.binding, &e)
}

/// Durable one-shot grants are separate from per-open retry/apply counts.
pub(super) fn install_asset_reads(tx: &Transaction<'_>) -> Result<(), Error> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_asset_first_reads(
        inbox INTEGER PRIMARY KEY REFERENCES inbox(id) ON DELETE CASCADE,
        binding BLOB NOT NULL REFERENCES bindings(id));",
    )?;
    Ok(())
}

fn asset_read_identity(entry: &InboxEntry) -> Result<[u8; 32], Error> {
    let mut identity = entry.clone();
    // Another pass charging this version does not make it a new arrival.
    identity.retry_count = 0;
    Ok(entity_codec::sha256(encode(&identity)?.as_bytes()))
}

impl TrackingStore {
    pub(super) fn asset_read_pass_members(
        &self,
        b: BindingId,
    ) -> Result<std::collections::BTreeMap<i64, [u8; 32]>, Error> {
        // Inbox rowids may be reused after replacement. Retain only fixed-size
        // identity digests, then page live rows through the existing selector.
        let mut query = self
            .connection
            .prepare("SELECT id,entry FROM inbox WHERE binding=?1 ORDER BY id")?;
        let rows = query.query_map([b.0.as_slice()], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (id, text) = row?;
            Ok((id, asset_read_identity(&decode::<InboxEntry>(&text)?)?))
        })
        .collect()
    }

    pub(super) fn select_asset_read(
        &self,
        b: BindingId,
        kind: AssetReadKind,
        after: i64,
        members: &std::collections::BTreeMap<i64, [u8; 32]>,
        excluded: &std::collections::BTreeSet<i64>,
    ) -> Result<Option<StoredInboxEntry>, Error> {
        let protected: std::collections::BTreeSet<i64> = intents(&self.connection, b)?
            .into_iter()
            .flat_map(|i| i.inbox_ids)
            .collect();
        let mut query = self
            .connection
            .prepare("SELECT id,entry FROM inbox WHERE binding=?1 AND id>?2 AND id<=?3 ORDER BY id")?;
        let through = members.last_key_value().map_or(0, |(id, _)| *id);
        let rows = query.query_map(params![b.0.as_slice(), after, through], |r| {
            Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
        })?;
        for row in rows {
            let (id, text) = row?;
            if excluded.contains(&id) || protected.contains(&id) {
                continue;
            }
            let entry: InboxEntry = decode(&text)?;
            if members.get(&id) != Some(&asset_read_identity(&entry)?) {
                continue;
            }
            if self.asset_read_eligible(b, id, &entry, kind)? {
                return Ok(Some(StoredInboxEntry { id, entry }));
            }
        }
        Ok(None)
    }

    fn asset_read_eligible(
        &self,
        b: BindingId,
        id: i64,
        entry: &InboxEntry,
        kind: AssetReadKind,
    ) -> Result<bool, Error> {
        if entry.record_type != RecordType::Entity || entry.reason != InboxReason::Asset {
            return Ok(false);
        }
        let InboxValue::Record { envelope, .. } = &entry.value else {
            return Ok(false);
        };
        if !matches!(entity_codec::Entity::decode(&envelope.0),
            Ok(entity_codec::DecodeOutcome::Supported(e)) if e.fields.generation.is_none())
        {
            return Ok(false);
        }
        let first: bool = self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_asset_first_reads WHERE binding=?1 AND inbox=?2)",
            params![b.0.as_slice(), id],
            |r| r.get(0),
        )?;
        Ok(match kind {
            AssetReadKind::SaveConflictFirst => first,
            AssetReadKind::Retry => !first && entry.retry_count < 5,
        })
    }

    /// Only called after the synchronous queue has accepted this exact read.
    pub(super) fn admit_asset_read(
        &mut self,
        b: BindingId,
        expected: &StoredInboxEntry,
        kind: AssetReadKind,
    ) -> Result<StoredInboxEntry, Error> {
        if !self.asset_read_current(b, expected)? || !self.asset_read_eligible(b, expected.id, &expected.entry, kind)? {
            return Err(Error::InvalidData);
        }
        let mut admitted = expected.clone();
        let tx = self.connection.transaction()?;
        match kind {
            AssetReadKind::SaveConflictFirst => {
                tx.execute(
                    "DELETE FROM cloud_asset_first_reads WHERE binding=?1 AND inbox=?2",
                    params![b.0.as_slice(), expected.id],
                )?;
            }
            AssetReadKind::Retry => {
                admitted.entry.retry_count += 1;
                tx.execute(
                    "UPDATE inbox SET entry=?3 WHERE binding=?1 AND id=?2",
                    params![b.0.as_slice(), expected.id, encode(&admitted.entry)?],
                )?;
            }
        }
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.tracking.commit.asset_read")?;
        tx.commit()?;
        Ok(admitted)
    }

    pub(super) fn asset_read_current(&self, b: BindingId, expected: &StoredInboxEntry) -> Result<bool, Error> {
        Ok(self.inbox(b)?.iter().any(|entry| entry == expected)
            && !intents(&self.connection, b)?
                .iter()
                .any(|i| i.inbox_ids.contains(&expected.id)))
    }

    pub(super) fn settle_asset_read(
        &mut self,
        b: BindingId,
        expected: &StoredInboxEntry,
        entries: &[InboxEntry],
        refused: &[RefusedEntry],
        same: bool,
    ) -> Result<Vec<StoredInboxEntry>, Error> {
        let tx = self.connection.transaction()?;
        let text: Option<String> = tx
            .query_row(
                "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                params![b.0.as_slice(), expected.id],
                |r| r.get(0),
            )
            .optional()?;
        if text.as_deref().map(decode::<InboxEntry>).transpose()?.as_ref() != Some(&expected.entry)
            || intents(&tx, b)?.iter().any(|i| i.inbox_ids.contains(&expected.id))
        {
            return Err(Error::InvalidData);
        }
        // Failed reads retain all metadata, including the original system fields.
        if !(same && entries.first().is_some_and(|e| e.reason == InboxReason::Asset)) {
            let mut entries = entries.to_vec();
            if same {
                for entry in &mut entries {
                    entry.source = expected.entry.source;
                    entry.absence_pass = expected.entry.absence_pass;
                    entry.fetched_at_ms = expected.entry.fetched_at_ms;
                    entry.retry_count = 0;
                }
            }
            receive_selected(&tx, b, &entries, refused, same.then_some(expected.id))?;
        }
        // Collect retained paths within the transaction so a read failure rolls
        // back before temporary-file destructors can remove referenced data.
        let live = {
            let mut query = tx.prepare("SELECT id,entry FROM inbox WHERE binding=?1 ORDER BY id")?;
            let rows = query.query_map([b.0.as_slice()], |row| {
                Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
            })?;
            rows.map(|row| {
                let (id, entry) = row?;
                Ok(StoredInboxEntry {
                    id,
                    entry: decode(&entry)?,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?
        };
        tx.commit()?;
        Ok(live)
    }

    /// Called only after the runtime proves the actual handler and handed flight.
    pub(super) fn receive_asset_save_conflict(
        &mut self,
        b: BindingId,
        entries: &[InboxEntry],
        refused: &[RefusedEntry],
    ) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        receive(&tx, b, entries, refused)?;
        for original in entries {
            if original.reason != InboxReason::Asset {
                continue;
            }
            let mut entry = original.clone();
            entry.absence_pass = super::adapter_store::running_pass(&tx, b)?;
            let id: Option<i64> = tx
                .query_row(
                    "SELECT id FROM inbox WHERE binding=?1 AND entry=?2",
                    params![b.0.as_slice(), encode(&entry)?],
                    |r| r.get(0),
                )
                .optional()?;
            // A stale result never grants an extra read to an existing fetch entry.
            if let Some(id) = id {
                tx.execute(
                    "INSERT OR IGNORE INTO cloud_asset_first_reads(inbox,binding) VALUES(?1,?2)",
                    params![id, b.0.as_slice()],
                )?;
            }
        }
        tx.commit()?;
        Ok(())
    }
}

// K12.5: delete server copies only. Intents retain their exact inbox ids and
// staged assets; the existing owned-file sweep handles unreferenced files.
pub(super) fn remove_server_cache(tx: &Transaction<'_>, binding: BindingId) -> Result<(), Error> {
    let protected: std::collections::BTreeSet<_> =
        intents(tx, binding)?.into_iter().flat_map(|i| i.inbox_ids).collect();
    let mut q = tx.prepare("SELECT id FROM inbox WHERE binding=?1")?;
    let ids = q
        .query_map([binding.0.as_slice()], |row| row.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    drop(q);
    for id in ids {
        if !protected.contains(&id) {
            tx.execute(
                "DELETE FROM inbox WHERE binding=?1 AND id=?2",
                params![binding.0.as_slice(), id],
            )?;
        }
    }
    tx.execute("DELETE FROM cloud_refused WHERE binding=?1", [binding.0.as_slice()])?;
    Ok(())
}
