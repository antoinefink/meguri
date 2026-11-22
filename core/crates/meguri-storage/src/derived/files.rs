//! Files a component owns beside the store (`storage/derived-store` D1,
//! D5.1; `stats/graphs-common` G9.3): one directory per component under the
//! store's files directory, the sweep at bind of every file no record row
//! names, and read-only memory maps of the files a reader uses.

use std::collections::HashSet;
use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::derived::registry::{ComponentSchema, Registry, Target};
use crate::error::StorageError;

/// The directory, next to `meguri-derived.db`, that holds one directory per
/// component with files (D1).
pub const FILES_DIR: &str = "meguri-derived.files";

/// The files directory of the store at `derived` (its `meguri-derived.db`).
pub fn files_root(derived: &Path) -> PathBuf {
    derived.with_file_name(FILES_DIR)
}

/// The directory only `component` uses, for the store at `derived`.
pub fn component_dir(derived: &Path, component: &str) -> PathBuf {
    files_root(derived).join(component)
}

fn io_error(what: &str, path: &Path, error: &io::Error) -> StorageError {
    StorageError::Io {
        detail: format!("{what} {}: {error}", path.display()),
    }
}

/// The names the live record table of `schema` holds, when it has one.
pub(crate) fn named_files(conn: &Connection, schema: &ComponentSchema) -> Result<HashSet<String>, StorageError> {
    let Some(table) = &schema.files else {
        return Ok(HashSet::new());
    };
    let stored = schema.stored_table(table, Target::Live);
    let mut statement = conn.prepare(&format!("SELECT name FROM \"{stored}\""))?;
    let names = statement
        .query_map([], |row| row.get::<_, String>(0))?
        .collect::<rusqlite::Result<HashSet<_>>>()?;
    Ok(names)
}

/// Deletes every file of `dir` that `keep` does not hold; a missing
/// directory has none. The failures, each logged by the caller, are
/// retried at the next sweep: a file no row names is never read (D1).
pub fn sweep_dir(dir: &Path, keep: &HashSet<String>) -> Vec<StorageError> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => return vec![io_error("listing", dir, &error)],
    };
    let mut failures = Vec::new();
    for entry in entries {
        let entry = match entry {
            Ok(entry) => entry,
            Err(error) => {
                failures.push(io_error("listing", dir, &error));
                continue;
            }
        };
        let name = entry.file_name();
        if name.to_str().is_some_and(|name| keep.contains(name)) {
            continue;
        }
        let path = entry.path();
        let removed = if entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            fs::remove_dir_all(&path)
        } else {
            fs::remove_file(&path)
        };
        if let Err(error) = removed
            && error.kind() != io::ErrorKind::NotFound
        {
            failures.push(io_error("removing", &path, &error));
        }
    }
    failures
}

/// D5.1's sweep, after the components were synced: each registered
/// component's directory loses every file no row of its record table names;
/// the directory of a component that is no longer registered, or that has
/// no files, is removed; when the file was created or recreated, every
/// component directory goes. Returns the failures, which the next bind
/// retries.
pub(crate) fn sweep(conn: &Connection, derived: &Path, registry: &Registry, recreated: bool) -> Vec<StorageError> {
    let root = files_root(derived);
    if recreated {
        return sweep_dir(&root, &HashSet::new());
    }
    let mut failures = Vec::new();
    let mut kept = HashSet::new();
    for schema in registry.schemas() {
        if schema.files.is_none() {
            continue;
        }
        match named_files(conn, schema) {
            Ok(names) => {
                failures.extend(sweep_dir(&root.join(&schema.name), &names));
                kept.insert(schema.name.clone());
            }
            // Its directory is kept as it is: the next bind tries again.
            Err(error) => {
                failures.push(error);
                kept.insert(schema.name.clone());
            }
        }
    }
    failures.extend(sweep_dir(&root, &kept));
    failures
}

/// Writes `bytes` whole into a new file `name` of `dir` (created when
/// missing) and flushes it to disk (G9.3). Fails when the name exists.
pub fn write_new(dir: &Path, name: &str, bytes: &[u8]) -> Result<PathBuf, StorageError> {
    fs::create_dir_all(dir).map_err(|e| io_error("creating", dir, &e))?;
    let path = dir.join(name);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| io_error("creating", &path, &e))?;
    let written = file.write_all(bytes).and_then(|()| file.sync_all());
    if let Err(error) = written {
        let _ = fs::remove_file(&path);
        return Err(io_error("writing", &path, &error));
    }
    // The new name reaches the disk with the directory.
    if let Ok(directory) = fs::File::open(dir) {
        let _ = directory.sync_all();
    }
    Ok(path)
}

/// A file mapped read-only: its pages are the file's, so they do not count
/// against the process's memory and the system can drop them (G9.3). The
/// mapping stays valid after the file is deleted.
pub struct MappedFile {
    address: *mut libc::c_void,
    len: usize,
}

// SAFETY: the mapping is read-only and private to this value, and a
// component file is never written again once `write_new` created it
// (`create_new`, then only read and deleted): its bytes cannot change under
// a reader. Deleting it while mapped is fine; truncating it from outside
// the store would not be (SIGBUS), which D1 rules out while the collection
// is open.
unsafe impl Send for MappedFile {}
// SAFETY: as above.
unsafe impl Sync for MappedFile {}

impl std::fmt::Debug for MappedFile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MappedFile").field("len", &self.len).finish()
    }
}

impl MappedFile {
    /// Maps the whole file at `path`.
    pub fn open(path: &Path) -> Result<MappedFile, StorageError> {
        let file = fs::File::open(path).map_err(|e| io_error("opening", path, &e))?;
        let len = file.metadata().map_err(|e| io_error("reading", path, &e))?.len();
        let len = usize::try_from(len).map_err(|_| StorageError::Io {
            detail: format!("{} is too large to map", path.display()),
        })?;
        if len == 0 {
            return Ok(MappedFile {
                address: std::ptr::null_mut(),
                len: 0,
            });
        }
        use std::os::fd::AsRawFd;
        // SAFETY: a read-only private mapping of a whole regular file that
        // this process opened; the result is checked before use.
        let address = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ,
                libc::MAP_PRIVATE,
                file.as_raw_fd(),
                0,
            )
        };
        if address == libc::MAP_FAILED {
            return Err(io_error("mapping", path, &io::Error::last_os_error()));
        }
        Ok(MappedFile { address, len })
    }

    pub fn bytes(&self) -> &[u8] {
        if self.len == 0 {
            return &[];
        }
        // SAFETY: the mapping holds `len` readable bytes until `drop`.
        unsafe { std::slice::from_raw_parts(self.address.cast::<u8>(), self.len) }
    }
}

impl Drop for MappedFile {
    fn drop(&mut self) {
        if self.len > 0 {
            // SAFETY: the address and length are those mmap returned.
            unsafe {
                libc::munmap(self.address, self.len);
            }
        }
    }
}
