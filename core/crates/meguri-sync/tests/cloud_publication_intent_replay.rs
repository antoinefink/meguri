//! Tests FIRST: all four Q11 K15.9/K9.3/K10.3 move/rewrite replay variants.
//! Enabled missing-path RED. Root extracts, reviews, observes RED and commits
//! this target and support/publication_incoming.rs BEFORE production application.
//! Additional Point values below must gate actual source-process work. They
//! must never be implemented by advancing a rig-owned model or clearing rows.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{BindingId, entity_codec::Payload};
use meguri_testkit::cloud::coordinator::Point;
use support::*;

fn conflict(point: Point, both_names: bool) -> (Pair, BindingId) {
    let mut pair = Pair::adopted();
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    let fields = if both_names {
        format!("<img src=\"photo.jpg\"><img src=\"{}\">", hash_name())
    } else {
        "<img src=\"photo.jpg\">".into()
    };
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&fields, "local back"])
            .unwrap();
    }
    assert_eq!(
        pair.receiver.driver().put_media("photo.jpg", LOCAL).unwrap(),
        "photo.jpg"
    );
    assert_eq!(
        pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap(),
        "photo.jpg"
    );
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(point).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(point));
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap().len(), 1);
    (pair, binding)
}
fn assert_finished(pair: &mut Pair, binding: BindingId) {
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files["photo.jpg"], REMOTE);
    assert_eq!(files[&hash_name()], LOCAL);
    for note in notes(&mut pair.receiver) {
        let fields = std::str::from_utf8(&note.flds).unwrap();
        assert!(fields.contains(&hash_name()));
        assert!(!fields.contains("src=\"photo.jpg\""));
    }
    // Drain actual sends and check the named file became an acknowledged
    // kind-9 entity, rather than only a local filesystem move.
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let facts = pair.receiver.facts().unwrap();
    let moved = facts
        .entities
        .values()
        .find(|entity| {
            matches!(&entity.payload,
        Payload::MediaName(media) if media.name == hash_name().as_bytes())
        })
        .unwrap();
    assert_eq!(moved.local_rev, moved.acked_rev);
    assert!(moved.mark.is_none());
}

#[test]
fn q11_kill_before_move_then_fixed_name_write_before_step8_rewrites_no_note() {
    let (mut pair, binding) = conflict(Point::ResolutionBeforeMove, false);
    let before = notes(&mut pair.receiver);
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    // Exact-name MediaService write, while the verified identity is unavailable.
    // This must not use attachment-add's collision name or overwrite tracking.
    let replacement = b"synthetic fixed-name write after crash";
    assert_eq!(
        pair.receiver.driver().put_media("photo.jpg", replacement).unwrap(),
        "photo.jpg"
    );
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert_eq!(notes(&mut pair.receiver), before);
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files["photo.jpg"], replacement);
    assert!(!files.contains_key(&hash_name()));
    assert!(
        !pair
            .receiver
            .facts()
            .unwrap()
            .entities
            .values()
            .any(|entity| matches!(&entity.payload,
        Payload::MediaName(media) if media.name == hash_name().as_bytes()))
    );
    // Read the real typed inbox projection; abandoning the move must leave
    // the original incoming version ready for a later, freshly planned apply.
    assert!(
        pair.receiver
            .driver()
            .inbox_entries(binding)
            .unwrap()
            .iter()
            .any(|entry| entry.reason == meguri_sync::cloud::InboxReason::Ready)
    );
    pair.close();
}

#[test]
fn q11_kill_before_move_replays_the_move_before_the_note_operation() {
    let (mut pair, binding) = conflict(Point::ResolutionBeforeMove, false);
    let before = notes(&mut pair.receiver);
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    pair.receiver.hold(Point::ResolutionMoveBeforeRewrite).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ResolutionMoveBeforeRewrite));
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files[&hash_name()], LOCAL);
    assert!(!files.contains_key("photo.jpg"));
    assert_eq!(notes(&mut pair.receiver), before);
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap().len(), 1);
    pair.receiver.release(Point::ResolutionMoveBeforeRewrite).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_finished(&mut pair, binding);
    pair.close();
}

fn edit_every_key_before_step8(pair: &mut Pair) {
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    let rows = notes(&mut pair.receiver);
    assert!(rows.len() >= 2);
    for note in rows {
        // Both references were in the BEFORE image. The mere presence of the
        // hash name must not be mistaken for proof the rewrite already ran.
        let fields = format!(
            "edited {} <img src=\"photo.jpg\"><img src=\"{}\">",
            note.id,
            hash_name()
        );
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&fields, "edited back"])
            .unwrap();
    }
}

#[test]
fn q11_rerun_after_every_key_is_edited_keeps_edits_and_leaves_no_old_name() {
    let (mut pair, binding) = conflict(Point::ResolutionMoveBeforeRewrite, true);
    edit_every_key_before_step8(&mut pair);
    pair.verify_and_drive();
    assert_finished(&mut pair, binding);
    for note in notes(&mut pair.receiver) {
        assert!(
            std::str::from_utf8(&note.flds)
                .unwrap()
                .starts_with(&format!("edited {} ", note.id))
        );
        assert!(note.flds.ends_with(b"\x1fedited back"));
    }
    pair.close();
}

#[test]
fn q11_kill_after_rerun_commit_settles_recorded_digests_without_a_third_run() {
    let (mut pair, binding) = conflict(Point::ResolutionMoveBeforeRewrite, true);
    edit_every_key_before_step8(&mut pair);
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    let rewritten = notes(&mut pair.receiver);
    assert!(
        rewritten
            .iter()
            .all(|note| !std::str::from_utf8(&note.flds).unwrap().contains("src=\"photo.jpg\""))
    );
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap().len(), 1);
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.verify_and_drive();
    // The gate is at a real rewrite COMMIT. Reaching it again would be the
    // forbidden third run, even if the row operation were idempotent.
    assert!(!pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_eq!(notes(&mut pair.receiver), rewritten);
    assert_finished(&mut pair, binding);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
