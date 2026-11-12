#![cfg(feature = "test-hooks")]
//! This binary alone owns the process-wide FFI runtime and cloud platform.
use meguri_ffi::{
    collection::{CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    collection_entry::{self, CollectionActivationOutcome, CollectionInspectOutcome},
    lifecycle,
    sync::{self, CloudAccountStatus, CloudEnvironment, CloudKeyByteProvider, CloudWorkflow},
    test_hooks,
};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::space::StatvfsSpace;
use meguri_testkit::generate::recipe::{Recipe, generate_recipe};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use std::time::Duration;

struct Clock;
impl HostClock for Clock {
    fn now_ms(&self) -> i64 {
        1_735_689_600_000
    }
    fn offset_from_utc(&self, _: i64) -> i32 {
        0
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}
struct Space;
impl DiskSpaceProvider for Space {
    fn available_bytes(&self, _: String) -> DiskCapacity {
        DiskCapacity::Available { bytes: u64::MAX }
    }
}
struct Keys(AtomicUsize);
impl CloudKeyByteProvider for Keys {
    fn draw_key(&self) -> Option<Vec<u8>> {
        self.0.fetch_add(1, Ordering::SeqCst);
        None
    }
}
struct Close;
impl Drop for Close {
    fn drop(&mut self) {
        lifecycle::close_collection().unwrap();
    }
}

#[test]
fn configured_cloud_is_available_after_initial_import_without_reopen() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let recipe = Recipe::parse(
        "recipe: cloud-initial-import\ndescription: Public initial import regression\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Public Study\n    notetype: basic\n    new: 1\n    fields: [\"public front {n}\", \"public back {n}\"]\n",
    )
    .unwrap();
    generate_recipe(&recipe, 801, &source).unwrap();
    let source_host = CoreHost::open(HostConfig::new(
        source,
        root.path().join("source-backups"),
        root.path().join("source-widget"),
        Arc::new(FixedClock::new(1_735_689_600_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(802)),
        Arc::new(StatvfsSpace),
    ))
    .unwrap();
    assert!(source_host.derived().wait_idle(Duration::from_secs(20)));
    let package = source_host
        .write_collection_export(
            &source_host.collection().unwrap(),
            &meguri_engine::CancelToken::new(),
            &meguri_engine::ProgressSink::new(),
        )
        .unwrap();
    source_host.close().unwrap();

    let target = root.path().join("target");
    std::fs::create_dir(&target).unwrap();
    assert!(!target.join("collection.anki2").exists(), "initial import must create an absent collection");
    let location = CollectionLocation {
        collection_directory: target.to_str().unwrap().into(),
        backups_directory: root.path().join("backups").to_str().unwrap().into(),
        app_group_snapshot: root.path().join("widget").to_str().unwrap().into(),
        manual_transfer_directory: root.path().join("manual").to_str().unwrap().into(),
    };
    let keys = Arc::new(Keys(AtomicUsize::new(0)));
    let retained_keys = Arc::downgrade(&keys);
    sync::configure_cloud_platform(
        "synthetic-initial-import".into(),
        CloudEnvironment::Development,
        CloudAccountStatus::Available,
        Some(b"synthetic-initial-account".to_vec()),
        keys.clone(),
    )
    .unwrap();
    drop(keys);
    let assert_keys_lazy = || {
        let keys = retained_keys.upgrade().expect("FFI must retain the key provider");
        assert_eq!(keys.0.load(Ordering::SeqCst), 0);
    };
    assert_keys_lazy();

    let engine = test_hooks::engine_for_testing().unwrap();
    let events = engine.stream().subscribe();
    let wait = |id| loop {
        let event = events
            .recv_timeout(Duration::from_secs(20))
            .expect("original import job terminal event");
        if let meguri_engine::EventKind::JobFinished { id: actual, outcome } = &event.kind
            && actual.0 == id
        {
            assert_eq!(outcome, &meguri_engine::JobOutcome::Succeeded);
            break;
        }
    };
    let inspection = collection_entry::start_collection_inspection(
        package.path.to_str().unwrap().into(),
        location,
        Arc::new(Clock),
        Arc::new(Space),
        803,
    )
    .unwrap();
    wait(inspection.job.id);
    let preview = match inspection.result.outcome().unwrap() {
        CollectionInspectOutcome::Ready { inspection } => inspection,
        CollectionInspectOutcome::Failed { failure } => panic!("inspection failed: {failure:?}"),
        CollectionInspectOutcome::Pending => panic!("inspection pending after terminal event"),
    };
    let facts = preview.facts().unwrap();
    assert!(!facts.replacing_existing);
    assert_eq!((facts.notes, facts.cards), (1, 1));
    assert_keys_lazy();

    let activation = collection_entry::start_collection_activation(preview).unwrap();
    let _close = Close;
    wait(activation.job.id);
    match activation.result.outcome().unwrap() {
        CollectionActivationOutcome::Activated { activation } => assert!(activation.backup.is_none()),
        other => panic!("initial activation failed: {other:?}"),
    }
    assert!(target.join("collection.anki2").is_file());
    assert_keys_lazy();

    // Keep the actual initially imported host; a normal reopen masks the bug.
    let workflow = CloudWorkflow::new().expect("initial import must retain configured cloud runtime");
    let _connection = workflow.connection().expect("initial import cloud connection");
    let _policy = workflow.policy().expect("initial import cloud policy");
    let _media = workflow.media().expect("initial import cloud media");
    assert_keys_lazy();
}
