//! Durable acknowledgement order. New metadata never invents an order for
//! legacy ACKs: unchanged headers retain no per-entity counter until a new ACK.
use super::{entity_codec::Kind, store::decode, workflow::EntityFacts, *};
use rusqlite::{Connection, OptionalExtension, Transaction, params};
use serde::Deserialize;

const SCHEMA: &str = "
CREATE TABLE cloud_ack_schema(version INTEGER NOT NULL CHECK(version=1));
INSERT INTO cloud_ack_schema VALUES(1);
CREATE TABLE cloud_ack_bindings(
 binding BLOB PRIMARY KEY REFERENCES bindings(id) ON DELETE CASCADE,
 counter INTEGER NOT NULL CHECK(typeof(counter)='integer' AND counter>=0)
);
CREATE TABLE cloud_ack_entities(
 binding BLOB NOT NULL REFERENCES cloud_ack_bindings(binding) ON DELETE CASCADE,
 name TEXT NOT NULL CHECK(length(name)>0),
 counter INTEGER NOT NULL CHECK(typeof(counter)='integer' AND counter>0),
 PRIMARY KEY(binding,name)
);";

fn present(c: &Connection) -> Result<bool, Error> {
    let exists: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_ack_schema' AND type='table')",
        [],
        |r| r.get(0),
    )?;
    if !exists {
        let orphan: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name IN ('cloud_ack_schema','cloud_ack_bindings','cloud_ack_entities'))", [], |r| r.get(0))?;
        return if orphan { Err(Error::InvalidData) } else { Ok(false) };
    }
    let mut q = c.prepare("SELECT version FROM cloud_ack_schema")?;
    let versions = q
        .query_map([], |r| r.get::<_, i64>(0))?
        .collect::<Result<Vec<_>, _>>()?;
    if versions != [1] {
        return Err(Error::RuntimeSchemaUnsupported);
    }
    c.prepare("SELECT binding,counter FROM cloud_ack_bindings LIMIT 0")?;
    c.prepare("SELECT binding,name,counter FROM cloud_ack_entities LIMIT 0")?;
    Ok(true)
}
pub(super) fn validate(c: &Connection) -> Result<(), Error> {
    if !present(c)? {
        return Ok(());
    }
    let bad: bool = c.query_row(
        "SELECT EXISTS(SELECT 1 FROM cloud_ack_bindings a LEFT JOIN bindings b ON b.id=a.binding
        WHERE b.id IS NULL OR typeof(a.counter)!='integer' OR a.counter<0)",
        [],
        |r| r.get(0),
    )?;
    let bad_entity: bool = c.query_row("SELECT EXISTS(SELECT 1 FROM cloud_ack_entities e LEFT JOIN cloud_ack_bindings b ON b.binding=e.binding
        WHERE b.binding IS NULL OR typeof(e.name)!='text' OR length(e.name)=0 OR typeof(e.counter)!='integer' OR e.counter<=0 OR e.counter>b.counter)", [], |r| r.get(0))?;
    if bad || bad_entity {
        return Err(Error::InvalidData);
    }
    Ok(())
}
pub(super) fn install(c: &Connection) -> Result<(), Error> {
    validate(c)?;
    if !present(c)? {
        if c.is_autocommit() {
            let tx = c.unchecked_transaction()?;
            tx.execute_batch(SCHEMA)?;
            tx.commit()?;
        } else {
            c.execute_batch(SCHEMA)?;
        }
    }
    Ok(())
}

// The caller's ACK write and both counters share this transaction. A duplicate
// accepted version is filtered by observe before reaching this primitive.
fn record(tx: &Transaction<'_>, binding: BindingId, name: &str) -> Result<(), Error> {
    if name.is_empty() {
        return Err(Error::InvalidData);
    }
    let prior: Option<i64> = tx
        .query_row(
            "SELECT counter FROM cloud_ack_bindings WHERE binding=?1",
            [binding.0.as_slice()],
            |r| r.get(0),
        )
        .optional()?;
    let current = prior.unwrap_or(0);
    if current < 0 {
        return Err(Error::InvalidData);
    }
    let next = current.checked_add(1).ok_or(Error::InvalidData)?;
    tx.execute("INSERT INTO cloud_ack_bindings(binding,counter) VALUES(?1,?2) ON CONFLICT(binding) DO UPDATE SET counter=excluded.counter",
               params![binding.0.as_slice(), next])?;
    tx.execute("INSERT INTO cloud_ack_entities(binding,name,counter) VALUES(?1,?2,?3) ON CONFLICT(binding,name) DO UPDATE SET counter=excluded.counter",
               params![binding.0.as_slice(), name, next])?;
    Ok(())
}

#[derive(Deserialize, PartialEq, Eq)]
struct Version {
    erev: u64,
    payload_digest: [u8; 32],
    row_digest: [u8; 32],
}
#[derive(Deserialize)]
struct PriorNote {
    name: String,
    acknowledged: Version,
}
#[derive(Deserialize)]
struct PriorEntity {
    name: String,
    server_erev: u64,
    acked_payload_digest: [u8; 32],
    acked_row_digest: [u8; 32],
}

pub(super) fn observe(tx: &Transaction<'_>, b: BindingId, e: &EntityFacts, old: Option<&str>) -> Result<(), Error> {
    if e.server_erev == 0 || e.acked_system_fields.is_empty() || e.acked_envelope.0.is_empty() {
        return Ok(());
    }
    let version = Version {
        erev: e.server_erev,
        payload_digest: e.acked_payload_digest,
        row_digest: e.acked_row_digest,
    };
    let prior = old
        .map(|text| -> Result<_, Error> {
            if e.kind == Kind::Note {
                let old: PriorNote = decode(text)?;
                Ok((old.name, old.acknowledged))
            } else {
                let old: PriorEntity = decode(text)?;
                Ok((
                    old.name,
                    Version {
                        erev: old.server_erev,
                        payload_digest: old.acked_payload_digest,
                        row_digest: old.acked_row_digest,
                    },
                ))
            }
        })
        .transpose()?;
    if prior
        .as_ref()
        .is_some_and(|(name, old)| name == &e.name && old == &version)
    {
        return Ok(());
    }
    record(tx, b, &e.name)
}
pub(super) fn forget(tx: &Transaction<'_>, b: BindingId, name: &str) -> Result<(), Error> {
    if present(tx)? {
        tx.execute(
            "DELETE FROM cloud_ack_entities WHERE binding=?1 AND name=?2",
            params![b.0.as_slice(), name],
        )?;
    }
    Ok(())
}

impl TrackingStore {
    pub fn binding_acknowledgement_sequence(&self, b: BindingId) -> Result<u64, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if !present(&self.connection)? {
            return Ok(0);
        }
        let value: Option<i64> = self
            .connection
            .query_row(
                "SELECT counter FROM cloud_ack_bindings WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .optional()?;
        value.unwrap_or(0).try_into().map_err(|_| Error::InvalidData)
    }
    pub fn entity_acknowledgement_sequence(&self, b: BindingId, name: &str) -> Result<Option<u64>, Error> {
        self.binding(b)?.ok_or(Error::BindingNotFound)?;
        if !present(&self.connection)? {
            return Ok(None);
        }
        let value: Option<(i64, i64)> = self
            .connection
            .query_row(
                "SELECT e.counter,b.counter FROM cloud_ack_entities e
            JOIN cloud_ack_bindings b ON b.binding=e.binding WHERE e.binding=?1 AND e.name=?2",
                params![b.0.as_slice(), name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .optional()?;
        value
            .map(|(value, total)| {
                if value > 0 && value <= total {
                    u64::try_from(value).map_err(|_| Error::InvalidData)
                } else {
                    Err(Error::InvalidData)
                }
            })
            .transpose()
    }
}

#[cfg(test)]
mod tests {
    use crate::cloud::*;
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        rng::SeededRandom,
    };
    use rusqlite::{Connection, OptionalExtension, params};

    fn scope() -> BuildScope {
        BuildScope {
            container: ContainerId::new("iCloud.example.ack-sequence").unwrap(),
            environment: Environment::Development,
        }
    }
    fn open(root: &std::path::Path) -> TrackingStore {
        let mut store = match TrackingStore::open(root, scope()).unwrap() {
            OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
            _ => panic!("usable tracking"),
        };
        store.install_runtime().unwrap();
        store
    }
    fn binding(store: &mut TrackingStore) -> BindingId {
        store
            .create_binding(
                BindingFacts {
                    account: OpaqueBytes(vec![1]),
                    epoch: EpochId([2; 16]),
                    record_key: RecordKey([3; 32]),
                    publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                    state: BindingState::Active,
                },
                &FixedClock::new(1000, UtcOffset::UTC),
                &SeededRandom::new(793),
            )
            .unwrap()
            .id
    }
    fn counter(c: &Connection, binding: BindingId) -> u64 {
        let value: Option<i64> = c
            .query_row(
                "SELECT counter FROM cloud_ack_bindings WHERE binding=?1",
                [binding.0.as_slice()],
                |r| r.get(0),
            )
            .optional()
            .unwrap();
        value.unwrap_or(0).try_into().unwrap()
    }
    fn last(c: &Connection, binding: BindingId, name: &str) -> Option<u64> {
        c.query_row(
            "SELECT counter FROM cloud_ack_entities WHERE binding=?1 AND name=?2",
            params![binding.0.as_slice(), name],
            |r| r.get::<_, i64>(0),
        )
        .optional()
        .unwrap()
        .map(|value| value.try_into().unwrap())
    }
    #[test]
    fn accepted_ack_sequence_rolls_back_and_survives_reopen_with_its_tracking_transaction() {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store);
        assert_eq!(counter(&store.connection, b), 0);
        assert_eq!(last(&store.connection, b, "generated-note"), None);
        {
            let tx = store.connection.transaction().unwrap();
            tx.execute(
                "INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES(?1,?2,1000)",
                params![b.0.as_slice(), b"rolled-back-sdk-state".as_slice()],
            )
            .unwrap();
            super::super::adapter_store::mark_ack(&tx, b, "generated-note").unwrap();
            super::record(&tx, b, "generated-note").unwrap();
            super::super::adapter_store::mark_ack(&tx, b, "generated-card").unwrap();
            super::record(&tx, b, "generated-card").unwrap();
            assert_eq!(counter(&tx, b), 2);
            assert_eq!(last(&tx, b, "generated-note"), Some(1));
            assert_eq!(last(&tx, b, "generated-card"), Some(2));
            tx.rollback().unwrap();
        }
        assert_eq!(counter(&store.connection, b), 0);
        assert_eq!(last(&store.connection, b, "generated-card"), None);
        assert!(store.engine_state(b).unwrap().is_none());
        {
            let tx = store.connection.transaction().unwrap();
            tx.execute(
                "INSERT INTO engine_state(binding,serialization,saved_at_ms) VALUES(?1,?2,1000)",
                params![b.0.as_slice(), b"committed-sdk-state".as_slice()],
            )
            .unwrap();
            super::super::adapter_store::mark_ack(&tx, b, "generated-note").unwrap();
            super::record(&tx, b, "generated-note").unwrap();
            super::super::adapter_store::mark_ack(&tx, b, "generated-card").unwrap();
            super::record(&tx, b, "generated-card").unwrap();
            super::super::adapter_store::mark_ack(&tx, b, "generated-note").unwrap();
            super::record(&tx, b, "generated-note").unwrap();
            tx.commit().unwrap();
        }
        drop(store);
        let store = open(root.path());
        assert_eq!(counter(&store.connection, b), 3);
        assert_eq!(last(&store.connection, b, "generated-note"), Some(3));
        assert_eq!(last(&store.connection, b, "generated-card"), Some(2));
        assert!(store.engine_state(b).unwrap().is_some());
    }

    fn row(note: bool) -> super::EntityFacts {
        let payload = if note {
            entity_codec::Payload::Note(entity_codec::Note {
                id: 11,
                mid: 12,
                nt_struct_gen: 1,
                flds: b"public question\x1fpublic answer".to_vec(),
                ..Default::default()
            })
        } else {
            entity_codec::Payload::Config(entity_codec::Config {
                key: b"public-counter-key".to_vec(),
                val: b"1".to_vec(),
                ..Default::default()
            })
        };
        super::super::workflow_store::new_pending_entity(
            &RecordKey([3; 32]),
            payload,
            [5; 16],
            runtime::DirtySource::Observer,
            &mut 0,
            (1, 1, 1),
        )
        .unwrap()
    }
    fn ack(e: &mut super::EntityFacts, erev: u64) {
        e.sent = true;
        e.server_erev = erev;
        e.acked_rev = e.local_rev;
        e.acked_payload_digest = e.payload.payload_digest().unwrap();
        e.acked_row_digest = e.payload.row_digest().unwrap();
        e.acked_payload = Some(e.payload.clone());
        e.acked_system_fields = vec![1];
        e.acked_envelope = OpaqueBytes(vec![1]);
        e.mark = None;
        e.base_payload = None;
    }
    fn put(store: &mut TrackingStore, b: BindingId, e: &super::EntityFacts) {
        let tx = store.connection.transaction().unwrap();
        super::super::workflow_store::put(&tx, b, e).unwrap();
        tx.commit().unwrap();
    }
    #[test]
    fn only_new_ack_versions_advance_notes_and_generic_entities_not_local_edits_or_scans() {
        for note in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let mut store = open(root.path());
            let b = binding(&mut store);
            let mut e = row(note);
            put(&mut store, b, &e);
            assert_eq!(store.binding_acknowledgement_sequence(b).unwrap(), 0);
            ack(&mut e, 1);
            put(&mut store, b, &e);
            assert_eq!(store.entity_acknowledgement_sequence(b, &e.name).unwrap(), Some(1));
            put(&mut store, b, &e);
            e.observer_open = 2;
            e.observed_seq = 3;
            put(&mut store, b, &e);
            let mut changed = e.payload.clone();
            match &mut changed {
                entity_codec::Payload::Note(note) => note.flds = b"changed question\x1fpublic answer".to_vec(),
                entity_codec::Payload::Config(config) => config.val = b"2".to_vec(),
                _ => unreachable!(),
            }
            e.observe(changed, runtime::DirtySource::Observer, &mut 3).unwrap();
            put(&mut store, b, &e);
            assert_eq!(store.binding_acknowledgement_sequence(b).unwrap(), 1);
            ack(&mut e, 2);
            put(&mut store, b, &e);
            assert_eq!(store.binding_acknowledgement_sequence(b).unwrap(), 2);
            let tx = store.connection.transaction().unwrap();
            super::super::workflow_store::remove_entity(&tx, b, &e).unwrap();
            tx.commit().unwrap();
            assert_eq!(store.entity_acknowledgement_sequence(b, &e.name).unwrap(), None);
            assert_eq!(store.binding_acknowledgement_sequence(b).unwrap(), 2);
        }
    }
    #[test]
    fn legacy_headers_are_unchanged_and_gain_no_invented_ack_position() {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store);
        let mut e = row(false);
        ack(&mut e, 1);
        put(&mut store, b, &e);
        let before: String = store
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_entities WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        store
            .connection
            .execute_batch("DROP TABLE cloud_ack_entities; DROP TABLE cloud_ack_bindings; DROP TABLE cloud_ack_schema")
            .unwrap();
        store.install_runtime().unwrap();
        let after: String = store
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_entities WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        put(&mut store, b, &e);
        assert_eq!(store.entity_acknowledgement_sequence(b, &e.name).unwrap(), None);
        ack(&mut e, 2);
        put(&mut store, b, &e);
        assert_eq!(store.entity_acknowledgement_sequence(b, &e.name).unwrap(), Some(1));
    }
    #[test]
    fn counter_overflow_rolls_back_the_ack_header_with_its_metadata() {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store);
        let mut e = row(false);
        ack(&mut e, 1);
        put(&mut store, b, &e);
        store
            .connection
            .execute(
                "UPDATE cloud_ack_bindings SET counter=?1 WHERE binding=?2",
                params![i64::MAX, b.0.as_slice()],
            )
            .unwrap();
        let before: String = store
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_entities WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        ack(&mut e, 2);
        let tx = store.connection.transaction().unwrap();
        assert!(matches!(
            super::super::workflow_store::put(&tx, b, &e),
            Err(Error::InvalidData)
        ));
        tx.rollback().unwrap();
        let after: String = store
            .connection
            .query_row(
                "SELECT content FROM cloud_runtime_entities WHERE binding=?1",
                [b.0.as_slice()],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(before, after);
        assert_eq!(last(&store.connection, b, &e.name), Some(1));
    }
    #[test]
    fn future_counter_extension_refuses_before_installing_any_runtime_tables() {
        let root = tempfile::tempdir().unwrap();
        let mut store = match TrackingStore::open(root.path(), scope()).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("new store"),
        };
        store
            .connection
            .execute_batch(
                "CREATE TABLE cloud_ack_schema(version INTEGER NOT NULL); INSERT INTO cloud_ack_schema VALUES(2)",
            )
            .unwrap();
        assert!(matches!(store.install_runtime(), Err(Error::RuntimeSchemaUnsupported)));
        let runtime: bool = store
            .connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_schema')",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert!(!runtime);
        assert_eq!(
            store
                .connection
                .query_row("SELECT version FROM cloud_ack_schema", [], |r| r.get::<_, i64>(0))
                .unwrap(),
            2
        );
    }

    #[test]
    fn another_build_scope_cannot_read_the_acknowledgement_sequences() {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store);
        let mut e = row(false);
        ack(&mut e, 1);
        put(&mut store, b, &e);
        drop(store);
        for other in [
            BuildScope {
                environment: Environment::Production,
                ..scope()
            },
            BuildScope {
                container: ContainerId::new("iCloud.example.other-sequence").unwrap(),
                ..scope()
            },
        ] {
            let store = match TrackingStore::open(root.path(), other).unwrap() {
                OpenOutcome::Opened(store) => store,
                _ => panic!("supported tracking"),
            };
            assert_eq!(store.binding_acknowledgement_sequence(b), Err(Error::BindingNotFound));
            assert_eq!(
                store.entity_acknowledgement_sequence(b, &e.name),
                Err(Error::BindingNotFound)
            );
        }
        let store = open(root.path());
        assert_eq!(store.binding_acknowledgement_sequence(b).unwrap(), 1);
        assert_eq!(store.entity_acknowledgement_sequence(b, &e.name).unwrap(), Some(1));
    }
}
