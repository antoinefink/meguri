//! Focused production-phase tests; the enabled cloud_publication contract is
//! unchanged and still requires the child coordinator and incoming/media paths.
use meguri_host::CoreHost;
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_sync::cloud::{
    self as cloud,
    control_codec::{DecodeOutcome, Epoch, EpochState},
    publication::*,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus},
    workflow::Saved,
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, Mutation, MutationSuccess, Operation, RecordInput, Reply, RequestedFields, Scope,
        Service, SystemFields,
        coordinator::{execute_publication_request, publication_control_record},
        devices::{NativeFiles, options},
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
const NOW: i64 = 1_735_689_600_000;
struct Account;
struct MutableAccount(Mutex<IdentityStatus>);
impl IdentityProvider for MutableAccount {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
impl IdentityProvider for Account {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        IdentityStatus::Available(cloud::OpaqueBytes(b"synthetic-publication-account".to_vec()))
    }
}
// Deliberately no Debug: a secure provider need not expose its state to diagnostics.
struct Keys {
    calls: Mutex<usize>,
    fail: Option<usize>,
}
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let mut calls = self.calls.lock().unwrap();
        let draw = *calls;
        *calls += 1;
        if self.fail == Some(draw) {
            return Err(KeySourceError::Unavailable);
        }
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(13).wrapping_add(17 + draw as u8)
        }))
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.publication-phase".into(),
        environment: "development".into(),
        account: "synthetic-publication-account".into(),
    }
}
fn open_host(dir: &Path, keys: Arc<Keys>) -> CoreHost {
    open_host_with(dir, keys, Arc::new(Account), scope())
}
fn open_host_with(dir: &Path, keys: Arc<Keys>, identity: Arc<dyn IdentityProvider>, scope: Scope) -> CoreHost {
    let mut config = options(dir, NOW, 91);
    config.clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    config.cloud = Some(CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope.container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity,
        files: Arc::new(NativeFiles),
    });
    config.cloud_key_bytes = Some(keys);
    CoreHost::open(config).unwrap()
}
fn generated(dir: &Path) {
    let recipe = Recipe::parse("recipe: publication-phase\ndescription: Public synthetic rows\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Publication\n    notetype: basic\n    new: 1\n    due: 1\n    tags: [generic]\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 91, dir).unwrap();
}
fn preview(host: &CoreHost, service: &mut Service<SeededRandom>) -> Preview {
    let runtime = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = runtime
        .begin_connection_preview(&host.collection().unwrap(), &host.media().unwrap())
        .unwrap()
    else {
        panic!("read required");
    };
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let read = match response.result {
        Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
        Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
        other => panic!("unexpected actual read: {}", other.is_ok()),
    };
    let PreviewWork::Ready(p) = runtime.connection_active_read(id, read).unwrap() else {
        panic!("empty cloud expected");
    };
    *p
}
fn own_until_scan(host: &CoreHost, service: &mut Service<SeededRandom>) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    for _ in 0..16 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => {
                let (result, _) =
                    execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Scan => return,
            other => panic!("publication stopped before its scan: {other:?}"),
        }
    }
    panic!("publication never requested its scan");
}
#[test]
fn secure_key_failure_has_no_confirmation_or_binding_and_preview_draws_nothing() {
    for failed in [0, 1] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let keys = Arc::new(Keys {
            calls: Mutex::new(0),
            fail: Some(failed),
        });
        let host = open_host(&dir, keys.clone());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let before = meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
            .publication_decision()
            .unwrap();
        let p = preview(&host, &mut service);
        assert_eq!(*keys.calls.lock().unwrap(), 0);
        let runtime = host.cloud_runtime().unwrap();
        assert!(
            runtime
                .confirm_connection_publication(&p, &host.collection().unwrap())
                .is_err()
        );
        assert_eq!(*keys.calls.lock().unwrap(), failed + 1);
        assert!(runtime.publication_op().unwrap().is_none());
        assert!(runtime.status().unwrap().binding.is_none());
        assert_eq!(
            meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
                .publication_decision()
                .unwrap(),
            before
        );
        assert!(service.inspect(&scope()).unwrap().is_none_or(|db| db.zones.is_empty()));
        host.close().unwrap();
    }
}
#[test]
fn initial_rows_are_unacknowledged_until_actual_sdk_saves_and_own_switch_is_last() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = Arc::new(Keys {
        calls: Mutex::new(0),
        fail: None,
    });
    let host = open_host(&dir, keys.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let confirmed = runtime.publication_op().unwrap().unwrap();
    assert_eq!(confirmed.step, PublishStep::Confirmed);
    assert!(confirmed.record_key.0 == std::array::from_fn(|i| (i as u8).wrapping_mul(13).wrapping_add(17)));
    assert!(confirmed.media_key.0 == std::array::from_fn(|i| (i as u8).wrapping_mul(13).wrapping_add(18)));
    assert!(runtime.status().unwrap().binding.is_none());
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let mut entities = BTreeMap::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() {
                break;
            }
            for entity in page {
                after = Some(entity.key.clone());
                assert_eq!(entity.acked_rev, 0);
                assert_eq!(entity.server_erev, 0);
                assert!(entity.acked_payload.is_none() && entity.acked_system_fields.is_empty());
                assert!(entity.mark.is_some());
                entities.insert(entity.name.clone(), entity);
            }
        }
    }
    assert!(entities.values().any(|e| e.kind == cloud::entity_codec::Kind::Revlog));
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    for _ in 0..32 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::SendEntities => {
                let names: BTreeSet<_> = entities.keys().cloned().collect();
                let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
                assert!(!batch.saves.is_empty());
                let changes = batch
                    .saves
                    .iter()
                    .map(|s| Mutation::Save {
                        record: RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone()),
                        if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
                    })
                    .collect();
                let response = service
                    .request(
                        &scope(),
                        Caller {
                            instance: Some(instance.clone()),
                            ..Caller::default()
                        },
                        Operation::Modify {
                            zone: cloud::adapter::collection_zone(binding.facts.epoch),
                            atomic: false,
                            changes,
                        },
                    )
                    .unwrap();
                let Ok(Reply::Modified(items)) = response.result else {
                    panic!("actual saves expected");
                };
                let mut saved = Vec::new();
                for item in items {
                    let Ok(MutationSuccess::Saved(record)) = item.result else {
                        panic!("successful save expected");
                    };
                    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                        panic!("encrypted entity");
                    };
                    saved.push(Saved {
                        name: record.name.clone(),
                        envelope: envelope.clone(),
                        system_fields: record.system_fields.bytes().to_vec(),
                    });
                }
                runtime.enter_sent_handler(&instance, response.request_id).unwrap();
                runtime
                    .sent_results(&instance, response.request_id, &saved, &[], saved.len())
                    .unwrap();
                runtime.return_sent_handler(&instance).unwrap();
            }
            PublicationWork::Own(request) => {
                if request.kind == OwnKind::SaveActive {
                    assert!(
                        entities
                            .keys()
                            .all(|n| runtime.entity_facts(n).unwrap().unwrap().mark.is_none())
                    );
                }
                let (result, _) =
                    execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Idle if runtime.publication_op().unwrap().is_none() => break,
            other => panic!("unexpected stop: {other:?}"),
        }
    }
    assert!(runtime.publication_op().unwrap().is_none());
    let db = service.inspect(&scope()).unwrap().unwrap();
    let epoch = &db.zones[&cloud::adapter::collection_zone(binding.facts.epoch)].records["epoch"];
    let Field::EncryptedBytes(bytes) = &epoch.fields["e"] else {
        panic!("encrypted epoch");
    };
    let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap() else {
        panic!("supported epoch");
    };
    assert_eq!(epoch.state, EpochState::Complete);
    assert_eq!(epoch.counts.len(), 9);
    assert_eq!(*keys.calls.lock().unwrap(), 2);
    let media_authority = runtime.publication_media_state().unwrap().unwrap();
    assert!(media_authority.0.0 == confirmed.media_key.0 && media_authority.1);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys.clone());
    assert_eq!(
        reopened.cloud_runtime().unwrap().status().unwrap().binding.unwrap(),
        binding
    );
    assert_eq!(*keys.calls.lock().unwrap(), 2);
    assert!(
        reopened
            .cloud_runtime()
            .unwrap()
            .publication_media_state()
            .unwrap()
            .unwrap()
            == media_authority
    );
    reopened.close().unwrap();
}

fn keys() -> Arc<Keys> {
    Arc::new(Keys {
        calls: Mutex::new(0),
        fail: None,
    })
}
fn account_binding(account: &str) -> meguri_engine::device_state::AccountBinding {
    meguri_engine::device_state::AccountBinding::Connected {
        account: account.into(),
        configuration: scope().container,
        environment: scope().environment,
    }
}
fn decision(host: &CoreHost) -> DecisionSnapshot {
    meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
        .publication_decision()
        .unwrap()
        .unwrap()
}
fn candidate_stage(dir: &Path) -> meguri_engine::replace::ReplacementStage {
    let candidate = dir.join("candidate-source");
    generated(&candidate);
    let stage = meguri_engine::replace::ReplacementStage::new(dir).unwrap();
    std::fs::copy(candidate.join("collection.anki2"), stage.path()).unwrap();
    stage
}
fn next_own(host: &CoreHost) -> OwnRequest {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    for _ in 0..4 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => return *request,
            other => panic!("own request required: {other:?}"),
        }
    }
    panic!("no own request");
}

// Q11: the host must pass actual platform authority into the existing R6.6
// crash recovery, including unavailable accounts. No decision fixture overrides.
#[test]
fn unfinished_adoption_restart_uses_actual_host_account_and_real_staging_proofs() {
    use meguri_engine::device_state::{DeviceState, PublicationDecision};
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    for point in [
        "replace.after_rename",
        "replace.after_dir_sync",
        "replace.after_durable_activation",
    ] {
        for unavailable in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("device");
            generated(&dir);
            crash_worker(&dir, "adopt", point);
            let live = dir.join("collection.anki2");
            let document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dir.join("meguri-sidecar.json")).unwrap()).unwrap();
            let intent = &document["deviceState"]["intent"];
            assert!(!intent.is_null());
            let bytes = std::fs::read(&live).unwrap();
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            assert_eq!(
                intent["candidate_file"]["sha256"],
                serde_json::to_value(digest).unwrap()
            );
            assert_eq!(intent["candidate_file"]["length"].as_u64(), Some(bytes.len() as u64));
            let native = std::fs::symlink_metadata(&live).unwrap();
            assert_eq!(intent["candidate_native"]["device"].as_u64(), Some(native.dev()));
            assert_eq!(intent["candidate_native"]["inode"].as_u64(), Some(native.ino()));
            let candidate: meguri_engine::device_state::Publication =
                serde_json::from_value(intent["candidate"].clone()).unwrap();
            for _ in 0..2 {
                let identity = Arc::new(MutableAccount(Mutex::new(if unavailable {
                    IdentityStatus::ReadFailed
                } else {
                    IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()))
                })));
                let host = open_host_with(&dir, keys(), identity, scope());
                let state = decision(&host);
                assert_eq!(state.publication.identity, candidate.identity);
                assert_eq!(state.publication.binding, candidate.binding);
                assert_eq!(state.publication.decision, PublicationDecision::Pending);
                assert_eq!(state.pending_reason, None);
                assert!(
                    DeviceState::for_collection(&live)
                        .publication_token(&account_binding("changed-account"))
                        .is_err()
                );
                let recovered: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(dir.join("meguri-sidecar.json")).unwrap()).unwrap();
                assert!(recovered["deviceState"]["intent"].is_null());
                host.close().unwrap();
            }
        }
    }
}

#[test]
fn cloud_publication_crash_worker() {
    use meguri_model::faultpoint::{self, Fault};
    let Some(dir) = std::env::var_os("MEGURI_PUBLICATION_CRASH_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mode = std::env::var("MEGURI_PUBLICATION_CRASH_MODE").unwrap();
    let point = std::env::var("MEGURI_PUBLICATION_CRASH_POINT").unwrap();
    let host = open_host(&dir, keys());
    if mode == "adopt" {
        use meguri_engine::{
            LongOp,
            device_state::{Publication, PublicationDecision},
            replace::CloudAdoptInput,
        };
        let stage = candidate_stage(&dir);
        let input = CloudAdoptInput {
            operation: serde_json::from_value(serde_json::json!([6106, 1])).unwrap(),
            candidate: Publication {
                identity: serde_json::from_value(serde_json::json!([6106, 2])).unwrap(),
                binding: account_binding(&scope().account),
                decision: PublicationDecision::Clear,
            },
        };
        let slot = host.engine().acquire_slot(LongOp::CloudAdopt).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        faultpoint::arm(&point, Fault::Abort);
        exclusive.replace_verified_cloud_adopt(stage, input).unwrap();
    } else {
        assert!(mode == "decision" || mode == "cancel");
        if mode == "cancel" {
            restore_collection(&host, &dir);
        }
        let mut service = Service::open(dir.parent().unwrap(), Config::default(), SeededRandom::new(44)).unwrap();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let p = preview(&host, &mut service);
        let op = runtime.confirm_connection_publication(&p, &collection).unwrap();
        runtime
            .perform_publication_local(op, LocalWork::Bind, &collection)
            .unwrap();
        if mode == "decision" {
            faultpoint::arm(&point, Fault::Abort);
        }
        runtime
            .perform_publication_local(op, LocalWork::WriteDecision, &collection)
            .unwrap();
        if mode == "cancel" {
            faultpoint::arm(&point, Fault::Abort);
            runtime.cancel_publication(op, &collection).unwrap();
        }
    }
    panic!("actual crash boundary was not reached");
}
fn crash_worker(dir: &Path, mode: &str, point: &str) {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cloud_publication_crash_worker", "--nocapture"])
        .env("MEGURI_PUBLICATION_CRASH_DIR", dir)
        .env("MEGURI_PUBLICATION_CRASH_MODE", mode)
        .env("MEGURI_PUBLICATION_CRASH_POINT", point)
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(6));
}

#[test]
fn decision_write_kill_restart_reopens_durable_binding_and_preserves_new_restore_or_other_build_decisions() {
    use meguri_engine::{LongOp, device_state::PublicationDecision, replace::ReplaceReason};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    crash_worker(&dir, "decision", "cloud.publish.after_fence_clear");
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.step, PublishStep::Bound);
    assert_eq!(decision(&host).publication.decision, PublicationDecision::Clear);
    assert_eq!(decision(&host).publication.binding, account_binding(&scope().account));
    // Exercise the writer's actual future reopen binding as well as startup.
    assert!(matches!(
        host.engine().suspend(Duration::from_secs(30)).unwrap(),
        meguri_engine::suspend::SuspendOutcome::Suspended
    ));
    host.engine().resume().unwrap();
    let collection = host.collection().unwrap();
    let stage = candidate_stage(&dir);
    let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    drop(slot);
    let fresh = host.collection().unwrap();
    let restored = decision(&host);
    assert_eq!(restored.publication.decision, PublicationDecision::Pending);
    assert!(
        collection
            .resolve_cloud_publication_decision(op.id, &op.fence_before)
            .is_err()
    );
    assert!(fresh.resolve_cloud_publication_decision(op.id, &restored).is_err());
    assert_eq!(decision(&host), restored);
    host.close().unwrap();
    drop(host);
    let mut other_scope = scope();
    other_scope.container.push_str(".other");
    let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope);
    assert!(
        other
            .collection()
            .unwrap()
            .resolve_cloud_publication_decision(op.id, &restored)
            .is_err()
    );
    assert_eq!(decision(&other), restored);
    other.close().unwrap();
}

#[test]
fn no_durable_recovery_row_cannot_resolve_a_reader_supplied_decision() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    for cloud_enabled in [false, true] {
        let host = if cloud_enabled {
            open_host(&dir, keys())
        } else {
            CoreHost::open(options(&dir, NOW, 91)).unwrap()
        };
        let before = decision(&host);
        assert!(
            host.collection()
                .unwrap()
                .resolve_cloud_publication_decision([7; 16], &before)
                .is_err()
        );
        assert_eq!(decision(&host), before);
        host.close().unwrap();
    }
}

#[test]
fn held_publication_request_revalidates_account_file_decision_and_restart_before_service_submission() {
    use meguri_engine::{LongOp, replace::ReplaceReason};
    for change in ["account", "unavailable", "file", "decision", "restart"] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
            cloud::OpaqueBytes(scope().account.into_bytes()),
        ))));
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        runtime.confirm_connection_publication(&p, &collection).unwrap();
        let held = next_own(&host);
        let after = service.request_log().unwrap().len();
        match change {
            "account" => {
                *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()))
            }
            "unavailable" => *account.0.lock().unwrap() = IdentityStatus::ReadFailed,
            "file" => {
                // Rename the real open file out and back via a different native
                // file, preserving bytes; no file-provider fixture overrides.
                let live = collection.path();
                let copy = dir.join("held-file-copy");
                std::fs::copy(live, &copy).unwrap();
                std::fs::rename(live, dir.join("held-file-original")).unwrap();
                std::fs::rename(copy, live).unwrap();
            }
            "decision" => {
                let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
                slot.replace_verified(candidate_stage(&dir), ReplaceReason::Restore, |_| Ok(()))
                    .unwrap();
            }
            "restart" => {
                host.close().unwrap();
                let reopened = open_host(&dir, keys());
                assert!(
                    execute_publication_request(
                        &mut service,
                        &scope(),
                        &held,
                        reopened.cloud_runtime().unwrap(),
                        &reopened.collection().unwrap()
                    )
                    .is_err()
                );
                assert_eq!(service.request_log().unwrap().len(), after);
                reopened.close().unwrap();
                continue;
            }
            _ => unreachable!(),
        }
        assert!(execute_publication_request(&mut service, &scope(), &held, runtime, &collection).is_err());
        assert_eq!(
            service.request_log().unwrap().len(),
            after,
            "stale request must reach no Service operation"
        );
        if change == "file" {
            std::fs::remove_file(collection.path()).unwrap();
            std::fs::rename(dir.join("held-file-original"), collection.path()).unwrap();
        }
        host.close().unwrap();
    }
}

fn entity_names(runtime: &cloud::runtime::Runtime) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() {
                break;
            }
            for entity in page {
                after = Some(entity.key);
                names.insert(entity.name);
            }
        }
    }
    names
}
fn send_actual_batch(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    batch: &cloud::workflow::BuiltBatch,
) -> BTreeSet<String> {
    use cloud::workflow::SendError;
    use meguri_testkit::cloud::ErrorCode;
    let runtime = host.cloud_runtime().unwrap();
    let instance = runtime.instance().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let changes = batch
        .saves
        .iter()
        .map(|s| Mutation::Save {
            record: RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone()),
            if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
        })
        .collect();
    let response = service
        .request(
            &scope(),
            Caller {
                instance: Some(instance.clone()),
                ..Caller::default()
            },
            Operation::Modify {
                zone: cloud::adapter::collection_zone(binding.facts.epoch),
                atomic: false,
                changes,
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("per-record outcomes required");
    };
    let mut saved = Vec::new();
    let mut failed = Vec::new();
    let mut failed_names = BTreeSet::new();
    for item in items {
        match item.result {
            Ok(MutationSuccess::Saved(record)) => {
                let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
                    panic!("encrypted entity");
                };
                saved.push(Saved {
                    name: record.name.clone(),
                    envelope: envelope.clone(),
                    system_fields: record.system_fields.bytes().to_vec(),
                });
            }
            Err(error) => {
                assert_eq!(error.code, ErrorCode::LimitExceeded);
                failed_names.insert(item.name.clone());
                failed.push((item.name, SendError::LimitExceeded));
            }
            _ => panic!("entity save expected"),
        }
    }
    runtime.enter_sent_handler(&instance, response.request_id).unwrap();
    runtime
        .sent_results(&instance, response.request_id, &saved, &failed, batch.saves.len())
        .unwrap();
    runtime.return_sent_handler(&instance).unwrap();
    failed_names
}

#[test]
fn non_atomic_parent_failure_never_submits_children_before_persisted_parent_acknowledgement() {
    use cloud::entity_codec::{self as codec, Payload};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    // Fixed public key vector: draw 19 puts this Notetype before its new Note.
    let host = open_host(
        &dir,
        Arc::new(Keys {
            calls: Mutex::new(19),
            fail: None,
        }),
    );
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let names = entity_names(runtime);
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    let parent = batch
        .saves
        .iter()
        .filter(|s| runtime.entity_facts(&s.name).unwrap().unwrap().kind == codec::Kind::Notetype)
        .max_by_key(|s| s.envelope.len())
        .expect("initial parent in actual batch");
    let parent_fact = runtime.entity_facts(&parent.name).unwrap().unwrap();
    assert_eq!(parent_fact.server_erev, 0);
    let children: BTreeSet<_> = names
        .iter()
        .filter(|name| {
            matches!(runtime.entity_facts(name).unwrap().unwrap().payload,
        Payload::Note(ref note) if codec::id_key(note.mid).as_slice() == parent_fact.key.as_slice())
        })
        .cloned()
        .collect();
    assert!(!children.is_empty());
    assert!(
        children.iter().any(|name| name > &parent.name),
        "fixture must exercise the erroneous earlier-parent shortcut"
    );
    assert!(batch.saves.iter().all(|s| !children.contains(&s.name)));
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let parent_size = meguri_testkit::cloud::record_non_asset_bytes(
        &zone,
        &RecordInput::encrypted(&parent.name, "MGEntity", parent.envelope.clone()),
    );
    assert!(
        batch
            .saves
            .iter()
            .any(|s| meguri_testkit::cloud::record_non_asset_bytes(
                &zone,
                &RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone())
            ) < parent_size)
    );
    let mut limited = Config::default();
    limited.limits.non_asset_per_record = parent_size - 1;
    service
        .request(&scope(), Caller::default(), Operation::Configure(limited))
        .unwrap()
        .result
        .unwrap();
    let failed = send_actual_batch(&host, &mut service, &batch);
    assert!(failed.contains(&parent.name));
    assert!(
        failed.len() < batch.saves.len(),
        "actual non-atomic partial success required"
    );
    assert_eq!(runtime.entity_facts(&parent.name).unwrap().unwrap().server_erev, 0);
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert!(next.saves.iter().all(|s| !children.contains(&s.name)));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(children.iter().all(|name| !db.zones[&zone].records.contains_key(name)));
    service
        .request(&scope(), Caller::default(), Operation::Configure(Config::default()))
        .unwrap()
        .result
        .unwrap();
    assert!(send_actual_batch(&host, &mut service, &next).is_empty());
    assert!(runtime.entity_facts(&parent.name).unwrap().unwrap().server_erev > 0);
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert!(next.saves.iter().any(|s| children.contains(&s.name)));
    host.close().unwrap();
}

fn until_active(host: &CoreHost, service: &mut Service<SeededRandom>) -> OwnRequest {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let mut sdk = false;
    for _ in 0..32 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) if request.kind == OwnKind::SaveActive => return *request,
            PublicationWork::Own(request) => {
                let (result, _) =
                    execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Scan => {
                runtime
                    .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
                    .unwrap();
            }
            PublicationWork::SendEntities => {
                let instance = runtime.instance().unwrap();
                if !sdk {
                    runtime.start_sdk_instance(&instance).unwrap();
                    sdk = true;
                }
                let batch = runtime
                    .build_collection_batch(&instance, &collection, &entity_names(runtime))
                    .unwrap();
                assert!(!batch.saves.is_empty());
                assert!(send_actual_batch(host, service, &batch).is_empty());
            }
            other => panic!("publication stopped: {other:?}"),
        }
    }
    panic!("no active submission");
}
#[test]
fn held_active_is_not_sent_and_actual_lost_reply_retains_submission_evidence_on_restart() {
    for submit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
            cloud::OpaqueBytes(scope().account.into_bytes()),
        ))));
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        host.media().unwrap().store().register_full(|_, _| true).unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        runtime.confirm_connection_publication(&p, &collection).unwrap();
        let held = until_active(&host, &mut service);
        let before = runtime.publication_op().unwrap().unwrap();
        assert!(!before.switch_sent && before.active_attempt.is_none());
        let after = service.request_log().unwrap().len();
        if submit {
            let (result, _) = execute_publication_request(&mut service, &scope(), &held, runtime, &collection).unwrap();
            assert!(matches!(result, OwnResult::Saved(_)));
            // Deliberately lose the actual reply, never deliver a fake ack.
            assert!(
                runtime
                    .publication_result(&held, OwnResult::NotStarted, &collection)
                    .is_err()
            );
            let durable = runtime.publication_op().unwrap().unwrap();
            assert!(durable.switch_sent && durable.active_attempt.is_some());
        } else {
            *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()));
            assert!(execute_publication_request(&mut service, &scope(), &held, runtime, &collection).is_err());
            assert_eq!(service.request_log().unwrap().len(), after);
            assert_eq!(runtime.publication_op().unwrap().unwrap(), before);
        }
        host.close().unwrap();
        drop(host);
        let reopened = open_host(&dir, keys());
        let durable = reopened.cloud_runtime().unwrap().publication_op().unwrap().unwrap();
        assert_eq!(durable.switch_sent, submit);
        assert_eq!(durable.active_attempt.is_some(), submit);
        let db = service.inspect(&scope()).unwrap().unwrap();
        assert_eq!(db.zones["meguri-root"].records.contains_key("active"), submit);
        reopened.close().unwrap();
    }
}

fn restore_collection(host: &CoreHost, dir: &Path) {
    use meguri_engine::{LongOp, replace::ReplaceReason};
    let source = tempfile::tempdir_in(dir).unwrap();
    generated(source.path());
    let stage = meguri_engine::replace::ReplacementStage::new(dir).unwrap();
    std::fs::copy(source.path().join("collection.anki2"), stage.path()).unwrap();
    let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
}

fn actual_preview(host: &CoreHost, service: &mut Service<SeededRandom>, scope: &Scope) -> Preview {
    fn read(service: &mut Service<SeededRandom>, scope: &Scope, zone: String, name: &str) -> RecordRead {
        let response = service
            .request(
                scope,
                Caller::default(),
                Operation::Read {
                    zone,
                    name: name.into(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        match response.result {
            Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
            Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
            Err(error) if error.code == meguri_testkit::cloud::ErrorCode::UnknownItem => RecordRead::Missing,
            other => panic!("unexpected actual preview read: {}", other.is_ok()),
        }
    }
    let runtime = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = runtime
        .begin_connection_preview(&host.collection().unwrap(), &host.media().unwrap())
        .unwrap()
    else {
        panic!("active read required");
    };
    let work = runtime
        .connection_active_read(id, read(service, scope, "meguri-root".into(), "active"))
        .unwrap();
    match work {
        PreviewWork::Ready(p) => *p,
        PreviewWork::ReadEpoch { id, epoch } => {
            let work = runtime
                .connection_epoch_read(
                    id,
                    read(service, scope, cloud::adapter::collection_zone(epoch), "epoch"),
                )
                .unwrap();
            let PreviewWork::Ready(p) = work else {
                panic!("epoch preview required");
            };
            *p
        }
        _ => panic!("unexpected preview work"),
    }
}

fn publish_actual_collection(host: &CoreHost, service: &mut Service<SeededRandom>) -> cloud::Binding {
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let p = actual_preview(host, service, &scope());
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let request = until_active(host, service);
    let (result, _) = execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    runtime.status().unwrap().binding.unwrap()
}

fn save_other_writer_publication_note(
    service: &mut Service<SeededRandom>,
    old: &cloud::Binding,
    before: &cloud::workflow::EntityFacts,
    base: &cloud::entity_codec::Note,
) -> (
    cloud::entity_codec::Payload,
    String,
    meguri_testkit::cloud::Record,
    Saved,
) {
    use cloud::entity_codec::{self as codec, Payload};

    let mut remote = base.clone();
    let mut fields: Vec<_> = remote.flds.split(|b| *b == 31).map(Vec::from).collect();
    assert_eq!(fields.len(), 2);
    fields[1] = b"remote update committed before republication".to_vec();
    remote.flds = fields.join(&31);
    remote.modified += 1;
    let after = Payload::Note(remote);
    let writer = old.id.0.map(|b| b ^ 0x80);
    assert_ne!(writer, old.id.0);
    let envelope = codec::Entity::build(
        codec::EntityFields {
            kind: before.kind,
            key: before.key.clone(),
            origin: before.origin,
            erev: before.server_erev + 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer,
            base_sha256: Some(before.acked_payload_digest),
        },
        &after.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(
        codec::entity_name(&old.facts.record_key.0, before.kind, &before.key).unwrap(),
        before.name
    );
    let old_zone = cloud::adapter::collection_zone(old.facts.epoch);
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: old_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&before.name, "MGEntity", envelope.clone()),
                    if_unchanged: Some(SystemFields::from_bytes(before.acked_system_fields.clone())),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(mut items)) = response.result else {
        panic!("actual remote save required");
    };
    assert_eq!(items.len(), 1);
    let Ok(MutationSuccess::Saved(record)) = items.remove(0).result else {
        panic!("remote save must use the acknowledged change tag");
    };
    assert!(matches!(record.fields.get("e"), Some(Field::EncryptedBytes(bytes)) if bytes == &envelope));
    let saved = Saved {
        name: record.name.clone(),
        envelope,
        system_fields: record.system_fields.bytes().to_vec(),
    };
    (after, old_zone, record, saved)
}

// V17.6 / V2.4 step 8, K13.2 and K12.1 rule 8: a committed ordinary
// apply must settle before publication scans or discards its prior binding.
#[test]
fn stopped_binding_committed_incoming_settles_before_republication_and_survives_cleanup() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

    fn note_row(host: &CoreHost, id: i64, notetype: &codec::Notetype) -> Payload {
        let collection = host.collection().unwrap();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let row = snapshot.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        cloud::row_payload::project(
            &cloud::row_payload::ReadView(&*snapshot),
            codec::Kind::Note,
            &row,
            |mid| {
                assert_eq!(mid, notetype.id);
                Ok(notetype.clone())
            },
        )
        .unwrap()
    }

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("actual publication must acknowledge a note");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());

    // A different writer builds on the actually acknowledged payload and origin,
    // then saves conditionally to the real Service. This is not a seeded ACK.
    let (after, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    let inbox = runtime.inbox_entries(old.id).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, cloud::InboxReason::Ready);
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());

    // A returned error immediately after COMMIT leaves the real intent/result
    // seq and inbox intact. Disarm before any assertion or subsequent work.
    faultpoint::arm("cloud.apply.after_commit", Fault::IoError(std::io::ErrorKind::Other));
    let applied = runtime.drain_incoming(&collection, Duration::from_secs(30));
    faultpoint::disarm("cloud.apply.after_commit");
    assert!(
        matches!(
            applied,
            Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::Io(
                std::io::ErrorKind::Other
            )))
        ),
        "the post-commit I/O fault must fire"
    );
    assert_eq!(note_row(&host, base.id, &notetype), after);
    let pending = runtime.apply_intents(old.id).unwrap();
    assert_eq!(pending.len(), 1);
    let intent = &pending[0];
    assert_eq!(intent.binding, old.id);
    assert_eq!(intent.inbox_ids, vec![inbox[0].id]);
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, before.name);
    assert_eq!(intent.keys[0].role, cloud::incoming::ApplyKeyRole::Entry);
    assert_eq!(intent.keys[0].before_digest, Some(before.acked_row_digest));
    assert_eq!(intent.keys[0].after_digest, Some(after.row_digest().unwrap()));
    assert_ne!(intent.keys[0].before_digest, intent.keys[0].after_digest);
    let runs = runtime.incoming_runs();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.intent_id, intent.id);
    assert_eq!(intent.result_seq, Some(run.seq));
    assert_eq!(intent.native_file, run.native_file);
    assert_eq!(intent.generation, run.generation);
    assert_eq!(run.entry_ids, intent.inbox_ids);
    assert_eq!(run.changed_keys, BTreeSet::from([before.name.clone()]));
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);

    runtime.adapter_stop_syncing().unwrap();
    let stopped = runtime.status().unwrap().binding.unwrap();
    assert_eq!(stopped.id, old.id);
    assert_eq!(
        stopped.facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::Stopped)
    );
    assert_eq!(runtime.apply_intents(old.id).unwrap(), pending);
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.native_file, intent.native_file);
    let calls = service.request_log().unwrap().len();
    let op_id = runtime.confirm_connection_publication(&p, &collection).unwrap();

    assert_eq!(runtime.status().unwrap().binding.unwrap(), stopped);
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.step, PublishStep::Confirmed);
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(op.binding_before.as_ref().unwrap().state, stopped.facts.state);
    assert!(op.binding_after.is_none());
    assert_eq!(service.request_log().unwrap().len(), calls);
    let after_confirm_intents = runtime.apply_intents(old.id).unwrap();
    if !after_confirm_intents.is_empty() {
        assert_eq!(after_confirm_intents, pending);
    }
    let after_confirm_inbox = runtime.inbox_entries(old.id).unwrap();
    if !after_confirm_inbox.is_empty() {
        assert_eq!(after_confirm_inbox, inbox);
    }

    // Run real binding, decision and control work, then the new binding's scan.
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();

    assert!(
        runtime.apply_intents(old.id).unwrap().is_empty(),
        "pending old intent not settled before the new publication scan"
    );
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.incoming_wal_receipts().iter().any(|receipt| {
        receipt.identity().native_file() == (run.native_file.device, run.native_file.inode)
            && receipt.identity().generation().0 == run.generation
            && receipt.through().0 >= run.seq
    }));
    let settled = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert_eq!(settled.server_erev, before.server_erev + 1);
    assert_eq!(settled.acked_payload, Some(after.clone()));
    assert_eq!(settled.acked_row_digest, after.row_digest().unwrap());
    assert_eq!(settled.acked_payload_digest, after.payload_digest().unwrap());
    assert_eq!(settled.acked_system_fields, saved.system_fields);
    assert_eq!(settled.acked_envelope.0, saved.envelope);
    assert!(settled.mark.is_none() && settled.base_payload.is_none());
    assert_eq!(note_row(&host, base.id, &notetype), after);
    assert_eq!(runtime.incoming_runs(), runs, "unexpected apply count");

    // Continue with actual SDK batch saves and own-operation acknowledgements.
    // With no conflict copies the prior binding must be deletable without an FK
    // error, and no send may escape to its isolated epoch.
    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert!(
        runtime
            .publication_facts(&collection)
            .unwrap()
            .bindings
            .iter()
            .all(|b| b.id != old.id)
    );
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(after.clone()));
    assert!(new_note.mark.is_none() && new_note.server_erev > 0);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(db.zones[&old_zone].records[&before.name] == record);
    assert_eq!(note_row(&host, base.id, &notetype), after);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .iter()
            .all(|b| b.id != old.id)
    );
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(after.clone()));
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert_eq!(note_row(&reopened, base.id, &notetype), after);
    reopened.close().unwrap();
}

// V15.2 step 1 excludes fetched, unapplied changes; V17.6 settles existing intents.
#[test]
fn queued_ready_incoming_is_not_applied_by_republication_and_is_discarded_only_at_completion() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

    fn note_row(host: &CoreHost, id: i64, notetype: &codec::Notetype) -> Payload {
        let collection = host.collection().unwrap();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let row = snapshot.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        cloud::row_payload::project(
            &cloud::row_payload::ReadView(&*snapshot),
            codec::Kind::Note,
            &row,
            |mid| {
                assert_eq!(mid, notetype.id);
                Ok(notetype.clone())
            },
        )
        .unwrap()
    }

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("acknowledged note required");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.facts.state, cloud::BindingState::Active);
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert!(!before.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());

    let (after, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    assert_ne!(after, Payload::Note(base.clone()));
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    let inbox = runtime.inbox_entries(old.id).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, cloud::InboxReason::Ready);
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);

    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.active_epoch, Some(old.facts.epoch));
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert_eq!(
        runtime
            .inbox_entries(old.id)
            .unwrap()
            .iter()
            .filter(|entry| entry.entry.reason == cloud::InboxReason::Ready)
            .count(),
        1
    );
    let calls = service.request_log().unwrap().len();
    let op_id = runtime.confirm_connection_publication(&p, &collection).unwrap();
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.step, PublishStep::Confirmed);
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(service.request_log().unwrap().len(), calls);

    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    assert!(runtime.publication_op().unwrap().is_some());
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert!(
        runtime
            .publication_facts(&collection)
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.id != old.id)
    );
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().is_none());
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|entity| entity.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(Payload::Note(base.clone())));
    assert!(new_note.mark.is_none() && new_note.acked_rev > 0 && new_note.server_erev > 0);
    assert!(!new_note.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(db.zones[&old_zone].records[&before.name] == record);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.id != old.id)
    );
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(Payload::Note(base.clone())));
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert_eq!(note_row(&reopened, base.id, &notetype), Payload::Note(base));
    reopened.close().unwrap();
}

fn bind_and_write(host: &CoreHost, service: &mut Service<SeededRandom>, scope: &Scope) -> PublishOp {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let p = actual_preview(host, service, scope);
    let id = runtime.confirm_connection_publication(&p, &collection).unwrap();
    runtime
        .perform_publication_local(id, LocalWork::Bind, &collection)
        .unwrap();
    runtime
        .perform_publication_local(id, LocalWork::WriteDecision, &collection)
        .unwrap();
    runtime.publication_op().unwrap().unwrap()
}

fn cancel_with_account_status(status: IdentityStatus) {
    use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
        cloud::OpaqueBytes(scope().account.into_bytes()),
    ))));
    let host = open_host_with(&dir, keys(), account.clone(), scope());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    restore_collection(&host, &dir);
    let before = decision(&host);
    assert_eq!(before.publication.decision, PublicationDecision::Pending);
    assert_eq!(before.pending_reason, Some(PendingDecisionReason::Restore));
    let op = bind_and_write(&host, &mut service, &scope());
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(
        op.binding_before.as_ref().unwrap().state,
        cloud::BindingState::Isolated(cloud::IsolationReason::ReplacedLocally)
    );
    assert_ne!(decision(&host), before);
    *account.0.lock().unwrap() = status;
    let runtime = host.cloud_runtime().unwrap();
    runtime.refresh_identity().unwrap();
    let calls = service.request_log().unwrap().len();
    runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
    assert_eq!(decision(&host), before);
    assert!(runtime.publication_op().unwrap().is_none());
    let returned = runtime.status().unwrap().binding.unwrap();
    assert_eq!(returned.id, old.id);
    assert_eq!(
        returned.facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::ReplacedLocally)
    );
    assert!(runtime.start_sdk_instance(&runtime.instance().unwrap()).is_err());
    assert_eq!(service.request_log().unwrap().len(), calls, "Cancel needs no network");
    host.close().unwrap();
}

#[test]
fn cancellation_restores_exact_prior_decision_when_account_is_unavailable_or_signed_out() {
    for status in [
        IdentityStatus::ReadFailed,
        IdentityStatus::TemporarilyUnavailable,
        IdentityStatus::SignedOut,
    ] {
        cancel_with_account_status(status);
    }
}

#[test]
fn cancellation_restores_exact_prior_decision_after_account_changes() {
    cancel_with_account_status(IdentityStatus::Available(cloud::OpaqueBytes(
        b"changed-account".to_vec(),
    )));
}

#[test]
fn cancellation_after_native_file_mismatch_restores_fence_and_keeps_old_binding_isolated() {
    use meguri_engine::{EngineError, LongOp};
    for restore in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let old = publish_actual_collection(&host, &mut service);
        if restore {
            restore_collection(&host, &dir);
        }
        let before = decision(&host);
        let op = bind_and_write(&host, &mut service, &scope());
        let written = decision(&host);
        let native_before = host.collection().unwrap().open_evidence().unwrap().native_file;
        assert!(host.review().active_session().is_none());
        {
            let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            // ReplaceTask closes/checkpoints the writer and retires its journal
            // before this closure copies the MAIN file to a different inode.
            exclusive
                .replace(|live| {
                    let copy = live.with_file_name("replacement-native-file");
                    std::fs::copy(live, &copy).map_err(|e| EngineError::Storage(e.into()))?;
                    std::fs::rename(copy, live).map_err(|e| EngineError::Storage(e.into()))
                })
                .unwrap();
        }
        assert_eq!(decision(&host), written);
        let native_after = host.collection().unwrap().open_evidence().unwrap().native_file;
        assert_ne!(native_after, native_before);
        assert!(host.review().active_session().is_none());
        host.close().unwrap();
        drop(host);
        let host = open_host(&dir, keys());
        assert_eq!(decision(&host), written);
        assert_eq!(
            host.collection().unwrap().open_evidence().unwrap().native_file,
            native_after
        );
        let runtime = host.cloud_runtime().unwrap();
        assert_eq!(Some(runtime.status().unwrap().binding.unwrap().id), op.binding_after);
        assert_eq!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::FileMismatch)
        );
        let calls = service.request_log().unwrap().len();
        runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), before);
        assert!(runtime.publication_op().unwrap().is_none());
        let returned = runtime.status().unwrap().binding.unwrap();
        assert_eq!(returned.id, old.id);
        assert_eq!(
            returned.facts.state,
            cloud::BindingState::Isolated(if restore {
                cloud::IsolationReason::ReplacedLocally
            } else {
                cloud::IsolationReason::FileMismatch
            })
        );
        assert!(runtime.start_sdk_instance(&runtime.instance().unwrap()).is_err());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
    }
}

#[test]
fn cancellation_leaves_intervening_restore_or_other_build_decision_and_row_unchanged() {
    for other_build in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        restore_collection(&host, &dir);
        let op = bind_and_write(&host, &mut service, &scope());
        let mut other_scope = scope();
        other_scope.container.push_str(".other");
        let other_op = if other_build {
            host.close().unwrap();
            let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope.clone());
            let row = bind_and_write(&other, &mut service, &other_scope);
            other.close().unwrap();
            Some(row)
        } else {
            restore_collection(&host, &dir);
            host.close().unwrap();
            None
        };
        drop(host);
        let host = open_host(&dir, keys());
        let intervening = decision(&host);
        let calls = service.request_log().unwrap().len();
        let runtime = host.cloud_runtime().unwrap();
        runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), intervening);
        assert!(runtime.publication_op().unwrap().is_none());
        assert!(runtime.status().unwrap().binding.is_none());
        assert!(runtime.instance().is_err());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
        drop(host);
        if let Some(row) = other_op {
            let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope);
            assert_eq!(other.cloud_runtime().unwrap().publication_op().unwrap(), Some(row));
            assert_eq!(decision(&other), intervening);
            other.close().unwrap();
        }
    }
}

#[test]
fn decision_cas_io_error_keeps_inprocess_reopen_binding_consistent_for_publish_and_cancel() {
    use meguri_model::faultpoint::{self, Fault};
    for cancel in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        restore_collection(&host, &dir);
        let before = decision(&host);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        let id = runtime.confirm_connection_publication(&p, &collection).unwrap();
        runtime
            .perform_publication_local(id, LocalWork::Bind, &collection)
            .unwrap();
        if cancel {
            runtime
                .perform_publication_local(id, LocalWork::WriteDecision, &collection)
                .unwrap();
        }
        let point = if cancel {
            "cloud.publish.after_cancel_fence"
        } else {
            "cloud.publish.after_fence_clear"
        };
        // Focused returned-error probe, distinct from F3's conformance modes:
        // the operation fails after CAS while this engine stays alive.
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        let result = if cancel {
            runtime.cancel_publication(id, &collection)
        } else {
            runtime.perform_publication_local(id, LocalWork::WriteDecision, &collection)
        };
        faultpoint::disarm(point);
        assert!(result.is_err(), "the post-CAS operation fault must fire");
        let written = decision(&host);
        if cancel {
            assert_eq!(written, before);
        } else {
            assert_eq!(written.publication.binding, account_binding(&scope().account));
        }
        assert!(matches!(
            host.engine().suspend(Duration::from_secs(30)).unwrap(),
            meguri_engine::suspend::SuspendOutcome::Suspended
        ));
        host.engine().resume().unwrap();
        assert_eq!(decision(&host), written);
        let calls = service.request_log().unwrap().len();
        runtime.cancel_publication(id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), before);
        assert!(runtime.publication_op().unwrap().is_none());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
    }
}

#[test]
fn cancel_fence_kill_restart_finishes_cancel_without_account_and_preserves_prior_restore_reason() {
    use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    crash_worker(&dir, "cancel", "cloud.publish.after_cancel_fence");
    let host = open_host_with(
        &dir,
        keys(),
        Arc::new(MutableAccount(Mutex::new(IdentityStatus::ReadFailed))),
        scope(),
    );
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.step, PublishStep::Cancelling);
    assert_eq!(decision(&host), op.fence_before);
    assert_eq!(decision(&host).publication.decision, PublicationDecision::Pending);
    assert_eq!(decision(&host).pending_reason, Some(PendingDecisionReason::Restore));
    assert!(matches!(runtime.next_publication_work(&collection).unwrap(),
        PublicationWork::Local { op: id, work: LocalWork::Cancel } if id == op.id));
    runtime
        .perform_publication_local(op.id, LocalWork::Cancel, &collection)
        .unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(decision(&host), op.fence_before);
    host.close().unwrap();
}
