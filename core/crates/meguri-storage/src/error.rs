//! The crate's errors: the open gate's typed refusals and failures
//! (`storage/open-and-connections` C8), and the errors of the connections
//! once a collection is open.

use std::fmt;

use crate::rows::RowError;

/// Where an upgrade failed (`storage/schema-upgrade` U2, U10).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum UpgradeStage {
    /// The pre-upgrade copy (U2), before anything touched the original.
    Copy,
    /// The step that sets `col.ver` to this number.
    Step(u32),
}

/// What the open gate refuses or fails with (C8). Each refusal tells the user
/// what to do; the UI copy is U153's.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OpenError {
    OpenInAnotherProgram,
    /// `col.ver` below 11.
    TooOld {
        version: i64,
    },
    /// `col.ver` 12, 13 or above 18: made by a newer or unclean client.
    TooNew {
        version: i64,
    },
    /// The collection still uses the v1 scheduler (C2.2, R1482).
    V1Scheduler,
    /// Not SQLite, no `col` row with an integer `ver`, or required tables
    /// missing (C1, C2.5). `missing_tables` names the required tables that
    /// are absent, when that is the reason.
    NotACollection {
        missing_tables: Vec<String>,
    },
    /// The upgrade could not convert the file: rolled back, original intact.
    UpgradeFailed {
        version: i64,
        stage: UpgradeStage,
        detail: String,
    },
    /// SQLite reports the file corrupt.
    Corrupt {
        detail: String,
    },
    /// The collation or an initializer could not be registered (C5).
    ConnectionInit {
        detail: String,
    },
    /// No file at the path.
    NotFound,
    /// Any other I/O or SQLite failure.
    Io {
        detail: String,
    },
}

impl OpenError {
    /// An SQLite failure while the gate reads or opens the file: "not a
    /// database" refuses, corruption fails with `Corrupt`, anything else is
    /// `Io` (C2.1).
    pub(crate) fn from_sqlite(error: &rusqlite::Error) -> OpenError {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked) => {
                OpenError::OpenInAnotherProgram
            }
            Some(rusqlite::ErrorCode::NotADatabase) => OpenError::NotACollection {
                missing_tables: Vec::new(),
            },
            Some(rusqlite::ErrorCode::DatabaseCorrupt) => OpenError::Corrupt {
                detail: error.to_string(),
            },
            _ => OpenError::Io {
                detail: error.to_string(),
            },
        }
    }
}

impl fmt::Display for OpenError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OpenError::OpenInAnotherProgram => f.write_str("open in another program"),
            OpenError::TooOld { version } => write!(f, "collection version {version} is too old"),
            OpenError::TooNew { version } => {
                write!(f, "collection version {version} was made by a newer or unclean client")
            }
            OpenError::V1Scheduler => f.write_str("the collection uses the v1 scheduler"),
            OpenError::NotACollection { missing_tables } if missing_tables.is_empty() => {
                f.write_str("not an Anki collection")
            }
            OpenError::NotACollection { missing_tables } => {
                write!(f, "not an Anki collection: no table {}", missing_tables.join(", "))
            }
            OpenError::UpgradeFailed { version, stage, detail } => {
                write!(f, "upgrading version {version} failed at {stage:?}: {detail}")
            }
            OpenError::Corrupt { detail } => write!(f, "corrupt collection: {detail}"),
            OpenError::ConnectionInit { detail } => write!(f, "connection initialization failed: {detail}"),
            OpenError::NotFound => f.write_str("no collection file at the path"),
            OpenError::Io { detail } => write!(f, "I/O error: {detail}"),
        }
    }
}

impl std::error::Error for OpenError {}

/// Errors of an open collection's connections and of writes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StorageError {
    /// A no-replace file publication found an existing destination.
    DestinationExists,
    InsufficientSpace {
        required: u64,
        available: u64,
    },
    /// The checkpoint primitive ran inside a transaction (C7); nothing done.
    TransactionOpen,
    /// A stored row cannot be read (`model/row-records` R10).
    Row(RowError),
    /// Every id from now + 1 ms to now + 1 day is in use in the table
    /// (`storage/ids-usn-mtime` I3): the operation writes nothing.
    IdsExhausted {
        table: &'static str,
    },
    /// The write is not one this scope's stamping mode makes.
    Mode {
        detail: String,
    },
    /// The storage-only builder refuses the path.
    BuilderRefused {
        reason: BuilderRefusal,
    },
    /// The collation or an initializer could not be registered on a new
    /// reader (C5).
    ConnectionInit {
        detail: String,
    },
    /// SQLite failed: its primary and extended result codes (2067 for a
    /// unique index, 1555 for a primary key) and its message.
    Sqlite {
        code: i32,
        extended_code: i32,
        message: String,
    },
    /// A file-system operation failed.
    Io {
        detail: String,
    },
    /// The derived store refused a component definition or a table name
    /// (`storage/derived-store` D4).
    Derived {
        detail: String,
    },
}

/// Why the storage-only builder refuses a path (`write_scope`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuilderRefusal {
    /// A file is at the path, or a `-wal`, `-shm` or `-journal` file next to
    /// it.
    Exists,
    /// The path is open as a collection in this process.
    Open,
}

impl From<RowError> for StorageError {
    fn from(error: RowError) -> StorageError {
        StorageError::Row(error)
    }
}

impl From<rusqlite::Error> for StorageError {
    fn from(error: rusqlite::Error) -> StorageError {
        if error.sqlite_error_code() == Some(rusqlite::ErrorCode::DiskFull) {
            return StorageError::InsufficientSpace {
                required: 1,
                available: 0,
            };
        }
        match &error {
            rusqlite::Error::SqliteFailure(failure, message) => StorageError::Sqlite {
                code: failure.extended_code & 0xff,
                extended_code: failure.extended_code,
                message: message.clone().unwrap_or_else(|| failure.to_string()),
            },
            other => StorageError::Sqlite {
                code: 0,
                extended_code: 0,
                message: other.to_string(),
            },
        }
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StorageError::InsufficientSpace { required, available } => {
                write!(f, "insufficient space: need {required} bytes, {available} available")
            }
            StorageError::TransactionOpen => f.write_str("a transaction is open"),
            StorageError::Row(error) => write!(f, "unreadable row: {error}"),
            StorageError::IdsExhausted { table } => {
                write!(f, "no free id within one day of now in table {table}")
            }
            StorageError::Mode { detail } => f.write_str(detail),
            StorageError::BuilderRefused { reason } => match reason {
                BuilderRefusal::Exists => f.write_str("the builder writes only new files; a file is at the path"),
                BuilderRefusal::Open => f.write_str("the path is open as a collection"),
            },
            StorageError::ConnectionInit { detail } => write!(f, "connection initialization failed: {detail}"),
            StorageError::Sqlite {
                extended_code, message, ..
            } => write!(f, "SQLite error {extended_code}: {message}"),
            StorageError::DestinationExists => f.write_str("destination already exists"),
            StorageError::Io { detail } => write!(f, "I/O error: {detail}"),
            StorageError::Derived { detail } => write!(f, "derived store: {detail}"),
        }
    }
}

impl std::error::Error for StorageError {}

impl From<std::io::Error> for StorageError {
    fn from(error: std::io::Error) -> Self {
        if matches!(error.raw_os_error(), Some(libc::ENOSPC | libc::EDQUOT))
            || error.kind() == std::io::ErrorKind::StorageFull
        {
            Self::InsufficientSpace {
                required: 1,
                available: 0,
            }
        } else {
            Self::Io {
                detail: error.to_string(),
            }
        }
    }
}
