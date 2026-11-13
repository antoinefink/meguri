use super::*;
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

fn scope() -> BuildScope {
    BuildScope { container: ContainerId::new("synthetic-incomplete").unwrap(), environment: Environment::Development }
}
fn open(root: &std::path::Path) -> TrackingStore {
    let mut store = match TrackingStore::open(root, scope()).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store, _ => panic!("usable tracking"),
    };
    store.install_runtime().unwrap();
    store
}
fn binding(store: &mut TrackingStore, seed: u64) -> BindingId {
    store.create_binding(BindingFacts { account: OpaqueBytes(vec![1]), epoch: EpochId([2; 16]),
        record_key: RecordKey([3; 32]), publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
        state: BindingState::Active }, &FixedClock::new(1000, UtcOffset::UTC), &SeededRandom::new(seed)).unwrap().id
}
fn ended(native: u64, opened: u64, outcome: CollectionOutcome) -> Ended {
    Ended { order: Order { opened, native }, boundary: 10, ended_ms: 1000,
        outgoing: if outcome == CollectionOutcome::Incomplete { [("original-name".into(), Outgoing { revision: 2, done: false })].into() } else { BTreeMap::new() },
        incoming: Vec::new(), outcome, work_known: true, accepted: false, cleared_by_success: false, settled: false }
}
fn persist(store: &mut TrackingStore, b: BindingId, end: &Ended) {
    let tx = store.connection.transaction().unwrap();
    record_end(&tx, b, end).unwrap();
    tx.commit().unwrap();
}
fn result(end: &Ended, policy: u64) -> super::super::policy::ExchangeRecord {
    super::super::policy::ExchangeRecord { exchange: policy, ended_ms: end.ended_ms, source: super::super::policy::Source::Command,
        outcome: match end.outcome {
            CollectionOutcome::Succeeded => ExchangeOutcome::Succeeded { selected: 0, held_back: 0, newer_pending: 0,
                incoming_waiting: 0, media: Default::default() },
            CollectionOutcome::Incomplete => ExchangeOutcome::Incomplete { outstanding: 1, blocked: 0 },
            CollectionOutcome::Cancelled => ExchangeOutcome::Cancelled,
            CollectionOutcome::Failed => ExchangeOutcome::Failed { class: super::super::policy::OperationError::Other },
            CollectionOutcome::WaitingNoAccount => ExchangeOutcome::Waiting { class: None, retry_after_ms: None },
        } }
}
fn accept_end(store: &mut TrackingStore, b: BindingId, end: &Ended, policy: u64) {
    store.record_cloud_policy_exchange_original(b, "original-opening", &result(end, policy), Some(end.order.native)).unwrap();
}
fn queued_config(revision: u64) -> InboxEntry {
    let payload = entity_codec::Payload::Config(entity_codec::Config { key: b"public-key".to_vec(), val: b"1".to_vec(), ..Default::default() });
    let fields = entity_codec::EntityFields {
        origin: [8; 16], writer: [9; 16], kind: entity_codec::Kind::Config,
        key: b"public-key".to_vec(), erev: revision, generation: None,
        tombstone: false, deleted_at_ms: 0, base_sha256: None,
    };
    let bytes = entity_codec::Entity::build(fields, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let value = InboxValue::Record {
        envelope: OpaqueBytes(bytes), staged_asset: None, erev: revision,
        system_fields: OpaqueBytes(vec![revision as u8]),
    };
    InboxEntry {
        value, record_name: "original-incoming".into(), record_type: RecordType::Entity,
        fetched_at_ms: 1000, retry_count: 0, absence_pass: None,
        source: InboxSource::Fetch, reason: InboxReason::Ready,
    }
}

#[test]
fn incomplete_store_new_binding_history_does_not_backfill_legacy_order() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = open(directory.path());
    let id = binding(&mut db, 1);
    let known = |db: &TrackingStore| db.connection.query_row(
        "SELECT history_known FROM cloud_incomplete_bindings WHERE binding=?1", [id.0.as_slice()], |row| row.get::<_, bool>(0)).unwrap();
    assert!(known(&db));
    assert!(!db.exchange_incomplete(id).unwrap());
    for table in ["outgoing", "incoming", "flights", "results", "ends", "legacy", "bindings", "schema"] {
        db.connection.execute_batch(&format!("DROP TABLE cloud_incomplete_{table}")).unwrap();
    }
    let historical = result(&ended(1, 1, CollectionOutcome::Incomplete), 1);
    db.connection.execute("INSERT INTO cloud_policy_exchanges(content,binding,owner,exchange) VALUES(?2,?1,'legacy',1)",
        params![id.0.as_slice(), encode(&historical).unwrap()]).unwrap();
    db.install_runtime().unwrap();
    assert!(!known(&db));
    assert!(db.exchange_incomplete(id).unwrap());
    let recent = ended(2, 99, CollectionOutcome::Succeeded);
    persist(&mut db, id, &recent);
    accept_end(&mut db, id, &recent, 2);
    assert!(db.exchange_incomplete(id).unwrap());
    assert_eq!(ends(&db.connection, id).unwrap().len(), 1);
    drop(db);
    let db = open(directory.path());
    assert!(!known(&db));
    assert!(db.exchange_incomplete(id).unwrap());
}

#[test]
fn incomplete_store_empty_upgrade_invents_neither_incomplete_nor_complete_history() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 2);
    store.connection.execute("DELETE FROM cloud_incomplete_bindings WHERE binding=?1", [b.0.as_slice()]).unwrap();
    store.install_runtime().unwrap();
    assert!(!store.exchange_incomplete(b).unwrap());
    assert!(!store.connection.query_row("SELECT history_known FROM cloud_incomplete_bindings WHERE binding=?1", [b.0.as_slice()], |r| r.get::<_, bool>(0)).unwrap());
    assert!(ends(&store.connection, b).unwrap().is_empty());
}

#[test]
fn incomplete_store_partial_legacy_loss_refuses_reopen_installation_without_writes() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = open(directory.path());
    let id = binding(&mut db, 22);
    for table in ["outgoing", "incoming", "flights", "results", "ends", "legacy", "bindings", "schema"] {
        db.connection.execute_batch(&format!("DROP TABLE cloud_incomplete_{table}")).unwrap();
    }
    for exchange in [1, 2] {
        let record = result(&ended(exchange, 1, CollectionOutcome::Incomplete), exchange);
        db.connection.execute("INSERT INTO cloud_policy_exchanges(binding,owner,exchange,content) VALUES(?1,'legacy',?2,?3)",
            params![id.0.as_slice(), exchange as i64, encode(&record).unwrap()]).unwrap();
    }
    db.install_runtime().unwrap();
    assert_eq!(db.connection.query_row("SELECT count(*) FROM cloud_incomplete_legacy WHERE binding=?1", [id.0.as_slice()], |r| r.get::<_, i64>(0)).unwrap(), 2);
    assert!(db.exchange_incomplete(id).unwrap());
    assert_eq!(db.connection.execute("DELETE FROM cloud_incomplete_legacy WHERE binding=?1 AND owner='legacy' AND exchange=1", [id.0.as_slice()]).unwrap(), 1);
    assert_eq!(db.connection.query_row("SELECT count(*) FROM cloud_incomplete_results WHERE binding=?1 AND native IS NULL", [id.0.as_slice()], |r| r.get::<_, i64>(0)).unwrap(), 2);
    drop(db);
    let outside = BuildScope { container: ContainerId::new("other-incomplete-scope").unwrap(), environment: Environment::Production };
    let mut other = match TrackingStore::open(directory.path(), outside).unwrap() {
        OpenOutcome::Opened(db) => db, _ => panic!("legacy loss preserves SQL integrity"),
    };
    other.install_runtime().unwrap();
    drop(other);
    let mut db = match TrackingStore::open(directory.path(), scope()).unwrap() {
        OpenOutcome::Opened(db) => db, _ => panic!("legacy loss preserves SQL integrity"),
    };
    let changes = db.connection.total_changes();
    assert!(matches!(db.install_runtime(), Err(Error::InvalidData)));
    assert_eq!(db.connection.total_changes(), changes);
    assert!(db.exchange_incomplete(id).unwrap());
    assert_eq!(db.connection.query_row("SELECT count(*) FROM cloud_incomplete_legacy WHERE binding=?1", [id.0.as_slice()], |r| r.get::<_, i64>(0)).unwrap(), 1);
}

#[test]
fn incomplete_store_future_or_orphan_schema_refuses_before_runtime_writes() {
    for occupied in [0, 1, 2] {
        let root = tempfile::tempdir().unwrap();
        let mut store = match TrackingStore::open(root.path(), scope()).unwrap() { OpenOutcome::Missing(s) => s, _ => panic!("new") };
        store.connection.execute_batch(match occupied {
            0 => "CREATE TABLE cloud_incomplete_schema(version INTEGER); INSERT INTO cloud_incomplete_schema VALUES(2)",
            1 => "CREATE TABLE cloud_incomplete_ends(binding BLOB,native INTEGER,content TEXT)",
            _ => "CREATE VIEW cloud_incomplete_schema AS SELECT 1 AS version",
        }).unwrap();
        let before = store.connection.total_changes();
        assert!(store.install_runtime().is_err());
        assert_eq!(store.connection.total_changes(), before);
        assert!(!store.connection.query_row("SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE name='cloud_runtime_schema')", [], |r| r.get::<_, bool>(0)).unwrap());
    }
}

#[test]
fn incomplete_store_corrupt_or_unknown_ended_fields_refuse_before_upgrade_writes() {
    for content in ["{}", "{\"order\":{\"opened\":0,\"native\":1}}"] {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store, 3);
        store.connection.execute("INSERT INTO cloud_incomplete_ends(binding,native,content,opened,active,accepted,success) VALUES(?1,1,?2,1,1,0,0)", params![b.0.as_slice(), content]).unwrap();
        let before = store.connection.total_changes();
        assert!(store.install_runtime().is_err());
        assert_eq!(store.connection.total_changes(), before);
    }
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 4);
    persist(&mut store, b, &ended(1, 1, CollectionOutcome::Incomplete));
    store.connection.execute("UPDATE cloud_incomplete_ends SET content=json_set(content,'$.future_proof',json('true')) WHERE binding=?1 AND native=1", [b.0.as_slice()]).unwrap();
    let before = store.connection.total_changes();
    assert!(store.install_runtime().is_err());
    assert_eq!(store.connection.total_changes(), before);
}

#[test]
fn incomplete_store_policy_failure_atomically_rolls_back_both_addition_and_success_clear() {
    for clear in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let mut store = open(root.path());
        let b = binding(&mut store, 5);
        let first = ended(1, 1, CollectionOutcome::Incomplete);
        persist(&mut store, b, &first);
        if clear { accept_end(&mut store, b, &first, 1); }
        let next = ended(2, 1, CollectionOutcome::Succeeded);
        if clear { persist(&mut store, b, &next); }
        let before = store.cloud_policy_evidence(b).unwrap();
        store.connection.execute_batch("CREATE TEMP TRIGGER reject_incomplete_evidence BEFORE INSERT ON cloud_policy_evidence BEGIN SELECT RAISE(ABORT,'incomplete evidence fault'); END;
            CREATE TEMP TRIGGER reject_incomplete_update BEFORE UPDATE ON cloud_policy_evidence BEGIN SELECT RAISE(ABORT,'incomplete evidence fault'); END;").unwrap();
        let target = if clear { &next } else { &first };
        assert!(store.record_cloud_policy_exchange_original(b, "original-opening", &result(target, target.order.native), Some(target.order.native)).is_err());
        assert_eq!(store.exchange_incomplete(b).unwrap(), clear);
        assert_eq!(store.cloud_policy_evidence(b).unwrap(), before);
        assert!(!store.policy_exchange_recorded(b, "original-opening", target.order.native).unwrap());
        assert!(!ends(&store.connection, b).unwrap().iter().find(|e| e.order.native == target.order.native).unwrap().accepted);
    }
}

#[test]
fn incomplete_store_original_completion_and_adapter_commit_are_atomic() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 6);
    store.connection.execute_batch("CREATE TEMP TRIGGER reject_original_adapter BEFORE INSERT ON cloud_adapter_state BEGIN SELECT RAISE(ABORT,'original completion fault'); END;").unwrap();
    assert!(store.adapter_record_exchange(b, &Default::default(), &ended(1, 1, CollectionOutcome::Incomplete)).is_err());
    assert!(ends(&store.connection, b).unwrap().is_empty());
}

#[test]
fn incomplete_store_later_original_success_clears_durably_without_clock_or_delivery_order() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 7);
    let old_success = ended(1, 1, CollectionOutcome::Succeeded);
    let first = ended(2, 1, CollectionOutcome::Incomplete);
    let mut later_success = ended(3, 2, CollectionOutcome::Succeeded);
    later_success.ended_ms = -10_000;
    for end in [&old_success, &first, &later_success] { persist(&mut store, b, end); }
    accept_end(&mut store, b, &first, 2);
    assert!(store.exchange_incomplete(b).unwrap());
    accept_end(&mut store, b, &old_success, 1); // first late delivery
    accept_end(&mut store, b, &old_success, 1); // duplicate
    assert!(store.exchange_incomplete(b).unwrap());
    drop(store);
    let mut store = open(root.path());
    assert!(store.exchange_incomplete(b).unwrap());
    // Mere production of a later success, including across reopen, did not clear.
    accept_end(&mut store, b, &later_success, 3);
    assert!(!store.exchange_incomplete(b).unwrap());
    accept_end(&mut store, b, &first, 2);
    assert!(!store.exchange_incomplete(b).unwrap());
    let newer_incomplete = ended(4, 2, CollectionOutcome::Incomplete);
    persist(&mut store, b, &newer_incomplete);
    accept_end(&mut store, b, &newer_incomplete, 4);
    accept_end(&mut store, b, &later_success, 3);
    assert!(store.exchange_incomplete(b).unwrap());
    drop(store);
    assert!(open(root.path()).exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_success_accepted_before_older_incomplete_delivery_still_proves_original_order() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 8);
    let first = ended(1, 1, CollectionOutcome::Incomplete);
    let success = ended(2, 1, CollectionOutcome::Succeeded);
    persist(&mut store, b, &first);
    persist(&mut store, b, &success);
    accept_end(&mut store, b, &success, 2);
    drop(store);
    let mut store = open(root.path());
    accept_end(&mut store, b, &first, 1);
    assert!(!store.exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_wrong_or_missing_original_result_facts_cannot_create_success_proof() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 9);
    let first = ended(1, 1, CollectionOutcome::Incomplete);
    persist(&mut store, b, &first);
    accept_end(&mut store, b, &first, 1);
    let success = ended(2, 1, CollectionOutcome::Succeeded);
    assert!(store.record_cloud_policy_exchange_original(b, "original-opening", &result(&success, 2), Some(2)).is_err());
    persist(&mut store, b, &success);
    let mut wrong = result(&success, 2);
    wrong.ended_ms += 1;
    assert!(store.record_cloud_policy_exchange_original(b, "original-opening", &wrong, Some(2)).is_err());
    assert!(store.record_cloud_policy_exchange_original(b, "original-opening", &result(&success, 2), Some(1)).is_err());
    // Display-only legacy success has no original-order authority.
    store.record_cloud_policy_exchange_original(b, "unqualified-history", &result(&success, 2), None).unwrap();
    assert!(store.exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_ack_boundary_namespace_and_unsent_revision_do_not_mix_obligations() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 10);
    let first = ended(1, 1, CollectionOutcome::Incomplete);
    let mut second = ended(2, 1, CollectionOutcome::Incomplete);
    second.boundary = 20;
    second.outgoing.get_mut("original-name").unwrap().revision = 3;
    for e in [&first, &second] { persist(&mut store, b, e); accept_end(&mut store, b, e, e.order.native); }
    {
        let tx = store.connection.transaction().unwrap();
        ack_image(&tx, b, "original-name", 1, 9).unwrap();
        dropped(&tx, b, "original-name", 1).unwrap();
        tx.commit().unwrap();
    }
    assert!([1, 2].into_iter().all(|native| pending_name(&store, b, native, "original-name")));
    {
        let tx = store.connection.transaction().unwrap();
        ack_image(&tx, b, "original-name", 1, 10).unwrap();
        tx.commit().unwrap();
    }
    assert!(!pending_name(&store, b, 1, "original-name"));
    assert!(pending_name(&store, b, 2, "original-name"));
    assert!(store.exchange_incomplete(b).unwrap());
    {
        let tx = store.connection.transaction().unwrap();
        // A later accepted opening's snapshot follows the original barrier.
        ack_image(&tx, b, "original-name", 2, 1).unwrap();
        tx.rollback().unwrap();
    }
    assert!(store.exchange_incomplete(b).unwrap());
    {
        let tx = store.connection.transaction().unwrap();
        dropped(&tx, b, "original-name", 3).unwrap();
        tx.commit().unwrap();
    }
    assert!(!store.exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_incoming_identity_is_separate_from_outgoing_and_survives_raw_absence() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 11);
    let original = queued_config(2);
    store.receive_records(b, &[original.clone()], &[], None, None).unwrap();
    let inbox = store.inbox(b).unwrap()[0].id;
    let mut end = ended(1, 1, CollectionOutcome::Incomplete);
    end.incoming.push(Incoming { inbox, identity: incoming_version_key(&original).unwrap(), done: false });
    persist(&mut store, b, &end);
    accept_end(&mut store, b, &end, 1);
    store.connection.execute("DELETE FROM inbox WHERE binding=?1", [b.0.as_slice()]).unwrap();
    assert!(store.exchange_incomplete(b).unwrap());
    {
        let tx = store.connection.transaction().unwrap();
        incoming_done(&tx, b, inbox, &queued_config(3)).unwrap(); // same name, different entry/version
        ack_image(&tx, b, "original-name", 1, 10).unwrap();
        tx.commit().unwrap();
    }
    assert!(store.exchange_incomplete(b).unwrap());
    {
        let tx = store.connection.transaction().unwrap();
        incoming_done(&tx, b, inbox, &original).unwrap();
        tx.rollback().unwrap();
    }
    assert!(store.exchange_incomplete(b).unwrap());
    {
        let tx = store.connection.transaction().unwrap();
        incoming_done(&tx, b, inbox, &original).unwrap();
        tx.commit().unwrap();
    }
    assert!(!store.exchange_incomplete(b).unwrap());
    drop(store);
    assert!(!open(root.path()).exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_other_binding_and_build_scope_have_no_clearing_authority() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 12);
    let first = ended(1, 1, CollectionOutcome::Incomplete);
    persist(&mut store, b, &first);
    accept_end(&mut store, b, &first, 1);
    // Keep the original active binding; another publication/account is isolated.
    store.connection.execute("UPDATE bindings SET facts=json_set(facts,'$.state',json('{\"Isolated\":\"Stopped\"}')) WHERE id=?1", [b.0.as_slice()]).unwrap();
    let other = binding(&mut store, 13);
    let success = ended(2, 2, CollectionOutcome::Succeeded);
    persist(&mut store, other, &success);
    accept_end(&mut store, other, &success, 2);
    assert!(store.exchange_incomplete(b).unwrap());
    let tx = store.connection.transaction().unwrap();
    ack_image(&tx, other, "original-name", 99, 99).unwrap();
    tx.commit().unwrap();
    assert!(store.exchange_incomplete(b).unwrap());
    let foreign_scope = BuildScope { container: ContainerId::new("synthetic-foreign").unwrap(), environment: Environment::Production };
    drop(store);
    let mut foreign = match TrackingStore::open(root.path(), foreign_scope).unwrap() { OpenOutcome::Opened(s) => s, _ => panic!("foreign store") };
    foreign.install_runtime().unwrap();
    assert!(matches!(foreign.exchange_incomplete(b), Err(Error::BindingNotFound)));
    assert!(foreign.record_cloud_policy_exchange_original(b, "original-opening", &result(&success, 2), Some(2)).is_err());
    drop(foreign);
    assert!(open(root.path()).exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_unknown_work_remains_until_accepted_original_later_success() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 14);
    let mut first = ended(1, 1, CollectionOutcome::Incomplete);
    first.outgoing.clear();
    first.work_known = false;
    persist(&mut store, b, &first);
    accept_end(&mut store, b, &first, 1);
    assert!(store.exchange_incomplete(b).unwrap());
    let success = ended(2, 1, CollectionOutcome::Succeeded);
    persist(&mut store, b, &success);
    assert!(store.exchange_incomplete(b).unwrap());
    accept_end(&mut store, b, &success, 2);
    assert!(!store.exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_historical_writer_after_upgrade_cannot_hide_an_incomplete() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 15);
    let old = result(&ended(1, 1, CollectionOutcome::Incomplete), 1);
    store.connection.execute("INSERT INTO cloud_policy_exchanges(binding,owner,exchange,content) VALUES(?1,'historical-writer',1,?2)", params![b.0.as_slice(), encode(&old).unwrap()]).unwrap();
    drop(store);
    let store = open(root.path());
    assert!(store.exchange_incomplete(b).unwrap());
    assert!(!store.connection.query_row("SELECT history_known FROM cloud_incomplete_bindings WHERE binding=?1", [b.0.as_slice()], |r| r.get::<_, bool>(0)).unwrap());
}

#[test]
fn incomplete_store_prepared_and_legacy_flights_are_not_handed_ack_proof() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 16);
    let end = ended(1, 1, CollectionOutcome::Incomplete);
    persist(&mut store, b, &end);
    accept_end(&mut store, b, &end, 1);
    let payload = entity_codec::Payload::Config(entity_codec::Config { key: b"public-key".to_vec(), val: b"1".to_vec(), ..Default::default() });
    let flight = Flight { local_rev: 2, payload_digest: payload.payload_digest().unwrap(), row_digest: payload.row_digest().unwrap(),
        payload: payload.into(), erev: 2, image_seq: 10, held: true, tombstone: false, tombstone_envelope_digest: None };
    {
        let tx = store.connection.transaction().unwrap();
        stage_flight_origin(&tx, b, "original-name", &flight, 1).unwrap();
        ack(&tx, b, "original-name", &flight).unwrap();
        tx.commit().unwrap();
    }
    assert!(store.exchange_incomplete(b).unwrap());
    let mut legacy = flight.clone();
    legacy.erev = 3;
    store.record_incomplete_handoffs(b, 1, &[("original-name".into(), legacy.identity(), 10)]).unwrap();
    {
        let tx = store.connection.transaction().unwrap();
        ack(&tx, b, "original-name", &legacy).unwrap();
        tx.commit().unwrap();
    }
    assert!(store.exchange_incomplete(b).unwrap());
    store.record_incomplete_handoffs(b, 1, &[("original-name".into(), flight.identity(), 10)]).unwrap();
    {
        let tx = store.connection.transaction().unwrap();
        ack(&tx, b, "original-name", &flight).unwrap();
        tx.commit().unwrap();
    }
    assert!(!store.exchange_incomplete(b).unwrap());
}

#[test]
fn incomplete_store_missing_accepted_receipt_is_corruption_not_a_success_order_proof() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 17);
    let end = ended(1, 1, CollectionOutcome::Succeeded);
    persist(&mut store, b, &end);
    accept_end(&mut store, b, &end, 1);
    store.connection.execute("DELETE FROM cloud_incomplete_results WHERE binding=?1", [b.0.as_slice()]).unwrap();
    let before = store.connection.total_changes();
    assert!(store.install_runtime().is_err());
    assert_eq!(store.connection.total_changes(), before);
}

fn pending_name(store: &TrackingStore, b: BindingId, native: i64, name: &str) -> bool {
    store.connection.query_row("SELECT EXISTS(SELECT 1 FROM cloud_incomplete_outgoing WHERE binding=?1 AND native=?2 AND name=?3)",
        params![b.0.as_slice(), native, name], |r| r.get(0)).unwrap()
}

#[test]
fn incomplete_store_proofs_touch_pending_names_without_decoding_settled_history() {
    let root = tempfile::tempdir().unwrap();
    let mut store = open(root.path());
    let b = binding(&mut store, 18);
    let old = ended(1, 1, CollectionOutcome::Incomplete);
    persist(&mut store, b, &old);
    accept_end(&mut store, b, &old, 1);
    {
        let tx = store.connection.transaction().unwrap();
        ack_image(&tx, b, "original-name", 1, 10).unwrap();
        tx.commit().unwrap();
    }
    let current = ended(2, 2, CollectionOutcome::Incomplete);
    persist(&mut store, b, &current);
    accept_end(&mut store, b, &current, 2);
    // A callback must not inspect or rewrite the retired completion. Open-time
    // validation still rejects its invalid content, independently of this proof.
    store.connection.execute("UPDATE cloud_incomplete_ends SET content='{}' WHERE binding=?1 AND native=1", [b.0.as_slice()]).unwrap();
    store.connection.execute_batch("CREATE TEMP TRIGGER reject_retired_completion BEFORE UPDATE ON cloud_incomplete_ends WHEN OLD.native=1 BEGIN SELECT RAISE(ABORT,'retired completion touched'); END;").unwrap();
    let tx = store.connection.transaction().unwrap();
    ack_image(&tx, b, "unrelated-name", 2, 10).unwrap();
    ack_image(&tx, b, "original-name", 2, 10).unwrap();
    tx.commit().unwrap();
    assert!(!store.exchange_incomplete(b).unwrap());
    assert!(store.install_runtime().is_err());
}

#[test]
fn incomplete_store_other_scope_skips_semantics_but_refuses_future_schema() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = open(directory.path());
    let id = binding(&mut db, 19);
    // Valid SQL and valid JSON, but not a typed completion receipt.
    db.connection.execute("INSERT INTO cloud_incomplete_ends(binding,native,content,opened,active,accepted,success) VALUES(?1,1,'{}',1,1,0,0)",
        [id.0.as_slice()]).unwrap();
    drop(db);
    let outside = BuildScope { container: ContainerId::new("other-incomplete-scope").unwrap(), environment: Environment::Production };
    let mut db = match TrackingStore::open(directory.path(), outside).unwrap() {
        OpenOutcome::Opened(db) => db, _ => panic!("physically valid database"),
    };
    db.install_runtime().unwrap();
    assert!(matches!(db.exchange_incomplete(id), Err(Error::BindingNotFound)));
    let changes = db.connection.total_changes();
    assert!(validate_scope(&db.connection, &scope()).is_err());
    assert_eq!(db.connection.total_changes(), changes);
    drop(db);
    let mut own = match TrackingStore::open(directory.path(), scope()).unwrap() {
        OpenOutcome::Opened(db) => db, _ => panic!("integrity probe accepts valid SQL"),
    };
    let changes = own.connection.total_changes();
    assert!(own.install_runtime().is_err());
    assert_eq!(own.connection.total_changes(), changes);
    // Model a future extension with its own valid version constraint.
    own.connection.execute_batch("DROP TABLE cloud_incomplete_schema; CREATE TABLE cloud_incomplete_schema(version INTEGER NOT NULL CHECK(version=2)); INSERT INTO cloud_incomplete_schema VALUES(2)").unwrap();
    let changes = own.connection.total_changes();
    assert!(matches!(own.install_runtime(), Err(Error::RuntimeSchemaUnsupported)));
    assert_eq!(own.connection.total_changes(), changes);
    drop(own);
    let outside = BuildScope { container: ContainerId::new("other-incomplete-scope").unwrap(), environment: Environment::Production };
    let source_bytes = || ["meguri-cloud.db", "meguri-cloud.db-wal", "meguri-cloud.db-shm"].map(|name| {
        match std::fs::read(directory.path().join(name)) {
            Ok(bytes) => Some(bytes),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => None,
            Err(error) => panic!("synthetic tracking source: {error}"),
        }
    });
    let before = source_bytes();
    for build in [scope(), outside] {
        assert!(matches!(TrackingStore::open(directory.path(), build), Err(Error::RuntimeSchemaUnsupported)));
        assert_eq!(source_bytes(), before);
    }
}

#[test]
fn incomplete_store_check_damage_is_global_before_scope_validation() {
    let directory = tempfile::tempdir().unwrap();
    let mut db = open(directory.path());
    let id = binding(&mut db, 20);
    db.connection.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
    db.connection.execute("UPDATE cloud_incomplete_bindings SET history_known=9 WHERE binding=?1", [id.0.as_slice()]).unwrap();
    drop(db);
    let outside = BuildScope { container: ContainerId::new("other-incomplete-scope").unwrap(), environment: Environment::Production };
    for build in [scope(), outside] {
        assert!(matches!(TrackingStore::open(directory.path(), build).unwrap(), OpenOutcome::Damaged));
    }
}

#[test]
fn incomplete_store_partial_cause_loss_cannot_finish_before_or_after_result_acceptance() {
    for incoming in [false, true] {
        for (accepted, reopen_first) in [(false, false), (false, true), (true, false), (true, true)] {
            let directory = tempfile::tempdir().unwrap();
            let mut db = open(directory.path());
            let id = binding(&mut db, 21);
            let mut original = ended(1, 1, CollectionOutcome::Incomplete);
            let entry = queued_config(2);
            if incoming {
                original.outgoing.clear();
                original.incoming = [1, 2].into_iter().map(|inbox| Incoming {
                    inbox, identity: incoming_version_key(&entry).unwrap(), done: false,
                }).collect();
            } else {
                original.outgoing.insert("lost-name".into(), Outgoing { revision: 2, done: false });
            }
            persist(&mut db, id, &original);
            if accepted { accept_end(&mut db, id, &original, 1); }
            let sql = if incoming { "DELETE FROM cloud_incomplete_incoming WHERE binding=?1 AND inbox=2" }
                else { "DELETE FROM cloud_incomplete_outgoing WHERE binding=?1 AND name='lost-name'" };
            assert_eq!(db.connection.execute(sql, [id.0.as_slice()]).unwrap(), 1);
            if reopen_first {
                drop(db);
                db = match TrackingStore::open(directory.path(), scope()).unwrap() {
                    OpenOutcome::Opened(db) => db, _ => panic!("partial loss preserves SQL integrity"),
                };
            }
            let changes = db.connection.total_changes();
            assert!(db.install_runtime().is_err());
            assert_eq!(db.connection.total_changes(), changes);
            let tx = db.connection.transaction().unwrap();
            if incoming { incoming_done(&tx, id, 1, &entry).unwrap(); }
            else { ack_image(&tx, id, "original-name", 1, 10).unwrap(); }
            tx.commit().unwrap();
            if !accepted { accept_end(&mut db, id, &original, 1); }
            assert!(db.exchange_incomplete(id).unwrap());
            assert!(!ends(&db.connection, id).unwrap()[0].settled);
            drop(db);
            let mut db = match TrackingStore::open(directory.path(), scope()).unwrap() {
                OpenOutcome::Opened(db) => db, _ => panic!("SQL remains physically valid"),
            };
            let changes = db.connection.total_changes();
            assert!(db.install_runtime().is_err());
            assert_eq!(db.connection.total_changes(), changes);
            assert!(db.exchange_incomplete(id).unwrap());
        }
    }
}
