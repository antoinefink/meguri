//! Legacy execution and sealed admission refusal, using synthetic transports.
use bytes::Bytes;
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::{Endpoint, HostKey, JobOptions, Method, ServerKind, SyncError, SyncTransport, normal,
    provider::{MeguriRoundRefusal, Selection},
    transport::{BoxFuture, RawBody, TransportRequest, TransportResponse}};
use meguri_testkit::normal_support::{Fixture, NOW};
use std::{collections::VecDeque, future::poll_fn, sync::{Arc, Mutex}};
struct Body(Option<Bytes>);
impl RawBody for Body {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<Bytes>, SyncError>> { Box::pin(async move { Ok(self.0.take()) }) }
}
type ScriptReplies = Arc<Mutex<VecDeque<(Method, Vec<u8>)>>>;
struct Script(ScriptReplies);
impl SyncTransport for Script {
    fn post(&self, mut request: TransportRequest) -> BoxFuture<'_, Result<TransportResponse, SyncError>> {
        Box::pin(async move {
            let (method, bytes) = self.0.lock().unwrap().pop_front().expect("unapproved legacy request");
            assert!(request.url.ends_with(&format!("/{}", method.name())));
            while let Some(chunk) = poll_fn(|cx| request.body.as_mut().poll_next(cx)).await { chunk.unwrap(); }
            Ok(TransportResponse { status: 200, headers: vec![("anki-original-size".into(), bytes.len().to_string().into_bytes())],
                body: Box::new(Body(Some(Bytes::from(zstd::stream::encode_all(bytes.as_slice(), 0).unwrap())))) })
        })
    }
}
#[test]
fn legacy_start_remains_always_admitted_and_uses_its_original_task() {
    let fixture = Fixture::new(1);
    let schema = fixture.collection.reader(ReaderRole::Query).unwrap().col().unwrap().schema_modified.0;
    let meta = format!(r#"{{"mod":{},"scm":{schema},"usn":5,"ts":{},"msg":"","cont":true,"empty":false,"media_usn":0}}"#, NOW-60_000, NOW/1000).into_bytes();
    let steps = Arc::new(Mutex::new(VecDeque::from([
        (Method::Meta, meta), (Method::Start, br#"{"cards":[],"notes":[],"decks":[]}"#.to_vec()),
        (Method::ApplyChanges, br#"{"models":[],"decks":[[],[]],"tags":[]}"#.to_vec()),
        (Method::Chunk, br#"{"done":true}"#.to_vec()), (Method::ApplyChunk, b"null".to_vec()),
        (Method::SanityCheck2, br#"{"status":"ok"}"#.to_vec()), (Method::Finish, NOW.to_string().into_bytes()),
    ])));
    let task = normal::start(fixture.host.engine(), fixture.collection.clone(),
        JobOptions::new(Endpoint::parse("https://legacy.synthetic.example/").unwrap(), ServerKind::SelfHosted),
        HostKey::new("synthetic"), fixture.clock.clone(), fixture.random.clone(), fixture.memory.clone(), Some(Box::new(Script(steps.clone())))).unwrap();
    let outcome = task.wait().unwrap(); assert!(!outcome.merge.unwrap().one_way);
    assert!(steps.lock().unwrap().is_empty()); fixture.host.close().unwrap();
}
#[test]
fn credentials_and_selected_provider_cannot_construct_ready_admission() {
    let fixture = Fixture::new(0);
    let connection = fixture.host.configure_meguri_connection("https://routine.synthetic.example/").unwrap();
    let handoff = fixture.host.begin_provider_handoff(Selection::Meguri).unwrap();
    fixture.host.finish_provider_handoff(&handoff).unwrap();
    let before = fixture.host.provider_authority().snapshot().unwrap();
    let admission = fixture.host.provider_authority().admit_meguri_round(fixture.collection.clone(),
        fixture.host.original_resource_authority(&fixture.collection), fixture.host.manual_transfer_history().clone(), &connection, "synthetic", fixture.host.media().unwrap().store().clone());
    assert!(matches!(admission, Err(MeguriRoundRefusal::NotReady)));
    assert_eq!(fixture.host.provider_authority().snapshot().unwrap(), before);
    assert!(fixture.host.engine().jobs().running().is_empty()); fixture.host.close().unwrap();
}
