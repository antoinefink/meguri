//! W12's local writer guard, bootstrap generation changes and retained setup.
use super::{b, r, s};
use meguri_engine::{CancelToken, EngineError, Generation, ProgressSink, UndoScope,
    notetypes::{self, remove::{self, RemoveNotetype}},
    undo::UndoStatus};
use meguri_host::{CoreHost, HostConfig, MeguriBootstrapDirection,
    MeguriStructureOutcome, MeguriStructureStop, MeguriStructureUploadOutcome, MeguriUploadPending};
use meguri_model::ids::NotetypeId;
use meguri_storage::{ReadRows, ReaderRole, raw::{RawRows, SqlValue}};
use meguri_sync::{Method, policy::{Operation, Phase}, provider::{MeguriConnection, MeguriReadiness, Selection}};
use std::{path::{Path, PathBuf}, sync::{Arc, Mutex}, time::Duration};

fn request(notetype: NotetypeId) -> RemoveNotetype {
    RemoveNotetype { notetype, confirmed_notes: 1, confirmed_cards: 1 }
}
fn remove_local(host: &CoreHost, notetype: NotetypeId) -> Result<meguri_engine::OpResult<remove::Removed>, EngineError> {
    host.remove_notetype_locally_for_generation(host.collection().unwrap().generation(), request(notetype),
        CancelToken::new(), ProgressSink::new())
}
fn transfer_after_unbound_read(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation, NotetypeId, Generation) {
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
    assert!(op.collection_complete && op.receipt.is_none(), "{op:?}");
    assert_ne!(host.collection().unwrap().generation(), generation, "bootstrap activation advances the generation");
    (host, config, connection, op, notetype, generation)
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
    snapshot_facts(host, config)
}
fn snapshot_facts(host: &CoreHost, config: &HostConfig) -> Facts {
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
fn outcome_details(outcome: &MeguriStructureOutcome) -> String {
    match outcome {
        MeguriStructureOutcome::Saved(upload) => format!("Saved({upload:?})"),
        MeguriStructureOutcome::NotSaved(stop) => match stop {
            MeguriStructureStop::Refused(error) => format!("NotSaved(Refused({error:?}))"),
            MeguriStructureStop::Round { result, .. } => format!(
                "NotSaved(Round: recovery={:?}, collection={:?}, refusal={:?}, error={:?}, acceptance_unknown={:?})",
                result.recovery, result.collection.as_ref().map(|c| c.state),
                result.collection.as_ref().and_then(|c| c.refusal),
                result.collection.as_ref().and_then(|c| c.error.as_ref()),
                result.collection.as_ref().map(|c| c.acceptance_unknown)),
            MeguriStructureStop::FullSyncRequired => "NotSaved(FullSyncRequired)".into(),
            MeguriStructureStop::Cancelled => "NotSaved(Cancelled)".into(),
            MeguriStructureStop::Unavailable => "NotSaved(Unavailable)".into(),
        },
    }
}

#[test]
fn bootstrap_after_unbound_read_refuses_local_removal_from_the_previous_generation() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op, notetype, observed) = transfer_after_unbound_read(root.path());
    let op = b::finish_empty_media(&host, &connection, &op);
    let generation = host.collection().unwrap().generation();
    // This is the real UI interleaving: the .other read preceded setup.
    // Its generation is stale even before the final binding publication.
    let before = facts(&host, &config);
    let refused = host.remove_notetype_locally_for_generation(observed, request(notetype),
        CancelToken::new(), ProgressSink::new());
    assert!(matches!(&refused, Err(EngineError::StaleGeneration { planned, current })
        if *planned == observed && *current == generation), "local removal: {:?}", refused.err());
    assert_eq!(facts(&host, &config), before);
    host.complete_meguri_bootstrap_for_generation(generation, &connection, b::USER, &op.id).unwrap();
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
    // Keep nonempty undo AND redo so refusing cannot clear either silently.
    let collection = host.collection().unwrap();
    let stored = collection.reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().unwrap();
    let mut draft = notetypes::Draft::of(stored);
    draft.config.css.push_str("\n/* Removal sentinel */");
    notetypes::save(&collection, draft, Default::default()).unwrap();
    s::add_note(&host, notetype, "Redo sentinel", "Answer");
    collection.undo(UndoScope::Global).unwrap();
    let before = facts(&host, &config);
    assert!(matches!(before.undo, UndoStatus::Available(_)) && matches!(before.redo, UndoStatus::Available(_)));
    let refused = host.remove_notetype_locally_for_generation(generation, request(notetype),
        CancelToken::new(), ProgressSink::new());
    assert!(matches!(&refused, Err(EngineError::Busy { .. })), "bound local removal: {:?}", refused.err());
    assert_eq!(facts(&host, &config), before);
    host.close().unwrap();
}

#[test]
fn reopened_bootstrap_retains_setup_guards_until_same_generation_binding_publication() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op, notetype, _) = transfer_after_unbound_read(root.path());
    host.close().unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    b::settle(&host);
    let generation = host.collection().unwrap().generation();
    assert_eq!(host.provider_authority().snapshot().unwrap().meguri.as_ref(), Some(&connection));
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Unbound { intent: Some(id) } if id == op.id));
    assert_eq!(host.pending_meguri_bootstrap_preparation(&connection, b::USER).unwrap().as_ref(), Some(&op));
    assert!(host.meguri_setup_transfer_holds_admission());

    let op = b::finish_empty_media(&host, &connection, &op);
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(host.pending_meguri_bootstrap_preparation(&connection, b::USER).unwrap().is_none());
    // An inactive policy row is not genuinely unbound. The facade resolves
    // this retained intent to the Complete receipt; noteTypeDeletionService's
    // meguriRetainsEffects guard refuses .other, even with no pending row.
    assert_eq!(op.phase, Phase::Complete, "{op:?}");
    assert!(op.collection_complete && op.receipt.is_some(), "{op:?}");
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Unbound { intent: Some(id) } if id == op.id));
    assert_eq!(host.manual_transfer_history().get(&op.id).unwrap(), op);
    assert!(!host.meguri_structure_upload_pending().unwrap());
    assert!(host.complete_meguri_bootstrap_for_generation(generation, &connection, b::USER, &op.id).unwrap().is_complete());
    assert_eq!(host.collection().unwrap().generation(), generation);
    assert!(matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready));
    assert_eq!(host.provider_authority().snapshot().unwrap().meguri_binding.unwrap().operation(), &op.id);
    let before = facts(&host, &config);
    let refused = remove_local(&host, notetype);
    assert!(matches!(&refused, Err(EngineError::Busy { .. })), "bound local removal: {:?}", refused.err());
    assert_eq!(facts(&host, &config), before);
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
        let generation = rig.host.collection().unwrap().generation();
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
        let details = format!("reachable={reachable}, {}, generation={generation:?}->{:?}, readiness={:?}, active={:?}",
            outcome_details(&outcome), rig.host.collection().unwrap().generation(), rig.host.meguri_readiness(),
            rig.host.manual_transfer_history().active());
        let removed = saved.lock().unwrap().expect(&details);
        assert!(removed.removed && removed.notes_removed == 1 && removed.cards_removed == 1, "{details}: {removed:?}");
        assert!(rig.host.collection().unwrap().reader(ReaderRole::Query).unwrap().notetype(rig.notetype).unwrap().is_none());
        if reachable {
            assert!(matches!(*outcome, MeguriStructureOutcome::Saved(MeguriStructureUploadOutcome::Uploaded { media_complete: true })), "{details}");
            assert!(matches!(rig.host.meguri_readiness().unwrap(), MeguriReadiness::Ready), "{details}");
            assert_ne!(rig.host.collection().unwrap().generation(), generation, "{details}");
            assert!(!rig.host.meguri_structure_upload_pending().unwrap(), "{details}");
            assert_eq!(script.bodies(Method::Download).len(), 1, "{details}");
            assert_eq!(script.bodies(Method::Upload).len(), 1, "{details}");
            assert_eq!(script.bodies(Method::MediaBegin).len(), 1, "{details}");
            assert_eq!(rig.host.collection().unwrap().undo_depth_for_testing().unwrap(), 0, "{details}");
        } else {
            assert!(matches!(*outcome, MeguriStructureOutcome::Saved(
                MeguriStructureUploadOutcome::Pending(MeguriUploadPending::Unreachable))), "{details}");
            assert_eq!(rig.host.collection().unwrap().generation(), generation, "{details}");
            assert!(rig.host.meguri_structure_upload_pending().unwrap(), "{details}");
            rig.host.collection().unwrap().undo(UndoScope::Global).unwrap();
            assert!(rig.host.collection().unwrap().reader(ReaderRole::Query).unwrap().notetype(rig.notetype).unwrap().is_some(), "{details}");
            assert!(!rig.host.meguri_structure_upload_pending().unwrap(), "{details}");
        }
        script.assert_done();
        rig.host.close().unwrap();
    }
}
