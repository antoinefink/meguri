//! P14.4 reference attention from actual owned SDK events and K13 applies.
use super::*;
use meguri_sync::cloud::{InboxReason, policy::{Attention, Lifecycle}};

fn reference_count(host: &CoreHost) -> Option<u64> {
    let db = host.collection().unwrap();
    let projection = host.cloud_runtime().unwrap().policy_projection(Lifecycle::Ready, db.generation().0).unwrap();
    assert!(!projection.facts.summary.attention_known, "other K10 producers remain missing");
    projection.facts.summary.attention.get(&Attention::ReferenceWaiting).copied()
}

#[test]
fn reference_waiting_same_fetch_classification_qualifies_and_reopen_preserves_original() {
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(original.entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), None);
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    // The SDK call has not returned successfully yet.
    assert_eq!(reference_count(&fixture.host), None);
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(1));
    fixture.host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
    assert_eq!(fixture.host.cloud_policy_driver().unwrap().status().state, StatusState::NeedsAttention);
    assert_eq!(cloud.policy_projection(Lifecycle::Ready, db.generation().0).unwrap().facts.summary.incoming_waiting, 1);
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), Some(1));
    let retained = reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap();
    assert_eq!(retained, vec![original]);
    reopened.close().unwrap();
}

#[test]
fn reference_waiting_predrain_foreign_failed_and_unowned_fetches_supply_no_receipt() {
    let fixture = rig();
    // Real accepted drain before there is any inbox entry.
    let runtime = fixture.host.cloud_runtime().unwrap();
    let old = runtime.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let old_group = format!("meguri-{old}");
    runtime.adapter_will_fetch(&fixture.sdk, old_group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&fixture.sdk, &old_group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&fixture.sdk, old).unwrap());
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(original.entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), None);
    for zone in ["meguri-root".to_owned(), "c-00000000000000000000000000000001".to_owned()] {
        // Neither boundary belongs to the collection zone under review.
        cloud.adapter_will_fetch(&fixture.sdk, "scheduled-foreign".into(), false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, zone, false, false).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, "scheduled-foreign", false).unwrap();
        assert_eq!(reference_count(&fixture.host), None);
    }
    for failed_zone in [true, false] {
        let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
        let group = format!("meguri-{fetch}");
        cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, failed_zone).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
        if failed_zone {
            assert!(!cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
        } else {
            // A paired boundary is still not accepted when the SDK API fails.
            assert!(!cloud.adapter_abort_fetch(&fixture.sdk, fetch).unwrap());
        }
        assert_eq!(reference_count(&fixture.host), None);
    }
    // Joining an older scheduled fetch does not own a manual drain.
    cloud.adapter_will_fetch(&fixture.sdk, "older-scheduled".into(), false).unwrap();
    let joined = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, "older-scheduled", false).unwrap();
    assert!(!cloud.adapter_finish_fetch(&fixture.sdk, joined).unwrap());
    assert_eq!(reference_count(&fixture.host), None);
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![original.clone()]);
    cloud.adapter_will_fetch(&fixture.sdk, "scheduled-current-zone".into(), false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, "scheduled-current-zone", false).unwrap();
    assert_eq!(reference_count(&fixture.host), Some(1));
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![original]);
    fixture.host.close().unwrap();
}

#[test]
fn reference_waiting_replacement_reopen_dependency_and_apply_expire_only_their_own_receipt() {
    let mut fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    // A second, independent remote child needs a different parent.
    let codec::DecodeOutcome::Supported(mut second_entity) = codec::Entity::decode(&remote.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut second_payload) = Payload::decode(second_entity.fields.kind, second_entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    second_payload.id = 9_000_002;
    second_payload.parent_id = 9_000_003;
    second_payload.name = b"Other reference child".to_vec();
    second_entity.fields.key = codec::id_key(second_payload.id).to_vec();
    let second = Saved {
        name: codec::entity_name(&binding.facts.record_key.0, codec::Kind::Deck, &second_entity.fields.key).unwrap(),
        envelope: codec::Entity::build(second_entity.fields, &Payload::Deck(second_payload).encode().unwrap()).unwrap().encode().unwrap(),
        system_fields: b"second-reference-version".to_vec(),
    };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[remote.clone(), second.clone()], &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(original.len(), 2);
    assert!(original.iter().all(|entry| entry.entry.reason == InboxReason::Reference));
    assert_eq!(reference_count(&fixture.host), None);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(2));
    let replacement = advance_server_record(&remote);
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&replacement), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let replaced = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(replaced.len(), 2);
    let old_first = original.iter().find(|e| e.entry.record_name == remote.name).unwrap();
    let new_first = replaced.iter().find(|e| e.entry.record_name == remote.name).unwrap();
    assert_ne!(old_first.id, new_first.id);
    assert_eq!(new_first.entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), Some(1));
    let second_original = original.iter().find(|e| e.entry.record_name == second.name).unwrap();
    assert!(replaced.contains(second_original));
    drop(db);
    fixture.host.close().unwrap();
    fixture.host = Arc::new(CoreHost::open(fixture.config.clone()).unwrap());
    fixture.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    })).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())), origin_sdk: None,
    }).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
    let reopened = fixture.host.cloud_runtime().unwrap();
    reopened.reconcile_binding(&fixture.host.collection().unwrap(), &fixture.host.media().unwrap(), Duration::from_secs(10)).unwrap();
    reopened.accept_root_fetch(&reopened.instance().unwrap(), Some(&fixture.active), true, 1).unwrap();
    let (sdk, _, zone) = reopened.register_sdk(false).unwrap();
    fixture.sdk = sdk;
    fixture.zone = zone;
    assert_eq!(reference_count(&fixture.host), Some(1));
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), replaced);
    // Accept and apply only the second child's dependency. The first child
    // remains waiting and gains no qualification from the older drain.
    let codec::DecodeOutcome::Supported(mut parent_entity) = codec::Entity::decode(&second.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut parent) = Payload::decode(parent_entity.fields.kind, parent_entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    parent.id = 9_000_003;
    parent.parent_id = 0;
    parent.name = b"Arrived reference parent".to_vec();
    parent_entity.fields.key = codec::id_key(parent.id).to_vec();
    let dependency = Saved {
        name: codec::entity_name(&binding.facts.record_key.0, codec::Kind::Deck, &parent_entity.fields.key).unwrap(),
        envelope: codec::Entity::build(parent_entity.fields, &Payload::Deck(parent).encode().unwrap()).unwrap().encode().unwrap(),
        system_fields: b"arrived-parent-version".to_vec(),
    };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[dependency], &[]).unwrap();
    assert!(matches!(cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap(), meguri_sync::cloud::incoming::IncomingDisposition::Progress(n) if n > 0));
    // The bounded worker applied the parent first; it has not applied the
    // already-waiting child yet, so that child's receipt is still present.
    assert_eq!(reference_count(&fixture.host), Some(1));
    assert!(matches!(cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap(), meguri_sync::cloud::incoming::IncomingDisposition::Progress(n) if n > 0));
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![new_first.clone()]);
    assert_eq!(reference_count(&fixture.host), None);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(1));
    // A later accepted version becomes Generation instead of Reference.
    let update = advance_server_record(&replacement);
    let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    entity.fields.generation = Some([83; 16]);
    let generation = Saved { envelope: codec::Entity::build(entity.fields, entity.payload.as_deref().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[generation], &[]).unwrap();
    assert_eq!(cloud.inbox_entries(binding.id).unwrap()[0].entry.reason, InboxReason::Generation);
    assert_eq!(reference_count(&fixture.host), None);
    // The next ordinary version has no missing reference and really applies.
    let update = advance_server_record(&advance_server_record(&replacement));
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 0;
    let ready = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[ready], &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    assert!(cloud.inbox_entries(binding.id).unwrap().is_empty());
    assert_eq!(reference_count(&fixture.host), None);
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), None);
    reopened.close().unwrap();
}

#[test]
fn reference_waiting_stale_sdk_account_incarnation_file_and_publication_cannot_grant_or_clear() {
    for change in ["sdk", "account", "incarnation", "file", "publication"] {
        for qualified in [false, true] {
            let fixture = rig();
            let cloud = fixture.host.cloud_runtime().unwrap();
            let db = fixture.host.collection().unwrap();
            let binding = cloud.status().unwrap().binding.unwrap();
            let source = fixture.records.iter().find(|saved| {
                matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
                    if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
            }).unwrap();
            let update = advance_server_record(source);
            let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
            let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
            deck.parent_id = 9_000_001;
            let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
            cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
            cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
            let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
            assert_eq!(original.entry.reason, InboxReason::Reference);
            assert_eq!(reference_count(&fixture.host), None);
            if qualified {
                let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
                let group = format!("meguri-{fetch}");
                cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
                cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
                cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
                cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
                assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
            }
            let expected = qualified.then_some(1);
            assert_eq!(reference_count(&fixture.host), expected);
            let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
            let group = format!("meguri-{fetch}");
            cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
            cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
            cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
            cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
            let before = cloud.adapter_fetch_drains();
            let live = fixture._root.path().join("collection/collection.anki2");
            let retained = fixture._root.path().join("collection/original-for-drain");
            let sidecar = fixture._root.path().join("collection/meguri-sidecar.json");
            let sidecar_before = std::fs::read(&sidecar).unwrap();
            match change {
                "sdk" => cloud.retire_sdk(&fixture.sdk, false).unwrap(),
                "account" => cloud.adapter_account(None, IdentityStatus::Available(OpaqueBytes(b"different-account".to_vec()))).unwrap(),
                "incarnation" => { fixture.host.engine().suspend(Duration::from_secs(10)).unwrap(); },
                "file" => {
                    let copy = live.with_extension("drain-copy");
                    std::fs::copy(&live, &copy).unwrap();
                    std::fs::rename(&live, &retained).unwrap();
                    std::fs::rename(copy, &live).unwrap();
                    cloud.validate_sdk(&fixture.sdk).unwrap();
                }
                "publication" => {
                    // Another healthy fixture emits a different real publication.
                    let donor = rig_with_root_seed(true, 2901);
                    let bytes = std::fs::read(donor._root.path().join("collection/meguri-sidecar.json")).unwrap();
                    donor.host.close().unwrap();
                    std::fs::write(&sidecar, bytes).unwrap();
                    cloud.validate_sdk(&fixture.sdk).unwrap();
                }
                _ => unreachable!(),
            }
            assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).is_err(), "{change}");
            assert_eq!(cloud.adapter_fetch_drains(), before, "{change}");
            if change == "file" {
                std::fs::remove_file(&live).unwrap();
                std::fs::rename(retained, &live).unwrap();
            }
            if change == "publication" { std::fs::write(sidecar, sidecar_before).unwrap(); }
            if change == "incarnation" {
                // Inspect durable results through a newly opened Host after suspension.
                fixture.host.close().unwrap();
                let reopened = CoreHost::open(fixture.config.clone()).unwrap();
                assert_eq!(reference_count(&reopened), expected, "{change}");
                assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap(), vec![original]);
                reopened.close().unwrap();
            } else {
                assert_eq!(reference_count(&fixture.host), expected, "{change}");
                assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![original]);
                fixture.host.close().unwrap();
            }
        }
    }
}

#[test]
fn reference_waiting_no_state_point_rejects_pre_reset_zone_completion() {
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(original.entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), None);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_account(Some(&fixture.sdk), IdentityStatus::Available(binding.facts.account.clone())).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(!cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), None);
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![original]);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(1));
    fixture.host.close().unwrap();
}

#[test]
fn reference_waiting_legacy_drain_history_is_unknown_until_new_accepted_work() {
    let mut fixture = rig();
    let runtime = fixture.host.cloud_runtime().unwrap();
    let fetch = runtime.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(runtime.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(original.entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), None);
    drop(db);
    fixture.host.close().unwrap();
    // Model the old physical schema, keeping its real inbox and accepted drain
    // history. No attention receipt, summary or accepted result is seeded.
    let c = meguri_storage::rusqlite::Connection::open(fixture._root.path().join("collection/meguri-cloud.db")).unwrap();
    c.execute_batch("DROP TRIGGER cloud_reference_deleted; DROP TRIGGER cloud_reference_changed; DROP TRIGGER cloud_reference_arrived;
        DROP TABLE cloud_reference_waiting; DROP TABLE cloud_reference_drains; DROP TABLE cloud_reference_schema;").unwrap();
    drop(c);
    fixture.host.close().unwrap();
    fixture.host = Arc::new(CoreHost::open(fixture.config.clone()).unwrap());
    fixture.host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: false,
    })).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())), origin_sdk: None,
    }).unwrap();
    fixture.host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
    let reopened = fixture.host.cloud_runtime().unwrap();
    reopened.reconcile_binding(&fixture.host.collection().unwrap(), &fixture.host.media().unwrap(), Duration::from_secs(10)).unwrap();
    reopened.accept_root_fetch(&reopened.instance().unwrap(), Some(&fixture.active), true, 1).unwrap();
    let (sdk, _, zone) = reopened.register_sdk(false).unwrap();
    fixture.sdk = sdk;
    fixture.zone = zone;
    let cloud = fixture.host.cloud_runtime().unwrap();
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![original]);
    assert_eq!(reference_count(&fixture.host), None);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(1));
    fixture.host.close().unwrap();
}

#[test]
fn reference_waiting_ready_at_drain_counts_after_classification_without_second_fetch() {
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    let remote = Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update };
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[remote], &[]).unwrap();
    let ready = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(ready.entry.reason, InboxReason::Ready);
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), None);
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let waiting = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(waiting.id, ready.id);
    assert_eq!(waiting.entry.value, ready.entry.value);
    assert_eq!(waiting.entry.reason, InboxReason::Reference);
    // P14.4: the same version was already covered by the accepted fetch.
    assert_eq!(reference_count(&fixture.host), Some(1));
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![waiting.clone()]);
    assert_eq!(reference_count(&fixture.host), Some(1));
    // A real fetched deletion replaces the qualified entry, without granting
    // the deletion its predecessor's proof (ordinary structural apply is separate).
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], &[waiting.entry.record_name]).unwrap();
    let deleted = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(deleted.len(), 1);
    assert_ne!(deleted[0].entry.value, waiting.entry.value);
    assert_eq!(deleted[0].entry.value, meguri_sync::cloud::InboxValue::Deletion);
    assert_eq!(deleted[0].entry.reason, InboxReason::Ready);
    assert_eq!(reference_count(&fixture.host), None);
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), None);
    reopened.close().unwrap();
}

fn missing_parent(fixture: &Rig) -> Saved {
    let source = fixture.records.iter().find(|saved| {
        matches!(codec::Entity::decode(&saved.envelope).unwrap(), codec::DecodeOutcome::Supported(e)
            if e.fields.kind == codec::Kind::Deck && e.fields.key == codec::id_key(fixture.baseline_deck.0))
    }).unwrap();
    let update = advance_server_record(source);
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&update.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.parent_id = 9_000_001;
    Saved { envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(), ..update }
}

#[test]
fn reference_waiting_asset_retry_and_open_keep_same_service_drain_but_not_newer_version() {
    use meguri_sync::cloud::{InboxValue, incoming::AssetReadKind, workflow::FetchedRecord};
    for reopen in [false, true] {
        let mut fixture = rig();
        let seed = missing_parent(&fixture);
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&seed.envelope).unwrap() else { unreachable!() };
        let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
        deck.name = vec![b'z'; codec::INLINE_ENVELOPE_BYTES + 1];
        let payload = Payload::Deck(deck).encode().unwrap();
        let remote = Saved {
            envelope: codec::Entity::build(entity.fields, &payload).unwrap().encode().unwrap(),
            ..seed
        };
        let cloud = fixture.host.cloud_runtime().unwrap();
        let db = fixture.host.collection().unwrap();
        let binding = cloud.status().unwrap().binding.unwrap().id;
        cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
        let original = cloud.inbox_entries(binding).unwrap().remove(0);
        assert_eq!(original.entry.reason, InboxReason::Asset);
        assert!(matches!(&original.entry.value, InboxValue::Record { staged_asset: None, .. }));
        cloud.adapter_will_fetch(&fixture.sdk, "asset-original-drain".into(), false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, "asset-original-drain", false).unwrap();
        assert_eq!(reference_count(&fixture.host), None);

        // Consume an actual failed read before exercising the open-time reset.
        let pass = cloud.begin_asset_read_pass(&fixture.sdk, &db, AssetReadKind::Retry).unwrap();
        let request = cloud.next_asset_read(&db, &pass).unwrap().unwrap();
        assert!(cloud.submit_asset_read_queue(&db, &request, || true).unwrap());
        assert!(!cloud.asset_read_result(&fixture.sdk, &db, &request, None).unwrap());
        cloud.cancel_asset_read_pass(&pass);
        let failed = cloud.inbox_entries(binding).unwrap().remove(0);
        assert_eq!(failed.id, original.id);
        assert_eq!(failed.entry.value, original.entry.value);
        assert_eq!(failed.entry.reason, InboxReason::Asset);
        assert_eq!(failed.entry.retry_count, 1);
        drop(db);
        if reopen { incomplete_reopen(&mut fixture); }
        let cloud = fixture.host.cloud_runtime().unwrap();
        let db = fixture.host.collection().unwrap();
        let retained = cloud.inbox_entries(binding).unwrap().remove(0);
        assert_eq!(retained.id, original.id);
        assert_eq!(retained.entry.value, original.entry.value);
        assert_eq!(retained.entry.reason, InboxReason::Asset);
        assert_eq!(retained.entry.retry_count, if reopen { 0 } else { 1 });
        assert_eq!(reference_count(&fixture.host), None);

        let downloaded = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(downloaded.path(), &payload).unwrap();
        for newer in [false, true] {
            let record = if newer {
                let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&remote.envelope).unwrap() else { unreachable!() };
                entity.fields.erev += 1;
                let record = Saved {
                    name: remote.name.clone(),
                    envelope: codec::Entity::build(entity.fields, &payload).unwrap().encode().unwrap(),
                    system_fields: b"asset-newer-service-version".to_vec(),
                };
                cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&record), &[]).unwrap();
                assert_eq!(reference_count(&fixture.host), None);
                record
            } else { remote.clone() };
            let before = cloud.inbox_entries(binding).unwrap().remove(0);
            assert_eq!(before.entry.reason, InboxReason::Asset);
            if newer { assert_ne!(before.entry.value, original.entry.value); }
            let pass = cloud.begin_asset_read_pass(&fixture.sdk, &db, AssetReadKind::Retry).unwrap();
            let request = cloud.next_asset_read(&db, &pass).unwrap().unwrap();
            assert_eq!(request.record_name(), record.name.as_str());
            assert!(cloud.submit_asset_read_queue(&db, &request, || true).unwrap());
            assert!(cloud.asset_read_result(&fixture.sdk, &db, &request, Some(&FetchedRecord {
                record: record.clone(), asset_path: Some(downloaded.path().to_owned()),
            })).unwrap());
            cloud.cancel_asset_read_pass(&pass);
            let staged = cloud.inbox_entries(binding).unwrap().remove(0);
            assert_eq!(staged.id, before.id);
            assert_eq!(staged.entry.reason, InboxReason::Ready);
            assert_eq!(staged.entry.retry_count, 0);
            assert_eq!(staged.entry.source, before.entry.source);
            assert_eq!(staged.entry.absence_pass, before.entry.absence_pass);
            assert_eq!(staged.entry.fetched_at_ms, before.entry.fetched_at_ms);
            let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(&record.envelope).unwrap() else { unreachable!() };
            let InboxValue::Record { envelope, erev, system_fields, staged_asset } = &staged.entry.value else { unreachable!() };
            assert_eq!(envelope.0, record.envelope);
            assert_eq!(*erev, entity.fields.erev);
            assert_eq!(system_fields.0, record.system_fields);
            assert!(staged_asset.is_some());
            cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
            let waiting = cloud.inbox_entries(binding).unwrap().remove(0);
            assert_eq!(waiting.id, before.id);
            assert_eq!(waiting.entry.reason, InboxReason::Reference);
            assert_eq!(waiting.entry.value, staged.entry.value);
            assert_eq!(reference_count(&fixture.host), if newer { None } else { Some(1) });
            if newer {
                cloud.adapter_will_fetch(&fixture.sdk, "asset-newer-drain".into(), false).unwrap();
                cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
                cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
                cloud.adapter_did_fetch(&fixture.sdk, "asset-newer-drain", false).unwrap();
                assert_eq!(reference_count(&fixture.host), Some(1));
                assert_eq!(cloud.inbox_entries(binding).unwrap(), vec![waiting]);
            }
        }
        let retained = cloud.inbox_entries(binding).unwrap();
        fixture.host.close().unwrap();
        let reopened = CoreHost::open(fixture.config.clone()).unwrap();
        assert_eq!(reference_count(&reopened), Some(1));
        assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding).unwrap(), retained);
        reopened.close().unwrap();
    }
}

#[test]
fn reference_waiting_no_state_found_epoch_id_read_qualifies_existing_version() {
    use meguri_sync::cloud::publication::{ControlRecord, RecordRead};
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[missing_parent(&fixture)], &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(original.len(), 1);
    assert_eq!(original[0].entry.reason, InboxReason::Reference);
    assert_eq!(reference_count(&fixture.host), None);
    cloud.adapter_account(Some(&fixture.sdk), IdentityStatus::Available(binding.facts.account.clone())).unwrap();
    let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
    let group = format!("meguri-{fetch}");
    cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
    // This fetch omits the zone; only its issued record-ID read can decide it.
    cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
    let mut reads = cloud.adapter_fetch_reads(&fixture.sdk, fetch, &db).unwrap();
    assert_eq!(reads.len(), 1);
    let read = reads.remove(0);
    assert!(!read.is_active());
    assert_eq!(reference_count(&fixture.host), None);
    // P3 accepts typed epoch existence even without an encrypted body. This
    // supplies no epoch-crt or other control-record authority.
    cloud.adapter_complete_fetch_read(&read, &db, RecordRead::Found(ControlRecord {
        encrypted: None, system_fields: OpaqueBytes(b"found-epoch".to_vec()),
    }), false, None, None).unwrap();
    assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
    assert_eq!(reference_count(&fixture.host), Some(1));
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), original);
    assert!(cloud.adapter_fetch_drains().iter().any(|d| d.zone == fixture.zone && !d.fetched));
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), Some(1));
    assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap(), original);
    reopened.close().unwrap();
}

// Same provider seam as the existing live-resume fixture: a one-shot hook
// pauses an actual file observation, outside State and tracking ownership.
#[derive(Default)]
struct PausedFiles(std::sync::Mutex<Option<Box<dyn FnOnce() + Send>>>);
impl meguri_sync::cloud::runtime::FileIdentityProvider for PausedFiles {
    fn identity(&self, path: &std::path::Path) -> Result<meguri_sync::cloud::runtime::NativeFileIdentity, RuntimeError> {
        let hook = self.0.lock().unwrap().take();
        if let Some(hook) = hook { hook(); }
        meguri_sync::cloud::runtime::FileIdentityProvider::identity(&NativeFiles, path)
    }
}

#[test]
fn reference_waiting_paused_scheduled_drain_cannot_qualify_newer_accepted_version() {
    use std::sync::mpsc;
    let files = Arc::new(PausedFiles::default());
    let fixture = rig_with_files(true, 1914, true, files.clone());
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let original_record = missing_parent(&fixture);
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&original_record), &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(original.entry.reason, InboxReason::Reference);
    cloud.adapter_will_fetch(&fixture.sdk, "scheduled-A".into(), false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    let (reached_tx, reached_rx) = mpsc::channel();
    let (go_tx, go_rx) = mpsc::channel();
    *files.0.lock().unwrap() = Some(Box::new(move || {
        reached_tx.send(()).unwrap();
        go_rx.recv_timeout(Duration::from_secs(10)).unwrap();
    }));
    std::thread::scope(|scope| {
        let a = scope.spawn(|| cloud.adapter_did_fetch(&fixture.sdk, "scheduled-A", false));
        let reached = reached_rx.recv_timeout(Duration::from_secs(10));
        if reached.is_err() {
            let _ = go_tx.send(());
            let _ = a.join();
            panic!("scheduled A must reach its actual file provider");
        }
        let b = (|| {
            cloud.adapter_will_fetch(&fixture.sdk, "scheduled-B".into(), false)?;
            cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false)?;
            cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[advance_server_record(&original_record)], &[])?;
            cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10))?;
            cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false)
        })();
        // Release even if the interleaved production operation returned an error.
        go_tx.send(()).unwrap();
        a.join().unwrap().unwrap();
        b.unwrap();
    });
    let newer = cloud.inbox_entries(binding.id).unwrap().remove(0);
    assert_eq!(newer.entry.reason, InboxReason::Reference);
    assert_ne!(newer.entry.value, original.entry.value);
    assert_eq!(reference_count(&fixture.host), None, "A cannot cover B's arrival");
    cloud.adapter_did_fetch(&fixture.sdk, "scheduled-B", false).unwrap();
    assert_eq!(reference_count(&fixture.host), Some(1));
    assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![newer.clone()]);
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), Some(1));
    assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap(), vec![newer]);
    reopened.close().unwrap();
}

#[test]
fn reference_waiting_damaged_storage_is_refused_before_any_original_file_mutation() {
    use meguri_sync::cloud::{OpenOutcome, TrackingStore, runtime::with_tracking_access};
    for damage in ["missing-key", "identity", "arrival", "watermark", "clock", "missing-stamp", "future", "expiry-trigger"] {
        let fixture = rig();
        let cloud = fixture.host.cloud_runtime().unwrap();
        let db = fixture.host.collection().unwrap();
        cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[missing_parent(&fixture)], &[]).unwrap();
        cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
        cloud.adapter_will_fetch(&fixture.sdk, "accepted".into(), false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, "accepted", false).unwrap();
        assert_eq!(reference_count(&fixture.host), Some(1));
        let scope = fixture.config.cloud.as_ref().unwrap().scope.clone();
        fixture.host.close().unwrap();
        let directory = fixture._root.path().join("collection");
        let c = meguri_storage::rusqlite::Connection::open(directory.join("meguri-cloud.db")).unwrap();
        c.execute_batch("PRAGMA ignore_check_constraints=ON").unwrap();
        match damage {
            "missing-key" => c.execute_batch("DROP TABLE cloud_reference_waiting;
                CREATE TABLE cloud_reference_waiting(binding BLOB NOT NULL REFERENCES bindings(id),
                  inbox INTEGER REFERENCES inbox(id) ON DELETE CASCADE CHECK(inbox>0),
                  arrival INTEGER NOT NULL CHECK(typeof(arrival)='integer' AND arrival>=0),
                  identity BLOB CHECK(identity IS NULL OR (typeof(identity)='blob' AND length(identity)=32)));
                CREATE INDEX cloud_reference_binding ON cloud_reference_waiting(binding)").unwrap(),
            "identity" => { c.execute("UPDATE cloud_reference_waiting SET identity=zeroblob(32)", []).unwrap(); },
            "arrival" => { c.execute("UPDATE cloud_reference_waiting SET arrival=0", []).unwrap(); },
            "watermark" => { c.execute("UPDATE cloud_reference_drains SET through=-1", []).unwrap(); },
            "clock" => { c.execute("UPDATE cloud_reference_schema SET serial=-1", []).unwrap(); },
            "missing-stamp" => { c.execute("DELETE FROM cloud_reference_waiting", []).unwrap(); },
            "future" => { c.execute("UPDATE cloud_reference_schema SET version=2", []).unwrap(); },
            "expiry-trigger" => c.execute_batch("DROP TRIGGER cloud_reference_changed;
                CREATE TRIGGER cloud_reference_changed AFTER UPDATE ON inbox BEGIN SELECT 1; END").unwrap(),
            _ => unreachable!(),
        }
        drop(c);
        let bytes = || {
            ["meguri-cloud.db", "meguri-cloud.db-wal", "meguri-cloud.db-shm"].map(|name| {
                match std::fs::read(directory.join(name)) {
                    Ok(bytes) => Some(bytes),
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
                    Err(e) => panic!("cannot inspect source: {e}"),
                }
            })
        };
        let before = bytes();
        let refused = with_tracking_access(|| match TrackingStore::open(&directory, scope) {
            Ok(OpenOutcome::Damaged) => damage != "future",
            Err(meguri_sync::cloud::Error::RuntimeSchemaUnsupported) => damage == "future",
            _ => false,
        });
        assert!(refused, "{damage}");
        assert_eq!(bytes(), before, "{damage}");
    }
}

#[test]
// This fixture has parked Reference entries and no retained apply/rekey intents.
fn reference_waiting_large_backlog_drain_is_bounded_and_preserves_entries() {
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    let seed = missing_parent(&fixture);
    let mut records = Vec::new();
    for id in 9_100_000..9_101_024 {
        let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&seed.envelope).unwrap() else { unreachable!() };
        let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
        deck.id = id;
        deck.name = format!("Waiting {id} {}", "x".repeat(8192)).into_bytes();
        entity.fields.key = codec::id_key(id).to_vec();
        records.push(Saved {
            name: codec::entity_name(&binding.facts.record_key.0, codec::Kind::Deck, &entity.fields.key).unwrap(),
            envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(),
            system_fields: format!("large-backlog-{id}").into_bytes(),
        });
    }
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &records, &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(original.len(), records.len());
    assert!(original.iter().all(|e| e.entry.reason == InboxReason::Reference));
    assert_eq!(reference_count(&fixture.host), None);
    for group in ["backlog-first", "backlog-unchanged"] {
        cloud.adapter_will_fetch(&fixture.sdk, group.into(), false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
        let started = meguri_engine::monotonic::now();
        cloud.adapter_did_fetch(&fixture.sdk, group, false).unwrap();
        assert!(meguri_engine::monotonic::now().saturating_sub(started) < Duration::from_secs(1), "intent-free drain must not process parked Reference bodies");
        assert_eq!(reference_count(&fixture.host), Some(1024));
        assert_eq!(cloud.inbox_entries(binding.id).unwrap(), original);
    }
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), Some(1024));
    assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap(), original);
    reopened.close().unwrap();
}

#[test]
fn reference_waiting_shared_writer_preserves_exact_repeat_and_stamps_actual_new_intake() {
    use meguri_sync::cloud::{OpenOutcome, TrackingStore, runtime::with_tracking_access};
    let fixture = rig();
    let cloud = fixture.host.cloud_runtime().unwrap();
    let db = fixture.host.collection().unwrap();
    let binding = cloud.status().unwrap().binding.unwrap();
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[missing_parent(&fixture)], &[]).unwrap();
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
    cloud.adapter_will_fetch(&fixture.sdk, "shared-before".into(), false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, "shared-before", false).unwrap();
    assert_eq!(reference_count(&fixture.host), Some(1));
    // Obtain the second entry through another Host using normal record intake.
    let donor = rig();
    let seed = missing_parent(&donor);
    let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&seed.envelope).unwrap() else { unreachable!() };
    let Payload::Deck(mut deck) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else { unreachable!() };
    deck.id = 9_000_002;
    deck.name = b"Shared writer child".to_vec();
    entity.fields.key = codec::id_key(deck.id).to_vec();
    let remote = Saved {
        name: codec::entity_name(&binding.facts.record_key.0, codec::Kind::Deck, &entity.fields.key).unwrap(),
        envelope: codec::Entity::build(entity.fields, &Payload::Deck(deck).encode().unwrap()).unwrap().encode().unwrap(),
        system_fields: b"shared-writer-new-version".to_vec(),
    };
    let donor_cloud = donor.host.cloud_runtime().unwrap();
    let donor_binding = donor_cloud.status().unwrap().binding.unwrap();
    assert!(donor_binding.facts.record_key == binding.facts.record_key);
    donor_cloud.adapter_fetched(&donor.sdk, &donor.zone, &[remote], &[]).unwrap();
    let arrival = donor_cloud.inbox_entries(donor_binding.id).unwrap().remove(0).entry;
    assert_eq!(arrival.reason, InboxReason::Ready);
    donor.host.close().unwrap();
    // Exercise the existing shared-connection storage intake, including its
    // exact-repeat path. No extension SQL is written by this witness.
    with_tracking_access(|| {
        let mut store = match TrackingStore::open(&fixture.config.collection_directory,
            fixture.config.cloud.as_ref().unwrap().scope.clone()).unwrap() {
            OpenOutcome::Opened(store) => store,
            _ => panic!("live shared tracking store required"),
        };
        let cursor = store.engine_state(binding.id).unwrap().unwrap().serialization;
        store.commit_fetched(binding.id, &[original.entry.clone(), arrival.clone()], &cursor, fixture.clock.as_ref()).unwrap();
    });
    cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
    let entries = cloud.inbox_entries(binding.id).unwrap();
    assert_eq!(entries.len(), 2);
    assert!(entries.contains(&original));
    assert!(entries.iter().all(|e| e.entry.reason == InboxReason::Reference));
    assert_eq!(reference_count(&fixture.host), Some(1), "later shared intake cannot inherit the earlier drain");
    cloud.adapter_will_fetch(&fixture.sdk, "shared-after".into(), false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
    cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
    cloud.adapter_did_fetch(&fixture.sdk, "shared-after", false).unwrap();
    assert_eq!(reference_count(&fixture.host), Some(2));
    // Service deletion removes the second entry; the first keeps its coverage.
    cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[], &[arrival.record_name]).unwrap();
    assert_eq!(reference_count(&fixture.host), Some(1));
    let retained = cloud.inbox_entries(binding.id).unwrap();
    assert!(retained.contains(&original));
    fixture.host.close().unwrap();
    let reopened = CoreHost::open(fixture.config.clone()).unwrap();
    assert_eq!(reference_count(&reopened), Some(1));
    assert_eq!(reopened.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap(), retained);
    reopened.close().unwrap();
}

#[test]
fn reference_waiting_manual_return_and_no_state_reply_cannot_inherit_post_boundary_arrival() {
    use meguri_sync::cloud::publication::{ControlRecord, RecordRead};
    for id_read in [false, true] {
        let fixture = rig();
        let cloud = fixture.host.cloud_runtime().unwrap();
        let db = fixture.host.collection().unwrap();
        let binding = cloud.status().unwrap().binding.unwrap();
        let remote = missing_parent(&fixture);
        cloud.adapter_fetched(&fixture.sdk, &fixture.zone, std::slice::from_ref(&remote), &[]).unwrap();
        cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
        let original = cloud.inbox_entries(binding.id).unwrap().remove(0);
        if id_read {
            cloud.adapter_account(Some(&fixture.sdk), IdentityStatus::Available(binding.facts.account.clone())).unwrap();
        }
        let fetch = cloud.adapter_begin_fetch(&fixture.sdk, vec![fixture.zone.clone()]).unwrap();
        let group = format!("meguri-{fetch}");
        cloud.adapter_will_fetch(&fixture.sdk, group.clone(), true).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, &group, true).unwrap();
        let reads = if id_read { cloud.adapter_fetch_reads(&fixture.sdk, fetch, &db).unwrap() } else { Vec::new() };
        assert_eq!(reads.len(), usize::from(id_read));
        cloud.adapter_fetched(&fixture.sdk, &fixture.zone, &[advance_server_record(&remote)], &[]).unwrap();
        cloud.drain_native_incoming(&db, &fixture.sdk, Duration::from_secs(10)).unwrap();
        let newer = cloud.inbox_entries(binding.id).unwrap().remove(0);
        assert_eq!(newer.entry.reason, InboxReason::Reference);
        assert_ne!(newer.entry.value, original.entry.value);
        for read in reads {
            cloud.adapter_complete_fetch_read(&read, &db, RecordRead::Found(ControlRecord {
                encrypted: None, system_fields: OpaqueBytes(b"found-after-later-arrival".to_vec()),
            }), false, None, None).unwrap();
        }
        assert!(cloud.adapter_finish_fetch(&fixture.sdk, fetch).unwrap());
        assert_eq!(reference_count(&fixture.host), None, "the retained boundary predates this version");
        assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![newer.clone()]);
        cloud.adapter_will_fetch(&fixture.sdk, "later-owned-drain".into(), false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), true, false).unwrap();
        cloud.adapter_fetch_zone(&fixture.sdk, fixture.zone.clone(), false, false).unwrap();
        cloud.adapter_did_fetch(&fixture.sdk, "later-owned-drain", false).unwrap();
        assert_eq!(reference_count(&fixture.host), Some(1));
        assert_eq!(cloud.inbox_entries(binding.id).unwrap(), vec![newer]);
        fixture.host.close().unwrap();
    }
}
