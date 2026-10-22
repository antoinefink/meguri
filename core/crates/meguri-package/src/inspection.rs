//! Collection-only P12 inspection. Input ownership/copying is the app job's
//! responsibility; this API keeps only its own unpublished staging directory.
use crate::{CollectionPackageFacts, Error, Result, cancelled, corrupt, inspection_zip, media_map, reader};
use meguri_model::{clock::Clock, rng::RandomSource};
use meguri_storage::{
    StorageError,
    package_inspection::{self, CandidateContext, CandidateError},
    space::SpaceProvider,
};
use std::{
    fs::File,
    io::{Cursor, Read, Write},
    path::{Path, PathBuf},
};

pub struct CollectionPackageInspectContext<'a> {
    pub collection_directory: &'a Path,
    pub live_collection_size: u64,
    /// Caller budget for the required safety backup (and other app-job work).
    /// Added to the first preflight with checked arithmetic, before any write.
    pub reserved_space_bytes: u64,
    pub clock: &'a dyn Clock,
    pub random: &'a dyn RandomSource,
    pub space: &'a dyn SpaceProvider,
    pub is_cancelled: &'a (dyn Fn() -> bool + Sync),
}

#[derive(Debug)]
pub struct InspectedCollection {
    staging: StagedCollection,
    facts: CollectionPackageFacts,
}
impl InspectedCollection {
    pub fn facts(&self) -> &CollectionPackageFacts {
        &self.facts
    }
    pub fn collection_path(&self) -> PathBuf {
        self.staging.collection_path()
    }
    /// Consuming handoff: no caller-supplied pathname can construct this token.
    pub fn into_staged(self) -> StagedCollection {
        self.staging
    }
}

/// Owned staging for the future verified-replace consumer. Dropping it removes
/// only this operation's U32 directory. All ZIP/SQLite handles are already shut.
#[derive(Debug)]
pub struct StagedCollection {
    directory: tempfile::TempDir,
}
impl StagedCollection {
    pub fn collection_path(&self) -> PathBuf {
        self.directory.path().join("collection.anki2")
    }
    pub fn directory(&self) -> &Path {
        self.directory.path()
    }
    /// Transfers cleanup ownership, without publishing or activating anything.
    pub fn into_directory(self) -> tempfile::TempDir {
        self.directory
    }
}
fn preflight(context: &CollectionPackageInspectContext<'_>, needed: u64) -> Result<()> {
    meguri_storage::space::require_space(context.space, context.collection_directory, needed).map_err(|e| match e {
        StorageError::InsufficientSpace { required, available } => Error::InsufficientSpace {
            needed: required,
            available,
        },
        other => other.into(),
    })
}
fn candidate_error(error: CandidateError) -> Error {
    match error {
        CandidateError::Gate(
            gate @ (meguri_storage::OpenError::TooOld { .. }
            | meguri_storage::OpenError::TooNew { .. }
            | meguri_storage::OpenError::V1Scheduler),
        ) => Error::SchemaUnsupported { gate },
        CandidateError::Gate(meguri_storage::OpenError::Io { detail }) => Error::Io { detail },
        CandidateError::Gate(gate) => corrupt(format!("candidate gate: {gate}")),
        CandidateError::Storage(error) => match error {
            StorageError::Sqlite { .. } | StorageError::Row(_) => corrupt(format!("candidate: {error}")),
            other => other.into(),
        },
        CandidateError::Corrupt { detail } => corrupt(detail),
        CandidateError::InsufficientSpace { needed, available } => Error::InsufficientSpace { needed, available },
        CandidateError::DiskFull => Error::DiskFull,
        CandidateError::Cancelled => Error::Cancelled,
    }
}

/// Inspect a caller-stabilized readable package. Never reads or writes the live
/// database, its sidecars, backups or media. General layout follows P2, not P11.
pub fn inspect_collection_package(
    source: &Path,
    context: &CollectionPackageInspectContext<'_>,
) -> Result<InspectedCollection> {
    cancelled(context.is_cancelled)?;
    let mut input = inspection_zip::open(File::open(source)?, context.is_cancelled)?;
    let mut total = 0;
    let version = if let Some(index) = input.meta {
        let entry = input.archive.by_index(index)?;
        let mut bytes = Vec::new();
        reader::bounded_read(entry, 64 * 1024, &mut total, context.is_cancelled, |chunk| {
            bytes.extend_from_slice(chunk)
        })?;
        reader::meta_version(&bytes)?
    } else {
        return Err(Error::UnsupportedFormat);
    };
    match version {
        3 => {}
        1 | 2 => return Err(Error::UnsupportedFormat),
        _ => return Err(Error::TooNew),
    }
    let index = input.collection.ok_or_else(|| corrupt("missing collection.anki21b"))?;
    let mut entry = input.archive.by_index(index)?;
    let compressed_size = entry.compressed_size();
    let limit = reader::decoded_limit(compressed_size, 8 * 1024 * 1024 * 1024);
    // The longest ordinary zstd header is 18 bytes. Read no collection body
    // into memory and make no staging writes until P10 has passed.
    let mut header = [0; 18];
    let mut n = 0;
    while n < header.len() {
        cancelled(context.is_cancelled)?;
        let read = entry.read(&mut header[n..]).map_err(reader::decode_error)?;
        if read == 0 {
            break;
        }
        n += read;
    }
    let declared = if header[..n].starts_with(&[0x28, 0xb5, 0x2f, 0xfd]) {
        zstd::zstd_safe::get_frame_content_size(&header[..n]).map_err(|_| corrupt("zstd first-frame header"))?
    } else if n >= 4 && header[0] & 0xf0 == 0x50 && header[1..4] == [0x2a, 0x4d, 0x18] {
        None
    } else {
        return Err(corrupt("zstd first-frame magic"));
    };
    if declared.is_some_and(|size| size > limit) {
        return Err(corrupt("declared collection byte limit"));
    }
    let estimate = match declared {
        Some(size) => size,
        None => compressed_size
            .checked_mul(4)
            .ok_or_else(|| corrupt("inspect estimate overflow"))?
            .max(context.live_collection_size),
    };
    let needed = estimate
        .checked_add(4 * 1024 * 1024)
        .and_then(|n| n.checked_add(context.reserved_space_bytes))
        .ok_or_else(|| corrupt("inspect preflight overflow"))?;
    preflight(context, needed)?;
    cancelled(context.is_cancelled)?;
    let directory = meguri_storage::leftovers::staging_directory(context.collection_directory)?;
    let path = directory.path().join("collection.anki2");
    let mut output = File::options().write(true).create_new(true).open(&path)?;
    reader::zstd_read_fallible(
        Cursor::new(&header[..n]).chain(entry),
        limit,
        &mut total,
        context.is_cancelled,
        |bytes| {
            meguri_model::faultpoint!("package.inspect.write")?;
            output.write_all(bytes).map_err(Into::into)
        },
    )?;
    output.flush()?;
    drop(output);
    cancelled(context.is_cancelled)?;
    let facts = package_inspection::inspect_candidate(
        &path,
        &CandidateContext {
            clock: context.clock,
            random: context.random,
            space: context.space,
            is_cancelled: context.is_cancelled,
        },
    )
    .map_err(candidate_error)?;
    if let Some(index) = input.media {
        let entry = input.archive.by_index(index)?;
        let limit = reader::decoded_limit(entry.compressed_size(), 64 * 1024 * 1024);
        let mut bytes = Vec::new();
        reader::zstd_read(entry, limit, &mut total, context.is_cancelled, |chunk| {
            bytes.extend_from_slice(chunk)
        })?;
        let count = media_map::validate(&bytes, context.is_cancelled)?;
        if count != 0 {
            return Err(Error::UnsupportedMedia);
        }
    }
    if input.carried_media {
        return Err(Error::UnsupportedMedia);
    }
    drop(input);
    cancelled(context.is_cancelled)?;
    Ok(InspectedCollection {
        staging: StagedCollection { directory },
        facts,
    })
}
