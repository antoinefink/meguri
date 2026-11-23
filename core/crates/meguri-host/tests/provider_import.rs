//! `sync/meguri-provider#MP7.1`: a deck package import as the structure
//! job's write step, against a scripted Meguri server.
#[allow(dead_code)]
#[path = "support/meguri_bootstrap_script.rs"] mod bootstrap;
#[allow(dead_code)]
#[path = "support/meguri_round_script.rs"] mod round;
#[allow(dead_code)]
#[path = "support/meguri_structure.rs"] mod structure;
#[allow(dead_code)]
#[path = "support/meguri_import.rs"] mod import;
use bootstrap as b;
use import as i;
use round as r;
use structure as s;
use meguri_engine::{EngineError, LongOp, UndoScope, session::StartResult};
use meguri_host::{CoreHost, DeckImportAnswer, MeguriBootstrapDirection, MeguriCollectionState, MeguriStructureOutcome as O,
    MeguriStructureRefusal, MeguriStructureStop as Stop, MeguriStructureUploadOutcome as U, MeguriStructureWork, MeguriUploadPending as P};
use meguri_model::ids::DeckId;
use meguri_package::apkg::{ApkgImportError, ConfirmationToken, ImportOptions, ImportSession, StructuralReason};
use meguri_sync::{Method, provider::MeguriReadiness};
use serde_json::json;
use std::sync::Arc;

fn ready(host: &CoreHost) -> bool { matches!(host.meguri_readiness().unwrap(), MeguriReadiness::Ready) }
fn record(host: &CoreHost) -> bool { host.provider_authority().snapshot().unwrap().meguri_structure.is_some() }
/// Runs the job for one accepted confirmation against `steps`.
fn job(rig: &s::Rig, session: &Arc<ImportSession>, options: ImportOptions, token: Option<&ConfirmationToken>,
    steps: Vec<r::Step>, reachable: bool) -> (Arc<O>, DeckImportAnswer, Arc<r::Script>) {
    let script = r::Script::new(steps);
    let (work, answer) = rig.host.deck_import_work(session.clone(), options, token.cloned());
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, reachable).unwrap();
    (s::drive(&rig.host, &task), answer, script)
}
fn refusal(answer: &DeckImportAnswer) -> ApkgImportError {
    answer.lock().unwrap().clone().expect("the import ran").expect_err("the import was refused")
}
/// The confirmation a first accepted sheet gets for the sorting package.
fn confirmation(rig: &s::Rig, session: &Arc<ImportSession>) -> ConfirmationToken {
    let (outcome, answer, script) = job(rig, session, i::OFF, None, s::quiet_round(&rig.host), true);
    assert!(matches!(*outcome, O::NotSaved(Stop::Refused(_))));
    script.assert_done();
    let ApkgImportError::ConfirmationRequired { preview, reasons, token } = refusal(&answer) else {
        panic!("a structural plan asks first")
    };
    assert!(preview.is_empty(), "a sort field change alone lists no loss");
    assert_eq!(reasons, [StructuralReason::SortFieldChanged]);
    *token
}
/// Nothing imported, nothing pending, the lane free, the binding as it was
/// and nothing sent but the round's own read.
fn assert_untouched(rig: &s::Rig, before: &meguri_model::col::Col, script: &r::Script) {
    assert_eq!(i::guids(&rig.host), ["g-two"]);
    assert_eq!(i::sort_field(&rig.host), 0);
    let col = s::col(&rig.host);
    assert_eq!((col.schema_modified, col.last_sync), (before.schema_modified, before.last_sync));
    assert!(!record(&rig.host) && !rig.host.meguri_structure_upload_pending().unwrap() && ready(&rig.host));
    assert!(script.bodies(Method::Upload).is_empty() && script.bodies(Method::Download).is_empty());
    assert_ne!(rig.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), Some("Import"));
    let quiet = r::Script::new(s::quiet_round(&rig.host));
    let result = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &quiet))));
    assert_eq!(result.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
}

/// MP7.1 step 3, last case: a plan without a structural reason commits as
/// an ordinary import, sets no `scm`, needs no upload, and the next normal
/// round carries it.
#[test]
fn an_ordinary_import_needs_no_upload_and_travels_in_the_next_normal_round() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig_with(root.path(), 1, i::target_setup);
    let before = s::col(&rig.host);
    let session = i::inspect(&rig.host, &i::plain_package(root.path()));
    let (outcome, answer, script) = job(&rig, &session, i::OFF, None, s::quiet_round(&rig.host), true);
    assert!(matches!(*outcome, O::Saved(U::NotNeeded)));
    script.assert_done();
    let result = answer.lock().unwrap().clone().unwrap().unwrap();
    assert_eq!((result.counts.notes_added, result.undo_step, result.safety_backup.is_some()), (1, Some("Import"), false));
    assert_eq!(i::guids(&rig.host), ["g-plain", "g-two"]);
    let col = s::col(&rig.host);
    assert_eq!(col.schema_modified, before.schema_modified, "an ordinary import sets no scm");
    assert!(!record(&rig.host) && !rig.host.meguri_structure_upload_pending().unwrap() && ready(&rig.host));
    assert_eq!(rig.host.collection().unwrap().peek_undo(UndoScope::Global).unwrap().name(), Some("Import"));
    // The next round is a normal one and sends the imported note.
    let sent = r::Script::new(r::normal(&rig.host, json!({"done": true}), r::empty_changes(), s::USN));
    let round = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &sent))));
    assert_eq!(round.collection.as_ref().unwrap().state, MeguriCollectionState::Merged);
    sent.assert_done();
    let chunk = String::from_utf8(sent.bodies(Method::ApplyChunk)[0].clone()).unwrap();
    assert!(chunk.contains("g-plain"), "the round sends the imported note");
    rig.host.close().unwrap();
}

/// MP7.1 steps 1 to 6 for a structural import: the confirmation first, with
/// nothing written; then the round, the import with its own safety backup,
/// and the upload, which clears the undo history. The next round on this
/// device is a normal one. A new device set up from the server gets the
/// structure with no question; a device that was bound before meets the
/// full-sync requirement, as after a note type save.
#[test]
fn a_structural_import_is_confirmed_then_imported_and_uploaded() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    // The other device, bound to the same server state before the import.
    let other_root = tempfile::tempdir().unwrap();
    let other = s::rig_with(other_root.path(), 1, i::target_setup);
    let rig = s::rig_with(root.path(), 1, i::target_setup);
    let before = s::col(&rig.host);
    let session = i::inspect(&rig.host, &i::sorting_package(root.path()));

    // The first accepted sheet has no token: the plan's reason comes back.
    let script = r::Script::new(s::quiet_round(&rig.host));
    let token = confirmation(&rig, &session);
    assert_untouched(&rig, &before, &script);

    let mut steps = s::quiet_round(&rig.host);
    steps.extend(s::upload_steps(root.path(), &before));
    let (outcome, answer, script) = job(&rig, &session, i::OFF, Some(&token), steps, true);
    assert!(matches!(*outcome, O::Saved(U::Uploaded { media_complete: true })), "the upload completed");
    script.assert_done();
    assert_eq!((script.bodies(Method::Upload).len(), script.bodies(Method::Download).len()), (1, 1));
    let result = answer.lock().unwrap().clone().unwrap().unwrap();
    assert_eq!(result.counts.notes_added, 1);
    assert_eq!(result.structure.reasons, [3, 8]);
    // The import's own safety backup, and the retained server copy.
    let backup = result.safety_backup.expect("a structural import takes its safety backup");
    let backups: Vec<String> = std::fs::read_dir(root.path().join("local").join("backups")).unwrap()
        .map(|entry| entry.unwrap().file_name().into_string().unwrap()).collect();
    assert!(backups.contains(&backup) && backups.iter().any(|name| name.starts_with("server-copy-")));
    // Step 6: the upload cleared the history, so the Import step is gone.
    let collection = rig.host.collection().unwrap();
    assert_eq!(collection.undo_depth_for_testing().unwrap(), 0);
    assert_eq!(collection.peek_undo(UndoScope::Global).unwrap().name(), None);
    assert_eq!(collection.peek_redo(UndoScope::Global).unwrap().name(), None);
    assert_eq!((i::guids(&rig.host), i::sort_field(&rig.host)), (vec!["g-one".to_owned(), "g-two".into()], 1));
    let after = s::col(&rig.host);
    assert!(!meguri_engine::schema_gate::is_modified(&after) && after.schema_modified.0 > before.schema_modified.0);
    assert!(!record(&rig.host) && !rig.host.meguri_structure_upload_pending().unwrap() && ready(&rig.host));
    // The next round on this device is a normal one with nothing to exchange.
    let quiet = r::Script::new(s::quiet_round(&rig.host));
    let round = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &quiet))));
    assert_eq!(round.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
    // The session committed once: it never applies again.
    let (work, again) = rig.host.deck_import_work(session.clone(), i::OFF, Some(token.clone()));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &r::Script::new(s::quiet_round(&rig.host))), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::NotSaved(Stop::Refused(_))));
    assert_eq!(refusal(&again), ApkgImportError::SessionClosed);

    // A new device signs in: it takes the server's collection, which is the
    // uploaded one, with no collection choice (MP6.3 rows 1 and 2).
    let uploaded = script.bodies(Method::Upload).remove(0);
    let (fresh, _) = b::seeded(&root.path().join("fresh"));
    let connection = b::select(&fresh);
    let meta = || b::Reply::Bytes(s::meta(after.modified.0, after.schema_modified.0, after.usn.0, false));
    let download = b::Script::new(vec![(Method::Meta, meta()), (Method::Download, b::Reply::Bytes(uploaded)),
        (Method::Meta, meta()), (Method::Meta, meta())]);
    let preview = b::prepare(&fresh, &connection, &download, MeguriBootstrapDirection::UseServerCollection);
    let op = b::confirm(&fresh, &connection, &download, preview);
    let op = b::finish_empty_media(&fresh, &connection, &op);
    assert!(fresh.complete_meguri_bootstrap(&connection, b::USER, &op.id).unwrap().is_complete());
    assert_eq!((i::guids(&fresh), i::sort_field(&fresh)), (vec!["g-one".to_owned(), "g-two".into()], 1));
    assert!(ready(&fresh));
    fresh.close().unwrap();

    // A device bound before the import: its next round finds the server's
    // new `scm` and reports the full-sync requirement (MP6.3's choice, as
    // MP7.1 says for other devices). Nothing of it is replaced or sent.
    let held = i::guids(&other.host);
    let facts = r::Script::new(vec![r::Step::new(Method::Meta,
        s::meta(after.modified.0, after.schema_modified.0, after.usn.0, false))]);
    let round = r::complete(&other.host, &r::started(other.host.start_meguri_collection(r::access(&other.connection, &facts))));
    assert_eq!(round.collection.as_ref().unwrap().state, MeguriCollectionState::FullSyncRequired);
    facts.assert_done();
    assert_eq!((i::guids(&other.host), i::sort_field(&other.host)), (held, 0));
    other.host.close().unwrap();
    rig.host.close().unwrap();
}

/// Every refusal and cancel before the commit ends the job as after
/// `LossesChanged`: nothing imported, no structure upload pending, the lane
/// released, the server not written. The session stays open (I2).
#[test]
fn refusals_and_cancels_leave_the_collection_and_the_server_as_they_were() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig_with(root.path(), 1, i::target_setup);
    let before = s::col(&rig.host);
    let session = i::inspect(&rig.host, &i::sorting_package(root.path()));
    let token = confirmation(&rig, &session);

    // A token given for other options confirms nothing.
    let other = ImportOptions { with_scheduling: true, ..i::OFF };
    let (outcome, answer, script) = job(&rig, &session, other, Some(&token), s::quiet_round(&rig.host), true);
    assert!(matches!(*outcome, O::NotSaved(Stop::Refused(_))));
    assert!(matches!(refusal(&answer), ApkgImportError::ConfirmationRequired { .. }));
    assert_untouched(&rig, &before, &script);

    // Another long task holds the slot. Before the job: it does not start.
    let slot = rig.host.engine().acquire_slot(LongOp::CheckDatabase).unwrap();
    let silent = r::Script::new(vec![]);
    let (work, answer) = rig.host.deck_import_work(session.clone(), i::OFF, Some(token.clone()));
    assert!(matches!(rig.host.start_meguri_structure_save(s::source(&rig.connection, &silent), work, true),
        Err(MeguriStructureRefusal::Round(_))));
    assert!(answer.lock().unwrap().is_none());
    drop(slot);
    assert_untouched(&rig, &before, &silent);
    // Taken after the round: the apply's own `Busy`, as the engine words it.
    let script = r::Script::new(s::quiet_round(&rig.host));
    let (import, answer) = rig.host.deck_import_work(session.clone(), i::OFF, Some(token.clone()));
    let engine = rig.host.engine().clone();
    let work: MeguriStructureWork = Box::new(move |collection, job| {
        let _slot = engine.acquire_slot(LongOp::CheckDatabase)?;
        import(collection, job)
    });
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(&*s::drive(&rig.host, &task), O::NotSaved(Stop::Refused(EngineError::Busy { .. }))));
    assert!(matches!(refusal(&answer), ApkgImportError::Engine(error) if matches!(*error, EngineError::Busy { .. })));
    assert_untouched(&rig, &before, &script);

    // Cancelled as the import starts: nothing staged, nothing written.
    let script = r::Script::new(s::quiet_round(&rig.host));
    let (import, answer) = rig.host.deck_import_work(session.clone(), i::OFF, Some(token.clone()));
    let work: MeguriStructureWork = Box::new(move |collection, job| { job.cancel.cancel(); import(collection, job) });
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(&*s::drive(&rig.host, &task), O::NotSaved(Stop::Refused(EngineError::Cancelled) | Stop::Cancelled)));
    assert_eq!(refusal(&answer), ApkgImportError::Cancelled);
    assert_untouched(&rig, &before, &script);

    // The round finds that the server already requires a full sync: the job
    // stops before importing, and the import never ran.
    let col = s::col(&rig.host);
    let full = vec![r::Step::new(Method::Meta, s::meta(col.modified.0 + 1000, col.schema_modified.0 + 1000, s::USN, false))];
    let (outcome, answer, script) = job(&rig, &session, i::OFF, Some(&token), full, true);
    assert!(matches!(*outcome, O::NotSaved(Stop::FullSyncRequired)));
    assert!(answer.lock().unwrap().is_none());
    assert_eq!(i::guids(&rig.host), ["g-two"]);
    assert!(!record(&rig.host) && script.bodies(Method::Upload).is_empty());

    // The session is still open (I2): once the choice is made, or with the
    // service off, the same confirmation imports.
    let alone = meguri_package::apkg::apply_confirming(rig.host.engine(), &session, i::OFF, Some(&token), true,
        &meguri_engine::CancelToken::new(), &meguri_engine::ProgressSink::new()).unwrap();
    assert_eq!((alone.counts.notes_added, i::guids(&rig.host)), (1, vec!["g-one".to_owned(), "g-two".into()]));
    rig.host.close().unwrap();
}

/// MP7.1 step 4: imports run over an open review, and no replacement runs
/// during one. The import commits, the upload is left pending with the
/// needs-you state, and undoing the import withdraws it (step 6).
#[test]
fn an_import_over_an_open_review_leaves_the_upload_pending_and_undo_withdraws_it() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig_with(root.path(), 2, |host| {
        meguri_engine::sched::fsrs_policy::enable_fsrs(&host.collection().unwrap(), &Default::default()).unwrap();
        i::target_setup(host);
    });
    let before = s::col(&rig.host);
    let session = i::inspect(&rig.host, &i::sorting_package(root.path()));
    let token = confirmation(&rig, &session);
    let session_id = match rig.host.review().start(DeckId(1), None).unwrap() {
        StartResult::Started { session_id, .. } => session_id,
        StartResult::Refused(refusal) => panic!("review must start, not {refusal:?}"),
    };
    let mut steps = s::quiet_round(&rig.host);
    steps.push(r::Step::new(Method::Meta, s::meta(before.last_sync.0, before.schema_modified.0, before.usn.0, true)));
    let (outcome, answer, script) = job(&rig, &session, i::OFF, Some(&token), steps, true);
    assert!(matches!(*outcome, O::Saved(U::Pending(P::Busy))), "no replacement during a review");
    script.assert_done();
    assert!(script.bodies(Method::Upload).is_empty() && script.bodies(Method::Download).is_empty());
    assert!(answer.lock().unwrap().clone().unwrap().is_ok());
    assert_eq!(rig.host.review().active_session(), Some(session_id), "the review stays open");
    assert_eq!(i::guids(&rig.host), ["g-one", "g-two"]);
    assert!(record(&rig.host) && rig.host.meguri_structure_upload_pending().unwrap() && ready(&rig.host));
    // No automatic round runs while the upload is pending.
    assert!(matches!(rig.host.start_meguri_collection(r::access(&rig.connection, &r::Script::new(vec![]))),
        meguri_host::MeguriRoundStart::Refused(_)));
    rig.host.review().end(session_id).unwrap();
    // Undo restores `scm` (I14.3): nothing is pending any more.
    let collection = rig.host.collection().unwrap();
    assert_eq!(collection.peek_undo(UndoScope::Global).unwrap().name(), Some("Import"));
    collection.undo(UndoScope::Global).unwrap();
    assert!(!rig.host.meguri_structure_upload_pending().unwrap());
    assert_eq!((i::guids(&rig.host), i::sort_field(&rig.host)), (vec!["g-two".to_owned()], 0));
    let quiet = r::Script::new(s::quiet_round(&rig.host));
    let round = r::complete(&rig.host, &r::started(rig.host.start_meguri_collection(r::access(&rig.connection, &quiet))));
    assert_eq!(round.collection.as_ref().unwrap().state, MeguriCollectionState::NoChanges);
    assert!(!record(&rig.host));
    rig.host.close().unwrap();
}

/// The unreachable case and step 5: no round, the import, the upload left
/// pending; Upload… then uploads with no further question.
#[test]
fn an_offline_import_is_left_pending_and_upload_finishes_it() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig_with(root.path(), 1, i::target_setup);
    let before = s::col(&rig.host);
    let session = i::inspect(&rig.host, &i::sorting_package(root.path()));
    let token = confirmation(&rig, &session);
    let (outcome, _, script) = job(&rig, &session, i::OFF, Some(&token), vec![], false);
    assert!(matches!(*outcome, O::Saved(U::Pending(P::Unreachable))));
    assert!(script.bodies(Method::Meta).is_empty(), "nothing was asked of the server");
    assert_eq!(i::guids(&rig.host), ["g-one", "g-two"]);
    assert!(record(&rig.host) && rig.host.meguri_structure_upload_pending().unwrap());
    let script = r::Script::new(s::upload_steps(root.path(), &before));
    let task = rig.host.start_meguri_structure_upload(s::source(&rig.connection, &script)).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Uploaded { media_complete: true })));
    script.assert_done();
    assert!(!record(&rig.host) && ready(&rig.host));
    assert_eq!(rig.host.collection().unwrap().undo_depth_for_testing().unwrap(), 0);
    rig.host.close().unwrap();
}

/// MP7.1 step 4, `MediaCommitPending`: an op that reports a failure after
/// its commit is never reported as not saved. With a structural change the
/// upload is left pending; without one nothing is pending and an ordinary
/// round sends the rows.
#[test]
fn a_failure_reported_after_the_commit_keeps_the_rows_and_leaves_a_structural_upload_pending() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let rig = s::rig_with(root.path(), 1, i::target_setup);
    let pending = |seq| EngineError::MediaCommitPending { seq: meguri_engine::Seq(seq), message: "synthetic".into() };

    // Without a structural change.
    let plain = i::inspect(&rig.host, &i::plain_package(root.path()));
    let (import, answer) = rig.host.deck_import_work(plain, i::OFF, None);
    let work: MeguriStructureWork = Box::new(move |collection, job| { import(collection, job)?; Err(pending(1)) });
    let script = r::Script::new(s::quiet_round(&rig.host));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::NotNeeded)));
    assert!(answer.lock().unwrap().clone().unwrap().is_ok());
    assert!(!record(&rig.host) && ready(&rig.host));
    assert_eq!(i::guids(&rig.host), ["g-plain", "g-two"]);

    // With one: the rows are committed and the upload waits for Upload….
    let session = i::inspect(&rig.host, &i::sorting_package(root.path()));
    let (first, answer) = rig.host.deck_import_work(session.clone(), i::OFF, None);
    let script = r::Script::new(s::quiet_round(&rig.host));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), first, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::NotSaved(Stop::Refused(_))));
    let ApkgImportError::ConfirmationRequired { token, .. } = refusal(&answer) else { panic!("asks first") };
    let (import, _) = rig.host.deck_import_work(session, i::OFF, Some(*token));
    let work: MeguriStructureWork = Box::new(move |collection, job| { import(collection, job)?; Err(pending(2)) });
    let script = r::Script::new(s::quiet_round(&rig.host));
    let task = rig.host.start_meguri_structure_save(s::source(&rig.connection, &script), work, true).unwrap();
    assert!(matches!(*s::drive(&rig.host, &task), O::Saved(U::Pending(P::Failed))));
    script.assert_done();
    assert!(script.bodies(Method::Upload).is_empty());
    assert!(record(&rig.host) && rig.host.meguri_structure_upload_pending().unwrap());
    assert_eq!(i::guids(&rig.host), ["g-one", "g-plain", "g-two"]);
    rig.host.close().unwrap();
}

/// I2: closing or dropping a session removes its staging directory, and a
/// session left by a killed process is swept when the collection opens.
#[test]
fn a_session_staging_directory_goes_with_the_session_and_at_the_next_open() {
    let _lane = b::LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let package = i::plain_package(root.path());
    let (host, config) = b::seeded(&root.path().join("local"));
    assert_eq!(i::staging(&host), 0);
    let session = i::inspect(&host, &package);
    assert_eq!(i::staging(&host), 1);
    drop(session);
    assert_eq!(i::staging(&host), 0, "a dropped session leaves nothing");
    let closed = i::inspect(&host, &package);
    Arc::try_unwrap(closed).unwrap().close();
    assert_eq!(i::staging(&host), 0, "a closed session leaves nothing");
    // A kill: the session is never dropped.
    std::mem::forget(i::inspect(&host, &package));
    assert_eq!(i::staging(&host), 1);
    host.close().unwrap();
    let host = CoreHost::open(config).unwrap();
    assert_eq!(i::staging(&host), 0, "the launch sweep removes a dead session's staging");
    host.close().unwrap();
}
