//! Focused runtime components through CoreHost; no cloud transport or full scan.
#![cfg(feature = "test-hooks")]

use std::{
    path::Path,
    sync::{Arc, Mutex, mpsc},
    time::{Duration, UNIX_EPOCH},
};

use meguri_engine::{Collection, OpSource, OpSpec, SuspendOutcome, device_state::DeviceState};
use meguri_host::{CoreHost, HostConfig, ProcessCore, wiring};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    ids::{DeckId, NoteId},
    rng::SeededRandom,
};
use meguri_storage::{Cell, ReadRows, ReaderRole, RowKey, Table, space::StatvfsSpace};
use meguri_sync::cloud::{
    self, BindingFacts, BindingId, BindingState, BuildScope, ContainerId, Environment, EpochId, IsolationReason,
    OpaqueBytes, OpenOutcome, RecordKey, TrackingStore,
    entity_codec::{self as codec, Entity, EntityFields, Kind, Payload},
    runtime::{
        CloudInputs, DirtySource, FileIdentityProvider, IdentityProvider, IdentityStatus, NativeFileIdentity, Runtime,
        RuntimeError, ScanPausePoint, with_tracking_access,
    },
};

const BUDGET: Duration = Duration::from_secs(10);
const BARRIER_TIMEOUT: Duration = Duration::from_secs(30);
const ACCOUNT: &[u8] = b"synthetic-account";
const RECORD_KEY: [u8; 32] = [19; 32];

struct Account(Mutex<IdentityStatus>);
impl IdentityProvider for Account {
    fn read_identity(&self, _: &BuildScope) -> IdentityStatus {
        self.0.lock().unwrap().clone()
    }
}
impl Account {
    fn set(&self, value: IdentityStatus) {
        *self.0.lock().unwrap() = value;
    }
}
#[derive(Default)]
struct Files(Mutex<bool>);
impl FileIdentityProvider for Files {
    fn identity(&self, path: &Path) -> Result<NativeFileIdentity, RuntimeError> {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).map_err(|_| RuntimeError::FileIdentityUnavailable)?;
        let created = metadata
            .created()
            .map_err(|_| RuntimeError::FileIdentityUnavailable)?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| RuntimeError::FileIdentityUnavailable)?;
        let mut value = NativeFileIdentity {
            device: metadata.dev(),
            inode: metadata.ino(),
            created_seconds: i64::try_from(created.as_secs()).unwrap(),
            created_nanos: created.subsec_nanos(),
        };
        // Damage only the provider's creation fact; the actual device/inode stay equal.
        if *self.0.lock().unwrap() {
            value.created_seconds += 1;
        }
        Ok(value)
    }
}

fn scope(container: &str, environment: Environment) -> BuildScope {
    BuildScope {
        container: ContainerId::new(container).unwrap(),
        environment,
    }
}
fn with_store<T>(directory: &Path, scope: BuildScope, work: impl FnOnce(&mut TrackingStore) -> T) -> T {
    with_tracking_access(|| {
        let mut store = match TrackingStore::open(directory, scope).unwrap() {
            OpenOutcome::Missing(store) | OpenOutcome::Opened(store) => store,
            _ => panic!("expected disposable supported tracking store"),
        };
        work(&mut store)
    })
}
// Preserve stored cell classes in the independent expectations and canonical ack.
fn payload(collection: &Collection, id: NoteId) -> Payload {
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let row = lease.row_image(Table::Notes, &RowKey::Id(id.0)).unwrap().unwrap();
    let integer = |index| match &row[index] {
        Cell::Integer(value) => *value,
        _ => panic!("integer cell"),
    };
    let text = |index| match &row[index] {
        Cell::Text(value) => value.clone(),
        _ => panic!("text cell"),
    };
    let sfld = match &row[7] {
        Cell::Integer(v) => codec::Stored::Integer(*v),
        Cell::Real(v) => codec::Stored::Real(*v),
        Cell::Text(v) => codec::Stored::Text(v.clone()),
        Cell::Blob(v) => codec::Stored::Blob(v.clone()),
        Cell::Null => panic!("sort cell"),
    };
    Payload::Note(codec::Note {
        id: integer(0),
        guid: text(1),
        mid: integer(2),
        modified: integer(3),
        tags: text(5),
        flds: text(6),
        sfld,
        csum: integer(8),
        flags: integer(9),
        data: text(10),
        nt_struct_gen: 1,
    })
}
fn update_input(collection: &Collection, id: NoteId, back: &str) -> meguri_engine::notes::UpdateNote {
    let lease = collection.reader(ReaderRole::Query).unwrap();
    let note = lease.note(id).unwrap().unwrap();
    let mut fields: Vec<_> = note.field_values().map(str::to_owned).collect();
    fields[1] = back.into();
    meguri_engine::notes::UpdateNote {
        id,
        fields,
        tags: Vec::new(),
    }
}
fn edit(collection: &Collection, id: NoteId, back: &str) -> meguri_engine::OpResult<meguri_engine::notes::Updated> {
    meguri_engine::notes::update(collection, update_input(collection, id, back), OpSource::UserUndoable).unwrap()
}

struct Fixture {
    _root: tempfile::TempDir,
    options: HostConfig,
    ids: Vec<NoteId>,
    baselines: Vec<Payload>,
    account: Arc<Account>,
    files: Arc<Files>,
    binding: BindingId,
}
impl Fixture {
    fn new(state: BindingState) -> Self {
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("collection");
        std::fs::create_dir_all(&directory).unwrap();
        let mut options = HostConfig::new(
            directory,
            root.path().join("backups"),
            root.path().join("widget"),
            Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
            Arc::new(SeededRandom::new(20632)),
            Arc::new(StatvfsSpace),
        );
        meguri_engine::bootstrap::create(
            &options.collection_directory.join("collection.anki2"),
            options.clock.as_ref(),
            options.random.as_ref(),
        )
        .unwrap();
        let host = CoreHost::open(options.clone()).unwrap();
        assert!(host.cloud_runtime().is_none());
        let collection = host.collection().unwrap();
        let kind = meguri_engine::notetypes::add_stock(
            &collection,
            meguri_engine::notetypes::StockKind::Basic,
            Some("Runtime fixture".into()),
        )
        .unwrap()
        .value
        .id;
        let ids: Vec<_> = (0..3)
            .map(|index| {
                meguri_engine::notes::add(
                    &collection,
                    meguri_engine::notes::AddNote {
                        notetype_id: kind,
                        fields: vec![format!("runtime front {index}"), "baseline back".into()],
                        tags: Vec::new(),
                        deck_id: DeckId(1),
                        guid: None,
                        update_add_defaults: false,
                    },
                    OpSource::UserUndoable,
                )
                .unwrap()
                .value
                .id
            })
            .collect();
        let baselines = ids.iter().map(|id| payload(&collection, *id)).collect();
        host.close().unwrap();
        let account = Arc::new(Account(Mutex::new(IdentityStatus::Available(OpaqueBytes(
            ACCOUNT.into(),
        )))));
        let files = Arc::new(Files::default());
        options.cloud = Some(CloudInputs {
            scope: scope("test.runtime", Environment::Development),
            identity: account.clone(),
            files: files.clone(),
        });
        let mut fixture = Self {
            _root: root,
            options,
            ids,
            baselines,
            account,
            files,
            binding: BindingId([0; 16]),
        };
        fixture.binding = fixture.seed_scope(fixture.scope(), state, false);
        fixture
    }
    fn scope(&self) -> BuildScope {
        self.options.cloud.as_ref().unwrap().scope.clone()
    }
    fn seed_scope(&self, scope: BuildScope, state: BindingState, wrong_publication: bool) -> BindingId {
        let path = self.options.collection_directory.join("collection.anki2");
        let publication = DeviceState::for_collection(&path)
            .publication()
            .unwrap()
            .unwrap()
            .identity;
        // A different synthetic publication identity exercises the real bind gate.
        let publication = if wrong_publication {
            let mut bits = serde_json::to_value(publication).unwrap();
            bits[0] = serde_json::json!(bits[0].as_u64().unwrap() ^ 1);
            serde_json::from_value(bits).unwrap()
        } else {
            publication
        };
        with_store(&self.options.collection_directory, scope, |store| {
            let binding = store
                .create_binding(
                    BindingFacts {
                        account: OpaqueBytes(ACCOUNT.into()),
                        epoch: EpochId([8; 16]),
                        record_key: RecordKey(RECORD_KEY),
                        publication,
                        state,
                    },
                    self.options.clock.as_ref(),
                    self.options.random.as_ref(),
                )
                .unwrap();
            for baseline in &self.baselines {
                let Payload::Note(note) = baseline else {
                    panic!("note fixture")
                };
                let key = codec::id_key(note.id);
                let name = codec::entity_name(&RECORD_KEY, Kind::Note, &key).unwrap();
                let envelope = Entity::build(
                    EntityFields {
                        kind: Kind::Note,
                        key: key.to_vec(),
                        origin: [7; 16],
                        erev: 1,
                        generation: None,
                        tombstone: false,
                        deleted_at_ms: 0,
                        writer: [9; 16],
                        base_sha256: None,
                    },
                    &baseline.encode().unwrap(),
                )
                .unwrap()
                .encode()
                .unwrap();
                store
                    .prepare_note_baseline_for_testing(
                        binding.id,
                        &name,
                        &envelope,
                        OpaqueBytes(b"synthetic-system-fields".to_vec()),
                    )
                    .unwrap();
            }
            binding.id
        })
    }
    fn open(&self) -> CoreHost {
        CoreHost::open(self.options.clone()).unwrap()
    }
    fn durable(&self) -> cloud::runtime::StoredRuntimeState {
        with_store(&self.options.collection_directory, self.scope(), |store| {
            store.stored_runtime_state(self.binding).unwrap().unwrap()
        })
    }
}
fn incomplete(runtime: &Runtime) {
    let status = runtime.status().unwrap();
    assert!(status.scan_required);
    assert!(!status.scan_drained);
    assert!(!status.scan_completed_this_open);
    assert!(runtime.require_collection_sending().is_err());
    assert!(wiring::require(meguri_engine::ports::ServicePort::Sync).is_err());
}

#[test]
fn optional_cloud_composition_captures_without_live_identity_and_keeps_sync_pending() {
    let fixture = Fixture::new(BindingState::Active);
    fixture.account.set(IdentityStatus::TemporarilyUnavailable);
    let process = ProcessCore::new(fixture.options.engine.clone()).unwrap();
    let host = process.open(fixture.options.clone()).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    assert!(
        runtime.status().unwrap().stored_usable,
        "real open-step bound the runtime"
    );
    runtime.refresh_identity().unwrap();
    assert!(!runtime.status().unwrap().identity_verified);
    let result = edit(&collection, fixture.ids[0], "unavailable account edit");
    let proof = runtime.capture_proof(result.seq, result.generation).unwrap();
    assert!(proof.attempted && proof.persisted);
    let tracked = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
    let actual = payload(&collection, fixture.ids[0]);
    assert_eq!(tracked.local_payload.0, actual.encode().unwrap());
    assert_eq!(tracked.last_observed_digest, Some(actual.row_digest().unwrap()));
    assert_eq!(tracked.acknowledged.payload.0, fixture.baselines[0].encode().unwrap());
    assert_eq!(
        tracked.acknowledged.row_digest,
        fixture.baselines[0].row_digest().unwrap()
    );
    assert_eq!(
        tracked.acknowledged.payload_digest,
        fixture.baselines[0].payload_digest().unwrap()
    );
    assert_eq!(tracked.base.unwrap().0, fixture.baselines[0].encode().unwrap());
    let mark = tracked.mark.unwrap();
    assert_eq!(mark.source, DirtySource::Observer);
    assert_eq!(mark.payload_digest, actual.payload_digest().unwrap());
    assert_eq!(mark.change_seq, 1);
    assert_eq!(tracked.local_rev, 1);
    assert_eq!(
        (tracked.observed_seq, tracked.observed_generation),
        (result.seq.0, result.generation.0)
    );
    assert_ne!(
        result.seq.0, mark.change_seq,
        "engine and binding counters have separate domains"
    );
    assert_eq!(runtime.status().unwrap().change_counter, 1);
    incomplete(runtime);
    assert!(runtime.reconcile_notes(&collection, BUDGET).is_err());
    host.close().unwrap();
    assert!(
        !runtime.status().unwrap().stored_usable,
        "real close invalidated the old binding"
    );
    assert!(!fixture.durable().drained);
    with_store(&fixture.options.collection_directory, fixture.scope(), |store| {
        let durable = store.tracked_note(fixture.binding, fixture.ids[0].0).unwrap().unwrap();
        assert_eq!(durable.local_payload.0, actual.encode().unwrap());
        assert_eq!(durable.local_rev, 1);
    });
    let reopened = process.open(fixture.options.clone()).unwrap();
    assert_ne!(reopened.collection().unwrap().generation(), collection.generation());
    let next = reopened.cloud_runtime().unwrap();
    assert_eq!(next.status().unwrap().change_counter, 1);
    incomplete(next);
    reopened.engine().suspend(BUDGET).unwrap();
    assert!(
        !next.status().unwrap().stored_usable,
        "real suspend port invalidated binding"
    );
    reopened.engine().resume().unwrap();
    assert!(next.status().unwrap().stored_usable);
    assert!(!next.status().unwrap().identity_verified);
    incomplete(next);
    reopened.close().unwrap();
}

#[test]
fn mid_capture_write_failure_rolls_back_every_tracking_row_but_not_collection_commit() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    let before: Vec<_> = fixture
        .ids
        .iter()
        .map(|id| runtime.tracked_note(id.0).unwrap().unwrap())
        .collect();
    let inputs: Vec<_> = fixture
        .ids
        .iter()
        .map(|id| update_input(&collection, *id, "atomic collection edit"))
        .collect();
    runtime.arm_capture_failure_for_testing(2).unwrap();
    let result = collection
        .run(
            OpSpec::named("runtime atomic capture fixture", OpSource::UserUndoable),
            move |ctx| {
                for input in &inputs {
                    meguri_engine::notes::update_in(ctx.scope(), input)?;
                }
                Ok(())
            },
        )
        .unwrap();
    let proof = runtime.capture_proof(result.seq, result.generation).unwrap();
    assert!(proof.attempted && !proof.persisted);
    for (index, id) in fixture.ids.iter().enumerate() {
        assert_ne!(
            payload(&collection, *id).encode().unwrap(),
            fixture.baselines[index].encode().unwrap()
        );
        assert_eq!(runtime.tracked_note(id.0).unwrap().unwrap(), before[index]);
    }
    assert_eq!(runtime.status().unwrap().change_counter, 0);
    assert!(runtime.status().unwrap().tracking_stopped);
    incomplete(runtime);
    // A later successful observer clears the local tracking stop, preserving scan-required.
    let later = edit(&collection, fixture.ids[0], "later real capture");
    assert!(runtime.capture_proof(later.seq, later.generation).unwrap().persisted);
    let tracked = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
    assert!(tracked.base.is_none(), "lost before-image cannot be invented from ack");
    assert_eq!(tracked.local_rev, 1);
    assert_eq!(tracked.mark.unwrap().change_seq, 1);
    assert!(!runtime.status().unwrap().tracking_stopped);
    incomplete(runtime);
    runtime.refresh_identity().unwrap();
    let scan = runtime.reconcile_notes(&collection, BUDGET).unwrap();
    assert_eq!(scan.recovered_names.len(), 2);
    for id in &fixture.ids[1..] {
        let repaired = runtime.tracked_note(id.0).unwrap().unwrap();
        let actual = payload(&collection, *id);
        assert_eq!(repaired.local_payload.0, actual.encode().unwrap());
        assert_eq!(repaired.last_observed_digest, Some(actual.row_digest().unwrap()));
        assert!(repaired.base.is_none());
        assert_eq!(repaired.local_rev, 1);
        assert_eq!(repaired.mark.as_ref().unwrap().source, DirtySource::Snapshot);
        assert_eq!(
            repaired.mark.as_ref().unwrap().payload_digest,
            actual.payload_digest().unwrap()
        );
    }
    assert_eq!(runtime.status().unwrap().change_counter, 3);
    incomplete(runtime);
    host.close().unwrap();
    assert!(!fixture.durable().drained);
}

fn completed<T>(receiver: &mpsc::Receiver<T>, event: &str) -> T {
    receiver
        .recv_timeout(BARRIER_TIMEOUT)
        .unwrap_or_else(|error| panic!("{event}: {error}"))
}

// Construct inside the scoped closure: disconnect on unwind before scope joins
// its workers, including a writer blocked behind a regressed scan lock.
struct PausedScan {
    reached: mpsc::Receiver<()>,
    resume: Option<mpsc::Sender<()>>,
}
impl PausedScan {
    fn arm(runtime: &Runtime, point: ScanPausePoint) -> Self {
        let (reached, wait) = mpsc::channel();
        let (resume, go) = mpsc::channel();
        runtime.arm_note_scan_pause_for_testing(point, reached, go).unwrap();
        Self {
            reached: wait,
            resume: Some(resume),
        }
    }
    fn wait(&self) {
        completed(&self.reached, "scan did not reach required pause");
    }
    fn resume(&mut self) {
        self.resume.take().unwrap().send(()).unwrap();
    }
}
impl Drop for PausedScan {
    fn drop(&mut self) {
        self.resume.take();
    }
}

#[test]
fn paused_snapshot_scan_allows_a_real_note_op_and_preserves_its_newer_capture() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    std::thread::scope(|threads| {
        let mut pause = PausedScan::arm(runtime, ScanPausePoint::BeforeMerge);
        let (scan_done, scan_result) = mpsc::channel();
        let scanning = &collection;
        threads.spawn(move || {
            let _ = scan_done.send(runtime.reconcile_notes(scanning, BUDGET));
        });
        pause.wait();
        // A real completion signal is required while the scan remains paused.
        let (edit_done, edit_result) = mpsc::channel();
        let editing = &collection;
        let id = fixture.ids[0];
        threads.spawn(move || {
            let _ = edit_done.send(edit(editing, id, "newer than snapshot"));
        });
        let edit = completed(&edit_result, "note edit blocked while scan paused");
        let captured = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
        assert!(runtime.capture_proof(edit.seq, edit.generation).unwrap().persisted);
        pause.resume();
        let proof = completed(&scan_result, "note scan did not finish").unwrap();
        assert!(proof.snapshot_seq < edit.seq);
        assert_eq!(
            proof.receipt.identity().native_file(),
            (proof.snapshot_native_file.device, proof.snapshot_native_file.inode)
        );
        assert_eq!(proof.receipt.identity().generation(), proof.snapshot_generation);
        assert!(proof.receipt.through() >= proof.snapshot_seq);
        assert_eq!(runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap(), captured);
    });
    assert!(runtime.status().unwrap().note_scan_completed_this_open);
    incomplete(runtime);
    host.close().unwrap();
    let durable = fixture.durable();
    assert!(durable.note_scan.is_some());
    assert!(!durable.drained);
}

#[test]
fn account_event_while_scan_paused_prevents_page_and_cursor_commit() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    let baseline = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
    std::thread::scope(|threads| {
        let mut pause = PausedScan::arm(runtime, ScanPausePoint::BeforeMerge);
        let (scan_done, scan_result) = mpsc::channel();
        let scanning = &collection;
        threads.spawn(move || {
            let _ = scan_done.send(runtime.reconcile_notes(scanning, BUDGET));
        });
        pause.wait();
        fixture
            .account
            .set(IdentityStatus::Available(OpaqueBytes(b"different-account".to_vec())));
        runtime.refresh_identity().unwrap();
        pause.resume();
        assert!(matches!(
            completed(&scan_result, "account-invalidated scan did not finish"),
            Err(RuntimeError::StaleIncarnation)
        ));
    });
    assert_eq!(runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap(), baseline);
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        BindingState::Isolated(IsolationReason::AccountChanged)
    );
    assert!(runtime.note_scan_proof().is_none());
    incomplete(runtime);
    host.close().unwrap();
    assert!(fixture.durable().note_scan.is_none());
}

#[test]
fn suspend_resume_during_pre_snapshot_pause_invalidates_same_generation_scan() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    std::thread::scope(|threads| {
        let mut pause = PausedScan::arm(runtime, ScanPausePoint::BeforeSnapshot);
        let (scan_done, scan_result) = mpsc::channel();
        let scanning = &collection;
        threads.spawn(move || {
            let _ = scan_done.send(runtime.reconcile_notes(scanning, BUDGET));
        });
        pause.wait();
        assert_eq!(host.engine().suspend(BUDGET).unwrap(), SuspendOutcome::Suspended);
        host.engine().resume().unwrap();
        runtime.refresh_identity().unwrap();
        pause.resume();
        assert!(matches!(
            completed(&scan_result, "old-incarnation scan did not finish"),
            Err(RuntimeError::StaleIncarnation)
        ));
    });
    assert!(runtime.note_scan_proof().is_none());
    incomplete(runtime);
    host.close().unwrap();
    assert!(fixture.durable().note_scan.is_none());
}

#[test]
fn creation_fact_mismatch_during_pause_prevents_scan_completion() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let collection = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    std::thread::scope(|threads| {
        let mut pause = PausedScan::arm(runtime, ScanPausePoint::BeforeMerge);
        let (scan_done, scan_result) = mpsc::channel();
        let scanning = &collection;
        threads.spawn(move || {
            let _ = scan_done.send(runtime.reconcile_notes(scanning, BUDGET));
        });
        pause.wait();
        *fixture.files.0.lock().unwrap() = true;
        pause.resume();
        assert!(matches!(
            completed(&scan_result, "file-invalidated scan did not finish"),
            Err(RuntimeError::FileIdentityUnavailable)
        ));
    });
    incomplete(runtime);
    host.close().unwrap();
    assert!(fixture.durable().note_scan.is_none());
    let reopened = fixture.open();
    let next = reopened.cloud_runtime().unwrap();
    assert_eq!(
        next.status().unwrap().binding.unwrap().facts.state,
        BindingState::Isolated(IsolationReason::FileMismatch)
    );
    incomplete(next);
    reopened.close().unwrap();
    assert!(!fixture.durable().drained);
}

#[test]
fn isolation_account_precedence_preserves_pending_and_permanent_reasons() {
    for (before, expected) in [
        (IsolationReason::ZoneMissing, IsolationReason::AccountChanged),
        (IsolationReason::SignedOut, IsolationReason::AccountChanged),
        (IsolationReason::FileMismatch, IsolationReason::FileMismatch),
        (IsolationReason::ReplacedLocally, IsolationReason::ReplacedLocally),
    ] {
        let fixture = Fixture::new(BindingState::Active);
        let active = fixture.open();
        let collection = active.collection().unwrap();
        edit(&collection, fixture.ids[0], "pending before isolation");
        let pending = active
            .cloud_runtime()
            .unwrap()
            .tracked_note(fixture.ids[0].0)
            .unwrap()
            .unwrap();
        assert!(pending.mark.is_some());
        active.close().unwrap();
        with_store(&fixture.options.collection_directory, fixture.scope(), |store| {
            store
                .isolate_runtime_binding_for_testing(fixture.binding, before)
                .unwrap();
        });
        let host = fixture.open();
        let runtime = host.cloud_runtime().unwrap();
        fixture
            .account
            .set(IdentityStatus::Available(OpaqueBytes(b"later-account".to_vec())));
        runtime.refresh_identity().unwrap();
        assert_eq!(
            runtime.status().unwrap().binding.unwrap().facts.state,
            BindingState::Isolated(expected)
        );
        assert_eq!(runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap(), pending);
        assert!(!runtime.status().unwrap().stored_usable);
        incomplete(runtime);
        host.close().unwrap();
    }
}

#[test]
fn malformed_other_build_runtime_payloads_do_not_block_current_build_but_future_version_does() {
    for other_scope in [
        scope("test.other-runtime", Environment::Development),
        scope("test.runtime", Environment::Production),
    ] {
        let fixture = Fixture::new(BindingState::Active);
        let other = fixture.seed_scope(other_scope.clone(), BindingState::Active, false);
        let mut other_options = fixture.options.clone();
        other_options.cloud.as_mut().unwrap().scope = other_scope.clone();
        let other_host = CoreHost::open(other_options).unwrap();
        assert!(other_host.cloud_runtime().unwrap().status().unwrap().stored_usable);
        other_host.close().unwrap();
        with_store(&fixture.options.collection_directory, other_scope.clone(), |store| {
            store.damage_runtime_payload_for_testing(other, None).unwrap();
            store
                .damage_runtime_payload_for_testing(other, Some(fixture.ids[0].0))
                .unwrap();
        });
        let host = fixture.open();
        let runtime = host.cloud_runtime().unwrap();
        assert!(runtime.status().unwrap().stored_usable);
        let collection = host.collection().unwrap();
        let edit = edit(&collection, fixture.ids[0], "current build capture");
        assert!(runtime.capture_proof(edit.seq, edit.generation).unwrap().persisted);
        host.close().unwrap();
        with_store(&fixture.options.collection_directory, other_scope, |store| {
            assert!(store.stored_runtime_state(other).is_err());
            assert!(store.tracked_note(other, fixture.ids[0].0).is_err());
        });
        with_store(&fixture.options.collection_directory, fixture.scope(), |store| {
            store.future_runtime_version_for_testing().unwrap();
        });
        let refused = with_tracking_access(|| {
            matches!(
                TrackingStore::open(&fixture.options.collection_directory, fixture.scope()),
                Err(cloud::Error::RuntimeSchemaUnsupported)
            )
        });
        assert!(refused);
        let local = fixture.open();
        let stopped = local.cloud_runtime().unwrap();
        assert!(stopped.status().unwrap().tracking_stopped);
        assert!(stopped.status().unwrap().binding.is_none());
        incomplete(stopped);
        local.close().unwrap();
    }
}

#[test]
fn publication_mismatch_is_permanent_and_never_allows_scan_or_sending() {
    let fixture = Fixture::new(BindingState::Active);
    let other_scope = scope("test.publication-mismatch", Environment::Development);
    let binding = fixture.seed_scope(other_scope.clone(), BindingState::Active, true);
    let mut options = fixture.options.clone();
    options.cloud.as_mut().unwrap().scope = other_scope.clone();
    let host = CoreHost::open(options).unwrap();
    let runtime = host.cloud_runtime().unwrap();
    let baseline = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
    runtime.refresh_identity().unwrap();
    assert_eq!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        BindingState::Isolated(IsolationReason::ReplacedLocally)
    );
    let collection = host.collection().unwrap();
    edit(&collection, fixture.ids[0], "local publication mismatch edit");
    assert_eq!(runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap(), baseline);
    assert!(runtime.reconcile_notes(&collection, BUDGET).is_err());
    incomplete(runtime);
    host.close().unwrap();
    with_store(&fixture.options.collection_directory, other_scope, |store| {
        let durable = store.stored_runtime_state(binding).unwrap().unwrap();
        assert!(!durable.drained);
        assert!(durable.note_scan.is_none());
    });
}

#[test]
fn restored_collection_runs_real_generation_listener_and_invalidates_old_capture_binding() {
    let fixture = Fixture::new(BindingState::Active);
    let host = fixture.open();
    let runtime = host.cloud_runtime().unwrap();
    let old = host.collection().unwrap();
    runtime.refresh_identity().unwrap();
    let receipt = host
        .backup()
        .before_colpkg_import(&old, &meguri_engine::CancelToken::new())
        .unwrap();
    edit(&old, fixture.ids[0], "pending before restore");
    let pending = runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap();
    host.restore_backup(
        &receipt.name,
        &meguri_engine::CancelToken::new(),
        &meguri_engine::ProgressSink::new(),
    )
    .unwrap();
    let current = host.collection().unwrap();
    assert_ne!(current.generation(), old.generation());
    assert_eq!(runtime.status().unwrap().generation, Some(current.generation()));
    assert!(!runtime.status().unwrap().identity_verified);
    assert!(!runtime.status().unwrap().stored_usable);
    assert!(matches!(
        runtime.status().unwrap().binding.unwrap().facts.state,
        BindingState::Isolated(IsolationReason::ReplacedLocally | IsolationReason::FileMismatch)
    ));
    assert_eq!(runtime.tracked_note(fixture.ids[0].0).unwrap().unwrap(), pending);
    assert!(runtime.reconcile_notes(&old, BUDGET).is_err());
    incomplete(runtime);
    host.close().unwrap();
    assert!(!fixture.durable().drained);
}

#[test]
fn delayed_media_notification_cannot_stop_a_reopened_runtime() {
    use meguri_engine::media::MediaFeedConsumer;
    let fixture = Fixture::new(BindingState::Active);
    let process = ProcessCore::new(fixture.options.engine.clone()).unwrap();
    let host = process.open(fixture.options.clone()).unwrap();
    let old_collection = host.collection().unwrap();
    let old_media = host.media().unwrap();
    let issued = old_collection.generation();
    host.close().unwrap();
    let reopened = process.open(fixture.options.clone()).unwrap();
    let current = reopened.collection().unwrap();
    let media = reopened.media().unwrap();
    let runtime = reopened.cloud_runtime().unwrap();
    runtime.refresh_identity().unwrap();
    assert_ne!(issued, current.generation());
    let before = runtime.status().unwrap();
    assert!(before.stored_usable);
    assert!(before.identity_verified);
    assert!(!before.tracking_stopped);
    runtime.committed(&old_collection, &old_media, issued);
    runtime.committed(&current, &media, issued);
    let after = runtime.status().unwrap();
    assert_eq!(after.generation, before.generation);
    assert_eq!(after.stored_usable, before.stored_usable);
    assert_eq!(after.identity_verified, before.identity_verified);
    assert_eq!(after.tracking_stopped, before.tracking_stopped);
    assert_eq!(after.change_counter, before.change_counter);
    media.write_exact("callback.bin", b"current generation file").unwrap();
    assert!(!runtime.status().unwrap().tracking_stopped);
    reopened.close().unwrap();
}
