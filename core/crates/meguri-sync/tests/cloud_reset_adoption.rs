//! V2.2/V10.1/V14: observe these tests before extracting the production change.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_engine::{device_state::DeviceState, replace::ReplaceReason};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    BindingState, IsolationReason,
    adoption::{AdoptStep, RecoveryKind},
    entity_codec::{self as codec, Payload},
    publication::PublicationFacts,
    runtime::{KeyByteSource, KeySourceError},
};
use meguri_testkit::cloud::{
    Caller, ErrorCode, Field, LogOutcome, Mutation, Operation, RemovalCause, RequestKind,
    coordinator::{Point, PublicationRig},
    devices::IdentityReads,
};
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::*;

const RETENTION: i64 = 30 * 86_400_000;
const RETAINED: &[u8] = b"synthetic receiver-only retained media";

struct RecoveryKeys(AtomicUsize);
impl KeyByteSource for RecoveryKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let call = self.0.fetch_add(1, Ordering::SeqCst);
        if call >= 2 {
            return Err(KeySourceError::Unavailable);
        }
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(13).wrapping_add(71 + 83 * call as u8)
        }))
    }
}

fn delete_for_reset(pair: &mut Pair, zone: String) {
    assert!(
        pair.service
            .request(
                &scope(),
                Caller::default(),
                Operation::DeleteZone {
                    zone,
                    cause: RemovalCause::EncryptedDataReset,
                }
            )
            .unwrap()
            .result
            .is_ok()
    );
}

fn recovered_pair() -> Pair {
    let mut pair = Pair::published();
    let expired = pair.sender.collection_zone().unwrap();
    // Two ordinary publications establish a genuinely retained prior zone.
    publish(&mut pair.receiver, &mut pair.service);
    let preview = pair.sender.preview(&mut pair.service, STEPS).unwrap();
    pair.sender.adopt_cloud(&preview).unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let active = pair.receiver.collection_zone().unwrap();
    assert_ne!(active, expired);
    assert_eq!(pair.sender.collection_zone().unwrap(), active);

    for rig in [&mut pair.sender, &mut pair.receiver] {
        assert_eq!(rig.facts().unwrap().zones[&expired].first_seen_ms, NOW);
        rig.hold(Point::BeforeZoneDeletion).unwrap();
        rig.advance_clock_ms(RETENTION).unwrap();
        rig.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert!(rig.is_held(Point::BeforeZoneDeletion));
        assert!(rig.facts().unwrap().zones[&expired].deletion_due);
    }
    // Keep MGActive readable until both real cleanup requests are admitted.
    // Service's original ZoneNotFound/reset replies, not constructed errors,
    // establish the reset isolation on each normally bound device.
    delete_for_reset(&mut pair, expired.clone());
    for rig in [&mut pair.sender, &mut pair.receiver] {
        rig.hold(Point::ZoneDeletionReply).unwrap();
        rig.release(Point::BeforeZoneDeletion).unwrap();
        let after = pair.service.request_log().unwrap().last().unwrap().request_id;
        rig.drive_ready(&mut pair.service, STEPS).unwrap();
        assert!(rig.is_held(Point::ZoneDeletionReply));
        let held = rig.held_request().unwrap();
        assert!(matches!(&held.operation, Operation::DeleteZone {
            zone, cause: RemovalCause::Deleted,
        } if zone == &expired));
        let logs = pair.service.request_log().unwrap();
        let replies: Vec<_> = logs
            .iter()
            .filter(|r| r.request_id > after && r.kind == RequestKind::DeleteZone)
            .collect();
        assert_eq!(replies.len(), 1);
        let reply = replies[0];
        assert!(reply.caller == held.caller);
        assert_eq!(reply.outcome, LogOutcome::Failure(ErrorCode::ZoneNotFound));
        let outcomes = pair.service.outcomes().unwrap();
        let failure = outcomes
            .iter()
            .find(|o| o.request_id == reply.request_id)
            .unwrap()
            .result
            .as_ref()
            .unwrap_err();
        assert_eq!(failure.code, ErrorCode::ZoneNotFound);
        assert!(failure.encrypted_data_reset);
        let before = rig.facts().unwrap();
        let rows = rig.driver().collection_rows().unwrap();
        assert_eq!(before.current.as_ref().unwrap().facts.state, BindingState::Active);
        rig.release(Point::ZoneDeletionReply).unwrap();
        rig.drive_ready(&mut pair.service, STEPS).unwrap();
        let isolated = rig.facts().unwrap();
        assert_eq!(
            isolated.current.as_ref().unwrap().id,
            before.current.as_ref().unwrap().id
        );
        assert_eq!(
            isolated.current.as_ref().unwrap().facts.state,
            BindingState::Isolated(IsolationReason::EncryptedDataReset)
        );
        assert!(isolated.decision == before.decision);
        assert_eq!(isolated.native_file, before.native_file);
        assert_eq!(rig.driver().collection_rows().unwrap(), rows);
        assert_eq!(
            pair.service
                .request_log()
                .unwrap()
                .iter()
                .filter(|r| r.request_id > after && r.kind == RequestKind::DeleteZone)
                .count(),
            1
        );
    }
    let zones: Vec<_> = pair
        .service
        .inspect(&scope())
        .unwrap()
        .unwrap()
        .zones
        .into_iter()
        .filter_map(|(name, zone)| zone.removed.is_none().then_some(name))
        .collect();
    for zone in zones {
        delete_for_reset(&mut pair, zone);
    }
    assert!(
        pair.service
            .inspect(&scope())
            .unwrap()
            .unwrap()
            .zones
            .values()
            .all(|zone| zone.removed == Some(RemovalCause::EncryptedDataReset) && zone.records.is_empty())
    );

    // Edits and physical media committed during isolation remain local until
    // the normal successful replacement; the observer cannot resume the binding.
    let id = notes(&mut pair.receiver)[0].id;
    pair.receiver
        .driver()
        .edit_note_fields(id, &["isolated local front", "isolated local back"])
        .unwrap();
    pair.receiver.driver().put_media("photo.jpg", LOCAL).unwrap();
    pair.receiver.driver().put_media("retained.bin", RETAINED).unwrap();
    let isolated = pair.receiver.facts().unwrap();
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.facts().unwrap().current == isolated.current);
    assert!(pair.receiver.facts().unwrap().bindings == isolated.bindings);

    // Use the existing missing-root publication path with fresh platform keys
    // and a distinct process random stream, never a seeded binding or ACK.
    pair.sender.close_clean().unwrap();
    pair.sender = PublicationRig::open(
        &pair.root.path().join("sender"),
        &scope(),
        IdentityReads::Available(scope().account),
        false,
        NOW + RETENTION,
        821,
        Arc::new(RecoveryKeys(AtomicUsize::new(0))),
    )
    .unwrap();
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    let id = notes(&mut pair.sender)[0].id;
    pair.sender
        .driver()
        .edit_note_fields(id, &["<img src=\"photo.jpg\"> recovered front", "recovered back"])
        .unwrap();
    publish(&mut pair.sender, &mut pair.service);
    assert_ne!(pair.sender.collection_zone().unwrap(), active);
    assert!(pair.receiver.facts().unwrap().current == isolated.current);
    pair
}

fn assert_prior_intact(pair: &mut Pair, before: &PublicationFacts, rows: &[Payload]) {
    let now = pair.receiver.facts().unwrap();
    assert!(now.current == before.current);
    assert!(now.bindings == before.bindings);
    assert!(now.entities == before.entities);
    assert!(now.media_key == before.media_key);
    assert_eq!(now.root_acked, before.root_acked);
    assert!(now.decision == before.decision);
    assert_eq!(now.native_file, before.native_file);
    assert_eq!(
        now.current.as_ref().unwrap().facts.state,
        BindingState::Isolated(IsolationReason::EncryptedDataReset)
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files["photo.jpg"], LOCAL);
    assert_eq!(files["retained.bin"], RETAINED);
}

#[test]
fn encrypted_reset_fresh_adoption_replaces_rows_retains_media_and_reopens_acked_and_reviewable() {
    let mut pair = recovered_pair();
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let remote = pair.sender.driver().collection_rows().unwrap();
    assert_ne!(local, remote);
    let source = pair.sender.facts().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    assert_prior_intact(&mut pair, &before, &local);
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionStagingRecorded));
    let staged = pair.receiver.facts().unwrap();
    assert!(matches!(&staged.recovery_ops[0].kind,
        RecoveryKind::Adopt(op) if op.step == AdoptStep::Staging));
    assert_prior_intact(&mut pair, &before, &local);
    pair.receiver.release(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.facts().unwrap();
    let binding = adopted.current.as_ref().unwrap();
    assert_eq!(binding.facts.state, BindingState::Active);
    assert_ne!(binding.id, before.current.as_ref().unwrap().id);
    assert_eq!(binding.facts.epoch, source.current.as_ref().unwrap().facts.epoch);
    assert!(binding.facts.record_key == source.current.as_ref().unwrap().facts.record_key);
    assert!(adopted.media_key == source.media_key);
    assert!(adopted.root_acked && adopted.recovery_ops.is_empty());
    assert!(
        adopted
            .bindings
            .iter()
            .all(|b| b.id != before.current.as_ref().unwrap().id
                || b.facts.state == BindingState::Isolated(IsolationReason::EncryptedDataReset))
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files["photo.jpg"], REMOTE);
    assert_eq!(files["retained.bin"], RETAINED);

    pair.receiver.close_clean().unwrap();
    let evidence = DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
        .replacement_evidence_for(before.decision.publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert!(evidence.candidate == adopted.decision.publication);
    let backup = evidence.safety_backup.unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let reopened = pair.receiver.facts().unwrap();
    assert!(reopened.current.as_ref().unwrap().facts == binding.facts);
    assert!(reopened.decision == adopted.decision);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        local
    );
    let database = pair.service.inspect(&scope()).unwrap().unwrap();
    let Field::EncryptedBytes(active_bytes) = &database.zones["meguri-root"].records["active"].fields["e"] else {
        panic!("Service Active envelope")
    };
    let active = meguri_sync::cloud::control_codec::Active::decode(active_bytes).unwrap();
    assert_eq!(active.epoch, binding.facts.epoch.0);
    let zone = &database.zones[&pair.sender.collection_zone().unwrap()];
    let key = &binding.facts.record_key.0;
    let mut acked_rows = remote.clone();
    acked_rows.extend(pair.sender.driver().media_rows().unwrap());
    for row in acked_rows {
        let name = codec::entity_name(
            key,
            row.kind(),
            &meguri_sync::cloud::row_payload::payload_key(&row).unwrap(),
        )
        .unwrap();
        let ack = &reopened.entities[&name];
        let record = &zone.records[&name];
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("Service envelope")
        };
        let received = codec::receive(key, &name, envelope, codec::AssetInput::Absent).unwrap();
        assert_eq!(received.payload.as_ref(), Some(&row));
        assert_eq!(ack.payload, row);
        assert_eq!(ack.acked_payload.as_ref(), Some(&row));
        assert_eq!(ack.acked_envelope.0, *envelope);
        assert_eq!(ack.acked_system_fields, record.system_fields.bytes());
        assert_eq!(ack.server_erev, received.entity.fields.erev);
        assert!(ack.acked_rev > 0 && ack.local_rev == ack.acked_rev);
        assert!(ack.mark.is_none() && ack.in_flight.is_empty());
    }
    let server = pair.service.inspect(&scope()).unwrap().unwrap().zones;
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.service.inspect(&scope()).unwrap().unwrap().zones, server);
    let id = notes(&mut pair.receiver)[0].id;
    let session = pair.receiver.driver().review_begin(id).unwrap();
    let card = pair.receiver.driver().review_start(session).unwrap();
    assert_eq!(card.nid, id);
    assert!(remote.contains(&Payload::Card(card.clone())));
    pair.receiver.driver().review_show_answer(session, card.id).unwrap();
    pair.receiver.driver().review_answer_good(session, card.id).unwrap();
    pair.receiver.driver().review_end_and_drain(session).unwrap();
    pair.close();

    // Inspect and restore the real media trash only after its host has closed.
    // The superseded local bytes survive relaunch and can actually be recovered.
    let media = meguri_storage::media::MediaStore::open(
        &pair.receiver_dir.join("collection.anki2"),
        Arc::new(FixedClock::new(NOW + RETENTION, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(822)),
    )
    .unwrap();
    assert!(
        media
            .trash_entries()
            .unwrap()
            .iter()
            .any(|entry| entry.name == "photo.jpg" && entry.size == LOCAL.len() as u64)
    );
    media.restore().unwrap();
    assert_eq!(std::fs::read(media.media_dir().join(hash_name())).unwrap(), LOCAL);
    assert_eq!(std::fs::read(media.media_dir().join("photo.jpg")).unwrap(), REMOTE);
    assert_eq!(std::fs::read(media.media_dir().join("retained.bin")).unwrap(), RETAINED);
}

#[test]
fn encrypted_reset_cancelled_adoption_keeps_prior_collection_and_isolation_after_reopen() {
    let mut pair = recovered_pair();
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let stale = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.cancel_preview().unwrap();
    assert!(pair.receiver.adopt_cloud(&stale).is_err());
    assert_prior_intact(&mut pair, &before, &local);
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    let op = pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionStagingRecorded));
    assert_prior_intact(&mut pair, &before, &local);
    pair.receiver.cancel_recovery_op(op).unwrap();
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.facts().unwrap().recovery_ops.is_empty());
    assert_prior_intact(&mut pair, &before, &local);
    pair.close();
}

#[test]
fn encrypted_reset_malformed_candidate_keeps_prior_collection_and_isolated_binding() {
    let mut pair = recovered_pair();
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let source = pair.sender.facts().unwrap();
    let name = source
        .entities
        .values()
        .find(|entity| entity.kind == codec::Kind::Notetype)
        .unwrap()
        .name
        .clone();
    let zone = pair.sender.collection_zone().unwrap();
    assert!(
        pair.service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone: zone.clone(),
                    atomic: false,
                    changes: vec![Mutation::Delete { name }],
                }
            )
            .unwrap()
            .result
            .is_ok()
    );
    let after = pair.service.request_log().unwrap().last().unwrap().request_id;
    // Either V13 prefetch or V14's fetched reference validation may reject it.
    if let Ok(preview) = pair.receiver.preview(&mut pair.service, STEPS) {
        pair.receiver.adopt_cloud(&preview).unwrap();
        assert!(pair.receiver.drive_ready(&mut pair.service, STEPS).is_err());
    }
    assert!(pair.service.request_log().unwrap().iter().any(|r| r.request_id > after
        && r.kind == RequestKind::FetchZone
        && r.outcome == LogOutcome::Success
        && r.zone.as_deref() == Some(zone.as_str())));
    assert!(pair.receiver.facts().unwrap().recovery_ops.is_empty());
    assert_prior_intact(&mut pair, &before, &local);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_prior_intact(&mut pair, &before, &local);
    pair.close();
}

#[test]
fn encrypted_reset_adoption_restart_during_staging_preserves_prior_isolation() {
    let mut pair = recovered_pair();
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionStagingRecorded));
    assert_prior_intact(&mut pair, &before, &local);
    pair.receiver.kill().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.facts().unwrap().recovery_ops.is_empty());
    assert_prior_intact(&mut pair, &before, &local);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
