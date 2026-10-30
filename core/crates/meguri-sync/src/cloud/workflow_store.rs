//! Additive stable-layout entity tracking. Notes retain their original table.
use super::{
    entity_codec as codec,
    row_payload::{self, View},
    runtime::{DirtySource, NoteAcknowledgement, NoteTracking},
    runtime_store::{next, put_binding},
    store::{decode, encode},
    workflow::*,
    *,
};
use meguri_storage::{RowImage, RowKey, Table};
use rusqlite::{Connection, OptionalExtension, params};
#[cfg(feature = "test-hooks")]
thread_local! { static BODY_READS: std::cell::Cell<usize> = const { std::cell::Cell::new(0) }; }
#[cfg(feature = "test-hooks")]
pub(super) fn body_reads() -> usize {
    BODY_READS.with(std::cell::Cell::get)
}
#[cfg(feature = "test-hooks")]
fn body_read() {
    BODY_READS.with(|v| v.set(v.get().saturating_add(1)));
}
fn generic_facts(content: &str) -> Result<EntityFacts, Error> {
    #[cfg(feature = "test-hooks")]
    body_read();
    decode(content)
}
const SCHEMA: &str = "CREATE TABLE IF NOT EXISTS cloud_workflow_schema(version INTEGER NOT NULL); INSERT INTO cloud_workflow_schema SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM cloud_workflow_schema); CREATE TABLE IF NOT EXISTS cloud_runtime_entities(binding BLOB NOT NULL REFERENCES bindings(id),kind INTEGER NOT NULL,key BLOB NOT NULL,content TEXT NOT NULL CHECK(json_valid(content)),PRIMARY KEY(binding,kind,key));";
pub(super) fn check_version(c: &Connection) -> Result<(), Error> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_workflow_schema')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let mut s = c.prepare("SELECT version FROM cloud_workflow_schema")?;
        let v = s
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if v != [1] {
            return Err(Error::RuntimeSchemaUnsupported);
        }
    } else {
        let orphan: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_entities')",
            [],
            |r| r.get(0),
        )?;
        if orphan {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}
pub(super) fn row_key(kind: codec::Kind, key: &[u8]) -> Result<RowKey, Error> {
    Ok(if kind as u8 <= 6 {
        RowKey::Id(i64::from_be_bytes(key.try_into().map_err(|_| Error::InvalidData)?))
    } else {
        RowKey::text(std::str::from_utf8(key).map_err(|_| Error::InvalidData)?)
    })
}
pub(super) fn note_facts(n: NoteTracking) -> Result<EntityFacts, Error> {
    #[cfg(feature = "test-hooks")]
    body_read();
    let p = codec::Payload::decode(codec::Kind::Note, &n.local_payload.0).map_err(|_| Error::InvalidData)?;
    let ack = if n.acknowledged.erev == 0 {
        None
    } else {
        Some(codec::Payload::decode(codec::Kind::Note, &n.acknowledged.payload.0).map_err(|_| Error::InvalidData)?)
    };
    Ok(EntityFacts {
        name: n.name,
        kind: codec::Kind::Note,
        key: codec::id_key(n.id).to_vec(),
        origin: n.origin,
        local_rev: n.local_rev,
        acked_rev: n.acked_rev,
        payload: p,
        base_payload: n
            .base
            .map(|v| codec::Payload::decode(codec::Kind::Note, &v.0).map_err(|_| Error::InvalidData))
            .transpose()?,
        mark: n.mark,
        server_erev: n.acknowledged.erev,
        acked_system_fields: n.acknowledged.system_fields.0,
        acked_row_digest: n.acknowledged.row_digest,
        acked_payload_digest: n.acknowledged.payload_digest,
        acked_payload: ack,
        acked_envelope: n.acked_envelope,
        in_flight: n.in_flight,
        observer_open: n.observer_open,
        observed_seq: n.observed_seq,
        observed_generation: n.observed_generation,
    })
}
pub(super) fn entity(
    c: &Connection,
    b: BindingId,
    kind: codec::Kind,
    key: &[u8],
) -> Result<Option<EntityFacts>, Error> {
    if kind == codec::Kind::Note {
        let RowKey::Id(id) = row_key(kind, key)? else {
            return Err(Error::InvalidData);
        };
        return super::runtime_store::note(c, b, id)?.map(note_facts).transpose();
    }
    let value: Option<String> = c
        .query_row(
            "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![b.0.as_slice(), kind as i64, key],
            |r| r.get(0),
        )
        .optional()?;
    value.map(|v| generic_facts(&v)).transpose()
}
pub(super) fn put(c: &rusqlite::Transaction<'_>, b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    e.validate()?;
    if e.kind == codec::Kind::Note {
        let codec::Payload::Note(p) = &e.payload else {
            return Err(Error::InvalidData);
        };
        let ack = e.acked_payload.as_ref();
        let n = NoteTracking {
            id: p.id,
            name: e.name.clone(),
            origin: e.origin,
            local_rev: e.local_rev,
            last_observed_digest: Some(e.payload.row_digest().map_err(|_| Error::InvalidData)?),
            local_payload: OpaqueBytes(e.payload.encode().map_err(|_| Error::InvalidData)?),
            base: e
                .base_payload
                .as_ref()
                .map(|p| p.encode().map(OpaqueBytes).map_err(|_| Error::InvalidData))
                .transpose()?,
            mark: e.mark.clone(),
            acknowledged: NoteAcknowledgement {
                erev: e.server_erev,
                system_fields: OpaqueBytes(e.acked_system_fields.clone()),
                payload: OpaqueBytes(
                    ack.map(|p| p.encode().map_err(|_| Error::InvalidData))
                        .transpose()?
                        .unwrap_or_default(),
                ),
                row_digest: e.acked_row_digest,
                payload_digest: e.acked_payload_digest,
            },
            observer_open: e.observer_open,
            observed_seq: e.observed_seq,
            observed_generation: e.observed_generation,
            acked_rev: e.acked_rev,
            in_flight: e.in_flight.clone(),
            acked_envelope: e.acked_envelope.clone(),
        };
        c.execute("INSERT INTO cloud_runtime_notes(binding,id,content) VALUES(?1,?2,?3) ON CONFLICT(binding,id) DO UPDATE SET content=excluded.content",params![b.0.as_slice(),p.id,encode(&n)?])?;
        return Ok(());
    }
    c.execute("INSERT INTO cloud_runtime_entities(binding,kind,key,content) VALUES(?1,?2,?3,?4) ON CONFLICT(binding,kind,key) DO UPDATE SET content=excluded.content",params![b.0.as_slice(),e.kind as i64,&e.key,encode(e)?])?;
    Ok(())
}
pub(super) fn new_pending_entity(
    record_key: &RecordKey,
    payload: codec::Payload,
    origin: [u8; 16],
    source: DirtySource,
    counter: &mut u64,
    observed: (u64, u64, u64),
) -> Result<EntityFacts, Error> {
    let kind = payload.kind();
    let key = row_payload::payload_key(&payload)?;
    let name = codec::entity_name(&record_key.0, kind, &key).map_err(|_| Error::InvalidData)?;
    let payload_digest = payload.payload_digest().map_err(|_| Error::InvalidData)?;
    *counter = next(*counter)?;
    Ok(EntityFacts {
        name,
        kind,
        key,
        origin,
        local_rev: 1,
        acked_rev: 0,
        payload,
        base_payload: None,
        mark: Some(runtime::DirtyMark {
            source,
            change_seq: *counter,
            payload_digest,
        }),
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
    })
}
fn keys(c: &Connection, b: BindingId, kind: codec::Kind, after: Option<&[u8]>) -> Result<Vec<Vec<u8>>, Error> {
    if kind == codec::Kind::Note {
        let after = after.map(|k| i64::from_be_bytes(k.try_into().unwrap_or([0; 8])));
        let mut s = c.prepare(
            "SELECT id FROM cloud_runtime_notes WHERE binding=?1 AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT 250",
        )?;
        return Ok(s
            .query_map(params![b.0.as_slice(), after], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(|id| codec::id_key(id).to_vec())
            .collect());
    }
    let mut s=c.prepare("SELECT key FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND (?3 IS NULL OR key>?3) ORDER BY key LIMIT 250")?;
    Ok(s.query_map(params![b.0.as_slice(), kind as i64, after], |r| r.get(0))?
        .collect::<Result<Vec<_>, _>>()?)
}
impl TrackingStore {
    pub(super) fn install_workflow(&mut self) -> Result<(), Error> {
        check_version(&self.connection)?;
        self.connection.execute_batch(SCHEMA)?;
        super::media::store::install(&self.connection)?;
        // Expression indexes are maintained by every note/entity writer, including
        // the legacy note capture path. Dirty discovery reads keys only.
        self.connection.execute_batch("CREATE INDEX IF NOT EXISTS cloud_notes_by_name ON cloud_runtime_notes(binding,json_extract(content,'$.name'));
CREATE INDEX IF NOT EXISTS cloud_entities_by_name ON cloud_runtime_entities(binding,json_extract(content,'$.name'));
CREATE INDEX IF NOT EXISTS cloud_dirty_notes ON cloud_runtime_notes(binding,json_extract(content,'$.name')) WHERE json_extract(content,'$.mark') IS NOT NULL;
CREATE INDEX IF NOT EXISTS cloud_dirty_entities ON cloud_runtime_entities(binding,kind,json_extract(content,'$.name')) WHERE json_extract(content,'$.mark') IS NOT NULL;")?;
        let mut s=self.connection.prepare("SELECT r.content,r.kind,r.key FROM cloud_runtime_entities r JOIN bindings b ON b.id=r.binding WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0")?;
        let mut rows = s.query(params![self.scope.container.as_str(), self.scope.environment.code()])?;
        while let Some(r) = rows.next()? {
            let e: EntityFacts = decode(&r.get::<_, String>(0)?)?;
            e.validate()?;
            if e.kind as i64 != r.get::<_, i64>(1)? || e.key != r.get::<_, Vec<u8>>(2)? {
                return Err(Error::InvalidData);
            }
        }
        Ok(())
    }
    pub(super) fn entity_keys_page(
        &self,
        b: BindingId,
        kind: codec::Kind,
        after: Option<&[u8]>,
    ) -> Result<Vec<Vec<u8>>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        keys(&self.connection, b, kind, after)
    }
    pub(super) fn entity_content(&self, b: BindingId, kind: codec::Kind, key: &[u8]) -> Result<Option<String>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        Ok(if kind == codec::Kind::Note {
            let RowKey::Id(id) = row_key(kind, key)? else {
                return Err(Error::InvalidData);
            };
            self.connection
                .query_row(
                    "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
                    params![b.0.as_slice(), id],
                    |r| r.get(0),
                )
                .optional()?
        } else {
            self.connection
                .query_row(
                    "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
                    params![b.0.as_slice(), kind as i64, key],
                    |r| r.get(0),
                )
                .optional()?
        })
    }
    pub(super) fn dirty_names(&self, b: BindingId) -> Result<PendingSnapshot, Error> {
        let mut names = std::collections::BTreeMap::new();
        let mut notes = self.connection.prepare("SELECT json_extract(content,'$.name') FROM cloud_runtime_notes INDEXED BY cloud_dirty_notes WHERE binding=?1 AND json_extract(content,'$.mark') IS NOT NULL")?;
        for name in notes.query_map([b.0.as_slice()], |r| r.get::<_, String>(0))? {
            names.insert(name?, PendingChange::Save);
        }
        let publishing = self.publication_op()?.is_some_and(|op| {
            op.binding_after == Some(b)
                && matches!(
                    op.step,
                    super::publication::PublishStep::Zones
                        | super::publication::PublishStep::Sent
                        | super::publication::PublishStep::Complete
                )
        });
        let mut entities = self.connection.prepare("SELECT json_extract(content,'$.name') FROM cloud_runtime_entities INDEXED BY cloud_dirty_entities WHERE binding=?1 AND (kind IN (2,3,4,8,9) OR ?2) AND json_extract(content,'$.mark') IS NOT NULL")?;
        for name in entities.query_map(params![b.0.as_slice(), publishing], |r| r.get::<_, String>(0))? {
            names.insert(name?, PendingChange::Save);
        }
        let media = self.media_ledger(b)?;
        names.retain(|name, _| {
            self.entity_facts(b, name).ok().flatten().is_none_or(|e| {
                e.kind != codec::Kind::MediaName
                    || std::str::from_utf8(&e.key)
                        .ok()
                        .and_then(|key| media.names.get(key))
                        .is_none_or(|f| f.sendable())
            })
        });
        let blocked: Vec<_> = names
            .keys()
            .filter_map(|name| match self.incoming_blocks_name(b, name) {
                Ok(true) => Some(Ok(name.clone())),
                Ok(false) => None,
                Err(e) => Some(Err(e)),
            })
            .collect::<Result<_, Error>>()?;
        for name in blocked {
            names.remove(&name);
        }
        Ok(PendingSnapshot(names))
    }
    pub(super) fn workflow_has_layouts(&self, b: BindingId) -> Result<bool, Error> {
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_runtime_entities WHERE binding=?1 AND kind=6)",
            [b.0.as_slice()],
            |r| r.get(0),
        )?)
    }
    pub fn entity_page(
        &self,
        b: BindingId,
        kind: codec::Kind,
        after: Option<&[u8]>,
    ) -> Result<Vec<EntityFacts>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        keys(&self.connection, b, kind, after)?
            .into_iter()
            .map(|k| entity(&self.connection, b, kind, &k)?.ok_or(Error::InvalidData))
            .collect()
    }
    pub fn entity_facts(&self, b: BindingId, name: &str) -> Result<Option<EntityFacts>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let note: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND json_extract(content,'$.name')=?2",
                params![b.0.as_slice(), name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(note) = note {
            return Ok(Some(note_facts(decode(&note)?)?));
        }
        let generic: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND json_extract(content,'$.name')=?2",
                params![b.0.as_slice(), name],
                |r| r.get(0),
            )
            .optional()?;
        generic.map(|v| generic_facts(&v)).transpose()
    }
    pub(super) fn set_workflow_drained(
        &mut self,
        b: BindingId,
        complete: bool,
        metadata: (i64, i64),
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(b)?;
        if complete
            && !state
                .coverage
                .as_ref()
                .is_some_and(|c| c.open == state.open_counter && Some(c.cursor.native_file) == state.file)
        {
            return Err(Error::InvalidData);
        }
        state.drained = complete;
        state.whole_scan = complete;
        state.close_metadata = if complete { Some(metadata) } else { None };
        let tx = self.connection.transaction()?;
        put_binding(&tx, b, &state)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn insert_workflow_entity(
        &mut self,
        b: BindingId,
        payload: codec::Payload,
        origin: [u8; 16],
        observed: (u64, u64, u64),
    ) -> Result<EntityFacts, Error> {
        // Ordinary additions may use an existing layout, never introduce one.
        if !matches!(payload.kind(), codec::Kind::Note | codec::Kind::Card) {
            return Err(Error::InvalidData);
        }
        let binding = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let mut state = self.runtime_binding(b)?;
        if state.open_counter != observed.0 {
            return Err(Error::InvalidData);
        }
        let key = row_payload::payload_key(&payload)?;
        if let Some(existing) = entity(&self.connection, b, payload.kind(), &key)? {
            // Capture may have tracked a newer commit since the scan's read.
            return Ok(existing);
        }
        let e = new_pending_entity(
            &binding.facts.record_key,
            payload,
            origin,
            DirtySource::Snapshot,
            &mut state.change_counter,
            observed,
        )?;
        let tx = self.connection.transaction()?;
        put(&tx, b, &e)?;
        put_binding(&tx, b, &state)?;
        tx.commit()?;
        Ok(e)
    }
    pub(super) fn workflow_merge(
        &mut self,
        b: BindingId,
        open: u64,
        seq: u64,
        generation: u64,
        mut image: EntityFacts,
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(b)?;
        if state.open_counter != open {
            return Err(Error::InvalidData);
        }
        let mut current = entity(&self.connection, b, image.kind, &image.key)?.ok_or(Error::InvalidData)?;
        if current.observer_open == open && current.observed_generation == generation && current.observed_seq > seq {
            return Ok(());
        }
        current.observe(image.payload, DirtySource::Snapshot, &mut state.change_counter)?;
        image = current;
        let tx = self.connection.transaction()?;
        put(&tx, b, &image)?;
        put_binding(&tx, b, &state)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn persist_flight(&mut self, b: BindingId, name: &str, flight: Flight) -> Result<(), Error> {
        let mut e = self.entity_facts(b, name)?.ok_or(Error::InvalidData)?;
        if e.in_flight.len() >= 250 {
            return Err(Error::InvalidData);
        }
        if !e
            .in_flight
            .iter()
            .any(|f| f.local_rev == flight.local_rev && f.payload_digest == flight.payload_digest)
        {
            e.in_flight.push(flight);
        }
        let tx = self.connection.transaction()?;
        put(&tx, b, &e)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn acknowledge(
        &mut self,
        b: BindingId,
        saved: &Saved,
        received: codec::Received,
    ) -> Result<bool, Error> {
        if received.entity.fields.kind == codec::Kind::MediaName && received.entity.fields.tombstone {
            return self.acknowledge_media_tombstone(b, saved, received);
        }
        let p = received.payload.ok_or(Error::InvalidData)?;
        let mut e = self.entity_facts(b, &saved.name)?.ok_or(Error::InvalidData)?;
        if received.entity.fields.origin != e.origin
            || received.entity.fields.writer != b.0
            || received.entity.fields.tombstone
            || received.entity.fields.generation.is_some()
        {
            return Err(Error::InvalidData);
        }
        let digest = p.payload_digest().map_err(|_| Error::InvalidData)?;
        let f = e
            .in_flight
            .iter()
            .find(|f| f.payload_digest == digest && f.erev == received.entity.fields.erev)
            .cloned()
            .ok_or(Error::InvalidData)?;
        e.server_erev = f.erev;
        e.acked_rev = f.local_rev;
        e.acked_row_digest = f.row_digest;
        e.acked_payload_digest = digest;
        e.acked_system_fields = saved.system_fields.clone();
        e.acked_envelope = OpaqueBytes(saved.envelope.clone());
        e.acked_payload = Some(p.clone());
        e.in_flight.retain(|v| v.local_rev > f.local_rev);
        if e.payload.row_digest().map_err(|_| Error::InvalidData)? == f.row_digest {
            e.acked_rev = e.local_rev;
            e.mark = None;
            e.base_payload = None;
        } else {
            e.base_payload = Some(p);
        }
        let pending = e.mark.is_some();
        let tx = self.connection.transaction()?;
        put(&tx, b, &e)?;
        super::adapter_store::mark_ack(&tx, b, &e.name)?;
        super::publication_store::advance_sent(&tx, &self.scope, b)?;
        tx.commit()?;
        Ok(pending)
    }
    pub(super) fn finish_workflow_scan(&mut self, b: BindingId, ledger: CoverageLedger) -> Result<(), Error> {
        ledger.validate()?;
        let mut s = self.runtime_binding(b)?;
        if s.open_counter != ledger.open || s.file != Some(ledger.cursor.native_file) {
            return Err(Error::InvalidData);
        }
        s.coverage = Some(ledger);
        let tx = self.connection.transaction()?;
        put_binding(&tx, b, &s)?;
        super::publication_store::advance_sent(&tx, &self.scope, b)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn save_root_ack(&mut self, b: BindingId, ack: RootAcknowledgement) -> Result<(), Error> {
        let mut s = self.runtime_binding(b)?;
        if s.open_counter != ack.open {
            return Err(Error::InvalidData);
        }
        s.root_ack = Some(ack);
        let tx = self.connection.transaction()?;
        put_binding(&tx, b, &s)?;
        tx.commit()?;
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn prepare_entity_baseline_for_testing(
        &mut self,
        b: BindingId,
        name: &str,
        envelope: &[u8],
        system_fields: OpaqueBytes,
    ) -> Result<(), Error> {
        self.install_runtime()?;
        let binding = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if binding.facts.state != BindingState::Active || system_fields.0.is_empty() {
            return Err(Error::InvalidData);
        }
        let r = codec::receive(&binding.facts.record_key.0, name, envelope, codec::AssetInput::Absent)
            .map_err(|_| Error::InvalidData)?;
        let p = r.payload.ok_or(Error::InvalidData)?;
        let f = r.entity.fields;
        if f.tombstone
            || f.generation.is_some()
            || f.erev != 1
            || entity(&self.connection, b, f.kind, &f.key)?.is_some()
        {
            return Err(Error::InvalidData);
        }
        let e = EntityFacts {
            name: name.into(),
            kind: f.kind,
            key: f.key,
            origin: f.origin,
            local_rev: 1,
            acked_rev: 1,
            acked_row_digest: p.row_digest().map_err(|_| Error::InvalidData)?,
            acked_payload_digest: p.payload_digest().map_err(|_| Error::InvalidData)?,
            payload: p.clone(),
            acked_payload: Some(p),
            acked_envelope: OpaqueBytes(envelope.to_vec()),
            acked_system_fields: system_fields.0,
            server_erev: 1,
            base_payload: None,
            mark: None,
            in_flight: Vec::new(),
            observer_open: 0,
            observed_seq: 0,
            observed_generation: 0,
        };
        let tx = self.connection.transaction()?;
        put(&tx, b, &e)?;
        tx.commit()?;
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub fn prepare_drained_baseline_for_testing(
        &mut self,
        b: BindingId,
        file: runtime::NativeFileIdentity,
        cursor: runtime::NoteScanCursor,
        feed_id: [u8; 16],
        media_cursor: u64,
    ) -> Result<(), Error> {
        let mut s = self.runtime_binding(b)?;
        if cursor.native_file != file {
            return Err(Error::InvalidData);
        }
        s.close_metadata = Some((cursor.modified, cursor.schema_modified));
        s.note_scan = Some(cursor.clone());
        s.file = Some(file);
        s.whole_scan = true;
        s.drained = true;
        let counts = row_payload::KINDS
            .into_iter()
            .map(|kind| {
                let mut count = 0;
                let mut after = None;
                loop {
                    let page = keys(&self.connection, b, kind, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    count += page.len() as u64;
                    after = page.last().cloned();
                }
                Ok((kind, count))
            })
            .collect::<Result<Vec<_>, Error>>()?;
        s.coverage = Some(CoverageLedger {
            open: s.open_counter,
            cursor,
            counts,
            media_feed_id: feed_id,
            media_cursor,
        });
        let tx = self.connection.transaction()?;
        put_binding(&tx, b, &s)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn capture_workflow(
        &mut self,
        b: BindingId,
        open: u64,
        commit: &meguri_engine::ObservedCommit<'_>,
        rows: &meguri_engine::ObserverRows<'_>,
        random: &dyn meguri_model::rng::RandomSource,
        mut hook: impl FnMut(usize) -> Result<(), Error>,
    ) -> Result<(), runtime::RuntimeError> {
        if commit.changes.whole_collection
            || !commit.changes.notetypes.is_empty()
            || !commit.changes.media_added.is_empty()
            || !commit.changes.media_removed.is_empty()
        {
            return Err(Error::InvalidData.into());
        }
        let binding = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let mut state = self.runtime_binding(b)?;
        if state.open_counter != open {
            return Err(Error::InvalidData.into());
        }
        // add_deck may create several ancestors, in child-to-parent order.
        // Resolve their native names against this same committed writer view,
        // including rows that have not reached tracking yet.
        let mut added_decks = Vec::new();
        for change in commit.payload.rows(Table::Decks) {
            let delta = change?;
            if delta.before.is_none() && matches!(delta.after, meguri_engine::changes::observers::RowAfter::Row(_)) {
                let RowKey::Id(id) = delta.key else {
                    return Err(Error::InvalidData.into());
                };
                added_decks.push(codec::id_key(id).to_vec());
            }
        }
        let tx = self.connection.transaction().map_err(Error::from)?;
        let incoming = super::incoming_store::intents(&tx, b)?;
        let applying = commit.changes.source == meguri_model::changes::OpSource::CloudIncoming;
        if applying {
            for mut intent in incoming.iter().filter(|i| i.result_seq.is_none()).cloned() {
                intent.result_seq = Some(commit.seq.0);
                tx.execute(
                    "UPDATE cloud_apply_intents SET content=?2 WHERE id=?1",
                    params![intent.id, encode(&intent)?],
                )
                .map_err(Error::from)?;
            }
        }
        let mut written = 0;
        for kind in row_payload::KINDS {
            let Some(table) = row_payload::table(kind) else {
                continue;
            };
            for change in commit.payload.rows(table) {
                let delta = change?;
                let raw = rows.row_image(table, &delta.key)?;
                let key = match raw.as_ref().or(delta.before.as_ref()) {
                    Some(r) => row_payload::key(kind, r)?,
                    None => return Err(Error::InvalidData.into()),
                };
                let protected = incoming
                    .iter()
                    .flat_map(|i| &i.keys)
                    .find(|k| k.kind == kind && k.key == key);
                if applying && protected.is_some() {
                    continue;
                }
                let existing = entity(&tx, b, kind, &key)?;
                if raw.is_none() {
                    let e = existing.ok_or(Error::InvalidData)?;
                    if kind != codec::Kind::Revlog || e.server_erev != 0 || !e.in_flight.is_empty() {
                        return Err(Error::InvalidData.into());
                    }
                    tx.execute(
                        "DELETE FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
                        params![b.0.as_slice(), kind as i64, key],
                    )
                    .map_err(Error::from)?;
                    continue;
                }
                let view = CommitView {
                    c: &tx,
                    b,
                    rows,
                    added_decks: &added_decks,
                };
                let p = row_payload::project(&view, kind, &raw.ok_or(Error::InvalidData)?, |id| layout(&tx, b, id))?;
                let mut e = if let Some(e) = existing {
                    e
                } else {
                    let ordinary_addition = matches!(kind, codec::Kind::Note | codec::Kind::Card | codec::Kind::Deck)
                        && delta.before.is_none();
                    if !ordinary_addition && kind != codec::Kind::Revlog && protected.is_none() {
                        return Err(Error::InvalidData.into());
                    }
                    let mut origin = [0; 16];
                    origin[..8].copy_from_slice(&random.next_u64().to_le_bytes());
                    origin[8..].copy_from_slice(&random.next_u64().to_le_bytes());
                    new_pending_entity(
                        &binding.facts.record_key,
                        p.clone(),
                        origin,
                        DirtySource::Observer,
                        &mut state.change_counter,
                        (open, commit.seq.0, commit.generation.0),
                    )?
                };
                if !matches!(
                    kind,
                    codec::Kind::Note
                        | codec::Kind::Card
                        | codec::Kind::Revlog
                        | codec::Kind::Deck
                        | codec::Kind::Config
                ) && e.payload != p
                {
                    return Err(Error::InvalidData.into());
                }
                if let (codec::Payload::Deck(old), codec::Payload::Deck(new)) = (&e.payload, &p)
                    && (old.kind != new.kind || old.parent_id != new.parent_id)
                {
                    return Err(Error::InvalidData.into());
                }
                if let Some(expected) = protected.and_then(|k| k.after_payload.as_ref()) {
                    // Capture later edits against the incoming after-image,
                    // including an undo that restored the old tracked payload.
                    e.payload = expected.clone();
                }
                e.observe(p, DirtySource::Observer, &mut state.change_counter)?;
                e.observer_open = open;
                e.observed_seq = commit.seq.0;
                e.observed_generation = commit.generation.0;
                put(&tx, b, &e)?;
                written += 1;
                hook(written)?;
            }
        }
        put_binding(&tx, b, &state)?;
        tx.commit().map_err(Error::from)?;
        Ok(())
    }
}
fn layout(c: &Connection, b: BindingId, id: i64) -> Result<codec::Notetype, Error> {
    match entity(c, b, codec::Kind::Notetype, &codec::id_key(id))?.map(|e| e.acked_payload.unwrap_or(e.payload)) {
        Some(codec::Payload::Notetype(n)) if n.struct_gen > 0 => Ok(n),
        _ => Err(Error::InvalidData),
    }
}
struct CommitView<'a, 'r> {
    c: &'a Connection,
    b: BindingId,
    rows: &'a meguri_engine::ObserverRows<'r>,
    added_decks: &'a [Vec<u8>],
}
impl View for CommitView<'_, '_> {
    fn row(&self, t: Table, k: &RowKey) -> Result<Option<RowImage>, Error> {
        Ok(self.rows.row_image(t, k)?)
    }
    fn page(&self, t: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, Error> {
        let kind = row_payload::KINDS
            .into_iter()
            .find(|k| row_payload::table(*k) == Some(t))
            .ok_or(Error::InvalidData)?;
        let start = after
            .map(|k| match k {
                RowKey::Id(id) => Ok(codec::id_key(*id).to_vec()),
                _ => Err(Error::InvalidData),
            })
            .transpose()?;
        let mut page = keys(self.c, self.b, kind, start.as_deref())?;
        if kind == codec::Kind::Deck {
            page.extend(
                self.added_decks
                    .iter()
                    .filter(|key| start.as_ref().is_none_or(|after| *key > after))
                    .cloned(),
            );
            page.sort();
            page.dedup();
            page.truncate(250);
        }
        page.into_iter()
            .map(|k| self.row(t, &row_key(kind, &k)?)?.ok_or(Error::InvalidData))
            .collect()
    }
    fn parts(&self, t: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
        let n = layout(self.c, self.b, id)?;
        let ords: Vec<_> = if t == Table::Fields {
            n.fields.iter().map(|f| f.ord).collect()
        } else {
            n.templates.iter().map(|f| f.ord).collect()
        };
        ords.into_iter()
            .filter(|ord| after.is_none_or(|a| *ord > a))
            .take(250)
            .map(|ord| self.row(t, &RowKey::Pair(id, ord))?.ok_or(Error::InvalidData))
            .collect()
    }
    fn history(&self, id: i64) -> Result<i64, Error> {
        Ok(self.rows.max_revlog_id(meguri_model::ids::CardId(id))?)
    }
}

pub(super) fn decode_entity(kind: codec::Kind, content: &str) -> Result<EntityFacts, Error> {
    if kind == codec::Kind::Note {
        note_facts(decode(content)?)
    } else {
        generic_facts(content)
    }
}

impl TrackingStore {
    /// K17.4.
    pub(super) fn record_unknown_item(
        &mut self,
        binding: BindingId,
        record_name: &str,
        sent: &HandedSave,
        received_at: i64,
    ) -> Result<MissingSaveDisposition, Error> {
        let current = self.entity_facts(binding, record_name)?.ok_or(Error::InvalidData)?;
        if current.origin != sent.origin {
            return Err(Error::InvalidData);
        }
        if current.server_erev >= sent.erev {
            return Ok(MissingSaveDisposition::Outstanding);
        }
        let flight = current
            .in_flight
            .iter()
            .find(|flight| {
                flight.local_rev == sent.local_rev
                    && flight.erev == sent.erev
                    && flight.payload_digest == sent.payload_digest
            })
            .ok_or(Error::InvalidData)?;
        if current.server_erev == 0 {
            return Ok(MissingSaveDisposition::Outstanding);
        }
        // Existing intake may describe a later server state. Preserve it and
        // any apply already using it rather than replacing it with this reply.
        if self.incoming_blocks_name(binding, record_name)?
            || super::incoming_store::intents(&self.connection, binding)?
                .iter()
                .any(|intent| intent.keys.iter().any(|key| key.name == record_name))
        {
            return Ok(MissingSaveDisposition::Outstanding);
        }
        let mut media_state = self.media_ledger(binding)?;
        let drop_media = sent.tombstone
            && current.kind == codec::Kind::MediaName
            && current.local_rev == flight.local_rev
            && media_state
                .names
                .get(std::str::from_utf8(&current.key).map_err(|_| Error::InvalidData)?)
                .is_some_and(|entry| entry.tombstone_at.is_some() && entry.local.is_none());
        let transaction = self.connection.transaction()?;
        let deletion = InboxEntry {
            record_type: RecordType::Entity,
            record_name: record_name.into(),
            value: InboxValue::Deletion,
            reason: InboxReason::Ready,
            source: InboxSource::SaveConflict,
            absence_pass: None,
            retry_count: 0,
            fetched_at_ms: received_at,
        };
        super::incoming_store::receive(&transaction, binding, &[deletion], &[])?;
        let disposition = if drop_media {
            // J6 has already removed the file. Settle only this unchanged
            // handed revision; no collection operation or Saved receipt exists.
            transaction.execute(
                "DELETE FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
                params![binding.0.as_slice(), current.kind as i64, current.key],
            )?;
            transaction.execute(
                "DELETE FROM inbox WHERE binding=?1 AND json_extract(entry,'$.record_name')=?2 AND json_extract(entry,'$.value')='Deletion'",
                params![binding.0.as_slice(), record_name],
            )?;
            transaction.execute(
                "DELETE FROM cloud_absence_seen WHERE binding=?1 AND name=?2",
                params![binding.0.as_slice(), record_name],
            )?;
            media_state
                .names
                .remove(std::str::from_utf8(&current.key).map_err(|_| Error::InvalidData)?);
            media_state.revision = next(media_state.revision)?;
            transaction.execute(
                "UPDATE cloud_media_state SET content=?2 WHERE binding=?1",
                params![binding.0.as_slice(), encode(&media_state)?],
            )?;
            MissingSaveDisposition::TerminalAbsence {
                local_rev: flight.local_rev,
            }
        } else {
            MissingSaveDisposition::Outstanding
        };
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&transaction, "cloud.tracking.commit.inbox")?;
        meguri_model::faultpoint!("cloud.tracking.commit.inbox")?;
        transaction.commit()?;
        Ok(disposition)
    }
}
