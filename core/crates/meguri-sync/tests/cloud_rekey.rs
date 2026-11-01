//! Tests first: K14.1-3, K17.1-3, K12.3/K13.2; I5; O1.1; M9.8/M9.9.
//! Starting commit: 0d519bd4 (supplied clean master).
//! Source-only packet: no compilation, execution, gate or acceptance claim.
//! P24/Q11 checked for the rules used here; no K19 machinery or closure claim.
//! These three red cases cover Note/Card and an own landed Note tombstone,
//! not the required six-kind implementation, recovery, holds or failure matrix.

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
        .deliver_collection_records(&[record.clone()])
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
        .deliver_collection_records(&[record.clone()])
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
        .deliver_collection_records(&[landed.clone()])
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
        .deliver_collection_records(&[landed.clone()])
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
        .deliver_collection_records(&[record.clone()])
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
