use super::*;
use cloud::publication::*;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_sync::cloud::runtime::{KeyByteSource, KeySourceError};
use meguri_testkit::cloud::{
    SystemFields,
    coordinator::{execute_publication_request, publication_control_record},
};
use std::sync::Mutex;

pub(super) struct MutableAccount(pub Mutex<IdentityStatus>);
impl IdentityProvider for MutableAccount {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
struct Keys(Mutex<u8>);
impl KeyByteSource for Keys {
    fn draw_key(&self) -> Result<[u8; 32], KeySourceError> {
        let mut draw = self.0.lock().unwrap();
        let key = std::array::from_fn(|i| (i as u8).wrapping_mul(13).wrapping_add(17 + *draw));
        *draw += 1;
        Ok(key)
    }
}
fn config(dir: &Path, identity: Arc<dyn IdentityProvider>) -> meguri_host::HostConfig {
    let mut config = options(dir, NOW, 91);
    config.clock = Arc::new(StepClock::new(NOW, UtcOffset::UTC, "UTC"));
    config.cloud = Some(CloudInputs {
        scope: cloud::BuildScope {
            container: cloud::ContainerId::new(scope().container).unwrap(),
            environment: cloud::Environment::Development,
        },
        identity,
        files: Arc::new(NativeFiles),
    });
    config.cloud_key_bytes = Some(Arc::new(Keys(Mutex::new(0))));
    config
}
pub(super) fn open_actual(dir: &Path, identity: Arc<dyn IdentityProvider>) -> CoreHost {
    CoreHost::open(config(dir, identity)).unwrap()
}
pub(super) fn generated(dir: &Path) {
    let recipe = Recipe::parse("recipe: resume-absence\ndescription: Public synthetic Resume regression\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Resume\n    notetype: basic\n    new: 1\n    due: 1\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 91, dir).unwrap();
}
pub(super) fn read_control(service: &mut Service<SeededRandom>, zone: &str, name: &str) -> RecordRead {
    read_control_for_scope(service, &scope(), zone, name)
}
fn read_control_for_scope(
    service: &mut Service<SeededRandom>,
    service_scope: &Scope,
    zone: &str,
    name: &str,
) -> RecordRead {
    let response = service
        .request(
            service_scope,
            Caller::default(),
            Operation::Read {
                zone: zone.into(),
                name: name.into(),
                fields: RequestedFields::All,
            },
        )
        .unwrap();
    match response.result {
        Ok(Reply::Record(record)) => RecordRead::Found(publication_control_record(&record).unwrap()),
        Err(error) if error.code == meguri_testkit::cloud::ErrorCode::ZoneNotFound => RecordRead::ZoneMissing,
        Err(error) if error.code == meguri_testkit::cloud::ErrorCode::UnknownItem => RecordRead::Missing,
        _ => panic!("actual control read required"),
    }
}
pub(super) fn actual_preview(host: &CoreHost, service: &mut Service<SeededRandom>) -> Preview {
    actual_preview_for_scope(host, service, &scope())
}
pub(super) fn actual_preview_for_scope(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    service_scope: &Scope,
) -> Preview {
    let runtime = host.cloud_runtime().unwrap();
    let PreviewWork::ReadActive { id } = host.begin_cloud_connection_preview().unwrap() else {
        panic!("active read required")
    };
    let work = runtime
        .connection_active_read(
            id,
            read_control_for_scope(service, service_scope, "meguri-root", "active"),
        )
        .unwrap();
    match work {
        PreviewWork::Ready(p) => *p,
        PreviewWork::ReadEpoch { id, epoch } => {
            let PreviewWork::Ready(p) = runtime
                .connection_epoch_read(
                    id,
                    read_control_for_scope(service, service_scope, &cloud::adapter::collection_zone(epoch), "epoch"),
                )
                .unwrap()
            else {
                panic!("epoch preview required")
            };
            *p
        }
        _ => panic!("unexpected preview work"),
    }
}
pub(super) fn names(runtime: &Runtime) -> BTreeSet<String> {
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
/// Every returned Saved comes from an accepted conditional Service mutation.
fn send_batch(
    host: &CoreHost,
    service: &mut Service<SeededRandom>,
    batch: &cloud::workflow::BuiltBatch,
    instance: &str,
) -> Vec<Saved> {
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
                instance: Some(instance.into()),
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
        panic!("actual save outcomes required")
    };
    assert_eq!(items.len(), batch.saves.len());
    let receipts: Vec<_> = items
        .into_iter()
        .map(|item| {
            let Ok(MutationSuccess::Saved(record)) = item.result else {
                panic!("accepted Saved required; no retry")
            };
            assert!(!record.system_fields.bytes().is_empty());
            saved(&record)
        })
        .collect();
    runtime.enter_sent_handler(instance, batch.id).unwrap();
    runtime
        .sent_results(
            instance,
            &host.collection().unwrap(),
            batch.id,
            &receipts,
            &[],
            batch.saves.len(),
        )
        .unwrap();
    runtime.return_sent_handler(instance).unwrap();
    receipts
}
pub(super) fn send_pending(host: &CoreHost, service: &mut Service<SeededRandom>, sdk: &str) -> Vec<Saved> {
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let mut accepted = Vec::new();
    for _ in 0..16 {
        let batch = runtime
            .build_collection_batch(sdk, &collection, &names(runtime))
            .unwrap();
        if batch.saves.is_empty() {
            return accepted;
        }
        accepted.extend(send_batch(host, service, &batch, sdk));
    }
    panic!("successful batches did not drain within existing bounded steps")
}
pub(super) fn publish(host: &CoreHost, service: &mut Service<SeededRandom>) -> cloud::Binding {
    host.media().unwrap().store().register_full(|_, _| true).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime
        .confirm_connection_publication(&actual_preview(host, service), &collection)
        .unwrap();
    let mut started = false;
    for _ in 0..32 {
        match runtime.next_publication_work(&collection).unwrap() {
            PublicationWork::Local { op, work } => runtime.perform_publication_local(op, work, &collection).unwrap(),
            PublicationWork::Own(request) => {
                let (result, _) =
                    execute_publication_request(service, &scope(), &request, runtime, &collection).unwrap();
                assert!(matches!(result, OwnResult::Saved(_)) || request.kind != OwnKind::SaveActive);
                runtime.publication_result(&request, result, &collection).unwrap();
                if request.kind == OwnKind::SaveActive {
                    assert!(runtime.publication_op().unwrap().is_none());
                    return runtime.status().unwrap().binding.unwrap();
                }
            }
            PublicationWork::Scan => {
                runtime
                    .reconcile_binding(&collection, &host.media().unwrap(), BUDGET)
                    .unwrap();
            }
            PublicationWork::SendEntities => {
                let instance = runtime.instance().unwrap();
                if !started {
                    runtime.start_sdk_instance(&instance).unwrap();
                    started = true;
                }
                let batch = runtime
                    .build_collection_batch(&instance, &collection, &names(runtime))
                    .unwrap();
                assert!(!batch.saves.is_empty());
                send_batch(host, service, &batch, &instance);
            }
            other => panic!("publication stopped: {other:?}"),
        }
    }
    panic!("no active submission within existing bound")
}
/// Initial adoption: no pre-existing B collection, replacement fixture, or SQL.
pub(super) fn adopt(dir: &Path, service: &mut Service<SeededRandom>) -> CoreHost {
    use cloud::adoption::{AdoptionReply, AdoptionTask as T, StagingRecord};
    std::fs::create_dir_all(dir).unwrap();
    assert!(!dir.join("collection.anki2").exists());
    let config = config(dir, Arc::new(Account));
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.prepare_cloud_connection(config).unwrap();
    let preview = actual_preview(&host, service);
    host.confirm_cloud_adoption(&preview).unwrap();
    for _ in 0..64 {
        if host.cloud_runtime().unwrap().adoption_op().unwrap().is_none() {
            assert!(host.collection().is_ok());
            assert_eq!(host.engine().slot_holder(), None);
            return host;
        }
        let PublicationWork::Adoption(work) = host.next_cloud_publication_work().unwrap() else {
            panic!("actual adoption work required")
        };
        if matches!(work.task, T::Validate | T::Activate) {
            host.perform_cloud_adoption_local(&work).unwrap();
            continue;
        }
        let op = host.cloud_runtime().unwrap().adoption_op().unwrap().unwrap();
        let zone = cloud::adapter::collection_zone(op.epoch);
        let reply = match &work.task {
            T::ReadActive | T::RecheckActive => AdoptionReply::Read(read_control(service, "meguri-root", "active")),
            T::ReadEpoch => AdoptionReply::Read(read_control(service, &zone, "epoch")),
            T::ReadMediaRoot => AdoptionReply::Read(read_control(service, "meguri-media", "root")),
            T::Fetch { cursor } => {
                let p = page(
                    service,
                    &zone,
                    cursor.as_ref().map(|c| serde_json::from_slice(&c.0).unwrap()),
                );
                let mut records = Vec::new();
                let mut deleted = Vec::new();
                for change in p.changes {
                    match change {
                        RecordChange::Saved(record) => {
                            assert!(
                                record.fields.values().all(|field| !matches!(field, Field::Asset(_))),
                                "inline public fixture only"
                            );
                            records.push(StagingRecord {
                                record: publication_control_record(&record).unwrap(),
                                name: record.name,
                                record_type: record.record_type,
                                payload_file: None,
                            });
                        }
                        RecordChange::Deleted { name, .. } => deleted.push(name),
                    }
                }
                AdoptionReply::Page {
                    records,
                    deleted,
                    next: p.more.then(|| OpaqueBytes(serde_json::to_vec(&p.token).unwrap())),
                }
            }
            _ => unreachable!(),
        };
        host.cloud_adoption_reply(&work, reply).unwrap();
    }
    panic!("initial adoption did not finish within existing bound")
}
/// Observe the production adapter diagnostic's durable ledger, never edit it.
/// Snapshot encodes (binding, runtime, entities, inbox, durable, engine_state).
pub(super) fn running_pass(runtime: &Runtime) -> [u8; 16] {
    let snapshot = runtime.adapter_snapshot().unwrap();
    assert!(!snapshot.absence_pass_completed);
    let diagnostic: serde_json::Value = serde_json::from_slice(&snapshot.tracking_content).unwrap();
    let ledger = &diagnostic[4]["absence"];
    assert_eq!(ledger["completed"], false);
    serde_json::from_value(ledger["pass"].clone()).expect("actual running pass diagnostic")
}
pub(super) fn assert_send_fenced(host: &CoreHost, sdk: &str) {
    let runtime = host.cloud_runtime().unwrap();
    runtime.validate_sdk(sdk).unwrap();
    assert!(runtime.adapter_snapshot().unwrap().serialization.is_none());
    assert!(!runtime.sdk_root_confirmed());
    let batch = runtime
        .build_collection_batch(sdk, &host.collection().unwrap(), &names(runtime))
        .unwrap();
    assert_eq!(batch.id, 0);
    assert!(
        batch.saves.is_empty(),
        "fresh SDK root proof must fence the first collection send"
    );
}
