//! Retain actual child replies across overlapping incoming and automatic SDK work.
//! Source base: 78dbd2c7 plus the supplied actual incoming harness seed.
//! Source-only regression handoff; no full-sync acceptance claim.

#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{
    ApplyFailure, InboxReason,
    entity_codec::{self as codec, Payload},
    runtime::{KeyByteSource, KeySourceError},
    testing::IncomingStage,
};
use meguri_testkit::cloud::{
    Field, Record,
    child_driver::{ApplyInterruption, IncomingProgress},
    coordinator::{Point, PublicationRig},
    devices::IdentityReads,
};
use std::{collections::BTreeSet, sync::Arc};
use support::*;

struct NoReopenKeys;
impl KeyByteSource for NoReopenKeys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        Err(KeySourceError::Unavailable)
    }
}

fn note(rig: &mut PublicationRig, id: i64) -> codec::Note {
    notes(rig).into_iter().find(|note| note.id == id).unwrap()
}

fn saved(pair: &Pair, name: &str) -> Record {
    let zone = pair.sender.collection_zone().unwrap();
    pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone].records[name].clone()
}

fn saved_note(record: &Record) -> codec::Note {
    let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
        panic!("actual Service envelope")
    };
    let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(bytes).unwrap() else {
        panic!("supported Service envelope")
    };
    let Payload::Note(note) = Payload::decode(entity.fields.kind, entity.payload.as_deref().unwrap()).unwrap() else {
        panic!("actual saved note")
    };
    note
}

fn retained_drain_reply(interruption: Option<ApplyInterruption>) {
    let mut pair = Pair::adopted();
    pair.receiver.close_clean().unwrap();
    pair.receiver = PublicationRig::open(
        &pair.receiver_dir,
        &scope(),
        IdentityReads::Available(scope().account),
        true,
        NOW,
        809,
        Arc::new(NoReopenKeys),
    )
    .unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();

    let current = pair.receiver.facts().unwrap().current.unwrap();
    let binding = current.id;
    assert!(
        pair.receiver
            .driver()
            .incoming_observation(binding)
            .unwrap()
            .runs
            .is_empty()
    );
    let baseline = notes(&mut pair.receiver);
    assert_eq!(baseline.len(), 2);
    let remote_id = baseline[0].id;
    let local_id = baseline[1].id;
    assert_ne!(remote_id, local_id);
    let remote_name = codec::entity_name(
        &current.facts.record_key.0,
        codec::Kind::Note,
        &codec::id_key(remote_id),
    )
    .unwrap();
    let local_name =
        codec::entity_name(&current.facts.record_key.0, codec::Kind::Note, &codec::id_key(local_id)).unwrap();
    let remote_before = note(&mut pair.receiver, remote_id);
    let calls_before = pair.receiver.send_changes_calls().unwrap().len();
    pair.receiver
        .driver()
        .edit_note_fields(local_id, &["independent local question", "local answer"])
        .unwrap();
    let local_after = note(&mut pair.receiver, local_id);
    assert!(pair.receiver.facts().unwrap().entities[&local_name].mark.is_some());
    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(remote_id, &["remote reply question", "remote reply answer"])
        .unwrap();
    let remote_after = note(&mut pair.sender, remote_id);
    assert_ne!(remote_after, remote_before);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let record = saved(&pair, &remote_name);
    assert_eq!(saved_note(&record), remote_after);
    pair.receiver.driver().deliver_collection_records(&[record]).unwrap();
    assert_eq!(note(&mut pair.receiver, remote_id), remote_before);
    let entries = pair.receiver.driver().inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].record_name, remote_name);

    let (point, stage) = if let Some(interruption) = interruption {
        pair.receiver.driver().interrupt_next_incoming(interruption).unwrap();
        (Point::IncomingRollbackComplete, IncomingStage::IncomingRollbackComplete)
    } else {
        (Point::ApplyAfterCommit, IncomingStage::ApplyAfterCommit)
    };
    pair.receiver.hold(point).unwrap();
    pair.receiver.hold(Point::BeforeCollectionSave).unwrap();
    // A is the direct grant's real reply, left unconsumed while the SDK runs B.
    assert!(matches!(
        pair.receiver.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Paused(actual) if actual == stage
    ));
    assert!(pair.receiver.is_held(point));
    let paused = pair.receiver.driver().incoming_observation(binding).unwrap();
    if let Some(interruption) = interruption {
        assert!(paused.runs.is_empty());
        assert_eq!(note(&mut pair.receiver, remote_id), remote_before);
        assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
        let entries = pair.receiver.driver().inbox_entries(binding).unwrap();
        assert_eq!(entries.len(), 1);
        if interruption == ApplyInterruption::Cancel {
            assert_eq!(entries[0].reason, InboxReason::Ready);
            assert_eq!(entries[0].retry_count, 0);
        } else {
            assert!(matches!(
                entries[0].reason,
                InboxReason::ApplyFailed {
                    failure: ApplyFailure::Apply,
                    ..
                }
            ));
            assert_eq!(entries[0].retry_count, 1);
        }
    } else {
        assert_eq!(paused.runs.len(), 1);
        assert_eq!(paused.runs[0].entry_ids, vec![entries[0].id]);
        assert_eq!(paused.runs[0].changed_keys, BTreeSet::from([remote_name.clone()]));
        assert_eq!(note(&mut pair.receiver, remote_id), remote_after);
    }

    pair.receiver.release(point).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(
        pair.receiver.is_held(Point::BeforeCollectionSave),
        "automatic SDK Build B must pause"
    );
    pair.receiver.release(Point::BeforeCollectionSave).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.is_held(Point::BeforeCollectionSave));
    assert_eq!(saved_note(&saved(&pair, &local_name)), local_after);
    assert!(pair.receiver.facts().unwrap().entities[&local_name].mark.is_none());
    let calls = pair.receiver.send_changes_calls().unwrap();
    assert!(
        calls[calls_before..]
            .iter()
            .any(|call| { call.automatic && call.exchange.is_none() && call.returned && !call.request_ids.is_empty() })
    );

    let before_reply = pair.receiver.driver().incoming_observation(binding).unwrap();
    let rows_before_reply = pair.receiver.driver().collection_rows().unwrap();
    let failed_apply = interruption == Some(ApplyInterruption::Error);
    assert_eq!(before_reply.runs.len(), usize::from(!failed_apply));
    if !failed_apply {
        assert_eq!(before_reply.runs[0].entry_ids, vec![entries[0].id]);
        assert_eq!(before_reply.runs[0].changed_keys, BTreeSet::from([remote_name]));
    }
    assert_eq!(
        note(&mut pair.receiver, remote_id),
        if failed_apply { remote_before } else { remote_after }
    );
    if interruption.is_some() {
        // Both writer interruptions happened in A; B has since saved normally.
        pair.receiver
            .driver()
            .drain_incoming(STEPS)
            .expect_err("A's original writer error must survive B's completed reply");
    } else {
        assert!(
            matches!(
                pair.receiver.driver().drain_incoming(STEPS).unwrap(),
                IncomingProgress::Completed(1)
            ),
            "A must return its original nonzero count instead of resubmitting an empty drain"
        );
    }
    assert_eq!(
        pair.receiver.driver().incoming_observation(binding).unwrap().runs,
        before_reply.runs
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows_before_reply);
    // The original reply is consumed once; a fresh identical grant is empty.
    assert!(matches!(
        pair.receiver.driver().drain_incoming(STEPS).unwrap(),
        IncomingProgress::Completed(0)
    ));
    assert_eq!(
        pair.receiver.driver().incoming_observation(binding).unwrap().runs,
        before_reply.runs
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), rows_before_reply);
    pair.close();
}

#[test]
fn original_incoming_count_survives_a_later_automatic_build_reply() {
    retained_drain_reply(None);
}

#[test]
fn original_incoming_cancel_survives_a_later_automatic_build_reply() {
    retained_drain_reply(Some(ApplyInterruption::Cancel));
}

#[test]
fn original_incoming_error_survives_a_later_automatic_build_reply() {
    retained_drain_reply(Some(ApplyInterruption::Error));
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
