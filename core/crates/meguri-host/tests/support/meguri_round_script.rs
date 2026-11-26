//! Deterministic generated protocol replies, captured through SyncTransport.
use super::bootstrap as b;
use bytes::Bytes;
use meguri_engine::{OpSource, OpSpec};
use meguri_host::{CoreHost, MeguriAccess, MeguriRoundStart, MeguriRoundTask};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::{HostKey, Method, SyncError, SyncTransport,
    provider::MeguriConnection,
    transport::{BoxFuture, RawBody, TransportRequest, TransportResponse}};
use serde_json::{Value, json};
use std::{collections::{BTreeMap, VecDeque}, future::poll_fn, sync::{Arc, Mutex, mpsc}, time::Duration};

pub struct Step { pub method: Method, pub reply: Vec<u8>, pub lost: bool, pub after: Option<Box<dyn FnOnce() + Send>> }
impl Step {
    pub fn new(method: Method, reply: Vec<u8>) -> Self { Self { method, reply, lost: false, after: None } }
    pub fn after(mut self, body: impl FnOnce() + Send + 'static) -> Self { self.after = Some(Box::new(body)); self }
}
pub fn envelope(value: Value) -> Vec<u8> { serde_json::to_vec(&json!({"data": value, "err": ""})).unwrap() }
pub fn begin(usn: u64) -> Step { Step::new(Method::MediaBegin, envelope(json!({"usn": usn, "sk": "synthetic"}))) }
pub fn sanity(ok: bool) -> Step { Step::new(Method::MediaSanity, envelope(json!(if ok { "OK" } else { "mediaSanity" }))) }
pub fn changes(value: Value) -> Step { Step::new(Method::MediaChanges, envelope(value)) }
pub fn meta(host: &CoreHost, modified: i64, usn: i32) -> Vec<u8> {
    let collection = host.collection().unwrap();
    let col = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    serde_json::to_vec(&json!({"mod": modified, "scm": col.schema_modified.0, "usn": usn,
        "ts": b::NOW / 1000, "msg": "", "cont": true, "empty": false, "media_usn": 0})).unwrap()
}
pub fn normal(host: &CoreHost, chunk: Value, changes: Value, usn: i32) -> Vec<Step> {
    let local = host.collection().unwrap().reader(ReaderRole::Query).unwrap().col().unwrap();
    vec![Step::new(Method::Meta, meta(host, local.modified.0 - 1000, usn)),
        Step::new(Method::Start, br#"{"cards":[],"notes":[],"decks":[]}"#.to_vec()),
        Step::new(Method::ApplyChanges, serde_json::to_vec(&changes).unwrap()),
        Step::new(Method::Chunk, serde_json::to_vec(&chunk).unwrap()),
        Step::new(Method::ApplyChunk, b"null".to_vec()),
        Step::new(Method::SanityCheck2, br#"{"status":"ok"}"#.to_vec()),
        Step::new(Method::Finish, b::NOW.to_string().into_bytes())]
}
pub fn empty_changes() -> Value { json!({"models": [], "decks": [[], []], "tags": []}) }

pub struct Script {
    steps: Mutex<VecDeque<Step>>, calls: Mutex<Vec<(Method, Vec<u8>)>>,
    completed: Mutex<mpsc::Receiver<()>>, signal: mpsc::Sender<()>,
}
impl Script {
    pub fn new(steps: Vec<Step>) -> Arc<Self> {
        let (signal, completed) = mpsc::channel();
        Arc::new(Self { steps: Mutex::new(steps.into()), calls: Mutex::default(), completed: Mutex::new(completed), signal })
    }
    pub fn bodies(&self, method: Method) -> Vec<Vec<u8>> {
        self.calls.lock().unwrap().iter().filter(|(m, _)| *m == method).map(|(_, body)| body.clone()).collect()
    }
    pub fn methods(&self) -> Vec<Method> {
        self.calls.lock().unwrap().iter().map(|(method, _)| *method).collect()
    }
    pub fn assert_done(&self) {
        loop {
            if self.steps.lock().unwrap().is_empty() { break; }
            self.completed.lock().unwrap().recv_timeout(Duration::from_secs(10)).expect("script did not drain");
        }
    }
}
pub struct Transport(pub Arc<Script>);
struct Body(Option<Bytes>);
impl RawBody for Body {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> {
        Box::pin(async move { Ok(self.0.take()) })
    }
}
impl SyncTransport for Transport {
    fn post(&self, mut request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(async move {
            let step = self.0.steps.lock().unwrap().pop_front().expect("unapproved request");
            assert!(request.url.starts_with(b::ENDPOINT));
            assert!(request.url.ends_with(&format!("/{}", step.method.name())));
            let mut compressed = Vec::new();
            while let Some(bytes) = poll_fn(|cx| request.body.as_mut().poll_next(cx)).await { compressed.extend_from_slice(&bytes.unwrap()); }
            let body = zstd::stream::decode_all(compressed.as_slice()).unwrap();
            self.0.calls.lock().unwrap().push((step.method, body));
            if let Some(after) = step.after { after(); }
            let length = step.reply.len();
            let reply = zstd::stream::encode_all(step.reply.as_slice(), 0).unwrap();
            self.0.signal.send(()).unwrap();
            if step.lost { return Err(SyncError::Network(meguri_sync::NetworkError {
                cause: meguri_sync::NetworkCause::Connection, head_arrived: true, detail: "synthetic lost answer".into() })); }
            Ok(TransportResponse { status: 200,
                headers: vec![("anki-original-size".into(), length.to_string().into_bytes())],
                body: Box::new(Body(Some(Bytes::from(reply)))) })
        })
    }
}
pub fn access(connection: &MeguriConnection, script: &Arc<Script>) -> MeguriAccess {
    MeguriAccess::new(connection.clone(), b::USER, HostKey::new("synthetic-key")).unwrap()
        .with_transport_for_testing(Box::new(Transport(script.clone())))
}
pub fn started(value: MeguriRoundStart) -> Arc<MeguriRoundTask> {
    match value { MeguriRoundStart::Started(task) => task,
        MeguriRoundStart::Refused(refusal) => panic!("unexpected routine refusal: {refusal:?}") }
}
pub fn complete(host: &CoreHost, task: &MeguriRoundTask) -> Arc<meguri_host::MeguriRoundResult> {
    task.wait(); host.meguri_round_result(task).unwrap().expect("joined original result")
}
pub fn modified(host: &CoreHost) -> i64 {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().col().unwrap().modified.0
}
pub fn pending(host: &CoreHost) {
    host.collection().unwrap().run(OpSpec::named("generated pending edit", OpSource::UserUndoable), |ctx| {
        ctx.scope().set_config("routine-generated", &meguri_storage::ConfigValue::Bool(true))?;
        Ok(())
    }).unwrap();
}
/// Small stored ZIP fixture encoder, independent of production encoders. The
/// generated archive contains only numeric payload names and an exact _meta.
pub fn download_zip(name: &str, bytes: &[u8]) -> Vec<u8> {
    let meta = serde_json::to_vec(&json!({"0": name})).unwrap();
    let entries = [("0", bytes), ("_meta", meta.as_slice())];
    let mut output = Vec::new(); let mut central = Vec::new();
    for (entry, data) in entries {
        let offset = output.len() as u32;
        let mut crc = !0u32;
        for byte in data { crc ^= u32::from(*byte); for _ in 0..8 { crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1)); } }
        let crc = !crc;
        let size = data.len() as u32; let length = entry.len() as u16;
        output.extend(0x04034b50u32.to_le_bytes());
        for v in [20u16, 0, 0, 0, 0] { output.extend(v.to_le_bytes()); }
        for v in [crc, size, size] { output.extend(v.to_le_bytes()); }
        output.extend(length.to_le_bytes()); output.extend(0u16.to_le_bytes()); output.extend(entry.as_bytes()); output.extend(data);
        central.extend(0x02014b50u32.to_le_bytes());
        for v in [20u16, 20, 0, 0, 0, 0] { central.extend(v.to_le_bytes()); }
        for v in [crc, size, size] { central.extend(v.to_le_bytes()); }
        for v in [length, 0, 0, 0, 0] { central.extend(v.to_le_bytes()); }
        central.extend(0u32.to_le_bytes()); central.extend(offset.to_le_bytes()); central.extend(entry.as_bytes());
    }
    let start = output.len() as u32; let length = central.len() as u32; output.extend(central);
    output.extend(0x06054b50u32.to_le_bytes());
    for v in [0u16, 0, 2, 2] { output.extend(v.to_le_bytes()); }
    output.extend(length.to_le_bytes()); output.extend(start.to_le_bytes()); output.extend(0u16.to_le_bytes()); output
}
pub fn stored_entries(bytes: &[u8]) -> BTreeMap<String, Vec<u8>> {
    let u16_at = |i| u16::from_le_bytes(bytes[i..i+2].try_into().unwrap()) as usize;
    let u32_at = |i| u32::from_le_bytes(bytes[i..i+4].try_into().unwrap()) as usize;
    let end = bytes.len() - 22;
    assert_eq!(u32_at(end), 0x06054b50);
    let mut position = u32_at(end + 16); let mut result = BTreeMap::new();
    for _ in 0..u16_at(end + 10) {
        assert_eq!(u32_at(position), 0x02014b50); assert_eq!(u16_at(position + 10), 0);
        let length = u16_at(position + 28); let extra = u16_at(position + 30); let comment = u16_at(position + 32);
        let local = u32_at(position + 42); let size = u32_at(position + 24);
        let name = String::from_utf8(bytes[position+46..position+46+length].to_vec()).unwrap();
        let start = local + 30 + u16_at(local+26) + u16_at(local+28);
        result.insert(name, bytes[start..start+size].to_vec()); position += 46 + length + extra + comment;
    }
    result
}
