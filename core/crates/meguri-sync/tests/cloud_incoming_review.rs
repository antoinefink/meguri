//! Tests first: actual incremental incoming review and note edits, U113/U114.
//! Starting commit: d8c849a030b4a33b67c5614676d18e82b52e29d9.
//! Contracts: cloudkit-records K12.3, K13.1-3, K15, K17;
//! cloudkit-recovery V4/V5, V2.4 step 8; engine/op-model O1/O4.
//! Pair::adopted supplies real publication/adoption, never a seeded baseline.
//! Incoming provenance, crash, seq, WAL and intent proofs remain separate work.

#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::entity_codec::{Payload, Revlog, Stored};
use meguri_testkit::cloud::{
    LogOutcome, RequestKind,
    coordinator::{Point, PublicationRig},
};
use support::*;

fn history(rig: &mut PublicationRig) -> Vec<Revlog> {
    rig.driver()
        .collection_rows()
        .unwrap()
        .into_iter()
        .filter_map(|row| match row {
            Payload::Revlog(row) => Some(row),
            _ => None,
        })
        .collect()
}

fn last_request(pair: &Pair) -> u64 {
    pair.service.request_log().unwrap().last().unwrap().request_id
}

fn assert_collection_fetched(pair: &Pair, after: u64, zone: &str) {
    assert!(pair.service.request_log().unwrap().iter().any(|request| {
        request.request_id > after
            && request.kind == RequestKind::FetchZone
            && request.outcome == LogOutcome::Success
            && request.scope == scope()
            && request.zone.as_deref() == Some(zone)
    }));
}

#[test]
fn incoming_good_review_changes_card_and_exact_history_persists_and_is_not_duplicated() {
    let mut pair = Pair::adopted();
    let zone = pair.sender.collection_zone().unwrap();
    let note = notes(&mut pair.sender).into_iter().next().unwrap();
    let before = pair.sender.driver().collection_rows().unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    let prior_history = history(&mut pair.sender);
    assert!(prior_history.is_empty());
    assert_eq!(history(&mut pair.receiver), prior_history);

    // Every review call crosses ChildDriver into the actual CoreHost review
    // service. A nonzero millisecond remainder detects second-truncated ids.
    let session = pair.sender.driver().review_begin(note.id).unwrap();
    let question = pair.sender.driver().review_start(session).unwrap();
    let card_before = pair.sender.driver().read_card(question.id).unwrap();
    assert_eq!(card_before.nid, note.id);
    assert_eq!(pair.receiver.driver().read_card(question.id).unwrap(), card_before);
    pair.sender.advance_clock_ms(1_000).unwrap();
    pair.sender.driver().review_show_answer(session, question.id).unwrap();
    pair.sender.advance_clock_ms(1_537).unwrap();
    let answer = pair.sender.driver().review_answer_good(session, question.id).unwrap();
    let card_after = pair.sender.driver().read_card(question.id).unwrap();
    let revlog = pair.sender.driver().read_revlog(answer.revlog_id).unwrap();
    assert_eq!(card_after.id, card_before.id);
    assert_eq!(card_after.reps, card_before.reps + 1);
    assert_ne!(card_after.card_type, card_before.card_type);
    assert_ne!(card_after.due, card_before.due);
    assert_eq!(answer.revlog_id, NOW + 2_537);
    assert_eq!(revlog.id, answer.revlog_id);
    assert_eq!(card_after.last_review_id, answer.revlog_id);
    assert_eq!(revlog.cid, card_after.id);
    assert_eq!(revlog.ease, 3);
    assert_eq!(revlog.time, Stored::Integer(2_537));
    pair.sender.driver().review_end_and_drain(session).unwrap();

    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let sent_rows = pair.sender.driver().collection_rows().unwrap();
    let sent_history = history(&mut pair.sender);
    assert_ne!(sent_rows, before);
    assert_eq!(sent_history.len(), prior_history.len() + 1);
    assert_eq!(sent_history, vec![revlog.clone()]);
    assert_eq!(pair.sender.driver().read_card(question.id).unwrap(), card_after);
    // Publication/adoption finished before the edit: sending alone must not
    // change the other CoreHost's collection.
    assert_eq!(pair.receiver.driver().read_card(question.id).unwrap(), card_before);
    assert_eq!(history(&mut pair.receiver), prior_history);

    let after = last_request(&pair);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_collection_fetched(&pair, after, &zone);
    // These reads are actual collection rows, never tracking's acked payloads.
    // A fetched-baseline acknowledgement without a row apply must fail here.
    assert_eq!(pair.receiver.driver().read_card(question.id).unwrap(), card_after);
    assert_eq!(pair.receiver.driver().read_revlog(answer.revlog_id).unwrap(), revlog);
    assert_eq!(history(&mut pair.receiver), sent_history);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);

    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    // This key provider rejects every entropy draw on reopen.
    assert_eq!(pair.receiver.driver().read_card(question.id).unwrap(), card_after);
    assert_eq!(pair.receiver.driver().read_revlog(answer.revlog_id).unwrap(), revlog);
    assert_eq!(history(&mut pair.receiver), sent_history);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);

    // The existing SDK fetch exchange starts without a token, redelivering
    // these saved records through the same incoming path on this resync.
    let after = last_request(&pair);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_collection_fetched(&pair, after, &zone);
    assert_eq!(pair.receiver.driver().read_card(question.id).unwrap(), card_after);
    assert_eq!(pair.receiver.driver().read_revlog(answer.revlog_id).unwrap(), revlog);
    assert_eq!(history(&mut pair.receiver), sent_history);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);

    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.sender.driver().read_card(question.id).unwrap(), card_after);
    assert_eq!(pair.sender.driver().read_revlog(answer.revlog_id).unwrap(), revlog);
    assert_eq!(history(&mut pair.sender), sent_history);
    assert_eq!(pair.sender.driver().collection_rows().unwrap(), sent_rows);
    pair.close();
}

#[test]
fn incoming_note_field_edit_updates_actual_rows_and_survives_entropy_free_reopen() {
    let mut pair = Pair::adopted();
    let zone = pair.sender.collection_zone().unwrap();
    let note_before = notes(&mut pair.sender).into_iter().next().unwrap();
    assert!(notes(&mut pair.receiver).contains(&note_before));
    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(note_before.id, &["incoming question", "incoming answer"])
        .unwrap();
    let note_after = notes(&mut pair.sender)
        .into_iter()
        .find(|note| note.id == note_before.id)
        .unwrap();
    assert_ne!(note_after.flds, note_before.flds);
    assert_eq!(note_after.flds.as_slice(), b"incoming question\x1fincoming answer");
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let sent_rows = pair.sender.driver().collection_rows().unwrap();
    assert!(notes(&mut pair.receiver).contains(&note_before));

    let after = last_request(&pair);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_collection_fetched(&pair, after, &zone);
    let received = notes(&mut pair.receiver)
        .into_iter()
        .find(|note| note.id == note_before.id)
        .unwrap();
    assert_eq!(received, note_after);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    assert!(notes(&mut pair.receiver).contains(&note_after));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.sender.driver().collection_rows().unwrap(), sent_rows);
    pair.close();
}

#[test]
fn explicit_sync_after_reopen_survives_before_scan_hold_with_automatic_disabled() {
    let mut pair = Pair::adopted();
    let zone = pair.sender.collection_zone().unwrap();
    let before = pair.receiver.driver().collection_rows().unwrap();
    let note = notes(&mut pair.sender).into_iter().next().unwrap();
    pair.sender.advance_clock_ms(7_000).unwrap();
    pair.sender
        .driver()
        .edit_note_fields(note.id, &["held incoming question", "held incoming answer"])
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let sent_rows = pair.sender.driver().collection_rows().unwrap();
    assert_ne!(sent_rows, before);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);

    pair.receiver.close_clean().unwrap();
    // The support rig disables automatic sync and rejects entropy on reopen.
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.hold(Point::BeforeScan).unwrap();
    let after = last_request(&pair);
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeScan));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeScan));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), before);
    assert!(!pair.service.request_log().unwrap().iter().any(|request| {
        request.request_id > after
            && request.kind == RequestKind::FetchZone
            && request.zone.as_deref() == Some(zone.as_str())
    }));

    pair.receiver.release(Point::BeforeScan).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.is_held(Point::BeforeScan));
    assert_collection_fetched(&pair, after, &zone);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);

    // Driving an idle rig again must not submit the retained request twice.
    let completed = last_request(&pair);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.service.request_log().unwrap().iter().any(|request| {
        request.request_id > completed
            && request.kind == RequestKind::FetchZone
            && request.zone.as_deref() == Some(zone.as_str())
    }));
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), sent_rows);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
