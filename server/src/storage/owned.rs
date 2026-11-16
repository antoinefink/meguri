// SPDX-License-Identifier: MIT
//! Descriptor-relative private storage. Parent of the owner root is trusted.
use super::root::{owned, unavailable};
use crate::Result;
use std::{
    ffi::CString,
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd},
        unix::fs::MetadataExt,
    },
};

pub(crate) fn open(parent: &File, name: &str, directory: bool, create: bool) -> Result<File> {
    let name = CString::new(name).map_err(|_| unavailable())?;
    let flags = libc::O_CLOEXEC
        | libc::O_NOFOLLOW
        | if directory {
            libc::O_RDONLY | libc::O_DIRECTORY
        } else {
            libc::O_RDWR
        }
        | if create {
            libc::O_CREAT | libc::O_EXCL
        } else {
            0
        };
    // SAFETY: live directory descriptor and NUL-terminated single component.
    let fd = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, 0o600) };
    if fd < 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    // SAFETY: openat returned a new owned descriptor.
    let f = unsafe { File::from_raw_fd(fd) };
    let m = f.metadata()?;
    owned(&m)?;
    if m.is_dir() != directory
        || m.mode() & 0o777 != (if directory { 0o700 } else { 0o600 })
        || (!directory && (!m.is_file() || m.nlink() != 1))
    {
        return Err(unavailable());
    }
    Ok(f)
}
pub(super) fn dir(parent: &File, name: &str, create: bool) -> Result<File> {
    if create {
        let name = CString::new(name).map_err(|_| unavailable())?;
        // SAFETY: live descriptor and single component; never follows a link.
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) } != 0 {
            let e = std::io::Error::last_os_error();
            if e.kind() != std::io::ErrorKind::AlreadyExists {
                return Err(e.into());
            }
        }
        parent.sync_all()?;
    }
    open(parent, name, true, false)
}
pub(super) fn identity(f: &File) -> Result<(u64, u64)> {
    let m = f.metadata()?;
    Ok((m.dev(), m.ino()))
}
pub(super) fn read(parent: &File, name: &str) -> Result<Vec<u8>> {
    let mut result = Vec::new();
    open(parent, name, false, false)?
        .take(4097)
        .read_to_end(&mut result)?;
    if result.len() > 4096 {
        return Err(unavailable());
    }
    Ok(result)
}
pub(crate) fn unlink(parent: &File, name: &str) -> Result<()> {
    let name = CString::new(name).map_err(|_| unavailable())?;
    // SAFETY: live descriptor and single component; unlink never follows links.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), 0) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    parent.sync_all()?;
    Ok(())
}
/// Atomic no-replace publication. Both directories remain held throughout.
pub(super) fn publish_between(from: &File, stage: &str, to: &File, final_name: &str) -> Result<()> {
    let a = CString::new(stage).map_err(|_| unavailable())?;
    let b = CString::new(final_name).map_err(|_| unavailable())?;
    // SAFETY: live descriptors and NUL-terminated single components.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::syscall(
            libc::SYS_renameat2,
            from.as_raw_fd(),
            a.as_ptr(),
            to.as_raw_fd(),
            b.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_os = "macos")]
    let result = unsafe {
        libc::renameatx_np(
            from.as_raw_fd(),
            a.as_ptr(),
            to.as_raw_fd(),
            b.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    to.sync_all()?;
    from.sync_all()?;
    Ok(())
}
pub(crate) fn publish(parent: &File, stage: &str, final_name: &str) -> Result<()> {
    publish_between(parent, stage, parent, final_name)
}
/// Caller retains both complete generations before replacing only the pointer.
pub(super) fn replace(parent: &File, stage: &str, final_name: &str) -> Result<()> {
    let a = CString::new(stage).map_err(|_| unavailable())?;
    let b = CString::new(final_name).map_err(|_| unavailable())?;
    // SAFETY: live descriptor and service-selected single components.
    if unsafe { libc::renameat(parent.as_raw_fd(), a.as_ptr(), parent.as_raw_fd(), b.as_ptr()) } != 0 {
        return Err(std::io::Error::last_os_error().into());
    }
    crate::accounts::phase("collection_after_manifest_rename")?;
    parent.sync_all()?;
    Ok(())
}
pub(super) fn write_new(parent: &File, name: &str, bytes: &[u8]) -> Result<()> {
    let mut f = open(parent, name, false, true)?;
    f.write_all(bytes)?;
    f.sync_all()?;
    parent.sync_all()?;
    Ok(())
}
