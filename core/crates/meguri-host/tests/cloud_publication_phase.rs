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
    p
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
            PublicationWork::Own(request) => return request,
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
        assert_eq!(mode, "decision");
        let mut service = Service::open(dir.parent().unwrap(), Config::default(), SeededRandom::new(44)).unwrap();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let p = preview(&host, &mut service);
        let op = runtime.confirm_connection_publication(&p, &collection).unwrap();
        runtime
            .perform_publication_local(op, LocalWork::Bind, &collection)
            .unwrap();
        faultpoint::arm(&point, Fault::Abort);
        runtime
            .perform_publication_local(op, LocalWork::WriteDecision, &collection)
            .unwrap();
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
    crash_worker(&dir, "decision", "publication.after_decision_write");
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
    let host = open_host(&dir, keys());
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
            PublicationWork::Own(request) if request.kind == OwnKind::SaveActive => return request,
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
