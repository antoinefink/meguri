//! U38's FFI-side Rust harness. U37 later connects the exported surface to
//! this same composition root; this test does not claim CoreBridge coverage.
use meguri_engine::{ChangeKind, ChangeSet, Event, EventKind, OpSource, Seq, notes, notetypes};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::rng::SeededRandom;
use meguri_storage::{ReadRows, ReaderRole, Table, space::StatvfsSpace};
use meguri_testkit::{
    exec::{BackupWitness, ExecError, Executor, MeguriExecutor, Session, clock::ScenarioClock},
    scenario,
};
use std::sync::{Arc, Mutex};

struct Audit {
    calls: usize,
    checks: Vec<(Seq, i64)>,
    events: Vec<Arc<Event>>,
}
static TESTKIT_AUDIT: Mutex<Option<Audit>> = Mutex::new(None);
fn capture(session: &mut Session<'_>) -> Result<(), ExecError> {
    let audit = Audit {
        calls: session.storage.backup_calls(),
        checks: session.storage.backup_checks(),
        events: session.storage.take_changes(),
    };
    *TESTKIT_AUDIT.lock().unwrap() = Some(audit);
    Ok(())
}
fn deletion_changes(audit: &Audit) -> Vec<Arc<ChangeSet>> {
    assert_eq!(audit.calls, 1);
    let [(before, cards)] = audit.checks.as_slice() else {
        panic!("expected one pre-delete backup witness");
    };
    assert_eq!(*cards, 1, "the hook sees the card before deletion");
    let mut changes = Vec::new();
    for event in &audit.events {
        if let EventKind::Changes(change) = &event.kind
            && change.decks.iter().any(|d| d.kind == ChangeKind::Removed)
        {
            assert!(event.seq > *before, "backup precedes the deletion commit");
            changes.push(change.clone());
        }
    }
    assert_eq!(changes.len(), 1);
    changes
}

#[test]
fn testkit_and_ffi_host_harness_call_the_backup_before_the_same_deletion_changes() {
    let dir = tempfile::tempdir().unwrap();
    let scenario_path = dir.path().join("conformance/scenarios/host/parity.yaml");
    std::fs::create_dir_all(scenario_path.parent().unwrap()).unwrap();
    std::fs::write(&scenario_path, "id: host/parity\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsetup:\n  decks: [Parity]\n  notes: [{notetype: Basic, deck: Parity, fields: [front, back]}]\nsteps:\n  - delete_decks: {decks: [Parity]}\n").unwrap();
    let loaded = scenario::load(&scenario_path).unwrap();
    let mut registry = MeguriExecutor::core_registry();
    registry.fixture_backup_witness();
    registry.record_changes();
    registry.finish(capture);
    MeguriExecutor::with(registry).run(&loaded).unwrap();
    let testkit = TESTKIT_AUDIT.lock().unwrap().take().unwrap();

    let collection_directory = dir.path().join("ffi");
    let backups = dir.path().join("backups");
    std::fs::create_dir_all(&collection_directory).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    let clock = Arc::new(ScenarioClock::new("UTC", "2030-01-15T10:00:00").unwrap());
    let random = Arc::new(SeededRandom::new(
        u64::from_str_radix(&loaded.sha256[..16], 16).unwrap(),
    ));
    meguri_engine::bootstrap::create(
        &collection_directory.join("collection.anki2"),
        clock.as_ref(),
        random.as_ref(),
    )
    .unwrap();
    let witness = Arc::new(BackupWitness::default());
    witness.record_checks();
    let config = HostConfig::new(
        collection_directory,
        backups,
        dir.path().join("app-group/widget.snapshot"),
        clock,
        random,
        Arc::new(StatvfsSpace),
    )
    .with_backup_for_testing(witness.clone());
    let host = CoreHost::open(config).unwrap();
    let collection = host.collection().unwrap();
    witness.bind(collection.clone());
    let subscription = host.engine().stream().subscribe();
    let deck = meguri_engine::decks::add(&collection, "Parity".into())
        .unwrap()
        .value
        .id;
    let kind = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        notetypes::read::by_name(&*reader, "Basic").unwrap().unwrap().id
    };
    notes::add(
        &collection,
        notes::AddNote {
            notetype_id: kind,
            fields: vec!["front".into(), "back".into()],
            tags: vec![],
            deck_id: deck,
            guid: None,
            update_add_defaults: true,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    meguri_engine::decks::delete(&collection, vec![deck], meguri_engine::decks::ConfirmedLosses::Any, host.backup()).unwrap();
    let mut events = Vec::new();
    while let Some(event) = subscription.try_recv() {
        events.push(event);
    }
    let ffi = Audit {
        calls: witness.calls(),
        checks: witness.checks(),
        events,
    };
    assert_eq!(deletion_changes(&testkit), deletion_changes(&ffi));
    let reader = collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.deck(deck).unwrap().is_none());
    assert!(reader.ids_after(Table::Cards, None, 100).unwrap().is_empty());
    drop(reader);
    host.close().unwrap();
}
