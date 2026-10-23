//! R3/R4: storage owns the candidate writer, SQL transaction and finalisation.
use crate::connections::{READ_ONLY, file_uri, sibling};
use crate::package_inspection::{self, CandidateError};
use crate::{OpenError, OpenOptions, ReadRows, StampMode, StorageError, WriteScope};
use meguri_model::{
    clock::{Clock, FixedClock},
    time::EpochMillis,
};
use std::{fs, path::Path};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceError {
    CrossDevice,
    StagingRefused(OpenError),
    StagingCorrupt,
    CallerChangeFailed(StorageError),
    StagingNotFinalised,
    LiveWalNotEmpty,
    Io(StorageError),
}
impl From<StorageError> for ReplaceError {
    fn from(e: StorageError) -> Self {
        Self::Io(e)
    }
}
impl From<std::io::Error> for ReplaceError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.into())
    }
}
impl std::fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReplaceError {}

/// Owns only an unpublished reserved staging directory. Package inspection can
/// hand over its existing TempDir, without reopening an arbitrary source path.
pub struct ReplacementStage {
    directory: tempfile::TempDir,
}
impl ReplacementStage {
    pub fn new(directory: &Path) -> Result<Self, StorageError> {
        Ok(Self {
            directory: crate::leftovers::staging_directory(directory)?,
        })
    }
    pub fn from_staging(directory: tempfile::TempDir) -> Result<Self, StorageError> {
        if !directory
            .path()
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(".meguri-stage-") && n.len() > ".meguri-stage-".len())
        {
            return Err(StorageError::Io {
                detail: "replacement requires an owned reserved staging directory".into(),
            });
        }
        Ok(Self { directory })
    }
    pub fn path(&self) -> std::path::PathBuf {
        self.directory.path().join("collection.anki2")
    }
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
}

pub fn check_device(stage: &ReplacementStage, live: &Path) -> Result<(), ReplaceError> {
    use std::os::unix::fs::MetadataExt;
    if !fs::symlink_metadata(stage.path())?.file_type().is_file() {
        return Err(StorageError::Io {
            detail: "candidate must be a regular file".into(),
        }
        .into());
    }
    let candidate = fs::metadata(stage.path())?;
    if candidate.nlink() != 1 {
        return Err(StorageError::Io {
            detail: "candidate must own independent bytes, never a source hard link".into(),
        }
        .into());
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if fs::symlink_metadata(sibling(&stage.path(), suffix))
            .is_ok_and(|m| !m.file_type().is_file() || m.nlink() != 1)
        {
            return Err(ReplaceError::StagingNotFinalised);
        }
    }
    check_directory(stage, live)?;
    if candidate.dev() != fs::metadata(crate::atomic_file::parent(live))?.dev() {
        return Err(ReplaceError::CrossDevice);
    }
    if crate::open::is_open(&stage.path()) {
        return Err(ReplaceError::StagingNotFinalised);
    }
    Ok(())
}

/// Also usable before the snapshot creates the candidate.
pub(crate) fn check_directory(stage: &ReplacementStage, live: &Path) -> Result<(), ReplaceError> {
    use std::os::unix::fs::MetadataExt;
    if fs::metadata(stage.directory())?.dev() != fs::metadata(crate::atomic_file::parent(live))?.dev() {
        return Err(ReplaceError::CrossDevice);
    }
    if stage.directory().parent().map(Path::canonicalize).transpose()?
        != Some(crate::atomic_file::parent(live).canonicalize()?)
    {
        return Err(StorageError::Io {
            detail: "candidate must be in collection-owned staging".into(),
        }
        .into());
    }
    Ok(())
}

pub fn prepare(
    stage: &ReplacementStage,
    live: &Path,
    options: &OpenOptions<'_>,
    clock: &dyn Clock,
    package_stamps: bool,
    change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
) -> Result<(), ReplaceError> {
    let preparation = if package_stamps {
        Preparation::Package
    } else {
        Preparation::AsGiven
    };
    prepare_inner(stage, live, options, clock, preparation, change)
}

pub(crate) enum Preparation {
    AsGiven,
    Package,
    FullUpload,
}

pub(crate) fn prepare_inner(
    stage: &ReplacementStage,
    live: &Path,
    options: &OpenOptions<'_>,
    clock: &dyn Clock,
    preparation: Preparation,
    change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
) -> Result<(), ReplaceError> {
    check_device(stage, live)?;
    let path = stage.path();
    package_inspection::probe_candidate(&path).map_err(candidate_error)?;
    let opened = crate::open_collection(&path, options).map_err(ReplaceError::StagingRefused)?;
    if !healthy(&opened.writer().conn)? {
        return Err(ReplaceError::StagingCorrupt);
    }
    // Freeze once, including all caller Local stamps if it explicitly uses now.
    let now = clock.now_ms();
    let frozen = FixedClock::new(now, clock.utc_offset_at(now));
    let mode = if matches!(preparation, Preparation::FullUpload) {
        StampMode::CallerSupplied
    } else {
        StampMode::Raw
    };
    let mut scope = opened
        .writer()
        .begin_write(mode, &frozen, None)
        .map_err(ReplaceError::CallerChangeFailed)?;
    if matches!(preparation, Preparation::Package) {
        let mut col = scope.col().map_err(ReplaceError::CallerChangeFailed)?;
        col.modified = EpochMillis(now);
        col.schema_modified = EpochMillis(now);
        scope.put(&col).map_err(ReplaceError::CallerChangeFailed)?;
    }
    change(&mut scope).map_err(ReplaceError::CallerChangeFailed)?;
    let committed = scope.commit().map_err(ReplaceError::CallerChangeFailed)?;
    if let Some(error) = committed.post_commit_error {
        return Err(ReplaceError::CallerChangeFailed(error));
    }
    meguri_model::faultpoint!("replace.after_check")?;
    if matches!(preparation, Preparation::FullUpload) {
        // O6.2: statistics must describe the transformed rows, and pages
        // freed by clearing graves must be reclaimed before finalisation.
        opened
            .writer()
            .conn
            .execute_batch("VACUUM; ANALYZE;")
            .map_err(StorageError::from)?;
    }
    if !opened.writer().checkpoint()?.succeeded() {
        return Err(ReplaceError::StagingNotFinalised);
    }
    let mode: String = opened
        .writer()
        .conn
        .query_row("PRAGMA journal_mode=DELETE", [], |r| r.get(0))
        .map_err(StorageError::from)?;
    if !mode.eq_ignore_ascii_case("delete") {
        return Err(ReplaceError::StagingNotFinalised);
    }
    // Unmanaged close cannot alter the collection-directory sidecar.
    if !opened.close()?.succeeded() {
        return Err(ReplaceError::StagingNotFinalised);
    }
    for suffix in ["-wal", "-shm", "-journal"] {
        if fs::symlink_metadata(sibling(&path, suffix)).is_ok() {
            return Err(ReplaceError::StagingNotFinalised);
        }
    }
    let conn = crate::rusqlite::Connection::open_with_flags(file_uri(&path, Some("mode=ro&immutable=1")), READ_ONLY)
        .map_err(StorageError::from)?;
    crate::conn_init::probe(&conn).map_err(|detail| StorageError::ConnectionInit { detail })?;
    if !healthy(&conn)? {
        return Err(ReplaceError::StagingCorrupt);
    }
    conn.close().map_err(|(_, e)| StorageError::from(e))?;
    crate::atomic_file::full_sync(&fs::File::open(&path)?)?;
    meguri_model::faultpoint!("replace.after_finalise")?;
    Ok(())
}
fn candidate_error(error: CandidateError) -> ReplaceError {
    match error {
        CandidateError::Gate(error) => ReplaceError::StagingRefused(error),
        CandidateError::Storage(error) => ReplaceError::Io(error),
        CandidateError::InsufficientSpace { needed, available } => ReplaceError::Io(StorageError::InsufficientSpace {
            required: needed,
            available,
        }),
        CandidateError::Corrupt { .. } => ReplaceError::StagingCorrupt,
        CandidateError::DiskFull => ReplaceError::Io(std::io::Error::from(std::io::ErrorKind::StorageFull).into()),
        CandidateError::Cancelled => ReplaceError::Io(StorageError::Io {
            detail: "candidate probe cancelled".into(),
        }),
    }
}
pub(crate) fn healthy(conn: &crate::rusqlite::Connection) -> Result<bool, StorageError> {
    let result = (|| -> Result<Vec<String>, crate::rusqlite::Error> {
        let mut stmt = conn.prepare("PRAGMA main.integrity_check")?;
        stmt.query_map([], |r| r.get::<_, String>(0))?.collect()
    })();
    match result {
        Ok(rows) => Ok(rows == ["ok"]),
        Err(e)
            if matches!(
                e.sqlite_error_code(),
                Some(crate::rusqlite::ErrorCode::DatabaseCorrupt | crate::rusqlite::ErrorCode::NotADatabase)
            ) =>
        {
            Ok(false)
        }
        Err(e) => Err(e.into()),
    }
}

pub fn check_live_wal(live: &Path) -> Result<(), ReplaceError> {
    match fs::metadata(sibling(live, "-wal")) {
        Ok(m) if m.len() > 0 => Err(ReplaceError::LiveWalNotEmpty),
        Ok(_) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e.into()),
    }
}
pub fn remove_empty_live_sidecars(live: &Path) -> Result<(), ReplaceError> {
    check_live_wal(live)?;
    for suffix in ["-shm", "-wal"] {
        match fs::remove_file(sibling(live, suffix)) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok(())
}
