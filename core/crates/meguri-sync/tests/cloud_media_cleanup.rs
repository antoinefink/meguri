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

fn exercise_delayed_ack(tombstone: bool) {
    let mut pair = Pair::published();
    let (blob, original, initial) = publish_old(&mut pair);
    quota(&mut pair, Some(0));
    if tombstone {
        pair.sender.driver().trash_media("photo.jpg").unwrap();
        assert!(pair.sender.driver().media_files().unwrap().is_empty());
    } else {
        pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
        assert_eq!(pair.sender.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    }
    let failed_after = cursor(&pair);
    sync(&mut pair);
    let failed = pair
        .service
        .request_log()
        .unwrap()
        .into_iter()
        .find(|r| {
            r.request_id > failed_after
                && r.records.iter().any(|record| {
                    (if tombstone {
                        record.name == initial.name
                    } else {
                        record.name == initial.name || r.zone.as_deref() == Some(MEDIA) && record.name != blob
                    }) && record.outcome == LogOutcome::Failure(ErrorCode::QuotaExceeded)
                })
        })
        .expect("the retiring tombstone save or replacement upload/save must really fail quota");
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
        assert_retained(&mut pair, &blob, &original);
    }
    reopen(&mut pair, 31 * DAY);
    assert_retained(&mut pair, &blob, &original);
    sync(&mut pair);
    assert_eq!(name(&pair.sender).acked_envelope, initial.acked_envelope);
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
    exercise_delayed_ack(false);
}

#[test]
fn removal_ack_controls_retention() {
    exercise_delayed_ack(true);
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
