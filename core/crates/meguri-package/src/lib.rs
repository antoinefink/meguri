//! `.apkg` and `.colpkg` import and export, and backups (writer, retention,
//! restore).

use std::fmt;
use std::path::Path;

use meguri_model::clock::Clock;
use meguri_model::rng::RandomSource;
use meguri_storage::atomic_file::Purpose;
use meguri_storage::space::SpaceProvider;

/// Publication rule selected by the caller (package/container P11).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Finalisation {
    ExclusiveCreate,
    Replace,
}

/// Operation context for a collection-only package write.
pub struct CollectionPackageWriteContext<'a> {
    /// Parent for the operation-owned U32 staging directory (P9).
    pub collection_directory: &'a Path,
    /// U32 temporary-file purpose: Backup for backups, Export for exports.
    pub purpose: Purpose,
    pub finalisation: Finalisation,
    pub clock: &'a dyn Clock,
    pub random: &'a dyn RandomSource,
    /// P10 preflights both staging and output before any writes.
    pub space: &'a dyn SpaceProvider,
    /// Cancellation is honoured until publication (P11).
    pub is_cancelled: &'a (dyn Fn() -> bool + Sync),
}

/// The writer is not implemented at this red-test checkpoint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CollectionPackageWriteError {
    Unavailable,
}

impl fmt::Display for CollectionPackageWriteError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable => f.write_str("collection-only package writer is unavailable"),
        }
    }
}

impl std::error::Error for CollectionPackageWriteError {}

/// Publish a latest-format collection-only package from a caller-verified,
/// closed standalone collection (package/container P9–P11).
///
/// The caller keeps the source stable for the operation. A source with a WAL
/// or journal sibling must be refused. Snapshot and placeholder creation
/// belong to storage; the source must remain unchanged.
///
/// Currently returns `Unavailable` without reading, writing or consuming
/// injected time or randomness. The production continuation supplies the
/// storage seams and the complete P13 error set.
pub fn write_collection_package_from_standalone(
    _source: &Path,
    _destination: &Path,
    _context: &CollectionPackageWriteContext<'_>,
) -> Result<(), CollectionPackageWriteError> {
    Err(CollectionPackageWriteError::Unavailable)
}
