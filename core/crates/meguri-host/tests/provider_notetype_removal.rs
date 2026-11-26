//! W12's native local admission versus MP5's same-generation binding install.
use super::{b, r, s};
use meguri_engine::{CancelToken, EngineError, ProgressSink, UndoScope,
    notetypes::{self, remove::{self, RemoveNotetype}},
    progress::{Progress, ProgressListener, ProgressPhase}, undo::UndoStatus};
use meguri_host::{CoreHost, HostConfig, MeguriBootstrapDirection, MeguriBootstrapError,
    MeguriStructureOutcome, MeguriStructureUploadOutcome, MeguriUploadPending};
use meguri_model::ids::NotetypeId;
use meguri_storage::{ReadRows, ReaderRole, raw::{RawRows, SqlValue}};
use meguri_sync::{policy::Operation, provider::{MeguriConnection, MeguriReadiness, Selection}};
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex, mpsc}, time::Duration};

fn request(notetype: NotetypeId) -> RemoveNotetype {
    RemoveNotetype { notetype, confirmed_notes: 1, confirmed_cards: 1 }
}
fn remove_local(host: &CoreHost, notetype: NotetypeId) -> Result<meguri_engine::OpResult<remove::Removed>, EngineError> {
    host.remove_notetype_locally_for_generation(host.collection().unwrap().generation(), request(notetype),
        Arc::new(CancelToken::new()), ProgressSink::new())
}
fn unbound_after_transfer(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation, NotetypeId) {
    let (host, config) = b::seeded(&root.join("local"));
    let notetype = s::basic(&host);
    s::add_note(&host, notetype, "Removal sentinel", "Answer sentinel");
    let connection = b::select(&host);
    let generation = host.collection().unwrap().generation();
    // These are the actual facts that let noteTypeDeletionService return
    // .other: selected, configured, genuinely unbound, with no retained work.
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Unbound { intent: None }));
    assert!(host.pending_meguri_bootstrap_preparation(&connection, b::USER).unwrap().is_none());
    assert!(!host.meguri_structure_upload_pending().unwrap());
    let (_, data) = b::server(&root.join("server"));
    let script = b::collection_script(data, true);
    let preview = b::prepare(&host, &connection, &script, MeguriBootstrapDirection::UseDeviceCollection);
    let op = b::confirm(&host, &connection, &script, preview);
    let op = b::finish_empty_media(&host, &connection, &op);
    assert_eq!(host.collection().unwrap().generation(), generation);
    (host, config, connection, op, notetype)
}
fn files(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut found = Vec::new();
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() { found.extend(files(&entry.unwrap().path())); }
    } else if path.is_file() { found.push((path.to_owned(), std::fs::read(path).unwrap())); }
    found.sort_by(|a, b| a.0.cmp(&b.0));
    found
}
#[derive(Debug, PartialEq)]
struct Facts {
    rows: Vec<(String, RawRows)>, seq: u64, undo: UndoStatus, redo: UndoStatus,
    retained: Vec<(PathBuf, Vec<u8>)>,
}
fn facts(host: &CoreHost, config: &HostConfig) -> Facts {
    assert!(host.derived().wait_idle(Duration::from_secs(5)));
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    let tables = reader.execute_raw("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name", &[]).unwrap();
    let rows = tables.rows.iter().map(|row| {
        let SqlValue::Text(name) = &row[0] else { panic!("table name") };
        (name.clone(), reader.execute_raw(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")), &[]).unwrap())
    }).collect();
    Facts { rows, seq: collection.last_seq().0,
        undo: collection.peek_undo(UndoScope::Global).unwrap(), redo: collection.peek_redo(UndoScope::Global).unwrap(),
        retained: [files(&config.backups_directory), files(&config.manual_transfer_directory)].concat() }
}

#[test]
fn binding_installed_after_unbound_read_refuses_local_removal_in_the_same_generation() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op, notetype) = unbound_after_transfer(root.path());
    let generation = host.collection().unwrap().generation();
    // The helper observed a genuinely unbound service before setup began.
    // Complete real MP5 before the exact host entry used by the local FFI
    // removal worker, all within that same collection generation.
    host.complete_meguri_bootstrap_for_generation(generation, &connection, b::USER, &op.id).unwrap();
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
    // Keep nonempty undo AND redo so refusing cannot clear either silently.
    let collection = host.collection().unwrap();
    let stored = collection.reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().unwrap();
    let mut draft = notetypes::Draft::of(stored);
    draft.css.push_str("\n/* Removal sentinel */");
    notetypes::save(&collection, draft, Default::default()).unwrap();
    s::add_note(&host, notetype, "Redo sentinel", "Answer");
    collection.undo(UndoScope::Global).unwrap();
    let before = facts(&host, &config);
    assert!(matches!(before.undo, UndoStatus::Available(_)) && matches!(before.redo, UndoStatus::Available(_)));
    assert!(matches!(host.remove_notetype_locally_for_generation(generation, request(notetype),
        Arc::new(CancelToken::new()), ProgressSink::new()), Err(EngineError::Busy { .. })));
    assert_eq!(facts(&host, &config), before);
    host.close().unwrap();
}

struct HoldPlanning(Mutex<Option<(mpsc::Sender<()>, mpsc::Receiver<()>)>>);
impl ProgressListener for HoldPlanning {
    fn on_progress(&self, progress: &Progress) {
        if progress.phase == ProgressPhase::Planning && progress.completed == 1
            && let Some((entered, release)) = self.0.lock().unwrap().take() {
            entered.send(()).unwrap();
            // A dropped sender also releases the worker on an assertion failure.
            let _ = release.recv_timeout(Duration::from_secs(20));
        }
    }
}

#[test]
fn local_removal_holds_binding_installation_until_its_real_commit() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op, notetype) = unbound_after_transfer(root.path());
    let host = Arc::new(host);
    let generation = host.collection().unwrap().generation();
    let before = facts(&host, &config);
    let progress = ProgressSink::new();
    let (entered, wait) = mpsc::channel();
    let (release, held) = mpsc::channel();
    progress.set_listener(Arc::new(HoldPlanning(Mutex::new(Some((entered, held))))));
    let original = host.clone();
    let worker = std::thread::spawn(move || original.remove_notetype_locally_for_generation(generation,
        request(notetype), Arc::new(CancelToken::new()), progress));
    let reached = wait.recv_timeout(Duration::from_secs(10));
    // The removal has real native admission and is paused in the engine's
    // plan, before backup/commit; no fabricated flags or fake binding setter.
    let binding = host.complete_meguri_bootstrap_for_generation(generation, &connection, b::USER, &op.id);
    let during = facts(&host, &config);
    drop(release);
    let removed = worker.join().unwrap().unwrap();
    assert!(reached.is_ok());
    assert!(matches!(binding, Err(MeguriBootstrapError::Busy)));
    assert_eq!(during, before);
    assert!(removed.value.removed && removed.value.notes_removed == 1 && removed.value.cards_removed == 1);
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(host.provider_authority().snapshot().unwrap().meguri_binding.is_none());
    // Completion releases the lane; an actual metadata action can run again.
    host.configure_meguri_connection(b::ENDPOINT).unwrap();
    host.close().unwrap();
}

#[test]
fn true_off_and_selected_unbound_removals_still_commit_with_retained_configuration() {
    let _lane = b::LANE.lock().unwrap();
    for mode in ["off", "off with config", "selected unbound"] {
        let root = tempfile::tempdir().unwrap();
        let (seeded, config) = b::seeded(root.path());
        seeded.close().unwrap();
        let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
        let host = process.open(config.clone()).unwrap();
        if mode == "selected unbound" { b::select(&host); }
        else if mode == "off with config" { host.configure_meguri_connection(b::ENDPOINT).unwrap(); }
        let doc = host.provider_authority().snapshot().unwrap();
        assert_eq!(doc.selection, if mode == "selected unbound" { Selection::Meguri } else { Selection::LocalOnly });
        assert!(doc.meguri_binding.is_none());
        assert_eq!(doc.meguri.is_some(), mode != "off");
        // The old connection config survives a fresh core generation; it
        // must not turn Off or a genuinely unbound selection into MP7.
        let previous = host.collection().unwrap().generation();
        host.close().unwrap();
        let host = process.open(config).unwrap();
        assert_ne!(host.collection().unwrap().generation(), previous);
        assert_eq!(host.provider_authority().snapshot().unwrap(), doc);
        let notetype = s::basic(&host);
        s::add_note(&host, notetype, "Local sentinel", "Answer");
        let removed = remove_local(&host, notetype).unwrap();
        assert!(removed.value.removed && removed.value.notes_removed == 1 && removed.value.cards_removed == 1, "{mode}");
        host.collection().unwrap().undo(UndoScope::Global).unwrap();
        assert!(host.collection().unwrap().reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().is_some());
        host.close().unwrap();
    }
}

#[test]
fn bound_removal_still_runs_mp7_online_and_offline() {
    let _lane = b::LANE.lock().unwrap();
    for reachable in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let rig = s::rig(root.path(), 1);
        let mut steps = Vec::new();
        if reachable {
            steps.extend(s::quiet_round(&rig.host));
            steps.extend(s::upload_steps(root.path(), &s::col(&rig.host)));
        }
        let script = r::Script::new(steps);
        let saved = Arc::new(Mutex::new(None));
        let output = saved.clone();
        let input = request(rig.notetype);
        let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), Box::new(move |collection, job| {
            *output.lock().unwrap() = Some(remove::remove(collection, input, job.cancel.clone(), job.progress.clone())?.value);
            Ok(())
        }), reachable).unwrap();
        let outcome = s::drive(&rig.host, &task);
        assert!(saved.lock().unwrap().as_ref().unwrap().removed);
        assert!(rig.host.collection().unwrap().reader(ReaderRole::Query).unwrap().notetype(rig.notetype).unwrap().is_none());
        if reachable {
            assert!(matches!(*outcome, MeguriStructureOutcome::Saved(MeguriStructureUploadOutcome::Uploaded { media_complete: true })));
            assert!(matches!(rig.host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
        } else {
            assert!(matches!(*outcome, MeguriStructureOutcome::Saved(
                MeguriStructureUploadOutcome::Pending(MeguriUploadPending::Unreachable))));
            assert!(rig.host.meguri_structure_upload_pending().unwrap());
        }
        script.assert_done();
        rig.host.close().unwrap();
    }
}
