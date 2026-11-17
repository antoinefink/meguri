use meguri_sync::provider::{LegacySettings, ProviderDocument, Selection, canonical_username, native_endpoint};
use serde_json::json;

#[test]
fn absence_is_distinct_from_invalid_and_migration_is_not_a_binding() {
    let defaults = LegacySettings::decode(None).unwrap();
    assert!(!defaults.enabled && defaults.automatic && defaults.media);
    for value in [json!(null), json!({}), json!({"enabled":1,"paused":false,"automatic":true,"media":true})] {
        assert!(LegacySettings::decode(Some(&value)).is_err());
    }
    let legacy = LegacySettings::decode(Some(&json!({"enabled":true,"paused":true,"automatic":false,"media":false}))).unwrap();
    let document = ProviderDocument::migration(legacy);
    assert_eq!(document.selection, Selection::ICloud);
    assert!(document.binding.is_none());
    assert!(document.settings.paused && !document.settings.automatic && !document.settings.media);
}

#[test]
fn provider_metadata_and_authentication_do_not_grant_meguri_transfer_or_automatic_work() {
    let mut doc = ProviderDocument::migration(LegacySettings::decode(None).unwrap());
    doc.selection = Selection::Meguri;
    assert!(!doc.permits_icloud());
    assert!(!doc.permits_meguri_transfer());
}

#[test]
fn native_endpoint_and_w2_identity_are_strict_without_normalizing_passwords() {
    assert_eq!(native_endpoint("HTTPS://Sync.Example:443/prefix").unwrap().as_str(), "https://sync.example/prefix/");
    assert!(native_endpoint("http://127.0.0.1/").is_err());
    assert_eq!(canonical_username(" \tSynthetic.User_1\r\n").unwrap(), "synthetic.user_1");
    for name in ["ab", ".abc", "a b", "éabc", "\u{a0}abc"] { assert!(canonical_username(name).is_err()); }
}

#[test]
fn durable_intent_does_not_become_drain_proof_on_restart_and_meguri_commit_stays_setup_only() {
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), None).unwrap();
    let authority = Authority::load(root.path());
    let discovery = authority.begin_discovery().unwrap();
    assert!(authority.permits_icloud_account_read());
    let handoff = authority.begin_handoff(Selection::Meguri, None, 1).unwrap();
    assert!(!discovery.is_current());
    assert!(!authority.permits_icloud_account_read());
    let reopened = Authority::load(root.path());
    assert!(reopened.snapshot().unwrap().handoff.is_some());
    assert!(!reopened.permits_icloud_account_read());
    assert!(!reopened.permits_original_identity_completion());
    assert!(reopened.begin_discovery().is_err());
    // This lower metadata operation supplies no native drain certificate or
    // ready binding. Host/app tests exercise the actual join before invoking it.
    let committed = authority.commit_handoff(&handoff).unwrap();
    assert_eq!(committed.selection, Selection::Meguri);
    assert!(!committed.settings.automatic && !committed.permits_meguri_transfer());
    assert!(committed.binding.is_none());
    let after_commit = Authority::load(root.path()).snapshot().unwrap();
    assert!(after_commit.handoff.is_none() && !after_commit.permits_meguri_transfer());
}

#[test]
fn a_failed_sidecar_publication_does_not_report_an_in_memory_configuration_success() {
    use meguri_sync::provider::Authority;
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), None).unwrap();
    let authority = Authority::load(root.path());
    let bytes = std::fs::read(root.path().join("provider.json")).unwrap();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    let result = authority.configure_meguri("https://synthetic.example/", "synthetic-reference".into());
    faultpoint::disarm("storage.sidecar.before_save");
    assert!(result.is_err());
    assert!(authority.snapshot().is_err());
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), bytes);
    assert!(Authority::load(root.path()).snapshot().unwrap().meguri.is_none());
}

#[test]
fn migration_is_once_preserves_unknown_members_and_never_repairs_present_invalid_metadata() {
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    let legacy = json!({"enabled":true,"paused":true,"automatic":false,"media":false});
    Authority::migrate(root.path(), Some(&legacy)).unwrap();
    meguri_storage::sidecar::update_document(&root.path().join("provider.json"), |doc| {
        doc.insert("future".into(), json!({"retained":true})); Ok(())
    }).unwrap();
    Authority::migrate(root.path(), None).unwrap();
    let loaded = Authority::load(root.path());
    assert!(loaded.snapshot().unwrap().settings.paused);
    loaded.configure_meguri("https://synthetic.example/", "synthetic-reference".into()).unwrap();
    let document: serde_json::Value = serde_json::from_slice(&std::fs::read(root.path().join("provider.json")).unwrap()).unwrap();
    assert_eq!(document["future"], json!({"retained":true}));
    std::fs::write(root.path().join("provider.json"), b"{}").unwrap();
    assert!(Authority::migrate(root.path(), Some(&legacy)).is_err());
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), b"{}");
}

struct Body(Option<bytes::Bytes>);
impl meguri_sync::transport::RawBody for Body {
    fn next(&mut self) -> meguri_sync::transport::BoxFuture<'_, Result<Option<bytes::Bytes>, meguri_sync::SyncError>> {
        let value = self.0.take(); Box::pin(async move { Ok(value) })
    }
}
struct AuthenticationTransport {
    requests: std::sync::Arc<std::sync::Mutex<Vec<(String, serde_json::Value)>>>,
    redirect: bool,
    change: Option<Box<dyn Fn() + Send + Sync>>,
}
impl meguri_sync::SyncTransport for AuthenticationTransport {
    fn post(&self, mut request: meguri_sync::transport::TransportRequest) -> meguri_sync::transport::BoxFuture<'_, Result<meguri_sync::transport::TransportResponse, meguri_sync::SyncError>> {
        Box::pin(async move {
            let mut body = Vec::new();
            while let Some(chunk) = std::future::poll_fn(|cx| request.body.as_mut().poll_next(cx)).await { body.extend_from_slice(&chunk.unwrap()); }
            let plain = zstd::stream::decode_all(std::io::Cursor::new(body)).unwrap();
            self.requests.lock().unwrap().push((request.url, serde_json::from_slice(&plain).unwrap()));
            if let Some(change) = &self.change { change(); }
            Ok(meguri_sync::transport::TransportResponse {
                status: if self.redirect { 308 } else { 200 },
                headers: if self.redirect { vec![("location".into(), b"https://different.synthetic.example/".to_vec())] }
                    else { vec![("anki-original-size".into(), br#"{"key":"synthetic-hostkey"}"#.len().to_string().into_bytes())] },
                body: Box::new(Body(Some(bytes::Bytes::from(zstd::stream::encode_all(br#"{"key":"synthetic-hostkey"}"#.as_slice(), 0).unwrap())))),
            })
        })
    }
}

#[test]
fn authentication_posts_only_hostkey_refuses_redirects_and_rejects_a_changed_connection() {
    use meguri_sync::provider::{Authority, authenticate, AuthenticationFailure};
    use std::sync::{Arc, Mutex};
    let threads = meguri_engine::ThreadRegistry::new();
    for (redirect, change) in [(false, false), (true, false), (false, true)] {
        let root = tempfile::tempdir().unwrap();
        Authority::migrate(root.path(), None).unwrap();
        let authority = Authority::load(root.path());
        let connection = authority.configure_meguri("https://synthetic.example/prefix/", "synthetic-first".into()).unwrap();
        let requests = Arc::new(Mutex::new(Vec::new()));
        let original = authority.clone();
        let edit = change.then(|| Box::new(move || {
            original.configure_meguri("https://changed.synthetic.example/", "synthetic-next".into()).unwrap();
        }) as Box<dyn Fn() + Send + Sync>);
        let result = authenticate(&authority, &connection, &threads,
            &meguri_model::rng::SeededRandom::new(151), " \tSynthetic.User\r\n", " exact password bytes \n",
            meguri_engine::CancelToken::new(), Some(Box::new(AuthenticationTransport { requests: requests.clone(), redirect, change: edit })));
        let seen = requests.lock().unwrap();
        assert_eq!(seen.len(), 1);
        assert_eq!(seen[0].0, "https://synthetic.example/prefix/sync/hostKey");
        assert_eq!(seen[0].1, json!({"u":"synthetic.user","p":" exact password bytes \n"}));
        if redirect { assert!(matches!(result, Err(AuthenticationFailure::Transport(meguri_sync::SyncError::Redirect(_))))); }
        else if change { assert!(matches!(result, Err(AuthenticationFailure::ConnectionChanged))); }
        else { assert_eq!(result.unwrap().hostkey, "synthetic-hostkey"); }
        let doc = authority.snapshot().unwrap();
        assert_eq!(doc.selection, Selection::LocalOnly);
        assert!(doc.binding.is_none() && !doc.permits_meguri_transfer());
    }
}
