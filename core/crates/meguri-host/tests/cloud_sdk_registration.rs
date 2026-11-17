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
    config.manual_transfer_directory = root.path().join("synthetic-provider-installation");
    config.publication_binding = meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(), configuration: "synthetic-u114".into(), environment: "development".into(),
    };
    CoreHost::migrate_provider_configuration(&config.manual_transfer_directory, meguri_sync::provider::LegacyInput::Settings(
        meguri_sync::provider::LegacySettings { enabled: true, paused: false, automatic: true, media: true }
    )).unwrap();
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
    host.refresh_provider_admission();
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
    held_original_media_saved_finishes_during_provider_handoff(&host, &sdk);
    host.close().unwrap();
}

fn held_original_media_saved_finishes_during_provider_handoff(host: &CoreHost, sdk: &str) {
    use meguri_sync::cloud::{
        control_codec,
        media::{Reply, Request, ServerState, Transfer},
        workflow::{Observation, Saved},
    };

    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let media = host.media().unwrap();
    runtime.resume_cloud_media();
    let root = runtime.cloud_media_work(&collection, &media).unwrap().unwrap();
    assert!(matches!(root.request, Request::Root));
    // Only transport results are synthetic. The issued root read installs the
    // key through production intake; no ledger or completion proof is injected.
    runtime.cloud_media_reply(&collection, &media, &root, Reply::Record {
        name: "root".into(), record_type: "MGMediaRoot".into(),
        envelope: control_codec::media_root([115; 32], 1_894_701_600_000).unwrap(), asset: None,
    }).unwrap();
    let local = media.store().media_dir().join("held-original.bin");
    let bytes = b"synthetic held original media";
    std::fs::write(&local, bytes).unwrap();
    media.store().register_full(|_, _| true).unwrap();
    runtime.consume_media_feed(&collection, &media).unwrap();

    // ACK actual collection captures, including the original registration edit
    // and the new media name, before asking the runtime for an upload batch.
    for _ in 0..16 {
        let names = runtime.tracking_sendables().keys().cloned().collect();
        let batch = runtime.build_collection_batch(sdk, &collection, &names).unwrap();
        if batch.saves.is_empty() { break; }
        let saved: Vec<_> = batch.saves.iter().map(|save| Saved {
            name: save.name.clone(), envelope: save.envelope.clone(),
            system_fields: b"synthetic-collection-save-receipt".to_vec(),
        }).collect();
        runtime.enter_sent_handler(sdk, batch.id).unwrap();
        runtime.sent_results(sdk, &collection, batch.id, &saved, &[], saved.len()).unwrap();
        assert_eq!(runtime.return_sent_handler(sdk).unwrap(), batch.id);
        runtime.sdk_saved_removed(sdk, batch.id, &saved.iter().map(|r| r.name.clone()).collect::<Vec<_>>()).unwrap();
    }
    assert!(runtime.tracking_sendables().is_empty(), "bounded collection setup must finish");
    let mut upload = None;
    for _ in 0..16 {
        let work = runtime.cloud_media_work(&collection, &media).unwrap().expect("issued media setup work");
        match &work.request {
            Request::Upload { .. } => { upload = Some(work); break; }
            Request::Read { .. } => {
                runtime.cloud_media_reply(&collection, &media, &work, Reply::UnknownItem).unwrap();
            }
            _ => panic!("only metadata discovery may precede this local upload"),
        }
    }
    let work = upload.expect("bounded setup must issue the original upload");
    runtime.queue_cloud_media_upload(sdk, &collection, &work).unwrap();
    let Request::Upload { name, envelope, staged } = &work.request else { unreachable!() };
    let requested = BTreeMap::from([(name.clone(), PendingChange::Save)]);
    let (batch, original) = runtime.build_cloud_media_batch(sdk, &collection, &requested).unwrap().unwrap();
    assert!(original == work);
    assert!(runtime.validate_cloud_media_batch(sdk, &collection, batch).unwrap() == work);
    runtime.validate_prepared_batch(sdk, "meguri-media", batch).unwrap();
    assert_eq!(std::fs::read(staged).unwrap(), bytes);
    // Hold the original transport result while the real runtime owns its batch.
    let saved = Saved {
        name: name.clone(), envelope: envelope.clone(),
        system_fields: b"synthetic-original-blob-saved-receipt".to_vec(),
    };
    let before = runtime.media_blobs_for_testing().unwrap();
    assert_eq!(before[name].upload, Transfer::Running);
    assert_eq!(before[name].server, ServerState::Deleted);
    assert_eq!(before[name].staged.as_ref(), Some(staged));

    // A receipt never authorizes caller-invented authority, even before closure.
    for (index, mut stale) in [work.clone(), work.clone(), work.clone(), work.clone()].into_iter().enumerate() {
        match index {
            0 => stale.authority.generation += 1,
            1 => stale.authority.incarnation += 1,
            2 => stale.authority.account = OpaqueBytes(b"synthetic-other-account".to_vec()),
            _ => stale.authority.adapter_token += 1,
        }
        assert!(runtime.cloud_media_reply(&collection, &media, &stale, Reply::Saved {
            name: name.clone(), envelope: envelope.clone(),
        }).is_err());
    }
    assert!(runtime.media_blobs_for_testing().unwrap() == before);

    let _handoff = host.begin_provider_handoff(meguri_sync::provider::Selection::Meguri).unwrap();
    assert!(host.provider_authority().network_blocked());
    assert!(runtime.register_sdk(false).is_err());
    assert!(runtime.queue_cloud_media_upload(sdk, &collection, &work).is_err());
    assert!(runtime.build_cloud_media_batch(sdk, &collection, &requested).is_err());
    assert!(runtime.validate_cloud_media_batch(sdk, &collection, batch).is_err());
    assert!(runtime.validate_cloud_media_admission(sdk, &collection, &work).is_err());
    assert!(runtime.cloud_media_work(&collection, &media).is_err());
    assert!(runtime.adapter_begin_fetch(sdk, vec!["meguri-media".into()]).is_err());
    // Public replies cannot use a Saved result as a new admission capability.
    assert!(runtime.cloud_media_reply(&collection, &media, &work, Reply::Saved {
        name: name.clone(), envelope: envelope.clone(),
    }).is_err());
    assert!(runtime.cloud_media_sdk_sent("synthetic-stale-sdk", &collection, batch,
        std::slice::from_ref(&saved), &[], &[], &[], &[], &[]).is_err());
    assert!(!runtime.cloud_media_sdk_sent(sdk, &collection, batch + 1,
        std::slice::from_ref(&saved), &[], &[], &[], &[], &[]).unwrap());
    assert!(runtime.cloud_media_sdk_sent(sdk, &collection, batch,
        std::slice::from_ref(&saved), &[], &[], &[], &[], &[]).is_err(), "handler ownership is required");
    runtime.enter_sent_handler(sdk, batch).unwrap();
    let mut wrong = saved.clone();
    wrong.envelope.push(0);
    assert!(runtime.cloud_media_sdk_sent(sdk, &collection, batch,
        &[wrong], &[], &[], &[], &[], &[]).is_err());
    assert!(runtime.media_blobs_for_testing().unwrap() == before);
    assert!(staged.exists());

    assert!(runtime.cloud_media_sdk_sent(sdk, &collection, batch,
        std::slice::from_ref(&saved), &[], &[], &[], &[], &[]).unwrap());
    let after = runtime.media_blobs_for_testing().unwrap();
    assert_eq!(after.len(), before.len());
    assert_eq!(after[name].server, ServerState::Present);
    assert_eq!(after[name].upload, Transfer::Complete);
    assert!(after[name].staged.is_none());
    assert!(after[name].metadata == before[name].metadata);
    assert_eq!(after[name].download, before[name].download);
    assert!(!staged.exists());
    assert_eq!(std::fs::read(&local).unwrap(), bytes);
    assert!(runtime.sdk_saved_removed(sdk, batch, std::slice::from_ref(name)).is_err());
    assert_eq!(runtime.return_sent_handler(sdk).unwrap(), batch);
    assert!(runtime.sdk_saved_removed(sdk, batch + 1, std::slice::from_ref(name)).is_err());
    runtime.sdk_saved_removed(sdk, batch, std::slice::from_ref(name)).unwrap();
    assert!(runtime.observations().iter().any(|observation| matches!(observation,
        Observation::SavedPendingRemoved { name: actual, request_id } if actual == name && *request_id == batch)));
    assert!(!runtime.cloud_media_sdk_sent(sdk, &collection, batch,
        std::slice::from_ref(&saved), &[], &[], &[], &[], &[]).unwrap());
    assert!(runtime.sdk_saved_removed(sdk, batch, std::slice::from_ref(name)).is_err());
    assert!(runtime.queue_cloud_media_upload(sdk, &collection, &work).is_err());
    assert!(runtime.build_cloud_media_batch(sdk, &collection, &requested).is_err());
    assert!(runtime.cloud_media_work(&collection, &media).is_err());
    assert!(runtime.media_blobs_for_testing().unwrap() == after);
    assert!(host.provider_authority().network_blocked());
    // Keep the actual handoff pending: no finish call or fabricated native join.
}
