//! Tests FIRST: actual V14 staging, replacement provenance and restart refusal.
//! Enabled missing-path RED; root observes and commits this target and its
//! support file before production. Point gates must hold real Runtime work.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_engine::{device_state::DeviceState, replace::ReplaceReason};
use meguri_sync::cloud::entity_codec::Kind;
use meguri_testkit::cloud::{Caller, LogOutcome, Mutation, Operation, RequestKind, coordinator::Point};
use support::*;

#[test]
fn cloud_adopt_uses_fetched_rows_installs_media_and_keeps_an_actual_safety_backup() {
    let mut pair = Pair::published();
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &["unpublished receiver content", "local back"])
            .unwrap();
    }
    pair.receiver
        .driver()
        .put_media("photo.jpg", b"superseded local image")
        .unwrap();
    pair.receiver
        .driver()
        .put_media("withheld.bin", b"unrelated retained file")
        .unwrap();
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let local_rows = pair.receiver.driver().collection_rows().unwrap();
    let remote_rows = pair.sender.driver().collection_rows().unwrap();
    assert_ne!(local_rows, remote_rows);
    let old = pair.receiver.facts().unwrap().decision.publication;
    let server = pair.service.inspect(&scope()).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.facts().unwrap();
    assert!(adopted.recovery_ops.is_empty());
    assert!(adopted.current.is_some());
    assert_ne!(adopted.decision.publication.identity, old.identity);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote_rows);
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files["photo.jpg"], REMOTE);
    assert_eq!(files["withheld.bin"], b"unrelated retained file");
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(
        pair.service.inspect(&scope()).unwrap().unwrap().zones,
        server.unwrap().zones
    );
    pair.receiver.close_clean().unwrap();
    // Read only the now-closed synthetic collection's real durable R6 evidence.
    let evidence = DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
        .replacement_evidence_for(old.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, old);
    assert_eq!(evidence.candidate, adopted.decision.publication);
    let backup = evidence
        .safety_backup
        .expect("CloudAdopt must retain the replaced collection");
    pair.receiver = online(&pair.receiver_dir, false);
    let backup_path = pair.receiver.driver().backup_path(&backup.name).unwrap();
    assert!(backup_path.is_file());
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        local_rows
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote_rows);
    pair.close();
}

#[test]
fn cloud_adopt_refuses_a_fetched_graph_with_a_missing_notetype_without_replacing_local_data() {
    let mut pair = Pair::published();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let before = pair.receiver.facts().unwrap();
    let source = pair.sender.facts().unwrap();
    let name = source
        .entities
        .values()
        .find(|entity| entity.kind == Kind::Notetype)
        .unwrap()
        .name
        .clone();
    let zone = pair.sender.collection_zone().unwrap();
    // Real service deletion after a real publication: the staged reference
    // validator must reject this graph, not infer the missing row from a fixture.
    let reply = pair
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: zone.clone(),
                atomic: false,
                changes: vec![Mutation::Delete { name }],
            },
        )
        .unwrap();
    assert!(reply.result.is_ok());
    let after = pair.service.request_log().unwrap().last().unwrap().request_id;
    // V13 may discover the refusal in prefetch. Without a completed prefetch,
    // V14 must discover it in staging; both must have fetched actual zone data.
    if let Ok(preview) = pair.receiver.preview(&mut pair.service, STEPS) {
        pair.receiver.adopt_cloud(&preview).unwrap();
        assert!(pair.receiver.drive_ready(&mut pair.service, STEPS).is_err());
    }
    assert!(
        pair.service
            .request_log()
            .unwrap()
            .iter()
            .any(|request| request.request_id > after
                && request.kind == RequestKind::FetchZone
                && request.outcome == LogOutcome::Success
                && request.zone.as_deref() == Some(zone.as_str()))
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    let refused = pair.receiver.facts().unwrap();
    assert!(refused.current == before.current);
    assert!(refused.decision == before.decision);
    assert!(refused.recovery_ops.is_empty());
    assert_eq!(refused.native_file, before.native_file);
    pair.close();
}

#[test]
fn cloud_adopt_killed_after_record_discards_staging_and_keeps_the_original_collection() {
    let mut pair = Pair::published();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let before = pair.receiver.facts().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionStagingRecorded));
    assert!(!pair.receiver.facts().unwrap().recovery_ops.is_empty());
    pair.receiver.kill().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    let reopened = pair.receiver.facts().unwrap();
    assert!(reopened.recovery_ops.is_empty());
    assert!(reopened.current == before.current);
    assert!(reopened.decision == before.decision);
    assert_eq!(reopened.native_file, before.native_file);
    pair.close();
}

#[test]
fn account_change_cancels_recorded_adoption_before_any_collection_swap() {
    let mut pair = Pair::published();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let before = pair.receiver.facts().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionStagingRecorded));
    pair.receiver.verify_identity("synthetic-other-account".into()).unwrap();
    pair.receiver.release(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    let cancelled = pair.receiver.facts().unwrap();
    assert!(cancelled.recovery_ops.is_empty());
    assert!(cancelled.current == before.current);
    assert!(cancelled.decision == before.decision);
    assert_eq!(cancelled.native_file, before.native_file);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
