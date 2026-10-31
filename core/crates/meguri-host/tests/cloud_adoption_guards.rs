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
        .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, op.before.as_ref().unwrap().publication);
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
    let prior = op
        .before
        .as_ref()
        .map(|before| before.publication.identity)
        .unwrap_or_else(|| meguri_engine::device_state::PublicationId::from_bytes(op.id));
    let evidence = device.replacement_evidence_for(prior).unwrap();
    let host = meguri_host::CoreHost::open(cloud_disabled_config(directory, op)).unwrap();
    assert!(host.cloud_runtime().is_none());
    assert_eq!(Some(NativeFiles.identity(&path).unwrap()), runtime.file);
    let rows = stored_collection_rows(&host);
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
    assert_eq!(stored_collection_rows(&host), rows);
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
        assert_eq!(device.replacement_evidence_for(prior).unwrap(), evidence);
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
    let evidence = device
        .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
        .unwrap();
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
        device
            .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
            .unwrap(),
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
    assert_eq!(op.before.as_ref(), Some(&before.decision));
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
        .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
        .unwrap()
        .unwrap();
    assert_eq!(evidence.reason, ReplaceReason::CloudAdopt);
    assert_eq!(evidence.prior, op.before.as_ref().unwrap().publication);
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
        device
            .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
            .unwrap(),
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
    let evidence = device
        .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
        .unwrap();
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
        device
            .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
            .unwrap(),
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
    assert!(proof.matches(
        PublicationId::from_bytes(op.id),
        &op.before.as_ref().unwrap().publication,
        &op.candidate
    ));
    assert!(!proof.matches(
        op.candidate.identity,
        &op.before.as_ref().unwrap().publication,
        &op.candidate
    ));
    assert!(!proof.matches(PublicationId::from_bytes(op.id), &op.candidate, &op.candidate));
    assert!(!proof.matches(
        PublicationId::from_bytes(op.id),
        &op.before.as_ref().unwrap().publication,
        &op.before.as_ref().unwrap().publication
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
    let evidence = device
        .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
        .unwrap();
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
            device
                .replacement_evidence_for(op.before.as_ref().unwrap().publication.identity)
                .unwrap(),
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

// Initial adoption uses a genuinely absent destination and the original retained
// providers. The sender above publishes actual serialized synthetic rows.
struct InitialIdentity(std::sync::Mutex<cloud::runtime::IdentityStatus>);
impl cloud::runtime::IdentityProvider for InitialIdentity {
    fn read_identity(&self, _: &cloud::BuildScope) -> cloud::runtime::IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
struct HeldInitialIdentity {
    source: Arc<dyn cloud::runtime::IdentityProvider>,
    entered: std::sync::Barrier,
    release: std::sync::Barrier,
    did_hold: std::sync::atomic::AtomicBool,
}
impl cloud::runtime::IdentityProvider for HeldInitialIdentity {
    fn read_identity(&self, scope: &cloud::BuildScope) -> cloud::runtime::IdentityStatus {
        if !self.did_hold.swap(true, Ordering::SeqCst) {
            self.entered.wait();
            self.release.wait();
        }
        self.source.read_identity(scope)
    }
}
fn hold_initial_identity(config: &mut meguri_host::HostConfig) -> Arc<HeldInitialIdentity> {
    let inputs = config.cloud.as_mut().unwrap();
    let identity = Arc::new(HeldInitialIdentity {
        source: inputs.identity.clone(),
        entered: std::sync::Barrier::new(2),
        release: std::sync::Barrier::new(2),
        did_hold: std::sync::atomic::AtomicBool::new(false),
    });
    inputs.identity = identity.clone();
    identity
}
struct NoInitialBackup(AtomicUsize);
impl meguri_engine::ports::BackupService for NoInitialBackup {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(EngineError::Internal {
            message: "no collection exists to back up".into(),
        })
    }
}
fn initial_config(directory: &Path) -> (meguri_host::HostConfig, Arc<InitialIdentity>, Arc<NoInitialBackup>) {
    std::fs::create_dir_all(directory).unwrap();
    let identity = Arc::new(InitialIdentity(std::sync::Mutex::new(
        cloud::runtime::IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())),
    )));
    let backup = Arc::new(NoInitialBackup(AtomicUsize::new(0)));
    let mut config = options(directory, NOW, 881).with_backup_for_testing(backup.clone());
    config.cloud = Some(cloud::runtime::CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope().container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity: identity.clone(),
        files: Arc::new(NativeFiles),
    });
    (config, identity, backup)
}
fn initial_read(service: &mut Service<SeededRandom>, zone: &str, name: &str) -> cloud::publication::RecordRead {
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: zone.into(),
                name: name.into(),
                fields: meguri_testkit::cloud::RequestedFields::All,
            },
        )
        .unwrap();
    let Reply::Record(record) = response.result.unwrap() else {
        panic!("actual server record required")
    };
    cloud::publication::RecordRead::Found(
        meguri_testkit::cloud::coordinator::publication_control_record(&record).unwrap(),
    )
}
fn initial_preview(host: &meguri_host::CoreHost, service: &mut Service<SeededRandom>) -> cloud::publication::Preview {
    use cloud::publication::PreviewWork;
    let core = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = host.begin_cloud_connection_preview().unwrap() else {
        panic!("active")
    };
    let PreviewWork::ReadEpoch { epoch, .. } = core
        .connection_active_read(id, initial_read(service, "meguri-root", "active"))
        .unwrap()
    else {
        panic!("epoch")
    };
    let PreviewWork::Ready(p) = core
        .connection_epoch_read(
            id,
            initial_read(service, &cloud::adapter::collection_zone(epoch), "epoch"),
        )
        .unwrap()
    else {
        panic!("ready")
    };
    assert!(p.native_file.is_none());
    assert_eq!(
        p.decision_before,
        DeviceState::for_collection(&host.cloud_connection_directory().join("collection.anki2"))
            .publication_decision()
            .unwrap()
    );
    *p
}
fn initial_reply(
    service: &mut Service<SeededRandom>,
    epoch: cloud::EpochId,
    task: &cloud::adoption::AdoptionTask,
) -> cloud::adoption::AdoptionReply {
    use cloud::adoption::{AdoptionReply, AdoptionTask as T, StagingRecord};
    match task {
        T::ReadActive | T::RecheckActive => AdoptionReply::Read(initial_read(service, "meguri-root", "active")),
        T::ReadEpoch => AdoptionReply::Read(initial_read(service, &cloud::adapter::collection_zone(epoch), "epoch")),
        T::ReadMediaRoot => AdoptionReply::Read(initial_read(service, "meguri-media", "root")),
        T::Fetch { cursor } => {
            let response = service
                .request(
                    &scope(),
                    Caller::default(),
                    Operation::FetchZone {
                        zone: cloud::adapter::collection_zone(epoch),
                        token: cursor.as_ref().map(|c| serde_json::from_slice(&c.0).unwrap()),
                    },
                )
                .unwrap();
            let Reply::ZoneChanges(page) = response.result.unwrap() else {
                panic!("real page")
            };
            let mut records = Vec::new();
            let mut deleted = Vec::new();
            for change in page.changes {
                match change {
                    meguri_testkit::cloud::RecordChange::Saved(record) => records.push(StagingRecord {
                        record: meguri_testkit::cloud::coordinator::publication_control_record(&record).unwrap(),
                        name: record.name,
                        record_type: record.record_type,
                        payload_file: None,
                    }),
                    meguri_testkit::cloud::RecordChange::Deleted { name, .. } => deleted.push(name),
                }
            }
            AdoptionReply::Page {
                records,
                deleted,
                next: page
                    .more
                    .then(|| cloud::OpaqueBytes(serde_json::to_vec(&page.token).unwrap())),
            }
        }
        _ => panic!("local work has no server reply"),
    }
}
fn initial_step(
    host: &meguri_host::CoreHost,
    service: &mut Service<SeededRandom>,
) -> Result<(), cloud::runtime::RuntimeError> {
    use cloud::{adoption::AdoptionTask as T, publication::PublicationWork};
    let PublicationWork::Adoption(work) = host.next_cloud_publication_work()? else {
        panic!("actual adoption work")
    };
    if matches!(work.task, T::Validate | T::Activate) {
        host.perform_cloud_adoption_local(&work)
    } else {
        let op = host.cloud_runtime().unwrap().adoption_op()?.unwrap();
        host.cloud_adoption_reply(&work, initial_reply(service, op.epoch, &work.task))
    }
}
fn initial_rows(host: &meguri_host::CoreHost) -> Vec<codec::Payload> {
    use cloud::row_payload::{self, View};
    use meguri_storage::{ReaderRole, RowKey, Table};
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.snapshot().unwrap();
    let view = row_payload::ReadView(&*snapshot);
    let mut layouts = std::collections::BTreeMap::new();
    let mut after = None;
    loop {
        let page = view.page(Table::Notetypes, after.as_ref()).unwrap();
        if page.is_empty() {
            break;
        }
        for row in page {
            let id = row_payload::int(&row, 0).unwrap();
            after = Some(RowKey::Id(id));
            layouts.insert(id, row_payload::notetype(&view, &row, 1).unwrap());
        }
    }
    let mut result = Vec::new();
    for kind in row_payload::KINDS {
        let Some(table) = row_payload::table(kind) else {
            continue;
        };
        let mut after = None;
        loop {
            let page = view.page(table, after.as_ref()).unwrap();
            if page.is_empty() {
                break;
            }
            for row in page {
                after = Some(RowKey::from_stored(table, &row).unwrap());
                result.push(
                    row_payload::project(&view, kind, &row, |id| {
                        layouts.get(&id).cloned().ok_or(cloud::Error::InvalidData)
                    })
                    .unwrap(),
                );
            }
        }
    }
    result
}
fn stored_collection_rows(host: &meguri_host::CoreHost) -> Vec<(meguri_storage::Table, Vec<meguri_storage::RowImage>)> {
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Cloud).unwrap();
    let snapshot = reader.snapshot().unwrap();
    Table::ALL
        .into_iter()
        .map(|table| {
            let mut rows = Vec::new();
            let mut after = None;
            loop {
                let page = snapshot.row_page(table, after.as_ref()).unwrap();
                let Some(last) = page.last() else {
                    break;
                };
                after = Some(RowKey::from_stored(table, last).unwrap());
                rows.extend(page);
            }
            (table, rows)
        })
        .collect()
}
fn initial_file_bytes(directory: &Path) -> Vec<(&'static str, Option<Vec<u8>>)> {
    [
        "collection.anki2",
        "collection.anki2-wal",
        "collection.anki2-shm",
        "collection.anki2-journal",
        "meguri-sidecar.json",
    ]
    .into_iter()
    .map(|name| {
        let bytes = match std::fs::read(directory.join(name)) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => panic!("cannot read {name}: {e}"),
        };
        (name, bytes)
    })
    .collect()
}
#[test]
fn foreign_collection_arriving_during_account_read_is_preserved_and_never_opened() {
    for ready in [false, true] {
        for saved_sidecar in [false, true] {
            let mut pair = Pair::published();
            let directory = pair._root.path().join("initial");
            let pending =
                ready.then(|| crash_initial_adoption(&mut pair, &directory, "replace.media.initial_before_ensure"));
            let selection_before = pending.as_ref().map(|_| store(&directory).selected_binding().unwrap());
            let foreign = generated(pair._root.path(), "foreign", 991);
            let mut local_config = initial_config(&foreign).0;
            local_config.cloud = None;
            let local = meguri_host::CoreHost::open(local_config).unwrap();
            assert!(!stored_collection_rows(&local).is_empty());
            local.close().unwrap();
            let (mut config, _, backup) = initial_config(&directory);
            let identity = hold_initial_identity(&mut config);
            let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
            let generation = process.engine().generation();
            let worker = {
                let process = process.clone();
                std::thread::spawn(move || process.prepare_cloud_connection(config))
            };
            identity.entered.wait();
            assert!(!directory.join("collection.anki2").exists());
            std::fs::rename(foreign.join("collection.anki2"), directory.join("collection.anki2")).unwrap();
            if saved_sidecar {
                std::fs::rename(
                    foreign.join("meguri-sidecar.json"),
                    directory.join("meguri-sidecar.json"),
                )
                .unwrap();
            }
            let before = initial_file_bytes(&directory);
            let native = NativeFiles.identity(&directory.join("collection.anki2")).unwrap();
            identity.release.wait();
            assert!(worker.join().unwrap().is_err());
            assert_eq!(initial_file_bytes(&directory), before);
            assert_eq!(
                NativeFiles.identity(&directory.join("collection.anki2")).unwrap(),
                native
            );
            assert_eq!(process.engine().generation(), generation);
            assert!(matches!(
                process.engine().state(),
                meguri_engine::LifecycleState::NoCollection
            ));
            assert!(process.engine().collection().is_err());
            assert_eq!(process.engine().slot_holder(), None);
            if let Some(op) = pending {
                let saved = store(&directory);
                assert_eq!(saved.adoption_op().unwrap().as_ref(), Some(&op));
                assert!(
                    saved
                        .stored_runtime_state(op.binding_after)
                        .unwrap()
                        .unwrap()
                        .file
                        .is_none()
                );
                assert!(saved.selected_binding().unwrap() == selection_before.unwrap());
            } else {
                assert!(!directory.join("meguri-cloud.db").exists());
            }
            assert_eq!(backup.0.load(Ordering::SeqCst), 0);
            pair.close();
        }
    }
}
#[test]
fn exact_initial_r6_candidate_arriving_during_account_read_uses_the_prepared_host() {
    for point in ["replace.after_rename", "cloud.adopt.after_activation"] {
        let mut pair = Pair::published();
        let expected_rows = pair.sender.driver().collection_rows().unwrap();
        let directory = pair._root.path().join("initial");
        let op = crash_initial_adoption(&mut pair, &directory, point);
        let live = directory.join("collection.anki2");
        let native = NativeFiles.identity(&live).unwrap();
        let parked = directory.join("held-candidate.anki2");
        std::fs::rename(&live, &parked).unwrap();
        let (mut config, _, backup) = initial_config(&directory);
        let identity = hold_initial_identity(&mut config);
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let generation = process.engine().generation();
        let worker = {
            let process = process.clone();
            std::thread::spawn(move || process.prepare_cloud_connection(config))
        };
        identity.entered.wait();
        assert!(process.engine().collection().is_err());
        std::fs::rename(&parked, &live).unwrap();
        identity.release.wait();
        let host = worker.join().unwrap().unwrap();
        assert!(std::ptr::eq(host.review(), process.review()));
        assert_eq!(host.engine().generation().0, generation.0 + 1);
        assert_eq!(process.engine().generation(), host.engine().generation());
        assert_eq!(host.engine().slot_holder(), None);
        assert_eq!(initial_rows(&host), expected_rows);
        let proof = host.collection().unwrap().cloud_adopt_activation().unwrap().unwrap();
        assert!(proof.matches_initial_prior(
            meguri_engine::device_state::PublicationId::from_bytes(op.id),
            op.before.as_ref().map(|before| &before.publication),
            &op.candidate
        ));
        assert_eq!(proof.native_file(), (native.device, native.inode));
        assert_eq!(NativeFiles.identity(&live).unwrap(), native);
        assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
        let saved = store(&directory);
        assert_eq!(
            saved.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
            Some(native)
        );
        drop(saved);
        assert_eq!(backup.0.load(Ordering::SeqCst), 0);
        host.close().unwrap();
        pair.close();
    }
}
#[test]
fn retained_no_collection_host_adopts_real_inline_rows_without_backup_and_reopens_exact_binding() {
    let mut pair = Pair::published();
    let expected = pair.sender.driver().collection_rows().unwrap();
    let directory = pair._root.path().join("initial");
    let (config, _, backup) = initial_config(&directory);
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config.clone()).unwrap();
    let generation = host.engine().generation();
    let events = host.engine().stream().subscribe();
    let preview = initial_preview(&host, &mut pair.service);
    assert!(host.collection().is_err() && !directory.join("collection.anki2").exists());
    host.confirm_cloud_adoption(&preview).unwrap(); // internal transition, no user confirmation
    let op = host.cloud_runtime().unwrap().adoption_op().unwrap().unwrap();
    assert!(op.before.is_none() && op.file_before.is_none());
    for _ in 0..64 {
        if host.cloud_runtime().unwrap().adoption_op().unwrap().is_none() {
            break;
        }
        initial_step(&host, &mut pair.service).unwrap();
    }
    assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
    assert_eq!(host.engine().generation().0, generation.0 + 1);
    assert!(matches!(
        host.engine().state(),
        meguri_engine::LifecycleState::Ready { .. }
    ));
    assert_eq!(host.engine().slot_holder(), None);
    assert_eq!(initial_rows(&host), expected);
    let live = directory.join("collection.anki2");
    let device = DeviceState::for_collection(&live);
    assert_eq!(device.settled_publication().unwrap(), Some(op.candidate.clone()));
    let proof = host.collection().unwrap().cloud_adopt_activation().unwrap().unwrap();
    assert!(proof.matches_initial(
        meguri_engine::device_state::PublicationId::from_bytes(op.id),
        &op.candidate
    ));
    assert!(!proof.matches(
        meguri_engine::device_state::PublicationId::from_bytes(op.id),
        &op.candidate,
        &op.candidate
    ));
    let native = NativeFiles.identity(&live).unwrap();
    assert_eq!(proof.native_file(), (native.device, native.inode));
    let mut jobs = 0;
    let mut replacements = 0;
    while let Some(event) = events.recv_timeout(std::time::Duration::from_millis(1)) {
        match &event.kind {
            meguri_engine::EventKind::JobFinished {
                outcome: meguri_engine::JobOutcome::Succeeded,
                ..
            } => jobs += 1,
            meguri_engine::EventKind::CollectionReplaced { .. } => replacements += 1,
            _ => {}
        }
    }
    assert_eq!(jobs, 2);
    assert_eq!(replacements, 1);
    assert_eq!(backup.0.load(Ordering::SeqCst), 0);
    host.close().unwrap();
    let saved = store(&directory);
    let binding = saved.binding(op.binding_after).unwrap().unwrap();
    assert_eq!(binding.facts.publication, op.candidate.identity);
    assert_eq!(binding.facts.epoch, op.epoch);
    assert_eq!(
        saved.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
        Some(native)
    );
    drop(saved);
    let reopened = process.open(config).unwrap();
    assert_eq!(initial_rows(&reopened), expected);
    assert_eq!(NativeFiles.identity(&live).unwrap(), native);
    assert_eq!(
        DeviceState::for_collection(&live).settled_publication().unwrap(),
        Some(op.candidate)
    );
    reopened.close().unwrap();
    pair.close();
}
#[test]
fn initial_adoption_cancel_account_change_and_destination_collision_leave_no_false_binding() {
    for case in ["cancel", "account", "collision", "missing-stage"] {
        let mut pair = Pair::published();
        let directory = pair._root.path().join("initial");
        let (config, identity, backup) = initial_config(&directory);
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let host = process.prepare_cloud_connection(config).unwrap();
        let preview = initial_preview(&host, &mut pair.service);
        let id = host.confirm_cloud_adoption(&preview).unwrap();
        for _ in 0..3 {
            initial_step(&host, &mut pair.service).unwrap();
        }
        let op = host.cloud_runtime().unwrap().adoption_op().unwrap().unwrap();
        let stage = directory.join(&op.staging_name);
        let live = directory.join("collection.anki2");
        match case {
            "cancel" => host.cancel_cloud_adoption(id).unwrap(),
            "account" => {
                *identity.0.lock().unwrap() =
                    cloud::runtime::IdentityStatus::Available(cloud::OpaqueBytes(b"other-account".to_vec()));
                assert!(matches!(
                    host.next_cloud_publication_work().unwrap(),
                    cloud::publication::PublicationWork::Idle
                ));
            }
            "collision" => {
                std::fs::write(&live, b"foreign destination").unwrap();
                assert!(matches!(
                    host.next_cloud_publication_work().unwrap(),
                    cloud::publication::PublicationWork::Idle
                ));
                assert_eq!(std::fs::read(&live).unwrap(), b"foreign destination");
            }
            "missing-stage" => {
                initial_step(&host, &mut pair.service).unwrap(); // fetch actual rows
                std::fs::remove_file(stage.join("collection.anki2")).unwrap();
                assert!(initial_step(&host, &mut pair.service).is_err());
            }
            _ => unreachable!(),
        }
        assert!(host.collection().is_err());
        assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
        assert_eq!(host.engine().slot_holder(), None);
        assert!(!stage.exists());
        if case != "collision" {
            assert!(!live.exists());
        }
        assert_eq!(backup.0.load(Ordering::SeqCst), 0);
        host.close().unwrap();
        let saved = store(&directory);
        assert!(saved.selected_binding().unwrap().is_none());
        assert!(
            DeviceState::for_collection(&live)
                .publication_decision()
                .unwrap()
                .is_none()
        );
        drop(saved);
        pair.close();
    }
}

#[test]
fn adoption_payload_source_failures_and_owned_copy_cancellation_leave_live_untouched() {
    use cloud::{
        adoption::{AdoptionReply, AdoptionTask as T},
        publication::{PreviewWork, PublicationWork},
    };
    use std::io::Write;

    for case in [
        "missing",
        "truncated",
        "changed",
        "malformed",
        "owned-changed",
        "cancel",
        "restart",
    ] {
        let mut pair = Pair::published();
        let note = pair
            .sender
            .driver()
            .collection_rows()
            .unwrap()
            .into_iter()
            .find_map(|payload| {
                if let codec::Payload::Note(note) = payload {
                    Some(note.id)
                } else {
                    None
                }
            })
            .unwrap();
        let answer = "ordinary adoption payload ".repeat(44_000);
        pair.sender
            .driver()
            .edit_note_fields(note, &["ordinary adoption front", &answer])
            .unwrap();
        pair.sender.sync_now_and_drain(&mut pair.service, STEPS).unwrap();
        let zone = pair.sender.collection_zone().unwrap();
        let server = pair.service.inspect(&scope()).unwrap().unwrap();
        let remote = server.zones[&zone]
            .records
            .values()
            .find(|record| record.fields.contains_key("pa"))
            .unwrap();
        let Field::Asset(asset) = &remote.fields["pa"] else {
            panic!("real pa save")
        };
        let Field::EncryptedBytes(envelope) = &remote.fields["e"] else {
            panic!("real envelope")
        };
        let codec::DecodeOutcome::Supported(entity) = codec::Entity::decode(envelope).unwrap() else {
            panic!("supported")
        };
        assert!(entity.payload.is_none() && entity.fields.generation.is_none());
        assert_eq!(codec::sha256(asset.bytes()), entity.payload_sha256);
        assert_eq!(asset.size(), entity.payload_size);

        let directory = generated(pair._root.path(), "live-source-failure", 899);
        let (config, _, backup) = initial_config(&directory);
        let host = meguri_host::CoreHost::open(config.clone()).unwrap();
        let core = host.cloud_runtime().unwrap();
        let local = initial_rows(&host);
        let generation = host.engine().generation();
        let before_binding = core.status().unwrap().binding;
        let live = directory.join("collection.anki2");
        let before_file = NativeFiles.identity(&live).unwrap();
        let before_decision = DeviceState::for_collection(&live).publication_decision().unwrap();
        let PreviewWork::ReadActive { id } = host.begin_cloud_connection_preview().unwrap() else {
            panic!("active")
        };
        let PreviewWork::ReadEpoch { epoch, .. } = core
            .connection_active_read(id, initial_read(&mut pair.service, "meguri-root", "active"))
            .unwrap()
        else {
            panic!("epoch")
        };
        let PreviewWork::Ready(preview) = core
            .connection_epoch_read(id, initial_read(&mut pair.service, &zone, "epoch"))
            .unwrap()
        else {
            panic!("ready")
        };
        let id = host.confirm_cloud_adoption(&preview).unwrap();
        for _ in 0..3 {
            initial_step(&host, &mut pair.service).unwrap();
        }
        let op = core.adoption_op().unwrap().unwrap();
        let stage = directory.join(&op.staging_name);
        let PublicationWork::Adoption(work) = host.next_cloud_publication_work().unwrap() else {
            panic!("adoption fetch")
        };
        assert!(matches!(work.task, T::Fetch { .. }));
        let mut reply = initial_reply(&mut pair.service, epoch, &work.task);
        let AdoptionReply::Page { records, .. } = &mut reply else {
            panic!("real page")
        };
        let item = records.iter_mut().find(|item| item.name == remote.name).unwrap();
        assert_eq!(item.record.encrypted.as_ref().unwrap(), envelope);
        assert_eq!(item.record.system_fields.0, remote.system_fields.bytes());
        let mut source = tempfile::NamedTempFile::new_in(pair._root.path()).unwrap();
        source.write_all(asset.bytes()).unwrap();
        source.as_file().sync_all().unwrap();
        let source_path = source.path().to_path_buf();
        item.payload_file = Some(source_path.clone());
        match case {
            "missing" => std::fs::remove_file(&source_path).unwrap(),
            "truncated" => source.as_file().set_len(asset.size() - 1).unwrap(),
            "changed" => std::fs::write(&source_path, vec![0; asset.size() as usize]).unwrap(),
            "malformed" => std::fs::write(&source_path, b"not a canonical payload").unwrap(),
            _ => {}
        }
        let source_before = std::fs::read(&source_path).ok();
        let result = host.cloud_adoption_reply(&work, reply);
        if ["owned-changed", "cancel", "restart"].contains(&case) {
            result.unwrap();
            let owned = stage.join("payloads").join(format!("{}.pa", remote.name));
            assert_eq!(std::fs::read(&owned).unwrap(), asset.bytes());
            // The source is still caller-owned, and its removal cannot break
            // the core copy retained for validation/ACK installation.
            assert_eq!(std::fs::read(&source_path).unwrap(), asset.bytes());
            std::fs::remove_file(&source_path).unwrap();
            assert_eq!(std::fs::read(&owned).unwrap(), asset.bytes());
            if case == "owned-changed" {
                std::fs::write(&owned, b"changed after intake").unwrap();
                assert!(initial_step(&host, &mut pair.service).is_err());
            } else if case == "cancel" {
                host.cancel_cloud_adoption(id).unwrap();
            } else {
                // Closing and reopening must sweep unpublished stage files.
                assert_eq!(initial_rows(&host), local);
                host.close().unwrap();
                drop(host);
                let reopened = meguri_host::CoreHost::open(config).unwrap();
                assert_eq!(initial_rows(&reopened), local);
                assert_eq!(
                    reopened.cloud_runtime().unwrap().status().unwrap().binding,
                    before_binding
                );
                assert!(reopened.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
                assert!(!stage.exists());
                assert_eq!(NativeFiles.identity(&live).unwrap(), before_file);
                assert_eq!(
                    DeviceState::for_collection(&live).publication_decision().unwrap(),
                    before_decision
                );
                assert_eq!(backup.0.load(Ordering::SeqCst), 0);
                reopened.close().unwrap();
                pair.close();
                continue;
            }
        } else {
            assert!(result.is_err(), "a bad source cannot complete staging");
            assert_eq!(std::fs::read(&source_path).ok(), source_before);
        }
        assert_eq!(initial_rows(&host), local);
        assert_eq!(host.engine().generation(), generation);
        assert_eq!(core.status().unwrap().binding, before_binding);
        assert!(core.adoption_op().unwrap().is_none());
        assert_eq!(host.engine().slot_holder(), None);
        assert!(!stage.exists());
        assert_eq!(NativeFiles.identity(&live).unwrap(), before_file);
        assert_eq!(
            DeviceState::for_collection(&live).publication_decision().unwrap(),
            before_decision
        );
        assert_eq!(backup.0.load(Ordering::SeqCst), 0);
        host.close().unwrap();
        pair.close();
    }
}

// A small child consumes only actual Service replies prepared by its parent.
// All preview/adoption/journal/binding writes still run through the real Host.
#[test]
fn initial_adoption_crash_child_entry() {
    let Some(directory) = std::env::var_os("MEGURI_INITIAL_ADOPT_CHILD") else {
        return;
    };
    let directory = PathBuf::from(directory);
    let point = std::env::var("MEGURI_INITIAL_ADOPT_POINT").unwrap();
    let (config, _, backup) = initial_config(&directory);
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config).unwrap();
    let mut replies: std::collections::VecDeque<cloud::adoption::AdoptionReply> =
        serde_json::from_slice(&std::fs::read(directory.join("service-replies.json")).unwrap()).unwrap();
    use cloud::{
        adoption::{AdoptionReply, AdoptionTask as T},
        publication::{PreviewWork, PublicationWork},
    };
    let PreviewWork::ReadActive { id } = host.begin_cloud_connection_preview().unwrap() else {
        panic!("active")
    };
    let AdoptionReply::Read(active) = replies.pop_front().unwrap() else {
        panic!("active reply")
    };
    let PreviewWork::ReadEpoch { .. } = host
        .cloud_runtime()
        .unwrap()
        .connection_active_read(id, active)
        .unwrap()
    else {
        panic!("epoch")
    };
    let AdoptionReply::Read(epoch) = replies.pop_front().unwrap() else {
        panic!("epoch reply")
    };
    let PreviewWork::Ready(preview) = host.cloud_runtime().unwrap().connection_epoch_read(id, epoch).unwrap() else {
        panic!("ready")
    };
    host.confirm_cloud_adoption(&preview).unwrap();
    meguri_model::faultpoint::arm(&point, meguri_model::faultpoint::Fault::Abort);
    for _ in 0..64 {
        let PublicationWork::Adoption(work) = host.next_cloud_publication_work().unwrap() else {
            panic!("actual work")
        };
        if matches!(work.task, T::Validate | T::Activate) {
            if work.task == T::Activate {
                let runtime = host.cloud_runtime().unwrap();
                let op = runtime.adoption_op().unwrap().unwrap();
                assert!(op.file_before.is_none() && op.binding_before.is_none());
                assert_eq!(op.step, AdoptStep::Ready);
                assert!(host.collection().is_err());
                let status = runtime.status().unwrap();
                assert!(!status.stored_usable && !status.identity_verified);
                assert!(runtime.require_collection_sending().is_err());
                assert!(
                    store(&directory)
                        .stored_runtime_state(op.binding_after)
                        .unwrap()
                        .unwrap()
                        .file
                        .is_none()
                );
                assert_eq!(backup.0.load(Ordering::SeqCst), 0);
            }
            host.perform_cloud_adoption_local(&work).unwrap();
        } else {
            host.cloud_adoption_reply(&work, replies.pop_front().expect("actual Service reply"))
                .unwrap();
        }
    }
    panic!("selected crash boundary was never reached");
}
fn crash_initial_adoption(pair: &mut Pair, directory: &Path, point: &str) -> cloud::adoption::AdoptOp {
    use cloud::adoption::{AdoptionReply, AdoptionTask as T};
    let active = initial_read(&mut pair.service, "meguri-root", "active");
    let cloud::publication::RecordRead::Found(record) = &active else {
        panic!("active")
    };
    let epoch = cloud::EpochId(
        cloud::control_codec::Active::decode(record.encrypted.as_deref().unwrap())
            .unwrap()
            .epoch,
    );
    let mut replies = vec![
        AdoptionReply::Read(active),
        AdoptionReply::Read(initial_read(
            &mut pair.service,
            &cloud::adapter::collection_zone(epoch),
            "epoch",
        )),
    ];
    for task in [T::ReadActive, T::ReadEpoch, T::ReadMediaRoot] {
        replies.push(initial_reply(&mut pair.service, epoch, &task));
    }
    let mut cursor = None;
    loop {
        let reply = initial_reply(&mut pair.service, epoch, &T::Fetch { cursor });
        let AdoptionReply::Page { next, .. } = &reply else {
            unreachable!()
        };
        cursor = next.clone();
        replies.push(reply);
        if cursor.is_none() {
            break;
        }
    }
    replies.push(initial_reply(&mut pair.service, epoch, &T::RecheckActive));
    std::fs::create_dir_all(directory).unwrap();
    std::fs::write(
        directory.join("service-replies.json"),
        serde_json::to_vec(&replies).unwrap(),
    )
    .unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "initial_adoption_crash_child_entry", "--nocapture"])
        .env("MEGURI_INITIAL_ADOPT_CHILD", directory)
        .env("MEGURI_INITIAL_ADOPT_POINT", point)
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(
        status.signal(),
        Some(6),
        "child must abort at the actual fault, not fail an assertion"
    );
    let op = store(directory).adoption_op().unwrap().unwrap();
    assert_eq!(op.step, AdoptStep::Ready);
    assert!(op.file_before.is_none() && op.binding_before.is_none());
    op
}
#[test]
fn initial_adoption_intent_crash_relaunch_restores_absence_and_optional_prior_sidecar() {
    for prior_sidecar in [false, true] {
        let mut pair = Pair::published();
        let directory = if prior_sidecar {
            generated(pair._root.path(), "initial", 997)
        } else {
            pair._root.path().join("initial")
        };
        if prior_sidecar {
            let mut config = initial_config(&directory).0;
            config.cloud = None;
            let host = meguri_host::CoreHost::open(config).unwrap();
            host.close().unwrap();
            std::fs::remove_file(directory.join("collection.anki2")).unwrap();
        }
        let live = directory.join("collection.anki2");
        let device = DeviceState::for_collection(&live);
        let prior = device.publication_decision().unwrap();
        assert_eq!(prior.is_some(), prior_sidecar);
        let op = crash_initial_adoption(&mut pair, &directory, "replace.after_pause_recorded");
        assert_eq!(op.before, prior);
        assert!(!live.exists());
        let saved = store(&directory);
        assert!(
            saved
                .stored_runtime_state(op.binding_after)
                .unwrap()
                .unwrap()
                .file
                .is_none()
        );
        drop(saved);
        let (config, _, backup) = initial_config(&directory);
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let host = process.prepare_cloud_connection(config.clone()).unwrap();
        assert!(host.collection().is_err() && !live.exists());
        assert_eq!(host.engine().slot_holder(), None);
        assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
        assert!(!directory.join(&op.staging_name).exists());
        let saved = store(&directory);
        assert!(saved.binding(op.binding_after).unwrap().is_none());
        assert!(saved.selected_binding().unwrap().is_none());
        drop(saved);
        let settled = device.publication_decision().unwrap();
        if prior_sidecar {
            assert_eq!(settled, prior);
        } else {
            assert_ne!(settled.as_ref().unwrap().publication.identity, op.candidate.identity);
        }
        assert_eq!(backup.0.load(Ordering::SeqCst), 0);
        host.close().unwrap();
        let reopened = process.prepare_cloud_connection(config).unwrap();
        assert_eq!(device.publication_decision().unwrap(), settled);
        assert_eq!(reopened.engine().slot_holder(), None);
        reopened.close().unwrap();
        pair.close();
    }
}
#[test]
fn initial_adoption_activation_crashes_relaunch_with_actual_proof_and_account_guards() {
    use meguri_engine::device_state::PublicationDecision;
    for point in ["replace.after_rename", "cloud.adopt.after_activation"] {
        for account in ["same", "other", "unknown", "readfailed"] {
            let mut pair = Pair::published();
            let expected = pair.sender.driver().collection_rows().unwrap();
            let directory = pair._root.path().join("initial");
            let op = crash_initial_adoption(&mut pair, &directory, point);
            let live = directory.join("collection.anki2");
            let native = NativeFiles.identity(&live).unwrap();
            let (config, identity, backup) = initial_config(&directory);
            *identity.0.lock().unwrap() = match account {
                "same" => cloud::runtime::IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())),
                "other" => cloud::runtime::IdentityStatus::Available(cloud::OpaqueBytes(b"other-account".to_vec())),
                "unknown" => cloud::runtime::IdentityStatus::TemporarilyUnavailable,
                "readfailed" => cloud::runtime::IdentityStatus::ReadFailed,
                _ => unreachable!(),
            };
            let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
            let host = process.open(config.clone()).unwrap();
            assert_eq!(initial_rows(&host), expected);
            assert_eq!(host.engine().slot_holder(), None);
            let proof = host.collection().unwrap().cloud_adopt_activation().unwrap().unwrap();
            assert!(proof.matches_initial(
                meguri_engine::device_state::PublicationId::from_bytes(op.id),
                &op.candidate
            ));
            assert_eq!(proof.native_file(), (native.device, native.inode));
            assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().is_none());
            if account != "same" {
                assert!(!host.cloud_runtime().unwrap().status().unwrap().identity_verified);
                assert!(host.cloud_runtime().unwrap().require_collection_sending().is_err());
            }
            let device = DeviceState::for_collection(&live);
            let decision = device.publication_decision().unwrap().unwrap();
            assert_eq!(decision.publication.identity, op.candidate.identity);
            assert_eq!(decision.publication.binding, op.candidate.binding);
            let preserves_finished = point == "cloud.adopt.after_activation" && account == "readfailed";
            assert_eq!(
                decision.publication.decision,
                if account == "same" || preserves_finished {
                    PublicationDecision::Clear
                } else {
                    PublicationDecision::Pending
                }
            );
            assert!(decision.pending_reason.is_none());
            host.close().unwrap();
            let saved = store(&directory);
            assert_eq!(
                saved.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
                Some(native)
            );
            assert!(saved.adoption_op().unwrap().is_none());
            drop(saved);
            // A failed read never clears Pending, including on the next open.
            *identity.0.lock().unwrap() = cloud::runtime::IdentityStatus::ReadFailed;
            let reopened = process.open(config).unwrap();
            assert_eq!(device.publication_decision().unwrap(), Some(decision));
            assert_eq!(NativeFiles.identity(&live).unwrap(), native);
            assert_eq!(reopened.engine().slot_holder(), None);
            reopened.close().unwrap();
            assert_eq!(backup.0.load(Ordering::SeqCst), 0);
            pair.close();
        }
    }
}
#[test]
fn changed_inode_blocks_initial_cloud_recovery_without_retiring_the_durable_row() {
    let mut pair = Pair::published();
    let directory = pair._root.path().join("initial");
    let op = crash_initial_adoption(&mut pair, &directory, "cloud.adopt.after_activation");
    // Let the original file settle its SQLite journals through a real local
    // Host, then replace only its native identity with an independent copy.
    let local = meguri_host::CoreHost::open(cloud_disabled_config(&directory, &op)).unwrap();
    local.close().unwrap();
    let live = directory.join("collection.anki2");
    let native = NativeFiles.identity(&live).unwrap();
    let runtime = store(&directory)
        .stored_runtime_state(op.binding_after)
        .unwrap()
        .unwrap();
    assert_eq!(runtime.file, Some(native));
    let replacement = replace_native_candidate(&directory, &op, &runtime);
    assert_eq!(NativeFiles.identity(&live).unwrap(), replacement);
    assert_ne!(NativeFiles.identity(&live).unwrap(), native);
    let (config, _, backup) = initial_config(&directory);
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.open(config).unwrap();
    assert!(host.collection().unwrap().cloud_adopt_activation().unwrap().is_none());
    assert_eq!(host.cloud_runtime().unwrap().adoption_op().unwrap().as_ref(), Some(&op));
    assert_eq!(host.engine().slot_holder(), None);
    assert!(matches!(
        host.next_cloud_publication_work().unwrap(),
        cloud::publication::PublicationWork::Idle
    ));
    host.close().unwrap();
    let saved = store(&directory);
    assert_eq!(
        saved.binding(op.binding_after).unwrap().unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::FileMismatch)
    );
    assert_eq!(saved.adoption_op().unwrap().as_ref(), Some(&op));
    assert_eq!(
        saved.stored_runtime_state(op.binding_after).unwrap().unwrap().file,
        Some(native)
    );
    assert_eq!(backup.0.load(Ordering::SeqCst), 0);
    pair.close();
}
#[test]
fn initial_adoption_malformed_sidecar_is_an_error_and_is_preserved() {
    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("initial");
    let (config, _, backup) = initial_config(&directory);
    let sidecar = directory.join("meguri-sidecar.json");
    let malformed = b"{unreadable";
    std::fs::write(&sidecar, malformed).unwrap();
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    assert!(process.prepare_cloud_connection(config).is_err());
    assert_eq!(std::fs::read(&sidecar).unwrap(), malformed);
    assert!(!directory.join("collection.anki2").exists());
    assert_eq!(process.engine().slot_holder(), None);
    assert_eq!(backup.0.load(Ordering::SeqCst), 0);
}
