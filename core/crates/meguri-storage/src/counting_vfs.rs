//! Test-only VFS observer. Select it through a file URI, before SQLite opens
//! the file; the production WAL-sync path and SQLite's own syncs run unchanged.
use std::collections::BTreeMap;
use std::ffi::{CStr, c_char, c_int};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock, PoisonError};
use std::time::Duration;

use rusqlite::ffi;

#[derive(Clone, Debug)]
pub struct SyncRecord {
    pub filename: String,
    pub open_flags: i32,
    pub flags: i32,
    pub started: Duration,
    pub completed: Duration,
    pub result: i32,
}

type Records = Arc<Mutex<Vec<SyncRecord>>>;
static PATHS: Mutex<BTreeMap<PathBuf, Records>> = Mutex::new(BTreeMap::new());
static FILES: Mutex<BTreeMap<usize, Arc<File>>> = Mutex::new(BTreeMap::new());
static PARENT: OnceLock<usize> = OnceLock::new();
static REGISTERED: OnceLock<()> = OnceLock::new();

/// Monotonic clock shared by the observer and its tests, independent of the
/// executor's durability events.
pub mod clock {
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};
    #[allow(clippy::disallowed_methods)] // Test measurement clock, never collection time.
    pub fn now() -> Duration {
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed()
    }
}

/// Keeps URI selection scoped to one collection path. Existing open handles
/// retain their records until close, even after this guard drops.
pub struct CountingVfs {
    path: PathBuf,
    records: Records,
}
impl CountingVfs {
    pub fn for_path(path: &Path) -> Self {
        register();
        let path = path.canonicalize().expect("existing test database");
        let records = Arc::new(Mutex::new(Vec::new()));
        assert!(PATHS.lock().unwrap().insert(path.clone(), records.clone()).is_none());
        Self { path, records }
    }
    pub fn records(&self) -> Vec<SyncRecord> {
        self.records.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }
}
impl Drop for CountingVfs {
    fn drop(&mut self) {
        PATHS.lock().unwrap_or_else(PoisonError::into_inner).remove(&self.path);
    }
}
pub(crate) fn selected(path: &Path) -> bool {
    PATHS.lock().unwrap_or_else(PoisonError::into_inner).contains_key(path)
}

struct File {
    original: usize,
    methods: usize,
    filename: String,
    open_flags: i32,
    records: Option<Records>,
}
impl Drop for File {
    fn drop(&mut self) {
        // SAFETY: allocated once in open; SQLite no longer uses this table
        // after close (or after the last in-progress sync returns).
        unsafe { drop(Box::from_raw(self.methods as *mut ffi::sqlite3_io_methods)) }
    }
}
fn context(file: *mut ffi::sqlite3_file) -> Arc<File> {
    FILES.lock().unwrap_or_else(PoisonError::into_inner)[&(file as usize)].clone()
}
fn parent() -> *mut ffi::sqlite3_vfs {
    *PARENT.get().expect("registered VFS") as *mut ffi::sqlite3_vfs
}

unsafe extern "C" fn open(
    _: *mut ffi::sqlite3_vfs,
    name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out: *mut c_int,
) -> c_int {
    // SAFETY: SQLite supplies its allocated file buffer and filename. The
    // parent's file size is unchanged. Only xSync/xClose are interposed;
    // every other method keeps receiving the original file pointer.
    unsafe {
        let result = ((*parent()).xOpen.unwrap())(parent(), name, file, flags, out);
        if result != ffi::SQLITE_OK {
            return result;
        }
        let filename = if name.is_null() {
            String::new()
        } else {
            CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        let paths = PATHS.lock().unwrap_or_else(PoisonError::into_inner);
        let records = paths.iter().find_map(|(path, records)| {
            let base = path.to_string_lossy();
            (filename == base
                || ["-wal", "-journal", "-shm"]
                    .iter()
                    .any(|suffix| filename == format!("{base}{suffix}")))
            .then(|| records.clone())
        });
        drop(paths);
        let original = (*file).pMethods;
        let mut methods = Box::new(std::ptr::read(original));
        methods.xSync = Some(sync);
        methods.xClose = Some(close);
        let methods = Box::into_raw(methods);
        FILES.lock().unwrap_or_else(PoisonError::into_inner).insert(
            file as usize,
            Arc::new(File {
                original: original as usize,
                methods: methods as usize,
                filename,
                open_flags: flags,
                records,
            }),
        );
        (*file).pMethods = methods;
        result
    }
}
unsafe extern "C" fn sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    let context = context(file);
    let started = clock::now();
    // SAFETY: the live handle and flags pass unchanged to its original VFS.
    let result = unsafe { ((*(context.original as *const ffi::sqlite3_io_methods)).xSync.unwrap())(file, flags) };
    let completed = clock::now();
    if let Some(records) = &context.records {
        records.lock().unwrap_or_else(PoisonError::into_inner).push(SyncRecord {
            filename: context.filename.clone(),
            open_flags: context.open_flags,
            flags,
            started,
            completed,
            result,
        });
    }
    result
}
unsafe extern "C" fn close(file: *mut ffi::sqlite3_file) -> c_int {
    let context = FILES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .remove(&(file as usize))
        .unwrap();
    // SAFETY: restore the original table before forwarding close; the owned
    // copy outlives that call. SQLite serializes use of each file handle.
    unsafe {
        (*file).pMethods = context.original as *const ffi::sqlite3_io_methods;
        ((*(*file).pMethods).xClose.unwrap())(file)
    }
}

// Pass the parent's own VFS pointer to all its other callbacks, preserving
// its pAppData and any identity checks. Dynamic-loading callbacks are disabled
// on this test VFS; no Meguri connection loads extensions.
macro_rules! forward {
    ($name:ident, $field:ident, ($($arg:ident: $ty:ty),*) -> $ret:ty) => {
        unsafe extern "C" fn $name(_: *mut ffi::sqlite3_vfs, $($arg: $ty),*) -> $ret {
            // SAFETY: the callback contract and arguments are unchanged.
            unsafe { ((*parent()).$field.unwrap())(parent(), $($arg),*) }
        }
    };
}
forward!(delete, xDelete, (name: *const c_char, sync_dir: c_int) -> c_int);
forward!(access, xAccess, (name: *const c_char, flags: c_int, out: *mut c_int) -> c_int);
forward!(full_path, xFullPathname, (name: *const c_char, size: c_int, out: *mut c_char) -> c_int);
forward!(randomness, xRandomness, (size: c_int, out: *mut c_char) -> c_int);
forward!(sleep, xSleep, (micros: c_int) -> c_int);
forward!(current_time, xCurrentTime, (out: *mut f64) -> c_int);
forward!(last_error, xGetLastError, (size: c_int, out: *mut c_char) -> c_int);
forward!(current_time_i64, xCurrentTimeInt64, (out: *mut i64) -> c_int);

fn register() {
    REGISTERED.get_or_init(|| {
        // SAFETY: SQLite owns the default VFS for process lifetime. The
        // registered copy is also process-lived and never made the default.
        // OnceLock serializes registration; SQLite serializes its VFS list.
        unsafe {
            let original = ffi::sqlite3_vfs_find(std::ptr::null());
            assert!(!original.is_null());
            PARENT.set(original as usize).unwrap();
            let mut vfs = Box::new(std::ptr::read(original));
            vfs.iVersion = 2.min(vfs.iVersion);
            vfs.pNext = std::ptr::null_mut();
            vfs.zName = c"meguri-counting".as_ptr();
            vfs.xOpen = Some(open);
            vfs.xDelete = Some(delete);
            vfs.xAccess = Some(access);
            vfs.xFullPathname = Some(full_path);
            vfs.xDlOpen = None;
            vfs.xDlError = None;
            vfs.xDlSym = None;
            vfs.xDlClose = None;
            vfs.xRandomness = Some(randomness);
            vfs.xSleep = Some(sleep);
            vfs.xCurrentTime = Some(current_time);
            vfs.xGetLastError = Some(last_error);
            vfs.xCurrentTimeInt64 = Some(current_time_i64);
            assert_eq!(ffi::sqlite3_vfs_register(Box::into_raw(vfs), 0), ffi::SQLITE_OK);
        }
    });
}
