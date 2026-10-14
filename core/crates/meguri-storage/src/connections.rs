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
}

impl Writer {
    /// Opens the writer: read-write, never create, then C5 and C4's settings
    /// (the switch to WAL mode among them).
    pub(crate) fn open(path: &Path, initializers: &Initializers) -> Result<Writer, OpenError> {
        let conn =
            Connection::open_with_flags(file_uri(path, None), READ_WRITE).map_err(|e| OpenError::from_sqlite(&e))?;
        conn_init::full(&conn, initializers).map_err(|detail| OpenError::ConnectionInit { detail })?;
        pragmas::writer(&conn).map_err(|error| match error {
            WriterSettingsError::Sqlite(e) => OpenError::from_sqlite(&e),
            WriterSettingsError::NotWal(mode) => OpenError::Io {
                detail: format!("the journal mode stayed {mode}, not wal"),
            },
        })?;
        Ok(Writer { conn })
    }
}

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
    /// Maintenance, backups (`VACUUM INTO`), stats cache builds, derived
    /// rebuilds.
    Maintenance,
    /// The local servers' read handlers (macOS, two while the servers run).
    Server,
}

impl ReaderRole {
    pub const ALL: [ReaderRole; 6] = [
        ReaderRole::Interactive,
        ReaderRole::Query,
        ReaderRole::Search,
        ReaderRole::Sync,
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
    role: ReaderRole,
}

impl Reader {
    /// Opens a reader of `role` on a collection the writer has already set
    /// to WAL mode.
    pub(crate) fn open(path: &Path, role: ReaderRole, initializers: &Initializers) -> Result<Reader, StorageError> {
        let conn = Connection::open_with_flags(file_uri(path, Some("mode=ro")), READ_ONLY)?;
        conn_init::full(&conn, initializers).map_err(|detail| StorageError::ConnectionInit { detail })?;
        pragmas::reader(&conn, role.query_only())?;
        Ok(Reader { conn, role })
    }

    pub fn role(&self) -> ReaderRole {
        self.role
    }
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
}
