use meguri_testkit::bench::peak_alloc::PeakAlloc;
#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{
    BindingId,
    entity_codec::{self as codec, Payload},
    workflow::EntityFacts,
};
use meguri_testkit::cloud::{
    Field, Mutation, MutationSuccess, Operation, Record, RecordInput, Reply, child_driver::AddedNote,
    coordinator::PublicationRig,
};
use rusqlite::{Connection, OpenFlags, params_from_iter, types::Value};
use std::collections::BTreeSet;
use support::*;

fn binding(pair: &Pair) -> BindingId {
    pair.receiver.facts().unwrap().current.unwrap().id
}

fn name(pair: &Pair, kind: codec::Kind, id: i64) -> String {
    let key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    codec::entity_name(&key.0, kind, &codec::id_key(id)).unwrap()
}

fn saved(pair: &Pair, name: &str) -> Record {
    let zone = pair.sender.collection_zone().unwrap();
    pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone].records[name].clone()
}

fn envelope(record: &Record) -> codec::Entity {
    let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
        panic!("actual Service envelope")
    };
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(bytes).unwrap() else {
        panic!("supported Service envelope")
    };
    entity
}

fn add(rig: &mut PublicationRig, fields: &[&str]) -> AddedNote {
    let template = notes(rig).remove(0);
    let did = rig
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| match p {
            Payload::Card(c) if c.nid == template.id => Some(c.did),
            _ => None,
        })
        .unwrap();
    rig.driver().add_note(template.mid, did, fields).unwrap()
}

// Synthetic server input, persisted by the existing opaque Service. Return
// only its actual Saved result, including the generated system fields.
fn save_input(pair: &mut Pair, key: &str, fields: codec::EntityFields, payload: &Payload) -> Record {
    let entity = codec::Entity::build(fields, &payload.encode().unwrap()).unwrap();
    assert!(entity.payload.is_some());
    let zone = pair.sender.collection_zone().unwrap();
    let prior = pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone]
        .records
        .get(key)
        .map(|r| r.system_fields.clone());
    let response = pair
        .service
        .request(
            &scope(),
            Default::default(),
            Operation::Modify {
                zone,
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput {
                        name: key.into(),
                        record_type: "MGEntity".into(),
                        fields: [("e".into(), Field::EncryptedBytes(entity.encode().unwrap()))].into(),
                    },
                    if_unchanged: prior,
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual Service modify result")
    };
    let Ok(MutationSuccess::Saved(record)) = &items[0].result else {
        panic!("actual Service Saved result")
    };
    record.clone()
}

fn server_fields(pair: &Pair, kind: codec::Kind, id: i64) -> codec::EntityFields {
    codec::EntityFields {
        kind,
        key: codec::id_key(id).to_vec(),
        origin: [93; 16],
        erev: 1,
        generation: None,
        tombstone: false,
        deleted_at_ms: 0,
        writer: pair.sender.facts().unwrap().current.unwrap().id.0,
        base_sha256: None,
    }
}

fn assert_pending_new_identity(facts: &EntityFacts, old: &EntityFacts, kind: codec::Kind, id: i64) {
    assert_eq!(facts.kind, kind);
    assert_eq!(facts.key, codec::id_key(id));
    assert_eq!(facts.origin, old.origin, "the losing object keeps its origin");
    assert!(facts.mark.is_some());
    assert!(facts.tombstone_at.is_none());
    assert_eq!(facts.server_erev, 0, "no server version was saved at the new name");
    assert_eq!(facts.acked_rev, 0);
    assert!(facts.acked_system_fields.is_empty());
    assert!(facts.acked_envelope.0.is_empty());
    assert!(facts.acked_payload.is_none());
    assert!(facts.in_flight.is_empty(), "an old-name flight is not a new-name save");
}

fn assert_reference_pending(facts: &EntityFacts, before: &EntityFacts) {
    assert!(facts.mark.is_some());
    assert!(facts.local_rev > before.local_rev);
    assert_eq!(facts.origin, before.origin);
    assert_eq!(facts.server_erev, before.server_erev);
    assert_eq!(facts.acked_rev, before.acked_rev);
    assert_eq!(facts.acked_system_fields, before.acked_system_fields);
    assert_eq!(facts.acked_envelope, before.acked_envelope);
    assert_eq!(facts.acked_row_digest, before.acked_row_digest);
}

fn assert_ack(facts: &EntityFacts, record: &Record) {
    let entity = envelope(record);
    assert_eq!(facts.origin, entity.fields.origin);
    assert_eq!(facts.server_erev, entity.fields.erev);
    assert_eq!(facts.acked_system_fields, record.system_fields.bytes());
    let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
        panic!("Service envelope")
    };
    assert_eq!(facts.acked_envelope.0, *bytes);
    assert_eq!(facts.acked_payload_digest, entity.payload_sha256);
    assert!(facts.mark.is_none());
}

fn assert_settled(pair: &mut Pair) {
    let b = binding(pair);
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
}

/// K14.1/2, I5: skip both a physical row and a tracking-only tombstone,
/// then fill the first gap, even though a higher id already exists.
#[test]
fn fetched_note_collision_skips_tracking_reservation_and_rewrites_dependent_cards() {
    let mut pair = Pair::adopted();
    let local = add(&mut pair.receiver, &["local collision", "local back"]);
    let remote = add(&mut pair.sender, &["server collision", "server back"]);
    let old = local.note.id;
    assert_eq!(remote.note.id, old);
    assert_ne!(local.note.guid, remote.note.guid);
    let key = name(&pair, codec::Kind::Note, old);
    let reserved = add(&mut pair.receiver, &["reservation", "back"]);
    assert_eq!(reserved.note.id, old + 1);
    let reserved_key = name(&pair, codec::Kind::Note, reserved.note.id);
    // Keep a physical successor before deleting the reservation: ordinary
    // native add allocation may otherwise reuse the now-empty reserved id.
    let occupied = add(&mut pair.receiver, &["occupied next row", "back"]);
    assert_eq!(occupied.note.id, old + 2);
    let b = binding(&pair);
    let instance = pair
        .receiver
        .driver()
        .incoming_observation(b)
        .unwrap()
        .engine_instance
        .unwrap();
    let batch = pair
        .receiver
        .driver()
        .build_for_instance(instance, BTreeSet::from([reserved_key.clone()]))
        .unwrap();
    assert!(batch.saves.iter().any(|s| s.name == reserved_key));
    // A genuinely handed, never-ACKed deletion leaves a tracking reservation.
    // No Service request is made for this batch, and no result is fabricated.
    pair.receiver.driver().delete_note(reserved.note.id).unwrap();
    pair.receiver.advance_clock_ms(7_000).unwrap();
    let higher = add(&mut pair.receiver, &["higher than the gap", "back"]);
    let new = old + 3;
    assert!(higher.note.id > new);
    let new_key = name(&pair, codec::Kind::Note, new);
    let before = pair.receiver.facts().unwrap();
    let losing = before.entities[&key].clone();
    let reservation = before.entities[&reserved_key].clone();
    assert_eq!(losing.server_erev, 0);
    assert_eq!(reservation.server_erev, 0);
    assert!(reservation.tombstone_at.is_some() && !reservation.in_flight.is_empty());
    assert!(
        !notes(&mut pair.receiver)
            .iter()
            .any(|n| n.id == reserved.note.id || n.id == new)
    );
    assert!(!before.entities.contains_key(&new_key));
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let record = saved(&pair, &key);
    assert_ne!(envelope(&record).fields.origin, losing.origin);
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    // Current baseline may complete with zero applies (Structure parking).
    // Demand actual correct rows, rather than any particular error variant.
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let rows = notes(&mut pair.receiver);
    assert_eq!(
        rows.iter().find(|n| n.id == old),
        Some(&remote.note),
        "old guard leaves the losing Note here"
    );
    let mut moved = local.note.clone();
    moved.id = new;
    moved.modified = (NOW + 7_000) / 1_000;
    assert_eq!(rows.iter().find(|n| n.id == new), Some(&moved));
    assert!(rows.contains(&occupied.note) && rows.contains(&higher.note));
    let after = pair.receiver.facts().unwrap();
    assert_ack(&after.entities[&key], &record);
    assert_pending_new_identity(&after.entities[&new_key], &losing, codec::Kind::Note, new);
    assert!(after.entities[&reserved_key] == reservation);
    for original in &local.cards {
        let mut expected = original.clone();
        expected.nid = new;
        expected.modified = (NOW + 7_000) / 1_000;
        assert_eq!(pair.receiver.driver().read_card(original.id).unwrap(), expected);
        let card_key = name(&pair, codec::Kind::Card, original.id);
        assert_reference_pending(&after.entities[&card_key], &before.entities[&card_key]);
        assert!(after.entities[&card_key].payload == Payload::Card(expected));
    }
    assert_settled(&mut pair);
    pair.close();
}

/// K14.1/2: different nid makes a different Card; only the losing Card's
/// review history follows it. The history was made by the native review API.
#[test]
fn fetched_card_collision_moves_local_card_and_rewrites_actual_revlog() {
    let mut pair = Pair::adopted();
    let local = add(&mut pair.receiver, &["review local collision", "back"]);
    let session = pair.receiver.driver().review_begin(local.note.id).unwrap();
    let question = pair.receiver.driver().review_start(session).unwrap();
    pair.receiver.driver().review_show_answer(session, question.id).unwrap();
    pair.receiver.advance_clock_ms(1_537).unwrap();
    let answer = pair.receiver.driver().review_answer_good(session, question.id).unwrap();
    pair.receiver.driver().review_end_and_drain(session).unwrap();
    let reviewed = pair.receiver.driver().read_card(question.id).unwrap();
    let history = pair.receiver.driver().read_revlog(answer.revlog_id).unwrap();
    assert_eq!(history.cid, reviewed.id);
    let old = reviewed.id;
    let new = old + 1;
    let key = name(&pair, codec::Kind::Card, old);
    let new_key = name(&pair, codec::Kind::Card, new);
    let history_key = name(&pair, codec::Kind::Revlog, history.id);
    let before = pair.receiver.facts().unwrap();
    let losing = &before.entities[&key];
    assert_eq!(losing.server_erev, 0);
    assert!(!before.entities.contains_key(&new_key));
    let mut remote = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| match p {
            Payload::Card(c) => Some(c),
            _ => None,
        })
        .unwrap();
    remote.id = old;
    assert_ne!(remote.nid, reviewed.nid);
    let fields = server_fields(&pair, codec::Kind::Card, old);
    assert_ne!(fields.origin, losing.origin);
    let record = save_input(&mut pair, &key, fields, &Payload::Card(remote.clone()));
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(
        pair.receiver.driver().read_card(old).unwrap(),
        remote,
        "old guard parks the actual Card collision"
    );
    let mut moved = reviewed.clone();
    moved.id = new;
    moved.modified = (NOW + 1_537) / 1_000;
    assert_eq!(pair.receiver.driver().read_card(new).unwrap(), moved);
    let mut rewritten = history.clone();
    rewritten.cid = new;
    assert_eq!(pair.receiver.driver().read_revlog(history.id).unwrap(), rewritten);
    assert!(
        notes(&mut pair.receiver).contains(&local.note),
        "Card re-key does not re-key its Note"
    );
    let after = pair.receiver.facts().unwrap();
    assert_ack(&after.entities[&key], &record);
    assert_pending_new_identity(&after.entities[&new_key], losing, codec::Kind::Card, new);
    assert_reference_pending(&after.entities[&history_key], &before.entities[&history_key]);
    assert!(after.entities[&history_key].payload == Payload::Revlog(rewritten));
    assert_settled(&mut pair);
    pair.close();
}

// Raw SQL is confined to synthetic fixture observation/re-creation, with no
// tracking writes. Re-create only while the child is stopped, representing an
// import that keeps ids; the subsequent delete and Undo use native services.
fn fixture_rows(pair: &Pair, id: i64) -> (Vec<Value>, Vec<Vec<Value>>) {
    let conn = Connection::open_with_flags(
        pair.receiver_dir.join("collection.anki2"),
        OpenFlags::SQLITE_OPEN_READ_ONLY,
    )
    .unwrap();
    let note = conn
        .query_row("SELECT * FROM notes WHERE id=?1", [id], |r| {
            (0..11).map(|i| r.get(i)).collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap();
    let mut query = conn.prepare("SELECT * FROM cards WHERE nid=?1 ORDER BY id").unwrap();
    let cards = query
        .query_map([id], |r| {
            (0..18).map(|i| r.get(i)).collect::<rusqlite::Result<Vec<Value>>>()
        })
        .unwrap()
        .collect::<rusqlite::Result<Vec<_>>>()
        .unwrap();
    (note, cards)
}

fn recreate_fixture_rows(pair: &Pair, note: &[Value], cards: &[Vec<Value>]) {
    let mut conn = Connection::open(pair.receiver_dir.join("collection.anki2")).unwrap();
    let tx = conn.transaction().unwrap();
    tx.execute(
        "INSERT INTO notes VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11)",
        params_from_iter(note),
    )
    .unwrap();
    for card in cards {
        tx.execute(
            "INSERT INTO cards VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18)",
            params_from_iter(card),
        )
        .unwrap();
    }
    tx.commit().unwrap();
}

/// K13.2 own landed save + K14.3/K17.3: final old tombstone cannot be
/// overwritten by an Undo-restored live row; preserve that row at a new id.
#[test]
fn own_final_note_tombstone_moves_undo_row_to_a_new_key() {
    use meguri_testkit::cloud::coordinator::Point;

    let mut pair = Pair::adopted();
    let original = notes(&mut pair.receiver).into_iter().max_by_key(|n| n.id).unwrap();
    let old = original.id;
    let new = old + 1;
    let key = name(&pair, codec::Kind::Note, old);
    let new_key = name(&pair, codec::Kind::Note, new);
    let (note_image, card_images) = fixture_rows(&pair, old);
    pair.receiver.driver().delete_note(old).unwrap();
    pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeCollectionSave));
    let request = pair.receiver.held_request().unwrap();
    let Operation::Modify { zone, changes, .. } = request.operation else {
        panic!("real production-built deletion request")
    };
    // Land only the production-built Note deletion; Card deletions remain
    // unsaved so this case isolates Note re-key and its Card references.
    let changes: Vec<_> = changes
        .into_iter()
        .filter(|m| matches!(m, Mutation::Save { record, .. } if record.name == key))
        .collect();
    assert_eq!(changes.len(), 1);
    let response = pair
        .service
        .request(
            &scope(),
            request.caller,
            Operation::Modify {
                zone,
                atomic: false,
                changes,
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual lost Service reply")
    };
    let Ok(MutationSuccess::Saved(landed)) = &items[0].result else {
        panic!("Note tombstone really landed")
    };
    let remote = envelope(landed);
    assert!(remote.fields.tombstone);
    let b = binding(&pair);
    assert_eq!(remote.fields.writer, b.0);
    let handed = pair.receiver.facts().unwrap().entities[&key].clone();
    let Field::EncryptedBytes(bytes) = &landed.fields["e"] else {
        panic!("envelope")
    };
    assert!(handed.in_flight.iter().any(|f| {
        f.tombstone && f.erev == remote.fields.erev && f.tombstone_envelope_digest == Some(codec::sha256(bytes))
    }));
    assert!(
        pair.receiver.driver().undo().is_err(),
        "the handed deletion itself stays protected"
    );
    pair.receiver.kill().unwrap(); // never deliver the Saved result
    recreate_fixture_rows(&pair, &note_image, &card_images);
    pair.reopen_before_identity();
    pair.verify_and_drive();
    pair.receiver.driver().delete_note(old).unwrap();
    pair.receiver.driver().undo().unwrap();
    assert!(notes(&mut pair.receiver).contains(&original));
    let before = pair.receiver.facts().unwrap();
    assert!(!before.entities.contains_key(&new_key));
    assert!(
        before.entities[&key]
            .in_flight
            .iter()
            .any(|f| { f.tombstone && f.tombstone_envelope_digest == Some(codec::sha256(bytes)) })
    );
    let cards: Vec<_> = pair
        .receiver
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .filter_map(|p| match p {
            Payload::Card(c) if c.nid == old => Some(c),
            _ => None,
        })
        .collect();
    assert!(!cards.is_empty());
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(landed))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let rows = notes(&mut pair.receiver);
    assert!(!rows.iter().any(|n| n.id == old), "K14.3 requires a fresh key");
    let mut moved = original.clone();
    moved.id = new;
    moved.modified = NOW / 1_000;
    assert_eq!(rows.iter().find(|n| n.id == new), Some(&moved));
    let after = pair.receiver.facts().unwrap();
    assert_ack(&after.entities[&key], landed);
    assert!(after.entities[&key].tombstone_at.is_some());
    assert!(after.entities[&key].in_flight.is_empty());
    assert_pending_new_identity(
        &after.entities[&new_key],
        &before.entities[&key],
        codec::Kind::Note,
        new,
    );
    for card in cards {
        let card_key = name(&pair, codec::Kind::Card, card.id);
        let mut expected = card;
        expected.nid = new;
        expected.modified = NOW / 1_000;
        assert_eq!(pair.receiver.driver().read_card(expected.id).unwrap(), expected);
        assert_reference_pending(&after.entities[&card_key], &before.entities[&card_key]);
    }
    assert_settled(&mut pair);
    let stable = pair.receiver.driver().collection_rows().unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(landed))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), stable);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), stable);
    pair.close();
}

/// Passing K14.1 control: different origin, equal guid is the same Note.
#[test]
fn unacked_same_guid_note_accepts_server_origin_without_rekey() {
    let mut pair = Pair::adopted();
    let local = add(&mut pair.receiver, &["same imported object", "back"]);
    let id = local.note.id;
    let key = name(&pair, codec::Kind::Note, id);
    let before = pair.receiver.driver().collection_rows().unwrap();
    let fields = server_fields(&pair, codec::Kind::Note, id);
    let old = pair.receiver.facts().unwrap().entities[&key].clone();
    assert_eq!(old.server_erev, 0);
    assert_ne!(fields.origin, old.origin);
    let record = save_input(&mut pair, &key, fields, &Payload::Note(local.note));
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    assert_ack(&pair.receiver.facts().unwrap().entities[&key], &record);
    assert_settled(&mut pair);
    pair.close();
}

/// Passing K14.1/K20 control: an acked origin mismatch is refused.
#[test]
fn acked_note_origin_mismatch_is_refused_without_rekey() {
    let mut pair = Pair::adopted();
    let local = notes(&mut pair.receiver).remove(0);
    let key = name(&pair, codec::Kind::Note, local.id);
    let before = pair.receiver.driver().collection_rows().unwrap();
    let old = pair.receiver.facts().unwrap().entities[&key].clone();
    assert!(old.server_erev > 0);
    let mut fields = server_fields(&pair, codec::Kind::Note, local.id);
    fields.erev = old.server_erev + 1;
    assert_ne!(fields.origin, old.origin);
    let record = save_input(&mut pair, &key, fields, &Payload::Note(local));
    pair.receiver.driver().deliver_collection_records(&[record]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    assert!(pair.receiver.facts().unwrap().entities[&key] == old);
    let b = binding(&pair);
    let refused = pair.receiver.driver().refused_entries(b).unwrap();
    assert!(
        refused
            .iter()
            .any(|r| r.record_name == key && r.reason == "origin_mismatch")
    );
    assert_settled(&mut pair);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

fn pending_note_collision() -> (Pair, AddedNote, AddedNote, String, Record) {
    let mut pair = Pair::adopted();
    let local = add(&mut pair.receiver, &["bounded local collision", "local back"]);
    let remote = add(&mut pair.sender, &["bounded server collision", "server back"]);
    assert_eq!(local.note.id, remote.note.id);
    assert_ne!(local.note.guid, remote.note.guid);
    let key = name(&pair, codec::Kind::Note, local.note.id);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let record = saved(&pair, &key);
    assert_ne!(
        envelope(&record).fields.origin,
        pair.receiver.facts().unwrap().entities[&key].origin
    );
    (pair, local, remote, key, record)
}

#[test]
fn actual_service_save_conflict_rekeys_before_old_key_apply() {
    use meguri_sync::cloud::{InboxReason, InboxSource};
    let (mut pair, local, remote, key, record) = pending_note_collision();
    let b = binding(&pair);
    let old = pair.receiver.facts().unwrap().entities[&key].clone();
    // This helper sends the production-built flight to Service and forwards
    // Service's actual ServerRecordChanged reply through the SDK handler.
    let handoff = pair
        .receiver
        .driver()
        .deliver_real_asset_save_conflict_with_small_handoff(&mut pair.service, &scope(), &key)
        .unwrap();
    let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].source, InboxSource::SaveConflict);
    assert_eq!(inbox[0].reason, InboxReason::Ready);
    let mut metadata = handoff.clone();
    assert_eq!(metadata.in_flight.len(), old.in_flight.len() + 1);
    let flight = metadata.in_flight.pop().unwrap();
    assert_eq!(flight.local_rev, old.local_rev);
    assert_eq!(flight.erev, old.server_erev + 1);
    assert_eq!(flight.row_digest, old.payload.row_digest().unwrap());
    assert_eq!(flight.payload_digest, old.payload.payload_digest().unwrap());
    assert!(!flight.tombstone);
    assert!(metadata == old, "handoff changes only the retained flight");
    assert!(pair.receiver.facts().unwrap().entities[&key] == handoff);
    assert!(notes(&mut pair.receiver).contains(&local.note));
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let new = local.note.id + 1;
    let new_key = name(&pair, codec::Kind::Note, new);
    assert!(notes(&mut pair.receiver).contains(&remote.note));
    assert_pending_new_identity(
        &pair.receiver.facts().unwrap().entities[&new_key],
        &old,
        codec::Kind::Note,
        new,
    );
    assert_ack(&pair.receiver.facts().unwrap().entities[&key], &record);
    assert_settled(&mut pair);
    pair.close();
}

#[test]
fn rekey_rollback_preserves_rows_and_cancel_stays_ready() {
    use meguri_sync::cloud::{ApplyFailure, InboxReason, testing::ApplyInterruption};
    for interruption in [
        ApplyInterruption::Cancel,
        ApplyInterruption::Error,
        ApplyInterruption::Panic,
    ] {
        let (mut pair, _, _, key, record) = pending_note_collision();
        let b = binding(&pair);
        let rows = pair.receiver.driver().collection_rows().unwrap();
        let facts = pair.receiver.facts().unwrap().entities;
        pair.receiver
            .driver()
            .deliver_collection_records(std::slice::from_ref(&record))
            .unwrap();
        pair.receiver.driver().interrupt_next_incoming(interruption).unwrap();
        assert!(pair.receiver.driver().drain_incoming(STEPS).is_err());
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert!(pair.receiver.facts().unwrap().entities == facts);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].record_name, key);
        if interruption == ApplyInterruption::Cancel {
            assert_eq!(inbox[0].reason, InboxReason::Ready);
            assert_eq!(inbox[0].retry_count, 0);
            pair.receiver.driver().drain_incoming(STEPS).unwrap();
            assert_settled(&mut pair);
        } else {
            assert!(matches!(
                inbox[0].reason,
                InboxReason::ApplyFailed {
                    failure: ApplyFailure::Rekey,
                    ..
                }
            ));
            assert_eq!(inbox[0].retry_count, 1);
            // A tracking change/continuation alone is not an event retry.
            pair.receiver.driver().drain_incoming(STEPS).unwrap();
            assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
            assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        }
        pair.close();
    }
}

fn crash_inbox_summary(pair: &mut Pair, b: BindingId) -> (usize, [usize; 8], u32) {
    use meguri_sync::cloud::{ApplyFailure, InboxReason};
    let mut reasons = [0; 8];
    let mut retries = 0;
    for entry in pair.receiver.driver().inbox_entries(b).unwrap() {
        let index = match entry.reason {
            InboxReason::Ready => 0,
            InboxReason::Generation => 1,
            InboxReason::Reference => 2,
            InboxReason::Structure => 3,
            InboxReason::Format => 4,
            InboxReason::Asset => 5,
            InboxReason::ApplyFailed {
                failure: ApplyFailure::Apply,
                ..
            } => 6,
            InboxReason::ApplyFailed {
                failure: ApplyFailure::Rekey,
                ..
            } => 7,
        };
        reasons[index] += 1;
        retries = retries.max(entry.retry_count);
    }
    (pair.receiver.driver().apply_intents(b).unwrap().len(), reasons, retries)
}

#[test]
fn rekey_reopen_uses_before_after_digests_at_each_existing_crash_boundary() {
    use meguri_testkit::cloud::coordinator::Point;
    for point in [Point::ApplyAfterIntent, Point::ApplyAfterCommit, Point::ApplyAfterFlush] {
        let (mut pair, local, remote, key, record) = pending_note_collision();
        let b = binding(&pair);
        let new = local.note.id + 1;
        let new_key = name(&pair, codec::Kind::Note, new);
        let old = pair.receiver.facts().unwrap().entities[&key].clone();
        pair.receiver.hold(point).unwrap();
        pair.receiver
            .driver()
            .deliver_collection_records(std::slice::from_ref(&record))
            .unwrap();
        pair.receiver.driver().drain_incoming(STEPS).unwrap();
        assert!(pair.receiver.is_held(point));
        let intents = pair.receiver.driver().apply_intents(b).unwrap();
        assert_eq!(intents.len(), 1);
        let intent = &intents[0];
        assert!(
            intent
                .keys
                .iter()
                .any(|k| k.name == key && k.before_digest.is_some() && k.after_digest.is_none())
        );
        assert!(
            intent
                .keys
                .iter()
                .any(|k| k.name == new_key && k.before_digest.is_none() && k.after_digest.is_some())
        );
        assert!(
            intent
                .keys
                .iter()
                .all(|k| k.before_payload.is_none() && k.after_payload.is_none())
        );
        let rows = notes(&mut pair.receiver);
        assert_eq!(rows.iter().any(|n| n.id == new), point != Point::ApplyAfterIntent);
        assert!(pair.receiver.facts().unwrap().entities[&key] == old);
        assert!(!pair.receiver.facts().unwrap().entities.contains_key(&new_key));
        pair.receiver.kill().unwrap();
        pair.reopen_before_identity();
        let reopened = crash_inbox_summary(&mut pair, b);
        pair.verify_and_drive();
        let driven = crash_inbox_summary(&mut pair, b);
        assert!(
            notes(&mut pair.receiver).contains(&remote.note),
            "{point:?}: reopened={reopened:?}, driven={driven:?}"
        );
        let mut moved = local.note.clone();
        moved.id = new;
        assert!(notes(&mut pair.receiver).contains(&moved));
        assert_ack(&pair.receiver.facts().unwrap().entities[&key], &record);
        let fresh = pair.receiver.facts().unwrap().entities[&new_key].clone();
        assert_eq!(fresh.origin, old.origin);
        assert_eq!(fresh.server_erev, 0);
        assert!(fresh.mark.is_some());
        assert_settled(&mut pair);
        let stable = pair.receiver.driver().collection_rows().unwrap();
        pair.receiver.close_clean().unwrap();
        pair.receiver = online(&pair.receiver_dir, false);
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), stable);
        assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
        pair.close();
    }
}

#[test]
fn clock_rollback_past_i5_cap_fails_once_without_collection_or_ack_changes() {
    use meguri_sync::cloud::{ApplyFailure, InboxReason};
    let mut pair = Pair::adopted();
    let ahead = 86_400_000 + 5_000;
    pair.receiver.advance_clock_ms(ahead).unwrap();
    let local = add(&mut pair.receiver, &["future local identity", "back"]);
    pair.receiver.advance_clock_ms(-ahead).unwrap();
    let mut remote = notes(&mut pair.sender).remove(0);
    remote.id = local.note.id;
    assert_ne!(local.note.guid, remote.guid);
    let b = binding(&pair);
    let key = name(&pair, codec::Kind::Note, local.note.id);
    let fields = server_fields(&pair, codec::Kind::Note, local.note.id);
    let record = save_input(&mut pair, &key, fields, &Payload::Note(remote));
    let rows = pair.receiver.driver().collection_rows().unwrap();
    let before = pair.receiver.facts().unwrap().entities;
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    assert!(pair.receiver.driver().drain_incoming(STEPS).is_err());
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    assert!(pair.receiver.facts().unwrap().entities == before);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    let inbox = pair.receiver.driver().inbox_entries(b).unwrap();
    assert_eq!(inbox.len(), 1);
    assert!(matches!(
        inbox[0].reason,
        InboxReason::ApplyFailed {
            failure: ApplyFailure::Rekey,
            ..
        }
    ));
    assert_eq!(inbox[0].retry_count, 1);
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().inbox_entries(b).unwrap(), inbox);
    pair.close();
}

#[test]
fn new_local_api_config_after_adoption_captures_and_syncs_without_losing_incarnation() {
    let mut pair = Pair::adopted();
    let b = binding(&pair);
    let config_key = format!("_deck_{}_fixture", NOW + 20);
    let record_key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    let key = codec::entity_name(&record_key.0, codec::Kind::Config, config_key.as_bytes()).unwrap();
    let original_rows = pair.receiver.driver().collection_rows().unwrap();
    assert!(!pair.receiver.facts().unwrap().entities.contains_key(&key));
    let before = pair.receiver.driver().incoming_observation(b).unwrap();
    assert!(before.engine_instance.is_some());
    // Create this key only after real publication/adoption, through LocalApi.
    let capture = pair
        .receiver
        .driver()
        .rekey_config_value(config_key.clone(), 731)
        .unwrap();
    assert!(capture.attempted && capture.persisted);
    let written = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(written.engine_instance, before.engine_instance);
    assert_eq!(written.native_file, before.native_file);
    assert_eq!(written.generation, before.generation);
    assert_eq!(capture.observed_seq, written.seq);
    assert_eq!(capture.generation, written.generation);
    assert!(written.seq > before.seq);
    let payload = Payload::Config(codec::Config {
        key: config_key.into_bytes(),
        mtime_secs: NOW / 1_000,
        val: b"731".to_vec(),
    });
    let rows = pair.receiver.driver().collection_rows().unwrap();
    assert_eq!(rows.len(), original_rows.len() + 1);
    assert!(rows.contains(&payload));
    assert!(original_rows.iter().all(|row| rows.contains(row)));
    let pending = pair.receiver.facts().unwrap().entities[&key].clone();
    assert!(pending.payload == payload);
    assert!(pending.mark.is_some());
    assert_eq!(pending.server_erev, 0);
    assert_eq!(pending.acked_rev, 0);
    assert!(pending.acked_envelope.0.is_empty() && pending.acked_system_fields.is_empty());
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let record = saved(&pair, &key);
    let acknowledged = pair.receiver.facts().unwrap();
    assert_eq!(acknowledged.current.unwrap().id, b);
    assert_ack(&acknowledged.entities[&key], &record);
    assert_eq!(acknowledged.entities[&key].acked_rev, pending.local_rev);
    assert!(acknowledged.entities[&key].payload == payload);
    let synced = pair.receiver.driver().incoming_observation(b).unwrap();
    assert_eq!(synced.engine_instance, written.engine_instance);
    assert_eq!(synced.native_file, written.native_file);
    assert_eq!(synced.generation, written.generation);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    assert_settled(&mut pair);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
    assert_ack(&pair.receiver.facts().unwrap().entities[&key], &record);
    pair.close();
}

#[test]
fn deck_config_rename_lost_tombstone_reply_reopens_with_exact_flight_authority() {
    config_tombstone_reopen(false);
}

#[test]
fn deck_config_lost_tombstone_reply_retains_newer_local_api_value_on_reopen() {
    config_tombstone_reopen(true);
}

fn config_tombstone_reopen(newer_value: bool) {
    use meguri_model::rng::SeededRandom;
    use meguri_testkit::cloud::{Config, Service};
    let id = NOW + 20;
    let root = tempfile::tempdir().unwrap();
    let sender_dir = generated(root.path(), "sender");
    let receiver_dir = generated(root.path(), "receiver");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut sender = online(&sender_dir, true);
    sender.driver().rekey_deck_fixture(id, false).unwrap();
    publish(&mut sender, &mut service);
    let mut receiver = online(&receiver_dir, true);
    let preview = receiver.preview(&mut service, STEPS).unwrap();
    receiver.adopt_cloud(&preview).unwrap();
    receiver.drive_ready(&mut service, STEPS).unwrap();
    let mut pair = Pair {
        service,
        sender,
        receiver,
        receiver_dir,
        root,
    };
    let b = binding(&pair);
    let record_key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    let config_key = format!("_deck_{id}_fixture");
    let old_name = codec::entity_name(&record_key.0, codec::Kind::Config, config_key.as_bytes()).unwrap();
    let acked = pair.receiver.facts().unwrap().entities[&old_name].clone();
    assert!(acked.server_erev > 0 && acked.mark.is_none());
    pair.receiver.driver().rekey_deck_fixture(id, true).unwrap();
    let deck_key = name(&pair, codec::Kind::Deck, id);
    let Payload::Deck(mut remote) = pair.receiver.facts().unwrap().entities[&deck_key].payload.clone() else {
        panic!("Deck");
    };
    remote.name = b"Server collision deck".to_vec();
    let fields = server_fields(&pair, codec::Kind::Deck, id);
    let record = save_input(&mut pair, &deck_key, fields, &Payload::Deck(remote));
    pair.receiver.driver().deliver_collection_records(&[record]).unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_settled(&mut pair);
    let renamed_key = format!("_deck_{}_fixture", id + 1);
    let new_name = codec::entity_name(&record_key.0, codec::Kind::Config, renamed_key.as_bytes()).unwrap();
    let before = pair.receiver.facts().unwrap();
    assert_reference_pending(&before.entities[&old_name], &acked);
    assert!(before.entities[&old_name].tombstone_at.is_some());
    assert!(before.entities[&new_name].mark.is_some());
    assert_eq!(before.entities[&new_name].server_erev, 0);
    let saved = pair
        .receiver
        .driver()
        .lose_named_save_reply(&mut pair.service, &scope(), &old_name)
        .unwrap();
    let tombstone = envelope(&saved);
    assert!(tombstone.fields.tombstone);
    assert_eq!(tombstone.fields.origin, acked.origin);
    assert_eq!(tombstone.fields.writer, b.0);
    assert_eq!(tombstone.fields.erev, acked.server_erev + 1);
    assert_eq!(tombstone.fields.base_sha256, Some(acked.acked_payload_digest));
    assert_eq!(tombstone.payload.as_deref(), Some(&[][..]));
    let handed = pair.receiver.facts().unwrap().entities[&old_name].clone();
    assert_eq!(handed.in_flight.len(), 1);
    let flight = &handed.in_flight[0];
    assert_eq!(flight.local_rev, handed.local_rev);
    assert_eq!(flight.erev, tombstone.fields.erev);
    assert_eq!(flight.row_digest, codec::sha256(&[]));
    assert_eq!(flight.payload_digest, codec::sha256(&[]));
    assert_eq!(
        flight.tombstone_envelope_digest,
        Some(codec::sha256(&tombstone.encode().unwrap()))
    );
    let handed_flight = flight.clone();
    let mut expected = handed.clone();
    let mut newer = None;
    if newer_value {
        pair.receiver
            .driver()
            .rekey_config_value(config_key.clone(), id + 99)
            .unwrap();
        expected = pair.receiver.facts().unwrap().entities[&old_name].clone();
        assert!(expected.local_rev > handed.local_rev);
        assert!(expected.tombstone_at.is_none());
        assert!(expected.in_flight == handed.in_flight);
        assert_eq!(expected.acked_rev, acked.acked_rev);
        assert_eq!(expected.server_erev, acked.server_erev);
        assert_eq!(expected.acked_envelope, acked.acked_envelope);
        let Payload::Config(config) = &expected.payload else {
            panic!("Config");
        };
        assert_eq!(config.key, config_key.as_bytes());
        assert_eq!(config.val, (id + 99).to_string().as_bytes());
        newer = Some(config.clone());
    }
    if newer_value {
        pair.receiver.close_clean().unwrap();
    } else {
        pair.receiver.kill().unwrap();
    }
    pair.reopen_before_identity();
    let reopened = pair.receiver.facts().unwrap();
    assert!(reopened.entities[&old_name] == expected);
    assert!(reopened.entities[&old_name].in_flight[0] == handed_flight);
    if let Some(config) = newer {
        assert!(
            pair.receiver
                .driver()
                .collection_rows()
                .unwrap()
                .contains(&Payload::Config(config))
        );
    }
    assert!(reopened.entities[&new_name] == before.entities[&new_name]);
    assert_eq!(reopened.entities[&old_name].acked_envelope, acked.acked_envelope);
    assert_eq!(reopened.entities[&old_name].acked_rev, acked.acked_rev);
    assert!(reopened.entities[&old_name].mark.is_some());
    assert!(reopened.entities[&new_name].mark.is_some());
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    pair.close();
}

#[test]
fn latest_revlog_collision_journals_derived_card_reference_and_cuts_answer_undo() {
    let mut pair = Pair::adopted();
    let local = add(&mut pair.receiver, &["latest history collision", "back"]);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let session = pair.receiver.driver().review_begin(local.note.id).unwrap();
    let question = pair.receiver.driver().review_start(session).unwrap();
    pair.receiver.driver().review_show_answer(session, question.id).unwrap();
    pair.receiver.advance_clock_ms(1_537).unwrap();
    let answer = pair.receiver.driver().review_answer_good(session, question.id).unwrap();
    assert!(
        pair.receiver
            .driver()
            .read_cloud_review_hold(session)
            .unwrap()
            .op_ids
            .contains(&answer.op_id)
    );
    let history = pair.receiver.driver().read_revlog(answer.revlog_id).unwrap();
    let card = pair.receiver.driver().read_card(question.id).unwrap();
    assert_eq!(card.last_review_id, history.id);
    let card_key = name(&pair, codec::Kind::Card, card.id);
    let old_key = name(&pair, codec::Kind::Revlog, history.id);
    let new_key = name(&pair, codec::Kind::Revlog, history.id + 1);
    let before = pair.receiver.facts().unwrap();
    assert_eq!(before.entities[&old_key].server_erev, 0);
    let mut remote = history.clone();
    remote.cid = pair
        .sender
        .driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .find_map(|p| match p {
            Payload::Card(c) => Some(c.id),
            _ => None,
        })
        .unwrap();
    assert_ne!(remote.cid, history.cid);
    let fields = server_fields(&pair, codec::Kind::Revlog, history.id);
    let record = save_input(&mut pair, &old_key, fields, &Payload::Revlog(remote.clone()));
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    assert_eq!(pair.receiver.driver().read_revlog(history.id).unwrap(), remote);
    let mut moved = history.clone();
    moved.id += 1;
    assert_eq!(pair.receiver.driver().read_revlog(moved.id).unwrap(), moved);
    let mut expected = card.clone();
    expected.last_review_id = moved.id;
    expected.modified = (NOW + 1_537) / 1_000;
    assert_eq!(pair.receiver.driver().read_card(card.id).unwrap(), expected);
    let after = pair.receiver.facts().unwrap();
    assert_reference_pending(&after.entities[&card_key], &before.entities[&card_key]);
    assert!(after.entities[&card_key].payload == Payload::Card(expected));
    assert_pending_new_identity(
        &after.entities[&new_key],
        &before.entities[&old_key],
        codec::Kind::Revlog,
        moved.id,
    );
    assert_ack(&after.entities[&old_key], &record);
    assert!(
        pair.receiver.driver().undo().is_err(),
        "the answer's dependent Card target removes its undo step"
    );
    assert_eq!(pair.receiver.driver().read_revlog(moved.id).unwrap(), moved);
    assert!(
        pair.receiver
            .driver()
            .read_cloud_review_hold(session)
            .unwrap()
            .op_ids
            .is_empty()
    );
    let b = binding(&pair);
    let instance = pair
        .receiver
        .driver()
        .incoming_observation(b)
        .unwrap()
        .engine_instance
        .unwrap();
    let requested = BTreeSet::from([card_key.clone(), new_key.clone()]);
    let held = pair
        .receiver
        .driver()
        .build_for_instance(instance.clone(), requested.clone())
        .unwrap();
    assert!(held.saves.is_empty(), "detached review references stay held");
    pair.receiver.driver().review_end_and_drain(session).unwrap();
    let released = pair.receiver.driver().build_for_instance(instance, requested).unwrap();
    let names: BTreeSet<_> = released.saves.iter().map(|save| save.name.clone()).collect();
    assert!(names.contains(&card_key));
    assert!(names.contains(&new_key));
    assert_settled(&mut pair);
    pair.close();
}

#[test]
fn large_note_and_dependent_card_rekey_stay_under_absolute_child_rust_heap_limit() {
    const BODY: usize = 80 * 1024 * 1024;
    const LIMIT: usize = 300_000_000;
    let (mut pair, local, _, key, record) = pending_note_collision();
    let b = binding(&pair);
    let record_key = pair.receiver.facts().unwrap().current.unwrap().facts.record_key;
    let new_name = codec::entity_name(&record_key.0, codec::Kind::Note, &codec::id_key(local.note.id + 1)).unwrap();
    let card = local.cards[0].id;
    let card_name = name(&pair, codec::Kind::Card, card);
    pair.receiver
        .driver()
        .edit_generated_payload(local.note.id, BODY, 7111)
        .unwrap();
    pair.receiver.driver().generated_card_data(card, BODY).unwrap();
    let note_before = pair.receiver.driver().payload_proof(&key).unwrap();
    let card_before = pair.receiver.driver().payload_proof(&card_name).unwrap();
    let rows_before = pair
        .receiver
        .driver()
        .rekey_rows_proof(local.note.id, card, local.note.id)
        .unwrap();
    // The reset seeds the peak from all currently live child allocations.
    let live_at_start = pair.receiver.driver().payload_memory_reset().unwrap();
    pair.receiver
        .driver()
        .deliver_collection_records(std::slice::from_ref(&record))
        .unwrap();
    pair.receiver.driver().drain_incoming(STEPS).unwrap();
    let (drain_peak, drain_live) = pair.receiver.driver().payload_memory_peak().unwrap();
    eprintln!("device/core Rust heap phase=after_drain cumulative_peak={drain_peak} live={drain_live}");
    for sample in pair
        .receiver
        .driver()
        .payload_memory_stages()
        .unwrap()
        .into_iter()
        .flatten()
    {
        let stage = sample.stage;
        let (peak, live) = sample.first;
        eprintln!("device/core Rust heap stage={stage:?} first cumulative_peak={peak} live={live}");
        let (peak, live) = sample.high_water;
        eprintln!("device/core Rust heap stage={stage:?} high_water cumulative_peak={peak} live={live}");
    }
    let rows_after = pair
        .receiver
        .driver()
        .rekey_rows_proof(local.note.id + 1, card, local.note.id)
        .unwrap();
    assert_eq!(
        rows_after, rows_before,
        "all physical cells match except the allocated Note id and cards.nid"
    );
    let moved = pair.receiver.driver().payload_proof(&new_name).unwrap();
    assert!(moved.dirty);
    assert_eq!(moved.server_erev, 0);
    assert_eq!(moved.acked_rev, 0);
    assert_eq!(moved.flights, 0);
    assert!(moved.system_fields.is_empty() && moved.envelope.is_empty());
    let rewritten = pair.receiver.driver().payload_proof(&card_name).unwrap();
    assert!(rewritten.dirty && rewritten.local_rev > card_before.local_rev);
    assert_ne!(rewritten.payload_digest, card_before.payload_digest);
    assert_eq!(rewritten.acked_rev, card_before.acked_rev);
    assert_eq!(rewritten.server_erev, card_before.server_erev);
    assert_eq!(rewritten.acked_payload_digest, card_before.acked_payload_digest);
    assert_eq!(rewritten.system_fields, card_before.system_fields);
    assert_eq!(rewritten.envelope, card_before.envelope);
    let applied = pair.receiver.driver().payload_proof(&key).unwrap();
    let entity = envelope(&record);
    assert!(!applied.dirty);
    assert_eq!(applied.server_erev, entity.fields.erev);
    assert_eq!(applied.acked_payload_digest, entity.payload_sha256);
    assert_eq!(applied.system_fields, record.system_fields.bytes());
    assert_eq!(applied.envelope, entity.encode().unwrap());
    assert_ne!(applied.payload_digest, note_before.payload_digest);
    assert!(pair.receiver.driver().apply_intents(b).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(b).unwrap().is_empty());
    let (peak, live) = pair.receiver.driver().payload_memory_peak().unwrap();
    eprintln!("device/core Rust heap phase=after_row_ack_proof cumulative_peak={peak} live={live}");
    assert!(peak >= live_at_start && peak >= live);
    assert!(
        peak < LIMIT,
        "absolute child Rust heap peak={peak}, live={live}, start={live_at_start}"
    );
    // The native process RSS requirement remains a separate integration gate.
    pair.close();
}
