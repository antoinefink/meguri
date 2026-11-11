//! Versioned additive runtime tables inside the V3 tracking file. This
//! extension never reconstructs server facts.
use rusqlite::{OptionalExtension, Transaction, params};

use super::runtime::{DirtyMark, DirtySource, NativeFileIdentity, NoteTracking, RuntimeError, StoredRuntimeState};
use super::{
    entity_codec as codec,
    store::{decode, encode},
    *,
};

pub(super) const VERSION: i64 = 7;
const SCHEMA: &str = "
CREATE TABLE cloud_runtime_schema(version INTEGER NOT NULL CHECK(version > 0));
INSERT INTO cloud_runtime_schema VALUES (2);
CREATE TABLE cloud_runtime_bindings(
 binding BLOB PRIMARY KEY REFERENCES bindings(id), content TEXT NOT NULL CHECK(json_valid(content))
);
CREATE TABLE cloud_runtime_notes(
 binding BLOB NOT NULL REFERENCES bindings(id), id INTEGER NOT NULL,
 content TEXT NOT NULL CHECK(json_valid(content)), PRIMARY KEY(binding,id)
);
";

/// The outer recovery schema is V3. Refuse a newer runtime extension in
/// the private WAL-aware probe, before SQLite opens the original path.
pub(super) fn check_version(connection: &rusqlite::Connection) -> Result<bool, Error> {
    super::policy_store::check_version(connection)?;
    super::workflow_store::check_version(connection)?;
    super::publication_store::check_version(connection)?;
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_runtime_schema')",
        [],
        |row| row.get(0),
    )?;
    if exists {
        let mut statement = connection.prepare("SELECT version FROM cloud_runtime_schema")?;
        let versions = statement
            .query_map([], |row| row.get::<_, i64>(0))?
            .collect::<Result<Vec<_>, _>>()?;
        if versions != [1]
            && versions != [2]
            && versions != [3]
            && versions != [4]
            && versions != [5]
            && versions != [6]
            && versions != [VERSION]
        {
            return Err(Error::RuntimeSchemaUnsupported);
        }
        if versions == [6] || versions == [7] {
            let owners: bool = connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_payload_owners')",
                [],
                |r| r.get(0),
            )?;
            if !owners {
                return Err(Error::InvalidData);
            }
        }
    } else {
        let orphan: bool = connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_runtime_bindings','cloud_runtime_notes'))",
            [],
            |row| row.get(0),
        )?;
        if orphan {
            return Err(Error::InvalidData);
        }
    }
    Ok(exists)
}

fn decode_binding(connection: &rusqlite::Connection, content: &str) -> Result<StoredRuntimeState, Error> {
    let version: i64 = connection.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get(0))?;
    let value: StoredRuntimeState = decode(content)?;
    if version != VERSION && value.durable_file.is_some() {
        return Err(Error::InvalidData);
    }
    value.validate()?;
    Ok(value)
}

pub(super) fn validate_bindings(connection: &rusqlite::Connection, scope: &BuildScope) -> Result<(), Error> {
    let exists: bool = connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_bindings')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        return Ok(());
    }
    let mut statement = connection.prepare(
        "SELECT r.content FROM cloud_runtime_bindings r JOIN bindings b ON b.id=r.binding WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0",
    )?;
    let mut rows = statement.query(params![scope.container.as_str(), scope.environment.code()])?;
    while let Some(row) = rows.next()? {
        decode_binding(connection, &row.get::<_, String>(0)?)?;
    }
    Ok(())
}

impl TrackingStore {
    /// Called only under the runtime's process-wide tracking access lock.
    /// A missing extension installs an empty one; no ack is inferred. A future
    /// extension is refused before any DDL or data mutation.
    pub(super) fn install_runtime(&mut self) -> Result<(), Error> {
        super::acknowledgement_store::validate(&self.connection)?;
        let exists = check_version(&self.connection)?;
        if !exists {
            let transaction = self.connection.transaction()?;
            transaction.execute_batch(SCHEMA)?;
            transaction.commit()?;
        }
        let transaction = self.connection.transaction()?;
        super::adapter_store::install(&transaction)?;
        super::incoming_store::install_asset_reads(&transaction)?;
        super::tracking_payload::migrate(&transaction, &self.scope)?;
        transaction.commit()?;
        self.install_workflow()?;
        self.install_publication()?;
        self.install_adoption()?;
        self.install_cloud_policy()?;
        // Ordinary payloads belong to their container/environment only.
        let mut statement = self.connection.prepare(
            "SELECT r.binding,r.content FROM cloud_runtime_bindings r JOIN bindings b ON b.id=r.binding
             WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0",
        )?;
        let mut rows = statement.query(params![self.scope.container.as_str(), self.scope.environment.code()])?;
        while let Some(row) = rows.next()? {
            let _: BindingId = super::store::binding_id(row.get(0)?)?;
            decode_binding(&self.connection, &row.get::<_, String>(1)?)?;
        }
        let mut statement = self.connection.prepare(
            "SELECT r.id,r.content,r.binding FROM cloud_runtime_notes r JOIN bindings b ON b.id=r.binding
             WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0",
        )?;
        let mut rows = statement.query(params![self.scope.container.as_str(), self.scope.environment.code()])?;
        while let Some(row) = rows.next()? {
            let owner = super::store::binding_id(row.get(2)?)?;
            let value = super::tracking_payload::load_note(&self.connection, owner, &row.get::<_, String>(1)?)?;
            if value.id != row.get::<_, i64>(0)? {
                return Err(Error::InvalidData);
            }
            value.validate()?;
            super::tracking_payload::validate_flights(
                &self.connection,
                owner,
                codec::Kind::Note,
                &codec::id_key(value.id),
                &value.in_flight,
            )?;
        }
        Ok(())
    }

    pub(super) fn runtime_binding(&self, binding: BindingId) -> Result<StoredRuntimeState, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let content: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
                [binding.0.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        content
            .map(|value| decode_binding(&self.connection, &value))
            .transpose()
            .map(|value| value.unwrap_or_default())
    }

    /// Closed-collection readout for adapters; no raw SQL or reconstructed
    /// defaults. The caller serializes access with runtime owners.
    pub fn stored_runtime_state(&self, binding: BindingId) -> Result<Option<StoredRuntimeState>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if !check_version(&self.connection)? {
            return Ok(None);
        }
        let content: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_bindings WHERE binding=?1",
                [binding.0.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        content
            .map(|value| decode_binding(&self.connection, &value))
            .transpose()
    }

    pub(super) fn open_runtime_binding(
        &mut self,
        binding: BindingId,
        file: NativeFileIdentity,
        durable_file: super::durable_identity::DurableFileIdentity,
    ) -> Result<StoredRuntimeState, Error> {
        let mut value = self.runtime_binding(binding)?;
        if !value.reopen_matches(file, durable_file)? {
            return Err(Error::RecoveryRefused);
        }
        value.open_counter = next(value.open_counter)?;
        value.drained = false;
        value.record_identity(file, durable_file)?;
        let transaction = self.connection.transaction()?;
        put_binding(&transaction, binding, &value)?;
        // A lost post-handler first read follows ordinary retries on the next open.
        transaction.execute(
            "DELETE FROM cloud_asset_first_reads WHERE binding=?1",
            [binding.0.as_slice()],
        )?;
        // Only an actual collection open resets asset attempts, including 1..4.
        transaction.execute(
            "UPDATE inbox SET entry=json_set(entry,'$.retry_count',0)
             WHERE binding=?1 AND json_extract(entry,'$.reason')='Asset'",
            [binding.0.as_slice()],
        )?;
        transaction.commit()?;
        Ok(value)
    }

    /// A real isolated open needs a fresh observer namespace too. Retain its
    /// file evidence and all recovery/send state until an explicit restore.
    pub(super) fn open_isolated_runtime_binding(&mut self, binding: BindingId) -> Result<StoredRuntimeState, Error> {
        let mut value = self.runtime_binding(binding)?;
        value.open_counter = next(value.open_counter)?;
        value.drained = false;
        let transaction = self.connection.transaction()?;
        put_binding(&transaction, binding, &value)?;
        transaction.commit()?;
        Ok(value)
    }

    pub(super) fn isolate_runtime_binding(&mut self, binding: BindingId, reason: IsolationReason) -> Result<(), Error> {
        let mut value = self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if !isolation_replaces(value.facts.state, reason) {
            return Ok(());
        }
        value.facts.state = BindingState::Isolated(reason);
        let transaction = self.connection.transaction()?;
        transaction.execute(
            "UPDATE bindings SET facts=?2 WHERE id=?1",
            params![binding.0.as_slice(), encode(&value.facts)?],
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub(super) fn isolate_runtime_binding_with_active(
        &mut self,
        binding: BindingId,
        reason: IsolationReason,
        active: super::workflow::Saved,
    ) -> Result<super::adapter::Durable, Error> {
        let mut value = self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let replaces = isolation_replaces(value.facts.state, reason);
        let transaction = self.connection.transaction()?;
        let content: Option<String> = transaction
            .query_row(
                "SELECT content FROM cloud_adapter_state WHERE binding=?1",
                [binding.0.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        let mut durable: super::adapter::Durable = content.map(|text| decode(&text)).transpose()?.unwrap_or_default();
        durable.active = Some(active);
        // Persist before isolation: its trigger must clear absence and seen
        // state without a later write restoring the previous absence ledger.
        transaction.execute(
            "INSERT INTO cloud_adapter_state(binding,content) VALUES (?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content",
            params![binding.0.as_slice(), encode(&durable)?],
        )?;
        if replaces {
            value.facts.state = BindingState::Isolated(reason);
            transaction.execute(
                "UPDATE bindings SET facts=?2 WHERE id=?1",
                params![binding.0.as_slice(), encode(&value.facts)?],
            )?;
        }
        transaction.commit()?;
        self.adapter_durable(binding)
    }

    pub fn tracked_note(&self, binding: BindingId, id: i64) -> Result<Option<NoteTracking>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let text: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
                params![binding.0.as_slice(), id],
                |r| r.get(0),
            )
            .optional()?;
        text.map(|text| super::tracking_payload::dump_note(&self.connection, binding, &text))
            .transpose()
    }

    pub(super) fn capture_runtime_notes(
        &mut self,
        binding: BindingId,
        open: u64,
        commit: &meguri_engine::ObservedCommit<'_>,
        rows: &meguri_engine::ObserverRows<'_>,
        mut after_write: impl FnMut(usize) -> Result<(), Error>,
    ) -> Result<(), RuntimeError> {
        checked(commit.seq.0)?;
        checked(commit.generation.0)?;
        let mut state = self.runtime_binding(binding)?;
        if state.open_counter != open {
            return Err(Error::InvalidData.into());
        }
        let transaction = self.connection.transaction().map_err(Error::from)?;
        // Journal iteration and all image reads remain inside this transaction.
        // Dropping it on any later failure also rolls back earlier entries.
        for (index, delta) in commit.payload.rows(meguri_storage::Table::Notes).enumerate() {
            let delta = delta?;
            let meguri_storage::RowKey::Id(id) = delta.key else {
                return Err(Error::InvalidData.into());
            };
            let mut tracked = note(&transaction, binding, id)?
                .ok_or(RuntimeError::Incomplete(super::runtime::Incomplete::NewOrRemovedNote))?;
            let layout = tracked.layout_generation()?;
            let before = delta
                .before
                .as_ref()
                .map(|row| super::runtime::note_image(row, layout))
                .transpose()?;
            let after = rows
                .row_image(meguri_storage::Table::Notes, &meguri_storage::RowKey::Id(id))?
                .as_ref()
                .map(|row| super::runtime::note_image(row, layout))
                .transpose()?;
            if before.is_none() || after.is_none() {
                return Err(RuntimeError::Incomplete(super::runtime::Incomplete::NewOrRemovedNote));
            }
            tracked.require_supported_update(before.as_ref(), after.as_ref())?;
            apply_image(
                &mut state,
                &mut tracked,
                before.as_ref(),
                after.as_ref(),
                DirtySource::Observer,
            )?;
            tracked.observer_open = open;
            tracked.observed_seq = commit.seq.0;
            tracked.observed_generation = commit.generation.0;
            put_note(&transaction, binding, &tracked)?;
            after_write(index + 1)?;
        }
        put_binding(&transaction, binding, &state)?;
        transaction.commit().map_err(Error::from)?;
        Ok(())
    }

    // Return encoded tracking inputs only; scan decoding happens off the writer guards.
    pub(super) fn runtime_note_contents(&self, binding: BindingId, ids: &[i64]) -> Result<Vec<String>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        ids.iter()
            .map(|id| {
                self.connection
                    .query_row(
                        "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
                        params![binding.0.as_slice(), id],
                        |row| row.get(0),
                    )
                    .map_err(Error::from)
                    .and_then(|text: String| super::tracking_payload::load_note(&self.connection, binding, &text))
                    .and_then(|n| encode(&n))
            })
            .collect()
    }

    pub(super) fn runtime_note_ids_page(&self, binding: BindingId, after: Option<i64>) -> Result<Vec<i64>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let mut statement = self.connection.prepare(
            "SELECT id FROM cloud_runtime_notes WHERE binding=?1 AND (?2 IS NULL OR id>?2) ORDER BY id LIMIT 250",
        )?;
        let values = statement
            .query_map(params![binding.0.as_slice(), after], |row| row.get(0))?
            .collect::<Result<Vec<_>, _>>()?;
        Ok(values)
    }

    pub(super) fn merge_runtime_note_page(
        &mut self,
        binding: BindingId,
        open: u64,
        seq: meguri_engine::Seq,
        generation: meguri_engine::Generation,
        images: Vec<PreparedNote>,
    ) -> Result<Vec<String>, Error> {
        let mut state = self.runtime_binding(binding)?;
        if state.open_counter != open {
            return Err(Error::InvalidData);
        }
        let transaction = self.connection.transaction()?;
        let mut changed = Vec::new();
        for image in images {
            let mut tracked = note(&transaction, binding, image.image.id)?.ok_or(Error::InvalidData)?;
            // Only EngineSeq values from this incarnation/generation are comparable.
            if tracked.observer_open == open
                && tracked.observed_generation == generation.0
                && tracked.observed_seq > seq.0
            {
                continue;
            }
            tracked.require_supported_update(None, Some(&image.image))?;
            if tracked.last_observed_digest == Some(image.row_digest) {
                continue;
            }
            tracked.local_rev = next(tracked.local_rev)?;
            tracked.last_observed_digest = Some(image.row_digest);
            tracked.local_payload = image.payload;
            if image.row_digest == tracked.acknowledged.row_digest {
                tracked.mark = None;
                tracked.base = None;
            } else {
                state.change_counter = next(state.change_counter)?;
                tracked.mark = Some(DirtyMark {
                    source: DirtySource::Snapshot,
                    change_seq: state.change_counter,
                    payload_digest: image.payload_digest,
                });
            }
            changed.push(tracked.name.clone());
            put_note(&transaction, binding, &tracked)?;
        }
        put_binding(&transaction, binding, &state)?;
        transaction.commit()?;
        Ok(changed)
    }

    pub(super) fn finish_runtime_note_scan(
        &mut self,
        binding: BindingId,
        open: u64,
        cursor: super::runtime::NoteScanCursor,
    ) -> Result<(), Error> {
        let mut state = self.runtime_binding(binding)?;
        if state.open_counter != open || state.file != Some(cursor.native_file) {
            return Err(Error::InvalidData);
        }
        state.note_scan = Some(cursor);
        let transaction = self.connection.transaction()?;
        put_binding(&transaction, binding, &state)?;
        transaction.commit()?;
        Ok(())
    }

    /// Inject a service/lifecycle isolation event through the real transition.
    #[cfg(feature = "test-hooks")]
    pub fn isolate_runtime_binding_for_testing(
        &mut self,
        binding: BindingId,
        reason: IsolationReason,
    ) -> Result<(), Error> {
        self.isolate_runtime_binding(binding, reason)
    }

    /// Seed validated historical state for a generated, closed collection fixture.
    #[cfg(feature = "test-hooks")]
    pub fn prepare_runtime_state_for_testing(
        &mut self,
        binding: BindingId,
        value: &StoredRuntimeState,
    ) -> Result<(), Error> {
        self.stored_runtime_state(binding)?.ok_or(Error::InvalidData)?;
        let transaction = self.connection.transaction()?;
        put_binding(&transaction, binding, value)?;
        transaction.commit()?;
        Ok(())
    }

    /// Corrupt an existing ordinary payload in a disposable tracking fixture.
    #[cfg(feature = "test-hooks")]
    pub fn damage_runtime_payload_for_testing(&mut self, binding: BindingId, note: Option<i64>) -> Result<(), Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let changed = match note {
            Some(id) => self.connection.execute(
                "UPDATE cloud_runtime_notes SET content='{}' WHERE binding=?1 AND id=?2",
                params![binding.0.as_slice(), id],
            )?,
            None => self.connection.execute(
                "UPDATE cloud_runtime_bindings SET content='{}' WHERE binding=?1",
                [binding.0.as_slice()],
            )?,
        };
        if changed != 1 {
            return Err(Error::InvalidData);
        }
        Ok(())
    }
    /// Inject a future extension marker without changing the outer schema.
    #[cfg(feature = "test-hooks")]
    pub fn future_runtime_version_for_testing(&mut self) -> Result<(), Error> {
        let changed = self
            .connection
            .execute("UPDATE cloud_runtime_schema SET version=?1", [VERSION + 1])?;
        if changed != 1 {
            return Err(Error::InvalidData);
        }
        Ok(())
    }

    /// Synthetic fixture preparation only, never a publication/adoption path.
    /// The caller owns the closed collection and has a real acknowledged
    /// canonical Note record from its synthetic service. No server write runs.
    #[cfg(feature = "test-hooks")]
    pub fn prepare_note_baseline_for_testing(
        &mut self,
        binding: BindingId,
        name: &str,
        envelope: &[u8],
        system_fields: OpaqueBytes,
    ) -> Result<(), Error> {
        self.install_runtime()?;
        let owner = self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if owner.facts.state != BindingState::Active || system_fields.0.is_empty() {
            return Err(Error::InvalidData);
        }
        let received = codec::receive(&owner.facts.record_key.0, name, envelope, codec::AssetInput::Absent)
            .map_err(|_| Error::InvalidData)?;
        let Some(codec::Payload::Note(image)) = received.payload else {
            return Err(Error::InvalidData);
        };
        if received.entity.fields.tombstone
            || received.entity.fields.generation.is_some()
            || image.nt_struct_gen == 0
            || received.entity.fields.erev == 0
            || received.entity.fields.key != codec::id_key(image.id)
        {
            return Err(Error::InvalidData);
        }
        checked(image.nt_struct_gen)?;
        checked(received.entity.fields.erev)?;
        let payload = codec::Payload::Note(image.clone());
        let bytes = payload.encode().map_err(|_| Error::InvalidData)?;
        let value = NoteTracking {
            id: image.id,
            name: name.into(),
            origin: received.entity.fields.origin,
            sent: true,
            local_rev: 0,
            last_observed_digest: Some(payload.row_digest().map_err(|_| Error::InvalidData)?),
            local_payload: OpaqueBytes(bytes.clone()),
            base: None,
            mark: None,
            acknowledged: super::runtime::NoteAcknowledgement {
                erev: received.entity.fields.erev,
                system_fields,
                payload: OpaqueBytes(bytes),
                row_digest: payload.row_digest().map_err(|_| Error::InvalidData)?,
                payload_digest: payload.payload_digest().map_err(|_| Error::InvalidData)?,
            },
            observer_open: 0,
            observed_seq: 0,
            observed_generation: 0,
            acked_rev: 0,
            in_flight: Vec::new(),
            acked_envelope: OpaqueBytes(envelope.to_vec()),
            tombstone_at: None,
        };
        // Do not allow a fixture to erase pending facts or rewrite a baseline.
        let transaction = self.connection.transaction()?;
        if note(&transaction, binding, value.id)?.is_some() {
            return Err(Error::Constraint);
        }
        super::tracking_payload::put_note(&transaction, binding, &value)?;
        transaction.commit()?;
        Ok(())
    }
}

pub(super) fn checked(value: u64) -> Result<i64, Error> {
    i64::try_from(value).map_err(|_| Error::InvalidData)
}
pub(super) fn next(value: u64) -> Result<u64, Error> {
    let value = value.checked_add(1).ok_or(Error::InvalidData)?;
    checked(value)?;
    Ok(value)
}
pub(super) fn note(
    connection: &rusqlite::Connection,
    binding: BindingId,
    id: i64,
) -> Result<Option<NoteTracking>, Error> {
    let value: Option<String> = connection
        .query_row(
            "SELECT content FROM cloud_runtime_notes WHERE binding=?1 AND id=?2",
            params![binding.0.as_slice(), id],
            |row| row.get(0),
        )
        .optional()?;
    value
        .map(|content| super::tracking_payload::load_note(connection, binding, &content))
        .transpose()
}
pub(super) fn put_note(transaction: &Transaction<'_>, binding: BindingId, value: &NoteTracking) -> Result<(), Error> {
    value.validate()?;
    super::tracking_payload::put_note(transaction, binding, value)?;
    Ok(())
}
pub(super) fn put_binding(
    transaction: &Transaction<'_>,
    binding: BindingId,
    value: &StoredRuntimeState,
) -> Result<(), Error> {
    value.validate()?;
    if value.durable_file.is_some() {
        let version: i64 = transaction.query_row("SELECT version FROM cloud_runtime_schema", [], |r| r.get(0))?;
        if version != VERSION {
            return Err(Error::RuntimeSchemaUnsupported);
        }
    }
    transaction.execute(
        "INSERT INTO cloud_runtime_bindings(binding,content) VALUES (?1,?2)
        ON CONFLICT(binding) DO UPDATE SET content=excluded.content",
        params![binding.0.as_slice(), encode(value)?],
    )?;
    Ok(())
}
fn apply_image(
    state: &mut StoredRuntimeState,
    tracked: &mut NoteTracking,
    before: Option<&codec::Note>,
    after: Option<&codec::Note>,
    source: DirtySource,
) -> Result<bool, Error> {
    let after = after.ok_or(Error::InvalidData)?;
    let payload = codec::Payload::Note(after.clone());
    let digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
    if tracked.last_observed_digest == Some(digest) {
        return Ok(false);
    }
    let bytes = payload.encode().map_err(|_| Error::InvalidData)?;
    tracked.local_rev = next(tracked.local_rev)?;
    tracked.last_observed_digest = Some(digest);
    tracked.local_payload = OpaqueBytes(bytes);
    if digest == tracked.acknowledged.row_digest {
        tracked.mark = None;
        tracked.base = None;
    } else {
        state.change_counter = next(state.change_counter)?;
        // A snapshot never recreates a lost before-image. Its merge fallback
        // is the acknowledged payload, per K11.2.
        if tracked.mark.is_none()
            && let Some(before) = before
        {
            let payload = codec::Payload::Note(before.clone());
            if payload.row_digest().map_err(|_| Error::InvalidData)? == tracked.acknowledged.row_digest {
                tracked.base = Some(OpaqueBytes(payload.encode().map_err(|_| Error::InvalidData)?));
            }
        }
        tracked.mark = Some(DirtyMark {
            source,
            change_seq: state.change_counter,
            payload_digest: payload.payload_digest().map_err(|_| Error::InvalidData)?,
        });
    }
    Ok(true)
}

// Service reasons keep their first event. Account events may supersede them
// or a previous account event; permanent reasons cannot be superseded.
fn isolation_replaces(state: BindingState, reason: IsolationReason) -> bool {
    use IsolationReason::*;
    match state {
        BindingState::Active => true,
        BindingState::Isolated(ReplacedLocally | FileMismatch | Stopped | Republished | TrackingDamaged) => false,
        BindingState::Isolated(
            SignedOut | AccountChanged | EpochSwitched | ZoneMissing | ZoneDeletedByUser | EncryptedDataReset
            | ActiveMissing,
        ) => matches!(reason, SignedOut | AccountChanged),
    }
}

pub(super) struct PreparedNote {
    pub image: codec::Note,
    pub payload: OpaqueBytes,
    pub row_digest: [u8; 32],
    pub payload_digest: [u8; 32],
}
impl PreparedNote {
    pub fn new(image: codec::Note) -> Result<Self, Error> {
        let payload = codec::Payload::Note(image.clone());
        Ok(Self {
            image,
            payload: OpaqueBytes(payload.encode().map_err(|_| Error::InvalidData)?),
            row_digest: payload.row_digest().map_err(|_| Error::InvalidData)?,
            payload_digest: payload.payload_digest().map_err(|_| Error::InvalidData)?,
        })
    }
}
