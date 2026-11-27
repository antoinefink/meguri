//! Production Host startup must not turn configured transports into a service choice.
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
use meguri_sync::cloud::{BuildScope, ContainerId, Environment, OpaqueBytes,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus}};
use meguri_testkit::cloud::devices::NativeFiles;

struct Account(AtomicUsize);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        self.0.fetch_add(1, Ordering::SeqCst);
        IdentityStatus::Available(OpaqueBytes(b"synthetic-provider-account".to_vec()))
    }
}
fn unopened_fixture(root: &std::path::Path, account: Arc<Account>) -> HostConfig {
    let directory = root.join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(151));
    let mut config = HostConfig::new(directory, root.join("backups"), root.join("widget"),
        clock, random, Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    config.cloud = Some(CloudInputs {
        scope: BuildScope { container: ContainerId::new("synthetic-provider").unwrap(), environment: Environment::Development },
        identity: account, files: Arc::new(NativeFiles),
    });
    config
}
fn fixture(root: &std::path::Path, account: Arc<Account>) -> HostConfig {
    let config = unopened_fixture(root, account);
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
    config
}

#[cfg(unix)]
#[test]
fn aliased_ordinary_host_metadata_configure_save_and_retry_preserve_original_collection() {
    use meguri_model::faultpoint::{self, Fault};
    use std::os::unix::{fs::MetadataExt, fs::symlink};
    for configure in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let account = Arc::new(Account(AtomicUsize::new(0)));
        let mut config = fixture(&root.path().join("original"), account.clone());
        let alias = root.path().join("collection-alias");
        symlink(&config.collection_directory, &alias).unwrap();
        config.collection_directory = alias;
        let host = CoreHost::open(config.clone()).unwrap();
        let collection = host.collection().unwrap();
        let path = collection.path().to_owned();
        assert_ne!(path, config.collection_directory.join("collection.anki2"));
        assert_eq!(path, config.collection_directory.canonicalize().unwrap().join("collection.anki2"));
        assert!(host.derived().wait_idle(std::time::Duration::from_secs(5)));
        let file = std::fs::metadata(&path).unwrap();
        let bytes = std::fs::read(&path).unwrap();
        let sidecar = path.parent().unwrap().join("meguri-sidecar.json");
        let sidecar_bytes = std::fs::read(&sidecar).unwrap();
        let state = meguri_engine::device_state::DeviceState::for_collection(&path);
        let publication = state.settled_publication().unwrap().unwrap();
        let generation = host.cloud_connection_generation().unwrap();
        let connection = host.configure_meguri_connection("https://initial-alias.synthetic.example/").unwrap();
        let mut settings = host.provider_authority().snapshot().unwrap().settings;
        settings.automatic = false;
        host.save_provider_settings(settings).unwrap();
        let metadata_path = config.manual_transfer_directory.join("provider.json");
        let before = std::fs::read(&metadata_path).unwrap();
        settings.paused = true;
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
        let failure = if configure {
            host.configure_meguri_connection("https://retry-alias.synthetic.example/").unwrap_err()
        } else { host.save_provider_settings(settings).unwrap_err() };
        faultpoint::disarm("storage.sidecar.before_save");
        let retained_error = host.provider_authority().snapshot().unwrap_err();
        host.refresh_provider_admission();
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert!(host.provider_authority().network_blocked());
        assert_eq!(host.save_provider_settings(settings).unwrap_err(), failure);
        assert_eq!(std::fs::read(&metadata_path).unwrap(), before);
        host.retry_provider_publication().unwrap();
        let document = host.provider_authority().snapshot().unwrap();
        assert_eq!(meguri_sync::provider::Authority::load(&config.manual_transfer_directory).snapshot().unwrap(), document);
        if configure {
            assert_eq!(document.meguri.as_ref().unwrap().endpoint, "https://retry-alias.synthetic.example/");
        } else {
            assert_eq!(document.meguri.as_ref(), Some(&connection));
            assert_eq!(document.settings, settings);
        }
        assert_eq!(document.selection, meguri_sync::provider::Selection::LocalOnly);
        assert!(document.binding.is_none() && document.handoff.is_none());
        assert!(!document.permits_meguri_transfer());
        assert!(!host.provider_authority().permits_icloud_account_read());
        let runtime = host.cloud_runtime().unwrap();
        let status = runtime.status().unwrap();
        assert!(status.binding.is_none() && !status.stored_usable && !status.identity_verified);
        assert!(runtime.require_provider_collection_admission().is_err());
        assert!(runtime.register_sdk(false).is_err());
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        assert_eq!(host.cloud_connection_generation().unwrap(), generation);
        assert_eq!(host.collection().unwrap().path(), path);
        assert_eq!(state.settled_publication().unwrap(), Some(publication));
        let after = std::fs::metadata(&path).unwrap();
        assert_eq!((after.dev(), after.ino()), (file.dev(), file.ino()));
        assert_eq!(std::fs::read(&path).unwrap(), bytes);
        assert_eq!(std::fs::read(&sidecar).unwrap(), sidecar_bytes);
        host.close().unwrap();
    }
}

#[cfg(unix)]
#[test]
fn retargeted_alias_cannot_rebind_original_host_metadata_to_a_foreign_open_collection() {
    use meguri_engine::{ChangeObserver, GenerationListener, OpenStep, SuspendParticipant};
    use meguri_model::faultpoint::{self, Fault};
    use std::os::unix::fs::symlink;

    // CoreHost exposes its real review service by reference. Forward its ports
    // without retaining a Host -> Engine -> ports -> Host ownership cycle.
    struct OriginalReview(std::sync::Weak<CoreHost>);
    impl OpenStep for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn run(&self, collection: &meguri_engine::Collection) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().run(collection)
        }
    }
    impl ChangeObserver for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn observe(&self, commit: &meguri_engine::changes::observers::ObservedCommit<'_>) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().observe(commit)
        }
    }
    impl GenerationListener for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn collection_reopened(&self, collection: &meguri_engine::Collection) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().collection_reopened(collection)
        }
        fn collection_replaced(&self, old: meguri_engine::Generation, new: meguri_engine::Generation) {
            self.0.upgrade().unwrap().review().collection_replaced(old, new);
        }
    }
    impl SuspendParticipant for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn flush(&self) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().flush()
        }
        fn flush_until(&self, deadline: std::time::Duration) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().flush_until(deadline)
        }
        fn suspend(&self) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().suspend()
        }
        fn suspend_until(&self, deadline: std::time::Duration) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().suspend_until(deadline)
        }
        fn release_connections_until(&self, deadline: std::time::Duration) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().release_connections_until(deadline)
        }
        fn resume(&self) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().resume()
        }
        fn resume_with_collection(&self, collection: &meguri_engine::Collection) -> Result<(), meguri_engine::EngineError> {
            self.0.upgrade().unwrap().review().resume_with_collection(collection)
        }
    }

    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let mut config = fixture(&root.path().join("original"), account.clone());
    let foreign = fixture(&root.path().join("foreign"), account.clone());
    let alias = root.path().join("collection-alias");
    symlink(&config.collection_directory, &alias).unwrap();
    config.collection_directory = alias.clone();
    let host = Arc::new(CoreHost::open(config.clone()).unwrap());
    let original_path = host.collection().unwrap().path().to_owned();
    let original_state = meguri_engine::device_state::DeviceState::for_collection(&original_path);
    let publication = original_state.settled_publication().unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(host.save_provider_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    let metadata_path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&metadata_path).unwrap();
    // Replace the actual engine owner, while keeping the original Host alive.
    host.engine().close().unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&foreign.collection_directory, &alias).unwrap();
    let foreign_path = foreign.collection_directory.canonicalize().unwrap().join("collection.anki2");
    let review = Arc::new(OriginalReview(Arc::downgrade(&host)));
    let ports = host.derived().ports(meguri_engine::Ports::new())
        .open_step(review.clone())
        .observer(review.clone())
        .generation_listener(review.clone())
        .suspend_participant(review);
    let replacement = host.engine().open_raw(&foreign_path,
        meguri_engine::OpenConfig::new(foreign.clock.clone(), foreign.random.clone()).ports(ports)).unwrap();
    let current_generation = replacement.generation();
    assert_ne!(current_generation, generation);
    assert_eq!(config.collection_directory.canonicalize().unwrap().join("collection.anki2"), replacement.path());
    let foreign_state = meguri_engine::device_state::DeviceState::for_collection(&foreign_path);
    let foreign_publication = foreign_state.settled_publication().unwrap();
    // Even the current generation plus an alias resolving to that actual open
    // path cannot grant the original Host ownership of the foreign collection.
    assert!(matches!(host.configure_meguri_connection_for_generation(current_generation, "https://foreign.synthetic.example/"),
        Err(meguri_engine::EngineError::CollectionUnavailable { reason }) if reason == "provider metadata collection owner changed"));
    assert!(host.save_provider_settings_for_generation(current_generation, settings).is_err());
    assert!(host.retry_provider_publication_for_generation(current_generation).is_err());
    assert!(host.configure_meguri_connection_for_generation(generation, "https://stale.synthetic.example/").is_err());
    assert!(host.save_provider_settings_for_generation(generation, settings).is_err());
    assert!(host.retry_provider_publication_for_generation(generation).is_err());
    assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
    assert_eq!(std::fs::read(&metadata_path).unwrap(), before);
    assert_eq!(original_state.settled_publication().unwrap(), publication);
    assert_eq!(foreign_state.settled_publication().unwrap(), foreign_publication);
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
    drop(replacement);
    host.close().unwrap();
}

#[cfg(unix)]
#[test]
fn prepared_aliased_host_detects_a_database_in_original_directory_after_alias_retargeting() {
    use meguri_model::faultpoint::{self, Fault};
    use std::os::unix::fs::symlink;
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let mut config = unopened_fixture(&root.path().join("original"), account.clone());
    let original_directory = config.collection_directory.canonicalize().unwrap();
    let alias = root.path().join("collection-alias");
    symlink(&original_directory, &alias).unwrap();
    config.collection_directory = alias.clone();
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config.clone()).unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(host.save_provider_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    let metadata_path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&metadata_path).unwrap();
    let empty = root.path().join("empty");
    std::fs::create_dir(&empty).unwrap();
    std::fs::remove_file(&alias).unwrap();
    symlink(&empty, &alias).unwrap();
    let path = original_directory.join("collection.anki2");
    meguri_engine::bootstrap::create(&path, config.clock.as_ref(), config.random.as_ref()).unwrap();
    let bytes = std::fs::read(&path).unwrap();
    assert!(matches!(host.configure_meguri_connection("https://appeared.synthetic.example/"),
        Err(meguri_engine::EngineError::CollectionUnavailable { reason }) if reason == "existing collection requires open"));
    assert!(host.save_provider_settings(settings).is_err());
    assert!(host.retry_provider_publication().is_err());
    assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
    assert_eq!(std::fs::read(&metadata_path).unwrap(), before);
    assert_eq!(host.cloud_connection_generation().unwrap(), generation);
    assert_eq!(host.engine().state(), meguri_engine::LifecycleState::NoCollection);
    assert!(host.collection().is_err());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    assert!(!original_directory.join("meguri-sidecar.json").exists());
    assert!(!empty.join("collection.anki2").exists());
    assert!(!empty.join("meguri-sidecar.json").exists());
    assert_eq!(meguri_engine::device_state::DeviceState::for_collection(&path).settled_publication().unwrap(), None);
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
    host.close().unwrap();
}

fn assert_unconfigured_without_collection(host: &CoreHost, config: &HostConfig, account: &Account) {
    assert_eq!(host.engine().state(), meguri_engine::LifecycleState::NoCollection);
    assert!(host.collection().is_err());
    assert!(!config.collection_directory.join("collection.anki2").exists());
    assert!(!config.collection_directory.join("meguri-sidecar.json").exists());
    assert_eq!(meguri_engine::device_state::DeviceState::for_collection(
        &config.collection_directory.join("collection.anki2")).settled_publication().unwrap(), None);
    let doc = host.provider_authority().snapshot().unwrap();
    assert_eq!(doc.selection, meguri_sync::provider::Selection::LocalOnly);
    assert!(doc.binding.is_none() && doc.handoff.is_none());
    assert!(!doc.permits_meguri_transfer());
    assert!(!host.provider_authority().permits_icloud_account_read());
    let status = host.cloud_runtime().unwrap().status().unwrap();
    assert!(status.binding.is_none() && !status.stored_usable && !status.identity_verified);
    assert!(host.cloud_runtime().unwrap().register_sdk(false).is_err());
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
}

#[test]
fn prepared_host_configures_and_saves_settings_in_the_same_installation_without_a_collection() {
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = unopened_fixture(root.path(), account.clone());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config.clone()).unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let connection = host.configure_meguri_connection("https://first-setup.synthetic.example/").unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.enabled = true;
    settings.paused = true;
    host.save_provider_settings(settings).unwrap();
    assert_eq!(host.cloud_connection_generation().unwrap(), generation);
    assert_eq!(host.provider_authority().snapshot().unwrap().meguri.as_ref(), Some(&connection));
    assert_unconfigured_without_collection(&host, &config, &account);
    let path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&path).unwrap();
    let persisted = meguri_sync::provider::Authority::load(&config.manual_transfer_directory).snapshot().unwrap();
    assert_eq!(persisted.meguri.as_ref(), Some(&connection));
    assert_eq!(persisted.settings, settings);
    host.close().unwrap();
    let successor = process.prepare_cloud_connection(config.clone()).unwrap();
    // NoCollection can retain the same engine generation: the original Host's
    // close lease must still exclude its old metadata operations.
    assert_eq!(successor.cloud_connection_generation().unwrap(), generation);
    assert!(host.configure_meguri_connection_for_generation(generation, "https://stale.synthetic.example/").is_err());
    assert!(host.save_provider_settings_for_generation(generation, settings).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(successor.provider_authority().snapshot().unwrap().meguri.as_ref(), Some(&connection));
    assert_unconfigured_without_collection(&successor, &config, &account);
    successor.close().unwrap();
}

#[test]
fn prepared_host_failed_configuration_and_settings_retry_keep_original_none_publication_and_error() {
    use meguri_model::faultpoint::{self, Fault};
    for configure in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let account = Arc::new(Account(AtomicUsize::new(0)));
        let config = unopened_fixture(root.path(), account.clone());
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let host = process.prepare_cloud_connection(config.clone()).unwrap();
        let generation = host.cloud_connection_generation().unwrap();
        let mut settings = host.provider_authority().snapshot().unwrap().settings;
        settings.paused = true;
        let path = config.manual_transfer_directory.join("provider.json");
        let before = std::fs::read(&path).unwrap();
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
        let failure = if configure {
            host.configure_meguri_connection("https://retained-setup.synthetic.example/").unwrap_err()
        } else { host.save_provider_settings(settings).unwrap_err() };
        faultpoint::disarm("storage.sidecar.before_save");
        let retained_error = host.provider_authority().snapshot().unwrap_err();
        host.refresh_provider_admission();
        assert!(host.provider_authority().network_blocked());
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert_eq!(host.configure_meguri_connection("https://unrelated.synthetic.example/").unwrap_err(), failure);
        assert_eq!(host.save_provider_settings(settings).unwrap_err(), failure);
        faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
        assert!(host.retry_provider_publication().is_err());
        faultpoint::disarm("storage.sidecar.before_save");
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert!(host.provider_authority().network_blocked());
        assert_eq!(std::fs::read(&path).unwrap(), before);
        host.retry_provider_publication().unwrap();
        let doc = host.provider_authority().snapshot().unwrap();
        if configure {
            assert_eq!(doc.meguri.unwrap().endpoint, "https://retained-setup.synthetic.example/");
        } else { assert_eq!(doc.settings, settings); }
        assert_eq!(host.cloud_connection_generation().unwrap(), generation);
        assert_unconfigured_without_collection(&host, &config, &account);
        host.close().unwrap();
    }
}

#[cfg(feature = "test-hooks")]
#[test]
fn prepared_host_metadata_refuses_held_close_owner_and_real_lifecycle_cleanup_lane() {
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = unopened_fixture(root.path(), account.clone());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config.clone()).unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    let path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&path).unwrap();
    host.with_close_state_held_for_testing(|| {
        assert!(host.configure_meguri_connection_for_generation(generation, "https://busy.synthetic.example/").is_err());
        assert!(host.save_provider_settings_for_generation(generation, settings).is_err());
        assert_eq!(std::fs::read(&path).unwrap(), before);
    });
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(host.save_provider_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    // This is close's production control lane, not an invented completed proof.
    host.engine().close_with_host_cleanup(|collection| {
        assert!(collection.is_none());
        assert!(host.configure_meguri_connection("https://busy.synthetic.example/").is_err());
        assert!(host.save_provider_settings(settings).is_err());
        assert!(host.retry_provider_publication().is_err());
        assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
        assert_eq!(std::fs::read(&path).unwrap(), before);
        Ok(())
    }).unwrap();
    host.retry_provider_publication().unwrap();
    assert_eq!(host.provider_authority().snapshot().unwrap().settings, settings);
    assert_unconfigured_without_collection(&host, &config, &account);
    host.close().unwrap();
}

#[test]
fn closed_prepared_host_cannot_retry_failed_metadata_over_a_successor_in_the_same_generation() {
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = unopened_fixture(root.path(), account.clone());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let old = process.prepare_cloud_connection(config.clone()).unwrap();
    let generation = old.cloud_connection_generation().unwrap();
    let mut settings = old.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(old.save_provider_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = old.provider_authority().snapshot().unwrap_err();
    old.close().unwrap();
    let successor = process.prepare_cloud_connection(config.clone()).unwrap();
    let connection = successor.configure_meguri_connection("https://successor.synthetic.example/").unwrap();
    assert_eq!(successor.cloud_connection_generation().unwrap(), generation);
    let path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&path).unwrap();
    assert!(old.retry_provider_publication_for_generation(generation).is_err());
    assert!(old.configure_meguri_connection_for_generation(generation, "https://stale.synthetic.example/").is_err());
    assert!(old.save_provider_settings_for_generation(generation, settings).is_err());
    assert_eq!(old.provider_authority().snapshot().unwrap_err(), retained_error);
    assert!(old.provider_authority().network_blocked());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert_eq!(successor.provider_authority().snapshot().unwrap().meguri.as_ref(), Some(&connection));
    assert!(!successor.provider_authority().snapshot().unwrap().settings.paused);
    assert_unconfigured_without_collection(&successor, &config, &account);
    successor.close().unwrap();
}

fn prepare_provider_review(host: &CoreHost) -> meguri_model::ids::DeckId {
    let collection = host.collection().unwrap();
    let deck = meguri_engine::decks::add(&collection, "Provider review".into()).unwrap().value.id;
    let kind = meguri_engine::notetypes::add_stock(&collection,
        meguri_engine::notetypes::StockKind::Basic, Some("Provider review".into())).unwrap().value.id;
    meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
        notetype_id: kind, fields: vec!["Provider front".into(), "Provider back".into()],
        tags: vec![], deck_id: deck, guid: None, update_add_defaults: false,
    }, meguri_engine::OpSource::UserUndoable).unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    assert_eq!(meguri_engine::sched::fsrs_policy::policy_status(&collection).unwrap(),
        meguri_engine::sched::fsrs_policy::PolicyStatus::Ok);
    deck
}

fn start_provider_review(host: &CoreHost, deck: meguri_model::ids::DeckId) -> meguri_engine::SessionId {
    let (session_id, head) = match host.review().start(deck, None).unwrap() {
        meguri_engine::session::StartResult::Started { session_id, head, .. } => (session_id, head),
        actual => panic!("provider fixture must start review: {actual:?}"),
    };
    assert_eq!(head.items.len(), 1);
    session_id
}

#[test]
fn ordinary_host_open_and_reopen_do_not_discover_an_account_for_a_new_installation() {
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = fixture(root.path(), account.clone());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    for index in 0..2 {
        let host = process.open(config.clone()).unwrap();
        let collection = host.collection().unwrap();
        // Local engine operations remain independent of network eligibility.
        meguri_engine::decks::add(&collection, format!("Local study {index}")).unwrap();
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        assert!(host.cloud_runtime().unwrap().register_sdk(false).is_err());
        host.close().unwrap();
    }
}

#[test]
fn application_upgrade_reopens_archived_connected_publication_through_host_startup() {
    use meguri_engine::device_state::{AccountBinding, DeviceState, PublicationDecision};
    use meguri_sync::provider::{LegacySettings, ProviderDocument, Selection};
    use std::os::unix::fs::MetadataExt;
    for manual in [false, true] {
        for decision in [PublicationDecision::Clear, PublicationDecision::Pending] {
            let root = tempfile::tempdir().unwrap();
            let account = Arc::new(Account(AtomicUsize::new(0)));
            let mut config = fixture(root.path(), account.clone());
            config.cloud = None; // The application supplies no retired runtime.
            let original = CoreHost::open(config.clone()).unwrap();
            prepare_provider_review(&original);
            original.close().unwrap();
            drop(original);
            let live = config.collection_directory.join("collection.anki2");
            let bytes = std::fs::read(&live).unwrap();
            let native = std::fs::metadata(&live).unwrap();
            let sidecar = config.collection_directory.join("meguri-sidecar.json");
            // Synthetic archival publication, with real populated collection
            // bytes. No live account or completion authority is supplied.
            meguri_storage::sidecar::update_document(&sidecar, |raw| {
                raw.get_mut("deviceState").unwrap()["active"]["binding"] = serde_json::json!(AccountBinding::Connected {
                    account: "synthetic-archived-account".into(), configuration: "synthetic-retired-app".into(),
                    environment: "development".into(),
                });
                raw.get_mut("deviceState").unwrap()["active"]["decision"] = serde_json::json!(decision);
                raw.get_mut("deviceState").unwrap()["active"]["futurePublication"] = serde_json::json!([1, 2]);
                Ok(())
            }).unwrap();
            let archive: serde_json::Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
            let publication = DeviceState::for_collection(&live).settled_publication().unwrap();
            let provider = ProviderDocument::migration(LegacySettings { enabled: true, paused: false, automatic: true, media: true });
            let provider_path = config.manual_transfer_directory.join("provider.json");
            std::fs::write(&provider_path, serde_json::to_vec(&provider).unwrap()).unwrap();
            for _ in 0..2 {
                let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
                let host = if manual { process.open_for_manual_import(config.clone()) }
                    else { process.open(config.clone()) }.unwrap();
                assert!(host.collection().is_ok(), "startup must open the archived local collection");
                assert!(host.derived().wait_idle(std::time::Duration::from_secs(5)));
                assert!(host.cloud_runtime().is_none());
                let provider = host.provider_authority().snapshot().unwrap();
                assert_eq!(provider.selection, Selection::LocalOnly);
                assert!(!provider.settings.enabled && provider.handoff.is_none());
                assert!(!provider.permits_meguri_transfer());
                assert_eq!(DeviceState::for_collection(&live).settled_publication().unwrap(), publication);
                host.close().unwrap();
                let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
                assert_eq!(after["deviceState"], archive["deviceState"]);
                let file = std::fs::metadata(&live).unwrap();
                assert_eq!((file.dev(), file.ino()), (native.dev(), native.ino()));
                assert_eq!(std::fs::read(&live).unwrap(), bytes, "all synthetic collection contents survive startup");
            }
            assert_eq!(account.0.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn application_upgrade_settles_absent_initial_adoption_before_prepared_host_attachment() {
    use meguri_engine::device_state::DeviceState;
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    for entry in 0..3 {
        for malformed in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let account = Arc::new(Account(AtomicUsize::new(0)));
            let mut config = unopened_fixture(root.path(), account.clone());
            config.cloud = None;
            let live = config.collection_directory.join("collection.anki2");
            let sidecar = config.collection_directory.join("meguri-sidecar.json");
            let stage = root.path().join("synthetic-staged.anki2");
            meguri_engine::bootstrap::create(&stage, config.clock.as_ref(), config.random.as_ref()).unwrap();
            let bytes = std::fs::read(&stage).unwrap();
            let native = std::fs::metadata(&stage).unwrap();
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            let prior = serde_json::json!({"identity":[1,2],"binding":"LocalOnly","decision":"Clear"});
            let candidate = serde_json::json!({"identity":[3,4],"binding":{"Connected":{
                "account":"synthetic-archived-account","configuration":"synthetic-retired-app","environment":"development"}},"decision":"Clear"});
            let mut raw = serde_json::json!({"futureTop":{"retained":true},"deviceState":{"active":prior,"intent":{
                "operation":[5,6],"reason":"CloudAdopt","prior":prior,"candidate":candidate,"phase":"Intent",
                "old_file":null,"old_native":null,"candidate_file":{"length":bytes.len(),"sha256":digest},
                "candidate_native":{"device":native.dev(),"inode":native.ino()}}}});
            if malformed { raw["deviceState"]["intent"]["candidate_native"] = serde_json::Value::Null; }
            let evidence = serde_json::to_vec_pretty(&raw).unwrap();
            std::fs::write(&sidecar, &evidence).unwrap();
            let tracking = config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE);
            std::fs::write(&tracking, b"synthetic retained tracking").unwrap();
            let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
            let result = match entry {
                0 => process.open_for_manual_import(config.clone()),
                1 => process.prepare_empty_collection(config.clone()),
                _ => process.prepare_cloud_connection(config.clone()),
            };
            if malformed {
                assert!(result.is_err());
                assert_eq!(std::fs::read(&sidecar).unwrap(), evidence);
            } else {
                let host = result.unwrap();
                assert_eq!(host.engine().state(), meguri_engine::LifecycleState::NoCollection);
                assert_eq!(host.engine().generation(), meguri_engine::Generation(0));
                assert_eq!(serde_json::to_value(DeviceState::for_collection(&live).settled_publication().unwrap().unwrap()).unwrap(), prior);
                let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&sidecar).unwrap()).unwrap();
                assert!(after["deviceState"]["intent"].is_null());
                assert_eq!(after["futureTop"], raw["futureTop"]);
                assert!(host.cloud_runtime().is_none());
                host.close().unwrap();
            }
            assert_eq!(live.symlink_metadata().unwrap_err().kind(), std::io::ErrorKind::NotFound);
            assert_eq!(std::fs::read(&stage).unwrap(), bytes);
            let file = std::fs::metadata(&stage).unwrap();
            assert_eq!((file.dev(), file.ino()), (native.dev(), native.ino()));
            assert_eq!(std::fs::read(&tracking).unwrap(), b"synthetic retained tracking");
            assert!(!config.collection_directory.join("collection.media").exists());
            assert_eq!(account.0.load(Ordering::SeqCst), 0);
        }
    }
}

#[test]
fn application_provider_initialization_ignores_retired_preferences_and_only_accepts_exact_marker() {
    use meguri_sync::provider::{Authority, LegacyInput, LegacySettings, Selection};
    let root = tempfile::tempdir().unwrap();
    // These inputs remain part of the historical Rust API, but the application
    // must neither import them nor need them to upgrade a prior failure marker.
    for input in [LegacyInput::Invalid, LegacyInput::Settings(LegacySettings {
        enabled: true, paused: false, automatic: true, media: true,
    })] {
        let installation = tempfile::tempdir_in(root.path()).unwrap();
        CoreHost::migrate_application_provider_configuration(installation.path(), input).unwrap();
        let provider = Authority::load(installation.path()).snapshot().unwrap();
        assert_eq!(provider.selection, Selection::LocalOnly);
        assert!(!provider.settings.enabled && provider.meguri.is_none());
    }
    for extra in [false, true] {
        let account = Arc::new(Account(AtomicUsize::new(0)));
        let mut config = fixture(&root.path().join(if extra { "malformed" } else { "marker" }), account.clone());
        config.cloud = None;
        std::fs::create_dir_all(&config.manual_transfer_directory).unwrap();
        let path = config.manual_transfer_directory.join("provider.json");
        let mut marker = serde_json::json!({"migrationFailure":"invalidLegacySettings"});
        if extra { marker["selection"] = serde_json::json!("Meguri"); }
        let bytes = serde_json::to_vec(&marker).unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let migration = CoreHost::migrate_application_provider_configuration(&config.manual_transfer_directory, LegacyInput::Absent);
        assert_eq!(migration.is_err(), extra);
        let host = CoreHost::open(config).unwrap();
        assert!(host.collection().is_ok());
        assert_eq!(host.provider_authority().snapshot().is_err(), extra);
        assert_eq!(host.provider_authority().network_blocked(), extra);
        if extra { assert_eq!(std::fs::read(path).unwrap(), bytes); }
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        host.close().unwrap();
    }
}

#[test]
fn invalid_provider_document_blocks_account_discovery_without_preventing_local_open() {
    for document in ["null", "{}", "{\"version\":999}", "{\"version\":1,\"handoff\":true}"] {
        let root = tempfile::tempdir().unwrap();
        let account = Arc::new(Account(AtomicUsize::new(0)));
        let config = fixture(root.path(), account.clone());
        std::fs::create_dir_all(&config.manual_transfer_directory).unwrap();
        std::fs::write(config.manual_transfer_directory.join("provider.json"), document).unwrap();
        let host = CoreHost::open(config).unwrap();
        meguri_engine::decks::add(&host.collection().unwrap(), "Still local".into()).unwrap();
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        assert!(host.cloud_runtime().unwrap().register_sdk(false).is_err());
        host.close().unwrap();
    }
}

#[test]
fn reopened_host_explicitly_recovers_lost_handoff_to_local_only_and_retains_intent() {
    use meguri_sync::provider::Selection;
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = fixture(root.path(), account.clone());
    let host = CoreHost::open(config.clone()).unwrap();
    let connection = host.configure_meguri_connection("https://retained.synthetic.example/").unwrap();
    let original = host.begin_provider_handoff(Selection::Meguri).unwrap();
    assert!(host.recover_provider_handoff_to_local_only().is_err());
    drop(original);
    meguri_storage::sidecar::update_document(&config.manual_transfer_directory.join("provider.json"), |raw| {
        raw.get_mut("handoff").unwrap()["futureIntentEvidence"] = serde_json::json!({"retained": true});
        Ok(())
    }).unwrap();
    let before = std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap();
    let original_intent: serde_json::Value = serde_json::from_slice::<serde_json::Value>(&before).unwrap()["handoff"].clone();
    host.close().unwrap();
    drop(host);
    let reopened = CoreHost::open(config.clone()).unwrap();
    let deck = prepare_provider_review(&reopened);
    let session = start_provider_review(&reopened, deck);
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    assert_eq!(reopened.review().active_session(), Some(session));
    assert_eq!(std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap(), before);
    reopened.review().end(session).unwrap();
    let exclusive = reopened.engine().reserve_collection_export_handoff().unwrap();
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    drop(exclusive);
    let (entered, ready) = std::sync::mpsc::channel();
    let (release, wait) = std::sync::mpsc::channel();
    let events = reopened.engine().stream().subscribe();
    let current_job = reopened.start_cloud_connection_job(reopened.cloud_connection_generation().unwrap(), "synthetic current owner", move |_| {
        entered.send(()).unwrap();
        wait.recv().unwrap();
        Ok(())
    }).unwrap();
    ready.recv().unwrap();
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    release.send(()).unwrap();
    loop {
        if matches!(events.recv().unwrap().kind, meguri_engine::EventKind::JobFinished { id, .. } if id == current_job.id) { break; }
    }
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    reopened.refresh_provider_admission();
    assert!(reopened.provider_authority().network_blocked());
    assert_eq!(std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap(), before);
    assert!(reopened.provider_authority().failed_publication_requires_handoff_recovery().unwrap());
    let session = match reopened.review().start(deck, None).unwrap() {
        meguri_engine::session::StartResult::Started { session_id, .. } => session_id,
        actual => panic!("recovery fixture must restart review: {actual:?}"),
    };
    assert!(reopened.retry_provider_publication().is_err());
    assert_eq!(reopened.review().active_session(), Some(session));
    assert!(reopened.provider_authority().network_blocked());
    assert_eq!(std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap(), before);
    reopened.review().end(session).unwrap();
    reopened.retry_provider_publication().unwrap();
    let recovered = reopened.provider_authority().snapshot().unwrap();
    assert_eq!(recovered.selection, Selection::LocalOnly);
    assert!(recovered.handoff.is_none() && recovered.binding.is_none() && !recovered.settings.enabled);
    assert_eq!(recovered.meguri.as_ref(), Some(&connection));
    assert!(!reopened.provider_authority().permits_icloud_account_read());
    let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap()).unwrap();
    assert_eq!(raw["handoffRecoveries"][0]["intent"], original_intent);
    assert_eq!(raw["handoffRecoveries"][0]["outcome"], "localOnlySetupRequired");
    assert_eq!(raw["handoffRecoveries"][0]["currentPublication"], original_intent["publication"]);
    assert_eq!(raw["handoffRecoveries"][0]["currentGeneration"], reopened.cloud_connection_generation().unwrap().0);
    meguri_engine::decks::add(&reopened.collection().unwrap(), "Local after recovery".into()).unwrap();
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
    reopened.close().unwrap();
    drop(reopened);
    let final_host = CoreHost::open(config).unwrap();
    assert_eq!(final_host.provider_authority().snapshot().unwrap().selection, Selection::LocalOnly);
    final_host.close().unwrap();
}

#[test]
fn local_only_recovery_does_not_claim_pending_credential_removal_completed() {
    let root = tempfile::tempdir().unwrap();
    let config = fixture(root.path(), Arc::new(Account(AtomicUsize::new(0))));
    let original = CoreHost::open(config.clone()).unwrap();
    drop(original.begin_provider_handoff_with_removal(meguri_sync::provider::Selection::LocalOnly, true).unwrap());
    original.close().unwrap();
    drop(original);
    let path = config.manual_transfer_directory.join("provider.json");
    let before = std::fs::read(&path).unwrap();
    let reopened = CoreHost::open(config).unwrap();
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    assert!(reopened.provider_authority().snapshot().unwrap().handoff.unwrap().removal_pending());
    assert!(reopened.provider_authority().network_blocked());
    assert_eq!(std::fs::read(path).unwrap(), before);
    reopened.close().unwrap();
}

#[test]
fn failed_host_settings_publication_stays_closed_through_projection_until_guarded_retry() {
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let config = fixture(root.path(), Arc::new(Account(AtomicUsize::new(0))));
    let host = CoreHost::open(config.clone()).unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    let before = std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    assert!(host.save_provider_settings(settings).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    host.refresh_provider_admission();
    assert!(host.provider_authority().network_blocked());
    assert_eq!(std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap(), before);
    let exclusive = host.engine().reserve_collection_export_handoff().unwrap();
    assert!(host.retry_provider_publication().is_err());
    drop(exclusive);
    host.retry_provider_publication().unwrap();
    assert!(host.provider_authority().snapshot().unwrap().settings.paused);
    host.close().unwrap();
}

#[cfg(feature = "test-hooks")]
#[test]
fn failed_pause_of_a_qualified_icloud_writer_cannot_be_undone_by_host_refresh() {
    use meguri_model::faultpoint::{self, Fault};
    struct SDKAccount;
    impl IdentityProvider for SDKAccount {
        fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
            IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec()))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let mut config = fixture(root.path(), Arc::new(Account(AtomicUsize::new(0))));
    let cloud = config.cloud.as_mut().unwrap();
    cloud.scope.container = ContainerId::new("synthetic-u114").unwrap();
    cloud.identity = Arc::new(SDKAccount);
    config.publication_binding = meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(), configuration: "synthetic-u114".into(), environment: "development".into(),
    };
    CoreHost::migrate_provider_configuration(&config.manual_transfer_directory, meguri_sync::provider::LegacyInput::Settings(
        meguri_sync::provider::LegacySettings { enabled: true, paused: false, automatic: true, media: true }
    )).unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    // Include review data and the real scheduler enable in the qualified baseline.
    let deck = prepare_provider_review(&host);
    // The existing stable-binding fixture reads this actual Host collection;
    // provider admission is subsequently produced by production qualification.
    runtime.prepare_adapter_baseline_for_testing(&host.collection().unwrap(), config.clock.as_ref(), config.random.as_ref(), None).unwrap();
    host.refresh_provider_admission();
    runtime.require_provider_collection_admission().unwrap();
    let session = start_provider_review(&host, deck);
    let head = host.review().head(session, 1).unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let publication = meguri_engine::device_state::DeviceState::for_collection(
        &config.collection_directory.join("collection.anki2")).settled_publication().unwrap();
    let before = std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap();
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    let failure = host.save_provider_settings(settings).unwrap_err();
    faultpoint::disarm("storage.sidecar.before_save");
    let retained_error = host.provider_authority().snapshot().unwrap_err();
    host.refresh_provider_admission();
    assert_eq!(host.review().active_session(), Some(session));
    assert_eq!(host.review().head(session, 1).unwrap(), head);
    assert_eq!(host.provider_authority().snapshot().unwrap_err(), retained_error);
    assert!(!host.provider_authority().failed_publication_requires_handoff_recovery().unwrap());
    assert!(runtime.require_provider_collection_admission().is_err());
    assert_eq!(std::fs::read(config.manual_transfer_directory.join("provider.json")).unwrap(), before);
    assert!(!meguri_sync::provider::Authority::load(&config.manual_transfer_directory).snapshot().unwrap().settings.paused);
    assert!(host.collection().unwrap().reader(meguri_storage::ReaderRole::Query).is_ok());
    assert_eq!(host.save_provider_settings(settings).unwrap_err(), failure);
    host.retry_provider_publication().unwrap();
    assert_eq!(host.review().active_session(), Some(session));
    assert_eq!(host.review().head(session, 1).unwrap(), head);
    assert_eq!(host.cloud_connection_generation().unwrap(), generation);
    assert_eq!(meguri_engine::device_state::DeviceState::for_collection(
        &config.collection_directory.join("collection.anki2")).settled_publication().unwrap(), publication);
    assert!(host.provider_authority().snapshot().unwrap().settings.paused);
    assert!(runtime.require_provider_collection_admission().is_err());
    assert!(runtime.register_sdk(false).is_err());
    host.refresh_provider_admission();
    assert!(runtime.require_provider_collection_admission().is_err());
    meguri_engine::decks::add(&host.collection().unwrap(), "Study while paused".into()).unwrap();
    assert_eq!(host.review().active_session(), Some(session));
    host.review().end(session).unwrap();
    host.close().unwrap();
}

#[test]
fn recovery_refuses_a_changed_publication_and_preserves_the_original_durable_intent() {
    use meguri_sync::provider::Selection;
    let root = tempfile::tempdir().unwrap();
    let config = fixture(root.path(), Arc::new(Account(AtomicUsize::new(0))));
    let host = CoreHost::open(config.clone()).unwrap();
    drop(host.begin_provider_handoff(Selection::Meguri).unwrap());
    host.close().unwrap();
    drop(host);
    // Retain a stale comparison snapshot, never a completed authority or join.
    let path = config.manual_transfer_directory.join("provider.json");
    meguri_storage::sidecar::update_document(&path, |raw| {
        raw.get_mut("handoff").unwrap()["publication"]["decision"] = serde_json::json!("Pending");
        Ok(())
    }).unwrap();
    let before = std::fs::read(&path).unwrap();
    let reopened = CoreHost::open(config).unwrap();
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    assert!(reopened.provider_authority().network_blocked());
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(reopened.provider_authority().snapshot().unwrap().handoff.is_some());
    reopened.close().unwrap();
}

struct TransferBody(Option<bytes::Bytes>);
impl meguri_sync::transport::RawBody for TransferBody {
    fn next(&mut self) -> meguri_sync::transport::BoxFuture<'_, Result<Option<bytes::Bytes>, meguri_sync::SyncError>> {
        Box::pin(async move { Ok(self.0.take()) })
    }
}
struct RetainedUploadScript(std::sync::Mutex<std::collections::VecDeque<(meguri_sync::Method, Vec<u8>)>>);
struct TransferTransport(Arc<RetainedUploadScript>);
impl meguri_sync::SyncTransport for TransferTransport {
    fn post(&self, mut request: meguri_sync::transport::TransportRequest) -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
        Box::pin(async move {
            let (method, reply) = self.0.0.lock().unwrap().pop_front().expect("no request without its original operation");
            assert!(request.url.ends_with(&format!("/{}", method.name())));
            while let Some(chunk) = std::future::poll_fn(|cx| request.body.as_mut().poll_next(cx)).await { chunk.unwrap(); }
            Ok(meguri_sync::transport::TransportResponse {
                status: 200, headers: vec![("anki-original-size".into(), reply.len().to_string().into_bytes())],
                body: Box::new(TransferBody(Some(bytes::Bytes::from(zstd::stream::encode_all(reply.as_slice(), 0).unwrap())))),
            })
        })
    }
}
fn transfer_access(script: Arc<RetainedUploadScript>) -> meguri_sync::oneway::TransferAccess {
    let endpoint = meguri_sync::Endpoint::parse("https://manual.synthetic.example/prefix/").unwrap();
    meguri_sync::oneway::TransferAccess::new(endpoint.clone(), "Synthetic Account", meguri_sync::HostKey::new("synthetic-manual-key"),
        meguri_sync::JobOptions::new(endpoint, meguri_sync::ServerKind::SelfHosted)).unwrap()
        .with_transport_for_testing(Box::new(TransferTransport(script)))
}
fn complete_transfer<T>(events: meguri_engine::Subscription, task: meguri_host::ManualTransferTask<T>) -> Result<T, meguri_sync::oneway::TransferError> {
    loop {
        let event = events.recv().unwrap();
        if matches!(event.kind, meguri_engine::EventKind::JobFinished { id, .. } if id == task.job.id) { break; }
    }
    task.take_result().expect("original worker result")
}

#[test]
fn possibly_accepted_upload_survives_provider_configuration_handoff_refusal_and_reopen() {
    use meguri_storage::{ReadRows, ReaderRole};
    use meguri_sync::policy::{Acceptance, Direction, Scope};
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = fixture(&root.path().join("local"), account.clone());
    let server_config = fixture(&root.path().join("server"), Arc::new(Account(AtomicUsize::new(0))));
    let server = CoreHost::open(server_config.clone()).unwrap();
    let collection = server.collection().unwrap();
    let facts = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    drop(collection);
    server.close().unwrap();
    let server_bytes = std::fs::read(server_config.collection_directory.join("collection.anki2")).unwrap();
    let meta = serde_json::to_vec(&serde_json::json!({"mod":facts.modified.0,"scm":facts.schema_modified.0,"usn":facts.usn.0,
        "ts":1_894_701_600,"msg":"","cont":true,"empty":true,"media_usn":0})).unwrap();
    let script = Arc::new(RetainedUploadScript(std::sync::Mutex::new(std::collections::VecDeque::from([
        (meguri_sync::Method::Meta, meta.clone()), (meguri_sync::Method::Download, server_bytes),
        (meguri_sync::Method::Meta, meta.clone()), (meguri_sync::Method::Meta, meta),
    ]))));
    let host = CoreHost::open(config.clone()).unwrap();
    let events = host.engine().stream().subscribe();
    let preparation = host.prepare_manual_transfer(transfer_access(script.clone()), Direction::Export, Scope::CollectionOnly).unwrap();
    let preview = complete_transfer(events, preparation).unwrap();
    faultpoint::arm("oneway.upload.after_intent", Fault::IoError(std::io::ErrorKind::Other));
    let events = host.engine().stream().subscribe();
    let confirmation = host.confirm_manual_transfer(transfer_access(script.clone()), preview.confirmation).unwrap();
    let result = complete_transfer(events, confirmation);
    faultpoint::disarm("oneway.upload.after_intent");
    assert!(result.is_err());
    let operation = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(operation.acceptance, Acceptance::PossiblyAccepted);
    let policy_path = config.manual_transfer_directory.join("manual-transfers.json");
    let evidence = std::fs::read(&policy_path).unwrap();
    host.configure_meguri_connection("https://new.synthetic.example/").unwrap();
    assert!(host.begin_provider_handoff(meguri_sync::provider::Selection::Meguri).is_err());
    assert_eq!(std::fs::read(&policy_path).unwrap(), evidence);
    assert!(host.provider_authority().network_blocked());
    assert!(host.cancel_prepared_manual_transfer(&operation.id, false).is_err());
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    let retained = reopened.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(retained.id, operation.id);
    assert_eq!(retained.acceptance, Acceptance::PossiblyAccepted);
    assert!(reopened.provider_authority().network_blocked());
    assert!(reopened.recover_provider_handoff_to_local_only().is_err());
    assert!(reopened.retry_provider_publication().is_err());
    assert_eq!(std::fs::read(&policy_path).unwrap(), evidence);
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
    assert!(script.0.lock().unwrap().is_empty());
    reopened.close().unwrap();
}
