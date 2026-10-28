//! R1 fixes from source base fb4dfd14c3e790163bf76e2d30d296fd9b261009.
//! Actual CoreHost/runtime/store and shared child SDK; no acceptance claim.
use meguri_engine::{Collection, OpSource, OpSpec};
use meguri_host::CoreHost;
use meguri_model::{ids::NoteId, rng::SeededRandom};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, StampMode, Table};
use meguri_sync::cloud::{
    self as core, entity_codec as codec,
    runtime::{CloudInputs, IdentityProvider, IdentityStatus, Runtime, ScanPausePoint},
};
use meguri_testkit::{
    cloud::{
        Caller, Config, Field, Operation, Reply, RequestedFields, Service,
        adapter::SDKRig,
        devices::{NativeFiles, PreboundDevice, options},
    },
    generate::recipe::{Recipe, generate_recipe},
};
use std::{
    collections::BTreeSet,
    sync::{Arc, Mutex, mpsc},
    time::Duration,
};

const NOW: i64 = 1_735_689_600_000;
const SEED: u64 = 0x5231;
const TIMEOUT: Duration = Duration::from_secs(30);
fn fixture(notes: usize) -> (tempfile::TempDir, Service<SeededRandom>, PreboundDevice) {
    let root = tempfile::tempdir().unwrap();
    let dir = root.path().join("device");
    let recipe = Recipe::parse(&format!("recipe: r1-regressions\ndescription: Generic synthetic rows\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic deck\n    notetype: basic\n    new: {notes}\n    fields: [\"generic\", \"back\"]\n")).unwrap();
    generate_recipe(&recipe, SEED, &dir).unwrap();
    let mut service = Service::open(root.path(), Config::default(), SeededRandom::new(SEED)).unwrap();
    let files = PreboundDevice::arrange(&dir, &mut service, SEED).unwrap();
    (root, service, files)
}
struct Account(Mutex<IdentityStatus>);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &core::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
fn host(files: &PreboundDevice) -> CoreHost {
    let mut config = options(&files.directory, NOW, SEED);
    config.cloud = Some(CloudInputs {
        scope: files.binding.scope.clone(),
        identity: Arc::new(Account(Mutex::new(IdentityStatus::Available(core::OpaqueBytes(
            files.scope.account.as_bytes().to_vec(),
        ))))),
        files: Arc::new(NativeFiles),
    });
    let host = CoreHost::open(config).unwrap();
    host.cloud_runtime().unwrap().refresh_identity().unwrap();
    host
}
fn edit(c: &Collection, id: i64, back: &str) -> meguri_engine::OpResult<meguri_engine::notes::Updated> {
    let mut fields = {
        let lease = c.reader(ReaderRole::Query).unwrap();
        lease
            .note(NoteId(id))
            .unwrap()
            .unwrap()
            .field_values()
            .map(str::to_owned)
            .collect::<Vec<_>>()
    };
    fields[1] = back.into();
    meguri_engine::notes::update(
        c,
        meguri_engine::notes::UpdateNote {
            id: NoteId(id),
            fields,
            tags: Vec::new(),
        },
        OpSource::UserUndoable,
    )
    .unwrap()
}
fn durable_state(files: &PreboundDevice) -> core::runtime::StoredRuntimeState {
    core::runtime::with_tracking_access(|| {
        let store = match core::TrackingStore::open(&files.directory, files.binding.scope.clone()).unwrap() {
            core::OpenOutcome::Opened(store) => store,
            _ => panic!("durable tracking"),
        };
        store.stored_runtime_state(files.binding.id).unwrap().unwrap()
    })
}
fn equal_entity(a: &core::workflow::EntityFacts, b: &core::workflow::EntityFacts) {
    // Whole production facts, including marks, all inflight flights and acknowledgements.
    assert_eq!(serde_json::to_value(a).unwrap(), serde_json::to_value(b).unwrap());
}

#[test]
fn delayed_saved_batch_and_root_preserve_actual_pending_and_durable_flights_after_isolation_or_reopen() {
    for event in ["account", "signout", "reopen"] {
        let (_root, mut service, files) = fixture(2);
        let mut sdk = SDKRig::open(&files, true, NOW, SEED).unwrap();
        sdk.reconcile_and_confirm_root(&mut service, 10_000).unwrap();
        let root = sdk.accepted_root_read().unwrap();
        let origin = sdk.instance();
        sdk.driver()
            .edit_note_fields(files.note.id, &["generic", "pending before delayed Saved"])
            .unwrap();
        let response = sdk.drive_until_response(&mut service, 10_000).unwrap();
        assert_eq!(response.instance, origin);
        let name = files.entity_name.clone();
        let before = sdk.entity_facts(&name).unwrap();
        assert!(before.mark.is_some());
        assert!(!before.in_flight.is_empty());
        let pending = sdk.sdk_pending();
        assert!(pending.contains_key(&name));
        match event {
            "account" => sdk
                .driver()
                .set_account_for_testing("different synthetic account".into())
                .unwrap(),
            "signout" => sdk.driver().sign_out_for_testing().unwrap(),
            "reopen" => {
                let current = sdk.driver().reopen_for_testing().unwrap();
                assert_ne!(current, origin);
                sdk.driver().reconcile_binding().unwrap();
                let caller = Caller {
                    instance: Some(current.clone()),
                    ..Caller::default()
                };
                let fetch = service
                    .request(&files.scope, caller.clone(), Operation::FetchDatabase { token: None })
                    .unwrap();
                let Ok(Reply::DatabaseChanges(page)) = fetch.result else {
                    panic!("actual completed fetch")
                };
                assert!(!page.more);
                sdk.driver().qualify_without_root(current.clone()).unwrap();
                let read = service
                    .request(
                        &files.scope,
                        caller,
                        Operation::Read {
                            zone: "meguri-root".into(),
                            name: "active".into(),
                            fields: RequestedFields::All,
                        },
                    )
                    .unwrap();
                let Ok(Reply::Record(record)) = read.result else {
                    panic!("actual current root")
                };
                let Field::EncryptedBytes(envelope) = record.fields.get("e").unwrap() else {
                    panic!("root envelope")
                };
                assert!(
                    sdk.driver()
                        .accept_root_for_instance(
                            current,
                            read.request_id,
                            envelope.clone(),
                            record.system_fields.bytes().to_vec()
                        )
                        .unwrap()
                );
            }
            _ => unreachable!(),
        }
        let runtime_before = sdk.driver().stored_runtime_state().unwrap();
        assert!(sdk.deliver_sent_event(&response.response).is_err());
        equal_entity(&before, &sdk.entity_facts(&name).unwrap());
        assert_eq!(sdk.sdk_pending(), pending);
        assert!(
            sdk.driver()
                .build_for_instance(origin.clone(), BTreeSet::from([name.clone()]))
                .unwrap()
                .saves
                .is_empty()
        );
        let Field::EncryptedBytes(envelope) = root.accepted_record.fields.get("e").unwrap() else {
            panic!("root envelope")
        };
        assert!(
            !sdk.driver()
                .accept_root_for_instance(
                    origin,
                    root.request_id,
                    envelope.clone(),
                    root.accepted_record.system_fields.bytes().to_vec()
                )
                .unwrap()
        );
        equal_entity(&before, &sdk.entity_facts(&name).unwrap());
        assert_eq!(sdk.sdk_pending(), pending);
        let runtime_after = sdk.driver().stored_runtime_state().unwrap();
        assert_eq!(
            serde_json::to_value(&runtime_after).unwrap(),
            serde_json::to_value(&runtime_before).unwrap()
        );
        sdk.close_clean().unwrap();
        equal_entity(&before, &files.read_entity_durable(&name).unwrap());
        assert_eq!(durable_state(&files).root_ack, runtime_before.root_ack);
    }
}

struct Pause {
    reached: mpsc::Receiver<()>,
    resume: Option<mpsc::Sender<()>>,
}
impl Pause {
    fn arm(runtime: &Runtime) -> Self {
        let (hit, reached) = mpsc::channel();
        let (resume, wait) = mpsc::channel();
        runtime
            .arm_note_scan_pause_for_testing(ScanPausePoint::BeforeMerge, hit, wait)
            .unwrap();
        Self {
            reached,
            resume: Some(resume),
        }
    }
    fn resume(&mut self) {
        if let Some(tx) = self.resume.take() {
            let _ = tx.send(());
        }
    }
}
impl Drop for Pause {
    fn drop(&mut self) {
        self.resume();
    }
}

#[test]
fn failed_capture_then_unrelated_success_cannot_publish_old_scan_or_durable_drain() {
    let (_root, _service, files) = fixture(2);
    let h = host(&files);
    let c = h.collection().unwrap();
    let runtime = h.cloud_runtime().unwrap();
    runtime.reconcile_binding(&c, &h.media().unwrap(), TIMEOUT).unwrap();
    let other = {
        let lease = c.reader(ReaderRole::Query).unwrap();
        lease
            .row_page(Table::Notes, None)
            .unwrap()
            .iter()
            .map(|r| core::row_payload::int(r, 0).unwrap())
            .find(|id| *id != files.note.id)
            .unwrap()
    };
    std::thread::scope(|threads| {
        let mut pause = Pause::arm(runtime);
        let (done, result) = mpsc::channel();
        let scanning = &c;
        let media = h.media().unwrap();
        threads.spawn(move || {
            let _ = done.send(runtime.reconcile_binding(scanning, &media, TIMEOUT));
        });
        // Pause drops before scope joins on every assertion/timeout path.
        pause.reached.recv_timeout(TIMEOUT).unwrap();
        runtime.arm_capture_failure_for_testing(1).unwrap();
        let lost = edit(&c, files.note.id, "lost observer change");
        assert!(!runtime.capture_proof(lost.seq, lost.generation).unwrap().persisted);
        let unrelated = edit(&c, other, "unrelated successful change");
        assert!(
            runtime
                .capture_proof(unrelated.seq, unrelated.generation)
                .unwrap()
                .persisted
        );
        assert!(runtime.status().unwrap().scan_required);
        pause.resume();
        assert!(result.recv_timeout(TIMEOUT).unwrap().is_err());
    });
    assert!(runtime.status().unwrap().scan_required);
    h.close().unwrap();
    assert!(!durable_state(&files).drained);
    let reopened = host(&files);
    let runtime = reopened.cloud_runtime().unwrap();
    assert!(runtime.status().unwrap().scan_required);
    let proof = runtime
        .reconcile_binding(&reopened.collection().unwrap(), &reopened.media().unwrap(), TIMEOUT)
        .unwrap();
    assert!(proof.recovered_names.contains(&files.entity_name));
    let recovered = runtime.entity_facts(&files.entity_name).unwrap().unwrap();
    let codec::Payload::Note(note) = recovered.payload else {
        panic!("note")
    };
    assert_eq!(note.flds.as_slice(), b"generic\x1flost observer change");
    assert!(recovered.mark.is_some());
    assert!(!runtime.status().unwrap().scan_required);
    reopened.close().unwrap();
    assert!(durable_state(&files).drained);
}

#[test]
fn ordinary_edit_materializes_only_affected_bodies_with_large_clean_baseline() {
    let mut counts = Vec::new();
    for baseline in [2, 600] {
        let (_root, mut service, files) = fixture(baseline);
        let mut sdk = SDKRig::open(&files, true, NOW, SEED).unwrap();
        sdk.reconcile_and_confirm_root(&mut service, 10_000).unwrap();
        let op = sdk
            .driver()
            .edit_note_fields(files.note.id, &["generic", "one affected note"])
            .unwrap();
        let reads = sdk.driver().last_capture_body_reads().unwrap();
        assert!(
            reads > 0 && reads <= 8,
            "actual body materializations: {reads} for {baseline} notes"
        );
        let facts = sdk.entity_facts(&files.entity_name).unwrap();
        assert_eq!(facts.observed_seq, op.seq);
        assert!(facts.mark.is_some());
        counts.push(reads);
        sdk.close_clean().unwrap();
    }
    assert_eq!(counts[0], counts[1]);
}

#[test]
fn corehost_card_capture_uses_untracked_raw_history_and_preserves_negative_maximum() {
    for history in [vec![NOW + 123, NOW + 999], vec![i64::MIN, -1]] {
        let (_root, _service, files) = fixture(1);
        // Store real untracked history through CoreHost before the cloud owner opens.
        let plain = CoreHost::open(options(&files.directory, NOW, SEED)).unwrap();
        let c = plain.collection().unwrap();
        let card_id = {
            let lease = c.reader(ReaderRole::Query).unwrap();
            core::row_payload::int(&lease.row_page(Table::Cards, None).unwrap()[0], 0).unwrap()
        };
        let max = *history.iter().max().unwrap();
        c.run(
            OpSpec::named("raw synthetic history", OpSource::SyncIncoming).stamp(StampMode::Raw),
            move |ctx| {
                for (index, id) in history.into_iter().enumerate() {
                    let row = vec![
                        Cell::Integer(id),
                        Cell::Integer(card_id),
                        Cell::Integer(0),
                        Cell::Integer(0),
                        Cell::Integer(1),
                        Cell::Integer(0),
                        Cell::Integer(0),
                        Cell::Integer(0),
                        Cell::Integer(if index == 0 { 3 } else { 5 }),
                    ];
                    ctx.scope().restore(Table::Revlog, &RowKey::Id(id), Some(&row))?;
                }
                Ok(())
            },
        )
        .unwrap();
        plain.close().unwrap();
        let h = host(&files);
        let c = h.collection().unwrap();
        let runtime = h.cloud_runtime().unwrap();
        let name = codec::entity_name(
            &files.binding.facts.record_key.0,
            codec::Kind::Card,
            &codec::id_key(card_id),
        )
        .unwrap();
        let revlog_name = codec::entity_name(
            &files.binding.facts.record_key.0,
            codec::Kind::Revlog,
            &codec::id_key(max),
        )
        .unwrap();
        assert!(runtime.entity_facts(&revlog_name).unwrap().is_none());
        let op = c
            .run(
                OpSpec::named("card-only edit", OpSource::SyncIncoming).stamp(StampMode::Raw),
                move |ctx| {
                    let mut row = ctx.scope().row_image(Table::Cards, &RowKey::Id(card_id))?.unwrap();
                    row[16] = Cell::Integer(1);
                    ctx.scope().restore(Table::Cards, &RowKey::Id(card_id), Some(&row))?;
                    Ok(())
                },
            )
            .unwrap();
        assert!(runtime.capture_proof(op.seq, op.generation).unwrap().persisted);
        assert!(runtime.last_capture_body_reads_for_testing() <= 8);
        let facts = runtime.entity_facts(&name).unwrap().unwrap();
        let codec::Payload::Card(card) = facts.payload else {
            panic!("card")
        };
        assert_eq!(card.last_review_id, max);
        let lease = c.reader(ReaderRole::Query).unwrap();
        assert_eq!(lease.max_revlog_id(meguri_model::ids::CardId(card_id)).unwrap(), max);
        drop(lease);
        h.close().unwrap();
    }
}

#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[test]
fn unsupported_static_and_deck_structure_remain_gated_through_clean_close_and_reopen() {
    for table in [Table::Notetypes, Table::Decks] {
        let (_root, _service, files) = fixture(2);
        let h = host(&files);
        let c = h.collection().unwrap();
        let runtime = h.cloud_runtime().unwrap();
        runtime.reconcile_binding(&c, &h.media().unwrap(), TIMEOUT).unwrap();
        let (key, mut row) = {
            let lease = c.reader(ReaderRole::Query).unwrap();
            let row = lease.row_page(table, None).unwrap().remove(0);
            (RowKey::from_stored(table, &row).unwrap(), row)
        };
        row[1] = Cell::text("Changed generic structure");
        let op = c
            .run(
                OpSpec::named("unsupported structural edit", OpSource::SyncIncoming).stamp(StampMode::Raw),
                move |ctx| {
                    ctx.scope().restore(table, &key, Some(&row))?;
                    Ok(())
                },
            )
            .unwrap();
        assert!(!runtime.capture_proof(op.seq, op.generation).unwrap().persisted);
        if table == Table::Decks {
            let later = edit(&c, files.note.id, "supported after unsupported deck edit");
            assert!(runtime.capture_proof(later.seq, later.generation).unwrap().persisted);
        }
        assert!(runtime.status().unwrap().scan_required);
        assert!(runtime.reconcile_binding(&c, &h.media().unwrap(), TIMEOUT).is_err());
        assert!(runtime.require_collection_sending().is_err());
        h.close().unwrap();
        assert!(!durable_state(&files).drained);
        let reopened = host(&files);
        let runtime = reopened.cloud_runtime().unwrap();
        assert!(runtime.status().unwrap().scan_required);
        assert!(
            runtime
                .reconcile_binding(&reopened.collection().unwrap(), &reopened.media().unwrap(), TIMEOUT)
                .is_err()
        );
        reopened.close().unwrap();
        assert!(!durable_state(&files).drained);
    }
}
