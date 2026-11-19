#[path = "support/meguri_bootstrap_script.rs"]
mod support;
use support::*;
use meguri_engine::{EngineError, LongOp, device_state::{DeviceState, PublicationDecision}, media::MediaJobKind};
use meguri_host::{CoreHost, MeguriBootstrapDirection as D, MeguriBootstrapError as E};
use meguri_sync::{Method, oneway::TransferAccess, policy::{Phase, Scope, Direction},
    provider::{Authority, MeguriReadiness as R, MeguriReadyRefusal as F, Selection}};
use std::fs;

fn bytes(config: &meguri_host::HostConfig) -> Vec<u8> { fs::read(config.manual_transfer_directory.join("provider.json")).unwrap() }
fn unbound(host: &CoreHost) { assert!(matches!(host.meguri_readiness().unwrap(), R::Unbound { .. })); }
fn leases_free(host: &CoreHost) {
    let media = host.media().unwrap();
    let slot = host.engine().acquire_slot(LongOp::OneWaySync).unwrap();
    let lease = media.try_job(MediaJobKind::Sync).unwrap();
    drop(lease); drop(slot);
}

#[cfg(unix)]
#[test]
fn meguri_server_preview_uses_opened_collection_through_ancestor_alias() {
    use meguri_storage::{ReadRows, ReaderRole};
    use std::os::unix::fs::symlink;

    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let mut config = config(&real.join("local"), std::sync::Arc::new(meguri_model::clock::FixedClock::new(
        NOW, meguri_model::clock::UtcOffset::UTC)));
    let destination = config.collection_directory.canonicalize().unwrap().join("collection.anki2");
    meguri_engine::bootstrap::create(&destination, config.clock.as_ref(), config.random.as_ref()).unwrap();
    // Alias an ancestor directory; the database itself remains a regular file.
    let alias = root.path().join("alias");
    symlink(&real, &alias).unwrap();
    config.collection_directory = alias.join("local/collection");
    config.backups_directory = alias.join("local/backups");
    let raw_path = config.collection_directory.join("collection.anki2");
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    let collection = host.collection().unwrap();
    assert_ne!(collection.path(), raw_path);
    assert_eq!(collection.path(), destination);
    assert!(!fs::symlink_metadata(&destination).unwrap().file_type().is_symlink());
    let connection = select(&host);
    let generation = collection.generation();
    let before = fs::read(&destination).unwrap();
    let col_before = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let state_path = config.manual_transfer_directory.join("manual-transfers.json");
    assert!(!state_path.exists());
    let (_, data) = server(&root.path().join("server"));
    let script = offline_preparation_script(data);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(
        access(&connection, &script), D::UseServerCollection).unwrap()).unwrap();
    script.assert_done();
    assert_eq!(script.count(Method::Meta), 2);
    assert_eq!(script.count(Method::Download), 1);
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(script.count(Method::MediaBegin), 0);
    assert_eq!(preview.operation.phase, Phase::Preview);
    assert_eq!(preview.operation.direction, Direction::Import);
    assert_eq!(preview.operation.actual_generation, Some(generation.0));
    assert!(state_path.is_file());
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&preview.operation));
    let current = host.collection().unwrap();
    assert_eq!(current.generation(), generation);
    assert_eq!(current.path(), destination);
    assert_eq!(fs::read(&destination).unwrap(), before);
    let reader = current.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.col().unwrap(), col_before);
    assert!(reader.config_value("bootstrap-server").unwrap().is_none());
    drop(reader);
    let discarded = host.discard_meguri_bootstrap_preparation(&connection, &preview.operation).unwrap();
    assert_eq!(discarded.phase, Phase::Cancelled);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    host.close().unwrap();
}

#[cfg(unix)]
#[test]
fn meguri_server_preview_rejects_forbidden_ancestor_alias_before_requests_or_journal() {
    use std::os::unix::fs::symlink;

    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let real = root.path().join("real");
    let mut config = config(&real.join("local"), std::sync::Arc::new(meguri_model::clock::FixedClock::new(
        NOW, meguri_model::clock::UtcOffset::UTC)));
    let destination = config.collection_directory.canonicalize().unwrap().join("collection.anki2");
    meguri_engine::bootstrap::create(&destination, config.clock.as_ref(), config.random.as_ref()).unwrap();
    let alias = root.path().join(".meguri-stage-alias");
    symlink(&real, &alias).unwrap();
    config.collection_directory = alias.join("local/collection");
    assert_eq!(config.collection_directory.canonicalize().unwrap().join("collection.anki2"), destination);
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    assert_eq!(host.collection().unwrap().path(), destination);
    let connection = select(&host);
    let state_path = config.manual_transfer_directory.join("manual-transfers.json");
    assert!(!state_path.exists());
    let offline = Script::new(vec![]);
    let result = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(
        access(&connection, &offline), D::UseServerCollection).unwrap());
    assert!(matches!(result, Err(meguri_sync::oneway::TransferError::Artifact)));
    offline.assert_done();
    assert!(!state_path.exists());
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    host.close().unwrap();
}

#[test]
fn meguri_initial_server_download_can_start_without_a_local_collection() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(&root.path().join("local"), std::sync::Arc::new(meguri_model::clock::FixedClock::new(
        NOW, meguri_model::clock::UtcOffset::UTC)));
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.open_for_manual_import(config).unwrap();
    assert!(host.collection().is_err());
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, false);
    let preview = prepare(&host, &connection, &script, D::UseServerCollection);
    let op = confirm(&host, &connection, &script, preview);
    assert!(op.safety_backups.is_empty());
    let done = finish_empty_media(&host, &connection, &op);
    host.complete_meguri_bootstrap(&connection, USER, &done.id).unwrap();
    assert!(matches!(host.meguri_readiness().unwrap(), R::Ready));
    host.close().unwrap();
}

#[test]
fn meguri_server_download_completes_collection_but_stays_unbound_until_media_accounted() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let prior = DeviceState::for_collection(host.collection().unwrap().path()).settled_publication().unwrap().unwrap();
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, false);
    let preview = prepare(&host, &connection, &script, D::UseServerCollection);
    assert_eq!(preview.operation.scope, Scope::CollectionAndMedia);
    let op = confirm(&host, &connection, &script, preview);
    assert!(op.collection_complete && op.receipt.is_none());
    assert_eq!(op.safety_backups.len(), 1);
    assert!(config.backups_directory.join(&op.safety_backups[0].name).is_file());
    let now = DeviceState::for_collection(host.collection().unwrap().path()).settled_publication().unwrap().unwrap();
    assert_ne!(prior.identity, now.identity);
    assert_eq!(now.decision, PublicationDecision::Pending);
    assert_eq!(op.resulting_publication, Some(now.identity));
    assert_eq!(script.count(Method::MediaBegin), 0);
    unbound(&host);
    assert!(matches!(host.complete_meguri_bootstrap(&connection, USER, &op.id), Err(E::Ready(F::MediaEpochMissing))));
    assert!(!host.provider_authority().snapshot().unwrap().settings.automatic);
    host.close().unwrap();
}

#[test]
fn meguri_device_upload_retains_backup_and_server_copy_without_binding() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, _, op) = selected_collection(root.path());
    assert_eq!(op.direction, Direction::Export);
    assert_eq!(op.safety_backups.len(), 1);
    assert!(op.server_copy.is_some());
    assert!(config.backups_directory.join(&op.safety_backups[0].name).is_file());
    assert!(config.backups_directory.join(&op.server_copy.as_ref().unwrap().artifact.name).is_file());
    unbound(&host);
    host.close().unwrap();
}

#[test]
fn meguri_ready_binding_uses_actual_operation_account_publication_and_epoch_across_reopen() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = ready(root.path());
    let binding = host.provider_authority().snapshot().unwrap().meguri_binding.unwrap();
    assert_eq!(binding.account(), &op.account);
    assert_eq!(binding.operation(), &op.id);
    let baseline = host.media().unwrap().store().sync_baseline().unwrap();
    assert_eq!(binding.media_owner(), baseline.owner.clone().unwrap());
    assert!(binding.media_epoch().matches(&baseline));
    assert!(host.provider_authority().permits_meguri_binding(&connection,
        DeviceState::for_collection(host.collection().unwrap().path()).settled_publication().unwrap().as_ref()));
    let doc = host.provider_authority().snapshot().unwrap();
    assert!(doc.settings.automatic && doc.meguri_settings.automatic && doc.settings.enabled);
    assert!(doc.meguri_bootstrap.is_none());
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    assert!(matches!(reopened.meguri_readiness().unwrap(), R::Ready));
    assert_eq!(reopened.media().unwrap().store().sync_baseline().unwrap().epoch, baseline.epoch);
    assert_eq!(reopened.provider_authority().snapshot().unwrap().meguri_binding, Some(binding));
    reopened.close().unwrap();
}

#[test]
fn meguri_media_retry_after_restart_validates_recorded_epoch() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = selected_collection(root.path());
    let partial = media_run(&host, &connection, &Script::new(vec![(Method::MediaBegin, Reply::Lost)]), media_preview(&host, &connection, &op));
    assert!(partial.receipt.is_none());
    let epoch = host.provider_authority().meguri_bootstrap_intent().unwrap().media_epoch().unwrap().clone();
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    settle(&reopened);
    let done = finish_empty_media(&reopened, &connection, &partial);
    reopened.complete_meguri_bootstrap(&connection, USER, &done.id).unwrap();
    assert_eq!(reopened.provider_authority().snapshot().unwrap().meguri_binding.unwrap().media_epoch(), &epoch);
    reopened.close().unwrap();
}

#[test]
fn meguri_excluded_file_prevents_ready_and_automatic_sync() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _, connection, op) = selected_collection(root.path());
    fs::write(host.media().unwrap().store().media_dir().join("empty.synthetic"), []).unwrap();
    let done = finish_empty_media(&host, &connection, &op);
    assert_eq!(done.phase, Phase::Complete);
    match host.complete_meguri_bootstrap(&connection, USER, &done.id) {
        Err(E::Ready(F::MediaIncomplete(accounting))) => {
            assert!(!accounting.is_complete());
            assert_eq!(accounting.affected().count(), 1);
            assert!(!format!("{accounting:?}").contains("empty.synthetic"));
        }
        _ => panic!("excluded file must block binding"),
    }
    host.refresh_provider_admission();
    unbound(&host);
    assert!(!host.provider_authority().snapshot().unwrap().settings.automatic);
    host.close().unwrap();
}

#[test]
fn meguri_finalization_reenumerates_files_changed_after_media_join() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _, connection, op) = selected_collection(root.path());
    let done = finish_empty_media(&host, &connection, &op);
    fs::write(host.media().unwrap().store().media_dir().join("new.synthetic"), b"pending bytes").unwrap();
    assert!(matches!(host.complete_meguri_bootstrap(&connection, USER, &done.id), Err(E::Ready(F::MediaIncomplete(_)))));
    host.close().unwrap();
}

#[test]
fn meguri_bootstrap_checks_provider_connection_and_active_operation_before_request() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _) = seeded(root.path());
    let connection = host.configure_meguri_connection(ENDPOINT).unwrap();
    let offline = Script::new(vec![]);
    assert!(matches!(host.prepare_meguri_bootstrap(access(&connection, &offline), D::UseDeviceCollection), Err(E::NotSelected)));
    let handoff = host.begin_provider_handoff(Selection::Meguri).unwrap();
    host.finish_provider_handoff(&handoff).unwrap();
    let newer = host.configure_meguri_connection("https://changed.synthetic.example/").unwrap();
    assert!(matches!(host.prepare_meguri_bootstrap(access(&connection, &offline), D::UseDeviceCollection), Err(E::ConnectionChanged)));
    assert_ne!(newer, connection);
    offline.assert_done();
    let connection = host.configure_meguri_connection(ENDPOINT).unwrap();
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, true);
    let preview = prepare(&host, &connection, &script, D::UseDeviceCollection);
    assert!(matches!(host.prepare_meguri_bootstrap(access(&connection, &offline), D::UseServerCollection), Err(E::Busy)));
    host.cancel_prepared_manual_transfer(&preview.operation.id, false).unwrap();
    host.close().unwrap();
}

#[test]
fn meguri_ready_and_held_long_jobs_refuse_bootstrap_without_requests() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _, connection, op) = ready(root.path());
    let offline = Script::new(vec![]);
    assert!(matches!(host.prepare_meguri_bootstrap(access(&connection, &offline), D::UseDeviceCollection), Err(E::AlreadyBound)));
    assert!(matches!(host.complete_meguri_bootstrap(&connection, USER, &op.id), Err(E::AlreadyBound)));
    offline.assert_done();
    host.close().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _, connection, op) = selected_collection(root.path());
    let done = finish_empty_media(&host, &connection, &op);
    let slot = host.engine().acquire_slot(LongOp::OneWaySync).unwrap();
    assert!(host.complete_meguri_bootstrap(&connection, USER, &done.id).is_err());
    drop(slot);
    let media = host.media().unwrap();
    let lease = media.try_job(MediaJobKind::Sync).unwrap();
    assert!(host.complete_meguri_bootstrap(&connection, USER, &done.id).is_err());
    drop(lease);
    leases_free(&host);
    host.close().unwrap();
}

#[test]
fn meguri_ready_evidence_refuses_wrong_account_without_document_write() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = selected_collection(root.path());
    let done = finish_empty_media(&host, &connection, &op);
    let before = bytes(&config);
    assert!(matches!(host.complete_meguri_bootstrap(&connection, "other-user", &done.id), Err(E::Ready(F::AccountChanged))));
    assert_eq!(bytes(&config), before);
    unbound(&host);
    leases_free(&host);
    host.close().unwrap();
}

#[test]
fn meguri_ready_commit_runs_through_g12_with_its_host_protections() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = selected_collection(root.path());
    let done = finish_empty_media(&host, &connection, &op);
    let g = host.cloud_connection_generation().unwrap();
    let before = bytes(&config);
    assert!(matches!(host.with_close_state_held_for_testing(|| host.complete_meguri_bootstrap_for_generation(g, &connection, USER, &done.id)),
        Err(E::Busy) | Err(E::Engine(EngineError::Busy { .. }))));
    assert_eq!(bytes(&config), before);
    unbound(&host);
    leases_free(&host);
    host.complete_meguri_bootstrap_for_generation(g, &connection, USER, &done.id).unwrap();
    assert!(host.provider_authority().snapshot().unwrap().meguri_binding.is_some());
    leases_free(&host);
    host.close().unwrap();

    g12_retargeted_alias_is_refused_after_ready_preparation();

    // A separate retained collection-only replacement changes publication after media.
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = selected_collection(root.path());
    let done = finish_empty_media(&host, &connection, &op);
    let (_, data) = server(&root.path().join("next-server"));
    let script = collection_script(data, false);
    let legacy = || {
        let endpoint = meguri_sync::Endpoint::parse(ENDPOINT).unwrap();
        TransferAccess::new(endpoint.clone(), USER, meguri_sync::HostKey::new("synthetic-key"),
            meguri_sync::JobOptions::new(endpoint, meguri_sync::ServerKind::SelfHosted)).unwrap()
            .with_transport_for_testing(Box::new(ScriptTransport(script.clone())))
    };
    let events = host.engine().stream().subscribe();
    let preview = complete(events, host.prepare_manual_transfer(legacy(), Direction::Import, Scope::CollectionOnly).unwrap()).unwrap();
    let events = host.engine().stream().subscribe();
    complete(events, host.confirm_manual_transfer(legacy(), preview.confirmation).unwrap()).unwrap();
    script.assert_done();
    let before = bytes(&config);
    assert!(matches!(host.complete_meguri_bootstrap(&connection, USER, &done.id), Err(E::Ready(F::PublicationChanged))));
    assert_eq!(bytes(&config), before);
    host.refresh_provider_admission();
    unbound(&host);
    host.close().unwrap();
}

fn g12_retargeted_alias_is_refused_after_ready_preparation() {
    use meguri_engine::{ChangeObserver, GenerationListener, OpenStep, SuspendParticipant};
    use std::{path::Path, sync::{Arc, Weak}, os::unix::fs::symlink};
    struct OriginalReview(Weak<CoreHost>);
    impl OpenStep for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn run(&self, collection: &meguri_engine::Collection) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().run(collection)
        }
    }
    impl ChangeObserver for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn observe(&self, commit: &meguri_engine::changes::observers::ObservedCommit<'_>) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().observe(commit)
        }
    }
    impl GenerationListener for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn collection_reopened(&self, collection: &meguri_engine::Collection) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().collection_reopened(collection)
        }
        fn collection_replaced(&self, old: meguri_engine::Generation, new: meguri_engine::Generation) {
            self.0.upgrade().unwrap().review().collection_replaced(old, new);
        }
    }
    impl SuspendParticipant for OriginalReview {
        fn name(&self) -> &str { "review-session" }
        fn flush(&self) -> Result<(), EngineError> { self.0.upgrade().unwrap().review().flush() }
        fn flush_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().flush_until(deadline)
        }
        fn suspend(&self) -> Result<(), EngineError> { self.0.upgrade().unwrap().review().suspend() }
        fn suspend_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().suspend_until(deadline)
        }
        fn release_connections_until(&self, deadline: std::time::Duration) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().release_connections_until(deadline)
        }
        fn resume(&self) -> Result<(), EngineError> { self.0.upgrade().unwrap().review().resume() }
        fn resume_with_collection(&self, collection: &meguri_engine::Collection) -> Result<(), EngineError> {
            self.0.upgrade().unwrap().review().resume_with_collection(collection)
        }
    }
    fn copy_fixture(from: &Path, to: &Path) {
        fs::create_dir_all(to).unwrap();
        for entry in fs::read_dir(from).unwrap() {
            let entry = entry.unwrap();
            let target = to.join(entry.file_name());
            if entry.file_type().unwrap().is_dir() { copy_fixture(&entry.path(), &target); }
            else { fs::copy(entry.path(), target).unwrap(); }
        }
    }
    let root = tempfile::tempdir().unwrap();
    let (initial, mut config) = seeded(&root.path().join("original"));
    let connection = select(&initial);
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, true);
    let preview = prepare(&initial, &connection, &script, D::UseDeviceCollection);
    let op = confirm(&initial, &connection, &script, preview);
    let done = finish_empty_media(&initial, &connection, &op);
    initial.close().unwrap();
    drop(initial);
    let original_directory = config.collection_directory.clone();
    let alias = root.path().join("alias");
    symlink(&original_directory, &alias).unwrap();
    config.collection_directory = alias.clone();
    let host = Arc::new(CoreHost::open(config.clone()).unwrap());
    settle(&host);
    let before = bytes(&config);
    host.engine().close().unwrap();
    // Clone only the closed synthetic fixture, including its settled publication
    // and media journal. Finalization preparation can then reach the path fence.
    let foreign = root.path().join("foreign");
    copy_fixture(&original_directory, &foreign);
    fs::remove_file(&alias).unwrap();
    symlink(&foreign, &alias).unwrap();
    let review = Arc::new(OriginalReview(Arc::downgrade(&host)));
    let ports = host.derived().ports(meguri_engine::Ports::new()).open_step(review.clone())
        .observer(review.clone()).generation_listener(review.clone()).suspend_participant(review);
    let replacement = host.engine().open_raw(&foreign.canonicalize().unwrap().join("collection.anki2"),
        meguri_engine::OpenConfig::new(config.clock.clone(), config.random.clone()).ports(ports)).unwrap();
    let g = replacement.generation();
    assert!(matches!(host.complete_meguri_bootstrap_for_generation(g, &connection, USER, &done.id),
        Err(E::Engine(EngineError::CollectionUnavailable { reason })) if reason == "provider metadata collection owner changed"));
    assert_eq!(bytes(&config), before);
    leases_free(&host);
    drop(replacement);
    host.close().unwrap();
}

#[test]
fn meguri_full_sync_hold_clears_exact_binding_and_requires_rebootstrap() {
    let _lane = LANE.lock().unwrap();
    for paused in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (host, config, connection, _) = ready(root.path());
        let binding = host.provider_authority().snapshot().unwrap().meguri_binding.unwrap();
        let mut settings = host.provider_authority().snapshot().unwrap().settings;
        settings.paused = paused;
        host.save_provider_settings(settings).unwrap();
        let settled = DeviceState::for_collection(host.collection().unwrap().path()).settled_publication().unwrap();
        let g = host.cloud_connection_generation().unwrap();
        let mut other = serde_json::to_value(&binding).unwrap();
        other["connection"]["endpoint"] = serde_json::json!("https://different.synthetic.example/");
        let other = serde_json::from_value(other).unwrap();
        let before = bytes(&config);
        assert!(matches!(host.provider_authority().hold_meguri_binding_for_full_sync(&other, g.0, settled.clone()), Err(F::BindingChanged)));
        assert_eq!(bytes(&config), before);
        host.refresh_provider_admission();
        host.provider_authority().hold_meguri_binding_for_full_sync(&binding, g.0, settled).unwrap();
        let doc = host.provider_authority().snapshot().unwrap();
        assert!(!doc.settings.automatic && !doc.meguri_settings.automatic);
        assert_eq!(doc.settings.paused, paused);
        assert!(!doc.permits_meguri_transfer());
        unbound(&host);
        // Bootstrap remains an explicit decision even when automatic work was paused.
        let (_, data) = server(&root.path().join("again"));
        let script = collection_script(data, true);
        let preview = prepare(&host, &connection, &script, D::UseDeviceCollection);
        assert_eq!(preview.operation.scope, Scope::CollectionAndMedia);
        host.cancel_prepared_manual_transfer(&preview.operation.id, false).unwrap();
        host.close().unwrap();
    }
}

#[test]
fn meguri_bootstrap_establishes_fresh_epoch_once_and_validates_on_resume() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, first) = ready(root.path());
    let binding = host.provider_authority().snapshot().unwrap().meguri_binding.unwrap();
    let store = host.media().unwrap().store().clone();
    let e0 = store.sync_baseline().unwrap().epoch;
    let p = DeviceState::for_collection(host.collection().unwrap().path()).settled_publication().unwrap();
    host.provider_authority().hold_meguri_binding_for_full_sync(&binding, host.cloud_connection_generation().unwrap().0, p).unwrap();
    fs::write(store.media_dir().join("keep.synthetic"), b"retained bytes").unwrap();
    drop(store);
    let (_, data) = server(&root.path().join("again"));
    let script = collection_script(data, true);
    let preview = prepare(&host, &connection, &script, D::UseDeviceCollection);
    assert_ne!(preview.operation.id, first.id);
    let op = confirm(&host, &connection, &script, preview);
    let store = host.media().unwrap().store().clone();
    let partial = media_run(&host, &connection, &Script::new(vec![(Method::MediaBegin, Reply::Lost)]), media_preview(&host, &connection, &op));
    let epoch = host.provider_authority().meguri_bootstrap_intent().unwrap().media_epoch().unwrap().clone();
    assert_ne!(epoch.epoch(), e0);
    assert_eq!(fs::read(store.media_dir().join("keep.synthetic")).unwrap(), b"retained bytes");
    media_run(&host, &connection, &Script::new(vec![(Method::MediaBegin, Reply::Lost)]), media_preview(&host, &connection, &partial));
    assert_eq!(store.sync_baseline().unwrap().epoch, epoch.epoch());
    drop(store);
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    settle(&reopened);
    // A real upload acknowledgement followed by sanity completes the retained file.
    let script = Script::new(vec![(Method::MediaBegin, media_begin(0)),
        (Method::UploadChanges, media_answer(serde_json::json!([1, 1]))),
        (Method::MediaSanity, media_answer(serde_json::json!("OK")))]);
    let done = media_run(&reopened, &connection, &script, media_preview(&reopened, &connection, &partial));
    reopened.complete_meguri_bootstrap(&connection, USER, &done.id).unwrap();
    assert_eq!(reopened.provider_authority().snapshot().unwrap().meguri_binding.unwrap().media_epoch(), &epoch);
    reopened.close().unwrap();

    let root = tempfile::tempdir().unwrap();
    let (host, _, connection, op) = selected_collection(root.path());
    let partial = media_run(&host, &connection, &Script::new(vec![(Method::MediaBegin, Reply::Lost)]), media_preview(&host, &connection, &op));
    let preview = media_preview(&host, &connection, &partial);
    let media = host.media().unwrap();
    media.store().reset_sync().unwrap();
    let tampered = media.store().sync_baseline().unwrap().epoch;
    let offline = Script::new(vec![]);
    assert!(matches!(host.run_meguri_bootstrap_media(access(&connection, &offline), preview.confirmation), Err(E::Ready(F::MediaEpochChanged))));
    assert_eq!(media.store().sync_baseline().unwrap().epoch, tampered);
    offline.assert_done();
    unbound(&host);
    host.close().unwrap();
}

#[test]
fn meguri_binding_clear_keeps_the_auto_invariant() {
    let _lane = LANE.lock().unwrap();
    for case in ["reconfigure", "icloud", "local", "recovery"] {
        let root = tempfile::tempdir().unwrap();
        let (host, config, _, _) = ready(root.path());
        let before = host.provider_authority().snapshot().unwrap();
        assert!(before.settings.automatic && before.meguri_settings.automatic);
        let icloud = serde_json::to_vec(&before.icloud_settings).unwrap();
        let host = if case == "recovery" {
            host.begin_provider_handoff(Selection::ICloud).unwrap();
            host.close().unwrap();
            let reopened = CoreHost::open(config.clone()).unwrap();
            reopened.recover_provider_handoff_to_local_only().unwrap();
            reopened
        } else {
            if case == "reconfigure" { host.configure_meguri_connection("https://new.synthetic.example/").unwrap(); }
            else {
                let destination = if case == "icloud" { Selection::ICloud } else { Selection::LocalOnly };
                let handoff = host.begin_provider_handoff(destination).unwrap();
                host.finish_provider_handoff(&handoff).unwrap();
            }
            host
        };
        let doc = host.provider_authority().snapshot().unwrap();
        assert!(doc.meguri_binding.is_none() && doc.meguri_bootstrap.is_none());
        assert!(!doc.meguri_settings.automatic);
        assert_eq!(doc.settings.paused, before.settings.paused);
        assert_eq!(serde_json::to_vec(&doc.icloud_settings).unwrap(), icloud);
        assert!(Authority::load(&config.manual_transfer_directory).snapshot().is_ok());
        match case {
            "reconfigure" => { assert_eq!(doc.selection, Selection::Meguri); assert!(!doc.settings.automatic); }
            "icloud" => { assert_eq!(doc.selection, Selection::ICloud); let mut expected = doc.icloud_settings; expected.enabled = true; assert_eq!(doc.settings, expected); }
            "local" => { let mut expected = doc.icloud_settings; expected.enabled = false; assert_eq!(doc.settings, expected); }
            _ => { assert_eq!(doc.selection, Selection::LocalOnly); assert!(!doc.settings.automatic); }
        }
        if case == "reconfigure" || case == "icloud" {
            let handoff = host.begin_provider_handoff(Selection::Meguri).unwrap();
            host.finish_provider_handoff(&handoff).unwrap();
            assert!(!host.provider_authority().snapshot().unwrap().settings.automatic);
        }
        host.close().unwrap();
    }
}

// New worker rendezvous use one deadline for the entire wait, including
// unrelated events. No pending-policy identity is manufactured by these tests.
fn bootstrap_complete<T>(host: &CoreHost, start: impl FnOnce() -> meguri_host::ManualTransferTask<T>)
    -> Result<T, meguri_sync::oneway::TransferError> {
    let events = host.engine().stream().subscribe();
    let task = start();
    let deadline = meguri_engine::monotonic::now() + std::time::Duration::from_secs(20);
    loop {
        let remaining = deadline.saturating_sub(meguri_engine::monotonic::now());
        let event = events.recv_timeout(remaining).expect("bootstrap worker deadline");
        if matches!(event.kind, meguri_engine::EventKind::JobFinished { id, .. } if id == task.job.id) { break; }
    }
    task.take_result().expect("actual typed bootstrap result")
}
fn offline_preparation_script(data: Vec<u8>) -> std::sync::Arc<Script> {
    Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(data)), (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))])
}
fn retained_artifacts(config: &meguri_host::HostConfig, op: &meguri_sync::policy::Operation) -> Vec<(std::path::PathBuf, Vec<u8>)> {
    let mut paths = op.safety_backups.iter().map(|a| config.backups_directory.join(&a.name)).collect::<Vec<_>>();
    if let Some(copy) = &op.server_copy { paths.push(config.backups_directory.join(&copy.artifact.name)); }
    if let Some(candidate) = &op.candidate {
        paths.push(config.manual_transfer_directory.join(op.id.as_str()).join(&candidate.name));
    }
    assert!(!paths.is_empty());
    paths.into_iter().map(|path| { let bytes = fs::read(&path).unwrap(); (path, bytes) }).collect()
}
fn assert_artifacts(artifacts: &[(std::path::PathBuf, Vec<u8>)]) {
    for (path, before) in artifacts { assert_eq!(&fs::read(path).unwrap(), before); }
}
fn discard_preparation_roundtrip(cold: bool) {
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = offline_preparation_script(data.clone());
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    script.assert_done();
    assert_eq!(preview.operation.scope, Scope::CollectionAndMedia);
    assert!(host.provider_authority().meguri_bootstrap_intent().is_none());
    let artifacts = retained_artifacts(&config, &preview.operation);
    host.refresh_provider_admission();
    assert!(host.provider_authority().network_blocked());
    let host = if cold { host.close().unwrap(); CoreHost::open(config.clone()).unwrap() } else { host };
    settle(&host);
    let actual = host.pending_meguri_bootstrap_preparation(&connection, " Synthetic-User\t").unwrap().unwrap();
    assert_eq!(actual, preview.operation);
    let provider_before = bytes(&config);
    let cancelled = host.discard_meguri_bootstrap_preparation(&connection, &actual).unwrap();
    assert_eq!(cancelled.phase, Phase::Cancelled);
    assert_eq!(cancelled.boundary, Some(meguri_sync::policy::Boundary::Cancelled));
    assert!(cancelled.receipt.is_none());
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert_eq!(host.manual_transfer_history().get(&actual.id).unwrap(), cancelled);
    assert!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().is_none());
    assert!(!host.provider_authority().network_blocked());
    assert_eq!(bytes(&config), provider_before);
    assert_artifacts(&artifacts);
    let offline = Script::new(vec![]);
    assert!(matches!(host.confirm_meguri_bootstrap(access(&connection, &offline), preview), Err(E::Busy)));
    offline.assert_done();
    for method in [Method::Upload, Method::MediaBegin, Method::UploadChanges, Method::MediaChanges, Method::DownloadFiles, Method::MediaSanity] {
        assert_eq!(script.count(method), 0);
    }
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &actual), Err(E::Ready(F::OperationMismatch))));
    let again = offline_preparation_script(data);
    let next = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &again), D::UseDeviceCollection).unwrap()).unwrap();
    again.assert_done();
    assert_ne!(next.operation.id, actual.id);
    host.discard_meguri_bootstrap_preparation(&connection, &next.operation).unwrap();
    host.close().unwrap();
}
#[test]
fn meguri_discard_live_preparation_preserves_artifacts_and_reopens_explicit_admission() {
    let _lane = LANE.lock().unwrap();
    discard_preparation_roundtrip(false);
}
#[test]
fn meguri_cold_pending_preparation_discard_preserves_history_and_allows_explicit_prepare() {
    let _lane = LANE.lock().unwrap();
    discard_preparation_roundtrip(true);
}
#[test]
fn meguri_pending_and_discard_keep_original_generation_and_close_ownership_guards() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = offline_preparation_script(data);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    let generation = host.cloud_connection_generation().unwrap();
    let wrong = meguri_engine::Generation(generation.0 + 1);
    let before = bytes(&config);
    assert!(matches!(host.pending_meguri_bootstrap_preparation_for_generation(wrong, &connection, USER),
        Err(E::Engine(EngineError::StaleGeneration { .. }))));
    assert!(matches!(host.discard_meguri_bootstrap_preparation_for_generation(wrong, &connection, &preview.operation),
        Err(E::Engine(EngineError::StaleGeneration { .. }))));
    let held = host.with_close_state_held_for_testing(||
        host.discard_meguri_bootstrap_preparation_for_generation(generation, &connection, &preview.operation));
    assert!(matches!(held, Err(E::Engine(EngineError::Busy { .. }))));
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&preview.operation));
    assert_eq!(bytes(&config), before);
    script.assert_done();
    host.discard_meguri_bootstrap_preparation(&connection, &preview.operation).unwrap();
    host.close().unwrap();
}
#[test]
fn meguri_pending_preparation_checks_connection_account_and_actual_revision_offline() {
    use meguri_sync::{oneway::TransferError, policy::Boundary};
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = offline_preparation_script(data);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    let before = bytes(&config);
    for field in ["reference", "revision", "endpoint"] {
        let mut wrong = connection.clone();
        match field { "reference" => wrong.reference.push('x'), "revision" => wrong.revision += 1,
            _ => wrong.endpoint = "https://foreign.synthetic.example/".into() }
        assert!(matches!(host.pending_meguri_bootstrap_preparation(&wrong, USER), Err(E::ConnectionChanged)));
        assert!(matches!(host.discard_meguri_bootstrap_preparation(&wrong, &preview.operation), Err(E::ConnectionChanged)));
    }
    assert!(matches!(host.pending_meguri_bootstrap_preparation(&connection, "other-user"), Err(E::Ready(F::AccountChanged))));
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&preview.operation));
    // NotSent has no recovery confirmation. This is the real retained worker,
    // which records the interruption decision without creating another writer.
    let recovered = bootstrap_complete(&host, || host.prepare_meguri_bootstrap_recovery(&connection, &preview.operation).unwrap());
    assert!(matches!(recovered, Err(TransferError::NeedsDecision(Boundary::PreparationInterrupted))));
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &preview.operation), Err(E::Ready(F::OperationMismatch))));
    let actual = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    assert_eq!(actual.id, preview.operation.id);
    assert_eq!(actual.phase, Phase::NeedsDecision);
    host.discard_meguri_bootstrap_preparation(&connection, &actual).unwrap();
    script.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(script.count(Method::MediaBegin), 0);
    assert_eq!(bytes(&config), before);
    host.close().unwrap();
    assert!(matches!(host.pending_meguri_bootstrap_preparation(&connection, USER), Err(E::Engine(_))));
}
#[test]
fn meguri_lost_ack_offline_recovery_and_discard_preserve_global_exclusion() {
    use meguri_sync::{oneway::TransferError, policy::{Acceptance, Boundary}};
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(data)), (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))), (Method::Upload, Reply::Lost)]);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    assert!(bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &script), preview).unwrap()).is_err());
    script.assert_done();
    let retained = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(retained.acceptance, Acceptance::PossiblyAccepted);
    let artifacts = retained_artifacts(&config, &retained);
    host.close().unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    let actual = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    let before = bytes(&config);
    assert!(matches!(bootstrap_complete(&host, || host.prepare_meguri_bootstrap_recovery(&connection, &actual).unwrap()),
        Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown))));
    let actual = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &actual),
        Err(E::Transfer(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown)))));
    assert_eq!(host.manual_transfer_history().active().unwrap(), Some(actual));
    assert!(host.provider_authority().network_blocked());
    let offline = Script::new(vec![]);
    assert!(matches!(host.prepare_meguri_bootstrap(access(&connection, &offline), D::UseDeviceCollection), Err(E::Busy)));
    offline.assert_done();
    assert_eq!(script.count(Method::Upload), 1);
    assert_eq!(script.count(Method::Meta), 3);
    assert_eq!(script.count(Method::MediaBegin), 0);
    assert_eq!(bytes(&config), before);
    assert_artifacts(&artifacts);
    host.close().unwrap();
}
#[test]
fn meguri_exact_ok_offline_recovery_confirms_retained_candidate_without_second_upload() {
    use meguri_sync::{oneway::TransferError, policy::{Acceptance, Boundary}};
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, true);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    let digest = preview.operation.candidate.as_ref().unwrap().identity.sha256;
    meguri_model::faultpoint::arm("oneway.upload.after_ok",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other));
    let confirmed = bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &script), preview).unwrap());
    meguri_model::faultpoint::disarm_all();
    assert!(confirmed.is_err());
    script.assert_done();
    assert_eq!(host.manual_transfer_history().active().unwrap().unwrap().acceptance, Acceptance::ExactOk);
    host.close().unwrap();
    let host = CoreHost::open(config).unwrap();
    settle(&host);
    let actual = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap_recovery(&connection, &actual).unwrap()).unwrap();
    assert_eq!(preview.operation.acceptance, Acceptance::ExactOk);
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &preview.operation),
        Err(E::Transfer(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown)))));
    let offline = Script::new(vec![]);
    let done = bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &offline), preview).unwrap()).unwrap();
    offline.assert_done();
    assert!(done.collection_complete && done.receipt.is_none());
    assert_eq!(done.scope, Scope::CollectionAndMedia);
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &done),
        Err(E::Transfer(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown)))));
    assert_eq!(script.count(Method::Upload), 1);
    for method in [Method::Upload, Method::Meta, Method::MediaBegin] { assert_eq!(offline.count(method), 0); }
    assert_eq!(DeviceState::for_collection(host.collection().unwrap().path()).replacement_evidence().unwrap().unwrap().candidate_file.sha256, digest);
    unbound(&host);
    host.close().unwrap();
}

fn discarded_failed_confirmation_allows_next_preview(cold: bool) {
    use meguri_sync::{oneway::TransferError, policy::{Acceptance, Boundary}};
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    // A genuine changed remote observation fails confirmation before upload,
    // while retaining both the provider intent and the actual decision row.
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(data.clone())), (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW, 9)))]);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    let first_id = preview.operation.id.clone();
    let prepared_generation = preview.operation.actual_generation.expect("real preparation captures a generation");
    assert!(matches!(bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &script), preview).unwrap()),
        Err(TransferError::Stale(Boundary::RemoteChanged))));
    script.assert_done();
    let first = host.manual_transfer_history().get(&first_id).unwrap();
    assert_eq!(first.acceptance, Acceptance::NotSent);
    assert_eq!(first.phase, Phase::NeedsDecision);
    assert_eq!(first.actual_generation, Some(prepared_generation));
    assert_eq!(host.provider_authority().meguri_bootstrap_intent().unwrap().operation(), &first_id);
    let first_artifacts = retained_artifacts(&config, &first);
    let provider_before = bytes(&config);
    let host = if cold { host.close().unwrap(); CoreHost::open(config.clone()).unwrap() } else { host };
    settle(&host);
    let pending = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    assert_eq!(pending, first);
    let cancelled_first = host.discard_meguri_bootstrap_preparation(&connection, &pending).unwrap();
    assert_eq!(cancelled_first.phase, Phase::Cancelled);
    assert_eq!(cancelled_first.actual_generation, Some(prepared_generation));
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert!(!host.provider_authority().network_blocked());
    // The cold case also proves the stale intent and real cancellation survive
    // reopening before the next explicit preparation.
    let host = if cold { host.close().unwrap(); CoreHost::open(config.clone()).unwrap() } else { host };
    settle(&host);
    let next_script = offline_preparation_script(data);
    let next = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &next_script), D::UseDeviceCollection).unwrap()).unwrap();
    next_script.assert_done();
    assert_ne!(next.operation.id, first_id);
    let next_artifacts = retained_artifacts(&config, &next.operation);
    assert_eq!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().as_ref(), Some(&next.operation));
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &cancelled_first), Err(E::Ready(F::OperationMismatch))));
    assert!(matches!(host.pending_meguri_bootstrap_preparation(&connection, "other-user"), Err(E::Ready(F::AccountChanged))));
    let cancelled_next = host.discard_meguri_bootstrap_preparation(&connection, &next.operation).unwrap();
    assert_eq!(cancelled_next.phase, Phase::Cancelled);
    assert!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().is_none());
    assert!(!host.provider_authority().network_blocked());
    assert_eq!(host.manual_transfer_history().get(&first_id).unwrap(), cancelled_first);
    assert_eq!(host.manual_transfer_history().get(&next.operation.id).unwrap(), cancelled_next);
    assert_eq!(host.manual_transfer_history().history().unwrap().len(), 2);
    assert_eq!(bytes(&config), provider_before);
    assert_artifacts(&first_artifacts);
    assert_artifacts(&next_artifacts);
    for method in [Method::Upload, Method::MediaBegin, Method::UploadChanges, Method::MediaChanges, Method::DownloadFiles, Method::MediaSanity] {
        assert_eq!(script.count(method), 0);
        assert_eq!(next_script.count(method), 0);
    }
    host.close().unwrap();
}
#[test]
fn meguri_warm_cancelled_failed_confirmation_intent_allows_next_preview_and_discard() {
    let _lane = LANE.lock().unwrap();
    discarded_failed_confirmation_allows_next_preview(false);
}
#[test]
fn meguri_cold_cancelled_failed_confirmation_intent_allows_next_preview_and_discard() {
    let _lane = LANE.lock().unwrap();
    discarded_failed_confirmation_allows_next_preview(true);
}

fn nonactive_failed_confirmation_allows_fresh_preview_discard_and_confirm(cold: bool) {
    use meguri_sync::{oneway::TransferError, policy::{Acceptance, Boundary, MediaOutcome}};
    use std::os::unix::fs::MetadataExt;
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    // The final read-only Meta fails before Upload or local activation.
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(data.clone())), (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Lost)]);
    let preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &script), D::UseDeviceCollection).unwrap()).unwrap();
    let first_id = preview.operation.id.clone();
    let prepared_generation = preview.operation.actual_generation.expect("real preparation captures a generation");
    assert_eq!(preview.operation.phase, Phase::Preview);
    assert_eq!(preview.operation.scope, Scope::CollectionAndMedia);
    let confirmation = bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &script), preview).unwrap());
    match &confirmation {
        Err(TransferError::Server(error)) => assert_eq!(error.kind.id(), "network"),
        other => panic!("expected the final Meta network failure, got {other:?}"),
    }
    script.assert_done();
    assert_eq!(script.count(Method::Meta), 3);
    assert_eq!(script.count(Method::Download), 1);
    let first = host.manual_transfer_history().get(&first_id).unwrap();
    assert_eq!(first.phase, Phase::Failed);
    assert_eq!(first.boundary, Some(Boundary::Server));
    assert_eq!(first.acceptance, Acceptance::NotSent);
    assert_eq!(first.actual_generation, Some(prepared_generation));
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert!(first.activation_identity.is_none());
    assert!(first.resulting_publication.is_none());
    assert!(first.receipt.is_none());
    assert!(!first.collection_complete);
    assert_eq!(first.media, MediaOutcome::default());
    let first_intent = host.provider_authority().meguri_bootstrap_intent().unwrap();
    assert_eq!(first_intent.operation(), &first_id);
    assert!(first_intent.media_epoch().is_none());
    let first_artifacts = retained_artifacts(&config, &first);
    let provider_before = bytes(&config);
    let collection_path = host.collection().unwrap().path().to_owned();
    let collection_before = fs::read(&collection_path).unwrap();
    let metadata = fs::metadata(&collection_path).unwrap();
    let file_before = (metadata.dev(), metadata.ino(), metadata.len());
    let state = DeviceState::for_collection(&collection_path);
    let publication_before = state.settled_publication().unwrap();
    let replacement_before = state.replacement_evidence().unwrap();
    unbound(&host);
    for method in [Method::Upload, Method::MediaBegin, Method::UploadChanges, Method::MediaChanges, Method::DownloadFiles, Method::MediaSanity] {
        assert_eq!(script.count(method), 0);
    }

    let host = if cold { host.close().unwrap(); CoreHost::open(config.clone()).unwrap() } else { host };
    settle(&host);
    assert_eq!(host.manual_transfer_history().get(&first_id).unwrap(), first);
    assert_eq!(host.provider_authority().meguri_bootstrap_intent(), Some(first_intent.clone()));
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().is_none());
    // Ordinary admission refresh observes the actual cleared active row.
    host.refresh_provider_admission();
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert!(!host.provider_authority().network_blocked());
    assert_eq!(bytes(&config), provider_before);
    assert_eq!(fs::read(&collection_path).unwrap(), collection_before);
    let metadata = fs::metadata(&collection_path).unwrap();
    assert_eq!((metadata.dev(), metadata.ino(), metadata.len()), file_before);
    assert_eq!(state.settled_publication().unwrap(), publication_before);
    assert_eq!(state.replacement_evidence().unwrap(), replacement_before);
    assert_artifacts(&first_artifacts);

    let next_script = offline_preparation_script(data.clone());
    let next = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &next_script), D::UseDeviceCollection).unwrap()).unwrap();
    next_script.assert_done();
    assert_ne!(next.operation.id, first_id);
    assert_eq!(next.operation.phase, Phase::Preview);
    assert_eq!(next.operation.scope, Scope::CollectionAndMedia);
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&next.operation));
    assert_eq!(host.provider_authority().meguri_bootstrap_intent(), Some(first_intent.clone()));
    assert_eq!(bytes(&config), provider_before);
    let next_artifacts = retained_artifacts(&config, &next.operation);
    // Intended RED boundary: a fresh preview must be discoverable despite the
    // historical, nonactive Failed intent, without writing new consent.
    assert_eq!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().as_ref(), Some(&next.operation));
    assert!(matches!(host.pending_meguri_bootstrap_preparation(&connection, "other-user"), Err(E::Ready(F::AccountChanged))));
    for field in ["reference", "revision", "endpoint"] {
        let mut wrong = connection.clone();
        match field { "reference" => wrong.reference.push('x'), "revision" => wrong.revision += 1,
            _ => wrong.endpoint = "https://foreign.synthetic.example/".into() }
        assert!(matches!(host.pending_meguri_bootstrap_preparation(&wrong, USER), Err(E::ConnectionChanged)));
        assert!(matches!(host.discard_meguri_bootstrap_preparation(&wrong, &next.operation), Err(E::ConnectionChanged)));
    }
    assert!(matches!(host.discard_meguri_bootstrap_preparation(&connection, &first), Err(E::Ready(F::OperationMismatch))));
    assert_eq!(host.manual_transfer_history().get(&first_id).unwrap(), first);
    assert_eq!(host.manual_transfer_history().get(&next.operation.id).unwrap(), next.operation);
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&next.operation));
    assert_eq!(host.provider_authority().meguri_bootstrap_intent(), Some(first_intent.clone()));
    assert_eq!(bytes(&config), provider_before);

    let cancelled_next = host.discard_meguri_bootstrap_preparation(&connection, &next.operation).unwrap();
    assert_eq!(cancelled_next.phase, Phase::Cancelled);
    assert_eq!(cancelled_next.boundary, Some(Boundary::Cancelled));
    assert_eq!(cancelled_next.acceptance, Acceptance::NotSent);
    assert!(cancelled_next.activation_identity.is_none());
    assert!(cancelled_next.resulting_publication.is_none());
    assert!(cancelled_next.receipt.is_none());
    assert!(!cancelled_next.collection_complete);
    assert_eq!(cancelled_next.media, MediaOutcome::default());
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().is_none());
    assert_eq!(host.manual_transfer_history().get(&first_id).unwrap(), first);
    assert_eq!(host.manual_transfer_history().get(&next.operation.id).unwrap(), cancelled_next);
    assert_eq!(host.provider_authority().meguri_bootstrap_intent(), Some(first_intent.clone()));
    assert_eq!(bytes(&config), provider_before);
    assert_eq!(fs::read(&collection_path).unwrap(), collection_before);
    let metadata = fs::metadata(&collection_path).unwrap();
    assert_eq!((metadata.dev(), metadata.ino(), metadata.len()), file_before);
    assert_eq!(state.settled_publication().unwrap(), publication_before);
    assert_eq!(state.replacement_evidence().unwrap(), replacement_before);
    assert_artifacts(&first_artifacts);
    assert_artifacts(&next_artifacts);
    for method in [Method::Upload, Method::MediaBegin, Method::UploadChanges, Method::MediaChanges, Method::DownloadFiles, Method::MediaSanity] {
        assert_eq!(script.count(method), 0);
        assert_eq!(next_script.count(method), 0);
    }
    unbound(&host);

    let final_script = collection_script(data, true);
    let final_preview = bootstrap_complete(&host, || host.prepare_meguri_bootstrap(access(&connection, &final_script), D::UseDeviceCollection).unwrap()).unwrap();
    let final_id = final_preview.operation.id.clone();
    assert_ne!(final_id, first_id);
    assert_ne!(final_id, next.operation.id);
    assert_eq!(final_preview.operation.phase, Phase::Preview);
    assert_eq!(final_preview.operation.scope, Scope::CollectionAndMedia);
    assert_eq!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().as_ref(), Some(&final_preview.operation));
    assert_eq!(host.provider_authority().meguri_bootstrap_intent(), Some(first_intent));
    assert_eq!(bytes(&config), provider_before);
    assert_eq!(final_script.count(Method::Upload), 0);
    let done = bootstrap_complete(&host, || host.confirm_meguri_bootstrap(access(&connection, &final_script), final_preview).unwrap()).unwrap();
    final_script.assert_done();
    assert_eq!(done.id, final_id);
    assert!(done.collection_complete);
    assert_eq!(done.scope, Scope::CollectionAndMedia);
    assert_eq!(done.acceptance, Acceptance::ExactOk);
    assert!(done.receipt.is_none());
    assert_eq!(done.media, MediaOutcome::default());
    assert_eq!(host.manual_transfer_history().get(&final_id).unwrap(), done);
    assert_eq!(host.manual_transfer_history().get(&first_id).unwrap(), first);
    assert_eq!(host.manual_transfer_history().get(&next.operation.id).unwrap(), cancelled_next);
    assert_eq!(host.manual_transfer_history().history().unwrap().len(), 3);
    let final_intent = host.provider_authority().meguri_bootstrap_intent().unwrap();
    assert_eq!(final_intent.operation(), &final_id);
    assert!(final_intent.media_epoch().is_none());
    let doc = host.provider_authority().snapshot().unwrap();
    assert!(doc.meguri_binding.is_none());
    assert!(!doc.meguri_settings.automatic);
    unbound(&host);
    assert_eq!(final_script.count(Method::Meta), 3);
    assert_eq!(final_script.count(Method::Download), 1);
    assert_eq!(final_script.count(Method::Upload), 1);
    for method in [Method::MediaBegin, Method::UploadChanges, Method::MediaChanges, Method::DownloadFiles, Method::MediaSanity] {
        assert_eq!(final_script.count(method), 0);
    }
    host.close().unwrap();
}
#[test]
fn meguri_warm_nonactive_failed_confirmation_intent_allows_fresh_preview_discard_and_confirm() {
    let _lane = LANE.lock().unwrap();
    nonactive_failed_confirmation_allows_fresh_preview_discard_and_confirm(false);
}
#[test]
fn meguri_cold_nonactive_failed_confirmation_intent_allows_fresh_preview_discard_and_confirm() {
    let _lane = LANE.lock().unwrap();
    nonactive_failed_confirmation_allows_fresh_preview_discard_and_confirm(true);
}
