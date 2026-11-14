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
