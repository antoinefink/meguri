//! Capacity seam (F5); callers supply their complete operation budget.
use crate::StorageError;
use std::path::Path;

pub trait SpaceProvider: Send + Sync {
    fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError>;
}
#[derive(Debug, Default)]
pub struct StatvfsSpace;
impl SpaceProvider for StatvfsSpace {
    fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
        use std::os::unix::ffi::OsStrExt;
        let path = std::ffi::CString::new(directory.as_os_str().as_bytes())
            .map_err(|e| StorageError::Io { detail: e.to_string() })?;
        let mut stat = std::mem::MaybeUninit::<libc::statvfs>::uninit();
        // SAFETY: a nul-terminated path and valid out storage; initialized only on success.
        let stat = unsafe {
            if libc::statvfs(path.as_ptr(), stat.as_mut_ptr()) != 0 {
                return Err(std::io::Error::last_os_error().into());
            }
            stat.assume_init()
        };
        #[allow(clippy::unnecessary_cast)]
        (stat.f_bavail as u64)
            .checked_mul(stat.f_frsize as u64)
            .ok_or_else(|| StorageError::Io {
                detail: "capacity overflow".into(),
            })
    }
}
pub fn require_space(provider: &dyn SpaceProvider, directory: &Path, required: u64) -> Result<(), StorageError> {
    let available = provider.available_bytes(directory)?;
    if available < required {
        Err(StorageError::InsufficientSpace { required, available })
    } else {
        Ok(())
    }
}
