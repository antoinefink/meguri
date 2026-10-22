//! Collection-only latest-format package publication (package/container P11).
mod reader;
mod writer;

use meguri_model::{clock::Clock, rng::RandomSource};
use meguri_storage::{StorageError, atomic_file::Purpose, space::SpaceProvider};
use std::{fmt, path::Path};

pub use writer::write_collection_package_from_standalone;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finalisation {
    ExclusiveCreate,
    Replace,
}

/// The caller owns source stability and cancellation until publication.
pub struct CollectionPackageWriteContext<'a> {
    pub collection_directory: &'a Path,
    /// Backup for backups; Export for exports.
    pub purpose: Purpose,
    pub finalisation: Finalisation,
    pub clock: &'a dyn Clock,
    pub random: &'a dyn RandomSource,
    pub space: &'a dyn SpaceProvider,
    pub is_cancelled: &'a (dyn Fn() -> bool + Sync),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CollectionPackageWriteError {
    TooNew,
    UnsupportedFormat,
    Corrupt { detail: String },
    InsufficientSpace { needed: u64, available: u64 },
    DiskFull,
    SourceNotFinalised,
    DestinationExists,
    Cancelled,
    Io { detail: String },
}
impl fmt::Display for CollectionPackageWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for CollectionPackageWriteError {}

type Error = CollectionPackageWriteError;
type Result<T> = std::result::Result<T, Error>;

impl From<StorageError> for Error {
    fn from(error: StorageError) -> Self {
        match error {
            StorageError::InsufficientSpace { .. } => Self::DiskFull,
            StorageError::DestinationExists => Self::DestinationExists,
            other => Self::Io {
                detail: other.to_string(),
            },
        }
    }
}
impl From<std::io::Error> for Error {
    fn from(error: std::io::Error) -> Self {
        StorageError::from(error).into()
    }
}
impl From<zip::result::ZipError> for Error {
    fn from(error: zip::result::ZipError) -> Self {
        match error {
            zip::result::ZipError::Io(error) => error.into(),
            other => corrupt(other.to_string()),
        }
    }
}
fn corrupt(detail: impl Into<String>) -> Error {
    Error::Corrupt { detail: detail.into() }
}
fn cancelled(check: &(dyn Fn() -> bool + Sync)) -> Result<()> {
    if check() { Err(Error::Cancelled) } else { Ok(()) }
}
