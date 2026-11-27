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
    assert!(authority.reload().is_err());
    assert!(authority.network_blocked());
    assert!(!authority.failed_publication_requires_handoff_recovery().unwrap());
    assert!(authority.save_settings(LegacySettings::decode(None).unwrap()).is_err());
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), bytes);
    assert!(Authority::load(root.path()).snapshot().unwrap().meguri.is_none());
    authority.retry_failed_publication(0, None).unwrap();
    assert_eq!(authority.snapshot().unwrap().meguri.unwrap().reference, "synthetic-reference");
}

#[test]
fn failed_pause_keeps_its_error_and_desired_state_until_explicit_publication_retry() {
    use meguri_sync::provider::Authority;
    use meguri_model::faultpoint::{self, Fault};
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), Some(&json!({"enabled":true,"paused":false,"automatic":true,"media":true}))).unwrap();
    let authority = Authority::load(root.path());
    let mut settings = authority.snapshot().unwrap().settings;
    settings.paused = true;
    let before = std::fs::read(root.path().join("provider.json")).unwrap();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    let error = authority.save_settings(settings).unwrap_err();
    faultpoint::disarm("storage.sidecar.before_save");
    assert_eq!(authority.reload().unwrap_err(), error);
    assert_eq!(authority.snapshot().unwrap_err(), error);
    assert!(!authority.failed_publication_requires_handoff_recovery().unwrap());
    assert!(authority.network_blocked());
    assert!(!authority.permits_icloud_account_read());
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), before);
    assert!(authority.configure_meguri("https://unrelated.synthetic.example/", "unrelated".into()).is_err());
    authority.retry_failed_publication(0, None).unwrap();
    assert!(authority.snapshot().unwrap().settings.paused);
    assert!(!authority.permits_icloud_account_read());
    authority.reload().unwrap();
    std::fs::write(root.path().join("provider.json"), b"{}").unwrap();
    assert!(authority.reload().is_err());
    assert!(authority.retry_failed_publication(0, None).is_err());
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), b"{}");
}

#[test]
fn failed_metadata_retry_keeps_the_captured_owner_generation_and_publication_fences() {
    use meguri_engine::device_state::{AccountBinding, Publication, PublicationDecision, PublicationId};
    use meguri_model::faultpoint::{self, Fault};
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), None).unwrap();
    let authority = Authority::load(root.path());
    let mut settings = authority.snapshot().unwrap().settings;
    settings.paused = true;
    let publication = Publication { identity: PublicationId::from_bytes([1; 16]),
        binding: AccountBinding::LocalOnly, decision: PublicationDecision::Clear };
    let before = std::fs::read(root.path().join("provider.json")).unwrap();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    let error = authority.save_settings_for_owner(settings, 7, Some(publication.clone())).unwrap_err();
    faultpoint::disarm("storage.sidecar.before_save");
    assert!(!authority.failed_publication_requires_handoff_recovery().unwrap());
    let mut changed = publication.clone();
    changed.identity = PublicationId::from_bytes([2; 16]);
    let mut changed_account = publication.clone();
    changed_account.binding = AccountBinding::Unknown;
    let mut pending = publication.clone();
    pending.decision = PublicationDecision::Pending;
    for (generation, current) in [(8, Some(&publication)), (7, None), (7, Some(&changed)),
        (7, Some(&changed_account)), (7, Some(&pending))] {
        assert!(authority.retry_failed_publication(generation, current).is_err());
        assert_eq!(authority.snapshot().unwrap_err(), error);
        assert_eq!(authority.reload().unwrap_err(), error);
        assert!(authority.network_blocked());
        assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), before);
    }
    authority.retry_failed_publication(7, Some(&publication)).unwrap();
    assert!(authority.snapshot().unwrap().settings.paused);
    assert!(!authority.permits_icloud_account_read());
    assert!(!authority.snapshot().unwrap().permits_meguri_transfer());
    assert!(authority.failed_publication_requires_handoff_recovery().is_err());
    assert!(authority.retry_failed_publication(7, Some(&publication)).is_err());
}

#[test]
fn failed_first_setup_retry_keeps_absent_publication_and_original_error_until_exact_retry() {
    use meguri_engine::device_state::{AccountBinding, Publication, PublicationDecision, PublicationId};
    use meguri_model::faultpoint::{self, Fault};
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), None).unwrap();
    let authority = Authority::load(root.path());
    let before = std::fs::read(root.path().join("provider.json")).unwrap();
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::PermissionDenied));
    let error = authority.configure_meguri_for_owner("https://first-setup.synthetic.example/", "retained-first-setup".into(), 0, None).unwrap_err();
    faultpoint::disarm("storage.sidecar.before_save");
    let unrelated = Publication { identity: PublicationId::from_bytes([3; 16]),
        binding: AccountBinding::LocalOnly, decision: PublicationDecision::Clear };
    for (generation, publication) in [(1, None), (0, Some(&unrelated))] {
        assert!(authority.retry_failed_publication(generation, publication).is_err());
        assert_eq!(authority.snapshot().unwrap_err(), error);
        assert_eq!(authority.reload().unwrap_err(), error);
        assert!(authority.network_blocked());
        assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), before);
    }
    faultpoint::arm("storage.sidecar.before_save", Fault::IoError(std::io::ErrorKind::Other));
    assert!(authority.retry_failed_publication(0, None).is_err());
    faultpoint::disarm("storage.sidecar.before_save");
    assert_eq!(authority.snapshot().unwrap_err(), error);
    assert_eq!(authority.reload().unwrap_err(), error);
    assert_eq!(std::fs::read(root.path().join("provider.json")).unwrap(), before);
    authority.retry_failed_publication(0, None).unwrap();
    let doc = authority.snapshot().unwrap();
    assert_eq!(doc.meguri.unwrap().reference, "retained-first-setup");
    assert_eq!(doc.selection, Selection::LocalOnly);
    assert!(doc.binding.is_none() && doc.handoff.is_none());
    assert!(!authority.permits_icloud_account_read());
    assert!(!authority.snapshot().unwrap().permits_meguri_transfer());
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

// Persisted format from before retirement: keep this literal independent of
// ProviderDocument::migration so a future default cannot hide an upgrade bug.
const PRE_UPGRADE_PROVIDER: &str = r#"{
  "version":1,"revision":7,"selection":"ICloud",
  "settings":{"enabled":true,"paused":true,"automatic":false,"media":false},
  "binding":{"binding":[1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16],"account":[1,2,3],
    "publication":{"identity":[5,6],"binding":"LocalOnly","decision":"Clear","futurePublication":19},
    "container":"synthetic-container","environment":"Development","futureCloudBinding":true},
  "handoff":null,"legacy_binding_pending":false,
  "meguri":{"reference":"synthetic-connection","revision":4,"endpoint":"https://sync.example.test/"},
  "icloud_settings":{"enabled":true,"paused":true,"automatic":false,"media":false},
  "meguri_settings":{"enabled":true,"paused":false,"automatic":false,"media":true},
  "meguri_binding":null,
  "meguri_bootstrap":{"connection":{"reference":"synthetic-connection","revision":4,"endpoint":"https://sync.example.test/"},
    "operation":"synthetic-pending","operation_revision":2,"direction":"Export","futureIntent":23},
  "future":{"untouched":[1,2,3]}
}"#;

#[test]
fn application_upgrade_retires_persisted_icloud_and_keeps_local_bytes_and_inactive_meguri_intent() {
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("provider.json");
    std::fs::write(&path, PRE_UPGRADE_PROVIDER).unwrap();
    // A sentinel is deliberately not a valid collection: retirement cannot
    // open or rewrite a user's collection as part of installation migration.
    let collection = root.path().join("collection.anki2");
    std::fs::write(&collection, b"synthetic unopened collection").unwrap();
    Authority::retire_icloud(root.path()).unwrap();
    let document = Authority::load(root.path()).snapshot().unwrap();
    assert_eq!(document.selection, Selection::LocalOnly);
    assert!(!document.settings.enabled && !document.settings.paused);
    assert!(!document.legacy_binding_pending && document.binding.is_none() && document.handoff.is_none());
    assert!(!document.permits_meguri_transfer());
    let before: serde_json::Value = serde_json::from_str(PRE_UPGRADE_PROVIDER).unwrap();
    let after: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
    for key in ["future", "meguri", "meguri_bootstrap", "meguri_settings"] { assert_eq!(after[key], before[key]); }
    assert_eq!(after["retiredICloud"], before);
    assert_eq!(std::fs::read(collection).unwrap(), b"synthetic unopened collection");
    let bytes = std::fs::read(&path).unwrap();
    Authority::retire_icloud(root.path()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes, "upgrade must be idempotent");
}

#[test]
fn application_upgrade_leaves_steady_meguri_binding_and_pending_structure_byte_exact() {
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    let path = root.path().join("provider.json");
    let mut before: serde_json::Value = serde_json::from_str(PRE_UPGRADE_PROVIDER).unwrap();
    before["selection"] = json!("Meguri");
    before["settings"] = json!({"enabled":true,"paused":false,"automatic":true,"media":true});
    before["meguri_settings"] = before["settings"].clone();
    before["meguri_bootstrap"] = json!(null);
    before["meguri_binding"] = json!({"connection":before["meguri"],"account":"synthetic-account",
        "publication":[1,2],"operation":"synthetic-completed","media_epoch":{"owner":"synthetic-account","epoch":3},
        "completed_at_ms":1000,"futureBinding":"retained"});
    before["meguri_structure"] = json!({"binding":before["meguri_binding"],"baseline":null,
        "automatic":true,"schema_before":12,"schema_saved":13,"futureUpload":true});
    let bytes = serde_json::to_vec_pretty(&before).unwrap();
    std::fs::write(&path, &bytes).unwrap();
    Authority::retire_icloud(root.path()).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), bytes);
    let document = Authority::load(root.path()).snapshot().unwrap();
    assert_eq!(document.selection, Selection::Meguri);
    assert!(document.meguri_binding.is_some() && document.meguri_structure.is_some());
    assert!(document.settings.automatic);
}

#[test]
fn application_upgrade_retires_both_interrupted_icloud_handoff_directions_and_credentials_fence() {
    use meguri_sync::provider::Authority;
    for old in [Selection::Meguri, Selection::ICloud] {
        for remove in [false, true] {
            let root = tempfile::tempdir().unwrap();
            let path = root.path().join("provider.json");
            let mut before: serde_json::Value = serde_json::from_str(PRE_UPGRADE_PROVIDER).unwrap();
            before["selection"] = json!(old);
            before["handoff"] = json!({"old":old,
                "destination":if old == Selection::Meguri { Selection::ICloud } else { Selection::Meguri },
                "binding":null,"publication":null,"generation":1,"revision":7,"remove_credentials":remove,
                "futureHandoff":{"retained":true}});
            std::fs::write(&path, serde_json::to_vec(&before).unwrap()).unwrap();
            Authority::retire_icloud(root.path()).unwrap();
            let document = Authority::load(root.path()).snapshot().unwrap();
            assert_eq!(document.selection, Selection::LocalOnly);
            assert!(document.handoff.is_none() && !document.settings.enabled);
            assert_eq!(document.meguri.is_none(), remove);
            assert_eq!(document.meguri_bootstrap.is_none(), remove);
            let raw: serde_json::Value = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
            assert_eq!(raw["retiredICloud"]["handoff"], before["handoff"]);
        }
    }
}

#[test]
fn application_upgrade_retires_legacy_enabled_defaults_without_enabling_another_provider() {
    use meguri_sync::provider::Authority;
    let root = tempfile::tempdir().unwrap();
    Authority::migrate(root.path(), Some(&json!({"enabled":true,"paused":false,"automatic":true,"media":true}))).unwrap();
    Authority::retire_icloud(root.path()).unwrap();
    let document = Authority::load(root.path()).snapshot().unwrap();
    assert_eq!(document.selection, Selection::LocalOnly);
    assert!(!document.settings.enabled && document.meguri.is_none());
}
