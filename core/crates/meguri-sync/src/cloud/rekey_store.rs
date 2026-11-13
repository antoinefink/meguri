//! Paired identity and collection intent persistence.
use super::{
    incoming::*,
    store::{decode, encode},
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Rekey {
    pub format: u32,
    pub binding: BindingId,
    pub kind: entity_codec::Kind,
    pub old: Vec<u8>,
    pub new: Vec<u8>,
    pub old_name: String,
    pub new_name: String,
    pub origin: [u8; 16],
    pub local_rev: u64,
    pub inbox: i64,
    pub server_digest: [u8; 32],
    pub preserve_old: bool,
    pub created: Vec<(String, [u8; 16])>,
    pub struct_gen: Option<u64>,
}

pub(super) fn install(tx: &Transaction<'_>) -> Result<(), Error> {
    tx.execute_batch(
        "CREATE TABLE IF NOT EXISTS cloud_rekey_intents(
        id INTEGER PRIMARY KEY REFERENCES cloud_apply_intents(id) ON DELETE CASCADE,
        content TEXT NOT NULL CHECK(json_valid(content)));",
    )?;
    Ok(())
}
pub(super) fn get(c: &Connection, id: i64) -> Result<Option<Rekey>, Error> {
    let text: Option<String> = c
        .query_row("SELECT content FROM cloud_rekey_intents WHERE id=?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    text.map(|v| decode(&v)).transpose()
}
pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    let mut q = c.prepare("SELECT id,content FROM cloud_rekey_intents")?;
    let mut rows = q.query([])?;
    while let Some(row) = rows.next()? {
        let id: i64 = row.get(0)?;
        let r: Rekey = decode(&row.get::<_, String>(1)?)?;
        let text: String = c.query_row("SELECT content FROM cloud_apply_intents WHERE id=?1", [id], |r| {
            r.get(0)
        })?;
        let i: ApplyIntent = decode(&text)?;
        if i.id != id {
            return Err(Error::InvalidData);
        }
        validate_pair(&i, &r)?;
    }
    Ok(())
}
fn validate_pair(i: &ApplyIntent, r: &Rekey) -> Result<(), Error> {
    let old = i64::from_be_bytes(r.old.as_slice().try_into().map_err(|_| Error::InvalidData)?);
    let new = i64::from_be_bytes(r.new.as_slice().try_into().map_err(|_| Error::InvalidData)?);
    if r.format != 1
        || r.kind as u8 > 6
        || new <= old
        || i.binding != r.binding
        || i.inbox_ids != [r.inbox]
        || i.media.is_some()
        || i.originating_open.is_none()
        || r.local_rev == 0
        || (r.kind == entity_codec::Kind::Notetype) != r.struct_gen.is_some()
        || r.struct_gen == Some(0)
        || i.keys.iter().any(|k| k.role != ApplyKeyRole::SideEffect)
        || !i.keys.iter().any(|k| {
            k.name == r.old_name
                && k.kind == r.kind
                && k.key == r.old
                && k.before_digest.is_some()
                && k.after_digest.is_none()
        })
        || !i.keys.iter().any(|k| {
            k.name == r.new_name
                && k.kind == r.kind
                && k.key == r.new
                && k.before_digest.is_none()
                && k.after_digest.is_some()
        })
    {
        return Err(Error::InvalidData);
    }
    let created: std::collections::BTreeSet<_> = r.created.iter().map(|(name, _)| name).collect();
    let expected: std::collections::BTreeSet<_> = i
        .keys
        .iter()
        .filter(|k| k.name != r.new_name && k.before_digest.is_none() && k.after_digest.is_some())
        .map(|k| &k.name)
        .collect();
    if created.len() != r.created.len() || created != expected {
        return Err(Error::InvalidData);
    }
    Ok(())
}
fn header(
    c: &Connection,
    b: BindingId,
    kind: entity_codec::Kind,
    key: &[u8],
) -> Result<Option<super::tracking_payload::Header>, Error> {
    let text: Option<String> = if kind == entity_codec::Kind::Note {
        let id = i64::from_be_bytes(key.try_into().map_err(|_| Error::InvalidData)?);
        c.query_row(
            "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![b.0.as_slice(), id],
            |r| r.get(0),
        )
        .optional()?
    } else {
        c.query_row(
            "SELECT content FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3",
            params![b.0.as_slice(), kind as i64, key],
            |r| r.get(0),
        )
        .optional()?
    };
    text.map(|text| super::tracking_payload::header(kind, &text))
        .transpose()
}
impl TrackingStore {
    pub(super) fn has_rekey_intents(&self, b: BindingId) -> Result<bool, Error> {
        Ok(self.connection.query_row("SELECT EXISTS(SELECT 1 FROM cloud_rekey_intents r JOIN cloud_apply_intents i ON i.id=r.id WHERE i.binding=?1)", [b.0.as_slice()], |r| r.get(0))?)
    }
    /// Indexed integer/key lookup only. Reservations include clean entities,
    /// tombstones and retained flights, without loading a single image body.
    pub(super) fn rekey_holds(&self, b: BindingId, kind: entity_codec::Kind, id: i64) -> Result<bool, Error> {
        Ok(if kind == entity_codec::Kind::Note {
            self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM cloud_runtime_notes WHERE binding=?1 AND id=?2)",
                params![b.0.as_slice(), id],
                |r| r.get(0),
            )?
        } else {
            self.connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM cloud_runtime_entities WHERE binding=?1 AND kind=?2 AND key=?3)",
                params![b.0.as_slice(), kind as i64, entity_codec::id_key(id).as_slice()],
                |r| r.get(0),
            )?
        })
    }
    pub(super) fn persist_rekey(&mut self, i: &ApplyIntent, r: &Rekey) -> Result<i64, Error> {
        validate_pair(i, r)?;
        let tx = self.connection.transaction()?;
        let text: String = tx.query_row(
            "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), r.inbox],
            |r| r.get(0),
        )?;
        let entry: InboxEntry = decode(&text)?;
        let InboxValue::Record { envelope, .. } = &entry.value else {
            return Err(Error::InvalidData);
        };
        if entry.record_name != r.old_name || entity_codec::sha256(&envelope.0) != r.server_digest {
            return Err(Error::InvalidData);
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
        tx.execute(
            "INSERT INTO cloud_rekey_intents(id,content) VALUES(?1,?2)",
            params![id, encode(r)?],
        )?;
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.tracking.commit.intent")?;
        meguri_model::faultpoint!("cloud.tracking.commit.intent")?;
        tx.commit()?;
        Ok(id)
    }
    /// Move tracking after WAL coverage; leave the incoming record ready.
    pub(super) fn settle_rekey(
        &mut self,
        i: &ApplyIntent,
        r: &Rekey,
        observed: (u64, u64, u64),
        mut read: impl FnMut(&ApplyKey) -> Result<Option<entity_codec::Payload>, Error>,
        commit_guard: impl FnOnce() -> Result<(), Error>,
    ) -> Result<(), Error> {
        validate_pair(i, r)?;
        let binding = self.binding(i.binding)?.ok_or(Error::BindingNotFound)?;
        if entity_codec::entity_name(&binding.facts.record_key.0, r.kind, &r.old).map_err(|_| Error::InvalidData)?
            != r.old_name
            || entity_codec::entity_name(&binding.facts.record_key.0, r.kind, &r.new).map_err(|_| Error::InvalidData)?
                != r.new_name
        {
            return Err(Error::InvalidData);
        }
        let mut state = self.runtime_binding(i.binding)?;
        let tx = self.connection.transaction()?;
        if get(&tx, i.id)?.as_ref() != Some(r) {
            return Err(Error::InvalidData);
        }
        let mut old_header = header(&tx, i.binding, r.kind, &r.old)?.ok_or(Error::InvalidData)?;
        let old = &mut old_header.facts;
        if old.origin != r.origin || old.local_rev != r.local_rev || header(&tx, i.binding, r.kind, &r.new)?.is_some() {
            return Err(Error::RecoveryRefused);
        }
        {
            let moved_key = i.keys.iter().find(|k| k.name == r.new_name).ok_or(Error::InvalidData)?;
            let moved = read(moved_key)?.ok_or(Error::InvalidData)?;
            let fresh = super::workflow_store::new_pending_entity(
                &binding.facts.record_key,
                moved,
                r.origin,
                runtime::DirtySource::Observer,
                &mut state.change_counter,
                observed,
            )?;
            super::workflow_store::put(&tx, i.binding, &fresh)?;
        }
        if r.preserve_old {
            // This is a real landed tombstone, whose local restoration just
            // moved. Keep its exact flight/ACK until ordinary apply settles it.
            if old.acked_tombstone() {
                let entity_codec::DecodeOutcome::Supported(server) =
                    entity_codec::Entity::decode(&old.acked_envelope.0).map_err(|_| Error::InvalidData)?
                else {
                    return Err(Error::InvalidData);
                };
                old.tombstone_at = Some(server.fields.deleted_at_ms);
            } else {
                let text: String = tx.query_row(
                    "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                    params![i.binding.0.as_slice(), r.inbox],
                    |r| r.get(0),
                )?;
                let entry: InboxEntry = decode(&text)?;
                let InboxValue::Record { envelope, .. } = entry.value else {
                    return Err(Error::InvalidData);
                };
                let entity_codec::DecodeOutcome::Supported(server) =
                    entity_codec::Entity::decode(&envelope.0).map_err(|_| Error::InvalidData)?
                else {
                    return Err(Error::InvalidData);
                };
                if old
                    .own_row_tombstone_flight(i.binding.0, &server, &envelope.0)
                    .is_none()
                {
                    return Err(Error::InvalidData);
                }
                old.tombstone_at = Some(server.fields.deleted_at_ms);
            }
            old_header.clear_rekey_base();
            super::tracking_payload::put_header(&tx, i.binding, &old_header)?;
        } else {
            super::workflow_store::remove_entity(&tx, i.binding, old)?;
        }
        for k in i.keys.iter().filter(|k| k.name != r.old_name && k.name != r.new_name) {
            let local = read(k)?;
            let prior = header(&tx, i.binding, k.kind, &k.key)?;
            match (prior, local) {
                (Some(mut h), Some(p)) => {
                    h.rekey_reference(&tx, i.binding, p, &mut state.change_counter, observed)?;
                    super::tracking_payload::put_header(&tx, i.binding, &h)?;
                }
                (Some(mut h), None) if k.kind == entity_codec::Kind::Config => {
                    if !h.facts.sent && h.facts.server_erev == 0 && h.facts.in_flight.is_empty() {
                        super::incomplete_store::dropped(&tx, i.binding, &h.facts.name, h.facts.local_rev)?;
                        super::workflow_store::remove_entity(&tx, i.binding, &h.facts)?;
                    } else {
                        h.rekey_config_removed(i.planned_at_ms, &mut state.change_counter, observed)?;
                        super::tracking_payload::put_header(&tx, i.binding, &h)?;
                    }
                }
                (None, Some(p)) if k.kind == entity_codec::Kind::Config => {
                    let origin = r
                        .created
                        .iter()
                        .find(|(name, _)| name == &k.name)
                        .ok_or(Error::RecoveryRefused)?
                        .1;
                    let e = super::workflow_store::new_pending_entity(
                        &binding.facts.record_key,
                        p,
                        origin,
                        runtime::DirtySource::Observer,
                        &mut state.change_counter,
                        observed,
                    )?;
                    super::workflow_store::put(&tx, i.binding, &e)?;
                }
                _ => return Err(Error::RecoveryRefused),
            }
        }
        super::runtime_store::put_binding(&tx, i.binding, &state)?;
        tx.execute("DELETE FROM cloud_rekey_intents WHERE id=?1", [i.id])?;
        tx.execute(
            "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), i.id],
        )?;
        super::tracking_payload::collect_retired(&tx, i.binding)?;
        meguri_model::faultpoint!("cloud.tracking.commit.ack")?;
        commit_guard()?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn clear_rolled_back_rekey(&mut self, i: &ApplyIntent) -> Result<(), Error> {
        let tx = self.connection.transaction()?;
        tx.execute("DELETE FROM cloud_rekey_intents WHERE id=?1", [i.id])?;
        tx.execute(
            "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
            params![i.binding.0.as_slice(), i.id],
        )?;
        super::tracking_payload::collect_retired(&tx, i.binding)?;
        tx.commit()?;
        Ok(())
    }
}
