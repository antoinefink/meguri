//! sync/meguri-provider MP6.3 (rows, both extra rules, precedence, the races)
//! and MP6.9 (settling), on the real retained transfer path with a scripted server.
#[path = "support/meguri_bootstrap_script.rs"]
mod support;
use meguri_host::{
    CoreHost, HostConfig, MeguriAccess, MeguriBootstrapError as E, MeguriSetupChange as Change,
    MeguriSetupChoice as Choice, MeguriSetupDecision, MeguriSetupRow as Row, MeguriSetupSettlement as Settled,
};
use meguri_model::{clock::{FixedClock, UtcOffset}, rng::SeededRandom, time::{EpochMillis, EpochSecs}, usn::Usn};
use meguri_storage::{OpenOptions, ReadRows, ReaderRole, StampMode, open_collection};
use meguri_sync::{
    HostKey, Method,
    oneway::{Preview, TransferError},
    policy::{Acceptance, Boundary, Direction, Operation, Phase},
    provider::{MeguriConnection, Selection},
};
use std::{fs, path::Path, sync::Arc};
use support::*;

fn meta_with(modified: i64, schema: i64, usn: i32, media_usn: i32) -> Vec<u8> {
    format!(
        r#"{{"mod":{modified},"scm":{schema},"usn":{usn},"ts":{},"msg":"","cont":true,"empty":true,"media_usn":{media_usn}}}"#,
        NOW / 1000
    )
    .into_bytes()
}
/// A server collection whose facts answer `meta(NOW - 1000, 8)`; `decks` adds
/// that many decks, which are user content without a card.
fn server_with(root: &Path, decks: i64) -> Vec<u8> {
    fs::create_dir_all(root).unwrap();
    let path = root.join("collection.anki2");
    let clock = FixedClock::new(NOW, UtcOffset::UTC);
    let random = SeededRandom::new(75);
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let db = open_collection(&path, &OpenOptions::new(&clock, &random)).unwrap();
    let mut scope = db.writer().begin_write(StampMode::Raw, &clock, None).unwrap();
    let mut col = scope.col().unwrap();
    col.modified = EpochMillis(NOW - 1000);
    col.schema_modified = EpochMillis(NOW - 5000);
    col.usn = Usn(8);
    scope.put_col(&col).unwrap();
    let default = scope.deck(meguri_model::ids::DeckId(1)).unwrap().unwrap();
    for index in 0..decks {
        let mut deck = default.clone();
        deck.id = meguri_model::ids::DeckId(100 + index);
        deck.name = format!("Server deck {index}");
        deck.mtime = EpochSecs(77);
        scope.put(&deck).unwrap();
    }
    scope.commit().unwrap();
    db.close().unwrap();
    fs::read(path).unwrap()
}
fn add_deck(host: &CoreHost, name: &str) {
    meguri_engine::decks::add(&host.collection().unwrap(), name.into()).unwrap();
    settle(host);
}
fn access_as(connection: &MeguriConnection, script: &Arc<Script>, user: &str) -> MeguriAccess {
    MeguriAccess::new(connection.clone(), user, HostKey::new("synthetic-key"))
        .unwrap()
        .with_transport_for_testing(Box::new(ScriptTransport(script.clone())))
}
fn run<T>(host: &CoreHost, task: meguri_host::ManualTransferTask<T>, events: meguri_engine::Subscription) -> Result<T, TransferError> {
    let _ = host;
    complete(events, task)
}
fn prepare_setup(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>) -> Preview {
    let events = host.engine().stream().subscribe();
    run(host, host.prepare_meguri_setup(access(connection, script)).unwrap(), events).unwrap()
}
fn decide(host: &CoreHost, connection: &MeguriConnection, preview: &Preview) -> MeguriSetupDecision {
    host.meguri_setup_decision(connection, USER, preview).unwrap()
}
fn confirm_setup(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>, shown: &MeguriSetupDecision,
    preview: Preview, choice: Choice) -> Result<Operation, TransferError> {
    let events = host.engine().stream().subscribe();
    run(host, host.confirm_meguri_setup(access(connection, script), shown, preview, choice).unwrap(), events)
}
fn prepare_download(host: &CoreHost, connection: &MeguriConnection, script: &Arc<Script>, shown: &MeguriSetupDecision,
    preview: Preview, choice: Choice) -> Preview {
    let events = host.engine().stream().subscribe();
    run(host, host.prepare_meguri_setup_download(access(connection, script), shown, preview, choice).unwrap(), events).unwrap()
}
fn upload_preparation(data: Vec<u8>) -> Arc<Script> {
    Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))), (Method::Download, Reply::Bytes(data)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8)))])
}
fn refused<T>(result: Result<T, E>) -> E {
    match result { Ok(_) => panic!("refusal expected"), Err(error) => error }
}
fn local_decks(host: &CoreHost) -> usize {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().user_content_rows().unwrap().other_decks as usize + 1
}
fn no_collection(root: &Path) -> (CoreHost, HostConfig) {
    let config = config(root, Arc::new(FixedClock::new(NOW, UtcOffset::UTC)));
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let host = process.open_for_manual_import(config.clone()).unwrap();
    (host, config)
}

#[test]
fn row_1_no_collection_downloads_with_no_question() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = no_collection(&root.path().join("local"));
    let connection = select(&host);
    let script = collection_script(server_with(&root.path().join("server"), 2), false);
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(preview.operation.direction, Direction::Import);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Download);
    assert!(shown.device.is_none());
    for choice in [Choice::KeepDevice, Choice::KeepServer] {
        assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(), choice)),
            E::SetupChanged(Change::Choice)));
    }
    let op = confirm_setup(&host, &connection, &script, &shown, preview, Choice::NoQuestion).unwrap();
    script.assert_done();
    assert!(op.collection_complete && op.safety_backups.is_empty());
    assert_eq!(local_decks(&host), 3);
    host.close().unwrap();
}

#[test]
fn row_2_empty_device_downloads_after_a_backup_with_no_question() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    script.assert_done();
    assert_eq!(preview.operation.direction, Direction::Export);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::DownloadOverEmptyDevice);
    assert_eq!(shown.device.unwrap().media_files, Some(0));
    // The prepared upload is never the answer to row 2.
    for choice in [Choice::NoQuestion, Choice::KeepDevice] {
        assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(), choice)),
            E::SetupChanged(Change::Choice)));
    }
    assert!(matches!(refused(host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview.clone(),
        Choice::KeepServer)), E::SetupChanged(Change::Choice)));
    let script = collection_script(data, false);
    let download = prepare_download(&host, &connection, &script, &shown, preview.clone(), Choice::NoQuestion);
    assert_eq!(host.manual_transfer_history().get(&preview.operation.id).unwrap().phase, Phase::Cancelled);
    assert!(matches!(refused(host.confirm_meguri_setup_download(access(&connection, &script), &shown, download.clone(),
        Choice::KeepServer)), E::SetupChanged(Change::Choice)));
    let events = host.engine().stream().subscribe();
    let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
        Choice::NoQuestion).unwrap(), events).unwrap();
    script.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    assert!(op.collection_complete);
    // MP4: the collection that was on this device is in the backups.
    assert!(!op.safety_backups.is_empty());
    for backup in &op.safety_backups { assert!(config.backups_directory.join(&backup.name).is_file()); }
    assert_eq!(local_decks(&host), 3);
    host.close().unwrap();
}

#[test]
fn row_3_empty_server_on_a_first_binding_uploads_with_no_question() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = collection_script(server_with(&root.path().join("server"), 0), true);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::UploadToEmptyServer);
    assert_eq!(shown.server.unwrap().media_files, Some(0));
    assert!(matches!(refused(host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview.clone(),
        Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
    let op = confirm_setup(&host, &connection, &script, &shown, preview, Choice::NoQuestion).unwrap();
    script.assert_done();
    assert_eq!(op.acceptance, Acceptance::ExactOk);
    assert!(op.collection_complete && !op.safety_backups.is_empty());
    // MP4: the server copy is retained even though it held nothing.
    assert!(config.backups_directory.join(&op.server_copy.unwrap().artifact.name).is_file());
    host.close().unwrap();
}

#[test]
fn row_4_two_collections_ask_and_each_answer_runs_only_its_direction() {
    let _lane = LANE.lock().unwrap();
    for keep_device in [true, false] {
        let root = tempfile::tempdir().unwrap();
        let (host, _config) = seeded(&root.path().join("local"));
        add_deck(&host, "Mine");
        let connection = select(&host);
        let data = server_with(&root.path().join("server"), 2);
        let script = collection_script(data.clone(), true);
        let preview = prepare_setup(&host, &connection, &script);
        let shown = decide(&host, &connection, &preview);
        assert_eq!(shown.row, Row::Ask);
        assert!(shown.device.is_some() && shown.server.is_some());
        // No answer, no transfer.
        assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(),
            Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
        assert!(matches!(refused(host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview.clone(),
            Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
        assert_eq!(script.count(Method::Upload), 0);
        if keep_device {
            let op = confirm_setup(&host, &connection, &script, &shown, preview, Choice::KeepDevice).unwrap();
            script.assert_done();
            assert_eq!(op.direction, Direction::Export);
            assert_eq!(local_decks(&host), 2);
        } else {
            let script = collection_script(data, false);
            let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
            let events = host.engine().stream().subscribe();
            let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
                Choice::KeepServer).unwrap(), events).unwrap();
            script.assert_done();
            assert_eq!(script.count(Method::Upload), 0);
            assert!(op.collection_complete && !op.safety_backups.is_empty());
            assert_eq!(local_decks(&host), 3);
        }
        host.close().unwrap();
    }
}

#[test]
fn a_side_that_is_not_proved_empty_asks() {
    let _lane = LANE.lock().unwrap();
    // The server reports media: its empty collection is not an empty server.
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(server_with(&root.path().join("server"), 0))),
        (Method::Meta, Reply::Bytes(meta_with(NOW - 1000, NOW - 5000, 8, 3)))]);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Ask);
    assert_eq!(shown.server.unwrap().media_files, None);
    host.close().unwrap();

    // A media file on this device is user content, tracked or not.
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    let media = host.media().unwrap().store().media_dir().to_owned();
    fs::create_dir_all(&media).unwrap();
    fs::write(media.join("sound.mp3"), b"synthetic").unwrap();
    let connection = select(&host);
    let script = upload_preparation(server_with(&root.path().join("server"), 2));
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    host.close().unwrap();

    // A retained copy that is no longer the file the preview recorded proves nothing.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = upload_preparation(server_with(&root.path().join("server"), 0));
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::UploadToEmptyServer);
    let copy = config.backups_directory.join(&preview.operation.server_copy.as_ref().unwrap().artifact.name);
    let mut bytes = fs::read(&copy).unwrap();
    bytes.push(0);
    let mut permissions = fs::metadata(&copy).unwrap().permissions();
    #[allow(clippy::permissions_set_readonly_false)]
    permissions.set_readonly(false);
    fs::set_permissions(&copy, permissions).unwrap();
    fs::write(&copy, bytes).unwrap();
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    host.close().unwrap();
}

/// A device that completed setup once, then went to Off and chose Meguri Sync again.
fn returned(root: &Path) -> (CoreHost, HostConfig, MeguriConnection) {
    let (host, config, _connection, _op) = ready(root);
    let handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    host.finish_provider_handoff(&handoff).unwrap();
    let connection = select(&host);
    (host, config, connection)
}

#[test]
fn after_an_earlier_binding_rows_2_and_3_are_never_taken() {
    let _lane = LANE.lock().unwrap();
    // The device holds no user content: row 2 without the earlier binding.
    let root = tempfile::tempdir().unwrap();
    let (host, _config, connection) = returned(root.path());
    let script = upload_preparation(server_with(&root.path().join("again"), 2));
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    host.close().unwrap();

    // The server is proved empty and the account is the same: still row 4's sheet.
    let root = tempfile::tempdir().unwrap();
    let (host, _config, connection) = returned(root.path());
    add_deck(&host, "Mine");
    let script = upload_preparation(server_with(&root.path().join("again"), 0));
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Ask);
    assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview, Choice::NoQuestion)),
        E::SetupChanged(Change::Choice)));
    host.close().unwrap();
}

#[test]
fn another_account_with_an_empty_server_offers_only_the_upload_and_wins_precedence() {
    let _lane = LANE.lock().unwrap();
    let other = "other-user";
    let root = tempfile::tempdir().unwrap();
    let (host, _config, connection) = returned(root.path());
    add_deck(&host, "Mine");
    let script = collection_script(server_with(&root.path().join("again"), 0), true);
    let events = host.engine().stream().subscribe();
    let preview = run(&host, host.prepare_meguri_setup(access_as(&connection, &script, other)).unwrap(), events).unwrap();
    // Both rules apply here (an earlier binding, and another account): one direction.
    let shown = host.meguri_setup_decision(&connection, other, &preview).unwrap();
    assert_eq!(shown.row, Row::AskUploadOnly);
    // The preview belongs to the account that prepared it.
    assert!(host.meguri_setup_decision(&connection, USER, &preview).is_err());
    assert!(matches!(refused(host.confirm_meguri_setup(access_as(&connection, &script, other), &shown, preview.clone(),
        Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
    assert!(matches!(refused(host.prepare_meguri_setup_download(access_as(&connection, &script, other), &shown,
        preview.clone(), Choice::KeepServer)), E::SetupChanged(Change::Choice)));
    assert_eq!(script.count(Method::Upload), 0);
    let events = host.engine().stream().subscribe();
    let op = run(&host, host.confirm_meguri_setup(access_as(&connection, &script, other), &shown, preview,
        Choice::KeepDevice).unwrap(), events).unwrap();
    script.assert_done();
    assert_eq!(op.acceptance, Acceptance::ExactOk);
    host.close().unwrap();

    // Another account whose server holds user content: both directions, row 4.
    let root = tempfile::tempdir().unwrap();
    let (host, _config, connection) = returned(root.path());
    add_deck(&host, "Mine");
    let script = upload_preparation(server_with(&root.path().join("again"), 2));
    let events = host.engine().stream().subscribe();
    let preview = run(&host, host.prepare_meguri_setup(access_as(&connection, &script, other)).unwrap(), events).unwrap();
    assert_eq!(host.meguri_setup_decision(&connection, other, &preview).unwrap().row, Row::Ask);
    host.close().unwrap();
}

#[test]
fn row_3_stops_when_the_server_changed_before_dispatch() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = upload_preparation(server_with(&root.path().join("server"), 0));
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::UploadToEmptyServer);
    // Another client wrote between the retained copy and the dispatch.
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 500, 9)))]);
    let result = confirm_setup(&host, &connection, &script, &shown, preview.clone(), Choice::NoQuestion);
    assert!(matches!(result, Err(TransferError::Stale(Boundary::RemoteChanged))));
    script.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    let kept = host.manual_transfer_history().get(&preview.operation.id).unwrap();
    assert_eq!(kept.acceptance, Acceptance::NotSent);
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

#[test]
fn row_2_stops_when_user_content_appears_on_this_device() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::DownloadOverEmptyDevice);
    // Content written after the decision: the decision no longer holds.
    add_deck(&host, "Made while setup ran");
    assert!(matches!(refused(host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview.clone(),
        Choice::NoQuestion)), E::SetupChanged(Change::Local)));
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&preview.operation));
    host.discard_meguri_bootstrap_preparation(&connection, &preview.operation).unwrap();
    // Preparation again gives row 4.
    let script = upload_preparation(data);
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

#[test]
fn row_2_download_is_refused_when_this_device_changed_after_the_download_was_prepared() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    let script = collection_script(data, false);
    let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::NoQuestion);
    add_deck(&host, "Made after the download was prepared");
    let before = local_decks(&host);
    // The guard's sequence no longer matches: the rows cannot be proved, so this is content.
    assert!(matches!(refused(host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
        Choice::NoQuestion)), E::SetupChanged(Change::Local)));
    assert_eq!(script.count(Method::Meta), 2);
    assert_eq!(local_decks(&host), before);
    host.close().unwrap();
}

#[test]
fn keeping_the_server_collection_needs_the_server_the_sheet_showed() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = upload_preparation(server_with(&root.path().join("server"), 2));
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Ask);
    // The server gained a deck between the sheet and the download.
    let script = collection_script(server_with(&root.path().join("changed"), 3), false);
    let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
    assert!(matches!(refused(host.confirm_meguri_setup_download(access(&connection, &script), &shown, download.clone(),
        Choice::KeepServer)), E::SetupChanged(Change::Remote)));
    assert_eq!(local_decks(&host), 2);
    assert_eq!(host.manual_transfer_history().active().unwrap().as_ref(), Some(&download.operation));
    assert!(download.operation.safety_backups.is_empty());
    host.close().unwrap();
}

/// A confirmed upload whose answer was lost: acceptance unknown, exclusion held.
fn unknown(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation) {
    let (host, config) = seeded(&root.join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(server_with(&root.join("server"), 2))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))), (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Upload, Reply::Lost)]);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert!(confirm_setup(&host, &connection, &script, &shown, preview, Choice::KeepDevice).is_err());
    script.assert_done();
    let op = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(op.acceptance, Acceptance::PossiblyAccepted);
    (host, config, connection, op)
}
fn settle_with(host: &CoreHost, connection: &MeguriConnection, reply: Reply) -> Result<Settled, TransferError> {
    let script = Script::new(vec![(Method::Meta, reply)]);
    let events = host.engine().stream().subscribe();
    let result = run(host, host.settle_meguri_setup(access(connection, &script)).unwrap(), events);
    script.assert_done();
    for method in [Method::Upload, Method::Download] { assert_eq!(script.count(method), 0); }
    result
}
fn candidate_meta(config: &HostConfig, op: &Operation) -> Vec<u8> {
    let path = config.manual_transfer_directory.join(op.id.as_str()).join(&op.candidate.as_ref().unwrap().name);
    let facts = meguri_storage::one_way::verify_server_copy(&path).unwrap().unwrap();
    meta_with(facts.stamps.modified.0, facts.stamps.schema_modified.0, facts.stamps.usn.0, 0)
}

#[test]
fn settling_accepted_records_the_acceptance_and_keeps_the_exclusion_for_activation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    let Settled::Accepted { operation, local_unchanged } =
        settle_with(&host, &connection, Reply::Bytes(candidate_meta(&config, &op))).unwrap() else { panic!("accepted") };
    assert!(local_unchanged);
    assert_eq!(operation.acceptance, Acceptance::ExactOk);
    assert_eq!(host.manual_transfer_history().active().unwrap(), Some(operation.clone()));
    // The retained recovery path finishes local activation without another upload.
    let offline = Script::new(vec![]);
    let events = host.engine().stream().subscribe();
    let recovery = run(&host, host.prepare_meguri_bootstrap_recovery(&connection, &operation).unwrap(), events).unwrap();
    let done = confirm(&host, &connection, &offline, recovery);
    assert!(done.collection_complete);
    assert_eq!(offline.count(Method::Upload), 0);
    host.close().unwrap();
}

#[test]
fn settling_accepted_reports_a_device_that_changed_since_the_upload() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    add_deck(&host, "Made while waiting");
    let Settled::Accepted { local_unchanged, .. } =
        settle_with(&host, &connection, Reply::Bytes(candidate_meta(&config, &op))).unwrap() else { panic!("accepted") };
    assert!(!local_unchanged);
    host.close().unwrap();
}

#[test]
fn settling_not_accepted_releases_the_exclusion_and_setup_can_run_again() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    let Settled::NotAccepted { operation } =
        settle_with(&host, &connection, Reply::Bytes(meta(NOW - 1000, 8))).unwrap() else { panic!("not accepted") };
    assert_eq!(operation.acceptance, Acceptance::NotSent);
    assert_eq!(operation.phase, Phase::Failed);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    // Candidate, backup and server copy are kept.
    assert!(config.manual_transfer_directory.join(op.id.as_str()).join(&op.candidate.as_ref().unwrap().name).is_file());
    assert!(config.backups_directory.join(&op.server_copy.as_ref().unwrap().artifact.name).is_file());
    host.refresh_provider_admission();
    let script = upload_preparation(server_with(&root.path().join("again"), 2));
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    host.close().unwrap();
}

#[test]
fn settling_neither_or_unreadable_changes_nothing() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config, connection, op) = unknown(root.path());
    assert!(matches!(settle_with(&host, &connection, Reply::Bytes(meta(NOW - 500, 9))).unwrap(), Settled::Neither));
    assert_eq!(host.manual_transfer_history().active().unwrap(), Some(op.clone()));
    assert!(settle_with(&host, &connection, Reply::Lost).is_err());
    assert_eq!(host.manual_transfer_history().active().unwrap(), Some(op.clone()));
    // No writer starts while the state lasts.
    let offline = Script::new(vec![]);
    assert!(host.prepare_meguri_setup(access(&connection, &offline)).is_err());
    offline.assert_done();
    host.close().unwrap();
}

#[test]
fn settling_is_refused_without_an_upload_whose_acceptance_is_unknown() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let offline = Script::new(vec![]);
    assert!(host.settle_meguri_setup(access(&connection, &offline)).is_err());
    let script = upload_preparation(server_with(&root.path().join("server"), 2));
    let _preview = prepare_setup(&host, &connection, &script);
    assert!(host.settle_meguri_setup(access(&connection, &offline)).is_err());
    offline.assert_done();
    host.close().unwrap();
}
