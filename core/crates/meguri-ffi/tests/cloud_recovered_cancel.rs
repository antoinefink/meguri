//! Test-first: actual child crash, engine-owned native replacement, public FFI
//! recovery handles. No fabricated Ready row, summary or activation authority.
#![cfg(feature = "test-hooks")]
use meguri_engine::{EngineError, LongOp, device_state::DeviceState};
use meguri_ffi::{
    CoreEvent, CoreEventKind, CoreEventListener, JobOutcome,
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    lifecycle,
    cloud_policy::{CloudPolicyAttention, CloudPolicyDeviceSettings, CloudPolicyEvent, CloudPolicyChangeCause, CloudPolicyStatusState},
    sync::{self, CloudAccountStatus, CloudEnvironment, CloudKeyByteProvider, CloudWorkflow},
};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{self as c, runtime::{FileIdentityProvider, KeyByteSource, KeySourceError}};
use meguri_testkit::{
    cloud::{Config, Scope, Service, coordinator::PublicationRig, devices::{IdentityReads, NativeFiles, options}},
    generate::recipe::{Recipe, generate_recipe},
};
use std::{path::Path, sync::{Arc, mpsc}, time::Duration};

const NOW: i64 = 1_735_689_600_000;
struct Keys;
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> { Ok([31; 32]) }
}
struct NoNewKeys;
impl CloudKeyByteProvider for NoNewKeys {
    fn draw_key(&self) -> Option<Vec<u8>> { panic!("local recovery Cancel must not draw keys") }
}
struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 { NOW }
    fn offset_from_utc(&self, _: i64) -> i32 { 0 }
    fn zone_id(&self) -> String { "UTC".into() }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity { DiskCapacity::Available { bytes: u64::MAX } }
}
struct Listener(mpsc::Sender<CoreEvent>);
impl CoreEventListener for Listener {
    fn on_event(&self, event: CoreEvent) { let _ = self.0.send(event); }
}
fn finished(rx: &mpsc::Receiver<CoreEvent>, job: u64) -> JobOutcome {
    loop {
        if let CoreEventKind::JobFinished { id, outcome } = rx.recv_timeout(Duration::from_secs(20)).unwrap().kind
            && id == job { return outcome; }
    }
}
fn tracking(directory: &Path, scope: &Scope) -> c::TrackingStore {
    match c::TrackingStore::open(directory, c::BuildScope {
        container: c::ContainerId::new(scope.container.clone()).unwrap(), environment: c::Environment::Development,
    }).unwrap() {
        c::OpenOutcome::Opened(store) => store,
        _ => panic!("actual tracking store"),
    }
}
#[test]
fn cloud_child_entry() { meguri_testkit::cloud::child_driver::child_entry().unwrap(); }

#[test]
fn recovery_status_cancel_operation_retires_only_recovered_candidate() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("receiver");
    let source = root.path().join("source");
    let recipe = Recipe::parse("recipe: recovered-cancel\ndescription: Public synthetic R6 recovery\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic\n    notetype: basic\n    new: 2\n    fields: [\"public {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 801, &source).unwrap();
    generate_recipe(&recipe, 802, &directory).unwrap();
    let scope = Scope { container: "synthetic-recovered-cancel".into(), environment: "development".into(), account: "synthetic-account".into() };
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let open = |path: &Path, seed| PublicationRig::open(path, &scope, IdentityReads::Available(scope.account.clone()), false, NOW, seed, Arc::new(Keys)).unwrap();
    let mut sender = open(&source, 809);
    let preview = sender.preview(&mut service, 10_000).unwrap();
    sender.confirm_publication(&preview).unwrap();
    sender.drive_ready(&mut service, 10_000).unwrap();
    let mut receiver = open(&directory, 810);
    let preview = receiver.preview(&mut service, 10_000).unwrap();
    receiver.adopt_cloud(&preview).unwrap();
    receiver.driver().arm_adoption_activation_crash().unwrap();
    assert!(receiver.drive_ready(&mut service, 10_000).is_err());
    receiver.driver().reap_adoption_activation_crash().unwrap();
    sender.close_clean().unwrap();
    let op = tracking(&directory, &scope).adoption_op().unwrap().unwrap();
    assert_eq!(op.step, c::adoption::AdoptStep::Ready);
    // Drain A through a real Host; the exclusive replacement retires A's
    // writer before copying B. It creates no R6 or cloud authority.
    let mut config = options(&directory, NOW, 819);
    config.publication_binding = op.candidate.binding.clone();
    let local = meguri_host::CoreHost::open(config).unwrap();
    let native_a = NativeFiles.identity(&directory.join("collection.anki2")).unwrap();
    {
        let slot = local.engine().acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        exclusive.replace(|live| {
            let copy = live.with_file_name("independent-copy.anki2");
            std::fs::copy(live, &copy).map_err(|e| EngineError::Storage(e.into()))?;
            std::fs::rename(copy, live).map_err(|e| EngineError::Storage(e.into()))?;
            Ok(())
        }).unwrap();
    }
    local.close().unwrap();
    assert_ne!(NativeFiles.identity(&directory.join("collection.anki2")).unwrap(), native_a);
    let (tx, rx) = mpsc::channel();
    meguri_ffi::events::set_event_listener(Arc::new(Listener(tx))).unwrap();
    sync::configure_cloud_platform(scope.container.clone(), CloudEnvironment::Development,
        CloudAccountStatus::Available, Some(scope.account.as_bytes().to_vec()), Arc::new(NoNewKeys)).unwrap();
    let location = CollectionLocation {
        collection_directory: directory.to_string_lossy().into_owned(),
        backups_directory: root.path().join("backups").to_string_lossy().into_owned(),
        app_group_snapshot: root.path().join("widget").to_string_lossy().into_owned(),
        manual_transfer_directory: root.path().join("manual").to_string_lossy().into_owned(),
    };
    // The FFI installation root is distinct from the publication rigs' root.
    // Admit its prior iCloud choice without changing the replacement Host's R6.
    meguri_host::CoreHost::migrate_provider_configuration(
        Path::new(&location.manual_transfer_directory),
        meguri_sync::provider::LegacyInput::Settings(meguri_sync::provider::LegacySettings {
            enabled: true, paused: false, automatic: false, media: false,
        }),
    ).unwrap();
    let open = || {
        let job = collection::start_open(location.clone(), Arc::new(Clock), Arc::new(Space), 821).unwrap();
        assert_eq!(finished(&rx, job.id), JobOutcome::Succeeded);
        CloudWorkflow::new().unwrap().connection().unwrap()
    };
    let stale = open();
    let stale_status = stale.recovery_status().unwrap().unwrap();
    assert!(stale_status.adoption_recovered_cancel_only);
    let stale_op = stale_status.operation;
    lifecycle::close_collection().unwrap();
    let connection = open();
    assert!(stale.cancel_operation(stale_op.clone()).is_err());
    assert!(connection.cancel_operation(stale_op).is_err());
    let status = connection.recovery_status().unwrap().unwrap();
    assert!(status.adoption_cancellable && !status.adoption_transport_wait);
    assert!(status.adoption_recovered_cancel_only);
    let policy = CloudWorkflow::new().unwrap().policy().unwrap();
    policy.clone().input(CloudPolicyEvent::Settings { settings: CloudPolicyDeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    } }).unwrap();
    policy.clone().refresh_account().unwrap();
    let before = policy.status().unwrap();
    assert!(matches!(before.state, CloudPolicyStatusState::NeedsAttention));
    assert!(!before.controls.decision_required);
    assert!(before.attention.iter().any(|a| matches!(a.kind, CloudPolicyAttention::RecoveryOpWaiting)));
    assert!(!before.attention.iter().any(|a| matches!(a.kind, CloudPolicyAttention::ChooseCollection)));
    assert!(matches!(connection.next_publication_work().unwrap(), meguri_ffi::cloud_connection::CloudPublicationWork::Idle));
    // The public resume job returns its actual failure, never an activation task.
    let resume = connection.clone().start_resume_adoption(status.operation.clone()).unwrap();
    assert!(matches!(finished(&rx, resume.handle.id), JobOutcome::Failed { .. }));
    assert!(resume.result.result().is_err());
    let sidecar = std::fs::read(directory.join("meguri-sidecar.json")).unwrap();
    let file = std::fs::read(directory.join("collection.anki2")).unwrap();
    connection.cancel_operation(status.operation.clone()).unwrap();
    assert!(connection.cancel_operation(status.operation).is_err());
    assert!(connection.recovery_status().unwrap().is_none());
    assert_eq!(std::fs::read(directory.join("meguri-sidecar.json")).unwrap(), sidecar);
    assert_eq!(std::fs::read(directory.join("collection.anki2")).unwrap(), file);
    policy.clone().input(CloudPolicyEvent::Refresh { cause: CloudPolicyChangeCause::Cloud }).unwrap();
    let after = policy.status().unwrap();
    assert!(matches!(after.state, CloudPolicyStatusState::NeedsAttention));
    assert!(!after.controls.decision_required);
    assert!(after.attention.iter().any(|a| matches!(a.kind, CloudPolicyAttention::ChooseCollection)));
    assert!(!after.attention.iter().any(|a| matches!(a.kind, CloudPolicyAttention::RecoveryOpWaiting)));
    let saved = tracking(&directory, &scope);
    assert!(saved.adoption_op().unwrap().is_none());
    assert!(saved.binding(op.binding_after).unwrap().is_none());
    drop(saved);
    let device = DeviceState::for_collection(&directory.join("collection.anki2"));
    let evidence = device.replacement_evidence_for(op.before.as_ref().unwrap().publication.identity).unwrap();
    assert!(evidence.is_some());
    lifecycle::close_collection().unwrap();
    let reopened = open();
    assert!(reopened.recovery_status().unwrap().is_none());
    assert_eq!(device.replacement_evidence_for(op.before.as_ref().unwrap().publication.identity).unwrap(), evidence);
    let policy = CloudWorkflow::new().unwrap().policy().unwrap();
    policy.clone().input(CloudPolicyEvent::Settings { settings: CloudPolicyDeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    } }).unwrap();
    policy.clone().refresh_account().unwrap();
    let reopened_status = policy.status().unwrap();
    assert!(matches!(reopened_status.state, CloudPolicyStatusState::NeedsAttention));
    assert!(!reopened_status.controls.decision_required);
    assert!(reopened_status.attention.iter().any(|a| matches!(a.kind, CloudPolicyAttention::ChooseCollection)));
    lifecycle::close_collection().unwrap();
}
