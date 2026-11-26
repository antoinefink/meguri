//! The exported MP7 path, including its real access factory. Only the remote
//! peer is synthetic; no access-source or worker-result substitution.
use super::*;
use meguri_engine::{EventKind, OpSource, OpSpec, device_state::DeviceState};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask, MeguriBootstrapDirection};
use meguri_model::{clock::{StepClock, UtcOffset}, rng::SeededRandom};
use meguri_storage::{OpenOptions, ReadRows, ReaderRole, StampMode, Table, open_collection};
use meguri_sync::{oneway::TransferError, provider::{MeguriReadiness, Selection}};
use meguri_testkit::sync_stub::{Reply, SyncStub, TestRoot};
use serde_json::json;
use std::{collections::VecDeque, path::Path, sync::mpsc, time::Duration};

const NOW: i64 = 1_894_701_600_000;
const USER: &str = "synthetic-user";
type Replies = Arc<Mutex<VecDeque<(&'static str, Vec<u8>, Option<mpsc::Receiver<()>>)>>>;
fn push(replies: &Replies, path: &'static str, payload: Vec<u8>) {
    replies.lock().unwrap().push_back((path, payload, None));
}
fn meta(modified: i64, schema: i64, usn: i32) -> Vec<u8> {
    serde_json::to_vec(&json!({"mod": modified, "scm": schema, "usn": usn, "ts": NOW / 1000,
        "msg": "", "cont": true, "empty": true, "media_usn": 0})).unwrap()
}
fn media() -> Vec<u8> { br#"{"data":{"usn":0,"sk":"synthetic"},"err":""}"#.to_vec() }
fn server_copy(root: &Path, modified: i64, schema: i64, usn: i32) -> Vec<u8> {
    std::fs::create_dir_all(root).unwrap();
    let path = root.join("collection.anki2");
    let clock = StepClock::new(NOW, UtcOffset::UTC, "UTC");
    let random = SeededRandom::new(761);
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let mut writer = db.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut col = writer.col().unwrap();
    col.modified.0 = modified; col.schema_modified.0 = schema; col.usn.0 = usn;
    writer.put_col(&col).unwrap(); writer.commit().unwrap(); db.close().unwrap();
    std::fs::read(path).unwrap()
}
fn wait(host: &CoreHost, events: &meguri_engine::Subscription, id: u64) {
    let deadline = meguri_engine::monotonic::now() + Duration::from_secs(20);
    while host.engine().jobs().running().iter().any(|job| job.id.0 == id) {
        let event = events.recv_timeout(deadline.saturating_sub(meguri_engine::monotonic::now()))
            .expect("production-path worker deadline");
        if matches!(event.kind, EventKind::JobFinished { id: done, .. } if done.0 == id) { break; }
    }
}
fn transfer<T>(host: &CoreHost, start: impl FnOnce() -> Result<ManualTransferTask<T>, meguri_host::MeguriBootstrapError>) -> T {
    let events = host.engine().stream().subscribe();
    let task = start().unwrap(); wait(host, &events, task.job.id.0);
    task.take_result().unwrap().unwrap_or_else(|_: TransferError| panic!("actual bootstrap worker failed"))
}
fn credentials(connection: &ProviderConnection) -> ProviderCredentials {
    ProviderCredentials { connection: super::super::information(&connection.inner), username: USER.into(), hostkey: "synthetic-key".into() }
}
fn started(start: ProviderStructureStart) -> Arc<ProviderStructureJob> {
    match start { ProviderStructureStart::Started { job } => job, _ => panic!("production MP7 admission required") }
}
fn drive(job: &ProviderStructureJob, mut upload_gate: Option<mpsc::Sender<()>>, wrong_publication: bool) -> ProviderStructureUpload {
    let host = &job.owner.host;
    loop {
        let events = host.engine().stream().subscribe();
        match job.advance().unwrap() {
            ProviderStructureStep::Running { stage, handle } => {
                if stage == ProviderStructureStage::Upload { upload_gate.take().unwrap().send(()).unwrap(); }
                wait(host, &events, handle.id);
                if stage == ProviderStructureStage::Upload && wrong_publication {
                    // Model an unrelated durable replacement with a reused process
                    // generation. Its publication alone must fence this old task.
                    let collection = host.collection().unwrap();
                    let generation = collection.generation();
                    let path = collection.path().parent().unwrap().join("meguri-sidecar.json");
                    meguri_storage::sidecar::update_document(&path, |doc| {
                        doc.get_mut("deviceState").unwrap()["active"]["identity"] = json!([813, 927]);
                        Ok(())
                    }).unwrap();
                    assert_eq!(host.collection().unwrap().generation(), generation);
                }
            }
            ProviderStructureStep::Finished { outcome: ProviderStructureOutcome::Saved { upload } } => return upload,
            _ => panic!("deletion must commit through the production path"),
        }
    }
}

fn deletion(offline_then_upload: bool, wrong_publication: bool) {
    let root = tempfile::tempdir().unwrap();
    let replies: Replies = Arc::default();
    let queued = replies.clone();
    let ca = TestRoot::generate();
    let stub = SyncStub::serve_tls(&ca.server_cert(&["127.0.0.1"]), move |head| {
        let (expected, bytes, gate) = queued.lock().unwrap().pop_front().expect("no unapproved request");
        assert_eq!(head.path(), expected);
        if let Some(gate) = gate { gate.recv_timeout(Duration::from_secs(20)).expect("upload phase admitted"); }
        Reply::ok(&bytes)
    });
    let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    let directory = root.path().join("local"); std::fs::create_dir_all(&directory).unwrap();
    let mut config = HostConfig::new(directory, root.path().join("backups"), root.path().join("widget"),
        clock.clone(), Arc::new(SeededRandom::new(381)), Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.path().join("installation");
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"), config.clock.as_ref(), config.random.as_ref()).unwrap();
    let host = Arc::new(CoreHost::open(config.clone()).unwrap());
    host.collection().unwrap().run(OpSpec::named("fixture barrier", OpSource::Rollover), |_| Ok(())).unwrap();
    host.install_meguri_fixture_transport(host.cloud_connection_generation().unwrap(),
        &config.collection_directory.canonicalize().unwrap().join("collection.anki2"),
        &config.manual_transfer_directory, &stub.url(), &ca.der()).unwrap();
    let inner = host.configure_meguri_connection(&stub.url()).unwrap();
    let handoff = host.begin_provider_handoff(Selection::Meguri).unwrap(); host.finish_provider_handoff(&handoff).unwrap();
    let setup = ProviderConnection { owner: Arc::new(ProviderOwner { host: host.clone(), generation: host.cloud_connection_generation().unwrap() }), inner };
    let access = || setup.access(credentials(&setup)).unwrap();
    let collection = host.collection().unwrap();
    let notetype = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        reader.ids_after(Table::Notetypes, None, 100).unwrap().into_iter()
            .find(|id| reader.notetype(NotetypeId(*id)).unwrap().unwrap().name == "Basic").unwrap()
    };
    meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote { notetype_id: NotetypeId(notetype),
        fields: vec!["synthetic question".into(), "synthetic answer".into()], tags: vec![], deck_id: meguri_model::ids::DeckId(1),
        guid: None, update_add_defaults: false }, OpSource::UserUndoable).unwrap();
    drop(collection);
    push(&replies, "/sync/meta", meta(NOW - 2000, NOW - 5000, 7));
    push(&replies, "/sync/download", server_copy(&root.path().join("initial-server"), NOW - 1000, NOW - 5000, 8));
    for _ in 0..2 { push(&replies, "/sync/meta", meta(NOW - 1000, NOW - 5000, 8)); }
    push(&replies, "/sync/upload", b"OK".to_vec()); push(&replies, "/msync/begin", media());
    let preview = transfer(&host, || host.prepare_meguri_bootstrap(access(), MeguriBootstrapDirection::UseDeviceCollection));
    let op = transfer(&host, || host.confirm_meguri_bootstrap(access(), preview));
    let preview = transfer(&host, || host.prepare_meguri_bootstrap_media(access(), op.id));
    let op = transfer(&host, || host.run_meguri_bootstrap_media(access(), preview.confirmation));
    assert!(host.complete_meguri_bootstrap(&setup.inner, USER, &op.id).unwrap().is_complete());
    clock.set_now_ms(NOW + 60_000);
    let connection = ProviderConnection { owner: Arc::new(ProviderOwner { host: host.clone(), generation: host.cloud_connection_generation().unwrap() }), inner: setup.inner.clone() };
    let before = host.collection().unwrap().reader(ReaderRole::Query).unwrap().col().unwrap();
    let remove = ProviderStructureOp::Remove { request: NotetypeRemoval { notetype_id: notetype, confirmed_notes: 1, confirmed_cards: 1 } };
    if !offline_then_upload { push(&replies, "/sync/meta", meta(before.modified.0, before.schema_modified.0, before.usn.0)); }
    // Production credentials are still checked before starting the round.
    let mut wrong = credentials(&connection); wrong.connection.revision += 1;
    assert!(matches!(connection.start_structure_save(wrong, remove.clone(), true).unwrap(),
        ProviderStructureStart::Refused { refusal: ProviderStructureRefusal::NotReady }));
    push(&replies, "/sync/meta", meta(before.last_sync.0, before.schema_modified.0, before.usn.0));
    push(&replies, "/sync/meta", meta(before.last_sync.0, before.schema_modified.0, before.usn.0));
    let served_mod = before.last_sync.0 + 500;
    push(&replies, "/sync/download", server_copy(&root.path().join("structure-server"), served_mod, before.schema_modified.0, before.usn.0 + 1));
    for _ in 0..2 { push(&replies, "/sync/meta", meta(served_mod, before.schema_modified.0, before.usn.0 + 1)); }
    let (release, blocked) = mpsc::channel();
    replies.lock().unwrap().push_back(("/sync/upload", b"OK".to_vec(), Some(blocked)));
    if !wrong_publication { push(&replies, "/msync/begin", media()); }
    let job = started(connection.start_structure_save(credentials(&connection), remove, !offline_then_upload).unwrap());
    let deletion_job = job.clone();
    let job = if offline_then_upload {
        assert!(matches!(drive(&job, None, false), ProviderStructureUpload::Pending { reason: ProviderUploadPending::Unreachable }));
        assert!(host.meguri_structure_upload_pending().unwrap());
        started(connection.start_structure_upload(credentials(&connection)).unwrap())
    } else { job };
    let upload = drive(&job, Some(release), wrong_publication);
    assert!(matches!(upload, ProviderStructureUpload::Uploaded { media_complete } if media_complete != wrong_publication));
    assert_ne!(host.cloud_connection_generation().unwrap(), connection.owner.generation);
    assert!(connection.owner.validate().is_err(), "activation must not refresh the ordinary owner");
    assert!(connection.start_structure_upload(credentials(&connection)).is_err());
    assert!(connection.start_structure_save(credentials(&connection), ProviderStructureOp::Remove {
        request: NotetypeRemoval { notetype_id: notetype, confirmed_notes: 1, confirmed_cards: 1 } }, true).is_err());
    assert!(matches!(connection.start_media_round(credentials(&connection)).unwrap(),
        super::super::normal::ProviderRoundStart::Refused { refusal: ProviderRoundRefusal::Interrupted }));
    let saved = deletion_job.saved().unwrap().expect("typed deletion result retained");
    assert!(matches!(saved, ProviderStructureSaved::Removed { removed } if removed.removed && removed.notes_removed == 1 && removed.cards_removed == 1));
    let collection = host.collection().unwrap();
    assert!(collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(notetype)).unwrap().is_none());
    let publication = DeviceState::for_collection(collection.path()).settled_publication().unwrap().unwrap();
    let requests = stub.requests();
    let uploads: Vec<_> = requests.iter().filter(|request| request.path() == "/sync/upload").collect();
    assert_eq!(uploads.len(), 2);
    let sent = root.path().join("uploaded.anki2");
    std::fs::write(&sent, uploads[1].body.as_ref().expect("decoded actual upload")).unwrap();
    let db = open_collection(&sent, &OpenOptions::new(config.clock.as_ref(), config.random.as_ref())).unwrap();
    assert!(db.open_reader(ReaderRole::Query).unwrap().notetype(NotetypeId(notetype)).unwrap().is_none());
    db.close().unwrap();
    assert_eq!(requests.iter().filter(|request| request.path() == "/msync/begin").count(), if wrong_publication { 1 } else { 2 });
    assert!(replies.lock().unwrap().is_empty());
    if wrong_publication {
        assert_eq!(serde_json::to_value(publication.identity).unwrap(), json!([813, 927]));
        assert!(!matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
    } else {
        assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
        let doc = host.provider_authority().snapshot().unwrap();
        assert!(doc.meguri_structure.is_none() && doc.meguri_bootstrap.is_none());
        assert!(host.manual_transfer_history().active().unwrap().is_none());
        assert!(!host.meguri_structure_upload_pending().unwrap());
    }
    host.close().unwrap();
}

#[test]
fn ffi_online_delete_completes_media_after_its_own_activation() { deletion(false, false); }
#[test]
fn ffi_pending_delete_upload_completes_media_after_its_own_activation() { deletion(true, false); }
#[test]
fn ffi_delete_media_refuses_wrong_publication_even_with_unchanged_generation() { deletion(false, true); }
