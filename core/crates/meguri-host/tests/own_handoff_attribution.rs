//! Issuance uses the real Host/Runtime/provider; only the existing public
//! baseline fixture establishes the synthetic previously confirmed binding.
#![cfg(feature = "test-hooks")]

use std::{path::Path, sync::Arc, time::Duration};
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom};
use meguri_sync::{
    cloud::{BuildScope, ContainerId, Environment, OpaqueBytes,
        runtime::{CloudInputs, IdentityProvider, IdentityStatus, PreparationAttemptFailure, RuntimeError}},
    provider::{LegacyInput, LegacySettings, Selection},
};
use meguri_testkit::cloud::devices::NativeFiles;

const TIMEOUT: Duration = Duration::from_secs(1);

struct Account;
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec()))
    }
}

fn config(root: &Path) -> HostConfig {
    let directory = root.join("collection");
    std::fs::create_dir_all(&directory).unwrap();
    let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
    let random = Arc::new(SeededRandom::new(151));
    let mut config = HostConfig::new(directory, root.join("backups"), root.join("widget"),
        clock, random, Arc::new(meguri_storage::space::StatvfsSpace));
    config.manual_transfer_directory = root.join("installation");
    config.cloud = Some(CloudInputs {
        scope: BuildScope { container: ContainerId::new("synthetic-u114").unwrap(), environment: Environment::Development },
        identity: Arc::new(Account), files: Arc::new(NativeFiles),
    });
    config.publication_binding = meguri_engine::device_state::AccountBinding::Connected {
        account: "synthetic-account-A".into(), configuration: "synthetic-u114".into(), environment: "development".into(),
    };
    meguri_engine::bootstrap::create(&config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(), config.random.as_ref()).unwrap();
    config
}

fn admitted(config: &HostConfig) -> CoreHost {
    CoreHost::migrate_provider_configuration(&config.manual_transfer_directory, LegacyInput::Settings(
        LegacySettings { enabled: true, paused: false, automatic: true, media: true }
    )).unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    let collection = host.collection().unwrap();
    let deck = meguri_engine::decks::add(&collection, "Provider review".into()).unwrap().value.id;
    let kind = meguri_engine::notetypes::add_stock(&collection,
        meguri_engine::notetypes::StockKind::Basic, Some("Provider review".into())).unwrap().value.id;
    meguri_engine::notes::add(&collection, meguri_engine::notes::AddNote {
        notetype_id: kind, fields: vec!["Provider front".into(), "Provider back".into()],
        tags: vec![], deck_id: deck, guid: None, update_add_defaults: false,
    }, meguri_engine::OpSource::UserUndoable).unwrap();
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default()).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    runtime.prepare_adapter_baseline_for_testing(&collection,
        config.clock.as_ref(), config.random.as_ref(), None).unwrap();
    host.refresh_provider_admission();
    runtime.require_provider_collection_admission().unwrap();
    let status = runtime.status().unwrap();
    assert!(status.stored_usable && status.identity_verified);
    assert_eq!(status.binding.unwrap().facts.state, meguri_sync::cloud::BindingState::Active);
    host
}

fn identity_refusal(result: Result<(), PreparationAttemptFailure>, own_handoff: bool) {
    let failure = result.unwrap_err();
    assert!(matches!(&failure.error, RuntimeError::IdentityUnverified), "{:?}", failure.error);
    assert_eq!(failure.own_handoff().is_some(), own_handoff);
}

fn stale_refusal(result: Result<(), PreparationAttemptFailure>) {
    let failure = result.unwrap_err();
    assert!(matches!(&failure.error, RuntimeError::StaleIncarnation), "{:?}", failure.error);
    assert!(failure.own_handoff().is_none());
}

#[test]
fn r_plus_1_issued_work_refused_by_its_own_later_handoff() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), true);
    host.close().unwrap();
}

#[test]
fn r_plus_2_without_handoff_matches_unscoped_preparation() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let attempt = host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT);
    let plain = host.prepare_cloud_runtime(TIMEOUT);
    match (attempt, plain) {
        (Ok(()), Ok(())) => {},
        (Err(failure), Err(expected)) => {
            assert_eq!(format!("{:?}", failure.error), format!("{expected:?}"));
            assert!(failure.own_handoff().is_none());
        }
        (attempt, plain) => panic!("preparation changed: {attempt:?} versus {plain:?}"),
    }
    host.close().unwrap();
}

#[test]
fn n1_preexisting_handoff_cannot_issue() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    assert!(host.issue_cloud_preparation().is_none());
    identity_refusal(host.prepare_cloud_runtime(TIMEOUT).map_err(Into::into), false);
    host.close().unwrap();
}

#[test]
fn n2_paused_or_local_only_cannot_issue() {
    let root = tempfile::tempdir().unwrap();
    let paused_config = config(&root.path().join("paused"));
    let paused = admitted(&paused_config);
    let mut settings = paused.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    paused.save_provider_settings(settings).unwrap();
    assert!(paused.issue_cloud_preparation().is_none());
    identity_refusal(paused.prepare_cloud_runtime(TIMEOUT).map_err(Into::into), false);
    paused.close().unwrap();
    let local = CoreHost::open(config(&root.path().join("local"))).unwrap();
    assert!(local.issue_cloud_preparation().is_none());
    identity_refusal(local.prepare_cloud_runtime(TIMEOUT).map_err(Into::into), false);
    local.close().unwrap();
}

#[test]
fn n3_recovered_intent_and_pre_reopen_ticket_stay_strict() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let original = admitted(&config);
    let (_, ticket) = original.issue_cloud_preparation().expect("admitted issuance");
    drop(original.begin_provider_handoff(Selection::LocalOnly).unwrap());
    original.close().unwrap();
    drop(original);
    let reopened = CoreHost::open(config).unwrap();
    assert!(reopened.provider_authority().snapshot().unwrap().handoff.is_some());
    assert!(reopened.issue_cloud_preparation().is_none());
    identity_refusal(reopened.prepare_cloud_runtime(TIMEOUT).map_err(Into::into), false);
    reopened.recover_provider_handoff_to_local_only().unwrap();
    let generation = reopened.cloud_connection_generation().unwrap();
    stale_refusal(reopened.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT));
    reopened.close().unwrap();
}

#[test]
fn n4_foreign_ticket_is_rejected_even_when_new_host_is_admitted() {
    let root = tempfile::tempdir().unwrap();
    let original_config = config(&root.path().join("original"));
    let original = admitted(&original_config);
    let (_, ticket) = original.issue_cloud_preparation().expect("admitted issuance");
    let foreign_config = config(&root.path().join("foreign"));
    let foreign = admitted(&foreign_config);
    assert!(foreign.issue_cloud_preparation().is_some());
    stale_refusal(foreign.prepare_cloud_runtime_attempt(
        foreign.cloud_connection_generation().unwrap(), &ticket, TIMEOUT));
    foreign.close().unwrap();
    original.close().unwrap();
}

#[test]
fn n5_generation_changed_after_issuance() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.enter_exclusive(true).unwrap().replace(|_| Ok(())).unwrap();
    drop(slot);
    assert_ne!(host.cloud_connection_generation().unwrap(), generation);
    stale_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT));
    host.close().unwrap();
}

#[test]
fn n6_committed_handoff_carries_no_refusal_evidence() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    host.finish_provider_handoff(&handoff).unwrap();
    assert!(host.provider_authority().snapshot().unwrap().handoff.is_none());
    identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
    host.close().unwrap();
}

#[test]
fn n7_settings_pause_after_issuance_has_no_handoff() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let mut settings = host.provider_authority().snapshot().unwrap().settings;
    settings.paused = true;
    host.save_provider_settings(settings).unwrap();
    identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
    host.close().unwrap();
}

#[test]
fn n8_stopped_binding_stays_strict_with_or_without_own_handoff() {
    for handoff in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let host = admitted(&config);
        let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
        let _handoff = handoff.then(|| host.begin_provider_handoff(Selection::LocalOnly).unwrap());
        host.stop_using_icloud().unwrap();
        identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
        host.close().unwrap();
    }
}

#[test]
fn n9_unscoped_preparation_during_own_handoff_remains_plain() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let _issued = host.issue_cloud_preparation().expect("admitted issuance");
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    identity_refusal(host.prepare_cloud_runtime(TIMEOUT).map_err(Into::into), false);
    host.close().unwrap();
}

#[test]
fn n10_ordinary_close_before_handoff_invalidates_issued_attribution() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    // Same ordinary Authority entry point used by manual transfer/failed qualification.
    host.provider_authority().close_admission();
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
    host.close().unwrap();
}

#[test]
fn n11_ordinary_close_after_handoff_invalidates_issued_attribution() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    host.provider_authority().close_admission();
    identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
    host.close().unwrap();
}

fn pending_publication(host: &CoreHost, config: &HostConfig,
    step: meguri_sync::cloud::publication::PublishStep,
) -> meguri_sync::cloud::publication::PublishOp {
    use meguri_sync::cloud::{publication::{BindingBefore, PublishOp}, runtime::FileIdentityProvider};
    let binding = host.cloud_runtime().unwrap().status().unwrap().binding.unwrap();
    let path = config.collection_directory.join("collection.anki2");
    PublishOp {
        id: [171; 16], step,
        binding_before: Some(BindingBefore { id: binding.id, state: binding.facts.state }),
        binding_after: Some(binding.id), account: binding.facts.account,
        confirmed_publication: binding.facts.publication,
        confirmed_file: NativeFiles.identity(&path).unwrap(),
        fence_before: meguri_engine::device_state::DeviceState::for_collection(&path)
            .publication_decision().unwrap().unwrap(),
        epoch: binding.facts.epoch, record_key: binding.facts.record_key,
        media_key: meguri_sync::cloud::RecordKey([172; 32]),
        active: None, active_epoch: None,
        epoch_system_fields: Some(OpaqueBytes(vec![173])), complete_system_fields: None,
        root_acked: true, switch_sent: false, switch_conflict: None, active_attempt: None,
        started_at_ms: 1_894_701_600_000, retry_after_ms: None, crt: 0, reason: 1,
    }
}

// Install only source-derived, typed synthetic operation rows. The fixture
// shares production's State -> tracking order and leaves provider facts alone.
fn tracking_fixture(host: &CoreHost, config: &HostConfig,
    change: impl FnOnce(&meguri_storage::raw::PlainDatabase),
) {
    host.cloud_runtime().unwrap().with_state_held_for_testing(|| {
        meguri_sync::cloud::runtime::with_tracking_access(|| {
            let db = meguri_storage::raw::PlainDatabase::open(
                &config.collection_directory.join(meguri_sync::cloud::TRACKING_FILE)).unwrap();
            change(&db);
        });
    });
}

#[test]
fn n12_every_retained_publication_step_at_refusal_stays_strict() {
    use meguri_storage::raw::RawValue;
    use meguri_sync::cloud::publication::PublishStep;
    for step in [PublishStep::Confirmed, PublishStep::Bound, PublishStep::Zones,
        PublishStep::Sent, PublishStep::Complete, PublishStep::Cancelling] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let host = admitted(&config);
        let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
        let op = pending_publication(&host, &config, step);
        let scope = config.cloud.as_ref().unwrap().scope.clone();
        let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
        // Insert after handoff so the Host's earlier transition check cannot
        // substitute for the actual refusing guard's tracking exclusions.
        tracking_fixture(&host, &config, |db| {
            db.execute("INSERT INTO cloud_publication_ops(container,environment,id,content) VALUES(?1,0,?2,?3)",
                &[RawValue::Text(scope.container.as_str().as_bytes().to_vec()), RawValue::Blob(op.id.to_vec()),
                    RawValue::Text(serde_json::to_string(&op).unwrap().into_bytes())]).unwrap();
        });
        assert!(host.cloud_runtime().unwrap().publication_op().unwrap().as_ref() == Some(&op));
        identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
        tracking_fixture(&host, &config, |db| {
            db.execute("DELETE FROM cloud_publication_ops WHERE id=?1", &[RawValue::Blob(op.id.to_vec())]).unwrap();
        });
        host.close().unwrap();
    }
}

#[test]
fn n13_every_retained_adoption_step_at_refusal_stays_strict() {
    use meguri_storage::raw::RawValue;
    use meguri_sync::cloud::{adoption::{AdoptOp, AdoptStep}, publication::{ControlRecord, PublishStep}};
    for step in [AdoptStep::Staging, AdoptStep::Staged, AdoptStep::Ready] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path());
        let host = admitted(&config);
        let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
        let publication = pending_publication(&host, &config, PublishStep::Confirmed);
        let op = AdoptOp {
            id: [174; 16], step, binding_before: publication.binding_before,
            binding_after: publication.binding_after.unwrap(), account: publication.account,
            candidate: publication.fence_before.publication.clone(), before: Some(publication.fence_before),
            file_before: Some(publication.confirmed_file),
            active: ControlRecord { encrypted: Some(vec![175]), system_fields: OpaqueBytes(vec![176]) },
            epoch: publication.epoch, epoch_record: None, media_root: None,
            staging_name: "synthetic-own-handoff-adoption".into(),
            started_at_ms: 1_894_701_600_000, transport_wait: false, retry_after_ms: None,
        };
        let scope = config.cloud.as_ref().unwrap().scope.clone();
        let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
        tracking_fixture(&host, &config, |db| {
            db.execute("INSERT INTO cloud_adoption_ops(container,environment,content) VALUES(?1,0,?2)",
                &[RawValue::Text(scope.container.as_str().as_bytes().to_vec()),
                    RawValue::Text(serde_json::to_string(&op).unwrap().into_bytes())]).unwrap();
        });
        assert!(host.cloud_runtime().unwrap().adoption_op().unwrap().as_ref() == Some(&op));
        identity_refusal(host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT), false);
        tracking_fixture(&host, &config, |db| {
            db.execute("DELETE FROM cloud_adoption_ops WHERE container=?1 AND environment=0",
                &[RawValue::Text(scope.container.as_str().as_bytes().to_vec())]).unwrap();
        });
        host.close().unwrap();
    }
}

#[test]
fn n14_foreign_ticket_during_original_handoff_never_falls_back() {
    let root = tempfile::tempdir().unwrap();
    let original = admitted(&config(&root.path().join("original")));
    let (_, ticket) = original.issue_cloud_preparation().expect("admitted issuance");
    let _handoff = original.begin_provider_handoff(Selection::LocalOnly).unwrap();
    let foreign = admitted(&config(&root.path().join("foreign")));
    stale_refusal(foreign.prepare_cloud_runtime_attempt(
        foreign.cloud_connection_generation().unwrap(), &ticket, TIMEOUT));
    assert!(foreign.issue_cloud_preparation().is_some());
    foreign.close().unwrap();
    original.close().unwrap();
}

#[test]
fn r_plus_3_durable_handoff_alone_retains_evidence_and_redacts_debug() {
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path());
    let host = admitted(&config);
    let (generation, ticket) = host.issue_cloud_preparation().expect("admitted issuance");
    let _handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    let failure = host.prepare_cloud_runtime_attempt(generation, &ticket, TIMEOUT).unwrap_err();
    assert!(matches!(&failure.error, RuntimeError::IdentityUnverified));
    let fact = failure.own_handoff().expect("durable closure only");
    let revision = host.provider_authority().snapshot().unwrap().revision;
    assert_eq!(format!("{fact:?}"), format!(
        "OwnHandoffAdmissionRefusal {{ revision: {revision}, generation: {} }}", generation.0));
    host.close().unwrap();
}
