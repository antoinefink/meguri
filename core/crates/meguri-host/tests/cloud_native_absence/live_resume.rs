//! S2.1/S2.2 on the existing real provider/file/Engine/Service fixture.
use super::*;
use cloud::runtime::{FileIdentityProvider, NativeFileIdentity, RuntimeError};
use meguri_engine::{GenerationListener, SuspendParticipant};
use meguri_model::clock::{StepClock, UtcOffset};
use std::sync::{
    Mutex,
    atomic::{AtomicUsize, Ordering},
};

type FileReadHook = Box<dyn FnOnce() + Send>;
#[derive(Default)]
struct Files {
    hook: Mutex<Option<FileReadHook>>,
    // Zero delegates unchanged to the real path provider; 1..4 change each
    // native component independently; 5 is an actual unavailable observation.
    fault: AtomicUsize,
}
impl FileIdentityProvider for Files {
    fn identity(&self, path: &Path) -> Result<NativeFileIdentity, RuntimeError> {
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        let mut file = NativeFiles.identity(path)?;
        match self.fault.load(Ordering::SeqCst) {
            1 => file.device ^= 1,
            2 => file.inode ^= 1,
            3 => file.created_seconds += 1,
            4 => file.created_nanos = (file.created_nanos + 1) % 1_000_000_000,
            5 => return Err(RuntimeError::FileIdentityUnavailable),
            _ => {}
        }
        Ok(file)
    }
}
#[derive(Default)]
struct CountedAccount(AtomicUsize);
impl IdentityProvider for CountedAccount {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.fetch_add(1, Ordering::SeqCst);
        identity()
    }
}
fn live_fixture() -> (Rig, Arc<Files>, Arc<CountedAccount>, Arc<StepClock>) {
    let mut rig = fixture(3, false);
    rig.host.close().unwrap();
    let directory = rig.root.path().join("device");
    let files = Arc::new(Files::default());
    let account = Arc::new(CountedAccount::default());
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let mut config = options(&directory, NOW, 1717);
    config.clock = clock.clone();
    config.publication_binding = AccountBinding::Connected {
        account: scope().account,
        configuration: scope().container,
        environment: scope().environment,
    };
    config.cloud = Some(CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope().container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity: account.clone(),
        files: files.clone(),
    });
    rig.host = CoreHost::open(config).unwrap();
    rig.sdk = admit(&rig.host).0;
    confirm_root(&rig.host, &rig.sdk, &mut rig.service);
    (rig, files, account, clock)
}
fn cleanup_original(runtime: &Runtime, sdk: &str) {
    runtime.retire_sdk(sdk, false).unwrap();
    runtime.release_sdk(sdk).unwrap();
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
}
fn assert_local_write(host: &CoreHost) {
    let before = rows(host);
    let Cell::Integer(id) = before[&Table::Notes][0][0] else {
        panic!("fixture note required")
    };
    let fields = vec!["resumed local question".to_owned(), "resumed local answer".to_owned()];
    meguri_engine::notes::update(
        &host.collection().unwrap(),
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(id),
            fields: fields.clone(),
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    let after = rows(host);
    let note = after[&Table::Notes]
        .iter()
        .find(|row| row[0] == Cell::Integer(id))
        .unwrap();
    assert_eq!(note[6], Cell::Text(fields.join("\u{1f}").into_bytes()));
    assert_ne!(after[&Table::Notes], before[&Table::Notes]);
}
fn assert_process_refusal(runtime: &Runtime, sdk: &str, binding: &cloud::Binding, open: u64) {
    let status = runtime.status().unwrap();
    assert_eq!(status.binding.as_ref(), Some(binding));
    assert_eq!(status.binding.unwrap().facts.state, cloud::BindingState::Active);
    assert!(!status.stored_usable && !status.identity_verified);
    assert!(status.scan_required && status.tracking_stopped);
    assert_eq!(runtime.stored_state_for_testing().unwrap().open_counter, open);
    assert!(matches!(
        runtime.validate_native_sdk_for_testing(sdk),
        Err(RuntimeError::StaleIncarnation)
    ));
    // Exact original cleanup remains admitted, without any successor authority.
    cleanup_original(runtime, sdk);
}

#[test]
fn active_resume_preserves_original_fetch_result_and_root_and_does_not_read_account_or_start_threads() {
    let (mut rig, _, account, _) = live_fixture();
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let guard = runtime.adapter_guard();
    let stored = runtime.stored_state_for_testing().unwrap();
    let root = runtime.adapter_snapshot().unwrap().root;
    let reads = account.0.load(Ordering::SeqCst);
    let threads = rig.host.engine().threads().threads().len();
    let id = begin(runtime, &rig.sdk, "meguri-root");
    // The original SDK fetch and its real Service response stay outstanding.
    let reply = page(&mut rig.service, "meguri-root", None);
    assert!(!reply.more);
    let report = rig.host.engine().resume().unwrap();
    assert!(!report.external_change && !report.rolled_over);
    assert_eq!(runtime.adapter_guard(), guard);
    assert_eq!(runtime.stored_state_for_testing().unwrap(), stored);
    assert_eq!(runtime.adapter_snapshot().unwrap().root, root);
    runtime.validate_native_sdk_for_testing(&rig.sdk).unwrap();
    assert_eq!(account.0.load(Ordering::SeqCst), reads);
    assert_eq!(rig.host.engine().threads().threads().len(), threads);
    let records = reply
        .changes
        .iter()
        .map(|r| match r {
            RecordChange::Saved(r) => saved(r),
            _ => panic!("real root record"),
        })
        .collect::<Vec<_>>();
    runtime.adapter_fetched(&rig.sdk, "meguri-root", &records, &[]).unwrap();
    finish(runtime, &rig.sdk, "meguri-root", id, false);
    assert!(runtime.adapter_finish_fetch(&rig.sdk, id).is_err());
    assert_eq!(collection.generation().0, guard.generation);
    runtime.validate_native_sdk_for_testing(&rig.sdk).unwrap();
    cleanup_original(runtime, &rig.sdk);
    rig.host.close().unwrap();
}

#[test]
fn drained_suspend_rebinds_but_active_missing_unverified_paused_and_stale_sdk_never_repair() {
    for case in 0..8 {
        let (rig, _, account, _) = live_fixture();
        let runtime = rig.host.cloud_runtime().unwrap();
        let collection = rig.host.collection().unwrap();
        let stored = runtime.stored_state_for_testing().unwrap();
        let original_instance = runtime.instance().unwrap();
        match case {
            0 => {
                rig.host.engine().suspend(BUDGET).unwrap();
            }
            1 => {
                runtime.release_connections_until(BUDGET).unwrap(); // no prior suspend
            }
            2 => {
                runtime.collection_replaced(
                    collection.generation(),
                    meguri_engine::Generation(collection.generation().0 + 1),
                );
            }
            3 => {
                // A real provider read removes current-process verification.
                runtime
                    .adapter_account(None, IdentityStatus::TemporarilyUnavailable)
                    .unwrap();
            }
            4 => {
                runtime.adapter_pause(true).unwrap();
            }
            5 => {
                runtime.adapter_pause(true).unwrap();
                runtime.adapter_pause(false).unwrap();
                let first = runtime.adapter_guard();
                assert_eq!(first, runtime.adapter_guard());
                assert!(runtime.validate_native_sdk_for_testing(&rig.sdk).is_err());
            }
            6 => {
                runtime.suspend().unwrap();
            } // unbound, but connections not released
            7 => {
                runtime.retire_sdk(&rig.sdk, false).unwrap();
            }
            _ => unreachable!(),
        }
        let prior_refusal = runtime
            .validate_native_sdk_for_testing(&rig.sdk)
            .err()
            .map(|error| std::mem::discriminant(&error));
        let guard = runtime.adapter_guard();
        let reads = account.0.load(Ordering::SeqCst);
        let result = rig.host.engine().resume();
        if case == 0 {
            result.unwrap();
            assert!(runtime.stored_state_for_testing().unwrap().open_counter > stored.open_counter);
            assert!(!runtime.status().unwrap().identity_verified);
            assert_ne!(runtime.instance().unwrap(), original_instance);
        } else {
            result.unwrap();
            if case != 1 {
                assert_eq!(runtime.adapter_guard(), guard);
            }
            if case != 1 && case != 2 && case != 6 {
                assert_eq!(
                    runtime.stored_state_for_testing().unwrap().open_counter,
                    stored.open_counter
                );
            }
            let refusal = runtime
                .validate_native_sdk_for_testing(&rig.sdk)
                .err()
                .map(|error| std::mem::discriminant(&error));
            assert!(refusal.is_some());
            if case != 1 {
                assert_eq!(refusal, prior_refusal);
            }
            assert_local_write(&rig.host);
        }
        assert_eq!(account.0.load(Ordering::SeqCst), reads);
        cleanup_original(runtime, &rig.sdk);
        if case == 1 {
            rig.host.engine().resume().unwrap();
            runtime.collection_replaced(
                collection.generation(),
                meguri_engine::Generation(collection.generation().0 + 1),
            );
        }
        rig.host.close().unwrap();
    }
}

#[test]
fn active_resume_preserves_recorded_publication_and_held_original_own_request_and_result() {
    use cloud::publication::{LocalWork, OwnKind, OwnResult, PublicationWork};
    use meguri_testkit::cloud::coordinator::execute_publication_request;
    use sign_out_absence as actual;

    let root = tempfile::tempdir().unwrap();
    let directory = root.path().join("device");
    actual::generated(&directory);
    let account = Arc::new(CountedAccount::default());
    let host = actual::open_actual(&directory, account.clone());
    let mut service = service(root.path());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let preview = actual::actual_preview(&host, &mut service);
    let op = runtime.confirm_connection_publication(&preview, &collection).unwrap();
    for expected in [LocalWork::Bind, LocalWork::WriteDecision] {
        assert!(matches!(runtime.next_publication_work(&collection).unwrap(),
            PublicationWork::Local { op: id, work } if id == op && work == expected));
        runtime.perform_publication_local(op, expected, &collection).unwrap();
    }
    let guard = runtime.adapter_guard();
    let stored = runtime.stored_state_for_testing().unwrap();
    let publication = runtime.publication_op().unwrap();
    let assert_retained = || {
        assert_eq!(runtime.adapter_guard(), guard);
        assert_eq!(runtime.stored_state_for_testing().unwrap(), stored);
        assert_eq!(runtime.publication_op().unwrap(), publication);
        let status = runtime.status().unwrap();
        assert!(status.stored_usable && status.identity_verified && !status.tracking_stopped);
    };
    assert_retained();
    let reads = account.0.load(Ordering::SeqCst);
    let calls = service.request_log().unwrap().len();
    host.engine().resume().unwrap();
    assert_retained();
    assert_eq!(account.0.load(Ordering::SeqCst), reads);
    assert_eq!(service.request_log().unwrap().len(), calls);

    let PublicationWork::Own(request) = runtime.next_publication_work(&collection).unwrap() else {
        panic!("production own request required")
    };
    assert_eq!(request.op, op);
    assert_eq!(request.kind, OwnKind::CreateZones);
    let reads = account.0.load(Ordering::SeqCst);
    host.engine().resume().unwrap();
    assert_retained();
    assert_eq!(account.0.load(Ordering::SeqCst), reads);
    assert!(matches!(
        runtime.next_publication_work(&collection).unwrap(),
        PublicationWork::Idle
    ));
    let (result, replies) =
        execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(result, OwnResult::ZonesCreated));
    assert_eq!(replies.len(), 3);
    assert!(replies.iter().all(|reply| matches!(&reply.result, Ok(Reply::Done))));
    let reads = account.0.load(Ordering::SeqCst);
    let calls = service.request_log().unwrap().len();
    host.engine().resume().unwrap();
    assert_retained();
    assert_eq!(account.0.load(Ordering::SeqCst), reads);
    assert_eq!(service.request_log().unwrap().len(), calls);
    runtime
        .publication_result(&request, result.clone(), &collection)
        .unwrap();
    assert!(runtime.publication_result(&request, result, &collection).is_err());
    let PublicationWork::Own(next) = runtime.next_publication_work(&collection).unwrap() else {
        panic!("original request must complete")
    };
    assert_eq!(next.op, op);
    assert_eq!(next.kind, OwnKind::SaveMediaRoot);
    assert_eq!(next.attempt, request.attempt + 1);
    let (result, _) = execute_publication_request(&mut service, &scope(), &next, runtime, &collection).unwrap();
    assert!(matches!(result, OwnResult::Saved(_)));
    runtime.publication_result(&next, result, &collection).unwrap();
    host.close().unwrap();
}

#[test]
fn active_native_and_descriptor_observation_failures_retain_binding_without_durable_isolation() {
    for fault in 1..=6 {
        let (rig, files, account, _) = live_fixture();
        let runtime = rig.host.cloud_runtime().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap();
        let open = runtime.stored_state_for_testing().unwrap().open_counter;
        if fault == 6 {
            runtime.forget_live_descriptor_for_testing();
        } else {
            files.fault.store(fault, Ordering::SeqCst);
        }
        let reads = account.0.load(Ordering::SeqCst);
        rig.host.engine().resume().unwrap();
        assert_local_write(&rig.host);
        assert_process_refusal(runtime, &rig.sdk, &binding, open);
        assert_eq!(account.0.load(Ordering::SeqCst), reads);
        files.fault.store(0, Ordering::SeqCst);
        // Retry cannot bind or repair the process refusal.
        let refused = runtime.adapter_guard();
        rig.host.engine().resume().unwrap();
        assert_eq!(runtime.adapter_guard(), refused);
        assert!(!runtime.status().unwrap().identity_verified);
        rig.host.close().unwrap();
    }
}

#[test]
fn active_refusal_retains_accepted_asset_read_until_original_terminal_cleanup() {
    use cloud::{incoming::AssetReadKind, workflow::FetchedRecord};
    use meguri_testkit::cloud::{Asset, SystemFields};
    use sign_out_absence as actual;

    let root = tempfile::tempdir().unwrap();
    let sender_dir = root.path().join("sender");
    actual::generated(&sender_dir);
    let sender = actual::open_actual(&sender_dir, Arc::new(CountedAccount::default()));
    let mut service = Service::open(
        root.path(),
        Config {
            page_size: 32,
            ..Config::default()
        },
        SeededRandom::new(1717),
    )
    .unwrap();
    actual::publish(&sender, &mut service);
    let receiver = actual::adopt(&root.path().join("receiver"), &mut service);
    let (sender_sdk, _, zone) = admit(&sender);
    confirm_root(&sender, &sender_sdk, &mut service);
    let (sdk, _, _) = admit(&receiver);
    confirm_root(&receiver, &sdk, &mut service);
    let collection = sender.collection().unwrap();
    let Cell::Integer(note) = rows(&sender)[&Table::Notes][0][0] else {
        panic!("fixture note required")
    };
    meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note),
            fields: vec![
                "r".repeat(codec::INLINE_ENVELOPE_BYTES + 1),
                "remote asset answer".into(),
            ],
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    let sending = sender.cloud_runtime().unwrap();
    let batch = sending
        .build_collection_batch(&sender_sdk, &collection, &actual::names(sending))
        .unwrap();
    let asset_save = batch.saves.iter().find(|save| save.asset_path.is_some()).unwrap();
    let changes = batch
        .saves
        .iter()
        .map(|save| {
            let mut record = RecordInput::encrypted(&save.name, "MGEntity", save.envelope.clone());
            if let Some(path) = &save.asset_path {
                record.fields.insert(
                    "pa".into(),
                    Field::Asset(Asset::synthetic(std::fs::read(path).unwrap())),
                );
            }
            Mutation::Save {
                record,
                if_unchanged: save.system_fields.clone().map(SystemFields::from_bytes),
            }
        })
        .collect();
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: zone.clone(),
                atomic: false,
                changes,
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("real asset save receipts required")
    };
    let receipts = items
        .into_iter()
        .map(|item| {
            let Ok(MutationSuccess::Saved(record)) = item.result else {
                panic!("accepted asset save required")
            };
            saved(&record)
        })
        .collect::<Vec<_>>();
    sending.enter_sent_handler(&sender_sdk, batch.id).unwrap();
    sending
        .sent_results(&sender_sdk, &collection, batch.id, &receipts, &[], batch.saves.len())
        .unwrap();
    sending.return_sent_handler(&sender_sdk).unwrap();

    let runtime = receiver.cloud_runtime().unwrap();
    let collection = receiver.collection().unwrap();
    let remote = read(&mut service, &zone, &asset_save.name);
    runtime.adapter_fetched(&sdk, &zone, &[saved(&remote)], &[]).unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let waiting = runtime.inbox_entries(binding.id).unwrap();
    assert!(
        waiting
            .iter()
            .any(|entry| entry.entry.reason == cloud::InboxReason::Asset)
    );
    let pass = runtime
        .begin_asset_read_pass(&sdk, &collection, AssetReadKind::Retry)
        .unwrap();
    let request = runtime.next_asset_read(&collection, &pass).unwrap().unwrap();
    assert_eq!(request.record_name(), asset_save.name);
    let mut held = None;
    assert!(
        runtime
            .submit_asset_read_queue(&collection, &request, || {
                held = Some(read(&mut service, &zone, request.record_name()));
                true
            })
            .unwrap()
    );
    let waiting = runtime.inbox_entries(binding.id).unwrap();
    assert!(runtime.asset_read_owned_for_testing(&request));
    assert!(!runtime.cancel_asset_read(&request));
    let remote = held.unwrap();
    let Field::Asset(asset) = &remote.fields["pa"] else {
        panic!("Service asset required")
    };
    let mut downloaded = tempfile::NamedTempFile::new().unwrap();
    downloaded.write_all(asset.bytes()).unwrap();
    downloaded.as_file().sync_all().unwrap();
    let result = FetchedRecord {
        record: saved(&remote),
        asset_path: Some(downloaded.path().to_owned()),
    };
    let open = runtime.stored_state_for_testing().unwrap().open_counter;
    runtime.forget_live_descriptor_for_testing();
    receiver.engine().resume().unwrap();
    assert_local_write(&receiver);
    assert!(runtime.asset_read_owned_for_testing(&request));
    assert!(runtime.validate_native_sdk_for_testing(&sdk).is_err());
    assert!(
        runtime
            .asset_read_result(&sdk, &collection, &request, Some(&result))
            .is_err()
    );
    assert!(!runtime.asset_read_owned_for_testing(&request));
    assert_eq!(runtime.asset_read_owned_count_for_testing(), 0);
    runtime.cancel_asset_read_pass(&pass);
    assert_eq!(runtime.asset_read_pass_count_for_testing(), 0);
    assert_eq!(runtime.inbox_entries(binding.id).unwrap(), waiting);
    assert_eq!(std::fs::read(downloaded.path()).unwrap(), asset.bytes());
    downloaded.close().unwrap();
    assert_process_refusal(runtime, &sdk, &binding, open);
    cleanup_original(sending, &sender_sdk);
    receiver.close().unwrap();
    sender.close().unwrap();
}

#[test]
fn active_resume_final_original_owner_check_rejects_concurrent_pause_with_stable_fresh_guards() {
    let (rig, files, _, _) = live_fixture();
    let runtime = rig.host.cloud_runtime().unwrap();
    let weak = Arc::downgrade(runtime);
    *files.hook.lock().unwrap() = Some(Box::new(move || {
        let runtime = weak.upgrade().unwrap();
        runtime.adapter_pause(true).unwrap();
        runtime.adapter_pause(false).unwrap();
        assert_eq!(runtime.adapter_guard(), runtime.adapter_guard());
    }));
    let stored = runtime.stored_state_for_testing().unwrap();
    rig.host.engine().resume().unwrap();
    assert_eq!(runtime.stored_state_for_testing().unwrap(), stored);
    assert!(runtime.validate_native_sdk_for_testing(&rig.sdk).is_err());
    assert_local_write(&rig.host);
    let refused = runtime.adapter_guard();
    rig.host.engine().resume().unwrap();
    assert_eq!(runtime.adapter_guard(), refused);
    cleanup_original(runtime, &rig.sdk);
    rig.host.close().unwrap();
}

#[test]
fn active_external_same_file_edit_and_rollover_still_invalidate_original_sdk_and_require_scan() {
    let (rig, _, _, clock) = live_fixture();
    let runtime = rig.host.cloud_runtime().unwrap();
    let collection = rig.host.collection().unwrap();
    let guard = runtime.adapter_guard();
    let native = NativeFiles.identity(collection.path()).unwrap();
    let day = collection.check_rollover().unwrap().next_day_at;
    let db = meguri_storage::raw::PlainDatabase::open(collection.path()).unwrap();
    db.execute("UPDATE col SET mod = mod + 99", &[]).unwrap();
    drop(db);
    clock.set_now_ms((day + 1) * 1000);
    let report = rig.host.engine().resume().unwrap();
    assert!(report.external_change && report.rolled_over && report.next_day_at > day);
    assert_eq!(NativeFiles.identity(collection.path()).unwrap(), native);
    assert_ne!(runtime.adapter_guard(), guard);
    assert!(runtime.status().unwrap().scan_required);
    assert!(runtime.validate_native_sdk_for_testing(&rig.sdk).is_err());
    cleanup_original(runtime, &rig.sdk);
    rig.host.close().unwrap();
}

#[test]
fn active_sidecar_fences_and_failed_reads_deny_process_authority_without_inventing_durable_reason() {
    for fault in 0..8 {
        let (rig, files, _, _) = live_fixture();
        let runtime = rig.host.cloud_runtime().unwrap();
        let binding = runtime.status().unwrap().binding.unwrap();
        let open = runtime.stored_state_for_testing().unwrap().open_counter;
        let sidecar = rig
            .host
            .collection()
            .unwrap()
            .path()
            .with_file_name("meguri-sidecar.json");
        let original = meguri_storage::sidecar::read_document(&sidecar, |d| Ok(d.unwrap().clone())).unwrap();
        let path = sidecar.clone();
        // Change after Engine's normal sidecar reconcile, at the real external
        // path-provider boundary. Runtime must do its own later settled read.
        *files.hook.lock().unwrap() = Some(Box::new(move || {
            meguri_storage::sidecar::update_document(&path, |d| {
                let state = &mut d["deviceState"];
                match fault {
                    0 => {
                        let word = state["active"]["identity"][0].as_u64().unwrap();
                        state["active"]["identity"][0] = serde_json::json!(word ^ 1);
                    }
                    1 => state["active"]["decision"] = serde_json::json!("Pending"),
                    2 => state["active"]["binding"] = serde_json::json!("LocalOnly"),
                    3 => state["active"]["binding"]["Connected"]["account"] = serde_json::json!("other"),
                    4 => state["active"]["binding"]["Connected"]["configuration"] = serde_json::json!("other"),
                    5 => state["active"]["binding"]["Connected"]["environment"] = serde_json::json!("production"),
                    6 => state["intent"] = serde_json::json!({}),
                    7 => *state = serde_json::json!("unreadable-observation"),
                    _ => unreachable!(),
                }
                Ok(())
            })
            .unwrap();
        }));
        rig.host.engine().resume().unwrap();
        assert_process_refusal(runtime, &rig.sdk, &binding, open);
        // Observe the actual stored binding while its original Runtime is held.
        // It remains Active: observation failures invented no isolation reason.
        let stored = cloud::runtime::with_tracking_access(|| {
            let store =
                match cloud::TrackingStore::open(&rig.root.path().join("device"), binding.scope.clone()).unwrap() {
                    cloud::OpenOutcome::Opened(s) | cloud::OpenOutcome::Missing(s) => s,
                    _ => panic!("original supported store"),
                };
            store.binding(binding.id).unwrap().unwrap()
        });
        assert_eq!(stored, binding);
        meguri_storage::sidecar::update_document(&sidecar, |d| {
            *d = original;
            Ok(())
        })
        .unwrap();
        assert_local_write(&rig.host);
        rig.host.engine().resume().unwrap();
        assert!(!runtime.status().unwrap().identity_verified);
        rig.host.close().unwrap();
    }
}
