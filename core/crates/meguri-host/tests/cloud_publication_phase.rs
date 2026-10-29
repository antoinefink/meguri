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
                    .sent_results(&instance, &collection, response.request_id, &saved, &[], saved.len())
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
        .sent_results(
            &instance,
            &host.collection().unwrap(),
            response.request_id,
            &saved,
            &failed,
            batch.saves.len(),
        )
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
    use std::sync::mpsc::{self, Receiver, Sender};

    static AFTER_COMMIT: Mutex<Option<(Sender<()>, Receiver<()>)>> = Mutex::new(None);

    fn pause_after_commit() {
        let handshake = AFTER_COMMIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some((reached, release)) = handshake {
            if reached.send(()).is_ok() {
                release
                    .recv_timeout(Duration::from_secs(30))
                    .expect("parent must release the actual post-commit worker");
            }
        }
    }

    struct AfterCommitPause(Sender<()>);
    impl Drop for AfterCommitPause {
        fn drop(&mut self) {
            faultpoint::disarm("cloud.apply.after_commit");
            AFTER_COMMIT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let _ = self.0.send(());
        }
    }

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

    // Observe the actual COMMIT outside writer/state/tracking locks, then stop
    // the binding before this worker can settle its real intent/result seq.
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *AFTER_COMMIT.lock().unwrap() = Some((reached_tx, release_rx));
    let (reached, stopped, applied) = std::thread::scope(|scope| {
        // This local drops before scope joins, including on parent unwind.
        let pause = AfterCommitPause(release_tx);
        faultpoint::arm("cloud.apply.after_commit", Fault::Callback(pause_after_commit));
        let worker_runtime = Arc::clone(runtime);
        let worker_collection = collection.clone();
        let worker = scope.spawn(move || worker_runtime.drain_incoming(&worker_collection, Duration::from_secs(30)));
        let reached = reached_rx.recv_timeout(Duration::from_secs(30));
        let stopped = reached.as_ref().ok().map(|()| runtime.adapter_stop_syncing());
        drop(pause);
        (reached, stopped, worker.join())
    });
    // The callback is disarmed and released before any assertion.
    reached.expect("the actual drain worker must reach after_commit");
    stopped
        .expect("stop must follow the actual after_commit callback")
        .unwrap();
    let applied = applied.expect("the actual drain worker must not panic");
    assert!(
        matches!(applied, Err(cloud::runtime::RuntimeError::StaleIncarnation)),
        "stopping the binding must invalidate the actual post-commit worker"
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
    assert!(matches!(
        runtime.inbox_entries(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.apply_intents(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.binding_entity(old.id, &before.name),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
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
    assert!(matches!(
        runtime.inbox_entries(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.apply_intents(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
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

// V17.6 / V15.2 and K13.2 / K15.2 / K16: retain a real conflict's owner,
// discard its work, and preserve both the chosen row and the losing copy.
#[test]
fn actual_field_conflict_retains_isolated_owner_once_after_republication_and_clean_reopen() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_model::ids::NoteId;
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

    // Call only after closing and dropping this test folder's sole CoreHost
    // and all collection handles. Keep open, reads and drop inside the gate;
    // no Runtime call or tracking mutation belongs in this closure.
    fn closed_store<T>(dir: &Path, read: impl FnOnce(&cloud::TrackingStore) -> T) -> T {
        cloud::runtime::with_tracking_access(|| {
            let scope = cloud::BuildScope {
                container: cloud::ContainerId::new(scope().container).unwrap(),
                environment: cloud::Environment::Development,
            };
            let cloud::OpenOutcome::Opened(store) = cloud::TrackingStore::open(dir, scope).unwrap() else {
                panic!("the actual host's tracking store must exist");
            };
            read(&store)
        })
    }

    fn retained_copies_without_work(dir: &Path, owner: &cloud::Binding) -> Vec<cloud::StoredConflict> {
        closed_store(dir, |store| {
            assert_eq!(store.binding(owner.id).unwrap().as_ref(), Some(owner));
            assert!(store.engine_state(owner.id).unwrap().is_none());
            assert!(store.stored_runtime_state(owner.id).unwrap().is_none());
            assert!(store.inbox(owner.id).unwrap().is_empty());
            assert!(store.apply_intents(owner.id).unwrap().is_empty());
            assert!(store.refused_entries(owner.id).unwrap().is_empty());
            assert!(store.retained_zones(owner.id).unwrap().is_empty());
            for kind in cloud::row_payload::KINDS {
                assert!(store.entity_page(owner.id, kind, None).unwrap().is_empty());
            }
            store.conflict_copies().unwrap()
        })
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
        panic!("actually acknowledged baseline required");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|entity| match entity.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.facts.state, cloud::BindingState::Active);
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert!(!before.acked_system_fields.is_empty());
    assert_eq!(
        before.acked_row_digest,
        Payload::Note(base.clone()).row_digest().unwrap()
    );
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));

    // A normal validated native edit, with its real observer/base and op seq.
    let deck = {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        lease
            .cards_of_note(NoteId(base.id))
            .unwrap()
            .into_iter()
            .min_by_key(|card| card.id)
            .unwrap()
            .deck_id
            .0
    };
    let mut fields: Vec<String> = std::str::from_utf8(&base.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = "native local field retained through republication".into();
    let edit = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: NoteId(base.id),
            fields,
            tags: std::str::from_utf8(&base.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
    let chosen = note_row(&host, base.id, &notetype);
    let pending = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert_eq!(pending.observed_seq, edit.seq.0);
    assert_eq!(pending.base_payload, Some(Payload::Note(base.clone())));
    assert_eq!(pending.acked_payload, before.acked_payload);
    assert_eq!(pending.payload, chosen);

    let (remote, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    let (Payload::Note(local_note), Payload::Note(remote_note)) = (&chosen, &remote) else {
        panic!("two actual note versions required");
    };
    // K15.1/K15.2: both changed field 1 differently; the later native edit
    // wins. Other parts are unchanged, so the complete expected row is L.
    assert_ne!(local_note.flds, base.flds);
    assert_ne!(remote_note.flds, base.flds);
    assert_ne!(local_note.flds, remote_note.flds);
    assert!(local_note.modified > remote_note.modified);
    let codec::DecodeOutcome::Supported(remote_envelope) = codec::Entity::decode(&saved.envelope).unwrap() else {
        panic!("actual saved envelope required");
    };
    assert_ne!(remote_envelope.fields.writer, old.id.0);
    assert_eq!(remote_envelope.fields.origin, before.origin);
    assert_eq!(remote_envelope.fields.base_sha256, Some(before.acked_payload_digest));
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    assert_eq!(runtime.inbox_entries(old.id).unwrap().len(), 1);
    assert_eq!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap(), 1);
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    let merged = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert_eq!(merged.payload, chosen);
    assert_eq!(merged.acked_payload, Some(remote.clone()));
    assert_eq!(merged.acked_envelope.0, saved.envelope);
    assert_eq!(merged.acked_system_fields, saved.system_fields);
    assert_eq!(merged.server_erev, before.server_erev + 1);
    assert!(merged.mark.is_some());
    let old_zone_state = service.inspect(&scope()).unwrap().unwrap().zones[&old_zone].clone();
    assert!(old_zone_state.records[&before.name] == record);
    let calls = service.request_log().unwrap().len();

    // Clean close provides exclusive inspection; this is no crash/power-loss proof.
    host.close().unwrap();
    drop(collection);
    drop(host);
    let copies = closed_store(&dir, |store| {
        assert_eq!(store.binding(old.id).unwrap(), Some(old.clone()));
        store.conflict_copies().unwrap()
    });
    assert_eq!(copies.len(), 1);
    let retained = &copies[0];
    assert_eq!(retained.binding, old.id);
    assert_eq!(retained.copy.kind, cloud::ConflictKind::Note);
    assert_eq!(retained.copy.key.0, before.key);
    assert_eq!(retained.copy.reason, cloud::ConflictReason::FieldConflict);
    assert_eq!(retained.copy.losing_envelope.0, saved.envelope);
    assert_eq!(retained.copy.losing_payload_digest.0, remote.payload_digest().unwrap());
    assert_eq!(retained.copy.winning_payload_digest.0, chosen.payload_digest().unwrap());
    let names = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(
        retained.copy.field_names,
        Some(notetype.fields.iter().map(|f| names(&f.name)).collect())
    );
    assert_eq!(
        retained.copy.template_names,
        Some(notetype.templates.iter().map(|t| names(&t.name)).collect())
    );
    assert_eq!(retained.copy.note_deck, Some(deck));
    assert!(retained.copy.resolved_at_ms.is_none() && retained.copy.staged_path.is_none());

    let host = open_host(&dir, keys.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), old);
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.active_epoch, Some(old.facts.epoch));
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    let owner = runtime
        .publication_facts(&collection)
        .unwrap()
        .bindings
        .into_iter()
        .find(|b| b.id == old.id)
        .unwrap();
    let mut isolated = old.clone();
    isolated.facts.state = cloud::BindingState::Isolated(cloud::IsolationReason::Republished);
    assert_eq!(owner, isolated);
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
            .contains(&isolated)
    );
    // Retained owner means successful empty reads, never BindingNotFound.
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().is_none());
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(chosen.clone()));
    assert_eq!(new_note.acked_row_digest, chosen.row_digest().unwrap());
    assert!(new_note.mark.is_none() && new_note.acked_rev > 0 && new_note.server_erev > 0);
    assert!(!new_note.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    host.close().unwrap();
    drop(collection);
    drop(host);
    assert_eq!(retained_copies_without_work(&dir, &isolated), copies);

    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .contains(&isolated)
    );
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().is_none());
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(chosen.clone()));
    assert_eq!(reopened_note.acked_envelope, new_note.acked_envelope);
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert_eq!(note_row(&reopened, base.id, &notetype), chosen);
    reopened.close().unwrap();
    drop(reopened);
    // Full equality includes CopyId, envelope, names, timestamps and owner.
    assert_eq!(retained_copies_without_work(&dir, &isolated), copies);
    assert!(service.inspect(&scope()).unwrap().unwrap().zones[&old_zone] == old_zone_state);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.zone.as_deref() != Some(old_zone.as_str())
            || !matches!(
                r.kind,
                meguri_testkit::cloud::RequestKind::Modify | meguri_testkit::cloud::RequestKind::DeleteZone
            )
    }));
}

#[test]
fn partial_cloud_ack_keeps_a_multi_note_step_pending_until_every_named_key_is_saved() {
    use meguri_engine::{DisabledReason, OpSource, OpSpec, UndoScope, UndoStatus};
    use meguri_model::ids::NoteId;
    use meguri_storage::ReadRows;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("partial-ack");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(614)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let c = host.collection().unwrap();
    let notes = runtime
        .entity_facts_page(cloud::entity_codec::Kind::Note, None)
        .unwrap();
    assert_eq!(notes.len(), 2);
    let ids: Vec<_> = notes
        .iter()
        .map(|e| NoteId(i64::from_be_bytes(e.key.as_slice().try_into().unwrap())))
        .collect();
    c.run(
        OpSpec::named("cloud_multi_note_edit", OpSource::UserUndoable),
        move |ctx| {
            for (at, id) in ids.into_iter().enumerate() {
                let mut note = ctx.scope().note(id)?.unwrap();
                note.data = "x".repeat(if at == 0 { 16 } else { 8192 });
                ctx.scope().put(&note)?;
            }
            Ok(())
        },
    )
    .unwrap();
    let instance = runtime.instance().unwrap();
    let names = notes.iter().map(|e| e.name.clone()).collect();
    let batch = runtime.build_collection_batch(&instance, &c, &names).unwrap();
    assert_eq!(batch.saves.len(), 2);
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    let binding = runtime.status().unwrap().binding.unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let largest = batch
        .saves
        .iter()
        .map(|save| {
            meguri_testkit::cloud::record_non_asset_bytes(
                &zone,
                &RecordInput::encrypted(&save.name, "MGEntity", save.envelope.clone()),
            )
        })
        .max()
        .unwrap();
    let mut limited = Config::default();
    limited.limits.non_asset_per_record = largest - 1;
    service
        .request(&scope(), Caller::default(), Operation::Configure(limited))
        .unwrap()
        .result
        .unwrap();
    let failed = send_actual_batch(&host, &mut service, &batch);
    assert_eq!(failed.len(), 1);
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    let clean: BTreeSet<_> = names.difference(&failed).cloned().collect();
    assert!(
        runtime
            .build_collection_batch(&instance, &c, &clean)
            .unwrap()
            .saves
            .is_empty()
    );
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    service
        .request(&scope(), Caller::default(), Operation::Configure(Config::default()))
        .unwrap()
        .result
        .unwrap();
    let retry = runtime.build_collection_batch(&instance, &c, &failed).unwrap();
    assert_eq!(retry.saves.len(), 1);
    assert!(send_actual_batch(&host, &mut service, &retry).is_empty());
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded)
    ));
    host.close().unwrap();
}

fn republish_pending_native_note(stop_at_capture: bool) {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_engine::{DisabledReason, EntryId, OpSource, UndoScope, UndoStatus};
    use meguri_model::ids::NoteId;
    use meguri_storage::{ReadRows, ReaderRole};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("pending-republication");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(615)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let evidence = collection.open_evidence().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("actually published note required");
    };
    assert!(before.mark.is_none() && before.server_erev > 0);
    let old_zone = cloud::adapter::collection_zone(old.facts.epoch);
    let old_record = service.inspect(&scope()).unwrap().unwrap().zones[&old_zone].records[&before.name].clone();
    let mut fields: Vec<String> = std::str::from_utf8(&base.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = "native edit awaiting publication under its next binding".into();
    let expected_fields = fields.join("\u{1f}");
    let edit = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: NoteId(base.id),
            fields,
            tags: std::str::from_utf8(&base.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
    let UndoStatus::Available(step) = collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("the real native edit must start undoable");
    };
    assert_eq!(step.id, EntryId(edit.seq.0));
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 1);
    {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(lease.note(NoteId(base.id)).unwrap().unwrap().fields, expected_fields);
    }
    let edited = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(edited.observed_seq, step.id.0);
    assert!(edited.mark.is_some());
    let Payload::Note(note) = &edited.payload else {
        panic!("native note payload required");
    };
    assert_eq!(note.id, base.id);
    assert_eq!(note.flds, expected_fields.as_bytes());
    assert_ne!(edited.payload, Payload::Note(base.clone()));

    let instance = runtime.instance().unwrap();
    let names = BTreeSet::from([before.name.clone()]);
    let at_gap = Arc::new(Mutex::new(None));
    if stop_at_capture {
        let observed = at_gap.clone();
        let captured_collection = collection.clone();
        runtime.set_cloud_capture_hook_for_test(move |runtime| {
            let pending = captured_collection.peek_undo(UndoScope::Global);
            let stopped = runtime.adapter_stop_syncing();
            *observed.lock().unwrap() = Some((pending, stopped));
        });
    }
    let calls = service.request_log().unwrap().len();
    let built = runtime.build_collection_batch(&instance, &collection, &names);
    if stop_at_capture {
        let (pending, stopped) = at_gap.lock().unwrap().take().expect("capture gap must be reached");
        assert_eq!(
            pending.unwrap(),
            UndoStatus::Unavailable(DisabledReason::UploadPending),
            "Engine must have flagged the native step before Stop"
        );
        stopped.unwrap();
        assert!(matches!(built, Err(cloud::runtime::RuntimeError::StaleIncarnation)));
        assert_eq!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::Stopped)
        );
        assert!(
            runtime
                .entity_facts(&before.name)
                .unwrap()
                .unwrap()
                .in_flight
                .is_empty()
        );
    } else {
        let batch = built.unwrap();
        assert_eq!(batch.saves.len(), 1);
        let save = &batch.saves[0];
        assert_eq!(save.name, before.name);
        let image = codec::receive(
            &old.facts.record_key.0,
            &save.name,
            &save.envelope,
            codec::AssetInput::Absent,
        )
        .unwrap();
        assert_eq!(image.payload, Some(edited.payload.clone()));
        // Keep this actual old-binding image unacknowledged.
    }
    assert_eq!(
        service.request_log().unwrap().len(),
        calls,
        "the old build must send nothing"
    );
    assert_eq!(
        collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    );
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 1);

    // Explicit confirmation on the same live collection draws a new record key.
    // The helper drives real scans, Service entity saves/ACKs and the active switch.
    let new = publish_actual_collection(&host, &mut service);
    assert_ne!(new.id, old.id);
    assert_ne!(new.facts.epoch, old.facts.epoch);
    assert!(new.facts.record_key.0 != old.facts.record_key.0);
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert_eq!(collection.open_evidence().unwrap().generation, evidence.generation);
    assert_eq!(collection.open_evidence().unwrap().native_file, evidence.native_file);
    let published = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_ne!(published.name, before.name);
    assert_eq!(published.payload, edited.payload);
    assert_eq!(published.acked_payload, Some(edited.payload.clone()));
    assert!(published.mark.is_none() && published.acked_rev > 0 && published.server_erev > 0);
    let db = service.inspect(&scope()).unwrap().unwrap();
    let record = &db.zones[&cloud::adapter::collection_zone(new.facts.epoch)].records[&published.name];
    assert!(record.fields["e"] == Field::EncryptedBytes(published.acked_envelope.0.clone()));
    assert_eq!(record.system_fields.bytes(), published.acked_system_fields.as_slice());
    assert!(db.zones[&old_zone].records[&before.name] == old_record);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    assert_eq!(
        collection.undo_depth_for_testing().unwrap(),
        1,
        "retain the same native step"
    );
    let settled = collection.peek_undo(UndoScope::Global).unwrap();
    {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(lease.note(NoteId(base.id)).unwrap().unwrap().fields, expected_fields);
    }
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    let note = runtime.binding_entity(new.id, &published.name).unwrap().unwrap();
    assert_eq!(note.acked_payload, Some(edited.payload));
    assert_eq!(note.acked_envelope, published.acked_envelope);
    assert_eq!(note.acked_system_fields, published.acked_system_fields);
    assert!(note.mark.is_none());
    reopened.close().unwrap();

    // This is the status observed on the original Engine, before closing it.
    assert_eq!(
        settled,
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded),
        "the same native step {} must settle after real new-binding ACKs (capture gap: {stop_at_capture})",
        step.id
    );
}

#[test]
fn new_binding_publication_ack_settles_the_native_step_flagged_by_an_unacked_old_batch() {
    republish_pending_native_note(false);
}

#[test]
fn stop_between_cloud_capture_and_registration_rejects_the_old_build_and_new_binding_ack_settles_the_step() {
    republish_pending_native_note(true);
}

fn edit_workflow_note(collection: &meguri_engine::Collection, note: &cloud::entity_codec::Note, back: &str) {
    let mut fields: Vec<String> = std::str::from_utf8(&note.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = back.into();
    let edit = meguri_engine::notes::update(
        collection,
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note.id),
            fields,
            tags: std::str::from_utf8(&note.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
}

#[test]
fn prior_open_observation_does_not_block_reopened_batch_or_sync_now_selection() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_storage::ReaderRole;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("observation-restart");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(616)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &before.payload else {
        panic!("native note required");
    };
    edit_workflow_note(&collection, note, "pending across restart");
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert_eq!(
        pending.observer_open,
        runtime.stored_state_for_testing().unwrap().open_counter
    );
    host.close().unwrap();
    drop(collection);
    drop(host);

    let host = open_host(&dir, keys());
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    runtime.refresh_identity().unwrap();
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), binding);
    assert!(runtime.stored_state_for_testing().unwrap().open_counter > pending.observer_open);
    {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        let snapshot = lease.snapshot().unwrap();
        assert_eq!(pending.observed_generation, snapshot.generation().0);
        assert!(pending.observed_seq > snapshot.seq().0);
    }
    let rescanned = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(rescanned.observer_open, pending.observer_open);
    assert_eq!(rescanned.observed_generation, pending.observed_generation);
    assert_eq!(rescanned.observed_seq, pending.observed_seq);
    assert_eq!(rescanned.local_rev, pending.local_rev);
    assert_eq!(rescanned.payload, pending.payload);

    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::FetchZone {
                zone: "meguri-root".into(),
                token: None,
            },
        )
        .unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else {
        panic!("actual root fetch required");
    };
    assert!(!page.more, "the small published root must be fully fetched");
    let active = page
        .changes
        .into_iter()
        .find_map(|change| match change {
            meguri_testkit::cloud::RecordChange::Saved(record)
                if record.name == "active" && record.record_type == "MGActive" =>
            {
                Some(record)
            }
            _ => None,
        })
        .expect("actual published root required");
    let Field::EncryptedBytes(envelope) = &active.fields["e"] else {
        panic!("root envelope required");
    };
    assert!(
        runtime
            .accept_root_fetch(
                &instance,
                Some(&Saved {
                    name: active.name.clone(),
                    envelope: envelope.clone(),
                    system_fields: active.system_fields.bytes().to_vec(),
                }),
                !page.more,
                response.request_id,
            )
            .unwrap()
    );
    let exchange = runtime.sync_now(&instance, &collection).unwrap();
    let selected = runtime.exchange_result(exchange).unwrap();
    assert!(selected.selected_names.contains(&before.name));
    assert!(selected.boundary_seq < pending.observed_seq);
    let batch = runtime
        .build_collection_batch(&instance, &collection, &BTreeSet::from([before.name.clone()]))
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(batch.saves[0].name, before.name);
    let image = codec::receive(
        &binding.facts.record_key.0,
        &batch.saves[0].name,
        &batch.saves[0].envelope,
        codec::AssetInput::Absent,
    )
    .unwrap();
    assert_eq!(image.payload, Some(pending.payload));
    host.close().unwrap();
}

#[test]
fn same_open_observation_after_cloud_capture_defers_the_old_batch_and_preserves_the_edit() {
    use cloud::entity_codec::{self as codec, Payload};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("observation-capture");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(617)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = before.payload else {
        panic!("native note required");
    };
    edit_workflow_note(&collection, &note, "pending before capture");
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    let editing = collection.clone();
    runtime.set_cloud_capture_hook_for_test(move |_| {
        edit_workflow_note(&editing, &note, "committed after capture");
    });
    let instance = runtime.instance().unwrap();
    let names = BTreeSet::from([before.name.clone()]);
    let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    let newer = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(
        newer.observer_open,
        runtime.stored_state_for_testing().unwrap().open_counter
    );
    assert_eq!(newer.observer_open, pending.observer_open);
    assert_eq!(newer.observed_generation, pending.observed_generation);
    assert!(newer.observed_seq > pending.observed_seq);
    assert_ne!(newer.payload, pending.payload);
    assert!(newer.mark.is_some());
    assert!(batch.saves.is_empty());
    assert!(newer.in_flight.is_empty());
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert_eq!(next.saves.len(), 1);
    assert_eq!(next.saves[0].name, before.name);
    let image = codec::receive(
        &binding.facts.record_key.0,
        &next.saves[0].name,
        &next.saves[0].envelope,
        codec::AssetInput::Absent,
    )
    .unwrap();
    assert_eq!(image.payload, Some(newer.payload));
    host.close().unwrap();
}
