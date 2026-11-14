//! K14 rekeys the local type and its references before applying the server type.
use super::*;
use cloud::{entity_codec::{self as codec, Payload}, testing::IncomingStage, workflow::EntityFacts};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

struct CollisionCapture {
    layout: EntityFacts,
    note: EntityFacts,
    cards: Vec<EntityFacts>,
}
impl CollisionCapture {
    fn references(&self) -> impl Iterator<Item = &EntityFacts> {
        std::iter::once(&self.note).chain(&self.cards)
    }

    fn assert_k14_tracking_matches_capture(&self, runtime: &cloud::runtime::Runtime) {
        for expected in std::iter::once(&self.layout).chain(self.references()) {
            let tracked = runtime.entity_facts(&expected.name).unwrap().unwrap();
            assert!(tracked == *expected);
        }
    }
}

fn publish_other_origin(
    service: &mut Service<SeededRandom>,
    binding: &cloud::Binding,
    local: &EntityFacts,
) -> (Saved, Payload) {
    let mut remote = match &local.payload {
        Payload::Notetype(layout) => layout.clone(),
        _ => panic!("collision fixture requires a type"),
    };
    // Different ordered field names make this a different K14.1 object.
    remote.name = b"K14 server type".to_vec();
    remote.fields[0].name = b"K14 remote field".to_vec();
    let payload = Payload::Notetype(remote);
    let origin = [91; 16];
    assert_ne!(origin, local.origin);
    let header = codec::EntityFields {
        kind: codec::Kind::Notetype,
        key: local.key.clone(),
        origin,
        erev: 1,
        generation: None,
        tombstone: false,
        deleted_at_ms: 0,
        writer: [92; 16],
        base_sha256: None,
    };
    let envelope = codec::Entity::build(header, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let result = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: cloud::adapter::collection_zone(binding.facts.epoch),
        atomic: false,
        changes: vec![Mutation::Save {
            record: RecordInput::encrypted(&local.name, "MGEntity", envelope),
            if_unchanged: None,
        }],
    }).unwrap().result;
    let Ok(Reply::Modified(results)) = result else { panic!("modify reply required") };
    assert_eq!(results.len(), 1);
    let record = match results.into_iter().next().unwrap().result {
        Ok(MutationSuccess::Saved(record)) => record,
        _ => panic!("server save required"),
    };
    let envelope = match record.fields.get("e") {
        Some(Field::EncryptedBytes(bytes)) => bytes.clone(),
        _ => panic!("saved envelope required"),
    };
    (Saved {
        name: record.name.clone(),
        envelope,
        system_fields: record.system_fields.bytes().to_vec(),
    }, payload)
}

fn rekey_intent_count(path: &std::path::Path, id: Option<i64>) -> i64 {
    let connection = meguri_storage::rusqlite::Connection::open_with_flags(
        path, meguri_storage::rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY,
    ).unwrap();
    match id {
        Some(id) => connection.query_row("SELECT count(*) FROM cloud_rekey_intents WHERE id=?1", [id], |row| row.get(0)).unwrap(),
        None => connection.query_row("SELECT count(*) FROM cloud_rekey_intents", [], |row| row.get(0)).unwrap(),
    }
}

#[test]
fn k14_type_collision_preserves_tracking_until_paired_intents_settle() {
    let temporary = tempfile::tempdir().unwrap();
    let directory = temporary.path().join("device");
    generated(&directory);
    let host = open_host(&directory, keys());
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temporary.path(), Config::default(), SeededRandom::new(448)).unwrap();
    runtime.confirm_connection_publication(&preview(&host, &mut service), &collection).unwrap();
    own_until_scan(&host, &mut service);
    runtime.reconcile_binding(&collection, &media, Duration::from_secs(30)).unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let note = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let (note_id, type_key) = match &note.payload {
        Payload::Note(note) => (note.id, codec::id_key(note.mid)),
        _ => panic!("note required"),
    };
    let type_name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &type_key).unwrap();
    let cards: Vec<_> = runtime.entity_facts_page(codec::Kind::Card, None).unwrap().into_iter()
        .filter(|facts| matches!(&facts.payload, Payload::Card(card) if card.nid == note_id)).collect();
    assert!(!cards.is_empty());
    assert_eq!(runtime.entity_facts(&type_name).unwrap().unwrap().server_erev, 0);
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let batch = runtime.build_collection_batch(&instance, &collection, &[type_name.clone()].into()).unwrap();
    assert_eq!(batch.saves.len(), 1);
    let captured = Arc::new(CollisionCapture {
        layout: runtime.entity_facts(&type_name).unwrap().unwrap(), note, cards,
    });
    assert_eq!(captured.layout.in_flight.len(), 1);
    let (saved, server_payload) = publish_other_origin(&mut service, &binding, &captured.layout);
    runtime.receive_collection_records(&instance, std::slice::from_ref(&saved)).unwrap();

    let saw_commit = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let callback_flag = Arc::clone(&saw_commit);
    let callback_capture = Arc::clone(&captured);
    let runtime_weak = Arc::downgrade(runtime);
    let tracking_path = directory.join(cloud::TRACKING_FILE);
    let callback_path = tracking_path.clone();
    let owner = binding.id;
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage != IncomingStage::ApplyAfterCommit { return; }
        if callback_flag.swap(true, std::sync::atomic::Ordering::SeqCst) { return; }
        let runtime = runtime_weak.upgrade().unwrap();
        assert!(!runtime.status().unwrap().tracking_stopped);
        callback_capture.assert_k14_tracking_matches_capture(&runtime);
        let intents = runtime.apply_intents(owner).unwrap();
        assert_eq!(intents.len(), 1);
        assert!(intents[0].result_seq.is_some());
        assert_eq!(rekey_intent_count(&callback_path, Some(intents[0].id)), 1);
    });
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    assert!(saw_commit.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert!(runtime.apply_intents(owner).unwrap().is_empty());
    let inbox = runtime.inbox_entries(owner).unwrap();
    // Print only typed classification and attempts; never record contents.
    let pending: Vec<_> = inbox.iter().map(|item| {
        let reason = match &item.entry.reason {
            cloud::InboxReason::ApplyFailed { failure, .. } => Err(*failure),
            cloud::InboxReason::Ready => Ok("ready"),
            cloud::InboxReason::Generation => Ok("generation"),
            cloud::InboxReason::Reference => Ok("reference"),
            cloud::InboxReason::Structure => Ok("structure"),
            cloud::InboxReason::Format => Ok("format"),
            cloud::InboxReason::Asset => Ok("asset"),
        };
        (reason, item.entry.retry_count)
    }).collect();
    assert!(inbox.is_empty(), "retained inbox reason/attempts: {pending:?}");

    let moved = runtime.entity_facts_page(codec::Kind::Notetype, None).unwrap().into_iter()
        .find(|facts| facts.origin == captured.layout.origin && facts.key != captured.layout.key).unwrap();
    let moved_id = match &moved.payload {
        Payload::Notetype(layout) => layout.id,
        _ => panic!("moved type required"),
    };
    let reader = collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.row_image(Table::Notes, &RowKey::Id(note_id)).unwrap().unwrap()[2], meguri_storage::Cell::Integer(moved_id));
    for previous in captured.references() {
        let current = runtime.entity_facts(&previous.name).unwrap().unwrap();
        assert_eq!(current.acked_rev, previous.acked_rev);
        assert_eq!(current.acked_envelope, previous.acked_envelope);
        assert_eq!(current.acked_payload, previous.acked_payload);
        assert_eq!(current.in_flight, previous.in_flight);
        match &current.payload {
            Payload::Note(_) => assert!(current.local_rev > previous.local_rev && current.mark.is_some()),
            Payload::Card(card) => {
                assert!(current == *previous);
                assert_eq!(reader.row_image(Table::Cards, &RowKey::Id(card.id)).unwrap().unwrap()[1], meguri_storage::Cell::Integer(note_id));
            }
            _ => panic!("dependent kind required"),
        }
    }
    drop(reader);
    let original_key = runtime.entity_facts(&type_name).unwrap().unwrap();
    assert_eq!(original_key.acked_envelope.0, saved.envelope);
    assert!(original_key.acked_payload == Some(server_payload));
    assert!(runtime.row_classification_details().unwrap().1.is_empty());
    assert_eq!(rekey_intent_count(&tracking_path, None), 0);
    host.close().unwrap();
}
