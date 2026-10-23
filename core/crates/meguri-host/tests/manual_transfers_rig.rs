//! Opt-in local official-server evidence. Run only through tools/mg with the
//! same-checkout local rig up. These tests never select a public service.
#[path = "../../meguri-sync/tests/common/mod.rs"]
mod rig_common;
use meguri_engine::{
    EventKind, OpSource, OpSpec, Subscription,
    device_state::DeviceState,
    replace::{ReplaceReason, ReplacementStage},
};
use meguri_host::{CoreHost, HostConfig, ManualTransferTask};
use meguri_model::{
    clock::{Clock, FixedClock, UtcOffset},
    config::ConfigEntry,
    grave::GraveKind,
    ids::{CardId, RevlogId},
    revlog::{RevlogEntry, RevlogKind},
    rng::SeededRandom,
    time::EpochSecs,
    usn::Usn,
};
use meguri_storage::{ReadRows, ReaderRole, StampMode, space::StatvfsSpace};
use meguri_sync::{
    Job, Method, Payload, login_blocking,
    oneway::{TransferAccess, TransferError},
    policy::{Direction, Phase, Scope},
};
use std::{fs, path::Path, sync::Arc};
fn finish<T>(events: Subscription, task: ManualTransferTask<T>) -> Result<T, TransferError> {
    loop {
        let event = events.recv().unwrap();
        if matches!(event.kind, EventKind::JobFinished { id, .. } if id == task.job.id) {
            break;
        }
    }
    task.take_result().unwrap()
}
fn config(root: &Path, clock: Arc<dyn Clock>, seed: u64) -> HostConfig {
    let collection = root.join("collection");
    fs::create_dir_all(&collection).unwrap();
    HostConfig::new(
        collection,
        root.join("backups"),
        root.join("widget"),
        clock,
        Arc::new(SeededRandom::new(seed)),
        Arc::new(StatvfsSpace),
    )
}
#[test]
#[ignore = "requires tools/mg sync-rig up; mutates only the local rig test account"]
fn rig_rf4_collection_export_import_server_copy_restore_and_declared_transforms() {
    let rig = rig_common::rig();
    let endpoint = rig_common::base(&rig.base);
    let random = SeededRandom::new(109);
    let options = rig.options();
    let key = login_blocking(rig_common::threads(), &options, &random, &rig.user, &rig.password).unwrap();
    // The official server supplies time; no wall clock or external source.
    let mut probe = Job::new(&options, Some(key.clone()), &random).unwrap();
    let bytes = rig_common::block_on(async {
        probe
            .request(
                Method::Meta,
                Payload::Json(meguri_sync::wire::meta::encode_meta_request()),
            )
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap()
    });
    let meta = meguri_sync::wire::meta::decode_meta(&bytes).unwrap();
    meguri_sync::transport::runtime::shut_down(); // next runtime belongs to the real host registry.
    let now = meta.server_time.0 * 1000;
    let clock = Arc::new(FixedClock::new(now, UtcOffset::UTC));
    let root = tempfile::tempdir().unwrap();
    let outgoing = config(&root.path().join("outgoing"), clock.clone(), 119);
    meguri_engine::bootstrap::create(
        &outgoing.collection_directory.join("collection.anki2"),
        outgoing.clock.as_ref(),
        outgoing.random.as_ref(),
    )
    .unwrap();
    let host = CoreHost::open(outgoing.clone()).unwrap();
    let c = host.collection().unwrap();
    c.run(
        OpSpec::named("rig transfer rows", OpSource::Maintenance).stamp(StampMode::Raw),
        move |ctx| {
            let scope = ctx.scope();
            scope.put_config_entry(&ConfigEntry {
                key: "RF4-rig-pending-config".into(),
                value: br#"{"declared":1}"#.to_vec(),
                usn: Usn(-1),
                mtime: EpochSecs(1234),
            })?;
            scope.add_grave(987654321, GraveKind::Note, Usn(-1))?;
            scope.put(&RevlogEntry {
                id: RevlogId(now - 10),
                card_id: CardId(12345),
                usn: Usn(-1),
                button: 3,
                interval: 1,
                last_interval: 0,
                ease_factor: 2500,
                answer_millis: 900,
                kind: RevlogKind::Review,
            })?;
            Ok(())
        },
    )
    .unwrap();
    let original_usn = c.reader(ReaderRole::Sync).unwrap().transfer_facts().unwrap().usn.0;
    let access = || TransferAccess::new(endpoint.clone(), &rig.user, key.clone(), rig.options()).unwrap();
    let events = host.engine().stream().subscribe();
    let task = host
        .prepare_manual_transfer(access(), Direction::Export, Scope::CollectionOnly)
        .unwrap();
    let preview = finish(events, task).unwrap();
    let server_copy = preview
        .operation
        .server_copy
        .clone()
        .expect("every served file was preserved");
    let exact = preview.operation.candidate.as_ref().unwrap().identity.sha256;
    let events = host.engine().stream().subscribe();
    let task = host.confirm_manual_transfer(access(), preview.confirmation).unwrap();
    let done = finish(events, task).unwrap();
    assert_eq!(done.phase, Phase::Complete);
    assert_eq!(
        DeviceState::for_collection(c.path())
            .replacement_evidence()
            .unwrap()
            .unwrap()
            .candidate_file
            .sha256,
        exact
    );
    let facts = host
        .collection()
        .unwrap()
        .reader(ReaderRole::Sync)
        .unwrap()
        .transfer_facts()
        .unwrap();
    assert_eq!(
        (facts.modified.0, facts.schema_modified.0, facts.last_sync.0),
        (now, now, now)
    );
    assert_eq!(facts.usn.0, original_usn + 1);
    assert_eq!(facts.pending_reviews, 0);
    assert_eq!(facts.pending_rows.graves, 0);
    // Independent installation: the successful Export's shared cooldown is
    // intentionally still enforced on the first installation.
    meguri_sync::transport::runtime::shut_down(); // next network job uses the downloader's registry.
    let incoming = config(&root.path().join("incoming"), clock, 121);
    meguri_engine::bootstrap::create(
        &incoming.collection_directory.join("collection.anki2"),
        incoming.clock.as_ref(),
        incoming.random.as_ref(),
    )
    .unwrap();
    let downloader = CoreHost::open(incoming.clone()).unwrap();
    let events = downloader.engine().stream().subscribe();
    let task = downloader
        .prepare_manual_transfer(access(), Direction::Import, Scope::CollectionOnly)
        .unwrap();
    let preview = finish(events, task).unwrap();
    let events = downloader.engine().stream().subscribe();
    let task = downloader
        .confirm_manual_transfer(access(), preview.confirmation)
        .unwrap();
    assert_eq!(finish(events, task).unwrap().phase, Phase::Complete);
    let imported = downloader.collection().unwrap();
    let reader = imported.reader(ReaderRole::Sync).unwrap();
    let facts = reader.transfer_facts().unwrap();
    assert_eq!(facts.reviews, 1);
    assert_eq!(facts.pending_reviews, 0);
    assert_eq!(facts.pending_rows.graves, 0);
    assert_eq!(facts.schema_modified.0, now);
    assert_eq!(facts.last_sync, facts.modified);
    assert_eq!(facts.usn.0, original_usn + 2);
    let config = reader.config_entry("RF4-rig-pending-config").unwrap().unwrap();
    assert_eq!(config.entry.usn, Usn(-1));
    assert_eq!(config.entry.mtime, EpochSecs(1234));
    assert_eq!(reader.revlog_entry(RevlogId(now - 10)).unwrap().unwrap().usn, Usn(0));
    drop(reader);
    downloader.close().unwrap();
    // Restore the server copy saved by the real backend through the real
    // package inspection/replacement path, independent of uploaded-file hashes.
    assert_eq!(server_copy.kind, meguri_sync::policy::CopyKind::VerifiedPackage);
    let inspected = meguri_package::inspect_collection_package(
        &outgoing.backups_directory.join(&server_copy.artifact.name),
        &meguri_package::CollectionPackageInspectContext {
            collection_directory: &outgoing.collection_directory,
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: outgoing.clock.as_ref(),
            random: outgoing.random.as_ref(),
            space: outgoing.disk_space_provider.as_ref(),
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    let saved_counts = (
        inspected.facts().cards,
        inspected.facts().notes,
        inspected.facts().revlog,
    );
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.replace_verified(
        ReplacementStage::from_staging(inspected.into_staged().into_directory()).unwrap(),
        ReplaceReason::Restore,
        |_| Ok(()),
    )
    .unwrap();
    drop(slot);
    let facts = host
        .collection()
        .unwrap()
        .reader(ReaderRole::Sync)
        .unwrap()
        .transfer_facts()
        .unwrap();
    assert_eq!((facts.cards, facts.notes, facts.reviews), saved_counts);
    host.close().unwrap();
}
