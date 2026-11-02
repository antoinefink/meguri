//! Q11 / sync/policy P24: K15.9 extensionless naming and ACKed entity reuse.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    ConflictReason,
    entity_codec::{self as codec, Payload},
    workflow::{EntityFacts, Observation},
};
use meguri_testkit::cloud::{
    Field, RecordChange, Service,
    coordinator::{Point, PublicationRig},
};
use sha1::{Digest, Sha1};
use support::*;

fn media(name: &str, bytes: &[u8]) -> Payload {
    Payload::MediaName(codec::MediaName {
        name: name.as_bytes().to_vec(),
        sha256: codec::sha256(bytes),
        sha1: Sha1::digest(bytes).into(),
        size: bytes.len() as u64,
    })
}

fn tracked(rig: &PublicationRig, name: &str) -> EntityFacts {
    let facts = rig.facts().unwrap();
    let case_key = name.to_lowercase();
    let matches: Vec<_> = facts
        .entities
        .values()
        .filter(|entity| entity.kind == codec::Kind::MediaName && entity.key == case_key.as_bytes())
        .collect();
    assert_eq!(matches.len(), 1);
    let entity = matches[0];
    assert_eq!(
        entity.name,
        codec::entity_name(
            &facts.current.as_ref().unwrap().facts.record_key.0,
            codec::Kind::MediaName,
            case_key.as_bytes(),
        )
        .unwrap()
    );
    assert!(entity.tombstone_at.is_none());
    entity.clone()
}

#[track_caller]
fn acked(rig: &PublicationRig, service: &Service<SeededRandom>, payload: &Payload) -> EntityFacts {
    let facts = rig.facts().unwrap();
    let key = facts.current.as_ref().unwrap().facts.record_key.0;
    let name = codec::entity_name(
        &key,
        payload.kind(),
        &meguri_sync::cloud::row_payload::payload_key(payload).unwrap(),
    )
    .unwrap();
    let entity = &facts.entities[&name];
    assert_eq!(&entity.payload, payload);
    assert_eq!(entity.acked_payload.as_ref(), Some(payload));
    assert!(entity.tombstone_at.is_none());
    assert!(entity.acked_rev > 0);
    assert_eq!(entity.local_rev, entity.acked_rev);
    assert!(entity.mark.is_none());
    assert!(entity.in_flight.is_empty());
    let database = service.inspect(&scope()).unwrap().unwrap();
    let zone = &database.zones[&rig.collection_zone().unwrap()];
    let record = &zone.records[&name];
    let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
        panic!("expected encrypted Service entity")
    };
    let received = codec::receive(&key, &name, envelope, codec::AssetInput::Absent).unwrap();
    assert_eq!(received.payload.as_ref(), Some(payload));
    assert!(!received.entity.fields.tombstone);
    assert_eq!(received.entity.fields.origin, entity.origin);
    assert_eq!(received.entity.fields.erev, entity.server_erev);
    assert_eq!(entity.acked_envelope.0, *envelope);
    assert_eq!(entity.acked_system_fields, record.system_fields.bytes());
    assert_eq!(entity.acked_payload_digest, payload.payload_digest().unwrap());
    assert_eq!(entity.acked_row_digest, payload.row_digest().unwrap());
    assert!(zone.changes.iter().any(|change| matches!(
        &change.change,
        RecordChange::Saved(saved) if saved.name == name
            && saved.system_fields.bytes() == record.system_fields.bytes()
            && saved.fields.get("e") == Some(&Field::EncryptedBytes(envelope.clone()))
    )));
    entity.clone()
}

fn saved_versions(pair: &Pair, name: &str) -> usize {
    let database = pair.service.inspect(&scope()).unwrap().unwrap();
    let zone = &database.zones[&pair.receiver.collection_zone().unwrap()];
    zone.changes
        .iter()
        .filter(|change| match &change.change {
            RecordChange::Saved(record) => record.name == name,
            RecordChange::Deleted { name: deleted, .. } => {
                assert_ne!(deleted, name);
                false
            }
        })
        .count()
}

fn both_created(pair: &mut Pair, old: &str, moved: &str, extra_reference: bool) {
    create_both(pair, old, moved, extra_reference);
    hold_before_move(pair, old, moved, extra_reference);
}

fn create_both(pair: &mut Pair, old: &str, moved: &str, extra_reference: bool) {
    assert_ne!(LOCAL, REMOTE);
    for rig in [&mut pair.receiver, &mut pair.sender] {
        assert!(!rig.driver().media_files().unwrap().contains_key(old));
        assert!(
            !rig.facts()
                .unwrap()
                .entities
                .values()
                .any(|entity| { entity.kind == codec::Kind::MediaName && entity.key == old.as_bytes() })
        );
    }
    let extra = if extra_reference {
        format!("<img src=\"{moved}\">")
    } else {
        String::new()
    };
    let fields = format!("<img src=\"{old}\">{extra}");
    let before = notes(&mut pair.receiver);
    assert!(before.len() >= 2);
    for note in before {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&fields, "local back"])
            .unwrap();
    }
    assert_eq!(pair.receiver.driver().put_media(old, LOCAL).unwrap(), old);
    assert_eq!(pair.sender.driver().put_media(old, REMOTE).unwrap(), old);
    for (rig, bytes) in [(&mut pair.receiver, LOCAL), (&mut pair.sender, REMOTE)] {
        assert_eq!(rig.driver().media_files().unwrap()[old], bytes);
        let entity = tracked(rig, old);
        assert_eq!(entity.payload, media(old, bytes));
        assert_eq!(entity.acked_rev, 0);
        assert_eq!(entity.server_erev, 0);
        assert!(entity.acked_envelope.0.is_empty());
        assert!(entity.acked_payload.is_none());
        assert!(entity.base_payload.is_none());
        assert!(entity.mark.is_some());
    }
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let remote = acked(&pair.sender, &pair.service, &media(old, REMOTE));
    assert_ne!(remote.origin, tracked(&pair.receiver, old).origin);
}

fn hold_before_move(pair: &mut Pair, old: &str, moved: &str, extra_reference: bool) {
    pair.receiver.hold(Point::ResolutionBeforeMove).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_before_move(pair, old, moved, extra_reference);
}

fn assert_before_move(pair: &mut Pair, old: &str, moved: &str, extra_reference: bool) {
    let extra = if extra_reference {
        format!("<img src=\"{moved}\">")
    } else {
        String::new()
    };
    let fields = format!("<img src=\"{old}\">{extra}");
    assert!(pair.receiver.is_held(Point::ResolutionBeforeMove));
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    let intents = pair.receiver.driver().apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    let apply = intents[0].media.as_ref().unwrap();
    assert_eq!(apply.old_name, old);
    assert_eq!(apply.before.clone().map(Payload::MediaName), Some(media(old, LOCAL)));
    assert_eq!(apply.server.as_ref().unwrap().sha256, codec::sha256(REMOTE));
    assert_eq!(apply.moved.clone().map(Payload::MediaName), Some(media(moved, LOCAL)));
    assert_eq!(std::fs::read(apply.staged.as_ref().unwrap()).unwrap(), REMOTE);
    assert_eq!(pair.receiver.driver().media_files().unwrap()[old], LOCAL);
    for note in notes(&mut pair.receiver) {
        assert_eq!(note.flds, format!("{fields}\x1flocal back").as_bytes());
    }
}

fn converge(pair: &mut Pair, old: &str, moved: &str, extra_reference: bool) {
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ResolutionBeforeMove).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    let settled = tracked(&pair.receiver, moved);
    assert_eq!(settled.payload, media(moved, LOCAL));
    if extra_reference {
        acked(&pair.receiver, &pair.service, &media(moved, LOCAL));
    } else {
        // Observe the actual new entity before its first Service save and ACK.
        assert_eq!(settled.acked_rev, 0);
        assert!(settled.acked_envelope.0.is_empty());
        assert!(settled.mark.is_some());
        assert_eq!(saved_versions(pair, &settled.name), 0);
    }
    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    for _ in 0..2 {
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    }
    let expected = notes(&mut pair.receiver);
    assert!(expected.len() >= 2);
    assert_eq!(notes(&mut pair.sender), expected);
    let front = format!("<img src=\"{moved}\">");
    let fields = format!("{front}{}\x1flocal back", if extra_reference { &front } else { "" });
    for note in expected {
        assert_eq!(note.flds, fields.as_bytes());
        for rig in [&pair.receiver, &pair.sender] {
            acked(rig, &pair.service, &Payload::Note(note.clone()));
        }
    }
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files[old], REMOTE);
    assert_eq!(files[moved], LOCAL);
    assert_eq!(pair.sender.driver().media_files().unwrap(), files);
    let rows = pair.receiver.driver().media_rows().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&media(old, REMOTE)));
    assert!(rows.contains(&media(moved, LOCAL)));
    assert_eq!(pair.sender.driver().media_rows().unwrap(), rows);
    for rig in [&mut pair.receiver, &mut pair.sender] {
        let binding = rig.facts().unwrap().current.unwrap().id;
        assert!(rig.driver().apply_intents(binding).unwrap().is_empty());
        assert!(rig.driver().inbox_entries(binding).unwrap().is_empty());
        assert_eq!(
            rig.facts()
                .unwrap()
                .entities
                .values()
                .filter(|entity| entity.kind == codec::Kind::MediaName)
                .count(),
            2
        );
        for (name, bytes) in [(old, REMOTE), (moved, LOCAL)] {
            tracked(rig, name);
            acked(rig, &pair.service, &media(name, bytes));
        }
    }
    let copies = pair.receiver.driver().media_conflicts().unwrap();
    let copies: Vec<_> = copies
        .iter()
        .filter(|copy| copy.reason == ConflictReason::MediaContent)
        .collect();
    assert_eq!(copies.len(), 1);
    let path = std::path::Path::new(copies[0].staged_path.as_ref().unwrap());
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), moved);
    assert_eq!(std::fs::read(path).unwrap(), LOCAL);
}

#[test]
fn extensionless_both_created_conflict_tracks_and_acks_trimmed_hash_name() {
    let mut pair = Pair::adopted();
    let moved = format!("readme-{:x}", Sha1::digest(LOCAL));
    assert!(!moved.ends_with('.') && !moved.ends_with(' '));
    both_created(&mut pair, "readme", &moved, false);
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key(&moved));
    assert!(
        !pair
            .receiver
            .facts()
            .unwrap()
            .entities
            .values()
            .any(|entity| { entity.kind == codec::Kind::MediaName && entity.key == moved.as_bytes() })
    );
    converge(&mut pair, "readme", &moved, false);
    let entity = tracked(&pair.receiver, &moved);
    assert_eq!(saved_versions(&pair, &entity.name), 1);
    pair.close();
}

#[test]
fn both_created_conflict_reuses_preexisting_live_acked_hash_entity() {
    reuses_preexisting_live_acked_hash_entity(&hash_name());
}

#[test]
fn both_created_conflict_reuses_preexisting_live_acked_case_variant_hash_entity() {
    reuses_preexisting_live_acked_hash_entity(&hash_name().to_uppercase());
}

fn reuses_preexisting_live_acked_hash_entity(moved: &str) {
    let mut pair = Pair::adopted();
    assert_eq!(pair.receiver.driver().put_media(moved, LOCAL).unwrap(), moved);
    let pending = tracked(&pair.receiver, moved);
    assert_eq!(pending.acked_rev, 0);
    assert!(pending.acked_envelope.0.is_empty());
    let front = format!("<img src=\"{moved}\">");
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&front, "local back"])
            .unwrap();
    }
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    // Capture identity and the actual Service-backed ACK before creating photo.jpg.
    let baseline = acked(&pair.receiver, &pair.service, &media(moved, LOCAL));
    assert_eq!(baseline.name, pending.name);
    assert_eq!(baseline.origin, pending.origin);
    assert_eq!(pair.receiver.driver().media_files().unwrap()[moved], LOCAL);
    assert_eq!(pair.sender.driver().media_files().unwrap()[moved], LOCAL);
    for note in notes(&mut pair.receiver) {
        assert_eq!(note.flds, format!("{front}\x1flocal back").as_bytes());
        acked(&pair.receiver, &pair.service, &Payload::Note(note));
    }
    let versions = saved_versions(&pair, &baseline.name);
    assert_eq!(versions, 1);

    both_created(&mut pair, "photo.jpg", moved, true);
    let held = tracked(&pair.receiver, moved);
    assert!(held == baseline);
    converge(&mut pair, "photo.jpg", moved, true);
    let after = acked(&pair.receiver, &pair.service, &media(moved, LOCAL));
    assert_eq!(after.name, baseline.name);
    assert_eq!(after.origin, baseline.origin);
    assert_eq!(after.local_rev, baseline.local_rev);
    assert_eq!(after.acked_rev, baseline.acked_rev);
    assert_eq!(after.server_erev, baseline.server_erev);
    assert_eq!(after.acked_envelope.0, baseline.acked_envelope.0);
    assert_eq!(after.acked_system_fields, baseline.acked_system_fields);
    assert_eq!(saved_versions(&pair, &baseline.name), versions);
    pair.close();
}

// Fetch through the SDK first. The destination is still unchanged while normal
// scheduling reaches the real download reply, so it cannot be saved early.
// BeforeTracking owns durable staged bytes, unlike the earlier BeforeApply
// latch whose parent transport file need not survive a paused callback.
fn fetch_to_staged_download(pair: &mut Pair, baseline: &EntityFacts) {
    pair.receiver.hold(Point::MediaReplyBeforeTracking).unwrap();
    pair.receiver.fetch_without_sending(&mut pair.service, STEPS).unwrap();
    assert_retained_ack(&tracked(&pair.receiver, &hash_name()), baseline);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::MediaReplyBeforeTracking));
    assert_retained_ack(&tracked(&pair.receiver, &hash_name()), baseline);
    assert_eq!(saved_versions(pair, &baseline.name), 1);
}

fn resume_download_to_move(pair: &mut Pair, moved: &str) {
    pair.receiver.hold(Point::ResolutionBeforeMove).unwrap();
    pair.receiver.release(Point::MediaReplyBeforeTracking).unwrap();
    // A paused original callback returns an error; the exact latch and real
    // intent below, rather than that error, prove which phase was reached.
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert_before_move(pair, "photo.jpg", moved, true);
}

fn assert_retained_ack(entity: &EntityFacts, baseline: &EntityFacts) {
    assert_eq!(entity.name, baseline.name);
    assert_eq!(entity.kind, baseline.kind);
    assert_eq!(entity.key, baseline.key);
    assert_eq!(entity.origin, baseline.origin);
    assert_eq!(entity.sent, baseline.sent);
    assert_eq!(Some(&entity.payload), baseline.acked_payload.as_ref());
    assert_eq!(entity.acked_payload, baseline.acked_payload);
    assert_eq!(entity.local_rev, baseline.local_rev);
    assert_eq!(entity.acked_rev, baseline.acked_rev);
    assert_eq!(entity.server_erev, baseline.server_erev);
    assert_eq!(entity.acked_envelope.0, baseline.acked_envelope.0);
    assert_eq!(entity.acked_system_fields, baseline.acked_system_fields);
    assert_eq!(entity.acked_payload_digest, baseline.acked_payload_digest);
    assert_eq!(entity.acked_row_digest, baseline.acked_row_digest);
    assert!(entity.tombstone_at.is_none());
    assert!(entity.mark.is_none());
    assert!(entity.base_payload.is_none());
    assert!(entity.in_flight.is_empty());
}

fn assert_no_destination_batch(rig: &PublicationRig, start: usize, name: &str) {
    assert!(!rig.sdk_pending().contains_key(name));
    for observation in &rig.observations()[start..] {
        if let Observation::BatchBuilt { names, .. } = observation {
            assert!(!names.contains(name));
        }
    }
}

fn files_by_case(rig: &mut PublicationRig) -> std::collections::BTreeMap<String, Vec<u8>> {
    let mut result = std::collections::BTreeMap::new();
    for (name, bytes) in rig.driver().media_files().unwrap() {
        assert!(result.insert(name.to_lowercase(), bytes).is_none());
    }
    result
}

#[test]
fn both_created_conflict_keeps_canonical_ack_after_case_only_rename_and_reopen() {
    let mut pair = Pair::adopted();
    let physical = hash_name();
    let canonical = physical.to_uppercase();
    assert_ne!(physical, canonical);
    assert_eq!(physical.to_lowercase(), canonical.to_lowercase());
    assert_eq!(pair.receiver.driver().put_media(&canonical, LOCAL).unwrap(), canonical);
    let front = format!("<img src=\"{canonical}\">");
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&front, "local back"])
            .unwrap();
    }
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let baseline = acked(&pair.receiver, &pair.service, &media(&canonical, LOCAL));
    acked(&pair.sender, &pair.service, &media(&canonical, LOCAL));
    let versions = saved_versions(&pair, &baseline.name);
    assert_eq!(versions, 1);
    let observations = pair.receiver.observations().len();

    create_both(&mut pair, "photo.jpg", &physical, true);
    fetch_to_staged_download(&mut pair, &baseline);
    let directory = pair.receiver_dir.join("collection.media");
    std::fs::rename(directory.join(&canonical), directory.join(&physical)).unwrap();
    resume_download_to_move(&mut pair, &physical);
    let held = tracked(&pair.receiver, &physical);
    assert_retained_ack(&held, &baseline);
    assert_eq!(saved_versions(&pair, &baseline.name), versions);

    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ResolutionBeforeMove).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert_retained_ack(&tracked(&pair.receiver, &physical), &baseline);
    let rows = pair.receiver.driver().media_rows().unwrap();
    assert_eq!(rows.len(), 2);
    assert!(rows.contains(&media(&physical, LOCAL)));
    assert!(rows.contains(&media("photo.jpg", REMOTE)));
    assert!(!rows.contains(&media(&canonical, LOCAL)));
    assert_eq!(pair.receiver.driver().media_files().unwrap()[&physical], LOCAL);
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key(&canonical));
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(binding).unwrap().is_empty());
    assert_eq!(saved_versions(&pair, &baseline.name), versions);
    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver.driver().complete_pending_media_reply().unwrap();

    for _ in 0..2 {
        // A no-op local ingestion still invokes the ordinary feed consumer;
        // full registration/reconcile also reads the actual journal and files.
        assert_eq!(pair.receiver.driver().put_media(&physical, LOCAL).unwrap(), physical);
        pair.receiver.driver().reconcile_binding().unwrap();
        assert_retained_ack(&tracked(&pair.receiver, &physical), &baseline);
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        assert_retained_ack(
            &acked(&pair.receiver, &pair.service, &media(&canonical, LOCAL)),
            &baseline,
        );
        acked(&pair.sender, &pair.service, &media(&canonical, LOCAL));
        assert_eq!(saved_versions(&pair, &baseline.name), versions);
        assert_no_destination_batch(&pair.receiver, observations, &baseline.name);
    }
    let expected_notes = notes(&mut pair.receiver);
    assert!(expected_notes.len() >= 2);
    assert_eq!(notes(&mut pair.sender), expected_notes);
    let front = format!("<img src=\"{physical}\">");
    for note in &expected_notes {
        assert_eq!(note.flds, format!("{front}{front}\x1flocal back").as_bytes());
        for rig in [&pair.receiver, &pair.sender] {
            acked(rig, &pair.service, &Payload::Note(note.clone()));
        }
    }
    assert_eq!(files_by_case(&mut pair.receiver), files_by_case(&mut pair.sender));
    assert_eq!(pair.sender.driver().media_files().unwrap()[&canonical], LOCAL);
    let copies = pair.receiver.driver().media_conflicts().unwrap();
    let copies: Vec<_> = copies
        .iter()
        .filter(|copy| copy.reason == ConflictReason::MediaContent)
        .collect();
    assert_eq!(copies.len(), 1);
    let path = std::path::Path::new(copies[0].staged_path.as_ref().unwrap());
    assert_eq!(path.file_name().unwrap().to_str().unwrap(), physical);
    assert_eq!(std::fs::read(path).unwrap(), LOCAL);

    pair.receiver.close_clean().unwrap();
    pair.reopen_before_identity();
    pair.verify_and_drive();
    pair.receiver.driver().reconcile_binding().unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_retained_ack(
        &acked(&pair.receiver, &pair.service, &media(&canonical, LOCAL)),
        &baseline,
    );
    assert_no_destination_batch(&pair.receiver, 0, &baseline.name);
    assert_eq!(saved_versions(&pair, &baseline.name), versions);
    assert!(
        pair.receiver
            .driver()
            .media_rows()
            .unwrap()
            .contains(&media(&physical, LOCAL))
    );
    assert_eq!(pair.receiver.driver().media_files().unwrap()[&physical], LOCAL);
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key(&canonical));
    assert_eq!(files_by_case(&mut pair.receiver), files_by_case(&mut pair.sender));
    assert_eq!(notes(&mut pair.receiver), expected_notes);

    let later = b"media-update-payload-02";
    assert_ne!(later.as_slice(), LOCAL);
    assert_eq!(pair.receiver.driver().put_media(&physical, later).unwrap(), physical);
    let changed = tracked(&pair.receiver, &physical);
    assert_eq!(changed.payload, media(&physical, later));
    assert_eq!(changed.acked_payload, baseline.acked_payload);
    assert_eq!(changed.acked_envelope.0, baseline.acked_envelope.0);
    assert_eq!(changed.acked_system_fields, baseline.acked_system_fields);
    assert_eq!(changed.acked_payload_digest, baseline.acked_payload_digest);
    assert_eq!(changed.acked_row_digest, baseline.acked_row_digest);
    assert_eq!(changed.acked_rev, baseline.acked_rev);
    assert_eq!(changed.server_erev, baseline.server_erev);
    assert!(changed.local_rev > changed.acked_rev);
    assert!(changed.mark.is_some());
    assert!(changed.in_flight.is_empty());
    assert_eq!(saved_versions(&pair, &baseline.name), versions);
    for _ in 0..2 {
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    }
    for rig in [&pair.receiver, &pair.sender] {
        let after = acked(rig, &pair.service, &media(&physical, later));
        assert_eq!(after.name, baseline.name);
        assert_eq!(after.origin, baseline.origin);
        assert!(after.server_erev > baseline.server_erev);
        assert_ne!(after.acked_envelope.0, baseline.acked_envelope.0);
        assert_ne!(after.acked_system_fields, baseline.acked_system_fields);
    }
    assert_eq!(saved_versions(&pair, &baseline.name), versions + 1);
    assert_eq!(files_by_case(&mut pair.receiver), files_by_case(&mut pair.sender));
    pair.close();
}

#[test]
fn both_created_conflict_updates_preexisting_hash_entity_acked_with_other_content() {
    let mut pair = Pair::adopted();
    let moved = hash_name();
    let acked_name = moved.as_str();
    let acked_bytes = REMOTE;
    assert_eq!(
        pair.receiver.driver().put_media(acked_name, acked_bytes).unwrap(),
        acked_name
    );
    let front = format!("<img src=\"{acked_name}\">");
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&front, "local back"])
            .unwrap();
    }
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let baseline = acked(&pair.receiver, &pair.service, &media(acked_name, acked_bytes));
    acked(&pair.sender, &pair.service, &media(acked_name, acked_bytes));
    let versions = saved_versions(&pair, &baseline.name);
    assert_eq!(versions, 1);

    create_both(&mut pair, "photo.jpg", &moved, true);
    fetch_to_staged_download(&mut pair, &baseline);
    let replacement = pair.receiver_dir.join("replacement-image.bin");
    std::fs::write(&replacement, LOCAL).unwrap();
    // Replacing the file also changes the folder marker, so the store's normal
    // change check registers it before M4 reads its expected destination state.
    std::fs::rename(replacement, pair.receiver_dir.join("collection.media").join(&moved)).unwrap();
    resume_download_to_move(&mut pair, &moved);
    let held = tracked(&pair.receiver, &moved);
    assert_eq!(held.acked_payload, baseline.acked_payload);
    assert_eq!(held.acked_payload_digest, baseline.acked_payload_digest);
    assert_eq!(held.acked_row_digest, baseline.acked_row_digest);
    assert_eq!(held.acked_envelope.0, baseline.acked_envelope.0);
    assert_eq!(held.acked_rev, baseline.acked_rev);
    assert_eq!(held.server_erev, baseline.server_erev);
    assert!(held.in_flight.is_empty());
    assert_eq!(saved_versions(&pair, &baseline.name), versions);
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ResolutionBeforeMove).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    let settled = tracked(&pair.receiver, &moved);
    assert_eq!(settled.name, baseline.name);
    assert_eq!(settled.origin, baseline.origin);
    assert_eq!(settled.payload, media(&moved, LOCAL));
    assert_eq!(settled.acked_payload, baseline.acked_payload);
    assert_eq!(settled.acked_payload_digest, baseline.acked_payload_digest);
    assert_eq!(settled.acked_row_digest, baseline.acked_row_digest);
    assert_eq!(settled.acked_rev, baseline.acked_rev);
    assert_eq!(settled.server_erev, baseline.server_erev);
    assert_eq!(settled.acked_envelope.0, baseline.acked_envelope.0);
    assert_eq!(settled.acked_system_fields, baseline.acked_system_fields);
    assert!(settled.local_rev > settled.acked_rev);
    assert!(settled.mark.is_some());
    assert!(settled.in_flight.is_empty());
    assert_eq!(saved_versions(&pair, &baseline.name), versions);

    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver.driver().complete_pending_media_reply().unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    for _ in 0..2 {
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    }
    for rig in [&pair.receiver, &pair.sender] {
        let after = acked(rig, &pair.service, &media(&moved, LOCAL));
        assert_eq!(after.name, baseline.name);
        assert_eq!(after.origin, baseline.origin);
        assert!(after.server_erev > baseline.server_erev);
        assert_ne!(after.acked_envelope.0, baseline.acked_envelope.0);
    }
    assert_eq!(saved_versions(&pair, &baseline.name), versions + 1);
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files.len(), 2);
    assert_eq!(files["photo.jpg"], REMOTE);
    assert_eq!(files[&moved], LOCAL);
    assert_eq!(pair.sender.driver().media_files().unwrap(), files);
    let expected = notes(&mut pair.receiver);
    assert!(expected.len() >= 2);
    assert_eq!(notes(&mut pair.sender), expected);
    let front = format!("<img src=\"{moved}\">");
    for note in expected {
        assert_eq!(note.flds, format!("{front}{front}\x1flocal back").as_bytes());
        for rig in [&pair.receiver, &pair.sender] {
            acked(rig, &pair.service, &Payload::Note(note.clone()));
        }
    }
    for rig in [&mut pair.receiver, &mut pair.sender] {
        let binding = rig.facts().unwrap().current.unwrap().id;
        assert!(rig.driver().apply_intents(binding).unwrap().is_empty());
        assert!(rig.driver().inbox_entries(binding).unwrap().is_empty());
    }
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
