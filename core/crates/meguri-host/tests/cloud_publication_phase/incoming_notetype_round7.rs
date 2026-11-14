//! Bounded source fixtures for the approved recovery completion round.
use super::*;
use meguri_engine::{UndoScope, notetypes::{self, NewNotetype, NewTemplate}};
use meguri_storage::raw::{PlainDatabase, RawValue};
use meguri_testkit::bench::peak_alloc::PeakAlloc;

#[global_allocator]
static ALLOC: PeakAlloc = PeakAlloc;

fn add_type(host: &CoreHost, clone: bool) -> NotetypeId {
    let collection = host.collection().unwrap();
    if clone {
        notetypes::clone_notetype(&collection, NotetypeId(nt(&source_type(host)).id), Some("Native lifecycle clone".into())).unwrap().value.id
    } else {
        notetypes::add(&collection, NewNotetype {
            name: "Native lifecycle addition".into(), fields: vec!["Prompt".into(), "Reply".into()],
            templates: vec![NewTemplate { name: "Question".into(), front: "{{Prompt}}".into(), back: "{{Reply}}".into(), ..Default::default() }],
            ..Default::default()
        }).unwrap().value.id
    }
}

#[test]
fn native_notetype_add_clone_undo_redo_unsent_handed_and_accepted_reopen() {
    use meguri_engine::{EngineError, undo::reasons::DisabledReason};
    for clone in [false, true] {
        for delivery in [0, 1, 2] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("device");
            generated(&dir);
            let host = open_host(&dir, keys());
            let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(471)).unwrap();
            let binding = publish_actual_collection(&host, &mut service);
            let collection = host.collection().unwrap();
            let runtime = host.cloud_runtime().unwrap();
            let id = add_type(&host, clone);
            let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(id.0)).unwrap();
            let created = runtime.entity_facts(&name).unwrap().unwrap();
            if delivery == 0 {
                collection.undo(UndoScope::Global).unwrap();
                host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
                assert!(runtime.entity_facts(&name).unwrap().is_none());
                assert!(collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().is_none());
                collection.redo(UndoScope::Global).unwrap();
                host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
                let restored = runtime.entity_facts(&name).unwrap().unwrap();
                assert_eq!(restored.payload, created.payload);
                assert_eq!(restored.acked_rev, 0);
                assert!(restored.mark.is_some() && restored.acked_payload.is_none() && restored.base_payload.is_none());
            } else {
                let instance = runtime.instance().unwrap();
                let batch = runtime.build_collection_batch(&instance, &collection, &BTreeSet::from([name.clone()])).unwrap();
                assert_eq!(batch.saves.len(), 1);
                if delivery == 2 { assert!(send_actual_batch(&host, &mut service, &batch).is_empty()); }
                let before = runtime.entity_facts(&name).unwrap().unwrap();
                let pending = runtime.tracking_sendables();
                let seq = collection.last_seq();
                let native = notetypes::AggregateDigest::of(&collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap());
                let expected = if delivery == 1 { DisabledReason::UploadPending } else { DisabledReason::AlreadyUploaded };
                assert!(matches!(collection.undo(UndoScope::Global), Err(EngineError::UndoUnavailable { reason }) if reason == expected));
                assert_eq!(collection.last_seq(), seq);
                assert_eq!(notetypes::AggregateDigest::of(&collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap()), native);
                assert_eq!(runtime.entity_facts(&name).unwrap().unwrap(), before);
                assert_eq!(runtime.tracking_sendables(), pending);
                if delivery == 1 {
                    assert!(before.sent && !before.in_flight.is_empty() && before.acked_rev == 0);
                    assert!(send_actual_batch(&host, &mut service, &batch).is_empty());
                }
                let acked = runtime.entity_facts(&name).unwrap().unwrap();
                assert_eq!(acked.origin, created.origin);
                assert_eq!(acked.acked_payload.as_ref(), Some(&created.payload));
                assert!(acked.mark.is_none() && acked.in_flight.is_empty());
            }
            let retained = runtime.entity_facts(&name).unwrap().unwrap();
            collection.flush_wal(Duration::from_secs(30)).unwrap();
            host.close().unwrap(); drop(collection); drop(host);
            let host = open_host(&dir, keys());
            prove_reopened_binding(&host, &binding);
            host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
            let reopened = host.cloud_runtime().unwrap().entity_facts(&name).unwrap().unwrap();
            assert_eq!(reopened.payload, retained.payload);
            assert_eq!(reopened.origin, retained.origin);
            assert_eq!(reopened.acked_rev, retained.acked_rev);
            assert_eq!(reopened.base_payload, retained.base_payload);
            assert_eq!(reopened.mark, retained.mark);
            host.close().unwrap();
        }
    }
}

#[test]
fn stopped_reconnect_preview_250_large_notes_valid_and_refused_layouts_bounded_heap() {
    for refused in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        let recipe = Recipe::parse("recipe: preview-memory\ndescription: Public large preview\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic\n    notetype: reversed\n    new: 500\n    fields: [\"prompt {n}\", \"reply {n}\"]\n").unwrap();
        generate_recipe(&recipe, 471, &dir).unwrap();
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(472)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let source = source_type(&host);
        let mut ids = Vec::new();
        let mut after = None;
        loop {
            let page = host.cloud_runtime().unwrap().entity_facts_page(codec::Kind::Note, after.as_deref()).unwrap();
            if page.is_empty() { break; }
            for e in page {
                after = Some(e.key.clone());
                ids.push(i64::from_be_bytes(e.key.try_into().unwrap()));
            }
        }
        assert_eq!(ids.len(), 250);
        for id in &ids {
            meguri_engine::notes::update(&host.collection().unwrap(), meguri_engine::notes::UpdateNote {
                id: meguri_model::ids::NoteId(*id), fields: vec!["x".repeat(1_300_000), "reply".into()], tags: vec![],
            }, meguri_model::changes::OpSource::UserUndoable).unwrap();
        }
        host.stop_using_icloud().unwrap();
        host.close().unwrap(); drop(host);
        let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
        if refused {
            db.execute("UPDATE templates SET ord=-ord-1 WHERE ntid=?1", &[RawValue::Integer(nt(&source).id)]).unwrap();
            db.execute("UPDATE templates SET ord=ord+2 WHERE ntid=?1", &[RawValue::Integer(nt(&source).id)]).unwrap();
        }
        db.close().unwrap();
        let host = open_host(&dir, keys());
        let before = cloud::testing::entity_body_reads();
        PeakAlloc::reset_peak();
        let preview = actual_preview(&host, &mut service, &scope());
        let reads = cloud::testing::entity_body_reads() - before;
        assert_eq!(preview.local.counts[&codec::Kind::Note], 250);
        assert_eq!(preview.local.counts[&codec::Kind::Card], 500);
        assert_eq!(preview.unsent_counts_exact, !refused);
        if !refused { assert_eq!(preview.unsent_by_kind[&codec::Kind::Note], 250); }
        // Only selected type authorities are body inputs, never Note/ACK/base images.
        assert!(reads < 32, "preview hydrated unrelated retained bodies: {reads}");
        assert!(PeakAlloc::is_installed());
        assert!(PeakAlloc::peak() < 300_000_000, "absolute preview Rust heap {}", PeakAlloc::peak());
        assert_eq!(host.cloud_runtime().unwrap().status().unwrap().binding.unwrap().id, binding.id);
        host.close().unwrap();
    }
}

#[test]
fn replacement_blocked_ordinary_worker() {
    use meguri_model::faultpoint::{self, Fault};
    let Some(root) = std::env::var_os("MEGURI_ROUND7_REPLACEMENT_ROOT") else { return; };
    let root = std::path::PathBuf::from(root);
    let dir = root.join("device");
    generated_legacy_reversed(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(&root, Config::default(), SeededRandom::new(473)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let source = source_type(&host);
    let mut incoming = nt(&source);
    incoming.name = b"Unapplied ordinary name".to_vec();
    let saved = save_server(&mut service, &binding, &Payload::Notetype(incoming), source.origin, None);
    host.cloud_runtime().unwrap().receive_collection_records(&start(&host), &[saved]).unwrap();
    faultpoint::arm("cloud.apply.after_intent", Fault::Abort);
    host.cloud_runtime().unwrap().drain_incoming(&host.collection().unwrap(), Duration::from_secs(30)).unwrap();
    panic!("original intent boundary was not reached");
}

fn blocked_replacement(root: &Path) -> (CoreHost, Service<SeededRandom>, cloud::Binding, Vec<cloud::incoming::ApplyIntent>) {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "incoming_notetype::round7::replacement_blocked_ordinary_worker", "--nocapture"])
        .env("MEGURI_ROUND7_REPLACEMENT_ROOT", root).status().unwrap();
    assert_eq!(status.signal(), Some(6));
    let dir = root.join("device");
    let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
    db.execute_batch("BEGIN IMMEDIATE; UPDATE templates SET ord=-ord-1 WHERE ntid IN (SELECT ntid FROM templates GROUP BY ntid HAVING count(*)=2); UPDATE templates SET ord=ord+2 WHERE ord<0; COMMIT").unwrap();
    db.close().unwrap();
    let host = open_host(&dir, keys());
    let binding = host.cloud_runtime().unwrap().status().unwrap().binding.unwrap();
    prove_reopened_binding(&host, &binding);
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    assert_eq!(runtime.policy_projection(cloud::policy::Lifecycle::Ready, host.collection().unwrap().generation().0).unwrap().facts.fence,
        cloud::policy::Fence::ChooseCollection);
    let intents = runtime.apply_intents(binding.id).unwrap();
    assert_eq!(intents.len(), 1);
    assert!(intents[0].media.is_none() && !intents[0].keys.is_empty());
    let service = Service::open(root, Config::default(), SeededRandom::new(473)).unwrap();
    (host, service, binding, intents)
}

fn adoption_reply(service: &mut Service<SeededRandom>, epoch: cloud::EpochId, task: &cloud::adoption::AdoptionTask) -> cloud::adoption::AdoptionReply {
    use cloud::adoption::{AdoptionReply, AdoptionTask as T, StagingRecord};
    let read = |service: &mut Service<SeededRandom>, zone: String, name: &str| {
        let Reply::Record(record) = service.request(&scope(), Caller::default(), Operation::Read {
            zone, name: name.into(), fields: RequestedFields::All,
        }).unwrap().result.unwrap() else { panic!("actual server record") };
        RecordRead::Found(publication_control_record(&record).unwrap())
    };
    match task {
        T::ReadActive | T::RecheckActive => AdoptionReply::Read(read(service, "meguri-root".into(), "active")),
        T::ReadEpoch => AdoptionReply::Read(read(service, cloud::adapter::collection_zone(epoch), "epoch")),
        T::ReadMediaRoot => AdoptionReply::Read(read(service, "meguri-media".into(), "root")),
        T::Fetch { cursor } => {
            let Reply::ZoneChanges(page) = service.request(&scope(), Caller::default(), Operation::FetchZone {
                zone: cloud::adapter::collection_zone(epoch), token: cursor.as_ref().map(|c| serde_json::from_slice(&c.0).unwrap()),
            }).unwrap().result.unwrap() else { panic!("actual zone page") };
            let mut records = Vec::new(); let mut deleted = Vec::new();
            for change in page.changes {
                match change {
                    meguri_testkit::cloud::RecordChange::Saved(record) => records.push(StagingRecord {
                        record: publication_control_record(&record).unwrap(), name: record.name, record_type: record.record_type, payload_file: None,
                    }),
                    meguri_testkit::cloud::RecordChange::Deleted { name, .. } => deleted.push(name),
                }
            }
            AdoptionReply::Page { records, deleted, next: page.more.then(|| cloud::OpaqueBytes(serde_json::to_vec(&page.token).unwrap())) }
        }
        _ => panic!("local task"),
    }
}

#[test]
fn publication_retains_blocked_work_until_switch_acceptance() {
    let temp = tempfile::tempdir().unwrap();
    let (host, mut service, binding, intents) = blocked_replacement(temp.path());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let preview = actual_preview(&host, &mut service, &scope());
    let operation = runtime.confirm_connection_publication(&preview, &collection).unwrap();
    let PublicationWork::Local { op, work: LocalWork::Bind } = runtime.next_publication_work(&collection).unwrap() else { panic!("bind") };
    assert_eq!(op, operation);
    runtime.perform_publication_local(op, LocalWork::Bind, &collection).unwrap();
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    runtime.cancel_publication(operation, &collection).unwrap();
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    assert!(runtime.register_sdk(true).is_err());
    host.close().unwrap(); drop(collection); drop(host);
    let host = open_host(&temp.path().join("device"), keys());
    prove_reopened_binding(&host, &binding);
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let old_type = source_type(&host);
    host.stop_using_icloud().unwrap();
    let local_type = notetypes::add(&collection, NewNotetype {
        name: "Replacement independent layout".into(),
        fields: vec!["First".into(), "Second".into(), "Third".into()],
        templates: vec![NewTemplate { name: "Only card".into(), front: "{{First}}".into(), back: "{{Third}}".into(), ..Default::default() }],
        ..Default::default()
    }).unwrap().value.id;
    let old_name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(local_type.0)).unwrap();
    assert!(runtime.entity_facts(&old_name).unwrap().is_none());
    let preview = actual_preview(&host, &mut service, &scope());
    runtime.confirm_connection_publication(&preview, &collection).unwrap();
    let request = until_active(&host, &mut service);
    let new_binding = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new_binding.id, binding.id);
    for id in [nt(&old_type).id, local_type.0] {
        let name = codec::entity_name(&new_binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(id)).unwrap();
        let image = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(nt(&image).struct_gen, 1);
        assert_eq!(image.local_rev, image.acked_rev);
        assert!(image.server_erev > 0 && image.mark.is_none() && image.base_payload.is_none());
        assert_eq!(image.acked_payload.as_ref(), Some(&image.payload));
        assert!(image.in_flight.is_empty());
        if id == local_type.0 { assert_eq!((nt(&image).fields.len(), nt(&image).templates.len()), (3, 1)); }
        else {
            assert_ne!(image.origin, old_type.origin);
            assert_eq!((nt(&image).fields.len(), nt(&image).templates.len()), (2, 2));
        }
    }
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents, "old intents must survive until the conditional Active save is accepted");
    let (reply, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(reply, OwnResult::Saved(_)));
    runtime.publication_result(&request, reply, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(matches!(runtime.apply_intents(binding.id), Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))));
    assert!(matches!(runtime.inbox_entries(binding.id), Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))));
    host.close().unwrap();
}

#[test]
fn adoption_retains_blocked_work_until_activation() {
    use cloud::adoption::{AdoptStep, AdoptionTask as T};
    let temp = tempfile::tempdir().unwrap();
    let (host, mut service, binding, intents) = blocked_replacement(temp.path());
    for cancel in [true, false] {
        let preview = actual_preview(&host, &mut service, &scope());
        let operation = host.confirm_cloud_adoption(&preview).unwrap();
        for _ in 0..32 {
            let PublicationWork::Adoption(work) = host.next_cloud_publication_work().unwrap() else { panic!("adoption task") };
            let runtime = host.cloud_runtime().unwrap();
            let op = runtime.adoption_op().unwrap().unwrap();
            assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
            if work.task == T::Activate && cancel {
                assert_eq!(op.step, AdoptStep::Ready);
                host.cancel_cloud_adoption(operation).unwrap();
                assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
                assert!(runtime.register_sdk(true).is_err());
                break;
            }
            if matches!(work.task, T::Validate | T::Activate) { host.perform_cloud_adoption_local(&work).unwrap(); }
            else { host.cloud_adoption_reply(&work, adoption_reply(&mut service, op.epoch, &work.task)).unwrap(); }
            if runtime.adoption_op().unwrap().is_none() {
                assert!(!cancel);
                assert!(matches!(runtime.apply_intents(binding.id), Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))));
                assert!(matches!(runtime.inbox_entries(binding.id), Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))));
                break;
            }
        }
        assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
    }
    host.close().unwrap();
}

#[test]
fn settled_competing_root_cancel_restores_refusal_then_local_only_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let (host, mut service, binding, intents) = blocked_replacement(temp.path());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let preview = actual_preview(&host, &mut service, &scope());
    let old_active = preview.active.clone().unwrap();
    let operation = runtime.confirm_connection_publication(&preview, &collection).unwrap();
    let request = until_active(&host, &mut service);
    // Commit a competing root version through the Service before submitting
    // this replacement's conditional switch.
    let Reply::Modified(items) = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: "meguri-root".into(), atomic: false, changes: vec![Mutation::Save {
            record: RecordInput::encrypted("active", "MGActive", old_active.encrypted.unwrap()),
            if_unchanged: Some(SystemFields::from_bytes(old_active.system_fields.0)),
        }],
    }).unwrap().result.unwrap() else { panic!("competing save") };
    assert!(matches!(items[0].result, Ok(MutationSuccess::Saved(_))));
    let (reply, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(reply, OwnResult::Conflict(_)));
    runtime.publication_result(&request, reply, &collection).unwrap();
    assert!(runtime.publication_conflict_cancellable(operation).unwrap());
    let op = runtime.publication_op().unwrap().unwrap();
    assert!(op.switch_sent && op.switch_conflict.is_some());
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    runtime.cancel_publication(operation, &collection).unwrap();
    assert!(!runtime.publication_conflict_cancellable(operation).unwrap());
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    assert!(runtime.register_sdk(true).is_err());
    host.keep_cloud_local_only(host.cloud_connection_generation().unwrap(), None).unwrap();
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    host.close().unwrap(); drop(collection); drop(host);
    let host = open_host(&temp.path().join("device"), keys());
    let preview = actual_preview(&host, &mut service, &scope());
    assert!(preview.decision == ConnectDecision::ChooseCollection && !preview.unsent_counts_exact);
    assert_eq!(host.cloud_runtime().unwrap().apply_intents(binding.id).unwrap(), intents);
    assert!(host.cloud_runtime().unwrap().register_sdk(true).is_err());
    host.close().unwrap();
}

#[test]
fn published_native_notetype_local_edit_undo_redo_send_accept_and_reopen() {
    for clone in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(474)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let id = add_type(&host, clone);
        let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::Notetype, &codec::id_key(id.0)).unwrap();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let instance = runtime.instance().unwrap();
        let batch = runtime.build_collection_batch(&instance, &collection, &BTreeSet::from([name.clone()])).unwrap();
        assert_eq!(batch.saves.len(), 1);
        assert!(send_actual_batch(&host, &mut service, &batch).is_empty());
        let live = runtime.entity_facts(&name).unwrap().unwrap();
        let mut draft = notetypes::Draft::of(collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap());
        draft.name = "Editable published type".into();
        draft.config.css = "Local reversible styling".into();
        notetypes::save(&collection, draft, Default::default()).unwrap();
        let edited = runtime.entity_facts(&name).unwrap().unwrap();
        assert_ne!(edited.payload, live.payload);
        assert_eq!(edited.acked_payload, live.acked_payload);
        collection.undo(UndoScope::Global).unwrap();
        host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
        let undone = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(undone.payload, live.payload);
        assert_eq!(undone.origin, live.origin);
        assert_eq!(undone.acked_rev, live.acked_rev);
        collection.redo(UndoScope::Global).unwrap();
        host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
        let restored = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(restored.payload, edited.payload);
        assert_eq!(restored.origin, live.origin);
        let batch = runtime.build_collection_batch(&instance, &collection, &BTreeSet::from([name.clone()])).unwrap();
        assert_eq!(batch.saves.len(), 1);
        assert!(send_actual_batch(&host, &mut service, &batch).is_empty());
        let accepted = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(accepted.acked_payload.as_ref(), Some(&edited.payload));
        assert!(accepted.mark.is_none() && accepted.in_flight.is_empty());
        collection.flush_wal(Duration::from_secs(30)).unwrap();
        host.close().unwrap(); drop(collection); drop(host);
        let host = open_host(&dir, keys());
        prove_reopened_binding(&host, &binding);
        host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
        let reopened = host.cloud_runtime().unwrap().entity_facts(&name).unwrap().unwrap();
        assert_eq!(reopened.payload, accepted.payload);
        assert_eq!(reopened.origin, accepted.origin);
        assert_eq!(reopened.acked_rev, accepted.acked_rev);
        assert_eq!(reopened.base_payload, accepted.base_payload);
        assert!(reopened.mark.is_none() && reopened.in_flight.is_empty());
        host.close().unwrap();
    }
}

#[test]
fn large_notetype_part_pages_projection_and_raw_traversal_bounded_heap() {
    for table in [Table::Fields, Table::Templates] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(475)).unwrap();
        publish_actual_collection(&host, &mut service);
        let source = nt(&source_type(&host));
        host.close().unwrap(); drop(host);
        let db = PlainDatabase::open(&dir.join("collection.anki2")).unwrap();
        db.execute(&format!("DELETE FROM {} WHERE ntid=?1", table.name()), &[RawValue::Integer(source.id)]).unwrap();
        let config = if table == Table::Fields { source.fields[0].config.clone() } else { source.templates[0].config.clone() };
        db.execute_batch("BEGIN IMMEDIATE").unwrap();
        for ord in 0..250 {
            let mut name = format!("{ord:03}");
            name.push_str(&"x".repeat(1_300_000 - name.len()));
            let sql = if table == Table::Fields { "INSERT INTO fields VALUES(?1,?2,?3,?4)" }
                else { "INSERT INTO templates VALUES(?1,?2,?3,0,-1,?4)" };
            db.execute(sql, &[RawValue::Integer(source.id), RawValue::Integer(ord), RawValue::Text(name.into_bytes()), RawValue::Blob(config.clone())]).unwrap();
        }
        db.execute_batch("COMMIT").unwrap(); db.close().unwrap();
        let reader = meguri_storage::connections::ReaderFactory::recovery(
            &dir.join("collection.anki2"), &meguri_storage::conn_init::Initializers::new()).open(ReaderRole::Cloud).unwrap();
        PeakAlloc::reset_peak();
        let head = reader.row_image(Table::Notetypes, &RowKey::Id(source.id)).unwrap().unwrap();
        assert!(matches!(cloud::row_payload::notetype(&cloud::row_payload::ReadView(&reader), &head, 1),
            Err(cloud::Error::RowFailure(cloud::row_payload::RowFailure::Oversize))));
        let mut digests = Vec::new();
        for raw in [false, true] {
            let mut after = None;
            let mut count = 0;
            let mut pages = 0;
            let mut digest = [0u8; 32];
            loop {
                let page = if raw { reader.notetype_parts_raw_page(table, NotetypeId(source.id), after.as_ref()).unwrap() }
                    else { match table {
                        Table::Fields => reader.notetype_fields_page(NotetypeId(source.id), after.as_ref().map(|v| match v { meguri_storage::Cell::Integer(v) => *v, _ => panic!("ordinal") })).unwrap(),
                        Table::Templates => reader.notetype_templates_page(NotetypeId(source.id), after.as_ref().map(|v| match v { meguri_storage::Cell::Integer(v) => *v, _ => panic!("ordinal") })).unwrap(),
                        _ => unreachable!(),
                    }};
                if page.is_empty() { break; }
                assert!(page.len() < 250);
                let bytes: usize = page.iter().map(|r| match &r[2] { meguri_storage::Cell::Text(v) => v.len(), _ => panic!("name") }).sum();
                assert!(bytes <= 8 * 1024 * 1024);
                pages += 1;
                for row in page {
                    assert_eq!(row[1], meguri_storage::Cell::Integer(count));
                    after = Some(row[1].clone());
                    let row_digest = meguri_storage::journal::digest(&row);
                    for (left, right) in digest.iter_mut().zip(row_digest) { *left ^= right; }
                    count += 1;
                }
            }
            assert_eq!(count, 250);
            assert!(pages > 1);
            digests.push(digest);
        }
        assert_eq!(digests[0], digests[1]);
        assert!(PeakAlloc::is_installed());
        assert!(PeakAlloc::peak() < 300_000_000, "part traversal Rust heap {}", PeakAlloc::peak());
    }
}

#[test]
fn published_note_deletion_unsent_undo_redo_then_tombstone_send_accept_reopen() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(476)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let original = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &original.payload else { panic!("note") };
    let id = meguri_model::ids::NoteId(note.id);
    meguri_engine::notes::delete(&collection, vec![id], meguri_model::changes::OpSource::UserUndoable).unwrap().unwrap();
    collection.undo(UndoScope::Global).unwrap();
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let restored = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert_eq!(restored.payload, original.payload);
    assert_eq!(restored.origin, original.origin);
    collection.redo(UndoScope::Global).unwrap();
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let removed = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert!(removed.tombstone_at.is_some() && removed.mark.is_some());
    collection.flush_wal(Duration::from_secs(30)).unwrap();
    host.close().unwrap(); drop(collection); drop(host);
    let host = open_host(&dir, keys());
    prove_reopened_binding(&host, &binding);
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let batch = runtime.build_collection_batch(&start(&host), &host.collection().unwrap(), &BTreeSet::from([original.name.clone()])).unwrap();
    assert_eq!(batch.saves.len(), 1);
    let codec::DecodeOutcome::Supported(wire) = codec::Entity::decode(&batch.saves[0].envelope).unwrap() else { panic!("supported") };
    assert!(wire.fields.tombstone && wire.payload.as_deref() == Some(&[]));
    assert_eq!(wire.fields.origin, original.origin);
    assert!(send_actual_batch(&host, &mut service, &batch).is_empty());
    let accepted = runtime.entity_facts(&original.name).unwrap().unwrap();
    assert_eq!(accepted.acked_rev, removed.local_rev);
    assert!(accepted.mark.is_none() && accepted.in_flight.is_empty());
    assert!(host.collection().unwrap().reader(ReaderRole::Query).unwrap().note(id).unwrap().is_none());
    host.close().unwrap();
}

#[test]
fn blocked_publication_lost_active_reply_cancel_retains_zone_and_reopened_old_intents() {
    let temp = tempfile::tempdir().unwrap();
    let (host, mut service, binding, intents) = blocked_replacement(temp.path());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let preview = actual_preview(&host, &mut service, &scope());
    let operation = runtime.confirm_connection_publication(&preview, &collection).unwrap();
    let request = until_active(&host, &mut service);
    let before = runtime.publication_op().unwrap().unwrap();
    assert!(matches!(runtime.cancel_publication(operation, &collection), Err(cloud::runtime::RuntimeError::Publication(PublicationError::Busy))));
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    let (reply, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(reply, OwnResult::Saved(_)));
    // Model loss of this original successful reply at the transport boundary.
    runtime.publication_result(&request, OwnResult::Failed { retry_after_ms: None }, &collection).unwrap();
    assert!(runtime.publication_cancellable(operation).unwrap());
    assert!(!runtime.publication_conflict_cancellable(operation).unwrap());
    runtime.cancel_publication(operation, &collection).unwrap();
    assert_eq!(runtime.apply_intents(binding.id).unwrap(), intents);
    assert!(runtime.register_sdk(true).is_err());
    let zone = cloud::adapter::collection_zone(before.epoch);
    assert!(service.inspect(&scope()).unwrap().unwrap().zones.contains_key(&zone));
    host.close().unwrap(); drop(collection); drop(host);
    let build = cloud::BuildScope { container: cloud::ContainerId::new(scope().container).unwrap(), environment: cloud::Environment::Development };
    let cloud::OpenOutcome::Opened(store) = cloud::TrackingStore::open(&temp.path().join("device"), build).unwrap() else { panic!("tracking") };
    assert!(store.publication_op().unwrap().is_none());
    assert_eq!(store.apply_intents(binding.id).unwrap(), intents);
    assert!(!store.retained_zones(binding.id).unwrap().get(&zone).is_some_and(|z| z.deletion_due));
    drop(store);
    let host = open_host(&temp.path().join("device"), keys());
    prove_reopened_binding(&host, &binding);
    host.prepare_cloud_runtime(Duration::from_secs(30)).unwrap();
    let preview = actual_preview(&host, &mut service, &scope());
    assert_eq!(preview.decision, ConnectDecision::ChooseCollection);
    assert!(!preview.unsent_counts_exact);
    assert_eq!(host.cloud_runtime().unwrap().apply_intents(binding.id).unwrap(), intents);
    assert!(host.cloud_runtime().unwrap().register_sdk(true).is_err());
    host.close().unwrap();
}
