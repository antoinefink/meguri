//! Qualified P6 epoch creation-time knowledge. Absence is unknown; the older
//! publication/media column is deliberately never a migration source.
use super::{control_codec::{DecodeOutcome, Epoch, EpochState}, store::decode, *};
use rusqlite::{Transaction, params};
use rusqlite::OptionalExtension;

pub(super) const SCHEMA: &str = "
CREATE TABLE cloud_epoch_crt(
 binding BLOB PRIMARY KEY REFERENCES bindings(id),
 crt INTEGER NOT NULL,
 authority TEXT NOT NULL CHECK(authority IN ('published','adopted','fetched'))
);";

#[derive(Clone, Copy)]
pub(super) enum Authority {
    Published,
    Adopted,
    Fetched,
}

/// Only a complete, readable MGEpoch of this binding can establish fetched
/// knowledge. This does not replace the separate K20 refusal workflow.
pub(super) fn fetched(binding: &Binding, records: &[workflow::Saved]) -> Option<Epoch> {
    let mut epochs = records.iter().filter(|r| r.name == "epoch");
    let record = epochs.next()?;
    if epochs.next().is_some() || record.system_fields.is_empty() {
        return None;
    }
    let DecodeOutcome::Supported(epoch) = Epoch::decode(&record.envelope).ok()? else {
        return None;
    };
    (epoch.state == EpochState::Complete
        && epoch.epoch_id == binding.facts.epoch.0
        && epoch.record_key == binding.facts.record_key.0)
        .then_some(epoch)
}

/// Called inside the producer's existing owned tracking transaction, only
/// after its publication/adoption/fetch admission has qualified the evidence.
pub(super) fn put(
    tx: &Transaction<'_>,
    binding: BindingId,
    epoch: &Epoch,
    authority: Authority,
) -> Result<(), Error> {
    let facts: String = tx.query_row(
        "SELECT facts FROM bindings WHERE id=?1 AND salvage=0",
        [binding.0.as_slice()], |r| r.get(0),
    )?;
    let facts: BindingFacts = decode(&facts)?;
    if facts.epoch.0 != epoch.epoch_id || facts.record_key.0 != epoch.record_key
        || (!matches!(authority, Authority::Published) && epoch.state != EpochState::Complete)
    {
        return Err(Error::InvalidData);
    }
    let authority = match authority {
        Authority::Published => "published",
        Authority::Adopted => "adopted",
        Authority::Fetched => "fetched",
    };
    tx.execute(
        "INSERT INTO cloud_epoch_crt(binding,crt,authority) VALUES(?1,?2,?3)
         ON CONFLICT(binding) DO UPDATE SET crt=excluded.crt,authority=excluded.authority",
        params![binding.0.as_slice(), epoch.crt, authority],
    )?;
    Ok(())
}

impl TrackingStore {
    pub(super) fn epoch_crt(&self, binding: BindingId) -> Result<Option<i64>, Error> {
        self.binding(binding)?.ok_or(Error::BindingNotFound)?;
        self.connection.query_row(
            "SELECT crt FROM cloud_epoch_crt WHERE binding=?1",
            [binding.0.as_slice()], |r| r.get(0),
        ).optional().map_err(Error::from)
    }

    #[cfg(feature = "test-hooks")]
    /// Diagnostic read of qualified epoch knowledge, including after a child closes.
    pub fn epoch_crt_for_testing(&self, binding: BindingId) -> Result<Option<i64>, Error> {
        self.epoch_crt(binding)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

    fn fixture() -> (tempfile::TempDir, TrackingStore, Binding) {
        let directory = tempfile::tempdir().unwrap();
        let OpenOutcome::Missing(mut store) = TrackingStore::open(directory.path(), BuildScope {
            container: ContainerId::new("synthetic-epoch-crt").unwrap(),
            environment: Environment::Development,
        }).unwrap() else { panic!("new store required"); };
        store.install_runtime().unwrap();
        let binding = store.create_binding(BindingFacts {
            account: OpaqueBytes(vec![1]),
            epoch: EpochId([2; 16]),
            record_key: RecordKey([3; 32]),
            publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
            state: BindingState::Active,
        }, &FixedClock::new(1000, UtcOffset::UTC), &SeededRandom::new(1920)).unwrap();
        (directory, store, binding)
    }

    fn epoch(binding: &Binding) -> Epoch {
        Epoch {
            epoch_id: binding.facts.epoch.0,
            record_key: binding.facts.record_key.0,
            crt: 1234,
            ver: 18,
            state: EpochState::Complete,
            reason: control_codec::EpochReason::Initial,
            published_at_ms: 1000,
            counts: Vec::new(),
        }
    }

    fn record(epoch: &Epoch) -> workflow::Saved {
        workflow::Saved { name: "epoch".into(), envelope: epoch.encode().unwrap(), system_fields: vec![1] }
    }

    #[test]
    fn epoch_crt_legacy_and_media_root_remain_unknown_after_migration() {
        let (_directory, mut store, binding) = fixture();
        store.accept_media_root(binding.id, [5; 32], 1234).unwrap();
        assert_eq!(store.epoch_crt(binding.id).unwrap(), None);
        // Recreate the historical publication V2 shape, retaining its local fallback.
        store.connection.execute_batch("DROP TABLE cloud_epoch_crt; UPDATE cloud_publication_schema SET version=2;").unwrap();
        store.install_runtime().unwrap();
        assert_eq!(store.epoch_crt(binding.id).unwrap(), None);
        assert_eq!(store.publication_media(binding.id).unwrap().unwrap().2, 1234);
        let version: i64 = store.connection.query_row("SELECT version FROM cloud_publication_schema", [], |r| r.get(0)).unwrap();
        assert_eq!(version, 3);
    }

    #[test]
    fn epoch_crt_version_and_ownership_guards_are_preserved() {
        let (_directory, mut store, binding) = fixture();
        store.connection.execute("UPDATE cloud_publication_schema SET version=4", []).unwrap();
        assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::RuntimeSchemaUnsupported)));
        store.connection.execute("UPDATE cloud_publication_schema SET version=2", []).unwrap();
        assert!(matches!(runtime_store::check_version(&store.connection), Err(Error::InvalidData)));
        store.connection.execute("UPDATE cloud_publication_schema SET version=3", []).unwrap();
        store.scope.environment = Environment::Production;
        assert!(matches!(store.epoch_crt(binding.id), Err(Error::BindingNotFound)));
    }

    #[test]
    fn epoch_crt_fetched_requires_complete_bound_readable_evidence() {
        let (_directory, _store, binding) = fixture();
        let complete = epoch(&binding);
        assert_eq!(fetched(&binding, &[record(&complete)]).unwrap().crt, complete.crt);
        for invalid in [
            Epoch { epoch_id: [8; 16], ..complete.clone() },
            Epoch { record_key: [8; 32], ..complete.clone() },
            Epoch { state: EpochState::Publishing, ..complete.clone() },
        ] {
            assert!(fetched(&binding, &[record(&invalid)]).is_none());
        }
        let mut missing_fields = record(&complete);
        missing_fields.system_fields.clear();
        assert!(fetched(&binding, &[missing_fields]).is_none());
        let mut unreadable = record(&complete);
        unreadable.envelope = vec![255];
        assert!(fetched(&binding, &[unreadable]).is_none());
        assert!(fetched(&binding, &[]).is_none());
        assert!(fetched(&binding, &[record(&complete), record(&complete)]).is_none());
    }

    #[test]
    fn epoch_crt_fetch_knowledge_and_inbox_rollback_together() {
        let (_directory, mut store, binding) = fixture();
        let fetched = fetched(&binding, &[record(&epoch(&binding))]).unwrap();
        let entry = InboxEntry {
            record_type: RecordType::Entity,
            record_name: "synthetic-deletion".into(),
            value: InboxValue::Deletion,
            reason: InboxReason::Ready,
            source: InboxSource::Fetch,
            absence_pass: None,
            retry_count: 0,
            fetched_at_ms: 1000,
        };
        store.connection.execute_batch("CREATE TEMP TRIGGER reject_epoch_crt BEFORE INSERT ON cloud_epoch_crt BEGIN SELECT RAISE(ABORT,'epoch knowledge constraint'); END;").unwrap();
        assert!(store.receive_records(binding.id, &[entry], &[], Some(&fetched)).is_err());
        assert!(store.inbox(binding.id).unwrap().is_empty());
        assert_eq!(store.epoch_crt(binding.id).unwrap(), None);
    }
}
