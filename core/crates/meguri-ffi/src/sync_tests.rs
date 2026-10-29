//! Real host/runtime composition through the exported workflow methods. These
//! local hosts do not replace the process-wide FFI host or event listener.
use super::*;
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use meguri_sync::cloud::publication::{Preview, PreviewWork, PublicationError, RecordRead};
use std::{
    collections::VecDeque,
    sync::atomic::{AtomicUsize, Ordering},
    time::Duration,
};

const NOW: i64 = 1_735_689_600_000;
struct Account;
impl r::IdentityProvider for Account {
    fn read_identity(&self, _: &c::BuildScope) -> r::IdentityStatus {
        r::IdentityStatus::Available(c::OpaqueBytes(b"synthetic-account-A".to_vec()))
    }
}
// Deliberately no Debug: neither adapter nor failure assertions print bytes.
struct Keys {
    answers: Mutex<VecDeque<Option<Vec<u8>>>>,
    calls: AtomicUsize,
}
impl Keys {
    fn new(answers: impl IntoIterator<Item = Option<Vec<u8>>>) -> Arc<Self> {
        Arc::new(Self {
            answers: Mutex::new(answers.into_iter().collect()),
            calls: AtomicUsize::new(0),
        })
    }
}
impl CloudKeyByteProvider for Keys {
    fn draw_key(&self) -> Option<Vec<u8>> {
        #[cfg(debug_assertions)]
        assert!(std::panic::catch_unwind(crate::core_version).is_err());
        self.calls.fetch_add(1, Ordering::SeqCst);
        self.answers
            .lock()
            .unwrap()
            .pop_front()
            .expect("unexpected secure draw")
    }
}
struct Fixture {
    _directory: tempfile::TempDir,
    workflow: CloudWorkflow,
    clock: Arc<FixedClock>,
    random: Arc<SeededRandom>,
}
impl Fixture {
    fn new(keys: Arc<Keys>) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("collection");
        let backups = directory.path().join("backups");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        let clock = Arc::new(FixedClock::new(NOW, UtcOffset::UTC));
        let random = Arc::new(SeededRandom::new(114));
        meguri_engine::bootstrap::create(&data.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
        let mut config = HostConfig::new(
            data,
            backups,
            directory.path().join("widget"),
            clock.clone(),
            random.clone(),
            Arc::new(meguri_storage::space::StatvfsSpace),
        );
        config.cloud = Some(r::CloudInputs {
            scope: c::BuildScope {
                container: c::ContainerId::new("synthetic-u114").unwrap(),
                environment: c::Environment::Development,
            },
            identity: Arc::new(Account),
            files: Arc::new(NativeFiles),
        });
        config.cloud_key_bytes = Some(Arc::new(ForeignKeyBytes(keys)));
        let host = Arc::new(CoreHost::open(config).unwrap());
        let core = host.cloud_runtime().unwrap().clone();
        core.retain_adapter_inspection();
        Self {
            _directory: directory,
            workflow: CloudWorkflow {
                host,
                core,
                identity: None,
            },
            clock,
            random,
        }
    }
    fn baseline(&self) -> (CloudEngineRegistration, CloudRecord, CloudRecord) {
        let collection = self.workflow.host.collection().unwrap();
        let media = self.workflow.host.media().unwrap();
        media.store().register_full(|_, _| true).unwrap();
        let (_, root, incoming) = self
            .workflow
            .core
            .prepare_adapter_baseline_for_testing(
                &collection,
                self.clock.as_ref(),
                self.random.as_ref(),
                Some(b"old-cursor".to_vec()),
            )
            .unwrap();
        self.workflow
            .core
            .reconcile_binding(&collection, &media, Duration::from_secs(1))
            .unwrap();
        self.workflow
            .core
            .accept_root_fetch(&self.workflow.core.instance().unwrap(), Some(&root), true, 1)
            .unwrap();
        let record = |r: w::Saved| CloudRecord {
            name: r.name,
            envelope: r.envelope,
            system_fields: Some(r.system_fields),
        };
        (
            self.workflow.register_sdk(false).unwrap(),
            record(root),
            record(incoming),
        )
    }
    fn incoming_value(&self) -> Option<ConfigValue> {
        self.workflow
            .host
            .collection()
            .unwrap()
            .reader(ReaderRole::Query)
            .unwrap()
            .config_value("synthetic-incoming")
            .unwrap()
    }
    fn preview(&self) -> Preview {
        let collection = self.workflow.host.collection().unwrap();
        let PreviewWork::ReadActive { id } = self
            .workflow
            .core
            .begin_connection_preview(&collection, &self.workflow.host.media().unwrap())
            .unwrap()
        else {
            panic!("actual unbound discovery must request active");
        };
        let PreviewWork::Ready(preview) = self
            .workflow
            .core
            .connection_active_read(id, RecordRead::ZoneMissing)
            .unwrap()
        else {
            panic!("empty synthetic cloud must offer confirmation");
        };
        *preview
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        self.workflow.host.close().unwrap();
    }
}

#[test]
fn fetched_applies_to_real_collection_and_redelivery_does_not_write_again() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline();
    let workflow = &fixture.workflow;
    assert_eq!(fixture.incoming_value(), None);
    workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![incoming.clone()],
            vec![],
        )
        .unwrap();
    assert_eq!(fixture.incoming_value(), Some(ConfigValue::Bool(true)));
    assert!(
        !workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&incoming.name)
    );
    let facts = workflow.entity_facts(incoming.name.clone()).unwrap().unwrap();
    assert!(facts.payload_digest == facts.acked_payload_digest);
    assert_eq!(facts.acked_system_fields, incoming.system_fields.clone().unwrap());
    assert_eq!(workflow.core.incoming_runs().len(), 1);
    assert!(!workflow.core.incoming_wal_receipts().is_empty());
    let seq = workflow.host.collection().unwrap().last_seq();
    workflow
        .fetched(sdk.instance_id.clone(), sdk.collection_zone, vec![incoming], vec![])
        .unwrap();
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert_eq!(workflow.core.incoming_runs().len(), 1);
    workflow
        .state_update(sdk.instance_id, b"later-cursor".to_vec())
        .unwrap();
    assert_eq!(
        workflow.durable_snapshot().unwrap().serialization,
        Some(b"later-cursor".to_vec())
    );
}

#[test]
fn fetched_rejects_wrong_zone_and_retired_sdk_before_any_mutation() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline();
    let workflow = &fixture.workflow;
    let before = workflow.durable_snapshot().unwrap().tracking_content;
    let seq = workflow.host.collection().unwrap().last_seq();
    assert!(
        workflow
            .fetched(
                sdk.instance_id.clone(),
                format!("{}-other", sdk.collection_zone),
                vec![incoming.clone()],
                vec![]
            )
            .is_err()
    );
    assert_eq!(workflow.durable_snapshot().unwrap().tracking_content, before);
    workflow.retire_sdk(sdk.instance_id.clone(), false).unwrap();
    let retired = workflow.durable_snapshot().unwrap().tracking_content;
    assert!(
        workflow
            .fetched(sdk.instance_id, sdk.collection_zone, vec![incoming], vec![])
            .is_err()
    );
    assert_eq!(workflow.durable_snapshot().unwrap().tracking_content, retired);
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert_eq!(fixture.incoming_value(), None);
    assert!(workflow.core.incoming_runs().is_empty());
}

#[test]
fn failed_inbox_commit_never_admits_apply_or_later_cursor() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline();
    let workflow = &fixture.workflow;
    let before = workflow.durable_snapshot().unwrap();
    let seq = workflow.host.collection().unwrap().last_seq();
    workflow.core.arm_adapter_faults_for_testing(true, false);
    assert!(
        workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone,
                vec![incoming.clone()],
                vec![]
            )
            .is_err()
    );
    assert!(
        workflow
            .state_update(sdk.instance_id, b"must-not-land".to_vec())
            .is_err()
    );
    let after = workflow.durable_snapshot().unwrap();
    assert_eq!(after.serialization, before.serialization);
    assert!(!after.inbox_names.contains(&incoming.name));
    assert!(after.tracking_stopped);
    assert_eq!(fixture.incoming_value(), None);
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert!(workflow.core.incoming_runs().is_empty());
}

#[test]
fn root_fetch_does_not_apply_an_already_ready_collection_entry() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, root, incoming) = fixture.baseline();
    let workflow = &fixture.workflow;
    // Receive only: root delivery must not happen to drain this queued work.
    workflow
        .core
        .adapter_fetched(&sdk.instance_id, &sdk.collection_zone, &[incoming.clone().saved()], &[])
        .unwrap();
    assert!(
        workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&incoming.name)
    );
    let seq = workflow.host.collection().unwrap().last_seq();
    workflow
        .fetched(sdk.instance_id, "meguri-root".into(), vec![root], vec![])
        .unwrap();
    assert!(
        workflow
            .durable_snapshot()
            .unwrap()
            .inbox_names
            .contains(&incoming.name)
    );
    assert_eq!(fixture.incoming_value(), None);
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert!(workflow.core.incoming_runs().is_empty());
}

#[test]
fn foreign_key_bytes_map_exactly_32_bytes_and_reject_failure_or_other_lengths() {
    use r::KeyByteSource;
    let expected = std::array::from_fn::<_, 32, _>(|i| (i as u8).wrapping_mul(7));
    let keys = Keys::new([
        Some(expected.to_vec()),
        None,
        Some(vec![]),
        Some(vec![0; 31]),
        Some(vec![0; 33]),
        Some(vec![0; 64]),
    ]);
    let source = ForeignKeyBytes(keys.clone());
    assert!(source.draw_key().unwrap() == expected);
    for _ in 0..5 {
        assert!(source.draw_key() == Err(r::KeySourceError::Unavailable));
    }
    assert_eq!(keys.calls.load(Ordering::SeqCst), 6);
    // A completed callback leaves reentry tracking clear for later exports.
    crate::core_version().unwrap();
}

#[test]
fn secure_draws_are_lazy_and_two_distinct_full_width_keys_reach_real_confirmation() {
    let record = std::array::from_fn::<_, 32, _>(|i| (i as u8).wrapping_mul(7));
    let media = std::array::from_fn::<_, 32, _>(|i| (i as u8).wrapping_mul(11).wrapping_add(3));
    let keys = Keys::new([Some(record.to_vec()), Some(media.to_vec())]);
    let fixture = Fixture::new(keys.clone());
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    let preview = fixture.preview();
    assert_eq!(keys.calls.load(Ordering::SeqCst), 0);
    let collection = fixture.workflow.host.collection().unwrap();
    fixture
        .workflow
        .core
        .confirm_connection_publication(&preview, &collection)
        .unwrap();
    let op = fixture.workflow.core.publication_op().unwrap().unwrap();
    assert!(op.record_key.0 == record && op.media_key.0 == media && record != media);
    assert_eq!(keys.calls.load(Ordering::SeqCst), 2);
}

#[test]
fn foreign_key_failure_on_either_draw_leaves_real_publication_unconfirmed() {
    for failed_draw in 0..2 {
        for failure in [None, Some(vec![0; 31]), Some(vec![0; 33])] {
            let mut answers = vec![Some(vec![7; 32]); failed_draw];
            answers.push(failure);
            let keys = Keys::new(answers);
            let fixture = Fixture::new(keys.clone());
            let preview = fixture.preview();
            let collection = fixture.workflow.host.collection().unwrap();
            let state = meguri_engine::device_state::DeviceState::for_collection(collection.path());
            let before = state.publication_decision().unwrap();
            assert!(matches!(
                fixture
                    .workflow
                    .core
                    .confirm_connection_publication(&preview, &collection),
                Err(r::RuntimeError::Publication(PublicationError::KeyUnavailable))
            ));
            assert_eq!(keys.calls.load(Ordering::SeqCst), failed_draw + 1);
            assert!(fixture.workflow.core.publication_op().unwrap().is_none());
            assert!(fixture.workflow.core.status().unwrap().binding.is_none());
            assert_eq!(state.publication_decision().unwrap(), before);
        }
    }
}
