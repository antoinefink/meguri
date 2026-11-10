//! Focused production-phase tests; the enabled cloud_publication contract is
//! unchanged and still requires the child coordinator and incoming/media paths.
use meguri_host::CoreHost;
use meguri_model::{
    clock::{StepClock, UtcOffset},
    rng::{RandomSource, SeededRandom},
};
use meguri_sync::cloud::{
    self as cloud,
    control_codec::{DecodeOutcome, Epoch, EpochState},
    publication::*,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus},
    workflow::Saved,
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, Mutation, MutationSuccess, Operation, RecordInput, Reply, RequestedFields, Scope,
        Service, SystemFields,
        coordinator::{execute_publication_request, publication_control_record},
        devices::{NativeFiles, options},
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::Path,
    sync::{Arc, Mutex},
    time::Duration,
};
const NOW: i64 = 1_735_689_600_000;
struct Account;
struct MutableAccount(Mutex<IdentityStatus>);
impl IdentityProvider for MutableAccount {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
impl IdentityProvider for Account {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        IdentityStatus::Available(cloud::OpaqueBytes(b"synthetic-publication-account".to_vec()))
    }
}
// Deliberately no Debug: a secure provider need not expose its state to diagnostics.
struct Keys {
    calls: Mutex<usize>,
    fail: Option<usize>,
}
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let mut calls = self.calls.lock().unwrap();
        let draw = *calls;
        *calls += 1;
        if self.fail == Some(draw) {
            return Err(KeySourceError::Unavailable);
        }
        Ok(std::array::from_fn(|i| {
            (i as u8).wrapping_mul(13).wrapping_add(17 + draw as u8)
        }))
    }
}
fn scope() -> Scope {
    Scope {
        container: "test.meguri.publication-phase".into(),
        environment: "development".into(),
        account: "synthetic-publication-account".into(),
    }
}
fn open_host(dir: &Path, keys: Arc<Keys>) -> CoreHost {
    open_host_with(dir, keys, Arc::new(Account), scope())
}
fn open_host_with(dir: &Path, keys: Arc<Keys>, identity: Arc<dyn IdentityProvider>, scope: Scope) -> CoreHost {
    let mut config = options(dir, NOW, 91);
    config.clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    config.cloud = Some(CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope.container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity,
        files: Arc::new(NativeFiles),
    });
    config.cloud_key_bytes = Some(keys);
    CoreHost::open(config).unwrap()
}
fn generated(dir: &Path) {
    let recipe = Recipe::parse("recipe: publication-phase\ndescription: Public synthetic rows\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Publication\n    notetype: basic\n    new: 1\n    due: 1\n    tags: [generic]\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 91, dir).unwrap();
}

struct CountedUnavailable(std::sync::atomic::AtomicUsize);
impl IdentityProvider for CountedUnavailable {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        IdentityStatus::TemporarilyUnavailable
    }
}
fn information_bytes(dir: &Path) -> Vec<Option<Vec<u8>>> {
    [
        "collection.anki2",
        "collection.anki2-wal",
        "meguri-sidecar.json",
        "meguri-cloud.db",
        "meguri-cloud.db-wal",
    ]
    .map(|name| match std::fs::read(dir.join(name)) {
        Ok(bytes) => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => panic!("synthetic file read failed: {e}"),
    })
    .into_iter()
    .collect()
}
#[test]
fn local_information_uses_snapshot_dates_without_account_keys_or_durable_writes() {
    use meguri_storage::{ReadRows, ReaderRole};
    use std::sync::atomic::Ordering;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let account = Arc::new(CountedUnavailable(std::sync::atomic::AtomicUsize::new(0)));
    let keys = keys();
    let host = open_host_with(&dir, keys.clone(), account.clone(), scope());
    let collection = host.collection().unwrap();
    // Fixed stored history and collection modification times, independent of
    // the clock at which the information is requested.
    collection
        .run(
            meguri_engine::OpSpec::named(
                "synthetic information history",
                meguri_model::changes::OpSource::UserNonUndoable,
            ),
            |ctx| {
                let card = ctx.scope().row_page(meguri_storage::Table::Cards, None)?;
                let id = cloud::row_payload::int(&card[0], 0).map_err(|_| meguri_storage::StorageError::Io {
                    detail: "synthetic card id".into(),
                })?;
                ctx.scope().put(&meguri_model::revlog::RevlogEntry {
                    id: meguri_model::ids::RevlogId(NOW + 1234),
                    card_id: meguri_model::ids::CardId(id),
                    usn: meguri_model::usn::Usn(-1),
                    button: 3,
                    interval: 2,
                    last_interval: 1,
                    ease_factor: 2500,
                    answer_millis: 700,
                    kind: meguri_model::revlog::RevlogKind::Review,
                })?;
                Ok(())
            },
        )
        .unwrap();
    let expected = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        reader.snapshot().unwrap().transfer_facts().unwrap()
    };
    let before = information_bytes(&dir);
    let calls = account.0.load(Ordering::SeqCst);
    let info = host.local_publication_information(collection.generation()).unwrap();
    assert!(info.collection_present);
    assert_eq!(
        info.condition,
        Some(cloud::local_publication::LocalPublicationCondition::LocalOnly)
    );
    assert_eq!(info.local.as_ref().unwrap().counts[&cloud::entity_codec::Kind::Note], 2);
    assert_eq!(info.local.as_ref().unwrap().counts[&cloud::entity_codec::Kind::Card], 2);
    assert_eq!(
        info.local.as_ref().unwrap().counts[&cloud::entity_codec::Kind::Revlog],
        expected.totals.reviews
    );
    assert_eq!(info.last_review_ms, expected.last_review.map(|v| v.0));
    assert_eq!(info.last_review_ms, Some(NOW + 1234));
    assert_eq!(info.last_change_ms, Some(expected.stamps.modified.0));
    assert_eq!(info.local.as_ref().unwrap().counts.len(), 9);
    assert_eq!(account.0.load(Ordering::SeqCst), calls);
    assert_eq!(*keys.calls.lock().unwrap(), 0);
    assert_eq!(information_bytes(&dir), before);
    assert!(host.cloud_runtime().unwrap().publication_op().unwrap().is_none());
    host.cloud_runtime().unwrap().adapter_pause(true).unwrap();
    let paused_bytes = information_bytes(&dir);
    let paused_calls = account.0.load(Ordering::SeqCst);
    assert_eq!(
        host.local_publication_information(collection.generation()).unwrap(),
        info
    );
    assert_eq!(account.0.load(Ordering::SeqCst), paused_calls);
    assert_eq!(information_bytes(&dir), paused_bytes);
    host.close().unwrap();
    assert!(host.local_publication_information(collection.generation()).is_err());
}
#[test]
fn local_information_preserves_pending_reasons_and_refuses_replaced_generation() {
    use cloud::local_publication::{LocalPendingDecisionReason, LocalPublicationCondition};
    use meguri_engine::{LongOp, replace::ReplaceReason};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = CoreHost::open(options(&dir, NOW, 91)).unwrap();
    assert!(host.cloud_runtime().is_none());
    for (reason, expected) in [
        (ReplaceReason::Restore, LocalPendingDecisionReason::Restore),
        (ReplaceReason::ColpkgImport, LocalPendingDecisionReason::ColpkgImport),
        (ReplaceReason::FullDownload, LocalPendingDecisionReason::ManualImport),
    ] {
        let old = host.collection().unwrap().generation();
        let candidate = tempfile::tempdir().unwrap();
        generated(candidate.path());
        let stage = meguri_engine::replace::ReplacementStage::new(&dir).unwrap();
        std::fs::copy(candidate.path().join("collection.anki2"), stage.path()).unwrap();
        let slot = host
            .engine()
            .acquire_slot(match reason {
                ReplaceReason::Restore => LongOp::Restore,
                ReplaceReason::ColpkgImport => LongOp::ColpkgImport,
                _ => LongOp::OneWaySync,
            })
            .unwrap();
        slot.replace_verified(stage, reason, |_| Ok(())).unwrap();
        drop(slot);
        assert!(host.local_publication_information(old).is_err());
        let generation = host.collection().unwrap().generation();
        let before = information_bytes(&dir);
        let info = host.local_publication_information(generation).unwrap();
        assert_eq!(info.condition, Some(LocalPublicationCondition::Pending));
        assert_eq!(info.pending_reason, Some(expected));
        assert!(info.tracking.is_none());
        assert_eq!(information_bytes(&dir), before);
    }
    // A synthetic legacy R6.7 document has no reason member. The information
    // path must neither infer one nor rewrite the durable publication.
    let sidecar = &dir.join("meguri-sidecar.json");
    let mut doc: serde_json::Value = serde_json::from_slice(&std::fs::read(sidecar).unwrap()).unwrap();
    doc["deviceState"]
        .as_object_mut()
        .unwrap()
        .remove("pendingDecisionReason");
    std::fs::write(sidecar, serde_json::to_vec(&doc).unwrap()).unwrap();
    let before = information_bytes(&dir);
    let info = host
        .local_publication_information(host.collection().unwrap().generation())
        .unwrap();
    assert_eq!(info.condition, Some(LocalPublicationCondition::Pending));
    assert_eq!(info.pending_reason, None);
    assert_eq!(information_bytes(&dir), before);
    // Invalid metadata is an error, never a local-only/zero-count fallback.
    doc["deviceState"]["pendingDecisionReason"] = serde_json::Value::Null;
    std::fs::write(sidecar, serde_json::to_vec(&doc).unwrap()).unwrap();
    let invalid = information_bytes(&dir);
    assert!(
        host.local_publication_information(host.collection().unwrap().generation())
            .is_err()
    );
    assert_eq!(information_bytes(&dir), invalid);
    // Restore the synthetic document before the ordinary close protocol.
    doc["deviceState"]
        .as_object_mut()
        .unwrap()
        .remove("pendingDecisionReason");
    std::fs::write(sidecar, serde_json::to_vec(&doc).unwrap()).unwrap();
    host.close().unwrap();
}
#[test]
fn local_information_never_follows_a_replacement_or_closed_host() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = Arc::new(CoreHost::open(options(&dir, NOW, 91)).unwrap());
    let generation = host.collection().unwrap().generation();
    let candidate = tempfile::tempdir().unwrap();
    generated(candidate.path());
    let stage = meguri_engine::replace::ReplacementStage::new(&dir).unwrap();
    std::fs::copy(candidate.path().join("collection.anki2"), stage.path()).unwrap();
    let (reached, wait) = std::sync::mpsc::channel();
    let (release, resume) = std::sync::mpsc::channel();
    let replacing = host.clone();
    let worker = std::thread::spawn(move || {
        let slot = replacing.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        exclusive
            .replace_verified_before_commit(
                stage,
                meguri_engine::replace::ReplaceReason::Restore,
                |_| Ok(()),
                || {
                    reached.send(()).unwrap();
                    resume.recv_timeout(Duration::from_secs(30)).unwrap();
                    Ok(())
                },
            )
            .unwrap();
    });
    wait.recv_timeout(Duration::from_secs(30)).unwrap();
    // An exclusive phase can refuse the read or still provide the original
    // admitted view. It can never return facts for a successor generation.
    if let Ok(info) = host.local_publication_information(generation) {
        assert_eq!(
            info.condition,
            Some(cloud::local_publication::LocalPublicationCondition::LocalOnly)
        );
    }
    release.send(()).unwrap();
    worker.join().unwrap();
    assert!(host.local_publication_information(generation).is_err());
    let current = host.collection().unwrap().generation();
    let closing = host.clone();
    let closer = std::thread::spawn(move || closing.close());
    // Exercise the same read against concurrent reader drain. Either a fully
    // validated current snapshot or an explicit lifecycle refusal is correct.
    if let Ok(info) = host.local_publication_information(current) {
        assert_eq!(
            info.condition,
            Some(cloud::local_publication::LocalPublicationCondition::Pending)
        );
    }
    closer.join().unwrap().unwrap();
    assert!(host.local_publication_information(current).is_err());
}
#[test]
fn local_information_qualifies_isolated_work_without_claiming_a_retained_backup() {
    use cloud::{
        entity_codec::{Kind, Payload},
        local_publication::*,
    };
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(616)).unwrap();
    publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    assert_eq!(
        host.local_publication_information(collection.generation())
            .unwrap()
            .condition,
        Some(LocalPublicationCondition::Clear)
    );
    let entity = host
        .cloud_runtime()
        .unwrap()
        .entity_facts_page(Kind::Note, None)
        .unwrap()
        .remove(0);
    let Payload::Note(note) = entity.payload else {
        panic!("synthetic note required");
    };
    edit_workflow_note(&collection, &note, "unpublished synthetic change");
    restore_collection(&host, &dir);
    let generation = host.collection().unwrap().generation();
    let before = information_bytes(&dir);
    let calls = service.request_log().unwrap().len();
    let info = host.local_publication_information(generation).unwrap();
    let tracking = info.tracking.unwrap();
    assert_eq!(tracking.isolated.len(), 1);
    assert_eq!(tracking.isolated[0].pending_by_kind[&Kind::Note], 1);
    assert_eq!(tracking.isolated[0].content, IsolatedLocalContent::ReplacedLocally);
    assert_eq!(tracking.isolated[0].backup, LocalBackupKnowledge::Unknown);
    assert_eq!(tracking.unresolved_conflict_copies, 0);
    assert_eq!(service.request_log().unwrap().len(), calls);
    assert_eq!(information_bytes(&dir), before);
    host.close().unwrap();
}
#[test]
fn local_information_counts_isolated_revision_after_dirty_mark_loss_without_mutation() {
    use cloud::{
        entity_codec::{Kind, Payload},
        local_publication::*,
    };
    use meguri_storage::raw::{PlainDatabase, RawValue};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(617)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let entity = runtime.entity_facts_page(Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &entity.payload else {
        panic!("synthetic note required");
    };
    edit_workflow_note(&host.collection().unwrap(), note, "synthetic revision without mark");
    restore_collection(&host, &dir);
    let runtime = host.cloud_runtime().unwrap();
    // Lose only the mark on the already isolated ordinary owner. Keep the
    // captured revision, payload, ACK, flights and pending state intact.
    cloud::runtime::with_tracking_access(|| {
        let db = PlainDatabase::open(&dir.join(cloud::TRACKING_FILE)).unwrap();
        assert_eq!(
            db.execute(
                "UPDATE cloud_runtime_notes SET content=json_set(content,'$.mark',NULL) WHERE binding=?1 AND id=?2",
                &[RawValue::Blob(binding.id.0.to_vec()), RawValue::Integer(note.id)],
            )
            .unwrap(),
            1
        );
    });
    let before_entity = runtime.binding_entity(binding.id, &entity.name).unwrap().unwrap();
    assert!(before_entity.mark.is_none());
    assert!(before_entity.local_rev > before_entity.acked_rev);
    let generation = host.collection().unwrap().generation();
    let before = information_bytes(&dir);
    let status = format!("{:?}", runtime.status().unwrap());
    let pending = runtime.tracking_sendables();
    let observations = runtime.observations();
    let emitted = runtime.emitted_update_ids();
    let guard = runtime.adapter_guard();
    let calls = service.request_log().unwrap().len();
    let key_calls = *keys.calls.lock().unwrap();
    let info = host.local_publication_information(generation).unwrap();
    let tracking = info.tracking.as_ref().unwrap();
    assert_eq!(tracking.isolated.len(), 1);
    assert_eq!(tracking.isolated[0].pending_by_kind[&Kind::Note], 1);
    assert_eq!(tracking.isolated[0].content, IsolatedLocalContent::ReplacedLocally);
    assert_eq!(tracking.isolated[0].backup, LocalBackupKnowledge::Unknown);
    assert_eq!(host.local_publication_information(generation).unwrap(), info);
    assert_eq!(information_bytes(&dir), before);
    assert!(runtime.binding_entity(binding.id, &entity.name).unwrap().unwrap() == before_entity);
    assert_eq!(format!("{:?}", runtime.status().unwrap()), status);
    assert_eq!(runtime.tracking_sendables(), pending);
    assert!(runtime.observations() == observations);
    assert_eq!(runtime.emitted_update_ids(), emitted);
    assert_eq!(runtime.adapter_guard(), guard);
    assert_eq!(service.request_log().unwrap().len(), calls);
    assert_eq!(*keys.calls.lock().unwrap(), key_calls);
    host.close().unwrap();
}

fn information_copy(db: &meguri_storage::raw::PlainDatabase, owner: cloud::BindingId, id: u8, resolved: Option<i64>) {
    use meguri_storage::raw::RawValue;
    let copy = cloud::ConflictCopy {
        id: cloud::CopyId([id; 16]),
        kind: cloud::ConflictKind::Note,
        key: cloud::OpaqueBytes(vec![id]),
        reason: cloud::ConflictReason::FieldConflict,
        losing_envelope: cloud::OpaqueBytes(b"synthetic private copy payload".to_vec()),
        losing_payload_digest: cloud::PayloadDigest([id; 32]),
        winning_payload_digest: cloud::PayloadDigest([0; 32]),
        field_names: None,
        template_names: None,
        note_deck: None,
        staged_path: None,
        created_at_ms: NOW,
        resolved_at_ms: resolved,
    };
    db.execute(
        "INSERT INTO conflicts(id,binding,content) VALUES(?1,?2,?3)",
        &[
            RawValue::Blob(copy.id.0.to_vec()),
            RawValue::Blob(owner.0.to_vec()),
            RawValue::text(&serde_json::to_string(&copy).unwrap()),
        ],
    )
    .unwrap();
}

#[test]
fn local_information_filters_other_container_and_environment_copies_before_decoding() {
    local_information_foreign_copies(false);
}

#[test]
fn local_information_filters_salvage_copies_before_decoding() {
    local_information_foreign_copies(true);
}

fn local_information_foreign_copies(salvage: bool) {
    use meguri_storage::raw::{PlainDatabase, RawValue};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(618)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let generation = host.collection().unwrap().generation();
    cloud::runtime::with_tracking_access(|| {
        let db = PlainDatabase::open(&dir.join(cloud::TRACKING_FILE)).unwrap();
        information_copy(&db, binding.id, 201, None);
        information_copy(&db, binding.id, 202, None);
        information_copy(&db, binding.id, 203, Some(NOW));
    });
    let expected = host.local_publication_information(generation).unwrap();
    assert_eq!(expected.tracking.as_ref().unwrap().unresolved_conflict_copies, 2);
    cloud::runtime::with_tracking_access(|| {
        let db = PlainDatabase::open(&dir.join(cloud::TRACKING_FILE)).unwrap();
        for (id, container, environment) in [
            (211, "test.meguri.foreign-information", 0),
            (212, binding.scope.container.as_str(), 1),
        ] {
            let owner = cloud::BindingId([id; 16]);
            let mut facts = binding.facts.clone();
            facts.state = cloud::BindingState::Isolated(if salvage {
                cloud::IsolationReason::TrackingDamaged
            } else {
                cloud::IsolationReason::Stopped
            });
            if salvage && id == 212 {
                let metadata = cloud::SalvageBinding {
                    id: owner,
                    source: "meguri-cloud.db.damaged-2025-01-01-00-00-00.salvaging".into(),
                    original_binding: None,
                    created_at_ms: NOW,
                    original_created_at_ms: None,
                    container: Some(binding.scope.container.clone()),
                    environment: Some(binding.scope.environment),
                    account: None,
                    epoch: None,
                    record_key: None,
                    publication: None,
                };
                db.execute(
                    "INSERT INTO salvage_bindings(id,metadata) VALUES(?1,?2)",
                    &[
                        RawValue::Blob(owner.0.to_vec()),
                        RawValue::text(&serde_json::to_string(&metadata).unwrap()),
                    ],
                )
                .unwrap();
            } else {
                db.execute(
                    "INSERT INTO bindings(id,container,environment,created_at_ms,facts) VALUES(?1,?2,?3,?4,?5)",
                    &[
                        RawValue::Blob(owner.0.to_vec()),
                        RawValue::text(if salvage {
                            binding.scope.container.as_str()
                        } else {
                            container
                        }),
                        RawValue::Integer(if salvage { 0 } else { environment }),
                        RawValue::Integer(NOW),
                        RawValue::text(&serde_json::to_string(&facts).unwrap()),
                    ],
                )
                .unwrap();
            }
            information_copy(&db, owner, id, None);
            // Valid SQLite JSON, deliberately not a decodable ConflictCopy.
            assert_eq!(
                db.execute(
                    "UPDATE conflicts SET content='{}' WHERE id=?1",
                    &[RawValue::Blob(vec![id; 16]),]
                )
                .unwrap(),
                1
            );
        }
    });
    let before = information_bytes(&dir);
    let pending = runtime.tracking_sendables();
    let status = format!("{:?}", runtime.status().unwrap());
    let calls = service.request_log().unwrap().len();
    assert_eq!(host.local_publication_information(generation).unwrap(), expected);
    assert_eq!(host.local_publication_information(generation).unwrap(), expected);
    assert_eq!(information_bytes(&dir), before);
    assert_eq!(runtime.tracking_sendables(), pending);
    assert_eq!(format!("{:?}", runtime.status().unwrap()), status);
    assert_eq!(service.request_log().unwrap().len(), calls);
    // Remove only deliberately corrupt fixture rows before ordinary shutdown.
    cloud::runtime::with_tracking_access(|| {
        let db = PlainDatabase::open(&dir.join(cloud::TRACKING_FILE)).unwrap();
        db.execute(
            "DELETE FROM conflicts WHERE id IN (?1,?2)",
            &[RawValue::Blob(vec![211; 16]), RawValue::Blob(vec![212; 16])],
        )
        .unwrap();
    });
    host.close().unwrap();
}

fn preview(host: &CoreHost, service: &mut Service<SeededRandom>) -> Preview {
    let runtime = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = runtime
        .begin_connection_preview(&host.collection().unwrap(), &host.media().unwrap())
        .unwrap()
    else {
        panic!("read required");
    };
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let read = match response.result {
        Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
        Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
        other => panic!("unexpected actual read: {}", other.is_ok()),
    };
    let PreviewWork::Ready(p) = runtime.connection_active_read(id, read).unwrap() else {
        panic!("empty cloud expected");
    };
    *p
}
fn own_until_scan(host: &CoreHost, service: &mut Service<SeededRandom>) {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    for _ in 0..16 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => {
                let (result, _) =
                    execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Scan => return,
            other => panic!("publication stopped before its scan: {other:?}"),
        }
    }
    panic!("publication never requested its scan");
}
#[test]
fn secure_key_failure_has_no_confirmation_or_binding_and_preview_draws_nothing() {
    for failed in [0, 1] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let keys = Arc::new(Keys {
            calls: Mutex::new(0),
            fail: Some(failed),
        });
        let host = open_host(&dir, keys.clone());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let before = meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
            .publication_decision()
            .unwrap();
        let p = preview(&host, &mut service);
        assert_eq!(*keys.calls.lock().unwrap(), 0);
        let runtime = host.cloud_runtime().unwrap();
        assert!(
            runtime
                .confirm_connection_publication(&p, &host.collection().unwrap())
                .is_err()
        );
        assert_eq!(*keys.calls.lock().unwrap(), failed + 1);
        assert!(runtime.publication_op().unwrap().is_none());
        assert!(runtime.status().unwrap().binding.is_none());
        assert_eq!(
            meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
                .publication_decision()
                .unwrap(),
            before
        );
        assert!(service.inspect(&scope()).unwrap().is_none_or(|db| db.zones.is_empty()));
        host.close().unwrap();
    }
}
#[test]
fn initial_rows_are_unacknowledged_until_actual_sdk_saves_and_own_switch_is_last() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = Arc::new(Keys {
        calls: Mutex::new(0),
        fail: None,
    });
    let host = open_host(&dir, keys.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let confirmed = runtime.publication_op().unwrap().unwrap();
    assert_eq!(confirmed.step, PublishStep::Confirmed);
    assert!(confirmed.record_key.0 == std::array::from_fn(|i| (i as u8).wrapping_mul(13).wrapping_add(17)));
    assert!(confirmed.media_key.0 == std::array::from_fn(|i| (i as u8).wrapping_mul(13).wrapping_add(18)));
    assert!(runtime.status().unwrap().binding.is_none());
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let mut entities = BTreeMap::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() {
                break;
            }
            for entity in page {
                after = Some(entity.key.clone());
                assert_eq!(entity.acked_rev, 0);
                assert_eq!(entity.server_erev, 0);
                assert!(entity.acked_payload.is_none() && entity.acked_system_fields.is_empty());
                assert!(entity.mark.is_some());
                entities.insert(entity.name.clone(), entity);
            }
        }
    }
    assert!(entities.values().any(|e| e.kind == cloud::entity_codec::Kind::Revlog));
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    for _ in 0..32 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::SendEntities => {
                let names: BTreeSet<_> = entities.keys().cloned().collect();
                let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
                assert!(!batch.saves.is_empty());
                let changes = batch
                    .saves
                    .iter()
                    .map(|s| Mutation::Save {
                        record: RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone()),
                        if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
                    })
                    .collect();
                let response = service
                    .request(
                        &scope(),
                        Caller {
                            instance: Some(instance.clone()),
                            ..Caller::default()
                        },
                        Operation::Modify {
                            zone: cloud::adapter::collection_zone(binding.facts.epoch),
                            atomic: false,
                            changes,
                        },
                    )
                    .unwrap();
                let Ok(Reply::Modified(items)) = response.result else {
                    panic!("actual saves expected");
                };
                let mut saved = Vec::new();
                for item in items {
                    let Ok(MutationSuccess::Saved(record)) = item.result else {
                        panic!("successful save expected");
                    };
                    let Some(Field::EncryptedBytes(envelope)) = record.fields.get("e") else {
                        panic!("encrypted entity");
                    };
                    saved.push(Saved {
                        name: record.name.clone(),
                        envelope: envelope.clone(),
                        system_fields: record.system_fields.bytes().to_vec(),
                    });
                }
                assert_ne!(batch.id, response.request_id, "Service and runtime use independent IDs");
                runtime.enter_sent_handler(&instance, batch.id).unwrap();
                runtime
                    .sent_results(&instance, &collection, batch.id, &saved, &[], saved.len())
                    .unwrap();
                runtime.return_sent_handler(&instance).unwrap();
            }
            PublicationWork::Own(request) => {
                if request.kind == OwnKind::SaveActive {
                    assert!(
                        entities
                            .keys()
                            .all(|n| runtime.entity_facts(n).unwrap().unwrap().mark.is_none())
                    );
                }
                let (result, _) =
                    execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Idle if runtime.publication_op().unwrap().is_none() => break,
            other => panic!("unexpected stop: {other:?}"),
        }
    }
    assert!(runtime.publication_op().unwrap().is_none());
    let db = service.inspect(&scope()).unwrap().unwrap();
    let epoch = &db.zones[&cloud::adapter::collection_zone(binding.facts.epoch)].records["epoch"];
    let Field::EncryptedBytes(bytes) = &epoch.fields["e"] else {
        panic!("encrypted epoch");
    };
    let DecodeOutcome::Supported(epoch) = Epoch::decode(bytes).unwrap() else {
        panic!("supported epoch");
    };
    assert_eq!(epoch.state, EpochState::Complete);
    assert_eq!(epoch.counts.len(), 9);
    assert_eq!(*keys.calls.lock().unwrap(), 2);
    let media_authority = runtime.publication_media_state().unwrap().unwrap();
    assert!(media_authority.0.0 == confirmed.media_key.0 && media_authority.1);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys.clone());
    assert_eq!(
        reopened.cloud_runtime().unwrap().status().unwrap().binding.unwrap(),
        binding
    );
    assert_eq!(*keys.calls.lock().unwrap(), 2);
    assert!(
        reopened
            .cloud_runtime()
            .unwrap()
            .publication_media_state()
            .unwrap()
            .unwrap()
            == media_authority
    );
    reopened.close().unwrap();
}

fn keys() -> Arc<Keys> {
    Arc::new(Keys {
        calls: Mutex::new(0),
        fail: None,
    })
}
fn account_binding(account: &str) -> meguri_engine::device_state::AccountBinding {
    meguri_engine::device_state::AccountBinding::Connected {
        account: account.into(),
        configuration: scope().container,
        environment: scope().environment,
    }
}
fn decision(host: &CoreHost) -> DecisionSnapshot {
    meguri_engine::device_state::DeviceState::for_collection(host.collection().unwrap().path())
        .publication_decision()
        .unwrap()
        .unwrap()
}
fn candidate_stage(dir: &Path) -> meguri_engine::replace::ReplacementStage {
    let candidate = dir.join("candidate-source");
    generated(&candidate);
    let stage = meguri_engine::replace::ReplacementStage::new(dir).unwrap();
    std::fs::copy(candidate.join("collection.anki2"), stage.path()).unwrap();
    stage
}
fn next_own(host: &CoreHost) -> OwnRequest {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    for _ in 0..4 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => return *request,
            other => panic!("own request required: {other:?}"),
        }
    }
    panic!("no own request");
}

// Q11: the host must pass actual platform authority into the existing R6.6
// crash recovery, including unavailable accounts. No decision fixture overrides.
#[test]
fn unfinished_adoption_restart_uses_actual_host_account_and_real_staging_proofs() {
    use meguri_engine::device_state::{DeviceState, PublicationDecision};
    use sha2::{Digest, Sha256};
    use std::os::unix::fs::MetadataExt;
    for point in [
        "replace.after_rename",
        "replace.after_dir_sync",
        "replace.after_durable_activation",
    ] {
        for unavailable in [false, true] {
            let temp = tempfile::tempdir().unwrap();
            let dir = temp.path().join("device");
            generated(&dir);
            crash_worker(&dir, "adopt", point);
            let live = dir.join("collection.anki2");
            let document: serde_json::Value =
                serde_json::from_slice(&std::fs::read(dir.join("meguri-sidecar.json")).unwrap()).unwrap();
            let intent = &document["deviceState"]["intent"];
            assert!(!intent.is_null());
            let bytes = std::fs::read(&live).unwrap();
            let digest: [u8; 32] = Sha256::digest(&bytes).into();
            assert_eq!(
                intent["candidate_file"]["sha256"],
                serde_json::to_value(digest).unwrap()
            );
            assert_eq!(intent["candidate_file"]["length"].as_u64(), Some(bytes.len() as u64));
            let native = std::fs::symlink_metadata(&live).unwrap();
            assert_eq!(intent["candidate_native"]["device"].as_u64(), Some(native.dev()));
            assert_eq!(intent["candidate_native"]["inode"].as_u64(), Some(native.ino()));
            let candidate: meguri_engine::device_state::Publication =
                serde_json::from_value(intent["candidate"].clone()).unwrap();
            for _ in 0..2 {
                let identity = Arc::new(MutableAccount(Mutex::new(if unavailable {
                    IdentityStatus::ReadFailed
                } else {
                    IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()))
                })));
                let host = open_host_with(&dir, keys(), identity, scope());
                let state = decision(&host);
                assert_eq!(state.publication.identity, candidate.identity);
                assert_eq!(state.publication.binding, candidate.binding);
                assert_eq!(state.publication.decision, PublicationDecision::Pending);
                assert_eq!(state.pending_reason, None);
                assert!(
                    DeviceState::for_collection(&live)
                        .publication_token(&account_binding("changed-account"))
                        .is_err()
                );
                let recovered: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(dir.join("meguri-sidecar.json")).unwrap()).unwrap();
                assert!(recovered["deviceState"]["intent"].is_null());
                host.close().unwrap();
            }
        }
    }
}

#[test]
fn cloud_publication_crash_worker() {
    use meguri_model::faultpoint::{self, Fault};
    let Some(dir) = std::env::var_os("MEGURI_PUBLICATION_CRASH_DIR") else {
        return;
    };
    let dir = std::path::PathBuf::from(dir);
    let mode = std::env::var("MEGURI_PUBLICATION_CRASH_MODE").unwrap();
    let point = std::env::var("MEGURI_PUBLICATION_CRASH_POINT").unwrap();
    let host = open_host(&dir, keys());
    if mode == "adopt" {
        use meguri_engine::{
            LongOp,
            device_state::{Publication, PublicationDecision},
            replace::CloudAdoptInput,
        };
        let stage = candidate_stage(&dir);
        let input = CloudAdoptInput {
            operation: serde_json::from_value(serde_json::json!([6106, 1])).unwrap(),
            candidate: Publication {
                identity: serde_json::from_value(serde_json::json!([6106, 2])).unwrap(),
                binding: account_binding(&scope().account),
                decision: PublicationDecision::Clear,
            },
        };
        let slot = host.engine().acquire_slot(LongOp::CloudAdopt).unwrap();
        let mut exclusive = slot.enter_exclusive(true).unwrap();
        faultpoint::arm(&point, Fault::Abort);
        exclusive.replace_verified_cloud_adopt(stage, input).unwrap();
    } else {
        assert!(mode == "decision" || mode == "cancel");
        if mode == "cancel" {
            restore_collection(&host, &dir);
        }
        let mut service = Service::open(dir.parent().unwrap(), Config::default(), SeededRandom::new(44)).unwrap();
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let p = preview(&host, &mut service);
        let op = runtime.confirm_connection_publication(&p, &collection).unwrap();
        runtime
            .perform_publication_local(op, LocalWork::Bind, &collection)
            .unwrap();
        if mode == "decision" {
            faultpoint::arm(&point, Fault::Abort);
        }
        runtime
            .perform_publication_local(op, LocalWork::WriteDecision, &collection)
            .unwrap();
        if mode == "cancel" {
            faultpoint::arm(&point, Fault::Abort);
            runtime.cancel_publication(op, &collection).unwrap();
        }
    }
    panic!("actual crash boundary was not reached");
}
fn crash_worker(dir: &Path, mode: &str, point: &str) {
    use std::os::unix::process::ExitStatusExt;
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", "cloud_publication_crash_worker", "--nocapture"])
        .env("MEGURI_PUBLICATION_CRASH_DIR", dir)
        .env("MEGURI_PUBLICATION_CRASH_MODE", mode)
        .env("MEGURI_PUBLICATION_CRASH_POINT", point)
        .status()
        .unwrap();
    assert_eq!(status.signal(), Some(6));
}

#[test]
fn decision_write_kill_restart_reopens_durable_binding_and_preserves_new_restore_or_other_build_decisions() {
    use meguri_engine::{LongOp, device_state::PublicationDecision, replace::ReplaceReason};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    crash_worker(&dir, "decision", "cloud.publish.after_fence_clear");
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.step, PublishStep::Bound);
    assert_eq!(decision(&host).publication.decision, PublicationDecision::Clear);
    assert_eq!(decision(&host).publication.binding, account_binding(&scope().account));
    // Exercise the writer's actual future reopen binding as well as startup.
    assert!(matches!(
        host.engine().suspend(Duration::from_secs(30)).unwrap(),
        meguri_engine::suspend::SuspendOutcome::Suspended
    ));
    host.engine().resume().unwrap();
    let collection = host.collection().unwrap();
    let stage = candidate_stage(&dir);
    let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
    drop(slot);
    let fresh = host.collection().unwrap();
    let restored = decision(&host);
    assert_eq!(restored.publication.decision, PublicationDecision::Pending);
    assert!(
        collection
            .resolve_cloud_publication_decision(op.id, &op.fence_before)
            .is_err()
    );
    assert!(fresh.resolve_cloud_publication_decision(op.id, &restored).is_err());
    assert_eq!(decision(&host), restored);
    host.close().unwrap();
    drop(host);
    let mut other_scope = scope();
    other_scope.container.push_str(".other");
    let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope);
    assert!(
        other
            .collection()
            .unwrap()
            .resolve_cloud_publication_decision(op.id, &restored)
            .is_err()
    );
    assert_eq!(decision(&other), restored);
    other.close().unwrap();
}

#[test]
fn no_durable_recovery_row_cannot_resolve_a_reader_supplied_decision() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    for cloud_enabled in [false, true] {
        let host = if cloud_enabled {
            open_host(&dir, keys())
        } else {
            CoreHost::open(options(&dir, NOW, 91)).unwrap()
        };
        let before = decision(&host);
        assert!(
            host.collection()
                .unwrap()
                .resolve_cloud_publication_decision([7; 16], &before)
                .is_err()
        );
        assert_eq!(decision(&host), before);
        host.close().unwrap();
    }
}

#[test]
fn held_publication_request_revalidates_account_file_decision_and_restart_before_service_submission() {
    use meguri_engine::{LongOp, replace::ReplaceReason};
    for change in ["account", "unavailable", "file", "decision", "restart"] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
            cloud::OpaqueBytes(scope().account.into_bytes()),
        ))));
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        runtime.confirm_connection_publication(&p, &collection).unwrap();
        let held = next_own(&host);
        let after = service.request_log().unwrap().len();
        match change {
            "account" => {
                *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()))
            }
            "unavailable" => *account.0.lock().unwrap() = IdentityStatus::ReadFailed,
            "file" => {
                // Rename the real open file out and back via a different native
                // file, preserving bytes; no file-provider fixture overrides.
                let live = collection.path();
                let copy = dir.join("held-file-copy");
                std::fs::copy(live, &copy).unwrap();
                std::fs::rename(live, dir.join("held-file-original")).unwrap();
                std::fs::rename(copy, live).unwrap();
            }
            "decision" => {
                let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
                slot.replace_verified(candidate_stage(&dir), ReplaceReason::Restore, |_| Ok(()))
                    .unwrap();
            }
            "restart" => {
                host.close().unwrap();
                let reopened = open_host(&dir, keys());
                assert!(
                    execute_publication_request(
                        &mut service,
                        &scope(),
                        &held,
                        reopened.cloud_runtime().unwrap(),
                        &reopened.collection().unwrap()
                    )
                    .is_err()
                );
                assert_eq!(service.request_log().unwrap().len(), after);
                reopened.close().unwrap();
                continue;
            }
            _ => unreachable!(),
        }
        assert!(execute_publication_request(&mut service, &scope(), &held, runtime, &collection).is_err());
        assert_eq!(
            service.request_log().unwrap().len(),
            after,
            "stale request must reach no Service operation"
        );
        if change == "file" {
            std::fs::remove_file(collection.path()).unwrap();
            std::fs::rename(dir.join("held-file-original"), collection.path()).unwrap();
        }
        host.close().unwrap();
    }
}

fn entity_names(runtime: &cloud::runtime::Runtime) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for kind in cloud::row_payload::KINDS {
        let mut after = None;
        loop {
            let page = runtime.entity_facts_page(kind, after.as_deref()).unwrap();
            if page.is_empty() {
                break;
            }
            for entity in page {
                after = Some(entity.key);
                names.insert(entity.name);
            }
        }
    }
    names
}
fn send_actual_batch(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    batch: &cloud::workflow::BuiltBatch,
) -> BTreeSet<String> {
    send_actual_batch_with_instance(host, service, batch, host.cloud_runtime().unwrap().instance().unwrap())
}
fn send_actual_batch_with_instance(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    batch: &cloud::workflow::BuiltBatch,
    instance: String,
) -> BTreeSet<String> {
    use cloud::workflow::SendError;
    use meguri_testkit::cloud::ErrorCode;
    let runtime = host.cloud_runtime().unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let changes = batch
        .saves
        .iter()
        .map(|s| Mutation::Save {
            record: RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone()),
            if_unchanged: s.system_fields.clone().map(SystemFields::from_bytes),
        })
        .collect();
    let response = service
        .request(
            &scope(),
            Caller {
                instance: Some(instance.clone()),
                ..Caller::default()
            },
            Operation::Modify {
                zone: cloud::adapter::collection_zone(binding.facts.epoch),
                atomic: false,
                changes,
            },
        )
        .unwrap();
    let Ok(Reply::Modified(items)) = response.result else {
        panic!("per-record outcomes required");
    };
    let mut saved = Vec::new();
    let mut failed = Vec::new();
    let mut failed_names = BTreeSet::new();
    for item in items {
        match item.result {
            Ok(MutationSuccess::Saved(record)) => {
                let Field::EncryptedBytes(envelope) = &record.fields["e"] else {
                    panic!("encrypted entity");
                };
                saved.push(Saved {
                    name: record.name.clone(),
                    envelope: envelope.clone(),
                    system_fields: record.system_fields.bytes().to_vec(),
                });
            }
            Err(error) => {
                assert_eq!(error.code, ErrorCode::LimitExceeded);
                failed_names.insert(item.name.clone());
                failed.push((item.name, SendError::LimitExceeded));
            }
            _ => panic!("entity save expected"),
        }
    }
    runtime.enter_sent_handler(&instance, batch.id).unwrap();
    runtime
        .sent_results(
            &instance,
            &host.collection().unwrap(),
            batch.id,
            &saved,
            &failed,
            batch.saves.len(),
        )
        .unwrap();
    runtime.return_sent_handler(&instance).unwrap();
    failed_names
}

#[test]
fn non_atomic_parent_failure_never_submits_children_before_persisted_parent_acknowledgement() {
    use cloud::entity_codec::{self as codec, Payload};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    // Fixed public key vector: draw 19 puts this Notetype before its new Note.
    let host = open_host(
        &dir,
        Arc::new(Keys {
            calls: Mutex::new(19),
            fail: None,
        }),
    );
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let names = entity_names(runtime);
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let binding = runtime.status().unwrap().binding.unwrap();
    let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    let parent = batch
        .saves
        .iter()
        .filter(|s| runtime.entity_facts(&s.name).unwrap().unwrap().kind == codec::Kind::Notetype)
        .max_by_key(|s| s.envelope.len())
        .expect("initial parent in actual batch");
    let parent_fact = runtime.entity_facts(&parent.name).unwrap().unwrap();
    assert_eq!(parent_fact.server_erev, 0);
    let children: BTreeSet<_> = names
        .iter()
        .filter(|name| {
            matches!(runtime.entity_facts(name).unwrap().unwrap().payload,
        Payload::Note(ref note) if codec::id_key(note.mid).as_slice() == parent_fact.key.as_slice())
        })
        .cloned()
        .collect();
    assert!(!children.is_empty());
    assert!(
        children.iter().any(|name| name > &parent.name),
        "fixture must exercise the erroneous earlier-parent shortcut"
    );
    assert!(batch.saves.iter().all(|s| !children.contains(&s.name)));
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let parent_size = meguri_testkit::cloud::record_non_asset_bytes(
        &zone,
        &RecordInput::encrypted(&parent.name, "MGEntity", parent.envelope.clone()),
    );
    assert!(
        batch
            .saves
            .iter()
            .any(|s| meguri_testkit::cloud::record_non_asset_bytes(
                &zone,
                &RecordInput::encrypted(&s.name, "MGEntity", s.envelope.clone())
            ) < parent_size)
    );
    let mut limited = Config::default();
    limited.limits.non_asset_per_record = parent_size - 1;
    service
        .request(&scope(), Caller::default(), Operation::Configure(limited))
        .unwrap()
        .result
        .unwrap();
    let failed = send_actual_batch(&host, &mut service, &batch);
    assert!(failed.contains(&parent.name));
    assert!(
        failed.len() < batch.saves.len(),
        "actual non-atomic partial success required"
    );
    assert_eq!(runtime.entity_facts(&parent.name).unwrap().unwrap().server_erev, 0);
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert!(next.saves.iter().all(|s| !children.contains(&s.name)));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(children.iter().all(|name| !db.zones[&zone].records.contains_key(name)));
    service
        .request(&scope(), Caller::default(), Operation::Configure(Config::default()))
        .unwrap()
        .result
        .unwrap();
    assert!(send_actual_batch(&host, &mut service, &next).is_empty());
    assert!(runtime.entity_facts(&parent.name).unwrap().unwrap().server_erev > 0);
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert!(next.saves.iter().any(|s| children.contains(&s.name)));
    host.close().unwrap();
}

fn until_active(host: &CoreHost, service: &mut Service<SeededRandom>) -> OwnRequest {
    until_active_observing(host, service, |_, _| {})
}
fn until_active_observing(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    mut observe: impl FnMut(&cloud::runtime::Runtime, &OwnRequest),
) -> OwnRequest {
    until_active_observing_work(host, service, |runtime, work| {
        if let PublicationWork::Own(request) = work {
            observe(runtime, request);
        }
        None
    })
}
fn until_active_observing_work(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    mut observe: impl FnMut(&cloud::runtime::Runtime, &PublicationWork) -> Option<String>,
) -> OwnRequest {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let mut sdk = false;
    for _ in 0..32 {
        let work = runtime.next_publication_work(&collection).unwrap();
        let selected_sdk = observe(runtime, &work);
        match work {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => {
                if request.kind == OwnKind::SaveActive {
                    return *request;
                }
                let (result, _) =
                    execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
                runtime.publication_result(&request, result, &collection).unwrap();
            }
            PublicationWork::Scan => {
                runtime
                    .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
                    .unwrap();
            }
            PublicationWork::SendEntities => {
                let instance = selected_sdk.unwrap_or_else(|| runtime.instance().unwrap());
                if !sdk {
                    runtime.start_sdk_instance(&instance).unwrap();
                    sdk = true;
                }
                let batch = runtime
                    .build_collection_batch(&instance, &collection, &entity_names(runtime))
                    .unwrap();
                assert!(!batch.saves.is_empty());
                assert!(send_actual_batch_with_instance(host, service, &batch, instance).is_empty());
            }
            other => panic!("publication stopped: {other:?}"),
        }
    }
    panic!("no active submission");
}

#[test]
fn driver_refresh_at_sent_restores_automatic_off_after_real_zones_start() {
    publication_driver_automatic_terminal(true, false);
}

#[test]
fn driver_explicit_tracking_error_precedes_reconciled_zones_opening() {
    publication_driver_tracking_failure(false);
}

#[test]
fn driver_explicit_storage_full_precedes_reconciled_zones_opening() {
    publication_driver_tracking_failure(true);
}

fn publication_driver_tracking_failure(no_space: bool) {
    use cloud::policy::{ActionKind, Attention, ChangeCause, DeviceEvent, DeviceSettings, Lifecycle, Scan};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("publication-policy-failure");
    generated(&dir);
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true,
        paused: false,
        automatic: false,
        media: true,
    }))
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())),
        origin_sdk: None,
    })
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true })
        .unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(709)).unwrap();
    assert_eq!(bind_and_write(&host, &mut service, &scope()).step, PublishStep::Bound);
    host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    let driver = host.cloud_policy_driver().unwrap();
    let before = driver.state();
    assert_eq!(
        before.facts.binding.as_ref().unwrap().publish_step,
        Some(cloud::policy::PublishStep::Bound)
    );
    assert_eq!(before.tracking_stop, None);
    assert!(!before.gate_open() && before.engine.is_none());

    // Advance real publication and reconciliation without another policy input.
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let projection = runtime
        .policy_projection(Lifecycle::Ready, collection.generation().0)
        .unwrap();
    assert_eq!(
        projection.facts.binding.as_ref().unwrap().publish_step,
        Some(cloud::policy::PublishStep::Zones)
    );
    assert_eq!(projection.facts.summary.scan, Scan::None);
    assert!(projection.facts.summary.collection_known && projection.facts.summary.sendable > 0);
    assert!(!projection.tracking_stopped && projection.engine.is_none());
    assert_eq!(driver.state(), before);

    let commands = host
        .cloud_policy_input(DeviceEvent::TrackingWriteFailed { no_space })
        .unwrap();
    let state = driver.state();
    assert_eq!(state.facts.binding, projection.facts.binding);
    assert_eq!(state.tracking_stop, Some(no_space));
    assert!(!state.gate_open() && state.engine.is_none());
    assert!(!commands.iter().any(|c| matches!(
        c.action.kind,
        ActionKind::StartEngine { .. }
            | ActionKind::RequestFetch { .. }
            | ActionKind::RequestSend { .. }
            | ActionKind::ResumeMedia
    )));
    for kind in [
        ActionKind::CancelFlushTimer,
        ActionKind::CancelBackgroundFlush,
        ActionKind::PauseMedia,
    ] {
        assert_eq!(commands.iter().filter(|c| c.action.kind == kind).count(), 1);
    }
    let expected = if no_space {
        Attention::LocalStorageFull
    } else {
        Attention::TrackingError
    };
    let other = if no_space {
        Attention::TrackingError
    } else {
        Attention::LocalStorageFull
    };
    let mut statuses = Vec::new();
    for command in &commands {
        driver.validate(command).unwrap();
        if let ActionKind::StatusChanged(status) = &command.action.kind {
            assert_eq!(status.attention.get(&expected), Some(&1));
            assert!(!status.attention.contains_key(&other));
            assert!(!statuses.contains(status));
            statuses.push(status.clone());
        }
    }
    assert_eq!(statuses.last().map(|s| s.as_ref()), Some(&driver.status()));
    // Preserve Runtime's independent refusal even with reconciled sendable rows.
    assert!(matches!(
        runtime.register_sdk(state.automatic()),
        Err(cloud::runtime::RuntimeError::Incomplete(
            cloud::runtime::Incomplete::WholeBindingReconciliation
        ))
    ));
    let failed = runtime
        .policy_projection(Lifecycle::Ready, collection.generation().0)
        .unwrap();
    assert!(failed.tracking_stopped && failed.engine.is_none());
    host.close().unwrap();
    assert!(commands.iter().all(|c| driver.validate(c).is_err()));
}

#[test]
fn original_policy_command_survives_real_complete_save_active_row_removal() {
    publication_driver_automatic_terminal(true, true);
}

#[test]
fn driver_refresh_skipping_sent_invalidates_zones_command_at_terminal() {
    publication_driver_automatic_terminal(false, false);
}

fn publication_driver_automatic_terminal(observe_sent: bool, observe_complete: bool) {
    use cloud::policy::{ActionKind, ChangeCause, DeviceEvent, DeviceSettings, Lifecycle, Update};

    // Own-operation replies do not project Driver by themselves. Cover both
    // Sent -> None and Complete -> None, plus the non-equivalent Zones -> None.
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("publication-policy");
    generated(&dir);
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: true,
        paused: false,
        automatic: false,
        media: false,
    }))
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Account {
        identity: IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes())),
        origin_sdk: None,
    })
    .unwrap();
    host.cloud_policy_input(DeviceEvent::Network { reachable: true })
        .unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(708)).unwrap();
    let preview = actual_preview(&host, &mut service, &scope());
    runtime.confirm_connection_publication(&preview, &collection).unwrap();
    let driver = host.cloud_policy_driver().unwrap();
    let mut sdk = None;
    let mut zones_command = None;
    let mut configure_off = None;
    let mut configure_count = 0;
    let mut original = None;
    let active = until_active_observing_work(&host, &mut service, |runtime, work| {
        if matches!(work, PublicationWork::SendEntities) && sdk.is_none() {
            let commands = host
                .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
                .unwrap();
            assert_eq!(runtime.publication_op().unwrap().unwrap().step, PublishStep::Zones);
            assert_eq!(
                driver.state().facts.binding.as_ref().unwrap().publish_step,
                Some(cloud::policy::PublishStep::Zones)
            );
            let start = commands
                .into_iter()
                .find(|c| matches!(c.action.kind, ActionKind::StartEngine { automatic: true, .. }))
                .expect("Zones starts the actual engine with automatic scheduling on");
            let (registered, _, _) = driver.register_native_sdk(&start).unwrap();
            driver.validate_native_execution(&start, Some(&registered)).unwrap();
            host.cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
                .unwrap();
            let state = driver.state();
            assert!(!state.settings.automatic && state.automatic() && state.engine_runs());
            assert_eq!(state.engine.as_ref().unwrap().instance, registered);
            zones_command = Some(start);
            sdk = Some(registered);
        }
        if let PublicationWork::Own(request) = work
            && ((observe_sent && request.kind == OwnKind::SaveCompleteEpoch)
                || (observe_complete && request.kind == OwnKind::SaveActive))
        {
            let phase = if request.kind == OwnKind::SaveCompleteEpoch {
                cloud::policy::PublishStep::Sent
            } else {
                cloud::policy::PublishStep::Complete
            };
            assert_eq!(
                runtime.publication_op().unwrap().unwrap().step,
                if phase == cloud::policy::PublishStep::Sent {
                    PublishStep::Sent
                } else {
                    PublishStep::Complete
                }
            );
            // At Sent, actual ACKs have already settled every initial row.
            assert!(entity_names(runtime).iter().all(|name| {
                let entity = runtime.entity_facts(name).unwrap().unwrap();
                entity.mark.is_none() && entity.acked_rev == entity.local_rev && entity.acked_rev > 0
            }));
            let commands = host
                .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
                .unwrap();
            assert_eq!(driver.state().facts.binding.as_ref().unwrap().publish_step, Some(phase));
            assert!(!driver.state().settings.automatic && !driver.state().automatic());
            assert!(
                commands
                    .iter()
                    .any(|c| c.action.kind == ActionKind::UpdatePending(Update::Changes)),
                "the original cloud refresh still runs after the Binding input"
            );
            for command in &commands {
                if command.action.kind == (ActionKind::ConfigureAutomatic { on: false }) {
                    configure_count += 1;
                    driver.validate(command).unwrap();
                    driver.validate_native_execution(command, sdk.as_deref()).unwrap();
                    assert_eq!(driver.native_origin(command).unwrap(), sdk);
                    configure_off = Some(command.clone());
                }
            }
            assert_eq!(
                configure_count, 1,
                "Sent emits configure-off before terminal own operations"
            );
            assert_eq!(driver.state().guard(), zones_command.as_ref().unwrap().action.guard);
            let command = commands
                .into_iter()
                .find(|c| matches!(c.action.kind, ActionKind::UpdatePending(_)))
                .expect("original Driver-issued pending update");
            driver.validate(&command).unwrap();
            driver.validate_native_execution(&command, sdk.as_deref()).unwrap();
            original = Some(command);
        }
        sdk.clone()
    });
    let sdk = sdk.expect("Driver-issued Zones engine registered");
    let zones_command = zones_command.unwrap();
    let original = original.unwrap_or_else(|| zones_command.clone());
    let guard = original.action.guard.clone();
    assert_eq!(runtime.publication_op().unwrap().unwrap().step, PublishStep::Complete);
    let backend = runtime.adapter_guard();
    let (result, _) = execute_publication_request(&mut service, &scope(), &active, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&active, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(runtime.sdk_root_confirmed());
    let projection = runtime
        .policy_projection(Lifecycle::Ready, collection.generation().0)
        .unwrap();
    assert_eq!(projection.facts.binding.as_ref().unwrap().publish_step, None);
    assert_eq!(runtime.adapter_guard(), backend);
    let terminal_commands = host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    for command in &terminal_commands {
        if command.action.kind == (ActionKind::ConfigureAutomatic { on: false }) {
            configure_count += 1;
            driver.validate_native_execution(command, Some(&sdk)).unwrap();
            configure_off = Some(command.clone());
        }
    }
    if observe_sent {
        assert_eq!(driver.state().guard(), guard);
        assert_eq!(original.action.guard, guard);
        driver.validate(&original).unwrap();
        driver.validate_native_execution(&original, Some(&sdk)).unwrap();
        host.execute_cloud_policy_core(&original, Duration::from_secs(10))
            .unwrap();
        assert_eq!(driver.state().guard(), guard);
    } else {
        assert_ne!(
            driver.state().guard().token,
            guard.token,
            "Zones -> None is not terminal guard equivalence"
        );
        assert!(driver.validate(&zones_command).is_err());
        assert!(driver.validate_native_execution(&zones_command, Some(&sdk)).is_err());
    }
    assert_eq!(configure_count, 1, "one actual configure-off across ordinary refreshes");
    let configure_off = configure_off.expect("Binding progress restores the device setting");
    driver.validate_native_execution(&configure_off, Some(&sdk)).unwrap();
    driver.retire_native_sdk(&configure_off, &sdk).unwrap();
    runtime.release_sdk(&sdk).unwrap();
    let (replacement, _, _) = driver.register_native_sdk(&configure_off).unwrap();
    assert_ne!(replacement, sdk);
    assert!(runtime.validate_sdk(&sdk).is_err());
    driver
        .validate_native_execution(&configure_off, Some(&replacement))
        .unwrap();
    assert!(
        !runtime.wants_send(),
        "actual registration uses the command's automatic=false"
    );
    let commands = host
        .cloud_policy_input(DeviceEvent::Refresh(ChangeCause::Cloud))
        .unwrap();
    assert!(
        !commands
            .iter()
            .any(|c| matches!(c.action.kind, ActionKind::ConfigureAutomatic { .. }))
    );
    assert!(!driver.state().automatic() && !driver.state().settings.automatic);
    host.close().unwrap();
    assert!(
        driver.validate(&configure_off).is_err(),
        "close still invalidates original work"
    );
}

#[test]
fn held_active_is_not_sent_and_actual_lost_reply_retains_submission_evidence_on_restart() {
    for submit in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
            cloud::OpaqueBytes(scope().account.into_bytes()),
        ))));
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        host.media().unwrap().store().register_full(|_, _| true).unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        runtime.confirm_connection_publication(&p, &collection).unwrap();
        let held = until_active(&host, &mut service);
        let before = runtime.publication_op().unwrap().unwrap();
        assert!(!before.switch_sent && before.active_attempt.is_none());
        let after = service.request_log().unwrap().len();
        if submit {
            let (result, _) = execute_publication_request(&mut service, &scope(), &held, runtime, &collection).unwrap();
            assert!(matches!(result, OwnResult::Saved(_)));
            // Deliberately lose the actual reply, never deliver a fake ack.
            assert!(
                runtime
                    .publication_result(&held, OwnResult::NotStarted, &collection)
                    .is_err()
            );
            let durable = runtime.publication_op().unwrap().unwrap();
            assert!(durable.switch_sent && durable.active_attempt.is_some());
        } else {
            *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(b"changed-account".to_vec()));
            assert!(execute_publication_request(&mut service, &scope(), &held, runtime, &collection).is_err());
            assert_eq!(service.request_log().unwrap().len(), after);
            assert_eq!(runtime.publication_op().unwrap().unwrap(), before);
        }
        host.close().unwrap();
        drop(host);
        let reopened = open_host(&dir, keys());
        let durable = reopened.cloud_runtime().unwrap().publication_op().unwrap().unwrap();
        assert_eq!(durable.switch_sent, submit);
        assert_eq!(durable.active_attempt.is_some(), submit);
        let db = service.inspect(&scope()).unwrap().unwrap();
        assert_eq!(db.zones["meguri-root"].records.contains_key("active"), submit);
        reopened.close().unwrap();
    }
}

fn restore_collection(host: &CoreHost, dir: &Path) {
    use meguri_engine::{LongOp, replace::ReplaceReason};
    let source = tempfile::tempdir_in(dir).unwrap();
    generated(source.path());
    let stage = meguri_engine::replace::ReplacementStage::new(dir).unwrap();
    std::fs::copy(source.path().join("collection.anki2"), stage.path()).unwrap();
    let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
    slot.replace_verified(stage, ReplaceReason::Restore, |_| Ok(()))
        .unwrap();
}

fn actual_preview(host: &CoreHost, service: &mut Service<SeededRandom>, scope: &Scope) -> Preview {
    fn read(service: &mut Service<SeededRandom>, scope: &Scope, zone: String, name: &str) -> RecordRead {
        let response = service
            .request(
                scope,
                Caller::default(),
                Operation::Read {
                    zone,
                    name: name.into(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        match response.result {
            Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
            Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
            Err(error) if error.code == meguri_testkit::cloud::ErrorCode::UnknownItem => RecordRead::Missing,
            other => panic!("unexpected actual preview read: {}", other.is_ok()),
        }
    }
    let runtime = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = runtime
        .begin_connection_preview(&host.collection().unwrap(), &host.media().unwrap())
        .unwrap()
    else {
        panic!("active read required");
    };
    let work = runtime
        .connection_active_read(id, read(service, scope, "meguri-root".into(), "active"))
        .unwrap();
    match work {
        PreviewWork::Ready(p) => *p,
        PreviewWork::ReadEpoch { id, epoch } => {
            let work = runtime
                .connection_epoch_read(
                    id,
                    read(service, scope, cloud::adapter::collection_zone(epoch), "epoch"),
                )
                .unwrap();
            let PreviewWork::Ready(p) = work else {
                panic!("epoch preview required");
            };
            *p
        }
        _ => panic!("unexpected preview work"),
    }
}

fn publish_actual_collection(host: &CoreHost, service: &mut Service<SeededRandom>) -> cloud::Binding {
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let p = actual_preview(host, service, &scope());
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let request = until_active(host, service);
    let (result, _) = execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(runtime.stored_state_for_testing().unwrap().durable_file.is_some());
    runtime.status().unwrap().binding.unwrap()
}

struct HeldResumeAccount {
    identity: Mutex<IdentityStatus>,
    after_sample: Mutex<Option<Box<dyn FnOnce() + Send>>>,
}
impl IdentityProvider for HeldResumeAccount {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        let identity = self.identity.lock().unwrap().clone();
        let hook = self.after_sample.lock().unwrap().take();
        if let Some(hook) = hook {
            hook();
        }
        identity
    }
}

#[test]
fn policy_work_retained_resume_provider_cancellation_preserves_offer_until_invalidation() {
    use cloud::policy::{DeviceEvent, DriverError, PolicyWork};
    use meguri_sync::cloud::runtime::RuntimeError;
    use std::sync::mpsc;

    for after_effect in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("resume-policy-work");
        generated(&dir);
        let available = IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()));
        let account = Arc::new(HeldResumeAccount {
            identity: Mutex::new(available.clone()),
            after_sample: Mutex::new(None),
        });
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(707)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        *account.identity.lock().unwrap() = IdentityStatus::SignedOut;
        runtime.refresh_identity().unwrap();
        *account.identity.lock().unwrap() = available.clone();
        runtime.refresh_identity().unwrap();
        let preview = actual_preview(&host, &mut service, &scope());
        assert_eq!(preview.decision, ConnectDecision::ConfirmResume);
        assert_eq!(runtime.resume_binding_offer(), Some(binding.id));
        let driver = host.cloud_policy_driver().unwrap();
        let before = driver.state();
        let guard = runtime.adapter_guard();
        let cancel = meguri_engine::CancelToken::new();
        // Fail the actual validation read without an account-refresh mutation.
        *account.identity.lock().unwrap() = IdentityStatus::SignedOut;
        let (entered, reached) = mpsc::channel();
        let (release, resume) = mpsc::channel();
        *account.after_sample.lock().unwrap() = Some(Box::new(move || {
            entered.send(()).unwrap();
            resume.recv_timeout(Duration::from_secs(10)).expect("provider release");
        }));
        let (projected, projection_reached) = mpsc::channel();
        let (projection_release, projection_resume) = mpsc::channel();
        if after_effect {
            driver.set_projection_hook_for_testing(move || {
                projected.send(()).unwrap();
                projection_resume
                    .recv_timeout(Duration::from_secs(10))
                    .expect("projection release");
            });
        }
        let result = std::thread::scope(|threads| {
            let worker = threads.spawn(|| {
                driver.handle_with_work(
                    host.engine().state(),
                    collection.generation(),
                    DeviceEvent::Network { reachable: true },
                    &mut PolicyWork::new(&cancel),
                )
            });
            let held = reached.recv_timeout(Duration::from_secs(5));
            if !after_effect {
                cancel.cancel();
            }
            let _ = release.send(());
            let projected = if after_effect {
                let held = projection_reached.recv_timeout(Duration::from_secs(5));
                cancel.cancel();
                let _ = projection_release.send(());
                Some(held)
            } else {
                None
            };
            let result = worker.join().unwrap();
            held.expect("real retained-offer provider validation held");
            if let Some(held) = projected {
                held.expect("invalidation completed before cancellation");
            }
            result
        });
        *account.identity.lock().unwrap() = available;
        if after_effect {
            assert!(result.is_ok(), "a real invalidation must preserve its actual outcome");
            assert_eq!(runtime.resume_binding_offer(), None);
        } else {
            assert!(matches!(
                result,
                Err(DriverError::Runtime(RuntimeError::Engine(
                    meguri_engine::EngineError::Cancelled
                )))
            ));
            assert_eq!(driver.state(), before);
            assert_eq!(runtime.resume_binding_offer(), Some(binding.id));
        }
        assert_eq!(runtime.adapter_guard(), guard);
        assert!(matches!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut)
        ));
        // Verify that a provider error also clears the retained offer through the older helper.
        *account.identity.lock().unwrap() = IdentityStatus::SignedOut;
        assert_eq!(runtime.resume_binding_offer(), None);
        *account.identity.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()));
        assert_eq!(runtime.resume_binding_offer(), None);
        host.close().unwrap();
    }
}

#[derive(Debug)]
struct HeldBindProvider {
    random: SeededRandom,
    remaining: Mutex<Option<usize>>,
    hold_on_clock: bool,
    clock_armed: std::sync::atomic::AtomicBool,
    entered: std::sync::mpsc::Sender<()>,
    release: Mutex<std::sync::mpsc::Receiver<()>>,
}
impl HeldBindProvider {
    fn hold(&self) {
        self.entered.send(()).unwrap();
        self.release
            .lock()
            .unwrap()
            .recv_timeout(Duration::from_secs(30))
            .unwrap();
    }
}
impl RandomSource for HeldBindProvider {
    fn next_u64(&self) -> u64 {
        let value = self.random.next_u64();
        let last = {
            let mut remaining = self.remaining.lock().unwrap();
            if let Some(left) = remaining.as_mut() {
                *left -= 1;
                if *left == 0 {
                    *remaining = None;
                    true
                } else {
                    false
                }
            } else {
                false
            }
        };
        if last {
            if self.hold_on_clock {
                self.clock_armed.store(true, std::sync::atomic::Ordering::SeqCst);
            } else {
                self.hold();
            }
        }
        value
    }
}
impl meguri_model::clock::Clock for HeldBindProvider {
    fn now_ms(&self) -> i64 {
        if self.clock_armed.swap(false, std::sync::atomic::Ordering::SeqCst) {
            self.hold();
        }
        NOW
    }
    fn utc_offset_at(&self, _: i64) -> UtcOffset {
        UtcOffset::UTC
    }
    fn zone_id(&self) -> String {
        "UTC".into()
    }
}

#[test]
fn publication_work_uses_sent_step_when_last_entity_result_lands_during_selection() {
    use std::sync::mpsc;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let media = host.media().unwrap();
    media.store().register_full(|_, _| true).unwrap();
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let p = preview(&host, &mut service);
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &media, Duration::from_secs(30))
        .unwrap();
    let original = runtime.publication_op().unwrap().unwrap();
    assert_eq!(original.step, PublishStep::Zones);
    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let names = entity_names(runtime);

    let (entered, wait) = mpsc::channel();
    let (release, resume) = mpsc::channel();
    let clock = Arc::new(HeldBindProvider {
        random: SeededRandom::new(91),
        remaining: Mutex::new(None),
        hold_on_clock: true,
        clock_armed: std::sync::atomic::AtomicBool::new(true),
        entered,
        release: Mutex::new(resume),
    });
    runtime.attach_adapter_clock(clock.clone());
    let request = std::thread::scope(|scope| {
        let work = scope.spawn(|| runtime.next_publication_work(&collection));
        wait.recv_timeout(Duration::from_secs(5)).unwrap();
        // These are real Service saves. Their committed results advance the
        // original operation while its next-work read has no Runtime lock.
        for _ in 0..32 {
            if runtime.publication_op().unwrap().unwrap().step == PublishStep::Sent {
                break;
            }
            let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
            assert!(!batch.saves.is_empty());
            assert!(send_actual_batch(&host, &mut service, &batch).is_empty());
        }
        release.send(()).unwrap();
        assert_eq!(runtime.publication_op().unwrap().unwrap().step, PublishStep::Sent);
        let PublicationWork::Own(request) = work.join().unwrap().unwrap() else {
            panic!("same publication must offer its complete-epoch save");
        };
        request
    });
    assert_eq!(request.op, original.id);
    assert_eq!(request.kind, OwnKind::SaveCompleteEpoch);
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    runtime.publication_result(&request, result, &collection).unwrap();
    assert_eq!(runtime.publication_op().unwrap().unwrap().step, PublishStep::Complete);
    host.close().unwrap();
}

#[test]
fn publication_bind_held_final_providers_refuse_reopen_and_replacement_without_storing_identity() {
    for hold_on_clock in [false, true] {
        for lifecycle in ["reopen", "replace"] {
            for republish in [false, true] {
                let temp = tempfile::tempdir().unwrap();
                let dir = temp.path().join("held-bind");
                generated(&dir);
                let (entered_tx, entered) = std::sync::mpsc::channel();
                let (release, release_rx) = std::sync::mpsc::channel();
                let provider = Arc::new(HeldBindProvider {
                    random: SeededRandom::new(91),
                    remaining: Mutex::new(None),
                    hold_on_clock,
                    clock_armed: std::sync::atomic::AtomicBool::new(false),
                    entered: entered_tx,
                    release: Mutex::new(release_rx),
                });
                let mut config = options(&dir, NOW, 91);
                config.random = provider.clone();
                config.clock = provider.clone();
                config.cloud = Some(CloudInputs {
                    scope: cloud::BuildScope {
                        container: cloud::ContainerId::new(scope().container).unwrap(),
                        environment: cloud::Environment::Development,
                    },
                    identity: Arc::new(Account),
                    files: Arc::new(NativeFiles),
                });
                config.cloud_key_bytes = Some(keys());
                let host = CoreHost::open(config).unwrap();
                let runtime = host.cloud_runtime().unwrap();
                let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(711)).unwrap();
                let prior = republish.then(|| publish_actual_collection(&host, &mut service));
                if prior.is_some() {
                    let entity = runtime
                        .entity_facts_page(cloud::entity_codec::Kind::Note, None)
                        .unwrap()
                        .remove(0);
                    let cloud::entity_codec::Payload::Note(note) = &entity.payload else {
                        panic!("note")
                    };
                    edit_workflow_note(&host.collection().unwrap(), note, "pending while bind provider is held");
                    runtime
                        .reconcile_binding(
                            &host.collection().unwrap(),
                            &host.media().unwrap(),
                            Duration::from_secs(30),
                        )
                        .unwrap();
                }
                let collection = host.collection().unwrap();
                let preview = actual_preview(&host, &mut service, &scope());
                let id = runtime.confirm_connection_publication(&preview, &collection).unwrap();
                let confirmed = runtime.publication_op().unwrap().unwrap();
                assert_eq!(confirmed.step, PublishStep::Confirmed);
                assert!(confirmed.binding_after.is_none());
                let original = runtime.publication_facts(&collection).unwrap();
                let original_state = prior.as_ref().map(|_| runtime.stored_state_for_testing().unwrap());
                if republish {
                    assert!(
                        original
                            .entities
                            .values()
                            .any(|e| e.mark.is_some() && e.acked_payload.is_some())
                    );
                }
                *provider.remaining.lock().unwrap() = Some(2);
                std::thread::scope(|threads| {
                    let worker = threads.spawn(|| runtime.perform_publication_local(id, LocalWork::Bind, &collection));
                    entered.recv_timeout(Duration::from_secs(30)).unwrap();
                    if lifecycle == "reopen" {
                        assert!(matches!(
                            host.engine().suspend(Duration::from_secs(30)).unwrap(),
                            meguri_engine::suspend::SuspendOutcome::Suspended
                        ));
                        host.engine().resume().unwrap();
                    } else {
                        restore_collection(&host, &dir);
                    }
                    let fresh = host.collection().unwrap();
                    let before = runtime.publication_facts(&fresh).unwrap();
                    let before_state = prior.as_ref().map(|_| runtime.stored_state_for_testing().unwrap());
                    assert_eq!(before.bindings.len(), usize::from(republish));
                    assert!(runtime.publication_op().unwrap().as_ref() == Some(&confirmed));
                    assert!(before.entities == original.entities);
                    if let (Some(original), Some(before)) = (&original_state, &before_state) {
                        assert_eq!(before.file, original.file);
                        assert_eq!(before.durable_file, original.durable_file);
                    }
                    if let Some(binding) = prior.as_ref() {
                        let expected = if lifecycle == "replace" {
                            cloud::BindingState::Isolated(cloud::IsolationReason::ReplacedLocally)
                        } else {
                            binding.facts.state
                        };
                        assert_eq!(before.bindings[0].facts.state, expected);
                    }
                    if lifecycle == "replace" {
                        assert_ne!(
                            before.decision.publication.identity,
                            original.decision.publication.identity
                        );
                        assert_eq!(
                            before.decision.publication.decision,
                            meguri_engine::device_state::PublicationDecision::Pending
                        );
                    } else {
                        assert_eq!(before.decision, original.decision);
                    }
                    release.send(()).unwrap();
                    assert!(
                        worker.join().unwrap().is_err(),
                        "stale bind admitted: clock={hold_on_clock}, {lifecycle}, prior={republish}"
                    );
                    let after = runtime.publication_facts(&fresh).unwrap();
                    // Binding creation and its native/durable pair share a
                    // transaction. Neither may land before a late bind error.
                    assert!(after.bindings == before.bindings);
                    assert!(runtime.publication_op().unwrap().as_ref() == Some(&confirmed));
                    assert!(after.entities == before.entities);
                    assert_eq!(after.decision, before.decision);
                    assert!(after.current == before.current);
                    assert_eq!(
                        prior.as_ref().map(|_| runtime.stored_state_for_testing().unwrap()),
                        before_state
                    );
                });
                host.close().unwrap();
            }
        }
    }
}

#[cfg(target_vendor = "apple")]
#[test]
fn durable_identity_device_only_reopen_preserves_pending_and_ack_then_reconciles_before_send() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("durable-reopen");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(707)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let before = runtime
        .entity_facts_page(cloud::entity_codec::Kind::Note, None)
        .unwrap()
        .remove(0);
    let cloud::entity_codec::Payload::Note(note) = &before.payload else {
        panic!("note")
    };
    let collection = host.collection().unwrap();
    edit_workflow_note(&collection, note, "pending across simulated device drift");
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    let complete = runtime.stored_state_for_testing().unwrap();
    let decision = meguri_engine::device_state::DeviceState::for_collection(collection.path())
        .publication_decision()
        .unwrap();
    host.close().unwrap();
    // Model a prior mount in the saved native authorities only. The durable
    // UUID/inode/birthtime remains the real retained-FD capture, never injected.
    let scope = cloud::BuildScope {
        container: cloud::ContainerId::new(scope().container).unwrap(),
        environment: cloud::Environment::Development,
    };
    cloud::runtime::with_tracking_access(|| {
        let cloud::OpenOutcome::Opened(mut store) = cloud::TrackingStore::open(&dir, scope).unwrap() else {
            panic!("tracking store")
        };
        let coverage = complete.coverage.as_ref().unwrap();
        let mut file = complete.file.unwrap();
        file.device ^= 1;
        let mut cursor = coverage.cursor.clone();
        cursor.native_file = file;
        store
            .prepare_drained_baseline_for_testing(
                binding.id,
                file,
                cursor,
                coverage.media_feed_id,
                coverage.media_cursor,
            )
            .unwrap();
    });
    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    assert_eq!(runtime.status().unwrap().binding.as_ref(), Some(&binding));
    let refreshed = runtime.stored_state_for_testing().unwrap();
    assert_eq!(refreshed.durable_file, complete.durable_file);
    assert_eq!(refreshed.file, complete.file);
    assert!(refreshed.note_scan.is_none() && refreshed.coverage.is_none());
    assert!(!refreshed.whole_scan && !refreshed.drained);
    assert!(runtime.status().unwrap().scan_required);
    assert!(matches!(
        runtime.register_sdk(false),
        Err(cloud::runtime::RuntimeError::Incomplete(
            cloud::runtime::Incomplete::WholeBindingReconciliation
        ))
    ));
    let retained = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(retained.local_rev, pending.local_rev);
    assert_eq!(retained.acked_rev, pending.acked_rev);
    assert_eq!(retained.acked_payload, pending.acked_payload);
    assert_eq!(retained.acked_system_fields, pending.acked_system_fields);
    assert_eq!(retained.in_flight, pending.in_flight);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    assert!(!runtime.status().unwrap().scan_required);
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    runtime.validate_sdk(&sdk).unwrap();
    let reconciled = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(reconciled.local_rev, pending.local_rev);
    assert_eq!(reconciled.acked_payload, pending.acked_payload);
    assert_eq!(reconciled.mark, pending.mark);
    assert_eq!(
        meguri_engine::device_state::DeviceState::for_collection(collection.path())
            .publication_decision()
            .unwrap(),
        decision
    );
    host.close().unwrap();
}

fn save_other_writer_publication_note(
    service: &mut Service<SeededRandom>,
    old: &cloud::Binding,
    before: &cloud::workflow::EntityFacts,
    base: &cloud::entity_codec::Note,
) -> (
    cloud::entity_codec::Payload,
    String,
    meguri_testkit::cloud::Record,
    Saved,
) {
    use cloud::entity_codec::{self as codec, Payload};

    let mut remote = base.clone();
    let mut fields: Vec<_> = remote.flds.split(|b| *b == 31).map(Vec::from).collect();
    assert_eq!(fields.len(), 2);
    fields[1] = b"remote update committed before republication".to_vec();
    remote.flds = fields.join(&31);
    remote.modified += 1;
    let after = Payload::Note(remote);
    let writer = old.id.0.map(|b| b ^ 0x80);
    assert_ne!(writer, old.id.0);
    let envelope = codec::Entity::build(
        codec::EntityFields {
            kind: before.kind,
            key: before.key.clone(),
            origin: before.origin,
            erev: before.server_erev + 1,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer,
            base_sha256: Some(before.acked_payload_digest),
        },
        &after.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    assert_eq!(
        codec::entity_name(&old.facts.record_key.0, before.kind, &before.key).unwrap(),
        before.name
    );
    let old_zone = cloud::adapter::collection_zone(old.facts.epoch);
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: old_zone.clone(),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&before.name, "MGEntity", envelope.clone()),
                    if_unchanged: Some(SystemFields::from_bytes(before.acked_system_fields.clone())),
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(mut items)) = response.result else {
        panic!("actual remote save required");
    };
    assert_eq!(items.len(), 1);
    let Ok(MutationSuccess::Saved(record)) = items.remove(0).result else {
        panic!("remote save must use the acknowledged change tag");
    };
    assert!(matches!(record.fields.get("e"), Some(Field::EncryptedBytes(bytes)) if bytes == &envelope));
    let saved = Saved {
        name: record.name.clone(),
        envelope,
        system_fields: record.system_fields.bytes().to_vec(),
    };
    (after, old_zone, record, saved)
}

// V17.6 / V2.4 step 8, K13.2 and K12.1 rule 8: a committed ordinary
// apply must settle before publication scans or discards its prior binding.
#[test]
fn stopped_binding_committed_incoming_settles_before_republication_and_survives_cleanup() {
    stopped_binding_committed_incoming_transition(false);
}

/// P24 V2.4 step 8 / Q11: a real post-COMMIT apply intent must settle
/// before Resume's scan. This covers ordinary apply, not the media/re-key legs.
#[test]
fn stopped_binding_resume_settles_committed_incoming_before_scan() {
    stopped_binding_committed_incoming_transition(true);
}

fn stopped_binding_committed_incoming_transition(resume: bool) {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_model::faultpoint::{self, Fault};
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
    use std::sync::mpsc::{self, Receiver, Sender};

    static AFTER_COMMIT: Mutex<Option<(Sender<()>, Receiver<()>)>> = Mutex::new(None);

    fn pause_after_commit() {
        let handshake = AFTER_COMMIT
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some((reached, release)) = handshake
            && reached.send(()).is_ok()
        {
            release
                .recv_timeout(Duration::from_secs(30))
                .expect("parent must release the actual post-commit worker");
        }
    }

    struct AfterCommitPause(Sender<()>);
    impl Drop for AfterCommitPause {
        fn drop(&mut self) {
            faultpoint::disarm("cloud.apply.after_commit");
            AFTER_COMMIT
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .take();
            let _ = self.0.send(());
        }
    }

    fn note_row(host: &CoreHost, id: i64, notetype: &codec::Notetype) -> Payload {
        let collection = host.collection().unwrap();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let row = snapshot.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        cloud::row_payload::project(
            &cloud::row_payload::ReadView(&*snapshot),
            codec::Kind::Note,
            &row,
            |mid| {
                assert_eq!(mid, notetype.id);
                Ok(notetype.clone())
            },
        )
        .unwrap()
    }

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("actual publication must acknowledge a note");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());

    // A different writer builds on the actually acknowledged payload and origin,
    // then saves conditionally to the real Service. This is not a seeded ACK.
    let (after, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    let inbox = runtime.inbox_entries(old.id).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, cloud::InboxReason::Ready);
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());

    // Observe the actual COMMIT outside writer/state/tracking locks, then stop
    // the binding before this worker can settle its real intent/result seq.
    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    *AFTER_COMMIT.lock().unwrap() = Some((reached_tx, release_rx));
    let (reached, stopped, applied) = std::thread::scope(|scope| {
        // This local drops before scope joins, including on parent unwind.
        let pause = AfterCommitPause(release_tx);
        faultpoint::arm("cloud.apply.after_commit", Fault::Callback(pause_after_commit));
        let worker_runtime = Arc::clone(runtime);
        let worker_collection = collection.clone();
        let worker = scope.spawn(move || worker_runtime.drain_incoming(&worker_collection, Duration::from_secs(30)));
        let reached = reached_rx.recv_timeout(Duration::from_secs(30));
        let stopped = reached.as_ref().ok().map(|()| runtime.adapter_stop_syncing());
        drop(pause);
        (reached, stopped, worker.join())
    });
    // The callback is disarmed and released before any assertion.
    reached.expect("the actual drain worker must reach after_commit");
    stopped
        .expect("stop must follow the actual after_commit callback")
        .unwrap();
    let applied = applied.expect("the actual drain worker must not panic");
    assert!(
        matches!(applied, Err(cloud::runtime::RuntimeError::StaleIncarnation)),
        "stopping the binding must invalidate the actual post-commit worker"
    );
    assert_eq!(note_row(&host, base.id, &notetype), after);
    let pending = runtime.apply_intents(old.id).unwrap();
    assert_eq!(pending.len(), 1);
    let intent = &pending[0];
    assert_eq!(intent.binding, old.id);
    assert_eq!(intent.inbox_ids, vec![inbox[0].id]);
    assert_eq!(intent.keys.len(), 1);
    assert_eq!(intent.keys[0].name, before.name);
    assert_eq!(intent.keys[0].role, cloud::incoming::ApplyKeyRole::Entry);
    assert_eq!(intent.keys[0].before_digest, Some(before.acked_row_digest));
    assert_eq!(intent.keys[0].after_digest, Some(after.row_digest().unwrap()));
    assert_ne!(intent.keys[0].before_digest, intent.keys[0].after_digest);
    let runs = runtime.incoming_runs();
    assert_eq!(runs.len(), 1);
    let run = &runs[0];
    assert_eq!(run.intent_id, intent.id);
    assert_eq!(intent.result_seq, Some(run.seq));
    assert_eq!(intent.native_file, run.native_file);
    assert_eq!(intent.generation, run.generation);
    assert_eq!(run.entry_ids, intent.inbox_ids);
    assert_eq!(run.changed_keys, BTreeSet::from([before.name.clone()]));
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);

    let stopped = runtime.status().unwrap().binding.unwrap();
    assert_eq!(stopped.id, old.id);
    assert_eq!(
        stopped.facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::Stopped)
    );
    assert_eq!(runtime.apply_intents(old.id).unwrap(), pending);
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.native_file, Some(intent.native_file));
    if resume {
        assert_eq!(p.decision, ConnectDecision::ConfirmResume);
        runtime.resume_binding(old.id, &collection).unwrap();
        assert_eq!(runtime.status().unwrap().binding.unwrap(), old);
        assert!(runtime.apply_intents(old.id).unwrap().is_empty());
        assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
        let settled = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
        assert_eq!(settled.acked_payload, Some(after.clone()));
        assert_eq!(settled.acked_system_fields, saved.system_fields);
        assert_eq!(runtime.incoming_runs(), runs, "settle must not repeat the op");
        assert!(
            runtime.incoming_wal_receipts().iter().any(|receipt| {
                receipt.identity().generation().0 == run.generation && receipt.through().0 >= run.seq
            })
        );
        assert_eq!(note_row(&host, base.id, &notetype), after);
        assert!(!runtime.status().unwrap().scan_required);
        host.close().unwrap();
        return;
    }
    let calls = service.request_log().unwrap().len();
    let op_id = runtime.confirm_connection_publication(&p, &collection).unwrap();

    assert_eq!(runtime.status().unwrap().binding.unwrap(), stopped);
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.step, PublishStep::Confirmed);
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(op.binding_before.as_ref().unwrap().state, stopped.facts.state);
    assert!(op.binding_after.is_none());
    assert_eq!(service.request_log().unwrap().len(), calls);
    let after_confirm_intents = runtime.apply_intents(old.id).unwrap();
    if !after_confirm_intents.is_empty() {
        assert_eq!(after_confirm_intents, pending);
    }
    let after_confirm_inbox = runtime.inbox_entries(old.id).unwrap();
    if !after_confirm_inbox.is_empty() {
        assert_eq!(after_confirm_inbox, inbox);
    }

    // Run real binding, decision and control work, then the new binding's scan.
    own_until_scan(&host, &mut service);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();

    assert!(
        runtime.apply_intents(old.id).unwrap().is_empty(),
        "pending old intent not settled before the new publication scan"
    );
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.incoming_wal_receipts().iter().any(|receipt| {
        receipt.identity().native_file() == (run.native_file.device, run.native_file.inode)
            && receipt.identity().generation().0 == run.generation
            && receipt.through().0 >= run.seq
    }));
    let settled = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert_eq!(settled.server_erev, before.server_erev + 1);
    assert_eq!(settled.acked_payload, Some(after.clone()));
    assert_eq!(settled.acked_row_digest, after.row_digest().unwrap());
    assert_eq!(settled.acked_payload_digest, after.payload_digest().unwrap());
    assert_eq!(settled.acked_system_fields, saved.system_fields);
    assert_eq!(settled.acked_envelope.0, saved.envelope);
    assert!(settled.mark.is_none() && settled.base_payload.is_none());
    assert_eq!(note_row(&host, base.id, &notetype), after);
    assert_eq!(runtime.incoming_runs(), runs, "unexpected apply count");

    // Continue with actual SDK batch saves and own-operation acknowledgements.
    // With no conflict copies the prior binding must be deletable without an FK
    // error, and no send may escape to its isolated epoch.
    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert!(
        runtime
            .publication_facts(&collection)
            .unwrap()
            .bindings
            .iter()
            .all(|b| b.id != old.id)
    );
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(after.clone()));
    assert!(new_note.mark.is_none() && new_note.server_erev > 0);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(db.zones[&old_zone].records[&before.name] == record);
    assert_eq!(note_row(&host, base.id, &notetype), after);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .iter()
            .all(|b| b.id != old.id)
    );
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(after.clone()));
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert_eq!(note_row(&reopened, base.id, &notetype), after);
    reopened.close().unwrap();
}

// V15.2 step 1 excludes fetched, unapplied changes; V17.6 settles existing intents.
#[test]
fn queued_ready_incoming_is_not_applied_by_republication_and_is_discarded_only_at_completion() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

    fn note_row(host: &CoreHost, id: i64, notetype: &codec::Notetype) -> Payload {
        let collection = host.collection().unwrap();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let row = snapshot.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        cloud::row_payload::project(
            &cloud::row_payload::ReadView(&*snapshot),
            codec::Kind::Note,
            &row,
            |mid| {
                assert_eq!(mid, notetype.id);
                Ok(notetype.clone())
            },
        )
        .unwrap()
    }

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("acknowledged note required");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|e| match e.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.facts.state, cloud::BindingState::Active);
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert!(!before.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());

    let (after, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    assert_ne!(after, Payload::Note(base.clone()));
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    let inbox = runtime.inbox_entries(old.id).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, cloud::InboxReason::Ready);
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);

    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.active_epoch, Some(old.facts.epoch));
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert_eq!(
        runtime
            .inbox_entries(old.id)
            .unwrap()
            .iter()
            .filter(|entry| entry.entry.reason == cloud::InboxReason::Ready)
            .count(),
        1
    );
    let calls = service.request_log().unwrap().len();
    let op_id = runtime.confirm_connection_publication(&p, &collection).unwrap();
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.id, op_id);
    assert_eq!(op.step, PublishStep::Confirmed);
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(service.request_log().unwrap().len(), calls);

    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    assert!(runtime.publication_op().unwrap().is_some());
    assert_eq!(runtime.inbox_entries(old.id).unwrap(), inbox);
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().unwrap() == before);
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert!(
        runtime
            .publication_facts(&collection)
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.id != old.id)
    );
    assert!(matches!(
        runtime.inbox_entries(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.apply_intents(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.binding_entity(old.id, &before.name),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|entity| entity.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(Payload::Note(base.clone())));
    assert!(new_note.mark.is_none() && new_note.acked_rev > 0 && new_note.server_erev > 0);
    assert!(!new_note.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    let db = service.inspect(&scope()).unwrap().unwrap();
    assert!(db.zones[&old_zone].records[&before.name] == record);
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .iter()
            .all(|binding| binding.id != old.id)
    );
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(Payload::Note(base.clone())));
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert!(matches!(
        runtime.inbox_entries(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(matches!(
        runtime.apply_intents(old.id),
        Err(cloud::runtime::RuntimeError::Tracking(cloud::Error::BindingNotFound))
    ));
    assert!(runtime.inbox_entries(new.id).unwrap().is_empty());
    assert!(runtime.apply_intents(new.id).unwrap().is_empty());
    assert!(runtime.incoming_runs().is_empty());
    assert_eq!(note_row(&reopened, base.id, &notetype), Payload::Note(base));
    reopened.close().unwrap();
}

fn bind_and_write(host: &CoreHost, service: &mut Service<SeededRandom>, scope: &Scope) -> PublishOp {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let p = actual_preview(host, service, scope);
    let id = runtime.confirm_connection_publication(&p, &collection).unwrap();
    runtime
        .perform_publication_local(id, LocalWork::Bind, &collection)
        .unwrap();
    runtime
        .perform_publication_local(id, LocalWork::WriteDecision, &collection)
        .unwrap();
    runtime.publication_op().unwrap().unwrap()
}

fn cancel_with_account_status(status: IdentityStatus) {
    use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
        cloud::OpaqueBytes(scope().account.into_bytes()),
    ))));
    let host = open_host_with(&dir, keys(), account.clone(), scope());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    restore_collection(&host, &dir);
    let before = decision(&host);
    assert_eq!(before.publication.decision, PublicationDecision::Pending);
    assert_eq!(before.pending_reason, Some(PendingDecisionReason::Restore));
    let op = bind_and_write(&host, &mut service, &scope());
    assert_eq!(op.binding_before.as_ref().unwrap().id, old.id);
    assert_eq!(
        op.binding_before.as_ref().unwrap().state,
        cloud::BindingState::Isolated(cloud::IsolationReason::ReplacedLocally)
    );
    assert_ne!(decision(&host), before);
    *account.0.lock().unwrap() = status;
    let runtime = host.cloud_runtime().unwrap();
    runtime.refresh_identity().unwrap();
    let calls = service.request_log().unwrap().len();
    runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
    assert_eq!(decision(&host), before);
    assert!(runtime.publication_op().unwrap().is_none());
    let returned = runtime.status().unwrap().binding.unwrap();
    assert_eq!(returned.id, old.id);
    assert_eq!(
        returned.facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::ReplacedLocally)
    );
    assert!(runtime.start_sdk_instance(&runtime.instance().unwrap()).is_err());
    assert_eq!(service.request_log().unwrap().len(), calls, "Cancel needs no network");
    host.close().unwrap();
}

#[test]
fn cancellation_restores_exact_prior_decision_when_account_is_unavailable_or_signed_out() {
    for status in [
        IdentityStatus::ReadFailed,
        IdentityStatus::TemporarilyUnavailable,
        IdentityStatus::SignedOut,
    ] {
        cancel_with_account_status(status);
    }
}

#[test]
fn cancellation_restores_exact_prior_decision_after_account_changes() {
    cancel_with_account_status(IdentityStatus::Available(cloud::OpaqueBytes(
        b"changed-account".to_vec(),
    )));
}

#[test]
fn cancellation_after_native_file_mismatch_restores_fence_and_keeps_old_binding_isolated() {
    use meguri_engine::{EngineError, LongOp};
    for restore in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let old = publish_actual_collection(&host, &mut service);
        if restore {
            restore_collection(&host, &dir);
        }
        let before = decision(&host);
        let op = bind_and_write(&host, &mut service, &scope());
        let written = decision(&host);
        let native_before = host.collection().unwrap().open_evidence().unwrap().native_file;
        assert!(host.review().active_session().is_none());
        {
            let slot = host.engine().acquire_slot(LongOp::Restore).unwrap();
            let mut exclusive = slot.enter_exclusive(true).unwrap();
            // ReplaceTask closes/checkpoints the writer and retires its journal
            // before this closure copies the MAIN file to a different inode.
            exclusive
                .replace(|live| {
                    let copy = live.with_file_name("replacement-native-file");
                    std::fs::copy(live, &copy).map_err(|e| EngineError::Storage(e.into()))?;
                    std::fs::rename(copy, live).map_err(|e| EngineError::Storage(e.into()))
                })
                .unwrap();
        }
        assert_eq!(decision(&host), written);
        let native_after = host.collection().unwrap().open_evidence().unwrap().native_file;
        assert_ne!(native_after, native_before);
        assert!(host.review().active_session().is_none());
        host.close().unwrap();
        drop(host);
        let host = open_host(&dir, keys());
        assert_eq!(decision(&host), written);
        assert_eq!(
            host.collection().unwrap().open_evidence().unwrap().native_file,
            native_after
        );
        let runtime = host.cloud_runtime().unwrap();
        assert_eq!(Some(runtime.status().unwrap().binding.unwrap().id), op.binding_after);
        assert_eq!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::FileMismatch)
        );
        let calls = service.request_log().unwrap().len();
        runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), before);
        assert!(runtime.publication_op().unwrap().is_none());
        let returned = runtime.status().unwrap().binding.unwrap();
        assert_eq!(returned.id, old.id);
        assert_eq!(
            returned.facts.state,
            cloud::BindingState::Isolated(if restore {
                cloud::IsolationReason::ReplacedLocally
            } else {
                cloud::IsolationReason::FileMismatch
            })
        );
        assert!(runtime.start_sdk_instance(&runtime.instance().unwrap()).is_err());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
    }
}

#[test]
fn cancellation_leaves_intervening_restore_or_other_build_decision_and_row_unchanged() {
    for other_build in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        restore_collection(&host, &dir);
        let op = bind_and_write(&host, &mut service, &scope());
        let mut other_scope = scope();
        other_scope.container.push_str(".other");
        let other_op = if other_build {
            host.close().unwrap();
            let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope.clone());
            let row = bind_and_write(&other, &mut service, &other_scope);
            other.close().unwrap();
            Some(row)
        } else {
            restore_collection(&host, &dir);
            host.close().unwrap();
            None
        };
        drop(host);
        let host = open_host(&dir, keys());
        let intervening = decision(&host);
        let calls = service.request_log().unwrap().len();
        let runtime = host.cloud_runtime().unwrap();
        runtime.cancel_publication(op.id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), intervening);
        assert!(runtime.publication_op().unwrap().is_none());
        assert!(runtime.status().unwrap().binding.is_none());
        assert!(runtime.instance().is_err());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
        drop(host);
        if let Some(row) = other_op {
            let other = open_host_with(&dir, keys(), Arc::new(Account), other_scope);
            assert_eq!(other.cloud_runtime().unwrap().publication_op().unwrap(), Some(row));
            assert_eq!(decision(&other), intervening);
            other.close().unwrap();
        }
    }
}

#[test]
fn decision_cas_io_error_keeps_inprocess_reopen_binding_consistent_for_publish_and_cancel() {
    use meguri_model::faultpoint::{self, Fault};
    for cancel in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("device");
        generated(&dir);
        let host = open_host(&dir, keys());
        restore_collection(&host, &dir);
        let before = decision(&host);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
        let p = preview(&host, &mut service);
        let id = runtime.confirm_connection_publication(&p, &collection).unwrap();
        runtime
            .perform_publication_local(id, LocalWork::Bind, &collection)
            .unwrap();
        if cancel {
            runtime
                .perform_publication_local(id, LocalWork::WriteDecision, &collection)
                .unwrap();
        }
        let point = if cancel {
            "cloud.publish.after_cancel_fence"
        } else {
            "cloud.publish.after_fence_clear"
        };
        // Focused returned-error probe, distinct from F3's conformance modes:
        // the operation fails after CAS while this engine stays alive.
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        let result = if cancel {
            runtime.cancel_publication(id, &collection)
        } else {
            runtime.perform_publication_local(id, LocalWork::WriteDecision, &collection)
        };
        faultpoint::disarm(point);
        assert!(result.is_err(), "the post-CAS operation fault must fire");
        let written = decision(&host);
        if cancel {
            assert_eq!(written, before);
        } else {
            assert_eq!(written.publication.binding, account_binding(&scope().account));
        }
        assert!(matches!(
            host.engine().suspend(Duration::from_secs(30)).unwrap(),
            meguri_engine::suspend::SuspendOutcome::Suspended
        ));
        host.engine().resume().unwrap();
        assert_eq!(decision(&host), written);
        let calls = service.request_log().unwrap().len();
        runtime.cancel_publication(id, &host.collection().unwrap()).unwrap();
        assert_eq!(decision(&host), before);
        assert!(runtime.publication_op().unwrap().is_none());
        assert_eq!(service.request_log().unwrap().len(), calls);
        host.close().unwrap();
    }
}

#[test]
fn cancel_fence_kill_restart_finishes_cancel_without_account_and_preserves_prior_restore_reason() {
    use meguri_engine::device_state::{PendingDecisionReason, PublicationDecision};
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    crash_worker(&dir, "cancel", "cloud.publish.after_cancel_fence");
    let host = open_host_with(
        &dir,
        keys(),
        Arc::new(MutableAccount(Mutex::new(IdentityStatus::ReadFailed))),
        scope(),
    );
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let op = runtime.publication_op().unwrap().unwrap();
    assert_eq!(op.step, PublishStep::Cancelling);
    assert_eq!(decision(&host), op.fence_before);
    assert_eq!(decision(&host).publication.decision, PublicationDecision::Pending);
    assert_eq!(decision(&host).pending_reason, Some(PendingDecisionReason::Restore));
    assert!(matches!(runtime.next_publication_work(&collection).unwrap(),
        PublicationWork::Local { op: id, work: LocalWork::Cancel } if id == op.id));
    runtime
        .perform_publication_local(op.id, LocalWork::Cancel, &collection)
        .unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(decision(&host), op.fence_before);
    host.close().unwrap();
}

// V17.6 / V15.2 and K13.2 / K15.2 / K16: retain a real conflict's owner,
// discard its work, and preserve both the chosen row and the losing copy.
#[test]
fn actual_field_conflict_retains_isolated_owner_once_after_republication_and_clean_reopen() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_model::ids::NoteId;
    use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};

    fn note_row(host: &CoreHost, id: i64, notetype: &codec::Notetype) -> Payload {
        let collection = host.collection().unwrap();
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        let row = snapshot.row_image(Table::Notes, &RowKey::Id(id)).unwrap().unwrap();
        cloud::row_payload::project(
            &cloud::row_payload::ReadView(&*snapshot),
            codec::Kind::Note,
            &row,
            |mid| {
                assert_eq!(mid, notetype.id);
                Ok(notetype.clone())
            },
        )
        .unwrap()
    }

    // Call only after closing and dropping this test folder's sole CoreHost
    // and all collection handles. Keep open, reads and drop inside the gate;
    // no Runtime call or tracking mutation belongs in this closure.
    fn closed_store<T>(dir: &Path, read: impl FnOnce(&cloud::TrackingStore) -> T) -> T {
        cloud::runtime::with_tracking_access(|| {
            let scope = cloud::BuildScope {
                container: cloud::ContainerId::new(scope().container).unwrap(),
                environment: cloud::Environment::Development,
            };
            let cloud::OpenOutcome::Opened(store) = cloud::TrackingStore::open(dir, scope).unwrap() else {
                panic!("the actual host's tracking store must exist");
            };
            read(&store)
        })
    }

    fn retained_copies_without_work(dir: &Path, owner: &cloud::Binding) -> Vec<cloud::StoredConflict> {
        closed_store(dir, |store| {
            assert_eq!(store.binding(owner.id).unwrap().as_ref(), Some(owner));
            assert!(store.engine_state(owner.id).unwrap().is_none());
            assert!(store.stored_runtime_state(owner.id).unwrap().is_none());
            assert!(store.inbox(owner.id).unwrap().is_empty());
            assert!(store.apply_intents(owner.id).unwrap().is_empty());
            assert!(store.refused_entries(owner.id).unwrap().is_empty());
            assert!(store.retained_zones(owner.id).unwrap().is_empty());
            for kind in cloud::row_payload::KINDS {
                assert!(store.entity_page(owner.id, kind, None).unwrap().is_empty());
            }
            store.conflict_copies().unwrap()
        })
    }

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("device");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(44)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("actually acknowledged baseline required");
    };
    let notetype = runtime
        .entity_facts_page(codec::Kind::Notetype, None)
        .unwrap()
        .into_iter()
        .find_map(|entity| match entity.acked_payload {
            Some(Payload::Notetype(n)) if n.id == base.mid => Some(n),
            _ => None,
        })
        .unwrap();
    assert_eq!(old.facts.state, cloud::BindingState::Active);
    assert!(before.mark.is_none() && before.acked_rev > 0 && before.server_erev > 0);
    assert!(!before.acked_system_fields.is_empty());
    assert_eq!(
        before.acked_row_digest,
        Payload::Note(base.clone()).row_digest().unwrap()
    );
    assert_eq!(note_row(&host, base.id, &notetype), Payload::Note(base.clone()));

    // A normal validated native edit, with its real observer/base and op seq.
    let deck = {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        lease
            .cards_of_note(NoteId(base.id))
            .unwrap()
            .into_iter()
            .min_by_key(|card| card.id)
            .unwrap()
            .deck_id
            .0
    };
    let mut fields: Vec<String> = std::str::from_utf8(&base.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = "native local field retained through republication".into();
    let edit = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: NoteId(base.id),
            fields,
            tags: std::str::from_utf8(&base.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
    let chosen = note_row(&host, base.id, &notetype);
    let pending = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert_eq!(pending.observed_seq, edit.seq.0);
    assert_eq!(pending.base_payload, Some(Payload::Note(base.clone())));
    assert_eq!(pending.acked_payload, before.acked_payload);
    assert_eq!(pending.payload, chosen);

    let (remote, old_zone, record, saved) = save_other_writer_publication_note(&mut service, &old, &before, &base);
    let (Payload::Note(local_note), Payload::Note(remote_note)) = (&chosen, &remote) else {
        panic!("two actual note versions required");
    };
    // K15.1/K15.2: both changed field 1 differently; the later native edit
    // wins. Other parts are unchanged, so the complete expected row is L.
    assert_ne!(local_note.flds, base.flds);
    assert_ne!(remote_note.flds, base.flds);
    assert_ne!(local_note.flds, remote_note.flds);
    assert!(local_note.modified > remote_note.modified);
    let codec::DecodeOutcome::Supported(remote_envelope) = codec::Entity::decode(&saved.envelope).unwrap() else {
        panic!("actual saved envelope required");
    };
    assert_ne!(remote_envelope.fields.writer, old.id.0);
    assert_eq!(remote_envelope.fields.origin, before.origin);
    assert_eq!(remote_envelope.fields.base_sha256, Some(before.acked_payload_digest));
    runtime
        .receive_collection_records(&instance, std::slice::from_ref(&saved))
        .unwrap();
    assert_eq!(runtime.inbox_entries(old.id).unwrap().len(), 1);
    assert_eq!(runtime.drain_incoming(&collection, Duration::from_secs(30)).unwrap(), 1);
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    let merged = runtime.binding_entity(old.id, &before.name).unwrap().unwrap();
    assert_eq!(merged.payload, chosen);
    assert_eq!(merged.acked_payload, Some(remote.clone()));
    assert_eq!(merged.acked_envelope.0, saved.envelope);
    assert_eq!(merged.acked_system_fields, saved.system_fields);
    assert_eq!(merged.server_erev, before.server_erev + 1);
    assert!(merged.mark.is_some());
    let old_zone_state = service.inspect(&scope()).unwrap().unwrap().zones[&old_zone].clone();
    assert!(old_zone_state.records[&before.name] == record);
    let calls = service.request_log().unwrap().len();

    // Clean close provides exclusive inspection; this is no crash/power-loss proof.
    host.close().unwrap();
    drop(collection);
    drop(host);
    let copies = closed_store(&dir, |store| {
        assert_eq!(store.binding(old.id).unwrap(), Some(old.clone()));
        store.conflict_copies().unwrap()
    });
    assert_eq!(copies.len(), 1);
    let retained = &copies[0];
    assert_eq!(retained.binding, old.id);
    assert_eq!(retained.copy.kind, cloud::ConflictKind::Note);
    assert_eq!(retained.copy.key.0, before.key);
    assert_eq!(retained.copy.reason, cloud::ConflictReason::FieldConflict);
    assert_eq!(retained.copy.losing_envelope.0, saved.envelope);
    assert_eq!(retained.copy.losing_payload_digest.0, remote.payload_digest().unwrap());
    assert_eq!(retained.copy.winning_payload_digest.0, chosen.payload_digest().unwrap());
    let names = |bytes: &[u8]| String::from_utf8(bytes.to_vec()).unwrap();
    assert_eq!(
        retained.copy.field_names,
        Some(notetype.fields.iter().map(|f| names(&f.name)).collect())
    );
    assert_eq!(
        retained.copy.template_names,
        Some(notetype.templates.iter().map(|t| names(&t.name)).collect())
    );
    assert_eq!(retained.copy.note_deck, Some(deck));
    assert!(retained.copy.resolved_at_ms.is_none() && retained.copy.staged_path.is_none());

    let host = open_host(&dir, keys.clone());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), old);
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.active_epoch, Some(old.facts.epoch));
    runtime.confirm_connection_publication(&p, &collection).unwrap();
    let request = until_active(&host, &mut service);
    let new = runtime.status().unwrap().binding.unwrap();
    assert_ne!(new.id, old.id);
    let owner = runtime
        .publication_facts(&collection)
        .unwrap()
        .bindings
        .into_iter()
        .find(|b| b.id == old.id)
        .unwrap();
    let mut isolated = old.clone();
    isolated.facts.state = cloud::BindingState::Isolated(cloud::IsolationReason::Republished);
    assert_eq!(owner, isolated);
    let (result, _) = execute_publication_request(&mut service, &scope(), &request, runtime, &collection).unwrap();
    assert!(matches!(&result, OwnResult::Saved(_)));
    runtime.publication_result(&request, result, &collection).unwrap();
    assert!(runtime.publication_op().unwrap().is_none());
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert!(
        runtime
            .publication_facts(&collection)
            .unwrap()
            .bindings
            .contains(&isolated)
    );
    // Retained owner means successful empty reads, never BindingNotFound.
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().is_none());
    let new_note = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_eq!(new_note.acked_payload, Some(chosen.clone()));
    assert_eq!(new_note.acked_row_digest, chosen.row_digest().unwrap());
    assert!(new_note.mark.is_none() && new_note.acked_rev > 0 && new_note.server_erev > 0);
    assert!(!new_note.acked_system_fields.is_empty());
    assert_eq!(note_row(&host, base.id, &notetype), chosen);
    host.close().unwrap();
    drop(collection);
    drop(host);
    assert_eq!(retained_copies_without_work(&dir, &isolated), copies);

    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    assert!(
        runtime
            .publication_facts(&reopened.collection().unwrap())
            .unwrap()
            .bindings
            .contains(&isolated)
    );
    assert!(runtime.inbox_entries(old.id).unwrap().is_empty());
    assert!(runtime.apply_intents(old.id).unwrap().is_empty());
    assert!(runtime.refused_entries(old.id).unwrap().is_empty());
    assert!(runtime.binding_entity(old.id, &before.name).unwrap().is_none());
    let reopened_note = runtime.binding_entity(new.id, &new_note.name).unwrap().unwrap();
    assert_eq!(reopened_note.acked_payload, Some(chosen.clone()));
    assert_eq!(reopened_note.acked_envelope, new_note.acked_envelope);
    assert_eq!(reopened_note.acked_system_fields, new_note.acked_system_fields);
    assert_eq!(reopened_note.server_erev, new_note.server_erev);
    assert!(reopened_note.mark.is_none());
    assert_eq!(note_row(&reopened, base.id, &notetype), chosen);
    reopened.close().unwrap();
    drop(reopened);
    // Full equality includes CopyId, envelope, names, timestamps and owner.
    assert_eq!(retained_copies_without_work(&dir, &isolated), copies);
    assert!(service.inspect(&scope()).unwrap().unwrap().zones[&old_zone] == old_zone_state);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.zone.as_deref() != Some(old_zone.as_str())
            || !matches!(
                r.kind,
                meguri_testkit::cloud::RequestKind::Modify | meguri_testkit::cloud::RequestKind::DeleteZone
            )
    }));
}

#[test]
fn partial_cloud_ack_keeps_a_multi_note_step_pending_until_every_named_key_is_saved() {
    use meguri_engine::{DisabledReason, OpSource, OpSpec, UndoScope, UndoStatus};
    use meguri_model::ids::NoteId;
    use meguri_storage::ReadRows;
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("partial-ack");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(614)).unwrap();
    publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let c = host.collection().unwrap();
    let notes = runtime
        .entity_facts_page(cloud::entity_codec::Kind::Note, None)
        .unwrap();
    assert_eq!(notes.len(), 2);
    let ids: Vec<_> = notes
        .iter()
        .map(|e| NoteId(i64::from_be_bytes(e.key.as_slice().try_into().unwrap())))
        .collect();
    c.run(
        OpSpec::named("cloud_multi_note_edit", OpSource::UserUndoable),
        move |ctx| {
            for (at, id) in ids.into_iter().enumerate() {
                let mut note = ctx.scope().note(id)?.unwrap();
                note.data = "x".repeat(if at == 0 { 16 } else { 8192 });
                ctx.scope().put(&note)?;
            }
            Ok(())
        },
    )
    .unwrap();
    let instance = runtime.instance().unwrap();
    let names = notes.iter().map(|e| e.name.clone()).collect();
    let batch = runtime.build_collection_batch(&instance, &c, &names).unwrap();
    assert_eq!(batch.saves.len(), 2);
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    let binding = runtime.status().unwrap().binding.unwrap();
    let zone = cloud::adapter::collection_zone(binding.facts.epoch);
    let largest = batch
        .saves
        .iter()
        .map(|save| {
            meguri_testkit::cloud::record_non_asset_bytes(
                &zone,
                &RecordInput::encrypted(&save.name, "MGEntity", save.envelope.clone()),
            )
        })
        .max()
        .unwrap();
    let mut limited = Config::default();
    limited.limits.non_asset_per_record = largest - 1;
    service
        .request(&scope(), Caller::default(), Operation::Configure(limited))
        .unwrap()
        .result
        .unwrap();
    let failed = send_actual_batch(&host, &mut service, &batch);
    assert_eq!(failed.len(), 1);
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    let clean: BTreeSet<_> = names.difference(&failed).cloned().collect();
    assert!(
        runtime
            .build_collection_batch(&instance, &c, &clean)
            .unwrap()
            .saves
            .is_empty()
    );
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    service
        .request(&scope(), Caller::default(), Operation::Configure(Config::default()))
        .unwrap()
        .result
        .unwrap();
    let retry = runtime.build_collection_batch(&instance, &c, &failed).unwrap();
    assert_eq!(retry.saves.len(), 1);
    assert!(send_actual_batch(&host, &mut service, &retry).is_empty());
    assert!(matches!(
        c.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded)
    ));
    host.close().unwrap();
}

fn republish_pending_native_note(stop_at_capture: bool) {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_engine::{DisabledReason, EntryId, OpSource, UndoScope, UndoStatus};
    use meguri_model::ids::NoteId;
    use meguri_storage::{ReadRows, ReaderRole};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("pending-republication");
    generated(&dir);
    let keys = keys();
    let host = open_host(&dir, keys.clone());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(615)).unwrap();
    let old = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let evidence = collection.open_evidence().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Some(Payload::Note(base)) = before.acked_payload.clone() else {
        panic!("actually published note required");
    };
    assert!(before.mark.is_none() && before.server_erev > 0);
    let old_zone = cloud::adapter::collection_zone(old.facts.epoch);
    let old_record = service.inspect(&scope()).unwrap().unwrap().zones[&old_zone].records[&before.name].clone();
    let mut fields: Vec<String> = std::str::from_utf8(&base.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = "native edit awaiting publication under its next binding".into();
    let expected_fields = fields.join("\u{1f}");
    let edit = meguri_engine::notes::update(
        &collection,
        meguri_engine::notes::UpdateNote {
            id: NoteId(base.id),
            fields,
            tags: std::str::from_utf8(&base.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
    let UndoStatus::Available(step) = collection.peek_undo(UndoScope::Global).unwrap() else {
        panic!("the real native edit must start undoable");
    };
    assert_eq!(step.id, EntryId(edit.seq.0));
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 1);
    {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(lease.note(NoteId(base.id)).unwrap().unwrap().fields, expected_fields);
    }
    let edited = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(edited.observed_seq, step.id.0);
    assert!(edited.mark.is_some());
    let Payload::Note(note) = &edited.payload else {
        panic!("native note payload required");
    };
    assert_eq!(note.id, base.id);
    assert_eq!(note.flds, expected_fields.as_bytes());
    assert_ne!(edited.payload, Payload::Note(base.clone()));

    let instance = runtime.instance().unwrap();
    let names = BTreeSet::from([before.name.clone()]);
    let at_gap = Arc::new(Mutex::new(None));
    if stop_at_capture {
        let observed = at_gap.clone();
        let captured_collection = collection.clone();
        runtime.set_cloud_capture_hook_for_test(move |runtime| {
            let pending = captured_collection.peek_undo(UndoScope::Global);
            let stopped = runtime.adapter_stop_syncing();
            *observed.lock().unwrap() = Some((pending, stopped));
        });
    }
    let calls = service.request_log().unwrap().len();
    let built = runtime.build_collection_batch(&instance, &collection, &names);
    if stop_at_capture {
        let (pending, stopped) = at_gap.lock().unwrap().take().expect("capture gap must be reached");
        assert_eq!(
            pending.unwrap(),
            UndoStatus::Unavailable(DisabledReason::UploadPending),
            "Engine must have flagged the native step before Stop"
        );
        stopped.unwrap();
        assert!(matches!(built, Err(cloud::runtime::RuntimeError::StaleIncarnation)));
        assert_eq!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Isolated(cloud::IsolationReason::Stopped)
        );
        assert!(
            runtime
                .entity_facts(&before.name)
                .unwrap()
                .unwrap()
                .in_flight
                .is_empty()
        );
    } else {
        let batch = built.unwrap();
        assert_eq!(batch.saves.len(), 1);
        let save = &batch.saves[0];
        assert_eq!(save.name, before.name);
        let image = codec::receive(
            &old.facts.record_key.0,
            &save.name,
            &save.envelope,
            codec::AssetInput::Absent,
        )
        .unwrap();
        assert_eq!(image.payload, Some(edited.payload.clone()));
        // Keep this actual old-binding image unacknowledged.
    }
    assert_eq!(
        service.request_log().unwrap().len(),
        calls,
        "the old build must send nothing"
    );
    assert_eq!(
        collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    );
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 1);

    // Explicit confirmation on the same live collection draws a new record key.
    // The helper drives real scans, Service entity saves/ACKs and the active switch.
    let new = publish_actual_collection(&host, &mut service);
    assert_ne!(new.id, old.id);
    assert_ne!(new.facts.epoch, old.facts.epoch);
    assert!(new.facts.record_key.0 != old.facts.record_key.0);
    assert_eq!(new.facts.state, cloud::BindingState::Active);
    assert_eq!(collection.open_evidence().unwrap().generation, evidence.generation);
    assert_eq!(collection.open_evidence().unwrap().native_file, evidence.native_file);
    let published = runtime
        .entity_facts_page(codec::Kind::Note, None)
        .unwrap()
        .into_iter()
        .find(|e| e.key == before.key)
        .unwrap();
    assert_ne!(published.name, before.name);
    assert_eq!(published.payload, edited.payload);
    assert_eq!(published.acked_payload, Some(edited.payload.clone()));
    assert!(published.mark.is_none() && published.acked_rev > 0 && published.server_erev > 0);
    let db = service.inspect(&scope()).unwrap().unwrap();
    let record = &db.zones[&cloud::adapter::collection_zone(new.facts.epoch)].records[&published.name];
    assert!(record.fields["e"] == Field::EncryptedBytes(published.acked_envelope.0.clone()));
    assert_eq!(record.system_fields.bytes(), published.acked_system_fields.as_slice());
    assert!(db.zones[&old_zone].records[&before.name] == old_record);
    assert!(service.request_log().unwrap()[calls..].iter().all(|r| {
        r.kind != meguri_testkit::cloud::RequestKind::Modify || r.zone.as_deref() != Some(old_zone.as_str())
    }));
    assert_eq!(
        collection.undo_depth_for_testing().unwrap(),
        1,
        "retain the same native step"
    );
    let settled = collection.peek_undo(UndoScope::Global).unwrap();
    {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(lease.note(NoteId(base.id)).unwrap().unwrap().fields, expected_fields);
    }
    host.close().unwrap();
    drop(collection);
    drop(host);
    let reopened = open_host(&dir, keys);
    let runtime = reopened.cloud_runtime().unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), new);
    assert!(runtime.publication_op().unwrap().is_none());
    let note = runtime.binding_entity(new.id, &published.name).unwrap().unwrap();
    assert_eq!(note.acked_payload, Some(edited.payload));
    assert_eq!(note.acked_envelope, published.acked_envelope);
    assert_eq!(note.acked_system_fields, published.acked_system_fields);
    assert!(note.mark.is_none());
    reopened.close().unwrap();

    // This is the status observed on the original Engine, before closing it.
    assert_eq!(
        settled,
        UndoStatus::Unavailable(DisabledReason::AlreadyUploaded),
        "the same native step {} must settle after real new-binding ACKs (capture gap: {stop_at_capture})",
        step.id
    );
}

#[test]
fn new_binding_publication_ack_settles_the_native_step_flagged_by_an_unacked_old_batch() {
    republish_pending_native_note(false);
}

#[test]
fn stop_between_cloud_capture_and_registration_rejects_the_old_build_and_new_binding_ack_settles_the_step() {
    republish_pending_native_note(true);
}

fn edit_workflow_note(collection: &meguri_engine::Collection, note: &cloud::entity_codec::Note, back: &str) {
    let mut fields: Vec<String> = std::str::from_utf8(&note.flds)
        .unwrap()
        .split('\u{1f}')
        .map(str::to_owned)
        .collect();
    assert_eq!(fields.len(), 2);
    fields[1] = back.into();
    let edit = meguri_engine::notes::update(
        collection,
        meguri_engine::notes::UpdateNote {
            id: meguri_model::ids::NoteId(note.id),
            fields,
            tags: std::str::from_utf8(&note.tags)
                .unwrap()
                .split_whitespace()
                .map(str::to_owned)
                .collect(),
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    assert!(edit.value.written);
}

/// V9.2/V13/V26.2, Q6 sign_out_resume (focused single-device leg).
/// Publication, edits, review and remote reads all use production APIs.
#[test]
fn explicit_resume_after_real_isolated_edit_and_review_is_cursorless_and_requires_new_root_proof() {
    use cloud::entity_codec::{Kind, Payload};
    use meguri_engine::session::{AnswerOutcome, StartResult};
    use meguri_model::{ids::DeckId, time::EpochMillis};
    use meguri_sched::answer::Rating;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("resume");
    generated(&dir);
    let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
        cloud::OpaqueBytes(scope().account.into_bytes()),
    ))));
    let host = open_host_with(&dir, keys(), account.clone(), scope());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(706)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let original_instance = runtime.instance().unwrap();
    let before = runtime.entity_facts_page(Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &before.payload else {
        panic!("published note")
    };
    let (old_sdk, _, _) = runtime.register_sdk(false).unwrap();
    runtime
        .adapter_state_update(&old_sdk, b"native-sdk-checkpoint")
        .unwrap();
    let open = runtime.stored_state_for_testing().unwrap().open_counter;
    assert!(runtime.incoming_engine_state_for_testing(binding.id).unwrap().is_some());

    *account.0.lock().unwrap() = IdentityStatus::SignedOut;
    runtime.refresh_identity().unwrap();
    edit_workflow_note(&collection, note, "edited while signed out");
    let card = runtime.entity_facts_page(Kind::Card, None).unwrap().remove(0);
    let Payload::Card(card) = card.payload else {
        panic!("published card")
    };
    let StartResult::Started { session_id, head, .. } = host.review().start(DeckId(card.did), None).unwrap() else {
        panic!("local review remains available while isolated")
    };
    let item = &head.items[0];
    assert!(matches!(
        host.review()
            .answer(
                session_id,
                item.card_id,
                Rating::Easy,
                item.state_token,
                EpochMillis(NOW),
                123
            )
            .unwrap(),
        AnswerOutcome::Accepted { .. }
    ));
    host.review().end(session_id).unwrap();
    // Isolated capture skipped both operations; the preview must read actual rows.
    assert!(runtime.entity_facts(&before.name).unwrap().unwrap() == before);
    *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(scope().account.into_bytes()));
    runtime.refresh_identity().unwrap();
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        cloud::BindingState::Isolated(cloud::IsolationReason::SignedOut)
    );
    assert!(runtime.resume_binding_offer().is_none());
    assert!(runtime.resume_binding(binding.id, &collection).is_err());

    let p = actual_preview(&host, &mut service, &scope());
    assert_eq!(p.decision, ConnectDecision::ConfirmResume);
    assert_eq!(p.resume_binding, Some(binding.id));
    assert!(p.unsent_by_kind[&Kind::Note] >= 1);
    assert!(p.unsent_by_kind[&Kind::Revlog] >= 1);
    assert_eq!(runtime.resume_binding_offer(), Some(binding.id));
    assert_eq!(
        runtime
            .policy_projection(cloud::policy::Lifecycle::Ready, collection.generation().0)
            .unwrap()
            .facts
            .resume_binding_offered,
        Some(true)
    );
    let requests = service.request_log().unwrap().len();
    host.resume_cloud_binding(&p, collection.generation()).unwrap();
    assert_eq!(
        service.request_log().unwrap().len(),
        requests,
        "Resume performs no remote write"
    );
    assert_eq!(runtime.status().unwrap().binding.unwrap(), binding);
    assert_eq!(runtime.stored_state_for_testing().unwrap().open_counter, open);
    assert!(runtime.stored_state_for_testing().unwrap().durable_file.is_some());
    assert!(!runtime.stored_state_for_testing().unwrap().drained);
    assert!(runtime.incoming_engine_state_for_testing(binding.id).unwrap().is_none());
    assert!(!runtime.status().unwrap().scan_required);
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert_eq!(pending.acked_payload, before.acked_payload);
    assert_eq!(pending.acked_system_fields, before.acked_system_fields);
    assert_eq!(pending.acked_envelope, before.acked_envelope);
    assert!(!runtime.entity_facts_page(Kind::Revlog, None).unwrap().is_empty());
    assert!(runtime.inbox_entries(binding.id).unwrap().is_empty());
    assert!(runtime.validate_caller(&original_instance).is_err());
    assert!(runtime.validate_sdk(&old_sdk).is_err());
    assert!(runtime.adapter_state_update(&old_sdk, b"late").is_err());
    assert!(runtime.confirm_connection_resume(&p, &collection).is_err());
    assert!(!runtime.sdk_root_confirmed());

    // Native cancellation must finish before another object can be registered.
    assert!(runtime.register_sdk(false).is_err());
    runtime.release_sdk(&old_sdk).unwrap();
    let (new_sdk, cursor, _) = runtime.register_sdk(false).unwrap();
    assert!(cursor.is_none());
    let fenced_names = entity_names(runtime);
    let pending_before: Vec<_> = fenced_names
        .iter()
        .map(|name| runtime.entity_facts(name).unwrap().unwrap())
        .collect();
    let requests = service.request_log().unwrap().len();
    let fenced = runtime
        .build_collection_batch(&new_sdk, &collection, &fenced_names)
        .unwrap();
    assert_eq!(fenced.id, 0);
    assert!(fenced.saves.is_empty());
    assert!(!runtime.sdk_root_confirmed());
    assert_eq!(service.request_log().unwrap().len(), requests);
    assert!(
        pending_before
            .iter()
            .all(|before| runtime.entity_facts(&before.name).unwrap().unwrap() == *before),
        "a root-fenced build must preserve pending tracking state"
    );
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Read {
                zone: "meguri-root".into(),
                name: "active".into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    let Ok(Reply::Record(record)) = response.result else {
        panic!("independent Active read")
    };
    let active = publication_control_record(&record).unwrap();
    runtime.sdk_fetch_completed_without_root(&new_sdk).unwrap();
    assert!(
        runtime
            .accept_root_read(
                &new_sdk,
                706,
                active.encrypted.as_deref().unwrap(),
                &active.system_fields.0,
                false
            )
            .unwrap()
    );
    let batch = runtime
        .build_collection_batch(&new_sdk, &collection, &entity_names(runtime))
        .unwrap();
    assert!(!batch.saves.is_empty());
    assert!(send_actual_batch_with_instance(&host, &mut service, &batch, new_sdk).is_empty());
    let settled = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(settled.mark.is_none());
    assert!(settled.in_flight.is_empty());
    assert_eq!(settled.acked_rev, settled.local_rev);
    assert!(settled.acked_payload.as_ref() == Some(&settled.payload));
    host.close().unwrap();
}

#[test]
fn explicit_resume_with_retained_sdk_settles_real_media_intent_before_scan() {
    resume_media_fixture(false, false);
}

#[test]
fn disabled_settings_resume_settles_accepted_media_intent_before_scan_and_enable() {
    resume_media_fixture(true, false);
}

#[test]
fn disabled_settings_refuse_ready_media_intent_admission_and_network_reply() {
    resume_media_fixture(true, true);
}

fn disable_device_sync(host: &CoreHost) {
    use cloud::policy::{DeviceEvent, DeviceSettings};
    host.cloud_policy_input(DeviceEvent::Settings(DeviceSettings {
        enabled: false,
        paused: false,
        automatic: false,
        media: true,
    }))
    .unwrap();
    assert!(!host.cloud_policy_driver().unwrap().state().settings.enabled);
}

#[test]
fn disabled_settings_resume_with_empty_media_intents_finishes_scan_before_enable() {
    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("resume-empty-media");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(710)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
    runtime.adapter_stop_syncing().unwrap();
    disable_device_sync(&host); // The real Settings driver calls adapter_pause(true).
    let preview = actual_preview(&host, &mut service, &scope());
    assert_eq!(preview.decision, ConnectDecision::ConfirmResume);
    let requests = service.request_log().unwrap().len();
    host.resume_cloud_binding(&preview, collection.generation()).unwrap();
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());
    assert_eq!(runtime.status().unwrap().binding.unwrap(), binding);
    assert!(!runtime.status().unwrap().scan_required);
    assert!(runtime.status().unwrap().scan_completed_this_open);
    assert!(runtime.incoming_engine_state_for_testing(binding.id).unwrap().is_none());
    assert!(runtime.incoming_sdk_instance_for_testing().is_none());
    assert!(!host.cloud_policy_driver().unwrap().state().settings.enabled);
    // Whole reconciliation succeeded, but the actual adapter remains paused.
    assert!(runtime.register_sdk(false).is_err());
    assert_eq!(service.request_log().unwrap().len(), requests);
    host.close().unwrap();
}

fn resume_media_fixture(disabled: bool, ready_only: bool) {
    use cloud::entity_codec::{self as codec, Kind, Payload};
    use cloud::runtime::ScanPausePoint;
    use meguri_model::faultpoint::{self, Fault};
    use std::sync::mpsc;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("resume-media");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(709)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let media = host.media().unwrap();
    let old_caller = runtime.instance().unwrap();
    let (old_sdk, _, _) = runtime.register_sdk(false).unwrap();

    // Real local bytes enter the production feed; a different writer then saves
    // their deletion to Service and delivers its actual accepted envelope.
    let path = dir.join("collection.media/retired.jpg");
    std::fs::write(&path, b"synthetic media removed by another writer").unwrap();
    media.store().register_full(|_, _| true).unwrap();
    runtime.consume_media_feed(&collection, &media).unwrap();
    let before = runtime.entity_facts_page(Kind::MediaName, None).unwrap().remove(0);
    assert!(matches!(&before.payload, Payload::MediaName(p) if p.name == b"retired.jpg"));
    let envelope = codec::Entity::build(
        codec::EntityFields {
            kind: Kind::MediaName,
            key: before.key.clone(),
            origin: before.origin,
            erev: before.server_erev + 1,
            generation: None,
            tombstone: true,
            deleted_at_ms: NOW,
            writer: binding.id.0.map(|b| b ^ 0x80),
            base_sha256: None,
        },
        &before.payload.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap();
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::Modify {
                zone: cloud::adapter::collection_zone(binding.facts.epoch),
                atomic: false,
                changes: vec![Mutation::Save {
                    record: RecordInput::encrypted(&before.name, "MGEntity", envelope.clone()),
                    if_unchanged: None,
                }],
            },
        )
        .unwrap();
    let Ok(Reply::Modified(mut items)) = response.result else {
        panic!("actual media save")
    };
    assert_eq!(items.len(), 1);
    let Ok(MutationSuccess::Saved(record)) = items.remove(0).result else {
        panic!("accepted media tombstone")
    };
    let saved = Saved {
        name: record.name,
        envelope,
        system_fields: record.system_fields.bytes().to_vec(),
    };
    runtime
        .receive_collection_records(&old_caller, std::slice::from_ref(&saved))
        .unwrap();
    let inbox = runtime.inbox_entries(binding.id).unwrap();
    assert_eq!(inbox.len(), 1);
    assert_eq!(inbox[0].entry.reason, cloud::InboxReason::Ready);
    assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
    assert!(runtime.incoming_wal_receipts().is_empty());

    if ready_only {
        // Capture genuinely issued network work before pausing, then try both
        // a stale reply and fresh READY selection while the device is disabled.
        let work = runtime.cloud_media_work(&collection, &media).unwrap().unwrap();
        runtime.validate_cloud_media_work(&collection, &work).unwrap();
        let files = std::fs::read(&path).unwrap();
        disable_device_sync(&host);
        assert!(runtime.cloud_media_work(&collection, &media).unwrap().is_none());
        assert!(runtime.validate_cloud_media_work(&collection, &work).is_err());
        assert!(
            runtime
                .cloud_media_reply(&collection, &media, &work, cloud::media::Reply::UnknownItem)
                .is_err()
        );
        assert!(runtime.drain_incoming(&collection, Duration::from_secs(30)).is_err());
        assert!(runtime.apply_intents(binding.id).unwrap().is_empty());
        assert!(runtime.inbox_entries(binding.id).unwrap() == inbox);
        assert!(runtime.binding_entity(binding.id, &before.name).unwrap().unwrap() == before);
        assert!(runtime.incoming_wal_receipts().is_empty());
        assert_eq!(std::fs::read(&path).unwrap(), files);
        host.close().unwrap();
        return;
    }

    faultpoint::arm(
        "cloud.media.after_resolution_install",
        Fault::IoError(std::io::ErrorKind::Other),
    );
    let applied = runtime.drain_incoming(&collection, Duration::from_secs(30));
    faultpoint::disarm("cloud.media.after_resolution_install");
    assert!(applied.is_err());
    assert!(!path.exists());
    let pending = runtime.apply_intents(binding.id).unwrap();
    assert_eq!(pending.len(), 1);
    assert_eq!(pending[0].inbox_ids, vec![inbox[0].id]);
    let apply = pending[0].media.as_ref().expect("actual persisted media apply");
    assert!(apply.installed && !apply.abandoned && apply.server.is_none());
    assert!(apply.incoming == saved);
    assert!(runtime.binding_entity(binding.id, &before.name).unwrap().unwrap() == before);
    assert!(runtime.incoming_wal_receipts().is_empty());
    let wal_floor = collection.last_seq();
    runtime.adapter_stop_syncing().unwrap();
    if disabled {
        disable_device_sync(&host);
    }
    assert!(runtime.apply_intents(binding.id).unwrap() == pending);
    assert!(runtime.inbox_entries(binding.id).unwrap() == inbox);
    let preview = actual_preview(&host, &mut service, &scope());
    assert_eq!(preview.decision, ConnectDecision::ConfirmResume);
    let calls = service.request_log().unwrap().len();

    let (reached_tx, reached_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    runtime
        .arm_note_scan_pause_for_testing(ScanPausePoint::BeforeSnapshot, reached_tx, release_rx)
        .unwrap();
    let (reached, intents, receipts, engine, retained, callback, caller, result) = std::thread::scope(|threads| {
        let worker = threads.spawn(|| host.resume_cloud_binding(&preview, collection.generation()));
        let reached = reached_rx.recv_timeout(Duration::from_secs(30));
        let intents = runtime.apply_intents(binding.id);
        let receipts = runtime.incoming_wal_receipts();
        let engine = runtime.incoming_engine_state_for_testing(binding.id);
        let retained = runtime.incoming_sdk_instance_for_testing();
        let callback = runtime.adapter_state_update(&old_sdk, b"late-media-callback");
        let caller = runtime.validate_caller(&old_caller);
        let _ = release_tx.send(());
        (
            reached,
            intents,
            receipts,
            engine,
            retained,
            callback,
            caller,
            worker.join(),
        )
    });
    reached.expect("Resume must settle media before the whole scan's snapshot");
    assert!(intents.unwrap().is_empty());
    assert!(engine.unwrap().is_none());
    assert!(receipts.iter().any(|receipt| {
        receipt.identity().native_file() == (pending[0].native_file.device, pending[0].native_file.inode)
            && receipt.identity().generation().0 == collection.generation().0
            && receipt.through() >= wal_floor
    }));
    assert_eq!(retained.as_deref(), Some(old_sdk.as_str()));
    assert!(callback.is_err() && caller.is_err());
    result.expect("Resume worker must not panic").unwrap();
    assert_eq!(service.request_log().unwrap().len(), calls);
    assert!(runtime.inbox_entries(binding.id).unwrap().is_empty());
    let settled = runtime.binding_entity(binding.id, &before.name).unwrap().unwrap();
    assert_eq!(settled.acked_envelope.0, saved.envelope);
    assert_eq!(settled.acked_system_fields, saved.system_fields);
    assert_eq!(settled.acked_payload, Some(before.payload.clone()));
    assert_eq!(settled.acked_row_digest, before.payload.row_digest().unwrap());
    assert_eq!(settled.acked_payload_digest, before.payload.payload_digest().unwrap());
    assert!(matches!(codec::Entity::decode(&settled.acked_envelope.0),
        Ok(codec::DecodeOutcome::Supported(e)) if e.fields.tombstone && e.fields.deleted_at_ms == NOW));
    assert!(!runtime.status().unwrap().scan_required);
    assert!(runtime.register_sdk(false).is_err());
    assert!(runtime.confirm_connection_resume(&preview, &collection).is_err());
    runtime.release_sdk(&old_sdk).unwrap();
    if disabled {
        assert!(!host.cloud_policy_driver().unwrap().state().settings.enabled);
        assert!(runtime.incoming_sdk_instance_for_testing().is_none());
        assert!(
            runtime.register_sdk(false).is_err(),
            "local replay must not unpause the adapter"
        );
    } else {
        let (_, cursor, _) = runtime.register_sdk(false).unwrap();
        assert!(cursor.is_none());
    }
    host.close().unwrap();
}

/// Retained authority refuses stale explicit actions without changing the binding.
#[test]
fn resume_proof_refuses_changed_account_file_fence_operation_and_replaced_preview() {
    for change in ["account", "file", "fence", "operation", "preview", "pause", "reopen"] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("resume-refusal");
        generated(&dir);
        let account = Arc::new(MutableAccount(Mutex::new(IdentityStatus::Available(
            cloud::OpaqueBytes(scope().account.into_bytes()),
        ))));
        let host = open_host_with(&dir, keys(), account.clone(), scope());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(707)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        let collection = host.collection().unwrap();
        runtime.adapter_stop_syncing().unwrap();
        let p = actual_preview(&host, &mut service, &scope());
        assert_eq!(p.decision, ConnectDecision::ConfirmResume);
        let old_file = dir.join("held-original.anki2");
        match change {
            "account" => {
                *account.0.lock().unwrap() = IdentityStatus::Available(cloud::OpaqueBytes(b"other-account".to_vec()))
            }
            "file" => {
                std::fs::rename(collection.path(), &old_file).unwrap();
                std::fs::copy(&old_file, collection.path()).unwrap();
            }
            "fence" | "reopen" => restore_collection(&host, &dir),
            "operation" => {
                runtime.confirm_connection_publication(&p, &collection).unwrap();
            }
            "preview" => {
                let newer = actual_preview(&host, &mut service, &scope());
                assert_ne!(newer.id, p.id);
            }
            "pause" => runtime.adapter_pause(true).unwrap(),
            _ => unreachable!(),
        }
        let result = runtime.confirm_connection_resume(&p, &collection);
        if change == "account" {
            assert!(matches!(
                result,
                Err(cloud::runtime::RuntimeError::Publication(
                    PublicationError::AccountMismatch
                ))
            ));
        } else {
            assert!(result.is_err(), "stale {change} proof admitted");
        }
        if change == "file" {
            std::fs::remove_file(collection.path()).unwrap();
            std::fs::rename(&old_file, collection.path()).unwrap();
        }
        assert_ne!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            cloud::BindingState::Active
        );
        assert_eq!(runtime.status().unwrap().binding.unwrap().id, binding.id);
        if change == "operation" {
            let newer = actual_preview(&host, &mut service, &scope());
            assert_ne!(
                newer.decision,
                ConnectDecision::ConfirmResume,
                "binding-before is excluded"
            );
        }
        host.close().unwrap();
    }
}

/// V13 requires an actually read, complete bound MGEpoch; zone existence is
/// never supplied as a boolean. All mutations and read failures come from Service.
#[test]
fn resume_is_not_offered_for_actual_missing_zone_or_changed_epoch_manifest() {
    for missing in [false, true] {
        let temp = tempfile::tempdir().unwrap();
        let dir = temp.path().join("resume-zone");
        generated(&dir);
        let host = open_host(&dir, keys());
        let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(708)).unwrap();
        let binding = publish_actual_collection(&host, &mut service);
        let runtime = host.cloud_runtime().unwrap();
        runtime.adapter_stop_syncing().unwrap();
        let zone = cloud::adapter::collection_zone(binding.facts.epoch);
        if missing {
            assert!(
                service
                    .request(
                        &scope(),
                        Caller::default(),
                        Operation::DeleteZone {
                            zone: zone.clone(),
                            cause: meguri_testkit::cloud::RemovalCause::Deleted,
                        }
                    )
                    .unwrap()
                    .result
                    .is_ok()
            );
        } else {
            let response = service
                .request(
                    &scope(),
                    Caller::default(),
                    Operation::Read {
                        zone: zone.clone(),
                        name: "epoch".into(),
                        fields: RequestedFields::All,
                    },
                )
                .unwrap();
            let Ok(Reply::Record(record)) = response.result else {
                panic!("real manifest")
            };
            let control = publication_control_record(&record).unwrap();
            let DecodeOutcome::Supported(mut epoch) = Epoch::decode(control.encrypted.as_deref().unwrap()).unwrap()
            else {
                panic!("complete published manifest")
            };
            epoch.state = EpochState::Publishing;
            epoch.counts.clear();
            let response = service
                .request(
                    &scope(),
                    Caller::default(),
                    Operation::Modify {
                        zone: zone.clone(),
                        atomic: true,
                        changes: vec![Mutation::Save {
                            record: RecordInput::encrypted("epoch", "MGEpoch", epoch.encode().unwrap()),
                            if_unchanged: Some(record.system_fields),
                        }],
                    },
                )
                .unwrap();
            let Ok(Reply::Modified(items)) = response.result else {
                panic!("real manifest save")
            };
            assert!(items.iter().all(|i| i.result.is_ok()));
        }
        let PreviewWork::ReadActive { id } = host.begin_cloud_connection_preview().unwrap() else {
            panic!("Active read required")
        };
        let response = service
            .request(
                &scope(),
                Caller::default(),
                Operation::Read {
                    zone: "meguri-root".into(),
                    name: "active".into(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        let Ok(Reply::Record(record)) = response.result else {
            panic!("real Active read")
        };
        assert!(matches!(
            runtime
                .connection_active_read(id, RecordRead::Found(publication_control_record(&record).unwrap()))
                .unwrap(),
            PreviewWork::ReadEpoch { .. }
        ));
        let response = service
            .request(
                &scope(),
                Caller::default(),
                Operation::Read {
                    zone,
                    name: "epoch".into(),
                    fields: RequestedFields::All,
                },
            )
            .unwrap();
        let read = match response.result {
            Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
            Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
            _ => panic!("actual manifest result required"),
        };
        assert!(runtime.connection_epoch_read(id, read).is_err());
        assert!(runtime.resume_binding_offer().is_none());
        assert!(runtime.resume_binding(binding.id, &host.collection().unwrap()).is_err());
        host.close().unwrap();
    }
}

#[test]
fn prior_open_observation_does_not_block_reopened_batch_or_sync_now_selection() {
    use cloud::entity_codec::{self as codec, Payload};
    use meguri_storage::ReaderRole;

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("observation-restart");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(616)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = &before.payload else {
        panic!("native note required");
    };
    edit_workflow_note(&collection, note, "pending across restart");
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert!(pending.mark.is_some());
    assert_eq!(
        pending.observer_open,
        runtime.stored_state_for_testing().unwrap().open_counter
    );
    host.close().unwrap();
    drop(collection);
    drop(host);

    let host = open_host(&dir, keys());
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    runtime.refresh_identity().unwrap();
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    assert_eq!(runtime.status().unwrap().binding.unwrap(), binding);
    let current_open = runtime.stored_state_for_testing().unwrap().open_counter;
    assert!(current_open > pending.observer_open);
    let (seq, generation) = {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        let snapshot = lease.snapshot().unwrap();
        assert_eq!(pending.observed_generation, snapshot.generation().0);
        assert!(pending.observed_seq > snapshot.seq().0);
        (snapshot.seq().0, snapshot.generation().0)
    };
    let rescanned = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(rescanned.observer_open, current_open);
    assert_eq!(rescanned.observed_generation, generation);
    assert_eq!(rescanned.observed_seq, seq);
    assert_eq!(rescanned.local_rev, pending.local_rev);
    assert_eq!(rescanned.payload, pending.payload);
    let mut expected = pending.clone();
    expected.observer_open = current_open;
    expected.observed_seq = seq;
    expected.observed_generation = generation;
    assert!(
        rescanned == expected,
        "reconciliation may change only actual scan provenance"
    );

    let instance = runtime.instance().unwrap();
    runtime.start_sdk_instance(&instance).unwrap();
    let response = service
        .request(
            &scope(),
            Caller::default(),
            Operation::FetchZone {
                zone: "meguri-root".into(),
                token: None,
            },
        )
        .unwrap();
    let Ok(Reply::ZoneChanges(page)) = response.result else {
        panic!("actual root fetch required");
    };
    assert!(!page.more, "the small published root must be fully fetched");
    let active = page
        .changes
        .into_iter()
        .find_map(|change| match change {
            meguri_testkit::cloud::RecordChange::Saved(record)
                if record.name == "active" && record.record_type == "MGActive" =>
            {
                Some(record)
            }
            _ => None,
        })
        .expect("actual published root required");
    let Field::EncryptedBytes(envelope) = &active.fields["e"] else {
        panic!("root envelope required");
    };
    assert!(
        runtime
            .accept_root_fetch(
                &instance,
                Some(&Saved {
                    name: active.name.clone(),
                    envelope: envelope.clone(),
                    system_fields: active.system_fields.bytes().to_vec(),
                }),
                !page.more,
                response.request_id,
            )
            .unwrap()
    );
    let exchange = runtime.sync_now(&instance, &collection).unwrap();
    let selected = runtime.exchange_result(exchange).unwrap();
    assert!(selected.selected_names.contains(&before.name));
    assert!(selected.boundary_seq < pending.observed_seq);
    let batch = runtime
        .build_collection_batch(&instance, &collection, &BTreeSet::from([before.name.clone()]))
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    assert_eq!(batch.saves[0].name, before.name);
    let image = codec::receive(
        &binding.facts.record_key.0,
        &batch.saves[0].name,
        &batch.saves[0].envelope,
        codec::AssetInput::Absent,
    )
    .unwrap();
    assert_eq!(image.payload, Some(pending.payload));
    host.close().unwrap();
}

#[test]
fn whole_scan_after_actual_reopen_refreshes_acknowledged_row_provenance_without_tracking_changes() {
    use cloud::entity_codec::{Kind, Payload};
    use meguri_engine::{DisabledReason, UndoScope, UndoStatus};
    use meguri_model::ids::NoteId;
    use meguri_storage::{ReaderRole, RowKey, Table};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("acknowledged-scan-provenance");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(618)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let published = runtime.entity_facts_page(Kind::Note, None).unwrap();
    assert_eq!(published.len(), 2);
    assert!(
        published.iter().all(|row| {
            row.server_erev > 0 && row.mark.is_none() && row.acked_payload.as_ref() == Some(&row.payload)
        })
    );
    let Payload::Note(note) = &published[1].payload else {
        panic!("actual published note");
    };
    edit_workflow_note(&collection, note, "retained dirty flight across reopen");
    let pending_name = published[1].name.clone();
    let batch = runtime
        .build_collection_batch(
            &runtime.instance().unwrap(),
            &collection,
            &BTreeSet::from([pending_name.clone()]),
        )
        .unwrap();
    assert_eq!(batch.saves.len(), 1);
    let before = runtime.entity_facts_page(Kind::Note, None).unwrap();
    assert!(before[0].mark.is_none());
    assert!(before[1].mark.is_some() && !before[1].in_flight.is_empty());
    let old_open = runtime.stored_state_for_testing().unwrap().open_counter;
    let counter = runtime.status().unwrap().change_counter;
    host.close().unwrap();
    drop(collection);
    drop(host);

    let host = open_host(&dir, keys());
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    let open = runtime.stored_state_for_testing().unwrap().open_counter;
    assert!(open > old_open);
    let (seq, generation) = {
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        (snapshot.seq().0, snapshot.generation().0)
    };
    let history = collection.cloud_pending_keys().unwrap();
    assert!(history.is_empty(), "M6/M9.7: close drops history and its pending map");
    let undo_depth = collection.undo_depth_for_testing().unwrap();
    assert_eq!(undo_depth, 0, "a fresh opening has no undo history");
    // Phase A: prove unchanged acknowledged and dirty/in-flight tracking across
    // the actual reopen before creating any history in this opening.
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let status = runtime.status().unwrap();
    assert_eq!(status.binding.as_ref(), Some(&binding));
    assert!(!status.scan_required && !status.tracking_stopped);
    assert_eq!(status.change_counter, counter);
    let stored = runtime.stored_state_for_testing().unwrap();
    assert!(stored.whole_scan);
    let cursor = &stored.coverage.as_ref().unwrap().cursor;
    assert_eq!((cursor.seq, cursor.generation), (seq, generation));
    for mut expected in before {
        expected.observer_open = open;
        expected.observed_seq = seq;
        expected.observed_generation = generation;
        let actual = runtime.entity_facts(&expected.name).unwrap().unwrap();
        assert!(
            actual == expected,
            "scan may change only provenance for an equal payload"
        );
    }
    assert_eq!(collection.last_seq().0, seq);
    assert_eq!(collection.undo_depth_for_testing().unwrap(), undo_depth);
    assert_eq!(collection.cloud_pending_keys().unwrap(), history);
    let (sdk, _, _) = runtime.register_sdk(false).unwrap();
    runtime.validate_sdk(&sdk).unwrap();

    // Phase B: create real pending history in this same opening. Use the batch
    // builder's production Cloud barrier without another SDK send or fetch.
    let pending = runtime.entity_facts(&pending_name).unwrap().unwrap();
    let Payload::Note(note) = &pending.payload else {
        panic!("actual pending note");
    };
    edit_workflow_note(&collection, note, "new pending history within reopened collection");
    let touched = (Table::Notes, RowKey::Id(NoteId(note.id).0));
    {
        let mut lease = collection.reader(ReaderRole::Cloud).unwrap();
        let (_snapshot, hold) = lease.capture_cloud(&collection, vec![touched.clone()]).unwrap();
        assert!(!hold.flagged.is_empty(), "the real barrier must flag the new edit");
    }
    // O1.1/O4: the no-row barrier commits after its held snapshot and takes a
    // new sequence. Sample the distinct post-barrier frontier for this scan.
    let (seq, generation) = {
        let lease = collection.reader(ReaderRole::Cloud).unwrap();
        let snapshot = lease.snapshot().unwrap();
        (snapshot.seq().0, snapshot.generation().0)
    };
    let history = collection.cloud_pending_keys().unwrap();
    assert!(!history.is_empty(), "this opening must contain real pending history");
    assert_eq!(
        history.values().flatten().cloned().collect::<BTreeSet<_>>(),
        BTreeSet::from([touched])
    );
    let undo_depth = collection.undo_depth_for_testing().unwrap();
    assert!(undo_depth > 0);
    assert!(matches!(
        collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    let before = runtime.entity_facts_page(Kind::Note, None).unwrap();
    let counter = runtime.status().unwrap().change_counter;
    let current_open = runtime.stored_state_for_testing().unwrap().open_counter;
    assert_eq!(current_open, open);
    assert_eq!(collection.last_seq().0, seq);
    runtime
        .reconcile_binding(&collection, &host.media().unwrap(), Duration::from_secs(30))
        .unwrap();
    let status = runtime.status().unwrap();
    assert_eq!(status.binding.as_ref(), Some(&binding));
    assert!(!status.scan_required && !status.tracking_stopped);
    assert_eq!(status.change_counter, counter);
    let stored = runtime.stored_state_for_testing().unwrap();
    assert_eq!(stored.open_counter, current_open);
    assert!(stored.whole_scan);
    let cursor = &stored.coverage.as_ref().unwrap().cursor;
    assert_eq!((cursor.seq, cursor.generation), (seq, generation));
    for mut expected in before {
        expected.observer_open = current_open;
        expected.observed_seq = seq;
        expected.observed_generation = generation;
        let actual = runtime.entity_facts(&expected.name).unwrap().unwrap();
        assert!(
            actual == expected,
            "same-opening scan may change only actual provenance"
        );
    }
    assert_eq!(collection.last_seq().0, seq);
    assert_eq!(collection.undo_depth_for_testing().unwrap(), undo_depth);
    assert_eq!(collection.cloud_pending_keys().unwrap(), history);
    assert!(matches!(
        collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Unavailable(DisabledReason::UploadPending)
    ));
    runtime.validate_sdk(&sdk).unwrap();
    host.close().unwrap();
}

#[test]
fn same_open_observation_after_cloud_capture_defers_the_old_batch_and_preserves_the_edit() {
    use cloud::entity_codec::{self as codec, Payload};

    let temp = tempfile::tempdir().unwrap();
    let dir = temp.path().join("observation-capture");
    generated(&dir);
    let host = open_host(&dir, keys());
    let mut service = Service::open(temp.path(), Config::default(), SeededRandom::new(617)).unwrap();
    let binding = publish_actual_collection(&host, &mut service);
    let collection = host.collection().unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let before = runtime.entity_facts_page(codec::Kind::Note, None).unwrap().remove(0);
    let Payload::Note(note) = before.payload else {
        panic!("native note required");
    };
    edit_workflow_note(&collection, &note, "pending before capture");
    let pending = runtime.entity_facts(&before.name).unwrap().unwrap();
    let editing = collection.clone();
    runtime.set_cloud_capture_hook_for_test(move |_| {
        edit_workflow_note(&editing, &note, "committed after capture");
    });
    let instance = runtime.instance().unwrap();
    let names = BTreeSet::from([before.name.clone()]);
    let batch = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    let newer = runtime.entity_facts(&before.name).unwrap().unwrap();
    assert_eq!(
        newer.observer_open,
        runtime.stored_state_for_testing().unwrap().open_counter
    );
    assert_eq!(newer.observer_open, pending.observer_open);
    assert_eq!(newer.observed_generation, pending.observed_generation);
    assert!(newer.observed_seq > pending.observed_seq);
    assert_ne!(newer.payload, pending.payload);
    assert!(newer.mark.is_some());
    assert!(batch.saves.is_empty());
    assert!(newer.in_flight.is_empty());
    let next = runtime.build_collection_batch(&instance, &collection, &names).unwrap();
    assert_eq!(next.saves.len(), 1);
    assert_eq!(next.saves[0].name, before.name);
    let image = codec::receive(
        &binding.facts.record_key.0,
        &next.saves[0].name,
        &next.saves[0].envelope,
        codec::AssetInput::Absent,
    )
    .unwrap();
    assert_eq!(image.payload, Some(newer.payload));
    host.close().unwrap();
}
