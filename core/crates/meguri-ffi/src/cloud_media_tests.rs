//! Original exported Host/work handles and the production media batch path.
use super::*;
use crate::cloud_media::*;
use crate::cloud_policy::{CloudPolicy, CloudPolicyIssuedCommand, CloudPolicyOutput};
use meguri_sync::cloud::{control_codec, media as m, publication as p};
use meguri_testkit::cloud::{
    Caller, Config, ErrorCode, Field, Mutation, MutationSuccess, Operation, RecordInput, Reply, RequestedFields, Scope,
    Service, SystemFields,
    coordinator::{execute_publication_request, publication_control_record},
};

#[path = "cloud_zone_deletion_tests.rs"]
mod zone_deletion;

#[path = "cloud_payload_asset_tests.rs"]
mod payload_assets;

struct PublishedFixture {
    fixture: Fixture,
    policy: Arc<CloudPolicy>,
    outstanding_collection: Mutex<Vec<CloudPolicyIssuedCommand>>,
    outstanding_timers: Arc<Mutex<Vec<CloudPolicyIssuedCommand>>>,
    server: Mutex<Service<SeededRandom>>,
    scope: Scope,
    _server_directory: tempfile::TempDir,
}
impl std::ops::Deref for PublishedFixture {
    type Target = Fixture;
    fn deref(&self) -> &Fixture {
        &self.fixture
    }
}

fn published() -> (PublishedFixture, CloudEngineRegistration) {
    published_with_initial_media(None)
}

fn published_with_initial_media(initial_media: Option<(&str, &[u8])>) -> (PublishedFixture, CloudEngineRegistration) {
    let fixture = Fixture::new(Keys::new([Some(vec![31; 32]), Some(vec![47; 32])]));
    let policy = fixture.workflow.policy().unwrap();
    // K18.3 permits publication with media off: ACK the name through the real
    // collection batch while leaving its blob unknown and unuploaded.
    configure_media_policy(&fixture, &policy, initial_media.is_none());
    let media = fixture.workflow.host.media().unwrap();
    if let Some((name, bytes)) = initial_media {
        media.store().add(name, bytes).unwrap();
    }
    media.store().register_full(|_, _| true).unwrap();
    let collection = fixture.workflow.host.collection().unwrap();
    let preview = fixture.preview();
    fixture
        .workflow
        .core
        .confirm_connection_publication(&preview, &collection)
        .unwrap();
    let run = tempfile::tempdir().unwrap();
    let mut service = Service::open(run.path(), Config::default(), SeededRandom::new(811)).unwrap();
    let scope = Scope {
        container: "synthetic-u114".into(),
        environment: "development".into(),
        account: "synthetic-account-A".into(),
    };
    let mut sdk: Option<CloudEngineRegistration> = None;
    for _ in 0..128 {
        match fixture.workflow.core.next_publication_work(&collection).unwrap() {
            p::PublicationWork::Local { op, work } => fixture
                .workflow
                .core
                .perform_publication_local(op, work, &collection)
                .unwrap(),
            p::PublicationWork::Scan => {
                fixture
                    .workflow
                    .core
                    .reconcile_binding(&collection, &media, Duration::from_secs(1))
                    .unwrap();
            }
            p::PublicationWork::Own(request) => {
                let (reply, _) =
                    execute_publication_request(&mut service, &scope, &request, &fixture.workflow.core, &collection)
                        .unwrap();
                fixture
                    .workflow
                    .core
                    .publication_result(&request, reply, &collection)
                    .unwrap();
            }
            p::PublicationWork::SendEntities => {
                if sdk.is_none() {
                    sdk = Some(fixture.workflow.register_sdk(false).unwrap());
                }
                let sdk = sdk.as_ref().unwrap();
                let pending = fixture.workflow.tracking_sendables().unwrap();
                let batch = fixture
                    .workflow
                    .build_batch(sdk.instance_id.clone(), sdk.collection_zone.clone(), pending)
                    .unwrap()
                    .unwrap();
                let count = batch.saves.len() as u32;
                let changes = batch
                    .saves
                    .into_iter()
                    .map(|record| Mutation::Save {
                        record: RecordInput::encrypted(&record.name, "MGEntity", record.envelope),
                        if_unchanged: record.system_fields.map(SystemFields::from_bytes),
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
                    panic!("actual saved collection batch")
                };
                let saved = items
                    .into_iter()
                    .map(|item| {
                        let MutationSuccess::Saved(record) = item.result.unwrap() else {
                            panic!("actual Saved")
                        };
                        let value = publication_control_record(&record).unwrap();
                        CloudRecord {
                            name: record.name,
                            envelope: value.encrypted.unwrap(),
                            system_fields: Some(value.system_fields.0),
                            ..Default::default()
                        }
                    })
                    .collect();
                fixture
                    .workflow
                    .sent(sdk.instance_id.clone(), batch.id, saved, vec![], vec![], vec![], count)
                    .unwrap();
                fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
            }
            p::PublicationWork::Idle => {
                let sdk = sdk.unwrap();
                let inventory = service.inspect(&scope).unwrap().unwrap();
                let record = &inventory.zones["meguri-root"].records["active"];
                let value = publication_control_record(record).unwrap();
                let root = w::Saved {
                    name: "active".into(),
                    envelope: value.encrypted.unwrap(),
                    system_fields: value.system_fields.0,
                };
                fixture
                    .workflow
                    .core
                    .accept_root_fetch(&sdk.instance_id, Some(&root), true, 1)
                    .unwrap();
                let fixture = PublishedFixture {
                    fixture,
                    policy,
                    outstanding_collection: Mutex::new(Vec::new()),
                    outstanding_timers: Arc::new(Mutex::new(Vec::new())),
                    server: Mutex::new(service),
                    scope,
                    _server_directory: run,
                };
                let output = fixture
                    .policy
                    .clone()
                    .input(crate::cloud_policy::CloudPolicyEvent::Refresh {
                        cause: crate::cloud_policy::CloudPolicyChangeCause::Cloud,
                    })
                    .unwrap();
                assert!(settle_media_policy_core(&fixture, output).is_empty());
                let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
                assert!(state.gate_open());
                assert!(state.root_confirmed);
                assert_eq!(state.engine.as_ref().unwrap().instance, sdk.instance_id);
                assert!(!fixture.outstanding_collection.lock().unwrap().is_empty());
                return (fixture, sdk);
            }
            other => panic!("unexpected publication work: {other:?}"),
        }
    }
    panic!("bounded real publication")
}

struct ReadQueue(AtomicUsize);
impl CloudMediaQueue for ReadQueue {
    fn enqueue(&self) -> bool {
        self.0.fetch_add(1, Ordering::SeqCst);
        true
    }
}
fn root(media: &Arc<CloudMedia>, sdk: &CloudEngineRegistration) -> Arc<CloudMediaWork> {
    let issued = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(issued.request, CloudMediaRequest::Root));
    issued.handle
}
fn accept_root(media: &Arc<CloudMedia>, sdk: &CloudEngineRegistration) {
    let work = root(media, sdk);
    media
        .reply(
            work,
            CloudMediaReply::Record {
                name: "root".into(),
                record_type: "MGMediaRoot".into(),
                envelope: control_codec::MediaRoot {
                    media_key: [47; 32],
                    created_at_ms: NOW,
                }
                .encode()
                .unwrap(),
                asset_path: None,
            },
        )
        .unwrap();
}

#[test]
fn original_host_work_rejects_wrong_owner_stale_pause_sdk_and_close_before_enqueue() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    let other = fixture.workflow.media().unwrap();
    let work = root(&media, &sdk);
    let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
    let mut variants: [m::Work; 4] = std::array::from_fn(|_| work.retained_for_test().clone());
    variants[0].id += 1;
    variants[1].authority.file.inode += 1;
    variants[2].authority.generation += 1;
    variants[3].authority.account.0.push(0);
    for candidate in variants {
        assert!(
            fixture
                .workflow
                .core
                .validate_cloud_media_work(&fixture.workflow.host.collection().unwrap(), &candidate)
                .is_err()
        );
    }
    assert!(other.submit_read(work.clone(), queue.clone()).is_err());
    assert!(media.clone().next_work("wrong-sdk".into()).is_err());
    fixture.workflow.core.pause_cloud_media();
    assert!(media.submit_read(work.clone(), queue.clone()).is_err());
    assert!(media.reply(work, CloudMediaReply::UnknownItem).is_err());
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    fixture.workflow.core.resume_cloud_media();
    let work = root(&media, &sdk);
    fixture.workflow.host.close().unwrap();
    assert!(media.submit_read(work, queue.clone()).is_err());
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
}

#[test]
fn real_nonempty_upload_is_pending_until_original_sdk_saved_callback() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    let bytes = b"abc";
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .add("port.bin", bytes)
        .unwrap();
    let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(read.request, CloudMediaRequest::Read { .. }));
    media.reply(read.handle, CloudMediaReply::UnknownItem).unwrap();
    let upload = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Upload {
        name,
        staged_path,
        envelope,
    } = &upload.request
    else {
        panic!("real upload work")
    };
    assert_eq!(std::fs::read(staged_path).unwrap(), bytes);
    assert_eq!(m::blob::Blob::decode(envelope).unwrap().size, bytes.len() as u64);
    assert_eq!(
        m::blob::Blob::decode(envelope).unwrap().sha256,
        [
            0xba, 0x78, 0x16, 0xbf, 0x8f, 0x01, 0xcf, 0xea, 0x41, 0x41, 0x40, 0xde, 0x5d, 0xae, 0x22, 0x23, 0xb0, 0x03,
            0x61, 0xa3, 0x96, 0x17, 0x7a, 0x9c, 0xb4, 0x10, 0xff, 0x61, 0xf2, 0x00, 0x15, 0xad,
        ]
    );
    media.queue_upload(upload.handle.clone()).unwrap();
    for update in fixture.workflow.take_pending_updates(sdk.instance_id.clone()).unwrap() {
        fixture
            .workflow
            .update_applied(
                sdk.instance_id.clone(),
                update.id,
                update.adds.keys().cloned().collect(),
            )
            .unwrap();
    }
    let pending = fixture.workflow.tracking_sendables().unwrap();
    assert!(matches!(pending.get(name), Some(CloudPendingChange::Save)));
    let observations = fixture.workflow.core.observations().len();
    assert!(
        fixture
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                "meguri-media".into(),
                HashMap::from([(name.clone(), CloudPendingChange::Delete)]),
            )
            .is_err()
    );
    assert_eq!(fixture.workflow.core.observations().len(), observations);
    let batch = fixture
        .workflow
        .build_batch(sdk.instance_id.clone(), "meguri-media".into(), pending)
        .unwrap()
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(batch.saves[0].asset_path.as_deref(), Some(staged_path.as_str()));
    assert_eq!(batch.saves[0].record_type.as_deref(), Some("MGBlob"));
    assert!(
        fixture
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                sdk.collection_zone.clone(),
                HashMap::from([(name.clone(), CloudPendingChange::Save)])
            )
            .unwrap()
            .is_none()
    );
    assert!(
        fixture
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                "meguri-media".into(),
                HashMap::from([(name.clone(), CloudPendingChange::Delete)])
            )
            .is_err()
    );
    let retained = fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            "meguri-media".into(),
            HashMap::from([(name.clone(), CloudPendingChange::Save)]),
        )
        .unwrap()
        .unwrap();
    assert_eq!(retained.id, batch.id);
    assert!(matches!(
        fixture.workflow.tracking_sendables().unwrap().get(name),
        Some(CloudPendingChange::Save)
    ));
    let admission = Arc::new(ReadQueue(AtomicUsize::new(0)));
    assert!(
        fixture
            .workflow
            .submit_media_batch(sdk.instance_id.clone(), batch.id, admission.clone())
            .unwrap()
    );
    assert_eq!(admission.0.load(Ordering::SeqCst), 1);
    assert!(
        fixture
            .workflow
            .submit_media_batch("different-sdk".into(), batch.id, admission.clone())
            .is_err()
    );
    assert_eq!(admission.0.load(Ordering::SeqCst), 1);
    // Merely delivering an update never consumes the original work.
    fixture
        .workflow
        .core
        .validate_cloud_media_work(
            &fixture.workflow.host.collection().unwrap(),
            upload.handle.retained_for_test(),
        )
        .unwrap();
    assert!(
        fixture
            .workflow
            .sent(
                "wrong-sdk".into(),
                batch.id,
                batch.saves.clone(),
                vec![],
                vec![],
                vec![],
                1
            )
            .is_err()
    );
    fixture.workflow.core.pause_cloud_media();
    assert!(
        fixture
            .workflow
            .submit_media_batch(sdk.instance_id.clone(), batch.id, admission.clone())
            .is_err()
    );
    assert_eq!(admission.0.load(Ordering::SeqCst), 1);
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            batch.id,
            batch.saves,
            vec![],
            vec![],
            vec![],
            1,
        )
        .unwrap();
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    assert!(
        fixture
            .workflow
            .core
            .validate_cloud_media_work(
                &fixture.workflow.host.collection().unwrap(),
                upload.handle.retained_for_test()
            )
            .is_err()
    );
    assert!(!std::path::Path::new(staged_path).exists());
}

#[test]
fn replacement_sdk_keeps_upload_file_for_original_ack() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .add("pending.asset", b"pending blob data")
        .unwrap();
    let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    media.reply(read.handle, CloudMediaReply::UnknownItem).unwrap();
    let upload = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Upload { staged_path, .. } = &upload.request else {
        panic!("upload")
    };
    media.queue_upload(upload.handle.clone()).unwrap();
    let confirmed_root = fixture.workflow.core.adapter_snapshot().unwrap().root.unwrap();
    fixture.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    fixture.workflow.release_sdk(sdk.instance_id.clone()).unwrap();
    assert!(std::path::Path::new(staged_path).exists());
    let new_sdk = fixture.workflow.register_sdk(false).unwrap();
    assert!(media.queue_upload(upload.handle).is_err());
    fixture
        .workflow
        .core
        .accept_root_fetch(
            &new_sdk.instance_id,
            Some(&w::Saved {
                name: "active".into(),
                envelope: confirmed_root.envelope.0,
                system_fields: confirmed_root.system_fields.0,
            }),
            true,
            1,
        )
        .unwrap();
    accept_root(&media, &new_sdk);
    let next = media.clone().next_work(new_sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Upload {
        staged_path: next_path, ..
    } = next.request
    else {
        panic!("pending blob required")
    };
    assert_eq!(next_path, *staged_path);
    assert!(std::path::Path::new(&next_path).exists());
}

fn policy_commands(
    output: crate::cloud_policy::CloudPolicyOutput,
) -> Vec<crate::cloud_policy::CloudPolicyIssuedCommand> {
    let crate::cloud_policy::CloudPolicyOutput::Commands { commands } = output else {
        panic!("commands")
    };
    commands
}
fn settle_unpublished_media_policy(
    fixture: &Fixture,
    policy: &Arc<crate::cloud_policy::CloudPolicy>,
    output: crate::cloud_policy::CloudPolicyOutput,
) {
    use crate::cloud_policy::*;
    let mut commands = VecDeque::from(policy_commands(output));
    while let Some(command) = commands.pop_front() {
        match command.action {
            CloudPolicyAction::ConfigureAutomatic { on: true } => {
                let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
                assert!(state.guard().binding.is_none());
                assert!(state.engine.is_none());
                let guard = fixture.workflow.core.adapter_guard();
                assert_eq!(guard.generation, 0);
                assert!(guard.binding.is_empty());
                assert!(fixture.workflow.core.instance().unwrap_or_default().is_empty());
                assert!(policy.clone().prepare_execution(command.handle).unwrap().is_none());
                assert!(fixture.workflow.core.incoming_sdk_instance_for_testing().is_none());
                commands.extend(policy_commands(
                    policy
                        .clone()
                        .input(CloudPolicyEvent::Refresh {
                            cause: CloudPolicyChangeCause::Cloud,
                        })
                        .unwrap(),
                ));
            }
            CloudPolicyAction::UpdatePending { .. }
            | CloudPolicyAction::ApplyReady { .. }
            | CloudPolicyAction::RecordExchange { .. }
            | CloudPolicyAction::RecordQuietPoint { .. }
            | CloudPolicyAction::ProbeTracking
            | CloudPolicyAction::StatusChanged { .. } => {
                commands.extend(policy_commands(policy.clone().execute_core(command.handle).unwrap()));
            }
            other => panic!("unexpected SDK/network action in media fixture Core setup: {other:?}"),
        }
    }
}
fn configure_media_policy(fixture: &Fixture, policy: &Arc<CloudPolicy>, media_enabled: bool) {
    use crate::cloud_policy::*;
    let apply_setup = |output| {
        settle_unpublished_media_policy(fixture, policy, output);
        assert!(fixture.workflow.core.incoming_sdk_instance_for_testing().is_none());
    };
    apply_setup(
        policy
            .clone()
            .input(CloudPolicyEvent::Settings {
                settings: CloudPolicyDeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: true,
                    media: media_enabled,
                },
            })
            .unwrap(),
    );
    apply_setup(policy.clone().refresh_account().unwrap());
    apply_setup(
        policy
            .clone()
            .input(CloudPolicyEvent::Network { reachable: true })
            .unwrap(),
    );
}
fn media_policy(fixture: &PublishedFixture) -> Arc<CloudPolicy> {
    fixture.policy.clone()
}
struct CancelTimerQueue {
    timers: Arc<Mutex<Vec<CloudPolicyIssuedCommand>>>,
    calls: AtomicUsize,
}
impl crate::cloud_policy::CloudPolicyQueue for CancelTimerQueue {
    fn enqueue(&self) -> bool {
        assert_eq!(self.calls.fetch_add(1, Ordering::SeqCst), 0);
        self.timers.lock().unwrap().clear();
        true
    }
}
fn settle_media_policy_core(fixture: &PublishedFixture, output: CloudPolicyOutput) -> Vec<CloudPolicyIssuedCommand> {
    use crate::cloud_policy::*;
    let policy = &fixture.policy;
    let mut commands = VecDeque::from(policy_commands(output));
    let mut media = Vec::new();
    while let Some(command) = commands.pop_front() {
        match &command.action {
            CloudPolicyAction::UpdatePending { .. }
            | CloudPolicyAction::ApplyReady { .. }
            | CloudPolicyAction::RecordExchange { .. }
            | CloudPolicyAction::RecordQuietPoint { .. }
            | CloudPolicyAction::ProbeTracking
            | CloudPolicyAction::StatusChanged { .. } => {
                commands.extend(policy_commands(policy.clone().execute_core(command.handle).unwrap()));
            }
            CloudPolicyAction::RequestFetch { zones } => {
                assert_eq!(zones, &[CloudPolicyZone::Root, CloudPolicyZone::Collection]);
                fixture.outstanding_collection.lock().unwrap().push(command);
            }
            CloudPolicyAction::RequestSend { zones } => {
                assert_eq!(zones, &[CloudPolicyZone::Collection]);
                fixture.outstanding_collection.lock().unwrap().push(command);
            }
            CloudPolicyAction::ArmFlushTimer { .. } => fixture.outstanding_timers.lock().unwrap().push(command),
            CloudPolicyAction::CancelFlushTimer => {
                let queue = Arc::new(CancelTimerQueue {
                    timers: fixture.outstanding_timers.clone(),
                    calls: AtomicUsize::new(0),
                });
                assert!(matches!(
                    policy.clone().submit_native(command.handle, queue.clone()).unwrap(),
                    CloudPolicyAdmission::Admitted { accepted: true }
                ));
                assert_eq!(queue.calls.load(Ordering::SeqCst), 1);
                assert!(fixture.outstanding_timers.lock().unwrap().is_empty());
            }
            CloudPolicyAction::PauseMedia => {
                let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
                assert!(!execution.begin_media().unwrap());
            }
            CloudPolicyAction::ReadBlobs { .. }
            | CloudPolicyAction::ResumeMedia
            | CloudPolicyAction::PrioritizeMedia { .. } => media.push(command),
            other => panic!("unexpected post-publication action: {other:?}"),
        }
    }
    media
}
fn disable_media_policy(fixture: &PublishedFixture) {
    use crate::cloud_policy::*;
    let policy = media_policy(fixture);
    let output = policy
        .clone()
        .input(CloudPolicyEvent::Settings {
            settings: CloudPolicyDeviceSettings {
                enabled: true,
                paused: false,
                automatic: true,
                media: false,
            },
        })
        .unwrap();
    let commands = policy_commands(output);
    assert_eq!(
        commands
            .iter()
            .filter(|command| matches!(command.action, CloudPolicyAction::PauseMedia))
            .count(),
        1,
    );
    assert!(settle_media_policy_core(fixture, CloudPolicyOutput::Commands { commands }).is_empty());
    // Collection fetch/send commands stay outstanding throughout local recovery.
    let collection = fixture.outstanding_collection.lock().unwrap();
    assert!(!collection.is_empty());
    for command in collection.iter() {
        let execution = policy
            .clone()
            .prepare_execution(command.handle.clone())
            .unwrap()
            .unwrap();
        execution.validate_execution().unwrap();
        assert!(execution.begin_media().is_err());
    }
}

#[test]
fn media_off_empty_intents_allow_local_reconcile_without_resuming_network_media() {
    let (fixture, sdk) = published();
    let collection = fixture.workflow.host.collection().unwrap();
    let service = fixture.workflow.host.media().unwrap();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    disable_media_policy(&fixture);
    fixture
        .workflow
        .core
        .reconcile_binding(&collection, &service, Duration::from_secs(1))
        .unwrap();
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    let media = fixture.workflow.media().unwrap();
    assert!(media.next_work(sdk.instance_id).unwrap().is_none());
}

#[test]
fn media_off_settles_matching_media_name_and_its_tombstone_without_transfers() {
    use meguri_sync::cloud::{InboxReason, entity_codec as codec, incoming::IncomingDisposition};
    let (fixture, sdk) = published();
    let collection = fixture.workflow.host.collection().unwrap();
    let service = fixture.workflow.host.media().unwrap();
    let media = fixture.workflow.media().unwrap();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let bytes = b"synthetic matching collection media metadata";
    service.store().add("matching.asset", bytes).unwrap();
    disable_media_policy(&fixture);
    incoming_media_with_base(
        &fixture,
        &sdk,
        "matching.asset",
        bytes,
        None,
        Some(IncomingDelivery::InboxOnly),
    );
    let inbox = fixture.workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, InboxReason::Ready);
    assert!(matches!(
        fixture
            .workflow
            .core
            .drain_native_incoming(&collection, &sdk.instance_id, Duration::from_secs(1),)
            .unwrap(),
        IncomingDisposition::Progress(_)
    ));
    assert!(fixture.workflow.core.inbox_entries(binding).unwrap().is_empty());
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    assert_eq!(
        std::fs::read(service.store().media_dir().join("matching.asset")).unwrap(),
        bytes
    );
    let entity = fixture
        .workflow
        .core
        .entity_facts_page(codec::Kind::MediaName, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == b"matching.asset")
        .unwrap();
    assert_eq!(entity.server_erev, 1);
    assert_eq!(entity.acked_rev, entity.local_rev);
    let codec::DecodeOutcome::Supported(remote) = codec::Entity::decode(&entity.acked_envelope.0).unwrap() else {
        panic!("supported media-name record")
    };
    let mut fields = remote.fields;
    fields.tombstone = true;
    fields.deleted_at_ms = NOW;
    fields.erev += 1;
    let envelope = codec::Entity::build(fields, &entity.payload.encode().unwrap())
        .unwrap()
        .encode()
        .unwrap();
    deliver_incoming(
        &fixture,
        &sdk,
        w::Saved {
            name: entity.name.clone(),
            envelope,
            system_fields: vec![2],
        },
        IncomingDelivery::InboxOnly,
    );
    let inbox = fixture.workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, InboxReason::Ready);
    assert!(matches!(
        fixture
            .workflow
            .core
            .drain_native_incoming(&collection, &sdk.instance_id, Duration::from_secs(1),)
            .unwrap(),
        IncomingDisposition::Progress(_)
    ));
    assert!(!service.store().media_dir().join("matching.asset").exists());
    assert!(fixture.workflow.core.inbox_entries(binding).unwrap().is_empty());
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    let tombstone = fixture
        .workflow
        .core
        .entity_facts_page(codec::Kind::MediaName, None)
        .unwrap()
        .into_iter()
        .find(|e| e.name == entity.name)
        .unwrap();
    assert_eq!(tombstone.server_erev, 2);
    assert_eq!(tombstone.acked_rev, tombstone.local_rev);
    assert!(matches!(codec::Entity::decode(&tombstone.acked_envelope.0).unwrap(),
        codec::DecodeOutcome::Supported(record) if record.fields.tombstone));
    assert!(media.next_work(sdk.instance_id).unwrap().is_none());
}

#[test]
fn media_off_keeps_nonmatching_media_name_pending_without_download_or_replacement() {
    use meguri_sync::cloud::incoming::IncomingDisposition;
    let (fixture, sdk) = published();
    let collection = fixture.workflow.host.collection().unwrap();
    let service = fixture.workflow.host.media().unwrap();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    service
        .store()
        .add("different.asset", b"synthetic local content")
        .unwrap();
    disable_media_policy(&fixture);
    incoming_media(&fixture, &sdk, "different.asset", b"synthetic remote content");
    incoming_media(&fixture, &sdk, "absent.asset", b"synthetic absent content");
    let inbox = fixture.workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(inbox.len(), 2);
    assert!(matches!(
        fixture
            .workflow
            .core
            .drain_native_incoming(&collection, &sdk.instance_id, Duration::from_secs(1),)
            .unwrap(),
        IncomingDisposition::Progress(0)
    ));
    assert_eq!(fixture.workflow.core.inbox_entries(binding).unwrap(), inbox);
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    assert_eq!(
        std::fs::read(service.store().media_dir().join("different.asset")).unwrap(),
        b"synthetic local content"
    );
    assert!(!service.store().media_dir().join("absent.asset").exists());
    assert!(
        fixture
            .workflow
            .media()
            .unwrap()
            .next_work(sdk.instance_id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn media_off_settles_real_interrupted_download_before_local_reconcile() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    let bytes = b"synthetic interrupted download recovered with media off";
    let metadata = incoming_media(&fixture, &sdk, "recover.asset", bytes);
    let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Read { name } = read.request else {
        panic!("metadata read")
    };
    media
        .reply(
            read.handle,
            CloudMediaReply::Record {
                name: name.clone(),
                record_type: "MGBlob".into(),
                envelope: metadata.encode().unwrap(),
                asset_path: None,
            },
        )
        .unwrap();
    let download = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(download.request, CloudMediaRequest::Download { .. }));
    let source = fixture._directory.path().join("recovery-download-source");
    std::fs::write(&source, bytes).unwrap();
    let point = "cloud.media.during_install";
    meguri_model::faultpoint::arm(
        point,
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let reply = media.reply(
        download.handle,
        CloudMediaReply::Record {
            name,
            record_type: "MGBlob".into(),
            envelope: metadata.encode().unwrap(),
            asset_path: Some(source.to_string_lossy().into_owned()),
        },
    );
    meguri_model::faultpoint::disarm(point);
    assert!(reply.is_err());
    let collection = fixture.workflow.host.collection().unwrap();
    let service = fixture.workflow.host.media().unwrap();
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let intents = fixture.workflow.core.apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    assert!(intents[0].media.as_ref().unwrap().staged.as_ref().unwrap().exists());
    assert!(!service.store().media_dir().join("recover.asset").exists());
    disable_media_policy(&fixture);
    fixture
        .workflow
        .core
        .reconcile_binding(&collection, &service, Duration::from_secs(1))
        .unwrap();
    assert_eq!(
        std::fs::read(service.store().media_dir().join("recover.asset")).unwrap(),
        bytes
    );
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    assert!(fixture.workflow.core.inbox_entries(binding).unwrap().is_empty());
    assert!(media.next_work(sdk.instance_id).unwrap().is_none());
}

fn foreground_media_permit(
    fixture: &PublishedFixture,
    sdk: &CloudEngineRegistration,
    policy: &Arc<crate::cloud_policy::CloudPolicy>,
) -> Arc<crate::cloud_policy::CloudPolicyExecution> {
    use crate::cloud_policy::*;
    let mut commands = VecDeque::from(settle_media_policy_core(
        fixture,
        policy.clone().input(CloudPolicyEvent::Foreground).unwrap(),
    ));
    while let Some(command) = commands.pop_front() {
        match command.action {
            CloudPolicyAction::ReadBlobs { .. } => {
                let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
                assert!(execution.begin_media().unwrap());
                finish_issued_metadata_reads(fixture, sdk);
            }
            CloudPolicyAction::ResumeMedia => {
                assert!(commands.is_empty(), "unhandled media actions after ResumeMedia");
                return policy.clone().prepare_execution(command.handle).unwrap().unwrap();
            }
            other => panic!("unexpected foreground media action: {other:?}"),
        }
    }
    panic!("Foreground must issue ResumeMedia after the accepted publication projection")
}

fn finish_issued_metadata_reads(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) {
    let media = fixture.workflow.media().unwrap();
    for _ in 0..128 {
        let Some(issued) = media.clone().next_work(sdk.instance_id.clone()).unwrap() else {
            return;
        };
        let CloudMediaRequest::Reads { names } = issued.request else {
            panic!("issued ReadBlobs must select metadata work")
        };
        let results = names
            .into_iter()
            .map(|name| {
                let response = fixture
                    .server
                    .lock()
                    .unwrap()
                    .request(
                        &fixture.scope,
                        Caller::default(),
                        Operation::Read {
                            zone: "meguri-media".into(),
                            name: name.clone(),
                            fields: RequestedFields::Named(vec!["e".into()]),
                        },
                    )
                    .unwrap();
                let result = match response.result {
                    Ok(Reply::Record(record)) => {
                        assert_eq!(record.name, name);
                        assert_eq!(record.record_type, "MGBlob");
                        assert!(!record.fields.contains_key("a"));
                        let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                            panic!("blob metadata envelope")
                        };
                        CloudMediaBlobResult::Present {
                            envelope: envelope.clone(),
                        }
                    }
                    Err(error) => match error.code {
                        ErrorCode::UnknownItem => CloudMediaBlobResult::UnknownItem,
                        ErrorCode::ZoneNotFound => CloudMediaBlobResult::ZoneMissing,
                        code => CloudMediaBlobResult::Failed { code: code as i64 },
                    },
                    _ => panic!("record-ID read reply"),
                };
                CloudMediaBlobRead { name, result }
            })
            .collect();
        media
            .reply(issued.handle, CloudMediaReply::BlobReads { results })
            .unwrap();
    }
    panic!("bounded issued metadata round")
}

#[test]
fn issued_media_actions_read_root_each_resume_and_pause_revokes_delivery() {
    use crate::cloud_policy::*;
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    let policy = media_policy(&fixture);
    let permit = foreground_media_permit(&fixture, &sdk, &policy);
    assert!(permit.begin_media().unwrap());
    accept_root(&media, &sdk);
    let repeated = foreground_media_permit(&fixture, &sdk, &policy);
    assert!(repeated.begin_media().unwrap());
    let work = root(&media, &sdk);
    let output = policy
        .clone()
        .input(CloudPolicyEvent::Settings {
            settings: CloudPolicyDeviceSettings {
                enabled: true,
                paused: false,
                automatic: true,
                media: false,
            },
        })
        .unwrap();
    assert_eq!(
        policy_commands(output.clone())
            .iter()
            .filter(|command| matches!(command.action, CloudPolicyAction::PauseMedia))
            .count(),
        1,
    );
    assert!(settle_media_policy_core(&fixture, output).is_empty());
    let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
    assert!(media.submit_read(work, queue.clone()).is_err());
    assert_eq!(queue.0.load(Ordering::SeqCst), 0);
    assert!(repeated.begin_media().is_err());
}

enum IncomingDelivery {
    FetchAndApply,
    InboxOnly,
}

fn deliver_incoming(fixture: &Fixture, sdk: &CloudEngineRegistration, record: w::Saved, delivery: IncomingDelivery) {
    match delivery {
        IncomingDelivery::FetchAndApply => fixture
            .workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone.clone(),
                vec![CloudRecord {
                    name: record.name,
                    envelope: record.envelope,
                    system_fields: Some(record.system_fields),
                    ..Default::default()
                }],
                vec![],
            )
            .unwrap(),
        // Explicit apply fixtures stop after K13.1's guarded durable inbox
        // commit; their drain or policy action performs K13.2 separately.
        IncomingDelivery::InboxOnly => fixture
            .workflow
            .core
            .adapter_fetched(&sdk.instance_id, &sdk.collection_zone, &[record], &[])
            .unwrap(),
    }
}

fn incoming_media(fixture: &Fixture, sdk: &CloudEngineRegistration, name: &str, bytes: &[u8]) -> m::blob::Blob {
    incoming_media_with_base(fixture, sdk, name, bytes, None, None)
}

fn incoming_media_with_base(
    fixture: &Fixture,
    sdk: &CloudEngineRegistration,
    name: &str,
    bytes: &[u8],
    base: Option<&w::EntityFacts>,
    delivery: Option<IncomingDelivery>,
) -> m::blob::Blob {
    use meguri_sync::cloud::entity_codec as codec;
    let scratch = fixture.workflow.host.media().unwrap();
    let source_name = format!("metadata-{name}");
    scratch.store().add(&source_name, bytes).unwrap();
    fixture
        .workflow
        .core
        .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &scratch)
        .unwrap();
    let entity = fixture
        .workflow
        .core
        .entity_facts_page(codec::Kind::MediaName, None)
        .unwrap()
        .into_iter()
        .find(|e| matches!(&e.payload, codec::Payload::MediaName(p) if p.name == source_name.as_bytes()))
        .unwrap();
    let codec::Payload::MediaName(local) = entity.payload else {
        panic!("media facts")
    };
    let metadata = m::blob::Blob::from(&local);
    scratch.store().trash(&[source_name]).unwrap();
    fixture
        .workflow
        .core
        .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &scratch)
        .unwrap();
    let payload = codec::Payload::MediaName(metadata.media(name));
    let key = name.to_lowercase().into_bytes();
    let record_name = codec::entity_name(&[31; 32], codec::Kind::MediaName, &key).unwrap();
    let envelope = codec::Entity::build(
        codec::EntityFields {
            kind: codec::Kind::MediaName,
            key,
            origin: base.map_or([91; 16], |entity| entity.origin),
            erev: base.map_or(1, |entity| entity.server_erev.checked_add(1).unwrap()),
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: [92; 16],
            base_sha256: base.map(|entity| entity.acked_payload_digest),
        },
        &payload.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    deliver_incoming(
        fixture,
        sdk,
        w::Saved {
            name: record_name,
            envelope,
            system_fields: vec![1],
        },
        delivery.unwrap_or(IncomingDelivery::FetchAndApply),
    );
    metadata
}

#[test]
fn zero_progress_apply_waits_for_guarded_media_arrival_without_losing_ready_work() {
    use crate::cloud_policy::*;
    use meguri_sync::cloud::{InboxReason, entity_codec as codec};
    for existing_file in [false, true] {
        let (fixture, sdk) = published_with_initial_media(
            existing_file.then_some(("waiting.asset", b"retained local bytes".as_slice())),
        );
        let policy = media_policy(&fixture);
        let media = fixture.workflow.media().unwrap();
        let service = fixture.workflow.host.media().unwrap();
        let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
        let baseline = existing_file.then(|| {
            let record_name = codec::entity_name(&[31; 32], codec::Kind::MediaName, b"waiting.asset").unwrap();
            let entity = fixture.workflow.core.entity_facts(&record_name).unwrap().unwrap();
            assert!(entity.mark.is_none());
            assert_eq!(entity.local_rev, entity.acked_rev);
            assert!(entity.in_flight.is_empty());
            assert_eq!(entity.server_erev, 1);
            assert_eq!(entity.acked_payload.as_ref(), Some(&entity.payload));
            assert_eq!(entity.acked_row_digest, entity.payload.row_digest().unwrap());
            assert_eq!(entity.acked_payload_digest, entity.payload.payload_digest().unwrap());
            assert!(!entity.acked_system_fields.is_empty());
            assert!(
                !fixture
                    .workflow
                    .tracking_sendables()
                    .unwrap()
                    .contains_key(&record_name)
            );
            let codec::DecodeOutcome::Supported(acked) = codec::Entity::decode(&entity.acked_envelope.0).unwrap()
            else {
                panic!("actual ACKed media envelope")
            };
            assert_eq!(acked.fields.origin, entity.origin);
            assert_eq!(acked.fields.erev, entity.server_erev);
            assert_eq!(acked.payload_sha256, entity.acked_payload_digest);
            assert!(!acked.fields.tombstone);
            let inventory = fixture.server.lock().unwrap().inspect(&fixture.scope).unwrap().unwrap();
            let saved =
                publication_control_record(&inventory.zones[&sdk.collection_zone].records[&record_name]).unwrap();
            assert_eq!(saved.system_fields.0, entity.acked_system_fields);
            assert_eq!(saved.encrypted.as_deref(), Some(entity.acked_envelope.0.as_slice()));
            assert!(policy.status().unwrap().media_paused);
            let enabled = policy
                .clone()
                .input(CloudPolicyEvent::Settings {
                    settings: CloudPolicyDeviceSettings {
                        enabled: true,
                        paused: false,
                        automatic: true,
                        media: true,
                    },
                })
                .unwrap();
            let mut resumed = settle_media_policy_core(&fixture, enabled);
            assert_eq!(resumed.len(), 1);
            let resume = resumed.pop().unwrap();
            assert!(matches!(resume.action, CloudPolicyAction::ResumeMedia));
            assert!(
                policy
                    .clone()
                    .prepare_execution(resume.handle)
                    .unwrap()
                    .unwrap()
                    .begin_media()
                    .unwrap()
            );
            assert!(!policy.status().unwrap().media_paused);
            entity
        });
        let expected_erev = baseline.as_ref().map_or(1, |entity| entity.server_erev + 1);
        assert_eq!(expected_erev, if existing_file { 2 } else { 1 });
        // Retain real, nonempty evidence so a report-only completion cannot
        // replace the previous attempt with invented/default evidence.
        let mut requested = policy_commands(
            policy
                .clone()
                .input(CloudPolicyEvent::SyncNowRequested {
                    source: CloudPolicySource::Command,
                })
                .unwrap(),
        );
        let run = requested.remove(
            requested
                .iter()
                .position(|c| matches!(c.action, CloudPolicyAction::RunExchange { .. }))
                .unwrap(),
        );
        let CloudPolicyAction::RunExchange { exchange, .. } = run.action else {
            unreachable!()
        };
        assert!(settle_media_policy_core(&fixture, CloudPolicyOutput::Commands { commands: requested }).is_empty());
        let ended = policy
            .clone()
            .end_before_capture(exchange, CloudPolicyEarlyEnd::Cancelled)
            .unwrap();
        assert!(settle_media_policy_core(&fixture, ended).is_empty());
        accept_root(&media, &sdk);
        let local_blob = baseline.as_ref().map(|entity| {
            let codec::Payload::MediaName(local) = &entity.payload else {
                panic!("actual published media metadata")
            };
            assert_eq!(local.name, b"waiting.asset");
            let blob = m::blob::Blob::from(local);
            let inventory = fixture.server.lock().unwrap().inspect(&fixture.scope).unwrap().unwrap();
            assert!(
                !inventory.zones["meguri-media"]
                    .records
                    .contains_key(&blob.name(&[47; 32]))
            );
            blob
        });
        let bytes = b"actual arriving remote bytes";
        let metadata = incoming_media_with_base(&fixture, &sdk, "waiting.asset", bytes, baseline.as_ref(), None);
        let file = service.store().media_dir().join("waiting.asset");
        let before_bytes = std::fs::read(&file).ok();
        assert_eq!(
            before_bytes.as_deref(),
            existing_file.then_some(b"retained local bytes".as_slice())
        );
        let inbox = fixture.workflow.core.inbox_entries(binding).unwrap();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].entry.reason, InboxReason::Ready);
        let entities = fixture
            .workflow
            .core
            .entity_facts_page(codec::Kind::MediaName, None)
            .unwrap();
        let before = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert!(before.evidence.last_attempt.is_some());
        let mut issued = policy_commands(
            policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Local,
                })
                .unwrap(),
        );
        assert_eq!(
            issued
                .iter()
                .filter(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
                .count(),
            1
        );
        let apply = issued.remove(
            issued
                .iter()
                .position(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
                .unwrap(),
        );
        let reads = settle_media_policy_core(&fixture, CloudPolicyOutput::Commands { commands: issued });
        let projected = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        let observed = policy.clone().execute_core(apply.handle).unwrap();
        assert!(
            !policy_commands(observed.clone())
                .iter()
                .any(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
        );
        assert!(settle_media_policy_core(&fixture, observed).is_empty());
        let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert_eq!(after.evidence, before.evidence);
        assert_eq!(after.facts.summary.ready, 1);
        assert_eq!(after.facts.summary.unsettled_intents, 0);
        assert_eq!(
            after.facts.summary.pending_unheld,
            projected.facts.summary.pending_unheld
        );
        assert_eq!(
            after.facts.summary.incoming_waiting,
            projected.facts.summary.incoming_waiting
        );
        assert_eq!(after.facts.summary.media, projected.facts.summary.media);
        assert_eq!(fixture.workflow.core.inbox_entries(binding).unwrap(), inbox);
        assert!(
            fixture
                .workflow
                .core
                .entity_facts_page(codec::Kind::MediaName, None)
                .unwrap()
                == entities
        );
        assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
        assert_eq!(std::fs::read(&file).ok(), before_bytes);
        let facts = policy.status().unwrap().media;
        assert_eq!(facts.download_files, Some(1));
        assert_eq!(facts.download_bytes, Some(bytes.len() as u64));
        assert!(facts.unknown_blobs.is_some_and(|count| count > 0));
        assert_eq!(reads.len(), 1);
        let read = reads.into_iter().next().unwrap();
        assert!(matches!(
            read.action,
            CloudPolicyAction::ReadBlobs { include_missing: false }
        ));
        assert!(
            policy
                .clone()
                .prepare_execution(read.handle)
                .unwrap()
                .unwrap()
                .begin_media()
                .unwrap()
        );
        let work = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        let CloudMediaRequest::Reads { names } = work.request else {
            panic!("original bounded metadata read")
        };
        let remote_name = metadata.name(&[47; 32]);
        assert!(names.contains(&remote_name));
        let results = names
            .into_iter()
            .map(|name| {
                let blob = if name == remote_name {
                    &metadata
                } else {
                    let local = local_blob.as_ref().unwrap();
                    assert_eq!(name, local.name(&[47; 32]));
                    local
                };
                CloudMediaBlobRead {
                    name,
                    result: CloudMediaBlobResult::Present {
                        envelope: blob.encode().unwrap(),
                    },
                }
            })
            .collect();
        media
            .reply(work.handle, CloudMediaReply::BlobReads { results })
            .unwrap();
        let foreground = policy.clone().input(CloudPolicyEvent::Foreground).unwrap();
        assert_eq!(
            policy_commands(foreground.clone())
                .iter()
                .filter(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
                .count(),
            1
        );
        let resumed = settle_media_policy_core(&fixture, foreground);
        assert_eq!(resumed.len(), 1);
        let resume = resumed.into_iter().next().unwrap();
        assert!(matches!(resume.action, CloudPolicyAction::ResumeMedia));
        assert!(
            policy
                .clone()
                .prepare_execution(resume.handle)
                .unwrap()
                .unwrap()
                .begin_media()
                .unwrap()
        );
        accept_root(&media, &sdk);
        let download = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        assert!(matches!(download.request, CloudMediaRequest::Download { .. }));
        let events = fixture.workflow.host.engine().stream().subscribe();
        let stage = fixture._directory.path().join("zero-progress-arrival");
        std::fs::write(&stage, bytes).unwrap();
        media
            .reply(
                download.handle,
                CloudMediaReply::Record {
                    name: remote_name.clone(),
                    record_type: "MGBlob".into(),
                    envelope: metadata.encode().unwrap(),
                    asset_path: Some(stage.to_string_lossy().into_owned()),
                },
            )
            .unwrap();
        assert_eq!(std::fs::read(&file).unwrap(), bytes);
        let mut arrived = false;
        while let Some(event) = events.try_recv() {
            if let meguri_engine::EventKind::Extension(extension) = &event.kind
                && let Some(arrival) = extension.as_any().downcast_ref::<meguri_engine::media::MediaArrived>()
            {
                arrived |= arrival.names.iter().any(|name| name == "waiting.asset");
            }
        }
        assert!(arrived);
        assert!(fixture.workflow.core.inbox_entries(binding).unwrap().is_empty());
        assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
        let applied = fixture
            .workflow
            .core
            .entity_facts(&inbox[0].entry.record_name)
            .unwrap()
            .unwrap();
        assert_eq!(applied.server_erev, expected_erev);
        assert_eq!(
            applied.origin,
            baseline.as_ref().map_or([91; 16], |entity| entity.origin)
        );
        assert_eq!(applied.acked_system_fields, vec![1]);
        assert!(
            matches!(applied.acked_payload, Some(codec::Payload::MediaName(ref p)) if *p == metadata.media("waiting.asset"))
        );
        let refreshed = policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Local,
            })
            .unwrap();
        assert!(
            !policy_commands(refreshed.clone())
                .iter()
                .any(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
        );
        let _new_media_work = settle_media_policy_core(&fixture, refreshed);
        let after = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        assert_eq!(after.facts.summary.ready, 0);
        assert_eq!(after.evidence, before.evidence);
    }
}

#[test]
fn ordinary_incoming_progress_continues_before_waiting_media_stops_the_chain() {
    use crate::cloud_policy::*;
    use meguri_sync::cloud::{InboxReason, entity_codec as codec};
    let (fixture, sdk) = published();
    let policy = media_policy(&fixture);
    incoming_media(&fixture, &sdk, "still-waiting.asset", b"not downloaded yet");
    let entity = fixture
        .workflow
        .core
        .entity_facts_page(codec::Kind::Deck, None)
        .unwrap()
        .into_iter()
        .next()
        .unwrap();
    let codec::Payload::Deck(mut deck) = entity.payload else {
        panic!("real baseline deck")
    };
    deck.id = 8_000_000_001;
    deck.name = b"Remote ordinary progress".to_vec();
    deck.mtime_secs += 1;
    let codec::DecodeOutcome::Supported(mut remote) = codec::Entity::decode(&entity.acked_envelope.0).unwrap() else {
        panic!("acked deck envelope")
    };
    remote.fields.key = codec::id_key(deck.id).to_vec();
    remote.fields.origin = [94; 16];
    remote.fields.erev = 1;
    let record_name = codec::entity_name(&[31; 32], codec::Kind::Deck, &remote.fields.key).unwrap();
    let envelope = codec::Entity::build(remote.fields, &codec::Payload::Deck(deck.clone()).encode().unwrap())
        .unwrap()
        .encode()
        .unwrap();
    deliver_incoming(
        &fixture,
        &sdk,
        w::Saved {
            name: record_name.clone(),
            envelope,
            system_fields: vec![2],
        },
        IncomingDelivery::InboxOnly,
    );
    let binding = fixture.workflow.core.status().unwrap().binding.unwrap().id;
    let inbox = fixture.workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(inbox.len(), 2);
    let ordinary = inbox
        .iter()
        .find(|entry| entry.entry.record_name == record_name)
        .unwrap();
    assert_eq!(ordinary.entry.reason, InboxReason::Ready);
    let mut commands = policy_commands(
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Cloud,
            })
            .unwrap(),
    );
    let apply = commands.remove(
        commands
            .iter()
            .position(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
            .unwrap(),
    );
    assert!(settle_media_policy_core(&fixture, CloudPolicyOutput::Commands { commands }).is_empty());
    let mut continued = policy_commands(policy.clone().execute_core(apply.handle).unwrap());
    assert_eq!(
        continued
            .iter()
            .filter(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
            .count(),
        1
    );
    assert!(
        continued
            .iter()
            .any(|c| matches!(c.action, CloudPolicyAction::UpdatePending { .. }))
    );
    let next = continued.remove(
        continued
            .iter()
            .position(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
            .unwrap(),
    );
    assert!(settle_media_policy_core(&fixture, CloudPolicyOutput::Commands { commands: continued }).is_empty());
    let observed = policy.clone().execute_core(next.handle).unwrap();
    assert!(
        !policy_commands(observed.clone())
            .iter()
            .any(|c| matches!(c.action, CloudPolicyAction::ApplyReady { .. }))
    );
    assert!(settle_media_policy_core(&fixture, observed).is_empty());
    let applied = fixture.workflow.core.entity_facts(&record_name).unwrap().unwrap();
    assert!(matches!(applied.acked_payload, Some(codec::Payload::Deck(ref p)) if *p == deck));
    assert_eq!(fixture.workflow.core.inbox_entries(binding).unwrap().len(), 1);
    assert!(fixture.workflow.core.apply_intents(binding).unwrap().is_empty());
    assert_eq!(
        fixture
            .workflow
            .host
            .cloud_policy_driver()
            .unwrap()
            .state()
            .facts
            .summary
            .ready,
        1
    );
}

#[test]
fn nonempty_download_waits_for_collection_drain_and_arrives_in_engine_media_store() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    let bytes = b"nonempty incoming asset";
    let metadata = incoming_media(&fixture, &sdk, "visible.asset", bytes);
    let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Read { name } = read.request else {
        panic!("metadata first")
    };
    media
        .reply(
            read.handle,
            CloudMediaReply::Record {
                name: name.clone(),
                record_type: "MGBlob".into(),
                envelope: metadata.encode().unwrap(),
                asset_path: None,
            },
        )
        .unwrap();
    let call = fixture
        .workflow
        .begin_fetch(sdk.instance_id.clone(), vec![sdk.collection_zone.clone()])
        .unwrap();
    assert!(media.downloads_blocked(sdk.instance_id.clone()).unwrap());
    assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
    fixture
        .workflow
        .will_fetch(sdk.instance_id.clone(), "media-drain-test".into(), true)
        .unwrap();
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), true, false)
        .unwrap();
    fixture
        .workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), false, false)
        .unwrap();
    fixture
        .workflow
        .did_fetch(sdk.instance_id.clone(), "media-drain-test".into(), true)
        .unwrap();
    fixture.workflow.finish_fetch(sdk.instance_id.clone(), call).unwrap();
    assert!(!media.downloads_blocked(sdk.instance_id.clone()).unwrap());
    let download = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(download.request, CloudMediaRequest::Download { .. }));
    let events = fixture.workflow.host.engine().stream().subscribe();
    let stage = fixture._directory.path().join("download-source");
    std::fs::write(&stage, bytes).unwrap();
    media
        .reply(
            download.handle,
            CloudMediaReply::Record {
                name,
                record_type: "MGBlob".into(),
                envelope: metadata.encode().unwrap(),
                asset_path: Some(stage.to_string_lossy().into_owned()),
            },
        )
        .unwrap();
    let service = fixture.workflow.host.media().unwrap();
    let file = service.store().media_dir().join("visible.asset");
    assert_eq!(std::fs::read(file).unwrap(), bytes);
    assert!(service.store().open_file("visible.asset").unwrap().is_some());
    let mut arrived = false;
    while let Some(event) = events.try_recv() {
        if let meguri_engine::EventKind::Extension(extension) = &event.kind
            && let Some(arrival) = extension.as_any().downcast_ref::<meguri_engine::media::MediaArrived>()
        {
            arrived |= arrival.names.iter().any(|name| name == "visible.asset");
        }
    }
    assert!(arrived);
}

#[test]
fn unsyncable_local_file_blocks_download_candidates_and_retained_admission() {
    use crate::cloud_policy::*;
    for retain_download in [false, true] {
        let (fixture, sdk) = published_with_initial_media(Some(("photo.jpg", b"synthetic server image")));
        let record_name =
            c::entity_codec::entity_name(&[31; 32], c::entity_codec::Kind::MediaName, b"photo.jpg").unwrap();
        let initial = fixture.workflow.core.entity_facts(&record_name).unwrap().unwrap();
        assert!(initial.acked_rev > 0 && !initial.acked_envelope.0.is_empty());
        let Some(c::entity_codec::Payload::MediaName(local)) = &initial.acked_payload else {
            panic!("published target ACK required")
        };
        let metadata = m::blob::Blob::from(local);
        let service = fixture.workflow.host.media().unwrap();
        let file = service.store().media_dir().join("photo.jpg");
        // An external disappearance requests a download without a local tombstone.
        std::fs::remove_file(&file).unwrap();
        service.store().register_full(|_, _| true).unwrap();
        fixture
            .workflow
            .core
            .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &service)
            .unwrap();
        let policy = media_policy(&fixture);
        assert!(policy.status().unwrap().media_paused);
        let enabled = policy
            .clone()
            .input(CloudPolicyEvent::Settings {
                settings: CloudPolicyDeviceSettings {
                    enabled: true,
                    paused: false,
                    automatic: true,
                    media: true,
                },
            })
            .unwrap();
        let mut resumed = settle_media_policy_core(&fixture, enabled);
        assert_eq!(resumed.len(), 1);
        let resume = resumed.pop().unwrap();
        assert!(matches!(resume.action, CloudPolicyAction::ResumeMedia));
        assert!(
            policy
                .clone()
                .prepare_execution(resume.handle)
                .unwrap()
                .unwrap()
                .begin_media()
                .unwrap()
        );
        assert!(!policy.status().unwrap().media_paused);
        let media = fixture.workflow.media().unwrap();
        accept_root(&media, &sdk);
        let name = metadata.name(&[47; 32]);
        let read = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        assert!(matches!(&read.request, CloudMediaRequest::Read { name: n } if n == &name));
        media
            .reply(
                read.handle,
                CloudMediaReply::Record {
                    name,
                    record_type: "MGBlob".into(),
                    envelope: metadata.encode().unwrap(),
                    asset_path: None,
                },
            )
            .unwrap();
        let retained = retain_download.then(|| {
            let work = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
            assert!(matches!(work.request, CloudMediaRequest::Download { .. }));
            fixture
                .workflow
                .core
                .validate_cloud_media_work(
                    &fixture.workflow.host.collection().unwrap(),
                    work.handle.retained_for_test(),
                )
                .unwrap();
            work.handle
        });
        let ack = fixture
            .workflow
            .core
            .entity_facts_page(c::entity_codec::Kind::MediaName, None)
            .unwrap();
        std::fs::write(&file, b"").unwrap();
        service.store().register_full(|_, _| true).unwrap();
        fixture
            .workflow
            .core
            .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &service)
            .unwrap();
        if let Some(work) = retained {
            let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
            assert!(media.submit_read(work, queue.clone()).is_err());
            assert_eq!(queue.0.load(Ordering::SeqCst), 0);
        }
        assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
        assert_eq!(std::fs::read(file).unwrap(), b"");
        let after = fixture
            .workflow
            .core
            .entity_facts_page(c::entity_codec::Kind::MediaName, None)
            .unwrap();
        assert_eq!(after.len(), ack.len());
        for (before, after) in ack.iter().zip(&after) {
            assert_eq!(after.acked_envelope, before.acked_envelope);
            assert_eq!(after.acked_rev, before.acked_rev);
        }
        let target = after
            .iter()
            .find(|entity| entity.name == record_name && entity.key == b"photo.jpg")
            .expect("published target ACK");
        let before = ack.iter().find(|entity| entity.name == target.name).unwrap();
        assert!(target.acked_rev > 0 && !target.acked_envelope.0.is_empty());
        assert_eq!(target.acked_envelope, before.acked_envelope);
        assert_eq!(target.acked_envelope, initial.acked_envelope);
        assert_eq!(target.acked_rev, initial.acked_rev);
        let c::entity_codec::DecodeOutcome::Supported(entity) =
            c::entity_codec::Entity::decode(&target.acked_envelope.0).unwrap()
        else {
            panic!("supported target ACK required")
        };
        assert_eq!(entity.fields.kind, c::entity_codec::Kind::MediaName);
        assert_eq!(entity.fields.key, b"photo.jpg");
        assert!(!entity.fields.tombstone);
        if retain_download {
            let blob_name = metadata.name(&[47; 32]);
            assert_eq!(
                fixture.workflow.core.media_blobs_for_testing().unwrap()[&blob_name].download,
                m::Transfer::None,
            );
            // A later real incoming name can use the same content. Avoid a
            // local metadata scratch file changing the blob's server state.
            let payload = c::entity_codec::Payload::MediaName(metadata.media("second.jpg"));
            let record_name =
                c::entity_codec::entity_name(&[31; 32], c::entity_codec::Kind::MediaName, b"second.jpg").unwrap();
            let envelope = c::entity_codec::Entity::build(
                c::entity_codec::EntityFields {
                    kind: c::entity_codec::Kind::MediaName,
                    key: b"second.jpg".to_vec(),
                    origin: [93; 16],
                    erev: 1,
                    generation: None,
                    tombstone: false,
                    deleted_at_ms: 0,
                    writer: [94; 16],
                    base_sha256: None,
                },
                &payload.encode().unwrap(),
            )
            .unwrap()
            .encode()
            .unwrap();
            deliver_incoming(
                &fixture,
                &sdk,
                w::Saved {
                    name: record_name,
                    envelope,
                    system_fields: vec![1],
                },
                IncomingDelivery::FetchAndApply,
            );
            let download = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
            assert!(matches!(&download.request, CloudMediaRequest::Download { name } if name == &blob_name));
            fixture
                .workflow
                .core
                .validate_cloud_media_work(
                    &fixture.workflow.host.collection().unwrap(),
                    download.handle.retained_for_test(),
                )
                .unwrap();
            let stage = fixture._directory.path().join("shared-download-source");
            std::fs::write(&stage, b"synthetic server image").unwrap();
            media
                .reply(
                    download.handle,
                    CloudMediaReply::Record {
                        name: blob_name.clone(),
                        record_type: "MGBlob".into(),
                        envelope: metadata.encode().unwrap(),
                        asset_path: Some(stage.to_string_lossy().into_owned()),
                    },
                )
                .unwrap();
            assert_eq!(
                std::fs::read(service.store().media_dir().join("second.jpg")).unwrap(),
                b"synthetic server image"
            );
            assert_eq!(
                std::fs::read(service.store().media_dir().join("photo.jpg")).unwrap(),
                b""
            );
            assert_eq!(
                fixture.workflow.core.media_blobs_for_testing().unwrap()[&blob_name].download,
                m::Transfer::Complete
            );
            let first = fixture.workflow.core.entity_facts(&target.name).unwrap().unwrap();
            assert_eq!(first.acked_envelope, target.acked_envelope);
            assert_eq!(first.acked_rev, target.acked_rev);
            assert_eq!(first.acked_payload, target.acked_payload);
        }
    }
}

#[test]
fn policy_read_scope_preserves_missing_counts_and_visible_priority() {
    use crate::cloud_policy::*;
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    accept_root(&media, &sdk);
    let first = incoming_media(&fixture, &sdk, "one.asset", b"first asset");
    let second = incoming_media(&fixture, &sdk, "two.asset", b"second asset");
    let (visible, metadata) = if first.name(&[47; 32]) > second.name(&[47; 32]) {
        ("one.asset", first)
    } else {
        ("two.asset", second)
    };
    let policy = media_policy(&fixture);
    let priority = settle_media_policy_core(
        &fixture,
        policy
            .clone()
            .input(CloudPolicyEvent::VisibleMedia {
                names: vec![visible.into()],
            })
            .unwrap(),
    );
    assert_eq!(priority.len(), 1);
    let priority = priority.into_iter().next().unwrap();
    assert!(matches!(priority.action, CloudPolicyAction::PrioritizeMedia { .. }));
    assert!(
        !policy
            .clone()
            .prepare_execution(priority.handle)
            .unwrap()
            .unwrap()
            .begin_media()
            .unwrap()
    );
    let read = settle_media_policy_core(
        &fixture,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Local,
            })
            .unwrap(),
    );
    assert_eq!(read.len(), 1);
    let read = read.into_iter().next().unwrap();
    assert!(matches!(
        read.action,
        CloudPolicyAction::ReadBlobs { include_missing: false }
    ));
    assert!(
        policy
            .clone()
            .prepare_execution(read.handle)
            .unwrap()
            .unwrap()
            .begin_media()
            .unwrap()
    );
    let issued = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Reads { names } = issued.request else {
        panic!("one bounded metadata operation")
    };
    assert_eq!(names.len(), 2);
    assert_eq!(names[0], metadata.name(&[47; 32]));
    media
        .reply(
            issued.handle,
            CloudMediaReply::BlobReads {
                results: names
                    .into_iter()
                    .map(|name| CloudMediaBlobRead {
                        name,
                        result: CloudMediaBlobResult::UnknownItem,
                    })
                    .collect(),
            },
        )
        .unwrap();
    assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
    assert!(
        settle_media_policy_core(
            &fixture,
            policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Cloud,
                })
                .unwrap(),
        )
        .is_empty()
    );
    let facts = policy.status().unwrap().media;
    assert_eq!(facts.download_files, Some(2));
    assert_eq!(facts.missing_names, Some(2));
    assert_eq!(facts.unknown_blobs, Some(0));
    let permit = foreground_media_permit(&fixture, &sdk, &policy);
    assert!(permit.begin_media().unwrap());
    accept_root(&media, &sdk);
    let missing = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(missing.request, CloudMediaRequest::Reads { .. }));
}

fn begin_metadata_round(
    fixture: &PublishedFixture,
    policy: &Arc<crate::cloud_policy::CloudPolicy>,
) -> Arc<crate::cloud_policy::CloudPolicyExecution> {
    use crate::cloud_policy::*;
    let commands = settle_media_policy_core(
        fixture,
        policy
            .clone()
            .input(CloudPolicyEvent::Refresh {
                cause: CloudPolicyChangeCause::Local,
            })
            .unwrap(),
    );
    assert_eq!(commands.len(), 1);
    let command = commands.into_iter().next().unwrap();
    assert!(matches!(command.action, CloudPolicyAction::ReadBlobs { .. }));
    let execution = policy.clone().prepare_execution(command.handle).unwrap().unwrap();
    assert!(execution.begin_media().unwrap());
    execution
}

fn reply_absent(media: &Arc<CloudMedia>, issued: CloudMediaIssuedWork) {
    let CloudMediaRequest::Reads { names } = issued.request else {
        panic!("metadata round required")
    };
    media
        .reply(
            issued.handle,
            CloudMediaReply::BlobReads {
                results: names
                    .into_iter()
                    .map(|name| CloudMediaBlobRead {
                        name,
                        result: CloudMediaBlobResult::UnknownItem,
                    })
                    .collect(),
            },
        )
        .unwrap();
}

#[test]
fn metadata_action_leaves_new_local_upload_for_resume() {
    let (fixture, sdk) = published();
    let media = fixture.workflow.media().unwrap();
    let policy = media_policy(&fixture);
    assert!(foreground_media_permit(&fixture, &sdk, &policy).begin_media().unwrap());
    accept_root(&media, &sdk);
    let bytes = b"local media payload";
    fixture
        .workflow
        .host
        .media()
        .unwrap()
        .store()
        .add("local.asset", bytes)
        .unwrap();
    let pending = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    assert!(matches!(pending.request, CloudMediaRequest::Reads { .. }));
    begin_metadata_round(&fixture, &policy);
    reply_absent(
        &media,
        media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap(),
    );
    assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
    assert!(
        settle_media_policy_core(
            &fixture,
            policy
                .clone()
                .input(crate::cloud_policy::CloudPolicyEvent::Refresh {
                    cause: crate::cloud_policy::CloudPolicyChangeCause::Cloud,
                })
                .unwrap(),
        )
        .is_empty()
    );
    assert_eq!(policy.status().unwrap().media.upload_files, Some(1));
    assert!(foreground_media_permit(&fixture, &sdk, &policy).begin_media().unwrap());
    accept_root(&media, &sdk);
    let upload = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
    let CloudMediaRequest::Upload { staged_path, .. } = &upload.request else {
        panic!("resume upload required")
    };
    assert_eq!(std::fs::read(staged_path).unwrap(), bytes);
    media.queue_upload(upload.handle).unwrap();
}

#[test]
fn metadata_action_preserves_pending_and_sdk_queued_upload_work() {
    for queued in [false, true] {
        let (fixture, sdk) = published();
        let media = fixture.workflow.media().unwrap();
        let policy = media_policy(&fixture);
        assert!(foreground_media_permit(&fixture, &sdk, &policy).begin_media().unwrap());
        accept_root(&media, &sdk);
        let bytes = b"original media payload";
        fixture
            .workflow
            .host
            .media()
            .unwrap()
            .store()
            .add("first.asset", bytes)
            .unwrap();
        reply_absent(
            &media,
            media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap(),
        );
        let upload = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        let CloudMediaRequest::Upload { staged_path, .. } = &upload.request else {
            panic!("upload required")
        };
        if queued {
            media.queue_upload(upload.handle.clone()).unwrap();
        }
        fixture
            .workflow
            .host
            .media()
            .unwrap()
            .store()
            .add("second.asset", b"another media payload")
            .unwrap();
        let _ = media.clone().next_work(sdk.instance_id.clone()).unwrap();
        begin_metadata_round(&fixture, &policy);
        reply_absent(
            &media,
            media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap(),
        );
        assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
        assert_eq!(std::fs::read(staged_path).unwrap(), bytes);
        fixture
            .workflow
            .core
            .validate_cloud_media_work(
                &fixture.workflow.host.collection().unwrap(),
                upload.handle.retained_for_test(),
            )
            .unwrap();
        assert!(foreground_media_permit(&fixture, &sdk, &policy).begin_media().unwrap());
        accept_root(&media, &sdk);
        let resumed = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        assert!(resumed.handle.retained_for_test() == upload.handle.retained_for_test());
        let CloudMediaRequest::Upload {
            staged_path: resumed_path,
            ..
        } = &resumed.request
        else {
            panic!("original upload required")
        };
        assert_eq!(resumed_path, staged_path);
        assert_eq!(std::fs::read(resumed_path).unwrap(), bytes);
        media.queue_upload(resumed.handle).unwrap();
    }
}

#[test]
fn failed_metadata_and_download_reads_keep_successes_and_read_error_scope() {
    use crate::cloud_policy::*;
    use meguri_sync::cloud::policy as p;
    for metadata_failure in [true, false] {
        let (fixture, sdk) = published();
        let media = fixture.workflow.media().unwrap();
        let first = incoming_media(&fixture, &sdk, "remote-one.asset", b"first remote bytes");
        let second = incoming_media(&fixture, &sdk, "remote-two.asset", b"second remote bytes");
        let policy = media_policy(&fixture);
        let execution = foreground_media_permit(&fixture, &sdk, &policy);
        assert!(execution.begin_media().unwrap());
        accept_root(&media, &sdk);
        execution
            .operation_activity(CloudPolicyOperationFacts {
                zones: vec![CloudPolicyZone::Media],
                kind: CloudPolicyOperationKind::Read,
                started: true,
                failed: false,
                code: None,
                retry_after_ms: None,
                saved: 0,
                app_cancelled: false,
            })
            .unwrap();
        let issued = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
        let CloudMediaRequest::Reads { names } = &issued.request else {
            panic!("metadata required")
        };
        let failed_name = names[1].clone();
        let results = names
            .iter()
            .map(|name| {
                let metadata = if *name == first.name(&[47; 32]) {
                    &first
                } else {
                    &second
                };
                CloudMediaBlobRead {
                    name: name.clone(),
                    result: if metadata_failure && *name == failed_name {
                        CloudMediaBlobResult::Failed { code: 7 }
                    } else {
                        CloudMediaBlobResult::Present {
                            envelope: metadata.encode().unwrap(),
                        }
                    },
                }
            })
            .collect();
        let read = media.reply(issued.handle, CloudMediaReply::BlobReads { results });
        if metadata_failure {
            assert!(read.is_err());
        } else {
            read.unwrap();
            let download = media.clone().next_work(sdk.instance_id.clone()).unwrap().unwrap();
            assert!(matches!(download.request, CloudMediaRequest::Download { .. }));
            assert!(
                media
                    .reply(download.handle, CloudMediaReply::Failed { code: 7 })
                    .is_err()
            );
        }
        assert!(media.clone().next_work(sdk.instance_id.clone()).unwrap().is_none());
        execution
            .operation_activity(CloudPolicyOperationFacts {
                zones: vec![CloudPolicyZone::Media],
                kind: CloudPolicyOperationKind::Read,
                started: false,
                failed: true,
                code: Some(7),
                retry_after_ms: Some(120_000),
                saved: 0,
                app_cancelled: false,
            })
            .unwrap();
        let state = fixture.workflow.host.cloud_policy_driver().unwrap().state();
        let engine = state.engine.as_ref().unwrap();
        assert_eq!(engine.instance, sdk.instance_id);
        assert!(!engine.stopping);
        let error = state.last_error.unwrap();
        assert_eq!(error.class, p::OperationError::Throttled);
        assert_eq!(error.code, 7);
        assert_eq!(error.retry_after_ms, Some(120_000));
        assert_eq!(error.operation.zone, p::Zone::Media);
        assert_eq!(error.operation.kind, p::OperationKind::Read);
        assert!(!state.cycles[&error.operation].active);
        let refresh = policy_commands(
            policy
                .clone()
                .input(CloudPolicyEvent::Refresh {
                    cause: CloudPolicyChangeCause::Cloud,
                })
                .unwrap(),
        );
        assert!(!refresh.iter().any(|command| matches!(
            command.action,
            CloudPolicyAction::ReadBlobs { .. }
                | CloudPolicyAction::ResumeMedia
                | CloudPolicyAction::RequestSend { .. }
        )));
        assert!(settle_media_policy_core(&fixture, CloudPolicyOutput::Commands { commands: refresh }).is_empty());
        let facts = policy.status().unwrap().media;
        assert_eq!(facts.unknown_blobs, Some(u64::from(metadata_failure)));
        assert_eq!(facts.download_files, Some(2));
    }
}

fn fetch_published_collection(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) {
    fetch_published_collection_receipt(fixture, sdk);
    fixture
        .workflow
        .exchange_fetched(sdk.instance_id.clone(), true)
        .unwrap();
}
fn fetch_published_collection_receipt(fixture: &PublishedFixture, sdk: &CloudEngineRegistration) {
    let workflow = &fixture.workflow;
    let fetch = workflow
        .begin_fetch(sdk.instance_id.clone(), vec![sdk.collection_zone.clone()])
        .unwrap();
    let group = format!("media-exchange-{fetch}");
    workflow
        .will_fetch(sdk.instance_id.clone(), group.clone(), true)
        .unwrap();
    workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), true, false)
        .unwrap();
    let mut token = None;
    loop {
        let reply = fixture
            .server
            .lock()
            .unwrap()
            .request(
                &fixture.scope,
                Caller::default(),
                Operation::FetchZone {
                    zone: sdk.collection_zone.clone(),
                    token,
                },
            )
            .unwrap();
        let Reply::ZoneChanges(page) = reply.result.unwrap() else {
            panic!("collection page required")
        };
        let mut records = Vec::new();
        let mut deleted = Vec::new();
        for change in page.changes {
            match change {
                meguri_testkit::cloud::RecordChange::Saved(record) => {
                    let value = publication_control_record(&record).unwrap();
                    records.push(CloudRecord {
                        name: record.name,
                        envelope: value.encrypted.unwrap(),
                        system_fields: Some(value.system_fields.0),
                        ..Default::default()
                    });
                }
                meguri_testkit::cloud::RecordChange::Deleted { name, .. } => deleted.push(name),
            }
        }
        workflow
            .fetched(sdk.instance_id.clone(), sdk.collection_zone.clone(), records, deleted)
            .unwrap();
        if !page.more {
            break;
        }
        token = Some(page.token);
    }
    workflow
        .fetch_zone(sdk.instance_id.clone(), sdk.collection_zone.clone(), false, false)
        .unwrap();
    workflow.did_fetch(sdk.instance_id.clone(), group, true).unwrap();
    assert!(workflow.finish_fetch(sdk.instance_id.clone(), fetch).unwrap());
}

#[test]
fn actual_unknown_item_send_reply_drops_only_the_handed_media_tombstone() {
    handed_media_absence(false);
}

#[test]
fn actual_unknown_item_reply_settles_newer_identical_content_flight_without_ack() {
    handed_media_absence(true);
}

fn handed_media_absence(retain_older_flight: bool) {
    use meguri_sync::cloud::entity_codec as codec;
    let (fixture, sdk) = published_with_initial_media(Some(("missing.asset", b"acked synthetic content")));
    let runtime = &fixture.workflow.core;
    let name = codec::entity_name(&[31; 32], codec::Kind::MediaName, b"missing.asset").unwrap();
    let before = runtime.entity_facts(&name).unwrap().unwrap();
    assert!(before.server_erev > 0 && before.mark.is_none());
    let media = fixture.workflow.host.media().unwrap();
    media.store().trash(&["missing.asset".to_owned()]).unwrap();
    runtime
        .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &media)
        .unwrap();
    let older_flight = if retain_older_flight {
        let batch = fixture
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                sdk.collection_zone.clone(),
                fixture
                    .workflow
                    .tracking_sendables()
                    .unwrap()
                    .into_iter()
                    .filter(|(n, _)| n == &name)
                    .collect(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(batch.saves.len(), 1);
        let facts = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(facts.in_flight.len(), 1);
        let flight = facts.in_flight[0].clone();
        assert_eq!(flight.local_rev, facts.local_rev);
        media.store().add("missing.asset", b"acked synthetic content").unwrap();
        runtime
            .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &media)
            .unwrap();
        media.store().trash(&["missing.asset".to_owned()]).unwrap();
        runtime
            .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &media)
            .unwrap();
        assert!(runtime.entity_facts(&name).unwrap().unwrap().local_rev > flight.local_rev);
        Some(flight)
    } else {
        None
    };
    let exchange = fixture.workflow.begin_exchange(sdk.instance_id.clone()).unwrap();
    let selection = runtime.exchange_result(exchange).unwrap();
    assert_eq!(
        selection.selected_names,
        std::collections::BTreeSet::from([name.clone()])
    );
    assert!(selection.acked_names.is_empty());
    fetch_published_collection(&fixture, &sdk);
    // A later, unrelated edit is outside the captured selection.
    media.store().add("later.asset", b"post-capture media").unwrap();
    runtime
        .consume_media_feed(&fixture.workflow.host.collection().unwrap(), &media)
        .unwrap();
    let later_name = codec::entity_name(&[31; 32], codec::Kind::MediaName, b"later.asset").unwrap();
    let later = runtime.entity_facts(&later_name).unwrap().unwrap();
    let send = fixture.workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
    let batch = fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            fixture
                .workflow
                .tracking_sendables()
                .unwrap()
                .into_iter()
                .filter(|(n, _)| n == &name)
                .collect(),
        )
        .unwrap()
        .unwrap();
    let record = batch.saves.iter().find(|r| r.name == name).unwrap();
    assert!(
        matches!(codec::Entity::decode(&record.envelope).unwrap(), codec::DecodeOutcome::Supported(e) if e.fields.tombstone)
    );
    if let Some(older) = &older_flight {
        let current = runtime.entity_facts(&name).unwrap().unwrap();
        assert_eq!(current.in_flight.len(), 2);
        assert_eq!(current.in_flight[0].local_rev, older.local_rev);
        let handed = &current.in_flight[1];
        assert_eq!(handed.local_rev, current.local_rev);
        assert!(handed.local_rev > older.local_rev);
        assert_eq!(handed.erev, older.erev);
        assert_eq!(handed.payload_digest, older.payload_digest);
        assert!(handed.payload == older.payload);
        assert_eq!(current.acked_rev, before.acked_rev);
        assert_eq!(current.server_erev, before.server_erev);
    }
    let mut server = fixture.server.lock().unwrap();
    let deleted = server
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Delete { name: name.clone() }],
            },
        )
        .unwrap();
    assert!(matches!(deleted.result, Ok(Reply::Modified(_))));
    let response = server
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&name, "MGEntity", record.envelope.clone()),
                    if_unchanged: record.system_fields.clone().map(SystemFields::from_bytes),
                }],
            },
        )
        .unwrap();
    let Reply::Modified(items) = response.result.unwrap() else {
        panic!("native-style per-record result")
    };
    let error = items.into_iter().next().unwrap().result.err().unwrap();
    assert_eq!(error.code, ErrorCode::UnknownItem);
    drop(server);
    assert!(
        fixture
            .workflow
            .sent(
                "stale-sdk".into(),
                batch.id,
                vec![],
                vec![CloudRecordFailure {
                    name: name.clone(),
                    code: CloudEngineErrorCode::UnknownItem,
                    server_record: None,
                    zone: None,
                    native_code: None,
                    retry_after_ms: None,
                }],
                vec![],
                vec![],
                batch.saves.len() as u32
            )
            .is_err()
    );
    assert!(runtime.entity_facts(&name).unwrap().is_some());
    let trace_before = runtime.trace();
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            batch.id,
            vec![],
            vec![CloudRecordFailure {
                name: name.clone(),
                code: CloudEngineErrorCode::UnknownItem,
                server_record: None,
                zone: None,
                native_code: None,
                retry_after_ms: None,
            }],
            vec![],
            vec![],
            batch.saves.len() as u32,
        )
        .unwrap();
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    assert!(runtime.trace() == trace_before);
    assert!(runtime.entity_facts(&name).unwrap().is_none());
    assert!(!media.store().media_dir().join("missing.asset").exists());
    assert!(
        runtime
            .inbox_entries(runtime.status().unwrap().binding.unwrap().id)
            .unwrap()
            .is_empty()
    );
    let pending = fixture.workflow.tracking_sendables().unwrap();
    fixture
        .workflow
        .finish_send(sdk.instance_id.clone(), send, pending)
        .unwrap();
    assert!(!fixture.workflow.send_due(sdk.instance_id.clone()).unwrap());
    let settled = runtime.exchange_result(exchange).unwrap();
    assert_eq!(settled.selected_names, selection.selected_names);
    assert!(settled.outstanding_names.is_empty());
    assert!(settled.acked_names.is_empty());
    fetch_published_collection(&fixture, &sdk);
    let completed = fixture
        .workflow
        .complete_exchange(sdk.instance_id.clone(), exchange, true, false, false)
        .unwrap();
    assert!(matches!(completed.collection, CloudCollectionOutcome::Succeeded));
    let remaining = runtime.entity_facts(&later_name).unwrap().unwrap();
    assert_eq!(remaining.local_rev, later.local_rev);
    assert!(remaining.mark.is_some());
}

#[test]
fn missing_save_reply_preserves_recreated_media_and_other_selected_work() {
    use meguri_sync::cloud::entity_codec as codec;
    let (fixture, sdk) = published_with_initial_media(Some(("returned.asset", b"initial media")));
    let runtime = &fixture.workflow.core;
    let collection = fixture.workflow.host.collection().unwrap();
    let media = fixture.workflow.host.media().unwrap();
    let name = codec::entity_name(&[31; 32], codec::Kind::MediaName, b"returned.asset").unwrap();
    let other_name = codec::entity_name(&[31; 32], codec::Kind::MediaName, b"other.asset").unwrap();
    media.store().trash(&["returned.asset".to_owned()]).unwrap();
    media.store().add("other.asset", b"separate selection").unwrap();
    runtime.consume_media_feed(&collection, &media).unwrap();
    let exchange = fixture.workflow.begin_exchange(sdk.instance_id.clone()).unwrap();
    let selection = runtime.exchange_result(exchange).unwrap();
    assert_eq!(
        selection.selected_names,
        std::collections::BTreeSet::from([name.clone(), other_name.clone()])
    );
    fetch_published_collection(&fixture, &sdk);
    let send = fixture.workflow.begin_send(sdk.instance_id.clone()).unwrap().unwrap();
    let batch = fixture
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            fixture
                .workflow
                .tracking_sendables()
                .unwrap()
                .into_iter()
                .filter(|(n, _)| n == &name)
                .collect(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let record = &batch.saves[0];
    assert!(
        matches!(codec::Entity::decode(&record.envelope).unwrap(), codec::DecodeOutcome::Supported(e) if e.fields.tombstone)
    );
    let handed_revision = runtime.entity_facts(&name).unwrap().unwrap().local_rev;
    media
        .store()
        .add("returned.asset", b"new content after handoff")
        .unwrap();
    runtime.consume_media_feed(&collection, &media).unwrap();
    let recreated = runtime.entity_facts(&name).unwrap().unwrap();
    let other = runtime.entity_facts(&other_name).unwrap().unwrap();
    assert!(recreated.local_rev > handed_revision);
    let mut server = fixture.server.lock().unwrap();
    let removed = server
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Delete { name: name.clone() }],
            },
        )
        .unwrap();
    let Reply::Modified(removals) = removed.result.unwrap() else {
        panic!("server deletion required")
    };
    assert!(removals.into_iter().all(|item| item.result.is_ok()));
    let reply = server
        .request(
            &fixture.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&name, "MGEntity", record.envelope.clone()),
                    if_unchanged: Some(SystemFields::from_bytes(record.system_fields.clone().unwrap())),
                }],
            },
        )
        .unwrap();
    let Reply::Modified(results) = reply.result.unwrap() else {
        panic!("conditional save response required")
    };
    assert_eq!(
        results.into_iter().next().unwrap().result.err().unwrap().code,
        ErrorCode::UnknownItem
    );
    drop(server);
    let trace = runtime.trace();
    fixture
        .workflow
        .sent(
            sdk.instance_id.clone(),
            batch.id,
            vec![],
            vec![CloudRecordFailure {
                name: name.clone(),
                code: CloudEngineErrorCode::UnknownItem,
                server_record: None,
                zone: None,
                native_code: None,
                retry_after_ms: None,
            }],
            vec![],
            vec![],
            1,
        )
        .unwrap();
    fixture.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    assert!(runtime.trace() == trace);
    let retained = runtime.entity_facts(&name).unwrap().unwrap();
    assert_eq!(retained.local_rev, recreated.local_rev);
    assert!(retained.payload == recreated.payload);
    assert!(retained.mark.is_some());
    assert!(runtime.entity_facts(&other_name).unwrap().unwrap() == other);
    assert_eq!(
        std::fs::read(media.store().media_dir().join("returned.asset")).unwrap(),
        b"new content after handoff"
    );
    let pending = fixture.workflow.tracking_sendables().unwrap();
    fixture
        .workflow
        .finish_send(sdk.instance_id.clone(), send, pending)
        .unwrap();
    let result = runtime.exchange_result(exchange).unwrap();
    assert_eq!(result.outstanding_names, selection.selected_names);
    assert!(result.acked_names.is_empty());
    let completed = fixture
        .workflow
        .complete_exchange(sdk.instance_id, exchange, true, false, false)
        .unwrap();
    assert!(matches!(completed.collection, CloudCollectionOutcome::Incomplete));
}

// Store a tombstone through the test Service and forward its Saved result to
// Core through fixture callbacks. This setup does not establish a blob upload.
fn retired_policy_blob() -> (PublishedFixture, CloudEngineRegistration, String) {
    let (f, sdk) = published_with_initial_media(Some(("receipt.bin", b"synthetic receipt media")));
    let collection = f.workflow.host.collection().unwrap();
    let media = f.workflow.host.media().unwrap();
    media.store().trash(&["receipt.bin".into()]).unwrap();
    f.workflow.core.consume_media_feed(&collection, &media).unwrap();
    let batch = f
        .workflow
        .build_batch(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            f.workflow.tracking_sendables().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert!(
        batch
            .saves
            .iter()
            .any(|r| matches!(c::entity_codec::Entity::decode(&r.envelope).unwrap(),
        c::entity_codec::DecodeOutcome::Supported(e) if e.fields.tombstone))
    );
    let changes = batch
        .saves
        .iter()
        .map(|r| Mutation::Save {
            record: RecordInput::encrypted(&r.name, "MGEntity", r.envelope.clone()),
            if_unchanged: r.system_fields.clone().map(SystemFields::from_bytes),
        })
        .collect();
    let reply = f
        .server
        .lock()
        .unwrap()
        .request(
            &f.scope,
            Caller::default(),
            Operation::Modify {
                zone: sdk.collection_zone.clone(),
                atomic: false,
                changes,
            },
        )
        .unwrap()
        .result
        .unwrap();
    let Reply::Modified(items) = reply else {
        panic!("actual tombstone receipt")
    };
    let saved: Vec<_> = items
        .into_iter()
        .map(|item| {
            let MutationSuccess::Saved(record) = item.result.unwrap() else {
                panic!("actual Saved")
            };
            let value = publication_control_record(&record).unwrap();
            CloudRecord {
                name: record.name,
                envelope: value.encrypted.unwrap(),
                system_fields: Some(value.system_fields.0),
                ..Default::default()
            }
        })
        .collect();
    let count = saved.len() as u32;
    f.workflow
        .sent(sdk.instance_id.clone(), batch.id, saved, vec![], vec![], vec![], count)
        .unwrap();
    f.workflow.handler_returned(sdk.instance_id.clone()).unwrap();
    let blobs = f.workflow.core.media_blobs_for_testing().unwrap();
    assert_eq!(blobs.len(), 1);
    let (blob, state) = blobs.into_iter().next().unwrap();
    assert_eq!(state.unreferenced_since_ms, Some(NOW));
    f.policy
        .clone()
        .input(crate::cloud_policy::CloudPolicyEvent::Settings {
            settings: crate::cloud_policy::CloudPolicyDeviceSettings {
                enabled: true,
                paused: false,
                automatic: true,
                media: true,
            },
        })
        .unwrap();
    f.workflow.core.resume_cloud_media();
    let media = f.workflow.media().unwrap();
    accept_root(&media, &sdk);
    f.clock.step_ms(30 * 86_400_000);
    (f, sdk, blob)
}
fn issue_local_cleanup(f: &PublishedFixture) -> meguri_sync::cloud::policy::Command {
    use meguri_sync::cloud::policy::{ActionKind, ChangeCause, DeviceEvent};
    f.workflow
        .host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap()
        .into_iter()
        .find(|c| c.action.kind == ActionKind::RunCleanup)
        .expect("actual Driver-issued RunCleanup")
}
fn original_cleanup_drain(f: &PublishedFixture, sdk: &CloudEngineRegistration) {
    // Read collection pages from the test Service and deliver the fixture's
    // fetch callbacks to Core. Native execution is not exercised here.
    fetch_published_collection_receipt(f, sdk);
}
#[test]
fn delayed_cleanup_command_is_rejected_and_new_fetch_is_checked_once() {
    for delay_projection in [true, false] {
        let (f, sdk, blob) = retired_policy_blob();
        original_cleanup_drain(&f, &sdk);
        let early = (!delay_projection).then(|| issue_local_cleanup(&f));
        f.clock.step_ms(2 * 3_600_000);
        let command = early.unwrap_or_else(|| issue_local_cleanup(&f));
        let driver = f.workflow.host.cloud_policy_driver().unwrap();
        let collection = f.workflow.host.collection().unwrap();
        assert!(driver.compute_qualified_local_cleanup(&command, &collection).is_err());
        assert_eq!(driver.state().last_cleanup_ms, None);
        assert_eq!(
            f.workflow.core.media_blobs_for_testing().unwrap()[&blob].deletion_due_ms,
            None
        );
        assert!(
            f.workflow
                .build_batch(
                    sdk.instance_id.clone(),
                    "meguri-media".into(),
                    f.workflow.tracking_sendables().unwrap()
                )
                .unwrap()
                .is_none()
        );
        // Deliver another fixture fetch at the advanced clock before requesting a command.
        original_cleanup_drain(&f, &sdk);
        let fresh = issue_local_cleanup(&f);
        assert_ne!(fresh, command, "the issued command retains its own original receipt");
        assert!(driver.compute_qualified_local_cleanup(&command, &collection).is_err());
        driver.compute_qualified_local_cleanup(&fresh, &collection).unwrap();
        let at = NOW + 30 * 86_400_000 + 2 * 3_600_000;
        assert_eq!(
            f.workflow.core.media_blobs_for_testing().unwrap()[&blob].deletion_due_ms,
            Some(at)
        );
        assert!(driver.compute_qualified_local_cleanup(&fresh, &collection).is_err());
        let observations = f.workflow.core.observations().len();
        assert!(
            f.workflow
                .build_batch(
                    sdk.instance_id.clone(),
                    "meguri-media".into(),
                    HashMap::from([(blob.clone(), CloudPendingChange::Save)]),
                )
                .is_err()
        );
        assert_eq!(f.workflow.core.observations().len(), observations);
        let batch = f
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                "meguri-media".into(),
                f.workflow.tracking_sendables().unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(batch.deletes, vec![blob.clone()]);
        assert!(
            f.workflow
                .build_batch(
                    sdk.instance_id.clone(),
                    "meguri-media".into(),
                    HashMap::from([(blob.clone(), CloudPendingChange::Save)]),
                )
                .is_err()
        );
        let retained = f
            .workflow
            .build_batch(
                sdk.instance_id.clone(),
                "meguri-media".into(),
                HashMap::from([(blob.clone(), CloudPendingChange::Delete)]),
            )
            .unwrap()
            .unwrap();
        assert_eq!(retained.id, batch.id);
        let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
        assert!(
            f.workflow
                .submit_media_batch(sdk.instance_id.clone(), batch.id, queue.clone())
                .unwrap()
        );
        assert_eq!(queue.0.load(Ordering::SeqCst), 1);
        f.clock.step_ms(3_600_000);
        assert!(
            f.workflow
                .submit_media_batch(sdk.instance_id.clone(), batch.id, queue.clone())
                .is_err()
        );
        assert_eq!(queue.0.load(Ordering::SeqCst), 1);
        assert_eq!(
            f.workflow.core.media_blobs_for_testing().unwrap()[&blob].deletion_due_ms,
            None
        );
    }
}
#[test]
fn issued_local_cleanup_rejects_original_account_sdk_and_closed_open() {
    for changed in 0..3 {
        let (f, sdk, blob) = retired_policy_blob();
        original_cleanup_drain(&f, &sdk);
        let command = issue_local_cleanup(&f);
        let collection = f.workflow.host.collection().unwrap();
        let driver = f.workflow.host.cloud_policy_driver().unwrap();
        match changed {
            0 => {
                f.workflow
                    .account_changed(Some(sdk.instance_id.clone()), CloudAccountStatus::NoAccount, None)
                    .unwrap();
            }
            1 => {
                f.workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
            }
            _ => {
                f.workflow.host.close().unwrap();
            }
        }
        assert!(driver.compute_qualified_local_cleanup(&command, &collection).is_err());
        assert_eq!(driver.state().last_cleanup_ms, None);
        if changed != 2 {
            assert_eq!(
                f.workflow.core.media_blobs_for_testing().unwrap()[&blob].deletion_due_ms,
                None
            );
        }
    }
}

#[test]
fn other_owner_copy_is_rechecked_by_real_cleanup_builder_and_final_queue_admission() {
    use meguri_sync::cloud::entity_codec as codec;
    for phase in 0..3 {
        let (f, sdk, blob) = retired_policy_blob();
        let runtime = &f.workflow.core;
        let collection = f.workflow.host.collection().unwrap();
        original_cleanup_drain(&f, &sdk);
        let cleanup = issue_local_cleanup(&f);
        let driver = f.workflow.host.cloud_policy_driver().unwrap();
        if phase > 0 {
            driver.compute_qualified_local_cleanup(&cleanup, &collection).unwrap();
        }
        let selected = (phase == 2).then(|| {
            f.workflow
                .build_batch(
                    sdk.instance_id.clone(),
                    "meguri-media".into(),
                    f.workflow.tracking_sendables().unwrap(),
                )
                .unwrap()
                .unwrap()
        });
        // This fixture is a copy in real typed tracking storage. No pending
        // list, Saved result or UI conflict resolution is supplied by it.
        let metadata = runtime.media_blobs_for_testing().unwrap()[&blob].metadata.clone();
        c::runtime::with_tracking_access(|| {
            let scope = c::BuildScope {
                container: c::ContainerId::new("test.cleanup.foreign").unwrap(),
                environment: c::Environment::Production,
            };
            let mut store = match c::TrackingStore::open(collection.path().parent().unwrap(), scope).unwrap() {
                c::OpenOutcome::Opened(s) => s,
                _ => panic!("existing fixture store"),
            };
            let owner = store
                .create_binding(
                    c::BindingFacts {
                        account: c::OpaqueBytes(vec![90]),
                        epoch: c::EpochId([91; 16]),
                        record_key: c::RecordKey([92; 32]),
                        publication: serde_json::from_str("[1,2]").unwrap(),
                        state: c::BindingState::Isolated(c::IsolationReason::SignedOut),
                    },
                    f.clock.as_ref(),
                    &SeededRandom::new(913),
                )
                .unwrap();
            let payload = codec::Payload::MediaName(metadata.media("preserved.bin"));
            let envelope = codec::Entity::build(
                codec::EntityFields {
                    kind: codec::Kind::MediaName,
                    key: b"preserved.bin".to_vec(),
                    origin: [93; 16],
                    erev: 1,
                    generation: None,
                    tombstone: false,
                    deleted_at_ms: 0,
                    writer: owner.id.0,
                    base_sha256: None,
                },
                &payload.encode().unwrap(),
            )
            .unwrap()
            .encode()
            .unwrap();
            store
                .store_conflict(
                    owner.id,
                    &c::ConflictCopy {
                        id: c::CopyId([94; 16]),
                        kind: c::ConflictKind::MediaName,
                        key: c::OpaqueBytes(b"preserved.bin".to_vec()),
                        reason: c::ConflictReason::MediaContent,
                        losing_envelope: c::OpaqueBytes(envelope),
                        losing_payload_digest: c::PayloadDigest(payload.payload_digest().unwrap()),
                        winning_payload_digest: c::PayloadDigest([95; 32]),
                        field_names: None,
                        template_names: None,
                        note_deck: None,
                        staged_path: None,
                        created_at_ms: NOW,
                        resolved_at_ms: None,
                    },
                )
                .unwrap();
        });
        if phase == 0 {
            driver.compute_qualified_local_cleanup(&cleanup, &collection).unwrap();
        }
        if let Some(batch) = selected {
            assert_eq!(batch.deletes, vec![blob.clone()]);
            let queue = Arc::new(ReadQueue(AtomicUsize::new(0)));
            assert!(
                f.workflow
                    .submit_media_batch(sdk.instance_id.clone(), batch.id, queue.clone())
                    .is_err()
            );
            assert_eq!(queue.0.load(Ordering::SeqCst), 0);
        } else {
            assert!(
                f.workflow
                    .build_batch(
                        sdk.instance_id.clone(),
                        "meguri-media".into(),
                        f.workflow.tracking_sendables().unwrap()
                    )
                    .unwrap()
                    .is_none()
            );
        }
        assert_eq!(runtime.media_blobs_for_testing().unwrap()[&blob].deletion_due_ms, None);
        assert_ne!(
            runtime.media_blobs_for_testing().unwrap()[&blob].server,
            m::ServerState::Deleted
        );
    }
}
