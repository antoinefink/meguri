//! A setup media step killed mid-transfer (resume fix). Its own binary: it
//! re-runs itself with `--exact <test>`, which needs an unprefixed name.
#[path = "support/meguri_bootstrap_script.rs"]
mod support;
use support::*;
use meguri_engine::device_state::DeviceState;
use meguri_host::{CoreHost, MeguriBootstrapDirection as D};
use meguri_sync::{Method, policy::Phase, provider::MeguriReadiness as R};

/// Resume fix (MP6.5, changed 2026-11-21): the app is killed while setup's
/// media step runs. The reopened core keeps the setup's own admission and the
/// same operation, whose media step runs again with no collection step, and
/// the collection that setup activated is the one still in place.
#[test]
fn meguri_setup_media_killed_mid_transfer_resumes_the_same_operation_after_reopen() {
    if let Ok(root) = std::env::var("MEGURI_MEDIA_KILL_CHILD_ROOT") {
        let root = std::path::PathBuf::from(root);
        let host = CoreHost::open(config(&root.join("local"), std::sync::Arc::new(
            meguri_model::clock::FixedClock::new(NOW, meguri_model::clock::UtcOffset::UTC)))).unwrap();
        settle(&host);
        let connection = meguri_sync::provider::MeguriConnection {
            reference: std::env::var("MEGURI_MEDIA_KILL_CHILD_REFERENCE").unwrap(),
            revision: std::env::var("MEGURI_MEDIA_KILL_CHILD_REVISION").unwrap().parse().unwrap(),
            endpoint: ENDPOINT.into() };
        let op = host.manual_transfer_history().active().unwrap().unwrap();
        let preview = media_preview(&host, &connection, &op);
        // The process dies once the media step has started its first request.
        let script = Script::new(vec![(Method::MediaBegin, Reply::After(Box::new(|| { std::process::abort(); }), vec![]))]);
        let _ = media_run(&host, &connection, &script, preview);
        panic!("the media step must die while it runs");
    }
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (_, data) = server(&root.path().join("server"));
    let script = collection_script(data, false);
    let preview = prepare(&host, &connection, &script, D::UseServerCollection);
    let op = confirm(&host, &connection, &script, preview);
    assert!(op.collection_complete && op.receipt.is_none());
    host.close().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "meguri_setup_media_killed_mid_transfer_resumes_the_same_operation_after_reopen", "--nocapture"])
        .env("MEGURI_MEDIA_KILL_CHILD_ROOT", root.path())
        .env("MEGURI_MEDIA_KILL_CHILD_REFERENCE", &connection.reference)
        .env("MEGURI_MEDIA_KILL_CHILD_REVISION", connection.revision.to_string())
        .status().unwrap();
    assert!(!status.success());

    let reopened = CoreHost::open(config).unwrap();
    settle(&reopened);
    // The kill left the media step recorded as running, on the same operation.
    let killed = reopened.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(killed.id, op.id);
    assert_eq!(killed.phase, Phase::MediaIncomplete);
    assert_eq!(killed.media.stage, meguri_sync::policy::MediaStage::Running);
    assert!(killed.collection_complete && killed.receipt.is_none());
    assert!(matches!(reopened.meguri_readiness().unwrap(), R::Unbound { intent: Some(ref id) } if *id == op.id));
    assert_eq!(reopened.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().map(|o| o.id), Some(op.id.clone()));
    // Setup still holds admission: nothing else starts meanwhile, and it is not the blocked phase.
    assert!(reopened.provider_authority().network_blocked());
    assert!(reopened.meguri_setup_transfer_holds_admission());
    // The collection setup activated is still the live one: no replacement happened.
    let live = DeviceState::for_collection(reopened.collection().unwrap().path()).settled_publication().unwrap().unwrap();
    assert_eq!(Some(live.identity), killed.resulting_publication);
    // The same operation's media step runs again, offline preparation first, and completes setup.
    let done = finish_empty_media(&reopened, &connection, &killed);
    assert_eq!(done.id, op.id);
    reopened.complete_meguri_bootstrap(&connection, USER, &done.id).unwrap();
    assert!(matches!(reopened.meguri_readiness().unwrap(), R::Ready));
    assert_eq!(reopened.provider_authority().snapshot().unwrap().meguri_binding.unwrap().operation(), &op.id);
    assert!(!reopened.provider_authority().network_blocked());
    reopened.close().unwrap();
}
