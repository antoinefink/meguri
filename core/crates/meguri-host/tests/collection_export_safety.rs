//! RF3 E2/E3/E5: real backups, path aliases, preflight and narrow recovery.
use meguri_engine::{CancelToken, EngineError, ProgressSink};
use meguri_host::{CoreHost, HostConfig, remove_collection_export};
use meguri_model::{
    clock::{FixedClock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::{
    StorageError,
    space::{SpaceProvider, StatvfsSpace},
};
use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn config(root: &Path, backups: PathBuf, space: Arc<dyn SpaceProvider>) -> HostConfig {
    let config = HostConfig::new(
        root.join("collection"),
        backups,
        root.join("widget"),
        Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(31)),
        space,
    );
    fs::create_dir_all(&config.collection_directory).unwrap();
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    config
}
fn export(host: &CoreHost) -> Result<meguri_host::WrittenCollectionExport, EngineError> {
    host.write_collection_export(&host.collection().unwrap(), &CancelToken::new(), &ProgressSink::new())
}
fn force_backup(host: &CoreHost) -> PathBuf {
    let receipt = host
        .backup()
        .before_colpkg_import(&host.collection().unwrap(), &CancelToken::new())
        .unwrap();
    PathBuf::from(receipt.name)
}
#[test]
fn forced_backup_inside_owned_slot_is_preserved_by_overlap_refusal() {
    let _serial = SERIAL.lock().unwrap();
    for suffix in ["CollectionExports/owned", "CollectionExports/owned/nested/backups"] {
        let root = tempfile::tempdir().unwrap();
        let backups = root.path().join(suffix);
        let config = config(root.path(), backups.clone(), Arc::new(StatvfsSpace));
        let host = CoreHost::open(config).unwrap();
        let backup = backups.join(force_backup(&host));
        let bytes = fs::read(&backup).unwrap();
        assert!(matches!(export(&host), Err(EngineError::InvalidArgument { .. })));
        assert_eq!(fs::read(&backup).unwrap(), bytes);
        assert_eq!(fs::read_dir(&backups).unwrap().count(), 1);
        host.close().unwrap();
    }
}
#[test]
fn normal_export_discard_preserves_real_forced_backup_and_user_destination() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let backups = root.path().join("backups");
    let config = config(root.path(), backups.clone(), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config).unwrap();
    let output = export(&host).unwrap();
    let backup = backups.join(force_backup(&host));
    let bytes = fs::read(&backup).unwrap();
    let destination = root.path().join("user.colpkg");
    fs::copy(&output.path, &destination).unwrap();
    let saved = fs::read(&destination).unwrap();
    remove_collection_export(&output.directory, &output.protected_directories).unwrap();
    assert!(!output.directory.exists());
    assert_eq!(fs::read(&backup).unwrap(), bytes);
    assert_eq!(fs::read(&destination).unwrap(), saved);
    host.close().unwrap();
}
#[test]
fn reverse_nesting_and_absent_symlink_dotdot_aliases_refuse_before_output_creation() {
    let _serial = SERIAL.lock().unwrap();
    for case in 0..4 {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("alias-target")).unwrap();
        std::os::unix::fs::symlink(root.path(), root.path().join("alias")).unwrap();
        let backups = match case {
            0 => root.path().to_owned(), // backups contain output (reverse nesting)
            1 => root.path().join("alias/CollectionExports/owned/absent"),
            2 => root.path().join("alias-target/../CollectionExports/owned/absent"),
            _ => {
                std::os::unix::fs::symlink(root.path().join("alias-target"), root.path().join("CollectionExports"))
                    .unwrap();
                root.path().join("alias-target/owned/absent")
            }
        };
        let config = config(root.path(), backups, Arc::new(StatvfsSpace));
        let source = config.collection_directory.join("collection.anki2");
        let source_before = fs::read(&source).unwrap();
        let sentinel = root.path().join("keep-user-file");
        fs::write(&sentinel, b"unchanged user content").unwrap();
        if case == 0 {
            let paths_before: std::collections::BTreeSet<_> = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            // This layout is refused before a live host can admit any export.
            assert!(CoreHost::open(config).is_err());
            let paths_after: std::collections::BTreeSet<_> = fs::read_dir(root.path())
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect();
            assert_eq!(paths_after, paths_before);
            assert_eq!(fs::read(&source).unwrap(), source_before);
        } else {
            let host = CoreHost::open(config).unwrap();
            host.review().flush_for_background().unwrap();
            let before = fs::read(&source).unwrap();
            assert!(matches!(export(&host), Err(EngineError::InvalidArgument { .. })));
            assert_eq!(fs::read(&source).unwrap(), before);
            host.close().unwrap();
        }
        assert_eq!(fs::read(&sentinel).unwrap(), b"unchanged user content");
        assert!(!root.path().join("CollectionExports/owned").exists());
        if case == 3 {
            assert!(
                root.path()
                    .join("CollectionExports")
                    .symlink_metadata()
                    .unwrap()
                    .file_type()
                    .is_symlink()
            );
            assert!(!root.path().join("alias-target/owned").exists());
        } else {
            assert!(!root.path().join("CollectionExports").exists());
        }
    }
}
struct Space(AtomicBool);
impl SpaceProvider for Space {
    fn available_bytes(&self, _: &Path) -> Result<u64, StorageError> {
        Ok(if self.0.load(Ordering::SeqCst) { u64::MAX } else { 0 })
    }
}
#[test]
fn low_space_creates_no_export_paths_and_preserves_source_and_backups() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let space = Arc::new(Space(AtomicBool::new(true)));
    let config = config(root.path(), root.path().join("backups"), space.clone());
    let host = CoreHost::open(config.clone()).unwrap();
    let backup = config.backups_directory.join(force_backup(&host));
    host.review().flush_for_background().unwrap();
    let source = config.collection_directory.join("collection.anki2");
    let before = fs::read(&source).unwrap();
    let wal = source.with_extension("anki2-wal");
    let wal_before = fs::read(&wal).unwrap();
    let sidecar = config.collection_directory.join("meguri-sidecar.json");
    let sidecar_before = fs::read(&sidecar).unwrap();
    let backup_before = fs::read(&backup).unwrap();
    space.0.store(false, Ordering::SeqCst);
    assert!(export(&host).is_err());
    assert!(!root.path().join("CollectionExports").exists());
    assert_eq!(fs::read(source).unwrap(), before);
    assert_eq!(fs::read(wal).unwrap(), wal_before);
    assert_eq!(fs::read(sidecar).unwrap(), sidecar_before);
    assert_eq!(fs::read(backup).unwrap(), backup_before);
    assert_eq!(fs::read_dir(&config.backups_directory).unwrap().count(), 1);
    space.0.store(true, Ordering::SeqCst);
    host.close().unwrap();
}
#[test]
fn recovery_refuses_unrecognized_user_file_without_deleting_any_output() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config).unwrap();
    let output = export(&host).unwrap();
    let user_file = output.directory.join("user-file");
    fs::write(&user_file, b"keep me").unwrap();
    assert!(remove_collection_export(&output.directory, &output.protected_directories).is_err());
    assert!(output.path.is_file());
    assert_eq!(fs::read(&user_file).unwrap(), b"keep me");
    host.close().unwrap();
}
#[test]
fn collection_named_like_export_root_is_never_treated_as_owned_output() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let mut config = HostConfig::new(
        root.path().join("CollectionExports"),
        root.path().join("backups"),
        root.path().join("widget"),
        Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC)),
        Arc::new(SeededRandom::new(53)),
        Arc::new(StatvfsSpace),
    );
    fs::create_dir(&config.collection_directory).unwrap();
    meguri_engine::bootstrap::create(
        &config.collection_directory.join("collection.anki2"),
        config.clock.as_ref(),
        config.random.as_ref(),
    )
    .unwrap();
    // Leave an absent configured backup descendant to exercise normalization too.
    config.backups_directory = root.path().join("backups/absent");
    let host = CoreHost::open(config.clone()).unwrap();
    let source = config.collection_directory.join("collection.anki2");
    let before = fs::read(&source).unwrap();
    assert!(matches!(export(&host), Err(EngineError::InvalidArgument { .. })));
    assert_eq!(fs::read(&source).unwrap(), before);
    assert!(!config.collection_directory.join("owned").exists());
    host.close().unwrap();
}

#[test]
fn removal_retry_rejects_replaced_directory_unknown_files_and_symlinks() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config).unwrap();
    let output = export(&host).unwrap();
    let backup = host.collection().unwrap().path().to_owned();
    let bytes = fs::read(&backup).unwrap();
    let mut removal = meguri_host::CollectionExportRemoval::default();
    let point = "host.collection_export.after_marker_unlink";
    meguri_model::faultpoint::arm(
        point,
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    assert!(
        removal
            .remove(&output.directory, &output.protected_directories)
            .is_err()
    );
    meguri_model::faultpoint::disarm(point);
    let user = output.directory.join("user-file");
    fs::write(&user, b"preserve unknown file").unwrap();
    assert!(
        removal
            .remove(&output.directory, &output.protected_directories)
            .is_err()
    );
    assert_eq!(fs::read(&user).unwrap(), b"preserve unknown file");
    fs::remove_file(&user).unwrap();
    let held = output.directory.with_file_name("held-owned-directory");
    fs::rename(&output.directory, &held).unwrap();
    std::os::unix::fs::symlink(backup.parent().unwrap(), &output.directory).unwrap();
    assert!(
        removal
            .remove(&output.directory, &output.protected_directories)
            .is_err()
    );
    assert_eq!(fs::read(&backup).unwrap(), bytes);
    fs::remove_file(&output.directory).unwrap();
    fs::create_dir(&output.directory).unwrap();
    assert!(
        removal
            .remove(&output.directory, &output.protected_directories)
            .is_err()
    );
    assert!(output.directory.is_dir());
    fs::remove_dir(&output.directory).unwrap();
    fs::rename(&held, &output.directory).unwrap();
    assert!(removal.remove(&output.directory, &[root.path().to_owned()]).is_err());
    assert!(output.directory.is_dir());
    removal
        .remove(&output.directory, &output.protected_directories)
        .unwrap();
    removal
        .remove(&output.directory, &output.protected_directories)
        .unwrap();
    let next = export(&host).unwrap();
    remove_collection_export(&next.directory, &next.protected_directories).unwrap();
    host.close().unwrap();
}

#[test]
fn invalid_media_and_retained_output_hardlinks_refuse_before_recovery_mutates_output() {
    let _serial = SERIAL.lock().unwrap();
    for alias in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
        let host = CoreHost::open(config.clone()).unwrap();
        let output = export(&host).unwrap();
        let artifact = fs::read(&output.path).unwrap();
        let marker = fs::read(output.directory.join("ownership")).unwrap();
        let media = config.collection_directory.join("collection.media");
        fs::create_dir_all(&media).unwrap();
        if alias {
            fs::hard_link(&output.path, media.join("old-export.bin")).unwrap();
        } else {
            fs::write(media.join("con.jpg"), b"unsafe name").unwrap();
        }
        assert!(matches!(export(&host), Err(EngineError::ExportSourceInvalid)));
        assert_eq!(fs::read(&output.path).unwrap(), artifact);
        assert_eq!(fs::read(output.directory.join("ownership")).unwrap(), marker);
        assert!(media.join(if alias { "old-export.bin" } else { "con.jpg" }).is_file());
        host.close().unwrap();
    }
}
#[test]
fn completed_host_export_retains_only_read_only_media_validation_authority() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config.clone()).unwrap();
    let media = config.collection_directory.join("collection.media");
    fs::create_dir_all(&media).unwrap();
    fs::write(media.join("unregistered"), b"before").unwrap();
    let output = export(&host).unwrap();
    output.validate_sources(&CancelToken::new()).unwrap();
    fs::write(media.join("unregistered"), b"AFTER!").unwrap();
    assert!(matches!(
        output.validate_sources(&CancelToken::new()),
        Err(EngineError::ExportSourceChanged)
    ));
    assert!(output.path.is_file());
    // Source authority owns no reader lease or engine/media writer lock.
    host.close().unwrap();
    assert!(output.path.is_file());
    remove_collection_export(&output.directory, &output.protected_directories).unwrap();
}

#[test]
fn media_format_limit_reaches_the_host_as_a_distinct_refusal() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config.clone()).unwrap();
    let media = config.collection_directory.join("collection.media");
    fs::create_dir_all(&media).unwrap();
    fs::File::create(media.join("oversized.bin"))
        .unwrap()
        .set_len(u64::from(u32::MAX) + 1)
        .unwrap();
    assert!(matches!(export(&host), Err(EngineError::ExportFormatLimit)));
    assert!(!root.path().join("CollectionExports").exists());
    host.close().unwrap();
}

#[test]
fn interrupted_atomic_export_is_recovered_for_both_export_kinds_after_restart() {
    use meguri_storage::atomic_file::{AtomicFile, Purpose};
    let _serial = SERIAL.lock().unwrap();
    for deck in [false, true] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
        let host = CoreHost::open(config.clone()).unwrap();
        let previous = export(&host).unwrap();
        fs::remove_file(&previous.path).unwrap();
        let mut atomic = AtomicFile::new(&previous.path, Purpose::Export).unwrap();
        std::io::Write::write_all(atomic.file_mut(), b"interrupted compression").unwrap();
        let temporary = atomic.path().to_owned();
        // Retain just the bytes that survive a killed worker, without leaking its descriptor.
        let bytes = fs::read(&temporary).unwrap();
        drop(atomic);
        fs::write(&temporary, bytes).unwrap();
        host.close().unwrap();
        let host = CoreHost::open(config).unwrap();
        let output = if deck {
            host.write_deck_export(&host.collection().unwrap(), &CancelToken::new(), &ProgressSink::new(), 1,
                meguri_package::DeckExportOptions { include_scheduling: false, include_presets: false, include_media: false })
        } else { export(&host) }.unwrap();
        assert!(!temporary.exists());
        assert!(output.path.is_file());
        remove_collection_export(&output.directory, &output.protected_directories).unwrap();
        host.close().unwrap();
    }
}

#[test]
fn operation_recovery_refuses_foreign_names_symlinks_directories_and_provider_temps() {
    let _serial = SERIAL.lock().unwrap();
    for foreign in ["foreign", ".meguri-tmp-export-abcde", ".meguri-tmp-export-abcdefg", ".meguri-tmp-export-abcde_", ".meguri-tmp-backup-abcdef", "symlink", "directory", "provider"] {
        let root = tempfile::tempdir().unwrap();
        let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
        let host = CoreHost::open(config).unwrap();
        let output = export(&host).unwrap();
        let operation = output.path.parent().unwrap();
        let reserved = operation.join(".meguri-tmp-export-aB012z");
        fs::write(&reserved, b"partial").unwrap();
        let path = operation.join(match foreign { "symlink" | "directory" => ".meguri-tmp-export-zY123a", _ => foreign });
        match foreign {
            "symlink" => std::os::unix::fs::symlink(&output.path, &path).unwrap(),
            "directory" => fs::create_dir(&path).unwrap(),
            "provider" => meguri_host::collection_export_provider_state(&output.directory, true).unwrap(),
            _ => fs::write(&path, b"keep").unwrap(),
        }
        assert!(remove_collection_export(&output.directory, &output.protected_directories).is_err());
        assert!(output.path.is_file() && reserved.is_file());
        if foreign != "provider" { assert!(path.symlink_metadata().is_ok()); }
        host.close().unwrap();
    }
}

#[test]
fn selected_name_lookup_ignores_unrelated_malformed_deck_blobs() {
    let _serial = SERIAL.lock().unwrap();
    let root = tempfile::tempdir().unwrap();
    let config = config(root.path(), root.path().join("backups"), Arc::new(StatvfsSpace));
    let host = CoreHost::open(config.clone()).unwrap();
    host.close().unwrap();
    let db = meguri_storage::raw::PlainDatabase::open(&config.collection_directory.join("collection.anki2")).unwrap();
    db.execute_batch("INSERT INTO decks (id,name,mtime_secs,usn,common,kind) VALUES (200,'Unrelated',0,0,x'ff',x'ff')").unwrap();
    db.close().unwrap();
    let host = CoreHost::open(config).unwrap();
    let output = host.write_deck_export(&host.collection().unwrap(), &CancelToken::new(), &ProgressSink::new(), 1,
        meguri_package::DeckExportOptions { include_scheduling: false, include_presets: false, include_media: false }).unwrap();
    assert_eq!(output.deck_counts, Some(meguri_package::DeckExportCounts { notes: 0, cards: 0, media: 0 }));
    remove_collection_export(&output.directory, &output.protected_directories).unwrap();
    host.close().unwrap();
}
