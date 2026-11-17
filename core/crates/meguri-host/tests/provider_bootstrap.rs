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

