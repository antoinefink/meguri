//! The connections of an open collection (`storage/open-and-connections` C4,
//! C6): exactly one writer, and read-only readers each assigned to one role,
//! never pooled or shared across roles (KTD3). Also how every connection
//! names the file: a `file:` URI with the path percent-encoded (C2.1).

use std::fmt::Write as _;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags};

use crate::conn_init::{self, Initializers};
use crate::error::{OpenError, StorageError};
use crate::pragmas::{self, WriterSettingsError};

/// The `file:` URI of `path`, with `query` (no `?`) when given. Every byte
/// outside `A-Z a-z 0-9 - . _ ~ /` is written `%XX` (upper-case hex), so
/// paths with spaces, `?`, `#` or `%` name the right file.
pub(crate) fn file_uri(path: &Path, query: Option<&str>) -> String {
    let mut uri = String::from("file:");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~' | b'/') {
            uri.push(char::from(byte));
        } else {
            // Writing to a String cannot fail.
            let _ = write!(uri, "%{byte:02X}");
        }
    }
    if let Some(query) = query {
        uri.push('?');
        uri.push_str(query);
    }
    #[cfg(feature = "test-hooks")]
    if crate::counting_vfs::selected(path) {
        uri.push_str(if query.is_some() { "&" } else { "?" });
        uri.push_str("vfs=meguri-counting");
    }
    uri
}

/// `path` with `suffix` appended to its file name (`-wal`, `-journal`, ...).
pub(crate) fn sibling(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

pub(crate) const READ_ONLY: OpenFlags = OpenFlags::SQLITE_OPEN_READ_ONLY
    .union(OpenFlags::SQLITE_OPEN_URI)
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX);

/// Read-write without create: the gate never makes a file it did not find.
pub(crate) const READ_WRITE: OpenFlags = OpenFlags::SQLITE_OPEN_READ_WRITE
    .union(OpenFlags::SQLITE_OPEN_URI)
    .union(OpenFlags::SQLITE_OPEN_NO_MUTEX);

/// The writer connection (C4): every write to the collection goes through
/// it, in `BEGIN IMMEDIATE` transactions. The engine's writer thread owns it
/// (U24).
#[derive(Debug)]
pub struct Writer {
    pub(crate) conn: Connection,
    // Dropped after SQLite: closing another descriptor earlier releases POSIX locks.
    identity_file: std::fs::File,
    identity_bound: (u64, u64),
    #[cfg(feature = "test-hooks")]
    pub(crate) fail_next_wal_sync: std::cell::Cell<bool>,
}

impl Writer {
    /// Identity of the descriptor retained before SQLite opened, under collection ownership.
    pub fn native_identity(&self) -> Result<(u64, u64), StorageError> {
        use std::os::unix::fs::MetadataExt;
        let metadata = self.identity_file.metadata()?;
        Ok((metadata.dev(), metadata.ino()))
    }

    /// Cross-restart C12 binding from the original retained descriptor.
    pub fn persistent_identity(&self) -> Result<crate::persistent_identity::PersistentIdentity, StorageError> {
        crate::persistent_identity::read_from(&self.identity_file, self.identity_bound)
    }

    /// No second database FD: closing one would release SQLite's POSIX locks.
    pub fn cloud_file_evidence(&self) -> Result<crate::persistent_identity::CloudFileEvidence, StorageError> {
        let before = crate::persistent_identity::cloud_native(&self.identity_file)?;
        let identity = self.persistent_identity()?;
        let after = crate::persistent_identity::cloud_native(&self.identity_file)?;
        crate::persistent_identity::checked_cloud(self.identity_bound, before, identity, after)
    }

    /// Opens the writer: read-write, never create, then C5 and C4's settings
    /// (the switch to WAL mode among them).
    pub(crate) fn open(path: &Path, initializers: &Initializers) -> Result<Writer, OpenError> {
        let identity_file = std::fs::File::open(path).map_err(|e| OpenError::Io { detail: e.to_string() })?;
        #[cfg(feature = "test-hooks")]
        writer_open_hooks::checkpoint(path, WriterOpenStage::Retained);
        let conn =
            Connection::open_with_flags(file_uri(path, None), READ_WRITE).map_err(|e| OpenError::from_sqlite(&e))?;
        #[cfg(feature = "test-hooks")]
        writer_open_hooks::checkpoint(path, WriterOpenStage::SqliteOpened);
        // C12.1: validate the retained file and SQLite's main database before
        // allowing connection setup to mutate either file. These checks cover
        // the open sequence; they do not make pathname lookup atomic.
        use std::os::unix::fs::MetadataExt;
        let file_identity = {
            let metadata = identity_file.metadata().map_err(|error| OpenError::Io {
                detail: error.to_string(),
            })?;
            (metadata.dev(), metadata.ino())
        };
        let path_identity = {
            let metadata = std::fs::metadata(path).map_err(|error| OpenError::Io {
                detail: error.to_string(),
            })?;
            (metadata.dev(), metadata.ino())
        };
        if file_identity != path_identity {
            return Err(OpenError::Io {
                detail: "writer pathname no longer identifies the retained file".into(),
            });
        }

        // SQLite may have opened a different file even when the path has
        // already been restored. Ask that live connection about its main file.
        let mut path_changed: std::ffi::c_int = -1;
        // SAFETY: conn owns a live SQLite handle. The main database name is
        // NUL-terminated, and the output points to an int valid for this call.
        let control_status = unsafe {
            rusqlite::ffi::sqlite3_file_control(
                conn.handle(),
                c"main".as_ptr(),
                rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
                std::ptr::from_mut(&mut path_changed).cast(),
            )
        };
        match (control_status, path_changed) {
            (rusqlite::ffi::SQLITE_OK, 0) => {}
            (status, value) => {
                return Err(OpenError::Io {
                    detail: format!("SQLite file identity check failed (status {status}, result {value})"),
                });
            }
        }
        conn_init::full(&conn, initializers).map_err(|detail| OpenError::ConnectionInit { detail })?;
        conn.execute_batch("BEGIN IMMEDIATE; ROLLBACK;")
            .map_err(|e| OpenError::from_sqlite(&e))?;
        pragmas::writer(&conn).map_err(|error| match error {
            WriterSettingsError::Sqlite(e) => OpenError::from_sqlite(&e),
            WriterSettingsError::NotWal(mode) => OpenError::Io {
                detail: format!("the journal mode stayed {mode}, not wal"),
            },
        })?;
        Ok(Writer {
            conn,
            identity_file,
            identity_bound: file_identity,
            #[cfg(feature = "test-hooks")]
            fail_next_wal_sync: std::cell::Cell::new(false),
        })
    }
}

#[cfg(feature = "test-hooks")]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriterOpenStage {
    Retained,
    SqliteOpened,
}

#[cfg(feature = "test-hooks")]
mod writer_open_hooks {
    use super::*;
    use std::{
        collections::BTreeMap,
        sync::{Arc, Mutex},
    };
    type Hook = Arc<dyn Fn(WriterOpenStage) + Send + Sync>;
    static HOOKS: Mutex<BTreeMap<PathBuf, Hook>> = Mutex::new(BTreeMap::new());

    /// Scoped barriers for one disposable collection; never holds the map
    /// lock while a callback renames/restores its files.
    pub struct WriterOpenHook(PathBuf);
    impl WriterOpenHook {
        pub fn for_path(path: &Path, hook: Hook) -> Self {
            assert!(HOOKS.lock().unwrap().insert(path.to_owned(), hook).is_none());
            Self(path.to_owned())
        }
    }
    impl Drop for WriterOpenHook {
        fn drop(&mut self) {
            HOOKS.lock().unwrap().remove(&self.0);
        }
    }
    pub(super) fn checkpoint(path: &Path, stage: WriterOpenStage) {
        let hook = HOOKS.lock().unwrap().get(path).cloned();
        if let Some(hook) = hook {
            hook(stage);
        }
    }
}
#[cfg(feature = "test-hooks")]
pub use writer_open_hooks::WriterOpenHook;

/// What a reader is for (C6). Each reader serves one role; long-lived
/// snapshots (a sync, a backup, a derived rebuild) never use the interactive,
/// query or server readers.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ReaderRole {
    /// Review session, rendering, Card Info, editor open.
    Interactive,
    /// Row pages, deck counts, heatmap.
    Query,
    /// One per search worker; interruptible.
    Search,
    /// Normal and one-way sync snapshots, `VACUUM INTO` for a full upload.
    Sync,
    /// CloudKit snapshots and scans; independent of manual sync and backups.
    Cloud,
    /// Maintenance, backups (`VACUUM INTO`), stats cache builds, derived
    /// rebuilds.
    Maintenance,
    /// The local servers' read handlers (macOS, two while the servers run).
    Server,
}

impl ReaderRole {
    pub const ALL: [ReaderRole; 7] = [
        ReaderRole::Interactive,
        ReaderRole::Query,
        ReaderRole::Search,
        ReaderRole::Sync,
        ReaderRole::Cloud,
        ReaderRole::Maintenance,
        ReaderRole::Server,
    ];

    /// Whether the reader runs with `query_only`: every role but sync and
    /// maintenance, whose `VACUUM INTO` fails under it.
    pub fn query_only(self) -> bool {
        !matches!(self, ReaderRole::Sync | ReaderRole::Maintenance)
    }
}

/// A read-only connection of one role (C6). Opened with `mode=ro`, so
/// SQLite refuses every write to the collection file on it.
#[derive(Debug)]
pub struct Reader {
    pub(crate) conn: Connection,
    _recovery_copy: Option<tempfile::TempDir>,
    role: ReaderRole,
    native_identity: (u64, u64),
    /// The epoch of the derived store attached as `derived`
    /// (`storage/derived-store` D9).
    pub(crate) derived: Option<u64>,
}

/// Opens readers of an open collection from any thread (C6): the engine's
/// writer thread owns the [`crate::OpenedCollection`], and its readers open
/// lazily on the threads that first need them. Normal owners have already
/// set WAL mode; recorded-corrupt owners instead permit diagnostic failures
/// without opening a writer or releasing recovery ownership.
#[derive(Clone, Debug)]
pub struct ReaderFactory {
    path: PathBuf,
    initializers: Initializers,
    recovery: bool,
}

impl ReaderFactory {
    pub(crate) fn new(path: &Path, initializers: &Initializers) -> ReaderFactory {
        ReaderFactory {
            path: path.to_path_buf(),
            initializers: initializers.clone(),
            recovery: false,
        }
    }

    pub fn recovery(path: &Path, initializers: &Initializers) -> Self {
        Self {
            recovery: true,
            ..Self::new(path, initializers)
        }
    }

    /// Opens a reader of `role` (C6). The caller closes it before the
    /// collection closes.
    pub fn open(&self, role: ReaderRole) -> Result<Reader, StorageError> {
        if self.recovery {
            let copy = recovery_copy(&self.path)?;
            let path = copy.path().join("collection.anki2");
            let mut reader = Reader::open_with_query(
                &path,
                role,
                &self.initializers,
                crate::schema::version::probe_query(&path),
            )?;
            reader._recovery_copy = Some(copy);
            Ok(reader)
        } else {
            Reader::open(&self.path, role, &self.initializers)
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

impl Reader {
    /// Opens a reader of `role` on a collection the writer has already set
    /// to WAL mode.
    pub(crate) fn open(path: &Path, role: ReaderRole, initializers: &Initializers) -> Result<Reader, StorageError> {
        Self::open_with_query(path, role, initializers, "mode=ro")
    }

    fn open_with_query(
        path: &Path,
        role: ReaderRole,
        initializers: &Initializers,
        query: &str,
    ) -> Result<Reader, StorageError> {
        use std::os::unix::fs::MetadataExt;
        let before = std::fs::metadata(path)?;
        let native_identity = (before.dev(), before.ino());
        let conn = Connection::open_with_flags(file_uri(path, Some(query)), READ_ONLY)?;
        validate_identity(&conn, path, native_identity)?;
        conn_init::full(&conn, initializers).map_err(|detail| StorageError::ConnectionInit { detail })?;
        pragmas::reader(&conn, role.query_only())?;
        Ok(Reader {
            conn,
            _recovery_copy: None,
            role,
            native_identity,
            derived: None,
        })
    }

    /// Identity checked against SQLite at open; no extra descriptor may be
    /// closed here while the writer holds POSIX locks on this file.
    pub fn native_identity(&self) -> (u64, u64) {
        self.native_identity
    }

    pub fn role(&self) -> ReaderRole {
        self.role
    }
}

#[derive(PartialEq, Eq)]
struct RecoveryStamp {
    native: (u64, u64),
    bytes: u64,
    modified: (i64, i64),
    changed: (i64, i64),
}

fn recovery_stamp(path: &Path) -> Result<Option<RecoveryStamp>, StorageError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error.into()),
    };
    if !metadata.is_file() {
        return Err(StorageError::Io {
            detail: "recovery evidence is not a regular file".into(),
        });
    }
    Ok(Some(RecoveryStamp {
        native: (metadata.dev(), metadata.ino()),
        bytes: metadata.len(),
        modified: (metadata.mtime(), metadata.mtime_nsec()),
        changed: (metadata.ctime(), metadata.ctime_nsec()),
    }))
}

fn recovery_copy(path: &Path) -> Result<tempfile::TempDir, StorageError> {
    let suffixes = ["", "-wal", "-journal"];
    let before = suffixes
        .map(|suffix| recovery_stamp(&sibling(path, suffix)))
        .into_iter()
        .collect::<Result<Vec<_>, _>>()?;
    if before[0].is_none() {
        return Err(std::io::Error::from(std::io::ErrorKind::NotFound).into());
    }
    let copy = tempfile::tempdir()?;
    let destination = copy.path().join("collection.anki2");
    for (suffix, stamp) in suffixes.iter().zip(&before) {
        if stamp.is_some() {
            std::fs::copy(sibling(path, suffix), sibling(&destination, suffix))?;
        }
    }
    for (suffix, stamp) in suffixes.iter().zip(&before) {
        if recovery_stamp(&sibling(path, suffix))? != *stamp {
            return Err(StorageError::Io {
                detail: "recovery evidence changed during diagnostic capture".into(),
            });
        }
    }
    Ok(copy)
}

/// Checks SQLite's live main file as well as the pathname, without opening
/// another descriptor (whose close would release this process's POSIX locks).
pub(crate) fn validate_identity(conn: &Connection, path: &Path, expected: (u64, u64)) -> Result<(), StorageError> {
    use std::os::unix::fs::MetadataExt;
    let metadata = std::fs::metadata(path)?;
    let mut moved: std::ffi::c_int = -1;
    // SAFETY: the connection is live on its owning thread and the output int
    // remains valid throughout this non-retaining SQLite call.
    let code = unsafe {
        rusqlite::ffi::sqlite3_file_control(
            conn.handle(),
            c"main".as_ptr(),
            rusqlite::ffi::SQLITE_FCNTL_HAS_MOVED,
            std::ptr::from_mut(&mut moved).cast(),
        )
    };
    if (metadata.dev(), metadata.ino()) != expected || code != rusqlite::ffi::SQLITE_OK || moved != 0 {
        return Err(StorageError::Io {
            detail: "collection file identity changed".into(),
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uris_percent_encode_everything_but_the_unreserved_bytes_and_slash() {
        assert_eq!(
            file_uri(Path::new("/a/Application Support/x?y#z%.anki2"), None),
            "file:/a/Application%20Support/x%3Fy%23z%25.anki2"
        );
        assert_eq!(
            file_uri(Path::new("/tmp/Caf\u{e9}~-_.db"), Some("mode=ro&immutable=1")),
            "file:/tmp/Caf%C3%A9~-_.db?mode=ro&immutable=1"
        );
    }

    #[cfg(all(unix, feature = "test-hooks"))]
    #[test]
    fn uri_selects_counting_vfs_through_a_directory_symlink() {
        use crate::counting_vfs::CountingVfs;
        use std::os::unix::fs::symlink;

        let directory = tempfile::tempdir().unwrap();
        let physical = directory.path().canonicalize().unwrap();
        let path = physical.join("collection.anki2");
        std::fs::write(&path, []).unwrap();
        let alias = physical.join("alias");
        symlink(&physical, &alias).unwrap();
        let alias_path = alias.join("collection.anki2");
        assert_ne!(alias_path, path);
        assert_eq!(alias_path.canonicalize().unwrap(), path);
        let probe = CountingVfs::for_path(&path);

        for query in [None, Some("mode=ro"), Some("mode=ro&immutable=1")] {
            let uri = file_uri(&alias_path, query);
            let suffix = if query.is_some() {
                "&vfs=meguri-counting"
            } else {
                "?vfs=meguri-counting"
            };
            assert!(uri.ends_with(suffix), "{uri}");
        }
        let unrelated = alias.join("other.anki2");
        std::fs::write(&unrelated, []).unwrap();
        assert!(!file_uri(&unrelated, None).contains("vfs="));
        assert!(!file_uri(&alias.join("missing.anki2"), None).contains("vfs="));
        drop(probe);
        assert!(!file_uri(&alias_path, None).contains("vfs="));
    }
}
