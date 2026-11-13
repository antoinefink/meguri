use std::path::Path;

use meguri_model::{clock::Clock, rng::RandomSource};
use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};

use super::*;

pub(super) const V1_SCHEMA: &str = "
CREATE TABLE bindings (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    container TEXT NOT NULL,
    environment INTEGER NOT NULL CHECK(environment IN (0,1)),
    created_at_ms INTEGER NOT NULL,
    facts TEXT NOT NULL CHECK(json_valid(facts)),
    active INTEGER GENERATED ALWAYS AS (json_extract(facts,'$.state')='Active') VIRTUAL,
    salvage INTEGER GENERATED ALWAYS AS
        (coalesce(json_extract(facts,'$.state.Isolated')='TrackingDamaged',0)) VIRTUAL
);
CREATE UNIQUE INDEX one_active_binding ON bindings(container,environment) WHERE active=1;
CREATE TABLE engine_state (
    binding BLOB PRIMARY KEY REFERENCES bindings(id),
    serialization BLOB NOT NULL,
    saved_at_ms INTEGER NOT NULL
);
CREATE TABLE inbox (
    id INTEGER PRIMARY KEY,
    binding BLOB NOT NULL REFERENCES bindings(id),
    entry TEXT NOT NULL CHECK(json_valid(entry)),
    UNIQUE(binding,entry)
);
";

pub(super) const V2_SCHEMA: &str = "
CREATE TABLE salvage_bindings (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    metadata TEXT NOT NULL CHECK(json_valid(metadata))
);
CREATE TABLE conflicts (
    id BLOB PRIMARY KEY CHECK(length(id)=16),
    binding BLOB NOT NULL CHECK(length(binding)=16),
    content TEXT NOT NULL CHECK(json_valid(content))
);
CREATE INDEX conflicts_binding ON conflicts(binding);
";

/// A connection bound permanently to one container/environment. SQL access is
/// private so callers cannot accidentally write another build's binding rows.
pub struct TrackingStore {
    pub(super) connection: Connection,
    pub(super) scope: BuildScope,
    pub(super) directory: std::path::PathBuf,
}

impl TrackingStore {
    /// The caller serializes the entire probe/open against ALL writers and
    /// checkpoints, including other stores/builds sharing the directory.
    /// This function does not open or attach the collection file.
    pub fn open(directory: &Path, scope: BuildScope) -> Result<OpenOutcome, Error> {
        super::open::open(directory, scope)
    }

    /// V8.1 storage recovery under caller-held exclusive ownership against
    /// ALL tracking opens/writes/checkpoints, across stores and builds. This
    /// function does not acquire host ownership; production wiring is pending.
    /// A failure returns no store, including after a partial salvage commit.
    pub fn recover(
        directory: &Path,
        scope: BuildScope,
        clock: &dyn Clock,
        random: &dyn RandomSource,
    ) -> Result<OpenOutcome, Error> {
        super::recovery::recover(directory, scope, clock, random)
    }

    /// Permanent attention/debug filenames. Kept damaged sources are never
    /// read again, removed by retention, or treated as recoverable work.
    pub fn retained_damaged_sources(&self) -> Result<Vec<String>, Error> {
        super::recovery::retained_sources(&self.directory)
    }

    pub fn scope(&self) -> &BuildScope {
        &self.scope
    }

    /// Insert supplied binding facts. No existing binding is isolated/resumed
    /// here: those transitions require the later scan and recovery contracts.
    pub fn create_binding(
        &mut self,
        facts: BindingFacts,
        clock: &dyn Clock,
        random: &dyn RandomSource,
    ) -> Result<Binding, Error> {
        if facts.state == BindingState::Isolated(IsolationReason::TrackingDamaged) {
            // Only the salvage transaction may create such an owner.
            return Err(Error::InvalidData);
        }
        let mut bits = [0; 16];
        bits[..8].copy_from_slice(&random.next_u64().to_be_bytes());
        bits[8..].copy_from_slice(&random.next_u64().to_be_bytes());
        let binding = Binding {
            id: BindingId(bits),
            created_at_ms: clock.now_ms(),
            scope: self.scope.clone(),
            facts,
        };
        let encoded = encode(&binding.facts)?;
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES (?1,?2,?3,?4,?5)",
            params![
                binding.id.0.as_slice(),
                self.scope.container.as_str(),
                self.scope.environment.code(),
                binding.created_at_ms,
                encoded
            ],
        )?;
        super::incomplete_store::start_binding_ledger(&transaction, binding.id)?;
        transaction.commit()?;
        Ok(binding)
    }

    pub fn binding(&self, id: BindingId) -> Result<Option<Binding>, Error> {
        let row: Option<(i64, String)> = self
            .connection
            .query_row(
                "SELECT created_at_ms,facts FROM bindings WHERE id=?1 AND container=?2 AND environment=?3",
                params![
                    id.0.as_slice(),
                    self.scope.container.as_str(),
                    self.scope.environment.code()
                ],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        row.map(|(created_at_ms, facts)| {
            Ok(Binding {
                id,
                created_at_ms,
                scope: self.scope.clone(),
                facts: decode(&facts)?,
            })
        })
        .transpose()
    }

    /// Stored selection only: active first, otherwise newest non-salvage.
    /// Ties in device-clock time use the id for a stable result.
    pub fn selected_binding(&self) -> Result<Option<Binding>, Error> {
        let id: Option<Vec<u8>> = self
            .connection
            .query_row(
                "SELECT id FROM bindings WHERE container=?1 AND environment=?2 AND salvage=0
             ORDER BY active DESC,created_at_ms DESC,id DESC LIMIT 1",
                params![self.scope.container.as_str(), self.scope.environment.code()],
                |row| row.get(0),
            )
            .optional()?;
        match id {
            Some(id) => self.binding(binding_id(id)?),
            None => Ok(None),
        }
    }

    /// Commit opaque fetched inbox facts and the exact SDK serialization
    /// together. There is no independent cursor-write API. This is a storage
    /// primitive, not an engine event handler: the future coordinator must
    /// enforce account/instance guards and stop after any prior commit failure.
    /// Exact repeated entries are idempotent. Classification, absence-pass
    /// attribution and version ordering belong to the native intake boundary.
    pub fn commit_fetched(
        &mut self,
        binding: BindingId,
        entries: &[InboxEntry],
        engine_state: &OpaqueBytes,
        clock: &dyn Clock,
    ) -> Result<(), Error> {
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let facts: Option<String> = transaction
            .query_row(
                "SELECT facts FROM bindings WHERE id=?1 AND container=?2 AND environment=?3",
                params![
                    binding.0.as_slice(),
                    self.scope.container.as_str(),
                    self.scope.environment.code()
                ],
                |row| row.get(0),
            )
            .optional()?;
        let facts: BindingFacts = decode(&facts.ok_or(Error::BindingNotFound)?)?;
        if facts.state != BindingState::Active {
            return Err(Error::IsolatedBinding);
        }
        for entry in entries {
            validate_entry(entry)?;
            let added = transaction.execute(
                "INSERT INTO inbox(binding,entry) VALUES(?1,?2) ON CONFLICT(binding,entry) DO NOTHING",
                params![binding.0.as_slice(), encode(entry)?],
            )?;
            if added != 0 {
                super::reference_waiting_store::identify(&transaction, binding, transaction.last_insert_rowid(), entry)?;
            }
        }
        transaction.execute(
            "INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES (?1,?2,?3)
             ON CONFLICT(binding) DO UPDATE SET serialization=excluded.serialization,saved_at_ms=excluded.saved_at_ms",
            params![binding.0.as_slice(), engine_state.0, clock.now_ms()],
        )?;
        // An error here unwinds both writes through SQLite's transaction drop.
        meguri_model::faultpoint!("cloud.fetched.before_commit")?;
        transaction.commit()?;
        Ok(())
    }

    pub fn engine_state(&self, binding: BindingId) -> Result<Option<EngineState>, Error> {
        self.require_binding(binding)?;
        self.connection
            .query_row(
                "SELECT serialization,saved_at_ms FROM engine_state WHERE binding=?1",
                [binding.0.as_slice()],
                |row| {
                    Ok(EngineState {
                        serialization: OpaqueBytes(row.get(0)?),
                        saved_at_ms: row.get(1)?,
                    })
                },
            )
            .optional()
            .map_err(Error::from)
    }

    pub fn inbox(&self, binding: BindingId) -> Result<Vec<StoredInboxEntry>, Error> {
        self.require_binding(binding)?;
        let mut statement = self
            .connection
            .prepare("SELECT id,entry FROM inbox WHERE binding=?1 ORDER BY id")?;
        let rows = statement.query_map([binding.0.as_slice()], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        })?;
        let mut entries = Vec::new();
        for row in rows {
            let (id, entry) = row?;
            entries.push(StoredInboxEntry {
                id,
                entry: decode(&entry)?,
            });
        }
        Ok(entries)
    }

    /// Storage-only K16 made-once insertion, scoped to an ordinary binding of
    /// this build. Digests are caller-classified inputs, not envelope checks.
    pub fn store_conflict(&mut self, binding: BindingId, copy: &ConflictCopy) -> Result<CopyId, Error> {
        let owner = self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if owner.facts.state == BindingState::Isolated(IsolationReason::TrackingDamaged) {
            return Err(Error::IsolatedBinding);
        }
        let transaction = self
            .connection
            .transaction_with_behavior(TransactionBehavior::Immediate)?;
        let mut statement = transaction.prepare("SELECT content FROM conflicts WHERE binding=?1")?;
        let mut rows = statement.query([binding.0.as_slice()])?;
        while let Some(row) = rows.next()? {
            let prior: ConflictCopy = decode(&row.get::<_, String>(0)?)?;
            if prior.kind == copy.kind
                && prior.key == copy.key
                && prior.reason == copy.reason
                && prior.losing_payload_digest == copy.losing_payload_digest
                && prior.winning_payload_digest == copy.winning_payload_digest
            {
                return Ok(prior.id);
            }
        }
        drop(rows);
        drop(statement);
        transaction.execute(
            "INSERT INTO conflicts(id,binding,content) VALUES (?1,?2,?3)",
            params![copy.id.0.as_slice(), binding.0.as_slice(), encode(copy)?],
        )?;
        transaction.commit()?;
        Ok(copy.id)
    }

    /// Local preservation listing across all builds, including resolved copies.
    /// This grants no ordinary writes to other builds or salvage owners.
    pub fn conflict_copies(&self) -> Result<Vec<StoredConflict>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT binding,content FROM conflicts ORDER BY id")?;
        let rows = statement.query_map([], |row| Ok((row.get::<_, Vec<u8>>(0)?, row.get::<_, String>(1)?)))?;
        rows.map(|row| {
            let (binding, content) = row?;
            Ok(StoredConflict {
                binding: binding_id(binding)?,
                copy: decode(&content)?,
            })
        })
        .collect()
    }

    pub fn salvage_bindings(&self) -> Result<Vec<SalvageBinding>, Error> {
        let mut statement = self
            .connection
            .prepare("SELECT metadata FROM salvage_bindings ORDER BY id")?;
        let rows = statement.query_map([], |row| row.get::<_, String>(0))?;
        rows.map(|row| decode(&row?)).collect()
    }

    pub(super) fn require_binding(&self, binding: BindingId) -> Result<(), Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound).map(|_| ())
    }
}

pub(super) fn binding_id(bytes: Vec<u8>) -> Result<BindingId, Error> {
    Ok(BindingId(bytes.try_into().map_err(|_| Error::InvalidData)?))
}

pub(super) fn encode(value: &impl serde::Serialize) -> Result<String, Error> {
    serde_json::to_string(value).map_err(|_| Error::InvalidData)
}

pub(super) fn decode<T: serde::de::DeserializeOwned>(text: &str) -> Result<T, Error> {
    serde_json::from_str(text).map_err(|_| Error::InvalidData)
}

pub(super) fn validate_entry(entry: &InboxEntry) -> Result<(), Error> {
    let valid = matches!(
        (&entry.record_type, &entry.value, &entry.reason),
        (
            RecordType::Entity,
            InboxValue::Deletion,
            InboxReason::Ready | InboxReason::ApplyFailed { .. }
        ) | (RecordType::Entity, InboxValue::Record { .. }, _)
            | (
                RecordType::Generation,
                InboxValue::Record { .. },
                InboxReason::Asset | InboxReason::Generation
            )
    );
    if valid { Ok(()) } else { Err(Error::InvalidData) }
}

pub(super) fn validate(connection: &Connection, version: i64) -> Result<(), Error> {
    let mut bindings =
        connection.prepare("SELECT id,container,environment,created_at_ms,facts,active,salvage FROM bindings")?;
    let mut rows = bindings.query([])?;
    while let Some(row) = rows.next()? {
        binding_id(row.get(0)?)?;
        ContainerId::new(row.get::<_, String>(1)?)?;
        if !matches!(row.get::<_, i64>(2)?, 0 | 1) {
            return Err(Error::InvalidData);
        }
        let _: i64 = row.get(3)?;
        let _: BindingFacts = decode(&row.get::<_, String>(4)?)?;
        let _: i64 = row.get(5)?;
        let _: i64 = row.get(6)?;
    }
    let mut inbox = connection.prepare("SELECT id,binding,entry FROM inbox")?;
    let mut rows = inbox.query([])?;
    while let Some(row) = rows.next()? {
        let _: i64 = row.get(0)?;
        binding_id(row.get(1)?)?;
        validate_entry(&decode::<InboxEntry>(&row.get::<_, String>(2)?)?)?;
    }
    let mut states = connection.prepare("SELECT binding,serialization,saved_at_ms FROM engine_state")?;
    let mut rows = states.query([])?;
    while let Some(row) = rows.next()? {
        binding_id(row.get(0)?)?;
        let _: Vec<u8> = row.get(1)?;
        let _: i64 = row.get(2)?;
    }
    if version >= 2 {
        let mut statement = connection.prepare("SELECT id,metadata FROM salvage_bindings")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id = binding_id(row.get(0)?)?;
            let metadata: SalvageBinding = decode(&row.get::<_, String>(1)?)?;
            if metadata.id != id || !super::recovery::is_source_name(&metadata.source) {
                return Err(Error::InvalidData);
            }
        }
        let mut statement = connection.prepare("SELECT id,binding,content FROM conflicts")?;
        let mut rows = statement.query([])?;
        while let Some(row) = rows.next()? {
            let id: Vec<u8> = row.get(0)?;
            let binding = binding_id(row.get(1)?)?;
            let copy: ConflictCopy = decode(&row.get::<_, String>(2)?)?;
            if id != copy.id.0 {
                return Err(Error::InvalidData);
            }
            let owners: i64 = connection.query_row(
                "SELECT (SELECT count(*) FROM bindings WHERE id=?1) + (SELECT count(*) FROM salvage_bindings WHERE id=?1)",
                [binding.0.as_slice()], |row| row.get(0))?;
            if owners != 1 {
                return Err(Error::InvalidData);
            }
        }
    }
    super::conflict_expiry::validate(connection)?;
    super::reference_waiting_store::validate(connection)?;
    super::incoming_store::validate(connection, version >= 3)?;
    super::adapter_store::validate(connection)?;
    super::acknowledgement_store::validate(connection)?;
    let mut foreign_keys = connection.prepare("PRAGMA foreign_key_check")?;
    if foreign_keys.query([])?.next()?.is_some() {
        return Err(Error::InvalidData);
    }
    Ok(())
}
