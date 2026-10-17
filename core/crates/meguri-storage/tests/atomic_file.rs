mod common;

use meguri_storage::{
    StorageError,
    atomic_file::{AtomicFile, Purpose},
};
use std::io::Write;

#[test]
fn verifier_failure_keeps_original_and_removes_temp() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::write(&path, b"previous").unwrap();
    let mut temp = AtomicFile::new(&path, Purpose::Replace).unwrap();
    temp.file_mut().write_all(b"bad replacement").unwrap();
    assert!(
        temp.publish(|_| Err(StorageError::Io {
            detail: "invalid".into()
        }))
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"previous");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[test]
fn verified_bytes_replace_destination() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.colpkg");
    std::fs::write(&path, b"old").unwrap();
    let mut temp = AtomicFile::new(&path, Purpose::Backup).unwrap();
    temp.file_mut().write_all(b"verified").unwrap();
    temp.publish(|p| {
        assert_eq!(std::fs::read(p).unwrap(), b"verified");
        Ok(())
    })
    .unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"verified");
}

#[test]
fn launch_cleanup_preserves_valid_files_and_never_traverses_symlinks() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    for name in [
        ".meguri-tmp-stage-dead",
        ".meguri-tmp-stage-dead-wal",
        "meguri-export-old.partial",
        "meguri-backup-old.partial",
        "meguri-sync-stage-old.tmp",
    ] {
        std::fs::write(dir.path().join(name), b"interrupted").unwrap();
    }
    for name in [
        "collection.anki2",
        "backup.colpkg",
        "collection.anki2.pre-upgrade-v11",
        "notes.tmp",
        ".meguri-tmp-unknown-1",
    ] {
        std::fs::write(dir.path().join(name), b"keep").unwrap();
    }
    let protected = outside.path().join("important");
    std::fs::write(&protected, b"keep").unwrap();
    std::os::unix::fs::symlink(&protected, dir.path().join(".meguri-tmp-replace-link")).unwrap();
    std::fs::create_dir(dir.path().join(".meguri-tmp-backup-directory")).unwrap();
    assert_eq!(meguri_storage::leftovers::cleanup(dir.path()).unwrap().len(), 5);
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 7);
    assert_eq!(std::fs::read(protected).unwrap(), b"keep");
}

#[test]
fn precheck_refuses_and_enospc_before_rename_keeps_original() {
    use meguri_storage::space::{SpaceProvider, require_space};
    struct Capacity;
    impl SpaceProvider for Capacity {
        fn available_bytes(&self, _: &std::path::Path) -> Result<u64, StorageError> {
            Ok(99)
        }
    }
    let dir = tempfile::tempdir().unwrap();
    assert_eq!(
        require_space(&Capacity, dir.path(), 100),
        Err(StorageError::InsufficientSpace {
            required: 100,
            available: 99
        })
    );
    require_space(&Capacity, dir.path(), 99).unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::write(&path, b"original").unwrap();
    let mut temp = AtomicFile::new(&path, Purpose::Replace).unwrap();
    temp.file_mut().write_all(b"next").unwrap();
    meguri_model::faultpoint::arm(
        "storage.atomic.before_rename",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::StorageFull),
    );
    let error = temp.publish(|_| Ok(())).unwrap_err();
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(error, StorageError::InsufficientSpace { .. }));
    assert_eq!(std::fs::read(&path).unwrap(), b"original");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}

#[cfg(target_os = "linux")]
#[test]
fn real_disk_full_on_size_limited_tmpfs_leaves_original_intact() {
    use meguri_storage::space::{SpaceProvider, StatvfsSpace};
    let dir = tempfile::tempdir_in("/dev").unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(common::fixture("schema-18.anki2"), &path).unwrap();
    let before = std::fs::read(&path).unwrap();
    let mut replacement = AtomicFile::new(&path, Purpose::Replace).unwrap();
    let mut filler = tempfile::tempfile_in(dir.path()).unwrap();
    let capacity = StatvfsSpace.available_bytes(dir.path()).unwrap();
    assert!(capacity <= 256 * 1024 * 1024, "test requires a small tmpfs");
    let zeros = [0_u8; 16384];
    while filler.write_all(&zeros).is_ok() {}
    let error = replacement.file_mut().write_all(&zeros).unwrap_err();
    assert!(matches!(
        StorageError::from(error),
        StorageError::InsufficientSpace { .. }
    ));
    drop(replacement);
    drop(filler);
    assert_eq!(std::fs::read(&path).unwrap(), before);
    assert!(meguri_storage::integrity::integrity_check(&path).unwrap().is_ok());
}

#[test]
fn every_atomic_boundary_is_reachable_and_only_post_rename_publishes() {
    use meguri_model::faultpoint::{self, Fault};
    for point in [
        "storage.atomic.after_write",
        "storage.atomic.after_sync",
        "storage.atomic.after_verify",
        "storage.atomic.before_rename",
        "storage.atomic.after_rename",
        "storage.stage.before_rename",
        "storage.backup.before_rename",
    ] {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("file");
        std::fs::write(&path, b"old").unwrap();
        let purpose = if point.contains("backup") {
            Purpose::Backup
        } else {
            Purpose::Stage
        };
        let mut file = AtomicFile::new(&path, purpose).unwrap();
        file.file_mut().write_all(b"new").unwrap();
        faultpoint::arm(point, Fault::IoError(std::io::ErrorKind::Other));
        assert!(file.publish(|_| Ok(())).is_err(), "{point}");
        faultpoint::disarm_all();
        assert_eq!(
            std::fs::read(&path).unwrap(),
            if point.ends_with("after_rename") {
                b"new"
            } else {
                b"old"
            }
        );
    }
}

#[test]
fn launch_sweeps_only_reserved_staging_directories_without_following_links() {
    let dir = tempfile::tempdir().unwrap();
    let outside = tempfile::tempdir().unwrap();
    let kept = outside.path().join("kept");
    std::fs::write(&kept, b"private").unwrap();
    let stage = meguri_storage::leftovers::staging_directory(dir.path()).unwrap();
    std::fs::write(stage.path().join("entry"), b"partial").unwrap();
    std::os::unix::fs::symlink(outside.path(), stage.path().join("link")).unwrap();
    let stage = stage.keep();
    let ordinary = dir.path().join("user-data");
    std::fs::create_dir(&ordinary).unwrap();
    assert_eq!(
        meguri_storage::leftovers::cleanup(dir.path()).unwrap(),
        vec![stage.clone()]
    );
    assert!(!stage.exists());
    assert!(ordinary.exists());
    assert_eq!(std::fs::read(kept).unwrap(), b"private");
}

#[test]
fn private_paths_and_separate_backups_have_stable_names() {
    let dir = tempfile::tempdir().unwrap();
    let backups = tempfile::tempdir().unwrap();
    let paths = meguri_storage::paths::CollectionPaths::new(dir.path(), backups.path()).unwrap();
    assert_eq!(paths.collection(), dir.path().join("collection.anki2"));
    assert_eq!(paths.media(), dir.path().join("collection.media"));
    assert_eq!(paths.media_journal(), dir.path().join("collection.media.db"));
    assert_eq!(paths.derived(), dir.path().join("meguri-derived.db"));
    assert_eq!(paths.sidecar(), dir.path().join("meguri-sidecar.json"));
    assert!(meguri_storage::paths::CollectionPaths::new(dir.path(), dir.path()).is_err());
    let nested = dir.path().join("backups");
    std::fs::create_dir(&nested).unwrap();
    assert!(meguri_storage::paths::CollectionPaths::new(dir.path(), &nested).is_err());
}

#[test]
fn unique_backup_publication_never_overwrites_a_name_collision() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("backup.colpkg");
    let mut first = AtomicFile::new(&path, Purpose::Backup).unwrap();
    first.file_mut().write_all(b"first").unwrap();
    first.publish_new(|_| Ok(())).unwrap();
    let mut second = AtomicFile::new(&path, Purpose::Backup).unwrap();
    second.file_mut().write_all(b"second").unwrap();
    assert!(second.publish_new(|_| Ok(())).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), b"first");
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
