//! Durable publication transitions share the existing tracking connection.
use super::{
    publication::*,
    runtime::StoredRuntimeState,
    store::{decode, encode},
    *,
};
use rusqlite::{Connection, OptionalExtension, Transaction, params};

const SCHEMA: &str = "
CREATE TABLE IF NOT EXISTS cloud_publication_schema(version INTEGER NOT NULL);
INSERT INTO cloud_publication_schema SELECT 1 WHERE NOT EXISTS(SELECT 1 FROM cloud_publication_schema);
CREATE TABLE IF NOT EXISTS cloud_publication_ops(
 container TEXT NOT NULL, environment INTEGER NOT NULL, id BLOB NOT NULL CHECK(length(id)=16),
 content TEXT NOT NULL CHECK(json_valid(content)), PRIMARY KEY(container,environment), UNIQUE(id));
CREATE TABLE IF NOT EXISTS cloud_publication_bindings(
 binding BLOB PRIMARY KEY REFERENCES bindings(id), media_key BLOB NOT NULL CHECK(length(media_key)=32),
 root_acked INTEGER NOT NULL, crt INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS cloud_publication_zones(
 binding BLOB NOT NULL REFERENCES bindings(id), zone TEXT NOT NULL,
 first_seen_ms INTEGER NOT NULL, deletion_due INTEGER NOT NULL,
 PRIMARY KEY(binding,zone));
";
pub(super) fn check_version(c: &Connection) -> Result<(), Error> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_publication_schema')",
        [],
        |r| r.get(0),
    )?;
    if exists {
        let mut s = c.prepare("SELECT version FROM cloud_publication_schema")?;
        let versions = s
            .query_map([], |r| r.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if versions != [1] && versions != [2] && versions != [3] && versions != [4] {
            return Err(Error::RuntimeSchemaUnsupported);
        }
        let ledger: Option<String> = c.query_row(
            "SELECT type FROM sqlite_master WHERE name='cloud_epoch_crt'",
            [], |r| r.get(0),
        ).optional()?;
        if ((versions == [3] || versions == [4]) && ledger.as_deref() != Some("table"))
            || (versions != [3] && versions != [4] && ledger.is_some())
        {
            return Err(Error::InvalidData);
        }
        if ledger.is_some() {
            c.prepare("SELECT binding,crt,authority FROM cloud_epoch_crt LIMIT 0")?;
        }
        let root_table: Option<String> = c.query_row(
            "SELECT type FROM sqlite_master WHERE name='cloud_root_readability'", [], |r| r.get(0),
        ).optional()?;
        if (versions == [4] && root_table.as_deref() != Some("table")) || (versions != [4] && root_table.is_some()) {
            return Err(Error::InvalidData);
        }
        if root_table.is_some() { super::root_readability_store::validate(c)?; }
    } else {
        let orphan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_publication_ops','cloud_publication_bindings','cloud_publication_zones','cloud_epoch_crt','cloud_root_readability'))", [], |r| r.get(0))?;
        if orphan {
            return Err(Error::InvalidData);
        }
    }
    Ok(())
}
pub(super) fn put_op(tx: &Transaction<'_>, scope: &BuildScope, op: &PublishOp) -> Result<(), Error> {
    op.validate()?;
    tx.execute("INSERT INTO cloud_publication_ops(container,environment,id,content) VALUES(?1,?2,?3,?4) ON CONFLICT(container,environment) DO UPDATE SET id=excluded.id,content=excluded.content",
        params![scope.container.as_str(), scope.environment.code(), op.id.as_slice(), encode(op)?])?;
    Ok(())
}
fn remove_op(tx: &Transaction<'_>, scope: &BuildScope, id: [u8; 16]) -> Result<(), Error> {
    let n = tx.execute(
        "DELETE FROM cloud_publication_ops WHERE container=?1 AND environment=?2 AND id=?3",
        params![scope.container.as_str(), scope.environment.code(), id.as_slice()],
    )?;
    if n != 1 {
        return Err(Error::InvalidData);
    }
    Ok(())
}
// Binding work owned by the current storage representations (V17.6).
// Expiry reads this same inventory; it must never call remove_work.
pub(super) const WORK_TABLES: &[&str] = &[
    "cloud_ack_entities",
    "cloud_ack_bindings",
    "cloud_refused",
    "cloud_absence_seen",
    "cloud_absence_result",
    "engine_state",
    "inbox",
    "cloud_reference_waiting",
    "cloud_reference_drains",
    "cloud_row_classifications",
    "cloud_row_classification_coverage",
    "cloud_runtime_notes",
    "cloud_runtime_entities",
    "cloud_runtime_payloads",
    "cloud_runtime_payload_retired",
    "cloud_runtime_payload_owners",
    "cloud_media_state",
    "cloud_runtime_bindings",
    "cloud_adapter_state",
    "cloud_publication_bindings",
    "cloud_epoch_crt",
    "cloud_root_readability",
    "cloud_adoption_authority",
    "cloud_publication_zones",
];

/// Remove work only from an ordinary binding already checked in this scope.
pub(super) fn remove_work(tx: &Transaction<'_>, b: BindingId) -> Result<(), Error> {
    // The recovery workflow must settle these first. Never erase their inbox,
    // merge base or owner as a substitute for actual collection/WAL replay.
    if !super::incoming_store::intents(tx, b)?.is_empty() {
        return Err(Error::RecoveryRefused);
    }
    for table in WORK_TABLES {
        let exists: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name=?1)",
            [table],
            |r| r.get(0),
        )?;
        if exists {
            tx.execute(&format!("DELETE FROM {table} WHERE binding=?1"), [b.0.as_slice()])?;
        }
    }
    let copies: i64 = tx.query_row(
        "SELECT count(*) FROM conflicts WHERE binding=?1",
        [b.0.as_slice()],
        |r| r.get(0),
    )?;
    if copies == 0 {
        tx.execute("DELETE FROM bindings WHERE id=?1", [b.0.as_slice()])?;
    }
    Ok(())
}
impl TrackingStore {
    /// V9.2: no new open, no asset retry reset, no entity/work deletion.
    pub(super) fn resume_isolated_binding(
        &mut self,
        expected: &Binding,
        file: runtime::NativeFileIdentity,
        durable_file: runtime::DurableFileIdentity,
        publish: &Option<PublishOp>,
        adopt: &Option<adoption::AdoptOp>,
    ) -> Result<Binding, Error> {
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let facts: Option<String> = tx
            .query_row(
                "SELECT facts FROM bindings WHERE id=?1 AND container=?2 AND environment=?3 AND salvage=0",
                params![
                    expected.id.0.as_slice(),
                    self.scope.container.as_str(),
                    self.scope.environment.code()
                ],
                |r| r.get(0),
            )
            .optional()?;
        let facts: BindingFacts = decode(&facts.ok_or(Error::RecoveryRefused)?)?;
        let read_op = |table: &str| -> Result<Option<String>, Error> {
            Ok(tx
                .query_row(
                    &format!("SELECT content FROM {table} WHERE container=?1 AND environment=?2"),
                    params![self.scope.container.as_str(), self.scope.environment.code()],
                    |r| r.get(0),
                )
                .optional()?)
        };
        let current_publish: Option<PublishOp> = read_op("cloud_publication_ops")?.map(|s| decode(&s)).transpose()?;
        let current_adopt: Option<adoption::AdoptOp> =
            read_op("cloud_adoption_ops")?.map(|s| decode(&s)).transpose()?;
        let other_active: bool = tx.query_row(
            "SELECT EXISTS(SELECT 1 FROM bindings WHERE container=?1 AND environment=?2 AND active=1)",
            params![self.scope.container.as_str(), self.scope.environment.code()],
            |r| r.get(0),
        )?;
        if expected.scope != self.scope
            || facts != expected.facts
            || other_active
            || !matches!(
                facts.state,
                BindingState::Isolated(
                    IsolationReason::SignedOut | IsolationReason::AccountChanged | IsolationReason::Stopped
                )
            )
            || &current_publish != publish
            || &current_adopt != adopt
            || current_publish.as_ref().is_some_and(|p| {
                p.binding_after == Some(expected.id) || p.binding_before.as_ref().is_some_and(|b| b.id == expected.id)
            })
            || current_adopt.as_ref().is_some_and(|p| {
                p.binding_after == expected.id || p.binding_before.as_ref().is_some_and(|b| b.id == expected.id)
            })
        {
            return Err(Error::RecoveryRefused);
        }
        let content: String = tx.query_row(
            "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
            [expected.id.0.as_slice()],
            |r| r.get(0),
        )?;
        let mut state: StoredRuntimeState = decode(&content)?;
        if state.file.is_some_and(|stored| stored != file) {
            return Err(Error::RecoveryRefused);
        }
        state.record_identity(file, durable_file)?;
        state.drained = false;
        let mut resumed = expected.clone();
        resumed.facts.state = BindingState::Active;
        tx.execute(
            "UPDATE bindings SET facts=?2 WHERE id=?1",
            params![expected.id.0.as_slice(), encode(&resumed.facts)?],
        )?;
        // Preserve the existing DELETE trigger: it also clears absence progress.
        tx.execute("DELETE FROM engine_state WHERE binding=?1", [expected.id.0.as_slice()])?;
        super::runtime_store::put_binding(&tx, expected.id, &state)?;
        tx.commit()?;
        Ok(resumed)
    }
    pub(super) fn install_publication(&mut self) -> Result<(), Error> {
        check_version(&self.connection)?;
        let tx = self.connection.transaction()?;
        tx.execute_batch(SCHEMA)?;
        let version: i64 = tx.query_row("SELECT version FROM cloud_publication_schema", [], |r| r.get(0))?;
        if version < 3 {
            // No backfill: legacy crt may have come from media-root-local fallback.
            tx.execute_batch(super::epoch_crt_store::SCHEMA)?;
            tx.execute("UPDATE cloud_publication_schema SET version=3", [])?;
        }
        if version < 4 {
            // Legacy absence proves nothing: install an empty unknown ledger.
            tx.execute_batch(super::root_readability_store::SCHEMA)?;
            tx.execute("UPDATE cloud_publication_schema SET version=4", [])?;
        }
        tx.commit()?;
        if let Some(op) = self.publication_op()? {
            op.validate()?;
        }
        Ok(())
    }
    /// Scoped ordinary owners only. This read does not select or mutate an owner.
    pub(super) fn publication_bindings(&self) -> Result<Vec<Binding>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT id FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0 ORDER BY id")?;
        let ids = statement
            .query_map(
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |r| r.get::<_, Vec<u8>>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        ids.into_iter()
            .map(|id| {
                self.binding(super::store::binding_id(id)?)?
                    .ok_or(Error::BindingNotFound)
            })
            .collect()
    }
    /// Read only resolution metadata of ordinary owners in this build scope.
    /// Other builds and salvage payloads never enter the projection.
    pub(super) fn publication_unresolved_conflict_count(&self) -> Result<u64, Error> {
        // Keep scoped bindings outermost; look up only their indexed copies.
        let count: i64 = self.connection.query_row(
            "SELECT count(*) FROM bindings AS b CROSS JOIN conflicts AS c ON c.binding=b.id
             WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0
             AND json_extract(c.content,'$.resolved_at_ms') IS NULL",
            params![self.scope.container.as_str(), self.scope.environment.code()],
            |row| row.get(0),
        )?;
        u64::try_from(count).map_err(|_| Error::InvalidData)
    }
    pub fn publication_op(&self) -> Result<Option<PublishOp>, Error> {
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_publication_ops WHERE container=?1 AND environment=?2",
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |r| r.get(0),
            )
            .optional()?;
        let op: Option<PublishOp> = text.map(|t| decode(&t)).transpose()?;
        if let Some(op) = &op {
            op.validate()?;
        }
        Ok(op)
    }
    pub(super) fn publication_binding_for_account(&self, account: &OpaqueBytes) -> Result<Option<Binding>, Error> {
        let mut s = self.connection.prepare("SELECT id FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0 ORDER BY active DESC,created_at_ms DESC,id DESC")?;
        let ids = s
            .query_map(
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |r| r.get::<_, Vec<u8>>(0),
            )?
            .collect::<Result<Vec<_>, _>>()?;
        for id in ids {
            let b = self
                .binding(super::store::binding_id(id)?)?
                .ok_or(Error::BindingNotFound)?;
            if &b.facts.account == account {
                return Ok(Some(b));
            }
        }
        Ok(None)
    }
    pub(super) fn confirm_publication(&mut self, op: &PublishOp) -> Result<(), Error> {
        if self.publication_op()?.is_some() {
            return Err(Error::Constraint);
        }
        let tx = self.connection.transaction()?;
        put_op(&tx, &self.scope, op)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn write_publication_op(&mut self, op: &PublishOp) -> Result<(), Error> {
        if self.publication_op()?.as_ref().map(|o| o.id) != Some(op.id) {
            return Err(Error::InvalidData);
        }
        let tx = self.connection.transaction()?;
        put_op(&tx, &self.scope, op)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn discard_confirmed_publication(&mut self, op: &PublishOp) -> Result<(), Error> {
        if op.step != PublishStep::Confirmed {
            return Err(Error::InvalidData);
        }
        let tx = self.connection.transaction()?;
        remove_op(&tx, &self.scope, op.id)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn bind_publication(
        &mut self,
        op: &mut PublishOp,
        id: BindingId,
        now: i64,
        durable_file: runtime::DurableFileIdentity,
    ) -> Result<Binding, Error> {
        durable_file.paired(op.confirmed_file)?;
        if op.step != PublishStep::Confirmed
            || op.binding_after.is_some()
            || self.publication_op()?.as_ref() != Some(&*op)
        {
            return Err(Error::InvalidData);
        }
        let before = op
            .binding_before
            .as_ref()
            .map(|p| self.binding(p.id)?.ok_or(Error::BindingNotFound))
            .transpose()?;
        if before
            .as_ref()
            .zip(op.binding_before.as_ref())
            .is_some_and(|(b, p)| b.facts.state != p.state || b.facts.account != op.account)
        {
            return Err(Error::InvalidData);
        }
        let b = Binding {
            id,
            created_at_ms: now,
            scope: self.scope.clone(),
            facts: BindingFacts {
                account: op.account.clone(),
                epoch: op.epoch,
                record_key: op.record_key.clone(),
                publication: op.confirmed_publication,
                state: BindingState::Active,
            },
        };
        let tx = self.connection.transaction()?;
        if let Some(mut before) = before
            && before.facts.state == BindingState::Active
        {
            before.facts.state = BindingState::Isolated(IsolationReason::Republished);
            tx.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![before.id.0.as_slice(), encode(&before.facts)?],
            )?;
        }
        tx.execute(
            "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES(?1,?2,?3,?4,?5)",
            params![
                id.0.as_slice(),
                self.scope.container.as_str(),
                self.scope.environment.code(),
                now,
                encode(&b.facts)?
            ],
        )?;
        super::incomplete_store::start_binding_ledger(&tx, id)?;
        super::runtime_store::put_binding(
            &tx,
            id,
            &StoredRuntimeState {
                file: Some(op.confirmed_file),
                durable_file: Some(durable_file),
                ..StoredRuntimeState::default()
            },
        )?;
        tx.execute(
            "INSERT INTO cloud_publication_bindings(binding,media_key,root_acked,crt) VALUES(?1,?2,0,?3)",
            params![id.0.as_slice(), op.media_key.0.as_slice(), op.crt],
        )?;
        if let Some(epoch) = op.active_epoch {
            tx.execute(
                "INSERT INTO cloud_publication_zones(binding,zone,first_seen_ms,deletion_due) VALUES(?1,?2,?3,0)",
                params![id.0.as_slice(), super::adapter::collection_zone(epoch), now],
            )?;
        }
        op.binding_after = Some(id);
        op.step = PublishStep::Bound;
        put_op(&tx, &self.scope, op)?;
        tx.commit()?;
        Ok(b)
    }
    pub(super) fn publication_media(&self, id: BindingId) -> Result<Option<(RecordKey, bool, i64)>, Error> {
        self.binding(id)?.ok_or(Error::BindingNotFound)?;
        let row: Option<(Vec<u8>, bool, i64)> = self
            .connection
            .query_row(
                "SELECT media_key,root_acked,crt FROM cloud_publication_bindings WHERE binding=?1",
                [id.0.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .optional()?;
        row.map(|(key, ack, crt)| Ok((RecordKey(key.try_into().map_err(|_| Error::InvalidData)?), ack, crt)))
            .transpose()
    }
    pub(super) fn accept_media_root(&mut self, id: BindingId, key: [u8; 32], crt: i64) -> Result<(), Error> {
        self.binding(id)?.ok_or(Error::BindingNotFound)?;
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO cloud_publication_bindings(binding,media_key,root_acked,crt) VALUES(?1,?2,1,?3) ON CONFLICT(binding) DO UPDATE SET media_key=excluded.media_key,root_acked=1 WHERE root_acked=0",
            params![id.0.as_slice(), key.as_slice(), crt],
        )?;
        // Use only the authority actually retained by the conditional write.
        let retained: Vec<u8> = tx.query_row(
            "SELECT media_key FROM cloud_publication_bindings WHERE binding=?1 AND root_acked=1",
            [id.0.as_slice()],
            |r| r.get(0),
        )?;
        let retained: [u8; 32] = retained.try_into().map_err(|_| Error::InvalidData)?;
        let mut ledger = super::media::cleanup::ledger(&tx, id)?;
        if super::media::store::materialize_rechecks(&mut ledger, &retained) {
            super::media::store::put_ledger(&tx, id, &ledger)?;
        }
        tx.commit()?;
        Ok(())
    }
    pub(super) fn publication_ack_control(
        &mut self,
        op: &PublishOp,
        accepted_epoch: Option<&super::control_codec::Epoch>,
    ) -> Result<(), Error> {
        let id = op.binding_after.ok_or(Error::InvalidData)?;
        self.binding(id)?.ok_or(Error::BindingNotFound)?;
        let tx = self.connection.transaction()?;
        if let Some(epoch) = accepted_epoch {
            super::epoch_crt_store::put(&tx, id, epoch, super::epoch_crt_store::Authority::Published)?;
            super::root_readability_store::observe(&tx, id, None, Some(epoch.state == super::control_codec::EpochState::Complete))?;
        }
        tx.execute(
            "UPDATE cloud_publication_bindings SET media_key=?2,root_acked=?3 WHERE binding=?1",
            params![id.0.as_slice(), op.media_key.0.as_slice(), op.root_acked],
        )?;
        if op.root_acked {
            let mut ledger = super::media::cleanup::ledger(&tx, id)?;
            if super::media::store::materialize_rechecks(&mut ledger, &op.media_key.0) {
                super::media::store::put_ledger(&tx, id, &ledger)?;
            }
        }
        put_op(&tx, &self.scope, op)?;
        #[cfg(feature = "test-hooks")]
        super::testing::tracking_commit(&tx, "cloud.publication.ack_control")?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn publication_done(
        &mut self,
        op: &PublishOp,
        saved: &ControlRecord,
        open: u64,
        now: i64,
    ) -> Result<(), Error> {
        let id = op.binding_after.ok_or(Error::InvalidData)?;
        self.binding(id)?.ok_or(Error::BindingNotFound)?;
        let mut state = self.runtime_binding(id)?;
        state.root_ack = Some(super::workflow::RootAcknowledgement {
            open,
            request_id: 0,
            epoch: op.epoch,
            envelope: OpaqueBytes(saved.encrypted.clone().ok_or(Error::InvalidData)?),
            system_fields: saved.system_fields.clone(),
        });
        if let Some(before) = &op.binding_before {
            self.binding(before.id)?.ok_or(Error::BindingNotFound)?;
        }
        let tx = self.connection.transaction()?;
        super::runtime_store::put_binding(&tx, id, &state)?;
        // The adapter's durable last applied Active must agree with the same commit.
        let mut durable = super::adapter::Durable::default();
        let text: Option<String> = tx
            .query_row(
                "SELECT content FROM cloud_adapter_state WHERE binding=?1",
                [id.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        if let Some(text) = text {
            durable = decode(&text)?;
        }
        durable.active = Some(super::workflow::Saved {
            name: "active".into(),
            envelope: saved.encrypted.clone().ok_or(Error::InvalidData)?,
            system_fields: saved.system_fields.0.clone(),
        });
        super::root_readability_store::observe(&tx, id, durable.active.as_ref(), None)?;
        tx.execute("INSERT INTO cloud_adapter_state(binding,content) VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![id.0.as_slice(), encode(&durable)?])?;
        // E0/E1 and an earlier landed attempt's clock come from the actual
        // accepted Active, while local retention starts no earlier than done.
        tx.execute(
            "UPDATE cloud_publication_zones SET first_seen_ms=max(first_seen_ms,?2) WHERE binding=?1",
            params![id.0.as_slice(), now],
        )?;
        if let Some(before) = &op.binding_before {
            remove_work(&tx, before.id)?;
        }
        remove_op(&tx, &self.scope, op.id)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn cancel_publication(&mut self, op: &PublishOp, returned_state: BindingState) -> Result<(), Error> {
        let before = op
            .binding_before
            .as_ref()
            .map(|p| self.binding(p.id)?.ok_or(Error::BindingNotFound))
            .transpose()?;
        if let Some(id) = op.binding_after {
            self.binding(id)?.ok_or(Error::BindingNotFound)?;
        }
        let tx = self.connection.transaction()?;
        if let Some(id) = op.binding_after {
            remove_work(&tx, id)?;
        }
        if let Some(mut before) = before {
            before.facts.state = returned_state;
            tx.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![before.id.0.as_slice(), encode(&before.facts)?],
            )?;
            if returned_state == BindingState::Active {
                let text: Option<String> = tx
                    .query_row(
                        "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
                        [before.id.0.as_slice()],
                        |r| r.get(0),
                    )
                    .optional()?;
                let mut state: StoredRuntimeState = text.map(|s| decode(&s)).transpose()?.unwrap_or_default();
                state.drained = false;
                super::runtime_store::put_binding(&tx, before.id, &state)?;
            }
        }
        if !op.switch_sent
            && returned_state == BindingState::Active
            && let Some(before) = &op.binding_before
        {
            tx.execute("INSERT INTO cloud_publication_zones(binding,zone,first_seen_ms,deletion_due) VALUES(?1,?2,?3,1) ON CONFLICT(binding,zone) DO UPDATE SET deletion_due=1", params![before.id.0.as_slice(), super::adapter::collection_zone(op.epoch), op.started_at_ms])?;
        }
        remove_op(&tx, &self.scope, op.id)?;
        tx.commit()?;
        Ok(())
    }
}

/// Called by scan/ack transactions, never by a transport's claimed completion.
pub(super) fn advance_sent(tx: &Transaction<'_>, scope: &BuildScope, b: BindingId) -> Result<(), Error> {
    let text: Option<String> = tx
        .query_row(
            "SELECT content FROM cloud_publication_ops WHERE container=?1 AND environment=?2",
            params![scope.container.as_str(), scope.environment.code()],
            |r| r.get(0),
        )
        .optional()?;
    let Some(text) = text else {
        return Ok(());
    };
    let mut op: PublishOp = decode(&text)?;
    if op.step != PublishStep::Zones || op.binding_after != Some(b) {
        return Ok(());
    }
    let content: String = tx.query_row(
        "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
        [b.0.as_slice()],
        |r| r.get(0),
    )?;
    let state: StoredRuntimeState = decode(&content)?;
    if !state.coverage.as_ref().is_some_and(|c| c.open == state.open_counter) {
        return Ok(());
    }
    let pending: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cloud_runtime_notes WHERE binding=?1 AND (json_extract(content,'$.mark') IS NOT NULL OR json_extract(content,'$.acked_rev') < json_extract(content,'$.local_rev'))) OR EXISTS(SELECT 1 FROM cloud_runtime_entities WHERE binding=?1 AND (json_extract(content,'$.mark') IS NOT NULL OR json_extract(content,'$.acked_rev') < json_extract(content,'$.local_rev')))", [b.0.as_slice()], |r| r.get(0))?;
    let failed: bool = tx.query_row("SELECT EXISTS(SELECT 1 FROM cloud_row_classifications WHERE binding=?1 AND failure!=0)", [b.0.as_slice()], |r| r.get(0))?;
    if !pending && !failed {
        op.step = PublishStep::Sent;
        put_op(tx, scope, &op)?;
    }
    Ok(())
}
/// K6 references, independent of SDK order and the current batch budget.
pub(super) fn references(payload: &entity_codec::Payload) -> Result<Vec<(entity_codec::Kind, Vec<u8>)>, Error> {
    use entity_codec::{Kind as K, Payload as P};
    use meguri_model::proto::{
        BlobMessage,
        deck::{DeckKind, DeckKindMessage},
    };
    let mut out = Vec::new();
    let mut add = |kind, id| {
        if id != 0 {
            out.push((kind, entity_codec::id_key(id).to_vec()));
        }
    };
    match payload {
        P::Note(n) => add(K::Notetype, n.mid),
        P::Card(c) => {
            add(K::Note, c.nid);
            add(K::Deck, c.did);
            add(K::Deck, c.odid);
        }
        P::Deck(d) => {
            add(K::Deck, d.parent_id);
            if let DeckKind::Normal(n) = DeckKindMessage::decode(&d.kind).map_err(|_| Error::InvalidData)?.kind {
                add(K::Preset, n.config_id);
            }
        }
        _ => {}
    }
    Ok(out)
}

impl TrackingStore {
    pub(super) fn collection_references(
        &self,
        b: BindingId,
        payload: &entity_codec::Payload,
    ) -> Result<Vec<String>, Error> {
        let binding = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let key = &binding.facts.record_key.0;
        let mut names = references(payload)?
            .into_iter()
            .map(|(kind, canonical)| entity_codec::entity_name(key, kind, &canonical).map_err(|_| Error::InvalidData))
            .collect::<Result<Vec<_>, _>>()?;
        if let entity_codec::Payload::Note(note) = payload {
            let fields = std::str::from_utf8(&note.flds).map_err(|_| Error::InvalidData)?;
            let local_references = meguri_text::media_refs::local_references(fields);
            if local_references.is_empty() {
                return Ok(names);
            }
            let ledger = self.media_ledger(b)?;
            for local in local_references {
                let case = local.to_lowercase();
                let name = entity_codec::entity_name(key, entity_codec::Kind::MediaName, case.as_bytes())
                    .map_err(|_| Error::InvalidData)?;
                if ledger.names.get(&case).is_none_or(|flags| flags.withheld.is_none())
                    && self
                        .entity_header(b, &name)?
                        .is_some_and(|header| header.facts.server_erev == 0 || header.facts.acked_media_tombstone())
                {
                    names.push(name);
                }
            }
        }
        Ok(names)
    }
    pub fn retained_zones(&self, b: BindingId) -> Result<std::collections::BTreeMap<String, RetainedZone>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let mut s = self.connection.prepare(
            "SELECT zone,first_seen_ms,deletion_due FROM cloud_publication_zones WHERE binding=?1 ORDER BY zone",
        )?;
        Ok(s.query_map([b.0.as_slice()], |r| {
            Ok((
                r.get(0)?,
                RetainedZone {
                    first_seen_ms: r.get(1)?,
                    deletion_due: r.get(2)?,
                },
            ))
        })?
        .collect::<Result<_, _>>()?)
    }
    pub(super) fn record_publication_zones(&mut self, b: BindingId, names: &[String], now: i64) -> Result<(), Error> {
        let owner = self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let own = super::adapter::collection_zone(owner.facts.epoch);
        let tx = self.connection.transaction()?;
        for name in names.iter().filter(|n| valid_collection_zone(n) && **n != own) {
            tx.execute("INSERT INTO cloud_publication_zones(binding,zone,first_seen_ms,deletion_due) VALUES(?1,?2,?3,0) ON CONFLICT(binding,zone) DO NOTHING", params![b.0.as_slice(), name, now])?;
        }
        tx.commit()?;
        Ok(())
    }
    #[cfg(feature = "test-hooks")]
    pub(super) fn mark_publication_zones_due(&mut self, b: BindingId, now: i64) -> Result<(), Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        let tx = self
            .connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        mark_zones_due(&tx, b, now)?;
        tx.commit()?;
        Ok(())
    }
    pub(super) fn remove_publication_zone(&mut self, b: BindingId, zone: &str) -> Result<(), Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        self.connection.execute(
            "DELETE FROM cloud_publication_zones WHERE binding=?1 AND zone=?2",
            params![b.0.as_slice(), zone],
        )?;
        Ok(())
    }
}
pub(super) fn valid_collection_zone(zone: &str) -> bool {
    let Some(encoded) = zone.strip_prefix("c-") else {
        return false;
    };
    if encoded.len() != 26
        || !encoded
            .bytes()
            .all(|b| b.is_ascii_lowercase() || (b'2'..=b'7').contains(&b))
    {
        return false;
    }
    let Ok(bytes) = data_encoding::BASE32_NOPAD.decode(encoded.to_ascii_uppercase().as_bytes()) else {
        return false;
    };
    bytes.len() == 16 && data_encoding::BASE32_NOPAD.encode(&bytes).to_ascii_lowercase() == encoded
}

// Caller has already scoped and confirmed the active binding in this transaction.
pub(super) fn mark_zones_due(tx: &rusqlite::Transaction<'_>, b: BindingId, now: i64) -> Result<(), Error> {
    let state: super::runtime::StoredRuntimeState = decode(&tx.query_row(
        "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
        [b.0.as_slice()],
        |r| r.get::<_, String>(0),
    )?)?;
    let active = super::control_codec::Active::decode(&state.root_ack.ok_or(Error::InvalidData)?.envelope.0)
        .map_err(|_| Error::InvalidData)?;
    let facts: BindingFacts = decode(&tx.query_row(
        "SELECT facts FROM bindings WHERE id=?1",
        [b.0.as_slice()],
        |r| r.get::<_, String>(0),
    )?)?;
    if active.epoch != facts.epoch.0 || facts.state != BindingState::Active {
        return Err(Error::InvalidData);
    }
    let own = super::adapter::collection_zone(facts.epoch);
    let previous = active.previous.map(|e| super::adapter::collection_zone(EpochId(e)));
    let zones: Vec<(String, i64)> = {
        let mut q = tx.prepare("SELECT zone,first_seen_ms FROM cloud_publication_zones WHERE binding=?1")?;
        q.query_map([b.0.as_slice()], |r| Ok((r.get(0)?, r.get(1)?)))?
            .collect::<Result<_, _>>()?
    };
    for (name, first) in zones {
        if name == own {
            continue;
        }
        let from = if previous.as_ref() == Some(&name) {
            first.max(active.activated_at_ms)
        } else {
            first
        };
        if now.checked_sub(from).is_some_and(|age| age >= 30 * 86_400_000) {
            tx.execute(
                "UPDATE cloud_publication_zones SET deletion_due=1 WHERE binding=?1 AND zone=?2",
                params![b.0.as_slice(), name],
            )?;
        }
    }
    Ok(())
}
