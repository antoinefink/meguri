//! Test-only: reuses sync_tests::Fixture's actual Host; transport replies for
//! publication prefixes come from the existing Service/coordinator helper.
use super::*;
use crate::cloud_connection::*;
use meguri_sync::cloud::{control_codec, publication as p};
use meguri_testkit::cloud::{
    Caller, Config, Mutation, MutationSuccess, Operation, RecordInput, Reply, Scope, Service, SystemFields,
    coordinator::{execute_publication_request, publication_control_record},
};

fn keys() -> Arc<Keys> {
    Keys::new([Some(vec![31; 32]), Some(vec![47; 32])])
}
#[test]
fn local_facts_require_no_account_and_preserve_host_guards() {
    let identity_cache = Arc::new(CachedIdentity(Mutex::new(None)));
    let key_provider = keys();
    let setup = Fixture::with_identity(key_provider.clone(), Some(identity_cache));
    let api = setup.workflow.connection().unwrap();
    let host_generation = setup.workflow.host.cloud_connection_generation().unwrap().0;
    setup.workflow.set_paused(true).unwrap();
    let facts = api.local_publication_information(host_generation).unwrap();
    assert!(facts.collection_present);
    assert!(matches!(
        facts.condition,
        Some(CloudLocalPublicationCondition::LocalOnly)
    ));
    assert!(facts.pending_reason.is_none());
    assert_eq!(facts.local_counts.len(), 9);
    assert!(facts.last_review_ms.is_none());
    assert!(facts.last_change_ms.is_some());
    assert!(api.recovery_status().unwrap().is_none());
    assert_eq!(key_provider.calls.load(Ordering::SeqCst), 0);
    failure_is(
        api.begin_connection_preview(),
        CloudConnectionFailure::AccountUnavailable,
    );
    failure_is(
        api.local_publication_information(host_generation + 1),
        CloudConnectionFailure::AuthorityChanged,
    );
    setup.workflow.host.close().unwrap();
    failure_is(
        api.local_publication_information(host_generation),
        CloudConnectionFailure::Closed,
    );
}
#[test]
fn absent_local_information_does_not_bootstrap_collection_or_sidecar() {
    let (fixture, _process, _config, identity, keys) = no_collection_host_with_admission(false);
    let host = &fixture.workflow.host;
    let connection = fixture.workflow.connection().unwrap();
    let directory = host.cloud_connection_directory();
    let generation = host.cloud_connection_generation().unwrap();
    let guard = fixture.workflow.core.adapter_guard();
    let identity_calls = identity.calls.load(Ordering::SeqCst);
    let file_calls = identity.file_calls.load(Ordering::SeqCst);
    let sidecar = std::fs::read(directory.join("meguri-sidecar.json")).ok();
    assert!(connection.recovery_status().unwrap().is_none());
    let info = connection
        .local_publication_information(host.cloud_connection_generation().unwrap().0)
        .unwrap();
    assert!(!info.collection_present && info.local_counts.is_empty());
    assert!(info.condition.is_none() && info.last_change_ms.is_none());
    assert!(!directory.join("collection.anki2").exists());
    for suffix in ["", "-wal", "-shm"] {
        assert!(
            directory
                .join(format!("{}{suffix}", c::TRACKING_FILE))
                .symlink_metadata()
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        );
    }
    assert_eq!(std::fs::read(directory.join("meguri-sidecar.json")).ok(), sidecar);
    assert_eq!(host.cloud_connection_generation().unwrap(), generation);
    assert!(matches!(
        host.engine().state(),
        meguri_engine::LifecycleState::NoCollection
    ));
    assert_eq!(host.engine().slot_holder(), None);
    assert_eq!(fixture.workflow.core.adapter_guard(), guard);
    assert_eq!(identity.calls.load(Ordering::SeqCst), identity_calls);
    assert_eq!(identity.file_calls.load(Ordering::SeqCst), file_calls);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    // A file appearing in the retained no-collection host is stale, never
    // silently read/opened or treated as an absent collection.
    std::fs::write(directory.join("collection.anki2"), b"synthetic-invalid-file").unwrap();
    failure_is(
        connection.local_publication_information(host.cloud_connection_generation().unwrap().0),
        CloudConnectionFailure::AuthorityChanged,
    );
    failure_is(connection.recovery_status(), CloudConnectionFailure::AuthorityChanged);
    host.close().unwrap();
    failure_is(connection.recovery_status(), CloudConnectionFailure::Closed);
}
#[test]
fn recovery_discovery_absent_tracking_checks_both_runtime_reads_without_bootstrap() {
    let fixture = Fixture::without_prior_icloud_consent(keys(), false);
    let core = &fixture.workflow.core;
    let directory = fixture.workflow.host.cloud_connection_directory();
    let entries = recovery_discovery_entries(directory);
    assert!(core.adoption_op().unwrap().is_none());
    assert!(core.publication_op().unwrap().is_none());
    assert_eq!(recovery_discovery_entries(directory), entries);
    for name in ["collection.anki2", c::TRACKING_FILE, "meguri-sidecar.json"] {
        assert!(
            directory
                .join(name)
                .symlink_metadata()
                .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
        );
    }
    fixture.workflow.host.close().unwrap();
    assert!(matches!(core.adoption_op(), Err(r::RuntimeError::StaleIncarnation)));
    assert!(matches!(core.publication_op(), Err(r::RuntimeError::StaleIncarnation)));
}
fn recovery_discovery_entries(directory: &std::path::Path) -> std::collections::BTreeSet<std::ffi::OsString> {
    std::fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect()
}
#[test]
fn recovery_discovery_absent_store_rejects_present_tracking_entries_without_changes() {
    for entry in ["malformed", "newer", "directory", "symlink"] {
        #[cfg(not(unix))]
        if entry == "symlink" {
            continue;
        }
        let (fixture, _process, _config, identity, keys) = no_collection_host_with_admission(false);
        let host = &fixture.workflow.host;
        let core = &fixture.workflow.core;
        let connection = fixture.workflow.connection().unwrap();
        let directory = host.cloud_connection_directory();
        let path = directory.join(c::TRACKING_FILE);
        match entry {
            "malformed" => std::fs::write(&path, b"synthetic damaged tracking").unwrap(),
            "newer" => {
                let db = meguri_storage::rusqlite::Connection::open(&path).unwrap();
                db.pragma_update(None, "user_version", 9999).unwrap();
            }
            "directory" => std::fs::create_dir(&path).unwrap(),
            #[cfg(unix)]
            "symlink" => std::os::unix::fs::symlink("missing-tracking-target", &path).unwrap(),
            _ => unreachable!(),
        }
        let bytes = path.is_file().then(|| std::fs::read(&path).unwrap());
        let metadata = path.symlink_metadata().unwrap();
        let link = std::fs::read_link(&path).ok();
        let entries = recovery_discovery_entries(directory);
        let generation = host.cloud_connection_generation().unwrap();
        let guard = core.adapter_guard();
        let identity_calls = identity.calls.load(Ordering::SeqCst);
        let file_calls = identity.file_calls.load(Ordering::SeqCst);
        failure_is(connection.recovery_status(), CloudConnectionFailure::Tracking);
        assert!(matches!(core.adoption_op(), Err(r::RuntimeError::Tracking(_))));
        assert!(matches!(core.publication_op(), Err(r::RuntimeError::Tracking(_))));
        assert_eq!(path.symlink_metadata().unwrap().file_type(), metadata.file_type());
        assert_eq!(path.is_file().then(|| std::fs::read(&path).unwrap()), bytes);
        assert_eq!(std::fs::read_link(&path).ok(), link);
        if entry == "directory" {
            assert_eq!(std::fs::read_dir(&path).unwrap().count(), 0);
        }
        assert_eq!(recovery_discovery_entries(directory), entries);
        assert!(!directory.join("collection.anki2").exists());
        assert!(!directory.join("meguri-sidecar.json").exists());
        assert_eq!(host.cloud_connection_generation().unwrap(), generation);
        assert!(matches!(
            host.engine().state(),
            meguri_engine::LifecycleState::NoCollection
        ));
        assert_eq!(host.engine().slot_holder(), None);
        assert_eq!(core.adapter_guard(), guard);
        assert_eq!(identity.calls.load(Ordering::SeqCst), identity_calls);
        assert_eq!(identity.file_calls.load(Ordering::SeqCst), file_calls);
        assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
        host.close().unwrap();
    }
}
#[test]
fn local_facts_keep_recorded_reasons_and_retained_preview_usable() {
    let setup = Fixture::new(keys());
    let retained_host = &setup.workflow.host;
    let host_generation = retained_host.cloud_connection_generation().unwrap().0;
    let api = setup.workflow.connection().unwrap();
    let sidecar_path = retained_host.cloud_connection_directory().join("meguri-sidecar.json");
    let saved_sidecar = std::fs::read(&sidecar_path).unwrap();
    let mut sidecar_document: serde_json::Value = serde_json::from_slice(&saved_sidecar).unwrap();
    sidecar_document["deviceState"]["active"]["decision"] = serde_json::json!("Pending");
    for (recorded_reason, expected_reason) in [
        (Some("restore"), Some(CloudLocalPendingDecisionReason::Restore)),
        (
            Some("colpkgImport"),
            Some(CloudLocalPendingDecisionReason::ColpkgImport),
        ),
        (
            Some("manualImport"),
            Some(CloudLocalPendingDecisionReason::ManualImport),
        ),
        (None, None),
    ] {
        if let Some(recorded_reason) = recorded_reason {
            sidecar_document["deviceState"]["pendingDecisionReason"] = serde_json::json!(recorded_reason);
        } else {
            sidecar_document["deviceState"]
                .as_object_mut()
                .unwrap()
                .remove("pendingDecisionReason");
        }
        let encoded_sidecar = serde_json::to_vec(&sidecar_document).unwrap();
        std::fs::write(&sidecar_path, &encoded_sidecar).unwrap();
        let facts = api.local_publication_information(host_generation).unwrap();
        assert!(matches!(facts.condition, Some(CloudLocalPublicationCondition::Pending)));
        assert_eq!(facts.pending_reason, expected_reason);
        assert_eq!(std::fs::read(&sidecar_path).unwrap(), encoded_sidecar);
    }
    std::fs::write(&sidecar_path, saved_sidecar).unwrap();
    let retained_preview = ready(&api);
    let _queried_facts = api.local_publication_information(host_generation).unwrap();
    // Confirmation consumes the opaque handle acquired before reading these facts.
    // Reading facts leaves that existing confirmation available to its caller.
    let publication_operation = api.confirm_publication(retained_preview).unwrap();
    api.cancel_operation(publication_operation).unwrap();
}
#[test]
fn actual_no_collection_host_discovers_without_creating_an_empty_collection_or_publication() {
    let keys = keys();
    let fixture = Fixture::without_collection(keys.clone());
    let host = &fixture.workflow.host;
    assert!(matches!(
        host.engine().state(),
        meguri_engine::LifecycleState::NoCollection
    ));
    assert!(host.collection().is_err());
    let path = host.cloud_connection_directory().join("collection.anki2");
    assert!(!path.exists());
    let connection = fixture.workflow.connection().unwrap();
    let policy = fixture.workflow.policy().unwrap();
    fixture.workflow.media().unwrap();
    let output = policy.clone().refresh_account().unwrap();
    if let crate::cloud_policy::CloudPolicyOutput::Commands { commands } = output {
        for command in commands {
            if matches!(
                command.action,
                crate::cloud_policy::CloudPolicyAction::StatusChanged { .. }
            ) {
                policy.clone().execute_core(command.handle).unwrap();
            }
        }
    }
    policy.status().unwrap();
    for _ in 0..2 {
        let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap()
        else {
            panic!("actual active read required")
        };
        let CloudConnectionPreviewWork::Ready { preview, summary } = connection
            .connection_active_read(preview, CloudControlRead::ZoneMissing)
            .unwrap()
        else {
            panic!("actual absent-root discovery required")
        };
        assert!(!summary.local_collection_present && !summary.local_empty);
        assert!(summary.local_counts.is_empty() && summary.manifest_counts.is_none());
        assert!(matches!(summary.decision, CloudConnectionDecision::NoCollection));
        failure_is(
            connection.confirm_publication(preview.clone()),
            CloudConnectionFailure::NotOffered,
        );
        failure_is(
            connection.confirm_adoption(preview.clone()),
            CloudConnectionFailure::NotOffered,
        );
        connection.cancel_preview(preview).unwrap();
        assert!(!path.exists());
        assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    }
    host.close().unwrap();
    failure_is(connection.begin_connection_preview(), CloudConnectionFailure::Closed);
}
// Holds the actual original provider, without account-cache or transport fallback.
struct HeldNoCollectionAccount {
    held: Mutex<Option<(std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>)>>,
    calls: AtomicUsize,
    file_calls: Arc<AtomicUsize>,
}
struct CountedNoCollectionFiles(Arc<AtomicUsize>);
impl r::FileIdentityProvider for CountedNoCollectionFiles {
    fn identity(&self, path: &std::path::Path) -> Result<r::NativeFileIdentity, r::RuntimeError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        r::FileIdentityProvider::identity(&NativeFiles, path)
    }
}
struct ReleaseNoCollectionRead(std::sync::mpsc::Sender<()>);
impl Drop for ReleaseNoCollectionRead {
    fn drop(&mut self) {
        let _ = self.0.send(());
    }
}
impl HeldNoCollectionAccount {
    fn arm(&self) -> (std::sync::mpsc::Receiver<()>, std::sync::mpsc::Sender<()>) {
        let (reached, wait) = std::sync::mpsc::channel();
        let (release, resume) = std::sync::mpsc::channel();
        *self.held.lock().unwrap() = Some((reached, resume));
        (wait, release)
    }
}
impl r::IdentityProvider for HeldNoCollectionAccount {
    fn read_identity(&self, _: &c::BuildScope) -> r::IdentityStatus {
        self.calls.fetch_add(1, Ordering::SeqCst);
        let held = self.held.lock().unwrap().take();
        if let Some((reached, resume)) = held {
            reached.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(30)).unwrap();
        }
        r::IdentityStatus::Available(c::OpaqueBytes(b"synthetic-no-collection-owner".to_vec()))
    }
}
fn held_no_collection_host() -> (
    Fixture,
    meguri_host::ProcessCore,
    HostConfig,
    Arc<HeldNoCollectionAccount>,
    Arc<Keys>,
) {
    no_collection_host_with_admission(true)
}
// Read-only discovery callers retain absent consent; held account-read and
// adoption protocols explicitly represent an already selected iCloud service.
fn no_collection_host_with_admission(prior_icloud_consent: bool) -> (
    Fixture,
    meguri_host::ProcessCore,
    HostConfig,
    Arc<HeldNoCollectionAccount>,
    Arc<Keys>,
) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("collection")).unwrap();
    std::fs::create_dir_all(directory.path().join("backups")).unwrap();
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let random = Arc::new(SeededRandom::new(114));
    let identity = Arc::new(HeldNoCollectionAccount {
        held: Mutex::new(None),
        calls: AtomicUsize::new(0),
        file_calls: Arc::new(AtomicUsize::new(0)),
    });
    let keys = keys();
    let mut config = HostConfig::new(
        directory.path().join("collection"),
        directory.path().join("backups"),
        directory.path().join("widget"),
        clock.clone(),
        random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity: identity.clone(),
        files: Arc::new(CountedNoCollectionFiles(identity.file_calls.clone())),
    });
    config.cloud_key_bytes = Some(Arc::new(ForeignKeyBytes(keys.clone())));
    if prior_icloud_consent {
        prepare_prior_icloud_fixture_configuration(&config);
    }
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = Arc::new(process.prepare_cloud_connection(config.clone()).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    (
        Fixture {
            _directory: directory,
            workflow: CloudWorkflow {
                host,
                core,
                identity: None,
            },
            clock,
            random,
        },
        process,
        config,
        identity,
        keys,
    )
}

#[test]
fn actual_held_no_collection_account_read_cannot_reach_same_generation_successor_host() {
    let (fixture, process, config, identity, keys) = held_no_collection_host();
    let connection = fixture.workflow.connection().unwrap();
    let policy = fixture.workflow.policy().unwrap();
    let original_generation = fixture.workflow.host.engine().generation();
    let (reached, release) = identity.arm();
    std::thread::scope(|scope| {
        let read = scope.spawn(move || policy.refresh_account());
        let release = ReleaseNoCollectionRead(release);
        reached.recv_timeout(Duration::from_secs(30)).unwrap();
        fixture.workflow.host.close().unwrap(); // Does not wait for the provider.
        let successor = Arc::new(process.prepare_cloud_connection(config).unwrap());
        assert_eq!(successor.engine().generation(), original_generation);
        let workflow = CloudWorkflow {
            core: successor.cloud_runtime().unwrap().clone(),
            host: successor.clone(),
            identity: None,
        };
        assert!(matches!(
            fixture.workflow.core.adoption_op(),
            Err(r::RuntimeError::StaleIncarnation)
        ));
        assert!(matches!(
            fixture.workflow.core.publication_op(),
            Err(r::RuntimeError::StaleIncarnation)
        ));
        failure_is(connection.recovery_status(), CloudConnectionFailure::Closed);
        let successor_policy = workflow.policy().unwrap();
        successor_policy.clone().refresh_account().unwrap();
        let guard = workflow.core.adapter_guard();
        let status = successor_policy.status().unwrap();
        release.0.send(()).unwrap();
        assert!(read.join().unwrap().is_err());
        assert_eq!(workflow.core.adapter_guard(), guard);
        assert_eq!(successor_policy.status().unwrap().account_status, status.account_status);
        assert!(!successor.cloud_connection_directory().join("collection.anki2").exists());
        assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
        successor.close().unwrap();
    });
}

#[test]
fn actual_held_no_collection_account_read_rejects_appeared_local_file_before_result_and_apply() {
    let (fixture, _process, config, identity, keys) = held_no_collection_host();
    let core = fixture.workflow.core.clone();
    let guard = core.adapter_guard();
    let (reached, release) = identity.arm();
    let read_core = core.clone();
    std::thread::scope(|scope| {
        let read = scope.spawn(move || read_core.policy_account_identity());
        let release = ReleaseNoCollectionRead(release);
        reached.recv_timeout(Duration::from_secs(30)).unwrap();
        let path = config.collection_directory.join("collection.anki2");
        meguri_engine::bootstrap::create(&path, fixture.clock.as_ref(), fixture.random.as_ref()).unwrap();
        release.0.send(()).unwrap();
        assert!(matches!(read.join().unwrap(), Err(r::RuntimeError::StaleIncarnation)));
        // No new read can start for the still-unbound Host after a file appears.
        assert!(matches!(
            core.adapter_begin_identity_read(false, None),
            Err(r::RuntimeError::StaleIncarnation)
        ));
        assert_eq!(core.adapter_guard(), guard);
        assert!(
            fixture
                .workflow
                .connection()
                .unwrap()
                .begin_connection_preview()
                .is_err()
        );
        assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
        fixture.workflow.host.close().unwrap();
    });
}

#[test]
fn actual_no_collection_identity_completion_rechecks_local_file_without_mutating_policy() {
    let (fixture, _process, config, _identity, keys) = held_no_collection_host();
    let policy = fixture.workflow.policy().unwrap();
    let core = &fixture.workflow.core;
    let (request, identity) = core.policy_account_identity().unwrap();
    let guard = core.adapter_guard();
    let before = policy.status().unwrap().account_status;
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        fixture.clock.as_ref(),
        fixture.random.as_ref(),
    )
    .unwrap();
    assert!(!core.adapter_complete_identity_read(request, identity).unwrap());
    assert_eq!(core.adapter_guard(), guard);
    assert_eq!(policy.status().unwrap().account_status, before);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    fixture.workflow.host.close().unwrap();
}

#[test]
fn actual_no_collection_completed_read_cannot_apply_after_transition_to_real_collection() {
    let (fixture, process, config, _identity, keys) = held_no_collection_host();
    let core = fixture.workflow.core.clone();
    let (request, identity) = core.policy_account_identity().unwrap();
    let generation = fixture.workflow.host.engine().generation();
    fixture.workflow.host.close().unwrap();
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        fixture.clock.as_ref(),
        fixture.random.as_ref(),
    )
    .unwrap();
    let successor = Arc::new(process.open(config).unwrap());
    assert!(successor.engine().generation().0 > generation.0);
    assert!(successor.collection().unwrap().is_current());
    let workflow = CloudWorkflow {
        core: successor.cloud_runtime().unwrap().clone(),
        host: successor.clone(),
        identity: None,
    };
    let policy = workflow.policy().unwrap();
    policy.clone().refresh_account().unwrap();
    let guard = workflow.core.adapter_guard();
    assert!(!core.adapter_complete_identity_read(request, identity).unwrap());
    assert_eq!(workflow.core.adapter_guard(), guard);
    assert!(matches!(
        policy.status().unwrap().account_status,
        crate::cloud_policy::CloudPolicyAccountStatus::Available
    ));
    let connection = workflow.connection().unwrap();
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("active read")
    };
    let CloudConnectionPreviewWork::Ready { summary, .. } = connection
        .connection_active_read(preview, CloudControlRead::ZoneMissing)
        .unwrap()
    else {
        panic!("ready")
    };
    assert!(summary.local_collection_present);
    assert!(matches!(summary.decision, CloudConnectionDecision::ConfirmUpload));
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    successor.close().unwrap();
}

fn ready(connection: &CloudConnection) -> Arc<CloudConnectionPreview> {
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("actual record read required")
    };
    let CloudConnectionPreviewWork::Ready { preview, summary } = connection
        .connection_active_read(preview, CloudControlRead::ZoneMissing)
        .unwrap()
    else {
        panic!("actual confirmation required")
    };
    assert!(summary.manifest_counts.is_none());
    assert!(matches!(summary.decision, CloudConnectionDecision::ConfirmUpload));
    preview
}
fn own(connection: &CloudConnection) -> (Arc<CloudPublicationRequest>, CloudOwnOperation) {
    for _ in 0..8 {
        match connection.next_publication_work().unwrap() {
            CloudPublicationWork::Local { handle, .. } => connection.perform_local(handle).unwrap(),
            CloudPublicationWork::Own { request, command, .. } => return (request, command),
            _ => panic!("real publication must reach an own request"),
        }
    }
    panic!("bounded local transitions");
}
struct Queue(AtomicUsize);
impl CloudPublicationQueue for Queue {
    fn enqueue(&self) -> bool {
        // The exported entry guard must apply to this foreign callback too.
        #[cfg(debug_assertions)]
        assert!(std::panic::catch_unwind(crate::core_version).is_err());
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
fn failure_is<T>(result: Result<T, CoreError>, expected: CloudConnectionFailure) {
    match result {
        Err(CoreError::CloudConnection { failure }) => assert_eq!(failure, expected),
        Err(_) => panic!("expected CloudConnection failure, received another error family"),
        Ok(_) => panic!("expected CloudConnection failure, received success"),
    }
}
fn control(v: p::ControlRecord) -> CloudControlRecord {
    CloudControlRecord {
        encrypted: v.encrypted,
        system_fields: v.system_fields.0,
    }
}
fn result(v: p::OwnResult) -> CloudPublicationResult {
    match v {
        p::OwnResult::ZoneLosses(losses) => CloudPublicationResult::ZoneLosses {
            losses: losses
                .into_iter()
                .map(|l| crate::sync::CloudZoneLoss {
                    zone: l.zone,
                    cause: match l.cause {
                        meguri_sync::cloud::adapter::ZoneLossCause::Deleted => crate::sync::CloudZoneLossCause::Deleted,
                        meguri_sync::cloud::adapter::ZoneLossCause::Purged => crate::sync::CloudZoneLossCause::Purged,
                        meguri_sync::cloud::adapter::ZoneLossCause::EncryptedDataReset => {
                            crate::sync::CloudZoneLossCause::EncryptedDataReset
                        }
                    },
                })
                .collect(),
        },
        p::OwnResult::ZonesCreated => CloudPublicationResult::ZonesCreated,
        p::OwnResult::Saved(r) => CloudPublicationResult::Saved { record: control(r) },
        p::OwnResult::Conflict(r) => CloudPublicationResult::Conflict { record: control(r) },
        p::OwnResult::Read(r) => CloudPublicationResult::Read {
            read: match r {
                p::RecordRead::Found(r) => CloudControlRead::Found { record: control(r) },
                p::RecordRead::Missing => CloudControlRead::Missing,
                p::RecordRead::ZoneMissing => CloudControlRead::ZoneMissing,
                p::RecordRead::Failed => CloudControlRead::Failed,
            },
        },
        p::OwnResult::Failed { retry_after_ms } => CloudPublicationResult::Failed { retry_after_ms },
        p::OwnResult::RetryAfter { duration_ms } => CloudPublicationResult::RetryAfter { duration_ms },
        p::OwnResult::NotStarted => CloudPublicationResult::NotStarted,
    }
}

#[test]
fn bridge_preview_confirm_consumes_actual_preview_and_exposes_no_manifest_as_local_counts() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let preview = ready(&connection);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    let operation = connection.confirm_publication(preview.clone()).unwrap();
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    failure_is(
        connection.confirm_publication(preview),
        CloudConnectionFailure::Consumed,
    );
    let status = connection.recovery_status().unwrap().unwrap();
    assert!(matches!(status.step, CloudRecoveryStep::Confirmed));
    assert!(!status.adoption_recovered_cancel_only);
    assert!(!status.adoption_after_review);
    connection.cancel_operation(operation).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
}
#[test]
fn bridge_stale_confirmation_wrong_owner_and_old_preview_cancel_never_replace_new_authority() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let other = fixture.workflow.connection().unwrap();
    let old = ready(&connection);
    let current = ready(&connection);
    failure_is(
        other.confirm_publication(current.clone()),
        CloudConnectionFailure::WrongOwner,
    );
    failure_is(connection.cancel_preview(old), CloudConnectionFailure::AuthorityChanged);
    let stale = ready(&connection);
    let latest = ready(&connection);
    failure_is(
        connection.confirm_publication(stale),
        CloudConnectionFailure::AuthorityChanged,
    );
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    connection.confirm_publication(latest).unwrap();
}
#[test]
fn bridge_guarded_enqueue_is_zero_when_paused_and_one_after_actual_acceptance() {
    let fixture = Fixture::new(keys());
    let connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let (request, command) = own(&connection);
    assert!(matches!(command, CloudOwnOperation::CreateZones { ref zones } if zones.len() == 3));
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    fixture.workflow.set_paused(true).unwrap();
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .is_err()
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    connection
        .publication_result(request, CloudPublicationResult::NotStarted)
        .unwrap();
    fixture.workflow.set_paused(false).unwrap();
    connection.trigger().unwrap();
    let (request, _) = own(&connection);
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .unwrap()
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }),
        CloudConnectionFailure::Closed,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
}
#[test]
fn bridge_close_rejects_held_confirmation_before_entropy_or_successor_host_access() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let preview = ready(&connection);
    fixture.workflow.host.close().unwrap();
    failure_is(connection.confirm_publication(preview), CloudConnectionFailure::Closed);
    failure_is(connection.next_publication_work(), CloudConnectionFailure::Closed);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn bridge_accepted_active_with_lost_reply_retains_actual_durable_switch_attempt() {
    let fixture = Fixture::new(keys());
    let (connection, request) = active_request(&fixture);
    let before = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(!before.switch_sent && before.active_attempt.is_none());
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .unwrap()
    );
    let submitted = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(submitted.switch_sent && submitted.active_attempt.is_some());
    let recovery = connection.recovery_status().unwrap().unwrap();
    assert!(!recovery.publication_conflict_cancellable);
    failure_is(connection.cancel_operation(recovery.operation), CloudConnectionFailure::Busy);
    assert!(fixture.workflow.core.publication_op().unwrap().as_ref() == Some(&submitted));
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }),
        CloudConnectionFailure::Closed,
    );
    let c::OpenOutcome::Opened(store) = c::TrackingStore::open(
        &fixture._directory.path().join("collection"),
        c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
    )
    .unwrap() else {
        panic!("actual durable store")
    };
    assert!(store.publication_op().unwrap() == Some(submitted));
}
#[test]
fn bridge_found_active_without_e_is_unreadable_and_missing_is_an_upload_offer() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("active read")
    };
    let CloudConnectionPreviewWork::Ready { summary, .. } = connection
        .connection_active_read(
            preview,
            CloudControlRead::Found {
                record: CloudControlRecord {
                    encrypted: None,
                    system_fields: vec![9],
                },
            },
        )
        .unwrap()
    else {
        panic!("unreadable preview")
    };
    assert!(matches!(summary.decision, CloudConnectionDecision::CloudUnreadable));
    let preview = ready(&connection);
    connection.cancel_preview(preview).unwrap();
}
fn adoption_preview(connection: &CloudConnection) -> (Arc<CloudConnectionPreview>, CloudControlRecord) {
    let active = CloudControlRecord {
        encrypted: Some(
            control_codec::Active {
                epoch: [8; 16],
                activated_at_ms: NOW,
                previous: None,
                replaced_through_ms: None,
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![1],
    };
    let epoch = CloudControlRecord {
        encrypted: Some(
            control_codec::Epoch {
                epoch_id: [8; 16],
                record_key: [19; 32],
                crt: NOW / 1000,
                ver: 18,
                state: control_codec::EpochState::Complete,
                reason: control_codec::EpochReason::Initial,
                published_at_ms: NOW,
                counts: vec![],
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![2],
    };
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("active")
    };
    let CloudConnectionPreviewWork::ReadEpoch { preview, .. } = connection
        .connection_active_read(preview, CloudControlRead::Found { record: active.clone() })
        .unwrap()
    else {
        panic!("epoch")
    };
    let CloudConnectionPreviewWork::Ready { preview, summary } = connection
        .connection_epoch_read(preview, CloudControlRead::Found { record: epoch })
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(summary.published_at_ms, Some(NOW));
    assert!(summary.manifest_counts.is_some());
    (preview, active)
}
#[test]
fn bridge_adoption_confirmation_takes_actual_host_slot_and_exact_cancel_releases_it() {
    assert_adoption_recovery_operation(Fixture::new(Keys::new([])));
}
#[test]
fn recovery_discovery_absent_collection_retains_actual_adoption_operation() {
    assert_adoption_recovery_operation(Fixture::without_collection(Keys::new([])));
}
fn assert_adoption_recovery_operation(fixture: Fixture) {
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    let operation = connection.confirm_adoption(preview).unwrap();
    let before = fixture.workflow.core.adoption_op().unwrap().unwrap();
    let status = connection.recovery_status().unwrap().unwrap();
    assert!(matches!(status.step, CloudRecoveryStep::Staging));
    assert!(status.adoption_cancellable && !status.publication_cancellable);
    assert!(!status.adoption_recovered_cancel_only);
    assert!(!status.adoption_transport_wait);
    assert!(!status.switch_sent && !status.switch_conflict && status.retry_after_ms.is_none());
    assert_eq!(fixture.workflow.core.adoption_op().unwrap().as_ref(), Some(&before));
    let other = fixture.workflow.connection().unwrap();
    failure_is(
        other.cancel_operation(status.operation.clone()),
        CloudConnectionFailure::WrongOwner,
    );
    assert_eq!(
        fixture.workflow.host.engine().slot_holder(),
        Some(meguri_engine::LongOp::CloudAdopt)
    );
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption work")
    };
    assert!(matches!(action, CloudAdoptionAction::ReadActive { expected, .. }
        if expected.encrypted == active.encrypted && expected.system_fields == active.system_fields));
    connection
        .adoption_result(
            handle.clone(),
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Missing
                }
            )
            .is_err()
    );
    connection.cancel_operation(status.operation).unwrap();
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    failure_is(
        connection.cancel_operation(operation),
        CloudConnectionFailure::NotOffered,
    );
    assert!(connection.recovery_status().unwrap().is_none());
}
#[test]
fn bridge_adoption_close_rejects_held_reply_and_actual_host_releases_stage() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    connection.confirm_adoption(preview).unwrap();
    let CloudPublicationWork::Adoption { handle, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption")
    };
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        ),
        CloudConnectionFailure::Closed,
    );
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
}

#[test]
fn bridge_adoption_opaque_page_cursor_round_trips_and_deletions_abort_actual_host_staging() {
    let fixture = Fixture::new(Keys::new([]));
    let collection = fixture.workflow.host.collection().unwrap();
    let before = collection.generation();
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("actual stage page")
    };
    let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
    assert_eq!(handle.operation_id_for_test(), op.id);
    let expected = op.epoch_record.as_ref().unwrap();
    assert!(
        matches!(action, CloudAdoptionAction::Fetch { cursor: None, ref expected_epoch, ref zone }
        if expected_epoch.encrypted == expected.encrypted
            && expected_epoch.system_fields == expected.system_fields.0
            && zone == &c::adapter::collection_zone(op.epoch))
    );
    let cursor = vec![5, 4, 3];
    connection
        .adoption_result(
            handle,
            CloudAdoptionResult::Page {
                records: vec![],
                deleted: vec![],
                next: Some(cursor.clone()),
            },
        )
        .unwrap();
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("next actual page")
    };
    assert_eq!(handle.operation_id_for_test(), op.id);
    assert!(
        matches!(action, CloudAdoptionAction::Fetch { cursor: Some(ref value), ref expected_epoch, ref zone }
        if value == &cursor && expected_epoch.encrypted == expected.encrypted
            && expected_epoch.system_fields == expected.system_fields.0
            && zone == &c::adapter::collection_zone(op.epoch))
    );
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Page {
                    records: vec![],
                    deleted: vec!["deleted".into()],
                    next: None
                }
            )
            .is_err()
    );
    assert_eq!(fixture.workflow.host.collection().unwrap().generation(), before);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
}
#[test]
fn accepted_unbound_identity_read_is_retained_and_stale_completion_cannot_replace_it() {
    let fixture = Fixture::new(Keys::new([]));
    let cache = Arc::new(CachedIdentity(Mutex::new(None)));
    let workflow = CloudWorkflow {
        host: fixture.workflow.host.clone(),
        core: fixture.workflow.core.clone(),
        identity: Some(cache.clone()),
    };
    let old = workflow.begin_identity_read(false, None).unwrap();
    let current = workflow.begin_identity_read(false, None).unwrap();
    assert!(
        !workflow
            .complete_identity_read(old, CloudAccountStatus::Available, Some(b"stale-observation".to_vec()))
            .unwrap()
    );
    let scope = c::BuildScope {
        container: c::ContainerId::new("synthetic-u114").unwrap(),
        environment: c::Environment::Development,
    };
    assert!(matches!(
        r::IdentityProvider::read_identity(&*cache, &scope),
        r::IdentityStatus::ReadFailed
    ));
    assert!(
        workflow
            .complete_identity_read(
                current,
                CloudAccountStatus::Available,
                Some(b"synthetic-account-A".to_vec())
            )
            .unwrap()
    );
    for _ in 0..3 {
        assert!(matches!(
            r::IdentityProvider::read_identity(&*cache, &scope),
            r::IdentityStatus::Available(_)
        ));
    }
}

#[test]
fn retained_cancel_id_cannot_cancel_the_next_actual_publication() {
    let fixture = Fixture::new(Keys::new([
        Some(vec![11; 32]),
        Some(vec![13; 32]),
        Some(vec![17; 32]),
        Some(vec![19; 32]),
    ]));
    let connection = fixture.workflow.connection().unwrap();
    let old = connection.confirm_publication(ready(&connection)).unwrap();
    connection.cancel_operation(old.clone()).unwrap();
    let current = connection.confirm_publication(ready(&connection)).unwrap();
    failure_is(connection.cancel_operation(old), CloudConnectionFailure::NotOffered);
    assert!(connection.recovery_status().unwrap().is_some());
    connection.cancel_operation(current).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
}

fn active_request(fixture: &Fixture) -> (Arc<CloudConnection>, Arc<CloudPublicationRequest>) {
    let (connection, request, _) = active_request_with_epoch(fixture);
    (connection, request)
}

#[test]
fn normal_host_open_at_directory_alias_reaches_selected_control_checks() {
    use c::selected_observation::{Missing, Proof};
    let fixture = Fixture::with_recipe_at_alias(keys(), None, true, None, true);
    let (_connection, _request) = active_request(&fixture);
    let collection = fixture.workflow.host.collection().unwrap();
    assert_eq!(
        collection.path(),
        fixture
            ._directory
            .path()
            .join("collection")
            .canonicalize()
            .unwrap()
            .join("collection.anki2")
    );
    let runtime = fixture.workflow.host.cloud_runtime().unwrap();
    assert!(Arc::ptr_eq(runtime, &fixture.workflow.core));
    let observation = runtime.selected_control_for_testing(&collection).unwrap();
    // Actual publication has reached Active, but its reply is still held.
    assert_eq!(observation.status, Proof::Missing(Missing::OperationInProgress));
    assert!(observation.publication_done_active.is_none());
    let other = Fixture::new(keys());
    let foreign = runtime
        .selected_control_for_testing(&other.workflow.host.collection().unwrap())
        .unwrap();
    assert_eq!(foreign.status, Proof::Missing(Missing::CurrentCollectionChanged));
    fixture.workflow.host.close().unwrap();
    assert!(runtime.selected_control_for_testing(&collection).is_err());
}

#[test]
fn selected_reply_projection_retains_original_request_without_granting_commit_or_fresh_owner_proof() {
    use c::selected_observation::{Missing, Proof, PublicationEvent};
    let fixture = Fixture::new(keys());
    let (connection, request) = active_request(&fixture);
    let p::OwnOperation::Save { encrypted, .. } = &request.retained_for_test().operation else {
        panic!("actual retained Active request")
    };
    let reply = CloudPublicationResult::Saved {
        record: CloudControlRecord {
            encrypted: Some(encrypted.clone()),
            system_fields: vec![31, 41],
        },
    };
    let expected = PublicationEvent::from_reply(request.retained_for_test(), &reply.clone().into()).unwrap();
    assert_eq!(request.selected_event(reply.clone()), Some(expected.clone()));
    assert!(request.selected_event(CloudPublicationResult::NotStarted).is_none());
    let collection = fixture.workflow.host.collection().unwrap();
    let control = fixture.workflow.core.selected_control_for_testing(&collection).unwrap();
    assert_eq!(control.status, Proof::Missing(Missing::OperationInProgress));
    assert!(control.publication_done_active.is_none());
    fixture.workflow.host.close().unwrap();
    // Diagnostic projection can survive the old owner, but acceptance cannot.
    assert_eq!(request.selected_event(reply.clone()), Some(expected));
    assert!(connection.publication_result(request, reply).is_err());
}
fn active_request_with_epoch(
    fixture: &Fixture,
) -> (Arc<CloudConnection>, Arc<CloudPublicationRequest>, CloudControlRecord) {
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .register_full(|_, _| true)
        .unwrap();
    let connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let run = tempfile::tempdir().unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(801)).unwrap();
    let mut sdk: Option<CloudEngineRegistration> = None;
    let mut held = None;
    for _ in 0..64 {
        match connection.next_publication_work().unwrap() {
            CloudPublicationWork::Local { handle, .. } | CloudPublicationWork::Scan { handle } => {
                connection.perform_local(handle).unwrap()
            }
            CloudPublicationWork::Own { request, .. } => {
                if request.retained_for_test().kind == p::OwnKind::SaveActive {
                    held = Some(request);
                    break;
                }
                let (reply, _) = execute_publication_request(
                    &mut service,
                    &scope,
                    request.retained_for_test(),
                    &fixture.workflow.core,
                    &fixture.workflow.host.collection().unwrap(),
                )
                .unwrap();
                connection.publication_result(request, result(reply)).unwrap();
            }
            CloudPublicationWork::SendEntities => {
                if sdk.is_none() {
                    sdk = Some(fixture.workflow.register_sdk(false).unwrap());
                }
                send_publication_batch(fixture, sdk.as_ref().unwrap(), &mut service, &scope);
            }
            _ => panic!("publication failed before active"),
        }
    }
    let request = held.expect("real complete epoch must yield an active save");
    let op = fixture.workflow.core.publication_op().unwrap().unwrap();
    let zone = meguri_sync::cloud::adapter::collection_zone(op.epoch);
    let inventory = service.inspect(&scope).unwrap().unwrap();
    let epoch = publication_control_record(&inventory.zones[&zone].records["epoch"]).unwrap();
    (
        connection,
        request,
        CloudControlRecord {
            encrypted: epoch.encrypted,
            system_fields: epoch.system_fields.0,
        },
    )
}

fn send_publication_batch(
    fixture: &Fixture,
    sdk: &CloudEngineRegistration,
    service: &mut Service<SeededRandom>,
    scope: &Scope,
) {
    let pending = fixture.workflow.tracking_sendables().unwrap();
    let batch = fixture
        .workflow
        .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
        .unwrap()
        .expect("actual entity batch");
    let count = batch.saves.len() as u32;
    assert!(count > 0);
    let changes = batch
        .saves
        .into_iter()
        .map(|r| Mutation::Save {
            record: RecordInput::encrypted(&r.name, "MGEntity", r.envelope),
            if_unchanged: r.system_fields.map(SystemFields::from_bytes),
        })
        .collect();
    let response = service
        .request(
            scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes,
            },
        )
        .unwrap();
    let Reply::Modified(items) = response.result.unwrap() else {
        panic!("actual service save")
    };
    let saved = items
        .into_iter()
        .map(|item| {
            let MutationSuccess::Saved(record) = item.result.unwrap() else {
                panic!("actual saved entity")
            };
            let r = publication_control_record(&record).unwrap();
            CloudRecord {
                name: record.name,
                envelope: r.encrypted.unwrap(),
                system_fields: Some(r.system_fields.0),
                ..Default::default()
            }
        })
        .collect();
    fixture
        .workflow
        .sent(sdk.instance_id.clone(), batch.id, saved, vec![], vec![], vec![], count)
        .unwrap();
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
}

const BOUND_PREPARATION_RECIPE: &str = "recipe: publication-bound-preparation\ndescription: Public synthetic publication rows\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Publication\n    notetype: basic\n    new: 1\n    due: 1\n    tags: [generic]\n    fields: [\"generic {n}\", \"back {n}\"]\n";

fn bound_preparation_fixture() -> Fixture {
    let cache = Arc::new(CachedIdentity(Mutex::new(Some(r::IdentityStatus::Available(
        c::OpaqueBytes(b"synthetic-account-A".to_vec()),
    )))));
    Fixture::with_recipe(keys(), Some(cache), true, Some(BOUND_PREPARATION_RECIPE))
}

fn reopen_bound_preparation(fixture: &mut Fixture) {
    fixture.workflow.host.close().unwrap();
    let mut config = HostConfig::new(
        fixture._directory.path().join("collection"),
        fixture._directory.path().join("backups"),
        fixture._directory.path().join("widget"),
        fixture.clock.clone(),
        fixture.random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity: fixture.workflow.identity.as_ref().unwrap().clone(),
        files: Arc::new(NativeFiles),
    });
    prepare_prior_icloud_fixture_configuration(&config);
    let host = Arc::new(CoreHost::open(config).unwrap());
    let core = host.cloud_runtime().unwrap().clone();
    core.retain_adapter_inspection();
    fixture.workflow = CloudWorkflow {
        host,
        core,
        identity: fixture.workflow.identity.clone(),
    };
}

fn fresh_bound_identity(workflow: &CloudWorkflow) {
    let read = workflow.begin_identity_read(false, None).unwrap();
    assert!(
        workflow
            .complete_identity_read(
                read,
                CloudAccountStatus::Available,
                Some(b"synthetic-account-A".to_vec()),
            )
            .unwrap()
    );
}

fn assert_bound_preparation_closed(fixture: &Fixture) {
    let workflow = &fixture.workflow;
    let status = workflow.core.status().unwrap();
    assert!(status.identity_verified && status.stored_usable);
    assert!(status.scan_required && !status.scan_running && !status.scan_completed_this_open);
    assert!(!status.note_scan_completed_this_open);
    assert!(workflow.core.note_scan_proof().is_none());
    let stored = workflow.core.stored_state_for_testing().unwrap();
    assert!(!stored.whole_scan && stored.coverage.is_none() && stored.note_scan.is_none());
    for kind in c::row_payload::KINDS {
        assert!(workflow.core.entity_facts_page(kind, None).unwrap().is_empty());
    }
    assert!(workflow.durable_snapshot().unwrap().entities.is_empty());
    assert!(workflow.register_sdk(false).is_err());
    let instance = workflow.core.instance().unwrap();
    assert!(workflow.core.start_sdk_instance(&instance).is_err());
    let batch = workflow
        .core
        .build_collection_batch(
            &instance,
            &workflow.host.collection().unwrap(),
            &std::collections::BTreeSet::new(),
        )
        .unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    assert!(workflow.core.start_manual_send_call(&instance).unwrap().is_none());
    assert!(
        workflow
            .build_batch(instance.clone(), "meguri-root".into(), HashMap::new())
            .is_err()
    );
    assert!(workflow.begin_send(instance).unwrap().is_none());
}

fn bound_preparation_progress(reopen_after: Option<usize>) {
    let mut fixture = bound_preparation_fixture();
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .register_full(|_, _| true)
        .unwrap();
    let mut connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let run = tempfile::tempdir().unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(801)).unwrap();
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let (mut request, _) = own(&connection);
    let mut acks = 0;
    let mut reopened = false;
    loop {
        assert_eq!(
            fixture.workflow.core.publication_op().unwrap().unwrap().step,
            p::PublishStep::Bound
        );
        let mut repeated_zones = false;
        if reopen_after == Some(acks) && !reopened {
            let old = CloudWorkflow {
                host: fixture.workflow.host.clone(),
                core: fixture.workflow.core.clone(),
                identity: fixture.workflow.identity.clone(),
            };
            let old_connection = connection;
            let old_request = request;
            reopen_bound_preparation(&mut fixture);
            assert!(old.prepare_runtime().is_err());
            connection = fixture.workflow.connection().unwrap();
            failure_is(
                connection.publication_result(old_request, CloudPublicationResult::NotStarted),
                CloudConnectionFailure::WrongOwner,
            );
            assert!(old_connection.next_publication_work().is_err());
            // The identity cache is not process-local verification at reopen.
            fixture
                .workflow
                .identity
                .as_ref()
                .unwrap()
                .set(r::IdentityStatus::ReadFailed);
            assert!(fixture.workflow.prepare_runtime().is_err());
            assert!(!fixture.workflow.core.status().unwrap().identity_verified);
            fresh_bound_identity(&fixture.workflow);
            let (next, command) = own(&connection);
            assert_eq!(next.retained_for_test().kind, p::OwnKind::CreateZones);
            assert!(matches!(command, CloudOwnOperation::CreateZones { .. }));
            request = next;
            reopened = true;
            repeated_zones = true;
        }
        fresh_bound_identity(&fixture.workflow);
        let op = fixture.workflow.core.publication_op().unwrap().unwrap();
        let stored = fixture.workflow.core.stored_state_for_testing().unwrap();
        let requests = service.request_log().unwrap();
        // Exercises the original production entry with real structural rows;
        // the required outcome is deferred reconciliation, never Bound seeding.
        fixture.workflow.prepare_runtime().unwrap();
        assert!(fixture.workflow.core.publication_op().unwrap().unwrap() == op);
        assert_eq!(fixture.workflow.core.stored_state_for_testing().unwrap(), stored);
        assert!(service.request_log().unwrap() == requests);
        assert_bound_preparation_closed(&fixture);
        let expected = if repeated_zones || acks == 0 {
            p::OwnKind::CreateZones
        } else if acks == 1 {
            p::OwnKind::SaveMediaRoot
        } else {
            p::OwnKind::SavePublishingEpoch
        };
        assert_eq!(request.retained_for_test().kind, expected);
        let (reply, _) = execute_publication_request(
            &mut service,
            &scope,
            request.retained_for_test(),
            &fixture.workflow.core,
            &fixture.workflow.host.collection().unwrap(),
        )
        .unwrap();
        connection.publication_result(request, result(reply)).unwrap();
        // Zone creation is repeated at reopen; its success is process-local.
        acks = if expected == p::OwnKind::CreateZones {
            acks.max(1)
        } else {
            acks + 1
        };
        if fixture.workflow.core.publication_op().unwrap().unwrap().step == p::PublishStep::Zones {
            break;
        }
        request = own(&connection).0;
    }
    assert_eq!(reopened, reopen_after.is_some());
    assert!(!fixture.workflow.core.status().unwrap().scan_completed_this_open);
    assert!(fixture.workflow.register_sdk(false).is_err());
    fixture.workflow.prepare_runtime().unwrap();
    let status = fixture.workflow.core.status().unwrap();
    assert!(status.scan_completed_this_open && !status.scan_required);
    let proof = fixture.workflow.core.note_scan_proof().unwrap();
    assert_eq!(proof.receipt.identity().generation(), proof.snapshot_generation);
    assert_eq!(
        proof.receipt.identity().native_file(),
        (proof.snapshot_native_file.device, proof.snapshot_native_file.inode)
    );
    assert!(proof.receipt.through() >= proof.snapshot_seq);
    let stored = fixture.workflow.core.stored_state_for_testing().unwrap();
    assert!(!stored.drained && !stored.whole_scan);
    let coverage = stored.coverage.unwrap();
    assert_eq!(coverage.cursor.seq, proof.snapshot_seq.0);
    assert_eq!(coverage.cursor.covered_through, proof.receipt.through().0);
    assert_eq!(
        coverage.counts.iter().map(|(kind, _)| *kind).collect::<Vec<_>>(),
        c::row_payload::KINDS
    );
    for (kind, count) in coverage.counts {
        let entities = fixture.workflow.core.entity_facts_page(kind, None).unwrap();
        assert_eq!(entities.len() as u64, count);
        if kind != c::entity_codec::Kind::MediaName {
            assert!(count > 0, "generated {kind:?} rows must be reconciled");
        }
    }
    assert!(matches!(
        connection.next_publication_work().unwrap(),
        CloudPublicationWork::SendEntities
    ));
    let sdk = fixture.workflow.register_sdk(false).unwrap();
    for _ in 0..16 {
        match connection.next_publication_work().unwrap() {
            CloudPublicationWork::SendEntities => send_publication_batch(&fixture, &sdk, &mut service, &scope),
            CloudPublicationWork::Own { request, .. } => {
                assert_eq!(request.retained_for_test().kind, p::OwnKind::SaveCompleteEpoch);
                return;
            }
            _ => panic!("real publication must advance after the entity ACKs"),
        }
    }
    panic!("bounded publication progress after preparation");
}

#[test]
fn publication_bound_preparation_defers_initial_scan_until_control_acks() {
    bound_preparation_progress(None);
}

#[test]
fn publication_bound_preparation_reopen_preserves_partial_control_acks() {
    for control_acks in 0..3 {
        bound_preparation_progress(Some(control_acks));
    }
}

#[test]
fn publication_bound_preparation_does_not_bypass_account_mismatch() {
    for identity in [
        r::IdentityStatus::Available(c::OpaqueBytes(b"synthetic-account-B".to_vec())),
        r::IdentityStatus::SignedOut,
        r::IdentityStatus::TemporarilyUnavailable,
    ] {
        let fixture = bound_preparation_fixture();
        let connection = fixture.workflow.connection().unwrap();
        connection.confirm_publication(ready(&connection)).unwrap();
        let (request, _) = own(&connection);
        fixture.workflow.identity.as_ref().unwrap().set(identity);
        assert!(fixture.workflow.prepare_runtime().is_err());
        assert!(!fixture.workflow.core.status().unwrap().identity_verified);
        assert!(fixture.workflow.core.note_scan_proof().is_none());
        assert!(
            fixture
                .workflow
                .core
                .stored_state_for_testing()
                .unwrap()
                .coverage
                .is_none()
        );
        assert!(fixture.workflow.register_sdk(false).is_err());
        assert!(matches!(
            connection.next_publication_work().unwrap(),
            CloudPublicationWork::Idle
        ));
        assert!(fixture.workflow.core.publication_op().unwrap().unwrap().step == p::PublishStep::Bound);
        drop(request);
    }
}

struct RefusingQueue;
impl CloudPublicationQueue for RefusingQueue {
    fn enqueue(&self) -> bool {
        false
    }
}

#[test]
fn rejected_retired_sdk_account_events_preserve_actual_owner_preview_and_admission() {
    for rejected_status in [CloudAccountStatus::NoAccount, CloudAccountStatus::CouldNotDetermine] {
        let cache = Arc::new(CachedIdentity(Mutex::new(None)));
        let fixture = Fixture::with_identity(keys(), Some(cache));
        fixture
            .workflow
            .account_changed(
                None,
                CloudAccountStatus::Available,
                Some(b"synthetic-account-A".to_vec()),
            )
            .unwrap();
        let (retired, _, _) = fixture.baseline();
        fixture.workflow.retire_sdk(retired.instance_id.clone(), false).unwrap();
        fixture.workflow.release_sdk(retired.instance_id.clone()).unwrap();
        let current = fixture.workflow.register_sdk(false).unwrap();
        fixture
            .workflow
            .account_changed(
                Some(current.instance_id.clone()),
                CloudAccountStatus::Available,
                Some(b"synthetic-account-A".to_vec()),
            )
            .unwrap();
        assert!(
            fixture
                .workflow
                .account_changed(Some(retired.instance_id), rejected_status, None)
                .is_err()
        );
        fixture.workflow.validate_sdk(current.instance_id).unwrap();
        // Uses the very cache installed in CoreHost, through publication_account.
        let connection = fixture.workflow.connection().unwrap();
        connection.confirm_publication(ready(&connection)).unwrap();
        let (request, _) = own(&connection);
        assert!(
            connection
                .submit_publication_request(request, Arc::new(Queue(AtomicUsize::new(0))))
                .unwrap()
        );
    }
}

#[test]
fn false_queue_at_actual_active_save_restores_never_submitted_evidence() {
    let fixture = Fixture::new(keys());
    let (connection, request) = active_request(&fixture);
    let before = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(!before.switch_sent && before.active_attempt.is_none());
    assert!(
        !connection
            .submit_publication_request(request.clone(), Arc::new(RefusingQueue))
            .unwrap()
    );
    assert!(fixture.workflow.core.publication_op().unwrap().unwrap() == before);
    // No reply is fabricated for a queue that refused the operation.
    connection.trigger().unwrap();
    let (next, _) = own(&connection); // Fresh epoch read, then another save is allowed.
    assert!(
        connection
            .submit_publication_request(next, Arc::new(Queue(AtomicUsize::new(0))))
            .unwrap()
    );
    assert!(
        connection
            .publication_result(request, CloudPublicationResult::NotStarted)
            .is_err()
    );
}

#[test]
fn queue_refusal_retains_pending_attempt() {
    let fixture = Fixture::new(keys());
    let (connection, request, epoch) = active_request_with_epoch(&fixture);
    assert!(
        connection
            .submit_publication_request(request.clone(), Arc::new(Queue(AtomicUsize::new(0))))
            .unwrap()
    );
    let uncertain = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(uncertain.switch_sent && uncertain.active_attempt.is_some());
    // A failed terminal reply cannot prove the server did not accept it.
    connection
        .publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None })
        .unwrap();
    fixture.clock.step_ms(5_000);
    connection.trigger().unwrap();
    let (read, command) = own(&connection);
    assert!(matches!(command, CloudOwnOperation::Read { .. }));
    // Deliver the actual complete epoch saved by this fixture's Service prefix.
    assert!(
        connection
            .submit_publication_request(read.clone(), Arc::new(Queue(AtomicUsize::new(0))))
            .unwrap()
    );
    connection
        .publication_result(
            read,
            CloudPublicationResult::Read {
                read: CloudControlRead::Found { record: epoch },
            },
        )
        .unwrap();
    let (retry, command) = own(&connection);
    assert!(matches!(command, CloudOwnOperation::Save { .. }));
    assert!(
        !connection
            .submit_publication_request(retry, Arc::new(RefusingQueue))
            .unwrap()
    );
    assert!(fixture.workflow.core.publication_op().unwrap().unwrap() == uncertain);
}

#[test]
fn server_retry_duration_uses_reply_clock_and_survives_actual_host_restart() {
    let fixture = Fixture::new(keys());
    let connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let (request, _) = own(&connection);
    assert!(
        connection
            .submit_publication_request(request.clone(), Arc::new(Queue(AtomicUsize::new(0))))
            .unwrap()
    );
    fixture.clock.step_ms(3_000);
    connection
        .publication_result(
            request.clone(),
            CloudPublicationResult::RetryAfter { duration_ms: 120_000 },
        )
        .unwrap();
    let deadline = NOW + 123_000;
    assert_eq!(
        connection.recovery_status().unwrap().unwrap().retry_after_ms,
        Some(deadline)
    );
    fixture.clock.step_ms(5_000);
    connection.trigger().unwrap();
    assert!(matches!(
        connection.next_publication_work().unwrap(),
        CloudPublicationWork::Idle
    ));
    // Replaying the old reply must not extend the accepted deadline.
    assert!(
        connection
            .publication_result(request, CloudPublicationResult::RetryAfter { duration_ms: 120_000 })
            .is_err()
    );
    fixture.workflow.host.close().unwrap();
    let mut config = HostConfig::new(
        fixture._directory.path().join("collection"),
        fixture._directory.path().join("backups"),
        fixture._directory.path().join("widget"),
        fixture.clock.clone(),
        fixture.random.clone(),
        Arc::new(meguri_storage::space::StatvfsSpace),
    );
    config.cloud = Some(r::CloudInputs {
        scope: c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    prepare_prior_icloud_fixture_configuration(&config);
    let host = Arc::new(CoreHost::open(config).unwrap());
    let workflow = CloudWorkflow {
        core: host.cloud_runtime().unwrap().clone(),
        host: host.clone(),
        identity: None,
    };
    let reopened = workflow.connection().unwrap();
    assert_eq!(
        reopened.recovery_status().unwrap().unwrap().retry_after_ms,
        Some(deadline)
    );
    reopened.trigger().unwrap();
    assert!(matches!(
        reopened.next_publication_work().unwrap(),
        CloudPublicationWork::Idle
    ));
    fixture.clock.step_ms(115_001);
    reopened.trigger().unwrap();
    let (request, _) = own(&reopened);
    assert!(
        reopened
            .submit_publication_request(request, Arc::new(Queue(AtomicUsize::new(0))))
            .unwrap()
    );
    host.close().unwrap();
}

#[path = "cloud_connection_job_tests.rs"]
mod jobs;

// Real Host stage setup shared by the focused metadata/payload boundary tests.
fn adoption_fetch_ready(connection: &CloudConnection) {
    let (preview, active) = adoption_preview(connection);
    connection.confirm_adoption(preview).unwrap();
    let source_epoch = CloudControlRecord {
        encrypted: Some(
            control_codec::Epoch {
                epoch_id: [8; 16],
                record_key: [19; 32],
                crt: NOW / 1000,
                ver: 18,
                state: control_codec::EpochState::Complete,
                reason: control_codec::EpochReason::Initial,
                published_at_ms: NOW,
                counts: vec![],
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![2],
    };
    let root = CloudControlRecord {
        encrypted: Some(
            control_codec::MediaRoot {
                media_key: [23; 32],
                created_at_ms: NOW,
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![3],
    };
    for record in [active, source_epoch, root] {
        let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
            panic!("actual host adoption read")
        };
        assert!(matches!(
            action,
            CloudAdoptionAction::ReadActive { .. } | CloudAdoptionAction::Read { .. }
        ));
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record },
                },
            )
            .unwrap();
    }
}

fn adoption_config_record(key: &[u8], external: bool) -> (CloudStagingRecord, Vec<u8>) {
    use c::entity_codec as codec;
    let mut val = if external {
        vec![b'x'; codec::INLINE_ENVELOPE_BYTES + 16]
    } else {
        vec![b'x']
    };
    val.insert(0, b'"');
    val.push(b'"');
    let payload = codec::Payload::Config(codec::Config {
        key: key.to_vec(),
        mtime_secs: 1,
        val,
    });
    let bytes = payload.encode().unwrap();
    let entity = codec::Entity::build(
        codec::EntityFields {
            kind: codec::Kind::Config,
            key: key.to_vec(),
            origin: [1; 16],
            erev: 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: [2; 16],
            base_sha256: None,
        },
        &bytes,
    )
    .unwrap();
    assert_eq!(entity.payload.is_none(), external);
    let name = codec::entity_name(&[19; 32], codec::Kind::Config, key).unwrap();
    (
        CloudStagingRecord {
            name,
            record_type: "MGEntity".into(),
            record: CloudControlRecord {
                encrypted: Some(entity.encode().unwrap()),
                system_fields: vec![4],
            },
            payload_file: None,
        },
        bytes,
    )
}
fn adoption_action(connection: &CloudConnection) -> (Arc<CloudAdoptionWork>, CloudAdoptionAction) {
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("actual adoption work")
    };
    (handle, action)
}

#[test]
fn adoption_metadata_stages_inline_and_fetches_external_serially_before_cursor_or_validate() {
    for next in [None, Some(vec![5, 4, 3])] {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        adoption_fetch_ready(&connection);
        let (inline, _) = adoption_config_record(b"inline", false);
        let (one, one_bytes) = adoption_config_record(b"external-one", true);
        let (two, two_bytes) = adoption_config_record(b"external-two", true);
        let (page, action) = adoption_action(&connection);
        assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
        connection
            .adoption_result(
                page,
                CloudAdoptionResult::Page {
                    records: vec![inline, one.clone(), two.clone()],
                    deleted: vec![],
                    next: next.clone(),
                },
            )
            .unwrap();
        for (mut expected, bytes) in [(one, one_bytes), (two, two_bytes)] {
            let (handle, action) = adoption_action(&connection);
            assert!(
                matches!(action, CloudAdoptionAction::FetchPayload { ref name, expected: ref record, .. }
                if name == &expected.name && record.encrypted == expected.record.encrypted && record.system_fields == expected.record.system_fields)
            );
            assert_eq!(
                fixture.workflow.core.adoption_op().unwrap().unwrap().step,
                c::adoption::AdoptStep::Staging
            );
            let file = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(file.path(), &bytes).unwrap();
            expected.payload_file = Some(file.path().to_str().unwrap().into());
            connection
                .adoption_result(
                    handle.clone(),
                    CloudAdoptionResult::Payload {
                        record: expected.clone(),
                    },
                )
                .unwrap();
            // A consumed work handle never admits a second outstanding intake.
            assert!(
                connection
                    .adoption_result(handle, CloudAdoptionResult::Payload { record: expected })
                    .is_err()
            );
            assert_eq!(std::fs::read(file.path()).unwrap(), bytes);
        }
        let (_, action) = adoption_action(&connection);
        match next {
            Some(cursor) => {
                assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: Some(value), .. } if value == cursor))
            }
            None => assert!(matches!(action, CloudAdoptionAction::Validate)),
        }
        let status = connection.recovery_status().unwrap().unwrap();
        connection.cancel_operation(status.operation).unwrap();
        assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    }
}

#[test]
fn adoption_repeated_name_stages_external_then_inline_and_inline_then_external_in_arrival_order() {
    use c::entity_codec as codec;
    use meguri_storage::raw::{PlainDatabase, RawValue};
    for external_first in [true, false] {
        let account = c::OpaqueBytes(b"synthetic-account-A".to_vec());
        let identity = Arc::new(CachedIdentity(Mutex::new(Some(r::IdentityStatus::Available(
            account.clone(),
        )))));
        let fixture = Fixture::with_identity(Keys::new([]), Some(identity.clone()));
        let connection = fixture.workflow.connection().unwrap();
        adoption_fetch_ready(&connection);
        let (inline, inline_bytes) = adoption_config_record(b"repeated", false);
        let (external, external_bytes) = adoption_config_record(b"repeated", true);
        let (barrier, barrier_bytes) = adoption_config_record(b"barrier", true);
        assert_eq!(inline.name, external.name);
        let final_record = if external_first {
            inline.clone()
        } else {
            external.clone()
        };
        let final_bytes = if external_first { &inline_bytes } else { &external_bytes };
        let records = if external_first {
            vec![external.clone(), inline, barrier.clone()]
        } else {
            vec![inline, external.clone(), barrier.clone()]
        };
        let (page, _) = adoption_action(&connection);
        connection
            .adoption_result(
                page,
                CloudAdoptionResult::Page {
                    records,
                    deleted: vec![],
                    next: Some(vec![7, 8]),
                },
            )
            .unwrap();
        let (payload, action) = adoption_action(&connection);
        assert!(matches!(action, CloudAdoptionAction::FetchPayload { ref name, .. } if name == &external.name));
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &external_bytes).unwrap();
        let mut returned = external;
        returned.payload_file = Some(file.path().to_str().unwrap().into());
        identity.set(r::IdentityStatus::ReadFailed);
        failure_is(
            connection.adoption_result(
                payload,
                CloudAdoptionResult::Payload {
                    record: returned.clone(),
                },
            ),
            CloudConnectionFailure::AccountUnavailable,
        );
        assert_eq!(
            fixture.workflow.core.adoption_op().unwrap().unwrap().step,
            c::adoption::AdoptStep::Staging
        );
        identity.set(r::IdentityStatus::Available(account));
        let (restored, action) = adoption_action(&connection);
        assert!(
            matches!(action, CloudAdoptionAction::FetchPayload { ref name, expected: ref record, .. }
            if name == &returned.name && record.encrypted == returned.record.encrypted
                && record.system_fields == returned.record.system_fields)
        );
        connection
            .adoption_result(restored, CloudAdoptionResult::Payload { record: returned })
            .unwrap();
        // The later record wins in both the staged row and retained envelope.
        // Another external record holds the page cursor pending at this point.
        let op = fixture.workflow.core.adoption_op().unwrap().unwrap();
        assert_eq!(op.step, c::adoption::AdoptStep::Staging);
        let directory = fixture._directory.path().join("collection").join(&op.staging_name);
        let codec::Payload::Config(expected) = codec::Payload::decode(codec::Kind::Config, final_bytes).unwrap() else {
            panic!("config payload")
        };
        let db = PlainDatabase::open(&directory.join("collection.anki2")).unwrap();
        assert_eq!(
            db.rows("SELECT val FROM config WHERE key = ?", &[RawValue::text("repeated")])
                .unwrap(),
            vec![vec![RawValue::Blob(expected.val)]]
        );
        db.close().unwrap();
        let spool: serde_json::Value =
            serde_json::from_slice(&std::fs::read(directory.join("incoming").join(&final_record.name)).unwrap())
                .unwrap();
        assert_eq!(
            spool["envelope"],
            serde_json::json!(final_record.record.encrypted.unwrap())
        );
        let (payload, action) = adoption_action(&connection);
        assert!(matches!(action, CloudAdoptionAction::FetchPayload { ref name, .. } if name == &barrier.name));
        std::fs::write(file.path(), &barrier_bytes).unwrap();
        let mut returned = barrier;
        returned.payload_file = Some(file.path().to_str().unwrap().into());
        connection
            .adoption_result(payload, CloudAdoptionResult::Payload { record: returned })
            .unwrap();
        assert!(matches!(adoption_action(&connection).1,
            CloudAdoptionAction::Fetch { cursor: Some(cursor), .. } if cursor == vec![7, 8]));
        connection
            .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
            .unwrap();
    }
}

#[test]
fn adoption_direct_epoch_page_tampering_still_refuses_and_cleans_up() {
    for tamper_archive in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        let before = fixture.workflow.host.collection().unwrap().generation();
        adoption_fetch_ready(&connection);
        let (handle, action) = adoption_action(&connection);
        let CloudAdoptionAction::Fetch { mut expected_epoch, .. } = action else {
            panic!("epoch-bound page")
        };
        if tamper_archive {
            expected_epoch.system_fields.push(9);
        } else {
            expected_epoch.encrypted.as_mut().unwrap().push(9);
        }
        assert!(
            connection
                .adoption_result(
                    handle,
                    CloudAdoptionResult::Page {
                        records: vec![CloudStagingRecord {
                            name: "epoch".into(),
                            record_type: "MGEpoch".into(),
                            record: expected_epoch,
                            payload_file: None,
                        }],
                        deleted: vec![],
                        next: Some(vec![5]),
                    }
                )
                .is_err()
        );
        assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
        assert!(connection.recovery_status().unwrap().is_none());
        assert_eq!(fixture.workflow.host.collection().unwrap().generation(), before);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    }
}

#[test]
fn adoption_failed_missing_changed_or_corrupt_payload_aborts_candidate_and_provisional_cursor() {
    for failure in 0..6 {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        let before = fixture.workflow.host.collection().unwrap().generation();
        adoption_fetch_ready(&connection);
        let (record, bytes) = adoption_config_record(b"external", true);
        let (page, _) = adoption_action(&connection);
        connection
            .adoption_result(
                page,
                CloudAdoptionResult::Page {
                    records: vec![record.clone()],
                    deleted: vec![],
                    next: Some(vec![5]),
                },
            )
            .unwrap();
        let (handle, _) = adoption_action(&connection);
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), &bytes).unwrap();
        let mut returned = record;
        returned.payload_file = Some(file.path().to_str().unwrap().into());
        let reply = match failure {
            0 => CloudAdoptionResult::Failed,
            1 => {
                returned.payload_file = None;
                CloudAdoptionResult::Payload { record: returned }
            }
            2 => {
                returned.record.system_fields.push(9);
                CloudAdoptionResult::Payload { record: returned }
            }
            3 => {
                returned.record.encrypted.as_mut().unwrap().push(9);
                CloudAdoptionResult::Payload { record: returned }
            }
            4 => {
                std::fs::write(file.path(), b"wrong-payload").unwrap();
                CloudAdoptionResult::Payload { record: returned }
            }
            _ => CloudAdoptionResult::Read {
                read: CloudControlRead::Failed,
            },
        };
        assert!(connection.adoption_result(handle, reply).is_err());
        assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
        assert!(connection.recovery_status().unwrap().is_none());
        assert_eq!(fixture.workflow.host.collection().unwrap().generation(), before);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    }
}

#[test]
fn adoption_inline_metadata_selects_validate_and_oversized_metadata_aborts() {
    for variant in 0..3 {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        adoption_fetch_ready(&connection);
        let (mut record, _) = adoption_config_record(b"inline", false);
        let records = match variant {
            0 => vec![record],
            1 => vec![record; 33],
            _ => {
                record.record.system_fields = vec![1; 32 * 1024 * 1024];
                vec![record]
            }
        };
        let (page, _) = adoption_action(&connection);
        let result = connection.adoption_result(
            page,
            CloudAdoptionResult::Page {
                records,
                deleted: vec![],
                next: None,
            },
        );
        if variant == 0 {
            result.unwrap();
            assert!(matches!(adoption_action(&connection).1, CloudAdoptionAction::Validate));
            connection
                .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
                .unwrap();
        } else {
            assert!(result.is_err());
            assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
        }
    }
}

#[test]
fn adoption_pending_payload_cancel_and_late_page_failure_keep_old_collection() {
    for pending_payload in [false, true] {
        let fixture = Fixture::new(Keys::new([]));
        let connection = fixture.workflow.connection().unwrap();
        let before = fixture.workflow.host.collection().unwrap().generation();
        adoption_fetch_ready(&connection);
        let (page, _) = adoption_action(&connection);
        if pending_payload {
            let (record, _) = adoption_config_record(b"external", true);
            connection
                .adoption_result(
                    page,
                    CloudAdoptionResult::Page {
                        records: vec![record],
                        deleted: vec![],
                        next: Some(vec![5]),
                    },
                )
                .unwrap();
            let (handle, action) = adoption_action(&connection);
            assert!(matches!(action, CloudAdoptionAction::FetchPayload { .. }));
            connection
                .cancel_operation(connection.recovery_status().unwrap().unwrap().operation)
                .unwrap();
            assert!(connection.adoption_result(handle, CloudAdoptionResult::Failed).is_err());
        } else {
            // A later fetch failure must discard earlier staged input.
            let (record, _) = adoption_config_record(b"inline", false);
            connection
                .adoption_result(
                    page,
                    CloudAdoptionResult::Page {
                        records: vec![record],
                        deleted: vec![],
                        next: Some(vec![5]),
                    },
                )
                .unwrap();
            let (handle, action) = adoption_action(&connection);
            assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: Some(_), .. }));
            assert!(connection.adoption_result(handle, CloudAdoptionResult::Failed).is_err());
        }
        assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
        assert!(connection.recovery_status().unwrap().is_none());
        assert_eq!(fixture.workflow.host.collection().unwrap().generation(), before);
        assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    }
}

#[test]
fn adoption_wait_row_decodes_old_shape_and_same_generation_successor_discards_live_work() {
    let (fixture, process, config, _, _) = held_no_collection_host();
    let connection = fixture.workflow.connection().unwrap();
    adoption_fetch_ready(&connection);
    let original = fixture.workflow.core.adoption_op().unwrap().unwrap();
    let mut old = serde_json::to_value(&original).unwrap();
    old.as_object_mut().unwrap().remove("transport_wait");
    old.as_object_mut().unwrap().remove("retry_after_ms");
    let decoded: c::adoption::AdoptOp = serde_json::from_value(old).unwrap();
    assert_eq!(decoded, original);
    assert!(!decoded.transport_wait && decoded.retry_after_ms.is_none());
    let (handle, _) = adoption_action(&connection);
    connection
        .adoption_result(
            handle.clone(),
            CloudAdoptionResult::TransportFailed {
                code: 7,
                retry_after_duration_ms: Some(120_000),
            },
        )
        .unwrap();
    let waiting = fixture.workflow.core.adoption_op().unwrap().unwrap();
    let bytes = serde_json::to_vec(&waiting).unwrap();
    assert_eq!(serde_json::from_slice::<c::adoption::AdoptOp>(&bytes).unwrap(), waiting);
    let operation = connection.recovery_status().unwrap().unwrap().operation;
    let stage = fixture
        .workflow
        .host
        .cloud_connection_directory()
        .join(&waiting.staging_name);
    let generation = fixture.workflow.host.engine().generation();
    fixture.workflow.host.close().unwrap();
    assert!(!stage.exists());
    let successor = Arc::new(process.prepare_cloud_connection(config).unwrap());
    assert_eq!(successor.engine().generation(), generation);
    let workflow = CloudWorkflow {
        core: successor.cloud_runtime().unwrap().clone(),
        host: successor.clone(),
        identity: None,
    };
    let reopened = workflow.connection().unwrap();
    assert!(reopened.recovery_status().unwrap().is_none());
    failure_is(
        reopened.clone().start_resume_adoption(operation),
        CloudConnectionFailure::WrongOwner,
    );
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::TransportFailed {
                    code: 4,
                    retry_after_duration_ms: None,
                }
            )
            .is_err()
    );
    assert!(!successor.cloud_connection_directory().join("collection.anki2").exists());
    successor.close().unwrap();
}

#[test]
fn bridge_failed_active_attempt_can_cancel_after_original_delivery() {
    let fixture = Fixture::new(keys());
    let (connection, request) = active_request(&fixture);
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(connection.submit_publication_request(request.clone(), queue.clone()).unwrap());
    let status = connection.recovery_status().unwrap().unwrap();
    assert!(!status.publication_cancellable && !status.publication_conflict_cancellable);
    let submitted = fixture.workflow.core.publication_op().unwrap().unwrap();
    failure_is(connection.cancel_operation(status.operation.clone()), CloudConnectionFailure::Busy);
    assert!(fixture.workflow.core.publication_op().unwrap() == Some(submitted.clone()));
    connection.publication_result(request.clone(), CloudPublicationResult::Failed { retry_after_ms: None }).unwrap();
    let settled = connection.recovery_status().unwrap().unwrap();
    assert!(settled.switch_sent && settled.publication_cancellable && !settled.publication_conflict_cancellable);
    connection.cancel_operation(settled.operation).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
    failure_is(connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }), CloudConnectionFailure::AuthorityChanged);
    fixture.workflow.host.close().unwrap();
    let c::OpenOutcome::Opened(store) = c::TrackingStore::open(&fixture._directory.path().join("collection"), c::BuildScope {
        container: c::ContainerId::new("synthetic-u114").unwrap(), environment: c::Environment::Development,
    }).unwrap() else { panic!("durable tracking") };
    assert!(store.publication_op().unwrap().is_none());
    assert!(store.binding(submitted.binding_after.unwrap()).unwrap().is_none());
}

// P4/V15.3: cancellation deletes the publication binding before the next
// cloud refresh issues its stop. Keep the actual SDK and retained Host alive.
fn post_cancel_unbound_stop() -> (
    Fixture,
    Arc<crate::cloud_policy::CloudPolicy>,
    meguri_sync::cloud::policy::Command,
    String,
) {
    use meguri_sync::cloud::policy as policy;
    let fixture = Fixture::new(Keys::new([
        Some(vec![31; 32]), Some(vec![47; 32]), Some(vec![53; 32]), Some(vec![59; 32]),
    ]));
    let facade = fixture.workflow.policy().unwrap();
    let (connection, request) = active_request(&fixture);
    let sdk = fixture.workflow.core.incoming_sdk_instance_for_testing().unwrap();
    assert!(connection
        .submit_publication_request(request.clone(), Arc::new(Queue(AtomicUsize::new(0))))
        .unwrap());
    connection
        .publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None })
        .unwrap();
    let recovery = connection.recovery_status().unwrap().unwrap();
    assert!(recovery.switch_sent && recovery.publication_cancellable);
    connection.cancel_operation(recovery.operation).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
    let backend = fixture.workflow.core.adapter_guard();
    assert_eq!(backend.generation, 0);
    assert!(backend.binding.is_empty());
    let command = fixture.workflow.host
        .cloud_policy_input(policy::DeviceEvent::Refresh(policy::ChangeCause::Cloud))
        .unwrap()
        .into_iter()
        .find(|c| matches!(c.action.kind, policy::ActionKind::StopEngine { reason: policy::StopReason::GateClosed }))
        .expect("refresh-issued post-cancel StopEngine");
    assert_eq!(command.action.guard.generation, fixture.workflow.host.cloud_connection_generation().unwrap().0);
    assert!(command.action.guard.generation > 0);
    assert!(command.action.guard.binding.is_none() && command.owner.is_empty());
    assert_eq!(fixture.workflow.core.incoming_sdk_instance_for_testing(), Some(sdk.clone()));
    (fixture, facade, command, sdk)
}

fn stop_handle(
    policy: &Arc<crate::cloud_policy::CloudPolicy>,
    command: meguri_sync::cloud::policy::Command,
) -> Arc<crate::cloud_policy::CloudPolicyCommand> {
    let crate::cloud_policy::CloudPolicyOutput::Commands { mut commands } = policy.output(Ok(vec![command])).unwrap() else {
        panic!("issued stop handle")
    };
    commands.pop().unwrap().handle
}

#[test]
fn post_cancel_unbound_stop_prepares_validates_and_retires_only_original_sdk() {
    use meguri_sync::cloud::policy as policy;
    let (fixture, facade, command, sdk) = post_cancel_unbound_stop();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    driver.validate(&command).unwrap();
    assert!(matches!(driver.retire_native_sdk(&command, "other-sdk"), Err(policy::DriverError::StaleCommand)));
    assert_eq!(fixture.workflow.core.incoming_sdk_instance_for_testing(), Some(sdk.clone()));
    let execution = facade.clone().prepare_execution(stop_handle(&facade, command)).unwrap().unwrap();
    assert_eq!(execution.original_sdk().unwrap(), Some(sdk.clone()));
    execution.validate_control().unwrap();
    execution.retire_original().unwrap();
    // Match native ownership: cancel/drain precedes release and EngineStopped.
    fixture.workflow.release_sdk(sdk).unwrap();
    facade.clone().input(crate::cloud_policy::CloudPolicyEvent::Refresh {
        cause: crate::cloud_policy::CloudPolicyChangeCause::Cloud,
    }).unwrap();
    assert!(fixture.workflow.core.incoming_sdk_instance_for_testing().is_none());
    assert!(driver.state().engine.is_none());
    fixture.workflow.host.close().unwrap();
}

#[test]
fn post_cancel_unbound_stop_preserves_exact_guards_and_command_constraints() {
    use meguri_sync::cloud::{policy, BindingId};
    let (fixture, facade, command, sdk) = post_cancel_unbound_stop();
    let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
    driver.validate(&command).unwrap();
    for invalidation in 0..5 {
        let mut invalid = command.clone();
        match invalidation {
            0 => invalid.action.guard.token += 1,
            1 => invalid.action.guard.generation += 1,
            2 => invalid.action.guard.binding = Some(BindingId([7; 16])),
            3 => invalid.owner = "other-opening".into(),
            4 => invalid.action.kind = policy::ActionKind::RequestFetch { zones: vec![policy::Zone::Root] },
            _ => unreachable!(),
        }
        assert!(matches!(driver.validate(&invalid), Err(policy::DriverError::StaleCommand)), "case {invalidation}");
    }
    let mut wrong_action = command.clone();
    wrong_action.action.kind = policy::ActionKind::PauseMedia;
    assert!(matches!(driver.retire_native_sdk(&wrong_action, &sdk), Err(policy::DriverError::NotCoreAction)));
    let execution = facade.clone().prepare_execution(stop_handle(&facade, command.clone())).unwrap().unwrap();
    let policy_guard = driver.state().guard();
    let backend = fixture.workflow.core.adapter_guard();
    fixture.workflow.core.adapter_pause(true).unwrap();
    let changed = fixture.workflow.core.adapter_guard();
    assert_eq!(changed.generation, backend.generation);
    assert_eq!(changed.binding, backend.binding);
    assert!(changed.token > backend.token);
    assert_eq!(driver.state().guard(), policy_guard);
    assert!(matches!(driver.validate(&command), Err(policy::DriverError::StaleCommand)));
    assert!(matches!(execution.validate_control(), Err(CoreError::CloudPolicy {
        failure: crate::cloud_policy::CloudPolicyFailure::StaleCommand,
    })));
    assert!(matches!(execution.retire_original(), Err(CoreError::CloudPolicy {
        failure: crate::cloud_policy::CloudPolicyFailure::StaleCommand,
    })));
    assert!(matches!(facade.clone().prepare_execution(stop_handle(&facade, command)), Err(CoreError::CloudPolicy {
        failure: crate::cloud_policy::CloudPolicyFailure::StaleCommand,
    })));
    assert_eq!(fixture.workflow.core.incoming_sdk_instance_for_testing(), Some(sdk));
    fixture.workflow.host.close().unwrap();
}

#[test]
fn post_cancel_unbound_stop_rejects_account_binding_generation_close_and_other_host() {
    use crate::cloud_policy::CloudPolicyFailure as Failure;
    for invalidation in 0..5 {
        let (fixture, facade, command, _) = post_cancel_unbound_stop();
        let handle = stop_handle(&facade, command);
        let execution = facade.clone().prepare_execution(handle.clone()).unwrap().unwrap();
        let mut other = None;
        let expected = match invalidation {
            0 => {
                let driver = fixture.workflow.host.cloud_policy_driver().unwrap();
                let before = driver.state().guard();
                facade.clone().refresh_account().unwrap();
                assert_ne!(driver.state().guard(), before);
                Failure::StaleCommand
            }
            1 => {
                let connection = fixture.workflow.connection().unwrap();
                connection.confirm_publication(ready(&connection)).unwrap();
                let _ = own(&connection); // Settle the actual Bind and sidecar work.
                assert!(!fixture.workflow.core.adapter_guard().binding.is_empty());
                Failure::StaleCommand
            }
            2 => {
                let engine = fixture.workflow.host.engine();
                let slot = engine.acquire_slot(meguri_engine::LongOp::Restore).unwrap();
                let mut exclusive = slot.enter_exclusive(true).unwrap();
                exclusive.replace(|path| { assert!(path.exists()); Ok(()) }).unwrap();
                Failure::GenerationChanged
            }
            3 => {
                fixture.workflow.host.close().unwrap();
                Failure::Closed
            }
            4 => {
                let successor = Fixture::new(keys());
                assert!(matches!(successor.workflow.policy().unwrap().prepare_execution(handle.clone()),
                    Err(CoreError::CloudPolicy { failure: Failure::WrongOwner })));
                other = Some(successor);
                // The original command still belongs to its live Host.
                execution.validate_control().unwrap();
                fixture.workflow.host.close().unwrap();
                Failure::Closed
            }
            _ => unreachable!(),
        };
        for result in [
            facade.clone().prepare_execution(handle).map(|_| ()),
            execution.validate_control(),
            execution.retire_original(),
        ] {
            let Err(CoreError::CloudPolicy { failure }) = result else { panic!("old stop admitted: case {invalidation}") };
            assert_eq!(failure, expected);
        }
        if invalidation < 3 { fixture.workflow.host.close().unwrap(); }
        if let Some(other) = other { other.workflow.host.close().unwrap(); }
    }
}
