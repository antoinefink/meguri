use super::*;
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

fn unopened_configuration(root: &std::path::Path) -> meguri_host::HostConfig {
    let collection = root.join("collection");
    std::fs::create_dir_all(&collection).unwrap();
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(151));
    let mut config = meguri_host::HostConfig::new(collection, root.join("backups"), root.join("widget"),
        clock, random, Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    config
}
fn configuration(root: &std::path::Path) -> meguri_host::HostConfig {
    let config = unopened_configuration(root);
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
    config
}
fn host(root: &std::path::Path) -> Arc<meguri_host::CoreHost> {
    Arc::new(meguri_host::CoreHost::open(configuration(root)).unwrap())
}
fn owner(host: Arc<meguri_host::CoreHost>) -> Arc<ProviderOwner> {
    let generation = host.cloud_connection_generation().unwrap();
    Arc::new(ProviderOwner { host, generation })
}

fn assert_absent_collection_without_provider_authority(host: &meguri_host::CoreHost, config: &meguri_host::HostConfig) {
    assert_eq!(host.engine().state(), meguri_engine::LifecycleState::NoCollection);
    assert!(host.collection().is_err());
    assert!(!config.collection_directory.join("collection.anki2").exists());
    assert!(!config.collection_directory.join("meguri-sidecar.json").exists());
    assert_eq!(meguri_engine::device_state::DeviceState::for_collection(
        &config.collection_directory.join("collection.anki2")).settled_publication().unwrap(), None);
    let doc = host.provider_authority().snapshot().unwrap();
    assert_eq!(doc.selection, p::Selection::LocalOnly);
    assert!(doc.binding.is_none() && doc.handoff.is_none());
    assert!(!doc.permits_meguri_transfer());
    assert!(!host.provider_authority().permits_icloud_account_read());
}

#[test]
fn ffi_prepared_owner_configures_and_saves_settings_without_creating_a_collection_or_binding() {
    let root = tempfile::tempdir().unwrap();
    let config = unopened_configuration(root.path());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = Arc::new(process.prepare_cloud_connection(config.clone()).unwrap());
    let original = owner(host.clone());
    let connection = original.clone().configure_meguri("https://first-setup.synthetic.example/".into()).unwrap();
    let information = connection.information().unwrap();
    let mut settings = original.snapshot().unwrap().settings.unwrap();
    settings.enabled = true;
    settings.paused = true;
    let saved = original.save_settings(settings).unwrap();
    assert_eq!(saved.phase, ProviderPhase::LocalOnly);
    assert_eq!(saved.settings, Some(settings));
    assert_absent_collection_without_provider_authority(&host, &config);
    let path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&path).unwrap();
    host.close().unwrap();
    let successor = Arc::new(process.prepare_cloud_connection(config.clone()).unwrap());
    let current = owner(successor.clone());
    assert_eq!(current.generation, original.generation);
    let retained = current.clone().configured_meguri().unwrap().unwrap().information().unwrap();
    assert_eq!(retained.reference, information.reference);
    assert_eq!(retained.revision, information.revision);
    assert_eq!(retained.endpoint, information.endpoint);
    assert_eq!(current.snapshot().unwrap().settings, Some(settings));
    assert!(connection.information().is_err());
    assert!(original.clone().configure_meguri("https://stale.synthetic.example/".into()).is_err());
    assert!(original.save_settings(settings).is_err());
    assert!(original.retry_failed_publication().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_absent_collection_without_provider_authority(&successor, &config);
    successor.close().unwrap();
}

#[test]
fn ffi_prepared_metadata_retry_retains_failed_desire_error_and_none_publication_through_lifecycle_contention() {
    use meguri_model::faultpoint::{self, Fault};
    for configure in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let config = unopened_configuration(root.path());
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let host = Arc::new(process.prepare_cloud_connection(config.clone()).unwrap());
        let original = owner(host.clone());
        let mut settings = original.snapshot().unwrap().settings.unwrap();
        settings.paused = true;
        let path = config.manual_transfer_directory.join("provider.json");
        let before = std::fs::read(&path).unwrap();
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
        if configure {
            assert!(original.clone().configure_meguri("https://retained-setup.synthetic.example/".into()).is_err());
        } else { assert!(original.save_settings(settings).is_err()); }
        faultpoint::disarm("storage.sidecar.before_save");
        let retained_error = host.provider_authority().snapshot().unwrap_err();
        assert_eq!(original.refresh().unwrap().phase, ProviderPhase::Blocked);
        assert!(original.clone().configure_meguri("https://unrelated.synthetic.example/".into()).is_err());
        assert!(original.save_settings(settings).is_err());
        host.engine().close_with_host_cleanup(|collection| {
            assert!(collection.is_none());
            assert!(original.retry_failed_publication().is_err());
            assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
            assert_eq!(std::fs::read(&path).unwrap(), before);
            Ok(())
        }).unwrap();
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
        assert!(original.retry_failed_publication().is_err());
        faultpoint::disarm("storage.sidecar.before_save");
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert_eq!(original.refresh().unwrap().phase, ProviderPhase::Blocked);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        let retried = original.retry_failed_publication().unwrap();
        assert_eq!(retried.phase, ProviderPhase::LocalOnly);
        if configure {
            assert_eq!(original.clone().configured_meguri().unwrap().unwrap().information().unwrap().endpoint,
                "https://retained-setup.synthetic.example/");
        } else { assert_eq!(retried.settings, Some(settings)); }
        assert_eq!(host.cloud_connection_generation().unwrap(), original.generation);
        assert_absent_collection_without_provider_authority(&host, &config);

        // A second actual failure must not publish through this facade after
        // close, even when a successor owns the same installation/generation.
        settings.automatic = false;
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
        assert!(original.save_settings(settings).is_err());
        faultpoint::disarm("storage.sidecar.before_save");
        let retained_error = host.provider_authority().snapshot().unwrap_err();
        host.close().unwrap();
        let successor = Arc::new(process.prepare_cloud_connection(config.clone()).unwrap());
        let current = owner(successor.clone());
        current.clone().configure_meguri("https://successor.synthetic.example/".into()).unwrap();
        let before = std::fs::read(&path).unwrap();
        assert_eq!(current.generation, original.generation);
        assert!(original.retry_failed_publication().is_err());
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert!(host.provider_authority().network_blocked());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        assert_absent_collection_without_provider_authority(&successor, &config);
        successor.close().unwrap();
    }
}

#[test]
fn ffi_original_generation_cannot_publish_or_retry_metadata_after_actual_collection_replacement() {
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let original = owner(host.clone());
    let mut settings = original.snapshot().unwrap().settings.unwrap();
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(original.save_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    let replacement_root = tempfile::tempdir().unwrap();
    let replacement = configuration(replacement_root.path()).collection_directory.join("collection.anki2");
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.enter_exclusive(false).unwrap().replace(move |live| {
        std::fs::rename(&replacement, live).map_err(meguri_storage::StorageError::from)?;
        Ok(())
    }).unwrap();
    drop(slot);
    assert_ne!(host.cloud_connection_generation().unwrap(), original.generation);
    let path = root.path().join("installation/provider.json");
    let before = std::fs::read(&path).unwrap();
    assert!(original.clone().configure_meguri("https://stale.synthetic.example/".into()).is_err());
    assert!(original.save_settings(settings).is_err());
    assert!(original.retry_failed_publication().is_err());
    // Also exercise the guarded mutation itself, after a caller has already
    // captured its generation, independently of the facade's early check.
    assert!(host.configure_meguri_connection_for_generation(original.generation, "https://stale.synthetic.example/").is_err());
    assert!(host.save_provider_settings_for_generation(original.generation, settings.into()).is_err());
    assert!(host.retry_provider_publication_for_generation(original.generation).is_err());
    assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
    assert!(host.provider_authority().network_blocked());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    host.close().unwrap();
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

#[test]
fn lost_ffi_handoff_recovers_on_a_new_host_and_old_owner_cannot_commit_or_edit_successor() {
    let root = tempfile::tempdir().unwrap();
    let config = configuration(root.path());
    let original_host = Arc::new(meguri_host::CoreHost::open(config.clone()).unwrap());
    let original_owner = owner(original_host.clone());
    let lost = original_owner.clone().begin_handoff(ProviderSelection::Meguri, false).unwrap();
    drop(lost);
    original_host.close().unwrap();
    drop(original_owner);
    drop(original_host);
    let next_host = Arc::new(meguri_host::CoreHost::open(config.clone()).unwrap());
    let next_owner = owner(next_host.clone());
    assert_eq!(next_owner.refresh().unwrap().phase, ProviderPhase::HandoffRecovery);
    assert_eq!(next_owner.recover_handoff_to_local_only().unwrap().phase, ProviderPhase::LocalOnly);
    assert!(!next_host.provider_authority().permits_icloud_account_read());
    let stale = next_owner.clone().begin_handoff(ProviderSelection::Meguri, false).unwrap();
    next_host.close().unwrap();
    let successor = Arc::new(meguri_host::CoreHost::open(config).unwrap());
    let successor_owner = owner(successor.clone());
    successor_owner.recover_handoff_to_local_only().unwrap();
    let before = std::fs::read(root.path().join("installation/provider.json")).unwrap();
    assert!(next_owner.clone().finish_handoff(stale).is_err());
    assert!(next_owner.recover_handoff_to_local_only().is_err());
    assert!(next_owner.retry_failed_publication().is_err());
    assert_eq!(std::fs::read(root.path().join("installation/provider.json")).unwrap(), before);
    successor.close().unwrap();
}

#[test]
fn ffi_failed_pause_refresh_stays_blocked_and_explicit_retry_publishes_the_pause() {
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let owner = owner(host.clone());
    let collection = host.collection().unwrap();
    let deck = meguri_engine::decks::add(&collection, "FFI provider review".into()).unwrap().value.id;
    let kind = meguri_engine::notetypes::add_stock(&collection,
        meguri_engine::notetypes::StockKind::Basic, Some("FFI provider review".into())).unwrap().value.id;
    meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
        notetype_id: kind, fields: vec!["FFI provider front".into(), "FFI provider back".into()],
        tags: vec![], deck_id: deck, guid: None, update_add_defaults: false,
    }, meguri_engine::OpSource::UserUndoable).unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    assert_eq!(meguri_engine::sched::fsrs_policy::policy_status(&collection).unwrap(),
        meguri_engine::sched::fsrs_policy::PolicyStatus::Ok);
    let (session_id, head) = match host.review().start(deck, None).unwrap() {
        meguri_engine::session::StartResult::Started { session_id, head, .. } => (session_id, head),
        actual => panic!("FFI provider fixture must start review: {actual:?}"),
    };
    assert_eq!(head.items.len(), 1);
    let before = std::fs::read(root.path().join("installation/provider.json")).unwrap();
    let mut settings = owner.snapshot().unwrap().settings.unwrap();
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(owner.save_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    assert_eq!(owner.refresh().unwrap().phase, ProviderPhase::Blocked);
    assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
    assert_eq!(std::fs::read(root.path().join("installation/provider.json")).unwrap(), before);
    assert_eq!(host.review().active_session(), Some(session_id));
    assert_eq!(host.review().head(session_id, 1).unwrap(), head);
    assert!(owner.clone().configure_meguri("https://unrelated.synthetic.example/".into()).is_err());
    let retried = owner.retry_failed_publication().unwrap();
    assert!(retried.settings.unwrap().paused);
    assert_eq!(retried.phase, ProviderPhase::LocalOnly);
    assert_eq!(host.review().active_session(), Some(session_id));
    assert_eq!(host.review().head(session_id, 1).unwrap(), head);
    assert!(!host.provider_authority().permits_icloud_account_read());
    assert!(!host.provider_authority().snapshot().unwrap().permits_meguri_transfer());
    host.review().end(session_id).unwrap();
    host.close().unwrap();
}
