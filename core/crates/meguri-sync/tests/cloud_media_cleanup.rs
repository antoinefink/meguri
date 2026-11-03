//! Tests first: real publication, media ACKs and drained Sync Now operations.
//! No tracking, pending, acknowledgement or cleanup-result fixtures.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{
    entity_codec::{self as codec, Payload},
    media::{ServerState, Transfer},
    workflow::{EntityFacts, PendingChange},
};
use meguri_testkit::cloud::{
    coordinator::{Point, PublicationRig},
    *,
};
use support::*;

const DAY: i64 = 86_400_000;
const MEDIA: &str = "meguri-media";

fn policy_media_expected(
    uploads: (u64, u64),
    downloads: (u64, u64),
    unknown: u64,
    missing: u64,
) -> meguri_sync::cloud::policy::MediaFacts {
    meguri_sync::cloud::policy::MediaFacts {
        upload_files: Some(uploads.0),
        upload_bytes: Some(uploads.1),
        download_files: Some(downloads.0),
        download_bytes: Some(downloads.1),
        unknown_blobs: Some(unknown),
        missing_names: Some(missing),
        root_acked: Some(true),
        zone_missing: Some(false),
        no_space: Some(false),
        uploads_running: None,
        downloads_running: None,
        remaining_download_ms: None,
    }
}

#[test]
fn media_policy_lookup_cost_projection_visits_are_additive_and_keep_full_entity_reads() {
    for count in [100, 200] {
        let mut pair = Pair::published();
        for index in 0..count {
            pair.sender
                .driver()
                .put_media(
                    &format!("policy-cost-{index:04}.bin"),
                    format!("synthetic unique policy content {index:04}").as_bytes(),
                )
                .unwrap();
        }
        sync(&mut pair);
        let blobs = pair.sender.driver().media_blobs().unwrap();
        assert_eq!(blobs.len(), count);
        assert!(
            blobs
                .values()
                .all(|b| b.server == ServerState::Present && b.upload == Transfer::Complete)
        );
        // Fixture generation, transport, ACKs and driver observations are outside
        // this measurement. Both live-name and sendable-name lookups visit all N
        // distinct metadata values. Old linear searches cost N*(N+1) visits.
        let before = pair.sender.driver().entity_body_reads().unwrap();
        pair.sender.driver().reset_media_policy_blob_candidate_visits().unwrap();
        let facts = pair.sender.driver().policy_media_facts().unwrap();
        let visits = pair.sender.driver().media_policy_blob_candidate_visits().unwrap();
        let reads = pair.sender.driver().entity_body_reads().unwrap() - before;
        assert_eq!(facts, policy_media_expected((0, 0), (0, 0), 0, 0));
        assert_eq!(reads, count, "all M complete MediaName images must still be read");
        assert!(visits <= blobs.len() + 2 * count, "N={count}, visits={visits}");
        // A zero/unwired counter must not make the additive bound vacuous.
        assert!(visits >= 2 * count, "both lookup loops must select real candidates");
        pair.close();
    }
}

#[test]
fn media_policy_lookup_cost_real_shared_upload_present_and_tombstone_facts() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("Photo.JPG", LOCAL).unwrap();
    pair.sender.driver().put_media("shared.jpg", LOCAL).unwrap();
    pair.sender.driver().reconcile_binding().unwrap();
    assert_eq!(
        pair.sender.driver().policy_media_facts().unwrap(),
        policy_media_expected((1, LOCAL.len() as u64), (0, 0), 1, 0),
    );
    sync(&mut pair);
    assert_eq!(pair.sender.driver().media_blobs().unwrap().len(), 1);
    assert_eq!(
        pair.sender.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (0, 0), 0, 0)
    );
    pair.sender.driver().trash_media("Photo.JPG").unwrap();
    sync(&mut pair);
    assert_eq!(
        pair.sender.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (0, 0), 0, 0)
    );
    pair.sender.driver().trash_media("shared.jpg").unwrap();
    sync(&mut pair);
    assert_eq!(
        pair.sender.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (0, 0), 0, 0)
    );
    pair.close();
}

#[test]
fn media_policy_lookup_cost_real_inbox_overrides_local_image_before_download() {
    let mut pair = Pair::adopted();
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    sync(&mut pair);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], LOCAL);
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    sync(&mut pair);
    let entity = name(&pair.sender);
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    let record = state.zones[&pair.sender.collection_zone().unwrap()].records[&entity.name].clone();
    // Actual fetched record, held in the normal inbox without applying it.
    pair.receiver.driver().deliver_collection_records(&[record]).unwrap();
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    assert_eq!(pair.receiver.driver().inbox_entries(binding).unwrap().len(), 1);
    assert_eq!(
        pair.receiver.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (1, REMOTE.len() as u64), 1, 0),
    );
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    assert_eq!(
        pair.receiver.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (0, 0), 0, 0)
    );
    let blob = pair
        .receiver
        .driver()
        .media_blobs()
        .unwrap()
        .into_iter()
        .find(|(_, blob)| blob.metadata.sha256 == codec::sha256(REMOTE))
        .unwrap()
        .0;
    let deleted = pair
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: MEDIA.into(),
                atomic: false,
                changes: vec![Mutation::Delete { name: blob.clone() }],
            },
        )
        .unwrap();
    assert!(matches!(&deleted.result, Ok(Reply::Modified(items))
        if matches!(&items[0].result, Ok(MutationSuccess::Deleted))));
    // An outside file loss is a missing file, never a Meguri removal/tombstone.
    std::fs::remove_file(pair.receiver_dir.join("collection.media").join("photo.jpg")).unwrap();
    pair.receiver.driver().reconcile_binding().unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(
        pair.receiver.driver().media_blobs().unwrap()[&blob].server,
        ServerState::Deleted
    );
    assert_eq!(
        pair.receiver.driver().policy_media_facts().unwrap(),
        policy_media_expected((0, 0), (1, REMOTE.len() as u64), 0, 1)
    );
    pair.close();
}

#[test]
fn media_policy_lookup_cost_projection_rejects_damaged_local_base_ack_and_flight_bodies() {
    let mut pair = Pair::published();
    publish_old(&mut pair);
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    pair.sender.hold(Point::BeforeCollectionSave).unwrap();
    sync(&mut pair);
    assert!(pair.sender.is_held(Point::BeforeCollectionSave));
    let entity = name(&pair.sender);
    assert!(entity.base_payload.is_some() && entity.acked_payload.is_some() && !entity.in_flight.is_empty());
    let connection =
        rusqlite::Connection::open(pair.root.path().join("sender").join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
    let content: String = connection
        .query_row(
            "SELECT content FROM cloud_runtime_entities WHERE kind=9 AND key=?1",
            [entity.key.as_slice()],
            |row| row.get(0),
        )
        .unwrap();
    let header: serde_json::Value = serde_json::from_str(&content).unwrap();
    for field in ["local", "base", "acked", "flights"] {
        let image = &header["payload_images"][field];
        let image = if field == "flights" { &image[0] } else { image };
        let digest: Vec<u8> = serde_json::from_value(image.clone()).unwrap();
        assert_eq!(digest.len(), 32, "real {field} image reference");
        let bytes: Vec<u8> = connection
            .query_row(
                "SELECT bytes FROM cloud_runtime_payloads WHERE digest=?1",
                [&digest],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(
            connection
                .execute(
                    "UPDATE cloud_runtime_payloads SET bytes=X'ff' WHERE digest=?1",
                    [&digest]
                )
                .unwrap(),
            1
        );
        assert!(
            pair.sender.driver().policy_media_facts().is_err(),
            "accepted damaged {field} body"
        );
        assert_eq!(
            connection
                .execute(
                    "UPDATE cloud_runtime_payloads SET bytes=?1 WHERE digest=?2",
                    rusqlite::params![bytes, digest],
                )
                .unwrap(),
            1
        );
        assert!(pair.sender.driver().policy_media_facts().is_ok());
    }
    drop(connection);
    pair.sender.release(Point::BeforeCollectionSave).unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    pair.close();
}

fn sync(pair: &mut Pair) {
    let result = pair.sender.sync_now_and_drain(&mut pair.service, STEPS);
    result.unwrap_or_else(|error| panic!("{}: {error:?}", pair.sender.last_sync_phase()));
}

fn name(rig: &PublicationRig) -> EntityFacts {
    let facts = rig.facts().unwrap();
    let mut names = facts
        .entities
        .into_values()
        .filter(|e| e.kind == codec::Kind::MediaName);
    let entity = names.next().expect("real MediaName capture");
    assert!(names.next().is_none(), "no second name may protect the old content");
    entity
}

fn ack_tombstone(entity: &EntityFacts) -> bool {
    let codec::DecodeOutcome::Supported(acked) = codec::Entity::decode(&entity.acked_envelope.0).unwrap() else {
        panic!("supported actual ACK envelope required")
    };
    acked.fields.tombstone
}

fn cursor(pair: &Pair) -> u64 {
    pair.service.request_log().unwrap().last().map_or(0, |r| r.request_id)
}

fn quota(pair: &mut Pair, bytes: Option<u64>) {
    let reply = pair
        .service
        .request(&scope(), Caller::default(), Operation::SetQuota { bytes })
        .unwrap();
    assert!(matches!(reply.result, Ok(Reply::Done)));
}

fn publish_old(pair: &mut Pair) -> (String, Record, EntityFacts) {
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    sync(pair);
    let entity = name(&pair.sender);
    assert_eq!(entity.local_rev, entity.acked_rev);
    assert!(entity.acked_rev > 0 && entity.mark.is_none());
    assert!(!entity.acked_system_fields.is_empty());
    assert!(!ack_tombstone(&entity));
    let Some(Payload::MediaName(acked)) = &entity.acked_payload else {
        panic!("actual live MediaName ACK required")
    };
    assert_eq!(acked.sha256, codec::sha256(LOCAL));
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    assert!(
        state.zones[&pair.sender.collection_zone().unwrap()]
            .records
            .contains_key(&entity.name)
    );
    let blobs: Vec<_> = state.zones[MEDIA]
        .records
        .values()
        .filter(|r| r.record_type == "MGBlob")
        .collect();
    assert_eq!(blobs.len(), 1, "one actual uploaded old blob");
    let record = blobs[0].clone();
    assert!(matches!(record.fields.get("a"), Some(Field::Asset(a)) if !a.bytes().is_empty()));
    let tracked = pair.sender.driver().media_blobs().unwrap();
    assert_eq!(tracked[&record.name].metadata.sha256, codec::sha256(LOCAL));
    assert_eq!(tracked[&record.name].upload, Transfer::Complete);
    assert_eq!(tracked[&record.name].server, ServerState::Present);
    assert_unmasked(pair);
    (record.name.clone(), record, entity)
}

fn assert_unmasked(pair: &mut Pair) {
    let binding = pair.sender.facts().unwrap().current.unwrap().id;
    assert!(pair.sender.facts().unwrap().zones.is_empty());
    assert!(pair.sender.driver().media_conflicts().unwrap().is_empty());
    assert!(pair.sender.driver().inbox_entries(binding).unwrap().is_empty());
    assert!(pair.sender.driver().refused_entries(binding).unwrap().is_empty());
    assert!(pair.sender.driver().apply_intents(binding).unwrap().is_empty());
    let files = pair.sender.driver().media_files().unwrap();
    assert!(files.len() <= 1);
    assert!(files.keys().all(|n| n == "photo.jpg"));
    // The unused second device is unbound: it creates no hidden server name.
    assert!(pair.receiver.facts().unwrap().current.is_none());
}

fn assert_retained(pair: &mut Pair, blob: &str, original: &Record) {
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    assert_eq!(state.zones[MEDIA].records.get(blob), Some(original));
    assert_ne!(
        pair.sender.driver().media_blobs().unwrap()[blob].server,
        ServerState::Deleted
    );
    assert_ne!(pair.sender.sdk_pending().get(blob), Some(&PendingChange::Delete));
    assert_unmasked(pair);
}

// Pair's normal opener starts at NOW. Advance before doing any SDK work, so a
// reopened process sees the same deterministic wall-clock instant as its owner.
fn reopen(pair: &mut Pair, elapsed: i64) {
    pair.sender.close_clean().unwrap();
    pair.sender = online(&pair.root.path().join("sender"), false);
    pair.sender.advance_clock_ms(elapsed).unwrap();
}

fn assert_deleted_by_sdk(pair: &mut Pair, blob: &str, after: u64) {
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    assert!(
        !state.zones[MEDIA].records.contains_key(blob),
        "qualified cleanup must actually delete the retired MGBlob from Service"
    );
    let deletion = pair
        .service
        .outcomes()
        .unwrap()
        .into_iter()
        .find(|outcome| {
            outcome.request_id > after
                && matches!(&outcome.result, Ok(ReplyReference::Modified(items))
                if items.iter().any(|item| item.name == blob && matches!(&item.result, Ok(None))))
        })
        .expect("Service must persist a successful deletion result");
    let request = pair.sender.sent_request(deletion.request_id).unwrap();
    assert!(request.backend_batch_id > 0, "original typed SDK batch required");
    assert!(
        request
            .mutations
            .iter()
            .any(|m| matches!(m, Mutation::Delete { name } if name == blob))
    );
    let log = pair
        .service
        .request_log()
        .unwrap()
        .into_iter()
        .find(|r| r.request_id == deletion.request_id)
        .unwrap();
    assert_eq!(log.zone.as_deref(), Some(MEDIA));
    assert!(log.caller.instance.is_some());
    assert!(log.caller.own_operation.is_none());
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[blob].server,
        ServerState::Deleted
    );
    assert!(!pair.sender.sdk_pending().contains_key(blob));
}

#[derive(Clone, Copy)]
enum DelayedAck {
    ReplacementUpload,
    Removal,
    ReplacementName,
}

fn assert_pending_replacement(pair: &mut Pair, initial: &EntityFacts) {
    let pending = name(&pair.sender);
    assert_eq!(pending.name, initial.name);
    assert_eq!(pending.key, b"photo.jpg");
    assert!(pending.local_rev > pending.acked_rev);
    assert_eq!(pending.acked_rev, initial.acked_rev);
    assert_eq!(pending.acked_payload, initial.acked_payload);
    assert_eq!(pending.acked_envelope, initial.acked_envelope);
    assert_eq!(pending.acked_system_fields, initial.acked_system_fields);
    assert!(!ack_tombstone(&pending));
    assert!(matches!(&pending.payload, Payload::MediaName(value)
        if value.sha256 == codec::sha256(REMOTE)));
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    let server = &state.zones[&pair.sender.collection_zone().unwrap()].records[&initial.name];
    assert_eq!(server.system_fields.bytes(), initial.acked_system_fields);
    assert!(matches!(server.fields.get("e"), Some(Field::EncryptedBytes(envelope))
        if envelope == &initial.acked_envelope.0));
}

fn exercise_delayed_ack(mode: DelayedAck) {
    let tombstone = matches!(mode, DelayedAck::Removal);
    let name_quota = matches!(mode, DelayedAck::ReplacementName);
    let mut pair = Pair::published();
    let (blob, original, mut initial) = publish_old(&mut pair);
    let replacement_upload = if name_quota {
        // Warm the replacement through the same exact name, then ACK its old
        // content again. No second name can protect either blob in this case.
        let after = cursor(&pair);
        pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
        sync(&mut pair);
        let uploaded = name(&pair.sender);
        assert_eq!(uploaded.local_rev, uploaded.acked_rev);
        assert!(matches!(&uploaded.acked_payload, Some(Payload::MediaName(value))
            if value.sha256 == codec::sha256(REMOTE)));
        let tracked = pair.sender.driver().media_blobs().unwrap();
        let replacement = tracked
            .iter()
            .find(|(_, value)| value.metadata.sha256 == codec::sha256(REMOTE))
            .map(|(name, value)| {
                assert_eq!(value.upload, Transfer::Complete);
                assert_eq!(value.server, ServerState::Present);
                name.clone()
            })
            .expect("ordinary SDK upload of the replacement blob");
        assert_ne!(replacement, blob);
        let upload = pair
            .service
            .request_log()
            .unwrap()
            .into_iter()
            .find(|request| {
                request.request_id > after
                    && request.kind == RequestKind::Modify
                    && request.zone.as_deref() == Some(MEDIA)
                    && request
                        .records
                        .iter()
                        .any(|record| record.name == replacement && record.outcome == LogOutcome::Success)
            })
            .expect("original successful replacement blob save");
        assert!(upload.caller.instance.is_some() && upload.caller.own_operation.is_none());
        let sent = pair.sender.sent_request(upload.request_id).unwrap();
        assert!(sent.backend_batch_id > 0);
        assert!(sent.mutations.iter().any(|mutation| matches!(mutation,
            Mutation::Save { record, .. } if record.name == replacement
                && record.record_type == "MGBlob"
                && matches!(record.fields.get("a"), Some(Field::Asset(asset)) if !asset.bytes().is_empty()))));
        assert!(pair.service.outcomes().unwrap().iter().any(|outcome| {
            outcome.request_id == upload.request_id
                && matches!(&outcome.result, Ok(ReplyReference::Modified(items))
                    if items.iter().any(|item| item.name == replacement && matches!(&item.result, Ok(Some(_)))))
        }));
        pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
        sync(&mut pair);
        let restored = name(&pair.sender);
        assert_eq!(restored.name, initial.name);
        assert_eq!(restored.key, initial.key);
        assert_eq!(restored.local_rev, restored.acked_rev);
        assert!(restored.mark.is_none() && !ack_tombstone(&restored));
        assert_eq!(restored.acked_payload, initial.acked_payload);
        assert!(!restored.acked_system_fields.is_empty());
        initial = restored;
        assert_retained(&mut pair, &blob, &original);
        Some((replacement, upload.request_id))
    } else {
        quota(&mut pair, Some(0));
        None
    };
    if tombstone {
        pair.sender.driver().trash_media("photo.jpg").unwrap();
        assert!(pair.sender.driver().media_files().unwrap().is_empty());
    } else {
        pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
        assert_eq!(pair.sender.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    }
    let failed_after = cursor(&pair);
    let held_mutations = if let Some((replacement, uploaded_at)) = &replacement_upload {
        pair.sender.hold(Point::BeforeCollectionSave).unwrap();
        sync(&mut pair);
        assert!(pair.sender.is_held(Point::BeforeCollectionSave));
        let held = pair.sender.held_request().unwrap();
        let Operation::Modify { zone, atomic, changes } = held.operation else {
            panic!("original held collection save required")
        };
        assert_eq!(zone, pair.sender.collection_zone().unwrap());
        assert!(!atomic);
        assert!(held.caller.instance.is_some() && held.caller.own_operation.is_none());
        assert!(pair.service.request_log().unwrap().iter().all(|request| {
            request.request_id <= failed_after
                || request.kind != RequestKind::Modify
                || !request.records.iter().any(|record| record.name == initial.name)
        }));
        let state = pair.service.inspect(&scope()).unwrap().unwrap();
        let available = &state.zones[MEDIA].records[replacement];
        assert_eq!(available.record_type, "MGBlob");
        assert!(matches!(available.fields.get("a"), Some(Field::Asset(asset)) if !asset.bytes().is_empty()));
        assert!(*uploaded_at <= failed_after);
        assert_pending_replacement(&mut pair, &initial);
        assert_retained(&mut pair, &blob, &original);
        quota(&mut pair, Some(0));
        pair.sender.release(Point::BeforeCollectionSave).unwrap();
        Some(changes)
    } else {
        None
    };
    sync(&mut pair);
    let failed = pair
        .service
        .request_log()
        .unwrap()
        .into_iter()
        .find(|r| {
            r.request_id > failed_after
                && r.records.iter().any(|record| {
                    (if tombstone || name_quota {
                        record.name == initial.name
                    } else {
                        record.name == initial.name || r.zone.as_deref() == Some(MEDIA) && record.name != blob
                    }) && record.outcome == LogOutcome::Failure(ErrorCode::QuotaExceeded)
                })
        })
        .expect("the retiring tombstone save or replacement upload/save must really fail quota");
    if let Some(held) = held_mutations {
        assert_eq!(failed.kind, RequestKind::Modify);
        assert_eq!(failed.zone, Some(pair.sender.collection_zone().unwrap()));
        assert!(failed.caller.instance.is_some() && failed.caller.own_operation.is_none());
        let attempted = pair.sender.sent_request(failed.request_id).unwrap();
        assert!(attempted.backend_batch_id > 0);
        assert_eq!(attempted.mutations, held, "resume the original held SDK batch");
        assert_eq!(failed.caller.instance.as_deref(), Some(attempted.instance.as_str()));
        assert!(matches!(&attempted.response.result, Ok(Reply::Modified(items))
            if items.iter().any(|item| item.name == initial.name
                && matches!(&item.result, Err(error) if error.code == ErrorCode::QuotaExceeded))));
        assert!(pair.service.outcomes().unwrap().iter().any(|outcome| {
            outcome.request_id == failed.request_id
                && matches!(&outcome.result, Ok(ReplyReference::Modified(items))
                    if items.iter().any(|item| item.name == initial.name
                        && matches!(&item.result, Err(error) if error.code == ErrorCode::QuotaExceeded)))
        }));
        let key = pair.sender.facts().unwrap().current.unwrap().facts.record_key;
        let (record, system_fields) = attempted
            .mutations
            .iter()
            .find_map(|mutation| match mutation {
                Mutation::Save { record, if_unchanged } if record.name == initial.name => Some((record, if_unchanged)),
                _ => None,
            })
            .expect("specific original changed MediaName save");
        assert_eq!(record.record_type, "MGEntity");
        assert_eq!(system_fields.as_ref().unwrap().bytes(), initial.acked_system_fields);
        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
            panic!("original encrypted MediaName envelope required")
        };
        let received = codec::receive(&key.0, &record.name, envelope, codec::AssetInput::Absent).unwrap();
        assert_eq!(received.entity.fields.kind, codec::Kind::MediaName);
        assert_eq!(received.entity.fields.key, initial.key);
        assert_eq!(received.entity.fields.key, b"photo.jpg");
        assert!(!received.entity.fields.tombstone);
        let Some(Payload::MediaName(changed)) = received.payload else {
            panic!("original changed MediaName payload required")
        };
        assert_eq!(changed.name, b"photo.jpg");
        assert_eq!(changed.sha256, codec::sha256(REMOTE));
        assert_eq!(changed.size, REMOTE.len() as u64);
        assert!(replacement_upload.as_ref().unwrap().1 < failed.request_id);
        assert_pending_replacement(&mut pair, &initial);
    }
    if tombstone {
        let attempted = pair.sender.sent_request(failed.request_id).unwrap();
        assert!(attempted.backend_batch_id > 0);
        let key = pair.sender.facts().unwrap().current.unwrap().facts.record_key;
        assert!(
            attempted.mutations.iter().any(|mutation| {
                let Mutation::Save { record, .. } = mutation else {
                    return false;
                };
                if record.name != initial.name {
                    return false;
                }
                let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                    return false;
                };
                codec::receive(&key.0, &record.name, envelope, codec::AssetInput::Absent)
                    .expect("original attempted MediaName envelope")
                    .entity
                    .fields
                    .tombstone
            }),
            "the original SDK request attempted the pending media removal"
        );
    }
    // Exercise the actual drain and persisted acknowledgement boundary.
    for _ in 0..31 {
        pair.sender.advance_clock_ms(DAY).unwrap();
        sync(&mut pair);
        let pending = name(&pair.sender);
        assert_eq!(pending.acked_envelope, initial.acked_envelope);
        assert!(pending.local_rev > pending.acked_rev);
        assert!(!ack_tombstone(&pending));
        if name_quota {
            assert_pending_replacement(&mut pair, &initial);
        }
        assert_retained(&mut pair, &blob, &original);
    }
    reopen(&mut pair, 31 * DAY);
    assert_retained(&mut pair, &blob, &original);
    sync(&mut pair);
    assert_eq!(name(&pair.sender).acked_envelope, initial.acked_envelope);
    if name_quota {
        assert_pending_replacement(&mut pair, &initial);
    }
    quota(&mut pair, None);
    sync(&mut pair);
    let retired = name(&pair.sender);
    assert_eq!(retired.local_rev, retired.acked_rev);
    assert!(retired.mark.is_none());
    assert_eq!(ack_tombstone(&retired), tombstone);
    if !tombstone {
        let Some(Payload::MediaName(acked)) = retired.acked_payload else {
            panic!("replacement ACK required")
        };
        assert_eq!(acked.sha256, codec::sha256(REMOTE));
    }
    assert_retained(&mut pair, &blob, &original);
    reopen(&mut pair, 61 * DAY - 1);
    sync(&mut pair);
    assert_retained(&mut pair, &blob, &original);
    reopen(&mut pair, 61 * DAY);
    let after = cursor(&pair);
    sync(&mut pair);
    assert_deleted_by_sdk(&mut pair, &blob, after);
    reopen(&mut pair, 61 * DAY);
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].server,
        ServerState::Deleted
    );
    assert!(
        !pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
            .records
            .contains_key(&blob)
    );
    pair.close();
}

#[test]
fn replacement_ack_controls_retention() {
    exercise_delayed_ack(DelayedAck::ReplacementUpload);
}

#[test]
fn removal_ack_controls_retention() {
    exercise_delayed_ack(DelayedAck::Removal);
}

#[test]
fn changed_name_quota_after_blob_upload_controls_retention() {
    exercise_delayed_ack(DelayedAck::ReplacementName);
}

#[test]
fn original_engine_removal_survives_reopen() {
    let mut pair = Pair::published();
    let (blob, original, _) = publish_old(&mut pair);
    pair.sender.driver().trash_media("photo.jpg").unwrap();
    sync(&mut pair);
    let retired = name(&pair.sender);
    assert_eq!(retired.local_rev, retired.acked_rev);
    assert!(retired.mark.is_none() && ack_tombstone(&retired));
    assert!(pair.sender.driver().media_files().unwrap().is_empty());
    assert_retained(&mut pair, &blob, &original);
    reopen(&mut pair, 30 * DAY - 1);
    sync(&mut pair);
    assert_retained(&mut pair, &blob, &original);
    reopen(&mut pair, 30 * DAY);
    let after = cursor(&pair);
    sync(&mut pair);
    assert_deleted_by_sdk(&mut pair, &blob, after);
    reopen(&mut pair, 30 * DAY);
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].server,
        ServerState::Deleted
    );
    assert!(!pair.sender.sdk_pending().contains_key(&blob));
    assert!(pair.sender.facts().unwrap().current.is_some());
    pair.close();
}
#[test]
fn empty_replacement_preserves_server_reference_across_daily_drains() {
    let mut pair = Pair::published();
    let (blob, original, initial) = publish_old(&mut pair);
    pair.sender.driver().put_media("photo.jpg", b"").unwrap();
    for _ in 0..31 {
        pair.sender.advance_clock_ms(DAY).unwrap();
        sync(&mut pair);
        assert_eq!(name(&pair.sender).acked_envelope, initial.acked_envelope);
        assert_eq!(
            std::fs::read(pair.root.path().join("sender/collection.media/photo.jpg")).unwrap(),
            b""
        );
        assert_ne!(
            pair.sender.driver().media_blobs().unwrap()[&blob].download,
            Transfer::Running
        );
        assert_retained(&mut pair, &blob, &original);
    }
    reopen(&mut pair, 31 * DAY);
    assert_retained(&mut pair, &blob, &original);
    pair.sender.driver().trash_media("photo.jpg").unwrap();
    sync(&mut pair);
    assert!(ack_tombstone(&name(&pair.sender)));
    reopen(&mut pair, 61 * DAY - 1);
    sync(&mut pair);
    assert_retained(&mut pair, &blob, &original);
    reopen(&mut pair, 61 * DAY);
    let after = cursor(&pair);
    sync(&mut pair);
    assert_deleted_by_sdk(&mut pair, &blob, after);
    pair.close();
}

fn hold_due_blob(pair: &mut Pair) -> (String, Record) {
    let (blob, original, _) = publish_old(pair);
    pair.sender.driver().trash_media("photo.jpg").unwrap();
    sync(pair);
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].unreferenced_since_ms,
        Some(NOW)
    );
    pair.sender.advance_clock_ms(30 * DAY).unwrap();
    pair.sender.hold(Point::AfterCollectionCleanup).unwrap();
    sync(pair);
    assert!(pair.sender.is_held(Point::AfterCollectionCleanup));
    // The cleanup's real pending update has returned to the SDK by selection.
    (blob, original)
}

#[test]
fn original_missing_record_result_commits_blob_removal_without_collection_absence() {
    let mut pair = Pair::published();
    let (blob, _) = hold_due_blob(&mut pair);
    let selected = pair.sender.select_media_batch().unwrap().unwrap();
    assert!(matches!(&selected.1.request,meguri_sync::cloud::media::Request::Delete{name} if name==&blob));
    let raced = pair
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: MEDIA.into(),
                atomic: false,
                changes: vec![Mutation::Delete { name: blob.clone() }],
            },
        )
        .unwrap();
    assert!(
        matches!(raced.result,Ok(Reply::Modified(ref items)) if matches!(&items[0].result,Ok(MutationSuccess::Deleted)))
    );
    let response = pair.sender.send_selected_media(&mut pair.service, selected).unwrap();
    assert!(
        matches!(&response.response.result,Ok(Reply::Modified(items)) if items.iter().any(|i|i.name==blob && matches!(&i.result,Err(e) if e.code==ErrorCode::UnknownItem)))
    );
    assert!(response.backend_batch_id > 0);
    pair.sender.deliver_media_response(&response.response).unwrap();
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].server,
        ServerState::Deleted
    );
    let binding = pair.sender.facts().unwrap().current.unwrap().id;
    assert!(pair.sender.driver().inbox_entries(binding).unwrap().is_empty());
    assert!(!pair.sender.sdk_pending().contains_key(&blob));
    reopen(&mut pair, 30 * DAY);
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].server,
        ServerState::Deleted
    );
    pair.close();
}

#[test]
fn old_mark_is_cleared_and_new_qualified_drain_recomputes_it() {
    let mut pair = Pair::published();
    let (blob, original) = hold_due_blob(&mut pair);
    assert_eq!(
        pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms,
        Some(NOW + 30 * DAY)
    );
    pair.sender.advance_clock_ms(3_600_000).unwrap();
    assert!(pair.sender.select_media_batch().unwrap().is_none());
    assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
    assert_retained(&mut pair, &blob, &original);
    pair.sender.release(Point::AfterCollectionCleanup).unwrap();
    sync(&mut pair);
    assert_retained(&mut pair, &blob, &original);
    pair.sender.advance_clock_ms(DAY).unwrap();
    let after = cursor(&pair);
    sync(&mut pair);
    assert_deleted_by_sdk(&mut pair, &blob, after);
    pair.close();
}

#[test]
fn final_admission_rechecks_clock_after_the_original_batch_was_built() {
    let mut pair = Pair::published();
    let (blob, original) = hold_due_blob(&mut pair);
    let selected = pair.sender.select_media_batch().unwrap().unwrap();
    pair.sender.advance_clock_ms(-1).unwrap();
    assert!(pair.sender.send_selected_media(&mut pair.service, selected).is_err());
    assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
    assert_retained(&mut pair, &blob, &original);
    pair.close();
}

#[test]
fn fetched_server_name_blocks_both_builder_and_retained_batch_admission() {
    for build_first in [false, true] {
        let mut pair = Pair::published();
        let (blob, original) = hold_due_blob(&mut pair);
        let selected = if build_first {
            Some(pair.sender.select_media_batch().unwrap().unwrap())
        } else {
            None
        };
        // A second client connects to this collection and saves another file
        // name for the same content through its ordinary synchronization path.
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        pair.receiver.adopt_cloud(&preview).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        pair.receiver.driver().put_media("second.jpg", LOCAL).unwrap();
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let new_name = pair
            .receiver
            .facts()
            .unwrap()
            .entities
            .into_values()
            .find(|entity| entity.kind == codec::Kind::MediaName && entity.key == b"second.jpg")
            .unwrap();
        assert!(
            matches!(&new_name.acked_payload,Some(Payload::MediaName(value)) if value.sha256==codec::sha256(LOCAL))
        );
        pair.sender.fetch_without_sending(&mut pair.service, STEPS).unwrap();
        if let Some(selected) = selected {
            assert!(pair.sender.send_selected_media(&mut pair.service, selected).is_err());
        } else {
            assert!(pair.sender.select_media_batch().unwrap().is_none());
        }
        assert_eq!(
            pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
                .records
                .get(&blob),
            Some(&original)
        );
        assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
        assert!(!pair.sender.sdk_pending().contains_key(&blob));
        pair.close();
    }
}

#[test]
fn fetched_name_waiting_in_ready_inbox_or_media_intent_never_admits_removal() {
    for point in [Point::FetchAfterInbox, Point::MediaSettlementBeforeCommit] {
        let mut pair = Pair::published();
        let (blob, original) = hold_due_blob(&mut pair);
        let selected = pair.sender.select_media_batch().unwrap().unwrap();
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        pair.receiver.adopt_cloud(&preview).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        pair.receiver.driver().put_media("second.jpg", LOCAL).unwrap();
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        pair.sender.release(Point::AfterCollectionCleanup).unwrap();
        pair.sender.hold(point).unwrap();
        sync(&mut pair);
        assert!(pair.sender.is_held(point));
        let binding = pair.sender.facts().unwrap().current.unwrap().id;
        if point == Point::FetchAfterInbox {
            assert!(!pair.sender.driver().inbox_entries(binding).unwrap().is_empty());
        } else {
            assert!(
                pair.sender
                    .driver()
                    .apply_intents(binding)
                    .unwrap()
                    .iter()
                    .any(|intent| intent.media.is_some())
            );
        }
        assert!(pair.sender.send_selected_media(&mut pair.service, selected).is_err());
        assert_eq!(
            pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
                .records
                .get(&blob),
            Some(&original)
        );
        pair.sender.release(point).unwrap();
        sync(&mut pair);
        assert_eq!(
            pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
                .records
                .get(&blob),
            Some(&original)
        );
        pair.close();
    }
}

#[test]
fn unreadable_service_version_in_refused_tracking_blocks_removal() {
    let mut pair = Pair::published();
    let (blob, original) = hold_due_blob(&mut pair);
    let selected = pair.sender.select_media_batch().unwrap().unwrap();
    let zone = pair.sender.collection_zone().unwrap();
    let entity = name(&pair.sender);
    let state = pair.service.inspect(&scope()).unwrap().unwrap();
    let current = state.zones[&zone].records[&entity.name].clone();
    let response = pair
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone,
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&entity.name, "MGEntity", vec![0]),
                    if_unchanged: Some(current.system_fields),
                }],
            },
        )
        .unwrap();
    assert!(matches!(&response.result,Ok(Reply::Modified(items)) if items[0].result.is_ok()));
    pair.sender.fetch_without_sending(&mut pair.service, STEPS).unwrap();
    let binding = pair.sender.facts().unwrap().current.unwrap().id;
    assert!(
        pair.sender
            .driver()
            .refused_entries(binding)
            .unwrap()
            .iter()
            .any(|row| row.record_name == entity.name)
    );
    assert!(pair.sender.send_selected_media(&mut pair.service, selected).is_err());
    assert_eq!(
        pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
            .records
            .get(&blob),
        Some(&original)
    );
    assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
    pair.close();
}

#[track_caller]
fn assert_blob_read_after(pair: &Pair, blob: &str, after: u64, outcome: LogOutcome) {
    assert!(
        pair.service.request_log().unwrap().iter().any(|request| {
            request.request_id > after
                && request.zone.as_deref() == Some(MEDIA)
                && request.kind == RequestKind::Read
                && request
                    .records
                    .iter()
                    .any(|record| record.name == blob && record.outcome == outcome)
        }),
        "the media round must actually read the blob from Service"
    );
}

#[test]
fn stale_present_reuse_repairs_remote_deletion_within_the_freshness_window() {
    for deletion_order in ["before_reuse", "name_in_flight", "after_first_drain"] {
        for return_after_ms in [30 * 60_000, 2 * 60 * 60_000] {
            let mut pair = Pair::published();
            let (blob, original) = hold_due_blob(&mut pair);
            let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
            pair.receiver.adopt_cloud(&preview).unwrap();
            pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
            pair.receiver.advance_clock_ms(30 * DAY).unwrap();
            // Warm B's cache through a real name, read and ACK, then remove
            // that name. A has not fetched any of B's subsequent versions.
            pair.receiver.driver().put_media("reuse.jpg", LOCAL).unwrap();
            let warm_after = cursor(&pair);
            pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
            assert_blob_read_after(&pair, &blob, warm_after, LogOutcome::Success);
            pair.receiver.driver().trash_media("reuse.jpg").unwrap();
            pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
            assert_eq!(
                pair.receiver.driver().media_blobs().unwrap()[&blob].server,
                ServerState::Present
            );

            if deletion_order != "before_reuse" {
                pair.receiver.driver().put_media("reuse.jpg", LOCAL).unwrap();
                if deletion_order == "name_in_flight" {
                    pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
                }
                let named_after = cursor(&pair);
                pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
                let facts = pair.receiver.facts().unwrap();
                let naming = facts.entities.values().find(|e| e.key == b"reuse.jpg").unwrap();
                if deletion_order == "name_in_flight" {
                    assert!(pair.receiver.is_held(Point::BeforeCollectionSave));
                    assert!(!naming.in_flight.is_empty());
                    assert!(naming.local_rev > naming.acked_rev);
                    // This retains the real naming flight. The public rig
                    // cannot separately run its media read round while held,
                    // so this leg does not prove a warm Present at that ACK.
                    let request = pair.receiver.held_request().unwrap();
                    assert!(matches!(request.operation, Operation::Modify { ref changes, .. }
                        if changes.iter().any(|m| matches!(m, Mutation::Save { record, .. } if record.name == naming.name))));
                } else {
                    assert_eq!(naming.local_rev, naming.acked_rev);
                    assert_blob_read_after(&pair, &blob, named_after, LogOutcome::Success);
                    assert_eq!(
                        pair.receiver.driver().media_blobs().unwrap()[&blob].server,
                        ServerState::Present
                    );
                }
            }

            pair.sender.advance_clock_ms(return_after_ms).unwrap();
            let deletion_after = cursor(&pair);
            let selected = pair.sender.select_media_batch().unwrap();
            let deleted = return_after_ms < 60 * 60_000;
            if deleted {
                let selected = selected.expect("30-minute-old deletion remains eligible");
                assert!(
                    matches!(&selected.1.request, meguri_sync::cloud::media::Request::Delete { name } if name == &blob)
                );
                let response = pair.sender.send_selected_media(&mut pair.service, selected).unwrap();
                pair.sender.deliver_media_response(&response.response).unwrap();
                assert_deleted_by_sdk(&mut pair, &blob, deletion_after);
            } else {
                assert!(selected.is_none(), "a two-hour-old deletion must never reach Service");
                assert_eq!(
                    cursor(&pair),
                    deletion_after,
                    "expired selection makes no Service request"
                );
                assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
                assert!(!pair.sender.sdk_pending().contains_key(&blob));
            }

            if deletion_order == "before_reuse" {
                pair.receiver.driver().put_media("reuse.jpg", LOCAL).unwrap();
            } else if deletion_order == "name_in_flight" {
                pair.receiver.release(Point::BeforeCollectionSave).unwrap();
            }
            // The late deletion follows B's first drain. Only the qualified
            // 24-hour drain can discover it without another local media edit.
            pair.receiver
                .advance_clock_ms(if deletion_order == "after_first_drain" {
                    DAY
                } else {
                    return_after_ms
                })
                .unwrap();
            let repair_after = cursor(&pair);
            pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
            assert_blob_read_after(
                &pair,
                &blob,
                repair_after,
                if deleted {
                    LogOutcome::Failure(ErrorCode::UnknownItem)
                } else {
                    LogOutcome::Success
                },
            );
            let server = pair.service.inspect(&scope()).unwrap().unwrap();
            let restored = &server.zones[MEDIA].records[&blob];
            assert!(matches!(restored.fields.get("a"), Some(Field::Asset(asset)) if asset.bytes() == LOCAL));
            if deleted {
                let upload = pair
                    .service
                    .request_log()
                    .unwrap()
                    .into_iter()
                    .find(|request| {
                        request.request_id > repair_after
                            && request.zone.as_deref() == Some(MEDIA)
                            && request.kind == RequestKind::Modify
                            && request.records.iter().any(|r| r.name == blob)
                    })
                    .expect("B must reupload the deleted content");
                let sent = pair.receiver.sent_request(upload.request_id).unwrap();
                assert!(sent.backend_batch_id > 0);
                assert!(sent.mutations.iter().any(|m| matches!(m, Mutation::Save { record, .. }
                    if record.name == blob && matches!(record.fields.get("a"), Some(Field::Asset(a)) if a.bytes() == LOCAL))));
            } else {
                assert_eq!(*restored, original);
            }
            pair.sender.release(Point::AfterCollectionCleanup).unwrap();
            sync(&mut pair);
            assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
            assert!(!pair.sender.sdk_pending().contains_key(&blob));
            assert_eq!(pair.sender.driver().media_files().unwrap()["reuse.jpg"], LOCAL);
            assert_eq!(pair.receiver.driver().media_files().unwrap()["reuse.jpg"], LOCAL);
            pair.close();
        }
    }
}

#[test]
fn format_inbox_protects_blob_at_new_batch_request() {
    use meguri_sync::cloud::{
        InboxReason,
        runtime::{KeyByteSource, KeySourceError},
    };
    use meguri_testkit::cloud::devices::IdentityReads;
    use std::sync::Arc;

    struct NoReopenKeys;
    impl KeyByteSource for NoReopenKeys {
        fn draw_key(&self) -> std::result::Result<[u8; 32], KeySourceError> {
            Err(KeySourceError::Unavailable)
        }
    }
    for marked_before_fetch in [false, true] {
        let mut pair = Pair::published();
        pair.sender.close_clean().unwrap();
        pair.sender = PublicationRig::open(
            &pair.root.path().join("sender"),
            &scope(),
            IdentityReads::Available(scope().account),
            true,
            NOW,
            808,
            Arc::new(NoReopenKeys),
        )
        .unwrap();
        pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
        let (blob, original) = if marked_before_fetch {
            hold_due_blob(&mut pair)
        } else {
            let (blob, original, _) = publish_old(&mut pair);
            pair.sender.driver().trash_media("photo.jpg").unwrap();
            sync(&mut pair);
            pair.sender.advance_clock_ms(30 * DAY).unwrap();
            (blob, original)
        };
        if marked_before_fetch {
            let selected = pair
                .sender
                .select_media_batch()
                .unwrap()
                .expect("real stale SDK deletion");
            assert!(
                matches!(&selected.1.request, meguri_sync::cloud::media::Request::Delete { name } if name == &blob)
            );
            assert_eq!(pair.sender.sdk_pending().get(&blob), Some(&PendingChange::Delete));
            assert!(
                pair.sender.driver().media_blobs().unwrap()[&blob]
                    .deletion_due_ms
                    .is_some()
            );
        }
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        pair.receiver.adopt_cloud(&preview).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        pair.receiver.driver().put_media("future.jpg", LOCAL).unwrap();
        pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let entity = pair
            .receiver
            .facts()
            .unwrap()
            .entities
            .into_values()
            .find(|e| e.kind == codec::Kind::MediaName && e.key == b"future.jpg")
            .unwrap();
        let Some(Payload::MediaName(payload)) = &entity.acked_payload else {
            panic!("real live media ACK")
        };
        assert_eq!(payload.sha256, codec::sha256(LOCAL));
        let mut future = entity.acked_envelope.0;
        assert_eq!(&future[..2], &[8, 1]);
        future[1] = 2; // Only the format discriminator changes; the payload is intact.
        assert!(matches!(
            codec::Entity::decode(&future).unwrap(),
            codec::DecodeOutcome::UnsupportedFormat(2)
        ));
        let response = pair
            .service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone: pair.receiver.collection_zone().unwrap(),
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(&entity.name, "MGEntity", future),
                        if_unchanged: Some(SystemFields::from_bytes(entity.acked_system_fields)),
                    }],
                },
            )
            .unwrap();
        assert!(
            matches!(response.result, Ok(Reply::Modified(ref items)) if items.len() == 1 && items[0].result.is_ok())
        );

        pair.sender.release(Point::AfterCollectionCleanup).unwrap();
        let drains_before = pair.sender.driver().fetch_lifecycle_for_testing().unwrap().0.len();
        pair.sender.fetch_without_sending(&mut pair.service, STEPS).unwrap();
        let facts = pair.sender.facts().unwrap();
        assert!(facts.root_acked);
        let binding = facts.current.unwrap().id;
        let incoming = pair.sender.driver().incoming_observation(binding).unwrap();
        let sdk = incoming.engine_instance.expect("current SDK after the real fetch");
        assert!(pair.sender.trace().iter().any(|event|
            matches!(event, meguri_sync::cloud::workflow::TraceEvent::RootConfirmed { instance } if instance == &sdk)));
        let inbox = pair.sender.driver().inbox_entries(binding).unwrap();
        assert!(
            inbox
                .iter()
                .any(|entry| entry.record_name == entity.name && entry.reason == InboxReason::Format)
        );
        assert!(pair.sender.driver().refused_entries(binding).unwrap().is_empty());
        assert!(pair.sender.driver().apply_intents(binding).unwrap().is_empty());
        // Format stays durable without an apply intent. The public rig has no
        // read-only incoming-writer flag; the fresh builder must pass its gate.
        let zone = pair.sender.collection_zone().unwrap();
        let drains = pair.sender.driver().fetch_lifecycle_for_testing().unwrap().0;
        assert!(
            drains[drains_before..]
                .iter()
                .any(|drain| drain.zone == zone && drain.fetched)
        );
        // This asks the SDK for a new batch, consuming the drain's real full
        // UpdatePending before building; the Format entry remains unsettled.
        // fetch_without_sending does not refresh the public observation cache,
        // so Full emission cannot be separately observed here without driving
        // another scheduler round before this fresh builder.
        assert!(pair.sender.select_media_batch().unwrap().is_none());
        assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
        assert!(
            !pair.sender.sdk_pending().contains_key(&blob),
            "the stale SDK Delete must leave pending"
        );
        assert_eq!(
            pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
                .records
                .get(&blob),
            Some(&original)
        );
        sync(&mut pair);
        assert!(
            pair.sender
                .driver()
                .inbox_entries(binding)
                .unwrap()
                .iter()
                .any(|entry| entry.record_name == entity.name && entry.reason == InboxReason::Format)
        );
        assert!(pair.sender.select_media_batch().unwrap().is_none());
        assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
        assert_eq!(
            pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
                .records
                .get(&blob),
            Some(&original)
        );
        pair.close();
    }
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[test]
fn other_build_copy_alone_preserves_a_real_retired_service_blob() {
    use meguri_model::{
        clock::{FixedClock, UtcOffset},
        rng::SeededRandom,
    };
    use meguri_sync::cloud::{
        BindingFacts, BindingState, BuildScope, ConflictCopy, ConflictKind, ConflictReason, ContainerId, CopyId,
        Environment, IsolationReason, OpaqueBytes, OpenOutcome, PayloadDigest, RecordKey, TRACKING_FILE, TrackingStore,
    };
    let mut pair = Pair::published();
    let (blob, original, live) = publish_old(&mut pair);
    let current = pair.sender.facts().unwrap().current.unwrap();
    pair.sender.driver().trash_media("photo.jpg").unwrap();
    sync(&mut pair);
    assert!(ack_tombstone(&name(&pair.sender)));
    assert_unmasked(&mut pair);
    pair.sender.close_clean().unwrap();
    let directory = pair.root.path().join("sender");
    let other = BuildScope {
        container: ContainerId::new("test.cleanup.other-build").unwrap(),
        environment: Environment::Production,
    };
    let mut store = match TrackingStore::open(&directory, other.clone()).unwrap() {
        OpenOutcome::Opened(s) => s,
        _ => panic!("existing closed tracking fixture"),
    };
    let owner = store
        .create_binding(
            BindingFacts {
                record_key: RecordKey([79; 32]),
                state: BindingState::Isolated(IsolationReason::SignedOut),
                ..current.facts.clone()
            },
            &FixedClock::new(NOW, UtcOffset::UTC),
            &SeededRandom::new(919),
        )
        .unwrap();
    // The preservation input contains an actual earlier Service ACK. Its
    // isolated owner is a typed storage fixture, not UI resolution evidence.
    let mut copy = ConflictCopy {
        id: CopyId([80; 16]),
        kind: ConflictKind::MediaName,
        key: OpaqueBytes(live.key),
        reason: ConflictReason::MediaContent,
        losing_envelope: live.acked_envelope,
        losing_payload_digest: PayloadDigest(live.acked_payload_digest),
        winning_payload_digest: PayloadDigest([81; 32]),
        field_names: None,
        template_names: None,
        note_deck: None,
        staged_path: None,
        created_at_ms: NOW,
        resolved_at_ms: None,
    };
    store.store_conflict(owner.id, &copy).unwrap();
    drop(store);
    pair.sender = online(&directory, false);
    pair.sender.advance_clock_ms(30 * DAY).unwrap();
    sync(&mut pair);
    assert_eq!(
        pair.service.inspect(&scope()).unwrap().unwrap().zones[MEDIA]
            .records
            .get(&blob),
        Some(&original)
    );
    assert_eq!(pair.sender.driver().media_blobs().unwrap()[&blob].deletion_due_ms, None);
    assert!(!pair.sender.sdk_pending().contains_key(&blob));
    // Remove only the unresolved-copy condition, preserving every other input.
    pair.sender.close_clean().unwrap();
    copy.resolved_at_ms = Some(NOW + 30 * DAY);
    let connection = rusqlite::Connection::open(directory.join(TRACKING_FILE)).unwrap();
    assert_eq!(
        connection
            .execute(
                "UPDATE conflicts SET content=?1 WHERE id=?2",
                rusqlite::params![serde_json::to_string(&copy).unwrap(), copy.id.0.as_slice()]
            )
            .unwrap(),
        1
    );
    drop(connection);
    pair.sender = online(&directory, false);
    pair.sender.advance_clock_ms(30 * DAY).unwrap();
    let after = cursor(&pair);
    sync(&mut pair);
    assert_deleted_by_sdk(&mut pair, &blob, after);
    pair.close();
    let store = match TrackingStore::open(&directory, other).unwrap() {
        OpenOutcome::Opened(s) => s,
        _ => panic!("preserved owner"),
    };
    assert_eq!(store.conflict_copies().unwrap()[0].copy, copy);
    assert_eq!(store.binding(owner.id).unwrap().unwrap(), owner);
}

#[test]
fn held_final_service_callback_records_drain_once_without_refetching() {
    let mut pair = Pair::adopted();
    let (before, missing) = pair.receiver.driver().fetch_lifecycle_for_testing().unwrap();
    pair.receiver.hold(Point::FetchAfterCursor).unwrap();
    assert!(pair.receiver.fetch_without_sending(&mut pair.service, STEPS).is_err());
    assert!(pair.receiver.is_held(Point::FetchAfterCursor));
    assert_eq!(pair.receiver.driver().fetch_lifecycle_for_testing().unwrap().0, before);
    let requests = pair.service.request_log().unwrap().len();
    pair.receiver.release(Point::FetchAfterCursor).unwrap();
    // Resume the retained callback with no budget for another Service request.
    // The subsequent scheduler correctly reports its exhausted zero budget.
    assert!(pair.receiver.drive_ready(&mut pair.service, 0).is_err());
    assert!(pair.receiver.take_fetch_errors().is_empty());
    let (after, missing_after) = pair.receiver.driver().fetch_lifecycle_for_testing().unwrap();
    assert_eq!(missing_after, missing);
    assert_eq!(after.len(), before.len() + 2);
    assert!(after[before.len()..].iter().all(|drain| drain.fetched));
    assert!(after[before.len()..].iter().any(|drain| drain.zone == "meguri-root"));
    let zone = pair.receiver.collection_zone().unwrap();
    assert!(after[before.len()..].iter().any(|drain| drain.zone == zone));
    assert_eq!(pair.service.request_log().unwrap().len(), requests);
    // Re-entering the bounded driver cannot finalize that callback twice.
    assert!(pair.receiver.drive_ready(&mut pair.service, 0).is_err());
    assert_eq!(pair.receiver.driver().fetch_lifecycle_for_testing().unwrap().0, after);
    pair.close();
}

#[test]
fn account_change_during_held_service_callback_cannot_record_old_fetch_drain() {
    let mut pair = Pair::adopted();
    pair.receiver.hold(Point::FetchAfterCursor).unwrap();
    assert!(pair.receiver.fetch_without_sending(&mut pair.service, STEPS).is_err());
    assert!(pair.receiver.is_held(Point::FetchAfterCursor));
    pair.receiver
        .driver()
        .set_account_for_testing("other-synthetic-account".into())
        .unwrap();
    let before = pair.receiver.driver().fetch_lifecycle_for_testing().unwrap().0;
    let requests = pair.service.request_log().unwrap().len();
    pair.receiver.release(Point::FetchAfterCursor).unwrap();
    assert!(pair.receiver.drive_ready(&mut pair.service, 0).is_err());
    assert_eq!(
        pair.receiver.take_fetch_errors().len(),
        1,
        "the old SDK callback must fail with its original authority"
    );
    assert_eq!(pair.receiver.driver().fetch_lifecycle_for_testing().unwrap().0, before);
    assert_eq!(pair.service.request_log().unwrap().len(), requests);
    pair.close();
}

#[test]
fn service_media_tombstone_adoption_withholds_matching_and_differing_files_after_reopen() {
    for bytes in [LOCAL, REMOTE] {
        let mut pair = Pair::published();
        let (blob, _, _) = publish_old(&mut pair);
        pair.sender.driver().trash_media("photo.jpg").unwrap();
        sync(&mut pair);
        let tombstone = name(&pair.sender);
        assert!(ack_tombstone(&tombstone));
        let zone = pair.sender.collection_zone().unwrap();
        let server = pair.service.inspect(&scope()).unwrap().unwrap();
        let record = server.zones[&zone].records[&tombstone.name].clone();
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("actual Service tombstone envelope")
        };
        assert_eq!(*envelope, tombstone.acked_envelope.0);
        pair.receiver.driver().put_media("photo.jpg", bytes).unwrap();
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        let after = cursor(&pair);
        pair.receiver.adopt_cloud(&preview).unwrap();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        for reopen in [false, true] {
            if reopen {
                pair.receiver.close_clean().unwrap();
                pair.receiver = online(&pair.receiver_dir, false);
                pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
            }
            pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
            let adopted = name(&pair.receiver);
            assert!(ack_tombstone(&adopted));
            assert!(adopted.tombstone_at.is_none());
            assert!(adopted.mark.is_none() && adopted.in_flight.is_empty());
            assert!(adopted.acked_rev > 0);
            assert_eq!(adopted.local_rev, adopted.acked_rev);
            assert_eq!(adopted.server_erev, tombstone.server_erev);
            assert_eq!(adopted.acked_envelope.0, *envelope);
            assert_eq!(adopted.acked_system_fields, record.system_fields.bytes());
            assert_eq!(adopted.acked_payload, tombstone.acked_payload);
            assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], bytes);
            assert_eq!(
                pair.receiver.driver().collection_rows().unwrap(),
                pair.sender.driver().collection_rows().unwrap()
            );
            assert!(pair.receiver.sdk_pending().is_empty());
            assert!(
                pair.receiver
                    .driver()
                    .media_blobs()
                    .unwrap()
                    .values()
                    .all(|b| b.download == Transfer::None && b.upload == Transfer::None)
            );
            assert_eq!(pair.service.inspect(&scope()).unwrap().unwrap().zones, server.zones);
            assert!(
                !pair
                    .service
                    .request_log()
                    .unwrap()
                    .iter()
                    .any(|r| r.request_id > after && r.records.iter().any(|r| r.name == blob))
            );
        }
        pair.close();
    }
}

#[test]
fn service_media_tombstone_adoption_refuses_invalid_records_without_replacing_local_data() {
    for defect in [
        "missing_payload",
        "invalid_payload",
        "key",
        "hmac",
        "revision",
        "generation",
        "digest",
        "missing_asset",
        "other_tombstone",
    ] {
        let mut pair = Pair::published();
        publish_old(&mut pair);
        pair.sender.driver().trash_media("photo.jpg").unwrap();
        sync(&mut pair);
        let original = name(&pair.sender);
        assert!(ack_tombstone(&original));
        let codec::DecodeOutcome::Supported(mut entity) = codec::Entity::decode(&original.acked_envelope.0).unwrap()
        else {
            panic!("actual Service tombstone")
        };
        let mut payload = entity.payload.clone().unwrap();
        match defect {
            "missing_payload" => payload.clear(),
            "invalid_payload" => payload = vec![0xff],
            "key" => entity.fields.key = b"other.jpg".to_vec(),
            "revision" => entity.fields.erev = 0,
            "generation" => entity.fields.generation = Some([73; 16]),
            "other_tombstone" => {
                entity.fields.kind = codec::Kind::Note;
                entity.fields.key = codec::id_key(73).to_vec();
                payload = Payload::Note(codec::Note::default()).encode().unwrap();
            }
            _ => {}
        }
        let mut malformed = codec::Entity::build(entity.fields, &payload).unwrap();
        if defect == "digest" {
            malformed.payload_sha256[0] ^= 1;
        }
        if defect == "missing_asset" {
            malformed.payload = None;
        }
        let key = pair.sender.facts().unwrap().current.unwrap().facts.record_key;
        let record_name = codec::entity_name(
            if defect == "hmac" { &[74; 32] } else { &key.0 },
            malformed.fields.kind,
            &malformed.fields.key,
        )
        .unwrap();
        let if_unchanged =
            (record_name == original.name).then(|| SystemFields::from_bytes(original.acked_system_fields.clone()));
        let zone = pair.sender.collection_zone().unwrap();
        let response = pair
            .service
            .request(
                &scope(),
                Caller::default(),
                Operation::Modify {
                    zone,
                    atomic: false,
                    changes: vec![Mutation::Save {
                        record: RecordInput::encrypted(record_name, "MGEntity", malformed.encode().unwrap()),
                        if_unchanged,
                    }],
                },
            )
            .unwrap();
        let Reply::Modified(results) = response.result.unwrap() else {
            panic!("Service save")
        };
        assert!(results.iter().all(|result| result.result.is_ok()));
        pair.receiver.driver().put_media("photo.jpg", REMOTE).unwrap();
        let rows = pair.receiver.driver().collection_rows().unwrap();
        let before = pair.receiver.facts().unwrap();
        if let Ok(preview) = pair.receiver.preview(&mut pair.service, STEPS) {
            pair.receiver.adopt_cloud(&preview).unwrap();
            assert!(pair.receiver.drive_ready(&mut pair.service, STEPS).is_err(), "{defect}");
        }
        let after = pair.receiver.facts().unwrap();
        assert!(after.current == before.current && after.decision == before.decision);
        assert_eq!(after.native_file, before.native_file);
        assert!(after.recovery_ops.is_empty());
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows);
        assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
        pair.close();
    }
}

// These fixtures publish real files and receive actual Service ACKs before
// arranging the measured receipt. No tracking or flight authority is seeded.
const ACK_BATCH: usize = 250;
const RETIRING_SHARED: &[u8] = b"synthetic shared content retired by many ACKs";
const RETIRING_REPLACED: &[u8] = b"synthetic last reference replaced";
const RETIRING_REMOVED: &[u8] = b"synthetic last reference removed";

fn reference_cost_name(index: usize) -> String {
    format!("reference-cost-{index:04}.bin")
}

fn reference_cost_fixture(
    media_count: usize,
) -> (
    Pair,
    meguri_testkit::cloud::adapter::SDKRig,
    meguri_testkit::cloud::adapter::ResponseBatch,
) {
    use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
    struct NoNewKeys;
    impl KeyByteSource for NoNewKeys {
        fn draw_key(&self) -> std::result::Result<[u8; 32], KeySourceError> {
            Err(KeySourceError::Unavailable)
        }
    }
    let mut pair = Pair::published();
    for index in 0..media_count {
        let bytes = match index {
            0..=123 | 250 => LOCAL,
            124..=247 => RETIRING_SHARED,
            248 => RETIRING_REPLACED,
            249 => RETIRING_REMOVED,
            _ => REMOTE,
        };
        pair.sender
            .driver()
            .put_media(&reference_cost_name(index), bytes)
            .unwrap();
    }
    sync(&mut pair);
    let facts = pair.sender.facts().unwrap();
    let media: Vec<_> = facts
        .entities
        .values()
        .filter(|e| e.kind == codec::Kind::MediaName)
        .collect();
    assert_eq!(media.len(), media_count);
    assert!(
        media
            .iter()
            .all(|e| e.server_erev > 0 && e.mark.is_none() && e.in_flight.is_empty())
    );
    assert!(
        pair.sender
            .driver()
            .media_blobs()
            .unwrap()
            .values()
            .all(|blob| { blob.unreferenced_since_ms.is_none() && blob.deletion_due_ms.is_none() })
    );
    for index in 0..ACK_BATCH {
        if index % 2 == 1 {
            pair.sender.driver().trash_media(&reference_cost_name(index)).unwrap();
        } else {
            pair.sender
                .driver()
                .put_media(&reference_cost_name(index), REMOTE)
                .unwrap();
        }
    }
    pair.sender.close_clean().unwrap();
    let mut sdk = meguri_testkit::cloud::adapter::SDKRig::open_existing_collection(
        &pair.root.path().join("sender"),
        &scope(),
        NOW + DAY,
        808,
        std::sync::Arc::new(NoNewKeys),
    )
    .unwrap();
    sdk.reconcile_register_and_confirm_root(&mut pair.service, STEPS)
        .unwrap();
    let batch = sdk.request_collection_batch_response(&mut pair.service).unwrap();
    assert_eq!(batch.mutations.len(), ACK_BATCH);
    let Reply::Modified(items) = batch.response.result.as_ref().unwrap() else {
        panic!("actual handed Service receipt")
    };
    assert_eq!(items.len(), ACK_BATCH);
    for item in items {
        let MutationSuccess::Saved(record) = item.result.as_ref().unwrap() else {
            panic!("save fixture returned a different result variant")
        };
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("actual entity envelope")
        };
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(envelope).unwrap() else {
            panic!("supported entity")
        };
        assert_eq!(entity.fields.kind, codec::Kind::MediaName);
    }
    (pair, sdk, batch)
}

#[test]
fn media_ack_reference_cost_saved_loop_tracks_media_growth() {
    let mut costs = Vec::new();
    let mut event_costs = Vec::new();
    for media_count in [300, 600] {
        let (mut pair, mut sdk, batch) = reference_cost_fixture(media_count);
        // Fixture creation, reconciliation and real handoff are all excluded.
        // The event total includes pending-emission eligibility scans after ACKs.
        // The separate hook measures only the fully validated Saved ACK loop.
        let before = sdk.driver().entity_body_reads().unwrap();
        sdk.deliver_sent_event(&batch.response).unwrap();
        let reads = sdk.driver().entity_body_reads().unwrap() - before;
        let saved_reads = sdk
            .driver()
            .saved_ack_body_reads()
            .unwrap()
            .expect("Saved loop completed");
        eprintln!("Media ACK body reads: M={media_count} B={ACK_BATCH} event={reads} saved_loop={saved_reads}");
        event_costs.push((media_count, reads));
        costs.push((media_count, saved_reads));
        let blobs = sdk.driver().media_blobs().unwrap();
        for bytes in [RETIRING_SHARED, RETIRING_REPLACED, RETIRING_REMOVED] {
            let blob = blobs
                .values()
                .find(|blob| blob.metadata.sha256 == codec::sha256(bytes))
                .unwrap();
            assert_eq!(blob.unreferenced_since_ms, Some(NOW + DAY));
            assert_eq!(blob.deletion_due_ms, None);
        }
        for bytes in [LOCAL, REMOTE] {
            let blob = blobs
                .values()
                .find(|blob| blob.metadata.sha256 == codec::sha256(bytes))
                .unwrap();
            assert_eq!(
                blob.unreferenced_since_ms, None,
                "a shared live reference must protect content"
            );
            assert_eq!(blob.deletion_due_ms, None);
        }
        let Reply::Modified(items) = batch.response.result.as_ref().unwrap() else {
            unreachable!()
        };
        for item in items {
            let MutationSuccess::Saved(record) = item.result.as_ref().unwrap() else {
                unreachable!()
            };
            let e = sdk.entity_facts(&record.name).unwrap();
            assert!(e.mark.is_none() && e.in_flight.is_empty());
            assert_eq!(e.local_rev, e.acked_rev);
            assert_eq!(e.acked_system_fields, record.system_fields.bytes());
        }
        sdk.return_sent_handler().unwrap();
        sdk.close_clean().unwrap();
        pair.receiver.close_clean().unwrap();
    }
    for (media_count, reads) in &event_costs {
        assert!(
            *reads <= media_count + 6 * ACK_BATCH,
            "M={media_count} B={ACK_BATCH} event reads={reads}"
        );
    }
    for (media_count, reads) in &costs {
        // Allow per-record validation/materialization, but never B whole scans.
        assert!(
            *reads <= media_count + 6 * ACK_BATCH,
            "M={media_count} B={ACK_BATCH} reads={reads}"
        );
    }
    assert!(
        costs[1].1 <= costs[0].1 + 300,
        "adding 300 rows must add at most one scan: {costs:?}"
    );
}

#[test]
fn media_ack_reference_cost_malformed_tail_prevalidates_without_partial_ack() {
    let (mut pair, mut sdk, batch) = reference_cost_fixture(300);
    let Reply::Modified(items) = batch.response.result.as_ref().unwrap() else {
        unreachable!()
    };
    let before: Vec<_> = items.iter().map(|item| sdk.entity_facts(&item.name).unwrap()).collect();
    let mut malformed = batch.response.clone();
    let Reply::Modified(items) = malformed.result.as_mut().unwrap() else {
        unreachable!()
    };
    let MutationSuccess::Saved(record) = items.last_mut().unwrap().result.as_mut().unwrap() else {
        unreachable!()
    };
    record.fields.insert("e".into(), Field::EncryptedBytes(vec![0xff]));
    let reads = sdk.driver().entity_body_reads().unwrap();
    assert!(sdk.deliver_sent_event(&malformed).is_err());
    assert_eq!(sdk.driver().saved_ack_body_reads().unwrap(), None);
    assert_eq!(
        sdk.driver().entity_body_reads().unwrap(),
        reads,
        "no snapshot before receipt validation"
    );
    for e in before {
        assert_eq!(sdk.entity_facts(&e.name).unwrap(), e);
    }
    sdk.close_clean().unwrap();
    pair.receiver.close_clean().unwrap();
}

#[test]
fn media_ack_reference_cost_failed_second_commit_keeps_only_first_ack_durable() {
    let (mut pair, mut sdk, batch) = reference_cost_fixture(300);
    let Reply::Modified(items) = batch.response.result.as_ref().unwrap() else {
        unreachable!()
    };
    let before: Vec<_> = items.iter().map(|item| sdk.entity_facts(&item.name).unwrap()).collect();
    let key = before[1]
        .key
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let connection =
        rusqlite::Connection::open(pair.root.path().join("sender").join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
    connection
        .execute_batch(&format!(
            "CREATE TRIGGER reference_cost_abort BEFORE UPDATE ON cloud_runtime_entities WHEN NEW.key=X'{key}' BEGIN SELECT RAISE(ABORT,'synthetic ACK write failure'); END;"
        ))
        .unwrap();
    assert!(sdk.deliver_sent_event(&batch.response).is_err());
    assert_eq!(sdk.driver().saved_ack_body_reads().unwrap(), None);
    let first = sdk.entity_facts(&before[0].name).unwrap();
    assert!(first.server_erev > before[0].server_erev && first.in_flight.is_empty());
    for e in before.iter().skip(1) {
        assert_eq!(&sdk.entity_facts(&e.name).unwrap(), e);
    }
    connection.execute_batch("DROP TRIGGER reference_cost_abort").unwrap();
    drop(connection);
    sdk.close_clean().unwrap();
    pair.receiver.close_clean().unwrap();
}
