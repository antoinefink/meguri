//! Cloud tracking and the host-wired stable prebound workflow.
//!
//! Whole scans, conditional saves and acknowledgements require current account,
//! file, publication and root proofs. Explicit publication uses the durable
//! recovery protocol and active-epoch collection adoption. Structural
//! generations and nonempty media remain refused before replacement.

pub mod adapter;
mod adapter_store;
pub mod adoption;
mod adoption_store;
pub mod control_codec;
pub mod entity_codec;
mod held_view;
pub mod policy;
mod policy_store;
pub mod publication;
mod publication_store;
pub mod row_payload;
pub mod runtime;
pub mod workflow;
mod workflow_store;

pub mod incoming;
mod incoming_merge;
mod incoming_store;
mod open;
mod recovery;
mod runtime_store;
mod store;
#[cfg(feature = "test-hooks")]
pub mod testing;
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
    RecoveryRequired,
    RecoveryRefused,
    RuntimeSchemaUnsupported,
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
            Self::RecoveryRequired => "cloud tracking recovery must finish before opening",
            Self::RecoveryRefused => "cloud tracking recovery evidence is inconsistent",
            Self::RuntimeSchemaUnsupported => "cloud runtime tracking schema is unsupported",
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
            Some(rusqlite::ErrorCode::DiskFull) => Self::Io(std::io::ErrorKind::StorageFull),
            _ => Self::Database,
        }
    }
}

impl From<meguri_storage::StorageError> for Error {
    fn from(error: meguri_storage::StorageError) -> Self {
        match error {
            meguri_storage::StorageError::DestinationExists => Self::RecoveryRefused,
            _ => Self::Io(std::io::ErrorKind::Other),
        }
    }
}
