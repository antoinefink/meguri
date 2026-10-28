//! Device-only shutdown state; all mutations share a strict durable RMW (F3/R6).
use crate::{
    StorageError,
    atomic_file::{AtomicFile, Purpose, parent},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{
    collections::BTreeMap,
    io::Write,
    os::unix::fs::MetadataExt,
    path::Path,
    sync::{Arc, Mutex, Weak},
};

// Engine collection ownership excludes other processes; this lock also covers
// independent metadata consumers within that owner. Never nest an accessor,
// even for another filename in the same directory.
type DirectoryIdentity = (u64, u64);
#[derive(Default)]
struct DirectoryLocks {
    entries: BTreeMap<DirectoryIdentity, Weak<Mutex<()>>>,
    cleanup_cursor: Option<DirectoryIdentity>,
}
impl DirectoryLocks {
    fn get(&mut self, identity: DirectoryIdentity) -> Arc<Mutex<()>> {
        // Bound registry work regardless of the number of active collections.
        // Rotate through keys so dead entries beyond live entries are reclaimed.
        for _ in 0..8 {
            use std::ops::Bound::{Excluded, Unbounded};
            let next = self
                .cleanup_cursor
                .and_then(|key| self.entries.range((Excluded(key), Unbounded)).next())
                .or_else(|| self.entries.first_key_value())
                .map(|(&key, value)| (key, value.strong_count() == 0));
            let Some((key, dead)) = next else {
                break;
            };
            self.cleanup_cursor = Some(key);
            if dead {
                self.entries.remove(&key);
            }
        }
        if let Some(lock) = self.entries.get(&identity).and_then(Weak::upgrade) {
            return lock;
        }
        let lock = Arc::new(Mutex::new(()));
        self.entries.insert(identity, Arc::downgrade(&lock));
        lock
    }
}
static DIRECTORY_LOCKS: Mutex<DirectoryLocks> = Mutex::new(DirectoryLocks {
    entries: BTreeMap::new(),
    cleanup_cursor: None,
});

fn directory_identity(path: &Path) -> std::io::Result<DirectoryIdentity> {
    // stat follows parent aliases, without opening/closing the SQLite main file
    // (which could release this process's POSIX locks). Never key on the leaf:
    // atomic publication changes its inode, and case aliases may name it too.
    let metadata = std::fs::metadata(parent(path))?;
    if !metadata.is_dir() {
        return Err(std::io::Error::from_raw_os_error(libc::ENOTDIR));
    }
    Ok((metadata.dev(), metadata.ino()))
}
fn directory_lock(path: &Path) -> std::io::Result<(DirectoryIdentity, Arc<Mutex<()>>)> {
    let identity = directory_identity(path)?;
    let lock = DIRECTORY_LOCKS
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .get(identity);
    Ok((identity, lock))
}
fn validate_directory(path: &Path, identity: DirectoryIdentity) -> Result<(), StorageError> {
    // A waiter must not access a replacement directory under the old mutex.
    // Ownership must keep the directory stable throughout the accessor, as it
    // must for AtomicFile. Retain the Arc until after the operation's guard.
    if directory_identity(path)? != identity {
        return Err(StorageError::Io {
            detail: "sidecar parent directory changed while acquiring its lock".into(),
        });
    }
    Ok(())
}

fn invalid(detail: impl ToString) -> StorageError {
    StorageError::Io {
        detail: format!("unreadable sidecar: {}", detail.to_string()),
    }
}
fn read(path: &Path) -> Result<Option<Map<String, Value>>, StorageError> {
    match std::fs::read(path) {
        Ok(bytes) => match serde_json::from_slice(&bytes).map_err(invalid)? {
            Value::Object(value) => Ok(Some(value)),
            _ => Err(invalid("expected an object")),
        },
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}

/// Strict locked read. Missing and unreadable are distinct. The callback must
/// not call another sidecar accessor. Collection ownership is required.
pub fn read_document<T>(
    path: &Path,
    f: impl FnOnce(Option<&Map<String, Value>>) -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    // A missing parent also means a missing document. Other metadata failures
    // fail closed; never create a directory or use an alias-unsafe fallback key.
    let (identity, lock) = match directory_lock(path) {
        Ok(value) => value,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return f(None),
        Err(error) => return Err(error.into()),
    };
    let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    validate_directory(path, identity)?;
    let document = read(path)?;
    f(document.as_ref())
}

/// The shared read/modify/durable-write seam. Retains members the caller does
/// not edit; an unreadable existing document is never replaced by defaults.
pub fn update_document<T>(
    path: &Path,
    f: impl FnOnce(&mut Map<String, Value>) -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    // Missing/unstatable parents fail before the callback or any publication.
    let (identity, lock) = directory_lock(path)?;
    let _guard = lock.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    validate_directory(path, identity)?;
    let mut document = read(path)?.unwrap_or_default();
    let result = f(&mut document)?;
    meguri_model::faultpoint!("storage.sidecar.before_save")?;
    let bytes = serde_json::to_vec(&document).map_err(invalid)?;
    let mut file = AtomicFile::new(path, Purpose::Sidecar)?;
    file.file_mut().write_all(&bytes)?;
    file.publish(|_| Ok(()))?;
    Ok(result)
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Sidecar {
    pub clean_shutdown: bool,
    pub last_mod: i64,
    pub last_scm: i64,
    pub upgrade_in_progress: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}
impl Sidecar {
    pub fn load(path: &Path) -> Result<Self, StorageError> {
        Ok(Self::load_existing(path)?.unwrap_or_default())
    }
    /// F3 validity is independent of R6 validity. Partial F3 objects count as
    /// unclean, while their unknown members remain available to the strict RMW.
    pub(crate) fn load_existing(path: &Path) -> Result<Option<Self>, StorageError> {
        read_document(path, |doc| {
            Ok(doc.and_then(|d| serde_json::from_value(Value::Object(d.clone())).ok()))
        })
    }
    pub fn update<T>(path: &Path, f: impl FnOnce(&mut Self, bool) -> T) -> Result<T, StorageError> {
        update_document(path, |doc| {
            let previous = serde_json::from_value::<Self>(Value::Object(doc.clone())).ok();
            let known = previous.is_some();
            let mut sidecar = previous.unwrap_or_default();
            let result = f(&mut sidecar, known);
            write_fields(doc, &sidecar);
            Ok(result)
        })
    }
    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        update_document(path, |doc| {
            // Loaded extras may be stale. In particular F3 never writes R6.
            for (key, value) in &self.extra {
                if key != "deviceState" {
                    doc.entry(key.clone()).or_insert_with(|| value.clone());
                }
            }
            write_fields(doc, self);
            Ok(())
        })
    }
}
fn write_fields(doc: &mut Map<String, Value>, sidecar: &Sidecar) {
    doc.insert("clean_shutdown".into(), sidecar.clean_shutdown.into());
    doc.insert("last_mod".into(), sidecar.last_mod.into());
    doc.insert("last_scm".into(), sidecar.last_scm.into());
    doc.insert("upgrade_in_progress".into(), sidecar.upgrade_in_progress.into());
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn held_read_or_rmw_allows_another_directory_to_publish() {
        let a = tempfile::tempdir().unwrap();
        let b = tempfile::tempdir().unwrap();
        let path_a = a.path().join("meguri-sidecar.json");
        let path_b = b.path().join("meguri-sidecar.json");
        let (_, lock_a) = directory_lock(&path_a).unwrap();
        let (_, lock_b) = directory_lock(&path_b).unwrap();
        let check_domain = || {
            // Verify the public accessors actually hold the registered mutex.
            // The former global implementation fails here without hanging.
            assert!(matches!(lock_a.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
            assert!(lock_b.try_lock().is_ok());
        };
        read_document(&path_a, |_| {
            check_domain();
            Ok(())
        })
        .unwrap();
        update_document(&path_a, |_| {
            check_domain();
            Ok(())
        })
        .unwrap();
        // Hold that same A mutex while B performs a real durable RMW. Accessors
        // are never nested; no sleeps, barriers, threads or timing assertions.
        let guard_a = lock_a.lock().unwrap();
        assert!(lock_b.try_lock().is_ok());
        update_document(&path_b, |doc| {
            doc.insert("count".into(), 1.into());
            Ok(())
        })
        .unwrap();
        drop(guard_a);
        read_document(&path_b, |doc| {
            assert_eq!(doc.unwrap()["count"], 1);
            Ok(())
        })
        .unwrap();
    }

    #[test]
    fn parent_aliases_and_all_leaf_names_share_one_lock_across_publish() {
        use std::os::unix::fs::symlink;
        // Do not change process CWD: other tests may be running concurrently.
        let root = tempfile::tempdir_in(".").unwrap();
        let cwd = std::env::current_dir().unwrap();
        let relative = root.path().strip_prefix(&cwd).unwrap_or(root.path()).join("Owned");
        std::fs::create_dir(&relative).unwrap();
        let absolute = cwd.join(&relative);
        let link = root.path().join("alias");
        symlink(&absolute, &link).unwrap();
        let path = relative.join("meguri-sidecar.json");
        let (_, lock) = directory_lock(&path).unwrap();
        let mut aliases = vec![
            absolute.join("meguri-sidecar.json"),
            relative.join(".").join("another-sidecar.json"),
            link.join("MEGURI-SIDECAR.JSON"),
        ];
        // Exercise native case aliases on filesystems that provide them.
        let case_alias = root.path().join("owned").join("meguri-sidecar.json");
        if directory_identity(&case_alias).ok() == directory_identity(&path).ok() {
            aliases.push(case_alias);
        }
        let guard = lock.lock().unwrap();
        for alias in &aliases {
            let (_, other) = directory_lock(alias).unwrap();
            assert!(Arc::ptr_eq(&lock, &other));
            assert!(matches!(other.try_lock(), Err(std::sync::TryLockError::WouldBlock)));
        }
        drop(guard);
        // Both absent and repeatedly replaced sidecar leaves use the same key.
        for _ in 0..2 {
            update_document(&path, |_| Ok(())).unwrap();
            assert!(Arc::ptr_eq(&lock, &directory_lock(&path).unwrap().1));
        }
    }

    #[test]
    fn replacement_parent_is_rejected_and_old_directory_keeps_its_lock() {
        use std::os::unix::fs::symlink;
        let root = tempfile::tempdir().unwrap();
        let directory = root.path().join("owned");
        let moved = root.path().join("moved");
        std::fs::create_dir(&directory).unwrap();
        let path = directory.join("meguri-sidecar.json");
        let (identity, lock) = directory_lock(&path).unwrap();
        std::fs::rename(&directory, &moved).unwrap();
        std::fs::create_dir(&directory).unwrap();
        assert!(validate_directory(&path, identity).is_err());
        assert!(!Arc::ptr_eq(&lock, &directory_lock(&path).unwrap().1));
        assert!(Arc::ptr_eq(
            &lock,
            &directory_lock(&moved.join("meguri-sidecar.json")).unwrap().1
        ));
        let alias = root.path().join("alias");
        symlink(&moved, &alias).unwrap();
        let alias_path = alias.join("meguri-sidecar.json");
        assert!(Arc::ptr_eq(&lock, &directory_lock(&alias_path).unwrap().1));
        std::fs::remove_file(&alias).unwrap();
        symlink(&directory, &alias).unwrap();
        assert!(validate_directory(&alias_path, identity).is_err());
    }

    #[test]
    fn registry_retains_waiters_and_reclaims_dead_entries_in_bounded_steps() {
        let mut registry = DirectoryLocks::default();
        let lock = registry.get((1, 1));
        let waiter = registry.get((1, 1));
        let weak = Arc::downgrade(&lock);
        drop(lock);
        assert!(Arc::ptr_eq(&waiter, &registry.get((1, 1))));
        drop(waiter);
        assert!(weak.upgrade().is_none());
        let replacement = registry.get((1, 1));
        assert!(!Weak::ptr_eq(&weak, &Arc::downgrade(&replacement)));

        for key in 2..34 {
            registry.entries.insert((1, key), Weak::new());
        }
        let before = registry.entries.len();
        registry.get((1, 1));
        assert_eq!(registry.entries.len(), before - 8);
        for _ in 0..4 {
            registry.get((1, 1));
        }
        assert_eq!(registry.entries.len(), 1);
        assert!(Arc::ptr_eq(&replacement, &registry.get((1, 1))));
    }
}
