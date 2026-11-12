//! Registration failure and retry through the host-owned production runtime.
#![cfg(feature = "test-hooks")]

use std::{
    collections::{BTreeMap, BTreeSet},
    io::ErrorKind,
    sync::Arc,
    time::Duration,
};

use meguri_host::CoreHost;
use meguri_model::faultpoint::{self, Fault};
use meguri_sync::cloud::{
    BuildScope, ContainerId, Environment, Error, OpaqueBytes,
    adapter::collection_zone,
    entity_codec::{self as codec, Kind},
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, RuntimeError},
    workflow::{PendingChange, UpdateKind},
};
use meguri_testkit::cloud::devices::{NativeFiles, options};

#[path = "support/epoch_crt.rs"]
mod epoch_fixture;

struct Account;
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec()))
    }
}

#[test]
fn pending_discovery_failure_leaves_no_sdk_authority_and_allows_registration_retry() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let mut config = options(&directory, 1_894_701_600_000, 1145);
    meguri_engine::bootstrap::create(
        &directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    config.cloud = Some(CloudInputs {
        scope: BuildScope {
            container: ContainerId::new("synthetic-u114").unwrap(),
            environment: Environment::Development,
        },
        identity: Arc::new(Account),
        files: Arc::new(NativeFiles),
    });
    let host = CoreHost::open(config.clone()).unwrap();
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let deck = meguri_engine::decks::add(&collection, "Registration baseline".into())
        .unwrap()
        .value
        .id;
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    let saved_state = b"synthetic-saved-sdk-state".to_vec();
    // Reuse the SDK stable-binding baseline; no publication or new ack fixture.
    let (_, active, _) = runtime
        .prepare_adapter_baseline_for_testing(
            &collection,
            config.clock.as_ref(),
            config.random.as_ref(),
            Some(saved_state.clone()),
        )
        .unwrap();
    let mut service = meguri_testkit::cloud::Service::open(
        root.path(), meguri_testkit::cloud::Config::default(), meguri_model::rng::SeededRandom::new(1145),
    ).unwrap();
    epoch_fixture::accept(&host, &mut service, &meguri_testkit::cloud::Scope {
        container: "synthetic-u114".into(), environment: "development".into(), account: "synthetic-account-A".into(),
    });
    meguri_engine::decks::rename(&collection, deck, "Registration edit".into()).unwrap();
    runtime
        .reconcile_binding(&collection, &media, Duration::from_secs(10))
        .unwrap();
    let instance = runtime.instance().unwrap();
    assert!(runtime.accept_root_fetch(&instance, Some(&active), true, 1).unwrap());
    let status = runtime.status().unwrap();
    assert!(status.stored_usable && status.identity_verified && !status.scan_required);
    assert!(runtime.sdk_root_confirmed());
    let guard = runtime.adapter_guard();
    assert_eq!(guard.generation, collection.generation().0);
    assert_eq!(guard.binding, status.binding.as_ref().unwrap().id.0.to_vec());
    let name = codec::entity_name(&[114; 32], Kind::Deck, &codec::id_key(deck.0)).unwrap();
    let dirty = BTreeMap::from([(name.clone(), PendingChange::Save)]);
    assert_eq!(runtime.tracking_sendables(), dirty);
    assert!(!runtime.take_pending_updates(&instance).unwrap().is_empty());
    let emitted = runtime.emitted_update_ids();
    let observations = runtime.observations();

    const FAULT: &str = "cloud.adapter.register_sdk.before_pending";
    faultpoint::arm(FAULT, Fault::IoError(ErrorKind::Other));
    let failed = runtime.register_sdk(true);
    faultpoint::disarm(FAULT);
    assert!(matches!(
        failed,
        Err(RuntimeError::Tracking(Error::Io(ErrorKind::Other)))
    ));
    let failed_sdk = format!("sdk:{instance}:1");
    assert!(matches!(
        runtime.validate_sdk(&failed_sdk),
        Err(RuntimeError::StaleIncarnation)
    ));
    assert!(matches!(
        runtime.validate_sdk(&instance),
        Err(RuntimeError::StaleIncarnation)
    ));
    assert_eq!(runtime.emitted_update_ids(), emitted);
    assert_eq!(runtime.observations(), observations);
    assert!(runtime.take_pending_updates(&instance).unwrap().is_empty());
    assert_eq!(runtime.adapter_guard(), guard);
    assert!(runtime.sdk_root_confirmed());

    // Retry directly: neither retire nor release can be required for an ID
    // that register_sdk never returned. Failed attempts still consume IDs.
    let (sdk, serialization, zone) = runtime.register_sdk(true).unwrap();
    assert_eq!(sdk, format!("sdk:{instance}:2"));
    assert_eq!(serialization, Some(saved_state));
    assert_eq!(zone, collection_zone(status.binding.as_ref().unwrap().facts.epoch));
    runtime.validate_sdk(&sdk).unwrap();
    assert!(matches!(
        runtime.validate_sdk(&failed_sdk),
        Err(RuntimeError::StaleIncarnation)
    ));
    assert_eq!(runtime.adapter_guard(), guard);
    assert!(!runtime.sdk_root_confirmed());
    let updates = runtime.take_pending_updates(&sdk).unwrap();
    assert_eq!(updates.len(), 1);
    assert_eq!(updates[0].id, emitted.last().copied().unwrap() + 1);
    assert_eq!(updates[0].kind, UpdateKind::Full);
    assert!(updates[0].adds.is_empty());
    assert_eq!(updates[0].removes, BTreeSet::from([name]));
    assert!(runtime.tracking_sendables().is_empty());

    // The retried registration must discover pending names only after clearing
    // the previous proof. A normal root fetch makes the real edit sendable again.
    assert!(runtime.accept_root_fetch(&sdk, Some(&active), true, 2).unwrap());
    assert!(runtime.sdk_root_confirmed());
    runtime.validate_sdk(&sdk).unwrap();
    assert_eq!(runtime.adapter_guard(), guard);
    let confirmed = runtime.take_pending_updates(&sdk).unwrap();
    assert_eq!(confirmed.len(), 1);
    assert_eq!(confirmed[0].id, updates[0].id + 1);
    assert_eq!(confirmed[0].kind, UpdateKind::Full);
    assert_eq!(confirmed[0].adds, dirty);
    assert!(confirmed[0].removes.is_empty());
    assert_eq!(runtime.tracking_sendables(), dirty);
    host.close().unwrap();
}
