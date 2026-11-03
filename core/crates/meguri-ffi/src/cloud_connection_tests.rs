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
    let fixture = Fixture::without_collection(keys());
    let host = &fixture.workflow.host;
    let connection = fixture.workflow.connection().unwrap();
    let directory = host.cloud_connection_directory();
    let sidecar = std::fs::read(directory.join("meguri-sidecar.json")).ok();
    let info = connection
        .local_publication_information(host.cloud_connection_generation().unwrap().0)
        .unwrap();
    assert!(!info.collection_present && info.local_counts.is_empty());
    assert!(info.condition.is_none() && info.last_change_ms.is_none());
    assert!(!directory.join("collection.anki2").exists());
    assert_eq!(std::fs::read(directory.join("meguri-sidecar.json")).ok(), sidecar);
    // A file appearing in the retained no-collection host is stale, never
    // silently read/opened or treated as an absent collection.
    std::fs::write(directory.join("collection.anki2"), b"synthetic-invalid-file").unwrap();
    failure_is(
        connection.local_publication_information(host.cloud_connection_generation().unwrap().0),
        CloudConnectionFailure::AuthorityChanged,
    );
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
    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(directory.path().join("collection")).unwrap();
    std::fs::create_dir_all(directory.path().join("backups")).unwrap();
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let random = Arc::new(SeededRandom::new(114));
    let identity = Arc::new(HeldNoCollectionAccount { held: Mutex::new(None) });
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
        files: Arc::new(NativeFiles),
    });
    config.cloud_key_bytes = Some(Arc::new(ForeignKeyBytes(keys.clone())));
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
    assert!(matches!(result, Err(CoreError::CloudConnection { failure }) if failure == expected));
}
fn control(v: p::ControlRecord) -> CloudControlRecord {
    CloudControlRecord {
        encrypted: v.encrypted,
        system_fields: v.system_fields.0,
    }
}
fn result(v: p::OwnResult) -> CloudPublicationResult {
    match v {
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
    assert!(matches!(
        connection.recovery_status().unwrap().unwrap().step,
        CloudRecoveryStep::Confirmed
    ));
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
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    let operation = connection.confirm_adoption(preview).unwrap();
    assert_eq!(
        fixture.workflow.host.engine().slot_holder(),
        Some(meguri_engine::LongOp::CloudAdopt)
    );
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption work")
    };
    assert!(matches!(action, CloudAdoptionAction::Read { .. }));
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
    connection.cancel_operation(operation).unwrap();
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
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
    let (preview, active) = adoption_preview(&connection);
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
        assert!(matches!(action, CloudAdoptionAction::Read { .. }));
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record },
                },
            )
            .unwrap();
    }
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("actual stage page")
    };
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
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
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: Some(ref value), .. } if value == &cursor));
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
    assert!(stored.whole_scan);
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
