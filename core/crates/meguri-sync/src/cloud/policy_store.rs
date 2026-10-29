//! Additive P19 evidence, independent of the pre-existing runtime/adapter schema.
use super::{
    policy::*,
    store::{decode, encode},
    *,
};
use rusqlite::{Connection, OptionalExtension, params};

const SCHEMA: &str = "
CREATE TABLE cloud_policy_schema(version INTEGER NOT NULL CHECK(version=1));
INSERT INTO cloud_policy_schema VALUES(1);
CREATE TABLE cloud_policy_evidence(
 binding BLOB PRIMARY KEY REFERENCES bindings(id) ON DELETE CASCADE,
 content TEXT NOT NULL CHECK(json_valid(content)), probe_ms INTEGER
);
CREATE TABLE cloud_policy_exchanges(
 binding BLOB NOT NULL REFERENCES bindings(id) ON DELETE CASCADE,
 owner TEXT NOT NULL, exchange INTEGER NOT NULL CHECK(exchange>0),
 content TEXT NOT NULL CHECK(json_valid(content)), PRIMARY KEY(binding,owner,exchange)
);";

pub(super) fn check_version(c: &Connection) -> Result<bool, Error> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type='table' AND name='cloud_policy_schema')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        let orphan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_policy_evidence','cloud_policy_exchanges'))", [], |r| r.get(0))?;
        return if orphan { Err(Error::InvalidData) } else { Ok(false) };
    }
    let mut q = c.prepare("SELECT version FROM cloud_policy_schema")?;
    let versions = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions != [1] {
        return Err(Error::RuntimeSchemaUnsupported);
    }
    // Check both tables before accepting this schema version.
    c.prepare("SELECT binding,content,probe_ms FROM cloud_policy_evidence LIMIT 0")?;
    c.prepare("SELECT binding,owner,exchange,content FROM cloud_policy_exchanges LIMIT 0")?;
    Ok(true)
}
impl TrackingStore {
    pub(super) fn install_cloud_policy(&mut self) -> Result<(), Error> {
        if !check_version(&self.connection)? {
            let tx = self.connection.transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.commit()?;
        }
        let mut q = self.connection.prepare("SELECT e.content FROM cloud_policy_evidence e JOIN bindings b ON e.binding=b.id WHERE b.container=?1 AND b.environment=?2 AND b.salvage=0")?;
        for row in q.query_map(
            params![self.scope.container.as_str(), self.scope.environment.code()],
            |r| r.get::<_, String>(0),
        )? {
            let _: Evidence = decode(&row?)?;
        }
        Ok(())
    }
    pub fn cloud_policy_evidence(&self, binding: BindingId) -> Result<Evidence, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if !check_version(&self.connection)? {
            return Ok(Evidence::default());
        }
        let content: Option<String> = self
            .connection
            .query_row(
                "SELECT content FROM cloud_policy_evidence WHERE binding=?1",
                [binding.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        content.map(|c| decode(&c)).transpose().map(|c| c.unwrap_or_default())
    }
    pub(super) fn policy_exchange_recorded(
        &self,
        binding: BindingId,
        owner: &str,
        exchange: u64,
    ) -> Result<bool, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        Ok(self.connection.query_row(
            "SELECT EXISTS(SELECT 1 FROM cloud_policy_exchanges WHERE binding=?1 AND owner=?2 AND exchange=?3)",
            params![binding.0.as_slice(), owner, super::runtime_store::checked(exchange)?],
            |r| r.get(0),
        )?)
    }
    pub(super) fn record_cloud_policy_exchange(
        &mut self,
        binding: BindingId,
        owner: &str,
        record: &ExchangeRecord,
    ) -> Result<Evidence, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        if owner.is_empty() || record.exchange == 0 {
            return Err(Error::InvalidData);
        }
        if self.policy_exchange_recorded(binding, owner, record.exchange)? {
            return self.cloud_policy_evidence(binding);
        }
        let mut evidence = self.cloud_policy_evidence(binding)?;
        if matches!(record.outcome, ExchangeOutcome::Succeeded { .. }) {
            evidence.last_completed = Some(record.clone());
        } else {
            evidence.last_attempt = Some(record.clone());
        }
        let tx = self.connection.transaction()?;
        tx.execute(
            "INSERT INTO cloud_policy_exchanges(binding,owner,exchange,content) VALUES(?1,?2,?3,?4)",
            params![
                binding.0.as_slice(),
                owner,
                super::runtime_store::checked(record.exchange)?,
                encode(record)?
            ],
        )?;
        tx.execute("INSERT INTO cloud_policy_evidence(binding,content) VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(), encode(&evidence)?])?;
        tx.commit()?;
        Ok(evidence)
    }
    pub(super) fn record_cloud_quiet(&mut self, binding: BindingId, at_ms: i64) -> Result<Evidence, Error> {
        let mut evidence = self.cloud_policy_evidence(binding)?;
        if evidence.quiet_ms == Some(at_ms) {
            return Ok(evidence);
        }
        evidence.quiet_ms = Some(at_ms);
        let tx = self.connection.transaction()?;
        tx.execute("INSERT INTO cloud_policy_evidence(binding,content) VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET content=excluded.content", params![binding.0.as_slice(), encode(&evidence)?])?;
        tx.commit()?;
        Ok(evidence)
    }
    pub(super) fn record_cloud_probe(&mut self, binding: BindingId, at_ms: i64) -> Result<(), Error> {
        let evidence = self.cloud_policy_evidence(binding)?;
        let tx = self.connection.transaction()?;
        tx.execute("INSERT INTO cloud_policy_evidence(binding,content,probe_ms) VALUES(?1,?2,?3) ON CONFLICT(binding) DO UPDATE SET probe_ms=excluded.probe_ms", params![binding.0.as_slice(), encode(&evidence)?, at_ms])?;
        tx.commit()?;
        Ok(())
    }
    /// Dirty rows before sendability filters, so blocked work cannot disappear from status.
    pub(super) fn policy_pending_names(&self, binding: BindingId) -> Result<std::collections::BTreeSet<String>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        let mut q = self.connection.prepare("SELECT json_extract(content,'$.name') FROM cloud_runtime_notes WHERE binding=?1 AND json_extract(content,'$.mark') IS NOT NULL UNION SELECT json_extract(content,'$.name') FROM cloud_runtime_entities WHERE binding=?1 AND json_extract(content,'$.mark') IS NOT NULL")?;
        Ok(q.query_map([binding.0.as_slice()], |r| r.get::<_, String>(0))?
            .collect::<Result<_, _>>()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        rng::SeededRandom,
    };
    fn scope() -> BuildScope {
        BuildScope {
            container: ContainerId::new("synthetic-policy-schema").unwrap(),
            environment: Environment::Development,
        }
    }
    #[test]
    fn additive_extension_preserves_existing_runtime_rows_and_refuses_future_marker() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("new store"),
        };
        store.install_runtime().unwrap();
        let clock = FixedClock::new(1000, UtcOffset::UTC);
        let random = SeededRandom::new(1914);
        let binding = store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![1]),
                    epoch: EpochId([2; 16]),
                    record_key: RecordKey([3; 32]),
                    publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                    state: BindingState::Active,
                },
                &clock,
                &random,
            )
            .unwrap();
        let before = store
            .open_runtime_binding(
                binding.id,
                crate::cloud::runtime::NativeFileIdentity {
                    device: 1,
                    inode: 2,
                    created_seconds: 1,
                    created_nanos: 0,
                },
            )
            .unwrap();
        // This is the exact valid pre-policy extension layout, with existing rows.
        store
            .connection
            .execute_batch(
                "DROP TABLE cloud_policy_exchanges; DROP TABLE cloud_policy_evidence; DROP TABLE cloud_policy_schema;",
            )
            .unwrap();
        assert!(!check_version(&store.connection).unwrap());
        store.install_runtime().unwrap();
        let after = store.runtime_binding(binding.id).unwrap();
        assert_eq!(before.open_counter, after.open_counter);
        assert_eq!(before.file, after.file);
        assert_eq!(store.cloud_policy_evidence(binding.id).unwrap(), Evidence::default());
        store
            .connection
            .execute_batch("PRAGMA ignore_check_constraints=ON; UPDATE cloud_policy_schema SET version=2;")
            .unwrap();
        assert_eq!(store.install_runtime(), Err(Error::RuntimeSchemaUnsupported));
        assert_eq!(
            store.runtime_binding(binding.id).unwrap().open_counter,
            before.open_counter
        );
    }
    #[test]
    fn failed_evidence_commit_keeps_previous_qualified_timestamp_and_receipt() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = match TrackingStore::open(directory.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("new store"),
        };
        store.install_runtime().unwrap();
        let binding = store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![1]),
                    epoch: EpochId([2; 16]),
                    record_key: RecordKey([3; 32]),
                    publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                    state: BindingState::Active,
                },
                &FixedClock::new(1000, UtcOffset::UTC),
                &SeededRandom::new(24),
            )
            .unwrap();
        store.record_cloud_quiet(binding.id, 1200).unwrap();
        let first = ExchangeRecord {
            exchange: 1,
            ended_ms: 1300,
            source: Source::Key,
            outcome: ExchangeOutcome::Cancelled,
        };
        store
            .record_cloud_policy_exchange(binding.id, "opening-1", &first)
            .unwrap();
        let mut duplicate = first.clone();
        duplicate.ended_ms = 9999;
        assert_eq!(
            store
                .record_cloud_policy_exchange(binding.id, "opening-1", &duplicate)
                .unwrap()
                .last_attempt,
            Some(first.clone())
        );
        let before = store.cloud_policy_evidence(binding.id).unwrap();
        store
            .connection
            .execute_batch(
                "CREATE TEMP TRIGGER reject_policy_update BEFORE UPDATE ON cloud_policy_evidence
             BEGIN SELECT RAISE(ABORT,'policy evidence constraint'); END;",
            )
            .unwrap();
        let second = ExchangeRecord {
            exchange: 2,
            ended_ms: 1500,
            ..first
        };
        let failed = store.record_cloud_policy_exchange(binding.id, "opening-1", &second);
        assert!(failed.is_err());
        assert_eq!(store.cloud_policy_evidence(binding.id).unwrap(), before);
        assert!(!store.policy_exchange_recorded(binding.id, "opening-1", 2).unwrap());
    }
}
