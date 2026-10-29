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
    if let Some((kind, table)) = index {
        if kind != "index" || table != "cloud_apply_intents" {
            return Err(Error::InvalidData);
        }
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
            return Ok(Some(if notes {
                super::workflow_store::note_facts(decode(&text)?)?
            } else {
                decode(&text)?
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

/// Every caller (SDK and non-SDK) uses these same semantic ordering rules.
pub(super) fn receive(
    tx: &Transaction<'_>,
    b: BindingId,
    entries: &[InboxEntry],
    refused: &[RefusedEntry],
) -> Result<(), Error> {
    let protected: std::collections::BTreeSet<i64> = intents(tx, b)?.into_iter().flat_map(|i| i.inbox_ids).collect();
    for refusal in refused {
        let local = facts(tx, b, &refusal.record_name)?;
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
    for e in entries {
        validate_entry(e)?;
        let local = facts(tx, b, &e.record_name)?;
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
                .any(|(_, old)| version(old).is_some_and(|(o, v)| o == origin && v >= erev))
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
    ) -> Result<(), Error> {
        if self.binding(b)?.ok_or(Error::BindingNotFound)?.facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        let tx = self.connection.transaction()?;
        receive(&tx, b, entries, refused)?;
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
        let entries = q
            .query_map([b.0.as_slice()], |r| r.get::<_, String>(0))?
            .map(|r| decode(&r?))
            .collect();
        entries
    }
    pub(super) fn persist_apply_intent(&mut self, i: &ApplyIntent) -> Result<i64, Error> {
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
                    failure: ApplyFailure::Apply,
                    error: OpaqueBytes(b"collection_apply".to_vec()),
                }
            };
            tx.execute("UPDATE inbox SET entry=?2 WHERE id=?1", params![id, encode(&e)?])?;
        }
        for i in intents(&tx, b)? {
            if i.result_seq.is_none() && i.inbox_ids.iter().any(|id| ids.contains(id)) {
                tx.execute("DELETE FROM cloud_apply_intents WHERE id=?1", [i.id])?;
            }
        }
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
            if e.observer_open == open && e.observed_seq > seq {
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
        tx.execute(
            "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), i.id],
        )?;
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
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(i.binding)?;
        let tx = self.connection.transaction()?;
        for key in &i.keys {
            let prior = facts(&tx, i.binding, &key.name)?;
            let observed = current.get(&key.name).ok_or(Error::InvalidData)?.clone();
            // A newer observer may already have run after the covered snapshot.
            let local = prior
                .as_ref()
                .filter(|e| e.observer_open == open && e.observed_generation == generation && e.observed_seq > seq)
                .map(|e| Some(e.payload.clone()))
                .unwrap_or(observed)
                .ok_or(Error::InvalidData)?;
            let mut e = if let Some(e) = prior {
                e
            } else {
                workflow::EntityFacts {
                    name: key.name.clone(),
                    kind: key.kind,
                    key: key.key.clone(),
                    origin: [0; 16],
                    local_rev: 1,
                    acked_rev: 0,
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
                let entry: InboxEntry = decode(&text)?;
                let InboxValue::Record {
                    envelope,
                    system_fields,
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
                let payload = entity_codec::Payload::decode(
                    server.fields.kind,
                    server.payload.as_deref().ok_or(Error::InvalidData)?,
                )
                .map_err(|_| Error::InvalidData)?;
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
            if e.observer_open != open || e.observed_seq <= seq {
                e.observer_open = open;
                e.observed_seq = seq;
                e.observed_generation = generation;
            }
            super::workflow_store::put(&tx, i.binding, &e)?;
        }
        super::runtime_store::put_binding(&tx, i.binding, &state)?;
        for id in &i.inbox_ids {
            tx.execute(
                "DELETE FROM inbox WHERE binding=?1 AND id=?2",
                params![i.binding.0.as_slice(), id],
            )?;
        }
        tx.execute(
            "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), i.id],
        )?;
        meguri_model::faultpoint!("cloud.tracking.commit.ack")?;
        tx.commit()?;
        Ok(())
    }
}
