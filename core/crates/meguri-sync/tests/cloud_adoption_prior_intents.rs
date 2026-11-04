//! Tests first: V17.6, V2.4.8, V14.2.6/9; Q11/P24 row 495 (adoption).
//! Requested clean main base: a6cadda2; filtered source baseline: bde90b5.
//! Fixer session: s-20261103T133750Z-f276f58e. Source-only, no acceptance claim.
#[path = "support/publication_incoming.rs"]
mod support;

use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

use meguri_engine::{device_state::DeviceState, replace::ReplaceReason};
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    BindingState, IsolationReason,
    adoption::{AdoptStep, RecoveryKind, RecoveryStep},
    entity_codec::{self as codec, Payload},
    workflow::TraceEvent,
};
use meguri_testkit::cloud::{Config, Field, Service, coordinator::Point, devices::IdentityReads};
use support::*;

const FRESH: &[u8] = b"synthetic distinct active epoch image";

// Inspect only the generated fixture's actual trash/backups. Include nested
// trash entries so checking preservation does not depend on generated names.
fn files_under(directory: &Path) -> BTreeMap<PathBuf, Vec<u8>> {
    let mut files = BTreeMap::new();
    if directory.exists() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                files.extend(files_under(&path));
            } else {
                files.insert(path.clone(), std::fs::read(path).unwrap());
            }
        }
    }
    files
}

fn assert_local_copy(pair: &mut Pair, resolution: bool) {
    if resolution {
        assert_eq!(pair.receiver.driver().media_files().unwrap()[&hash_name()], LOCAL);
    } else {
        assert!(
            files_under(&pair.receiver_dir.join("media.trash"))
                .values()
                .any(|bytes| bytes == LOCAL)
        );
    }
}

fn assert_settled_notes(pair: &mut Pair, resolution: bool) {
    for note in notes(&mut pair.receiver) {
        if resolution {
            assert_eq!(
                note.flds,
                format!("<img src=\"{}\">\x1flocal retained back", hash_name()).as_bytes()
            );
        } else {
            assert_eq!(note.flds, b"local pre-adoption note\x1flocal retained back");
        }
    }
}

fn prior_intent_adoption(resolution: bool, kill_ready: bool) {
    let root = tempfile::tempdir().unwrap();
    let sender_dir = generated(root.path(), "sender");
    let receiver_dir = generated(root.path(), "receiver");
    // Local pagination only: every page/cursor is produced by Service.
    let mut service = Service::open(
        root.path(),
        Config {
            page_size: 32,
            ..Config::default()
        },
        SeededRandom::new(811),
    )
    .unwrap();
    let mut sender = online(&sender_dir, true);
    publish(&mut sender, &mut service);
    if !resolution {
        sender.driver().put_media("photo.jpg", LOCAL).unwrap();
        sender.sync_now_and_drain(&mut service, STEPS).unwrap();
    }
    let mut receiver = online(&receiver_dir, true);
    let preview = receiver.preview(&mut service, STEPS).unwrap();
    receiver.adopt_cloud(&preview).unwrap();
    receiver.drive_ready(&mut service, STEPS).unwrap();
    let mut pair = Pair {
        service,
        sender,
        receiver,
        receiver_dir,
        root,
    };
    let original = pair.receiver.facts().unwrap();
    let old = original.current.as_ref().unwrap().clone();
    assert_eq!(old.facts.state, BindingState::Active);
    let old_epoch = old.facts.epoch;
    let note_ids: Vec<_> = notes(&mut pair.receiver).iter().map(|note| note.id).collect();
    assert_eq!(note_ids.len(), 2);
    for id in &note_ids {
        pair.receiver
            .driver()
            .edit_note_fields(
                *id,
                &[
                    if resolution {
                        "<img src=\"photo.jpg\">"
                    } else {
                        "local pre-adoption note"
                    },
                    "local retained back",
                ],
            )
            .unwrap();
    }
    if resolution {
        assert!(!pair.receiver.driver().media_files().unwrap().contains_key("photo.jpg"));
        pair.receiver.driver().put_media("photo.jpg", LOCAL).unwrap();
    } else {
        assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], LOCAL);
    }
    pair.sender.driver().put_media("photo.jpg", REMOTE).unwrap();
    pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    let incoming_media_rows = pair.sender.driver().media_rows().unwrap();
    let incoming_photo = incoming_media_rows
        .iter()
        .find(|row| matches!(row, Payload::MediaName(media) if media.name == b"photo.jpg"))
        .unwrap();
    let point = if resolution {
        Point::ResolutionMoveBeforeRewrite
    } else {
        Point::MediaTrashBeforeInstall
    };
    pair.receiver.hold(point).unwrap();
    pair.receiver.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(point));
    let held = pair.receiver.driver().apply_intents(old.id).unwrap();
    assert_eq!(held.len(), 1);
    let intent = &held[0];
    assert_eq!(intent.binding, old.id);
    let media = intent
        .media
        .as_ref()
        .expect("normal incoming work must create a media intent");
    assert_eq!(media.old_name, "photo.jpg");
    assert_eq!(media.moved.is_some(), resolution);
    let staged = media.staged.as_ref().unwrap().clone();
    assert_eq!(std::fs::read(&staged).unwrap(), REMOTE);
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key("photo.jpg"));
    if resolution {
        // The SAME intent owns the move and every note-rewrite key.
        for id in &note_ids {
            assert!(intent.keys.iter().any(|key| matches!(&key.after_payload,
                Some(Payload::Note(note)) if note.id == *id)));
        }
        assert_eq!(pair.receiver.driver().media_files().unwrap()[&hash_name()], LOCAL);
        for note in notes(&mut pair.receiver) {
            assert_eq!(note.flds, b"<img src=\"photo.jpg\">\x1flocal retained back");
        }
    }
    // This drains the original callback and checks RuntimeTracking(Io), not
    // an arbitrary SDK failure. It fails if the original work succeeds.
    pair.receiver.fail_held_io(point).unwrap();
    assert!(!pair.receiver.is_held(point));
    let failed = pair.receiver.driver().apply_intents(old.id).unwrap();
    assert_eq!(failed.len(), 1, "the failed install must retain durable work");
    assert_eq!(failed[0].id, intent.id);
    assert_eq!(failed[0].binding, old.id);
    assert_eq!(failed[0].media.as_ref().unwrap().staged.as_ref(), Some(&staged));
    assert_eq!(std::fs::read(&staged).unwrap(), REMOTE);
    assert!(!pair.receiver.driver().media_files().unwrap().contains_key("photo.jpg"));
    assert_local_copy(&mut pair, resolution);
    for note in notes(&mut pair.receiver) {
        assert_eq!(
            note.flds,
            if resolution {
                b"<img src=\"photo.jpg\">\x1flocal retained back".as_slice()
            } else {
                b"local pre-adoption note\x1flocal retained back".as_slice()
            }
        );
    }

    // Publish a DISTINCT epoch via the normal confirmation path. Reopening the
    // publisher uses the same platform provider, with a fresh random stream.
    pair.sender.close_clean().unwrap();
    pair.sender = open_with_seed(&sender_dir, IdentityReads::Available(scope().account), true, 1818);
    for note in notes(&mut pair.sender) {
        pair.sender
            .driver()
            .edit_note_fields(
                note.id,
                &[
                    "fresh cloud source without a local media reference",
                    "distinct cloud back",
                ],
            )
            .unwrap();
    }
    pair.sender.driver().put_media("photo.jpg", FRESH).unwrap();
    publish(&mut pair.sender, &mut pair.service);
    let source = pair.sender.facts().unwrap().current.unwrap();
    assert_ne!(source.facts.epoch, old_epoch);
    let cloud_rows = pair.sender.driver().collection_rows().unwrap();
    let cloud_media_rows = pair.sender.driver().media_rows().unwrap();
    assert_ne!(incoming_media_rows, cloud_media_rows);
    let local_rows = pair.receiver.driver().collection_rows().unwrap();
    assert_ne!(cloud_rows, local_rows);
    assert_eq!(pair.receiver.driver().apply_intents(old.id).unwrap().len(), 1);
    let backups = pair.receiver_dir.with_file_name("receiver-backups");
    let backups_before = files_under(&backups);

    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    let before_confirmation = pair.receiver.facts().unwrap();
    let prior_state = before_confirmation
        .bindings
        .iter()
        .find(|binding| binding.id == old.id)
        .unwrap()
        .facts
        .state;
    pair.receiver.hold(Point::AdoptionBeforeActivate).unwrap();
    let op_id = pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionBeforeActivate));
    let ready = pair.receiver.facts().unwrap();
    assert_eq!(ready.recovery_ops.len(), 1);
    let row = &ready.recovery_ops[0];
    assert_eq!(row.id, op_id);
    assert_eq!(row.step, RecoveryStep::Adopt(AdoptStep::Ready));
    let RecoveryKind::Adopt(adopt) = &row.kind else {
        panic!("must exercise CloudAdopt")
    };
    let prior = adopt.binding_before.as_ref().unwrap();
    assert_eq!(prior.id, old.id);
    assert_eq!(prior.state, prior_state);
    assert_eq!(adopt.epoch, source.facts.epoch);
    let candidate = adopt.binding_after;
    assert_ne!(candidate, old.id);
    assert_eq!(
        ready
            .bindings
            .iter()
            .find(|binding| binding.id == old.id)
            .unwrap()
            .facts
            .state,
        match prior_state {
            BindingState::Active => BindingState::Isolated(IsolationReason::ReplacedLocally),
            isolated => isolated,
        }
    );
    assert_eq!(ready.native_file, original.native_file);
    assert!(ready.decision == original.decision);
    assert!(
        pair.receiver.driver().apply_intents(old.id).unwrap().is_empty(),
        "prior-owner work must settle before Ready/activation, not be deleted at done"
    );
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], REMOTE);
    assert!(pair.receiver.driver().media_rows().unwrap().contains(incoming_photo));
    assert_local_copy(&mut pair, resolution);
    assert_settled_notes(&mut pair, resolution);
    let settled_rows = pair.receiver.driver().collection_rows().unwrap();
    let settled_files = pair.receiver.driver().media_files().unwrap();
    assert_ne!(settled_rows, cloud_rows);
    assert_eq!(
        files_under(&backups),
        backups_before,
        "Activate has not made the safety package yet"
    );
    assert!(
        DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
            .replacement_evidence_for(original.decision.publication.identity)
            .unwrap()
            .is_none()
    );

    if kill_ready {
        let killed = pair.receiver.kill().unwrap();
        assert_eq!(killed.signal, Some(libc::SIGKILL));
        assert!(!killed.clean_close);
        // Observe rollback before a new identity/root check can isolate the
        // restored binding for the newer cloud epoch.
        pair.reopen_before_identity();
        let recovered = pair.receiver.facts().unwrap();
        assert!(recovered.recovery_ops.is_empty());
        assert!(!recovered.bindings.iter().any(|binding| binding.id == candidate));
        assert_eq!(
            recovered
                .bindings
                .iter()
                .find(|binding| binding.id == old.id)
                .unwrap()
                .facts
                .state,
            prior_state
        );
        assert!(recovered.decision == original.decision);
        assert_eq!(recovered.native_file, original.native_file);
        assert!(pair.receiver.driver().apply_intents(old.id).unwrap().is_empty());
        assert_eq!(pair.receiver.driver().collection_rows().unwrap(), settled_rows);
        assert_eq!(pair.receiver.driver().media_files().unwrap(), settled_files);
        assert_local_copy(&mut pair, resolution);
        assert_settled_notes(&mut pair, resolution);
        assert_eq!(files_under(&backups), backups_before);
        assert!(
            DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
                .replacement_evidence_for(original.decision.publication.identity)
                .unwrap()
                .is_none(),
            "a killed Ready stage is not a completed replacement or a successful backup"
        );
        pair.receiver.verify_identity(scope().account).unwrap();
        let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
        let fresh_op = pair.receiver.adopt_cloud(&preview).unwrap();
        assert_ne!(fresh_op, op_id);
    } else {
        pair.receiver.release(Point::AdoptionBeforeActivate).unwrap();
    }

    // Replacement and its safety package must precede the REAL new scan.
    pair.receiver.hold(Point::BeforeScan).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::BeforeScan));
    let replaced = pair.receiver.facts().unwrap();
    assert!(replaced.recovery_ops.is_empty());
    assert_eq!(replaced.current.as_ref().unwrap().facts.epoch, source.facts.epoch);
    assert_ne!(replaced.current.as_ref().unwrap().id, old.id);
    assert_ne!(replaced.native_file, original.native_file);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), cloud_rows);
    assert!(pair.receiver.driver().apply_intents(old.id).unwrap().is_empty());
    assert!(pair.receiver.driver().inbox_entries(old.id).unwrap().is_empty());
    for name in original.entities.keys() {
        assert!(pair.receiver.driver().binding_entity(old.id, name).unwrap().is_none());
    }
    let evidence = DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"))
        .replacement_evidence_for(original.decision.publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, original.decision.publication);
    assert_eq!(evidence.candidate, replaced.decision.publication);
    let backup = evidence
        .safety_backup
        .expect("completed CloudAdopt must have an actual safety package");
    assert!(pair.receiver.driver().backup_path(&backup.name).unwrap().is_file());
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        settled_rows,
        "the pre-replacement safety copy must include the settled note rewrite"
    );
    let trace_start = pair.receiver.trace().len();
    pair.receiver.release(Point::BeforeScan).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.trace()[trace_start..].iter().any(
        |event| matches!(event, TraceEvent::ScanCommitted { native_file, .. } if *native_file == replaced.native_file)
    ));
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], FRESH);
    let installed_media_rows = pair.receiver.driver().media_rows().unwrap();
    for row in &cloud_media_rows {
        assert!(installed_media_rows.contains(row));
    }
    if resolution {
        assert_eq!(pair.receiver.driver().media_files().unwrap()[&hash_name()], LOCAL);
    } else {
        assert!(
            files_under(&pair.receiver_dir.join("media.trash"))
                .values()
                .any(|bytes| bytes == LOCAL)
        );
    }
    assert!(
        files_under(&pair.receiver_dir.join("media.trash"))
            .values()
            .any(|bytes| bytes == REMOTE)
    );
    // Bindings and ACKs are runtime products: compare real source rows to real
    // Service versions, rather than treating a cleared dirty bit as evidence.
    let final_facts = pair.receiver.facts().unwrap();
    let new_binding = final_facts.current.as_ref().unwrap();
    let key = new_binding.facts.record_key.0;
    let database = pair.service.inspect(&scope()).unwrap().unwrap();
    let zone = &database.zones[&pair.receiver.collection_zone().unwrap()];
    for payload in &cloud_rows {
        let name = codec::entity_name(
            &key,
            payload.kind(),
            &meguri_sync::cloud::row_payload::payload_key(payload).unwrap(),
        )
        .unwrap();
        let entity = &final_facts.entities[&name];
        assert_eq!(&entity.payload, payload);
        assert!(entity.acked_rev > 0);
        let record = &zone.records[&name];
        let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
            panic!("real Service envelope required")
        };
        let received = codec::receive(&key, &name, envelope, codec::AssetInput::Absent).unwrap();
        assert_eq!(received.payload.as_ref(), Some(payload));
        assert_eq!(entity.acked_envelope.0, *envelope);
        assert_eq!(entity.acked_system_fields, record.system_fields.bytes());
    }
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let reopened = pair.receiver.facts().unwrap();
    assert_eq!(reopened.current.as_ref().unwrap().id, new_binding.id);
    assert!(reopened.recovery_ops.is_empty());
    assert!(pair.receiver.driver().apply_intents(old.id).unwrap().is_empty());
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), cloud_rows);
    assert_eq!(pair.receiver.driver().media_files().unwrap()["photo.jpg"], FRESH);
    assert_eq!(pair.receiver.driver().media_rows().unwrap(), installed_media_rows);
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        settled_rows
    );
    if resolution {
        assert_eq!(pair.receiver.driver().media_files().unwrap()[&hash_name()], LOCAL);
    } else {
        assert!(
            files_under(&pair.receiver_dir.join("media.trash"))
                .values()
                .any(|bytes| bytes == LOCAL)
        );
    }
    assert!(
        files_under(&pair.receiver_dir.join("media.trash"))
            .values()
            .any(|bytes| bytes == REMOTE)
    );
    pair.close();
}

#[test]
fn cloud_adopt_settles_failed_media_install_before_ready_and_safety_backup() {
    prior_intent_adoption(false, false);
}

#[test]
fn cloud_adopt_settles_combined_resolution_before_ready_and_safety_backup() {
    prior_intent_adoption(true, false);
}

#[test]
fn media_intent_ready_reopen_keeps_copies() {
    prior_intent_adoption(false, true);
}

#[test]
fn resolution_intent_ready_reopen_keeps_copies() {
    prior_intent_adoption(true, true);
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
