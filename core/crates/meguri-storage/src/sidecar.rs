//! Device-only shutdown state; all mutations share a strict durable RMW (F3/R6).
use crate::{
    StorageError,
    atomic_file::{AtomicFile, Purpose},
};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::{collections::BTreeMap, io::Write, path::Path, sync::Mutex};

// Engine collection ownership excludes other processes; this lock also covers
// independent metadata consumers within that owner. Never nest an accessor.
static LOCK: Mutex<()> = Mutex::new(());

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
    let _lock = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let document = read(path)?;
    f(document.as_ref())
}

/// The shared read/modify/durable-write seam. Retains members the caller does
/// not edit; an unreadable existing document is never replaced by defaults.
pub fn update_document<T>(
    path: &Path,
    f: impl FnOnce(&mut Map<String, Value>) -> Result<T, StorageError>,
) -> Result<T, StorageError> {
    let _lock = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
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
