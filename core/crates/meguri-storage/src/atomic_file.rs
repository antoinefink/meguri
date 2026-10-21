//! Publish verified files with a same-directory rename (durability-and-files F4).
use crate::StorageError;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug)]
pub enum Purpose {
    Replace,
    Stage,
    Export,
    Backup,
    Media,
    Sidecar,
}
impl Purpose {
    fn name(self) -> &'static str {
        match self {
            Self::Replace => "replace",
            Self::Stage => "stage",
            Self::Export => "export",
            Self::Backup => "backup",
            Self::Media => "media",
            Self::Sidecar => "sidecar",
        }
    }
}

pub struct AtomicFile {
    destination: PathBuf,
    temp: tempfile::NamedTempFile,
    purpose: Purpose,
}
impl AtomicFile {
    pub fn new(destination: &Path, purpose: Purpose) -> Result<Self, StorageError> {
        let dir = parent(destination);
        let temp = tempfile::Builder::new()
            .prefix(&format!(".meguri-tmp-{}-", purpose.name()))
            .tempfile_in(dir)?;
        Ok(Self {
            destination: destination.to_owned(),
            temp,
            purpose,
        })
    }
    pub fn path(&self) -> &Path {
        self.temp.path()
    }
    pub fn file_mut(&mut self) -> &mut File {
        self.temp.as_file_mut()
    }
    pub fn publish(self, verify: impl FnOnce(&Path) -> Result<(), StorageError>) -> Result<(), StorageError> {
        self.finish(verify, true)
    }

    /// Backups and other unique outputs must never overwrite a collision.
    pub fn publish_new(self, verify: impl FnOnce(&Path) -> Result<(), StorageError>) -> Result<(), StorageError> {
        self.finish(verify, false)
    }

    fn finish(self, verify: impl FnOnce(&Path) -> Result<(), StorageError>, replace: bool) -> Result<(), StorageError> {
        meguri_model::faultpoint!("storage.atomic.after_write")?;
        full_sync(self.temp.as_file())?;
        meguri_model::faultpoint!("storage.atomic.after_sync")?;
        verify(self.temp.path())?;
        meguri_model::faultpoint!("storage.atomic.after_verify")?;
        meguri_model::faultpoint!("storage.atomic.before_rename")?;
        match self.purpose {
            Purpose::Stage => meguri_model::faultpoint!("storage.stage.before_rename")?,
            Purpose::Backup => meguri_model::faultpoint!("storage.backup.before_rename")?,
            _ => {}
        }
        // NamedTempFile owns only the unpublished inode; persist atomically replaces.
        if replace {
            self.temp.persist(&self.destination)
        } else {
            self.temp.persist_noclobber(&self.destination)
        }
        .map_err(|e| {
            if e.error.kind() == std::io::ErrorKind::AlreadyExists {
                StorageError::DestinationExists
            } else {
                StorageError::from(e.error)
            }
        })?;
        meguri_model::faultpoint!("storage.atomic.after_rename")?;
        sync_directory(parent(&self.destination))
    }
}

pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}
pub fn full_sync(file: &File) -> Result<(), StorageError> {
    file.sync_all()?;
    #[cfg(target_vendor = "apple")]
    {
        use std::os::fd::AsRawFd;
        // SAFETY: a live file descriptor; fcntl retains no pointer.
        if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_FULLFSYNC) } == -1 {
            return Err(std::io::Error::last_os_error().into());
        }
    }
    Ok(())
}
pub fn sync_directory(path: &Path) -> Result<(), StorageError> {
    fs::File::open(path)?.sync_all().map_err(Into::into)
}

/// Move an already synchronized file without replacing another directory entry.
pub fn move_new(source: &Path, destination: &Path) -> Result<(), StorageError> {
    use std::ffi::CString;
    use std::os::unix::ffi::OsStrExt;
    let from = CString::new(source.as_os_str().as_bytes()).map_err(|e| StorageError::Io { detail: e.to_string() })?;
    let to =
        CString::new(destination.as_os_str().as_bytes()).map_err(|e| StorageError::Io { detail: e.to_string() })?;
    // SAFETY: both paths are NUL-terminated and live throughout the call.
    #[cfg(target_os = "linux")]
    let result = unsafe {
        libc::renameat2(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_NOREPLACE,
        )
    };
    #[cfg(target_vendor = "apple")]
    let result = unsafe {
        libc::renameatx_np(
            libc::AT_FDCWD,
            from.as_ptr(),
            libc::AT_FDCWD,
            to.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result != 0 {
        let error = std::io::Error::last_os_error();
        return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
            StorageError::DestinationExists
        } else {
            error.into()
        });
    }
    sync_directory(parent(destination))?;
    if parent(source) != parent(destination) {
        sync_directory(parent(source))?;
    }
    Ok(())
}
