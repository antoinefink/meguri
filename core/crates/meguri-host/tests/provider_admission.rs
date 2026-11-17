//! Production Host startup must not turn configured transports into a service choice.
use std::sync::{Arc, atomic::{AtomicUsize, Ordering}};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
use meguri_sync::cloud::{BuildScope, ContainerId, Environment, OpaqueBytes,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus}};
use meguri_testkit::cloud::devices::NativeFiles;

struct Account(AtomicUsize);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        self.0.fetch_add(1, Ordering::SeqCst);
        IdentityStatus::Available(OpaqueBytes(b"synthetic-provider-account".to_vec()))
    }
}
fn fixture(root: &std::path::Path, account: Arc<Account>) -> HostConfig {
    let directory = root.join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(151));
    meguri_engine::bootstrap::create(&directory.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
    let mut config = HostConfig::new(directory, root.join("backups"), root.join("widget"),
        clock, random, Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    config.cloud = Some(CloudInputs {
        scope: BuildScope { container: ContainerId::new("synthetic-provider").unwrap(), environment: Environment::Development },
        identity: account, files: Arc::new(NativeFiles),
    });
    config
}

#[test]
fn ordinary_host_open_and_reopen_do_not_discover_an_account_for_a_new_installation() {
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = fixture(root.path(), account.clone());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    for index in 0..2 {
        let host = process.open(config.clone()).unwrap();
        let collection = host.collection().unwrap();
        // Local engine operations remain independent of network eligibility.
        meguri_engine::decks::add(&collection, format!("Local study {index}")).unwrap();
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        assert!(host.cloud_runtime().unwrap().register_sdk(false).is_err());
        host.close().unwrap();
    }
}

#[test]
fn invalid_provider_document_blocks_account_discovery_without_preventing_local_open() {
    for document in ["null", "{}", "{\"version\":999}", "{\"version\":1,\"handoff\":true}"] {
        let root = tempfile::tempdir().unwrap();
        let account = Arc::new(Account(AtomicUsize::new(0)));
        let config = fixture(root.path(), account.clone());
        std::fs::create_dir_all(&config.manual_transfer_directory).unwrap();
        std::fs::write(config.manual_transfer_directory.join("provider.json"), document).unwrap();
        let host = CoreHost::open(config).unwrap();
        meguri_engine::decks::add(&host.collection().unwrap(), "Still local".into()).unwrap();
        assert_eq!(account.0.load(Ordering::SeqCst), 0);
        assert!(host.cloud_runtime().unwrap().register_sdk(false).is_err());
        host.close().unwrap();
    }
}

struct TransferBody(Option<bytes::Bytes>);
impl meguri_sync::transport::RawBody for TransferBody {
    fn next(&mut self) -> meguri_sync::transport::BoxFuture<'_, Result<Option<bytes::Bytes>, meguri_sync::SyncError>> {
        Box::pin(async move { Ok(self.0.take()) })
    }
}
struct RetainedUploadScript(std::sync::Mutex<std::collections::VecDeque<(meguri_sync::Method, Vec<u8>)>>);
struct TransferTransport(Arc<RetainedUploadScript>);
impl meguri_sync::SyncTransport for TransferTransport {
    fn post(&self, mut request: meguri_sync::transport::TransportRequest) -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
        Box::pin(async move {
            let (method, reply) = self.0.0.lock().unwrap().pop_front().expect("no request without its original operation");
            assert!(request.url.ends_with(&format!("/{}", method.name())));
            while let Some(chunk) = std::future::poll_fn(|cx| request.body.as_mut().poll_next(cx)).await { chunk.unwrap(); }
            Ok(meguri_sync::transport::TransportResponse {
                status: 200, headers: vec![("anki-original-size".into(), reply.len().to_string().into_bytes())],
                body: Box::new(TransferBody(Some(bytes::Bytes::from(zstd::stream::encode_all(reply.as_slice(), 0).unwrap())))),
            })
        })
    }
}
fn transfer_access(script: Arc<RetainedUploadScript>) -> meguri_sync::oneway::TransferAccess {
    let endpoint = meguri_sync::Endpoint::parse("https://manual.synthetic.example/prefix/").unwrap();
    meguri_sync::oneway::TransferAccess::new(endpoint.clone(), "Synthetic Account", meguri_sync::HostKey::new("synthetic-manual-key"),
        meguri_sync::JobOptions::new(endpoint, meguri_sync::ServerKind::SelfHosted)).unwrap()
        .with_transport_for_testing(Box::new(TransferTransport(script)))
}
fn complete_transfer<T>(events: meguri_engine::Subscription, task: meguri_host::ManualTransferTask<T>) -> Result<T, meguri_sync::oneway::TransferError> {
    loop {
        let event = events.recv().unwrap();
        if matches!(event.kind, meguri_engine::EventKind::JobFinished { id, .. } if id == task.job.id) { break; }
    }
    task.take_result().expect("original worker result")
}

#[test]
fn possibly_accepted_upload_survives_provider_configuration_handoff_refusal_and_reopen() {
    use meguri_storage::{ReadRows, ReaderRole};
    use meguri_sync::policy::{Acceptance, Direction, Scope};
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    let account = Arc::new(Account(AtomicUsize::new(0)));
    let config = fixture(&root.path().join("local"), account.clone());
    let server_config = fixture(&root.path().join("server"), Arc::new(Account(AtomicUsize::new(0))));
    let server = CoreHost::open(server_config.clone()).unwrap();
    let collection = server.collection().unwrap();
    let facts = collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    drop(collection);
    server.close().unwrap();
    let server_bytes = std::fs::read(server_config.collection_directory.join("collection.anki2")).unwrap();
    let meta = serde_json::to_vec(&serde_json::json!({"mod":facts.modified.0,"scm":facts.schema_modified.0,"usn":facts.usn.0,
        "ts":1_894_701_600,"msg":"","cont":true,"empty":true,"media_usn":0})).unwrap();
    let script = Arc::new(RetainedUploadScript(std::sync::Mutex::new(std::collections::VecDeque::from([
        (meguri_sync::Method::Meta, meta.clone()), (meguri_sync::Method::Download, server_bytes),
        (meguri_sync::Method::Meta, meta.clone()), (meguri_sync::Method::Meta, meta),
    ]))));
    let host = CoreHost::open(config.clone()).unwrap();
    let events = host.engine().stream().subscribe();
    let preparation = host.prepare_manual_transfer(transfer_access(script.clone()), Direction::Export, Scope::CollectionOnly).unwrap();
    let preview = complete_transfer(events, preparation).unwrap();
    faultpoint::arm("oneway.upload.after_intent", Fault::IoError(std::io::ErrorKind::Other));
    let events = host.engine().stream().subscribe();
    let confirmation = host.confirm_manual_transfer(transfer_access(script.clone()), preview.confirmation).unwrap();
    let result = complete_transfer(events, confirmation);
    faultpoint::disarm("oneway.upload.after_intent");
    assert!(result.is_err());
    let operation = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(operation.acceptance, Acceptance::PossiblyAccepted);
    let policy_path = config.manual_transfer_directory.join("manual-transfers.json");
    let evidence = std::fs::read(&policy_path).unwrap();
    host.configure_meguri_connection("https://new.synthetic.example/").unwrap();
    assert!(host.begin_provider_handoff(meguri_sync::provider::Selection::Meguri).is_err());
    assert_eq!(std::fs::read(&policy_path).unwrap(), evidence);
    assert!(host.provider_authority().network_blocked());
    assert!(host.cancel_prepared_manual_transfer(&operation.id, false).is_err());
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    let retained = reopened.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(retained.id, operation.id);
    assert_eq!(retained.acceptance, Acceptance::PossiblyAccepted);
    assert!(reopened.provider_authority().network_blocked());
    assert_eq!(account.0.load(Ordering::SeqCst), 0);
    assert!(script.0.lock().unwrap().is_empty());
    reopened.close().unwrap();
}
