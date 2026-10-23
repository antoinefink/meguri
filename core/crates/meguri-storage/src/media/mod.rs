//! Meguri's flat media store and independent journal (media/media-journal).
mod case_fold;
mod check_report;
mod folder;
mod journal_db;
mod staging;
mod sync_state;
mod trash;

pub use check_report::MediaCheckReport;
pub use journal_db::{MediaEntry, MediaSyncState, PendingChange, TrackedMediaName};
use meguri_model::clock::Clock;
use rusqlite::Connection;
pub use staging::{NoteReference, RecoveryReport, StagedOp};
use std::{
    collections::BTreeMap,
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard, Weak,
        atomic::{AtomicU64, Ordering},
    },
};
pub use sync_state::*;
pub use trash::{TrashEntry, TrashSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    InvalidName,
    IgnoredName,
    NameTaken,
    Cancelled,
    DiskFull,
    /// A published download receipt no longer describes the current row/file.
    StaleDownload,
    Io(String),
    Database(String),
    InvalidManifest(String),
}
impl fmt::Display for MediaError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidName => f.write_str("invalid media name"),
            Self::IgnoredName => f.write_str("ignored media name"),
            Self::NameTaken => f.write_str("media name is occupied"),
            Self::Cancelled => f.write_str("media scan cancelled"),
            Self::DiskFull => f.write_str("media storage is full"),
            Self::StaleDownload => f.write_str("download publication changed"),
            Self::Io(s) | Self::Database(s) | Self::InvalidManifest(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for MediaError {}
impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::from(crate::StorageError::from(e))
    }
}
impl From<rusqlite::Error> for MediaError {
    fn from(e: rusqlite::Error) -> Self {
        if e.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull) {
            Self::DiskFull
        } else {
            Self::Database(e.to_string())
        }
    }
}
impl From<crate::StorageError> for MediaError {
    fn from(e: crate::StorageError) -> Self {
        match e {
            crate::StorageError::DestinationExists => Self::NameTaken,
            crate::StorageError::InsufficientSpace { .. } => Self::DiskFull,
            crate::StorageError::Sqlite { code, .. } if code == rusqlite::ffi::SQLITE_FULL => Self::DiskFull,
            other => Self::Io(other.to_string()),
        }
    }
}
pub type Result<T> = std::result::Result<T, MediaError>;

#[derive(Debug)]
struct Inner {
    db: Mutex<Option<Connection>>,
    journal: PathBuf,
    // Access while holding db: registration and recovery share one boundary.
    active_stages: Mutex<BTreeMap<i64, Weak<()>>>,
    media: PathBuf,
    trash: PathBuf,
    staging: PathBuf,
    clock: Arc<dyn Clock>,
    scans: AtomicU64,
    hashes: AtomicU64,
    case_sensitive: bool,
}
/// Clone this handle; do not open competing stores for the same collection.
#[derive(Clone, Debug)]
pub struct MediaStore {
    inner: Arc<Inner>,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Diagnostics {
    pub scans: u64,
    pub hashes: u64,
    pub case_sensitive: bool,
}

impl MediaStore {
    pub fn open(collection: &Path, clock: Arc<dyn Clock>) -> Result<Self> {
        let root = collection.parent().unwrap_or(Path::new("."));
        let media = root.join("collection.media");
        let trash = root.join("media.trash");
        let staging = root.join("media.staging");
        for dir in [&media, &trash, &staging] {
            folder::ensure_directory(dir)?;
        }
        let journal = root.join("meguri-media.db");
        if std::fs::symlink_metadata(&journal).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(MediaError::InvalidName);
        }
        let db = journal_db::open(&journal)?;
        use std::os::unix::fs::MetadataExt;
        let device = std::fs::metadata(&media)?.dev();
        for dir in [&trash, &staging] {
            if std::fs::metadata(dir)?.dev() != device {
                return Err(MediaError::Io("media directories must share a volume".into()));
            }
        }
        let case_sensitive = folder::probe_case_sensitivity(&staging)?;
        let store = Self {
            inner: Arc::new(Inner {
                db: Mutex::new(Some(db)),
                journal,
                active_stages: Mutex::new(BTreeMap::new()),
                media,
                trash,
                staging,
                clock,
                scans: AtomicU64::new(0),
                hashes: AtomicU64::new(0),
                case_sensitive,
            }),
        };
        store.reconcile_trash()?;
        Ok(store)
    }
    fn db(&self) -> Result<JournalGuard<'_>> {
        let guard = self
            .inner
            .db
            .lock()
            .map_err(|_| MediaError::Database("media mutex poisoned".into()))?;
        if guard.is_none() {
            return Err(MediaError::Database("media journal is closed".into()));
        }
        Ok(JournalGuard(guard))
    }

    /// Close the shared connection only when no operation holds its guard.
    /// The engine supplies the deadline and waits outside the mutex.
    pub fn try_close(&self) -> Result<bool> {
        let mut guard = match self.inner.db.try_lock() {
            Ok(guard) => guard,
            Err(std::sync::TryLockError::WouldBlock) => return Ok(false),
            Err(_) => return Err(MediaError::Database("media mutex poisoned".into())),
        };
        if let Some(db) = guard.take()
            && let Err((db, error)) = db.close()
        {
            *guard = Some(db);
            return Err(error.into());
        }
        Ok(true)
    }

    /// Rebind every clone after the engine has reopened the collection.
    pub fn resume(&self) -> Result<()> {
        let mut guard = self
            .inner
            .db
            .lock()
            .map_err(|_| MediaError::Database("media mutex poisoned".into()))?;
        if guard.is_none() {
            if std::fs::symlink_metadata(&self.inner.journal).is_ok_and(|m| m.file_type().is_symlink()) {
                return Err(MediaError::InvalidName);
            }
            *guard = Some(journal_db::open(&self.inner.journal)?);
        }
        Ok(())
    }
    pub fn media_dir(&self) -> &Path {
        &self.inner.media
    }
    pub fn trash_dir(&self) -> &Path {
        &self.inner.trash
    }
    pub fn staging_dir(&self) -> &Path {
        &self.inner.staging
    }
    pub fn diagnostics(&self) -> Diagnostics {
        Diagnostics {
            scans: self.inner.scans.load(Ordering::Relaxed),
            hashes: self.inner.hashes.load(Ordering::Relaxed),
            case_sensitive: self.inner.case_sensitive,
        }
    }
    pub fn change_counter(&self) -> Result<u64> {
        journal_db::counter(&*self.db()?)
    }
    pub fn pending(&self) -> Result<Vec<PendingChange>> {
        journal_db::pending(&*self.db()?, i64::MAX as u64)
    }
    /// Oldest stamps first, up to the requested limit and Y10's 25 entries.
    pub fn pending_batch(&self, limit: u32) -> Result<Vec<PendingChange>> {
        journal_db::pending(&*self.db()?, u64::from(limit.min(25)))
    }
    pub fn pending_any(&self) -> Result<bool> {
        journal_db::pending_any(&*self.db()?)
    }
    pub fn acknowledge(&self, name: &str, counter: u64) -> Result<bool> {
        Ok(self.db()?.execute(
            "UPDATE media SET dirty=0 WHERE name=?1 AND change_counter=?2 AND dirty=1",
            rusqlite::params![name, journal_db::sqlite_integer(counter)?],
        )? != 0)
    }
    /// One journal guard covers the entire query; no filesystem work.
    pub fn tracked_names(&self) -> Result<Vec<TrackedMediaName>> {
        journal_db::tracked_names(&*self.db()?)
    }
    pub fn entries(&self) -> Result<Vec<MediaEntry>> {
        journal_db::entries(&*self.db()?)
    }
}

struct JournalGuard<'a>(MutexGuard<'a, Option<Connection>>);
impl std::ops::Deref for JournalGuard<'_> {
    type Target = Connection;
    fn deref(&self) -> &Connection {
        self.0.as_ref().expect("an open journal guard")
    }
}
impl std::ops::DerefMut for JournalGuard<'_> {
    fn deref_mut(&mut self) -> &mut Connection {
        self.0.as_mut().expect("an open journal guard")
    }
}

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    #[test]
    fn release_never_waits_on_an_active_journal_operation_and_resume_preserves_pending() {
        let dir = tempfile::tempdir().unwrap();
        let clock = Arc::new(meguri_model::clock::FixedClock::new(
            0,
            meguri_model::clock::UtcOffset::UTC,
        ));
        let store = MediaStore::open(&dir.path().join("collection.anki2"), clock).unwrap();
        store.add("owned.txt", b"synthetic").unwrap();
        let before = store.pending().unwrap();
        let held = store.db().unwrap();
        assert!(!store.try_close().unwrap());
        drop(held);
        assert!(store.try_close().unwrap());
        assert!(store.clone().pending().is_err());
        store.resume().unwrap();
        assert_eq!(store.pending().unwrap(), before);
    }
}
