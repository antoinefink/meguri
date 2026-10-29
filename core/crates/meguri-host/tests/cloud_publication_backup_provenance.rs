//! Tests FIRST: the safety receipt is carried into the durable R6 activation,
//! not inferred from lastBackup, directory differences or a driver argument.
use meguri_engine::{
    CancelToken, ProgressSink,
    device_state::DeviceState,
    notes::{self, UpdateNote},
};
use meguri_host::CoreHost;
use meguri_model::{changes::OpSource, ids::NoteId};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_testkit::{
    cloud::devices::options,
    generate::recipe::{Recipe, generate_recipe},
};
const NOW: i64 = 1_735_689_600_000;
fn generate(directory: &std::path::Path) {
    let recipe = Recipe::parse("recipe: replacement-receipt\ndescription: Public synthetic safety provenance\nnow: \"2025-01-01T00:00:00Z\"\ndecks:\n  - name: Generic::Receipt\n    notetype: basic\n    new: 1\n    due: 0\n    tags: []\n    fields: [\"generic {n}\", \"back {n}\"]\n").unwrap();
    generate_recipe(&recipe, 73, directory).unwrap();
}
fn edit(host: &CoreHost, text: &str) {
    let collection = host.collection().unwrap();
    let id = {
        let lease = collection.reader(ReaderRole::Query).unwrap();
        let rows = lease.row_page(Table::Notes, None::<&RowKey>).unwrap();
        meguri_sync::cloud::row_payload::int(&rows[0], 0).unwrap()
    };
    notes::update(
        &collection,
        UpdateNote {
            id: NoteId(id),
            fields: vec![text.into(), "back".into()],
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
}
#[test]
fn restore_activation_keeps_its_owned_safety_receipt_after_later_backups_and_reopen() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generate(&directory);
    let config = options(&directory, NOW, 73);
    let host = CoreHost::open(config.clone()).unwrap();
    let state = DeviceState::for_collection(&directory.join("collection.anki2"));
    let before = state.publication().unwrap().unwrap().identity;
    let source = host.engine().back_up_now(&CancelToken::new()).unwrap();
    edit(&host, "unpublished edit");
    let restored = host
        .restore_backup(&source.receipt.name, &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    let safety = restored.safety_backup.unwrap();
    let evidence = state.replacement_evidence_for(before).unwrap().unwrap();
    assert_eq!(evidence.safety_backup.as_ref().unwrap().name, safety);
    let later = host.engine().back_up_now(&CancelToken::new()).unwrap();
    assert_ne!(later.receipt.name, safety);
    assert_eq!(state.replacement_evidence_for(before).unwrap().unwrap(), evidence);
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    assert_eq!(state.replacement_evidence_for(before).unwrap().unwrap(), evidence);
    reopened.close().unwrap();
}
#[test]
fn successive_restores_record_distinct_safety_backup_evidence() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generate(&directory);
    let config = options(&directory, NOW, 73);
    let host = CoreHost::open(config).unwrap();
    let state = DeviceState::for_collection(&directory.join("collection.anki2"));
    let original = state.publication().unwrap().unwrap().identity;
    let source = host.engine().back_up_now(&CancelToken::new()).unwrap();
    edit(&host, "first displacement");
    let first = host
        .restore_backup(&source.receipt.name, &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    let middle = state.publication().unwrap().unwrap().identity;
    edit(&host, "second displacement");
    let second = host
        .restore_backup(&source.receipt.name, &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    let first_evidence = state.replacement_evidence_for(original).unwrap().unwrap();
    let second_evidence = state.replacement_evidence_for(middle).unwrap().unwrap();
    assert_eq!(first_evidence.safety_backup.unwrap().name, first.safety_backup.unwrap());
    assert_eq!(
        second_evidence.safety_backup.unwrap().name,
        second.safety_backup.unwrap()
    );
    host.close().unwrap();
}
#[test]
fn replacement_without_an_owned_receipt_does_not_borrow_last_backup_provenance() {
    let run = tempfile::tempdir().unwrap();
    let directory = run.path().join("device");
    generate(&directory);
    let config = options(&directory, NOW, 73);
    let host = CoreHost::open(config.clone()).unwrap();
    let state = DeviceState::for_collection(&directory.join("collection.anki2"));
    let before = state.publication().unwrap().unwrap().identity;
    let backup_result = host.engine().back_up_now(&CancelToken::new()).unwrap();
    assert!(state.last_backup().unwrap().is_some());
    let inspected = meguri_package::inspect_collection_package(
        &config.backups_directory.join(&backup_result.receipt.name),
        &meguri_package::CollectionPackageInspectContext {
            collection_directory: &directory,
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: config.clock.as_ref(),
            random: config.random.as_ref(),
            space: config.disk_space_provider.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    let stage =
        meguri_engine::replace::ReplacementStage::from_staging(inspected.into_staged().into_directory()).unwrap();
    // Exercise the legacy primitive that has no provider-owned receipt. This
    // establishes no safety-backup evidence for a product restore.
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    let mut exclusive = slot.enter_exclusive(true).unwrap();
    exclusive.restore_verified_before_commit(stage, || Ok(None)).unwrap();
    drop(exclusive);
    drop(slot);
    assert!(
        state
            .replacement_evidence_for(before)
            .unwrap()
            .unwrap()
            .safety_backup
            .is_none()
    );
    host.close().unwrap();
    let reopened = CoreHost::open(config).unwrap();
    assert!(
        state
            .replacement_evidence_for(before)
            .unwrap()
            .unwrap()
            .safety_backup
            .is_none()
    );
    reopened.close().unwrap();
}
