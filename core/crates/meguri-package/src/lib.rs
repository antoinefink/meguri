//! Latest-format package publication: collection-only P11 and opt-in file-export P15.
pub mod apkg;
pub mod backup;
mod deck_export;
mod deck_export_media;
mod export_capture;
mod file_export;
pub use deck_export::{
    DeckExportCounts, DeckExportOptions, DeckExportPlan, preflight_deck_export_from_live, write_deck_export_from_live,
};
pub use export_capture::ExportSourceValidation;
pub use file_export::{
    MediaCollectionExportPlan, MediaCollectionExportWritten, preflight_media_collection_export_from_live,
    write_media_collection_export_from_live,
};
mod server_copy;
pub use server_copy::{PublishedServerCopy, ServerCopyKind, preserve_server_copy};
pub mod entry;
mod inspection;
mod inspection_zip;
mod media_map;
mod reader;
pub mod restore;
/// Decode P3 metadata for package observations using the production reader.
pub fn package_meta_version(bytes: &[u8]) -> std::result::Result<i32, CollectionPackageWriteError> {
    if bytes.len() > 64 * 1024 {
        return Err(corrupt("meta byte limit"));
    }
    reader::meta_version(bytes)
}
mod writer;

use meguri_model::{clock::Clock, rng::RandomSource};
use meguri_storage::{StorageError, atomic_file::Purpose, space::SpaceProvider};
use std::{fmt, path::Path};

pub use backup::ForcedBackupProvider;

pub use inspection::{
    CollectionPackageInspectContext, InspectedCollection, StagedCollection, copy_and_inspect_collection_package,
    inspect_collection_package,
};
pub use meguri_storage::package_inspection::CandidateFacts as CollectionPackageFacts;
pub use writer::{
    LiveCollectionPackageWritten, preflight_collection_package_from_live, write_collection_package_from_live,
    write_collection_package_from_standalone,
};
pub type CollectionPackageInspectError = CollectionPackageWriteError;

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
    /// Explicit file-export source/format refusals; never intake or backup policy.
    ExportSourceChanged,
    ExportSourceInvalid {
        detail: String,
    },
    ExportFormatLimit,
    DeckNotFound { id: i64 },
    ExportInvalidIds,
    ExportNeedsCheckDatabase,
    TooNew,
    UnsupportedFormat,
    UnsupportedMedia,
    SchemaUnsupported {
        gate: meguri_storage::OpenError,
    },
    Corrupt {
        detail: String,
    },
    InsufficientSpace {
        needed: u64,
        available: u64,
    },
    DiskFull,
    SourceNotFinalised,
    DestinationExists,
    Cancelled,
    Io {
        detail: String,
    },
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
            StorageError::Sqlite { code: 13, .. } => Self::DiskFull, // SQLite SQLITE_FULL
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
