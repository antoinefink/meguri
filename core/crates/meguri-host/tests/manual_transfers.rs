//! RF4 focused manual collection transfers: scripted transport, real host,
//! storage, package writer, forced backup and R6. No external service is used.
use bytes::Bytes;
use meguri_engine::{
    EventKind, OpSource, OpSpec, Subscription,
    device_state::{DeviceState, PublicationDecision},
    replace::{ReplaceReason, ReplacementStage},
};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask, ProcessCore};
use meguri_model::{
    clock::{Clock, FixedClock, UtcOffset},
    config::ConfigEntry,
    ids::{CardId, DeckId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
    rng::SeededRandom,
    time::{EpochMillis, EpochSecs},
    usn::Usn,
};
use meguri_package::{CollectionPackageInspectContext, inspect_collection_package};
use meguri_storage::{ConfigValue, OpenOptions, ReadRows, ReaderRole, StampMode, open_collection, space::StatvfsSpace};
use meguri_sync::{
    Endpoint, HostKey, JobOptions, Method, ServerKind, SyncError, SyncTransport,
    oneway::{Preview, TransferAccess, TransferError},
    policy::{Acceptance, Boundary, CopyKind, Direction, Operation, Phase, Scope},
    transport::{BoxFuture, RawBody, TransportRequest, TransportResponse},
};
use sha2::{Digest, Sha256};
use std::{
    collections::VecDeque,
    fs,
    future::poll_fn,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
const NOW: i64 = 1_894_701_600_000;
static LANE: Mutex<()> = Mutex::new(());
fn config(root: &Path, clock: Arc<dyn Clock>) -> HostConfig {
    let collection = root.join("collection");
    fs::create_dir_all(&collection).unwrap();
    HostConfig::new(
        collection,
        root.join("backups"),
        root.join("widget"),
        clock,
        Arc::new(SeededRandom::new(918)),
        Arc::new(StatvfsSpace),
    )
}
fn fixed() -> Arc<dyn Clock> {
    Arc::new(FixedClock::new(NOW, UtcOffset::UTC))
}
fn seeded(root: &Path) -> (CoreHost, HostConfig) {
    let config = config(root, fixed());
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    (host, config)
}
fn settle(host: &CoreHost) {
    let c = host.collection().unwrap();
    c.run(OpSpec::named("RF4 barrier", OpSource::Rollover), |_| Ok(()))
        .unwrap();
    // Reader snapshots fix the committed seq; queue completion precedes JobFinished.
}
fn complete<T>(events: Subscription, task: ManualTransferTask<T>) -> Result<T, TransferError> {
    loop {
        let event = events.recv().expect("engine stream stays open");
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == task.job.id) {
            break;
        }
    }
    task.take_result().expect("worker supplies a typed result")
}
fn prepare(
    host: &CoreHost,
    script: &Arc<Script>,
    direction: Direction,
    scope: Scope,
) -> Result<Preview, TransferError> {
    let events = host.engine().stream().subscribe();
    let task = host
        .prepare_manual_transfer(access(script, "Synthetic User", "synthetic-key"), direction, scope)
        .unwrap();
    complete(events, task)
}
fn confirm(host: &CoreHost, script: &Arc<Script>, preview: Preview) -> Result<Operation, TransferError> {
    let events = host.engine().stream().subscribe();
    let task = host
        .confirm_manual_transfer(access(script, "Synthetic User", "rotated-key"), preview.confirmation)
        .unwrap();
    complete(events, task)
}
fn access(script: &Arc<Script>, user: &str, key: &str) -> TransferAccess {
    let endpoint = Endpoint::parse("http://127.0.0.1:28711/prefix/").unwrap();
    TransferAccess::new(
        endpoint.clone(),
        user,
        HostKey::new(key),
        JobOptions::new(endpoint, ServerKind::SelfHosted),
    )
    .unwrap()
    .with_transport_for_testing(Box::new(script.clone()))
}
fn digest(path: &Path) -> [u8; 32] {
    Sha256::digest(fs::read(path).unwrap()).into()
}
fn assert_transfer_modes(config: &HostConfig, operation: &Operation) {
    use std::os::unix::fs::PermissionsExt;
    let live = config.collection_directory.join("collection.anki2");
    let retained = config
        .manual_transfer_directory
        .join(operation.id.as_str())
        .join(&operation.candidate.as_ref().unwrap().name);
    assert_eq!(fs::metadata(live).unwrap().permissions().mode() & 0o777, 0o600);
    assert_eq!(fs::metadata(retained).unwrap().permissions().mode() & 0o777, 0o400);
}
fn source_bytes(config: &HostConfig) -> (Vec<u8>, Vec<u8>) {
    let path = config.collection_directory.join("collection.anki2");
    (
        fs::read(&path).unwrap(),
        fs::read(path.with_file_name("collection.anki2-wal")).unwrap_or_default(),
    )
}
fn current_facts(host: &CoreHost) -> meguri_storage::one_way::LocalTransferFacts {
    let c = host.collection().unwrap();
    let lease = c.reader(ReaderRole::Sync).unwrap();
    lease.transfer_facts().unwrap()
}
fn server(root: &Path) -> (PathBuf, Vec<u8>) {
    fs::create_dir_all(root).unwrap();
    let path = root.join("collection.anki2");
    let clock = FixedClock::new(NOW, UtcOffset::UTC);
    let random = SeededRandom::new(75);
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let mut scope = db.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut col = scope.col().unwrap();
    col.modified = EpochMillis(NOW - 1000);
    col.schema_modified = EpochMillis(NOW - 5000);
    col.usn = Usn(8);
    scope.put_col(&col).unwrap();
    scope
        .put_config_entry(&ConfigEntry {
            key: "RF4-server-nondefault".into(),
            value: b"[1,2,3]".to_vec(),
            usn: Usn(-1),
            mtime: EpochSecs(123),
        })
        .unwrap();
    scope
        .put(&RevlogEntry {
            id: RevlogId(NOW - 100),
            card_id: CardId(7),
            usn: Usn(-1),
            button: 3,
            interval: 1,
            last_interval: 0,
            ease_factor: 2000,
            answer_millis: 1500,
            kind: RevlogKind::Review,
        })
        .unwrap();
    scope.commit().unwrap();
    db.close().unwrap();
    (path.clone(), fs::read(path).unwrap())
}
fn meta(modified: i64, usn: i32) -> Vec<u8> {
    format!(
        r#"{{"mod":{modified},"scm":{},"usn":{usn},"ts":{},"msg":"","cont":true,"empty":true,"media_usn":0}}"#,
        NOW - 5000,
        NOW / 1000
    )
    .into_bytes()
}
struct Step {
    method: Method,
    reply: Reply,
}
enum Reply {
    Bytes(Vec<u8>),
    Lost,
    Cancelled,
    Status(u16),
}
#[derive(Default)]
struct Script {
    steps: Mutex<VecDeque<Step>>,
    calls: Mutex<Vec<(String, Vec<u8>)>>,
}
impl Script {
    fn new(steps: Vec<(Method, Reply)>) -> Arc<Self> {
        Arc::new(Self {
            steps: Mutex::new(
                steps
                    .into_iter()
                    .map(|(method, reply)| Step { method, reply })
                    .collect(),
            ),
            calls: Mutex::default(),
        })
    }
    fn count(&self, method: Method) -> usize {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .filter(|(url, _)| url.ends_with(&format!("/{}", method.name())))
            .count()
    }
    fn uploaded(&self) -> Vec<u8> {
        self.calls
            .lock()
            .unwrap()
            .iter()
            .find(|(url, _)| url.ends_with("/upload"))
            .unwrap()
            .1
            .clone()
    }
    fn assert_done(&self) {
        assert!(self.steps.lock().unwrap().is_empty());
    }
}
struct Body(Option<Bytes>);
impl RawBody for Body {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> {
        Box::pin(async move { Ok(self.0.take()) })
    }
}
impl SyncTransport for Arc<Script> {
    fn post(&self, mut request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(async move {
            let step = self
                .steps
                .lock()
                .unwrap()
                .pop_front()
                .expect("no automatic/unapproved request");
            assert!(request.url.ends_with(&format!("/{}", step.method.name())));
            assert!(request.url.starts_with("http://127.0.0.1:28711/prefix/"));
            let mut compressed = Vec::new();
            while let Some(bytes) = poll_fn(|cx| request.body.as_mut().poll_next(cx)).await {
                compressed.extend_from_slice(&bytes.unwrap());
            }
            let decoded = zstd::stream::decode_all(compressed.as_slice()).unwrap();
            self.calls.lock().unwrap().push((request.url, decoded));
            match step.reply {
                Reply::Cancelled => Err(SyncError::Cancelled),
                Reply::Lost => Err(SyncError::Network(meguri_sync::NetworkError {
                    cause: meguri_sync::NetworkCause::Connection,
                    head_arrived: true,
                    detail: "scripted lost answer".into(),
                })),
                Reply::Status(status) => Ok(TransportResponse {
                    status,
                    headers: vec![],
                    body: Box::new(Body(None)),
                }),
                Reply::Bytes(bytes) => Ok(TransportResponse {
                    status: 200,
                    headers: vec![("anki-original-size".into(), bytes.len().to_string().into_bytes())],
                    body: Box::new(Body(Some(Bytes::from(
                        zstd::stream::encode_all(bytes.as_slice(), 3).unwrap(),
                    )))),
                }),
            }
        })
    }
}
fn export_steps(bytes: Vec<u8>, answer: Reply) -> Arc<Script> {
    Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Upload, answer),
    ])
}
fn inspect(config: &HostConfig, path: &Path) -> meguri_package::InspectedCollection {
    inspect_collection_package(
        path,
        &CollectionPackageInspectContext {
            collection_directory: &config.collection_directory,
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: config.clock.as_ref(),
            random: config.random.as_ref(),
            space: config.disk_space_provider.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .unwrap()
}

#[test]
fn rf4_export_preserves_zero_card_nondefault_server_copy_then_activates_exact_upload() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let (_, bytes) = server(&root.path().join("server"));
    let script = export_steps(bytes, Reply::Bytes(b"OK".to_vec()));
    assert_eq!(script.count(Method::Meta), 0); // host open and provider construction are offline.
    let source = source_bytes(&config);
    let prior = DeviceState::for_collection(host.collection().unwrap().path())
        .publication()
        .unwrap()
        .unwrap();
    let preview = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(source_bytes(&config), source);
    assert_eq!(preview.operation.remote.as_ref().unwrap().modified, NOW - 1000);
    assert_eq!(preview.operation.remote.as_ref().unwrap().usn, 8);
    let copy = preview.operation.server_copy.as_ref().unwrap();
    assert_eq!(copy.kind, CopyKind::VerifiedPackage);
    assert_eq!(copy.served_facts.as_ref().unwrap().cards, 0);
    assert_eq!(copy.served_facts.as_ref().unwrap().reviews, 1);
    let server_copy_path = config.backups_directory.join(&copy.artifact.name);
    let candidate = config
        .manual_transfer_directory
        .join(preview.operation.id.as_str())
        .join("candidate.anki2");
    let expected = digest(&candidate);
    let completed = confirm(&host, &script, preview).unwrap();
    script.assert_done();
    let uploaded_digest: [u8; 32] = Sha256::digest(script.uploaded()).into();
    assert_eq!(uploaded_digest, expected);
    assert_eq!(digest(&candidate), expected); // independent retained bytes survive activation.
    assert_eq!(completed.phase, Phase::Complete);
    assert_transfer_modes(&config, &completed);
    assert!(completed.receipt.is_some());
    let publication = DeviceState::for_collection(host.collection().unwrap().path())
        .publication()
        .unwrap()
        .unwrap();
    assert_eq!(publication.decision, prior.decision);
    let evidence = DeviceState::for_collection(host.collection().unwrap().path())
        .replacement_evidence()
        .unwrap()
        .unwrap();
    assert_eq!(evidence.candidate_file.sha256, expected);
    let facts = current_facts(&host);
    assert_eq!(facts.modified.0, NOW);
    assert_eq!(facts.schema_modified.0, NOW);
    assert_eq!(facts.last_sync.0, NOW);
    host.collection()
        .unwrap()
        .run(
            OpSpec::named("write after upload activation", OpSource::UserNonUndoable),
            |ctx| {
                ctx.scope().set_config("RF4-after-upload", &ConfigValue::Bool(true))?;
                Ok(())
            },
        )
        .unwrap();
    host.close().unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    {
        let collection = host.collection().unwrap();
        let reader = collection.reader(ReaderRole::Sync).unwrap();
        assert_eq!(
            reader.config_value("RF4-after-upload").unwrap(),
            Some(ConfigValue::Bool(true))
        );
    }
    assert_transfer_modes(&config, &completed);
    assert_eq!(digest(&candidate), expected);
    // The published server-copy package is a real restore candidate, preserving
    // zero-card configuration/history. Restore through the existing primitive.
    let package = inspect(&config, &server_copy_path);
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.replace_verified(
        ReplacementStage::from_staging(package.into_staged().into_directory()).unwrap(),
        ReplaceReason::Restore,
        |_| Ok(()),
    )
    .unwrap();
    drop(slot);
    let c = host.collection().unwrap();
    let r = c.reader(ReaderRole::Sync).unwrap();
    assert_eq!(r.transfer_facts().unwrap().reviews, 1);
    assert!(r.config_entry("RF4-server-nondefault").unwrap().is_some());
    drop(r);
    host.close().unwrap();
}
#[test]
fn rf4_import_backups_persisted_reviews_and_creates_fresh_pending_cloud_generation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let c = host.collection().unwrap();
    c.run(OpSpec::named("persist a review", OpSource::UserNonUndoable), |ctx| {
        ctx.scope().put(&RevlogEntry {
            id: RevlogId(NOW),
            card_id: CardId(6),
            usn: Usn(-1),
            button: 4,
            interval: 2,
            last_interval: 1,
            ease_factor: 2500,
            answer_millis: 700,
            kind: RevlogKind::Review,
        })?;
        Ok(())
    })
    .unwrap();
    settle(&host);
    let prior = DeviceState::for_collection(c.path()).publication().unwrap().unwrap();
    let (_, bytes) = server(&root.path().join("server"));
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
    ]);
    let preview = prepare(&host, &script, Direction::Import, Scope::CollectionOnly).unwrap();
    assert!(preview.operation.safety_backups.is_empty());
    let done = confirm(&host, &script, preview).unwrap();
    script.assert_done();
    assert_eq!(done.phase, Phase::Complete);
    assert_eq!(done.safety_backups.len(), 1);
    assert_transfer_modes(&config, &done);
    let backed = inspect(&config, &config.backups_directory.join(&done.safety_backups[0].name));
    assert_eq!(backed.facts().revlog, 1);
    let now = DeviceState::for_collection(c.path()).publication().unwrap().unwrap();
    assert_ne!(now.identity, prior.identity);
    assert_eq!(now.decision, PublicationDecision::Pending);
    assert_eq!(current_facts(&host).last_sync.0, NOW - 1000);
    assert!(matches!(
        c.reader(ReaderRole::Query),
        Err(meguri_engine::EngineError::StaleGeneration { .. })
    ));
    host.close().unwrap();
}
#[test]
fn rf4_failed_server_copy_blocks_upload_and_source_bytes_survive() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(root.path());
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Lost),
    ]);
    let before = source_bytes(&config);
    assert!(prepare(&host, &script, Direction::Export, Scope::CollectionOnly).is_err());
    script.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(source_bytes(&config), before);
    let history = host.manual_transfer_history().history().unwrap();
    assert_eq!(history.len(), 1);
    assert!(history[0].receipt.is_none());
    assert_eq!(history[0].safety_backups.len(), 1);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    host.close().unwrap();
}
#[test]
fn rf4_unverified_server_copy_keeps_exact_raw_bytes_and_never_reports_known_counts() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(root.path());
    let raw = b"synthetic damaged server SQLite".to_vec();
    let script = export_steps(raw.clone(), Reply::Bytes(b"OK".to_vec()));
    let preview = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
    let copy = preview.operation.server_copy.as_ref().unwrap();
    assert_eq!(copy.kind, CopyKind::UnverifiedRaw);
    assert!(copy.served_facts.is_none());
    assert_eq!(
        fs::read(config.backups_directory.join(&copy.artifact.name)).unwrap(),
        raw
    );
    confirm(&host, &script, preview).unwrap();
    script.assert_done();
    host.close().unwrap();
}
#[test]
fn rf4_complete_refusal_and_nonexact_ok_never_mutate_local_or_consume_cooldown() {
    let _lane = LANE.lock().unwrap();
    for answer in [
        Reply::Bytes(b"OK\n".to_vec()),
        Reply::Status(403),
        Reply::Bytes(vec![b'x'; 20_000]),
    ] {
        let root = tempfile::tempdir().unwrap();
        let (host, config) = seeded(&root.path().join("local"));
        let (_, bytes) = server(&root.path().join("server"));
        let script = export_steps(bytes, answer);
        let before = source_bytes(&config);
        let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
        let account = p.operation.account.clone();
        let error = confirm(&host, &script, p).unwrap_err();
        assert!(!format!("{error:?}").contains("xxxxx"));
        assert_eq!(source_bytes(&config), before);
        script.assert_done();
        assert!(host.manual_transfer_history().active().unwrap().is_none());
        assert_eq!(host.manual_transfer_history().next_eligible_at(&account).unwrap(), None);
        host.close().unwrap();
    }
}
#[test]
fn rf4_lost_ok_and_killed_intent_keep_global_overlap_guard_across_restart_and_identity_change() {
    let _lane = LANE.lock().unwrap();
    for kill_intent in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (host, config) = seeded(&root.path().join("local"));
        let (_, bytes) = server(&root.path().join("server"));
        let script = export_steps(bytes, Reply::Lost);
        let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
        if kill_intent {
            meguri_model::faultpoint::arm(
                "oneway.upload.after_intent",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        assert!(confirm(&host, &script, p).is_err());
        meguri_model::faultpoint::disarm_all();
        let retained = host.manual_transfer_history().active().unwrap().unwrap();
        assert_eq!(retained.acceptance, Acceptance::PossiblyAccepted);
        assert_eq!(retained.phase, Phase::NeedsDecision);
        assert!(host.cancel_prepared_manual_transfer(&retained.id, false).is_err());
        host.close().unwrap();
        let restarted = CoreHost::open(config).unwrap();
        let no_requests = Script::new(vec![]);
        let events = restarted.engine().stream().subscribe();
        let task = restarted
            .prepare_manual_transfer(
                access(&no_requests, "Other Account", "new-key"),
                Direction::Import,
                Scope::CollectionOnly,
            )
            .unwrap();
        assert!(matches!(
            complete(events, task),
            Err(TransferError::Policy(meguri_sync::policy::PolicyError::Overlap(_)))
        ));
        assert!(no_requests.calls.lock().unwrap().is_empty());
        let events = restarted.engine().stream().subscribe();
        let task = restarted.prepare_manual_transfer_recovery(retained.id).unwrap();
        assert!(matches!(
            complete(events, task),
            Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown))
        ));
        restarted.close().unwrap();
    }
}
#[test]
fn rf4_known_ok_recovery_after_restart_activates_retained_digest_without_uploading_again() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let (_, bytes) = server(&root.path().join("server"));
    let script = export_steps(bytes, Reply::Bytes(b"OK".to_vec()));
    let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
    let expected = p.operation.candidate.as_ref().unwrap().identity.sha256;
    meguri_model::faultpoint::arm(
        "oneway.upload.after_ok",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    assert!(confirm(&host, &script, p).is_err());
    meguri_model::faultpoint::disarm_all();
    script.assert_done();
    let op = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::ExactOk);
    host.close().unwrap();
    let restarted = CoreHost::open(config).unwrap();
    settle(&restarted);
    let events = restarted.engine().stream().subscribe();
    let task = restarted.prepare_manual_transfer_recovery(op.id).unwrap();
    let preview = complete(events, task).unwrap();
    let no_network = Script::new(vec![]);
    let done = confirm(&restarted, &no_network, preview).unwrap();
    assert_eq!(done.phase, Phase::Complete);
    assert!(no_network.calls.lock().unwrap().is_empty());
    let evidence = DeviceState::for_collection(restarted.collection().unwrap().path())
        .replacement_evidence()
        .unwrap()
        .unwrap();
    assert_eq!(evidence.candidate_file.sha256, expected);
    restarted.close().unwrap();
}
#[test]
fn rf4_selected_media_stays_partial_and_never_starts_media_or_success_cooldown() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _) = seeded(&root.path().join("local"));
    let (_, bytes) = server(&root.path().join("server"));
    let script = export_steps(bytes, Reply::Bytes(b"OK".to_vec()));
    let p = prepare(&host, &script, Direction::Export, Scope::CollectionAndMedia).unwrap();
    assert!(matches!(
        confirm(&host, &script, p),
        Err(TransferError::MediaIncomplete { .. })
    ));
    script.assert_done();
    let done = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(done.scope, Scope::CollectionAndMedia);
    assert_eq!(done.phase, Phase::MediaIncomplete);
    assert!(done.collection_complete);
    assert!(done.receipt.is_none());
    assert_eq!(
        host.manual_transfer_history().next_eligible_at(&done.account).unwrap(),
        None
    );
    assert!(host.manual_transfer_history().active().unwrap().is_some());
    host.close().unwrap();
}

#[test]
fn rf4_stale_sequence_generation_account_remote_and_artifact_never_upload() {
    let _lane = LANE.lock().unwrap();
    for changed in ["sequence", "generation", "account", "remote", "artifact"] {
        let root = tempfile::tempdir().unwrap();
        let (host, config) = seeded(&root.path().join("local"));
        let (_, bytes) = server(&root.path().join("server"));
        let script = Script::new(vec![
            (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
            (Method::Download, Reply::Bytes(bytes)),
            (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        ]);
        let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
        script.assert_done();
        let c = host.collection().unwrap();
        if changed == "sequence" {
            // Same frozen millisecond and unchanged col stamps. The committed
            // operation/admission guard still invalidates the decision.
            c.run(
                OpSpec::named("same stamp new work", OpSource::Maintenance).stamp(StampMode::Raw),
                |ctx| {
                    ctx.scope().set_config("RF4-new-work", &ConfigValue::Bool(true))?;
                    Ok(())
                },
            )
            .unwrap();
            settle(&host);
        } else if changed == "generation" {
            let stage = ReplacementStage::new(&config.collection_directory).unwrap();
            let source = config
                .manual_transfer_directory
                .join(p.operation.id.as_str())
                .join("candidate.anki2");
            fs::copy(source, stage.path()).unwrap();
            let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
            slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(()))
                .unwrap();
            drop(slot);
        } else if changed == "artifact" {
            let source = config
                .manual_transfer_directory
                .join(p.operation.id.as_str())
                .join("candidate.anki2");
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&source, fs::Permissions::from_mode(0o600)).unwrap();
            fs::write(source, b"changed private test artifact").unwrap();
        }
        let final_script = if changed == "remote" {
            Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW, 9)))])
        } else if changed == "artifact" {
            Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))])
        } else {
            Script::new(vec![])
        };
        let events = host.engine().stream().subscribe();
        let task = host
            .confirm_manual_transfer(
                access(
                    &final_script,
                    if changed == "account" {
                        "Different User"
                    } else {
                        "Synthetic User"
                    },
                    "rotated-key",
                ),
                p.confirmation,
            )
            .unwrap();
        assert!(complete(events, task).is_err(), "changed {changed} must refuse");
        assert_eq!(final_script.count(Method::Upload), 0);
        host.close().unwrap();
    }
}
fn add_review_card(host: &CoreHost) {
    let c = host.collection().unwrap();
    c.run(OpSpec::named("RF4 review policy", OpSource::Maintenance), |ctx| {
        ctx.scope().set_config("fsrs", &ConfigValue::Bool(true))?;
        ctx.scope()
            .set_config("loadBalancerEnabled", &ConfigValue::Bool(false))?;
        Ok(())
    })
    .unwrap();
    let kind = meguri_engine::notetypes::add_stock(
        &c,
        meguri_engine::notetypes::StockKind::Basic,
        Some("RF4 review".into()),
    )
    .unwrap()
    .value
    .id;
    meguri_engine::notes::add(
        &c,
        meguri_engine::notes::AddNote {
            notetype_id: kind,
            fields: vec!["RF4 front".into(), "RF4 back".into()],
            tags: vec![],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    settle(host);
}
static RACE_HOST: Mutex<Option<Arc<CoreHost>>> = Mutex::new(None);
fn open_review_between_admission_and_drain() {
    let host = RACE_HOST.lock().unwrap().as_ref().unwrap().clone();
    assert!(matches!(
        host.review().start(DeckId(1), None).unwrap(),
        meguri_engine::session::StartResult::Started { .. }
    ));
}
#[test]
fn rf4_active_review_and_post_drain_race_refuse_before_backup_or_remote_request() {
    let _lane = LANE.lock().unwrap();
    for race in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (host, _) = seeded(root.path());
        let host = Arc::new(host);
        add_review_card(&host);
        if race {
            *RACE_HOST.lock().unwrap() = Some(host.clone());
            meguri_model::faultpoint::arm(
                "oneway.before_exclusive",
                meguri_model::faultpoint::Fault::Callback(open_review_between_admission_and_drain),
            );
        } else {
            host.review().start(DeckId(1), None).unwrap();
        }
        let no_requests = Script::new(vec![]);
        assert!(matches!(
            prepare(&host, &no_requests, Direction::Export, Scope::CollectionOnly),
            Err(TransferError::SessionActive)
        ));
        meguri_model::faultpoint::disarm_all();
        *RACE_HOST.lock().unwrap() = None;
        assert!(no_requests.calls.lock().unwrap().is_empty());
        assert!(
            host.manual_transfer_history()
                .history()
                .unwrap()
                .iter()
                .all(|op| op.safety_backups.is_empty())
        );
        // Closing owns the existing review drain protocol.
        host.close().unwrap();
    }
}
#[test]
fn rf4_refused_download_keeps_source_and_unopened_import_retains_raw_copy() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, local_config) = seeded(&root.path().join("local"));
    let before = source_bytes(&local_config);
    let invalid = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(b"not a collection".to_vec())),
    ]);
    assert!(matches!(
        prepare(&host, &invalid, Direction::Import, Scope::CollectionOnly),
        Err(TransferError::InvalidDownload(_))
    ));
    assert_eq!(source_bytes(&local_config), before);
    assert!(!local_config.backups_directory.exists());
    host.close().unwrap();
    let unopened_options = config(&root.path().join("unreadable_path"), fixed());
    let unreadable_path = unopened_options.collection_directory.join("collection.anki2");
    fs::write(&unreadable_path, b"fixture database cannot be opened").unwrap();
    let process = ProcessCore::new(unopened_options.engine.clone()).unwrap();
    let import_host = process.open_for_manual_import(unopened_options.clone()).unwrap();
    let (_, bytes) = server(&root.path().join("server"));
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
    ]);
    let p = prepare(&import_host, &script, Direction::Import, Scope::CollectionOnly).unwrap();
    let done = confirm(&import_host, &script, p).unwrap();
    script.assert_done();
    assert_eq!(done.phase, Phase::Complete);
    let raw = &done.safety_backups[0];
    assert!(raw.name.starts_with("corrupt-"));
    assert_eq!(
        fs::read(unopened_options.backups_directory.join(&raw.name)).unwrap(),
        b"fixture database cannot be opened"
    );
    import_host.close().unwrap();
}

#[test]
fn rf4_import_into_absent_collection_needs_no_backup_and_emits_one_replacement() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let options = config(&root.path().join("empty"), fixed());
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open_for_manual_import(options).unwrap();
    let (_, bytes) = server(&root.path().join("server"));
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
    ]);
    let p = prepare(&host, &script, Direction::Import, Scope::CollectionOnly).unwrap();
    let observer = host.engine().stream().subscribe();
    let done = confirm(&host, &script, p).unwrap();
    assert!(done.safety_backups.is_empty());
    // JobFinished follows the single replacement event; consume through it.
    let mut replacements = 0;
    loop {
        let event = observer.recv().unwrap();
        if matches!(event.kind, EventKind::CollectionReplaced { .. }) {
            replacements += 1;
        }
        if matches!(event.kind, EventKind::JobFinished { .. }) {
            break;
        }
    }
    assert_eq!(replacements, 1);
    host.close().unwrap();
}

#[test]
fn rf4_absent_import_recovers_after_durable_intent_with_fresh_confirmation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let options = config(&root.path().join("empty"), fixed());
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open_for_manual_import(options.clone()).unwrap();
    let (_, bytes) = server(&root.path().join("server"));
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
    ]);
    let prepared = prepare(&host, &script, Direction::Import, Scope::CollectionOnly).unwrap();
    let stale = prepared.clone();
    let candidate = options
        .manual_transfer_directory
        .join(prepared.operation.id.as_str())
        .join("candidate.anki2");
    let expected = digest(&candidate);
    meguri_model::faultpoint::arm(
        "replace.after_pause_recorded",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    assert!(matches!(
        confirm(&host, &script, prepared),
        Err(TransferError::Replacement { activated: false, .. })
    ));
    meguri_model::faultpoint::disarm_all();
    script.assert_done();
    let interrupted = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(interrupted.phase, Phase::NeedsDecision);
    assert!(interrupted.activation_identity.is_some());
    assert!(!options.collection_directory.join("collection.anki2").exists());
    assert_eq!(digest(&candidate), expected);
    assert!(host.cancel_prepared_manual_transfer(&interrupted.id, false).is_err());
    host.close().unwrap();
    drop(host);
    drop(process);

    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open_for_manual_import(options.clone()).unwrap();
    assert!(host.collection().is_err());
    let no_requests = Script::new(vec![]);
    assert!(confirm(&host, &no_requests, stale).is_err());
    let events = host.engine().stream().subscribe();
    let task = host.prepare_manual_transfer_recovery(interrupted.id.clone()).unwrap();
    let preview = complete(events, task).unwrap();
    assert!(preview.operation.revision > interrupted.revision);
    assert_eq!(preview.operation.candidate, interrupted.candidate);
    assert_eq!(preview.operation.activation_identity, interrupted.activation_identity);
    assert_eq!(digest(&candidate), expected);
    let events = host.engine().stream().subscribe();
    let task = host
        .confirm_manual_transfer(
            access(&no_requests, "Another User", "another-key"),
            preview.confirmation.clone(),
        )
        .unwrap();
    assert!(matches!(
        complete(events, task),
        Err(TransferError::Stale(Boundary::IdentityChanged))
    ));
    assert!(no_requests.calls.lock().unwrap().is_empty());
    let final_script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))]);
    let observer = host.engine().stream().subscribe();
    let replay = preview.clone();
    let done = confirm(&host, &final_script, preview).unwrap();
    final_script.assert_done();
    assert_eq!(done.id, interrupted.id);
    assert_eq!(done.phase, Phase::Complete);
    assert!(done.safety_backups.is_empty());
    assert_eq!(done.candidate, interrupted.candidate);
    assert_eq!(digest(&candidate), expected);
    assert_transfer_modes(&options, &done);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert_eq!(host.manual_transfer_history().history().unwrap().len(), 1);
    let device = DeviceState::for_collection(host.collection().unwrap().path());
    assert_eq!(
        device.publication().unwrap().unwrap().decision,
        PublicationDecision::Pending
    );
    let evidence = device.replacement_evidence().unwrap().unwrap();
    assert_eq!(evidence.reason, ReplaceReason::FullDownload);
    assert!(evidence.old_file.is_none());
    assert_eq!(
        evidence.candidate_file.sha256,
        done.activation_identity.as_ref().unwrap().sha256
    );
    let mut replacements = 0;
    loop {
        let event = observer.recv().unwrap();
        if matches!(event.kind, EventKind::CollectionReplaced { .. }) {
            replacements += 1;
        }
        if matches!(event.kind, EventKind::JobFinished { .. }) {
            break;
        }
    }
    assert_eq!(replacements, 1);
    assert!(confirm(&host, &no_requests, replay).is_err());
    assert_eq!(final_script.count(Method::Upload), 0);
    host.close().unwrap();
}

#[test]
fn rf4_absent_import_process_kill_recovery_preserves_r6_and_refuses_changed_source() {
    if let Ok(root) = std::env::var("MEGURI_RF4_IMPORT_CHILD_ROOT") {
        let root = PathBuf::from(root);
        let options = config(&root.join("empty"), fixed());
        let process = ProcessCore::new(options.engine.clone()).unwrap();
        let host = process.open_for_manual_import(options).unwrap();
        let bytes = fs::read(root.join("server/collection.anki2")).unwrap();
        let script = Script::new(vec![
            (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
            (Method::Download, Reply::Bytes(bytes)),
            (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
            (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        ]);
        let preview = prepare(&host, &script, Direction::Import, Scope::CollectionOnly).unwrap();
        meguri_model::faultpoint::arm("replace.after_pause_recorded", meguri_model::faultpoint::Fault::Abort);
        let _ = confirm(&host, &script, preview);
        panic!("expected termination before the rename");
    }
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let options = config(&root.path().join("empty"), fixed());
    server(&root.path().join("server"));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rf4_absent_import_process_kill_recovery_preserves_r6_and_refuses_changed_source",
            "--nocapture",
        ])
        .env("MEGURI_RF4_IMPORT_CHILD_ROOT", root.path())
        .status()
        .unwrap();
    use std::os::unix::process::ExitStatusExt;
    assert_eq!(status.signal(), Some(6));
    let process = ProcessCore::new(options.engine.clone()).unwrap();
    let host = process.open_for_manual_import(options.clone()).unwrap();
    let interrupted = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(interrupted.phase, Phase::Activating);
    assert_eq!(interrupted.acceptance, Acceptance::NotSent);
    let candidate = options
        .manual_transfer_directory
        .join(interrupted.id.as_str())
        .join("candidate.anki2");
    let expected = digest(&candidate);
    let live = options.collection_directory.join("collection.anki2");
    assert!(!live.exists());
    let sidecar_path = options.collection_directory.join("meguri-sidecar.json");
    let journal = fs::read(&sidecar_path).unwrap();
    let events = host.engine().stream().subscribe();
    let task = host.prepare_manual_transfer_recovery(interrupted.id.clone()).unwrap();
    let preview = complete(events, task).unwrap();
    assert_eq!(fs::read(&sidecar_path).unwrap(), journal);
    fs::write(&live, b"unexpected file at the formerly absent source").unwrap();
    let no_requests = Script::new(vec![]);
    assert!(matches!(
        confirm(&host, &no_requests, preview),
        Err(TransferError::Stale(Boundary::LocalChanged))
    ));
    assert_eq!(
        fs::read(&live).unwrap(),
        b"unexpected file at the formerly absent source"
    );
    assert!(no_requests.calls.lock().unwrap().is_empty());
    assert_eq!(fs::read(&sidecar_path).unwrap(), journal);
    assert_eq!(digest(&candidate), expected);
    fs::remove_file(live).unwrap();
    let events = host.engine().stream().subscribe();
    let task = host.prepare_manual_transfer_recovery(interrupted.id).unwrap();
    let preview = complete(events, task).unwrap();
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))]);
    let done = confirm(&host, &script, preview).unwrap();
    script.assert_done();
    assert_eq!(done.phase, Phase::Complete);
    assert_eq!(done.candidate, interrupted.candidate);
    assert_eq!(digest(&candidate), expected);
    assert_transfer_modes(&options, &done);
    assert_eq!(
        DeviceState::for_collection(host.collection().unwrap().path())
            .publication()
            .unwrap()
            .unwrap()
            .decision,
        PublicationDecision::Pending
    );
    host.close().unwrap();
}

#[test]
fn rf4_post_rename_recovery_reports_actual_generation_and_does_not_replace_twice() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let (_, bytes) = server(&root.path().join("server"));
    let script = export_steps(bytes, Reply::Bytes(b"OK".to_vec()));
    let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
    let before = host.engine().generation().0;
    meguri_model::faultpoint::arm(
        "replace.after_rename",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let error = confirm(&host, &script, p).unwrap_err();
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(error, TransferError::Replacement { activated: true, .. }));
    let op = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::ExactOk);
    assert_eq!(op.actual_generation, Some(before + 1));
    host.close().unwrap();
    let restarted = CoreHost::open(config).unwrap();
    settle(&restarted);
    let generation = restarted.engine().generation();
    let events = restarted.engine().stream().subscribe();
    let task = restarted.prepare_manual_transfer_recovery(op.id).unwrap();
    let p = complete(events, task).unwrap();
    let no_network = Script::new(vec![]);
    let done = confirm(&restarted, &no_network, p).unwrap();
    assert_eq!(done.phase, Phase::Complete);
    assert_eq!(restarted.engine().generation(), generation);
    assert!(no_network.calls.lock().unwrap().is_empty());
    restarted.close().unwrap();
}
#[test]
fn rf4_cancelled_upload_retains_guard_and_success_cooldown_is_shared_between_directions() {
    let _lane = LANE.lock().unwrap();
    for cancelled in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (host, _) = seeded(&root.path().join("local"));
        let (_, bytes) = server(&root.path().join("server"));
        let script = export_steps(
            bytes,
            if cancelled {
                Reply::Cancelled
            } else {
                Reply::Bytes(b"OK".to_vec())
            },
        );
        let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
        if cancelled {
            assert!(matches!(confirm(&host, &script, p), Err(TransferError::Cancelled)));
            assert_eq!(
                host.manual_transfer_history().active().unwrap().unwrap().acceptance,
                Acceptance::PossiblyAccepted
            );
        } else {
            let done = confirm(&host, &script, p).unwrap();
            let no_requests = Script::new(vec![]);
            assert!(matches!(
                prepare(&host, &no_requests, Direction::Import, Scope::CollectionOnly),
                Err(TransferError::Policy(meguri_sync::policy::PolicyError::Cooldown { .. }))
            ));
            assert_eq!(done.receipt.unwrap().next_eligible_at, NOW + 3_600_000);
            assert!(no_requests.calls.lock().unwrap().is_empty());
        }
        host.close().unwrap();
    }
}
#[test]
fn rf4_process_kill_after_upload_intent_preserves_evidence_and_overlap() {
    if let Ok(root) = std::env::var("MEGURI_RF4_CHILD_ROOT") {
        let root = PathBuf::from(root);
        let options = config(&root.join("local"), fixed());
        let host = CoreHost::open(options).unwrap();
        settle(&host);
        let bytes = fs::read(root.join("server/collection.anki2")).unwrap();
        let script = export_steps(bytes, Reply::Bytes(b"OK".to_vec()));
        let p = prepare(&host, &script, Direction::Export, Scope::CollectionOnly).unwrap();
        meguri_model::faultpoint::arm("oneway.upload.after_intent", meguri_model::faultpoint::Fault::Abort);
        let _ = confirm(&host, &script, p);
        panic!("fault must kill before upload");
    }
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, options) = seeded(&root.path().join("local"));
    host.close().unwrap();
    server(&root.path().join("server"));
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "rf4_process_kill_after_upload_intent_preserves_evidence_and_overlap",
            "--nocapture",
        ])
        .env("MEGURI_RF4_CHILD_ROOT", root.path())
        .status()
        .unwrap();
    assert!(!status.success());
    let reopened = CoreHost::open(options).unwrap();
    let op = reopened.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::PossiblyAccepted);
    assert!(op.candidate.is_some());
    assert!(op.original.is_some());
    assert!(op.server_copy.is_some());
    assert!(reopened.cancel_prepared_manual_transfer(&op.id, false).is_err());
    reopened.close().unwrap();
}
#[test]
fn rf4_server_copy_publication_failure_refuses_upload_and_keeps_real_local_backup() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let (_, bytes) = server(&root.path().join("server"));
    let script = Script::new(vec![
        (Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
    ]);
    let source = source_bytes(&config);
    meguri_model::faultpoint::arm(
        "package.server_copy.before_publish",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    assert!(prepare(&host, &script, Direction::Export, Scope::CollectionOnly).is_err());
    meguri_model::faultpoint::disarm_all();
    script.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(source_bytes(&config), source);
    let history = host.manual_transfer_history().history().unwrap();
    assert_eq!(history[0].safety_backups.len(), 1);
    assert!(history[0].receipt.is_none());
    host.close().unwrap();
}
#[test]
fn rf4_low_space_preflight_refuses_before_any_request() {
    use meguri_storage::{StorageError, space::SpaceProvider};
    struct NoSpace;
    impl SpaceProvider for NoSpace {
        fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
            Ok(0)
        }
    }
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (_, mut options) = seeded(root.path());
    // The temporary host has closed on drop before reopening with this provider.
    options.disk_space_provider = Arc::new(NoSpace);
    let host = CoreHost::open(options).unwrap();
    let no_requests = Script::new(vec![]);
    assert!(matches!(
        prepare(&host, &no_requests, Direction::Import, Scope::CollectionOnly),
        Err(TransferError::DiskFull { .. })
    ));
    assert!(no_requests.calls.lock().unwrap().is_empty());
    host.close().unwrap();
}
