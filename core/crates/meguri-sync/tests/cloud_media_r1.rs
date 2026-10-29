#[path = "support/publication_incoming.rs"]
mod support;
use meguri_sync::cloud::{
    ConflictReason, InboxReason,
    entity_codec::{self as codec, Payload},
};
use meguri_testkit::cloud::coordinator::Point;
use support::*;

fn remote_change(pair: &mut Pair, point: Point) {
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(point).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(point));
}

#[test]
fn paused_adapter_rejects_issued_download() {
    let mut pair = Pair::adopted();
    remote_change(&mut pair, Point::MediaReplyBeforeApply);
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    let facts = pair.receiver.facts().unwrap().entities;
    let inbox = pair.receiver.driver().inbox_entries(binding).unwrap();
    let files = pair.receiver.driver().media_files().unwrap();
    let staging = pair.receiver_dir.join("meguri-cloud-staging");
    let paths = || {
        let mut paths: Vec<_> = match std::fs::read_dir(&staging) {
            Ok(entries) => entries.map(|p| p.unwrap().path()).collect(),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
            Err(e) => panic!("{e}"),
        };
        paths.sort();
        paths
    };
    let staged = paths();
    pair.receiver.driver().pause_media_adapter(true).unwrap();
    pair.receiver.release(Point::MediaReplyBeforeApply).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert_eq!(pair.receiver.facts().unwrap().entities, facts);
    assert_eq!(pair.receiver.driver().inbox_entries(binding).unwrap(), inbox);
    assert_eq!(pair.receiver.driver().media_files().unwrap(), files);
    assert_eq!(paths(), staged);
    pair.close();
}

#[test]
fn file_added_before_install_keeps_ready_record() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    remote_change(&mut pair, Point::MediaTrashBeforeInstall);
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    let user = b"synthetic write while trash install is held";
    pair.receiver.driver().put_media("photo.jpg", user).unwrap();
    pair.receiver.release(Point::MediaTrashBeforeInstall).unwrap();
    pair.receiver.driver().complete_pending_media_reply().unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], user);
    assert!(
        pair.receiver
            .driver()
            .inbox_entries(binding)
            .unwrap()
            .iter()
            .any(|e| e.reason == InboxReason::Ready)
    );
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    pair.close();
}

#[test]
fn feed_revision_change_blocks_download_commit() {
    let mut pair = Pair::adopted();
    remote_change(&mut pair, Point::MediaReplyBeforeTracking);
    pair.receiver.driver().put_media("later.bin", LOCAL).unwrap();
    let facts = pair.receiver.facts().unwrap().entities;
    let later = facts
        .values()
        .find(|e| matches!(&e.payload, Payload::MediaName(p) if p.name == b"later.bin"))
        .unwrap()
        .clone();
    pair.receiver.release(Point::MediaReplyBeforeTracking).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert_eq!(pair.receiver.facts().unwrap().entities[&later.name], later);
    assert_eq!(pair.receiver.driver().media_files().unwrap()["later.bin"], LOCAL);
    pair.close();
}

#[test]
fn recovery_checks_moved_file_identity_before_note_update() {
    let mut pair = Pair::adopted();
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &["<img src=\"photo.jpg\">", "local"])
            .unwrap();
    }
    pair.receiver.driver().put_media(&hash_name(), LOCAL).unwrap();
    pair.receiver.driver().put_media("photo.jpg", LOCAL).unwrap();
    remote_change(&mut pair, Point::ResolutionBeforeMove);
    let saved_notes = notes(&mut pair.receiver);
    let receiver_binding = pair.receiver.facts().unwrap().current.unwrap().id;
    pair.receiver.kill().unwrap();
    let camera_bytes = b"synthetic reopened camera image";
    std::fs::write(pair.receiver_dir.join("collection.media/photo.jpg"), camera_bytes).unwrap();
    pair.reopen_before_identity();
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.verify_and_drive();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert_eq!(notes(&mut pair.receiver), saved_notes);
    let retained = pair.receiver.driver().media_files().unwrap();
    assert_eq!(retained["photo.jpg"], camera_bytes);
    assert_eq!(retained[&hash_name()], LOCAL);
    assert!(
        pair.receiver
            .driver()
            .apply_intents(receiver_binding)
            .unwrap()
            .is_empty()
    );
    assert!(
        pair.receiver
            .driver()
            .inbox_entries(receiver_binding)
            .unwrap()
            .iter()
            .any(|e| e.reason == InboxReason::Ready)
    );
    pair.close();
}

#[test]
fn note_capture_change_blocks_apply_commit() {
    let mut pair = Pair::adopted();
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &["<img src=\"photo.jpg\">", "local"])
            .unwrap();
    }
    pair.receiver.driver().put_media("photo.jpg", LOCAL).unwrap();
    remote_change(&mut pair, Point::MediaSettlementBeforeCommit);
    for note in notes(&mut pair.receiver) {
        pair.receiver
            .driver()
            .edit_note_fields(note.id, &["synthetic newer observer text", "new back"])
            .unwrap();
    }
    let rows = notes(&mut pair.receiver);
    let entities = pair.receiver.facts().unwrap().entities;
    pair.receiver.release(Point::MediaSettlementBeforeCommit).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert_eq!(notes(&mut pair.receiver), rows);
    for (name, e) in entities.iter().filter(|(_, e)| e.kind == codec::Kind::Note) {
        assert_eq!(&pair.receiver.facts().unwrap().entities[name], e);
    }
    pair.close();
}

#[test]
fn delete_ack_keeps_server_payload_across_open() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    pair.receiver.driver().trash_media("photo.jpg").unwrap();
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let server = pair
        .sender
        .facts()
        .unwrap()
        .entities
        .values()
        .find(|e| matches!(&e.payload, Payload::MediaName(p) if p.name == b"photo.jpg"))
        .unwrap()
        .payload
        .clone();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let copies = pair.receiver.driver().media_conflicts().unwrap();
    let copy = copies
        .iter()
        .find(|c| c.reason == ConflictReason::EditVsDelete)
        .unwrap();
    assert_eq!(std::fs::read(copy.staged_path.as_ref().unwrap()).unwrap(), REMOTE);
    let facts = pair.receiver.facts().unwrap();
    let deleted = facts
        .entities
        .values()
        .find(|e| e.kind == codec::Kind::MediaName)
        .unwrap();
    let codec::DecodeOutcome::Supported(envelope) = codec::Entity::decode(&deleted.acked_envelope.0).unwrap() else {
        panic!("entity decode failed")
    };
    assert!(
        envelope.fields.tombstone,
        "tombstone={} mark={} local_rev={} acked_rev={} flight_count={}",
        envelope.fields.tombstone,
        deleted.mark.is_some(),
        deleted.local_rev,
        deleted.acked_rev,
        deleted.in_flight.len(),
    );
    assert_eq!(envelope.payload.unwrap(), server.encode().unwrap());
    assert_eq!(deleted.local_rev, deleted.acked_rev);
    assert!(deleted.mark.is_none());
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(!pair.sender.driver().media_files().unwrap().contains_key("photo.jpg"));
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key("photo.jpg"));
    assert_eq!(std::fs::read(copy.staged_path.as_ref().unwrap()).unwrap(), REMOTE);
    pair.close();
}

#[test]
fn content_conflict_retains_remote_envelope_and_file() {
    use sha1::{Digest, Sha1};
    let payload = |bytes: &[u8]| {
        Payload::MediaName(codec::MediaName {
            name: b"photo.jpg".to_vec(),
            sha256: codec::sha256(bytes),
            sha1: Sha1::digest(bytes).into(),
            size: bytes.len() as u64,
        })
    };
    let (local, remote) = if payload(LOCAL).payload_digest().unwrap() > payload(REMOTE).payload_digest().unwrap() {
        (LOCAL, REMOTE)
    } else {
        (REMOTE, LOCAL)
    };
    let mut pair = Pair::published();
    pair.sender
        .driver()
        .put_media("photo.jpg", b"synthetic common ancestor image")
        .unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    pair.receiver.driver().put_media("photo.jpg", local).unwrap();
    pair.sender.driver().put_media("photo.jpg", remote).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], local);
    let copies = pair.receiver.driver().media_conflicts().unwrap();
    let copy = copies
        .iter()
        .find(|c| c.reason == ConflictReason::MediaContent)
        .unwrap();
    let codec::DecodeOutcome::Supported(envelope) = codec::Entity::decode(&copy.losing_envelope.0).unwrap() else {
        panic!("entity decode failed")
    };
    assert_eq!(envelope.payload.unwrap(), payload(remote).encode().unwrap());
    assert_eq!(std::fs::read(copy.staged_path.as_ref().unwrap()).unwrap(), remote);
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(binding).unwrap().is_empty());
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    assert_eq!(std::fs::read(copy.staged_path.as_ref().unwrap()).unwrap(), remote);
    pair.close();
}

#[test]
fn failed_file_read_keeps_issued_reply_until_completion() {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    remote_change(&mut pair, Point::MediaTrashBeforeInstall);
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    pair.receiver.fail_held_io(Point::MediaTrashBeforeInstall).unwrap();
    assert!(pair.receiver.driver().complete_pending_media_reply().is_err());
    assert_eq!(pair.receiver.driver().apply_intents(binding).unwrap().len(), 1);
    pair.receiver.kill().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

fn trash_race_preserves_edit(in_place: bool, tombstone: bool) {
    let mut pair = Pair::published();
    pair.sender.driver().put_media("photo.jpg", LOCAL).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let binding = pair.receiver.facts().unwrap().current.unwrap().id;
    let user = b"synthetic camera revision written at removal";
    pair.receiver
        .driver()
        .write_during_media_trash("photo.jpg", user, in_place)
        .unwrap();
    if tombstone {
        pair.sender.driver().trash_media("photo.jpg").unwrap();
    } else {
        pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    }
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    // Observe the first apply result.
    pair.receiver.hold(Point::IntentSettlementComplete).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::IntentSettlementComplete));
    assert_eq!(
        std::fs::read(pair.receiver_dir.join("collection.media/photo.jpg")).unwrap(),
        user
    );
    assert!(pair.receiver.driver().apply_intents(binding).unwrap().is_empty());
    assert!(
        pair.receiver
            .driver()
            .inbox_entries(binding)
            .unwrap()
            .iter()
            .any(|e| e.reason == InboxReason::Ready)
    );
    pair.receiver.release(Point::IntentSettlementComplete).unwrap();
    pair.close();
}

#[test]
fn trash_check_refuses_new_directory_entry() {
    trash_race_preserves_edit(false, false);
}

#[test]
fn trash_check_refuses_modified_inode() {
    trash_race_preserves_edit(true, false);
}

#[test]
fn delete_check_refuses_new_directory_entry() {
    trash_race_preserves_edit(false, true);
}

#[test]
fn delete_check_refuses_modified_inode() {
    trash_race_preserves_edit(true, true);
}
