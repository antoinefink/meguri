//! Test-only: reuses sync_tests::Fixture's actual Host; transport replies for
//! publication prefixes come from the existing Service/coordinator helper.
use super::*;
use crate::cloud_connection::*;
use meguri_sync::cloud::{control_codec, publication as p};
use meguri_testkit::cloud::{
    Caller, Config, Mutation, MutationSuccess, Operation, RecordInput, Reply, Scope, Service, SystemFields,
    coordinator::{execute_publication_request, publication_control_record},
};

fn keys() -> Arc<Keys> {
    Keys::new([Some(vec![31; 32]), Some(vec![47; 32])])
}
fn ready(connection: &CloudConnection) -> Arc<CloudConnectionPreview> {
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("actual record read required")
    };
    let CloudConnectionPreviewWork::Ready { preview, summary } = connection
        .connection_active_read(preview, CloudControlRead::ZoneMissing)
        .unwrap()
    else {
        panic!("actual confirmation required")
    };
    assert!(summary.manifest_counts.is_none());
    assert!(matches!(summary.decision, CloudConnectionDecision::ConfirmUpload));
    preview
}
fn own(connection: &CloudConnection) -> (Arc<CloudPublicationRequest>, CloudOwnOperation) {
    for _ in 0..8 {
        match connection.next_publication_work().unwrap() {
            CloudPublicationWork::Local { handle, .. } => connection.perform_local(handle).unwrap(),
            CloudPublicationWork::Own { request, command, .. } => return (request, command),
            _ => panic!("real publication must reach an own request"),
        }
    }
    panic!("bounded local transitions");
}
struct Queue(AtomicUsize);
impl CloudPublicationQueue for Queue {
    fn enqueue(&self) -> bool {
        // The exported entry guard must apply to this foreign callback too.
        #[cfg(debug_assertions)]
        assert!(std::panic::catch_unwind(crate::core_version).is_err());
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
fn failure_is<T>(result: Result<T, CoreError>, expected: CloudConnectionFailure) {
    assert!(matches!(result, Err(CoreError::CloudConnection { failure }) if failure == expected));
}
fn control(v: p::ControlRecord) -> CloudControlRecord {
    CloudControlRecord {
        encrypted: v.encrypted,
        system_fields: v.system_fields.0,
    }
}
fn result(v: p::OwnResult) -> CloudPublicationResult {
    match v {
        p::OwnResult::ZonesCreated => CloudPublicationResult::ZonesCreated,
        p::OwnResult::Saved(r) => CloudPublicationResult::Saved { record: control(r) },
        p::OwnResult::Conflict(r) => CloudPublicationResult::Conflict { record: control(r) },
        p::OwnResult::Read(r) => CloudPublicationResult::Read {
            read: match r {
                p::RecordRead::Found(r) => CloudControlRead::Found { record: control(r) },
                p::RecordRead::Missing => CloudControlRead::Missing,
                p::RecordRead::ZoneMissing => CloudControlRead::ZoneMissing,
                p::RecordRead::Failed => CloudControlRead::Failed,
            },
        },
        p::OwnResult::Failed { retry_after_ms } => CloudPublicationResult::Failed { retry_after_ms },
        p::OwnResult::NotStarted => CloudPublicationResult::NotStarted,
    }
}

#[test]
fn bridge_preview_confirm_consumes_actual_preview_and_exposes_no_manifest_as_local_counts() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let preview = ready(&connection);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    let operation = connection.confirm_publication(preview.clone()).unwrap();
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
    failure_is(
        connection.confirm_publication(preview),
        CloudConnectionFailure::Consumed,
    );
    assert!(matches!(
        connection.recovery_status().unwrap().unwrap().step,
        CloudRecoveryStep::Confirmed
    ));
    connection.cancel_operation(operation).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
}
#[test]
fn bridge_stale_confirmation_wrong_owner_and_old_preview_cancel_never_replace_new_authority() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let other = fixture.workflow.connection().unwrap();
    let old = ready(&connection);
    let current = ready(&connection);
    failure_is(
        other.confirm_publication(current.clone()),
        CloudConnectionFailure::WrongOwner,
    );
    failure_is(connection.cancel_preview(old), CloudConnectionFailure::AuthorityChanged);
    let stale = ready(&connection);
    let latest = ready(&connection);
    failure_is(
        connection.confirm_publication(stale),
        CloudConnectionFailure::AuthorityChanged,
    );
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    connection.confirm_publication(latest).unwrap();
}
#[test]
fn bridge_guarded_enqueue_is_zero_when_paused_and_one_after_actual_acceptance() {
    let fixture = Fixture::new(keys());
    let connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let (request, command) = own(&connection);
    assert!(matches!(command, CloudOwnOperation::CreateZones { ref zones } if zones.len() == 3));
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    fixture.workflow.set_paused(true).unwrap();
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .is_err()
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    connection
        .publication_result(request, CloudPublicationResult::NotStarted)
        .unwrap();
    fixture.workflow.set_paused(false).unwrap();
    connection.trigger().unwrap();
    let (request, _) = own(&connection);
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .unwrap()
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }),
        CloudConnectionFailure::Closed,
    );
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
}
#[test]
fn bridge_close_rejects_held_confirmation_before_entropy_or_successor_host_access() {
    let keys = keys();
    let fixture = Fixture::new(keys.clone());
    let connection = fixture.workflow.connection().unwrap();
    let preview = ready(&connection);
    fixture.workflow.host.close().unwrap();
    failure_is(connection.confirm_publication(preview), CloudConnectionFailure::Closed);
    failure_is(connection.next_publication_work(), CloudConnectionFailure::Closed);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
}
#[test]
fn bridge_accepted_active_with_lost_reply_retains_actual_durable_switch_attempt() {
    let fixture = Fixture::new(keys());
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .register_full(|_, _| true)
        .unwrap();
    let connection = fixture.workflow.connection().unwrap();
    connection.confirm_publication(ready(&connection)).unwrap();
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let run = tempfile::tempdir().unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(801)).unwrap();
    let mut sdk: Option<CloudEngineRegistration> = None;
    let mut held = None;
    for _ in 0..64 {
        match connection.next_publication_work().unwrap() {
            CloudPublicationWork::Local { handle, .. } | CloudPublicationWork::Scan { handle } => {
                connection.perform_local(handle).unwrap()
            }
            CloudPublicationWork::Own { request, .. } => {
                if request.retained_for_test().kind == p::OwnKind::SaveActive {
                    held = Some(request);
                    break;
                }
                let (reply, _) = execute_publication_request(
                    &mut service,
                    &scope,
                    request.retained_for_test(),
                    &fixture.workflow.core,
                    &fixture.workflow.host.collection().unwrap(),
                )
                .unwrap();
                connection.publication_result(request, result(reply)).unwrap();
            }
            CloudPublicationWork::SendEntities => {
                if sdk.is_none() {
                    sdk = Some(fixture.workflow.register_sdk(false).unwrap());
                }
                let sdk = sdk.as_ref().unwrap();
                let pending = fixture.workflow.tracking_sendables().unwrap();
                let batch = fixture
                    .workflow
                    .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
                    .unwrap()
                    .expect("actual entity batch");
                let count = batch.saves.len() as u32;
                let changes = batch
                    .saves
                    .into_iter()
                    .map(|r| Mutation::Save {
                        record: RecordInput::encrypted(&r.name, "MGEntity", r.envelope),
                        if_unchanged: r.system_fields.map(SystemFields::from_bytes),
                    })
                    .collect();
                let response = service
                    .request(
                        &scope,
                        Caller::default(),
                        Operation::Modify {
                            zone: sdk.collection_zone.clone(),
                            atomic: false,
                            changes,
                        },
                    )
                    .unwrap();
                let Reply::Modified(items) = response.result.unwrap() else {
                    panic!("actual service save")
                };
                let saved = items
                    .into_iter()
                    .map(|item| {
                        let MutationSuccess::Saved(record) = item.result.unwrap() else {
                            panic!("actual saved entity")
                        };
                        let r = publication_control_record(&record).unwrap();
                        CloudRecord {
                            name: record.name,
                            envelope: r.encrypted.unwrap(),
                            system_fields: Some(r.system_fields.0),
                        }
                    })
                    .collect();
                fixture
                    .workflow
                    .sent(sdk.instance_id.clone(), batch.id, saved, vec![], count)
                    .unwrap();
                fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
            }
            _ => panic!("publication failed before active"),
        }
    }
    let request = held.expect("real complete epoch must yield an active save");
    let before = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(!before.switch_sent && before.active_attempt.is_none());
    let queue = Arc::new(Queue(AtomicUsize::new(0)));
    assert!(
        connection
            .submit_publication_request(request.clone(), queue.clone())
            .unwrap()
    );
    let submitted = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(submitted.switch_sent && submitted.active_attempt.is_some());
    assert_eq!(queue.0.load(Ordering::SeqCst), 1);
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.publication_result(request, CloudPublicationResult::Failed { retry_after_ms: None }),
        CloudConnectionFailure::Closed,
    );
    let c::OpenOutcome::Opened(store) = c::TrackingStore::open(
        &fixture._directory.path().join("collection"),
        c::BuildScope {
            container: c::ContainerId::new("synthetic-u114").unwrap(),
            environment: c::Environment::Development,
        },
    )
    .unwrap() else {
        panic!("actual durable store")
    };
    assert!(store.publication_op().unwrap() == Some(submitted));
}
#[test]
fn bridge_found_active_without_e_is_unreadable_and_missing_is_an_upload_offer() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("active read")
    };
    let CloudConnectionPreviewWork::Ready { summary, .. } = connection
        .connection_active_read(
            preview,
            CloudControlRead::Found {
                record: CloudControlRecord {
                    encrypted: None,
                    system_fields: vec![9],
                },
            },
        )
        .unwrap()
    else {
        panic!("unreadable preview")
    };
    assert!(matches!(summary.decision, CloudConnectionDecision::CloudUnreadable));
    let preview = ready(&connection);
    connection.cancel_preview(preview).unwrap();
}
fn adoption_preview(connection: &CloudConnection) -> (Arc<CloudConnectionPreview>, CloudControlRecord) {
    let active = CloudControlRecord {
        encrypted: Some(
            control_codec::Active {
                epoch: [8; 16],
                activated_at_ms: NOW,
                previous: None,
                replaced_through_ms: None,
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![1],
    };
    let epoch = CloudControlRecord {
        encrypted: Some(
            control_codec::Epoch {
                epoch_id: [8; 16],
                record_key: [19; 32],
                crt: NOW / 1000,
                ver: 18,
                state: control_codec::EpochState::Complete,
                reason: control_codec::EpochReason::Initial,
                published_at_ms: NOW,
                counts: vec![],
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![2],
    };
    let CloudConnectionPreviewWork::ReadActive { preview, .. } = connection.begin_connection_preview().unwrap() else {
        panic!("active")
    };
    let CloudConnectionPreviewWork::ReadEpoch { preview, .. } = connection
        .connection_active_read(preview, CloudControlRead::Found { record: active.clone() })
        .unwrap()
    else {
        panic!("epoch")
    };
    let CloudConnectionPreviewWork::Ready { preview, summary } = connection
        .connection_epoch_read(preview, CloudControlRead::Found { record: epoch })
        .unwrap()
    else {
        panic!("ready")
    };
    assert_eq!(summary.published_at_ms, Some(NOW));
    assert!(summary.manifest_counts.is_some());
    (preview, active)
}
#[test]
fn bridge_adoption_confirmation_takes_actual_host_slot_and_exact_cancel_releases_it() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    let operation = connection.confirm_adoption(preview).unwrap();
    assert_eq!(
        fixture.workflow.host.engine().slot_holder(),
        Some(meguri_engine::LongOp::CloudAdopt)
    );
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption work")
    };
    assert!(matches!(action, CloudAdoptionAction::Read { .. }));
    connection
        .adoption_result(
            handle.clone(),
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        )
        .unwrap();
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Missing
                }
            )
            .is_err()
    );
    connection.cancel_operation(operation).unwrap();
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
}
#[test]
fn bridge_adoption_close_rejects_held_reply_and_actual_host_releases_stage() {
    let fixture = Fixture::new(Keys::new([]));
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    connection.confirm_adoption(preview).unwrap();
    let CloudPublicationWork::Adoption { handle, .. } = connection.next_publication_work().unwrap() else {
        panic!("adoption")
    };
    fixture.workflow.host.close().unwrap();
    failure_is(
        connection.adoption_result(
            handle,
            CloudAdoptionResult::Read {
                read: CloudControlRead::Found { record: active },
            },
        ),
        CloudConnectionFailure::Closed,
    );
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
}

#[test]
fn bridge_adoption_opaque_page_cursor_round_trips_and_deletions_abort_actual_host_staging() {
    let fixture = Fixture::new(Keys::new([]));
    let collection = fixture.workflow.host.collection().unwrap();
    let before = collection.generation();
    let connection = fixture.workflow.connection().unwrap();
    let (preview, active) = adoption_preview(&connection);
    connection.confirm_adoption(preview).unwrap();
    let source_epoch = CloudControlRecord {
        encrypted: Some(
            control_codec::Epoch {
                epoch_id: [8; 16],
                record_key: [19; 32],
                crt: NOW / 1000,
                ver: 18,
                state: control_codec::EpochState::Complete,
                reason: control_codec::EpochReason::Initial,
                published_at_ms: NOW,
                counts: vec![],
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![2],
    };
    let root = CloudControlRecord {
        encrypted: Some(
            control_codec::MediaRoot {
                media_key: [23; 32],
                created_at_ms: NOW,
            }
            .encode()
            .unwrap(),
        ),
        system_fields: vec![3],
    };
    for record in [active, source_epoch, root] {
        let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
            panic!("actual host adoption read")
        };
        assert!(matches!(action, CloudAdoptionAction::Read { .. }));
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Read {
                    read: CloudControlRead::Found { record },
                },
            )
            .unwrap();
    }
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("actual stage page")
    };
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: None, .. }));
    let cursor = vec![5, 4, 3];
    connection
        .adoption_result(
            handle,
            CloudAdoptionResult::Page {
                records: vec![],
                deleted: vec![],
                next: Some(cursor.clone()),
            },
        )
        .unwrap();
    let CloudPublicationWork::Adoption { handle, action, .. } = connection.next_publication_work().unwrap() else {
        panic!("next actual page")
    };
    assert!(matches!(action, CloudAdoptionAction::Fetch { cursor: Some(ref value), .. } if value == &cursor));
    assert!(
        connection
            .adoption_result(
                handle,
                CloudAdoptionResult::Page {
                    records: vec![],
                    deleted: vec!["deleted".into()],
                    next: None
                }
            )
            .is_err()
    );
    assert_eq!(fixture.workflow.host.collection().unwrap().generation(), before);
    assert!(fixture.workflow.core.adoption_op().unwrap().is_none());
    assert_eq!(fixture.workflow.host.engine().slot_holder(), None);
}
#[test]
fn accepted_unbound_identity_read_is_retained_and_stale_completion_cannot_replace_it() {
    let fixture = Fixture::new(Keys::new([]));
    let cache = Arc::new(CachedIdentity(Mutex::new(None)));
    let workflow = CloudWorkflow {
        host: fixture.workflow.host.clone(),
        core: fixture.workflow.core.clone(),
        identity: Some(cache.clone()),
    };
    let old = workflow.begin_identity_read(false, None).unwrap();
    let current = workflow.begin_identity_read(false, None).unwrap();
    assert!(
        !workflow
            .complete_identity_read(old, CloudAccountStatus::Available, Some(b"stale-observation".to_vec()))
            .unwrap()
    );
    let scope = c::BuildScope {
        container: c::ContainerId::new("synthetic-u114").unwrap(),
        environment: c::Environment::Development,
    };
    assert!(matches!(
        r::IdentityProvider::read_identity(&*cache, &scope),
        r::IdentityStatus::ReadFailed
    ));
    assert!(
        workflow
            .complete_identity_read(
                current,
                CloudAccountStatus::Available,
                Some(b"synthetic-account-A".to_vec())
            )
            .unwrap()
    );
    for _ in 0..3 {
        assert!(matches!(
            r::IdentityProvider::read_identity(&*cache, &scope),
            r::IdentityStatus::Available(_)
        ));
    }
}

#[test]
fn retained_cancel_id_cannot_cancel_the_next_actual_publication() {
    let fixture = Fixture::new(Keys::new([
        Some(vec![11; 32]),
        Some(vec![13; 32]),
        Some(vec![17; 32]),
        Some(vec![19; 32]),
    ]));
    let connection = fixture.workflow.connection().unwrap();
    let old = connection.confirm_publication(ready(&connection)).unwrap();
    connection.cancel_operation(old.clone()).unwrap();
    let current = connection.confirm_publication(ready(&connection)).unwrap();
    failure_is(connection.cancel_operation(old), CloudConnectionFailure::NotOffered);
    assert!(connection.recovery_status().unwrap().is_some());
    connection.cancel_operation(current).unwrap();
    assert!(connection.recovery_status().unwrap().is_none());
}
