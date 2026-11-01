//! Additive stable-layout entity tracking. Notes retain their original table.
use super::{
    entity_codec as codec,
    row_payload::{self, View},
    runtime::{DirtySource, NoteTracking},
    runtime_store::{next, put_binding},
    store::encode,
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
pub(super) fn body_read() {
    BODY_READS.with(|v| v.set(v.get().saturating_add(1)));
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
        if v != [1] && v != [2] {
            return Err(Error::RuntimeSchemaUnsupported);
        }
        let rekey_table: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_rekey_intents' AND type='table')",
            [],
            |r| r.get(0),
        )?;
        if v == [2] {
            if !rekey_table {
                return Err(Error::InvalidData);
            }
            super::rekey_store::validate(c)?;
        } else if rekey_table {
            return Err(Error::InvalidData);
        }
    } else {
        let orphan: bool = c.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_runtime_entities','cloud_rekey_intents'))",
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
        tombstone_at: n.tombstone_at,
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
    value
        .map(|v| super::tracking_payload::load_entity(c, b, kind, &v))
        .transpose()
}
pub(super) fn put(c: &rusqlite::Transaction<'_>, b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    e.validate()?;
    super::tracking_payload::put_entity(c, b, e)
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
        tombstone_at: None,
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
        let tx = self.connection.transaction()?;
        super::rekey_store::install(&tx)?;
        tx.execute("UPDATE cloud_workflow_schema SET version=2", [])?;
        tx.commit()?;
        super::media::store::install(&self.connection)?;
        // Expression indexes are maintained by every note/entity writer, including
        // the legacy note capture path. Dirty discovery reads keys only.
        self.connection.execute_batch("CREATE INDEX IF NOT EXISTS cloud_notes_by_name ON cloud_runtime_notes(binding,json_extract(content,'$.name'));
CREATE INDEX IF NOT EXISTS cloud_entities_by_name ON cloud_runtime_entities(binding,json_extract(content,'$.name'));
CREATE INDEX IF NOT EXISTS cloud_dirty_notes ON cloud_runtime_notes(binding,json_extract(content,'$.name')) WHERE json_extract(content,'$.mark') IS NOT NULL;
CREATE INDEX IF NOT EXISTS cloud_dirty_entities ON cloud_runtime_entities(binding,kind,json_extract(content,'$.name')) WHERE json_extract(content,'$.mark') IS NOT NULL;")?;
        let mut s=self.connection.prepare("SELECT r.content,r.kind,r.key,r.binding FROM cloud_runtime_entities r JOIN bindings b ON b.id=r.binding WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0")?;
        let mut rows = s.query(params![self.scope.container.as_str(), self.scope.environment.code()])?;
        while let Some(r) = rows.next()? {
            let owner = super::store::binding_id(r.get(3)?)?;
            let kind = codec::Kind::try_from(u64::try_from(r.get::<_, i64>(1)?).map_err(|_| Error::InvalidData)?)
                .map_err(|_| Error::InvalidData)?;
            let e = super::tracking_payload::load_entity(&self.connection, owner, kind, &r.get::<_, String>(0)?)?;
            e.validate()?;
            super::tracking_payload::validate_flights(&self.connection, owner, kind, &e.key, &e.in_flight)?;
            super::tracking_payload::validate_config_flights(owner, &e)?;
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
    pub(super) fn entity_image(
        &self,
        b: BindingId,
        kind: codec::Kind,
        key: &[u8],
    ) -> Result<Option<EntityFacts>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        entity(&self.connection, b, kind, key)
    }

    pub(super) fn entity_key_header(
        &self,
        b: BindingId,
        kind: codec::Kind,
        key: &[u8],
    ) -> Result<Option<super::tracking_payload::Header>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let text: Option<String> = if kind == codec::Kind::Note {
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
        };
        text.map(|text| super::tracking_payload::header(kind, &text))
            .transpose()
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
            self.entity_header(b, name).ok().flatten().is_none_or(|h| {
                let e = h.facts;
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
        let Some((kind, text)) = self.entity_header_text(b, name)? else {
            return Ok(None);
        };
        super::tracking_payload::load_entity(&self.connection, b, kind, &text).map(Some)
    }
    /// Explicit full-image diagnostic; normal storage callers use entity_facts.
    pub fn entity_dump(&self, b: BindingId, name: &str) -> Result<Option<EntityFacts>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        self.entity_header_text(b, name)?
            .map(|(kind, text)| super::tracking_payload::dump_entity(&self.connection, b, kind, &text))
            .transpose()
    }
    fn entity_header_text(&self, b: BindingId, name: &str) -> Result<Option<(codec::Kind, String)>, Error> {
        let note: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND json_extract(content,'$.name')=?2",
                params![b.0.as_slice(), name],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(text) = note {
            return Ok(Some((codec::Kind::Note, text)));
        }
        let value: Option<(i64, String)> = self.connection.query_row(
            "SELECT kind,content FROM cloud_runtime_entities WHERE binding=?1 AND json_extract(content,'$.name')=?2",
            params![b.0.as_slice(), name], |r| Ok((r.get(0)?, r.get(1)?))).optional()?;
        value
            .map(|(kind, text)| {
                Ok((
                    codec::Kind::try_from(u64::try_from(kind).map_err(|_| Error::InvalidData)?)
                        .map_err(|_| Error::InvalidData)?,
                    text,
                ))
            })
            .transpose()
    }
    pub(super) fn entity_header(
        &self,
        b: BindingId,
        name: &str,
    ) -> Result<Option<super::tracking_payload::Header>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        self.entity_header_text(b, name)?
            .map(|(kind, text)| super::tracking_payload::header(kind, &text))
            .transpose()
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
        let mut h = self.entity_header(b, name)?.ok_or(Error::InvalidData)?;
        if h.facts.in_flight.len() >= 250 {
            return Err(Error::InvalidData);
        }
        let exists = h.facts.in_flight.iter().any(|f| {
            f.local_rev == flight.local_rev
                && f.payload_digest == flight.payload_digest
                && f.erev == flight.erev
                && f.tombstone == flight.tombstone
                && f.tombstone_envelope_digest == flight.tombstone_envelope_digest
        });
        let tx = self.connection.transaction()?;
        if !exists {
            h.append_flight(&tx, b, &flight)?;
        }
        super::tracking_payload::put_header(&tx, b, &h)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn acknowledge(
        &mut self,
        b: BindingId,
        saved: &Saved,
        received: codec::Received,
        now_ms: i64,
    ) -> Result<bool, Error> {
        if received.entity.fields.tombstone
            && matches!(
                received.entity.fields.kind,
                codec::Kind::Note | codec::Kind::Card | codec::Kind::Config
            )
        {
            return self.acknowledge_row_tombstone(b, saved, received);
        }
        if received.entity.fields.kind == codec::Kind::MediaName && received.entity.fields.tombstone {
            return self.acknowledge_media_tombstone(b, saved, received, now_ms);
        }
        let mut h = self.entity_header(b, &saved.name)?.ok_or(Error::InvalidData)?;
        let e = &h.facts;
        if received.entity.fields.origin != e.origin
            || received.entity.fields.writer != b.0
            || received.entity.fields.tombstone
            || received.entity.fields.generation.is_some()
            || received.payload.is_none()
        {
            return Err(Error::InvalidData);
        }
        let digest = received.entity.payload_sha256;
        let index = e
            .in_flight
            .iter()
            .position(|f| f.payload_digest == digest && f.erev == received.entity.fields.erev)
            .ok_or(Error::InvalidData)?;
        let flight_rev = e.in_flight[index].local_rev;
        // Header payloads can be placeholders. Materialize the complete prior
        // local/ACK images before replacing the header's ACK references.
        let prior_media = if h.facts.kind == codec::Kind::MediaName {
            Some(self.entity_facts(b, &saved.name)?.ok_or(Error::InvalidData)?)
        } else {
            None
        };
        let mut media_ledger = prior_media.as_ref().map(|_| self.media_ledger(b)).transpose()?;
        h.ack(index, saved)?;
        let media_removal_pending = media_ledger.as_ref().is_some_and(|ledger| {
            std::str::from_utf8(&h.facts.key)
                .ok()
                .and_then(|key| ledger.names.get(key))
                .is_some_and(|name| name.tombstone_at.is_some())
        });
        if media_removal_pending {
            h.facts.acked_rev = flight_rev;
        }
        // Undo may have made the row clean against the previous ACK while an
        // older save was still unconfirmed. The new ACK makes that local row
        // pending again; it must remain sendable without another host edit.
        let state = if h.facts.mark.is_none()
            && (media_removal_pending
                || h.facts.tombstone_at.is_some()
                || h.images.local_row_digest != h.facts.acked_row_digest)
        {
            let mut state = self.runtime_binding(b)?;
            if h.facts.local_rev <= flight_rev {
                h.facts.local_rev = next(flight_rev)?;
            }
            state.change_counter = next(state.change_counter)?;
            h.facts.mark = Some(runtime::DirtyMark {
                source: DirtySource::Observer,
                change_seq: state.change_counter,
                payload_digest: h.local_digest(),
            });
            Some(state)
        } else {
            None
        };
        let pending = h.facts.mark.is_some();
        let tx = self.connection.transaction()?;
        if let Some(ledger) = &mut media_ledger {
            super::media::store::update_blob_reference_age(&tx, b, ledger, now_ms)?;
        }
        super::tracking_payload::put_header(&tx, b, &h)?;
        if let Some(ledger) = &mut media_ledger {
            super::media::store::update_blob_reference_age(&tx, b, ledger, now_ms)?;
            super::media::store::put_ledger(&tx, b, ledger)?;
        }
        if let Some(state) = state {
            put_binding(&tx, b, &state)?;
        }
        super::adapter_store::mark_ack(&tx, b, &h.facts.name)?;
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
            tombstone_at: None,
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
            retained_tombstones: Vec::new(),
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
            || (!commit.changes.notetypes.is_empty()
                && commit.changes.source != meguri_model::changes::OpSource::CloudRekey)
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
        let tx = self.connection.transaction().map_err(Error::from)?;
        let incoming = super::incoming_store::intents(&tx, b)?;
        let applying = matches!(
            commit.changes.source,
            meguri_model::changes::OpSource::CloudIncoming | meguri_model::changes::OpSource::CloudRekey
        );
        if applying {
            for mut intent in incoming.iter().filter(|i| i.result_seq.is_none()).cloned() {
                let rekey = super::rekey_store::get(&tx, intent.id)?.is_some();
                if rekey != (commit.changes.source == meguri_model::changes::OpSource::CloudRekey) {
                    continue;
                }
                intent.result_seq = Some(commit.seq.0);
                tx.execute(
                    "UPDATE cloud_apply_intents SET content=?2 WHERE id=?1",
                    params![intent.id, encode(&intent)?],
                )
                .map_err(Error::from)?;
            }
        }
        // add_deck may create several ancestors, in child-to-parent order.
        // Resolve their native names against this same committed writer view,
        // including rows that have not reached tracking yet.
        let mut added_decks = Vec::new();
        for row_key in commit.payload.row_keys(Table::Decks) {
            let key = row_payload::tracking_key(codec::Kind::Deck, row_key)?;
            if applying
                && let Some(protected) = incoming
                    .iter()
                    .flat_map(|i| &i.keys)
                    .find(|k| k.kind == codec::Kind::Deck && k.key == key)
            {
                if protected.before_digest.is_none() && protected.after_digest.is_some() {
                    added_decks.push(key);
                }
                continue;
            }
            let delta = commit.payload.row(Table::Decks, row_key)?.ok_or(Error::InvalidData)?;
            if delta.before.is_none() && matches!(delta.after, meguri_engine::changes::observers::RowAfter::Row(_)) {
                added_decks.push(key);
            }
        }
        let mut written = 0;
        for kind in row_payload::KINDS {
            let Some(table) = row_payload::table(kind) else {
                continue;
            };
            for row_key in commit.payload.row_keys(table) {
                let key = row_payload::tracking_key(kind, row_key)?;
                let protected = incoming
                    .iter()
                    .flat_map(|i| &i.keys)
                    .find(|k| k.kind == kind && k.key == key);
                if applying && protected.is_some() {
                    continue;
                }
                let delta = commit.payload.row(table, row_key)?.ok_or(Error::InvalidData)?;
                let raw = rows.row_image(table, &delta.key)?;
                let actual_key = match raw.as_ref().or(delta.before.as_ref()) {
                    Some(r) => row_payload::key(kind, r)?,
                    None => return Err(Error::InvalidData.into()),
                };
                if actual_key != key {
                    return Err(Error::InvalidData.into());
                }
                let existing = entity(&tx, b, kind, &key)?;
                if raw.is_none() {
                    // Only explicit committed removals create tombstones. A
                    // missing scan row never supplies a deletion or its base.
                    let Some(mut e) = existing else {
                        if matches!(kind, codec::Kind::Note | codec::Kind::Card) && delta.before.is_none() {
                            continue; // created and removed within this commit
                        }
                        return Err(Error::InvalidData.into());
                    };
                    if matches!(kind, codec::Kind::Note | codec::Kind::Card) {
                        if e.server_erev == 0 && e.in_flight.is_empty() {
                            remove_entity(&tx, b, &e)?;
                            continue;
                        }
                        let view = BeforeCommitView {
                            inner: CommitView {
                                c: &tx,
                                b,
                                rows,
                                added_decks: &added_decks,
                            },
                            commit,
                        };
                        let before = delta.before.as_ref().ok_or(Error::InvalidData)?;
                        let payload = row_payload::project(&view, kind, before, |id| layout(&tx, b, id))?;
                        // A fixed clock (or a raw incoming op) can leave col
                        // unchanged, so the journal has no CollectionMark.
                        // Read the actual same-commit metadata, never a made-up
                        // deletion time or an earlier tracking timestamp.
                        let at = match commit.collection {
                            Some(mark) => mark.col_mod,
                            None => row_payload::int(
                                &rows.row_image(Table::Col, &RowKey::Id(1))?.ok_or(Error::InvalidData)?,
                                2,
                            )?,
                        };
                        e.remove(payload, at, &mut state.change_counter)?;
                        e.observer_open = open;
                        e.observed_seq = commit.seq.0;
                        e.observed_generation = commit.generation.0;
                        put(&tx, b, &e)?;
                        written += 1;
                        hook(written)?;
                        continue;
                    }
                    if kind != codec::Kind::Revlog || e.server_erev != 0 || !e.in_flight.is_empty() {
                        return Err(Error::InvalidData.into());
                    }
                    remove_entity(&tx, b, &e)?;
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
                    let ordinary_addition = matches!(
                        kind,
                        codec::Kind::Note | codec::Kind::Card | codec::Kind::Deck | codec::Kind::Config
                    ) && delta.before.is_none();
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
        let drop_row = sent.tombstone
            && matches!(current.kind, codec::Kind::Note | codec::Kind::Card)
            && current.tombstone_at.is_some()
            && current.local_rev == flight.local_rev;
        if current.server_erev == 0 && !drop_row {
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
        let disposition = if drop_media || drop_row {
            // Settle only this unchanged, explicitly removed handed revision;
            // no collection operation or Saved receipt is fabricated.
            remove_entity(&transaction, binding, &current)?;
            transaction.execute(
                "DELETE FROM inbox WHERE binding=?1 AND json_extract(entry,'$.record_name')=?2 AND json_extract(entry,'$.value')='Deletion'",
                params![binding.0.as_slice(), record_name],
            )?;
            transaction.execute(
                "DELETE FROM cloud_absence_seen WHERE binding=?1 AND name=?2",
                params![binding.0.as_slice(), record_name],
            )?;
            if drop_media {
                media_state
                    .names
                    .remove(std::str::from_utf8(&current.key).map_err(|_| Error::InvalidData)?);
                media_state.revision = next(media_state.revision)?;
                transaction.execute(
                    "UPDATE cloud_media_state SET content=?2 WHERE binding=?1",
                    params![binding.0.as_slice(), encode(&media_state)?],
                )?;
            }
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

pub(super) fn remove_entity(tx: &rusqlite::Transaction<'_>, b: BindingId, e: &EntityFacts) -> Result<(), Error> {
    let removed = super::tracking_payload::removed_images(tx, b, e.kind, &e.key)?;
    if e.kind == codec::Kind::Note {
        let RowKey::Id(id) = row_key(e.kind, &e.key)? else {
            return Err(Error::InvalidData);
        };
        tx.execute(
            "DELETE FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![b.0.as_slice(), id],
        )?;
    } else {
        tx.execute(
            "DELETE FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![b.0.as_slice(), e.kind as i64, e.key],
        )?;
    }
    super::tracking_payload::collect_removed(tx, b, &e.name, removed)
}

/// Projection before this exact commit, including a card's deleted note and
/// same-op history. Unchanged rows still come from the committed writer view.
struct BeforeCommitView<'a, 'r, 'c> {
    inner: CommitView<'a, 'r>,
    commit: &'a meguri_engine::ObservedCommit<'c>,
}
impl View for BeforeCommitView<'_, '_, '_> {
    fn row(&self, table: Table, key: &RowKey) -> Result<Option<RowImage>, Error> {
        for delta in self.commit.payload.rows(table) {
            let delta = delta?;
            if &delta.key == key {
                return Ok(delta.before);
            }
        }
        self.inner.row(table, key)
    }
    fn page(&self, table: Table, after: Option<&RowKey>) -> Result<Vec<RowImage>, Error> {
        self.inner.page(table, after)
    }
    fn parts(&self, table: Table, id: i64, after: Option<i64>) -> Result<Vec<RowImage>, Error> {
        self.inner.parts(table, id, after)
    }
    fn history(&self, id: i64) -> Result<i64, Error> {
        let mut maximum = self.inner.history(id)?;
        let mut changed_maximum = false;
        for delta in self.commit.payload.rows(Table::Revlog) {
            let delta = delta?;
            if delta.key == RowKey::Id(maximum) {
                changed_maximum = true;
            }
            if let Some(row) = delta.before
                && row_payload::int(&row, 1)? == id
            {
                maximum = maximum.max(row_payload::int(&row, 0)?);
            }
        }
        if !changed_maximum {
            return Ok(maximum);
        }
        maximum = 0;
        let mut after = None;
        loop {
            let page = keys(self.inner.c, self.inner.b, codec::Kind::Revlog, after.as_deref())?;
            if page.is_empty() {
                break;
            }
            for key in &page {
                if let Some(row) = self.row(Table::Revlog, &row_key(codec::Kind::Revlog, key)?)?
                    && row_payload::int(&row, 1)? == id
                {
                    maximum = maximum.max(row_payload::int(&row, 0)?);
                }
            }
            after = page.last().cloned();
        }
        Ok(maximum)
    }
}

impl TrackingStore {
    fn acknowledge_row_tombstone(
        &mut self,
        b: BindingId,
        saved: &Saved,
        received: codec::Received,
    ) -> Result<bool, Error> {
        let mut e = self.entity_facts(b, &saved.name)?.ok_or(Error::InvalidData)?;
        let remote = &received.entity;
        if remote.fields.origin != e.origin
            || remote.fields.writer != b.0
            || remote.fields.kind != e.kind
            || remote.fields.key != e.key
            || remote.fields.generation.is_some()
            || remote.fields.erev <= e.server_erev
            || remote.payload.as_deref() != Some(&[])
            || received.payload.is_some()
        {
            return Err(Error::InvalidData);
        }
        let flight = e
            .own_row_tombstone_flight(b.0, remote, &saved.envelope)
            .cloned()
            .ok_or(Error::InvalidData)?;
        let mut state = self.runtime_binding(b)?;
        // An ACK of an older deletion never clears a newer native revision.
        e.server_erev = flight.erev;
        e.acked_rev = flight.local_rev;
        e.acked_payload = Some(super::tracking_payload::flight_payload(&self.connection, b, &flight)?);
        e.acked_row_digest = flight.row_digest;
        e.acked_payload_digest = flight.payload_digest;
        e.acked_envelope = OpaqueBytes(saved.envelope.clone());
        e.acked_system_fields = saved.system_fields.clone();
        e.in_flight.retain(|f| f.local_rev > flight.local_rev);
        if e.tombstone_at.is_some() && e.local_rev == flight.local_rev {
            e.mark = None;
            e.base_payload = None;
        } else if e.tombstone_at.is_none() && e.mark.is_none() {
            // An exact old-row Undo can have matched the previous live ACK.
            // The landed tombstone is a real ACK, but the restored row still
            // needs rekey; never report that row clean or send it under this id.
            if e.local_rev <= flight.local_rev {
                e.local_rev = next(flight.local_rev)?;
            }
            state.change_counter = next(state.change_counter)?;
            e.mark = Some(runtime::DirtyMark {
                source: DirtySource::Observer,
                change_seq: state.change_counter,
                payload_digest: e.payload.payload_digest().map_err(|_| Error::InvalidData)?,
            });
        }
        let pending = e.mark.is_some();
        let tx = self.connection.transaction()?;
        put(&tx, b, &e)?;
        put_binding(&tx, b, &state)?;
        super::adapter_store::mark_ack(&tx, b, &e.name)?;
        tx.commit()?;
        Ok(pending)
    }
}
