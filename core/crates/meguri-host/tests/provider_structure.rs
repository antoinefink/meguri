//! `sync/meguri-provider#MP7` and `notetypes/draft-save#D6`, `#D7`, `#D10`:
//! the structure save job against a scripted Meguri server.
#[allow(dead_code)]
#[path = "support/meguri_bootstrap_script.rs"] mod bootstrap;
#[allow(dead_code)]
#[path = "support/meguri_round_script.rs"] mod round;
#[allow(dead_code)]
#[path = "support/meguri_structure.rs"] mod structure;
use bootstrap as b;
use round as r;
use structure as s;
use meguri_engine::{EngineError, UndoScope,
    notetypes::{change, remove, restore, structure::{Losses, StructureRefusal}}};
use meguri_host::{CoreHost, MeguriCollectionState, MeguriRoundStart, MeguriStructureOutcome as O,
    MeguriStructureRefusal, MeguriStructureStage as Stage, MeguriStructureStop as Stop,
    MeguriStructureUploadOutcome as U, MeguriStructureWork, MeguriUploadPending as P};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_sync::{Method, policy::Acceptance, provider::{MeguriReadiness, MeguriRoundRefusal, Selection}};
use serde_json::json;
use std::sync::Arc;

fn ready(host: &CoreHost) -> bool { matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready) }
fn record(host: &CoreHost) -> bool { host.provider_authority().snapshot().unwrap().meguri_structure.is_some() }
fn notes(host: &CoreHost) -> usize {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().notes_after(None, 100).unwrap().len()
}
/// Nothing saved, nothing pending, the lane free and the binding as it was.
fn assert_untouched(rig: &s::Rig, fields: usize) {
    assert_eq!(s::field_names(&rig.host, rig.notetype).len(), fields);
    assert!(!record(&rig.host));
    assert!(!rig.host.meguri_structure_upload_pending().unwrap());
    assert!(ready(&rig.host));
    assert!(!meguri_engine::schema_gate::is_modified(&s::col(&rig.host)));
    // The exclusive admission is released: a round starts again.
    let script = r::Script::new(s::quiet_round(&rig.host));
    let result = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &script))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
}

/// D10: the round runs first, then the save, then the upload; the history
/// is cleared after the upload (D7) and the binding is ready again.
#[test]
fn structure_save_runs_the_round_then_the_save_then_the_upload_and_clears_the_history() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 3);
    let before = s::col(&rig.host);
    let binding = rig.host.provider_authority().snapshot().unwrap().meguri_binding.unwrap();
    let mut steps = s::quiet_round(&rig.host);
    steps.extend(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    // The save and the read of the facts each wait for this test, so that
    // the state between two phases can be read.
    let (save_gate, save_wait) = std::sync::mpsc::channel::<()>();
    let (facts_gate, facts_wait) = std::sync::mpsc::channel::<()>();
    let facts_wait = std::sync::Mutex::new(facts_wait);
    steps[1].after = Some(Box::new(move || facts_wait.lock().unwrap().recv().unwrap()));
    let script = r::Script::new(steps);
    let (save, saved) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 3));
    let work: MeguriStructureWork = Box::new(move |collection, job| { save_wait.recv().unwrap(); save(collection, job) });
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();

    // Step 2: the round ran first, and nothing is saved yet.
    let step = s::drive_until(&rig.host, &task, s::at(Stage::Save));
    assert!(matches!(step, meguri_host::MeguriStructureStep::Running { stage: Stage::Save, .. }));
    assert_eq!(script.bodies(Method::Meta).len(), 1);
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front", "Back"]);
    // The record precedes the save and names the baseline the round left.
    let armed = rig.host.provider_authority().snapshot().unwrap().meguri_structure.unwrap();
    assert_eq!(armed.binding(), &binding);
    assert_eq!(armed.baseline().map(|baseline| (baseline.modified, baseline.schema_modified)),
        Some((before.last_sync.0, before.schema_modified.0)));

    // Step 3 then step 4: the save is one undo step until the upload.
    save_gate.send(()).unwrap();
    let step = s::drive_until(&rig.host, &task, s::at(Stage::Facts));
    assert!(matches!(step, meguri_host::MeguriStructureStep::Running { stage: Stage::Facts, .. }));
    assert!(saved.lock().unwrap().as_ref().unwrap().written);
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    assert_eq!(rig.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), Some("Update Note Type"));
    assert!(script.bodies(Method::Upload).is_empty());
    assert!(rig.host.meguri_structure_upload_pending().unwrap());

    facts_gate.send(()).unwrap();
    let outcome = s::drive(&rig.host, &task);
    assert!(matches!(*outcome, O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert_eq!(script.bodies(Method::Upload).len(), 1);
    // The server copy was retained before the upload (MP4).
    assert_eq!(script.bodies(Method::Download).len(), 1);
    // D7: the upload cleared the undo and redo history.
    let collection = rig.host.collection().unwrap();
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 0);
    assert_eq!(collection.peek_undo(UndoScope::Global).unwrap().name(), None);
    assert_eq!(collection.peek_redo(UndoScope::Global).unwrap().name(), None);
    // The saved structure is the live collection's, and nothing is pending.
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    assert_eq!(notes(&rig.host), 3);
    let after = s::col(&rig.host);
    assert!(!meguri_engine::schema_gate::is_modified(&after));
    assert!(after.schema_modified.0 > before.schema_modified.0);
    assert!(!record(&rig.host));
    assert!(!rig.host.meguri_structure_upload_pending().unwrap());
    assert!(ready(&rig.host));
    let doc = rig.host.provider_authority().snapshot().unwrap();
    assert_ne!(doc.meguri_binding.as_ref(), Some(&binding));
    assert!(doc.settings.automatic && doc.meguri_settings.automatic);
    // The safety backup of the save and the retained server copy are on this device.
    let backups: Vec<_> = std::fs::read_dir(root.path().join("local").join("backups")).unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    assert!(backups.iter().any(|name| name.starts_with("server-copy-")));
    // A finished job answers the same outcome again and holds nothing.
    assert!(matches!(rig.host.advance_meguri_structure(&task).unwrap(), meguri_host::MeguriStructureStep::Finished(_)));
    let quiet = r::Script::new(s::quiet_round(&rig.host));
    let result = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &quiet))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
    rig.host.close().unwrap();
}

/// Sync Automatically off stays off through the job (MP6.2): the binding the
/// upload completes takes the stored choice back.
#[test]
fn structure_upload_keeps_sync_automatically_off() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 1);
    let mut settings = rig.host.provider_authority().snapshot().unwrap().settings;
    settings.automatic = false;
    rig.host.save_provider_settings(settings).unwrap();
    let before = s::col(&rig.host);
    let mut steps = s::quiet_round(&rig.host);
    steps.extend(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 1));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    let doc = rig.host.provider_authority().snapshot().unwrap();
    assert!(ready(&rig.host) && !doc.settings.automatic && !doc.meguri_settings.automatic);
    rig.host.close().unwrap();
}

/// D6 step 1, D10: a round that brings a change to this note type stops
/// the job before saving, and the draft meets `Stale`.
#[test]
fn a_round_that_changes_the_note_type_stops_the_job_with_stale_and_saves_nothing() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let (mut model, _) = s::exchanged_rows(&rig);
    // The draft is made on the note type as this device holds it.
    let draft = s::draft_without_back(&rig.host, rig.notetype, 3);
    // Another device changed the same note type since.
    model["flds"][0]["sticky"] = json!(false);
    model["mod"] = json!(model["mod"].as_i64().unwrap() + 30);
    let changes = json!({"models": [model], "decks": [[], []], "tags": []});
    let script = r::Script::new(r::normal(&rig.host, json!({"done": true}), changes, s::USN + 1));
    let (work, saved) = s::save_work(draft);
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    let outcome = s::drive(&rig.host, &task);
    assert!(matches!(&*outcome, O::NotSaved(Stop::Refused(EngineError::Stale { .. }))), "the draft meets Stale");
    script.assert_done();
    assert!(saved.lock().unwrap().is_none());
    assert!(script.bodies(Method::Upload).is_empty() && script.bodies(Method::Download).is_empty());
    assert_untouched(&rig, 2);
    rig.host.close().unwrap();
}

/// MP7 step 3: the losses are counted again after the round. A round that
/// brings a note with content in the removed field ends the job with the new
/// counts: nothing saved, nothing pending, the admission released.
#[test]
fn a_round_that_raises_a_confirmed_loss_ends_the_job_with_losses_changed() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let (_, mut note) = s::exchanged_rows(&rig);
    // The user confirmed the three notes this device holds.
    let draft = s::draft_without_back(&rig.host, rig.notetype, 3);
    // Another device added a fourth note with content in that field.
    note[0] = json!(note[0].as_i64().unwrap() + 7);
    note[1] = json!("generated-guid");
    let chunk = json!({"done": true, "notes": [note]});
    let script = r::Script::new(r::normal(&rig.host, chunk, r::empty_changes(), s::USN + 1));
    let backups = || std::fs::read_dir(root.path().join("local").join("backups")).unwrap().count();
    let kept = backups();
    let (work, saved) = s::save_work(draft);
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    let outcome = s::drive(&rig.host, &task);
    let O::NotSaved(Stop::Refused(EngineError::NotetypeStructure(StructureRefusal::LossesChanged(losses)))) = &*outcome
        else { panic!("the job ends with LossesChanged") };
    assert!(matches!(&**losses, Losses::Draft(summary) if summary.loses_content));
    script.assert_done();
    assert_eq!(notes(&rig.host), 4, "the round's note arrived before the count");
    assert!(saved.lock().unwrap().is_none());
    // Refused before the safety backup, and no undo step for it.
    assert_eq!(backups(), kept);
    assert_ne!(rig.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), Some("Update Note Type"));
    assert!(script.bodies(Method::Upload).is_empty());
    assert_untouched(&rig, 2);
    // Accepting the question again is a new job with a new round.
    let before = s::col(&rig.host);
    let mut steps = s::quiet_round(&rig.host);
    steps.extend(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 4));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    rig.host.close().unwrap();
}

/// D8's four operations each count their losses again after the round, and
/// each refusal ends the job with nothing saved and nothing pending.
#[test]
fn each_of_the_four_ops_is_refused_after_the_round_when_its_loss_was_not_confirmed() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let collection = rig.host.collection().unwrap();
    let (other, note_ids) = {
        let reader = collection.reader(ReaderRole::Query).unwrap();
        let other = reader.ids_after(meguri_storage::Table::Notetypes, None, 100).unwrap().into_iter()
            .map(meguri_model::ids::NotetypeId).find(|id| *id != rig.notetype).unwrap();
        let notes: Vec<_> = reader.notes_after(None, 100).unwrap().iter().map(|note| note.id).collect();
        (other, notes)
    };
    for name in ["draft save", "delete note type", "restore to default", "change note type"] {
        let notetype = rig.notetype;
        let work: MeguriStructureWork = match name {
            "draft save" => s::save_work(s::draft_without_back(&rig.host, notetype, 0)).0,
            "delete note type" => Box::new(move |collection, job| {
                remove::remove(collection, remove::RemoveNotetype { notetype, confirmed_notes: 0, confirmed_cards: 0 },
                    job.cancel.clone(), job.progress.clone()).map(|_| ())
            }),
            "restore to default" => {
                // The question showed a loss, and the note type changed since.
                let digest = restore::restore_summary(&collection, notetype, None,
                    &meguri_engine::CancelToken::new(), &meguri_engine::ProgressSink::new()).unwrap().digest;
                let stored = collection.reader(ReaderRole::Query).unwrap().notetype(notetype).unwrap().unwrap();
                let mut draft = meguri_engine::notetypes::Draft::of(stored);
                draft.fields[0].config.sticky = !draft.fields[0].config.sticky;
                meguri_engine::notetypes::save(&collection, draft, Default::default()).unwrap();
                let mut confirmed = meguri_engine::notetypes::structure::ConfirmedLosses::default();
                confirmed.fields.insert(0, 1);
                Box::new(move |collection, job| {
                    restore::restore(collection, restore::RestoreNotetype { notetype, kind: None, confirmed, digest: Some(digest) },
                        job.cancel.clone(), job.progress.clone()).map(|_| ())
                })
            }
            _ => {
                // The second field is discarded with content in two notes; none was confirmed.
                let info = change::info(&collection, &note_ids, other,
                    &meguri_engine::CancelToken::new(), &meguri_engine::ProgressSink::new()).unwrap();
                let mut fields = vec![None; info.new.fields.len()];
                fields[0] = Some(0);
                let request = change::ChangeNotetype { notes: note_ids.clone(), old_notetype: notetype, new_notetype: other,
                    fields, templates: info.templates.clone(), schema: info.schema, layout: info.layout.clone(),
                    confirmed: change::ChangeLosses { fields: Default::default(), templates: Default::default(), other_cards: 0 } };
                Box::new(move |collection, job| {
                    change::change(collection, request, job.cancel.clone(), job.progress.clone()).map(|_| ())
                })
            }
        };
        let script = r::Script::new(s::quiet_round(&rig.host));
        let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
        let outcome = s::drive(&rig.host, &task);
        assert!(matches!(&*outcome, O::NotSaved(Stop::Refused(EngineError::NotetypeStructure(StructureRefusal::LossesChanged(_))))),
            "{name}: LossesChanged after the round");
        script.assert_done();
        assert!(!record(&rig.host), "{name}: nothing pending");
        assert!(!rig.host.meguri_structure_upload_pending().unwrap(), "{name}");
        assert!(ready(&rig.host), "{name}: the binding stays ready");
        assert_eq!(notes(&rig.host), 2, "{name}: nothing deleted");
        assert_eq!(s::field_names(&rig.host, notetype).len(), 2, "{name}");
    }
    drop(collection);
    rig.host.close().unwrap();
}

/// D6 step 4, D10: a failed upload keeps the save and records the needs-you
/// state, which survives a relaunch; Upload… then uploads with no question.
#[test]
fn a_failed_upload_keeps_the_save_and_leaves_a_durable_pending_upload() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let before = s::col(&rig.host);
    let mut steps = s::quiet_round(&rig.host);
    // The server cannot be read after the save.
    let mut lost = r::Step::new(Method::Meta, Vec::new());
    lost.lost = true;
    steps.push(lost);
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 2));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Pending(P::Unreachable))));
    script.assert_done();
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    // The save stays one undo step until the upload (D7).
    assert_eq!(rig.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), Some("Update Note Type"));
    // Automatic rounds do not run while the upload is pending, and no request is made.
    let silent = r::Script::new(vec![]);
    for _ in 0..2 {
        assert!(matches!(rig.host.start_meguri_collection(r::access(&rig.connection, &silent)),
            MeguriRoundStart::Refused(MeguriRoundRefusal::NotReady)));
        assert!(matches!(rig.host.start_meguri_media(r::access(&rig.connection, &silent)),
            MeguriRoundStart::Refused(MeguriRoundRefusal::NotReady)));
    }
    // Another structure save waits for Upload…, and its op never runs.
    let work: MeguriStructureWork = Box::new(|_, _| panic!("no op runs while an upload is pending"));
    assert!(matches!(rig.host.start_meguri_structure_save(s::source(&rig.connection, &silent), work, true),
        Err(MeguriStructureRefusal::UploadPending)));
    assert!(rig.host.meguri_structure_upload_pending().unwrap());

    // A relaunch keeps the pending state.
    let Ok(host) = Arc::try_unwrap(rig.host) else { panic!("the rig owns its host") };
    host.close().unwrap();
    let host = CoreHost::open(rig.config.clone()).unwrap();
    b::settle(&host);
    assert!(host.meguri_structure_upload_pending().unwrap());
    assert!(ready(&host) && record(&host));
    assert_eq!(s::field_names(&host, rig.notetype), ["Front"]);
    assert!(matches!(host.start_meguri_collection(r::access(&rig.connection, &silent)),
        MeguriRoundStart::Refused(MeguriRoundRefusal::NotReady)));

    // Upload…: the facts, the server copy, the upload; no round and no question.
    let script = r::Script::new(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    let task = host.start_meguri_structure_upload(s::source(&rig.connection, &script)).unwrap();
    assert!(matches!(*s::drive(&host, &task), O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert!(script.bodies(Method::Start).is_empty());
    assert!(!host.meguri_structure_upload_pending().unwrap() && !record(&host) && ready(&host));
    assert_eq!(host.collection().unwrap().undo_depth_for_testing().unwrap(), 0);
    assert!(matches!(host.start_meguri_structure_upload(s::source(&rig.connection, &silent)),
        Err(MeguriStructureRefusal::NothingPending)));
    host.close().unwrap();
}

/// MP7 step 4: when another client synced in between, the server's facts
/// are not the baseline, and the job stops before uploading with MP6.3's
/// choice. The save and the pending state stay.
#[test]
fn changed_server_facts_stop_the_upload_before_any_transfer_and_ask_which_collection_to_keep() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let before = s::col(&rig.host);
    let mut steps = s::quiet_round(&rig.host);
    steps.push(r::Step::new(Method::Meta, s::meta(before.last_sync.0 + 1, before.schema_modified.0, s::USN + 1, false)));
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 2));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::ChooseCollection)));
    script.assert_done();
    assert!(script.bodies(Method::Download).is_empty() && script.bodies(Method::Upload).is_empty());
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    // The binding is no longer ready, so that the choice can run (MP6.3).
    assert!(matches!(rig.host.meguri_readiness().unwrap(), MeguriReadiness::Unbound { intent: None }));
    assert!(rig.host.manual_transfer_history().active().unwrap().is_none());
    // Upload… reads the facts again and gives the same answer while they differ.
    let again = r::Script::new(vec![r::Step::new(Method::Meta,
        s::meta(before.last_sync.0 + 1, before.schema_modified.0, s::USN + 1, false))]);
    let task = rig.host.start_meguri_structure_upload(s::source(&rig.connection, &again)).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::ChooseCollection)));
    again.assert_done();
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    rig.host.close().unwrap();
}

/// Another client's exchange between the read of the facts and the server
/// copy: the copy's stamps moved more than once, and nothing is uploaded.
#[test]
fn a_server_copy_that_moved_more_than_its_own_download_is_not_replaced() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 1);
    let before = s::col(&rig.host);
    let (ls, scm) = (before.last_sync.0, before.schema_modified.0);
    let mut steps = s::quiet_round(&rig.host);
    steps.extend([r::Step::new(Method::Meta, s::meta(ls, scm, s::USN, false)),
        r::Step::new(Method::Meta, s::meta(ls + 200, scm, s::USN + 1, false)),
        r::Step::new(Method::Download, s::served(&root.path().join("served"), ls + 500, scm, s::USN + 2)),
        r::Step::new(Method::Meta, s::meta(ls + 500, scm, s::USN + 2, true))]);
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 1));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::ChooseCollection)));
    script.assert_done();
    assert!(script.bodies(Method::Upload).is_empty());
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    // The preparation sent nothing and is gone; the next attempt starts clean.
    assert!(rig.host.manual_transfer_history().active().unwrap().is_none());
    rig.host.close().unwrap();
}

/// D4's unreachable case and D7: no round, the save, the upload left
/// pending; undoing the save withdraws the pending state.
#[test]
fn an_offline_save_skips_the_round_and_undo_withdraws_the_pending_upload() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let silent = r::Script::new(vec![]);
    let (work, saved) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 2));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &silent), work, false).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Pending(P::Unreachable))));
    assert!(saved.lock().unwrap().as_ref().unwrap().written);
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    assert!(ready(&rig.host), "the binding is held only when the upload runs");
    // Undo before the upload: `scm <= ls` again, so nothing is pending.
    rig.host.collection().unwrap().undo(UndoScope::Global).unwrap();
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front", "Back"]);
    assert!(!rig.host.meguri_structure_upload_pending().unwrap());
    assert!(matches!(rig.host.start_meguri_structure_upload(s::source(&rig.connection, &silent)),
        Err(MeguriStructureRefusal::NothingPending)));
    // Rounds are admitted again, and the record goes with the first admission.
    let mut lost = r::Step::new(Method::Meta, Vec::new());
    lost.lost = true;
    let offline = r::Script::new(vec![lost]);
    let result = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &offline))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::Failed);
    assert!(!record(&rig.host) && ready(&rig.host));
    rig.host.close().unwrap();
}

/// An offline save and its Upload… later: the baseline is the one the last
/// completed bootstrap or round left (MP7 step 4).
#[test]
fn upload_after_an_offline_save_uses_the_last_proved_baseline() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 2);
    let before = s::col(&rig.host);
    let silent = r::Script::new(vec![]);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 2));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &silent), work, false).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Pending(P::Unreachable))));
    let script = r::Script::new(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    let task = rig.host.start_meguri_structure_upload(s::source(&rig.connection, &script)).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert!(ready(&rig.host) && !record(&rig.host));
    rig.host.close().unwrap();
}

/// MP7 step 1: the job holds the provider's lane for its whole length. No
/// other round, job, setup or service change starts in between; a cancelled
/// round saves nothing and releases it.
#[test]
fn the_job_holds_the_lane_and_a_cancelled_round_saves_nothing() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 1);
    let (entered, proceed) = (std::sync::mpsc::channel::<()>(), std::sync::mpsc::channel::<()>());
    let (entered_tx, proceed_rx) = (entered.0, std::sync::Mutex::new(proceed.1));
    let mut steps = r::normal(&rig.host, json!({"done": true}), r::empty_changes(), s::USN);
    steps[0].after = Some(Box::new(move || { entered_tx.send(()).unwrap(); proceed_rx.lock().unwrap().recv().unwrap(); }));
    steps.truncate(1);
    let script = r::Script::new(steps);
    let (work, saved) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 1));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    entered.1.recv().unwrap();
    let silent = r::Script::new(vec![]);
    assert!(matches!(rig.host.start_meguri_collection(r::access(&rig.connection, &silent)),
        MeguriRoundStart::Refused(MeguriRoundRefusal::Busy)));
    assert!(matches!(rig.host.start_meguri_media(r::access(&rig.connection, &silent)),
        MeguriRoundStart::Refused(MeguriRoundRefusal::Busy)));
    let (other, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 1));
    assert!(matches!(rig.host.start_meguri_structure_save(s::source(&rig.connection, &silent), other, true),
        Err(MeguriStructureRefusal::Busy)));
    assert!(matches!(rig.host.start_meguri_structure_upload(s::source(&rig.connection, &silent)),
        Err(MeguriStructureRefusal::Busy)));
    assert!(matches!(rig.host.begin_provider_handoff(Selection::LocalOnly), Err(EngineError::Busy { .. })));
    assert!(matches!(rig.host.prepare_meguri_setup(r::access(&rig.connection, &silent)),
        Err(meguri_host::MeguriBootstrapError::Busy)));
    // Cancelled during the round: the round stops and nothing is saved.
    task.cancel();
    proceed.0.send(()).unwrap();
    let outcome = s::drive(&rig.host, &task);
    assert!(matches!(&*outcome, O::NotSaved(Stop::Round { .. } | Stop::Cancelled)));
    assert!(saved.lock().unwrap().is_none());
    assert_eq!(rig.host.provider_authority().snapshot().unwrap().selection, Selection::Meguri);
    assert_untouched(&rig, 2);
    rig.host.close().unwrap();
}

/// MP7 step 5: a lost acknowledgement stays MP6.9's acceptance unknown
/// first. The save and its record stay, and Upload… sends nothing meanwhile.
#[test]
fn a_lost_upload_acknowledgement_stays_acceptance_unknown_and_is_never_sent_twice() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig(root.path(), 1);
    let before = s::col(&rig.host);
    let mut steps = s::quiet_round(&rig.host);
    steps.extend(s::upload_steps(root.path(), before.last_sync.0, before.schema_modified.0));
    steps.pop();
    steps.last_mut().unwrap().lost = true;
    let script = r::Script::new(steps);
    let (work, _) = s::save_work(s::draft_without_back(&rig.host, rig.notetype, 1));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::AcceptanceUnknown)));
    script.assert_done();
    let op = rig.host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::PossiblyAccepted);
    assert_eq!(s::field_names(&rig.host, rig.notetype), ["Front"]);
    assert!(record(&rig.host) && rig.host.meguri_structure_upload_pending().unwrap());
    // The upload exclusion stands: Upload… makes no request (MP3, MP6.9).
    let silent = r::Script::new(vec![]);
    let task = rig.host.start_meguri_structure_upload(s::source(&rig.connection, &silent)).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::AcceptanceUnknown)));
    assert!(matches!(rig.host.start_meguri_collection(r::access(&rig.connection, &silent)), MeguriRoundStart::Refused(_)));
    // MP6.9 settles it: the server still holds what the copy retained, so
    // nothing was accepted, and Upload… then sends it under the same record.
    let (ls, scm) = (before.last_sync.0 + 500, before.schema_modified.0);
    let settle = b::Script::new(vec![(Method::Meta, b::Reply::Bytes(s::meta(ls, scm, s::USN + 1, true)))]);
    let events = rig.host.engine().stream().subscribe();
    let settled = b::complete(events, rig.host.settle_meguri_setup(b::access(&rig.connection, &settle)).unwrap()).unwrap();
    assert!(matches!(settled, meguri_host::MeguriSetupSettlement::NotAccepted { .. }));
    assert!(rig.host.meguri_structure_upload_pending().unwrap());
    let script = r::Script::new(s::upload_steps_from(root.path(), ls, scm, s::USN + 1));
    let task = rig.host.start_meguri_structure_upload(s::source(&rig.connection, &script)).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert_eq!(script.bodies(Method::Upload).len(), 1);
    assert!(ready(&rig.host) && !record(&rig.host));
    rig.host.close().unwrap();
}
