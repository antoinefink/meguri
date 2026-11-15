//! Ordinary K13/K15.7 receive through the real host, observer and WAL writer.
//! Source-only continuation of K10/K14; no generation claim or gate assertion.
use super::*;
use cloud::{entity_codec::{self as codec, Payload}, testing::IncomingStage, workflow::EntityFacts};
use meguri_model::{ids::{CardId, NotetypeId}, proto::{BlobMessage, notetype::*}};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

fn source_type(host: &CoreHost) -> EntityFacts {
    let runtime = host.cloud_runtime().unwrap();
    let note = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = note.payload else { panic!("note required") };
    runtime.entity_facts_page(codec::Kind::Notetype, None).unwrap().into_iter()
        .find(|e| matches!(&e.payload, Payload::Notetype(n) if n.id == note.mid)).unwrap()
}
fn nt(facts: &EntityFacts) -> codec::Notetype {
    let Payload::Notetype(n) = &facts.payload else { panic!("type required") };
    n.clone()
}
fn row(host: &CoreHost, payload: &Payload, authority: &codec::Notetype) -> Payload {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let kind = payload.kind();
    let key = cloud::row_payload::payload_key(payload).unwrap();
    let id = i64::from_be_bytes(key.try_into().unwrap());
    let head = snapshot.row_image(cloud::row_payload::table(kind).unwrap(), &RowKey::Id(id)).unwrap().unwrap();
    cloud::row_payload::project_ordinary(&cloud::row_payload::ReadView(&*snapshot), kind, &head, |id| {
        assert_eq!(id, authority.id);
        Ok(authority.clone())
    }).unwrap()
}
fn free_type(host: &CoreHost, source: &EntityFacts, below: bool) -> codec::Notetype {
    let mut n = nt(source);
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Cloud).unwrap();
    let mut id = if below { 1 } else { n.id + 100 };
    while lease.row_image(Table::Notetypes, &RowKey::Id(id)).unwrap().is_some() { id += 1; }
    if below { assert!(id < n.id); }
    n.id = id;
    n.struct_gen = 37;
    n.name = b"Ordinary received type".to_vec();
    n.config.extend_from_slice(&[0x98, 6, 7]);
    n.fields[0].config.extend_from_slice(&[0x98, 6, 9]);
    n.templates[0].config.extend_from_slice(&[0x98, 6, 11]);
    n
}
fn save_server(
    service: &mut Service<SeededRandom>, binding: &cloud::Binding, payload: &Payload,
    origin: [u8; 16], generation: Option<[u8; 16]>,
) -> Saved {
    let kind = payload.kind();
    let key = cloud::row_payload::payload_key(payload).unwrap();
    let name = codec::entity_name(&binding.facts.record_key.0, kind, &key).unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let prior = service.inspect(&scope()).unwrap().unwrap().zones[&zone].records.get(&name).cloned();
    let erev = prior.as_ref().map_or(1, |r| {
        let Field::EncryptedBytes(bytes) = &r.fields["e"] else { panic!("envelope") };
        let codec::DecodeOutcome::Supported(e) = codec::Entity::decode(bytes).unwrap() else { panic!("supported") };
        e.fields.erev + 1
    });
    let envelope = codec::Entity::build(codec::EntityFields {
        kind, key, origin, erev, generation, tombstone: false, deleted_at_ms: 0,
        writer: [91; 16], base_sha256: None,
    }, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let result = service.request(&scope(), Caller::default(), Operation::Modify {
        zone, atomic: false, changes: vec![Mutation::Save {
            record: RecordInput::encrypted(&name, "MGEntity", envelope),
            if_unchanged: prior.map(|r| r.system_fields),
        }],
    }).unwrap().result;
    let Ok(Reply::Modified(mut results)) = result else { panic!("modify required") };
    let Ok(MutationSuccess::Saved(record)) = results.remove(0).result else { panic!("save required") };
    let Field::EncryptedBytes(envelope) = &record.fields["e"] else { panic!("envelope required") };
    Saved { name, envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() }
}
fn start(host: &CoreHost) -> String {
    let runtime = host.cloud_runtime().unwrap();
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    instance
}
fn ack(host: &CoreHost, binding: &cloud::Binding, saved: &Saved, payload: &Payload) -> EntityFacts {
    let runtime = host.cloud_runtime().unwrap();
    let e = runtime.entity_facts(&saved.name).unwrap().unwrap();
    assert_eq!(e.acked_payload.as_ref(), Some(payload));
    assert_eq!(e.acked_envelope.0, saved.envelope);
    assert_eq!(e.acked_system_fields, saved.system_fields);
    assert_eq!(e.acked_row_digest, payload.row_digest().unwrap());
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert!(runtime.inbox_entries(binding.id).unwrap().is_empty());
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    e
}

#[test]
fn ordinary_notetype_free_id_and_dependent_rows_use_server_counter_after_real_wal_coverage() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(449)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let received = free_type(&host, &source, false);
    let mut payloads = vec![Payload::Notetype(received.clone())];
    let Payload::Note(mut note) = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0).payload else { panic!("note") };
    let old_note = note.id;
    note.id += 100;
    note.guid = b"ordinary-new-note".to_vec();
    note.mid = received.id;
    note.nt_struct_gen = received.struct_gen;
    let Payload::Card(mut card) = runtime.entity_facts_page(codec::Kind::Card, None).unwrap().into_iter()
        .find(|e| matches!(&e.payload, Payload::Card(c) if c.nid == old_note)).unwrap().payload else { panic!("card") };
    let source_card = CardId(card.id);
    card.id += 100;
    card.nid = note.id;
    card.nt_struct_gen = received.struct_gen;
    {
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        assert_eq!(card.last_review_id, snapshot.max_revlog_id(source_card).unwrap());
        assert_ne!(card.last_review_id, 0, "source card has review history");
        assert!(snapshot.row_image(Table::Cards, &RowKey::Id(card.id)).unwrap().is_none());
        let mut after = None;
        loop {
            let page = snapshot.row_page(Table::Revlog, after.as_ref()).unwrap();
            if page.is_empty() { break; }
            for review in page {
                assert_ne!(cloud::row_payload::int(&review, 1).unwrap(), card.id, "new card has no history");
                after = Some(RowKey::Id(cloud::row_payload::int(&review, 0).unwrap()));
            }
        }
        // Projection derives this value from revlog for the new ID, not the copied card.
        let last_review_id = snapshot.max_revlog_id(CardId(card.id)).unwrap();
        assert_eq!(last_review_id, 0, "canonical history value when no revlog exists");
        assert_ne!(card.last_review_id, last_review_id, "copied history belongs to the source ID");
        card.last_review_id = last_review_id;
    }
    payloads.extend([Payload::Note(note), Payload::Card(card)]);
    let saved: Vec<_> = payloads.iter().map(|p| save_server(&mut service, &binding, p, [92; 16], None)).collect();
    let instance = start(&host);
    runtime.receive_collection_records(&instance, &saved).unwrap();
    collection.pause_automatic_wal_flush_for_testing(true);
    let weak = Arc::downgrade(runtime);
    let type_name = saved[0].name.clone();
    let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_commit = committed.clone();
    let flushed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_flush = flushed.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if !matches!(stage, IncomingStage::ApplyAfterCommit | IncomingStage::ApplyAfterFlush) { return; }
        let runtime = weak.upgrade().unwrap();
        assert!(runtime.entity_facts(&type_name).unwrap().is_none());
        assert!(!runtime.status().unwrap().tracking_stopped);
        let intents = runtime.apply_intents(binding.id).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].keys.len(), 3);
        assert!(intents[0].keys.iter().all(|k| k.after_digest.is_some()));
        if stage == IncomingStage::ApplyAfterCommit {
            assert!(runtime.incoming_wal_receipts().is_empty());
            saw_commit.store(true, std::sync::atomic::Ordering::SeqCst);
        } else {
            assert!(!runtime.incoming_wal_receipts().is_empty());
            saw_flush.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    collection.pause_automatic_wal_flush_for_testing(false);
    assert!(committed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(flushed.load(std::sync::atomic::Ordering::SeqCst));
    for (saved, payload) in saved.iter().zip(&payloads) {
        let e = ack(&host, &binding, saved, payload);
        if let (Payload::Card(actual), Payload::Card(expected)) = (&e.payload, payload) {
            assert_eq!(actual.last_review_id, expected.last_review_id, "projected card last_review_id");
        }
        assert_eq!(e.payload, *payload);
        assert!(e.mark.is_none());
        assert_eq!(row(&host, payload, &received), *payload);
    }
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys());
    for (saved, payload) in saved.iter().zip(&payloads) {
        ack(&reopened, &binding, saved, payload);
        assert_eq!(row(&reopened, payload, &received), *payload);
    }
    reopened.close().unwrap();
}

#[test]
fn ordinary_notetype_disjoint_parts_merge_raw_blobs_and_name_lists() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(450)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let before = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(nt(&source).id)).unwrap().unwrap();
    let mut draft = meguri_engine::notetypes::Draft::of(before);
    draft.config.css = "ordinary local styling".into();
    draft.fields[0].config.sticky = true;
    meguri_engine::notetypes::save(&collection, draft, Default::default()).unwrap();
    let local = runtime.entity_facts(&source.name).unwrap().unwrap();
    assert!(local.mark.is_some());
    assert_eq!(local.base_payload, source.acked_payload);
    let mut server = nt(&source);
    server.name = b"Ordinary renamed type".to_vec();
    server.fields[0].name = b"Ordinary field".to_vec();
    server.templates[0].name = b"Ordinary template".to_vec();
    let mut config = TemplateConfig::decode(&server.templates[0].config).unwrap();
    config.a_format = "ordinary server answer".into();
    server.templates[0].config = config.encode();
    server.templates[0].config.extend_from_slice(&[0x98, 6, 13]);
    server.templates[0].mtime_secs = 123;
    let saved = save_server(&mut service, &binding, &Payload::Notetype(server.clone()), source.origin, None);
    runtime.receive_collection_records(&start(&host), std::slice::from_ref(&saved)).unwrap();
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    let e = ack(&host, &binding, &saved, &Payload::Notetype(server.clone()));
    let merged = nt(&e);
    assert_eq!(merged.name, server.name);
    assert_eq!(merged.fields[0].name, server.fields[0].name);
    assert_eq!(merged.templates, server.templates);
    assert_eq!(merged.config, nt(&local).config);
    assert_eq!(merged.fields[0].config, nt(&local).fields[0].config);
    assert_eq!(e.base_payload, Some(Payload::Notetype(server)));
    assert!(e.mark.is_some());
    assert_eq!(host.local_publication_information(collection.generation()).unwrap().tracking.unwrap().unresolved_conflict_copies, 0);
    assert_eq!(row(&host, &e.payload, &merged), e.payload);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap().payload, e.payload);
    host.close().unwrap();
}

fn closed_copies(dir: &Path) -> Vec<cloud::StoredConflict> {
    cloud::runtime::with_tracking_access(|| {
        let cloud::OpenOutcome::Opened(store) = cloud::TrackingStore::open(dir, cloud::BuildScope {
            container: cloud::ContainerId::new(scope().container).unwrap(),
            environment: cloud::Environment::Development,
        }).unwrap() else { panic!("tracking required") };
        store.conflict_copies().unwrap()
    })
}

#[test]
fn ordinary_notetype_conflict_uses_actual_stamp_digest_order_and_one_full_copy() {
    for offset in [-1, 0, 1] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(451)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let source = source_type(&host);
        let before = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(nt(&source).id)).unwrap().unwrap();
        let mut draft = meguri_engine::notetypes::Draft::of(before);
        draft.name = "Ordinary local name".into();
        draft.config.css = "ordinary local conflict".into();
        meguri_engine::notetypes::save(&collection, draft, Default::default()).unwrap();
        let local = runtime.entity_facts(&source.name).unwrap().unwrap();
        let mut server = nt(&source);
        server.name = b"Ordinary server name".to_vec();
        let mut config = NotetypeConfig::decode(&server.config).unwrap();
        config.css = "ordinary server conflict".into();
        server.config = config.encode();
        server.mtime_secs = nt(&local).mtime_secs + offset;
        let payload = Payload::Notetype(server.clone());
        let later_local = (nt(&local).mtime_secs, local.payload.payload_digest().unwrap())
            > (server.mtime_secs, payload.payload_digest().unwrap());
        let saved = save_server(&mut service, &binding, &payload, source.origin, None);
        let instance = start(&host);
        runtime.receive_collection_records(&instance, std::slice::from_ref(&saved)).unwrap();
        runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
        let applied = ack(&host, &binding, &saved, &payload);
        let expected = if later_local { nt(&local) } else { server.clone() };
        assert_eq!(nt(&applied).name, expected.name);
        assert_eq!(nt(&applied).config, expected.config);
        assert_eq!(row(&host, &applied.payload, &expected), applied.payload);
        // Duplicate intake cannot create another copy or rerun a settled op.
        let runs = runtime.incoming_runs();
        runtime.receive_collection_records(&instance, std::slice::from_ref(&saved)).unwrap();
        runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
        assert_eq!(runtime.incoming_runs(), runs);
        host.close().unwrap();
        drop(collection);
        drop(host);
        let copies = closed_copies(&dir);
        assert_eq!(copies.len(), 1);
        let copy = &copies[0].copy;
        assert_eq!(copy.reason, cloud::ConflictReason::NotetypeConflict);
        let losing = if later_local { &payload } else { &local.payload };
        assert_eq!(copy.losing_payload_digest.0, losing.payload_digest().unwrap());
        let codec::DecodeOutcome::Supported(envelope) = codec::Entity::decode(&copy.losing_envelope.0).unwrap() else { panic!("copy envelope") };
        assert_eq!(Payload::decode(codec::Kind::Notetype, envelope.payload.as_ref().unwrap()).unwrap(), *losing);
        assert!(copy.field_names.is_some() && copy.template_names.is_some());
    }
}

#[test]
fn ordinary_notetype_name_uniqueness_protects_larger_existing_type_and_its_base() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(452)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let mut received = free_type(&host, &source, true);
    received.name = nt(&source).name;
    let payload = Payload::Notetype(received.clone());
    let saved = save_server(&mut service, &binding, &payload, [92; 16], None);
    runtime.receive_collection_records(&start(&host), std::slice::from_ref(&saved)).unwrap();
    let weak = Arc::downgrade(runtime);
    let captured = source.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage != IncomingStage::ApplyAfterCommit { return; }
        let runtime = weak.upgrade().unwrap();
        assert!(runtime.entity_facts(&captured.name).unwrap().unwrap() == captured);
        let intents = runtime.apply_intents(binding.id).unwrap();
        assert_eq!(intents.len(), 1);
        let side = intents[0].keys.iter().find(|k| k.name == captured.name).unwrap();
        assert_eq!(side.role, cloud::incoming::ApplyKeyRole::SideEffect);
        assert_eq!(side.before_payload.as_ref(), Some(&captured.payload));
        assert!(!runtime.status().unwrap().tracking_stopped);
    });
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    let applied = ack(&host, &binding, &saved, &payload);
    assert_eq!(applied.payload, payload);
    let changed = runtime.entity_facts(&source.name).unwrap().unwrap();
    assert_eq!(nt(&changed).name, [nt(&source).name, b"+".to_vec()].concat());
    assert_eq!(changed.base_payload.as_ref(), Some(&source.payload));
    assert_eq!(changed.acked_payload, source.acked_payload);
    assert_eq!(changed.in_flight, source.in_flight);
    assert!(changed.mark.is_some());
    assert_eq!(row(&host, &changed.payload, &nt(&changed)), changed.payload);
    assert_eq!(row(&host, &payload, &received), payload);
    host.close().unwrap();
}

#[test]
fn ordinary_notetype_structural_generation_and_malformed_shapes_stay_queued_without_ack() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(453)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let original = nt(&source);
    let instance = start(&host);
    for case in 0..9 {
        let mut server = original.clone();
        let mut generation = None;
        match case {
            0 => server.struct_gen += 1,
            1 => { server.fields.pop(); },
            2 => server.fields[1].ord += 1,
            3 => {
                let mut config = NotetypeConfig::decode(&server.config).unwrap();
                config.kind = 1;
                server.config = config.encode();
            }
            4 => {
                let mut config = NotetypeConfig::decode(&server.config).unwrap();
                config.sort_field_idx = 1;
                server.config = config.encode();
            }
            5 => server.fields[0].config = vec![0xff],
            6 => server.fields[1].name = server.fields[0].name.clone(),
            7 => generation = Some([93; 16]),
            _ => {
                server.fields.swap(0, 1);
                server.fields[0].ord = 0;
                server.fields[1].ord = 1;
            }
        }
        let saved = save_server(&mut service, &binding, &Payload::Notetype(server), source.origin, generation);
        runtime.receive_collection_records(&instance, &[saved]).unwrap();
        runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
        assert!(runtime.entity_facts(&source.name).unwrap().unwrap() == source);
        let inbox = runtime.inbox_entries(binding.id).unwrap();
        assert_eq!(inbox.len(), 1);
        assert!(matches!(inbox[0].entry.reason, cloud::InboxReason::Structure | cloud::InboxReason::Generation));
        assert_eq!(inbox[0].entry.retry_count, 0);
        assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
        assert_eq!(row(&host, &source.payload, &original), source.payload);
        assert!(!runtime.status().unwrap().tracking_stopped);
    }
    host.close().unwrap();
}

#[test]
fn ordinary_notetype_crash_worker() {
    use meguri_model::faultpoint::{self, Fault};
    let Some(root) = std::env::var_os("MEGURI_ORDINARY_TYPE_CRASH_ROOT") else { return; };
    let root = std::path::PathBuf::from(root);
    let point = std::env::var("MEGURI_ORDINARY_TYPE_CRASH_POINT").unwrap();
    let dir = root.join("device");
    let host = open_host(&dir, keys());
    let mut service = Service::open(&root, Config::default(), SeededRandom::new(454)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let received = free_type(&host, &source_type(&host), false);
    let saved = save_server(&mut service, &binding, &Payload::Notetype(received), [92; 16], None);
    runtime.receive_collection_records(&start(&host), &[saved]).unwrap();
    faultpoint::arm(&point, Fault::Abort);
    runtime.drain_incoming(&host.collection().unwrap(), Duration::from_secs(30)).unwrap();
    panic!("actual ordinary apply crash boundary not reached");
}

#[test]
fn ordinary_notetype_crash_reopen_replays_intent_or_settles_actual_rows_before_scan() {
    use std::os::unix::process::ExitStatusExt;
    for point in ["cloud.apply.after_intent", "cloud.apply.after_commit", "cloud.apply.after_flush"] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let status = std::process::Command::new(std::env::current_exe().unwrap())
            .args(["--exact", "incoming_notetype::ordinary_notetype_crash_worker", "--nocapture"])
            .env("MEGURI_ORDINARY_TYPE_CRASH_ROOT", temp.path())
            .env("MEGURI_ORDINARY_TYPE_CRASH_POINT", point).status().unwrap();
        assert_eq!(status.signal(), Some(6));
        let service = Service::open(temp.path(), Config::default(), SeededRandom::new(454)).unwrap();
        let host = open_host(&dir, keys());
        let runtime = host.cloud_runtime().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap();
        let database = service.inspect(&scope()).unwrap().unwrap();
        let record = database.zones[&cloud::adapter::collection_zone(binding.facts.epoch)].records.values().find(|record| {
            let Some(Field::EncryptedBytes(bytes)) = record.fields.get("e") else { return false; };
            matches!(codec::Entity::decode(bytes), Ok(codec::DecodeOutcome::Supported(e)) if e.fields.writer == [91; 16] && e.fields.kind == codec::Kind::Notetype)
        }).unwrap();
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else { panic!("entity") };
        let received = codec::receive(&binding.facts.record_key.0, &record.name, envelope, codec::AssetInput::Absent).unwrap();
        let payload = received.payload.unwrap();
        let saved = Saved { name: record.name.clone(), envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() };
        runtime.drain_incoming(&host.collection().unwrap(), Duration::from_secs(30)).unwrap();
        ack(&host, &binding, &saved, &payload);
        let Payload::Notetype(nt) = &payload else { panic!("type") };
        assert_eq!(nt.struct_gen, 37);
        assert_eq!(row(&host, &payload, nt), payload);
        assert!(!runtime.incoming_wal_receipts().is_empty());
        host.close().unwrap();
    }
}

fn saved_type_name(binding: &cloud::Binding, nt: &codec::Notetype) -> String {
    codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(nt.id)).unwrap()
}

#[test]
fn ordinary_notetype_free_id_later_native_edit_keeps_base_proof_and_dependent_rows() {
    let temp = tempfile::tempdir().unwrap();
    edit_during_received_type_commit(temp.path(), false);
}

fn edit_during_received_type_commit(root: &Path, crash: bool) {
    let dir = root.join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(root, Config::default(), SeededRandom::new(449)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let received = free_type(&host, &source, false);
    let mut payloads = vec![Payload::Notetype(received.clone())];
    let Payload::Note(mut note) = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0).payload else { panic!("note") };
    let old_note = note.id;
    note.id += 100;
    note.guid = b"ordinary-new-note".to_vec();
    note.mid = received.id;
    note.nt_struct_gen = received.struct_gen;
    let Payload::Card(mut card) = runtime.entity_facts_page(codec::Kind::Card, None).unwrap().into_iter()
        .find(|e| matches!(&e.payload, Payload::Card(c) if c.nid == old_note)).unwrap().payload else { panic!("card") };
    let source_card = CardId(card.id);
    card.id += 100;
    card.nid = note.id;
    card.nt_struct_gen = received.struct_gen;
    {
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        assert_eq!(card.last_review_id, snapshot.max_revlog_id(source_card).unwrap());
        assert_ne!(card.last_review_id, 0, "source card has review history");
        assert!(snapshot.row_image(Table::Cards, &RowKey::Id(card.id)).unwrap().is_none());
        let mut after = None;
        loop {
            let page = snapshot.row_page(Table::Revlog, after.as_ref()).unwrap();
            if page.is_empty() { break; }
            for review in page {
                assert_ne!(cloud::row_payload::int(&review, 1).unwrap(), card.id, "new card has no history");
                after = Some(RowKey::Id(cloud::row_payload::int(&review, 0).unwrap()));
            }
        }
        // Projection derives this value from revlog for the new ID, not the copied card.
        let last_review_id = snapshot.max_revlog_id(CardId(card.id)).unwrap();
        assert_eq!(last_review_id, 0, "canonical history value when no revlog exists");
        assert_ne!(card.last_review_id, last_review_id, "copied history belongs to the source ID");
        card.last_review_id = last_review_id;
    }
    payloads.extend([Payload::Note(note), Payload::Card(card)]);
    let saved: Vec<_> = payloads.iter().map(|p| save_server(&mut service, &binding, p, [92; 16], None)).collect();
    let instance = start(&host);
    runtime.receive_collection_records(&instance, &saved).unwrap();
    collection.pause_automatic_wal_flush_for_testing(true);
    let weak = Arc::downgrade(runtime);
    let type_name = saved[0].name.clone();
    let committed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_commit = committed.clone();
    let flushed = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let saw_flush = flushed.clone();
    let edit_collection = collection.clone();
    let edit_id = received.id;
    let proof = Arc::new(Mutex::new(None));
    let captured_proof = proof.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if !matches!(stage, IncomingStage::ApplyAfterCommit | IncomingStage::ApplyAfterFlush) { return; }
        let runtime = weak.upgrade().unwrap();
        if stage == IncomingStage::ApplyAfterCommit {
            assert!(runtime.entity_facts(&type_name).unwrap().is_none());
        } else {
            let local = runtime.entity_facts(&type_name).unwrap().unwrap();
            assert!(local.acked_payload.is_none() && local.acked_envelope.0.is_empty());
        }
        assert!(!runtime.status().unwrap().tracking_stopped);
        let intents = runtime.apply_intents(binding.id).unwrap();
        assert_eq!(intents.len(), 1);
        assert_eq!(intents[0].keys.len(), 3);
        assert!(intents[0].keys.iter().all(|k| k.after_digest.is_some()));
        if stage == IncomingStage::ApplyAfterCommit {
            assert!(runtime.incoming_wal_receipts().is_empty());
            let original = intents[0].clone();
            *captured_proof.lock().unwrap() = Some(runtime.incoming_runs());
            let before = edit_collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(edit_id)).unwrap().unwrap();
            let mut draft = meguri_engine::notetypes::Draft::of(before);
            draft.name = "Saved during incoming apply".into();
            draft.config.css = ".card { color: #243648; }".into();
            meguri_engine::notetypes::save(&edit_collection, draft, Default::default()).unwrap();
            assert_eq!(runtime.apply_intents(binding.id).unwrap(), vec![original]);
            let local = runtime.entity_facts(&type_name).unwrap().unwrap();
            assert!(local.mark.is_some() && local.acked_payload.is_none());
            assert!(runtime.incoming_wal_receipts().is_empty());
            assert!(!runtime.status().unwrap().tracking_stopped);
            if crash { std::process::abort(); }
            saw_commit.store(true, std::sync::atomic::Ordering::SeqCst);
        } else {
            assert!(!runtime.incoming_wal_receipts().is_empty());
            saw_flush.store(true, std::sync::atomic::Ordering::SeqCst);
        }
    });
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    runtime.set_incoming_stage_hook_for_testing(|_| {});
    collection.pause_automatic_wal_flush_for_testing(false);
    assert!(committed.load(std::sync::atomic::Ordering::SeqCst));
    assert!(flushed.load(std::sync::atomic::Ordering::SeqCst));
    assert_eq!(Some(runtime.incoming_runs()), *proof.lock().unwrap());
    for (saved, payload) in saved.iter().zip(&payloads) {
        let e = ack(&host, &binding, saved, payload);
        if let (Payload::Card(actual), Payload::Card(expected)) = (&e.payload, payload) {
            assert_eq!(actual.last_review_id, expected.last_review_id, "projected card last_review_id");
        }
        if payload.kind() == codec::Kind::Notetype {
            let actual = nt(&e);
            assert_eq!(actual.name, b"Saved during incoming apply");
            assert_eq!(NotetypeConfig::decode(&actual.config).unwrap().css, ".card { color: #243648; }");
            assert_eq!(actual.struct_gen, received.struct_gen);
            assert_eq!(e.base_payload, Some(payload.clone()));
            assert!(e.mark.is_some());
            assert_eq!(row(&host, &e.payload, &actual), e.payload);
        } else {
            assert_eq!(e.payload, *payload);
            assert!(e.mark.is_none());
            let actual = nt(&runtime.entity_facts(&saved_type_name(&binding, &received)).unwrap().unwrap());
            assert_eq!(row(&host, payload, &actual), *payload);
        }
    }
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys());
    for (saved, payload) in saved.iter().zip(&payloads) {
        ack(&reopened, &binding, saved, payload);
        let actual = nt(&reopened.cloud_runtime().unwrap().entity_facts(&saved_type_name(&binding, &received)).unwrap().unwrap());
        let retained = reopened.cloud_runtime().unwrap().entity_facts(&saved.name).unwrap().unwrap();
        assert_eq!(row(&reopened, &retained.payload, &actual), retained.payload);
        if payload.kind() != codec::Kind::Notetype { assert_eq!(retained.payload, *payload); }
    }
    reopened.close().unwrap();
}

#[test]
fn ordinary_notetype_later_edit_crash_worker() {
    let Some(root) = std::env::var_os("MEGURI_TYPE_LATER_EDIT_CRASH_ROOT") else { return; };
    edit_during_received_type_commit(&std::path::PathBuf::from(root), true);
}

#[test]
fn ordinary_notetype_later_edit_crash_reopen_settles_original_intent_and_remote_base() {
    use std::os::unix::process::ExitStatusExt;
    let temp = tempfile::tempdir().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "incoming_notetype::ordinary_notetype_later_edit_crash_worker", "--nocapture"])
        .env("MEGURI_TYPE_LATER_EDIT_CRASH_ROOT", temp.path()).status().unwrap();
    assert_eq!(status.signal(), Some(6));
    let dir = temp.path().join("device");
    let service = Service::open(temp.path(), Config::default(), SeededRandom::new(449)).unwrap();
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let local_type = runtime.entity_facts_page(codec::Kind::Notetype, None).unwrap().into_iter()
        .find(|e| nt(e).name == b"Saved during incoming apply").unwrap();
    let native = nt(&local_type);
    assert_eq!(native.struct_gen, 37);
    assert_eq!(NotetypeConfig::decode(&native.config).unwrap().css, ".card { color: #243648; }");
    runtime.drain_incoming(&host.collection().unwrap(), Duration::from_secs(30)).unwrap();
    assert!(runtime.incoming_runs().is_empty(), "settle the original apply without a replacement writer");
    assert!(!runtime.incoming_wal_receipts().is_empty());
    let database = service.inspect(&scope()).unwrap().unwrap();
    let mut checked = 0;
    for record in database.zones[&cloud::adapter::collection_zone(binding.facts.epoch)].records.values() {
        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else { continue; };
        let Ok(codec::DecodeOutcome::Supported(entity)) = codec::Entity::decode(envelope) else { continue; };
        if entity.fields.writer != [91; 16] { continue; }
        let payload = codec::receive(&binding.facts.record_key.0, &record.name, envelope, codec::AssetInput::Absent).unwrap().payload.unwrap();
        let saved = Saved { name: record.name.clone(), envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() };
        let actual = ack(&host, &binding, &saved, &payload);
        if payload.kind() == codec::Kind::Notetype {
            assert_eq!(actual.payload, local_type.payload);
            assert_eq!(actual.base_payload, Some(payload));
            assert!(actual.mark.is_some());
        } else { assert_eq!(actual.payload, payload); }
        assert_eq!(row(&host, &actual.payload, &native), actual.payload);
        checked += 1;
    }
    assert_eq!(checked, 3);
    host.close().unwrap();
    drop(host);
    let reopened = open_host(&dir, keys());
    let retained = reopened.cloud_runtime().unwrap().entity_facts(&local_type.name).unwrap().unwrap();
    assert_eq!(retained.payload, local_type.payload);
    assert_eq!(row(&reopened, &retained.payload, &native), retained.payload);
    reopened.close().unwrap();
}

fn generated_legacy_reversed(dir: &Path) {
    use meguri_storage::raw::{PlainDatabase, RawValue};
    let recipe = Recipe::parse("recipe: legacy-layout-origins\ndescription: Public synthetic legacy layout\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Legacy\n    notetype: reversed\n    new: 2\n    due: 0\n    fields: [\"question {n}\", \"answer {n}\"]\n").unwrap();
    generate_recipe(&recipe, 91, dir).unwrap();
    let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
    for (table, column) in [("fields", "config"), ("templates", "config")] {
        let rows = db.rows(&format!("SELECT ntid,ord,{column} FROM {table}"), &[]).unwrap();
        for row in rows {
            let RawValue::Blob(bytes) = &row[2] else { panic!("config") };
            let bytes = if table == "fields" {
                let mut config = FieldConfig::decode(bytes).unwrap();
                config.id = None;
                config.encode()
            } else {
                let mut config = TemplateConfig::decode(bytes).unwrap();
                config.id = None;
                config.encode()
            };
            db.execute(&format!("UPDATE {table} SET {column}=?3 WHERE ntid=?1 AND ord=?2"),
                &[row[0].clone(), row[1].clone(), RawValue::Blob(bytes)]).unwrap();
        }
    }
    db.close().unwrap();
}

fn bookkeeping(host: &CoreHost) -> (meguri_model::time::EpochMillis, meguri_model::time::EpochMillis,
    Option<meguri_engine::device_state::LastBackup>) {
    let collection = host.collection().unwrap();
    let col = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let backup = meguri_engine::device_state::DeviceState::for_collection(collection.path()).last_backup().unwrap();
    (col.schema_modified, col.last_sync, backup)
}

fn prove_reopened_binding(host: &CoreHost, binding: &cloud::Binding) {
    let runtime = host.cloud_runtime().unwrap();
    assert!(!runtime.status().unwrap().identity_verified);
    // A reopened Runtime needs its own accepted account read before reconciliation.
    let request = runtime.adapter_begin_identity_read(false, None).unwrap();
    assert!(runtime.adapter_complete_identity_read(request,
        IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()))).unwrap());
    let status = runtime.status().unwrap();
    assert_eq!(status.binding.as_ref(), Some(binding));
    assert!(status.identity_verified && status.stored_usable);
}

#[test]
fn ordinary_notetype_legacy_external_template_reorder_reopen_holds_counter_and_dependents() {
    validated_layout_choice_case(false);
}

#[test]
fn ordinary_notetype_empty_local_same_epoch_refusal_uses_choice_with_unknown_own_work() {
    validated_layout_choice_case(true);
}

fn validated_layout_choice_case(empty: bool) {
    use meguri_storage::raw::{PlainDatabase, RawValue};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    if empty {
        let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
        db.execute_batch("DELETE FROM revlog; DELETE FROM cards; DELETE FROM notes; DELETE FROM decks WHERE id<>1").unwrap();
        db.close().unwrap();
    }
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(455)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = if empty {
        host.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Notetype, None).unwrap().into_iter()
            .find(|e| matches!(&e.payload, Payload::Notetype(n) if n.templates.len() == 2)).unwrap()
    } else { source_type(&host) };
    let original = nt(&source);
    assert_eq!(original.templates.len(), 2);
    assert!(original.templates.iter().all(|t| TemplateConfig::decode(&t.config).unwrap().id.is_none()));
    let before = bookkeeping(&host);
    let cards = host.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Card, None).unwrap();
    let notes = host.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Note, None).unwrap();
    host.close().unwrap();
    drop(host);
    let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE").unwrap();
    db.execute("UPDATE templates SET ord=-ord-1 WHERE ntid=?1", &[RawValue::Integer(original.id)]).unwrap();
    db.execute("UPDATE templates SET ord=ord+2 WHERE ntid=?1", &[RawValue::Integer(original.id)]).unwrap();
    db.execute("UPDATE cards SET ord=1-ord WHERE nid IN (SELECT id FROM notes WHERE mid=?1)", &[RawValue::Integer(original.id)]).unwrap();
    db.execute("UPDATE col SET mod=mod+99", &[]).unwrap();
    db.execute_batch("COMMIT").unwrap();
    db.close().unwrap();
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    prove_reopened_binding(&host, &binding);
    assert_eq!(bookkeeping(&host), before);
    // Exercise the Host entry used by CloudWorkflow.prepare_runtime before any
    // explicit failed reconciliation. SDK registration stays closed afterward.
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    assert!(runtime.register_sdk(true).is_err());
    let refusal = runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap_err();
    assert!(matches!(refusal, cloud::runtime::RuntimeError::Tracking(
        cloud::Error::ValidatedUnprovedLayout { notetype }) if notetype == original.id));
    runtime.prepare_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert!(matches!(host.engine().state(), meguri_engine::LifecycleState::Ready { .. }));
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert!(runtime.require_collection_sending().is_err());
    let projection = runtime.policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0).unwrap();
    assert_eq!(projection.facts.fence, cloud::policy::Fence::ChooseCollection);
    let p = actual_preview(&host, &mut service, &scope());
    assert!(p.decision == ConnectDecision::ChooseCollection);
    assert_eq!(p.active_epoch, Some(binding.facts.epoch));
    assert_eq!(p.local.empty, empty);
    assert!(!p.unsent_counts_exact && p.resume_binding.is_none());
    assert_eq!(p.local.counts[&codec::Kind::Note], if empty { 0 } else { notes.len() as u64 });
    let lower_bounds = p.unsent_by_kind.clone();
    runtime.cancel_connection_preview_id(p.id).unwrap();
    let again = actual_preview(&host, &mut service, &scope());
    assert!(again.decision == ConnectDecision::ChooseCollection && !again.unsent_counts_exact);
    assert_eq!(again.unsent_by_kind, lower_bounds);
    runtime.cancel_connection_preview_id(again.id).unwrap();
    assert_eq!(bookkeeping(&host), before);
    assert!(runtime.entity_facts(&source.name).unwrap().unwrap() == source);
    assert_eq!(runtime.entity_facts_page(codec::Kind::Card, None).unwrap(), cards);
    assert_eq!(runtime.entity_facts_page(codec::Kind::Note, None).unwrap(), notes);
    assert!(runtime.status().unwrap().scan_required);
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    // The exact current image cannot be projected under the retained layout.
    let lease = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let head = snapshot.row_image(Table::Notetypes, &RowKey::Id(original.id)).unwrap().unwrap();
    let actual = cloud::row_payload::notetype(&cloud::row_payload::ReadView(&*snapshot), &head, original.struct_gen).unwrap();
    assert_eq!(actual.templates[0].name, original.templates[1].name);
    assert_eq!(actual.templates[1].name, original.templates[0].name);
    assert!(matches!(cloud::row_payload::project_ordinary(&cloud::row_payload::ReadView(&*snapshot), codec::Kind::Notetype, &head,
        |_| Ok(original.clone())), Err(cloud::Error::ValidatedUnprovedLayout { notetype }) if notetype == original.id));
    let mut invalid = head.clone();
    invalid[4] = meguri_storage::Cell::Blob(vec![0xff]);
    assert!(matches!(cloud::row_payload::project_ordinary(&cloud::row_payload::ReadView(&*snapshot), codec::Kind::Notetype, &invalid,
        |_| Ok(original.clone())), Err(cloud::Error::RowFailure(_))));
    let mut unsupported = NotetypeConfig::decode(&actual.config).unwrap();
    unsupported.kind = 2;
    invalid[4] = meguri_storage::Cell::Blob(unsupported.encode());
    assert!(matches!(cloud::row_payload::project_ordinary(&cloud::row_payload::ReadView(&*snapshot), codec::Kind::Notetype, &invalid,
        |_| Ok(original.clone())), Err(cloud::Error::InvalidData)));
    drop(snapshot);
    drop(lease);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys());
    prove_reopened_binding(&reopened, &binding);
    assert_eq!(bookkeeping(&reopened), before);
    // Preview itself reconstructs the decision before a preparation/scan call.
    let p = actual_preview(&reopened, &mut service, &scope());
    assert!(p.decision == ConnectDecision::ChooseCollection && !p.unsent_counts_exact);
    assert_eq!(p.unsent_by_kind, lower_bounds);
    assert!(p.resume_binding.is_none());
    let error = reopened.cloud_runtime().unwrap().reconcile_binding(&reopened.collection().unwrap(),
        &reopened.media().unwrap(), Duration::from_secs(30)).unwrap_err();
    assert!(matches!(error, cloud::runtime::RuntimeError::Tracking(cloud::Error::ValidatedUnprovedLayout { .. })));
    assert!(reopened.cloud_runtime().unwrap().entity_facts(&source.name).unwrap().unwrap() == source);
    let generation = reopened.cloud_connection_generation().unwrap();
    reopened.keep_cloud_local_only(generation, Some(&p)).unwrap();
    let stopped = reopened.cloud_runtime().unwrap().status().unwrap();
    assert!(!stopped.stored_usable && !stopped.identity_verified);
    assert_eq!(bookkeeping(&reopened), before);
    assert_eq!(reopened.cloud_runtime().unwrap().entity_facts(&source.name).unwrap().unwrap(), source);
    assert_eq!(reopened.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Note, None).unwrap(), notes);
    assert_eq!(reopened.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Card, None).unwrap(), cards);
    assert!(matches!(meguri_engine::device_state::DeviceState::for_collection(&dir.join("collection.anki2"))
        .publication_decision().unwrap().unwrap().publication.binding, meguri_engine::device_state::AccountBinding::LocalOnly));
    let reconnect = actual_preview(&reopened, &mut service, &scope());
    assert!(reconnect.decision == ConnectDecision::ChooseCollection && !reconnect.unsent_counts_exact);
    assert!(reopened.cloud_runtime().unwrap().register_sdk(true).is_err());
    reopened.close().unwrap();
}

#[test]
fn ordinary_notetype_native_legacy_field_and_template_name_swaps_keep_layout_and_bookkeeping() {
    use meguri_engine::notetypes::{self, AggregateDigest, FieldIdentity, FieldRename, RenameFields};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated_legacy_reversed(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(456)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let source = source_type(&host);
    let original = nt(&source);
    let before = bookkeeping(&host);
    let base = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(original.id)).unwrap().unwrap();
    let request = RenameFields {
        notetype: base.id, base: AggregateDigest::of(&base),
        fields: vec![FieldRename { field: FieldIdentity::of(&base.fields[0]), name: base.fields[1].name.clone() },
                     FieldRename { field: FieldIdentity::of(&base.fields[1]), name: base.fields[0].name.clone() }],
    };
    notetypes::rename_fields(&collection, request, meguri_engine::CancelToken::new(), meguri_engine::ProgressSink::new()).unwrap();
    let renamed_fields = runtime.entity_facts(&source.name).unwrap().unwrap();
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert_eq!(nt(&renamed_fields).fields[0].name, original.fields[1].name);
    assert_eq!(nt(&renamed_fields).fields[1].name, original.fields[0].name);
    let base = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(original.id)).unwrap().unwrap();
    let mut draft = notetypes::Draft::of(base);
    let first = draft.templates[0].name.clone();
    draft.templates[0].name = draft.templates[1].name.clone();
    draft.templates[1].name = first;
    draft.name = "Native legacy swapped names".into();
    draft.config.css = "native legacy styling".into();
    notetypes::save(&collection, draft, Default::default()).unwrap();
    let changed = runtime.entity_facts(&source.name).unwrap().unwrap();
    let actual = nt(&changed);
    assert_eq!(actual.templates[0].name, original.templates[1].name);
    assert_eq!(actual.templates[1].name, original.templates[0].name);
    assert_eq!(actual.struct_gen, original.struct_gen);
    assert_eq!(changed.base_payload.as_ref(), Some(&source.payload));
    assert_eq!(changed.acked_payload, source.acked_payload);
    assert_eq!(changed.acked_envelope, source.acked_envelope);
    assert_eq!(changed.in_flight, source.in_flight);
    assert!(changed.mark.is_some());
    assert!(!runtime.status().unwrap().tracking_stopped);
    assert_eq!(bookkeeping(&host), before);
    assert_eq!(row(&host, &changed.payload, &actual), changed.payload);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_eq!(runtime.entity_facts(&source.name).unwrap().unwrap().payload, changed.payload);
    assert_eq!(bookkeeping(&host), before);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys());
    let retained = reopened.cloud_runtime().unwrap().entity_facts(&source.name).unwrap().unwrap();
    assert_eq!(retained.payload, changed.payload);
    assert_eq!(retained.base_payload, changed.base_payload);
    assert_eq!(retained.acked_payload, changed.acked_payload);
    assert_eq!(row(&reopened, &retained.payload, &actual), retained.payload);
    assert_eq!(bookkeeping(&reopened), before);
    reopened.close().unwrap();
}

fn current_type(host: &CoreHost, original: &codec::Notetype) -> Payload {
    let collection = host.collection().unwrap();
    let lease = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = lease.snapshot().unwrap();
    let head = snapshot.row_image(Table::Notetypes, &RowKey::Id(original.id)).unwrap().unwrap();
    Payload::Notetype(cloud::row_payload::notetype(
        &cloud::row_payload::ReadView(&*snapshot), &head, original.struct_gen,
    ).unwrap())
}

fn assert_restored_type(
    host: &CoreHost, source: &EntityFacts, expected: &Payload, dependents: &[EntityFacts],
) -> EntityFacts {
    let runtime = host.cloud_runtime().unwrap();
    let facts = runtime.entity_facts(&source.name).unwrap().unwrap();
    let original = nt(source);
    assert_eq!(facts.payload, *expected);
    assert_eq!(current_type(host, &original), *expected, "tracking equals the entire stored aggregate");
    assert_eq!(nt(&facts).struct_gen, original.struct_gen);
    assert_eq!(facts.origin, source.origin);
    assert_eq!(facts.key, source.key);
    assert_eq!(facts.acked_payload, source.acked_payload);
    assert_eq!(facts.acked_envelope, source.acked_envelope);
    assert_eq!(facts.acked_system_fields, source.acked_system_fields);
    assert_eq!(facts.acked_row_digest, source.acked_row_digest);
    assert_eq!(facts.acked_payload_digest, source.acked_payload_digest);
    assert_eq!(facts.server_erev, source.server_erev);
    assert_eq!(facts.acked_rev, source.acked_rev);
    assert_eq!(facts.in_flight, source.in_flight);
    if expected != &source.payload {
        assert_eq!(facts.base_payload.as_ref(), Some(&source.payload));
        assert!(facts.mark.is_some());
    } else {
        assert_eq!(facts.base_payload, source.base_payload);
        assert_eq!(facts.mark, source.mark);
    }
    assert!(!runtime.status().unwrap().tracking_stopped);
    for prior in dependents {
        let dependent = runtime.entity_facts(&prior.name).unwrap().unwrap();
        assert_eq!(dependent.payload, prior.payload);
        assert_eq!(dependent.acked_payload, prior.acked_payload);
        assert_eq!(dependent.acked_envelope, prior.acked_envelope);
        assert_eq!(row(host, &dependent.payload, &nt(&facts)), dependent.payload);
    }
    facts
}

fn cover_restored_type(host: &CoreHost, seq: meguri_engine::Seq) {
    let collection = host.collection().unwrap();
    let (identity, snapshot_seq) = {
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        (snapshot.wal_identity(), snapshot.seq())
    };
    assert!(snapshot_seq >= seq);
    let meguri_engine::WalCoverage::Covered(admitted) = collection.wait_wal_covered(identity, seq, Duration::ZERO).unwrap()
        else { panic!("native layout authority requires prior WAL coverage") };
    assert_eq!(admitted.identity(), identity);
    assert!(admitted.through() >= seq);
    collection.flush_wal(Duration::from_secs(30)).unwrap();
    let meguri_engine::WalCoverage::Covered(receipt) = collection.wait_wal_covered(identity, snapshot_seq,
        Duration::from_secs(30)).unwrap() else { panic!("restored commit needs actual WAL coverage") };
    assert_eq!(receipt.identity(), identity);
    assert!(receipt.through() >= snapshot_seq);
}

fn exercise_native_restore(host: &CoreHost, source: &EntityFacts, swaps: bool) -> (Payload, Vec<EntityFacts>) {
    use meguri_engine::notetypes::{self, AggregateDigest, FieldIdentity, FieldRename, RenameFields};
    use meguri_engine::UndoScope;
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let original = nt(source);
    assert!(original.struct_gen > 0);
    assert!(source.server_erev > 0 && source.mark.is_none());
    assert_eq!(source.acked_payload.as_ref(), Some(&source.payload));
    let mut dependents: Vec<_> = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().into_iter()
        .filter(|facts| matches!(&facts.payload, Payload::Note(note) if note.mid == original.id)).collect();
    let note_ids: BTreeSet<_> = dependents.iter().map(|facts| {
        let Payload::Note(note) = &facts.payload else { unreachable!() };
        note.id
    }).collect();
    dependents.extend(runtime.entity_facts_page(codec::Kind::Card, None).unwrap().into_iter()
        .filter(|facts| matches!(&facts.payload, Payload::Card(card) if note_ids.contains(&card.nid))));
    let bookkeeping_before = bookkeeping(host);
    collection.flush_wal(Duration::from_secs(30)).unwrap();
    collection.pause_automatic_wal_flush_for_testing(true);
    let after_fields = if swaps {
        let base = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(original.id)).unwrap().unwrap();
        let last = base.fields.len() - 1;
        let renamed = notetypes::rename_fields(&collection, RenameFields {
            notetype: base.id, base: AggregateDigest::of(&base),
            fields: vec![
                FieldRename { field: FieldIdentity::of(&base.fields[0]), name: base.fields[last].name.clone() },
                FieldRename { field: FieldIdentity::of(&base.fields[last]), name: base.fields[0].name.clone() },
            ],
        }, meguri_engine::CancelToken::new(), meguri_engine::ProgressSink::new()).unwrap();
        let expected = current_type(host, &original);
        let Payload::Notetype(actual) = &expected else { unreachable!() };
        assert_eq!(actual.fields[0].name, original.fields[last].name);
        assert_eq!(actual.fields[last].name, original.fields[0].name);
        assert_restored_type(host, source, &expected, &dependents);
        cover_restored_type(host, renamed.seq);
        expected
    } else { source.payload.clone() };
    let base = collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(original.id)).unwrap().unwrap();
    let mut draft = notetypes::Draft::of(base);
    if swaps {
        let last = draft.templates.len() - 1;
        let first_name = draft.templates[0].name.clone();
        draft.templates[0].name = draft.templates[last].name.clone();
        draft.templates[last].name = first_name;
    }
    draft.name = "Editable published type".into();
    draft.config.css = ".card { margin: 7px; }".into();
    let saved = notetypes::save(&collection, draft, Default::default()).unwrap();
    let changed = current_type(host, &original);
    let Payload::Notetype(actual) = &changed else { unreachable!() };
    assert_eq!(actual.name, b"Editable published type");
    assert_eq!(NotetypeConfig::decode(&actual.config).unwrap().css, ".card { margin: 7px; }");
    if swaps {
        let last = original.templates.len() - 1;
        assert_eq!(actual.templates[0].name, original.templates[last].name);
        assert_eq!(actual.templates[last].name, original.templates[0].name);
    }
    assert_restored_type(host, source, &changed, &dependents);
    cover_restored_type(host, saved.seq);
    let undone = collection.undo(UndoScope::Global).unwrap();
    assert_restored_type(host, source, &after_fields, &dependents);
    cover_restored_type(host, undone.seq);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_restored_type(host, source, &after_fields, &dependents);
    if swaps {
        let undone = collection.undo(UndoScope::Global).unwrap();
        assert_restored_type(host, source, &source.payload, &dependents);
        cover_restored_type(host, undone.seq);
        runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
        assert_restored_type(host, source, &source.payload, &dependents);
        let redone = collection.redo(UndoScope::Global).unwrap();
        assert_restored_type(host, source, &after_fields, &dependents);
        cover_restored_type(host, redone.seq);
    }
    let redone = collection.redo(UndoScope::Global).unwrap();
    assert_restored_type(host, source, &changed, &dependents);
    cover_restored_type(host, redone.seq);
    assert_eq!(bookkeeping(host), bookkeeping_before);
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    assert_restored_type(host, source, &changed, &dependents);
    assert_eq!(bookkeeping(host), bookkeeping_before);
    collection.pause_automatic_wal_flush_for_testing(false);
    (changed, dependents)
}

fn published_type_restore_case(swaps: bool) {
    for reopen_after_undo in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated_legacy_reversed(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(459)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let source = source_type(&host);
        let before = bookkeeping(&host);
        let (mut changed, dependents) = exercise_native_restore(&host, &source, swaps);
        assert!(dependents.iter().any(|facts| facts.kind == codec::Kind::Note));
        assert!(dependents.iter().any(|facts| facts.kind == codec::Kind::Card));
        if reopen_after_undo {
            let collection = host.collection().unwrap();
            collection.pause_automatic_wal_flush_for_testing(true);
            let undone = collection.undo(meguri_engine::UndoScope::Global).unwrap();
            cover_restored_type(&host, undone.seq);
            if swaps {
                let undone = collection.undo(meguri_engine::UndoScope::Global).unwrap();
                cover_restored_type(&host, undone.seq);
            }
            changed = source.payload.clone();
            assert_restored_type(&host, &source, &changed, &dependents);
            host.cloud_runtime().unwrap().reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
            collection.pause_automatic_wal_flush_for_testing(false);
        }
        let retained = assert_restored_type(&host, &source, &changed, &dependents);
        assert!(host.cloud_runtime().unwrap().inbox_entries(binding.id).unwrap().is_empty());
        assert!(host.cloud_runtime().unwrap().apply_intents(binding.id).unwrap().is_empty());
        host.close().unwrap();
        drop(host);
        let reopened = open_host(&dir, keys());
        let runtime = reopened.cloud_runtime().unwrap();
        let facts = assert_restored_type(&reopened, &source, &changed, &dependents);
        assert_eq!(facts.base_payload, retained.base_payload);
        assert_eq!(facts.mark, retained.mark);
        assert_eq!(bookkeeping(&reopened), before);
        prove_reopened_binding(&reopened, &binding);
        runtime.reconcile_binding(&reopened.collection().unwrap(), &reopened.media().unwrap(), Duration::from_secs(30)).unwrap();
        assert_restored_type(&reopened, &source, &changed, &dependents);
        assert_eq!(bookkeeping(&reopened), before);
        reopened.close().unwrap();
    }
}

#[test]
fn ordinary_notetype_published_name_css_save_undo_redo_retains_counter_base_and_ack() {
    published_type_restore_case(false);
}

#[test]
fn ordinary_notetype_published_legacy_name_swaps_undo_redo_reopen_keep_dependents() {
    published_type_restore_case(true);
}

fn multipage_restore_case(swaps: bool) {
    use meguri_engine::notetypes::{self, NewNotetype, NewTemplate};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let added = notetypes::add(&host.collection().unwrap(), NewNotetype {
        name: "Paged editable type".into(),
        fields: (0..501).map(|i| format!("Column{i}")).collect(),
        templates: (0..251).map(|i| NewTemplate {
            name: format!("Layout{i}"), front: format!("{{{{Column{i}}}}}"), back: "{{Column500}}".into(),
            ..Default::default()
        }).collect(),
        ..Default::default()
    }).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(460)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(added.value.id.0)).unwrap();
    let source = host.cloud_runtime().unwrap().entity_facts(&name).unwrap().unwrap();
    assert_eq!(nt(&source).fields.len(), 501);
    assert_eq!(nt(&source).templates.len(), 251);
    let (changed, dependents) = exercise_native_restore(&host, &source, swaps);
    assert_restored_type(&host, &source, &changed, &dependents);
    host.close().unwrap();
    drop(host);
    let reopened = open_host(&dir, keys());
    assert_restored_type(&reopened, &source, &changed, &dependents);
    reopened.close().unwrap();
}

#[test]
fn ordinary_notetype_multipage_css_save_undo_redo_restores_complete_aggregate() {
    multipage_restore_case(false);
}

#[test]
fn ordinary_notetype_multipage_name_swaps_undo_redo_restore_complete_aggregate() {
    multipage_restore_case(true);
}

#[path = "incoming_notetype_round6.rs"]
mod round6;

#[path = "incoming_notetype_round7.rs"]
mod round7;

#[path = "incoming_notetype_normal_transfer.rs"]
mod normal_transfer;
