//! R2 media regressions through the real Host, Service, file feed and SDK callbacks.
use super::*;
use super::media_protocol as protocol;
use cloud::{
    entity_codec::{self as codec, Kind, Payload},
    media::{Request, Work, blob::Blob},
    policy::{DeviceEvent, DeviceSettings},
    workflow::PendingChange,
};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::cloud::Asset;

fn setup() -> (tempfile::TempDir, CoreHost, Service<SeededRandom>, String) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("r2-media");
    generated(&dir);
    let host = open_prior_icloud_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(1193)).unwrap();
    publish_actual_collection(&host, &mut service);
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: false, media: true,
    })).unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())),
        origin_sdk: None,
    }).unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    runtime.resume_cloud_media();
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    (temp, host, service, sdk)
}

fn saved_record(service: &mut Service<SeededRandom>, zone: &str, record: RecordInput) -> Saved {
    saved_record_if_unchanged(service, zone, record, None)
}

fn saved_record_if_unchanged(
    service: &mut Service<SeededRandom>, zone: &str, record: RecordInput, if_unchanged: Option<SystemFields>,
) -> Saved {
    let response = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: zone.into(), atomic: false,
        changes: vec![Mutation::Save { record, if_unchanged }],
    }).unwrap();
    let Ok(Reply::Modified(mut items)) = response.result else { panic!("actual save required"); };
    let Ok(MutationSuccess::Saved(record)) = items.remove(0).result else { panic!("accepted save required"); };
    let Field::EncryptedBytes(envelope) = &record.fields["e"] else { panic!("encrypted record required"); };
    Saved { name: record.name, envelope: envelope.clone(), system_fields: record.system_fields.bytes().to_vec() }
}

fn add_local_named(host: &CoreHost, name: &str, bytes: &[u8]) -> std::path::PathBuf {
    let media = host.media().unwrap();
    let path = media.store().media_dir().join(name);
    std::fs::write(&path, bytes).unwrap();
    media.store().register_full(|_, _| true).unwrap();
    host.cloud_runtime().unwrap().consume_media_feed(&host.collection().unwrap(), &media).unwrap();
    path
}

fn add_local(host: &CoreHost, bytes: &[u8]) -> std::path::PathBuf {
    add_local_named(host, "photo.jpg", bytes)
}

fn prepare_upload(host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str) -> (u64, Work) {
    protocol::prepare_upload(host, service, sdk, &mut 16)
}

fn send_upload(host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str, id: u64, work: &Work) {
    assert!(matches!(work.request, Request::Upload { .. }));
    protocol::send_batch(host, service, sdk, id, work);
}

fn change_crt(host: &CoreHost, crt: i64) {
    host.collection().unwrap().run(
        meguri_engine::OpSpec::named("r2 media creation time", meguri_engine::OpSource::UserUndoable),
        move |ctx| {
            let mut col = ctx.scope().col()?;
            col.created = meguri_model::time::EpochSecs(crt);
            ctx.scope().put_col(&col)?;
            Ok(())
        },
    ).unwrap();
}

#[test]
fn actual_upload_reprepares_under_new_equal_crt_without_reassigning_original_callback() {
    let (_temp, host, mut service, sdk) = setup();
    add_local(&host, b"synthetic R2 upload");
    let (old, work) = prepare_upload(&host, &mut service, &sdk);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.validate_prepared_batch(&sdk, "meguri-media", old).unwrap();
    let crt = collection.reader(ReaderRole::Query).unwrap().col().unwrap().created.0;
    change_crt(&host, crt + 1);
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let equal = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    runtime.adapter_fetched(&sdk, &zone, &[equal], &[]).unwrap();
    assert!(runtime.validate_prepared_batch(&sdk, "meguri-media", old).is_err());
    let Request::Upload { name, staged, .. } = &work.request else { panic!("upload required"); };
    let requested = BTreeMap::from([(name.clone(), PendingChange::Save)]);
    let (new, retained) = runtime.build_cloud_media_batch(&sdk, &collection, &requested).unwrap().unwrap();
    assert_ne!(old, new);
    assert_eq!(retained, work);
    assert!(runtime.validate_prepared_batch(&sdk, "meguri-media", old).is_err());
    runtime.validate_prepared_batch(&sdk, "meguri-media", new).unwrap();
    assert_eq!(runtime.validate_cloud_media_batch(&sdk, &collection, old).unwrap(), work);
    runtime.enter_sent_handler(&sdk, old).unwrap();
    assert!(runtime.cloud_media_sdk_sent(&sdk, &collection, old, &[], &[], &[], &[], &[], std::slice::from_ref(name)).unwrap());
    assert_eq!(runtime.return_sent_handler(&sdk).unwrap(), old);
    runtime.validate_prepared_batch(&sdk, "meguri-media", new).unwrap();
    assert_eq!(runtime.build_cloud_media_batch(&sdk, &collection, &requested).unwrap().unwrap().0, new);
    assert!(staged.exists());
    send_upload(&host, &mut service, &sdk, new, &retained);
    assert!(!staged.exists());
    host.close().unwrap();
}

#[test]
fn original_saved_upload_revokes_unhanded_preparation_before_staging_release() {
    let (_temp, host, mut service, sdk) = setup();
    let bytes = b"synthetic original Saved upload";
    add_local(&host, bytes);
    let (old, work) = prepare_upload(&host, &mut service, &sdk);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.validate_prepared_batch(&sdk, "meguri-media", old).unwrap();
    let Request::Upload { name, envelope, staged } = &work.request else { panic!("upload required"); };
    let mut record = RecordInput::encrypted(name, "MGBlob", envelope.clone());
    record.fields.insert("a".into(), Field::Asset(Asset::synthetic(std::fs::read(staged).unwrap())));
    // The Service accepts the original before its callback is delivered.
    let saved = saved_record(&mut service, "meguri-media", record);
    assert_eq!(saved.name, *name);
    assert_eq!(saved.envelope, *envelope);
    assert!(!saved.system_fields.is_empty());
    let crt = collection.reader(ReaderRole::Query).unwrap().col().unwrap().created.0;
    change_crt(&host, crt + 1);
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let equal = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |epoch| epoch.crt = crt + 1);
    runtime.adapter_fetched(&sdk, &zone, &[equal], &[]).unwrap();
    assert!(runtime.validate_prepared_batch(&sdk, "meguri-media", old).is_err());
    let requested = BTreeMap::from([(name.clone(), PendingChange::Save)]);
    let (fresh, retained) = runtime.build_cloud_media_batch(&sdk, &collection, &requested).unwrap().unwrap();
    assert_ne!(old, fresh);
    assert_eq!(retained, work);
    assert!(runtime.validate_prepared_batch(&sdk, "meguri-media", old).is_err());
    runtime.validate_prepared_batch(&sdk, "meguri-media", fresh).unwrap();
    assert_eq!(std::fs::read(staged).unwrap(), bytes);
    assert_eq!(runtime.validate_cloud_media_batch(&sdk, &collection, old).unwrap(), work);
    runtime.enter_sent_handler(&sdk, old).unwrap();
    assert!(runtime.cloud_media_sdk_sent(&sdk, &collection, old, &[saved], &[], &[], &[], &[], &[]).unwrap());
    assert!(runtime.validate_prepared_batch(&sdk, "meguri-media", fresh).is_err());
    assert!(runtime.validate_cloud_media_batch(&sdk, &collection, fresh).is_err());
    assert!(!staged.exists());
    assert_eq!(runtime.return_sent_handler(&sdk).unwrap(), old);
    assert!(runtime.sdk_saved_removed(&sdk, fresh, std::slice::from_ref(name)).is_err());
    runtime.sdk_saved_removed(&sdk, old, std::slice::from_ref(name)).unwrap();
    assert!(runtime.build_cloud_media_batch(&sdk, &collection, &requested).unwrap().is_none());
    host.close().unwrap();
}

#[test]
fn actual_local_delete_wins_stages_conflict_copy_and_settles_with_held_snapshot() {
    let (_temp, host, mut service, sdk) = setup();
    let path = add_local(&host, b"synthetic R2 local delete");
    let (id, work) = prepare_upload(&host, &mut service, &sdk);
    send_upload(&host, &mut service, &sdk, id, &work);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    protocol::flush_collection(&host, &mut service, &sdk, &mut 16);
    let before = runtime.entity_facts_page(Kind::MediaName, None).unwrap().remove(0);
    assert!(before.server_erev > 0 && before.mark.is_none());
    host.media().unwrap().store().trash(&["photo.jpg".into()]).unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    assert!(host.media().unwrap().store().media_feed_snapshot().unwrap().items.iter().any(|item|
        item.name == "photo.jpg" && item.state == meguri_storage::media::MediaFileState::Absent(meguri_storage::media::MediaAbsenceCause::Removed)));
    runtime.consume_media_feed(&collection, &host.media().unwrap()).unwrap();
    let deleted = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(deleted.mark.is_some());
    // Local media tombstones live in the ledger, not the generic entity column.
    // Inspect the real outgoing envelope without sending or ACKing the deletion.
    let pending_delete = runtime.build_collection_batch(&sdk, &collection, &BTreeSet::from([before.name.clone()])).unwrap();
    assert_eq!(pending_delete.saves.len(), 1);
    let deletion = &pending_delete.saves[0];
    assert_eq!(deletion.name, before.name);
    let binding = runtime.status().unwrap().binding.unwrap();
    let outgoing = codec::receive(&binding.facts.record_key.0, &deletion.name, &deletion.envelope,
        codec::AssetInput::Absent).unwrap();
    assert_eq!(outgoing.entity.fields.kind, Kind::MediaName);
    assert_eq!(outgoing.entity.fields.key, b"photo.jpg");
    assert!(outgoing.entity.fields.tombstone);
    let unacked = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(unacked.mark.is_some());
    assert_eq!(unacked.acked_envelope, before.acked_envelope);
    let envelope = codec::Entity::build(codec::EntityFields {
        kind: Kind::MediaName, key: before.key.clone(), origin: before.origin,
        erev: before.server_erev + 1, generation: None, tombstone: false,
        deleted_at_ms: 0, writer: [93; 16], base_sha256: None,
    }, &before.payload.encode().unwrap()).unwrap().encode().unwrap();
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let saved = saved_record_if_unchanged(&mut service, &zone,
        RecordInput::encrypted(&before.name, "MGEntity", envelope),
        Some(SystemFields::from_bytes(before.acked_system_fields.clone())));
    runtime.adapter_fetched(&sdk, &zone, &[saved], &[]).unwrap();
    runtime.resume_cloud_media();
    let download = protocol::prepare_download(&host, &mut service, &sdk, &mut 16);
    protocol::deliver(&host, &mut service, &sdk, &download).unwrap();
    assert!(!path.exists());
    let copies = runtime.media_conflicts_for_testing().unwrap();
    assert_eq!(copies.len(), 1);
    assert_eq!(copies[0].reason, cloud::ConflictReason::EditVsDelete);
    assert_eq!(std::fs::read(copies[0].staged_path.as_ref().unwrap()).unwrap(), b"synthetic R2 local delete");
    assert!(runtime.apply_intents(runtime.status().unwrap().binding.unwrap().id).unwrap().is_empty());
    assert!(!runtime.incoming_wal_receipts().is_empty());
    host.close().unwrap();
}

#[test]
fn removed_note_intent_finishes_after_crt_change() {
    use meguri_model::faultpoint::{self, Fault};
    use sha2::{Digest, Sha256};
    use std::sync::atomic::{AtomicBool, Ordering};

    let (_temp, host, mut service, sdk) = setup();
    add_local_named(&host, "uploaded-baseline.jpg", b"synthetic R2 collision bytes");
    let (id, work) = prepare_upload(&host, &mut service, &sdk);
    send_upload(&host, &mut service, &sdk, id, &work);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    protocol::flush_collection(&host, &mut service, &sdk, &mut 16);
    let baseline = runtime.entity_facts_page(Kind::MediaName, None).unwrap().remove(0);
    assert!(baseline.server_erev > 0 && baseline.mark.is_none());
    let note = runtime.entity_facts_page(Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = note.payload else { panic!("note required"); };
    let note_id = meguri_model::ids::NoteId(note.id);
    meguri_engine::notes::update(&collection, meguri_engine::notes::UpdateNote {
        id: note_id, fields: vec!["<img src=\"photo.jpg\">".into(), "R2 deletion".into()], tags: vec![],
    }, meguri_engine::OpSource::UserUndoable).unwrap();
    protocol::flush_collection(&host, &mut service, &sdk, &mut 16);
    // Reuse the actually uploaded content under a newly created local name.
    // Its unacknowledged creation still selects collision move/rewrite.
    let path = add_local(&host, b"synthetic R2 collision bytes");
    let local = runtime.entity_facts_page(Kind::MediaName, None).unwrap().into_iter()
        .find(|entity| entity.key == b"photo.jpg").unwrap();
    assert_eq!(local.server_erev, 0);
    assert!(local.mark.is_some());
    let bytes = b"abc";
    let blob = Blob {
        sha256: Sha256::digest(bytes).into(),
        sha1: [0xa9,0x99,0x3e,0x36,0x47,0x06,0x81,0x6a,0xba,0x3e,0x25,0x71,0x78,0x50,0xc2,0x6c,0x9c,0xd0,0xd8,0x9d],
        size: 3,
    };
    let key = runtime.publication_media_state().unwrap().unwrap().0;
    let mut record = RecordInput::encrypted(blob.name(&key.0), "MGBlob", blob.encode().unwrap());
    record.fields.insert("a".into(), Field::Asset(Asset::synthetic(bytes.to_vec())));
    saved_record(&mut service, "meguri-media", record);
    let payload = Payload::MediaName(blob.media("photo.jpg"));
    let name = codec::entity_name(&binding.facts.record_key.0, Kind::MediaName, b"photo.jpg").unwrap();
    let envelope = codec::Entity::build(codec::EntityFields {
        kind: Kind::MediaName, key: b"photo.jpg".to_vec(), origin: [94; 16], erev: 1,
        generation: None, tombstone: false, deleted_at_ms: 0, writer: [95; 16], base_sha256: None,
    }, &payload.encode().unwrap()).unwrap().encode().unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let saved = saved_record(&mut service, &zone, RecordInput::encrypted(&name, "MGEntity", envelope));
    runtime.adapter_fetched(&sdk, &zone, &[saved], &[]).unwrap();
    let deleted = Arc::new(AtomicBool::new(false));
    let reached = deleted.clone();
    let writer = collection.clone();
    runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == cloud::testing::IncomingStage::ResolutionMoveBeforeRewrite && !reached.swap(true, Ordering::SeqCst) {
            meguri_engine::notes::delete(&writer, vec![note_id], meguri_engine::OpSource::UserUndoable)
                .unwrap().expect("real deletion");
        }
    });
    let download = protocol::prepare_download(&host, &mut service, &sdk, &mut 16);
    faultpoint::arm("cloud.media.after_resolution_install", Fault::IoError(std::io::ErrorKind::Other));
    let result = protocol::deliver(&host, &mut service, &sdk, &download);
    faultpoint::disarm("cloud.media.after_resolution_install");
    assert!(result.is_err());
    assert!(deleted.load(Ordering::SeqCst));
    let pending = runtime.apply_intents(binding.id).unwrap();
    assert_eq!(pending.len(), 1);
    assert!(!pending[0].keys.is_empty());
    assert!(pending[0].keys.iter().all(|key| key.before_digest.is_none() && key.after_digest.is_none()
        && key.before_payload.is_none() && key.after_payload.is_none()));
    let apply = pending[0].media.as_ref().unwrap();
    assert!(apply.installed && apply.move_evidence.is_some());
    let moved = apply.moved.as_ref().unwrap();
    let moved_path = path.parent().unwrap().join(std::str::from_utf8(&moved.name).unwrap());
    assert_eq!(std::fs::read(moved_path).unwrap(), b"synthetic R2 collision bytes");
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let crt = collection.reader(ReaderRole::Query).unwrap().col().unwrap().created.0;
    change_crt(&host, crt + 1);
    // Settle local day-clock maintenance before measuring replay alone.
    collection.check_rollover().unwrap();
    let seq = collection.last_seq();
    runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap();
    assert_eq!(collection.last_seq(), seq);
    assert!(collection.reader(ReaderRole::Query).unwrap().row_image(meguri_storage::Table::Notes, &meguri_storage::RowKey::Id(note.id)).unwrap().is_none());
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(!runtime.incoming_wal_receipts().is_empty());
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    host.close().unwrap();
}
