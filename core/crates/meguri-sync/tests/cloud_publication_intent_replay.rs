//! Q11 K15.9/K9.3/K10.3 move/rewrite replay witnesses over actual child work.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::{
    BindingId,
    entity_codec::{self as codec, Payload},
};
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
    let intents = pair.receiver.driver().apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    // Real adoption replaced the receiver in this process; the next child
    // opens the same file in generation 1, without normalizing the fixture.
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().generation, 2);
    assert_eq!(intents[0].generation, 2);
    (pair, binding)
}
fn assert_replayed_rewrite_captured(pair: &mut Pair, binding: BindingId) {
    let observed = pair.receiver.driver().incoming_observation(binding).unwrap();
    assert_eq!(observed.generation, 1);
    let intents = pair.receiver.driver().apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].generation, observed.generation);
    let seq = intents[0].result_seq.unwrap();
    assert_eq!(seq, observed.seq);
    let capture = pair.receiver.driver().capture_proof(seq, observed.generation).unwrap();
    assert_eq!(capture.observed_seq, seq);
    assert_eq!(capture.generation, observed.generation);
    assert!(capture.attempted && capture.persisted);
    assert!(!pair.receiver.driver().scan_state().unwrap().tracking_stopped);
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
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().generation, 1);
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap()[0].generation, 2);
    pair.receiver.hold(Point::ResolutionMoveBeforeRewrite).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ResolutionMoveBeforeRewrite));
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files[&hash_name()], LOCAL);
    assert!(!files.contains_key("photo.jpg"));
    assert_eq!(notes(&mut pair.receiver), before);
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap().len(), 1);
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.release(Point::ResolutionMoveBeforeRewrite).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_replayed_rewrite_captured(&mut pair, binding);
    pair.receiver.release(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_finished(&mut pair, binding);
    pair.close();
}

fn edit_every_key_before_step8(pair: &mut Pair) {
    pair.receiver.kill().unwrap();
    pair.reopen_before_identity();
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().generation, 1);
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap()[0].generation, 2);
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
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_replayed_rewrite_captured(&mut pair, binding);
    pair.receiver.release(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
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
    assert_replayed_rewrite_captured(&mut pair, binding);
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
fn q11_kill_after_rewrite_with_download_held_keeps_intervening_edit_without_rewrite() {
    let (mut pair, binding) = conflict(Point::ResolutionRewriteCommitted, false);
    let rewritten = notes(&mut pair.receiver);
    assert!(rewritten.len() >= 2);
    for note in &rewritten {
        assert_eq!(
            note.flds,
            format!("<img src=\"{}\">\x1flocal back", hash_name()).as_bytes()
        );
    }
    // This is the real download callback, paused after the note COMMIT and
    // before installing its staged bytes under the incoming name.
    let intents = pair.receiver.driver().apply_intents(binding).unwrap();
    let staged = intents[0].media.as_ref().unwrap().staged.as_ref().unwrap();
    assert_eq!(std::fs::read(staged).unwrap(), REMOTE);
    let files = pair.receiver.driver().media_files().unwrap();
    assert_eq!(files[&hash_name()], LOCAL);
    assert!(!files.contains_key("photo.jpg"));
    let killed = pair.receiver.kill().unwrap();
    assert_eq!(killed.signal, Some(libc::SIGKILL));
    assert!(!killed.clean_close);

    pair.reopen_before_identity();
    assert_eq!(notes(&mut pair.receiver), rewritten);
    let edited_id = rewritten[0].id;
    let front = format!("synthetic edit after crash <img src=\"{}\">", hash_name());
    pair.receiver
        .driver()
        .edit_note_fields(edited_id, &[&front, "synthetic changed back"])
        .unwrap();
    let edited = notes(&mut pair.receiver);
    assert_eq!(
        edited.iter().find(|note| note.id == edited_id).unwrap().flds,
        format!("{front}\x1fsynthetic changed back").as_bytes()
    );
    let before = pair.receiver.driver().incoming_observation(binding).unwrap();
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert!(!pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_eq!(notes(&mut pair.receiver), edited);
    assert_eq!(
        pair.receiver.driver().incoming_observation(binding).unwrap().seq,
        before.seq
    );
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    let facts = pair.receiver.facts().unwrap();
    let moved = facts
        .entities
        .values()
        .find(|entity| matches!(&entity.payload, Payload::MediaName(media) if media.name == hash_name().as_bytes()))
        .unwrap();
    assert!(moved.tombstone_at.is_none());
    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_replay_convergence(&mut pair, binding);
    assert!(!pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_eq!(notes(&mut pair.receiver), edited);
    pair.close();
}

#[test]
fn q11_deleted_note_key_is_skipped_while_replay_reruns_surviving_notes() {
    let (mut pair, binding) = conflict(Point::ResolutionMoveBeforeRewrite, true);
    let planned = pair.receiver.driver().apply_intents(binding).unwrap();
    let killed = pair.receiver.kill().unwrap();
    assert_eq!(killed.signal, Some(libc::SIGKILL));
    assert!(!killed.clean_close);
    pair.reopen_before_identity();
    assert_eq!(planned[0].generation, 2);
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().generation, 1);
    let rows = notes(&mut pair.receiver);
    assert!(rows.len() >= 2);
    let deleted_id = rows[0].id;
    assert!(
        planned[0]
            .keys
            .iter()
            .any(|key| key.kind == codec::Kind::Note && key.key == deleted_id.to_be_bytes())
    );
    pair.receiver.driver().delete_note(deleted_id).unwrap();
    let surviving = notes(&mut pair.receiver);
    assert_eq!(surviving.len(), rows.len() - 1);
    assert!(surviving.iter().all(|note| note.id != deleted_id));
    for note in &surviving {
        let front = format!("row {} <img src=\"photo.jpg\"><img src=\"{}\">", note.id, hash_name());
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &[&front, "tail"])
            .unwrap();
    }
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap(), planned);
    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    assert_replayed_rewrite_captured(&mut pair, binding);
    let rewritten = notes(&mut pair.receiver);
    assert_eq!(
        rewritten.iter().map(|note| note.id).collect::<Vec<_>>(),
        surviving.iter().map(|note| note.id).collect::<Vec<_>>()
    );
    for note in &rewritten {
        assert_eq!(
            note.flds,
            format!(
                "row {} <img src=\"{}\"><img src=\"{}\">\x1ftail",
                note.id,
                hash_name(),
                hash_name()
            )
            .as_bytes()
        );
    }
    let pending = pair.receiver.driver().apply_intents(binding).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].id, planned[0].id);
    let media = pending[0].media.as_ref().unwrap();
    assert_eq!(media.incoming, planned[0].media.as_ref().unwrap().incoming);
    assert_eq!(std::fs::read(media.staged.as_ref().unwrap()).unwrap(), REMOTE);
    pair.receiver.release(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_finished(&mut pair, binding);
    assert_replay_convergence(&mut pair, binding);
    assert_eq!(notes(&mut pair.receiver), rewritten);
    let facts = pair.receiver.facts().unwrap();
    let deleted = facts
        .entities
        .values()
        .find(|entity| matches!(&entity.payload, Payload::Note(note) if note.id == deleted_id))
        .unwrap();
    assert!(deleted.tombstone_at.is_some());
    for rig in [&mut pair.receiver, &mut pair.sender] {
        assert!(!rig.driver().collection_rows().unwrap().iter().any(|row| match row {
            Payload::Note(note) => note.id == deleted_id,
            Payload::Card(card) => card.nid == deleted_id,
            _ => false,
        }));
    }
    pair.close();
}

#[test]
fn paused_media_success_survives_repeated_holds_without_resubmission() {
    let (mut pair, binding) = conflict(Point::ResolutionMoveBeforeRewrite, false);
    let requests = pair.service.request_log().unwrap();
    let before = notes(&mut pair.receiver);
    // A still-held caller has no terminal result to consume. Repeated drives
    // must preserve the original callback and its download source.
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ResolutionMoveBeforeRewrite));
    assert_eq!(notes(&mut pair.receiver), before);
    assert!(pair.service.request_log().unwrap() == requests);

    pair.receiver.hold(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.release(Point::ResolutionMoveBeforeRewrite).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::ResolutionRewriteCommitted));
    let rewritten = notes(&mut pair.receiver);
    assert!(pair.service.request_log().unwrap() == requests);

    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.release(Point::ResolutionRewriteCommitted).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    assert!(pair.service.request_log().unwrap() == requests);

    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(notes(&mut pair.receiver), rewritten);
    assert_finished(&mut pair, binding);
    let settled = pair.receiver.driver().incoming_observation(binding).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().seq, settled.seq);
    assert_eq!(notes(&mut pair.receiver), rewritten);
    pair.close();
}

#[test]
fn stopped_tracking_returns_the_original_paused_media_error_once_without_replay() {
    let (mut pair, binding) = conflict(Point::ResolutionMoveBeforeRewrite, false);
    let before = pair.receiver.driver().incoming_observation(binding).unwrap();
    let requests = pair.service.request_log().unwrap();
    // Fail the real synchronous observer after the first tracking write. The
    // collection COMMIT survives; invalidated recovery authority then gives the
    // original MediaReply caller StaleIncarnation, before its boundary hook.
    pair.receiver.driver().arm_capture_failure_for_testing(1).unwrap();
    pair.receiver.release(Point::ResolutionMoveBeforeRewrite).unwrap();
    let error = pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap_err();
    assert_eq!(error.to_string(), "cloud device operation failed: RuntimeStaleIncarnation");
    let after = pair.receiver.driver().incoming_observation(binding).unwrap();
    assert!(after.seq > before.seq);
    assert_eq!(after.generation, before.generation);
    let capture = pair.receiver.driver().capture_proof(after.seq, after.generation).unwrap();
    assert!(capture.attempted && !capture.persisted);
    assert!(pair.receiver.driver().scan_state().unwrap().tracking_stopped);
    let rewritten = notes(&mut pair.receiver);
    assert!(rewritten.iter().all(|note| {
        let fields = std::str::from_utf8(&note.flds).unwrap();
        fields.contains(&hash_name()) && !fields.contains("src=\"photo.jpg\"")
    }));
    let unsettled = pair.receiver.driver().apply_intents(binding).unwrap();
    assert_eq!(unsettled.len(), 1);
    assert_eq!(unsettled[0].result_seq, None);
    // The terminal error was consumed once. Another stopped drive issues no
    // SDK operation and neither retries the callback nor rewrites/settles it.
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().incoming_observation(binding).unwrap().seq, after.seq);
    assert_eq!(notes(&mut pair.receiver), rewritten);
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap(), unsettled);
    assert!(pair.service.request_log().unwrap() == requests);
    pair.receiver.kill().unwrap();
    pair.sender.close_clean().unwrap();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
