use super::{Result, TransferError};
use crate::policy::{Artifact, FileIdentity};
use meguri_storage::{
    StorageError,
    atomic_file::{self, AtomicFile, Purpose},
};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
};

pub(super) fn identity(path: &Path) -> Result<FileIdentity> {
    let meta = fs::symlink_metadata(path)?;
    if !meta.file_type().is_file() || meta.nlink() != 1 {
        return Err(TransferError::Artifact);
    }
    let mut file = File::open(path)?;
    let opened = file.metadata()?;
    if opened.dev() != meta.dev() || opened.ino() != meta.ino() {
        return Err(TransferError::Artifact);
    }
    let mut hash = Sha256::new();
    let mut length = 0u64;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        length = length.checked_add(n as u64).ok_or(TransferError::Artifact)?;
        hash.update(&buffer[..n]);
    }
    let after = fs::symlink_metadata(path)?;
    if after.dev() != meta.dev()
        || after.ino() != meta.ino()
        || length != meta.len()
        || after.ctime() != meta.ctime()
        || after.ctime_nsec() != meta.ctime_nsec()
    {
        return Err(TransferError::Artifact);
    }
    Ok(FileIdentity {
        length,
        sha256: hash.finalize().into(),
    })
}
pub(super) fn artifact(path: &Path) -> Result<Artifact> {
    Ok(Artifact {
        name: path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or(TransferError::Artifact)?
            .into(),
        identity: identity(path)?,
    })
}
pub(super) fn check(path: &Path, expected: &Artifact) -> Result<()> {
    if identity(path)? != expected.identity {
        return Err(TransferError::Artifact);
    }
    Ok(())
}
/// A new independent inode, durable before its reference/intent is published.
pub(super) fn retain(source: &Path, destination: &Path) -> Result<Artifact> {
    let expected = identity(source)?;
    let mut output = AtomicFile::new(destination, Purpose::Stage)?;
    let mut input = File::open(source)?;
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let n = input.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        output.file_mut().write_all(&buffer[..n])?;
    }
    output.publish_new(|path| {
        if identity(path).map_err(|_| StorageError::Io {
            detail: "retained artifact check failed".into(),
        })? != expected
        {
            return Err(StorageError::Io {
                detail: "retained artifact changed".into(),
            });
        }
        Ok(())
    })?;
    fs::set_permissions(destination, fs::Permissions::from_mode(0o400))?;
    atomic_file::full_sync(&File::open(destination)?)?;
    atomic_file::sync_directory(destination.parent().ok_or(TransferError::Artifact)?)?;
    artifact(destination)
}
pub(super) fn directory(path: &Path) -> Result<()> {
    let mut missing = Vec::new();
    let mut ancestor = path;
    loop {
        match ancestor.symlink_metadata() {
            Ok(m) if m.file_type().is_dir() => break,
            Ok(_) => return Err(TransferError::Artifact),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                missing.push(ancestor.to_owned());
                ancestor = ancestor.parent().ok_or(TransferError::Artifact)?;
            }
            Err(e) => return Err(e.into()),
        }
    }
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))?;
    for created in &missing {
        atomic_file::sync_directory(created)?;
    }
    atomic_file::sync_directory(ancestor)?;
    Ok(())
}
pub(super) fn valid_name(name: &str) -> bool {
    !name.is_empty() && name != "." && name != ".." && !name.contains(['/', '\\', '\0'])
}
/// Counts compressed-source reads independently. A changed/truncated retained
/// file cannot be treated as the candidate whose identity was confirmed.
pub(super) struct FileStream {
    file: File,
    expected: FileIdentity,
    hash: Sha256,
    length: u64,
    ended: bool,
}
impl FileStream {
    pub fn new(path: &Path, expected: FileIdentity) -> Result<Self> {
        if identity(path)? != expected {
            return Err(TransferError::Artifact);
        }
        Ok(Self {
            file: File::open(path)?,
            expected,
            hash: Sha256::new(),
            length: 0,
            ended: false,
        })
    }
}
impl futures_core::Stream for FileStream {
    type Item = std::io::Result<bytes::Bytes>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        use std::task::Poll;
        if self.ended {
            return Poll::Ready(None);
        }
        let mut buffer = [0u8; 64 * 1024];
        match self.file.read(&mut buffer) {
            Ok(0) => {
                self.ended = true;
                let digest: [u8; 32] = self.hash.clone().finalize().into();
                if self.length != self.expected.length || digest != self.expected.sha256 {
                    Poll::Ready(Some(Err(std::io::Error::other("upload artifact changed"))))
                } else {
                    Poll::Ready(None)
                }
            }
            Ok(n) => {
                self.length += n as u64;
                self.hash.update(&buffer[..n]);
                if self.length > self.expected.length {
                    self.ended = true;
                    return Poll::Ready(Some(Err(std::io::Error::other("upload artifact grew"))));
                }
                Poll::Ready(Some(Ok(bytes::Bytes::copy_from_slice(&buffer[..n]))))
            }
            Err(e) => {
                self.ended = true;
                Poll::Ready(Some(Err(e)))
            }
        }
    }
}
