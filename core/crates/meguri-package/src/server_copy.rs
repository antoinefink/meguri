//! O6.1: preserve every served file, including card-empty and refused schemas.
use crate::{CollectionPackageWriteContext, CollectionPackageWriteError as Error, Finalisation};
use meguri_storage::{
    StorageError,
    atomic_file::{AtomicFile, Purpose},
};
use sha1::{Digest, Sha1};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ServerCopyKind {
    Verified,
    Unverified,
}
pub struct PublishedServerCopy {
    pub path: PathBuf,
    pub kind: ServerCopyKind,
    pub instant: i64,
    pub facts: Option<meguri_storage::one_way::LocalTransferFacts>,
}
/// Source is privately owned, closed and stable. Verification never upgrades or
/// changes its bytes. Publication failure (including a post-rename sync error)
/// is a refusal to upload; any visible recovery file stays protected forever.
pub fn preserve_server_copy(
    source: &Path,
    backups: &Path,
    context: &CollectionPackageWriteContext<'_>,
) -> Result<PublishedServerCopy, Error> {
    crate::cancelled(context.is_cancelled)?;
    let verified = match meguri_storage::one_way::verify_server_copy(source) {
        Ok(facts) => Some(facts),
        Err(
            meguri_storage::verified_replace::ReplaceError::StagingRefused(_)
            | meguri_storage::verified_replace::ReplaceError::StagingCorrupt,
        ) => None,
        Err(_) => {
            return Err(Error::Io {
                detail: "server-copy verification could not finish".into(),
            });
        }
    };
    let size = fs::metadata(source)?.len();
    // Uses the real package writer's capacity/provider checks before creating parents.
    crate::writer::preflight_publication(size, &backups.join("server-copy-preflight"), context)?;
    crate::backup::create_backup_directory(backups)?;
    let instant = context.clock.now_ms();
    let second = instant
        .div_euclid(1000)
        .checked_add(i64::from(context.clock.utc_offset_at(instant).seconds()))
        .ok_or_else(|| crate::corrupt("server-copy time overflow"))?;
    for advance in 0..=60 {
        let backup = crate::backup::backup_name(
            second
                .checked_add(advance)
                .ok_or_else(|| crate::corrupt("server-copy time overflow"))?,
        )?;
        let stem = backup.strip_prefix("backup-").unwrap().strip_suffix(".colpkg").unwrap();
        let name = format!(
            "server-copy-{stem}{}",
            if verified.is_some() {
                ".colpkg"
            } else {
                ".unverified.anki2"
            }
        );
        let destination = backups.join(name);
        if destination.symlink_metadata().is_ok() {
            continue;
        }
        crate::cancelled(context.is_cancelled)?;
        meguri_model::faultpoint!("package.server_copy.before_publish")?;
        let published = if verified.is_some() {
            let context = CollectionPackageWriteContext {
                finalisation: Finalisation::ExclusiveCreate,
                ..*context
            };
            crate::write_collection_package_from_standalone(source, &destination, &context)
        } else {
            copy_raw(source, &destination)
        };
        match published {
            Err(Error::DestinationExists) => continue,
            Err(error) => return Err(error),
            Ok(()) => {
                return Ok(PublishedServerCopy {
                    path: destination,
                    kind: if verified.is_some() {
                        ServerCopyKind::Verified
                    } else {
                        ServerCopyKind::Unverified
                    },
                    instant,
                    facts: verified.flatten(),
                });
            }
        }
    }
    Err(Error::DestinationExists)
}

pub(crate) fn copy_raw(source: &Path, destination: &Path) -> Result<(), Error> {
    // Frozen manual-transfer path retains its pre-copy source witness.
    copy_raw_checked(source, destination, Some(sha1(source)?), &|| false, false).map(RawCopy::keep)
}

pub(crate) fn copy_raw_cancellable(
    source: &Path,
    destination: &Path,
    cancelled: &(dyn Fn() -> bool + Sync),
) -> Result<RawCopy, Error> {
    copy_raw_checked(source, destination, None, cancelled, true)
}
fn copy_raw_checked(
    source: &Path,
    destination: &Path,
    expected: Option<(u64, [u8; 20])>,
    cancelled: &(dyn Fn() -> bool + Sync),
    strict: bool,
) -> Result<RawCopy, Error> {
    let mut output = AtomicFile::new(destination, Purpose::Backup)?;
    let mut input = fs::File::open(source)?;
    let mut digest = Sha1::new();
    let mut size = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        crate::cancelled(cancelled)?;
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        digest.update(&buffer[..n]);
        size += n as u64;
        output.file_mut().write_all(&buffer[..n])?;
    }
    let copied = (size, digest.finalize().into());
    // The local caller holds a stable writer window. Manual transfer also
    // compares against its existing pre-copy witness.
    let expected = expected.unwrap_or(copied);
    if copied != expected {
        return Err(Error::Io {
            detail: "raw recovery copy changed".into(),
        });
    }
    crate::cancelled(cancelled)?;
    use std::os::unix::fs::MetadataExt;
    let metadata = output.file_mut().metadata()?;
    let mut published = RawCopy {
        path: destination.to_owned(),
        identity: (metadata.dev(), metadata.ino()),
        cleanup: strict,
    };
    output.publish_new(|path| {
        if sha1(path).map_err(|_| StorageError::Io {
            detail: "raw recovery copy verification".into(),
        })? != expected
        {
            return Err(StorageError::Io {
                detail: "raw recovery copy changed".into(),
            });
        }
        if cancelled() {
            return Err(StorageError::Io {
                detail: "raw recovery copy cancelled".into(),
            });
        }
        Ok(())
    })?;
    published.cleanup = true;
    Ok(published)
}

/// Own only the inode we published; never remove a collision or a replacement.
pub(crate) struct RawCopy {
    path: PathBuf,
    identity: (u64, u64),
    cleanup: bool,
}
impl RawCopy {
    pub(crate) fn keep(mut self) {
        self.cleanup = false;
    }
}
impl Drop for RawCopy {
    fn drop(&mut self) {
        use std::os::unix::fs::MetadataExt;
        if self.cleanup && fs::symlink_metadata(&self.path).is_ok_and(|m| (m.dev(), m.ino()) == self.identity) {
            let _ = fs::remove_file(&self.path);
            if let Some(parent) = self.path.parent() {
                let _ = meguri_storage::atomic_file::sync_directory(parent);
            }
        }
    }
}

fn sha1(path: &Path) -> Result<(u64, [u8; 20]), Error> {
    let mut input = fs::File::open(path)?;
    let mut hash = Sha1::new();
    let mut length = 0;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        length += n as u64;
        hash.update(&buffer[..n]);
    }
    Ok((length, hash.finalize().into()))
}
