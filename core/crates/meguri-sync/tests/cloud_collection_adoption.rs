//! Focused tests FIRST: root must extract/observe/commit this file before production.
#[path = "support/publication_incoming.rs"]
mod support;
use meguri_engine::{device_state::DeviceState, replace::ReplaceReason};
use meguri_sync::cloud::adoption::{AdoptStep, RecoveryKind};
use meguri_sync::cloud::entity_codec::{self as codec, Payload};
use meguri_testkit::cloud::{Field, RequestKind, coordinator::Point};
use support::*;

#[test]
fn fetched_empty_media_collection_replaces_local_and_reopens_with_its_safety_package() {
    collection_adoption(false, false);
}

/// K3/K7/K18.4/K20, V14: ordinary adapter pa bytes survive first adoption.
#[test]
fn fetched_ordinary_note_payload_asset_replaces_local_and_reopens_acked_and_reviewable() {
    collection_adoption(true, false);
}

#[test]
fn metadata_first_ordinary_payload_adoption_reopens_acked_and_reviewable() {
    collection_adoption(true, true);
}

fn collection_adoption(with_payload_asset: bool, metadata_first: bool) {
    let mut pair = Pair::published();
    let large_note = with_payload_asset.then(|| {
        let id = notes(&mut pair.sender)[0].id;
        let large_answer = "ordinary adoption payload ".repeat(44_000);
        pair.sender
            .driver()
            .edit_note_fields(id, &["ordinary adoption front", &large_answer])
            .unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        id
    });
    // Key provider refuses every draw, including after relaunch.
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    if metadata_first {
        pair.receiver.use_metadata_first_adoption();
    }
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let remote = pair.sender.driver().collection_rows().unwrap();
    let source = pair.sender.facts().unwrap();
    let source_header = pair.sender.driver().collection_header().unwrap();
    let payload_record = large_note.map(|id| {
        let payload = remote
            .iter()
            .find(|row| matches!(row, Payload::Note(note) if note.id == id))
            .unwrap()
            .clone();
        let key = &source.current.as_ref().unwrap().facts.record_key;
        let name = codec::entity_name(&key.0, codec::Kind::Note, &codec::id_key(id)).unwrap();
        let zone = pair.sender.collection_zone().unwrap();
        let record = pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone].records[&name].clone();
        let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
            panic!("Service envelope")
        };
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(bytes).unwrap() else {
            panic!("supported Service envelope")
        };
        assert!(entity.payload.is_none());
        let Field::Asset(asset) = &record.fields["pa"] else {
            panic!("ordinary adapter must save pa")
        };
        assert!(asset.size() > codec::INLINE_ENVELOPE_BYTES as u64);
        assert_eq!(asset.bytes(), payload.encode().unwrap());
        assert_eq!(codec::sha256(asset.bytes()), entity.payload_sha256);
        (record, entity, payload)
    });
    assert_ne!(local, remote);
    assert!(pair.receiver.driver().media_files().unwrap().is_empty());
    assert!(pair.sender.driver().media_files().unwrap().is_empty());
    let remote_zone = pair.sender.collection_zone().unwrap();
    let after = pair.service.request_log().unwrap().last().unwrap().request_id;
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    assert!(pair.receiver.facts().unwrap().decision == before.decision);
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let staging = pair.receiver.facts().unwrap();
    assert!(matches!(&staging.recovery_ops[0].kind, RecoveryKind::Adopt(op) if op.step == AdoptStep::Staging));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    pair.receiver.release(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.facts().unwrap();
    assert!(adopted.recovery_ops.is_empty());
    let bound = adopted.current.as_ref().unwrap();
    let remote_binding = source.current.as_ref().unwrap();
    assert_eq!(bound.facts.epoch, remote_binding.facts.epoch);
    assert!(bound.facts.record_key == remote_binding.facts.record_key);
    assert!(adopted.media_key == source.media_key);
    assert_ne!(
        adopted.decision.publication.identity,
        before.decision.publication.identity
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    assert_eq!(
        pair.receiver.driver().collection_header().unwrap().crt,
        source_header.crt
    );
    assert!(pair.service.request_log().unwrap().iter().any(|r| r.request_id > after
        && r.kind == RequestKind::FetchZone
        && r.zone.as_deref() == Some(remote_zone.as_str())));
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
    assert!(reopened.decision == adopted.decision);
    assert!(reopened.current.as_ref().unwrap().facts == bound.facts);
    assert_eq!(reopened.native_file, adopted.native_file);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        local
    );
    if let Some((record, entity, payload)) = payload_record {
        let ack = &reopened.entities[&record.name];
        assert!(ack.mark.is_none() && ack.in_flight.is_empty());
        assert!(ack.acked_rev > 0);
        assert_eq!(ack.local_rev, ack.acked_rev);
        assert_eq!(ack.server_erev, entity.fields.erev);
        assert!(ack.payload == payload);
        assert!(ack.acked_payload.as_ref() == Some(&payload));
        assert_eq!(ack.acked_row_digest, payload.row_digest().unwrap());
        assert_eq!(ack.acked_payload_digest, entity.payload_sha256);
        let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
            unreachable!()
        };
        assert_eq!(ack.acked_envelope.0, *bytes);
        assert_eq!(ack.acked_system_fields, record.system_fields.bytes());
        let id = large_note.unwrap();
        let session = pair.receiver.driver().review_begin(id).unwrap();
        let question = pair.receiver.driver().review_start(session).unwrap();
        assert_eq!(question.nid, id);
        assert!(remote.contains(&Payload::Card(question.clone())));
        pair.receiver.driver().review_show_answer(session, question.id).unwrap();
        pair.receiver.driver().review_end_and_drain(session).unwrap();
    }
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
