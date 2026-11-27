use super::*;
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};

#[path = "deletion_admission_tests.rs"]
mod deletion_admission;

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

#[test]
fn application_ffi_entry_routes_retire_prior_icloud_and_keep_malformed_metadata_blocked() {
    use crate::collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock};
    use crate::collection_entry::{self, CollectionActivationOutcome, CollectionInspectOutcome};
    use crate::manual_transfers::{ManualTransferOutcome, ManualTransferSession, ManualTransferValue};
    use meguri_engine::{EventKind, JobId, JobOutcome};
    use meguri_sync::provider::{LegacySettings, ProviderDocument, Selection};
    struct Clock;
    impl HostClock for Clock {
        fn now_ms(&self) -> i64 { 1_894_701_600_000 }
        fn offset_from_utc(&self, _: i64) -> i32 { 0 }
        fn zone_id(&self) -> String { "UTC".into() }
    }
    struct Space;
    impl DiskSpaceProvider for Space {
        fn available_bytes(&self, _: String) -> DiskCapacity { DiskCapacity::Available { bytes: u64::MAX } }
    }
    let events = runtime::engine().unwrap().stream().subscribe();
    let wait = |id: u64| loop {
        let event = events.recv_timeout(std::time::Duration::from_secs(20)).unwrap();
        if let EventKind::JobFinished { id: actual, outcome } = &event.kind && *actual == JobId(id) {
            assert_eq!(outcome, &JobOutcome::Succeeded);
            break;
        }
    };
    // Ordinary open, direct creation, retained preparation/creation, initial
    // activation, and offline manual setup all enter through FFI reservations.
    for entry in 0..5 {
        let root = tempfile::tempdir().unwrap();
        let config = if entry == 0 { configuration(root.path()) } else { unopened_configuration(root.path()) };
        let fixture = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../../conformance/fixtures/storage/schema-18.anki2");
        if entry == 4 {
            // Manual setup opens an existing public synthetic collection, with
            // no login, account inputs, endpoint or server connection.
            std::fs::copy(&fixture, config.collection_directory.join("collection.anki2")).unwrap();
        }
        std::fs::create_dir_all(&config.manual_transfer_directory).unwrap();
        let path = config.manual_transfer_directory.join("provider.json");
        let mut prior = serde_json::to_value(ProviderDocument::migration(LegacySettings {
            enabled: true, paused: false, automatic: true, media: true,
        })).unwrap();
        prior["future"] = serde_json::json!({"retained":true});
        std::fs::write(&path, serde_json::to_vec(&prior).unwrap()).unwrap();
        let location = CollectionLocation {
            collection_directory: config.collection_directory.to_str().unwrap().into(),
            backups_directory: config.backups_directory.to_str().unwrap().into(),
            app_group_snapshot: config.app_group_snapshot.to_str().unwrap().into(),
            manual_transfer_directory: config.manual_transfer_directory.to_str().unwrap().into(),
        };
        match entry {
            0 => wait(collection::start_open(location, Arc::new(Clock), Arc::new(Space), 151).unwrap().id),
            1 => wait(collection::start_create_empty(location, Arc::new(Clock), Arc::new(Space), 151).unwrap().id),
            2 => {
                collection::prepare_provider_host(location.clone(), Arc::new(Clock), Arc::new(Space), 151).unwrap();
                assert!(!config.collection_directory.join("collection.anki2").exists());
                let prepared = runtime::host().unwrap().provider_authority().snapshot().unwrap();
                assert_eq!(prepared.selection, Selection::LocalOnly);
                assert!(!prepared.settings.enabled);
                wait(collection::start_create_empty(location, Arc::new(Clock), Arc::new(Space), 151).unwrap().id);
            }
            3 => {
                let source = root.path().join("input.colpkg");
                meguri_package::write_collection_package_from_standalone(&fixture, &source,
                    &meguri_package::CollectionPackageWriteContext {
                        collection_directory: &config.collection_directory,
                        purpose: meguri_storage::atomic_file::Purpose::Export,
                        finalisation: meguri_package::Finalisation::ExclusiveCreate,
                        clock: config.clock.as_ref(), random: config.random.as_ref(),
                        space: config.disk_space_provider.as_ref(), is_cancelled: &|| false,
                    }).unwrap();
                let inspected = collection_entry::start_collection_inspection(source.to_str().unwrap().into(),
                    location, Arc::new(Clock), Arc::new(Space), 151).unwrap();
                wait(inspected.job.id);
                let CollectionInspectOutcome::Ready { inspection } = inspected.result.outcome().unwrap() else {
                    panic!("initial inspection must be ready")
                };
                let activated = collection_entry::start_collection_activation(inspection).unwrap();
                wait(activated.job.id);
                assert!(matches!(activated.result.outcome().unwrap(), CollectionActivationOutcome::Activated { .. }));
            }
            _ => {
                assert!(config.collection_directory.join("collection.anki2").is_file());
                let session = ManualTransferSession::new(location, Arc::new(Clock), Arc::new(Space), 151).unwrap();
                let setup = session.start_setup().unwrap();
                wait(setup.job.id);
                assert!(matches!(setup.result.outcome().unwrap(),
                    ManualTransferOutcome::Value { value: ManualTransferValue::Ready }));
            }
        }
        let host = runtime::host().unwrap();
        assert!(host.collection().is_ok());
        assert!(host.cloud_runtime().is_none());
        let authority = host.provider_authority();
        let document = authority.snapshot().unwrap();
        assert_eq!(document.selection, Selection::LocalOnly);
        assert!(!document.settings.enabled && document.handoff.is_none());
        assert!(!document.permits_meguri_transfer() && !authority.permits_icloud_account_read());
        let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(after["retiredICloud"], prior);
        assert_eq!(after["future"], prior["future"]);
        drop(host);
        crate::lifecycle::close_collection().unwrap();
    }
    // Only the exact historical failure marker may recover through application
    // startup. Malformed metadata stays byte-for-byte intact and fails closed.
    for (bytes, blocked) in [
        ("null", true),
        ("{}", true),
        (r#"{"migrationFailure":"invalidLegacySettings"}"#, false),
        (r#"{"migrationFailure":"invalidLegacySettings","selection":"Meguri"}"#, true),
    ] {
        let root = tempfile::tempdir().unwrap();
        let config = configuration(root.path());
        std::fs::create_dir_all(&config.manual_transfer_directory).unwrap();
        let path = config.manual_transfer_directory.join("provider.json");
        std::fs::write(&path, bytes).unwrap();
        let location = CollectionLocation {
            collection_directory: config.collection_directory.to_str().unwrap().into(),
            backups_directory: config.backups_directory.to_str().unwrap().into(),
            app_group_snapshot: config.app_group_snapshot.to_str().unwrap().into(),
            manual_transfer_directory: config.manual_transfer_directory.to_str().unwrap().into(),
        };
        wait(collection::start_open(location, Arc::new(Clock), Arc::new(Space), 151).unwrap().id);
        let host = runtime::host().unwrap();
        assert!(host.collection().is_ok());
        assert!(host.cloud_runtime().is_none());
        let authority = host.provider_authority();
        assert_eq!(authority.snapshot().is_err(), blocked);
        assert_eq!(authority.network_blocked(), blocked);
        assert!(!authority.permits_icloud_account_read());
        if blocked { assert_eq!(std::fs::read(&path).unwrap(), bytes.as_bytes()); }
        else { assert_eq!(authority.snapshot().unwrap().selection, Selection::LocalOnly); }
        drop(host);
        crate::lifecycle::close_collection().unwrap();
    }
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

/// The first-run fix narrows only the Meguri setup transfer's closure. Any
/// other closed admission, with Meguri Sync selected and its document
/// readable, still reads as MP1's blocked phase.
#[test]
fn ffi_closed_admission_without_a_setup_transfer_stays_blocked_for_meguri() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let owner = owner(host.clone());
    owner.clone().configure_meguri("https://first-run.synthetic.example/".into()).unwrap();
    let handoff = owner.clone().begin_handoff(ProviderSelection::Meguri, false).unwrap();
    assert_eq!(owner.clone().finish_handoff(handoff).unwrap().phase, ProviderPhase::SetupRequired);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    host.provider_authority().close_admission();
    assert!(!host.meguri_setup_transfer_holds_admission());
    let closed = owner.snapshot().unwrap();
    assert_eq!(closed.selection, Some(ProviderSelection::Meguri));
    assert_eq!(closed.phase, ProviderPhase::Blocked);
    assert_eq!(owner.refresh().unwrap().phase, ProviderPhase::SetupRequired);
    host.close().unwrap();
}

/// The phase mapping itself: the setup transfer's closure reports the
/// document's phase; unreadable metadata and any other closure stay Blocked.
#[test]
fn ffi_setup_transfer_closure_maps_to_the_document_phase_only() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let owner = owner(host.clone());
    owner.clone().configure_meguri("https://first-run.synthetic.example/".into()).unwrap();
    let handoff = owner.clone().begin_handoff(ProviderSelection::Meguri, false).unwrap();
    owner.clone().finish_handoff(handoff).unwrap();
    let doc = || host.provider_authority().snapshot();
    let setup = admitted_snapshot(doc(), true, true);
    assert_eq!((setup.selection, setup.phase), (Some(ProviderSelection::Meguri), ProviderPhase::SetupRequired));
    assert!(setup.settings.is_some());
    assert_eq!(admitted_snapshot(doc(), true, false).phase, ProviderPhase::Blocked);
    assert_eq!(admitted_snapshot(doc(), false, false).phase, ProviderPhase::SetupRequired);
    let unreadable = admitted_snapshot(Err(meguri_storage::StorageError::DestinationExists), true, true);
    assert_eq!((unreadable.selection, unreadable.phase), (None, ProviderPhase::Blocked));
    assert_eq!(admitted_snapshot(Err(meguri_storage::StorageError::DestinationExists), false, false).phase,
        ProviderPhase::Blocked);
    host.close().unwrap();
}

#[cfg(feature = "test-hooks")]
mod fixture_transport_tests {
    use super::*;
    use super::super::{bootstrap::{ProviderBootstrapDirection, ProviderBootstrapFailure,
        ProviderBootstrapOutcome, ProviderBootstrapReadiness}, normal::{ProviderRoundRefusal, ProviderRoundStart}};
    use std::path::Path;

    const ORIGIN: &str = "https://127.0.0.1:18443/";
    const OTHER_ORIGIN: &str = "https://127.0.0.1:18444/";

    fn install(owner: &ProviderOwner, root: &Path, der: &[u8]) -> Result<(), CoreError> {
        owner.install_meguri_fixture_transport_for_testing(
            root.join("collection/collection.anki2").to_str().unwrap().into(),
            root.join("installation").to_str().unwrap().into(), ORIGIN.into(), der.to_vec())
    }

    // Inert constructor/refusal inputs only; no authentication is performed.
    fn credentials(connection: &ProviderConnection) -> ProviderCredentials {
        ProviderCredentials { connection: connection.information().unwrap(),
            username: "synthetic-user".into(), hostkey: "inert-constructor-input".into() }
    }

    fn assert_idle(host: &meguri_host::CoreHost) {
        assert!(host.engine().jobs().running().is_empty());
        assert!(host.manual_transfer_history().active().unwrap().is_none());
    }

    fn bootstrap_refusal(connection: &ProviderConnection, credentials: ProviderCredentials) -> ProviderBootstrapFailure {
        let job = connection.start_bootstrap_prepare(credentials, ProviderBootstrapDirection::UseServerCollection).unwrap();
        assert!(job.handle.is_none());
        assert!(job.result.retained_operation().unwrap().is_none());
        assert_idle(&connection.owner.host);
        match job.result.outcome().unwrap() {
            ProviderBootstrapOutcome::Refused { failure, accounting: None } => failure,
            _ => panic!("expected synchronous bootstrap refusal without accounting"),
        }
    }

    fn assert_round_refusals(connection: &ProviderConnection, credentials: ProviderCredentials, expected: ProviderRoundRefusal) {
        match connection.start_collection_round(credentials.clone()).unwrap() {
            ProviderRoundStart::Refused { refusal } => assert_eq!(refusal, expected),
            _ => panic!("expected synchronous collection refusal without a handle"),
        }
        assert_idle(&connection.owner.host);
        match connection.start_media_round(credentials).unwrap() {
            ProviderRoundStart::Refused { refusal } => assert_eq!(refusal, expected),
            _ => panic!("expected synchronous media refusal without a handle"),
        }
        assert_idle(&connection.owner.host);
    }

    fn assert_not_selected(connection: &ProviderConnection) {
        assert_eq!(connection.owner.snapshot().unwrap().selection, Some(ProviderSelection::LocalOnly));
        assert!(matches!(connection.bootstrap_readiness().unwrap(), ProviderBootstrapReadiness::NotSelected));
        let credentials = credentials(connection);
        assert!(matches!(bootstrap_refusal(connection, credentials.clone()), ProviderBootstrapFailure::NotSelected));
        assert_round_refusals(connection, credentials, ProviderRoundRefusal::NotSelected);
    }

    fn assert_early_input_guards(connection: &ProviderConnection) {
        for field in 0..3 {
            let mut credentials = credentials(connection);
            match field {
                0 => credentials.connection.reference.push('x'),
                1 => credentials.connection.revision += 1,
                _ => credentials.connection.endpoint = "https://foreign.synthetic.example/".into(),
            }
            // Identity mismatches must win even over an invalid username.
            credentials.username = "!".into();
            assert!(matches!(bootstrap_refusal(connection, credentials.clone()), ProviderBootstrapFailure::ConnectionChanged));
            assert_round_refusals(connection, credentials, ProviderRoundRefusal::ConnectionChanged);
        }
        let mut credentials = credentials(connection);
        credentials.username = "!".into();
        assert!(matches!(bootstrap_refusal(connection, credentials.clone()), ProviderBootstrapFailure::Username));
        assert_round_refusals(connection, credentials, ProviderRoundRefusal::AccountChanged);
    }

    #[test]
    fn ffi_fixture_installer_binds_exact_paths_is_one_shot_and_does_not_survive_host_close() {
        let root = tempfile::tempdir().unwrap();
        let config = configuration(root.path());
        let host = Arc::new(meguri_host::CoreHost::open(config.clone()).unwrap());
        let original = owner(host.clone());
        let der = meguri_testkit::sync_stub::TestRoot::generate().der();

        assert!(install(&original, root.path(), b"invalid DER").is_err());
        assert!(original.install_meguri_fixture_transport_for_testing(
            config.collection_directory.to_str().unwrap().into(),
            config.manual_transfer_directory.to_str().unwrap().into(), ORIGIN.into(), der.clone()).is_err());
        assert!(original.install_meguri_fixture_transport_for_testing(
            config.collection_directory.join("collection.anki2").to_str().unwrap().into(),
            root.path().join("wrong-installation").to_str().unwrap().into(), ORIGIN.into(), der.clone()).is_err());
        assert_idle(&host);
        install(&original, root.path(), &der).unwrap();
        assert!(install(&original, root.path(), &der).is_err());

        let foreign_root = tempfile::tempdir().unwrap();
        let foreign_host = super::host(foreign_root.path());
        let foreign = owner(foreign_host.clone());
        assert!(install(&foreign, root.path(), &der).is_err());
        assert!(foreign.install_meguri_fixture_transport_for_testing(
            foreign_root.path().join("collection/collection.anki2").to_str().unwrap().into(),
            config.manual_transfer_directory.to_str().unwrap().into(), ORIGIN.into(), der.clone()).is_err());
        assert_idle(&foreign_host);
        install(&foreign, foreign_root.path(), &der).unwrap();
        foreign_host.close().unwrap();

        assert_idle(&host);
        host.close().unwrap();
        assert!(install(&original, root.path(), &der).is_err());
        // Reopening the very same installation still creates a new policy owner.
        let reopened = Arc::new(meguri_host::CoreHost::open(config).unwrap());
        let current = owner(reopened.clone());
        let connection = current.configure_meguri(OTHER_ORIGIN.into()).unwrap();
        assert_not_selected(&connection);
        reopened.close().unwrap();
    }

    #[test]
    fn ffi_fixture_policy_absence_preserves_local_only_and_input_refusals() {
        let root = tempfile::tempdir().unwrap();
        let host = host(root.path());
        let connection = owner(host.clone()).configure_meguri("https://ordinary.synthetic.example/".into()).unwrap();
        // LocalOnly admission prevents dispatch to this synthetic public name.
        assert_not_selected(&connection);
        assert_early_input_guards(&connection);
        host.close().unwrap();
    }

    #[test]
    fn ffi_fixture_origin_mismatch_reaches_all_access_factories_after_early_guards() {
        let root = tempfile::tempdir().unwrap();
        let host = host(root.path());
        let original = owner(host.clone());
        let der = meguri_testkit::sync_stub::TestRoot::generate().der();
        install(&original, root.path(), &der).unwrap();
        let connection = original.clone().configure_meguri(OTHER_ORIGIN.into()).unwrap();
        assert_eq!(original.snapshot().unwrap().selection, Some(ProviderSelection::LocalOnly));
        let credentials = credentials(&connection);
        assert!(matches!(bootstrap_refusal(&connection, credentials.clone()), ProviderBootstrapFailure::Unavailable));
        assert_round_refusals(&connection, credentials.clone(), ProviderRoundRefusal::ConnectionChanged);
        assert_early_input_guards(&connection);
        assert_idle(&host);
        host.close().unwrap();
        assert!(install(&original, root.path(), &der).is_err());
        assert!(connection.start_bootstrap_prepare(credentials.clone(), ProviderBootstrapDirection::UseServerCollection).is_err());
        assert_round_refusals(&connection, credentials, ProviderRoundRefusal::Interrupted);
    }

    #[test]
    fn ffi_fixture_matching_origin_keeps_admission_and_replacement_keeps_policy_but_fences_stale_owner() {
        let root = tempfile::tempdir().unwrap();
        let host = host(root.path());
        let original = owner(host.clone());
        let der = meguri_testkit::sync_stub::TestRoot::generate().der();
        install(&original, root.path(), &der).unwrap();
        let connection = original.clone().configure_meguri(ORIGIN.into()).unwrap();
        assert_not_selected(&connection);
        let stale_credentials = credentials(&connection);

        {
            let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
            slot.enter_exclusive(true).unwrap().replace(|live| {
                let candidate = live.with_file_name("synthetic-replacement.anki2");
                std::fs::copy(live, &candidate).map_err(meguri_storage::StorageError::from)?;
                std::fs::rename(candidate, live).map_err(meguri_storage::StorageError::from)?;
                Ok(())
            }).unwrap();
        }
        assert_ne!(host.cloud_connection_generation().unwrap(), original.generation);
        assert!(install(&original, root.path(), &der).is_err());
        assert!(connection.information().is_err());
        assert!(connection.start_bootstrap_prepare(stale_credentials.clone(), ProviderBootstrapDirection::UseServerCollection).is_err());
        assert_round_refusals(&connection, stale_credentials, ProviderRoundRefusal::Interrupted);

        let current = owner(host.clone());
        let matching = current.clone().configured_meguri().unwrap().unwrap();
        assert_not_selected(&matching);
        let changed = current.configure_meguri(OTHER_ORIGIN.into()).unwrap();
        let credentials = credentials(&changed);
        // This mismatch proves that the original policy survived replacement.
        assert!(matches!(bootstrap_refusal(&changed, credentials.clone()), ProviderBootstrapFailure::Unavailable));
        assert_round_refusals(&changed, credentials, ProviderRoundRefusal::ConnectionChanged);
        host.close().unwrap();
    }
}
