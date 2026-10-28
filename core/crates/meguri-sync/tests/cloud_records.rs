//! K10.1-K10.3 stored facts and build ownership. No account-policy assertions.
use meguri_engine::device_state::PublicationId;
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::{RandomSource, SeededRandom},
};
use meguri_sync::cloud::*;
use std::{fs, path::Path};

fn scope(container: &str, environment: Environment) -> BuildScope {
    BuildScope {
        container: ContainerId::new(container).unwrap(),
        environment,
    }
}

fn clock(ms: i64) -> FixedClock {
    FixedClock::new(ms, UtcOffset::UTC)
}

fn facts(state: BindingState) -> BindingFacts {
    // Synthetic fixture of the engine's existing identity type, not a new
    // production identity generator. Only the engine creates publication ids.
    let random = SeededRandom::new(77);
    let publication: PublicationId =
        serde_json::from_value(serde_json::json!([random.next_u64(), random.next_u64()])).unwrap();
    BindingFacts {
        account: OpaqueBytes(b"private-account-fixture".to_vec()),
        epoch: EpochId([31; 16]),
        record_key: RecordKey([91; 32]),
        publication,
        state,
    }
}

fn open(directory: &Path, scope: BuildScope) -> TrackingStore {
    match TrackingStore::open(directory, scope).unwrap() {
        OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
        _ => panic!("expected supported tracking file"),
    }
}

fn record() -> InboxEntry {
    InboxEntry {
        record_type: RecordType::Entity,
        record_name: "e-synthetic".into(),
        value: InboxValue::Record {
            erev: u64::MAX,
            system_fields: OpaqueBytes(vec![0, 255, 4]),
            envelope: OpaqueBytes(b"opaque-private-envelope".to_vec()),
            staged_asset: Some("staging/synthetic-asset".into()),
        },
        reason: InboxReason::Asset,
        source: InboxSource::Fetch,
        absence_pass: Some([17; 16]),
        retry_count: 3,
        fetched_at_ms: 800,
    }
}

#[test]
fn exact_binding_inbox_and_sdk_state_survive_reopen_without_collection_writes() {
    let directory = tempfile::tempdir().unwrap();
    let collection = directory.path().join("collection.anki2");
    let sentinel = b"collection bytes owned exclusively by the engine";
    fs::write(&collection, sentinel).unwrap();
    let build = scope("test.container", Environment::Development);
    let random = SeededRandom::new(123);
    let expected_random = SeededRandom::new(123);
    let mut expected_id = [0; 16];
    expected_id[..8].copy_from_slice(&expected_random.next_u64().to_be_bytes());
    expected_id[8..].copy_from_slice(&expected_random.next_u64().to_be_bytes());
    let mut store = open(directory.path(), build.clone());
    let binding = store
        .create_binding(facts(BindingState::Active), &clock(700), &random)
        .unwrap();
    assert_eq!(binding.id, BindingId(expected_id));
    assert_eq!(binding.created_at_ms, 700);
    assert_eq!(store.engine_state(binding.id).unwrap(), None);
    assert!(store.inbox(binding.id).unwrap().is_empty());
    let entry = record();
    let mut deletion = entry.clone();
    deletion.value = InboxValue::Deletion;
    deletion.reason = InboxReason::Ready;
    deletion.absence_pass = None;
    deletion.retry_count = 0;
    let cursor = OpaqueBytes(vec![255, 0, 31, 99]);
    let entries = [entry, deletion];
    store
        .commit_fetched(binding.id, &entries, &cursor, &clock(900))
        .unwrap();
    let committed_inbox = store.inbox(binding.id).unwrap();
    // Exact event replay retains ids and never doubles the durable entries.
    store
        .commit_fetched(binding.id, &entries, &cursor, &clock(900))
        .unwrap();
    assert_eq!(store.inbox(binding.id).unwrap(), committed_inbox);
    drop(store);

    let store = open(directory.path(), build);
    assert_eq!(store.binding(binding.id).unwrap(), Some(binding.clone()));
    assert_eq!(store.selected_binding().unwrap(), Some(binding.clone()));
    assert_eq!(store.inbox(binding.id).unwrap(), committed_inbox);
    assert_eq!(
        committed_inbox.iter().map(|row| row.entry.clone()).collect::<Vec<_>>(),
        entries
    );
    assert_eq!(
        store.engine_state(binding.id).unwrap(),
        Some(EngineState {
            serialization: cursor,
            saved_at_ms: 900,
        })
    );
    assert_eq!(fs::read(&collection).unwrap(), sentinel);
    assert!(!directory.path().join("collection.anki2-wal").exists());
    assert!(!directory.path().join("collection.anki2-shm").exists());
    assert!(!format!("{binding:?}").contains("private-account-fixture"));
    assert!(!format!("{:?}", entries[0]).contains("opaque-private-envelope"));
}

#[test]
fn builds_share_a_file_but_cannot_read_or_write_each_others_facts() {
    let directory = tempfile::tempdir().unwrap();
    let build_a = scope("test.container", Environment::Development);
    let build_b = scope("test.container", Environment::Production);
    let build_c = scope("other.container", Environment::Development);
    let random = SeededRandom::new(9);
    let mut a = open(directory.path(), build_a.clone());
    let mut b = open(directory.path(), build_b.clone());
    let mut c = open(directory.path(), build_c.clone());
    let a_binding = a
        .create_binding(facts(BindingState::Active), &clock(10), &random)
        .unwrap();
    let b_binding = b
        .create_binding(facts(BindingState::Active), &clock(20), &random)
        .unwrap();
    let c_binding = c
        .create_binding(facts(BindingState::Active), &clock(30), &random)
        .unwrap();
    for (store, own, foreign) in [
        (&mut a, &a_binding, &b_binding),
        (&mut b, &b_binding, &a_binding),
        (&mut c, &c_binding, &a_binding),
    ] {
        assert_eq!(store.binding(foreign.id).unwrap(), None);
        assert_eq!(store.engine_state(foreign.id), Err(Error::BindingNotFound));
        assert_eq!(store.inbox(foreign.id), Err(Error::BindingNotFound));
        assert_eq!(
            store.commit_fetched(foreign.id, &[record()], &OpaqueBytes(vec![8]), &clock(40)),
            Err(Error::BindingNotFound)
        );
        store
            .commit_fetched(own.id, &[record()], &OpaqueBytes(vec![own.id.0[0]]), &clock(50))
            .unwrap();
        assert_eq!(store.selected_binding().unwrap(), Some(own.clone()));
    }
    drop((a, b, c));
    for (scope, binding) in [(build_a, a_binding), (build_b, b_binding), (build_c, c_binding)] {
        let store = open(directory.path(), scope);
        assert_eq!(store.binding(binding.id).unwrap(), Some(binding.clone()));
        assert_eq!(store.inbox(binding.id).unwrap().len(), 1);
        assert_eq!(
            store.engine_state(binding.id).unwrap().unwrap().serialization,
            OpaqueBytes(vec![binding.id.0[0]])
        );
    }
}

#[test]
fn database_enforces_one_active_and_selection_prefers_it_over_newer_isolated() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Development);
    let mut first = open(directory.path(), build.clone());
    let mut second = open(directory.path(), build.clone());
    let random = SeededRandom::new(19);
    let active = first
        .create_binding(facts(BindingState::Active), &clock(10), &random)
        .unwrap();
    assert_eq!(
        second.create_binding(facts(BindingState::Active), &clock(20), &random),
        Err(Error::Constraint)
    );
    let isolated = second
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::AccountChanged)),
            &clock(30),
            &random,
        )
        .unwrap();
    assert_eq!(first.selected_binding().unwrap(), Some(active.clone()));
    assert_eq!(
        second.commit_fetched(isolated.id, &[record()], &OpaqueBytes(vec![9]), &clock(40)),
        Err(Error::IsolatedBinding)
    );
    drop((first, second));
    let store = open(directory.path(), build);
    assert_eq!(store.selected_binding().unwrap(), Some(active));
    assert_eq!(store.binding(isolated.id).unwrap(), Some(isolated.clone()));
    assert_eq!(store.engine_state(isolated.id).unwrap(), None);
    assert!(store.inbox(isolated.id).unwrap().is_empty());
}

#[test]
fn without_active_selection_uses_newest_and_never_a_salvage_binding() {
    let directory = tempfile::tempdir().unwrap();
    let build = scope("test.container", Environment::Production);
    let random = SeededRandom::new(29);
    let mut store = open(directory.path(), build.clone());
    store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::SignedOut)),
            &clock(10),
            &random,
        )
        .unwrap();
    let newest = store
        .create_binding(
            facts(BindingState::Isolated(IsolationReason::Stopped)),
            &clock(20),
            &random,
        )
        .unwrap();
    drop(store);
    // Fixture of a future salvage's output; this slice performs no salvage.
    let connection = rusqlite::Connection::open(directory.path().join(TRACKING_FILE)).unwrap();
    connection
        .execute(
            "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES (?1,?2,1,999,?3)",
            rusqlite::params![
                [55_u8; 16].as_slice(),
                build.container.as_str(),
                serde_json::to_string(&facts(BindingState::Isolated(IsolationReason::TrackingDamaged))).unwrap()
            ],
        )
        .unwrap();
    drop(connection);
    let mut store = open(directory.path(), build);
    assert_eq!(store.selected_binding().unwrap(), Some(newest));
    assert_eq!(
        store.create_binding(
            facts(BindingState::Isolated(IsolationReason::TrackingDamaged)),
            &clock(1000),
            &random
        ),
        Err(Error::InvalidData)
    );
}
