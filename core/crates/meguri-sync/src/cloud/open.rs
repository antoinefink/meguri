use std::{
    fs::{File, OpenOptions},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
};

use rusqlite::{Connection, OpenFlags};

use super::{BuildScope, Error, TrackingStore, store};

pub const TRACKING_FILE: &str = "meguri-cloud.db";
pub(super) const SCHEMA_VERSION: i64 = 3;

/// Plain open refuses damage; explicit recovery must finish before use.
/// Missing creates an empty store, without inferring any server facts.
pub enum OpenOutcome {
    Missing(TrackingStore),
    Opened(TrackingStore),
    Damaged,
    NewerSchema { found: i64 },
}

pub(super) enum Probe {
    Supported,
    Damaged,
    Newer(i64),
}

pub(super) fn open(directory: &Path, scope: BuildScope) -> Result<OpenOutcome, Error> {
    super::recovery::require_settled(directory)?;
    open_current(directory, scope)
}

pub(super) fn open_current(directory: &Path, scope: BuildScope) -> Result<OpenOutcome, Error> {
    let path = directory.join(TRACKING_FILE);
    let exists = match std::fs::metadata(&path) {
        Ok(metadata) if metadata.is_file() => true,
        Ok(_) => return Ok(OpenOutcome::Damaged),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => return Err(error.into()),
    };
    if exists {
        match probe(&path)? {
            Probe::Damaged => return Ok(OpenOutcome::Damaged),
            Probe::Newer(found) => return Ok(OpenOutcome::NewerSchema { found }),
            Probe::Supported => {}
        }
    }
    // Only the private copy has been opened by SQLite until the version and
    // required rows have been checked. Opening even read-only at the original
    // path could change its sidecars or attempt WAL recovery.
    let flags = if exists {
        OpenFlags::SQLITE_OPEN_READ_WRITE
    } else {
        OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_CREATE
    };
    let mut connection = Connection::open_with_flags(&path, flags)?;
    // Server facts must survive the device cache before dependent work (K10.1, V3).
    #[cfg(target_vendor = "apple")]
    connection.execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
    connection.execute_batch("PRAGMA foreign_keys=ON; PRAGMA journal_mode=WAL; PRAGMA synchronous=FULL;")?;
    if !exists {
        let transaction = connection.transaction()?;
        transaction.execute_batch(store::V1_SCHEMA)?;
        transaction.execute_batch(store::V2_SCHEMA)?;
        super::incoming_store::install(&transaction)?;
        transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
        store::validate(&transaction, SCHEMA_VERSION)?;
        transaction.commit()?;
    }
    if exists {
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version < SCHEMA_VERSION {
            let transaction = connection.transaction()?;
            if version == 1 {
                transaction.execute_batch(store::V2_SCHEMA)?;
            }
            super::incoming_store::install(&transaction)?;
            transaction.pragma_update(None, "user_version", SCHEMA_VERSION)?;
            store::validate(&transaction, SCHEMA_VERSION)?;
            transaction.commit()?;
        }
    }
    let store = TrackingStore {
        connection,
        scope,
        directory: directory.to_owned(),
    };
    Ok(if exists {
        OpenOutcome::Opened(store)
    } else {
        OpenOutcome::Missing(store)
    })
}

/// Keep the suspended adoption's file open to prevent inode reuse.
pub(super) struct RetainedAdoptionStore {
    file: File,
    directory: super::recovery::NativeIdentity,
    identity: super::recovery::NativeIdentity,
    op: super::adoption::AdoptOp,
    before: Option<super::Binding>,
    after: Option<super::Binding>,
    authority: Option<(String, Option<String>)>,
}

impl TrackingStore {
    pub(super) fn retain_adoption_store(&self, op: &super::adoption::AdoptOp) -> Result<RetainedAdoptionStore, Error> {
        let directory = super::recovery::RecoveryDirectory::open(&self.directory)?;
        let identity = directory.entry_identity(TRACKING_FILE)?.ok_or(Error::RecoveryRefused)?;
        let file = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.directory.join(TRACKING_FILE))?;
        let pinned = file.metadata()?;
        let named = std::fs::symlink_metadata(self.directory.join(TRACKING_FILE))?;
        if !pinned.is_file()
            || (pinned.dev(), pinned.ino()) != (named.dev(), named.ino())
            || directory.entry_identity(TRACKING_FILE)? != Some(identity)
        {
            return Err(Error::RecoveryRefused);
        }
        let retained = RetainedAdoptionStore {
            file,
            directory: directory.native_identity(),
            identity,
            op: op.clone(),
            before: op
                .binding_before
                .as_ref()
                .map(|b| self.binding(b.id))
                .transpose()?
                .flatten(),
            after: self.binding(op.binding_after)?,
            authority: self.adoption_cleanup_authority(op)?,
        };
        if op.binding_before.is_some() != retained.before.is_some()
            || (op.step == super::adoption::AdoptStep::Ready
                && (retained.after.is_none() || retained.authority.is_none()))
        {
            return Err(Error::RecoveryRefused);
        }
        retained.validate(self)?;
        Ok(retained)
    }

    fn adoption_cleanup_authority(
        &self,
        op: &super::adoption::AdoptOp,
    ) -> Result<Option<(String, Option<String>)>, Error> {
        use rusqlite::OptionalExtension;
        self.connection
            .query_row(
                "SELECT controls,activated_file FROM cloud_adoption_authority WHERE binding=?1",
                [op.binding_after.0.as_slice()],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()
            .map_err(Error::from)
    }

    /// No CREATE, schema installation, migration, recovery or collection access.
    /// All refusal checks run on a private WAL-aware copy before SQLite ever
    /// opens the retained source (even read-only could change its sidecars).
    pub(super) fn open_retained_adoption(
        directory: &Path,
        scope: BuildScope,
        retained: &RetainedAdoptionStore,
    ) -> Result<Self, Error> {
        super::recovery::require_settled(directory)?;
        retained.check_file(directory)?;
        let (_private, copy) = private_copy(&directory.join(TRACKING_FILE))?;
        let connection = Connection::open_with_flags(copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let probe_store = Self {
            connection,
            scope: scope.clone(),
            directory: directory.to_owned(),
        };
        retained.validate(&probe_store)?;
        drop(probe_store);
        retained.check_file(directory)?;
        let connection = Connection::open_with_flags(directory.join(TRACKING_FILE), OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let store = Self {
            connection,
            scope,
            directory: directory.to_owned(),
        };
        // Caller holds with_tracking_access across probe, open and cleanup.
        retained.validate(&store)?;
        #[cfg(target_vendor = "apple")]
        store
            .connection
            .execute_batch("PRAGMA fullfsync=ON; PRAGMA checkpoint_fullfsync=ON;")?;
        store
            .connection
            .execute_batch("PRAGMA foreign_keys=ON; PRAGMA synchronous=FULL;")?;
        Ok(store)
    }
}

impl RetainedAdoptionStore {
    fn check_file(&self, directory: &Path) -> Result<(), Error> {
        let directory_guard = super::recovery::RecoveryDirectory::open(directory)?;
        let named = std::fs::symlink_metadata(directory.join(TRACKING_FILE))?;
        let pinned = self.file.metadata()?;
        if !named.is_file()
            || (named.dev(), named.ino()) != (pinned.dev(), pinned.ino())
            || directory_guard.native_identity() != self.directory
            || directory_guard.entry_identity(TRACKING_FILE)? != Some(self.identity)
        {
            return Err(Error::RecoveryRefused);
        }
        Ok(())
    }

    fn validate(&self, store: &TrackingStore) -> Result<(), Error> {
        let outer: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |r| r.get(0))?;
        if outer != SCHEMA_VERSION {
            return Err(Error::RecoveryRefused);
        }
        for (table, expected) in [
            ("cloud_runtime_schema", super::runtime_store::VERSION),
            ("cloud_workflow_schema", 2),
            ("cloud_publication_schema", 4),
        ] {
            let versions = store
                .connection
                .prepare(&format!("SELECT version FROM {table}"))?
                .query_map([], |r| r.get::<_, i64>(0))?
                .collect::<Result<Vec<_>, _>>()?;
            if versions != [expected] {
                return Err(Error::RecoveryRefused);
            }
        }
        super::runtime_store::check_version(&store.connection)?;
        store::validate(&store.connection, outer)?;
        if store.adoption_op()?.as_ref() != Some(&self.op)
            || self
                .op
                .binding_before
                .as_ref()
                .map(|b| store.binding(b.id))
                .transpose()?
                .flatten()
                != self.before
            || store.binding(self.op.binding_after)? != self.after
            || store.adoption_cleanup_authority(&self.op)? != self.authority
        {
            return Err(Error::RecoveryRefused);
        }
        Ok(())
    }
}

pub(super) fn sidecar(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn private_copy(path: &Path) -> Result<(tempfile::TempDir, PathBuf), Error> {
    let private = tempfile::tempdir()?;
    let copy = private.path().join(TRACKING_FILE);
    std::fs::copy(path, &copy)?;
    // fs::copy retains source modes. SQLite may recover/checkpoint this private
    // copy even when the original home is read-only; source permissions stay
    // untouched and version refusal must not depend on its write bits.
    std::fs::set_permissions(&copy, std::fs::Permissions::from_mode(0o600))?;
    match std::fs::copy(sidecar(path, "-wal"), sidecar(&copy, "-wal")) {
        Ok(_) => std::fs::set_permissions(sidecar(&copy, "-wal"), std::fs::Permissions::from_mode(0o600))?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(error) => return Err(error.into()),
    }
    // Do not copy SHM: SQLite reconstructs the private WAL index. It never
    // shares the original's locks, nor recovers/checkpoints the original WAL.
    Ok((private, copy))
}

pub(super) fn probe(path: &Path) -> Result<Probe, Error> {
    let (_private, copy) = private_copy(path)?;
    let checked = (|| -> Result<Probe, Error> {
        let connection = Connection::open_with_flags(&copy, OpenFlags::SQLITE_OPEN_READ_WRITE)?;
        let version: i64 = connection.pragma_query_value(None, "user_version", |row| row.get(0))?;
        if version > SCHEMA_VERSION {
            return Ok(Probe::Newer(version));
        }
        if !(1..=SCHEMA_VERSION).contains(&version) {
            return Ok(Probe::Damaged);
        }
        super::runtime_store::check_version(&connection)?;
        let integrity: String = connection.query_row("PRAGMA integrity_check", [], |row| row.get(0))?;
        if integrity != "ok" {
            return Ok(Probe::Damaged);
        }
        store::validate(&connection, version)?;
        Ok(Probe::Supported)
    })();
    match checked {
        Ok(result) => Ok(result),
        Err(Error::RuntimeSchemaUnsupported) => Err(Error::RuntimeSchemaUnsupported),
        Err(_) => Ok(Probe::Damaged),
    }
}

#[cfg(test)]
mod tests {
    use super::{OpenOutcome, TRACKING_FILE};
    use crate::cloud::{store, *};
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        rng::SeededRandom,
    };
    use rusqlite::{Connection, params};
    use std::path::Path;

    fn scope() -> BuildScope {
        BuildScope {
            container: ContainerId::new("test.container").unwrap(),
            environment: Environment::Development,
        }
    }

    fn assert_durable_settings(store: &TrackingStore) {
        // These settings belong to this connection; a separate SQL connection
        // would not establish the durability of the production store.
        let connection = &store.connection;
        let journal_mode: String = connection
            .pragma_query_value(None, "journal_mode", |row| row.get(0))
            .unwrap();
        assert_eq!(journal_mode, "wal");
        let synchronous: i64 = connection
            .pragma_query_value(None, "synchronous", |row| row.get(0))
            .unwrap();
        assert_eq!(synchronous, 2, "tracking commits require synchronous=FULL");
        #[cfg(target_vendor = "apple")]
        for pragma in ["fullfsync", "checkpoint_fullfsync"] {
            let enabled: i64 = connection.pragma_query_value(None, pragma, |row| row.get(0)).unwrap();
            assert_eq!(enabled, 1, "{pragma} must flush the device cache");
        }
    }

    struct LegacyFacts {
        binding: Binding,
        inbox: Vec<StoredInboxEntry>,
        state: Option<EngineState>,
        conflicts: Vec<StoredConflict>,
    }

    fn historical_store(directory: &Path, version: i64) -> (TrackingStore, LegacyFacts) {
        assert!(matches!(version, 1 | 2));
        let mut connection = Connection::open(directory.join(TRACKING_FILE)).unwrap();
        let transaction = connection.transaction().unwrap();
        transaction.execute_batch(store::V1_SCHEMA).unwrap();
        if version == 2 {
            transaction.execute_batch(store::V2_SCHEMA).unwrap();
        }
        transaction.pragma_update(None, "user_version", version).unwrap();
        transaction.commit().unwrap();
        // Construct the historical store directly: opening through today's
        // entry point would install the very schema this fixture must lack.
        let mut store = TrackingStore {
            connection,
            scope: scope(),
            directory: directory.to_owned(),
        };
        let clock = FixedClock::new(88, UtcOffset::UTC);
        let binding = store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![0, 255, 6]),
                    epoch: EpochId([13; 16]),
                    record_key: RecordKey([27; 32]),
                    publication: serde_json::from_str("[17,29]").unwrap(),
                    state: BindingState::Active,
                },
                &clock,
                &SeededRandom::new(9),
            )
            .unwrap();
        let entry = InboxEntry {
            record_type: RecordType::Entity,
            record_name: "historical-inbox".into(),
            value: InboxValue::Record {
                erev: 4,
                system_fields: OpaqueBytes(vec![0, 255]),
                envelope: OpaqueBytes(b"historical-opaque-envelope".to_vec()),
                staged_asset: Some("cloud-staging/historical".into()),
            },
            reason: InboxReason::Ready,
            source: InboxSource::Fetch,
            absence_pass: Some([7; 16]),
            retry_count: 2,
            fetched_at_ms: 77,
        };
        // commit_fetched now reads incoming tables absent in V1/V2. These
        // inserts are historical storage-fixture facts, not incoming/ACK proof.
        let transaction = store.connection.transaction().unwrap();
        transaction
            .execute(
                "INSERT INTO inbox(binding,entry) VALUES(?1,?2)",
                params![binding.id.0.as_slice(), serde_json::to_string(&entry).unwrap()],
            )
            .unwrap();
        transaction
            .execute(
                "INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES(?1,?2,?3)",
                params![binding.id.0.as_slice(), [0_u8, 255, 6, 7].as_slice(), 88],
            )
            .unwrap();
        transaction.commit().unwrap();
        if version == 2 {
            store
                .store_conflict(
                    binding.id,
                    &ConflictCopy {
                        id: CopyId([3; 16]),
                        kind: ConflictKind::Note,
                        key: OpaqueBytes(vec![3]),
                        reason: ConflictReason::FieldConflict,
                        losing_envelope: OpaqueBytes(vec![0, 255, 3]),
                        losing_payload_digest: PayloadDigest([3; 32]),
                        winning_payload_digest: PayloadDigest([99; 32]),
                        field_names: Some(vec!["Question".into(), "Answer".into()]),
                        template_names: Some(vec!["Reverse".into()]),
                        note_deck: Some(77),
                        staged_path: Some("cloud-staging/conflict".into()),
                        created_at_ms: 44,
                        resolved_at_ms: Some(55),
                    },
                )
                .unwrap();
        }
        let facts = LegacyFacts {
            inbox: store.inbox(binding.id).unwrap(),
            state: store.engine_state(binding.id).unwrap(),
            conflicts: if version == 2 {
                store.conflict_copies().unwrap()
            } else {
                Vec::new()
            },
            binding: store.binding(binding.id).unwrap().unwrap(),
        };
        assert_eq!(facts.inbox.len(), 1);
        assert!(facts.state.is_some());
        (store, facts)
    }

    fn assert_legacy_facts(store: &TrackingStore, facts: &LegacyFacts) {
        assert_eq!(store.binding(facts.binding.id).unwrap().as_ref(), Some(&facts.binding));
        assert_eq!(store.selected_binding().unwrap().as_ref(), Some(&facts.binding));
        assert_eq!(store.inbox(facts.binding.id).unwrap(), facts.inbox);
        assert_eq!(store.engine_state(facts.binding.id).unwrap(), facts.state);
        assert_eq!(store.conflict_copies().unwrap(), facts.conflicts);
    }

    fn assert_v3_schema(store: &TrackingStore) {
        let version: i64 = store
            .connection
            .pragma_query_value(None, "user_version", |row| row.get(0))
            .unwrap();
        assert_eq!(version, 3);
        for table in ["cloud_apply_intents", "cloud_refused"] {
            let count: i64 = store
                .connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| row.get(0))
                .unwrap();
            assert_eq!(count, 0);
        }
    }

    fn assert_historical_migration(version: i64) {
        let directory = tempfile::tempdir().unwrap();
        let (store, facts) = historical_store(directory.path(), version);
        let incoming_tables: i64 = store
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master WHERE name IN ('cloud_apply_intents','cloud_refused')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(incoming_tables, 0);
        drop(store);
        for _ in 0..2 {
            let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
                OpenOutcome::Opened(store) => store,
                _ => panic!("expected historical tracking migration"),
            };
            assert_legacy_facts(&store, &facts);
            assert_v3_schema(&store);
            assert!(store.apply_intents(facts.binding.id).unwrap().is_empty());
            assert!(store.refused_entries(facts.binding.id).unwrap().is_empty());
        }
    }

    #[test]
    fn v1_migration_preserves_original_binding_inbox_and_engine_serialization() {
        assert_historical_migration(1);
    }

    #[test]
    fn v2_migration_preserves_original_binding_inbox_state_and_conflicts() {
        assert_historical_migration(2);
    }

    fn schema_objects(connection: &Connection) -> Vec<(String, String, Option<String>)> {
        connection
            .prepare("SELECT type,name,sql FROM sqlite_master ORDER BY type,name")
            .unwrap()
            .query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    fn directory_bytes(directory: &Path) -> Vec<(std::ffi::OsString, Vec<u8>)> {
        let mut files: Vec<_> = std::fs::read_dir(directory)
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (entry.file_name(), std::fs::read(entry.path()).unwrap())
            })
            .collect();
        files.sort_by(|left, right| left.0.cmp(&right.0));
        files
    }

    #[test]
    fn incoming_migration_failure_leaves_no_partial_schema_version_or_lost_legacy_facts() {
        let directory = tempfile::tempdir().unwrap();
        let (store, facts) = historical_store(directory.path(), 2);
        // SQLite's IF NOT EXISTS suppresses table/view collisions, but an
        // index with the requested table name still fails CREATE TABLE. This
        // targets the second incoming table, after the first DDL can succeed.
        store
            .connection
            .execute_batch("CREATE INDEX cloud_refused ON inbox(binding)")
            .unwrap();
        let schema_before = schema_objects(&store.connection);
        drop(store);
        for _ in 0..2 {
            let bytes_before = directory_bytes(directory.path());
            // A stronger private probe may reject this malformed old schema
            // before migration; that is valid too, provided it stays untouched.
            match TrackingStore::open(directory.path(), scope()) {
                Ok(OpenOutcome::Damaged) => assert_eq!(directory_bytes(directory.path()), bytes_before),
                Err(_) => {}
                _ => panic!("schema collision must not yield a usable tracking store"),
            }
            let store = TrackingStore {
                connection: Connection::open(directory.path().join(TRACKING_FILE)).unwrap(),
                scope: scope(),
                directory: directory.path().to_owned(),
            };
            assert_legacy_facts(&store, &facts);
            let version: i64 = store
                .connection
                .pragma_query_value(None, "user_version", |row| row.get(0))
                .unwrap();
            assert_eq!(version, 2);
            assert_eq!(schema_objects(&store.connection), schema_before);
        }
    }

    #[test]
    fn newly_created_tracking_store_versions_required_incoming_tables() {
        let directory = tempfile::tempdir().unwrap();
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("expected missing tracking file"),
        };
        assert_v3_schema(&store);
    }

    #[test]
    fn newly_created_tracking_connection_uses_durable_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("expected missing tracking file"),
        };
        assert_durable_settings(&store);
    }

    #[test]
    fn reopened_tracking_connection_uses_durable_settings() {
        let directory = tempfile::tempdir().unwrap();
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("expected missing tracking file"),
        };
        drop(store);
        let store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Opened(store) => store,
            _ => panic!("expected supported tracking file"),
        };
        assert_durable_settings(&store);
    }
}
