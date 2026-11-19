// SPDX-License-Identifier: MIT
//! Descriptor-relative private storage. Parent of the owner root is trusted.
use super::root::{owned, unavailable};
use crate::Result;
use std::{
    ffi::{CStr, CString},
    fs::File,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, IntoRawFd},
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
/// Absence is only ENOENT; links, wrong types and permissions fail closed.
pub(super) fn optional_dir(parent: &File, name: &str) -> Result<Option<File>> {
    let component = CString::new(name).map_err(|_| unavailable())?;
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: valid descriptor, component and writable stat storage.
    if unsafe { libc::fstatat(parent.as_raw_fd(), component.as_ptr(), metadata.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 {
        let error = std::io::Error::last_os_error();
        return if error.raw_os_error() == Some(libc::ENOENT) { Ok(None) } else { Err(error.into()) };
    }
    Ok(Some(open(parent, name, true, false)?))
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

struct DirectoryStream(*mut libc::DIR);
impl DirectoryStream {
    fn new(directory: &File) -> Result<Self> {
        let duplicate = directory.try_clone()?;
        // SAFETY: duplicate is a live owned directory fd. On failure it stays
        // owned by File; on success ownership transfers to the DIR stream.
        let stream = unsafe { libc::fdopendir(duplicate.as_raw_fd()) };
        if stream.is_null() {
            return Err(std::io::Error::last_os_error().into());
        }
        let _ = duplicate.into_raw_fd();
        let stream = Self(stream);
        // SAFETY: live stream; a retry on a duplicated fd must start at zero.
        unsafe { libc::rewinddir(stream.0) };
        Ok(stream)
    }
}
impl Drop for DirectoryStream {
    fn drop(&mut self) {
        // SAFETY: this guard exclusively owns the live stream and its fd.
        unsafe { libc::closedir(self.0) };
    }
}

fn stat_at(parent: &File, name: &CStr) -> std::io::Result<libc::stat> {
    let mut metadata = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: live parent, NUL-terminated entry name and writable stat storage.
    if unsafe { libc::fstatat(parent.as_raw_fd(), name.as_ptr(), metadata.as_mut_ptr(), libc::AT_SYMLINK_NOFOLLOW) } != 0 {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: successful fstatat initialized the stat storage.
    Ok(unsafe { metadata.assume_init() })
}
fn checked_directory(directory: &File) -> Result<std::fs::Metadata> {
    let metadata = directory.metadata()?;
    owned(&metadata)?;
    if !metadata.is_dir() || metadata.mode() & 0o777 != 0o700 {
        return Err(unavailable());
    }
    Ok(metadata)
}
fn same_directory(named: &libc::stat, opened: &std::fs::Metadata) -> Result<()> {
    if named.st_mode & libc::S_IFMT != libc::S_IFDIR
        || named.st_dev as u64 != opened.dev()
        || named.st_ino as u64 != opened.ino()
        || named.st_uid != opened.uid()
        || named.st_mode & 0o777 != 0o700
    {
        return Err(unavailable());
    }
    Ok(())
}
fn unlink_empty_directory(parent: &File, name: &CStr, directory: &File, allow_missing: bool) -> Result<()> {
    let opened = checked_directory(directory)?;
    let named = match stat_at(parent, name) {
        Ok(named) => named,
        Err(error) if allow_missing && error.raw_os_error() == Some(libc::ENOENT) => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    same_directory(&named, &opened)?;
    // SAFETY: live parent and checked single component; never follows links.
    if unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), libc::AT_REMOVEDIR) } != 0 {
        let error = std::io::Error::last_os_error();
        if !allow_missing || error.raw_os_error() != Some(libc::ENOENT) { return Err(error.into()); }
    }
    Ok(())
}
/// Only unlink a name still bound to the exact descriptor emptied by purge.
/// The caller syncs the retained parent before clearing its durable intent.
pub(super) fn remove_empty_directory(parent: &File, name: &str, directory: &File) -> Result<()> {
    let name = CString::new(name).map_err(|_| unavailable())?;
    unlink_empty_directory(parent, &name, directory, false)
}
/// Empty this inode without resolving its original path again.
pub(super) fn purge(directory: &File) -> Result<()> {
    purge_on_device(directory, checked_directory(directory)?.dev())
}
fn purge_on_device(directory: &File, device: u64) -> Result<()> {
    if checked_directory(directory)?.dev() != device {
        return Err(unavailable());
    }
    let stream = DirectoryStream::new(directory)?;
    loop {
        // readdir reports EOF and errors as null; distinguish using errno.
        #[cfg(target_os = "linux")]
        // SAFETY: libc returns the current thread's errno storage.
        let errno = unsafe { libc::__errno_location() };
        #[cfg(target_os = "macos")]
        // SAFETY: libc returns the current thread's errno storage.
        let errno = unsafe { libc::__error() };
        // SAFETY: errno is thread-local and stream is live, exclusively owned.
        let entry = unsafe {
            *errno = 0;
            libc::readdir(stream.0)
        };
        if entry.is_null() {
            let error = std::io::Error::last_os_error();
            return if error.raw_os_error() == Some(0) { Ok(()) } else { Err(error.into()) };
        }
        // SAFETY: readdir's NUL-terminated name is valid until the next call.
        let name = unsafe { CStr::from_ptr((*entry).d_name.as_ptr()) };
        if name.to_bytes() == b"." || name.to_bytes() == b".." { continue; }
        // Native temporary cleanup may remove descendants outside the barrier.
        let named = match stat_at(directory, name) {
            Ok(named) => named,
            Err(error) if error.raw_os_error() == Some(libc::ENOENT) => continue,
            Err(error) => return Err(error.into()),
        };
        if named.st_dev as u64 != device { return Err(unavailable()); }
        if named.st_mode & libc::S_IFMT == libc::S_IFDIR {
            // SAFETY: live parent and entry name; only open an actual directory.
            let fd = unsafe { libc::openat(directory.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_CLOEXEC | libc::O_DIRECTORY | libc::O_NOFOLLOW) };
            if fd < 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() == Some(libc::ENOENT) { continue; }
                return Err(error.into());
            }
            // SAFETY: openat returned a new owned descriptor.
            let child = unsafe { File::from_raw_fd(fd) };
            same_directory(&named, &checked_directory(&child)?)?;
            purge_on_device(&child, device)?;
            unlink_empty_directory(directory, name, &child, true)?;
        } else {
            // SAFETY: live parent and entry name; unlink does not follow links
            // and refuses a directory substituted for a non-directory entry.
            if unsafe { libc::unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) } != 0 {
                let error = std::io::Error::last_os_error();
                if error.raw_os_error() != Some(libc::ENOENT) { return Err(error.into()); }
            }
        }
    }
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
    crate::accounts::phase("owned_after_publish_rename")?;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::storage::{DataRoot, deletion};
    use std::{fs, os::unix::ffi::OsStrExt, path::Path};

    #[test]
    fn missing_descendant_is_gone_but_root_removal_stays_strict() {
        let temp = tempfile::tempdir().unwrap();
        let parent = File::open(temp.path()).unwrap();
        let child = dir(&parent, "child", true).unwrap();
        remove_empty_directory(&parent, "child", &child).unwrap();
        let name = CString::new("child").unwrap();
        assert_eq!(stat_at(&parent, &name).err().unwrap().raw_os_error(), Some(libc::ENOENT));
        unlink_empty_directory(&parent, &name, &child, true).unwrap();
        assert!(remove_empty_directory(&parent, "child", &child).is_err());
    }

    #[test]
    fn purge_uses_opened_inode_and_rebound_quarantine_keeps_intent() {
        let temp = tempfile::tempdir().unwrap();
        let mut root = DataRoot::acquire(&temp.path().join("owner"), true).unwrap();
        root.init().unwrap();
        root.upgrade_media().unwrap();
        root.upgrade_deletions().unwrap();
        let parent = root.bound_dir(Path::new("state/accounts"), false).unwrap();
        let id = crate::crypto::random_token().unwrap();
        let quarantine = format!(".delete-{id}");
        let path = root.state().join("accounts").join(&quarantine);
        let original = dir(&parent, &quarantine, true).unwrap();
        write_new(&original, "remove", b"original bytes").unwrap();
        let child = dir(&original, "nested", true).unwrap();
        write_new(&child, "remove", b"nested bytes").unwrap();
        drop(child);
        fs::rename(path.join("nested"), path.join(std::ffi::OsStr::from_bytes(b"nested-\xff"))).unwrap();
        let job = deletion::Pending { id, binding: Some(identity(&original).unwrap()) };
        let connection = crate::storage::open(root.database(), false).unwrap();
        deletion::insert(&connection, &job).unwrap();
        drop(connection);

        let moved = path.with_file_name(".moved-original");
        fs::rename(&path, &moved).unwrap();
        let replacement = dir(&parent, &quarantine, true).unwrap();
        write_new(&replacement, "keep", b"replacement bytes").unwrap();
        assert_ne!(identity(&replacement).unwrap(), job.binding.unwrap());
        purge(&original).unwrap();
        assert!(fs::read_dir(&moved).unwrap().next().is_none());
        assert_eq!(remove_empty_directory(&parent, &quarantine, &original).unwrap_err(), unavailable());
        assert_eq!(deletion::finish(&root, &job).unwrap_err(), unavailable());
        assert_eq!(read(&replacement, "keep").unwrap(), b"replacement bytes");
        let connection = crate::storage::open(root.database(), false).unwrap();
        assert_eq!(deletion::read(&connection, &job.id).unwrap(), Some(job));
    }
}
