//! Adapter state shares the tracking connection, transaction and access gate.
use super::{
    adapter::Durable,
    store::{decode, encode},
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use std::collections::BTreeSet;
pub(super) struct AdapterCommitOptions {
    pub deleted_collection_zones: Vec<String>,
    pub at_ms: i64,
    pub fail: bool,
    pub start_absence: bool,
    pub resolved_epoch: Option<super::adapter::EpochVerification>,
}

impl TrackingStore {
    pub(super) fn adapter_install(&mut self) -> Result<(), Error> {
        install(&self.connection)?;
        Ok(())
    }
    pub(super) fn adapter_durable(&self, binding: BindingId) -> Result<Durable, Error> {
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_adapter_state WHERE binding=?1",
                [binding.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|s| decode(&s)).transpose().map(|v| v.unwrap_or_default())
    }
    pub(super) fn adapter_commit(
        &mut self,
        binding: BindingId,
        entries: &[InboxEntry],
        refused: &[super::incoming::RefusedEntry],
        state: Option<&[u8]>,
        durable: &Durable,
        options: AdapterCommitOptions,
    ) -> Result<(), Error> {
        self.adapter_commit_loss(binding, entries, refused, state, durable, options, None)
    }
    #[allow(clippy::too_many_arguments)] // Extends the existing callback transaction; no stored shape.
    pub(super) fn adapter_commit_loss(
        &mut self,
        binding: BindingId,
        entries: &[InboxEntry],
        refused: &[super::incoming::RefusedEntry],
        state: Option<&[u8]>,
        durable: &Durable,
        options: AdapterCommitOptions,
        loss: Option<(Option<IsolationReason>, bool, bool)>,
    ) -> Result<(), Error> {
        let AdapterCommitOptions {
            deleted_collection_zones,
            at_ms,
            fail,
            start_absence,
            resolved_epoch,
        } = options;
        // All-or-nothing even for callers below Swift's classification boundary.
        let owner = self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let bound = super::adapter::collection_zone(owner.facts.epoch);
        if deleted_collection_zones
            .iter()
            .any(|name| name == &bound || !super::publication_store::valid_collection_zone(name))
        {
            return Err(Error::InvalidData);
        }
        let mut durable = durable.clone();
        let prior = self.adapter_durable(binding)?;
        // Ordinary callbacks cannot forget accepted loss, including callbacks
        // whose in-memory metadata predates its transaction.
        durable.collection_loss = prior.collection_loss.or(durable.collection_loss);
        durable.epoch_verification = match (prior.epoch_verification, resolved_epoch) {
            (Some(pending), Some(resolved)) if pending == resolved => None,
            (_, Some(_)) => return Err(Error::InvalidData),
            (pending, None) => pending.or(durable.epoch_verification),
        };
        // A state-deletion/isolation trigger may have ended the pass since the
        // Runtime last observed it. An ordinary callback cannot resurrect it.
        if !start_absence {
            durable.absence = prior.absence;
        }
        if durable.active_deleted {
            durable.absence = None;
        }
        if self.binding(binding)?.ok_or(Error::BindingNotFound)?.facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        let mut facts = self.binding(binding)?.ok_or(Error::BindingNotFound)?.facts;
        let mut runtime = self.stored_runtime_state(binding)?;
        if let Some(reason) = loss.and_then(|v| v.0) {
            facts.state = BindingState::Isolated(reason);
        }
        if durable.active_deleted {
            facts.state = BindingState::Isolated(IsolationReason::ActiveMissing);
            if let Some(runtime) = runtime.as_mut() {
                runtime.root_ack = None;
            }
        }
        // Drains are appended only by the qualified adapter did-fetch path.
        // Ignore remembered drains; both fetched flags count for media rechecks.
        let recheck_media = facts.state == BindingState::Active
            && durable.drains.get(prior.drains.len()..).is_some_and(|drains| {
                let zone = super::adapter::collection_zone(facts.epoch);
                drains.iter().any(|drain| drain.zone == zone)
            });
        let publisher_root = if loss.is_some_and(|v| v.2) {
            let mut op = self.publication_op()?.ok_or(Error::InvalidData)?;
            if op.binding_after != Some(binding) || op.step == super::publication::PublishStep::Cancelling {
                return Err(Error::InvalidData);
            }
            op.active = None;
            op.active_attempt = None;
            Some(op)
        } else {
            None
        };
        let tx = self.connection.transaction()?;
        if let Some(op) = publisher_root {
            super::publication_store::put_op(&tx, &self.scope, &op)?;
        }
        if durable.active_deleted || loss.and_then(|v| v.0).is_some() {
            tx.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![binding.0.as_slice(), encode(&facts)?],
            )?;
            if let Some(runtime) = runtime {
                tx.execute(
                    "UPDATE cloud_runtime_bindings SET content=?2 WHERE binding=?1",
                    params![binding.0.as_slice(), encode(&runtime)?],
                )?;
            }
        }
        if start_absence && let Some(pass) = durable.absence.as_ref().filter(|p| !p.completed) {
            tx.execute(
                "DELETE FROM cloud_absence_seen WHERE binding=?1 AND pass!=?2",
                params![binding.0.as_slice(), pass.pass.as_slice()],
            )?;
        }
        super::incoming_store::receive(&tx, binding, entries, refused)?;
        for zone in &deleted_collection_zones {
            tx.execute(
                "DELETE FROM cloud_publication_zones WHERE binding=?1 AND zone=?2",
                params![binding.0.as_slice(), zone],
            )?;
        }
        if let Some((reason, media_missing, _)) = loss {
            if media_missing {
                let mut ledger = super::media::cleanup::ledger(&tx, binding)?;
                ledger.zone_missing = true;
                super::media::store::put_ledger(&tx, binding, &ledger)?;
            }
            if matches!(
                reason,
                Some(IsolationReason::ZoneDeletedByUser | IsolationReason::EncryptedDataReset)
            ) {
                super::incoming_store::remove_server_cache(&tx, binding)?;
            }
        }
        if !entries.is_empty() || !refused.is_empty() || !deleted_collection_zones.is_empty() {
            #[cfg(feature = "test-hooks")]
            super::testing::tracking_commit(&tx, "cloud.tracking.commit.inbox")?;
            meguri_model::faultpoint!("cloud.tracking.commit.inbox")?;
        }
        if let Some(state) = state {
            tx.execute("INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES (?1,?2,?3) ON CONFLICT(binding) DO UPDATE SET serialization=excluded.serialization,saved_at_ms=excluded.saved_at_ms",params![binding.0.as_slice(),state,at_ms])?;
        }
        tx.execute("INSERT INTO cloud_adapter_state(binding,content) VALUES (?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(),encode(&durable)?])?;
        if facts.state != BindingState::Active {
            tx.execute(
                "UPDATE cloud_adapter_state SET content=json_set(content,'$.absence',NULL) WHERE binding=?1",
                [binding.0.as_slice()],
            )?;
        }
        if recheck_media {
            super::media::store::recheck_after_drain(&tx, binding, at_ms)?;
        }
        // Fixture faults are consumed by Runtime only after preparation and
        // fail this same transaction before commit, rolling every write back.
        if fail {
            return Err(Error::Io(std::io::ErrorKind::Other));
        }
        if state.is_some() {
            #[cfg(feature = "test-hooks")]
            super::testing::tracking_commit(&tx, "cloud.tracking.commit.engine_state")?;
            meguri_model::faultpoint!("cloud.tracking.commit.engine_state")?;
        }
        meguri_model::faultpoint!("cloud.adapter.before_commit")?;
        tx.commit()?;
        Ok(())
    }
}

impl TrackingStore {
    pub(super) fn adapter_record_exchange(&mut self, binding: BindingId, durable: &Durable) -> Result<(), Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let mut durable = durable.clone();
        let prior = self.adapter_durable(binding)?;
        durable.absence = prior.absence;
        durable.collection_loss = prior.collection_loss.or(durable.collection_loss);
        durable.epoch_verification = prior.epoch_verification.or(durable.epoch_verification);
        let tx = self.connection.transaction()?;
        tx.execute("INSERT INTO cloud_adapter_state(binding,content) VALUES (?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(),encode(&durable)?])?;
        meguri_model::faultpoint!("cloud.adapter.before_commit")?;
        tx.commit()?;
        Ok(())
    }
}

// Runtime V3 adds metadata only: ordinary entity JSON and every older inbox stay intact.
// Triggers cover all binding writers, including recovery and explicit state deletion.
pub(super) fn install(c: &Connection) -> Result<(), Error> {
    super::acknowledgement_store::install(c)?;
    c.execute_batch("CREATE TABLE IF NOT EXISTS cloud_adapter_state(binding BLOB PRIMARY KEY REFERENCES bindings(id), content TEXT NOT NULL CHECK(json_valid(content)));
        CREATE TABLE IF NOT EXISTS cloud_absence_seen(binding BLOB NOT NULL REFERENCES bindings(id), name TEXT NOT NULL, pass BLOB NOT NULL CHECK(length(pass)=16), PRIMARY KEY(binding,name));
        CREATE TABLE IF NOT EXISTS cloud_absence_result(binding BLOB PRIMARY KEY REFERENCES bindings(id), pass BLOB NOT NULL CHECK(length(pass)=16), zone TEXT NOT NULL, completed_at_ms INTEGER NOT NULL, names TEXT NOT NULL CHECK(json_valid(names)));
        CREATE TRIGGER IF NOT EXISTS cloud_absence_state_deleted AFTER DELETE ON engine_state BEGIN
          UPDATE cloud_adapter_state SET content=json_set(content,'$.absence',NULL) WHERE binding=OLD.binding;
          DELETE FROM cloud_absence_seen WHERE binding=OLD.binding;
        END;
        CREATE TRIGGER IF NOT EXISTS cloud_absence_isolated AFTER UPDATE OF facts ON bindings WHEN json_extract(NEW.facts,'$.state') != 'Active' BEGIN
          UPDATE cloud_adapter_state SET content=json_set(content,'$.absence',NULL) WHERE binding=NEW.id;
          DELETE FROM cloud_absence_seen WHERE binding=NEW.id;
        END;")?;
    Ok(())
}

pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    let present: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_runtime_schema')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(());
    }
    let version: i64 = c.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get(0))?;
    if version < 3 {
        return Ok(());
    }
    let triggers: i64 = c.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name IN ('cloud_absence_state_deleted','cloud_absence_isolated')", [], |r| r.get(0))?;
    if triggers != 2 {
        return Err(Error::InvalidData);
    }
    // Prepare every required column even when the metadata tables are empty.
    c.prepare("SELECT binding,pass,zone,completed_at_ms,names FROM cloud_absence_result LIMIT 0")?;
    c.prepare("SELECT binding,name,pass FROM cloud_absence_seen LIMIT 0")?;
    // Validate metadata and ownership without decoding any other build's payload.
    for table in ["cloud_absence_seen", "cloud_absence_result"] {
        let sql = format!(
            "SELECT EXISTS(SELECT 1 FROM {table} m LEFT JOIN bindings b ON b.id=m.binding WHERE b.id IS NULL OR typeof(m.pass)!='blob' OR length(m.pass)!=16)"
        );
        if c.query_row(&sql, [], |r| r.get::<_, bool>(0))? {
            return Err(Error::InvalidData);
        }
    }
    let mut q = c.prepare("SELECT names FROM cloud_absence_result")?;
    for names in q.query_map([], |r| r.get::<_, String>(0))? {
        let _: BTreeSet<String> = decode(&names?)?;
    }
    Ok(())
}

pub(super) fn running_pass(c: &Connection, b: BindingId) -> Result<Option<[u8; 16]>, Error> {
    let present: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_adapter_state')",
        [],
        |r| r.get(0),
    )?;
    if !present {
        return Ok(None);
    }
    let content: Option<String> = c
        .query_row(
            "SELECT content FROM cloud_adapter_state WHERE binding=?1",
            [b.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    let durable: Durable = content.map(|s| decode(&s)).transpose()?.unwrap_or_default();
    Ok(durable
        .absence
        .filter(|p| !p.completed && !p.zone.is_empty())
        .map(|p| p.pass))
}

pub(super) fn mark_seen(tx: &Transaction<'_>, b: BindingId, name: &str, pass: [u8; 16]) -> Result<(), Error> {
    tx.execute("INSERT INTO cloud_absence_seen(binding,name,pass) VALUES(?1,?2,?3) ON CONFLICT(binding,name) DO UPDATE SET pass=excluded.pass", params![b.0.as_slice(), name, pass.as_slice()])?;
    Ok(())
}

pub(super) fn transfer_entry(
    tx: &Transaction<'_>,
    b: BindingId,
    name: &str,
    entry_pass: Option<[u8; 16]>,
) -> Result<(), Error> {
    if let Some(pass) = entry_pass
        && running_pass(tx, b)? == Some(pass)
    {
        mark_seen(tx, b, name, pass)?;
    }
    Ok(())
}
pub(super) fn mark_ack(tx: &Transaction<'_>, b: BindingId, name: &str) -> Result<(), Error> {
    if let Some(pass) = running_pass(tx, b)? {
        mark_seen(tx, b, name, pass)?;
    }
    Ok(())
}

impl TrackingStore {
    pub(super) fn absence_completed(&self, b: BindingId, durable: &Durable) -> Result<bool, Error> {
        let Some(pass) = durable.absence.as_ref().filter(|p| p.completed) else {
            return Ok(false);
        };
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_absence_result WHERE binding=?1 AND pass=?2 AND zone=?3)",
            params![b.0.as_slice(), pass.pass.as_slice(), pass.zone],
            |r| r.get(0),
        )?)
    }
    /// Only the adapter's already accepted fetched:true collection drain calls this.
    pub(super) fn complete_absence(
        &mut self,
        b: BindingId,
        zone: &str,
        at_ms: i64,
    ) -> Result<Option<(Durable, workflow::TraceEvent)>, Error> {
        if self.binding(b)?.ok_or(Error::BindingNotFound)?.facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        let mut durable = self.adapter_durable(b)?;
        let Some(pass) = durable
            .absence
            .as_ref()
            .filter(|p| !p.completed && p.zone == zone)
            .cloned()
        else {
            return Ok(None);
        };
        let tx = self.connection.transaction()?;
        let mut names = BTreeSet::new();
        // Bounded pages, scoped before decoding; no seen corpus is cloned or serialized.
        for (table, notes) in [("cloud_runtime_notes", true), ("cloud_runtime_entities", false)] {
            let mut after = i64::MIN;
            loop {
                let sql = format!(
                    "SELECT e.rowid,e.content FROM {table} e WHERE e.binding=?1 AND e.rowid>?2 AND NOT EXISTS(SELECT 1 FROM cloud_absence_seen s WHERE s.binding=e.binding AND s.name=json_extract(e.content,'$.name') AND s.pass=?3) ORDER BY e.rowid LIMIT 250"
                );
                let mut q = tx.prepare(&sql)?;
                let page = q
                    .query_map(params![b.0.as_slice(), after, pass.pass.as_slice()], |r| {
                        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
                    })?
                    .collect::<Result<Vec<_>, _>>()?;
                if page.is_empty() {
                    break;
                }
                for (rowid, content) in page {
                    after = rowid;
                    let e = if notes {
                        super::workflow_store::note_facts(super::tracking_payload::load_note(&tx, b, &content)?)?
                    } else {
                        {
                            let e: workflow::EntityFacts = decode(&content)?;
                            super::tracking_payload::load_entity(&tx, b, e.kind, &content)?
                        }
                    };
                    if e.server_erev > 0 && !e.acked_system_fields.is_empty() {
                        names.insert(e.name);
                    }
                }
            }
        }
        tx.execute("INSERT INTO cloud_absence_result(binding,pass,zone,completed_at_ms,names) VALUES(?1,?2,?3,?4,?5) ON CONFLICT(binding) DO UPDATE SET pass=excluded.pass,zone=excluded.zone,completed_at_ms=excluded.completed_at_ms,names=excluded.names", params![b.0.as_slice(), pass.pass.as_slice(), zone, at_ms, encode(&names)?])?;
        durable.absence.as_mut().ok_or(Error::InvalidData)?.completed = true;
        tx.execute(
            "UPDATE cloud_adapter_state SET content=?2 WHERE binding=?1",
            params![b.0.as_slice(), encode(&durable)?],
        )?;
        tx.execute("DELETE FROM cloud_absence_seen WHERE binding=?1", [b.0.as_slice()])?;
        meguri_model::faultpoint!("cloud.adapter.before_commit")?;
        tx.commit()?;
        let event = workflow::TraceEvent::AbsencePassCompleted {
            binding: b,
            zone: zone.into(),
            pass: pass.pass,
            absent_count: names.len(),
            absent_names: names,
        };
        Ok(Some((durable, event)))
    }
}

#[cfg(feature = "test-hooks")]
impl TrackingStore {
    pub(super) fn forget_adapter_state_for_testing(&mut self, b: BindingId) -> Result<(), Error> {
        self.require_binding(b)?;
        self.connection
            .execute("DELETE FROM engine_state WHERE binding=?1", [b.0.as_slice()])?;
        Ok(())
    }
}
