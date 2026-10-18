//! Meguri's flat media store and independent journal (media/media-journal).
mod case_fold;
mod folder;
mod journal_db;
mod staging;
mod trash;

pub use journal_db::{MediaEntry, PendingChange};
use meguri_model::clock::Clock;
use rusqlite::Connection;
pub use staging::{NoteReference, RecoveryReport, StagedOp};
use std::{
    fmt,
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex, MutexGuard,
        atomic::{AtomicU64, Ordering},
    },
};
pub use trash::{TrashEntry, TrashSummary};

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MediaError {
    InvalidName,
    IgnoredName,
    NameTaken,
    Cancelled,
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
            Self::Io(s) | Self::Database(s) | Self::InvalidManifest(s) => f.write_str(s),
        }
    }
}
impl std::error::Error for MediaError {}
impl From<std::io::Error> for MediaError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
impl From<rusqlite::Error> for MediaError {
    fn from(e: rusqlite::Error) -> Self {
        Self::Database(e.to_string())
    }
}
impl From<crate::StorageError> for MediaError {
    fn from(e: crate::StorageError) -> Self {
        match e {
            crate::StorageError::DestinationExists => Self::NameTaken,
            other => Self::Io(other.to_string()),
        }
    }
}
pub type Result<T> = std::result::Result<T, MediaError>;

#[derive(Debug)]
struct Inner {
    db: Mutex<Connection>,
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
        let case_sensitive = folder::probe_case_sensitivity(&media)?;
        let store = Self {
            inner: Arc::new(Inner {
                db: Mutex::new(db),
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
    fn db(&self) -> Result<MutexGuard<'_, Connection>> {
        self.inner
            .db
            .lock()
            .map_err(|_| MediaError::Database("media mutex poisoned".into()))
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
        journal_db::pending(&*self.db()?)
    }
    pub fn acknowledge(&self, name: &str, counter: u64) -> Result<bool> {
        Ok(self.db()?.execute(
            "UPDATE media SET dirty=0 WHERE name=?1 AND change_counter=?2 AND dirty=1",
            rusqlite::params![name, journal_db::sqlite_integer(counter)?],
        )? != 0)
    }
    pub fn entries(&self) -> Result<Vec<MediaEntry>> {
        journal_db::entries(&*self.db()?)
    }
}
