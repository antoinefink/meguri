use super::*;
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

fn host(root: &std::path::Path) -> Arc<meguri_host::CoreHost> {
    let collection = root.join("collection");
    std::fs::create_dir_all(&collection).unwrap();
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(151));
    meguri_engine::bootstrap::create(&collection.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
    let mut config = meguri_host::HostConfig::new(collection, root.join("backups"), root.join("widget"),
        clock, random, Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    Arc::new(meguri_host::CoreHost::open(config).unwrap())
}
fn owner(host: Arc<meguri_host::CoreHost>) -> Arc<ProviderOwner> {
    let generation = host.cloud_connection_generation().unwrap();
    Arc::new(ProviderOwner { host, generation })
}

#[test]
fn real_connection_handles_do_not_survive_endpoint_revisions_or_original_host_close() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let owner = owner(host.clone());
    let old = owner.clone().configure_meguri("https://first.synthetic.example/".into()).unwrap();
    let next = owner.clone().configure_meguri("https://second.synthetic.example/".into()).unwrap();
    assert!(old.information().is_err());
    assert!(old.start_login("synthetic.user".into(), "synthetic-password".into()).is_err());
    assert_eq!(next.information().unwrap().endpoint, "https://second.synthetic.example/");
    assert_eq!(owner.snapshot().unwrap().phase, ProviderPhase::LocalOnly);
    host.close().unwrap();
    assert!(next.information().is_err());
}

#[test]
fn exclusive_owner_refuses_handoff_without_changing_the_provider_document() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let owner = owner(host.clone());
    let before = std::fs::read(root.path().join("installation/provider.json")).unwrap();
    let exclusive = host.engine().reserve_collection_export_handoff().unwrap();
    assert!(owner.clone().begin_handoff(ProviderSelection::Meguri, false).is_err());
    assert_eq!(std::fs::read(root.path().join("installation/provider.json")).unwrap(), before);
    drop(exclusive);
    let handoff = owner.clone().begin_handoff(ProviderSelection::Meguri, false).unwrap();
    let pending = owner.snapshot().unwrap();
    assert_eq!(pending.selection, Some(ProviderSelection::LocalOnly));
    assert_eq!(pending.phase, ProviderPhase::HandoffRecovery);
    let exclusive = host.engine().reserve_collection_export_handoff().unwrap();
    assert!(host.close().is_err());
    assert!(owner.clone().finish_handoff(handoff.clone()).is_err());
    assert_eq!(owner.snapshot().unwrap().phase, ProviderPhase::HandoffRecovery);
    drop(exclusive);
    let committed = owner.clone().finish_handoff(handoff).unwrap();
    assert_eq!(committed.phase, ProviderPhase::SetupRequired);
    assert!(!committed.settings.unwrap().automatic);
    host.close().unwrap();
}
