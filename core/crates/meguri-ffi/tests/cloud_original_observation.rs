//! Focused original-read authority and real incoming COMMIT/WAL boundaries.
#![cfg(feature = "test-hooks")]
use meguri_engine::{Collection, device_state::DeviceState};
use meguri_host::CoreHost;
use meguri_model::{
    ids::{DeckId, NoteId},
    rng::SeededRandom,
};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_sync::cloud::{
    self, entity_codec as c,
    row_payload::{self, View},
    runtime::{
        CloudInputs, FileIdentityProvider, IdentityProvider, IdentityStatus, NativeFileIdentity, Runtime, RuntimeError,
        with_tracking_access,
    },
    testing::IncomingStage,
    workflow::Saved,
};
use meguri_testkit::cloud::devices::{NativeFiles, options};
use std::{
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::Duration,
};

const WAIT: Duration = Duration::from_secs(10);
const KEY: [u8; 32] = [41; 32];
struct Account(Mutex<IdentityStatus>);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &cloud::BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
#[derive(Default)]
struct Files(AtomicBool);
impl FileIdentityProvider for Files {
    fn identity(&self, path: &Path) -> Result<NativeFileIdentity, RuntimeError> {
        let mut file = NativeFiles.identity(path)?;
        if self.0.load(Ordering::SeqCst) {
            file.created_seconds += 1;
        }
        Ok(file)
    }
}
fn payload(rows: &impl ReadRows, id: NoteId) -> c::Payload {
    let view = row_payload::ReadView(rows);
    let note = rows.note(id).unwrap().unwrap();
    let layout = row_payload::notetype(
        &view,
        &view
            .row(Table::Notetypes, &RowKey::Id(note.notetype_id.0))
            .unwrap()
            .unwrap(),
        1,
    )
    .unwrap();
    row_payload::project(
        &view,
        c::Kind::Note,
        &view.row(Table::Notes, &RowKey::Id(id.0)).unwrap().unwrap(),
        |_| Ok(layout.clone()),
    )
    .unwrap()
}
fn original_baselines(rows: &impl ReadRows) -> Vec<c::Payload> {
    let view = row_payload::ReadView(rows);
    let mut baselines = Vec::new();
    for kind in row_payload::KINDS {
        let Some(table) = row_payload::table(kind) else {
            continue;
        };
        let mut after = None;
        loop {
            let page = view.page(table, after.as_ref()).unwrap();
            if page.is_empty() {
                break;
            }
            for row in page {
                after = Some(RowKey::from_stored(table, &row).unwrap());
                baselines.push(
                    row_payload::project(&view, kind, &row, |id| {
                        let notetype = view.row(Table::Notetypes, &RowKey::Id(id))?.unwrap();
                        row_payload::notetype(&view, &notetype, 1)
                    })
                    .unwrap(),
                );
            }
        }
    }
    baselines
}
fn envelope(row: &c::Payload, revision: u64, base: Option<[u8; 32]>) -> Vec<u8> {
    c::Entity::build(
        c::EntityFields {
            kind: row.kind(),
            key: row_payload::payload_key(row).unwrap(),
            origin: [3; 16],
            erev: revision,
            generation: None,
            tombstone: false,
            deleted_at_ms: 0,
            writer: [5; 16],
            base_sha256: base,
        },
        &row.encode().unwrap(),
    )
    .unwrap()
    .encode()
    .unwrap()
}
struct Fixture {
    root: tempfile::TempDir,
    host: CoreHost,
    collection: Collection,
    runtime: Arc<Runtime>,
    account: Arc<Account>,
    files: Arc<Files>,
    id: NoteId,
    binding: cloud::BindingId,
    incoming: Saved,
    original: c::Payload,
    changed: c::Payload,
}
impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("receiver");
        std::fs::create_dir(&directory).unwrap();
        let mut config = options(&directory, 1_735_689_600_000, 441);
        meguri_engine::bootstrap::create(
            &directory.join("collection.anki2"),
            config.clock.as_ref(),
            config.random.as_ref(),
        )
        .unwrap();
        let host = CoreHost::open(config.clone()).unwrap();
        let collection = host.collection().unwrap();
        let kind = meguri_engine::notetypes::add_stock(
            &collection,
            meguri_engine::notetypes::StockKind::Basic,
            Some("Observation fixture".into()),
        )
        .unwrap()
        .value
        .id;
        let id = meguri_engine::notes::add(
            &collection,
            meguri_engine::notes::AddNote {
                notetype_id: kind,
                fields: vec!["Original question".into(), "Original answer".into()],
                tags: vec!["retained-tag".into()],
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap()
        .value
        .id;
        // Capture every original canonical row, including raw notetype parts,
        // before producing the later incoming revision. An Active binding must
        // already own these structural images; reconcile cannot publish them.
        let (original, baselines) = {
            let reader = collection.reader(ReaderRole::Query).unwrap();
            let snapshot = reader.snapshot().unwrap();
            (payload(&*snapshot, id), original_baselines(&*snapshot))
        };
        collection.flush_wal(WAIT).unwrap();
        host.close().unwrap();
        // Produce the incoming image with the ordinary updater in another real
        // CoreHost. Only transport encoding below is synthetic.
        let source = root.path().join("source");
        std::fs::create_dir(&source).unwrap();
        std::fs::copy(directory.join("collection.anki2"), source.join("collection.anki2")).unwrap();
        let source_host = CoreHost::open(options(&source, 1_735_689_607_000, 442)).unwrap();
        let source_collection = source_host.collection().unwrap();
        meguri_engine::notes::update(
            &source_collection,
            meguri_engine::notes::UpdateNote {
                id,
                fields: vec!["Updated question".into(), "Original answer".into()],
                tags: vec!["retained-tag".into()],
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap();
        let changed = payload(&*source_collection.reader(ReaderRole::Query).unwrap(), id);
        source_host.close().unwrap();
        let account = Arc::new(Account(Mutex::new(IdentityStatus::Available(cloud::OpaqueBytes(
            b"synthetic-account".to_vec(),
        )))));
        let files = Arc::new(Files::default());
        let scope = cloud::BuildScope {
            container: cloud::ContainerId::new("synthetic.original-observation").unwrap(),
            environment: cloud::Environment::Development,
        };
        let publication = DeviceState::for_collection(&directory.join("collection.anki2"))
            .publication()
            .unwrap()
            .unwrap()
            .identity;
        let binding = with_tracking_access(|| {
            let mut store = match cloud::TrackingStore::open(&directory, scope.clone()).unwrap() {
                cloud::OpenOutcome::Missing(s) | cloud::OpenOutcome::Opened(s) => s,
                _ => panic!("supported tracking store"),
            };
            let binding = store
                .create_binding(
                    cloud::BindingFacts {
                        account: cloud::OpaqueBytes(b"synthetic-account".to_vec()),
                        epoch: cloud::EpochId([7; 16]),
                        record_key: cloud::RecordKey(KEY),
                        publication,
                        state: cloud::BindingState::Active,
                    },
                    config.clock.as_ref(),
                    &SeededRandom::new(443),
                )
                .unwrap();
            for baseline in &baselines {
                let key = row_payload::payload_key(baseline).unwrap();
                let name = c::entity_name(&KEY, baseline.kind(), &key).unwrap();
                store
                    .prepare_entity_baseline_for_testing(
                        binding.id,
                        &name,
                        &envelope(baseline, 1, None),
                        cloud::OpaqueBytes(b"initial-fields".to_vec()),
                    )
                    .unwrap();
            }
            binding.id
        });
        config.cloud = Some(CloudInputs {
            scope,
            identity: account.clone(),
            files: files.clone(),
        });
        let host = CoreHost::open(config).unwrap();
        let collection = host.collection().unwrap();
        let runtime = host.cloud_runtime().unwrap().clone();
        runtime.refresh_identity().unwrap();
        runtime
            .reconcile_binding(&collection, &host.media().unwrap(), WAIT)
            .unwrap();
        let incoming = Saved {
            name: c::entity_name(&KEY, c::Kind::Note, &c::id_key(id.0)).unwrap(),
            envelope: envelope(&changed, 2, Some(original.payload_digest().unwrap())),
            system_fields: b"incoming-original-fields".to_vec(),
        };
        Self {
            root,
            host,
            collection,
            runtime,
            account,
            files,
            id,
            binding,
            incoming,
            original,
            changed,
        }
    }
    fn observe(&self, hook: impl FnOnce()) -> Result<(cloud::workflow::EntityFacts, Option<u64>), RuntimeError> {
        let reader = self.collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        let digest = payload(&*snapshot, self.id).payload_digest().unwrap();
        self.runtime.original_note_observation_with_recheck_for_testing(
            &self.collection,
            self.id.0,
            snapshot.wal_identity(),
            snapshot.seq(),
            digest,
            hook,
        )
    }
    fn receive(&self) {
        self.runtime
            .receive_collection_records(&self.runtime.instance().unwrap(), std::slice::from_ref(&self.incoming))
            .unwrap();
    }
}

#[test]
fn original_observation_rechecks_file_account_publication_and_runtime_guard_after_actual_read() {
    for drift in [
        "file",
        "account",
        "account-error",
        "publication",
        "publication-binding",
        "publication-unavailable",
        "guard",
        "bookkeeping",
        "none",
    ] {
        let fixture = Fixture::new();
        assert_eq!(
            payload(&*fixture.collection.reader(ReaderRole::Query).unwrap(), fixture.id),
            fixture.original
        );
        let baseline_seq = fixture.collection.last_seq();
        // Reconcile preserves the seeded baseline without current-open observer
        // provenance. Obtain it through a real local edit, not a tracking stamp.
        let edited = meguri_engine::notes::update(
            &fixture.collection,
            meguri_engine::notes::UpdateNote {
                id: fixture.id,
                fields: vec!["Observed local question".into(), "Original answer".into()],
                tags: vec!["retained-tag".into()],
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap();
        assert!(edited.value.written);
        assert!(edited.seq > baseline_seq);
        fixture.collection.flush_wal(WAIT).unwrap();
        // Ordinary update returns after its synchronous observer; require the
        // actual commit's persisted capture before the initial observation.
        assert!(
            fixture
                .runtime
                .capture_proof(edited.seq, fixture.collection.generation())
                .is_some_and(|proof| proof.attempted && proof.persisted)
        );
        let updated = payload(&*fixture.collection.reader(ReaderRole::Query).unwrap(), fixture.id);
        assert_ne!(
            updated.payload_digest().unwrap(),
            fixture.original.payload_digest().unwrap()
        );
        let (initial, applied) = fixture.observe(|| {}).unwrap();
        assert_eq!(initial.payload, updated);
        assert_eq!(initial.observed_seq, edited.seq.0);
        assert_eq!(applied, None);
        let seq = fixture.collection.last_seq();
        let generation = fixture.collection.generation();
        let sidecar = fixture.collection.path().parent().unwrap().join("meguri-sidecar.json");
        let saved_sidecar = std::fs::read(&sidecar).unwrap();
        let result = fixture.observe(|| match drift {
            "file" => fixture.files.0.store(true, Ordering::SeqCst),
            "account" => {
                *fixture.account.0.lock().unwrap() =
                    IdentityStatus::Available(cloud::OpaqueBytes(b"other-account".to_vec()))
            }
            "account-error" => *fixture.account.0.lock().unwrap() = IdentityStatus::ReadFailed,
            "publication" | "publication-binding" => {
                // Corrupt only actual sidecar authority at the outside-lock
                // read boundary; no entity, ACK or returned proof is supplied.
                let mut document: serde_json::Value = serde_json::from_slice(&saved_sidecar).unwrap();
                let active = &mut document["deviceState"]["active"];
                if drift == "publication" {
                    active["decision"] =
                        serde_json::to_value(meguri_engine::device_state::PublicationDecision::Pending).unwrap();
                } else {
                    active["binding"] =
                        serde_json::to_value(meguri_engine::device_state::AccountBinding::Unknown).unwrap();
                }
                std::fs::write(&sidecar, serde_json::to_vec(&document).unwrap()).unwrap();
            }
            "publication-unavailable" => {
                std::fs::rename(&sidecar, fixture.root.path().join("held-sidecar")).unwrap();
            }
            "guard" => fixture.runtime.adapter_stop_syncing().unwrap(),
            "bookkeeping" => {
                let instance = fixture.runtime.instance().unwrap();
                let _ = fixture.runtime.take_pending_updates(&instance).unwrap();
                fixture.runtime.set_automatic(&instance, true).unwrap();
            }
            "none" => {
                let _ = fixture.runtime.status().unwrap();
            }
            _ => unreachable!(),
        });
        assert_eq!(fixture.collection.last_seq(), seq);
        assert_eq!(fixture.collection.generation(), generation);
        assert_eq!(
            result.is_ok(),
            matches!(drift, "none" | "bookkeeping"),
            "read-boundary drift {drift}"
        );
        fixture.files.0.store(false, Ordering::SeqCst);
        std::fs::write(sidecar, saved_sidecar).unwrap();
        *fixture.account.0.lock().unwrap() =
            IdentityStatus::Available(cloud::OpaqueBytes(b"synthetic-account".to_vec()));
        fixture.host.close().unwrap();
    }
}

#[test]
fn original_edit_witness_refuses_a_held_snapshot_frontier_after_a_commit() {
    let fixture = Fixture::new();
    {
        let reader = fixture.collection.reader(ReaderRole::Query).unwrap();
        let snapshot = reader.snapshot().unwrap();
        let identity = snapshot.wal_identity();
        let snapshot_seq = snapshot.seq();
        let digest = payload(&*snapshot, fixture.id).payload_digest().unwrap();
        let edited = meguri_engine::notes::update(
            &fixture.collection,
            meguri_engine::notes::UpdateNote {
                id: fixture.id,
                fields: vec!["Later committed question".into(), "Original answer".into()],
                tags: vec!["retained-tag".into()],
            },
            meguri_engine::OpSource::UserUndoable,
        )
        .unwrap();
        assert!(edited.value.written);
        assert!(edited.seq > snapshot_seq);
        assert!(matches!(
            fixture.runtime.original_edit_witness_for_testing(
                &fixture.collection,
                fixture.id.0,
                fixture.original.payload_digest().unwrap(),
                identity,
                snapshot_seq,
                edited.seq,
                digest,
            ),
            Err(RuntimeError::StaleIncarnation)
        ));
        assert_eq!(snapshot.seq(), snapshot_seq);
        assert_eq!(payload(&*snapshot, fixture.id).payload_digest().unwrap(), digest);
        assert_eq!(fixture.collection.last_seq(), edited.seq);
    }
    fixture.host.close().unwrap();
}

#[test]
fn incoming_read_uses_durable_receipt() {
    let fixture = Fixture::new();
    fixture.collection.pause_automatic_wal_flush_for_testing(true);
    fixture.receive();
    let (stage_tx, stage_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let release_rx = Mutex::new(release_rx);
    fixture.runtime.set_incoming_stage_hook_for_testing(move |stage| {
        if matches!(stage, IncomingStage::ApplyAfterCommit | IncomingStage::ApplyAfterFlush) {
            stage_tx.send(stage).unwrap();
            release_rx.lock().unwrap().recv_timeout(WAIT).unwrap();
        }
    });
    let runtime = fixture.runtime.clone();
    let collection = fixture.collection.clone();
    let worker = std::thread::spawn(move || runtime.drain_incoming(&collection, WAIT));
    assert_eq!(stage_rx.recv_timeout(WAIT).unwrap(), IncomingStage::ApplyAfterCommit);
    assert_eq!(
        payload(&*fixture.collection.reader(ReaderRole::Query).unwrap(), fixture.id),
        fixture.changed
    );
    assert_eq!(fixture.runtime.incoming_runs().len(), 1);
    let run = fixture.runtime.incoming_runs()[0].clone();
    assert!(
        !fixture
            .runtime
            .incoming_wal_receipts()
            .iter()
            .any(|r| r.through().0 >= run.seq)
    );
    assert!(!fixture.runtime.apply_intents(fixture.binding).unwrap().is_empty());
    assert!(fixture.observe(|| {}).is_err());
    release_tx.send(()).unwrap();
    assert_eq!(stage_rx.recv_timeout(WAIT).unwrap(), IncomingStage::ApplyAfterFlush);
    assert!(
        fixture
            .runtime
            .incoming_wal_receipts()
            .iter()
            .any(|r| r.through().0 >= run.seq)
    );
    assert!(!fixture.runtime.apply_intents(fixture.binding).unwrap().is_empty());
    assert!(!fixture.runtime.inbox_entries(fixture.binding).unwrap().is_empty());
    assert!(fixture.observe(|| {}).is_err());
    release_tx.send(()).unwrap();
    assert!(worker.join().unwrap().unwrap() > 0);
    assert!(fixture.runtime.apply_intents(fixture.binding).unwrap().is_empty());
    assert!(fixture.runtime.inbox_entries(fixture.binding).unwrap().is_empty());
    let (observed, applied) = fixture.observe(|| {}).unwrap();
    assert_eq!(applied, Some(run.seq));
    assert_eq!(observed.observed_seq, run.seq);
    assert_eq!(observed.payload, fixture.changed);
    assert_ne!(observed.payload, fixture.original);
    // A real later rewrite has its own observation seq. The prior apply/WAL
    // receipt must never be attached to this newer canonical image.
    meguri_engine::notes::update(
        &fixture.collection,
        meguri_engine::notes::UpdateNote {
            id: fixture.id,
            fields: vec!["Later local rewrite".into(), "Original answer".into()],
            tags: vec!["retained-tag".into()],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture.collection.flush_wal(WAIT).unwrap();
    let (rewritten, applied) = fixture.observe(|| {}).unwrap();
    assert!(rewritten.observed_seq > run.seq);
    assert_eq!(applied, None);
    fixture.collection.pause_automatic_wal_flush_for_testing(false);
    fixture.host.close().unwrap();
}

#[test]
fn historical_inspection_does_not_promote_current_rows_to_missing_original_control_or_edit_proof() {
    let fixture = Fixture::new();
    let edited = meguri_engine::notes::update(
        &fixture.collection,
        meguri_engine::notes::UpdateNote {
            id: fixture.id,
            fields: vec!["Actual ordinary edit".into(), "Original answer".into()],
            tags: vec!["retained-tag".into()],
        },
        meguri_engine::OpSource::UserUndoable,
    )
    .unwrap();
    fixture.collection.flush_wal(WAIT).unwrap();
    let (live, applied) = fixture.observe(|| {}).unwrap();
    assert_eq!(live.observed_seq, edited.seq.0);
    assert_eq!(applied, None);
    let before = fixture.collection.last_seq();
    // This fixture's prebound baseline is explicitly not an original selected
    // publication/adoption. Neither equal canonical content nor a live owner can
    // create its missing original control and post-flush journal correlations.
    assert!(
        fixture
            .runtime
            .selected_original_control_witness_for_testing(&fixture.collection)
            .is_err()
    );
    assert!(
        fixture
            .runtime
            .read_original_edit_for_testing(
                &fixture.collection,
                fixture.id.0,
                fixture.original.payload_digest().unwrap(),
                [0; 32]
            )
            .is_err()
    );
    assert_eq!(fixture.collection.last_seq(), before);
    assert_eq!(fixture.observe(|| {}).unwrap().0.payload, live.payload);
    fixture.host.close().unwrap();
}
