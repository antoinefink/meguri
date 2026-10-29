//! Focused tests FIRST: root must extract/observe/commit this file before production.
#[path = "support/publication_incoming.rs"]
mod support;
use meguri_engine::{device_state::DeviceState, replace::ReplaceReason};
use meguri_sync::cloud::adoption::{AdoptStep, RecoveryKind};
use meguri_testkit::cloud::{RequestKind, coordinator::Point};
use support::*;

#[test]
fn fetched_empty_media_collection_replaces_local_and_reopens_with_its_safety_package() {
    let mut pair = Pair::published();
    // Key provider refuses every draw, including after relaunch.
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    let before = pair.receiver.facts().unwrap();
    let local = pair.receiver.driver().collection_rows().unwrap();
    let remote = pair.sender.driver().collection_rows().unwrap();
    let source = pair.sender.facts().unwrap();
    let source_header = pair.sender.driver().collection_header().unwrap();
    assert_ne!(local, remote);
    assert!(pair.receiver.driver().media_files().unwrap().is_empty());
    assert!(pair.sender.driver().media_files().unwrap().is_empty());
    let remote_zone = pair.sender.collection_zone().unwrap();
    let after = pair.service.request_log().unwrap().last().unwrap().request_id;
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let staging = pair.receiver.facts().unwrap();
    assert!(matches!(&staging.recovery_ops[0].kind, RecoveryKind::Adopt(op) if op.step == AdoptStep::Staging));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    pair.receiver.release(Point::AdoptionStagingRecorded).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.facts().unwrap();
    assert!(adopted.recovery_ops.is_empty());
    let bound = adopted.current.as_ref().unwrap();
    let remote_binding = source.current.as_ref().unwrap();
    assert_eq!(bound.facts.epoch, remote_binding.facts.epoch);
    assert!(bound.facts.record_key == remote_binding.facts.record_key);
    assert!(adopted.media_key == source.media_key);
    assert_ne!(
        adopted.decision.publication.identity,
        before.decision.publication.identity
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    assert_eq!(
        pair.receiver.driver().collection_header().unwrap().crt,
        source_header.crt
    );
    assert!(pair.service.request_log().unwrap().iter().any(|r| r.request_id > after
        && r.kind == RequestKind::FetchZone
        && r.zone.as_deref() == Some(remote_zone.as_str())));
    pair.receiver.close_clean().unwrap();
    let evidence = DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
        .replacement_evidence_for(before.decision.publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert!(evidence.candidate == adopted.decision.publication);
    let backup = evidence.safety_backup.unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let reopened = pair.receiver.facts().unwrap();
    assert!(reopened.decision == adopted.decision);
    assert!(reopened.current.as_ref().unwrap().facts == bound.facts);
    assert_eq!(reopened.native_file, adopted.native_file);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), remote);
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        local
    );
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
