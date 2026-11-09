//! J3.2 observer evidence from generated collections, CoreHost and real Service
//! replies. This does not establish signed execution or recovery acceptance.
#[path = "support/publication_incoming.rs"]
mod support;
use meguri_engine::device_state::DeviceState;
use meguri_model::rng::SeededRandom;
use meguri_sync::cloud::{
    control_codec::{Active, DecodeOutcome, Epoch, EpochState, MAX_CONTROL_BYTES},
    entity_codec::sha256,
    publication::{ControlRecord, OwnKind, OwnRequest, OwnResult, PublishStep},
    selected_observation::{Missing, Proof, Provenance, PublicationEvent, ReplyOrigin, SelectedControlObservation},
};
use meguri_testkit::cloud::{
    Caller, Config, ErrorCode, Field, Operation, Reply, ReplyReference, Service, child_driver::TrackingFault,
    coordinator::Point,
};
use support::*;

fn configure_adoption(service: &mut Service<SeededRandom>) {
    assert!(
        service
            .request(
                &scope(),
                Caller::default(),
                Operation::Configure(Config {
                    page_size: 32,
                    ..Config::default()
                })
            )
            .unwrap()
            .result
            .is_ok()
    );
}

// Immutable references to the actual successful original modify responses.
// No current-count/key reconstruction and no direct tracking/evidence writes.
fn original_digests(service: &Service<SeededRandom>) -> ([u8; 32], [u8; 32]) {
    let mut actives = Vec::new();
    let mut complete_epochs = Vec::new();
    for outcome in service.outcomes().unwrap() {
        let Ok(ReplyReference::Modified(results)) = outcome.result else {
            continue;
        };
        for result in results {
            let Ok(Some(reference)) = result.result else {
                continue;
            };
            if result.name != "active" && result.name != "epoch" {
                continue;
            }
            let record = service.resolve_record(&scope(), &reference).unwrap();
            let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
                panic!("actual control payload");
            };
            if result.name == "active" {
                let active = Active::decode(bytes).unwrap();
                actives.push((active.epoch, sha256(&active.encode().unwrap())));
            } else if let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap()
                && epoch.state == EpochState::Complete
            {
                complete_epochs.push((epoch.epoch_id, sha256(&epoch.encode().unwrap())));
            }
        }
    }
    assert_eq!(actives.len(), 1);
    assert_eq!(complete_epochs.len(), 1);
    assert!(actives[0].0 == complete_epochs[0].0);
    (actives[0].1, complete_epochs[0].1)
}
fn assert_controls(observation: &SelectedControlObservation, digests: ([u8; 32], [u8; 32])) {
    assert_eq!(observation.status, Proof::Proven);
    assert_eq!(observation.binding_matches_epoch, Proof::Proven);
    assert!(observation.active_sha256 == Some(digests.0));
    assert!(observation.epoch_sha256 == Some(digests.1));
}

#[test]
fn selected_unbound_generated_collection_has_no_control_authority_or_remote_effect() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut rig = online(&directory, true);
    let before = rig.facts().unwrap();
    let observation = rig.driver().selected_control().unwrap();
    assert_eq!(observation.status, Proof::Missing(Missing::Unbound));
    assert!(observation.active_sha256.is_none() && observation.epoch_sha256.is_none());
    let after = rig.facts().unwrap();
    assert!(before.decision == after.decision && after.current.is_none() && after.recovery_ops.is_empty());
    rig.close_clean().unwrap();
}

#[test]
fn selected_publisher_requires_done_and_matches_original_accepted_control_responses() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeEpochRead).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeEpochRead));
    // Complete's original accepted payload is observed, but Active/done has not
    // committed. Neither that memory nor the MEDIA ROOT ack grants success.
    assert_ne!(rig.driver().selected_control().unwrap().status, Proof::Proven);
    rig.release(Point::BeforeEpochRead).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    let digests = original_digests(&service);
    let observation = rig.driver().selected_control().unwrap();
    assert_controls(&observation, digests);
    assert_eq!(observation.provenance, Some(Provenance::RuntimeAcceptedPublication));
    let actual = rig.driver().publication_reply_events().unwrap();
    assert_eq!(observation.publication_complete.as_ref(), actual.0.as_ref());
    assert_eq!(observation.publication_done_active.as_ref(), actual.1.as_ref());
    assert_eq!(actual.0.unwrap().origin, ReplyOrigin::Saved);
    assert_eq!(actual.1.unwrap().origin, ReplyOrigin::Saved);
    assert_ne!(observation.activation, Proof::Proven);
    assert_ne!(observation.owned_backup, Proof::Proven);
    let before = rig.facts().unwrap();
    assert!(rig.driver().selected_control().unwrap() == observation);
    let after = rig.facts().unwrap();
    assert!(
        before.decision == after.decision
            && before.current == after.current
            && before.recovery_ops == after.recovery_ops
    );
    rig.close_clean().unwrap();
    let mut reopened = online(&directory, false);
    reopened.drive_ready(&mut service, STEPS).unwrap();
    let gap = reopened.driver().selected_control().unwrap();
    assert_eq!(gap.status, Proof::Missing(Missing::MissingAfterRestart));
    assert_eq!(gap.provenance, Some(Provenance::CurrentAcceptedActive));
    assert!(gap.publication_complete.is_none() && gap.publication_done_active.is_none());
    assert!(gap.active_sha256 == Some(digests.0) && gap.epoch_sha256.is_none());
    reopened.close_clean().unwrap();
}

#[test]
fn selected_oversized_actual_decision_keeps_product_publication_but_has_no_observation() {
    use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
    use meguri_testkit::cloud::{coordinator::PublicationRig, devices::IdentityReads};
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    struct Keys(AtomicUsize);
    impl KeyByteSource for Keys {
        fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
            let call = self.0.fetch_add(1, Ordering::SeqCst);
            Ok(std::array::from_fn(|i| (i as u8).wrapping_add(31 + 93 * call as u8)))
        }
    }
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut actual_scope = scope();
    actual_scope.account = "a".repeat(8 * MAX_CONTROL_BYTES + 1);
    let mut rig = PublicationRig::open(
        &directory,
        &actual_scope,
        IdentityReads::Available(actual_scope.account.clone()),
        false,
        NOW,
        808,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap();
    publish(&mut rig, &mut service);
    let facts = rig.facts().unwrap();
    assert!(facts.root_acked && facts.recovery_ops.is_empty());
    assert!(facts.current.as_ref().unwrap().facts.account.0 == actual_scope.account.as_bytes());
    let mut complete = false;
    let mut active = false;
    for outcome in service.outcomes().unwrap() {
        let Ok(ReplyReference::Modified(items)) = outcome.result else {
            continue;
        };
        for item in items {
            if item.name != "active" && item.name != "epoch" {
                continue;
            }
            let Ok(Some(reference)) = item.result else { continue };
            let record = service.resolve_record(&actual_scope, &reference).unwrap();
            let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
                panic!("control bytes")
            };
            if item.name == "active" {
                Active::decode(bytes).unwrap();
                active = true;
            } else if let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap() {
                complete |= epoch.state == EpochState::Complete;
            }
        }
    }
    assert!(complete && active);
    let observation = rig.driver().selected_control().unwrap();
    assert_eq!(observation.status, Proof::Missing(Missing::MissingAfterRestart));
    assert!(observation.publication_complete.is_none() && observation.publication_done_active.is_none());
    assert!(observation.active_sha256.is_some() && observation.epoch_sha256.is_none());
    let actual = rig.driver().publication_reply_events().unwrap();
    assert!(actual.0.is_none() && actual.1.is_none());
    rig.close_clean().unwrap();
}

#[test]
fn selected_raw_correlation_accepts_exact_cap_and_discards_every_over_cap_event() {
    // Raw transport inputs, with no supplied observer verdict or digest.
    let decision = serde_json::json!({
        "publication": {"identity": [1, 2], "binding": "LocalOnly", "decision": "Clear"},
        "pending_reason": null
    });
    for kind in [OwnKind::SaveCompleteEpoch, OwnKind::SaveActive] {
        let mut raw = serde_json::json!({
            "op": [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0],
            "attempt": 7, "kind": kind,
            "operation": {"Save": {"zone": "meguri-root", "name": "active",
                "record_type": "Active", "encrypted": [], "if_unchanged": null}},
            "incarnation": 1, "generation": 1, "instance": "",
            "decision_before": decision, "decision_current": decision
        });
        let record = ControlRecord {
            encrypted: Some(vec![1, 2, 3]),
            system_fields: meguri_sync::cloud::OpaqueBytes(vec![4]),
        };
        for origin in [ReplyOrigin::Saved, ReplyOrigin::Conflict] {
            raw["instance"] = serde_json::json!("");
            let request: OwnRequest = serde_json::from_value(raw.clone()).unwrap();
            let base = serde_json::to_vec(&(&request, &record, origin)).unwrap().len();
            raw["instance"] = serde_json::json!("x".repeat(8 * MAX_CONTROL_BYTES - base));
            let request: OwnRequest = serde_json::from_value(raw.clone()).unwrap();
            let bytes = serde_json::to_vec(&(&request, &record, origin)).unwrap();
            assert_eq!(bytes.len(), 8 * MAX_CONTROL_BYTES);
            let reply = match origin {
                ReplyOrigin::Saved => OwnResult::Saved(record.clone()),
                ReplyOrigin::Conflict => OwnResult::Conflict(record.clone()),
            };
            let event = PublicationEvent::from_reply(&request, &reply).unwrap();
            assert_eq!(event.correlation, sha256(&bytes));
            assert_eq!(event.origin, origin);
            assert_eq!(event.attempt, request.attempt);
            assert_eq!(event.kind, kind);
            let mut over = raw.clone();
            over["instance"] = serde_json::json!(format!("{}x", raw["instance"].as_str().unwrap()));
            let request: OwnRequest = serde_json::from_value(over).unwrap();
            assert!(PublicationEvent::from_reply(&request, &reply).is_none());
            raw["instance"] = serde_json::json!("");
            let request: OwnRequest = serde_json::from_value(raw.clone()).unwrap();
            let mut oversized = record.clone();
            oversized.system_fields.0 = vec![0; 8 * MAX_CONTROL_BYTES + 1];
            let reply = match origin {
                ReplyOrigin::Saved => OwnResult::Saved(oversized),
                ReplyOrigin::Conflict => OwnResult::Conflict(oversized),
            };
            assert!(PublicationEvent::from_reply(&request, &reply).is_none());
        }
    }
}

#[test]
fn selected_matching_conflicts_keep_actual_reply_origin_and_original_request_events() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeCompleteEpochSave).unwrap();
    rig.hold(Point::BeforeActiveSave).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    for point in [Point::BeforeCompleteEpochSave, Point::BeforeActiveSave] {
        assert!(rig.is_held(point));
        let original = rig.held_request().unwrap();
        // Land the real guarded operation first; delivery of the queued
        // conditional save then returns Service's matching server conflict.
        let first = service.request(&scope(), original.caller, original.operation).unwrap();
        let Ok(Reply::Modified(items)) = first.result else {
            panic!("modify reply")
        };
        assert_eq!(items.len(), 1);
        assert!(items[0].result.is_ok());
        assert_ne!(rig.driver().selected_control().unwrap().status, Proof::Proven);
        rig.release(point).unwrap();
        rig.drive_ready(&mut service, STEPS).unwrap();
    }
    let mut active_digest = None;
    let mut epoch_digest = None;
    for outcome in service.outcomes().unwrap() {
        let Ok(ReplyReference::Modified(items)) = outcome.result else {
            continue;
        };
        for item in items {
            let Err(error) = item.result else { continue };
            if item.name != "active" && item.name != "epoch" {
                continue;
            }
            assert_eq!(error.code, ErrorCode::ServerRecordChanged);
            let reference = error.server_record.unwrap();
            let record = service.resolve_record(&scope(), &reference).unwrap();
            let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
                panic!("control bytes")
            };
            if item.name == "active" {
                assert!(active_digest.is_none());
                active_digest = Some(sha256(&Active::decode(bytes).unwrap().encode().unwrap()));
            } else {
                let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap() else {
                    panic!("epoch")
                };
                assert_eq!(epoch.state, EpochState::Complete);
                assert!(epoch_digest.is_none());
                epoch_digest = Some(sha256(&epoch.encode().unwrap()));
            }
            // Resolve the exact immutable outcome for Core's own queued request.
            let sent = rig.sent_request(outcome.request_id).unwrap();
            let Ok(Reply::Modified(actual)) = sent.response.result else {
                panic!("own reply")
            };
            assert_eq!(actual[0].result.as_ref().err().unwrap().code, error.code);
        }
    }
    let observation = rig.driver().selected_control().unwrap();
    assert_controls(&observation, (active_digest.unwrap(), epoch_digest.unwrap()));
    assert_eq!(observation.provenance, Some(Provenance::RuntimeAcceptedPublication));
    let actual = rig.driver().publication_reply_events().unwrap();
    assert_eq!(observation.publication_complete.as_ref(), actual.0.as_ref());
    assert_eq!(observation.publication_done_active.as_ref(), actual.1.as_ref());
    let complete = actual.0.unwrap();
    let done = actual.1.unwrap();
    assert_eq!(complete.origin, ReplyOrigin::Conflict);
    assert_eq!(done.origin, ReplyOrigin::Conflict);
    assert_eq!(complete.kind, OwnKind::SaveCompleteEpoch);
    assert_eq!(done.kind, OwnKind::SaveActive);
    rig.close_clean().unwrap();
}

#[test]
fn selected_actual_complete_reply_with_rolled_back_ack_control_commit_cannot_prove_publication() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeCompleteEpochSave).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeCompleteEpochSave));
    let before = rig.facts().unwrap();
    assert_eq!(before.recovery_ops.len(), 1);
    assert!(before.recovery_ops[0].step == PublishStep::Sent);
    assert!(rig.driver().publication_reply_events().unwrap().0.is_none());
    rig.driver()
        .arm_tracking_fault("cloud.publication.ack_control", TrackingFault::Io)
        .unwrap();
    rig.release(Point::BeforeCompleteEpochSave).unwrap();
    assert!(rig.drive_ready(&mut service, STEPS).is_err());
    let actual = rig.driver().publication_reply_events().unwrap();
    let complete = actual.0.unwrap();
    assert_eq!(complete.kind, OwnKind::SaveCompleteEpoch);
    assert_eq!(complete.origin, ReplyOrigin::Saved);
    assert!(actual.1.is_none());
    let mut committed_server_complete = None;
    for outcome in service.outcomes().unwrap() {
        let Ok(ReplyReference::Modified(items)) = outcome.result else {
            continue;
        };
        for item in items {
            if item.name != "epoch" {
                continue;
            }
            let Ok(Some(reference)) = item.result else { continue };
            let record = service.resolve_record(&scope(), &reference).unwrap();
            let Field::EncryptedBytes(bytes) = &record.fields["e"] else {
                panic!("epoch bytes")
            };
            if let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap()
                && epoch.state == EpochState::Complete
            {
                assert!(committed_server_complete.is_none());
                committed_server_complete = Some(sha256(&epoch.encode().unwrap()));
            }
        }
    }
    assert!(committed_server_complete.is_some());
    let after = rig.facts().unwrap();
    assert_eq!(after.recovery_ops, before.recovery_ops);
    let observation = rig.driver().selected_control().unwrap();
    assert_ne!(observation.status, Proof::Proven);
    assert!(observation.publication_complete.is_none() && observation.publication_done_active.is_none());
    rig.close_clean().unwrap();
}

#[test]
fn selected_cancelled_publisher_cannot_promote_observed_complete_reply_to_done() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeEpochRead).unwrap();
    let op = rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeEpochRead));
    rig.cancel_recovery_op(op).unwrap();
    assert_ne!(rig.driver().selected_control().unwrap().status, Proof::Proven);
    rig.close_clean().unwrap();
}

#[test]
fn selected_original_saved_active_reply_without_core_commit_cannot_prove_completion() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    rig.hold(Point::BeforeActiveSave).unwrap();
    rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert!(rig.is_held(Point::BeforeActiveSave));
    let original = rig.held_request().unwrap();
    let accepted = service.request(&scope(), original.caller, original.operation).unwrap();
    assert!(accepted.result.is_ok());
    // The actual original server write landed, but its response has never
    // crossed Core's successful publication_done transaction boundary.
    assert_ne!(rig.driver().selected_control().unwrap().status, Proof::Proven);
    rig.kill().unwrap();
    let mut reopened = online(&directory, false);
    assert_ne!(reopened.driver().selected_control().unwrap().status, Proof::Proven);
    reopened.drive_ready(&mut service, STEPS).unwrap();
    let gap = reopened.driver().selected_control().unwrap();
    assert_eq!(gap.status, Proof::Missing(Missing::MissingAfterRestart));
    assert!(gap.active_sha256.is_some() && gap.epoch_sha256.is_none());
    reopened.close_clean().unwrap();
}

#[test]
fn selected_failed_publication_has_no_original_completion_authority() {
    let root = tempfile::tempdir().unwrap();
    let directory = generated(root.path(), "sender");
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let mut rig = online(&directory, true);
    let preview = rig.preview(&mut service, STEPS).unwrap();
    assert!(
        service
            .request(&scope(), Caller::default(), Operation::SetQuota { bytes: Some(0) })
            .unwrap()
            .result
            .is_ok()
    );
    let op = rig.confirm_publication(&preview).unwrap();
    rig.drive_ready(&mut service, STEPS).unwrap();
    assert_ne!(rig.driver().selected_control().unwrap().status, Proof::Proven);
    assert!(!rig.facts().unwrap().recovery_ops.is_empty());
    rig.cancel_recovery_op(op).unwrap();
    rig.close_clean().unwrap();
}

#[test]
fn selected_adopter_joins_original_controls_exact_owned_backup_activation_and_reopen() {
    let mut pair = Pair::published();
    configure_adoption(&mut pair.service);
    let digests = original_digests(&pair.service);
    let before = pair.receiver.facts().unwrap();
    let local_rows = pair.receiver.driver().collection_rows().unwrap();
    let source_rows = pair.sender.driver().collection_rows().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.hold(Point::AdoptionBeforeActivate).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.is_held(Point::AdoptionBeforeActivate));
    assert_ne!(pair.receiver.driver().selected_control().unwrap().status, Proof::Proven);
    pair.receiver.release(Point::AdoptionBeforeActivate).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.driver().selected_control().unwrap();
    assert_controls(&adopted, digests);
    assert_eq!(adopted.provenance, Some(Provenance::RetainedAdoption));
    assert_eq!(adopted.activation, Proof::Proven);
    assert_eq!(adopted.owned_backup, Proof::Proven);
    assert!(pair.receiver.driver().collection_rows().unwrap() == source_rows);
    let device = DeviceState::for_collection(&pair.receiver_dir.join("collection.anki2"));
    let replacement = device
        .replacement_evidence_for(before.decision.publication.identity)
        .unwrap()
        .unwrap();
    let owned = replacement.safety_backup.as_ref().unwrap();
    assert!(pair.receiver.driver().backup_collection_rows(&owned.name).unwrap() == local_rows);
    // A later real edit and forced backup update lastBackup. The observer must
    // retain the exact adoption-owned receipt and original activation proof.
    let note = notes(&mut pair.receiver)[0].id;
    pair.receiver
        .driver()
        .edit_note_fields(note, &["later local front", "later local back"])
        .unwrap();
    pair.receiver.advance_clock_ms(86_400_000).unwrap();
    let later = pair.receiver.driver().backup_now().unwrap();
    assert!(later.name != owned.name);
    assert!(device.last_backup().unwrap().unwrap().name == later.name);
    assert!(
        device
            .replacement_evidence_for(before.decision.publication.identity)
            .unwrap()
            .as_ref()
            == Some(&replacement)
    );
    assert!(pair.receiver.driver().selected_control().unwrap() == adopted);
    pair.receiver.close_clean().unwrap();
    pair.receiver = online(&pair.receiver_dir, false);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(pair.receiver.driver().selected_control().unwrap() == adopted);
    pair.close();
}

#[test]
fn selected_adopted_proof_cannot_be_used_under_foreign_account() {
    let mut pair = Pair::published();
    configure_adoption(&mut pair.service);
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().selected_control().unwrap().status, Proof::Proven);
    pair.receiver.verify_identity("synthetic-other-account".into()).unwrap();
    assert_ne!(pair.receiver.driver().selected_control().unwrap().status, Proof::Proven);
    pair.close();
}

#[test]
fn selected_restore_generation_and_its_foreign_safety_receipt_cannot_prove_prior_adoption() {
    let mut pair = Pair::published();
    configure_adoption(&mut pair.service);
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let adopted = pair.receiver.facts().unwrap();
    assert_eq!(pair.receiver.driver().selected_control().unwrap().status, Proof::Proven);
    // An ordinary restore creates a different actual current file/generation
    // and its own safety receipt. Those facts cannot prove the prior CloudAdopt.
    let backup = pair.receiver.driver().backup_now().unwrap();
    pair.receiver.driver().restore_backup(&backup).unwrap();
    let restored = pair.receiver.facts().unwrap();
    assert!(restored.native_file != adopted.native_file);
    assert!(restored.decision.publication.identity != adopted.decision.publication.identity);
    let observed = pair.receiver.driver().selected_control();
    assert!(observed.is_err() || observed.unwrap().status != Proof::Proven);
    pair.close();
}

#[test]
fn selected_other_build_scope_cannot_use_retained_adoption_controls() {
    use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
    use meguri_testkit::cloud::{coordinator::PublicationRig, devices::IdentityReads};
    use std::sync::Arc;
    struct NoKeys;
    impl KeyByteSource for NoKeys {
        fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
            Err(KeySourceError::Unavailable)
        }
    }
    let mut pair = Pair::published();
    configure_adoption(&mut pair.service);
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert_eq!(pair.receiver.driver().selected_control().unwrap().status, Proof::Proven);
    pair.close();
    let mut foreign_scope = scope();
    foreign_scope.container.push_str(".other-build");
    let mut other = PublicationRig::open(
        &pair.receiver_dir,
        &foreign_scope,
        IdentityReads::Available(foreign_scope.account.clone()),
        false,
        NOW,
        809,
        Arc::new(NoKeys),
    )
    .unwrap();
    let observation = other.driver().selected_control().unwrap();
    assert_ne!(observation.status, Proof::Proven);
    assert!(observation.active_sha256.is_none() && observation.epoch_sha256.is_none());
    other.close_clean().unwrap();
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}
