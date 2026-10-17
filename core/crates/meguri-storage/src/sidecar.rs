//! Device-only shutdown state; unknown members survive (F3).
use crate::{
    StorageError,
    atomic_file::{AtomicFile, Purpose},
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
pub struct Sidecar {
    pub clean_shutdown: bool,
    pub last_mod: i64,
    pub last_scm: i64,
    pub upgrade_in_progress: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_json::Value>,
}
impl Sidecar {
    pub fn load(path: &Path) -> Result<Self, StorageError> {
        Ok(Self::load_existing(path)?.unwrap_or_default())
    }

    /// A valid previous record, distinguished from missing or malformed state.
    /// Zero timestamps are real values, never a sentinel for absence.
    pub(crate) fn load_existing(path: &Path) -> Result<Option<Self>, StorageError> {
        match std::fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes).ok()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.into()),
        }
    }
    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        meguri_model::faultpoint!("storage.sidecar.before_save")?;
        let bytes = serde_json::to_vec(self).map_err(|e| StorageError::Io { detail: e.to_string() })?;
        let mut file = AtomicFile::new(path, Purpose::Sidecar)?;
        file.file_mut().write_all(&bytes)?;
        file.publish(|_| Ok(()))
    }
}
