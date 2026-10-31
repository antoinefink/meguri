//! Tests FIRST for guarded submission, fresh cleanup reads and ordinary batches.
//! Extract and review this target before applying the production changes.
//! Synthetic inputs; edits use CoreHost and results come from the real Service.
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    control_codec::Active,
    entity_codec as codec,
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        adapter::{Observation, SDKRig},
        coordinator::{Point, PublicationRig},
        devices::{IdentityReads, PreboundDevice},
        *,
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    collections::BTreeMap,
    path::Path,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const NOW: i64 = 1_735_689_600_000;
const DAY: i64 = 86_400_000;
const STEPS: usize = 10_000;

// Platform input, independent of collection-id randomness. Never Debug.
struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let draw = self.0.fetch_add(1, Ordering::SeqCst);
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(17).wrapping_add(draw as u8)
        }))
    }
}
fn keys() -> Arc<Keys> {
    Arc::new(Keys(AtomicUsize::new(0)))
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.submission".into(),
        environment: "development".into(),
        account: "synthetic-account".into(),
    }
}
fn generated(directory: &Path, seed: u64) {
    let recipe = Recipe::parse("recipe: submission\ndescription: Public synthetic submission\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Child\n    notetype: basic\n    new: 2\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, seed, directory).unwrap();
}
fn open(directory: &Path, now: i64, seed: u64, keys: Arc<Keys>) -> PublicationRig {
    let scope = scope();
    PublicationRig::open(
        directory,
        &scope,
        IdentityReads::Available(scope.account.clone()),
        false,
        now,
        seed,
        keys,
    )
    .unwrap()
}
fn publish(rig: &mut PublicationRig, service: &mut Service<SeededRandom>) {
    let preview = rig.preview(service, STEPS).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(service, STEPS).unwrap();
    assert!(rig.facts().unwrap().recovery_ops.is_empty());
}
fn cursor(service: &Service<SeededRandom>) -> u64 {
    service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}
fn record(service: &Service<SeededRandom>, zone: &str, name: &str) -> Record {
    service.inspect(&scope()).unwrap().unwrap().zones[zone].records[name].clone()
}
fn active(service: &Service<SeededRandom>) -> Active {
    let record = record(service, "meguri-root", "active");
    let Field::EncryptedBytes(e) = &record.fields["e"] else {
        panic!("encrypted Active required")
    };
    Active::decode(e).unwrap()
}
fn assert_acks(
    expected: &BTreeMap<String, meguri_sync::cloud::workflow::EntityFacts>,
    actual: &BTreeMap<String, meguri_sync::cloud::workflow::EntityFacts>,
) {
    assert_eq!(expected.len(), actual.len());
    for (name, before) in expected {
        let after = &actual[name];
        assert_eq!(before.acked_rev, after.acked_rev);
        assert_eq!(before.server_erev, after.server_erev);
        assert_eq!(before.acked_system_fields, after.acked_system_fields);
        assert_eq!(before.acked_row_digest, after.acked_row_digest);
        assert_eq!(before.acked_payload_digest, after.acked_payload_digest);
        assert!(before.acked_payload == after.acked_payload);
        assert!(before.acked_envelope == after.acked_envelope);
        assert!(after.in_flight.is_empty());
    }
}

// Each child's first successful Service save follows its parent's successful
// save in a different request. Payloads are decoded independently from receipts.
fn assert_parent_order(service: &Service<SeededRandom>, zone: &str, key: &[u8; 32]) {
    use meguri_model::proto::{
        BlobMessage,
        deck::{DeckKind, DeckKindMessage},
    };
    let mut saved_at = BTreeMap::new();
    let mut payloads = BTreeMap::new();
    for log in service
        .request_log()
        .unwrap()
        .into_iter()
        .filter(|r| r.kind == RequestKind::Modify && r.zone.as_deref() == Some(zone))
    {
        let outcome = service
            .outcomes()
            .unwrap()
            .into_iter()
            .find(|r| r.request_id == log.request_id)
            .unwrap();
        let Ok(ReplyReference::Modified(items)) = outcome.result else {
            continue;
        };
        for item in items {
            let Ok(Some(reference)) = item.result else { continue };
            let record = service.resolve_record(&scope(), &reference).unwrap();
            if record.record_type != "MGEntity" {
                continue;
            }
            let Field::EncryptedBytes(e) = &record.fields["e"] else {
                panic!("entity envelope required")
            };
            let received = codec::receive(key, &record.name, e, codec::AssetInput::Absent).unwrap();
            saved_at.entry(record.name.clone()).or_insert(log.request_id);
            payloads.insert(record.name, received.payload.unwrap());
        }
    }
    let mut checked = 0;
    for (name, payload) in &payloads {
        let mut parents = Vec::new();
        match payload {
            codec::Payload::Note(n) => parents.push((codec::Kind::Notetype, n.mid)),
            codec::Payload::Card(c) => {
                parents.extend([
                    (codec::Kind::Note, c.nid),
                    (codec::Kind::Deck, c.did),
                    (codec::Kind::Deck, c.odid),
                ]);
            }
            codec::Payload::Deck(d) => {
                parents.push((codec::Kind::Deck, d.parent_id));
                if let DeckKind::Normal(n) = DeckKindMessage::decode(&d.kind).unwrap().kind {
                    parents.push((codec::Kind::Preset, n.config_id));
                }
            }
            _ => {}
        }
        for (kind, id) in parents.into_iter().filter(|(_, id)| *id != 0) {
            let parent = codec::entity_name(key, kind, &codec::id_key(id)).unwrap();
            assert!(saved_at[&parent] < saved_at[name]);
            checked += 1;
        }
    }
    assert!(checked > 0);
}

#[test]
fn held_request_close_clean_preserves_submitted_evidence_without_a_second_reply_or_ack() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("publisher");
    generated(&directory, 71);
    let keys = keys();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let mut rig = open(&directory, NOW, 71, keys.clone());
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeActiveSave).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeActiveSave));
    let prepared = rig.facts().unwrap();
    assert!(!prepared.recovery_ops[0].switch_sent);
    let request = rig.held_request().unwrap();
    let submitted = rig.facts().unwrap();
    assert!(submitted.recovery_ops[0].switch_sent);
    assert!(submitted.recovery_ops[0].active_attempt.is_some());
    assert!(
        submitted
            .entities
            .values()
            .all(|e| e.acked_rev == e.local_rev && e.mark.is_none())
    );
    let binding = submitted.current.as_ref().unwrap();
    assert_parent_order(&service, &rig.collection_zone().unwrap(), &binding.facts.record_key.0);
    let response = service.request(&scope(), request.caller, request.operation).unwrap();
    assert!(response.result.is_ok());
    // Keep the actual response in the parent. Close must drain the real CoreHost
    // without consuming or inventing a transport reply for the child.
    rig.close_clean().unwrap();
    let landed = record(&service, "meguri-root", "active");
    let after = cursor(&service);
    let mut reopened = open(&directory, NOW + DAY, 71, keys.clone());
    let durable = reopened.facts().unwrap();
    assert!(durable.recovery_ops == submitted.recovery_ops);
    assert!(durable.current == submitted.current);
    assert_acks(&submitted.entities, &durable.entities);
    assert_eq!(keys.0.load(Ordering::SeqCst), 2);
    reopened.drive_ready(&mut service, STEPS).unwrap();
    assert!(reopened.facts().unwrap().recovery_ops.is_empty());
    assert_eq!(record(&service, "meguri-root", "active"), landed);
    assert!(!service.outcomes().unwrap().iter().any(|r| r.request_id > after
        && matches!(&r.result, Ok(ReplyReference::Modified(items)) if items.iter().any(|i| i.result.is_ok()))));
    assert_acks(&submitted.entities, &reopened.facts().unwrap().entities);
    assert_eq!(keys.0.load(Ordering::SeqCst), 2);
    reopened.close_clean().unwrap();
}

#[test]
fn cleanup_after_offline_hold_reads_the_late_switch_before_deleting_a_never_active_zone() {
    let run = tempfile::tempdir().unwrap();
    let owner_dir = run.path().join("owner");
    let publisher_dir = run.path().join("late-publisher");
    generated(&owner_dir, 71);
    generated(&publisher_dir, 72);
    let owner_keys = keys();
    let publisher_keys = keys();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(73)).unwrap();
    let mut owner = open(&owner_dir, NOW, 71, owner_keys.clone());
    publish(&mut owner, &mut service);
    let owner_active = active(&service);
    owner.close_clean().unwrap();
    let mut publisher = open(&publisher_dir, NOW, 72, publisher_keys.clone());
    let preview = publisher.preview(&mut service, STEPS).unwrap();
    publisher.hold(Point::BeforeEpochRead).unwrap();
    publisher.confirm_publication(&preview).unwrap();
    publisher.drive_ready(&mut service, STEPS).unwrap();
    assert!(publisher.is_held(Point::BeforeEpochRead));
    let target = publisher.collection_zone().unwrap();
    // Reopen runs the actual no-state database listing; no local zone inventory.
    let mut owner = open(&owner_dir, NOW, 71, owner_keys.clone());
    owner.drive_ready(&mut service, STEPS).unwrap();
    assert_eq!(owner.facts().unwrap().zones[&target].first_seen_ms, NOW);
    owner.close_clean().unwrap();
    // This terminated device has no network activity for the retention period.
    let mut owner = open(&owner_dir, NOW + 30 * DAY, 71, owner_keys.clone());
    owner.drive_ready(&mut service, STEPS).unwrap();
    owner.hold(Point::BeforeZoneDeletion).unwrap();
    owner.sync_now_and_drain(&mut service, STEPS).unwrap();
    assert!(owner.is_held(Point::BeforeZoneDeletion));
    assert!(owner.facts().unwrap().zones[&target].deletion_due);
    assert!(active(&service) == owner_active);
    publisher.advance_clock_ms(30 * DAY).unwrap();
    publisher.release(Point::BeforeEpochRead).unwrap();
    publisher.drive_ready(&mut service, STEPS).unwrap();
    assert!(publisher.facts().unwrap().recovery_ops.is_empty());
    let late_active = active(&service);
    assert_ne!(late_active.epoch, owner_active.epoch);
    assert_eq!(late_active.activated_at_ms, NOW + 30 * DAY);
    let after = cursor(&service);
    owner.release(Point::BeforeZoneDeletion).unwrap();
    owner.drive_ready(&mut service, STEPS).unwrap();
    let logs = service.request_log().unwrap();
    assert!(logs.iter().any(|r| r.request_id > after
        && r.kind == RequestKind::Read
        && r.zone.as_deref() == Some("meguri-root")
        && r.outcome == LogOutcome::Success));
    assert!(
        !logs
            .iter()
            .any(|r| r.request_id > after && r.kind == RequestKind::DeleteZone)
    );
    assert!(
        service.inspect(&scope()).unwrap().unwrap().zones[&target]
            .removed
            .is_none()
    );
    assert!(owner.facts().unwrap().zones[&target].deletion_due);
    assert_eq!(owner_keys.0.load(Ordering::SeqCst), 2);
    assert_eq!(publisher_keys.0.load(Ordering::SeqCst), 2);
    owner.close_clean().unwrap();
    publisher.close_clean().unwrap();
}

#[test]
fn ordinary_partial_failure_keeps_a_sibling_saved_ack_and_the_failed_record_pending() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generated(&directory, 71);
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(71)).unwrap();
    let files = PreboundDevice::arrange(&directory, &mut service, 71).unwrap();
    let mut rig = SDKRig::open(&files, true, NOW, 71).unwrap();
    rig.reconcile_and_confirm_root(&mut service, STEPS).unwrap();
    let mut edits = BTreeMap::new();
    for note in files.notes() {
        let parent = codec::entity_name(
            &files.binding.facts.record_key.0,
            codec::Kind::Notetype,
            &codec::id_key(note.mid),
        )
        .unwrap();
        assert!(rig.entity_facts(&parent).unwrap().server_erev > 0);
        let edit = rig
            .driver()
            .edit_note_fields(note.id, &["local revision", "back"])
            .unwrap();
        let name = codec::entity_name(
            &files.binding.facts.record_key.0,
            codec::Kind::Note,
            &codec::id_key(note.id),
        )
        .unwrap();
        edits.insert(name, codec::Payload::Note(edit.note));
    }
    assert_eq!(edits.len(), 2);
    let failed = edits.keys().next().unwrap().clone();
    let success = edits.keys().nth(1).unwrap().clone();
    let baseline =
        service.inspect(&files.scope).unwrap().unwrap().zones[&files.collection_zone].records[&failed].clone();
    let changed = service
        .request(
            &files.scope,
            Caller::default(),
            Operation::Modify {
                zone: files.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput {
                        name: baseline.name.clone(),
                        record_type: baseline.record_type.clone(),
                        fields: baseline.fields.clone(),
                    },
                    if_unchanged: Some(baseline.system_fields.clone()),
                }],
            },
        )
        .unwrap();
    assert!(changed.result.is_ok());
    let batch = rig.drive_until_response(&mut service, STEPS).unwrap();
    assert_eq!(batch.mutations.len(), 2);
    let Ok(Reply::Modified(items)) = &batch.response.result else {
        panic!("actual partial Service result required")
    };
    assert_eq!(
        items
            .iter()
            .find(|i| i.name == failed)
            .unwrap()
            .result
            .as_ref()
            .unwrap_err()
            .code,
        ErrorCode::ServerRecordChanged
    );
    let Ok(MutationSuccess::Saved(saved)) = &items.iter().find(|i| i.name == success).unwrap().result else {
        panic!("sibling must land")
    };
    let receipt = service
        .outcomes()
        .unwrap()
        .into_iter()
        .find(|o| o.request_id == batch.response.request_id)
        .unwrap();
    let Ok(ReplyReference::Modified(receipts)) = receipt.result else {
        panic!("Service receipt required")
    };
    let reference = receipts
        .iter()
        .find(|i| i.name == success)
        .unwrap()
        .result
        .as_ref()
        .unwrap()
        .as_ref()
        .unwrap();
    assert_eq!(&service.resolve_record(&files.scope, reference).unwrap(), saved);
    let log = service
        .request_log()
        .unwrap()
        .into_iter()
        .find(|r| r.request_id == batch.response.request_id)
        .unwrap();
    assert!(
        log.records
            .iter()
            .any(|r| r.name == success && r.outcome == LogOutcome::Success)
    );
    assert!(
        log.records
            .iter()
            .any(|r| r.name == failed && r.outcome == LogOutcome::Failure(ErrorCode::ServerRecordChanged))
    );
    // Conflict application remains unavailable. Its error must not erase a
    // successful sibling's real acknowledgement or acknowledge the failed one.
    assert!(rig.deliver_sent_event(&batch.response).is_err());
    let ack = rig.entity_facts(&success).unwrap();
    assert_eq!(ack.acked_rev, ack.local_rev);
    assert!(ack.mark.is_none());
    assert!(ack.acked_payload.as_ref() == Some(&edits[&success]));
    assert_eq!(ack.acked_system_fields, saved.system_fields.bytes());
    let pending = rig.entity_facts(&failed).unwrap();
    assert_eq!(pending.acked_rev, 1);
    assert_eq!(pending.local_rev, 2);
    assert!(pending.mark.is_some());
    rig.return_sent_handler().unwrap();
    assert!(!rig.sdk_pending().contains_key(&success));
    assert!(rig.tracking_pending_names().unwrap().contains(&failed));
    assert_eq!(rig.observations().iter().filter(|e| matches!(e, Observation::SavedPendingRemoved { name, request_id } if name == &success && *request_id == batch.backend_batch_id)).count(), 1);
    rig.close_clean().unwrap();
    let durable = files.read_entity_durable(&success).unwrap();
    assert!(durable == ack);
    let durable_failed = files.read_entity_durable(&failed).unwrap();
    assert!(durable_failed == pending);
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
