use super::{MAX_ASSET_BYTES, blob::Blob};
use crate::cloud::Error;
use sha1::{Digest as Sha1Digest, Sha1};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[cfg(feature = "test-hooks")]
static FAIL_OPEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
#[cfg(feature = "test-hooks")]
pub(crate) fn fail_next_open() {
    FAIL_OPEN.store(true, std::sync::atomic::Ordering::SeqCst);
}
pub(crate) fn open(path: &Path) -> Result<File, Error> {
    #[cfg(feature = "test-hooks")]
    if FAIL_OPEN.swap(false, std::sync::atomic::Ordering::SeqCst) {
        return Err(Error::Io(std::io::ErrorKind::Other));
    }

    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(Error::InvalidData);
    }
    Ok(file)
}
pub(crate) fn stream(mut file: File, output: &mut impl Write) -> Result<Blob, Error> {
    let before = file.metadata()?;
    let mut sha256 = Sha256::new();
    let mut sha1 = Sha1::new();
    let mut size = 0;
    let mut buffer = [0u8; 65_536];
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        size += n as u64;
        if size > MAX_ASSET_BYTES {
            return Err(Error::InvalidData);
        }
        output.write_all(&buffer[..n])?;
        sha256.update(&buffer[..n]);
        sha1.update(&buffer[..n]);
    }
    let after = file.metadata()?;
    if size == 0
        || before.dev() != after.dev()
        || before.ino() != after.ino()
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(Error::InvalidData);
    }
    Ok(Blob {
        sha256: sha256.finalize().into(),
        sha1: sha1.finalize().into(),
        size,
    })
}
pub(crate) fn hash(file: File) -> Result<Blob, Error> {
    stream(file, &mut std::io::sink())
}
pub(crate) fn stage(file: File, directory: &Path) -> Result<(PathBuf, Blob), Error> {
    fs::create_dir_all(directory)?;
    if fs::symlink_metadata(directory)?.file_type().is_symlink() {
        return Err(Error::InvalidData);
    }
    let mut staged = tempfile::Builder::new().prefix("asset-").tempfile_in(directory)?;
    let metadata = stream(file, staged.as_file_mut())?;
    staged.as_file().sync_all()?;
    let (_, path) = staged.keep().map_err(|e| Error::from(e.error))?;
    File::open(directory)?.sync_all()?;
    Ok((path, metadata))
}
pub(crate) fn sha1_text(sha1: &[u8; 20]) -> String {
    sha1.iter().map(|b| format!("{b:02x}")).collect()
}
