//! The MP7 structure save job killed at each point between its round, its
//! save and its upload. Its own binary: it re-runs itself with `--exact
//! <test>`, which needs an unprefixed name. Every kill must leave one of:
//! nothing saved, or the save with a durable pending upload.
#[allow(dead_code)]
#[path = "support/meguri_bootstrap_script.rs"] mod bootstrap;
#[allow(dead_code)]
#[path = "support/meguri_round_script.rs"] mod round;
#[allow(dead_code)]
#[path = "support/meguri_structure.rs"] mod structure;
use bootstrap as b;
use round as r;
use structure as s;
use meguri_engine::UndoScope;
use meguri_host::{CoreHost, HostConfig, MeguriCollectionState, MeguriRoundStart, MeguriStructureOutcome as O,
    MeguriStructureUploadOutcome as U, MeguriStructureWork};
use meguri_model::ids::NotetypeId;
use meguri_sync::{Method, policy::Acceptance, provider::{MeguriConnection, MeguriReadiness, MeguriRoundRefusal}};
use std::sync::Arc;

const ROOT: &str = "MEGURI_STRUCTURE_KILL_ROOT";
fn die() -> Box<dyn FnOnce() + Send> { Box::new(|| std::process::abort()) }

/// The child: reopens the ready device, runs the job and dies at `point`.
fn child(point: &str) {
    let Ok(root) = std::env::var(ROOT) else { return };
    let root = std::path::PathBuf::from(root);
    let var = |name: &str| std::env::var(format!("MEGURI_STRUCTURE_KILL_{name}")).unwrap();
    let host = CoreHost::open(s::config(&root.join("local"), s::LATER)).unwrap();
    b::settle(&host);
    let connection = MeguriConnection { reference: var("REFERENCE"), revision: var("REVISION").parse().unwrap(),
        endpoint: b::ENDPOINT.into() };
    let notetype = NotetypeId(var("NOTETYPE").parse().unwrap());
    let col = s::col(&host);
    let mut steps = s::quiet_round(&host);
    steps.extend(s::upload_steps(&root, col.last_sync.0, col.schema_modified.0));
    // Steps: 0 the round; 1 the facts; 2, 3 the server copy; 4, 5 its reads; 6 the upload; 7 the media.
    let at = match point { "round" => Some(0), "facts" => Some(1), "server-copy" => Some(3), "upload" => Some(6),
        "media" => Some(7), _ => None };
    if let Some(at) = at { steps[at].after = Some(die()); }
    let script = r::Script::new(steps);
    let (save, _) = s::save_work(s::draft_without_back(&host, notetype, 2));
    let work: MeguriStructureWork = match point {
        // Killed after the record was written and before the op began.
        "before-save" => Box::new(|_, _| std::process::abort()),
        // Killed once the save committed, before anything of the upload.
        "after-save" => Box::new(move |collection, job| { save(collection, job)?; std::process::abort() }),
        _ => save,
    };
    let task = host.start_meguri_structure_save(s::source(&connection, &script), work, true).unwrap();
    let _ = s::drive(&host, &task);
    panic!("the job must die at {point}");
}
struct Killed { _root: tempfile::TempDir, host: CoreHost, connection: MeguriConnection, notetype: NotetypeId,
    path: std::path::PathBuf, ls: i64, scm: i64 }
/// Builds the ready device, kills a child in the job at the point the
/// calling test names, and reopens the device as a relaunch does.
fn kill(test: &str) -> Killed {
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let before = s::col(&rig.host);
    let config: HostConfig = rig.config.clone();
    let Ok(host) = Arc::try_unwrap(rig.host) else { panic!("the rig owns its host") };
    host.close().unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["--exact", test, "--nocapture"])
        .env(ROOT, root.path())
        .env("MEGURI_STRUCTURE_KILL_REFERENCE", &rig.connection.reference)
        .env("MEGURI_STRUCTURE_KILL_REVISION", rig.connection.revision.to_string())
        .env("MEGURI_STRUCTURE_KILL_NOTETYPE", rig.notetype.0.to_string())
        .status().unwrap();
    // The child died by its own abort at the named point, not by a failed setup.
    assert_eq!(std::os::unix::process::ExitStatusExt::signal(&status), Some(6), "SIGABRT");
    let host = CoreHost::open(config).unwrap();
    b::settle(&host);
    Killed { path: root.path().to_owned(), _root: root, host, connection: rig.connection, notetype: rig.notetype,
        ls: before.last_sync.0, scm: before.schema_modified.0 }
}
fn ready(host: &CoreHost) -> bool { matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready) }
fn record(host: &CoreHost) -> bool { host.provider_authority().snapshot().unwrap().meguri_structure.is_some() }
fn upload(k: &Killed, steps: Vec<r::Step>) -> (Arc<O>, Arc<r::Script>) {
    let script = r::Script::new(steps);
    let task = k.host.start_meguri_structure_upload(s::source(&k.connection, &script)).unwrap();
    (s::drive(&k.host, &task), script)
}
/// Nothing saved: the note type is whole, nothing is pending, the binding is
/// ready, and the same save runs again from its round to its upload.
fn assert_nothing_saved(k: &Killed) {
    assert_eq!(s::field_names(&k.host, k.notetype), ["Front", "Back"]);
    assert!(!k.host.meguri_structure_upload_pending().unwrap());
    assert!(!record(&k.host), "a record whose save never committed is withdrawn at open");
    assert!(ready(&k.host));
    assert!(k.host.manual_transfer_history().active().unwrap().is_none());
    let col = s::col(&k.host);
    assert!(!meguri_engine::schema_gate::is_modified(&col));
    let mut steps = s::quiet_round(&k.host);
    steps.extend(s::upload_steps(&k.path, col.last_sync.0, col.schema_modified.0));
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&k.host, k.notetype, 2));
    let task = k.host.start_meguri_structure_save(s::source(&k.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&k.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert_eq!(s::field_names(&k.host, k.notetype), ["Front"]);
}
/// Saved and pending: the save is on disk with its record, no round runs,
/// and nothing was sent.
fn assert_saved_and_pending(k: &Killed) {
    assert_eq!(s::field_names(&k.host, k.notetype), ["Front"]);
    assert!(record(&k.host) && k.host.meguri_structure_upload_pending().unwrap());
    assert!(meguri_engine::schema_gate::is_modified(&s::col(&k.host)));
    let silent = r::Script::new(vec![]);
    assert!(matches!(k.host.start_meguri_collection(r::access(&k.connection, &silent)), MeguriRoundStart::Refused(_)));
}

#[test]
fn structure_job_killed_during_its_round_saved_nothing() {
    child("round");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_during_its_round_saved_nothing");
    assert_nothing_saved(&k);
    k.host.close().unwrap();
}
#[test]
fn structure_job_killed_between_its_record_and_its_save_saved_nothing() {
    child("before-save");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_between_its_record_and_its_save_saved_nothing");
    assert_nothing_saved(&k);
    k.host.close().unwrap();
}
#[test]
fn structure_job_killed_after_its_save_leaves_the_upload_pending() {
    child("after-save");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_after_its_save_leaves_the_upload_pending");
    assert_saved_and_pending(&k);
    assert!(ready(&k.host), "the binding is held only once the upload runs");
    // The relaunch dropped the undo step; the backup of the save stays (D7).
    assert_eq!(k.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), None);
    let (outcome, script) = upload(&k, s::upload_steps(&k.path, k.ls, k.scm));
    assert!(matches!(*outcome, O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert!(ready(&k.host) && !record(&k.host) && !k.host.meguri_structure_upload_pending().unwrap());
    k.host.close().unwrap();
}
#[test]
fn structure_job_killed_while_reading_the_server_facts_leaves_the_upload_pending() {
    child("facts");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_while_reading_the_server_facts_leaves_the_upload_pending");
    assert_saved_and_pending(&k);
    assert!(ready(&k.host));
    let (outcome, script) = upload(&k, s::upload_steps(&k.path, k.ls, k.scm));
    assert!(matches!(*outcome, O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert!(ready(&k.host) && !record(&k.host));
    k.host.close().unwrap();
}
/// The server served its copy, which moved its stamps, and the process died
/// before the new baseline was recorded: this device can no longer show that
/// it holds what the server has, so Upload… asks (MP7 step 5).
#[test]
fn structure_job_killed_during_the_server_copy_leaves_the_upload_pending_and_asks() {
    child("server-copy");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_during_the_server_copy_leaves_the_upload_pending_and_asks");
    assert_saved_and_pending(&k);
    assert!(matches!(k.host.meguri_readiness().unwrap(), MeguriReadiness::Unbound { .. }));
    let moved = vec![r::Step::new(Method::Meta, s::meta(k.ls + 500, k.scm, s::USN + 1, true))];
    let (outcome, script) = upload(&k, moved);
    assert!(matches!(*outcome, O::Saved(U::ChooseCollection)));
    script.assert_done();
    assert!(script.bodies(Method::Upload).is_empty());
    // The killed preparation sent nothing and no longer blocks anything.
    assert!(k.host.manual_transfer_history().active().unwrap().is_none());
    assert_saved_and_pending(&k);
    k.host.close().unwrap();
}
/// The upload's intent is durable before its dispatch (MP4): a kill while
/// it is sent is an unknown acceptance, never a second upload.
#[test]
fn structure_job_killed_while_uploading_stays_acceptance_unknown() {
    child("upload");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_while_uploading_stays_acceptance_unknown");
    assert_eq!(s::field_names(&k.host, k.notetype), ["Front"]);
    assert!(record(&k.host));
    let op = k.host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::PossiblyAccepted);
    let (outcome, script) = upload(&k, vec![]);
    assert!(matches!(*outcome, O::Saved(U::AcceptanceUnknown)));
    assert!(script.bodies(Method::Upload).is_empty());
    assert!(matches!(k.host.start_meguri_collection(r::access(&k.connection, &r::Script::new(vec![]))),
        MeguriRoundStart::Refused(MeguriRoundRefusal::NotReady | MeguriRoundRefusal::Busy | MeguriRoundRefusal::BindingChanged)));
    k.host.close().unwrap();
}
/// Killed after the server accepted the upload and this device activated
/// it: the structure is on both sides, and setup's media step finishes the
/// binding as it does after any setup (MP6.5).
#[test]
fn structure_job_killed_during_its_media_step_is_uploaded_and_finishes_as_setup() {
    child("media");
    let _lane = b::LANE.lock().unwrap();
    let k = kill("structure_job_killed_during_its_media_step_is_uploaded_and_finishes_as_setup");
    assert_eq!(s::field_names(&k.host, k.notetype), ["Front"]);
    assert!(!meguri_engine::schema_gate::is_modified(&s::col(&k.host)));
    assert!(!k.host.meguri_structure_upload_pending().unwrap());
    let op = k.host.manual_transfer_history().active().unwrap().unwrap();
    assert!(op.collection_complete && op.acceptance == Acceptance::ExactOk && op.receipt.is_none());
    let done = b::finish_empty_media(&k.host, &k.connection, &op);
    k.host.complete_meguri_bootstrap(&k.connection, b::USER, &done.id).unwrap();
    assert!(ready(&k.host) && !record(&k.host));
    let quiet = r::Script::new(s::quiet_round(&k.host));
    let result = r::complete(&k.host, &r::started(k.host.start_meguri_collection(r::access(&k.connection, &quiet))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
    k.host.close().unwrap();
}
