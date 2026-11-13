//! Real generated Service controls and owned Runtime intake/read paths. Native-
//! shaped nil/invalid bodies exercise core semantics, not live CloudKit behavior.
use super::*;
use cloud::policy::{Attention, ChangeCause, DeviceEvent, DeviceSettings};

fn setup() -> (tempfile::TempDir, std::path::PathBuf, CoreHost, Service<SeededRandom>, String) {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("root-attention");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(1937)).unwrap();
    publish_actual_collection(&host, &mut service);
    policy(&host);
    let (sdk, _, _) = host.cloud_runtime().unwrap().register_sdk(false).unwrap();
    (temp, dir, host, service, sdk)
}

fn policy(host: &CoreHost) {
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true, paused: false, automatic: true, media: false,
    })).unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())), origin_sdk: None,
    }).unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true }).unwrap();
}

fn assert_attention(host: &CoreHost, expected: bool) {
    host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud)).unwrap();
    let state = host.cloud_policy_driver().unwrap().state();
    assert_eq!(state.facts.summary.attention.get(&Attention::RootUnreadable).copied().unwrap_or(0), u64::from(expected));
    assert_eq!(state.status().attention.contains_key(&Attention::RootUnreadable), expected);
    if expected { assert!(!state.controls().automatic_eligible); }
    assert!(!state.facts.summary.attention_known, "other K10 producers remain missing");
}

fn control(service: &mut Service<SeededRandom>, zone: &str, name: &str, fields: RequestedFields) -> ControlRecord {
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.into(), name: name.into(), fields,
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual Service control required"); };
    assert_eq!(record.record_type, if name == "active" { "MGActive" } else { "MGEpoch" });
    publication_control_record(&record).unwrap()
}

fn saved(name: &str, record: &ControlRecord) -> Saved {
    Saved { name: name.into(), envelope: record.encrypted.clone().unwrap_or_default(), system_fields: record.system_fields.0.clone() }
}

fn issued(host: &CoreHost, sdk: &str, zone: &str) -> (u64, cloud::adapter::FetchRead) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let fetch = runtime.adapter_begin_fetch(sdk, vec![zone.into()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(sdk, group.clone(), true).unwrap();
    runtime.adapter_did_fetch(sdk, &group, true).unwrap();
    let mut reads = runtime.adapter_fetch_reads(sdk, fetch, &collection).unwrap();
    assert_eq!(reads.len(), 1);
    let read = reads.remove(0);
    assert_eq!(read.zone(), zone);
    (fetch, read)
}

fn fresh_identity(runtime: &cloud::runtime::Runtime) {
    let request = runtime.adapter_begin_identity_read(false, None).unwrap();
    assert!(runtime.adapter_complete_identity_read(request, IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()))).unwrap());
}

#[test]
fn root_unreadable_owned_active_survives_reopen_and_real_incremental_no_change_fetch() {
    let (_temp, dir, host, mut service, sdk) = setup();
    let unreadable = control(&mut service, "meguri-root", "active", RequestedFields::Named(Vec::new()));
    assert!(unreadable.encrypted.is_none());
    let full = control(&mut service, "meguri-root", "active", RequestedFields::All);
    let runtime = host.cloud_runtime().unwrap();
    runtime.adapter_fetched(&sdk, "meguri-root", &[saved("active", &unreadable)], &[]).unwrap();
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(unreadable.system_fields.clone()));
    runtime.adapter_state_update(&sdk, b"synthetic-retained-cursor").unwrap();
    assert_attention(&host, true);
    host.close().unwrap();
    drop(host);
    let host = open_host(&dir, keys());
    policy(&host);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30)).unwrap();
    let (sdk, serialization, _) = runtime.register_sdk(false).unwrap();
    assert!(serialization.is_some());
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(unreadable.system_fields));
    // Service token from a real first page, then an unchanged incremental page.
    let response = service.request(&scope(), Caller::default(), Operation::FetchZone { zone: "meguri-root".into(), token: None }).unwrap();
    let Ok(Reply::ZoneChanges(first)) = response.result else { panic!("root page required"); };
    let response = service.request(&scope(), Caller::default(), Operation::FetchZone { zone: "meguri-root".into(), token: Some(first.token) }).unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else { panic!("incremental root page required"); };
    assert!(page.changes.is_empty() && !page.more);
    let fetch = runtime.adapter_begin_fetch(&sdk, vec!["meguri-root".into()]).unwrap();
    let group = format!("meguri-{fetch}");
    runtime.adapter_will_fetch(&sdk, group.clone(), true).unwrap();
    runtime.adapter_fetched(&sdk, "meguri-root", &[], &[]).unwrap();
    runtime.adapter_did_fetch(&sdk, &group, true).unwrap();
    // Planning finalizes the actual no-change boundary. Its possible corrective
    // Active read is intentionally not answered; idle cannot clear attention.
    let _reads = runtime.adapter_fetch_reads(&sdk, fetch, &collection).unwrap();
    assert!(runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
    assert!(!runtime.sdk_root_confirmed(), "an unreadable latest Active cannot fall back to an older root ACK");
    assert_attention(&host, true);
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    runtime.adapter_fetched(&sdk, "meguri-root", &[saved("active", &full)], &[]).unwrap();
    assert_attention(&host, false);
    host.close().unwrap();
}

#[test]
fn root_unreadable_epoch_intakes_require_latest_complete_matching_body_not_retained_crt() {
    let (_temp, _dir, host, mut service, sdk) = setup();
    let runtime = host.cloud_runtime().unwrap();
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let active = control(&mut service, "meguri-root", "active", RequestedFields::All);
    let complete = control(&mut service, &zone, "epoch", RequestedFields::All);
    let original_crt = runtime.epoch_crt_for_testing().unwrap();
    let mut invalid = vec![saved("epoch", &control(&mut service, &zone, "epoch", RequestedFields::Named(Vec::new())))];
    let mut bad = saved("epoch", &complete); bad.envelope = vec![255]; invalid.push(bad);
    let DecodeOutcome::Supported(epoch) = Epoch::decode(complete.encrypted.as_deref().unwrap()).unwrap() else { panic!("supported epoch"); };
    let mut publishing = epoch.clone(); publishing.state = EpochState::Publishing; publishing.counts.clear();
    invalid.push(Saved { envelope: publishing.encode().unwrap(), ..saved("epoch", &complete) });
    let mut wrong = epoch; wrong.epoch_id = [99;16];
    invalid.push(Saved { envelope: wrong.encode().unwrap(), ..saved("epoch", &complete) });
    for (index, record) in invalid.into_iter().enumerate() {
        if index % 2 == 0 { runtime.adapter_fetched(&sdk, &zone, &[record], &[]).unwrap(); }
        else { runtime.receive_collection_records(&runtime.instance().unwrap(), &[record]).unwrap(); }
        assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(active.system_fields.clone()));
        assert_eq!(runtime.epoch_crt_for_testing().unwrap(), original_crt, "historical crt stays qualified but cannot clear readability");
        runtime.adapter_fetched(&sdk, "meguri-root", &[saved("active", &active)], &[]).unwrap();
        assert_attention(&host, true);
    }
    runtime.receive_collection_records(&runtime.instance().unwrap(), &[saved("epoch", &complete)]).unwrap();
    assert_attention(&host, false);
    runtime.adapter_fetched(&sdk, &zone, &[], &["epoch".into()]).unwrap();
    assert_attention(&host, true);
    runtime.adapter_fetched(&sdk, &zone, &[saved("epoch", &complete)], &[]).unwrap();
    assert_attention(&host, false);
    host.close().unwrap();
}

#[test]
fn root_unreadable_issued_active_read_sets_version_with_existence_only_drain() {
    let (_temp, _dir, host, mut service, sdk) = setup();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.forget_adapter_state_for_testing(&sdk).unwrap();
    runtime.retire_sdk(&sdk, true).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    let (sdk, state, _) = runtime.register_sdk(false).unwrap();
    assert!(state.is_none() && !runtime.sdk_root_confirmed());
    let (fetch, read) = issued(&host, &sdk, "meguri-root");
    let before = runtime.adapter_fetch_drains();
    let unreadable = control(&mut service, "meguri-root", "active", RequestedFields::Named(Vec::new()));
    fresh_identity(runtime);
    assert!(runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(unreadable.clone()), false, None, None).unwrap().is_none());
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(unreadable.system_fields));
    assert_eq!(&runtime.adapter_fetch_drains()[before.len()..], &[cloud::adapter::FetchDrain { zone: "meguri-root".into(), fetched: false }]);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed && !runtime.sdk_root_confirmed());
    assert!(runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0); assert!(batch.saves.is_empty());
    assert_attention(&host, true);
    // A later owned complete readable Active clears only with the retained
    // actual complete Epoch, and is the operation that confirms this instance.
    let (fetch, read) = issued(&host, &sdk, "meguri-root");
    fresh_identity(runtime);
    let full = control(&mut service, "meguri-root", "active", RequestedFields::All);
    runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(full), false, None, None).unwrap();
    assert!(runtime.adapter_finish_fetch(&sdk, fetch).unwrap());
    assert!(runtime.sdk_root_confirmed());
    assert_attention(&host, false);
    host.close().unwrap();
}

#[test]
fn root_unreadable_stale_read_account_and_sdk_results_cannot_clear_version() {
    for stale in ["read", "account", "sdk"] {
        let (_temp, _dir, host, mut service, sdk) = setup();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
        let nil_epoch = control(&mut service, &zone, "epoch", RequestedFields::Named(Vec::new()));
        runtime.adapter_fetched(&sdk, &zone, &[saved("epoch", &nil_epoch)], &[]).unwrap();
        let mark = runtime.root_unreadable_for_testing().unwrap().unwrap();
        let (fetch, read) = issued(&host, &sdk, &zone);
        let complete = control(&mut service, &zone, "epoch", RequestedFields::All);
        match stale {
            "read" => { assert!(!runtime.adapter_finish_fetch(&sdk, fetch).unwrap()); }
            "account" => runtime.adapter_account(Some(&sdk), IdentityStatus::SignedOut).unwrap(),
            "sdk" => { runtime.retire_sdk(&sdk, true).unwrap(); runtime.release_sdk(&sdk).unwrap(); runtime.register_sdk(false).unwrap(); }
            _ => unreachable!(),
        }
        assert!(runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Found(complete.clone()), false, None, None).is_err());
        assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(mark.clone()));
        if stale != "read" { assert!(runtime.adapter_fetched(&sdk, &zone, &[saved("epoch", &complete)], &[]).is_err()); }
        assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(mark));
        host.close().unwrap();
    }
}

#[test]
fn root_unreadable_clear_failure_rolls_back_epoch_and_survives_reopen() {
    use meguri_model::faultpoint::{self, Fault};
    let (_temp, dir, host, mut service, sdk) = setup();
    let runtime = host.cloud_runtime().unwrap();
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    runtime.adapter_fetched(&sdk, &zone, &[], &["epoch".into()]).unwrap();
    let mark = runtime.root_unreadable_for_testing().unwrap();
    assert!(mark.is_some());
    let crt = runtime.epoch_crt_for_testing().unwrap();
    let complete = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |e| e.crt += 1);
    faultpoint::arm("cloud.adapter.before_commit", Fault::IoError(std::io::ErrorKind::Other));
    let result = runtime.adapter_fetched(&sdk, &zone, &[complete], &[]);
    faultpoint::disarm("cloud.adapter.before_commit");
    assert!(result.is_err() && runtime.status().unwrap().tracking_stopped);
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
    assert_eq!(runtime.epoch_crt_for_testing().unwrap(), crt);
    host.close().unwrap(); drop(host);
    let host = open_host(&dir, keys());
    assert_eq!(host.cloud_runtime().unwrap().root_unreadable_for_testing().unwrap(), mark);
    assert_eq!(host.cloud_runtime().unwrap().epoch_crt_for_testing().unwrap(), crt);
    host.close().unwrap();
}

#[test]
fn root_unreadable_missing_epoch_read_keeps_mark_and_cannot_finish_absence() {
    let (_temp, _dir, host, mut service, sdk) = setup();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let zone = cloud::adapter::collection_zone(runtime.status().unwrap().binding.unwrap().facts.epoch);
    let response = service.request(&scope(), Caller::default(), Operation::Modify {
        zone: zone.clone(), atomic: false, changes: vec![Mutation::Delete { name: "epoch".into() }],
    }).unwrap();
    assert!(matches!(response.result, Ok(Reply::Modified(_))));
    runtime.adapter_fetched(&sdk, &zone, &[], &["epoch".into()]).unwrap();
    let mark = runtime.root_unreadable_for_testing().unwrap();
    assert!(mark.is_some());
    let (_fetch, read) = issued(&host, &sdk, &zone);
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: zone.clone(), name: "epoch".into(), fields: RequestedFields::All,
    }).unwrap();
    assert!(response.result.is_err_and(|e| e.code == meguri_testkit::cloud::ErrorCode::UnknownItem));
    let drains = runtime.adapter_fetch_drains();
    let followup = runtime.adapter_complete_fetch_read(&read, &collection, RecordRead::Missing, false, None, None).unwrap().unwrap();
    assert!(followup.is_active());
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
    assert_eq!(runtime.adapter_fetch_drains(), drains);
    assert!(!runtime.adapter_snapshot().unwrap().absence_pass_completed && !runtime.sdk_root_confirmed());
    let batch = runtime.build_collection_batch(&sdk, &collection, &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0); assert!(batch.saves.is_empty());
    fresh_identity(runtime);
    let full = control(&mut service, "meguri-root", "active", RequestedFields::All);
    runtime.adapter_complete_fetch_read(&followup, &collection, RecordRead::Found(full), false, None, None).unwrap();
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
    assert_eq!(runtime.status().unwrap().binding.unwrap().facts.state, cloud::BindingState::Isolated(cloud::IsolationReason::ZoneMissing));
    host.close().unwrap();
}

#[test]
fn root_unreadable_only_actual_stop_or_publication_switch_ends_old_obligation() {
    for stop in [true, false] {
        let (_temp, _dir, host, mut service, sdk) = setup();
        let runtime = host.cloud_runtime().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap();
        let nil = control(&mut service, "meguri-root", "active", RequestedFields::Named(Vec::new()));
        runtime.adapter_fetched(&sdk, "meguri-root", &[saved("active", &nil)], &[]).unwrap();
        assert!(runtime.root_unreadable_for_testing().unwrap().is_some());
        runtime.adapter_pause(true).unwrap();
        assert!(runtime.root_unreadable_for_testing().unwrap().is_some(), "Pause never clears attention");
        runtime.adapter_pause(false).unwrap();
        if stop {
            runtime.adapter_stop_syncing().unwrap();
            assert_eq!(runtime.status().unwrap().binding.unwrap().id, binding.id);
            assert_eq!(runtime.root_unreadable_for_testing().unwrap(), None);
        } else {
            // Fresh real preview plus explicit confirmation drives V15. The
            // unreadable stored version is not used as an invented switch ACK.
            let next = publish_actual_collection(&host, &mut service);
            assert_ne!(next.id, binding.id);
            assert_eq!(runtime.root_unreadable_for_testing().unwrap(), None);
        }
        host.close().unwrap();
    }
}

#[test]
fn root_unreadable_non_sdk_active_read_retains_actual_version_without_confirmation() {
    let (_temp, _dir, host, mut service, sdk) = setup();
    let runtime = host.cloud_runtime().unwrap();
    runtime.retire_sdk(&sdk, true).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    let (successor, _, _) = runtime.register_sdk(false).unwrap();
    assert!(!runtime.sdk_root_confirmed());
    let instance = runtime.instance().unwrap();
    let response = service.request(&scope(), Caller::default(), Operation::Read {
        zone: "meguri-root".into(), name: "active".into(), fields: RequestedFields::Named(Vec::new()),
    }).unwrap();
    let Ok(Reply::Record(record)) = response.result else { panic!("actual Active read required"); };
    let unreadable = publication_control_record(&record).unwrap();
    assert!(unreadable.encrypted.is_none() && !unreadable.system_fields.0.is_empty());
    let before = runtime.adapter_snapshot().unwrap();
    let drains = runtime.adapter_fetch_drains();
    // A current caller alone is insufficient: the real no-root fetch boundary
    // must qualify this read before its unreadable body can affect attention.
    assert!(!runtime.accept_root_read(&instance, response.request_id, &[], &unreadable.system_fields.0, false).unwrap());
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), None);
    runtime.sdk_fetch_completed_without_root(&instance).unwrap();
    fresh_identity(runtime);
    assert!(!runtime.accept_root_read(&instance, response.request_id, &[], &unreadable.system_fields.0, false).unwrap());
    assert_eq!(runtime.root_unreadable_for_testing().unwrap(), Some(unreadable.system_fields.clone()));
    let after = runtime.adapter_snapshot().unwrap();
    let content: serde_json::Value = serde_json::from_slice(&after.tracking_content).unwrap();
    let applied: Saved = serde_json::from_value(content[4]["active"].clone()).unwrap();
    assert_eq!(applied, saved("active", &unreadable));
    assert_eq!(after.root, before.root, "invalid Active cannot replace a root ACK");
    assert_eq!(runtime.adapter_fetch_drains(), drains);
    assert_eq!(after.absence_pass_completed, before.absence_pass_completed);
    assert!(!runtime.sdk_root_confirmed());
    assert_attention(&host, true);
    let batch = runtime.build_collection_batch(&successor, &host.collection().unwrap(), &entity_names(runtime)).unwrap();
    assert_eq!(batch.id, 0);
    assert!(batch.saves.is_empty());
    host.close().unwrap();
}

// Obtain a real, pending publication from the engine's Restore path. Moving
// its emitted sidecar into another live home models an external publication
// change without changing that Runtime's cached instance or verified account.
fn restored_sidecar(dir: &Path) -> Vec<u8> {
    let donor = dir.join("publication-donor");
    generated(&donor);
    let host = open_host(&donor, keys());
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.replace_verified(candidate_stage(&donor), meguri_engine::replace::ReplaceReason::Restore, |_| Ok(())).unwrap();
    drop(slot);
    assert_eq!(decision(&host).publication.decision, meguri_engine::device_state::PublicationDecision::Pending);
    host.close().unwrap();
    std::fs::read(donor.join("meguri-sidecar.json")).unwrap()
}

#[test]
fn root_unreadable_non_sdk_epoch_rechecks_file_and_publication_at_inbox_boundary() {
    use cloud::runtime::FileIdentityProvider;
    use std::sync::atomic::{AtomicBool, Ordering};
    for change in ["file", "publication"] {
        let (_temp, dir, host, mut service, sdk) = setup();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap();
        let zone = cloud::adapter::collection_zone(binding.facts.epoch);
        runtime.adapter_fetched(&sdk, &zone, &[], &["epoch".into()]).unwrap();
        let mark = runtime.root_unreadable_for_testing().unwrap();
        assert!(mark.is_some());
        let crt = runtime.epoch_crt_for_testing().unwrap();
        let complete = super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |e| e.crt += 1);
        let instance = runtime.instance().unwrap();
        let file_before = NativeFiles.identity(collection.path()).unwrap();
        let publication_before = decision(&host);
        let sidecar = dir.join("meguri-sidecar.json");
        let sidecar_before = std::fs::read(&sidecar).unwrap();
        let replacement = if change == "publication" { Some(restored_sidecar(&dir)) } else { None };
        let reached = Arc::new(AtomicBool::new(false));
        let observed = reached.clone();
        let live = collection.path().to_path_buf();
        let original = dir.join("intake-original");
        let retained = original.clone();
        runtime.set_incoming_stage_hook_for_testing(move |stage| {
            if stage == cloud::testing::IncomingStage::FetchBeforeInbox && !observed.swap(true, Ordering::SeqCst) {
                if let Some(bytes) = &replacement {
                    std::fs::write(&sidecar, bytes).unwrap();
                } else {
                    let copy = live.with_extension("intake-copy");
                    std::fs::copy(&live, &copy).unwrap();
                    std::fs::rename(&live, &retained).unwrap();
                    std::fs::rename(copy, &live).unwrap();
                }
            }
        });
        assert!(runtime.receive_collection_records(&instance, &[complete]).is_err());
        assert!(reached.load(Ordering::SeqCst));
        runtime.validate_caller(&instance).unwrap();
        assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
        assert_eq!(runtime.epoch_crt_for_testing().unwrap(), crt);
        assert!(!runtime.status().unwrap().tracking_stopped, "a stale observation is refused before tracking writes");
        if change == "file" {
            assert_ne!(NativeFiles.identity(collection.path()).unwrap(), file_before);
            assert_eq!(decision(&host), publication_before);
            std::fs::remove_file(collection.path()).unwrap();
            std::fs::rename(original, collection.path()).unwrap();
        } else {
            assert_eq!(NativeFiles.identity(collection.path()).unwrap(), file_before);
            assert_ne!(decision(&host).publication.identity, publication_before.publication.identity);
            assert_eq!(decision(&host).publication.decision, meguri_engine::device_state::PublicationDecision::Pending);
            std::fs::write(dir.join("meguri-sidecar.json"), sidecar_before).unwrap();
        }
        host.close().unwrap();
    }
}

#[test]
fn root_unreadable_non_sdk_active_refuses_stale_file_and_changed_publication() {
    for change in ["file", "publication"] {
        let (_temp, dir, host, mut service, sdk) = setup();
        let runtime = host.cloud_runtime().unwrap();
        runtime.retire_sdk(&sdk, true).unwrap();
        runtime.release_sdk(&sdk).unwrap();
        runtime.register_sdk(false).unwrap();
        let instance = runtime.instance().unwrap();
        runtime.sdk_fetch_completed_without_root(&instance).unwrap();
        let response = service.request(&scope(), Caller::default(), Operation::Read {
            zone: "meguri-root".into(), name: "active".into(), fields: RequestedFields::Named(Vec::new()),
        }).unwrap();
        let Ok(Reply::Record(record)) = response.result else { panic!("actual Active read required"); };
        let unreadable = publication_control_record(&record).unwrap();
        let before = runtime.adapter_snapshot().unwrap();
        let drains = runtime.adapter_fetch_drains();
        let mark = runtime.root_unreadable_for_testing().unwrap();
        let live = dir.join("collection.anki2");
        let original = dir.join("active-original");
        let sidecar = dir.join("meguri-sidecar.json");
        let sidecar_before = std::fs::read(&sidecar).unwrap();
        if change == "file" {
            let copy = dir.join("active-copy");
            std::fs::copy(&live, &copy).unwrap();
            std::fs::rename(&live, &original).unwrap();
            std::fs::rename(copy, &live).unwrap();
        } else {
            std::fs::write(&sidecar, restored_sidecar(&dir)).unwrap();
        }
        runtime.validate_caller(&instance).unwrap();
        assert!(runtime.accept_root_read(&instance, response.request_id, &[], &unreadable.system_fields.0, false).is_err());
        assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
        let after = runtime.adapter_snapshot().unwrap();
        assert_eq!(after.root, before.root);
        assert_eq!(after.tracking_content, before.tracking_content);
        assert_eq!(runtime.adapter_fetch_drains(), drains);
        assert!(!runtime.sdk_root_confirmed());
        if change == "file" {
            std::fs::remove_file(&live).unwrap();
            std::fs::rename(original, live).unwrap();
        } else {
            std::fs::write(sidecar, sidecar_before).unwrap();
        }
        host.close().unwrap();
    }
}

#[test]
fn root_unreadable_sdk_active_and_epoch_refuse_external_replacement_before_set_or_clear() {
    use cloud::runtime::FileIdentityProvider;
    use std::sync::atomic::{AtomicBool, Ordering};
    for name in ["active", "epoch"] {
        for marked in [false, true] {
            for change in ["file", "publication"] {
                let (_temp, dir, host, mut service, sdk) = setup();
                let runtime = host.cloud_runtime().unwrap();
                let binding = runtime.status().unwrap().binding.unwrap();
                let zone = cloud::adapter::collection_zone(binding.facts.epoch);
                let full_active = control(&mut service, "meguri-root", "active", RequestedFields::All);
                let bad_active = control(&mut service, "meguri-root", "active", RequestedFields::Named(Vec::new()));
                let bad_epoch = control(&mut service, &zone, "epoch", RequestedFields::Named(Vec::new()));
                if marked {
                    if name == "active" {
                        runtime.adapter_fetched(&sdk, "meguri-root", &[saved("active", &bad_active)], &[]).unwrap();
                    } else {
                        runtime.adapter_fetched(&sdk, &zone, &[saved("epoch", &bad_epoch)], &[]).unwrap();
                    }
                }
                let record = if name == "active" {
                    saved(name, if marked { &full_active } else { &bad_active })
                } else if marked {
                    // A changed complete body would also update qualified crt
                    // if physical admission were accidentally omitted.
                    super::epoch_crt_knowledge::save_complete_epoch(&mut service, &zone, |e| e.crt += 1)
                } else {
                    saved(name, &bad_epoch)
                };
                runtime.adapter_state_update(&sdk, b"synthetic-sdk-refusal-cursor").unwrap();
                let before = runtime.adapter_snapshot().unwrap();
                let mark = runtime.root_unreadable_for_testing().unwrap();
                assert_eq!(mark.is_some(), marked);
                let crt = runtime.epoch_crt_for_testing().unwrap();
                let live = dir.join("collection.anki2");
                let file_before = NativeFiles.identity(&live).unwrap();
                let original = dir.join("sdk-intake-original");
                let sidecar = dir.join("meguri-sidecar.json");
                let sidecar_before = std::fs::read(&sidecar).unwrap();
                let replacement = if change == "publication" { Some(restored_sidecar(&dir)) } else { None };
                let replace = {
                    let live = live.clone();
                    let original = original.clone();
                    let sidecar = sidecar.clone();
                    move || {
                        if let Some(bytes) = &replacement {
                            std::fs::write(&sidecar, bytes).unwrap();
                        } else {
                            let copy = live.with_extension("sdk-intake-copy");
                            std::fs::copy(&live, &copy).unwrap();
                            std::fs::rename(&live, &original).unwrap();
                            std::fs::rename(copy, &live).unwrap();
                        }
                    }
                };
                let reached = Arc::new(AtomicBool::new(false));
                if name == "epoch" {
                    // Replace after the original SDK ScanGuard has been pinned.
                    let observed = reached.clone();
                    runtime.set_incoming_stage_hook_for_testing(move |stage| {
                        if stage == cloud::testing::IncomingStage::FetchBeforeInbox && !observed.swap(true, Ordering::SeqCst) {
                            replace();
                        }
                    });
                } else {
                    replace();
                    reached.store(true, Ordering::SeqCst);
                }
                // Cached SDK validity deliberately survives the external change.
                runtime.validate_sdk(&sdk).unwrap();
                let deleted = if name == "epoch" { vec!["synthetic-sdk-inbox-deletion".into()] } else { Vec::new() };
                assert!(runtime.adapter_fetched(&sdk, if name == "active" { "meguri-root" } else { &zone }, &[record], &deleted).is_err());
                assert!(reached.load(Ordering::SeqCst));
                assert_eq!(runtime.root_unreadable_for_testing().unwrap(), mark);
                assert_eq!(runtime.epoch_crt_for_testing().unwrap(), crt);
                let after = runtime.adapter_snapshot().unwrap();
                assert_eq!(after.serialization, before.serialization);
                assert_eq!(serde_json::to_vec(&after.inbox).unwrap(), serde_json::to_vec(&before.inbox).unwrap());
                assert_eq!(after.root, before.root);
                if change == "file" {
                    assert_ne!(NativeFiles.identity(&live).unwrap(), file_before);
                    std::fs::remove_file(&live).unwrap();
                    std::fs::rename(original, live).unwrap();
                } else {
                    assert_eq!(NativeFiles.identity(&live).unwrap(), file_before);
                    assert_ne!(decision(&host).publication.identity, binding.facts.publication);
                    std::fs::write(sidecar, sidecar_before).unwrap();
                }
                host.close().unwrap();
            }
        }
    }
}
