#![cfg(feature = "test-hooks")]
//! Offline policy/admission checks only. No TLS handshake or login acceptance.
use std::{path::Path, sync::Arc};
use meguri_engine::{EngineError, Generation, LongOp};
use meguri_host::{CoreHost, HostConfig, MeguriAccess, MeguriBootstrapError, ProcessCore};
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
use meguri_sync::{HostKey, provider::{AuthenticationFailure, MeguriConnection, Selection}};
use meguri_testkit::sync_stub::TestRoot;

const ORIGIN: &str = "https://127.0.0.1:18443/";
const OTHER: &str = "https://127.0.0.1:18444/";

fn config(root: &Path, create: bool) -> HostConfig {
    let directory = root.join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let mut config = HostConfig::new(directory, root.join("backups"), root.join("widget"),
        Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(1967)), Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    if create {
        meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
            config.clock.as_ref(), config.random.as_ref()).unwrap();
    }
    config
}
fn install(host: &CoreHost, config: &HostConfig, der: &[u8]) -> Result<(), EngineError> {
    host.install_meguri_fixture_transport(host.cloud_connection_generation()?,
        &config.collection_directory.canonicalize().unwrap().join("collection.anki2"),
        &config.manual_transfer_directory, ORIGIN, der)
}
fn access(host: &CoreHost, connection: &MeguriConnection) -> Result<MeguriAccess, MeguriBootstrapError> {
    // An inert input to factory construction, never an authenticated result.
    host.meguri_access_for_testing(connection.clone(), "synthetic-user", HostKey::new("unused-synthetic-input"))
}
fn connection(endpoint: &str) -> MeguriConnection {
    MeguriConnection { reference: "unconfigured-synthetic-input".into(), revision: 1, endpoint: endpoint.into() }
}

#[test]
fn default_absent_preserves_ordinary_access_validation_and_authentication_guard() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    let before = host.provider_authority().snapshot().unwrap();
    // Ordinary access construction does not certify a configured connection.
    assert!(access(&host, &connection("https://synthetic.example/")).is_ok());
    assert!(matches!(host.meguri_access_for_testing(connection(ORIGIN), "bad name", HostKey::new("unused")),
        Err(MeguriBootstrapError::Username)));
    assert!(matches!(access(&host, &connection("http://127.0.0.1/")),
        Err(MeguriBootstrapError::ConnectionChanged)));
    assert!(matches!(host.authenticate_meguri_connection(&connection(ORIGIN), "synthetic-user", "unused",
        meguri_engine::CancelToken::new()), Err(AuthenticationFailure::ConnectionChanged)));
    assert_eq!(host.provider_authority().snapshot().unwrap(), before);
    assert!(host.engine().jobs().running().is_empty());
    host.close().unwrap();
}

#[test]
fn one_shot_admission_is_local_inert_and_not_serialized() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    let metadata = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&metadata).unwrap();
    install(&host, &config, &der).unwrap();
    assert!(matches!(install(&host, &config, &der), Err(EngineError::Busy { .. })));
    assert_eq!(std::fs::read(&metadata).unwrap(), before);
    assert!(host.engine().jobs().running().is_empty());
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    let configured = host.configure_meguri_connection(ORIGIN).unwrap();
    assert!(access(&host, &configured).is_ok());
    assert!(access(&host, &configured).is_ok()); // Fresh real clients, no single-use result state.
    assert!(access(&host, &connection(ORIGIN)).is_err()); // Reference/revision still matter.
    let subpath = host.configure_meguri_connection("https://127.0.0.1:18443/native/").unwrap();
    assert!(access(&host, &subpath).is_ok()); // The policy binds the origin, not a URL path.
    assert!(access(&host, &configured).is_err());
    host.close().unwrap();
}

#[test]
fn invalid_der_and_origins_leave_the_cell_available_for_valid_admission() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    for bad in [Vec::new(), vec![1, 2, 3], der[..der.len() / 2].to_vec(), vec![0; 64 * 1024 + 1]] {
        assert!(matches!(install(&host, &config, &bad), Err(EngineError::InvalidArgument { .. })));
    }
    let path = host.collection().unwrap().path().to_owned();
    let generation = host.cloud_connection_generation().unwrap();
    for origin in ["https://public.synthetic.example/", "https://8.8.8.8/", "http://127.0.0.1/",
        "not a URL", "https://127.0.0.1/subpath/", "https://user:secret@127.0.0.1/",
        "https://127.0.0.1/?query=1", "https://127.0.0.1/#fragment"] {
        assert!(matches!(host.install_meguri_fixture_transport(generation, &path,
            &config.manual_transfer_directory, origin, &der), Err(EngineError::InvalidArgument { .. })));
    }
    install(&host, &config, &der).unwrap();
    host.close().unwrap();
}

#[test]
fn exact_directories_and_current_generation_are_required_before_the_store() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    let path = host.collection().unwrap().path().to_owned();
    let generation = host.cloud_connection_generation().unwrap();
    assert!(matches!(host.install_meguri_fixture_transport(Generation(generation.0 + 1), &path,
        &config.manual_transfer_directory, ORIGIN, &der), Err(EngineError::StaleGeneration { .. })));
    assert!(matches!(host.install_meguri_fixture_transport(generation, &root.path().join("foreign.anki2"),
        &config.manual_transfer_directory, ORIGIN, &der), Err(EngineError::InvalidArgument { .. })));
    assert!(matches!(host.install_meguri_fixture_transport(generation, &path,
        &root.path().join("foreign-installation"), ORIGIN, &der), Err(EngineError::InvalidArgument { .. })));
    install(&host, &config, &der).unwrap();
    host.close().unwrap();
}

#[test]
fn absent_collection_and_occupied_lifecycle_slots_refuse_without_running_jobs() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), false);
    let process = ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.open_for_manual_import(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    assert!(install(&host, &config, &der).is_err());
    host.close().unwrap();
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(), config.random.as_ref()).unwrap();
    let host = process.open(config.clone()).unwrap();
    for op in [LongOp::OneWaySync, LongOp::Restore] {
        let slot = host.engine().acquire_slot(op).unwrap();
        assert!(host.engine().jobs().running().is_empty());
        assert!(install(&host, &config, &der).is_err());
        drop(slot);
    }
    install(&host, &config, &der).unwrap();
    host.close().unwrap();
}

#[test]
fn actual_registered_local_work_blocks_admission_until_it_drains() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    let (entered, entry) = std::sync::mpsc::channel();
    let (release, exit) = std::sync::mpsc::channel();
    let mut events = host.engine().stream().subscribe();
    let job = host.engine().jobs().start("synthetic local admission owner", host.engine().generation(), None,
        move |_| { entered.send(()).unwrap(); exit.recv().unwrap(); Ok(()) });
    entry.recv().unwrap();
    let refused = install(&host, &config, &der);
    release.send(()).unwrap();
    while let Some(event) = events.recv() {
        if matches!(event.kind, meguri_engine::EventKind::JobFinished { id, .. } if id == job.id) { break; }
    }
    assert!(matches!(refused, Err(EngineError::Busy { .. })));
    install(&host, &config, &der).unwrap();
    host.close().unwrap();
}

#[test]
fn configured_nonlocal_handoff_and_pending_removal_refuse() {
    for state in 0..4 {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path(), true);
        let host = CoreHost::open(config.clone()).unwrap();
        let der = TestRoot::generate().der();
        match state {
            0 => { host.configure_meguri_connection(ORIGIN).unwrap(); }
            1 => {
                let handoff = host.begin_provider_handoff(Selection::ICloud).unwrap();
                host.finish_provider_handoff(&handoff).unwrap();
                assert_eq!(host.provider_authority().snapshot().unwrap().selection, Selection::ICloud);
            }
            2 => { host.begin_provider_handoff(Selection::ICloud).unwrap(); }
            _ => { host.begin_provider_handoff_with_removal(Selection::ICloud, true).unwrap(); }
        }
        assert!(matches!(install(&host, &config, &der), Err(EngineError::Busy { .. })));
        host.close().unwrap();
    }
}

#[test]
fn present_policy_refuses_origin_connection_handoff_and_close_mismatches_before_authentication() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    install(&host, &config, &TestRoot::generate().der()).unwrap();
    let original = host.configure_meguri_connection(ORIGIN).unwrap();
    assert!(access(&host, &original).is_ok());
    let other = host.configure_meguri_connection(OTHER).unwrap();
    assert!(access(&host, &other).is_err());
    assert!(access(&host, &original).is_err());
    assert!(matches!(host.authenticate_meguri_connection(&other, "synthetic-user", "unused",
        meguri_engine::CancelToken::new()), Err(AuthenticationFailure::ConnectionChanged)));
    let original = host.configure_meguri_connection(ORIGIN).unwrap();
    let handoff = host.begin_provider_handoff(Selection::Meguri).unwrap();
    assert!(access(&host, &original).is_err());
    host.finish_provider_handoff(&handoff).unwrap();
    assert!(access(&host, &original).is_ok());
    host.close().unwrap();
    assert!(access(&host, &original).is_err());
}

#[test]
fn original_host_policy_survives_actual_replacement_generation_progress() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let host = CoreHost::open(config.clone()).unwrap();
    install(&host, &config, &TestRoot::generate().der()).unwrap();
    let connection = host.configure_meguri_connection(ORIGIN).unwrap();
    let generation = host.engine().generation();
    {
        let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
        slot.enter_exclusive(true).unwrap().replace(|live| {
            let candidate = live.with_file_name("synthetic-replacement.anki2");
            std::fs::copy(live, &candidate).map_err(meguri_storage::StorageError::from)?;
            std::fs::rename(candidate, live).map_err(meguri_storage::StorageError::from)?;
            Ok(())
        }).unwrap();
    }
    assert_ne!(host.engine().generation(), generation);
    assert!(access(&host, &connection).is_ok());
    // The same installed origin restriction also survived, not an absent fallback.
    let other = host.configure_meguri_connection(OTHER).unwrap();
    assert!(access(&host, &other).is_err());
    host.close().unwrap();
}

#[test]
fn closed_original_never_follows_a_successor_and_cold_local_reopen_needs_new_admission() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), true);
    let process = ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.open(config.clone()).unwrap();
    let der = TestRoot::generate().der();
    let path = host.collection().unwrap().path().to_owned();
    let generation = host.engine().generation();
    install(&host, &config, &der).unwrap();
    host.close().unwrap();
    assert!(host.install_meguri_fixture_transport(generation, &path,
        &config.manual_transfer_directory, ORIGIN, &der).is_err());
    let successor = process.open(config.clone()).unwrap();
    assert!(access(&successor, &connection(OTHER)).is_ok()); // Default absent.
    install(&successor, &config, &der).unwrap(); // New explicit one-shot admission.
    let current = successor.configure_meguri_connection(ORIGIN).unwrap();
    assert!(access(&successor, &current).is_ok());
    assert!(access(&host, &current).is_err()); // Same engine/path; original closed.
    successor.close().unwrap();
    let cold = CoreHost::open(config).unwrap();
    assert!(access(&cold, &connection(OTHER)).is_ok()); // Never serialized.
    cold.close().unwrap();
}

#[test]
fn foreign_host_cannot_use_another_hosts_directory_admission() {
    let root = tempfile::tempdir().unwrap();
    let config_a = config(&root.path().join("a"), true);
    let config_b = config(&root.path().join("b"), true);
    let a = CoreHost::open(config_a.clone()).unwrap();
    let b = CoreHost::open(config_b.clone()).unwrap();
    let der = TestRoot::generate().der();
    install(&a, &config_a, &der).unwrap();
    assert!(install(&b, &config_a, &der).is_err());
    assert!(access(&b, &connection(OTHER)).is_ok());
    install(&b, &config_b, &der).unwrap();
    let connection_b = b.configure_meguri_connection(ORIGIN).unwrap();
    assert!(access(&a, &connection_b).is_err());
    assert!(access(&b, &connection_b).is_ok());
    a.close().unwrap();
    b.close().unwrap();
}
