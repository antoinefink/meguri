pub(crate) use super::cleanup::update_blob_reference_age;
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
pub(crate) fn put_ledger(tx: &rusqlite::Transaction<'_>, b: BindingId, ledger: &Ledger) -> Result<bool, Error> {
    let content: Option<String> = tx
        .query_row(
            "SELECT content FROM cloud_media_state WHERE binding=?1",
            [b.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    let current: Ledger = content.map(|v| decode(&v)).transpose()?.unwrap_or_default();
    if current.revision != ledger.revision {
        #[cfg(feature = "test-hooks")]
        eprintln!("cloudMedia.ledgerRefused check=revisionMismatch");
        return Err(Error::RecoveryRequired);
    }
    if matches!((current.feed, ledger.feed), (Some((old_id, old)), Some((id, next))) if old_id == id && next < old) {
        #[cfg(feature = "test-hooks")]
        eprintln!("cloudMedia.ledgerRefused check=feedRegression");
        return Err(Error::RecoveryRequired);
    }
    let mut next = ledger.clone();
    next.zone_missing |= current.zone_missing;
    next.revision = runtime_store::next(current.revision)?;
    tx.execute(
        "INSERT INTO cloud_media_state VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content",
        params![b.0.as_slice(), encode(&next)?],
    )?;
    Ok(next.zone_missing)
}
// Called in the transaction that records an actual fetched collection drain,
// before that drain can be projected to policy as FetchDrained.
pub(crate) fn recheck_after_drain(tx: &rusqlite::Transaction<'_>, b: BindingId, now_ms: i64) -> Result<(), Error> {
    let mut ledger = super::cleanup::ledger(tx, b)?;
    let mut changed = false;
    for state in ledger.blobs.values_mut() {
        let Some(acked_at) = state.recheck_at else { continue };
        let final_recheck = now_ms.checked_sub(acked_at).is_some_and(|age| age >= 86_400_000);
        if !state.rechecked || final_recheck {
            if state.server == ServerState::Present {
                state.server = ServerState::Unknown;
            }
            state.rechecked = !final_recheck;
            if final_recheck {
                state.recheck_at = None;
            }
            changed = true;
        }
    }
    for deferred in ledger.deferred_rechecks.values_mut() {
        let final_recheck = now_ms
            .checked_sub(deferred.acked_at_ms)
            .is_some_and(|age| age >= 86_400_000);
        if !deferred.rechecked || final_recheck && !deferred.final_rechecked {
            deferred.rechecked = true;
            deferred.final_rechecked |= final_recheck;
            changed = true;
        }
    }
    if changed {
        put_ledger(tx, b, &ledger)?;
    }
    Ok(())
}

// The caller persists both the shared root authority and the resulting ledger
// in one transaction, or invokes this before making any blob decision.
pub(crate) fn materialize_rechecks(ledger: &mut Ledger, key: &[u8; 32]) -> bool {
    if ledger.deferred_rechecks.is_empty() {
        return false;
    }
    for (_, deferred) in std::mem::take(&mut ledger.deferred_rechecks) {
        let state = ledger
            .blobs
            .entry(deferred.metadata.name(key))
            .or_insert_with(|| BlobState {
                metadata: deferred.metadata,
                ..Default::default()
            });
        state.server = ServerState::Unknown;
        if !deferred.final_rechecked {
            // A later original ACK keeps the full 24-hour obligation for all
            // names of this content; either outstanding first drain still runs.
            state.rechecked = if state.recheck_at.is_some() {
                state.rechecked && deferred.rechecked
            } else {
                deferred.rechecked
            };
            state.recheck_at = Some(
                state
                    .recheck_at
                    .map_or(deferred.acked_at_ms, |at| at.max(deferred.acked_at_ms)),
            );
        }
    }
    true
}
impl TrackingStore {
    // Only the validated local-flight ACK path calls this, before replacing its
    // ACK image. Its caller persists this ledger in the entity ACK transaction.
    pub(crate) fn media_ack_ledger(
        &self,
        b: BindingId,
        prior: &workflow::EntityFacts,
        payload: &c::Payload,
        now_ms: i64,
    ) -> Result<Ledger, Error> {
        let mut ledger = self.media_ledger(b)?;
        let c::Payload::MediaName(name) = payload else {
            return Err(Error::InvalidData);
        };
        let ack_named_content = prior.server_erev > 0
            && !prior.acked_media_tombstone()
            && matches!(&prior.acked_payload, Some(c::Payload::MediaName(acked)) if acked.sha256 == name.sha256);
        if !ack_named_content {
            let metadata = blob::Blob::from(name);
            if let Some((key, true, _)) = self.publication_media(b)? {
                materialize_rechecks(&mut ledger, &key.0);
                let state = ledger.blobs.entry(metadata.name(&key.0)).or_insert_with(|| BlobState {
                    metadata,
                    ..Default::default()
                });
                state.recheck_at = Some(state.recheck_at.map_or(now_ms, |at| at.max(now_ms)));
                state.rechecked = false;
            } else {
                let content = data_encoding::BASE32_NOPAD.encode(&metadata.sha256);
                ledger
                    .deferred_rechecks
                    .entry(content)
                    .and_modify(|mark| {
                        mark.acked_at_ms = mark.acked_at_ms.max(now_ms);
                        mark.rechecked = false;
                        mark.final_rechecked = false;
                    })
                    .or_insert(DeferredRecheck {
                        metadata,
                        acked_at_ms: now_ms,
                        rechecked: false,
                        final_rechecked: false,
                    });
            }
        }
        Ok(ledger)
    }
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
    pub(crate) fn materialize_media_rechecks(&mut self, b: BindingId) -> Result<(), Error> {
        let Some((key, true, _)) = self.publication_media(b)? else {
            return Ok(());
        };
        let mut ledger = self.media_ledger(b)?;
        if materialize_rechecks(&mut ledger, &key.0) {
            self.put_media(b, &ledger)?;
        }
        Ok(())
    }
    pub(crate) fn put_media(&mut self, b: BindingId, ledger: &Ledger) -> Result<bool, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let pending = self.dirty_names(b)?;
        self.commit_media_ledger(b, ledger)?;
        Ok(self.dirty_names(b)? != pending)
    }
    // Return the sticky value from the transaction, with no post-commit I/O.
    // Runtime publishes it before reading pending names or notifying observers.
    pub(crate) fn commit_media_ledger(&mut self, b: BindingId, ledger: &Ledger) -> Result<bool, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let zone_missing = put_ledger(&tx, b, ledger)?;
        #[cfg(feature = "test-hooks")]
        crate::cloud::testing::tracking_commit(&tx, "cloud.media.commit.ledger")?;
        tx.commit()?;
        Ok(zone_missing)
    }
    pub(crate) fn commit_media_feed(
        &mut self,
        binding_id: BindingId,
        next_ledger: &Ledger,
        file_changes: &[(c::Payload, [u8; 16])],
        observation: (u64, u64, u64),
        now_ms: i64,
        apply_commit: Option<MediaApplyCommit<'_>>,
    ) -> Result<bool, Error> {
        let binding = self.binding(binding_id)?.ok_or(Error::BindingNotFound)?;
        let mut runtime = self.runtime_binding(binding_id)?;
        let pending = self.dirty_names(binding_id)?;
        let mut changed_work = apply_commit.is_some();
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let mut next_ledger = next_ledger.clone();
        update_blob_reference_age(&tx, binding_id, &mut next_ledger, now_ms)?;
        if file_changes.is_empty() && apply_commit.is_none() {
            let mut current = super::cleanup::ledger(&tx, binding_id)?;
            // An unchanged feed has no effect. In particular, a check of the
            // current cursor must not invalidate another worker's ledger read.
            if current.revision >= next_ledger.revision
                && runtime_store::checked(current.revision).is_ok()
                && matches!((current.feed, next_ledger.feed), (Some(current), Some(next)) if current == next)
            {
                current.revision = next_ledger.revision;
                if current == next_ledger {
                    return Ok(false);
                }
            }
        }
        if let Some((_, proposals, _)) = apply_commit {
            for (prior, candidate) in proposals {
                if workflow_store::entity(&tx, binding_id, candidate.kind, &candidate.key)? != *prior {
                    #[cfg(feature = "test-hooks")]
                    eprintln!("cloudMedia.ledgerRefused check=applyPriorEntity");
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
            let local_tombstone = next_ledger.names.get(name).is_some_and(|n| n.tombstone_at.is_some());
            if local_tombstone != entity.acked_media_tombstone() {
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
                if entity.server_erev > 0 {
                    for id in &intent.inbox_ids {
                        let text: Option<String> = tx
                            .query_row(
                                "SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                                params![binding_id.0.as_slice(), id],
                                |r| r.get(0),
                            )
                            .optional()?;
                        if let Some(text) = text {
                            let entry: crate::cloud::InboxEntry = decode(&text)?;
                            if entry.record_name == entity.name {
                                crate::cloud::adapter_store::transfer_entry(
                                    &tx,
                                    binding_id,
                                    &entity.name,
                                    entry.absence_pass,
                                )?;
                            }
                        }
                    }
                }
            }
            if remove_inbox {
                for id in &intent.inbox_ids {
                    let text: String = tx.query_row("SELECT entry FROM inbox WHERE binding=?1 AND id=?2",
                        params![binding_id.0.as_slice(), id], |r| r.get(0))?;
                    crate::cloud::incomplete_store::incoming_done(&tx, binding_id, *id, &decode(&text)?)?;
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
        update_blob_reference_age(&tx, binding_id, &mut next_ledger, now_ms)?;
        put_ledger(&tx, binding_id, &next_ledger)?;
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
        now_ms: i64,
        mut references: Option<&mut super::cleanup::SavedReferences>,
        accepted_image: Option<(u64, u64)>,
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
            .find(|f| f.erev == r.entity.fields.erev && f.payload_digest == r.entity.payload_sha256)
            .cloned()
            .ok_or(Error::InvalidData)?;
        let image = crate::cloud::tracking_payload::flight_payload(&self.connection, b, &flight)?;
        if Some(&image) != r.payload.as_ref() {
            return Err(Error::InvalidData);
        }
        let mut ledger = self.media_ledger(b)?;
        let before = references
            .as_ref()
            .map(|_| super::cleanup::row_references(&e, &ledger))
            .transpose()?;
        e.acked_rev = flight.local_rev;
        e.server_erev = flight.erev;
        e.acked_payload = Some(image.clone());
        e.acked_row_digest = flight.row_digest;
        e.acked_payload_digest = flight.payload_digest;
        e.acked_envelope = OpaqueBytes(saved.envelope.clone());
        e.acked_system_fields = saved.system_fields.clone();
        e.in_flight.retain(|f| f.local_rev > flight.local_rev);
        if ledger
            .names
            .get(std::str::from_utf8(&e.key).map_err(|_| Error::InvalidData)?)
            .is_some_and(|n| n.tombstone_at.is_some())
        {
            e.payload = image;
            e.mark = None;
            e.base_payload = None;
            e.acked_rev = e.local_rev;
        }
        let pending = e.mark.is_some();
        let transition = before
            .map(|before| super::cleanup::row_references(&e, &ledger).map(|after| (before, after)))
            .transpose()?;
        let tx = self.connection.transaction()?;
        let delta = if let (Some(refs), Some(transition)) = (references.as_deref_mut(), transition) {
            Some(refs.before(&tx, b, &mut ledger, transition, now_ms)?)
        } else {
            update_blob_reference_age(&tx, b, &mut ledger, now_ms)?;
            None
        };
        if let Some((opened, seq)) = accepted_image {
            crate::cloud::incomplete_store::ack_image(&tx, b, &saved.name, opened, seq)?;
        } else {
            crate::cloud::incomplete_store::ack(&tx, b, &saved.name, &flight)?;
        }
        workflow_store::put(&tx, b, &e)?;
        if let (Some(refs), Some(delta)) = (references.as_deref(), &delta) {
            refs.after(&mut ledger, now_ms, delta);
        } else {
            update_blob_reference_age(&tx, b, &mut ledger, now_ms)?;
        }
        put_ledger(&tx, b, &ledger)?;
        crate::cloud::adapter_store::mark_ack(&tx, b, &e.name)?;
        tx.commit()?;
        if let (Some(refs), Some(delta)) = (references, delta) {
            refs.committed(delta);
        }
        Ok(pending)
    }
}
