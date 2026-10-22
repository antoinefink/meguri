use crate::{CollectionPackageWriteContext, Error, Finalisation, Result, cancelled, reader};
use meguri_storage::{
    StorageError,
    atomic_file::AtomicFile,
    collection_package::{self, StandaloneSnapshotError},
    leftovers::staging_directory,
    space::require_space,
};
use sha2::{Digest, Sha256};
use std::{
    fs::File,
    io::{Read, Write},
    path::Path,
};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

fn source_error(error: StandaloneSnapshotError) -> Error {
    match error {
        StandaloneSnapshotError::SourceNotFinalised => Error::SourceNotFinalised,
        StandaloneSnapshotError::Storage(error) => error.into(),
    }
}
fn preflight(context: &CollectionPackageWriteContext<'_>, directory: &Path, needed: u64) -> Result<()> {
    require_space(context.space, directory, needed).map_err(|error| match error {
        StorageError::InsufficientSpace { required, available } => Error::InsufficientSpace {
            needed: required,
            available,
        },
        other => other.into(),
    })
}

/// Publish a caller-verified closed source, kept stable by the caller. A
/// post-rename I/O error can leave the verified new file visible (U32 F4).
pub fn write_collection_package_from_standalone(
    source: &Path,
    destination: &Path,
    context: &CollectionPackageWriteContext<'_>,
) -> Result<()> {
    cancelled(context.is_cancelled)?;
    if destination
        .canonicalize()
        .ok()
        .zip(source.canonicalize().ok())
        .is_some_and(|(a, b)| a == b)
    {
        return Err(Error::Io {
            detail: "package destination aliases its source".into(),
        });
    }
    let size = collection_package::standalone_size(source).map_err(source_error)?;
    let needed = size
        .checked_mul(2)
        .and_then(|n| n.checked_add(4 * 1024 * 1024))
        .ok_or_else(|| Error::Io {
            detail: "standalone space estimate overflow".into(),
        })?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    preflight(context, context.collection_directory, needed)?;
    preflight(context, parent, needed)?;
    cancelled(context.is_cancelled)?;
    let staging = staging_directory(context.collection_directory)?;
    let snapshot = staging.path().join("snapshot.anki2");
    collection_package::snapshot_standalone(source, &snapshot).map_err(source_error)?;
    cancelled(context.is_cancelled)?;
    let placeholder = staging.path().join("collection.anki2");
    collection_package::create_placeholder(&placeholder, context.clock, context.random)?;
    cancelled(context.is_cancelled)?;
    let mut output = AtomicFile::new(destination, context.purpose)?;
    let mut zip = ZipWriter::new(output.file_mut());
    let stored = SimpleFileOptions::default().compression_method(CompressionMethod::Stored);
    zip.start_file("meta", stored)?;
    zip.write_all(&[0x08, 0x03])?;
    let snapshot_size = std::fs::metadata(&snapshot)?.len();
    if snapshot_size > 8 * 1024 * 1024 * 1024 {
        return Err(crate::corrupt("snapshot exceeds collection byte limit"));
    }
    // zstd can expand incompressible input slightly. Reserve Zip64 before
    // streaming whenever that bound could cross the 32-bit ZIP size field.
    let zip64 = snapshot_size >= u64::from(u32::MAX) - snapshot_size / 128 - 1024 * 1024;
    zip.start_file("collection.anki21b", stored.large_file(zip64))?;
    let mut encoder = zstd::stream::write::Encoder::new(&mut zip, 3)?;
    encoder.set_pledged_src_size(Some(snapshot_size))?;
    encoder.include_contentsize(true)?;
    encoder.include_checksum(true)?;
    let mut source_file = File::open(&snapshot)?;
    let mut digest = Sha256::new();
    let mut length = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        cancelled(context.is_cancelled)?;
        let n = source_file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        length += n as u64;
        digest.update(&buffer[..n]);
        encoder.write_all(&buffer[..n])?;
    }
    encoder.finish()?;
    drop(source_file);
    let expected = reader::SnapshotDigest {
        length,
        sha256: digest.finalize().into(),
    };
    zip.start_file("collection.anki2", stored)?;
    let mut notice = File::open(&placeholder)?;
    loop {
        cancelled(context.is_cancelled)?;
        let n = notice.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        zip.write_all(&buffer[..n])?;
    }
    drop(notice);
    zip.start_file("media", stored)?;
    let mut media = zstd::stream::write::Encoder::new(&mut zip, 3)?;
    media.set_pledged_src_size(Some(0))?;
    media.include_contentsize(true)?;
    media.include_checksum(true)?;
    media.finish()?;
    zip.finish()?;
    cancelled(context.is_cancelled)?;
    // AtomicFile's verifier has a storage error type. Preserve the package's
    // typed reason separately; never flatten cancellation or corrupt input.
    let mut verification_error = None;
    let verify = |path: &Path| match reader::verify(path, &expected, context.is_cancelled) {
        Ok(()) => Ok(()),
        Err(error) => {
            verification_error = Some(error);
            Err(StorageError::Io {
                detail: "package verification failed".into(),
            })
        }
    };
    let published = match context.finalisation {
        Finalisation::ExclusiveCreate => output.publish_new(verify),
        Finalisation::Replace => output.publish(verify),
    };
    if let Some(error) = verification_error {
        return Err(error);
    }
    published.map_err(Into::into)
}
