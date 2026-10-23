//! Capacity classes must survive each typed media/storage conversion seam.
use meguri_storage::{StorageError, media::MediaError};

#[test]
fn os_capacity_and_quota_codes_remain_typed_through_storage_and_media() {
    for code in [libc::ENOSPC, libc::EDQUOT] {
        assert_eq!(
            MediaError::from(std::io::Error::from_raw_os_error(code)),
            MediaError::DiskFull
        );
        let stored = StorageError::from(std::io::Error::from_raw_os_error(code));
        assert!(matches!(&stored, StorageError::InsufficientSpace { .. }));
        assert_eq!(MediaError::from(stored), MediaError::DiskFull);
    }
    for kind in [std::io::ErrorKind::StorageFull, std::io::ErrorKind::QuotaExceeded] {
        assert_eq!(MediaError::from(std::io::Error::from(kind)), MediaError::DiskFull);
    }
}

#[test]
fn sqlite_full_remains_typed_directly_and_through_storage() {
    let full = || rusqlite::Error::SqliteFailure(rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_FULL), None);
    assert_eq!(MediaError::from(full()), MediaError::DiskFull);
    assert_eq!(MediaError::from(StorageError::from(full())), MediaError::DiskFull);
    assert_eq!(
        MediaError::from(StorageError::Sqlite {
            code: rusqlite::ffi::SQLITE_FULL,
            extended_code: rusqlite::ffi::SQLITE_FULL,
            message: "arbitrary".into(),
        }),
        MediaError::DiskFull
    );
}

#[test]
fn full_disk_words_do_not_reclassify_other_io_or_sqlite_errors() {
    assert!(matches!(
        MediaError::from(std::io::Error::other("ENOSPC EDQUOT disk full")),
        MediaError::Io(_)
    ));
    let other = rusqlite::Error::SqliteFailure(
        rusqlite::ffi::Error::new(rusqlite::ffi::SQLITE_IOERR),
        Some("SQLITE_FULL".into()),
    );
    assert!(matches!(MediaError::from(other), MediaError::Database(_)));
}
