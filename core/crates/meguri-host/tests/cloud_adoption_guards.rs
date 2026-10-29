//! Tests FIRST: real Service/Runtime adoption, public synthetic collections only.
//! Extract this target and the child account-input hook before production fixes.
use meguri_engine::{
    EngineError, LongOp,
    device_state::{AccountBinding, DeviceState},
    replace::ReplaceReason,
};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    self,
    adoption::{AdoptStep, RecoveryKind},
    entity_codec as codec,
    runtime::{FileIdentityProvider, KeyByteSource, KeySourceError},
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, LogOutcome, Mutation, MutationSuccess, Operation, RecordInput, Reply, RequestKind,
        Scope, Service,
        coordinator::PublicationRig,
        devices::{IdentityReads, NativeFiles, options},
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
};

const NOW: i64 = 1_735_689_600_000;
const STEPS: usize = 10_000;
struct Keys(AtomicUsize);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        match self.0.fetch_add(1, Ordering::SeqCst) {
            0 => Ok([31; 32]),
            1 => Ok([124; 32]),
            _ => Err(KeySourceError::Unavailable),
        }
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.adoption-guards".into(),
        environment: "development".into(),
        account: "synthetic-adoption-account".into(),
    }
}
fn generated(root: &Path, name: &str, seed: u64) -> PathBuf {
    let directory = root.join(name);
    let recipe = Recipe::parse("recipe: adoption-guards\ndescription: Public synthetic adoption guards\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic\n    notetype: basic\n    new: 2\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, seed, &directory).unwrap();
    directory
}
fn online(directory: &Path) -> PublicationRig {
    PublicationRig::open(
        directory,
        &scope(),
        IdentityReads::Available(scope().account),
        false,
        NOW,
        809,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap()
}
struct Pair {
    service: Service<SeededRandom>,
    sender: PublicationRig,
    receiver: PublicationRig,
    directory: PathBuf,
    _root: tempfile::TempDir,
}
impl Pair {
    fn published() -> Self {
        let root = tempfile::tempdir().unwrap();
        let sender_dir = generated(root.path(), "sender", 801);
        let directory = generated(root.path(), "receiver", 802);
        let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(811)).unwrap();
        let mut sender = online(&sender_dir);
        let preview = sender.preview(&mut service, STEPS).unwrap();
        sender.confirm_publication(&preview).unwrap();
        sender.drive_ready(&mut service, STEPS).unwrap();
        let receiver = online(&directory);
        Self {
            service,
            sender,
            receiver,
            directory,
            _root: root,
        }
    }
    fn close(&mut self) {
        self.receiver.close_clean().unwrap();
        self.sender.close_clean().unwrap();
    }
}
fn store(directory: &Path) -> cloud::TrackingStore {
    let scope = cloud::BuildScope {
        container: cloud::ContainerId::new(scope().container).unwrap(),
        environment: cloud::Environment::Development,
    };
    match cloud::TrackingStore::open(directory, scope).unwrap() {
        cloud::OpenOutcome::Opened(store) => store,
        _ => panic!("actual tracking store required"),
    }
}
fn cloud_disabled_config(directory: &Path, op: &cloud::adoption::AdoptOp) -> meguri_host::HostConfig {
    let mut config = options(directory, NOW, 819);
    assert!(config.cloud.is_none());
    config.clock = Arc::new(FixedClock::new(NOW, UtcOffset::UTC));
    config.publication_binding = match &op.candidate.binding {
        AccountBinding::Connected { .. } => op.candidate.binding.clone(),
        _ => panic!("actual bound candidate required"),
    };
    config
}

// The process aborts inside the actual Runtime, after R6 reopen and before step 9.
// A cloud-disabled CoreHost then owns journal recovery and a clean close of A.
// It cannot settle the Adopt tracking row. No SQLite sidecar is unlinked here.
fn crash_and_drain(pair: &mut Pair) -> (cloud::adoption::AdoptOp, cloud::runtime::StoredRuntimeState) {
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.driver().arm_adoption_activation_crash().unwrap();
    assert!(pair.receiver.drive_ready(&mut pair.service, STEPS).is_err());
    pair.receiver.driver().reap_adoption_activation_crash().unwrap();
    let (op, runtime) = {
        let store = store(&pair.directory);
        let op = store.adoption_op().unwrap().unwrap();
        assert_eq!(op.step, AdoptStep::Ready);
        let runtime = store.stored_runtime_state(op.binding_after).unwrap().unwrap();
        (op, runtime)
    };
    let path = pair.directory.join("collection.anki2");
    let native = NativeFiles.identity(&path).unwrap();
    assert_eq!(
        runtime.file,
        Some(native),
        "R6 generation listener recorded A before the fault"
    );
    let device = DeviceState::for_collection(&path);
    let decision = device.settled_publication().unwrap();
    assert_eq!(decision.as_ref(), Some(&op.candidate));
    let evidence = device
        .replacement_evidence_for(op.before.publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, op.before.publication);
    assert_eq!(evidence.candidate, op.candidate);
    assert!(evidence.safety_backup.is_some());
    let host = meguri_host::CoreHost::open(cloud_disabled_config(&pair.directory, &op))
        .expect("A must recover its own journals through CoreHost");
    assert!(host.cloud_runtime().is_none());
    host.close().unwrap();
    assert_eq!(NativeFiles.identity(&path).unwrap(), native);
    let after = store(&pair.directory);
    assert_eq!(after.adoption_op().unwrap().as_ref(), Some(&op));
    assert_eq!(
        after.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
        runtime.file
    );
    (op, runtime)
}

fn replace_native_candidate(
    directory: &Path,
    op: &cloud::adoption::AdoptOp,
    runtime: &cloud::runtime::StoredRuntimeState,
) -> cloud::runtime::NativeFileIdentity {
    let path = directory.join("collection.anki2");
    let device = DeviceState::for_collection(&path);
    let decision = device.publication_decision().unwrap();
    let evidence = device.replacement_evidence_for(op.before.publication.identity).unwrap();
    let host = meguri_host::CoreHost::open(cloud_disabled_config(directory, op)).unwrap();
    assert!(host.cloud_runtime().is_none());
    assert_eq!(Some(NativeFiles.identity(&path).unwrap()), runtime.file);
    let sidecar_path = directory.join("meguri-sidecar.json");
    let mut expected: serde_json::Value = serde_json::from_slice(&std::fs::read(&sidecar_path).unwrap()).unwrap();
    assert!(
        expected
            .as_object_mut()
            .unwrap()
            .remove("clean_shutdown")
            .unwrap()
            .is_boolean()
    );
    let recorded_a = runtime.file;
    {
        let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        // ReplaceTask retires witnesses and A's incarnation before closing
        // its writer; only then may byte-identical B replace the native file.
        exclusive
            .replace(move |live| {
                let replacement = live.with_file_name("closed-candidate-b.anki2");
                std::fs::copy(live, &replacement).map_err(|e| EngineError::Storage(e.into()))?;
                assert_eq!(std::fs::read(live).unwrap(), std::fs::read(&replacement).unwrap());
                let native_b = NativeFiles.identity(&replacement).unwrap();
                assert_ne!(Some(native_b), recorded_a);
                std::fs::rename(&replacement, live).map_err(|e| EngineError::Storage(e.into()))?;
                assert_eq!(NativeFiles.identity(live).unwrap(), native_b);
                Ok(())
            })
            .unwrap();
    }
    let native_b = NativeFiles.identity(&path).unwrap();
    assert_ne!(Some(native_b), runtime.file);
    // The local fixture swap grants no new cloud authority, before or after close.
    for after_close in [false, true] {
        if after_close {
            host.close().unwrap();
        }
        assert_eq!(NativeFiles.identity(&path).unwrap(), native_b);
        let mut semantic_docs: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&sidecar_path).unwrap()).unwrap();
        assert!(
            semantic_docs
                .as_object_mut()
                .unwrap()
                .remove("clean_shutdown")
                .unwrap()
                .is_boolean()
        );
        assert!(semantic_docs == expected);
        assert_eq!(device.publication_decision().unwrap(), decision);
        assert_eq!(
            device.replacement_evidence_for(op.before.publication.identity).unwrap(),
            evidence
        );
        let tracking = store(directory);
        assert_eq!(tracking.adoption_op().unwrap().as_ref(), Some(op));
        assert_eq!(
            tracking.stored_runtime_state(op.binding_after).unwrap().as_ref(),
            Some(runtime)
        );
    }
    native_b
}

#[test]
fn crash_after_r6_activation_then_different_native_file_cannot_rebase_adoption_authority() {
    let mut pair = Pair::published();
    let (op, runtime) = crash_and_drain(&mut pair);
    let path = pair.directory.join("collection.anki2");
    let device = DeviceState::for_collection(&path);
    let decision = device.publication_decision().unwrap();
    let evidence = device.replacement_evidence_for(op.before.publication.identity).unwrap();
    let native_b = replace_native_candidate(&pair.directory, &op, &runtime);
    pair.receiver = online(&pair.directory); // must reach Runtime, not journal admission
    let facts = pair.receiver.facts().unwrap();
    assert_eq!(facts.native_file, native_b);
    assert_eq!(facts.current.as_ref().unwrap().id, op.binding_after);
    assert_eq!(
        facts.current.as_ref().unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::FileMismatch)
    );
    assert_eq!(facts.recovery_ops.len(), 1);
    assert!(matches!(&facts.recovery_ops[0].kind, RecoveryKind::Adopt(retained) if **retained == op));
    assert!(!pair.receiver.wants_send().unwrap());
    assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
    assert_eq!(device.publication_decision().unwrap(), decision);
    assert_eq!(
        device.replacement_evidence_for(op.before.publication.identity).unwrap(),
        evidence
    );
    pair.receiver.close_clean().unwrap();
    let retained = store(&pair.directory);
    let after = retained.stored_runtime_state(op.binding_after).unwrap().unwrap();
    assert_eq!(after.file, runtime.file);
    assert_eq!(retained.adoption_op().unwrap().as_ref(), Some(&op));
    drop(retained);
    pair.sender.close_clean().unwrap();
}

#[test]
fn crash_after_r6_activation_reopens_same_native_candidate_and_completes_step9() {
    let mut pair = Pair::published();
    let expected = pair.sender.driver().collection_rows().unwrap();
    let (op, runtime) = crash_and_drain(&mut pair);
    pair.receiver = online(&pair.directory);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let facts = pair.receiver.facts().unwrap();
    assert!(facts.recovery_ops.is_empty());
    assert_eq!(facts.current.as_ref().unwrap().id, op.binding_after);
    assert_eq!(facts.current.as_ref().unwrap().facts.state, cloud::BindingState::Active);
    assert_eq!(Some(facts.native_file), runtime.file);
    assert_eq!(facts.decision.publication, op.candidate);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected);
    pair.close();
}

// Alter an actual published Note with a valid codec envelope and HMAC name,
// conditional on the actual Service version. Keep first field/sfld/csum intact.
fn replace_service_fields(pair: &mut Pair, shape: i8) -> codec::Note {
    let source = pair.sender.facts().unwrap();
    let binding = source.current.unwrap();
    let note = source.entities.values().find(|e| e.kind == codec::Kind::Note).unwrap();
    let codec::Payload::Note(original) = &note.payload else {
        panic!("Note required")
    };
    let nt = source
        .entities
        .values()
        .find_map(|e| match &e.payload {
            codec::Payload::Notetype(nt) if nt.id == original.mid => Some(nt),
            _ => None,
        })
        .unwrap();
    let mut changed = original.clone();
    let mut fields: Vec<_> = std::str::from_utf8(&changed.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), nt.fields.len());
    assert_eq!(fields.len(), 2);
    match shape {
        -1 => {
            fields.pop();
        }
        1 => fields.push("synthetic extra".into()),
        0 => fields[1].clear(),
        _ => unreachable!(),
    }
    changed.flds = fields.join("\u{1f}").into_bytes();
    assert_eq!(
        changed.flds.split(|b| *b == 31).next(),
        original.flds.split(|b| *b == 31).next()
    );
    assert_eq!(changed.sfld, original.sfld);
    assert_eq!(changed.csum, original.csum);
    let zone = pair.sender.collection_zone().unwrap();
    let baseline = pair.service.inspect(&scope()).unwrap().unwrap().zones[&zone].records[&note.name].clone();
    let Field::EncryptedBytes(bytes) = &baseline.fields["e"] else {
        panic!("envelope required")
    };
    let mut received = codec::receive(
        &binding.facts.record_key.0,
        &baseline.name,
        bytes,
        codec::AssetInput::Absent,
    )
    .unwrap();
    received.entity.fields.erev += 1;
    let payload = codec::Payload::Note(changed.clone());
    let envelope = codec::Entity::build(received.entity.fields, &payload.encode().unwrap())
        .unwrap()
        .encode()
        .unwrap();
    assert_eq!(
        codec::receive(
            &binding.facts.record_key.0,
            &baseline.name,
            &envelope,
            codec::AssetInput::Absent
        )
        .unwrap()
        .payload,
        Some(payload)
    );
    let response = pair
        .service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone,
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&baseline.name, "MGEntity", envelope),
                    if_unchanged: Some(baseline.system_fields),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("actual conditional Service update required")
    };
    assert_eq!(items.len(), 1);
    assert!(matches!(&items[0].result, Ok(MutationSuccess::Saved(record)) if record.name == baseline.name));
    changed
}
fn malformed_field_count(shape: i8) {
    let mut pair = Pair::published();
    replace_service_fields(&mut pair, shape);
    let local = pair.receiver.driver().collection_rows().unwrap();
    let before = pair.receiver.facts().unwrap();
    let server = pair.service.inspect(&scope()).unwrap().unwrap();
    let zone = pair.sender.collection_zone().unwrap();
    let after = pair.service.request_log().unwrap().last().unwrap().request_id;
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    assert!(
        pair.receiver.drive_ready(&mut pair.service, STEPS).is_err(),
        "K21.2 must refuse before Ready/replacement"
    );
    let refused = pair.receiver.facts().unwrap();
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), local);
    assert_eq!(refused.current, before.current);
    assert_eq!(refused.bindings, before.bindings);
    assert_eq!(refused.decision, before.decision);
    assert_eq!(refused.native_file, before.native_file);
    assert!(refused.recovery_ops.is_empty());
    let log = pair.service.request_log().unwrap();
    assert!(log.iter().any(|r| r.request_id > after
        && r.kind == RequestKind::FetchZone
        && r.outcome == LogOutcome::Success
        && r.zone.as_deref() == Some(&zone)));
    assert!(
        !log.iter()
            .any(|r| r.request_id > after && r.kind == RequestKind::Modify)
    );
    assert_eq!(pair.service.inspect(&scope()).unwrap().unwrap().zones, server.zones);
    pair.close();
}
#[test]
fn fetched_note_with_fewer_fields_than_actual_notetype_is_refused() {
    malformed_field_count(-1);
}
#[test]
fn fetched_note_with_more_fields_than_actual_notetype_is_refused() {
    malformed_field_count(1);
}
#[test]
fn fetched_note_with_valid_trailing_empty_field_is_adopted_byte_for_byte() {
    let mut pair = Pair::published();
    let expected = replace_service_fields(&mut pair, 0);
    assert_eq!(expected.flds.last(), Some(&31));
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let facts = pair.receiver.facts().unwrap();
    assert!(facts.recovery_ops.is_empty());
    assert_eq!(facts.current.as_ref().unwrap().facts.state, cloud::BindingState::Active);
    assert!(
        pair.receiver
            .driver()
            .collection_rows()
            .unwrap()
            .contains(&codec::Payload::Note(expected))
    );
    pair.close();
}
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

fn online_as(directory: &Path, account: &str) -> PublicationRig {
    let mut current = scope();
    current.account = account.into();
    PublicationRig::open(
        directory,
        &current,
        IdentityReads::Available(current.account.clone()),
        false,
        NOW,
        829,
        Arc::new(Keys(AtomicUsize::new(0))),
    )
    .unwrap()
}

// No R6 result is supplied by this test: the boundary callback mutates only
// Account's external identity input, then the real Runtime port reads it.
fn live_adoption_account_capture(account: Option<String>, isolation: Option<cloud::IsolationReason>) {
    use meguri_engine::device_state::PublicationDecision;
    let mut pair = Pair::published();
    let local_rows = pair.receiver.driver().collection_rows().unwrap();
    let expected_rows = pair.sender.driver().collection_rows().unwrap();
    assert_ne!(local_rows, expected_rows);
    let before = pair.receiver.facts().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    let op = pair.receiver.driver().arm_adoption_account_change(account).unwrap();
    assert_eq!(op.before, before.decision);
    assert_eq!(op.account.0.as_slice(), scope().account.as_bytes());
    let requests = pair.service.request_log().unwrap().len();

    // On the unfixed source R6 activates Pending, but live finalization returns
    // an error before step 9, leaving Ready and the claimed slot in memory.
    let result = pair.receiver.drive_ready(&mut pair.service, STEPS);
    let completed = pair.receiver.driver().adoption_completion(&op).unwrap();
    assert_eq!(completed.identity_changes, 1);
    assert!(completed.proof_matches);
    let facts = pair.receiver.facts().unwrap();
    assert_ne!(facts.native_file, before.native_file);
    assert_eq!(facts.decision.publication.identity, op.candidate.identity);
    assert_eq!(facts.decision.publication.binding, op.candidate.binding);
    assert_eq!(facts.decision.pending_reason, None);
    let expected_decision = if isolation.is_some() {
        PublicationDecision::Pending
    } else {
        PublicationDecision::Clear
    };
    assert_eq!(facts.decision.publication.decision, expected_decision);
    let binding = facts.current.as_ref().unwrap();
    assert_eq!(binding.id, op.binding_after);
    assert_eq!(binding.facts.account, op.account);
    assert_eq!(binding.facts.publication, op.candidate.identity);
    let expected_state = isolation
        .map(cloud::BindingState::Isolated)
        .unwrap_or(cloud::BindingState::Active);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    let path = pair.directory.join("collection.anki2");
    let device = DeviceState::for_collection(&path);
    let evidence = device
        .replacement_evidence_for(op.before.publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, op.before.publication);
    assert_eq!(evidence.candidate, op.candidate);
    let backup = evidence.safety_backup.as_ref().expect("actual package safety backup");
    assert!(pair.receiver.driver().backup_path(&backup.name).unwrap().is_file());
    assert_eq!(
        pair.receiver.driver().backup_collection_rows(&backup.name).unwrap(),
        local_rows
    );
    // Establish the real swap and backup first, so a baseline failure reports
    // the live-finalization boundary rather than an unrelated setup error.
    assert!(facts.recovery_ops.is_empty());
    assert!(completed.slot_available);
    assert_eq!(binding.facts.state, expected_state);
    assert_eq!(completed.stored_usable, isolation.is_none());
    assert_eq!(completed.identity_verified, isolation.is_none());
    assert!(result.is_ok(), "live adoption must finalize after R6 activation");
    if isolation.is_some() {
        assert!(!pair.receiver.wants_send().unwrap());
        assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
        assert!(
            pair.service.request_log().unwrap()[requests..]
                .iter()
                .all(|request| { request.kind != RequestKind::Modify })
        );
        let after = pair.service.request_log().unwrap().len();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert_eq!(pair.service.request_log().unwrap().len(), after);
    }
    pair.receiver.close_clean().unwrap();
    {
        let tracking = store(&pair.directory);
        assert!(tracking.adoption_op().unwrap().is_none());
        assert_eq!(
            tracking.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
            Some(facts.native_file)
        );
    }
    // Reopen with A: neither the retained Pending decision nor V9 isolation
    // may be cleared just because this process sees the original identity.
    pair.receiver = online(&pair.directory);
    let reopened = pair.receiver.facts().unwrap();
    assert_eq!(reopened.native_file, facts.native_file);
    assert_eq!(reopened.decision, facts.decision);
    assert!(reopened.recovery_ops.is_empty());
    assert_eq!(reopened.current.as_ref().unwrap().id, op.binding_after);
    assert_eq!(reopened.current.as_ref().unwrap().facts.state, expected_state);
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    let reopened_proof = pair.receiver.driver().adoption_completion(&op).unwrap();
    assert!(reopened_proof.proof_matches);
    assert!(reopened_proof.slot_available);
    if isolation.is_some() {
        assert!(!reopened_proof.stored_usable);
        assert!(!reopened_proof.identity_verified);
        let after = pair.service.request_log().unwrap().len();
        pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
        assert!(!pair.receiver.wants_send().unwrap());
        assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
        assert_eq!(pair.service.request_log().unwrap().len(), after);
    }
    assert_eq!(
        device.replacement_evidence_for(op.before.publication.identity).unwrap(),
        Some(evidence)
    );
    pair.close();
}

#[test]
fn live_account_change_after_safety_check_finishes_step9_before_v9_isolation() {
    live_adoption_account_capture(
        Some("different-synthetic-account".into()),
        Some(cloud::IsolationReason::AccountChanged),
    );
}

#[test]
fn live_sign_out_after_safety_check_captures_unknown_and_finishes_step9() {
    live_adoption_account_capture(None, Some(cloud::IsolationReason::SignedOut));
}

#[test]
fn live_same_account_after_safety_check_retains_clear_through_actual_port() {
    live_adoption_account_capture(Some(scope().account), None);
}

#[test]
fn r6_post_activation_account_mismatch_finishes_step9_then_isolates_without_sending() {
    use meguri_engine::device_state::{PublicationDecision, PublicationId};
    let mut pair = Pair::published();
    let expected_rows = pair.sender.driver().collection_rows().unwrap();
    let (op, runtime) = crash_and_drain(&mut pair);
    let path = pair.directory.join("collection.anki2");
    let device = DeviceState::for_collection(&path);
    let evidence = device.replacement_evidence_for(op.before.publication.identity).unwrap();
    let requests = pair.service.request_log().unwrap().len();
    pair.receiver = online_as(&pair.directory, "different-synthetic-account");
    let facts = pair.receiver.facts().unwrap();
    assert_eq!(Some(facts.native_file), runtime.file);
    assert_eq!(facts.decision.publication.identity, op.candidate.identity);
    assert_eq!(facts.decision.publication.binding, op.candidate.binding);
    assert_eq!(facts.decision.publication.decision, PublicationDecision::Pending);
    assert_eq!(facts.decision.pending_reason, None);
    assert!(facts.recovery_ops.is_empty());
    let binding = facts.current.as_ref().unwrap();
    assert_eq!(binding.id, op.binding_after);
    assert_eq!(binding.facts.account, op.account);
    assert_eq!(binding.facts.publication, op.candidate.identity);
    assert_eq!(
        binding.facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::AccountChanged)
    );
    assert_eq!(pair.receiver.driver().collection_rows().unwrap(), expected_rows);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.wants_send().unwrap());
    assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
    assert_eq!(pair.service.request_log().unwrap().len(), requests);
    let pending = device.publication_decision().unwrap();
    assert_eq!(pending.as_ref(), Some(&facts.decision));
    pair.receiver.close_clean().unwrap();
    {
        let tracking = store(&pair.directory);
        assert!(tracking.adoption_op().unwrap().is_none());
        assert_eq!(
            tracking.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
            runtime.file
        );
    }
    // Returning to the captured account cannot undo R6's Pending decision.
    pair.receiver = online(&pair.directory);
    let reopened = pair.receiver.facts().unwrap();
    assert_eq!(Some(reopened.native_file), runtime.file);
    assert_eq!(reopened.decision, facts.decision);
    assert!(reopened.recovery_ops.is_empty());
    assert_eq!(reopened.current.as_ref().unwrap().id, op.binding_after);
    assert_eq!(reopened.current.as_ref().unwrap().facts.state, binding.facts.state);
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.wants_send().unwrap());
    assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
    assert_eq!(pair.service.request_log().unwrap().len(), requests);
    assert_eq!(device.publication_decision().unwrap(), pending);
    assert_eq!(
        device.replacement_evidence_for(op.before.publication.identity).unwrap(),
        evidence
    );
    // The retained R6 proof must still name this exact operation and candidate.
    pair.receiver.close_clean().unwrap();
    let host = meguri_host::CoreHost::open({
        let mut config = options(&pair.directory, NOW, 839);
        config.publication_binding = op.candidate.binding.clone();
        config
    })
    .unwrap();
    let proof = host.collection().unwrap().cloud_adopt_activation().unwrap().unwrap();
    assert!(proof.matches(PublicationId::from_bytes(op.id), &op.before.publication, &op.candidate));
    assert!(!proof.matches(op.candidate.identity, &op.before.publication, &op.candidate));
    assert!(!proof.matches(PublicationId::from_bytes(op.id), &op.candidate, &op.candidate));
    assert!(!proof.matches(
        PublicationId::from_bytes(op.id),
        &op.before.publication,
        &op.before.publication
    ));
    assert_eq!(
        proof.native_file(),
        (runtime.file.unwrap().device, runtime.file.unwrap().inode)
    );
    assert_eq!(device.publication_decision().unwrap(), pending);
    host.close().unwrap();
    pair.sender.close_clean().unwrap();
}

#[test]
fn account_change_after_completed_adoption_keeps_r6_pending_and_native_authority() {
    use meguri_engine::device_state::PublicationDecision;
    let mut pair = Pair::published();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    let before = pair.receiver.facts().unwrap();
    assert!(before.recovery_ops.is_empty());
    assert_eq!(before.decision.publication.decision, PublicationDecision::Clear);
    let binding = before.current.as_ref().unwrap();
    pair.receiver.close_clean().unwrap();
    let requests = pair.service.request_log().unwrap().len();
    pair.receiver = online_as(&pair.directory, "different-synthetic-account");
    let changed = pair.receiver.facts().unwrap();
    assert_eq!(changed.native_file, before.native_file);
    assert_eq!(
        changed.decision.publication.identity,
        before.decision.publication.identity
    );
    assert_eq!(
        changed.decision.publication.binding,
        before.decision.publication.binding
    );
    assert_eq!(changed.decision.publication.decision, PublicationDecision::Pending);
    assert_eq!(changed.decision.pending_reason, None);
    assert_eq!(changed.current.as_ref().unwrap().id, binding.id);
    assert_eq!(changed.current.as_ref().unwrap().facts.account, binding.facts.account);
    assert_eq!(
        changed.current.as_ref().unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::AccountChanged)
    );
    assert!(changed.recovery_ops.is_empty());
    pair.receiver.drive_ready(&mut pair.service, STEPS).unwrap();
    assert!(!pair.receiver.wants_send().unwrap());
    assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
    assert_eq!(pair.service.request_log().unwrap().len(), requests);
    pair.close();
    assert!(store(&pair.directory).adoption_op().unwrap().is_none());
    assert_eq!(
        DeviceState::for_collection(&pair.directory.join("collection.anki2"))
            .publication_decision()
            .unwrap()
            .as_ref(),
        Some(&changed.decision)
    );
}

#[test]
fn unresolved_adoption_preserves_evidence_after_cancel_error_and_reopen() {
    let mut pair = Pair::published();
    let (op, runtime) = crash_and_drain(&mut pair);
    let path = pair.directory.join("collection.anki2");
    let device = DeviceState::for_collection(&path);
    let decision = device.publication_decision().unwrap();
    let evidence = device.replacement_evidence_for(op.before.publication.identity).unwrap();
    let native_b = replace_native_candidate(&pair.directory, &op, &runtime);
    for _ in 0..2 {
        pair.receiver = online(&pair.directory);
        let facts = pair.receiver.facts().unwrap();
        assert_eq!(facts.native_file, native_b);
        assert_eq!(facts.current.as_ref().unwrap().id, op.binding_after);
        assert_eq!(
            facts.current.as_ref().unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::FileMismatch)
        );
        assert_eq!(facts.recovery_ops.len(), 1);
        assert!(matches!(&facts.recovery_ops[0].kind, RecoveryKind::Adopt(retained) if **retained == op));
        assert!(pair.receiver.cancel_recovery_op(op.id).is_err());
        assert!(!pair.receiver.wants_send().unwrap());
        assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
        pair.receiver.close_clean().unwrap();
        let tracking = store(&pair.directory);
        assert_eq!(tracking.adoption_op().unwrap().as_ref(), Some(&op));
        assert_eq!(
            tracking.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
            runtime.file
        );
        assert_eq!(device.publication_decision().unwrap(), decision);
        assert_eq!(
            device.replacement_evidence_for(op.before.publication.identity).unwrap(),
            evidence
        );
    }
    pair.sender.close_clean().unwrap();
}

#[test]
fn close_cancels_live_owned_adoption_and_releases_its_stage_before_reopen() {
    let mut pair = Pair::published();
    let before = pair.receiver.facts().unwrap();
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    let id = pair.receiver.adopt_cloud(&preview).unwrap();
    let op = store(&pair.directory).adoption_op().unwrap().unwrap();
    assert_eq!(op.id, id);
    assert_eq!(op.step, AdoptStep::Staging);
    let stage = pair.directory.join(&op.staging_name);
    assert!(stage.is_dir());
    pair.receiver.close_clean().unwrap();
    assert!(!stage.exists());
    assert!(store(&pair.directory).adoption_op().unwrap().is_none());
    pair.receiver = online(&pair.directory);
    let reopened = pair.receiver.facts().unwrap();
    assert_eq!(reopened.native_file, before.native_file);
    assert_eq!(reopened.decision, before.decision);
    assert_eq!(reopened.current, before.current);
    assert!(reopened.recovery_ops.is_empty());
    assert!(!pair.receiver.wants_send().unwrap());
    assert!(pair.receiver.send_changes_calls().unwrap().is_empty());
    // A new real adoption can acquire the released slot; close cleans it again.
    let preview = pair.receiver.preview(&mut pair.service, STEPS).unwrap();
    pair.receiver.adopt_cloud(&preview).unwrap();
    pair.close();
    assert!(store(&pair.directory).adoption_op().unwrap().is_none());
}
