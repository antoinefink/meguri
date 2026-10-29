#![cfg(feature = "test-hooks")]
//! The configured foreign provider is retained through the actual FFI opener;
//! no entropy is requested for composition, open or stable-binding discovery.
use meguri_ffi::{
    collection::{self, CollectionLocation, DiskCapacity, DiskSpaceProvider, HostClock},
    lifecycle,
    sync::{self, CloudAccountStatus, CloudEnvironment, CloudKeyByteProvider, CloudWorkflow},
    test_hooks,
};
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
fn configured_secure_provider_is_lazy_through_ffi_open_and_adapter_discovery() {
    let directory = tempfile::tempdir().unwrap();
    let data = directory.path().join("collection");
    let backups = directory.path().join("backups");
    std::fs::create_dir_all(&data).unwrap();
    std::fs::create_dir_all(&backups).unwrap();
    let location = CollectionLocation {
        collection_directory: data.to_str().unwrap().into(),
        backups_directory: backups.to_str().unwrap().into(),
        app_group_snapshot: directory.path().join("widget").to_str().unwrap().into(),
        manual_transfer_directory: directory.path().join("manual").to_str().unwrap().into(),
    };
    test_hooks::test_bootstrap_collection(location.clone()).unwrap();
    let keys = Arc::new(Keys(AtomicUsize::new(0)));
    sync::configure_cloud_platform(
        "synthetic-u114".into(),
        CloudEnvironment::Development,
        CloudAccountStatus::Available,
        Some(b"synthetic-account-A".to_vec()),
        keys.clone(),
    )
    .unwrap();
    assert_eq!(keys.0.load(Ordering::SeqCst), 0);
    let engine = test_hooks::engine_for_testing().unwrap();
    let events = engine.stream().subscribe();
    let job = collection::start_open(location, Arc::new(Clock), Arc::new(Space), 114).unwrap();
    let _close = Close;
    loop {
        let event = events
            .recv_timeout(Duration::from_secs(20))
            .expect("open terminal event");
        if let meguri_engine::EventKind::JobFinished { id, outcome } = &event.kind
            && id.0 == job.id
        {
            assert_eq!(outcome, &meguri_engine::JobOutcome::Succeeded);
            break;
        }
    }
    assert_eq!(keys.0.load(Ordering::SeqCst), 0);
    test_hooks::test_cloud_prepare_adapter_baseline(1, 3, None, false, false).unwrap();
    let workflow = CloudWorkflow::new().unwrap();
    workflow.register_sdk(false).unwrap();
    assert_eq!(keys.0.load(Ordering::SeqCst), 0);
    assert!(
        sync::configure_cloud_platform(
            "synthetic-u114".into(),
            CloudEnvironment::Development,
            CloudAccountStatus::NoAccount,
            None,
            keys.clone()
        )
        .is_err()
    );
    assert_eq!(keys.0.load(Ordering::SeqCst), 0);
}
