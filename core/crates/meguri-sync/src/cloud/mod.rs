//! Internal CloudKit tracking storage (K10, V2.1, V3, V8.1).
//!
//! This is a storage foundation, with no provider or account policy. Callers
//! supply already classified inbox facts. Collection writes, validation/apply,
//! account verification, engine-instance guards and recovery belong to later
//! slices. In particular, selecting a stored binding makes no readiness claim.

mod open;
mod store;
mod types;

pub use open::{OpenOutcome, TRACKING_FILE};
pub use store::TrackingStore;
pub use types::*;

/// Storage errors deliberately discard SQLite messages and opaque values.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    Io(std::io::ErrorKind),
    Database,
    Constraint,
    InvalidIdentifier,
    InvalidData,
    BindingNotFound,
    IsolatedBinding,
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::Io(_) => "cloud tracking file I/O failed",
            Self::Database => "cloud tracking database operation failed",
            Self::Constraint => "cloud tracking constraint failed",
            Self::InvalidIdentifier => "invalid cloud tracking identifier",
            Self::InvalidData => "invalid cloud tracking data",
            Self::BindingNotFound => "cloud binding not found in this build",
            Self::IsolatedBinding => "cloud binding is isolated",
        })
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error.kind())
    }
}

impl From<rusqlite::Error> for Error {
    fn from(error: rusqlite::Error) -> Self {
        match error.sqlite_error_code() {
            Some(rusqlite::ErrorCode::ConstraintViolation) => Self::Constraint,
            _ => Self::Database,
        }
    }
}
