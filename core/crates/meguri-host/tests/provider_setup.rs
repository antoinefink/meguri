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
    policy::{Acceptance, Boundary, CopyKind, Direction, Operation, Phase},
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
fn an_unreadable_server_leaves_the_acceptance_unknown_and_the_exclusion_held() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    assert!(settle_with(&host, &connection, Reply::Lost).is_err());
    assert_eq!(host.manual_transfer_history().active().unwrap(), Some(op.clone()));
    // No writer starts while the state lasts, and the record cannot be discarded.
    let offline = Script::new(vec![]);
    assert!(host.prepare_meguri_setup(access(&connection, &offline)).is_err());
    assert!(host.discard_meguri_bootstrap_preparation(&connection, &op).is_err());
    offline.assert_done();
    assert_eq!(artifacts(&config, &op), [true; 4]);
    host.close().unwrap();
}

/// Whether each artifact of a record is on disk: the retained candidate, the
/// private copy of the safety backup, the server copy, every published safety backup.
fn artifacts(config: &HostConfig, op: &Operation) -> [bool; 4] {
    let owned = config.manual_transfer_directory.join(op.id.as_str());
    [
        op.candidate.as_ref().is_some_and(|artifact| owned.join(&artifact.name).is_file()),
        op.original.as_ref().is_some_and(|artifact| owned.join(&artifact.name).is_file()),
        op.server_copy.as_ref().is_some_and(|copy| config.backups_directory.join(&copy.artifact.name).is_file()),
        !op.safety_backups.is_empty()
            && op.safety_backups.iter().all(|backup| config.backups_directory.join(&backup.name).is_file()),
    ]
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

// Review fixes of 2026-11-19 (S1 to S4, N3, N4).

fn recovery_of(host: &CoreHost, connection: &MeguriConnection, operation: &Operation) -> Preview {
    let events = host.engine().stream().subscribe();
    run(host, host.prepare_meguri_bootstrap_recovery(connection, operation).unwrap(), events).unwrap()
}
fn accepted(host: &CoreHost, config: &HostConfig, connection: &MeguriConnection, op: &Operation) -> Operation {
    let Settled::Accepted { operation, local_unchanged } =
        settle_with(host, connection, Reply::Bytes(candidate_meta(config, op))).unwrap() else { panic!("accepted") };
    assert!(local_unchanged);
    operation
}
fn reopened(host: CoreHost, config: &HostConfig) -> (CoreHost, MeguriConnection) {
    host.close().unwrap();
    let host = CoreHost::open(config.clone()).unwrap();
    let connection = host.provider_authority().snapshot().unwrap().meguri.unwrap();
    (host, connection)
}

// S1: the settled "unchanged" is a hint. The core decides where the replacement happens.
#[test]
fn accepted_activation_with_no_question_completes_on_the_device_the_upload_sent() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    let operation = accepted(&host, &config, &connection, &op);
    let recovery = recovery_of(&host, &connection, &operation);
    let offline = Script::new(vec![]);
    let events = host.engine().stream().subscribe();
    let done = run(&host, host.confirm_meguri_setup_activation(access(&connection, &offline), recovery).unwrap(), events).unwrap();
    offline.assert_done();
    assert!(done.collection_complete);
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

#[test]
fn an_edit_between_settling_and_finishing_is_refused_and_left_to_the_users_own_finish() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    // Settled while this device was still what the upload sent...
    let operation = accepted(&host, &config, &connection, &op);
    // ...and edited before the activation started.
    add_deck(&host, "Made after settling");
    let recovery = recovery_of(&host, &connection, &operation);
    let offline = Script::new(vec![]);
    assert!(matches!(refused(host.confirm_meguri_setup_activation(access(&connection, &offline), recovery.clone())),
        E::SetupChanged(Change::Local)));
    // Nothing was backed up or replaced, and the acceptance and the exclusion stand.
    assert_eq!(local_decks(&host), 3);
    let kept = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(kept, recovery.operation);
    assert_eq!(kept.acceptance, Acceptance::ExactOk);
    assert_eq!(kept.safety_backups, op.safety_backups);
    assert_eq!(artifacts(&config, &kept), [true; 4]);
    // The user's own Finish Setup is the retained confirmation of the same preview.
    let done = confirm(&host, &connection, &offline, recovery);
    assert!(done.collection_complete);
    assert!(done.safety_backups.len() > op.safety_backups.len());
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

#[test]
fn an_edit_after_the_recovery_preparation_stops_the_activation_with_nothing_replaced() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    let operation = accepted(&host, &config, &connection, &op);
    let recovery = recovery_of(&host, &connection, &operation);
    add_deck(&host, "Made after the preparation");
    let offline = Script::new(vec![]);
    let events = host.engine().stream().subscribe();
    let result = run(&host, host.confirm_meguri_setup_activation(access(&connection, &offline), recovery).unwrap(), events);
    assert!(matches!(result, Err(TransferError::Stale(Boundary::LocalChanged))));
    assert_eq!(local_decks(&host), 3);
    let kept = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(kept.acceptance, Acceptance::ExactOk);
    assert_eq!(kept.safety_backups, op.safety_backups);
    host.close().unwrap();
}

#[test]
fn a_change_that_leaves_the_facts_equal_is_found_inside_the_exclusive_phase() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    let operation = accepted(&host, &config, &connection, &op);
    // A write that moves no collection stamp and no pending count: the facts stay equal.
    let stamp = EpochSecs(NOW / 1000 + 600);
    host.collection().unwrap().run(
        meguri_engine::OpSpec::named("synthetic raw write", meguri_engine::OpSource::Rollover).stamp(StampMode::Raw),
        move |context| {
            let scope = context.scope();
            let mut deck = scope.deck(meguri_model::ids::DeckId(1))?.expect("default deck");
            deck.mtime = stamp;
            scope.put(&deck)?;
            Ok(())
        },
    ).unwrap();
    let recovery = recovery_of(&host, &connection, &operation);
    assert!(recovery.operation.local.is_some());
    assert_eq!(recovery.operation.recovery_local, recovery.operation.local);
    let offline = Script::new(vec![]);
    let events = host.engine().stream().subscribe();
    let result = run(&host, host.confirm_meguri_setup_activation(access(&connection, &offline), recovery).unwrap(), events);
    assert!(matches!(result, Err(TransferError::Stale(Boundary::LocalChanged))));
    let kept = host.manual_transfer_history().active().unwrap().unwrap();
    assert_eq!(kept.acceptance, Acceptance::ExactOk);
    assert_eq!(kept.safety_backups, op.safety_backups);
    let collection = host.collection().unwrap();
    let reader = collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.deck(meguri_model::ids::DeckId(1)).unwrap().unwrap().mtime, stamp);
    drop(reader);
    host.close().unwrap();
}

#[test]
fn activation_with_no_question_is_only_for_an_accepted_upload() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let script = upload_preparation(server_with(&root.path().join("server"), 2));
    let preview = prepare_setup(&host, &connection, &script);
    let offline = Script::new(vec![]);
    assert!(host.confirm_meguri_setup_activation(access(&connection, &offline), preview).is_err());
    offline.assert_done();
    assert_eq!(script.count(Method::Upload), 0);
    host.close().unwrap();
}

// S2: "neither" is recorded, releases the exclusion and leads to the sheet.
fn overtaken(root: &Path) -> (CoreHost, HostConfig, MeguriConnection, Operation) {
    let (host, config, connection, op) = unknown(root);
    let Settled::Neither { operation } = settle_with(&host, &connection, Reply::Bytes(meta(NOW - 500, 9))).unwrap()
        else { panic!("neither") };
    assert_eq!(operation.id, op.id);
    assert_eq!(operation.acceptance, Acceptance::Overtaken);
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert_eq!(artifacts(&config, &operation), [true; 4]);
    // The state is durable: a new process finds it and no pending preparation.
    let (host, connection) = reopened(host, &config);
    settle(&host);
    assert_eq!(host.manual_transfer_history().get(&op.id).unwrap(), operation);
    assert!(host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().is_none());
    (host, config, connection, operation)
}

#[test]
fn settling_neither_records_it_and_each_answer_of_the_sheet_supersedes_the_upload() {
    let _lane = LANE.lock().unwrap();
    for answer in ["device", "server", "cancel"] {
        let root = tempfile::tempdir().unwrap();
        let (host, config, connection, old) = overtaken(root.path());
        // The server is proved to hold no user content, which would be row 3
        // on a first binding. After an overtaken upload it is always the sheet.
        let data = server_with(&root.path().join("again"), 0);
        let script = collection_script(data.clone(), true);
        let preview = prepare_setup(&host, &connection, &script);
        let shown = decide(&host, &connection, &preview);
        assert_eq!(shown.row, Row::Ask, "{answer}");
        assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(),
            Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
        assert_eq!(script.count(Method::Upload), 0);
        match answer {
            "device" => {
                // A fresh, confirmed upload.
                let op = confirm_setup(&host, &connection, &script, &shown, preview, Choice::KeepDevice).unwrap();
                script.assert_done();
                assert_ne!(op.id, old.id);
                assert_eq!(op.acceptance, Acceptance::ExactOk);
                assert!(op.collection_complete);
            }
            "server" => {
                // A download, with this device backed up first.
                let script = collection_script(data, false);
                let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
                let events = host.engine().stream().subscribe();
                let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
                    Choice::KeepServer).unwrap(), events).unwrap();
                script.assert_done();
                assert_eq!(script.count(Method::Upload), 0);
                assert!(op.collection_complete && !op.safety_backups.is_empty());
                assert_eq!(local_decks(&host), 1);
            }
            _ => {
                // Cancel: the preview goes, the service can go to Off, and nothing stays excluded.
                host.discard_meguri_bootstrap_preparation(&connection, &preview.operation).unwrap();
                assert_eq!(local_decks(&host), 2);
                let handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
                host.finish_provider_handoff(&handoff).unwrap();
                let connection = select(&host);
                let script = upload_preparation(server_with(&root.path().join("later"), 0));
                let preview = prepare_setup(&host, &connection, &script);
                assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
            }
        }
        // Every artifact of the overtaken upload is kept, whatever the answer.
        assert_eq!(artifacts(&config, &old), [true; 4], "{answer}");
        assert_eq!(host.manual_transfer_history().get(&old.id).unwrap(), old);
        host.close().unwrap();
    }
}

// MP6.9 "not accepted" releases the exclusion on one read. When the server
// commits that upload afterwards, the next preparation finds a server that
// holds user content: the sheet, and no transfer before its answer.
#[test]
fn a_late_server_commit_after_not_accepted_gives_the_sheet_at_the_next_preparation() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config, connection, op) = unknown(root.path());
    assert!(matches!(settle_with(&host, &connection, Reply::Bytes(meta(NOW - 1000, 8))).unwrap(), Settled::NotAccepted { .. }));
    host.refresh_provider_admission();
    let sent = fs::read(config.manual_transfer_directory.join(op.id.as_str()).join(&op.candidate.as_ref().unwrap().name)).unwrap();
    let late = candidate_meta(&config, &op);
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(late.clone())), (Method::Download, Reply::Bytes(sent)),
        (Method::Meta, Reply::Bytes(late))]);
    let preview = prepare_setup(&host, &connection, &script);
    script.assert_done();
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Ask);
    assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(),
        Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
    assert!(matches!(refused(host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview,
        Choice::NoQuestion)), E::SetupChanged(Change::Choice)));
    assert_eq!(script.count(Method::Upload), 0);
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

// S3: a retained server copy without facts.
fn stamps_meta(facts: &meguri_storage::one_way::LocalTransferFacts) -> Vec<u8> {
    format!(
        r#"{{"mod":{},"scm":{},"usn":{},"ts":{},"msg":"","cont":true,"empty":{},"media_usn":0}}"#,
        facts.stamps.modified.0, facts.stamps.schema_modified.0, facts.stamps.usn.0, NOW / 1000, !facts.has_cards()
    )
    .into_bytes()
}
/// A served collection of a legacy schema, the facts of the file that a
/// download makes of it, and the meta answer a server holding it gives.
fn legacy_server(root: &Path) -> (Vec<u8>, meguri_storage::one_way::LocalTransferFacts, Vec<u8>) {
    let served = fs::read(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-11.anki2")).unwrap();
    fs::create_dir_all(root).unwrap();
    let stage = meguri_storage::verified_replace::ReplacementStage::new(root).unwrap();
    fs::write(stage.path(), &served).unwrap();
    let clock = FixedClock::new(NOW, UtcOffset::UTC);
    let random = SeededRandom::new(918);
    meguri_storage::verified_replace::prepare(&stage, &root.join("collection.anki2"), &OpenOptions::new(&clock, &random),
        &clock, false, |scope| {
            let mut col = scope.col()?;
            col.last_sync = col.modified;
            scope.put_col(&col).map(|_| ())
        }).unwrap();
    let facts = meguri_storage::one_way::verify_server_copy(&stage.path()).unwrap().unwrap();
    (served, facts, stamps_meta(&facts))
}
fn legacy_upload_preparation(served: &[u8], answer: &[u8]) -> Arc<Script> {
    Script::new(vec![(Method::Meta, Reply::Bytes(answer.to_vec())), (Method::Download, Reply::Bytes(served.to_vec())),
        (Method::Meta, Reply::Bytes(answer.to_vec()))])
}
fn legacy_download(served: &[u8], answer: &[u8]) -> Arc<Script> {
    Script::new(vec![(Method::Meta, Reply::Bytes(answer.to_vec())), (Method::Download, Reply::Bytes(served.to_vec())),
        (Method::Meta, Reply::Bytes(answer.to_vec())), (Method::Meta, Reply::Bytes(answer.to_vec()))])
}
fn local_facts(host: &CoreHost) -> meguri_storage::one_way::LocalTransferFacts {
    host.collection().unwrap().reader(ReaderRole::Query).unwrap().transfer_facts().unwrap()
}

#[test]
fn row_2_downloads_a_readable_server_collection_whose_retained_copy_has_no_facts() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let (served, facts, answer) = legacy_server(&root.path().join("legacy"));
    let script = legacy_upload_preparation(&served, &answer);
    let preview = prepare_setup(&host, &connection, &script);
    script.assert_done();
    let copy = preview.operation.server_copy.as_ref().unwrap();
    assert_eq!(copy.kind, CopyKind::VerifiedPackage);
    assert!(copy.served_facts.is_none());
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::DownloadOverEmptyDevice);
    assert!(shown.server.is_none());
    let script = legacy_download(&served, &answer);
    let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::NoQuestion);
    let events = host.engine().stream().subscribe();
    let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
        Choice::NoQuestion).unwrap(), events).unwrap();
    script.assert_done();
    assert!(op.collection_complete && !op.safety_backups.is_empty());
    assert_eq!(local_facts(&host).totals, facts.totals);
    host.close().unwrap();
}

#[test]
fn keeping_a_server_collection_whose_retained_copy_has_no_facts_compares_its_content() {
    let _lane = LANE.lock().unwrap();
    for changed in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let (host, _config) = seeded(&root.path().join("local"));
        add_deck(&host, "Mine");
        let connection = select(&host);
        let (served, facts, answer) = legacy_server(&root.path().join("legacy"));
        let script = legacy_upload_preparation(&served, &answer);
        let preview = prepare_setup(&host, &connection, &script);
        let shown = decide(&host, &connection, &preview);
        // A legacy schema never proves an empty server.
        assert_eq!(shown.row, Row::Ask);
        assert!(shown.server.is_none());
        if changed {
            // Another collection is on the server by the time of the download.
            let script = collection_script(server_with(&root.path().join("changed"), 3), false);
            let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
            assert!(matches!(refused(host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
                Choice::KeepServer)), E::SetupChanged(Change::Remote)));
            assert_eq!(local_decks(&host), 2);
        } else {
            let script = legacy_download(&served, &answer);
            let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
            let events = host.engine().stream().subscribe();
            let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
                Choice::KeepServer).unwrap(), events).unwrap();
            script.assert_done();
            assert!(op.collection_complete && !op.safety_backups.is_empty());
            assert_eq!(local_facts(&host).totals, facts.totals);
        }
        host.close().unwrap();
    }
}

#[test]
fn a_server_collection_that_cannot_be_read_is_never_downloaded_and_the_failure_is_typed() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let served = b"synthetic damaged server SQLite".to_vec();
    let script = upload_preparation(served.clone());
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(preview.operation.server_copy.as_ref().unwrap().kind, CopyKind::UnverifiedRaw);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Ask);
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))), (Method::Download, Reply::Bytes(served))]);
    let events = host.engine().stream().subscribe();
    let result = run(&host, host.prepare_meguri_setup_download(access(&connection, &script), &shown, preview.clone(),
        Choice::KeepServer).unwrap(), events);
    assert!(matches!(result, Err(TransferError::InvalidDownload(_))));
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert_eq!(local_decks(&host), 2);
    // The discarded preview's raw copy of what the server served went with it.
    assert_eq!(artifacts(&config, &preview.operation), [false, false, false, true]);
    host.close().unwrap();
}

// S4: what a discarded preview owned goes with it; nothing else does.
#[test]
fn a_discarded_preview_takes_its_own_artifacts_with_it_and_nothing_else() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    // A completed setup, then Off, then Meguri Sync again.
    let (host, config, _connection, done) = ready(root.path());
    assert_eq!(artifacts(&config, &done), [true; 4]);
    let handoff = host.begin_provider_handoff(Selection::LocalOnly).unwrap();
    host.finish_provider_handoff(&handoff).unwrap();
    let connection = select(&host);
    add_deck(&host, "Mine");
    for _ in 0..2 {
        let script = upload_preparation(server_with(&root.path().join("again"), 2));
        let preview = prepare_setup(&host, &connection, &script);
        assert_eq!(artifacts(&config, &preview.operation), [true; 4]);
        let cancelled = host.discard_meguri_bootstrap_preparation(&connection, &preview.operation).unwrap();
        assert_eq!(cancelled.phase, Phase::Cancelled);
        // The published safety backup is an ordinary backup and stays.
        assert_eq!(artifacts(&config, &cancelled), [false, false, false, true]);
        assert!(!config.manual_transfer_directory.join(cancelled.id.as_str()).exists());
        // The record itself is history and stays.
        assert_eq!(host.manual_transfer_history().get(&cancelled.id).unwrap(), cancelled);
        // What the completed setup retained is untouched.
        assert_eq!(artifacts(&config, &done), [true; 4]);
        // A record that was already discarded gives nothing more to remove.
        assert!(host.discard_meguri_bootstrap_preparation(&connection, &cancelled).is_err());
        fs::remove_dir_all(root.path().join("again")).unwrap();
    }
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

#[test]
fn keeping_the_server_collection_leaves_one_download_and_the_backup_of_this_device() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    let script = collection_script(data, false);
    let download = prepare_download(&host, &connection, &script, &shown, preview.clone(), Choice::KeepServer);
    // The upload's preview sent nothing: its candidate and its server copy are gone.
    assert_eq!(artifacts(&config, &preview.operation), [false, false, false, true]);
    let events = host.engine().stream().subscribe();
    let op = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
        Choice::KeepServer).unwrap(), events).unwrap();
    assert!(op.collection_complete);
    // The backup taken for the replacement that ran, and the file that was activated, stay.
    let owned = config.manual_transfer_directory.join(op.id.as_str());
    assert!(owned.join(&op.candidate.as_ref().unwrap().name).is_file());
    assert!(!op.safety_backups.is_empty());
    for backup in &op.safety_backups { assert!(config.backups_directory.join(&backup.name).is_file()); }
    assert_eq!(local_decks(&host), 3);
    host.close().unwrap();
}

// The arming of the exclusive-phase test in `confirm_meguri_bootstrap_retesting`.
#[test]
fn row_2_tests_this_device_again_inside_the_exclusive_phase() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, _config) = seeded(&root.path().join("local"));
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::DownloadOverEmptyDevice);
    let media = host.media().unwrap().store().media_dir().to_owned();
    let late = media.join("late.mp3");
    // The confirmation's own read of the server is made inside the exclusive
    // phase, after every check of this device that precedes it. A media file
    // that arrives then is seen only by the test armed for that phase.
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))), (Method::Download, Reply::Bytes(data)),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::After(Box::new(move || {
            fs::create_dir_all(&media).unwrap();
            fs::write(media.join("late.mp3"), b"synthetic").unwrap();
        }), meta(NOW - 1000, 8)))]);
    let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::NoQuestion);
    let events = host.engine().stream().subscribe();
    let result = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download.clone(),
        Choice::NoQuestion).unwrap(), events);
    script.assert_done();
    assert!(late.is_file());
    assert!(matches!(result, Err(TransferError::Stale(Boundary::LocalChanged))));
    // No backup, no activation: the collection on this device is the one it had.
    assert_eq!(local_decks(&host), 1);
    let kept = host.manual_transfer_history().get(&download.operation.id).unwrap();
    assert!(kept.safety_backups.is_empty() && !kept.collection_complete);
    assert_eq!(kept.acceptance, Acceptance::NotSent);
    host.close().unwrap();
}

#[test]
fn an_interruption_between_the_download_preparation_and_its_confirmation_replaces_nothing() {
    let _lane = LANE.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let (host, config) = seeded(&root.path().join("local"));
    add_deck(&host, "Mine");
    let connection = select(&host);
    let data = server_with(&root.path().join("server"), 2);
    let script = upload_preparation(data.clone());
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    let script = collection_script(data.clone(), false);
    let download = prepare_download(&host, &connection, &script, &shown, preview, Choice::KeepServer);
    // The process ends here: the answer was given, the download is prepared, nothing is confirmed.
    let (host, connection) = reopened(host, &config);
    settle(&host);
    assert_eq!(local_decks(&host), 2);
    let pending = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    assert_eq!(pending, download.operation);
    assert!(pending.safety_backups.is_empty());
    // No other preparation starts beside it.
    let offline = Script::new(vec![]);
    assert!(matches!(refused(host.prepare_meguri_setup(access(&connection, &offline))), E::Busy));
    offline.assert_done();
    // The earlier process's answer does not carry over: its guard names that process.
    let events = host.engine().stream().subscribe();
    let result = run(&host, host.confirm_meguri_setup_download(access(&connection, &script), &shown, download,
        Choice::KeepServer).unwrap(), events);
    assert!(matches!(result, Err(TransferError::Stale(Boundary::LocalChanged))));
    assert_eq!(script.count(Method::Meta), 2);
    assert_eq!(local_decks(&host), 2);
    // It is discarded, with its download, and setup asks again.
    let pending = host.pending_meguri_bootstrap_preparation(&connection, USER).unwrap().unwrap();
    let cancelled = host.discard_meguri_bootstrap_preparation(&connection, &pending).unwrap();
    assert!(!config.manual_transfer_directory.join(cancelled.id.as_str()).exists());
    let script = upload_preparation(data);
    let preview = prepare_setup(&host, &connection, &script);
    assert_eq!(decide(&host, &connection, &preview).row, Row::Ask);
    assert_eq!(local_decks(&host), 2);
    host.close().unwrap();
}

// N3: media files on a device with no collection are that device's content.
#[test]
fn row_1_is_not_taken_while_the_media_folder_holds_files_or_cannot_be_read() {
    let _lane = LANE.lock().unwrap();
    // A file in the folder: the one-direction question, and its answer downloads.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = no_collection(&root.path().join("local"));
    let media = config.collection_directory.join("collection.media");
    fs::create_dir_all(&media).unwrap();
    fs::write(media.join("sound.mp3"), b"synthetic").unwrap();
    let connection = select(&host);
    let script = collection_script(server_with(&root.path().join("server"), 2), false);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::AskDownloadOnly);
    assert_eq!(shown.device.unwrap().media_files, Some(1));
    for choice in [Choice::NoQuestion, Choice::KeepDevice] {
        assert!(matches!(refused(host.confirm_meguri_setup(access(&connection, &script), &shown, preview.clone(), choice)),
            E::SetupChanged(Change::Choice)));
    }
    assert!(host.collection().is_err());
    let op = confirm_setup(&host, &connection, &script, &shown, preview, Choice::KeepServer).unwrap();
    script.assert_done();
    assert!(op.collection_complete);
    assert_eq!(local_decks(&host), 3);
    assert!(media.join("sound.mp3").is_file());
    host.close().unwrap();

    // A folder that cannot be read is content too.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = no_collection(&root.path().join("local"));
    fs::write(config.collection_directory.join("collection.media"), b"not a folder").unwrap();
    let connection = select(&host);
    let script = collection_script(server_with(&root.path().join("server"), 2), false);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::AskDownloadOnly);
    assert_eq!(shown.device.unwrap().media_files, None);
    host.close().unwrap();

    // A file that arrives after row 1 was decided stops the download where it would activate.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = no_collection(&root.path().join("local"));
    let media = config.collection_directory.join("collection.media");
    let connection = select(&host);
    let script = Script::new(vec![(Method::Meta, Reply::Bytes(meta(NOW - 2000, 7))),
        (Method::Download, Reply::Bytes(server_with(&root.path().join("server"), 2))),
        (Method::Meta, Reply::Bytes(meta(NOW - 1000, 8))),
        (Method::Meta, Reply::After(Box::new(move || {
            fs::create_dir_all(&media).unwrap();
            fs::write(media.join("late.mp3"), b"synthetic").unwrap();
        }), meta(NOW - 1000, 8)))]);
    let preview = prepare_setup(&host, &connection, &script);
    let shown = decide(&host, &connection, &preview);
    assert_eq!(shown.row, Row::Download);
    assert!(confirm_setup(&host, &connection, &script, &shown, preview, Choice::NoQuestion).is_err());
    script.assert_done();
    assert!(host.collection().is_err());
    assert!(!config.collection_directory.join("collection.anki2").exists());
    host.close().unwrap();
}

// N4: a collection file that is present but not open is neither row 1 nor a readable collection.
#[test]
fn a_closed_but_present_collection_is_refused_as_unusable_before_any_request() {
    let _lane = LANE.lock().unwrap();
    let clock = FixedClock::new(NOW, UtcOffset::UTC);
    let random = SeededRandom::new(77);
    // Present before preparation.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = no_collection(&root.path().join("local"));
    let connection = select(&host);
    let path = config.collection_directory.join("collection.anki2");
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let before = fs::read(&path).unwrap();
    let offline = Script::new(vec![]);
    assert!(matches!(refused(host.prepare_meguri_setup(access(&connection, &offline))), E::LocalUnusable));
    offline.assert_done();
    assert!(host.manual_transfer_history().active().unwrap().is_none());
    assert_eq!(fs::read(&path).unwrap(), before);
    host.close().unwrap();

    // It appears after row 1's preparation: the decision is refused the same way, and nothing replaces it.
    let root = tempfile::tempdir().unwrap();
    let (host, config) = no_collection(&root.path().join("local"));
    let connection = select(&host);
    let script = collection_script(server_with(&root.path().join("server"), 2), false);
    let preview = prepare_setup(&host, &connection, &script);
    let path = config.collection_directory.join("collection.anki2");
    meguri_engine::bootstrap::create(&path, &clock, &random).unwrap();
    let before = fs::read(&path).unwrap();
    assert!(matches!(refused(host.meguri_setup_decision(&connection, USER, &preview)), E::LocalUnusable));
    assert_eq!(script.count(Method::Meta), 2);
    assert_eq!(fs::read(&path).unwrap(), before);
    host.close().unwrap();
}
