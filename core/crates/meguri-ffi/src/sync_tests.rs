//! Real host/runtime composition through the exported workflow methods. These
//! local hosts do not replace the process-wide FFI host or event listener.
use super::*;
use meguri_host::{CoreHost, HostConfig};
use meguri_model::{
    clock::{StepClock, UtcOffset},
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
    clock: Arc<StepClock>,
    random: Arc<SeededRandom>,
}
impl Fixture {
    fn new(keys: Arc<Keys>) -> Self {
        Self::with_identity(keys, None)
    }
    fn with_identity(keys: Arc<Keys>, identity: Option<Arc<CachedIdentity>>) -> Self {
        Self::with_collection(keys, identity, true)
    }
    fn without_collection(keys: Arc<Keys>) -> Self {
        Self::with_collection(keys, None, false)
    }
    fn with_collection(keys: Arc<Keys>, identity: Option<Arc<CachedIdentity>>, local: bool) -> Self {
        Self::with_recipe(keys, identity, local, None)
    }
    fn with_recipe(keys: Arc<Keys>, identity: Option<Arc<CachedIdentity>>, local: bool, recipe: Option<&str>) -> Self {
        Self::with_recipe_at_alias(keys, identity, local, recipe, false)
    }
    fn with_recipe_at_alias(
        keys: Arc<Keys>,
        identity: Option<Arc<CachedIdentity>>,
        local: bool,
        recipe: Option<&str>,
        alias: bool,
    ) -> Self {
        let directory = tempfile::tempdir().unwrap();
        let data = directory.path().join("collection");
        let backups = directory.path().join("backups");
        std::fs::create_dir_all(&data).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        let clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
        let random = Arc::new(SeededRandom::new(114));
        if let Some(recipe) = recipe {
            let recipe = meguri_testkit::generate::recipe::Recipe::parse(recipe).unwrap();
            meguri_testkit::generate::recipe::generate_recipe(&recipe, 114, &data).unwrap();
        } else if local {
            meguri_engine::bootstrap::create(&data.join("collection.anki2"), clock.as_ref(), random.as_ref()).unwrap();
        }
        let mut config = HostConfig::new(
            if alias {
                data.join("..").join("collection")
            } else {
                data
            },
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
            identity: identity.clone().map_or_else(
                || Arc::new(Account) as Arc<dyn r::IdentityProvider>,
                |cache| cache as Arc<dyn r::IdentityProvider>,
            ),
            files: Arc::new(NativeFiles),
        });
        config.cloud_key_bytes = Some(Arc::new(ForeignKeyBytes(keys)));
        let host = Arc::new(if local {
            CoreHost::open(config).unwrap()
        } else {
            meguri_host::ProcessCore::new(config.engine.clone())
                .unwrap()
                .prepare_cloud_connection(config)
                .unwrap()
        });
        let core = host.cloud_runtime().unwrap().clone();
        core.retain_adapter_inspection();
        Self {
            _directory: directory,
            workflow: CloudWorkflow { host, core, identity },
            clock,
            random,
        }
    }
    fn baseline(&self) -> (CloudEngineRegistration, CloudRecord, CloudRecord) {
        self.baseline_with_root(true)
    }
    fn baseline_with_accepted_epoch(&self) -> (CloudEngineRegistration, CloudRecord, CloudRecord) {
        let (sdk, root, incoming) = self.baseline();
        let workflow = &self.workflow;
        let collection = workflow.host.collection().unwrap();
        let projection = || {
            workflow
                .core
                .policy_projection(c::policy::Lifecycle::Ready, collection.generation().0)
                .unwrap()
        };
        let before = projection();
        assert!(!before.facts.summary.crt_known && !before.facts.summary.crt_mismatch);
        assert_eq!(before.engine.as_ref().unwrap().instance, sdk.instance_id);
        assert!(!before.engine.as_ref().unwrap().stopping);
        let seq = collection.last_seq();
        let cursor = workflow.durable_snapshot().unwrap().serialization;
        // P6 authority comes from accepted SDK intake, independently of Active/root
        // proof. This byte producer alone neither accepts an epoch nor opens apply.
        workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone.clone(),
                vec![CloudRecord {
                    name: "epoch".into(),
                    envelope: workflow.core.bound_epoch_bytes_for_testing(0, false).unwrap(),
                    system_fields: Some(vec![1]),
                    record_type: Some("MGEpoch".into()),
                    zone: Some(sdk.collection_zone.clone()),
                    ..Default::default()
                }],
                vec![],
            )
            .unwrap();
        let after = projection();
        assert!(after.facts.summary.crt_known && !after.facts.summary.crt_mismatch);
        assert_eq!(after.engine, before.engine);
        assert_eq!(after.backend_guard, before.backend_guard);
        assert_eq!(after.root_confirmed, before.root_confirmed);
        assert_eq!(after.drains, before.drains);
        assert_eq!(after.evidence, before.evidence);
        workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
        assert_eq!(collection.last_seq(), seq);
        assert_eq!(workflow.durable_snapshot().unwrap().serialization, cursor);
        assert!(workflow.core.incoming_runs().is_empty());
        assert!(workflow.core.inbox_entries(after.facts.binding.unwrap().id).unwrap().is_empty());
        (sdk, root, incoming)
    }
    fn baseline_with_root(&self, confirm_root: bool) -> (CloudEngineRegistration, CloudRecord, CloudRecord) {
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
                confirm_root.then(|| b"old-cursor".to_vec()),
            )
            .unwrap();
        self.workflow
            .core
            .reconcile_binding(&collection, &media, Duration::from_secs(1))
            .unwrap();
        if confirm_root {
            self.workflow
                .core
                .accept_root_fetch(&self.workflow.core.instance().unwrap(), Some(&root), true, 1)
                .unwrap();
        }
        let record = |r: w::Saved| CloudRecord {
            name: r.name,
            envelope: r.envelope,
            system_fields: Some(r.system_fields),
            ..Default::default()
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
    let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
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

// Synthetic remote records pass the production codec, inbox and host writer.
fn incoming_record(payload: c::entity_codec::Payload, erev: u64) -> CloudRecord {
    use c::entity_codec as codec;
    let key = c::row_payload::payload_key(&payload).unwrap();
    CloudRecord {
        name: codec::entity_name(&[114; 32], payload.kind(), &key).unwrap(),
        envelope: codec::Entity::build(
            codec::EntityFields {
                kind: payload.kind(),
                key,
                origin: [51; 16],
                erev,
                generation: None,
                tombstone: false,
                deleted_at_ms: 0,
                writer: [61; 16],
                base_sha256: None,
            },
            &payload.encode().unwrap(),
        )
        .unwrap()
        .encode()
        .unwrap(),
        system_fields: Some(erev.to_be_bytes().to_vec()),
        ..Default::default()
    }
}
fn blocked_record(erev: u64) -> CloudRecord {
    incoming_record(
        c::entity_codec::Payload::Note(c::entity_codec::Note {
            id: 114_502,
            mid: 114_999,
            guid: b"synthetic-waiting".to_vec(),
            flds: erev.to_string().into_bytes(),
            ..Default::default()
        }),
        erev,
    )
}

#[test]
fn native_fetched_drains_502_exactly_once_in_operations_of_at_most_500() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, _) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let records: Vec<_> = (0..502)
        .map(|i| {
            incoming_record(
                c::entity_codec::Payload::Config(c::entity_codec::Config {
                    key: format!("synthetic-bounded-{i}").into_bytes(),
                    mtime_secs: NOW / 1000,
                    val: b"true".to_vec(),
                }),
                2,
            )
        })
        .collect();
    workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            records.clone(),
            vec![],
        )
        .unwrap();
    let snapshot = workflow.durable_snapshot().unwrap();
    let runs = workflow.core.incoming_runs();
    assert_eq!(runs.iter().map(|r| r.entry_ids.len()).sum::<usize>(), 502);
    assert_eq!(runs.len(), 2);
    assert!(runs.iter().all(|r| !r.entry_ids.is_empty() && r.entry_ids.len() <= 500));
    let ids: std::collections::BTreeSet<_> = runs.iter().flat_map(|r| r.entry_ids.iter()).collect();
    assert_eq!(ids.len(), 502);
    let collection = workflow.host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    for (i, record) in records.iter().enumerate() {
        assert!(!snapshot.inbox_names.contains(&record.name));
        assert_eq!(
            reader.config_value(&format!("synthetic-bounded-{i}")).unwrap(),
            Some(ConfigValue::Bool(true))
        );
        let facts = workflow.entity_facts(record.name.clone()).unwrap().unwrap();
        assert!(facts.payload_digest == facts.acked_payload_digest);
    }
    drop(reader);
    let seq = collection.last_seq();
    workflow
        .fetched(sdk.instance_id, sdk.collection_zone, records, vec![])
        .unwrap();
    assert_eq!(collection.last_seq(), seq);
    assert_eq!(workflow.core.incoming_runs().len(), 2);
}

#[test]
fn native_deferred_reference() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, _) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let record = blocked_record(2);
    workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![record.clone()],
            vec![],
        )
        .unwrap();
    let binding = workflow.core.status().unwrap().binding.unwrap().id;
    let entries = workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry.record_name, record.name);
    assert!(matches!(entries[0].entry.reason, c::InboxReason::Reference));
    assert!(workflow.core.incoming_runs().is_empty());
    let cursor = vec![2, 4, 6];
    workflow.state_update(sdk.instance_id, cursor.clone()).unwrap();
    assert_eq!(workflow.durable_snapshot().unwrap().serialization, Some(cursor));
}

#[test]
fn native_fetched_advances_ready_content_after_500_notes_park_on_references() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let mut records: Vec<_> = (0..500)
        .map(|i| {
            incoming_record(
                c::entity_codec::Payload::Note(c::entity_codec::Note {
                    id: 115_000 + i,
                    mid: 114_999,
                    guid: i.to_string().into_bytes(),
                    flds: b"2".to_vec(),
                    ..Default::default()
                }),
                2,
            )
        })
        .collect();
    records.push(incoming.clone());
    workflow
        .fetched(sdk.instance_id.clone(), sdk.collection_zone, records, vec![])
        .unwrap();
    assert_eq!(fixture.incoming_value(), Some(ConfigValue::Bool(true)));
    let binding = workflow.core.status().unwrap().binding.unwrap().id;
    let entries = workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 500);
    assert!(
        entries
            .iter()
            .all(|e| matches!(e.entry.reason, c::InboxReason::Reference))
    );
    assert!(entries.iter().all(|e| e.entry.retry_count == 0));
    let runs = workflow.core.incoming_runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].entry_ids.len(), 1);
    let facts = workflow.entity_facts(incoming.name).unwrap().unwrap();
    assert!(facts.payload_digest == facts.acked_payload_digest);
    let seq = workflow.host.collection().unwrap().last_seq();
    assert!(matches!(
        workflow
            .host
            .apply_native_cloud_incoming(&sdk.instance_id, Duration::from_secs(1))
            .unwrap(),
        c::incoming::IncomingDisposition::Progress(0)
    ));
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert_eq!(workflow.core.incoming_runs().len(), 1);
    assert_eq!(workflow.core.inbox_entries(binding).unwrap(), entries);
}

fn incoming_deck(fixture: &Fixture, id: i64, parent_id: i64, name: &[u8]) -> CloudRecord {
    use c::entity_codec as codec;
    let collection = fixture.workflow.host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let row = reader
        .row_image(meguri_storage::Table::Decks, &meguri_storage::RowKey::Id(1))
        .unwrap()
        .unwrap();
    let codec::Payload::Deck(mut deck) =
        c::row_payload::project(&c::row_payload::ReadView(&*reader), codec::Kind::Deck, &row, |_| {
            Err(c::Error::InvalidData)
        })
        .unwrap()
    else {
        panic!("expected deck payload");
    };
    deck.id = id;
    deck.parent_id = parent_id;
    deck.name = name.to_vec();
    incoming_record(codec::Payload::Deck(deck), 2)
}

#[test]
fn native_fetched_retries_a_parked_reference_when_its_real_parent_arrives() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, _) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let child = incoming_deck(&fixture, 116_002, 116_001, b"RemoteParent\x1fRemoteChild");
    workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![child.clone()],
            vec![],
        )
        .unwrap();
    let binding = workflow.core.status().unwrap().binding.unwrap().id;
    let entries = workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert!(matches!(entries[0].entry.reason, c::InboxReason::Reference));
    assert!(workflow.core.incoming_runs().is_empty());
    let parent = incoming_deck(&fixture, 116_001, 0, b"RemoteParent");
    workflow
        .fetched(sdk.instance_id, sdk.collection_zone, vec![parent.clone()], vec![])
        .unwrap();
    assert!(workflow.core.inbox_entries(binding).unwrap().is_empty());
    let collection = workflow.host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    for id in [116_001, 116_002] {
        assert!(
            reader
                .row_image(meguri_storage::Table::Decks, &meguri_storage::RowKey::Id(id))
                .unwrap()
                .is_some()
        );
    }
    for record in [parent, child] {
        let facts = workflow.entity_facts(record.name).unwrap().unwrap();
        assert!(facts.payload_digest == facts.acked_payload_digest);
    }
    assert_eq!(
        workflow
            .core
            .incoming_runs()
            .iter()
            .map(|r| r.entry_ids.len())
            .sum::<usize>(),
        2
    );
}

#[test]
fn failed_rollback_tracking_transaction_stops_the_sdk_and_retains_intent_and_cursor() {
    use c::testing::{ApplyInterruption, TrackingFault};
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let before = workflow.durable_snapshot().unwrap();
    let seq = workflow.host.collection().unwrap().last_seq();
    let binding = workflow.core.status().unwrap().binding.unwrap().id;
    workflow
        .core
        .interrupt_next_incoming_for_testing(ApplyInterruption::Error);
    c::testing::arm_tracking_fault("cloud.tracking.commit.rollback", TrackingFault::Constraint).unwrap();
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
    let after = workflow.durable_snapshot().unwrap();
    assert!(after.tracking_stopped);
    assert_eq!(after.serialization, before.serialization);
    assert_eq!(after.inbox_names, vec![incoming.name.clone()]);
    let entries = workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry.retry_count, 0);
    assert!(matches!(entries[0].entry.reason, c::InboxReason::Ready));
    let intents = workflow.core.apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].inbox_ids, vec![entries[0].id]);
    assert!(intents[0].result_seq.is_none());
    assert!(workflow.core.incoming_runs().is_empty());
    assert_eq!(fixture.incoming_value(), None);
    assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
    assert!(workflow.validate_sdk(sdk.instance_id.clone()).is_err());
    assert!(workflow.register_sdk(false).is_err());
    assert!(workflow.state_update(sdk.instance_id, vec![8, 6, 4]).is_err());
    assert_eq!(workflow.durable_snapshot().unwrap().serialization, before.serialization);
    assert_eq!(workflow.core.apply_intents(binding).unwrap(), intents);
    assert_eq!(workflow.core.inbox_entries(binding).unwrap(), entries);
}

#[test]
fn native_fetched_refuses_retired_sdk_between_bounded_operations_without_tracking_stop() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, _) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let before = workflow.durable_snapshot().unwrap();
    let core = Arc::downgrade(&workflow.core);
    let instance = sdk.instance_id.clone();
    let settled = Arc::new(AtomicUsize::new(0));
    let observed = settled.clone();
    workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::IntentSettlementComplete && observed.fetch_add(1, Ordering::SeqCst) == 0
        {
            core.upgrade().unwrap().retire_sdk(&instance, false).unwrap();
        }
    });
    let records: Vec<_> = (0..502)
        .map(|i| {
            incoming_record(
                c::entity_codec::Payload::Config(c::entity_codec::Config {
                    key: format!("synthetic-{i}").into_bytes(),
                    mtime_secs: NOW / 1000,
                    val: b"true".to_vec(),
                }),
                2,
            )
        })
        .collect();
    assert!(
        workflow
            .fetched(sdk.instance_id.clone(), sdk.collection_zone, records.clone(), vec![])
            .is_err()
    );
    assert_eq!(settled.load(Ordering::SeqCst), 1);
    let after = workflow.durable_snapshot().unwrap();
    assert!(!after.tracking_stopped);
    assert_eq!(after.serialization, before.serialization);
    assert_eq!(after.inbox_names.len(), 2);
    let runs = workflow.core.incoming_runs();
    assert_eq!(runs.len(), 1);
    assert_eq!(runs[0].entry_ids.len(), 500);
    let collection = workflow.host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    for (i, record) in records.iter().enumerate() {
        assert_eq!(
            reader.config_value(&format!("synthetic-{i}")).unwrap().is_some(),
            !after.inbox_names.contains(&record.name)
        );
    }
    assert!(workflow.validate_sdk(sdk.instance_id.clone()).is_err());
    assert!(workflow.state_update(sdk.instance_id, vec![7, 5, 3]).is_err());
    assert_eq!(
        workflow.durable_snapshot().unwrap().tracking_content,
        after.tracking_content
    );
}

#[test]
fn native_fetched_keeps_a_real_commit_completed_error_fatal_and_retains_replay_intent() {
    struct Disarm;
    impl Drop for Disarm {
        fn drop(&mut self) {
            meguri_model::faultpoint::disarm("storage.commit.after");
        }
    }
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    let before = workflow.durable_snapshot().unwrap();
    let seq = workflow.host.collection().unwrap().last_seq();
    let rollback = Arc::new(AtomicUsize::new(0));
    let observed = rollback.clone();
    workflow.core.set_incoming_stage_hook_for_testing(move |stage| {
        if stage == c::testing::IncomingStage::ApplyAfterIntent {
            meguri_model::faultpoint::arm(
                "storage.commit.after",
                meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
            );
        }
        if stage == c::testing::IncomingStage::IncomingRollbackComplete {
            observed.fetch_add(1, Ordering::SeqCst);
        }
    });
    let disarm = Disarm;
    let result = workflow.fetched(sdk.instance_id, sdk.collection_zone, vec![incoming.clone()], vec![]);
    drop(disarm);
    assert!(result.is_err());
    assert_eq!(rollback.load(Ordering::SeqCst), 0);
    assert_eq!(fixture.incoming_value(), Some(ConfigValue::Bool(true)));
    let committed = workflow.host.collection().unwrap().last_seq();
    assert!(committed > seq);
    let binding = workflow.core.status().unwrap().binding.unwrap().id;
    let entries = workflow.core.inbox_entries(binding).unwrap();
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].entry.retry_count, 0);
    assert!(matches!(entries[0].entry.reason, c::InboxReason::Ready));
    let intents = workflow.core.apply_intents(binding).unwrap();
    assert_eq!(intents.len(), 1);
    assert_eq!(intents[0].inbox_ids, vec![entries[0].id]);
    assert_eq!(intents[0].result_seq, Some(committed.0));
    assert!(workflow.core.incoming_runs().is_empty());
    assert_eq!(workflow.durable_snapshot().unwrap().serialization, before.serialization);
}

#[test]
fn native_fetched_keeps_rolled_back_and_cancelled_work_without_a_retry_grant() {
    use c::testing::ApplyInterruption;
    for interruption in [ApplyInterruption::Error, ApplyInterruption::Cancel] {
        let fixture = Fixture::new(Keys::new([]));
        let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
        let workflow = &fixture.workflow;
        let seq = workflow.host.collection().unwrap().last_seq();
        workflow.core.interrupt_next_incoming_for_testing(interruption);
        workflow
            .fetched(
                sdk.instance_id.clone(),
                sdk.collection_zone.clone(),
                vec![incoming.clone()],
                vec![],
            )
            .unwrap();
        let binding = workflow.core.status().unwrap().binding.unwrap().id;
        let entries = workflow.core.inbox_entries(binding).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].entry.record_name, incoming.name);
        if interruption == ApplyInterruption::Cancel {
            assert!(matches!(entries[0].entry.reason, c::InboxReason::Ready));
            assert_eq!(entries[0].entry.retry_count, 0);
        } else {
            assert!(matches!(entries[0].entry.reason, c::InboxReason::ApplyFailed { .. }));
            assert_eq!(entries[0].entry.retry_count, 1);
        }
        assert!(workflow.core.apply_intents(binding).unwrap().is_empty());
        assert!(workflow.core.incoming_runs().is_empty());
        assert_eq!(workflow.host.collection().unwrap().last_seq(), seq);
        assert_eq!(fixture.incoming_value(), None);
        workflow.validate_sdk(sdk.instance_id.clone()).unwrap();
        workflow
            .state_update(sdk.instance_id, b"after-deferred-apply".to_vec())
            .unwrap();
        assert_eq!(
            workflow.durable_snapshot().unwrap().serialization,
            Some(b"after-deferred-apply".to_vec())
        );
        // No new receive: an ordinary failure cannot get a retry from this drain.
        if interruption != ApplyInterruption::Cancel {
            assert_eq!(workflow.host.apply_cloud_incoming(Duration::from_secs(1)).unwrap(), 0);
            assert_eq!(workflow.core.inbox_entries(binding).unwrap()[0].entry.retry_count, 1);
        }
    }
}

#[test]
fn legacy_drain_still_returns_the_actual_rolled_back_engine_error() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, incoming) = fixture.baseline_with_accepted_epoch();
    let workflow = &fixture.workflow;
    workflow
        .core
        .adapter_fetched(&sdk.instance_id, &sdk.collection_zone, &[incoming.saved()], &[])
        .unwrap();
    workflow
        .core
        .interrupt_next_incoming_for_testing(c::testing::ApplyInterruption::Cancel);
    assert!(matches!(
        workflow.host.apply_cloud_incoming(Duration::from_secs(1)),
        Err(r::RuntimeError::Engine(meguri_engine::EngineError::Cancelled))
    ));
}

#[test]
fn durable_snapshot_distinguishes_real_same_name_queued_content() {
    let fixture = Fixture::new(Keys::new([]));
    let (sdk, _, _) = fixture.baseline();
    let workflow = &fixture.workflow;
    let first = blocked_record(2);
    let newer = blocked_record(3);
    assert_eq!(first.name, newer.name);
    workflow
        .fetched(
            sdk.instance_id.clone(),
            sdk.collection_zone.clone(),
            vec![first],
            vec![],
        )
        .unwrap();
    let before = workflow.durable_snapshot().unwrap();
    workflow
        .fetched(sdk.instance_id, sdk.collection_zone, vec![newer], vec![])
        .unwrap();
    let after = workflow.durable_snapshot().unwrap();
    assert_eq!(before.inbox_names.len(), 1);
    assert_eq!(before.inbox_names, after.inbox_names);
    assert_ne!(before.tracking_content, after.tracking_content);
    assert_eq!(before.serialization, after.serialization);
    assert!(workflow.core.incoming_runs().is_empty());
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

#[path = "cloud_connection_tests.rs"]
mod connection;

#[path = "cloud_policy_tests.rs"]
mod policy;

#[path = "cloud_policy_execution_tests.rs"]
mod policy_execution;

#[path = "cloud_media_tests.rs"]
mod media_port;
