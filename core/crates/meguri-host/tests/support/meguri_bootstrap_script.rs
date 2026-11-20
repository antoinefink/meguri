//! Synthetic transport on the real retained transfer/backup/publication path.
use bytes::Bytes;
use meguri_engine::{EventKind, OpSource, OpSpec, Subscription};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask, MeguriAccess, MeguriBootstrapDirection};
use meguri_model::{clock::{Clock, FixedClock, UtcOffset}, config::ConfigEntry,
    rng::SeededRandom, time::{EpochMillis, EpochSecs}, usn::Usn};
use meguri_storage::{OpenOptions, ReadRows, StampMode, open_collection, space::StatvfsSpace};
use meguri_sync::{HostKey, Method, SyncError, SyncTransport,
    oneway::{MediaPreview, Preview, TransferError}, policy::Operation,
    provider::{MeguriConnection, Selection},
    transport::{BoxFuture, RawBody, TransportRequest, TransportResponse}};
use std::{collections::VecDeque, fs, future::poll_fn, path::{Path, PathBuf}, sync::{Arc, Mutex}};

pub const NOW: i64 = 1_894_701_600_000;
pub const ENDPOINT: &str = "https://bootstrap.synthetic.example/";
pub const USER: &str = "synthetic-user";
pub static LANE: Mutex<()> = Mutex::new(());
pub fn config(root: &Path, clock: Arc<dyn Clock>) -> HostConfig {
    let collection = root.join("collection");
    fs::create_dir_all(&collection).unwrap();
    HostConfig::new(collection, root.join("backups"), root.join("widget"), clock,
        Arc::new(SeededRandom::new(918)), Arc::new(StatvfsSpace))
}
pub fn seeded(root: &Path) -> (CoreHost, HostConfig) {
    let config = config(root, Arc::new(FixedClock::new(NOW, UtcOffset::UTC)));
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(), config.random.as_ref()).unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    settle(&host);
    (host, config)
}
pub fn settle(host: &CoreHost) {
    host.collection().unwrap().run(OpSpec::named("bootstrap barrier", OpSource::Rollover), |_| Ok(())).unwrap();
}
pub fn complete<T>(events: Subscription, task: ManualTransferTask<T>) -> Result<T, TransferError> {
    loop {
        let event = events.recv().expect("engine stream stays open");
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == task.job.id) { break; }
    }
    task.take_result().expect("worker supplies a typed result")
}
pub fn server(root: &Path) -> (PathBuf, Vec<u8>) {
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
    scope.put_config_entry(&ConfigEntry { key: "bootstrap-server".into(), value: b"[1,2,3]".to_vec(),
        usn: Usn(-1), mtime: EpochSecs(123) }).unwrap();
    scope.commit().unwrap();
    db.close().unwrap();
    (path.clone(), fs::read(path).unwrap())
}
pub fn meta(modified: i64, usn: i32) -> Vec<u8> {
    format!(r#"{{"mod":{modified},"scm":{},"usn":{usn},"ts":{},"msg":"","cont":true,"empty":true,"media_usn":0}}"#,
        NOW - 5000, NOW / 1000).into_bytes()
}
struct Step { method: Method, reply: Reply }
pub enum Reply {
    Bytes(Vec<u8>),
    Lost,
    /// Runs the action when the request has arrived, then answers with the bytes.
    #[allow(dead_code)]
    After(Box<dyn FnOnce() + Send>, Vec<u8>),
}
#[derive(Default)]
pub struct Script { steps: Mutex<VecDeque<Step>>, calls: Mutex<Vec<String>> }
impl Script {
    pub fn new(steps: Vec<(Method, Reply)>) -> Arc<Self> {
        Arc::new(Self { steps: Mutex::new(steps.into_iter().map(|(method, reply)| Step { method, reply }).collect()),
            calls: Mutex::default() })
    }
    pub fn count(&self, method: Method) -> usize {
        self.calls.lock().unwrap().iter().filter(|url| url.ends_with(&format!("/{}", method.name()))).count()
    }
    pub fn assert_done(&self) { assert!(self.steps.lock().unwrap().is_empty()); }
}
struct Body(Option<Bytes>);
impl RawBody for Body {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> {
        Box::pin(async move { Ok(self.0.take()) })
    }
}
pub struct ScriptTransport(pub Arc<Script>);
impl SyncTransport for ScriptTransport {
    fn post(&self, mut request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(async move {
            let step = self.0.steps.lock().unwrap().pop_front().expect("no automatic/unapproved request");
            assert!(request.url.starts_with(ENDPOINT));
            assert!(request.url.ends_with(&format!("/{}", step.method.name())));
            let mut compressed = Vec::new();
            while let Some(bytes) = poll_fn(|cx| request.body.as_mut().poll_next(cx)).await {
                compressed.extend_from_slice(&bytes.unwrap());
            }
            zstd::stream::decode_all(compressed.as_slice()).unwrap();
            self.0.calls.lock().unwrap().push(request.url);
            let reply = match step.reply {
                Reply::After(action, bytes) => { action(); Reply::Bytes(bytes) }
                other => other,
            };
            match reply {
                Reply::After(..) => unreachable!("the action ran above"),
                Reply::Lost => Err(SyncError::Network(meguri_sync::NetworkError {
                    cause: meguri_sync::NetworkCause::Connection, head_arrived: true, detail: "scripted lost answer".into() })),
                Reply::Bytes(bytes) => Ok(TransportResponse { status: 200,
                    headers: vec![("anki-original-size".into(), bytes.len().to_string().into_bytes())],
                    body: Box::new(Body(Some(Bytes::from(zstd::stream::encode_all(bytes.as_slice(), 3).unwrap())))) }),
            }
        })
    }
}
pub fn access(connection: &MeguriConnection, script: &Arc<Script>) -> MeguriAccess {
    MeguriAccess::new(connection.clone(), USER, HostKey::new("synthetic-key")).unwrap()
        .with_transport_for_testing(Box::new(ScriptTransport(script.clone())))
}
pub fn select(host: &CoreHost) -> MeguriConnection {
    let connection = host.configure_meguri_connection(ENDPOINT).unwrap();
    let handoff = host.begin_provider_handoff(Selection::Meguri).unwrap();
    host.finish_provider_handoff(&handoff).unwrap();
    connection
}
pub fn collection_script(bytes: Vec<u8>, upload: bool) -> Arc<Script> {
    let mut steps = vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(bytes)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))];
    if upload { steps.push((Method::Upload, Reply::Bytes(b"OK".to_vec()))); }
    Script::new(steps)
}
pub fn prepare(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>, direction: MeguriBootstrapDirection) -> Preview {
    let events = host.engine().stream().subscribe();
    complete(events, host.prepare_meguri_bootstrap(access(connection, script), direction).unwrap()).unwrap()
}
pub fn confirm(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>, preview: Preview) -> Operation {
    let events = host.engine().stream().subscribe();
    let op = complete(events, host.confirm_meguri_bootstrap(access(connection, script), preview).unwrap()).unwrap();
    script.assert_done();
    op
}
pub fn selected_collection(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation) {
    let (host, config) = seeded(&root.join("local"));
    let connection = select(&host);
    let (_, bytes) = server(&root.join("server"));
    let script = collection_script(bytes, true);
    let preview = prepare(&host, &connection, &script, MeguriBootstrapDirection::UseDeviceCollection);
    let op = confirm(&host, &connection, &script, preview);
    assert!(op.collection_complete && op.receipt.is_none());
    (host, config, connection, op)
}
pub fn media_preview(host: &CoreHost, connection: &MeguriConnection, op: &Operation) -> MediaPreview {
    let offline = Script::new(vec![]);
    let events = host.engine().stream().subscribe();
    let preview = complete(events, host.prepare_meguri_bootstrap_media(access(connection, &offline), op.id.clone()).unwrap()).unwrap();
    assert_eq!(offline.count(Method::MediaBegin), 0);
    offline.assert_done();
    preview
}
pub fn media_run(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>, preview: MediaPreview) -> Operation {
    let events = host.engine().stream().subscribe();
    let result = complete(events, host.run_meguri_bootstrap_media(access(connection, script), preview.confirmation).unwrap()).unwrap();
    script.assert_done();
    for method in [Method::Upload, Method::Download, Method::Meta] { assert_eq!(script.count(method), 0); }
    result
}
pub fn media_answer(data: serde_json::Value) -> Reply {
    Reply::Bytes(serde_json::to_vec(&serde_json::json!({"data": data, "err": ""})).unwrap())
}
pub fn media_begin(usn: u64) -> Reply { media_answer(serde_json::json!({"usn": usn, "sk": "synthetic"})) }
pub fn finish_empty_media(host: &CoreHost, connection: &MeguriConnection, op: &Operation) -> Operation {
    media_run(host, connection, &Script::new(vec![(Method::MediaBegin, media_begin(0))]), media_preview(host, connection, op))
}
pub fn ready(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation) {
    let (host, config, connection, op) = selected_collection(root);
    let op = finish_empty_media(&host, &connection, &op);
    assert!(host.complete_meguri_bootstrap(&connection, USER, &op.id).unwrap().is_complete());
    (host, config, connection, op)
}
