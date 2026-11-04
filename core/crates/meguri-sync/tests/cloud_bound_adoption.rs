//! Ordinary bound adoption: extract this target and testkit seams before the runtime fix.
#[path = "support/publication_incoming.rs"]
mod support;

use meguri_sync::cloud::adoption::{AdoptStep, RecoveryKind};
use meguri_testkit::cloud::coordinator::Point;
use support::*;

#[test]
fn bound_sender_adopts_second_ordinary_publication_after_ready_and_reopens() {
    let mut pair = Pair::published();
    let old = pair.sender.facts().unwrap();
    let old_binding = old.current.as_ref().unwrap();
    assert_eq!(pair.sender.authority_status().unwrap(), (true, true));

    pair.receiver.driver().put_media("photo.jpg", REMOTE).unwrap();
    publish(&mut pair.receiver, &mut pair.service);
    let expected_rows = pair.receiver.driver().collection_rows().unwrap();
    let expected_media = pair.receiver.driver().media_files().unwrap();
    assert_eq!(expected_media["photo.jpg"], REMOTE);
    assert_ne!(pair.sender.driver().collection_rows().unwrap(), expected_rows);
    let active_zone = pair.receiver.collection_zone().unwrap();
    assert_ne!(pair.sender.collection_zone().unwrap(), active_zone);

    let preview = pair.sender.preview(&mut pair.service, STEPS).unwrap();
    pair.sender.hold(Point::AdoptionBeforeActivate).unwrap();
    let adoption = pair.sender.adopt_cloud(&preview).unwrap();
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.sender.is_held(Point::AdoptionBeforeActivate));
    let ready = pair.sender.facts().unwrap();
    assert_eq!(ready.recovery_ops.len(), 1);
    let RecoveryKind::Adopt(op) = &ready.recovery_ops[0].kind else {
        panic!("expected the real adoption row");
    };
    assert_eq!(op.id, adoption);
    assert_eq!(op.step, AdoptStep::Ready);
    assert_eq!(op.binding_before.as_ref().unwrap().id, old_binding.id);
    assert!(ready.current.as_ref() == Some(old_binding));
    assert_eq!(ready.decision, old.decision);
    assert_eq!(ready.native_file, old.native_file);
    assert_eq!(pair.sender.authority_status().unwrap(), (false, true));
    // Fixed task gate plus the actual durable Ready row and runtime booleans:
    // no identifiers, account values, row contents or media bytes in this witness.
    eprintln!("bound adoption boundary: task=Activate step=Ready stored_usable=false identity_verified=true");

    pair.sender.release(Point::AdoptionBeforeActivate).unwrap();
    // Against the unchanged precondition, this must fail semantically in
    // AdoptionLocal: ordinary replay asks the disabled old binding for authority.
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.sender.facts().unwrap();
    assert!(adopted.recovery_ops.is_empty());
    assert_ne!(adopted.current.as_ref().unwrap().id, old_binding.id);
    assert_eq!(pair.sender.collection_zone().unwrap(), active_zone);
    assert_eq!(pair.sender.authority_status().unwrap(), (true, true));
    assert_eq!(pair.sender.driver().collection_rows().unwrap(), expected_rows);
    assert_eq!(pair.sender.driver().media_files().unwrap(), expected_media);

    pair.sender.close_clean().unwrap();
    pair.sender = online(&pair.root.path().join("sender"), false);
    pair.sender.drive_ready(&mut pair.service, STEPS).unwrap();
    let reopened = pair.sender.facts().unwrap();
    assert!(reopened.recovery_ops.is_empty());
    assert!(reopened.current == adopted.current);
    assert_eq!(reopened.decision, adopted.decision);
    assert_eq!(pair.sender.collection_zone().unwrap(), active_zone);
    assert_eq!(pair.sender.authority_status().unwrap(), (true, true));
    assert_eq!(pair.sender.driver().collection_rows().unwrap(), expected_rows);
    assert_eq!(pair.sender.driver().media_files().unwrap(), expected_media);
    pair.close();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
