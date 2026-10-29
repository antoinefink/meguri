use super::*;
use crate::cloud::{
    runtime::DirtySource,
    runtime_store,
    store::{decode, encode},
    workflow_store,
};
use rusqlite::{Connection, OptionalExtension, params};

type MediaApplyCommit<'a> = (
    &'a incoming::ApplyIntent,
    &'a [(Option<workflow::EntityFacts>, workflow::EntityFacts)],
    bool,
);

pub(crate) fn install(c: &Connection) -> Result<(), Error> {
    c.execute_batch("CREATE TABLE IF NOT EXISTS cloud_media_state(binding BLOB PRIMARY KEY REFERENCES bindings(id), content TEXT NOT NULL CHECK(json_valid(content)));" )?;
    Ok(())
}
fn put_ledger(tx: &rusqlite::Transaction<'_>, b: BindingId, ledger: &Ledger) -> Result<(), Error> {
    let content: Option<String> = tx
        .query_row(
            "SELECT content FROM cloud_media_state WHERE binding=?1",
            [b.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    let current: Ledger = content.map(|v| decode(&v)).transpose()?.unwrap_or_default();
    if current.revision != ledger.revision
        || matches!((current.feed, ledger.feed), (Some((old_id, old)), Some((id, next))) if old_id == id && next < old)
    {
        return Err(Error::RecoveryRequired);
    }
    let mut next = ledger.clone();
    next.revision = runtime_store::next(current.revision)?;
    tx.execute(
        "INSERT INTO cloud_media_state VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content",
        params![b.0.as_slice(), encode(&next)?],
    )?;
    Ok(())
}
impl TrackingStore {
    pub(crate) fn media_ledger(&self, b: BindingId) -> Result<Ledger, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let value: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_media_state WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        value.map(|v| decode(&v)).transpose().map(Option::unwrap_or_default)
    }
    pub(crate) fn put_media(&mut self, b: BindingId, ledger: &Ledger) -> Result<bool, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let pending = self.dirty_names(b)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        put_ledger(&tx, b, ledger)?;
        tx.commit()?;
        Ok(self.dirty_names(b)? != pending)
    }
    pub(crate) fn commit_media_feed(
        &mut self,
        binding_id: BindingId,
        next_ledger: &Ledger,
        file_changes: &[(c::Payload, [u8; 16])],
        observation: (u64, u64, u64),
        apply_commit: Option<MediaApplyCommit<'_>>,
    ) -> Result<bool, Error> {
        let binding = self.binding(binding_id)?.ok_or(Error::BindingNotFound)?;
        let mut runtime = self.runtime_binding(binding_id)?;
        let pending = self.dirty_names(binding_id)?;
        let mut changed_work = apply_commit.is_some();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        put_ledger(&tx, binding_id, next_ledger)?;
        if let Some((_, proposals, _)) = apply_commit {
            for (prior, candidate) in proposals {
                if workflow_store::entity(&tx, binding_id, candidate.kind, &candidate.key)? != *prior {
                    return Err(Error::RecoveryRequired);
                }
            }
        }
        for (payload, origin) in file_changes {
            let key = row_payload::payload_key(payload)?;
            let prior = workflow_store::entity(&tx, binding_id, c::Kind::MediaName, &key)?;
            let mut entity = if let Some(mut entity) = prior.clone() {
                entity.observe(payload.clone(), DirtySource::Observer, &mut runtime.change_counter)?;
                entity
            } else {
                workflow_store::new_pending_entity(
                    &binding.facts.record_key,
                    payload.clone(),
                    *origin,
                    DirtySource::Observer,
                    &mut runtime.change_counter,
                    observation,
                )?
            };
            let name = std::str::from_utf8(&key).map_err(|_| Error::InvalidData)?;
            if next_ledger.names.get(name).is_some_and(|n| n.tombstone_at.is_some()) && !entity.acked_media_tombstone()
            {
                runtime.change_counter = runtime_store::next(runtime.change_counter)?;
                entity.local_rev = runtime_store::next(entity.local_rev)?;
                entity.mark = Some(runtime::DirtyMark {
                    source: DirtySource::Observer,
                    change_seq: runtime.change_counter,
                    payload_digest: entity.payload.payload_digest().map_err(|_| Error::InvalidData)?,
                });
                if entity.base_payload.is_none() {
                    entity.base_payload = entity.acked_payload.clone();
                }
            }
            if next_ledger.names.get(name).is_some_and(|n| !n.sendable()) {
                entity.mark = None;
                entity.base_payload = None;
            }
            changed_work |= prior.as_ref() != Some(&entity);
            entity.observer_open = observation.0;
            entity.observed_seq = observation.1;
            entity.observed_generation = observation.2;
            workflow_store::put(&tx, binding_id, &entity)?;
        }
        if let Some((intent, proposals, remove_inbox)) = apply_commit {
            for (_, entity) in proposals {
                let mut entity = entity.clone();
                if !entity.acked_media_tombstone()
                    && (entity.server_erev == 0
                        || entity.payload.row_digest().map_err(|_| Error::InvalidData)? != entity.acked_row_digest
                        || entity.kind == c::Kind::MediaName
                            && next_ledger
                                .names
                                .get(std::str::from_utf8(&entity.key).map_err(|_| Error::InvalidData)?)
                                .is_some_and(|n| n.tombstone_at.is_some()))
                {
                    runtime.change_counter = runtime_store::next(runtime.change_counter)?;
                    entity.mark = Some(runtime::DirtyMark {
                        source: DirtySource::Observer,
                        change_seq: runtime.change_counter,
                        payload_digest: entity.payload.payload_digest().map_err(|_| Error::InvalidData)?,
                    });
                    if entity.base_payload.is_none() {
                        entity.base_payload = entity.acked_payload.clone();
                    }
                }
                workflow_store::put(&tx, binding_id, &entity)?;
            }
            if remove_inbox {
                for id in &intent.inbox_ids {
                    tx.execute(
                        "DELETE FROM inbox WHERE binding=?1 AND id=?2",
                        params![binding_id.0.as_slice(), id],
                    )?;
                }
            }
            tx.execute(
                "DELETE FROM cloud_apply_intents WHERE binding=?1 AND id=?2",
                params![binding_id.0.as_slice(), intent.id],
            )?;
            meguri_model::faultpoint!("cloud.tracking.commit.ack")?;
        }
        runtime_store::put_binding(&tx, binding_id, &runtime)?;
        tx.commit()?;
        Ok(changed_work || self.dirty_names(binding_id)? != pending)
    }
    pub(crate) fn media_floor(&self, identity: [u8; 16]) -> Result<Option<u64>, Error> {
        let mut q = self.connection.prepare("SELECT json_extract(m.content,'$.feed') FROM cloud_media_state m JOIN bindings b ON b.id=m.binding WHERE b.active=1 AND json_extract(m.content,'$.feed') IS NOT NULL")?;
        let rows = q.query_map([], |r| r.get::<_, String>(0))?;
        let mut floor = None;
        for row in rows {
            let (id, n): ([u8; 16], u64) = decode(&row?)?;
            if id == identity {
                floor = Some(floor.map_or(n, |f: u64| f.min(n)));
            }
        }
        Ok(floor)
    }
    pub(crate) fn update_media_intent(&mut self, intent: &incoming::ApplyIntent) -> Result<(), Error> {
        self.connection.execute(
            "UPDATE cloud_apply_intents SET content=?3 WHERE binding=?1 AND id=?2",
            params![intent.binding.0.as_slice(), intent.id, encode(intent)?],
        )?;
        Ok(())
    }
}
impl TrackingStore {
    pub(crate) fn acknowledge_media_tombstone(
        &mut self,
        b: BindingId,
        saved: &workflow::Saved,
        r: c::Received,
    ) -> Result<bool, Error> {
        let mut e = self.entity_facts(b, &saved.name)?.ok_or(Error::InvalidData)?;
        if e.origin != r.entity.fields.origin
            || r.entity.fields.writer != b.0
            || !r.entity.fields.tombstone
            || r.entity.fields.kind != c::Kind::MediaName
            || r.entity.fields.key != e.key
            || r.entity.fields.erev <= e.server_erev
            || r.payload.as_ref().is_none_or(|p| p.kind() != c::Kind::MediaName)
        {
            return Err(Error::InvalidData);
        }
        let flight = e
            .in_flight
            .iter()
            .find(|f| {
                f.erev == r.entity.fields.erev
                    && f.payload_digest == r.entity.payload_sha256
                    && Some(&f.payload) == r.payload.as_ref()
            })
            .cloned()
            .ok_or(Error::InvalidData)?;
        e.acked_rev = flight.local_rev;
        e.server_erev = flight.erev;
        e.acked_payload = Some(flight.payload.clone());
        e.acked_row_digest = flight.row_digest;
        e.acked_payload_digest = flight.payload_digest;
        e.acked_envelope = OpaqueBytes(saved.envelope.clone());
        e.acked_system_fields = saved.system_fields.clone();
        e.in_flight.retain(|f| f.local_rev > flight.local_rev);
        let ledger = self.media_ledger(b)?;
        if ledger
            .names
            .get(std::str::from_utf8(&e.key).map_err(|_| Error::InvalidData)?)
            .is_some_and(|n| n.tombstone_at.is_some())
        {
            e.payload = flight.payload;
            e.mark = None;
            e.base_payload = None;
            e.acked_rev = e.local_rev;
        }
        let pending = e.mark.is_some();
        let tx = self.connection.transaction()?;
        workflow_store::put(&tx, b, &e)?;
        tx.commit()?;
        Ok(pending)
    }
}
