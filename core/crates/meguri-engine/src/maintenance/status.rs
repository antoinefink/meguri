//! Passive status; never takes the maintenance lane or the writer queue.
use super::*;
use crate::{
    FailedReason,
    device_state::{LastBackup, LastCheckDatabase, LastQuickCheck},
};

/// Composition-root seam for the existing package listing and media journal.
/// Implementations only read recorded data; they must not reenter the engine.
pub trait StatusProvider: Send + Sync {
    fn media_sizes(&self, collection: &Path) -> Result<(u64, u64), EngineError>;
    fn restore_available(&self) -> Result<bool, EngineError>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Remedy {
    Restore,
}
#[derive(Clone, Debug)]
pub struct MaintenanceStatus {
    pub last_backup: Result<Option<LastBackup>, StorageError>,
    pub last_check_database: Result<Option<LastCheckDatabase>, StorageError>,
    pub last_optimize: Result<Option<EpochMillis>, StorageError>,
    pub last_quick_check: Result<Option<LastQuickCheck>, StorageError>,
    pub collection_bytes: Result<u64, StorageError>,
    pub media_bytes: Result<u64, EngineError>,
    pub trash_bytes: Result<u64, EngineError>,
    pub remedies: Result<Vec<Remedy>, EngineError>,
}
fn collection_size(path: &Path) -> Result<u64, StorageError> {
    let main = fs::metadata(path)?.len();
    let mut wal = path.as_os_str().to_owned();
    wal.push("-wal");
    let wal = match fs::metadata(PathBuf::from(wal)) {
        Ok(m) => m.len(),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => 0,
        Err(e) => return Err(e.into()),
    };
    main.checked_add(wal).ok_or_else(|| StorageError::Io {
        detail: "collection size overflow".into(),
    })
}
impl Engine {
    pub fn maintenance_status(&self) -> Result<MaintenanceStatus, EngineError> {
        worker_only()?;
        // Freeze collection ownership/generation while reading. No writer,
        // reader lease, optimize token, cadence initialization or change scan.
        let state = self.lock_state();
        let corrupt = matches!(
            state.state,
            LifecycleState::Failed {
                reason: FailedReason::Corrupt { .. }
            }
        );
        if !corrupt && !matches!(state.state, LifecycleState::Ready { .. }) {
            return Err(EngineError::unavailable(state.state.describe()));
        }
        let inputs = self
            .maintenance()
            .lock()
            .health
            .clone()
            .ok_or_else(|| EngineError::unavailable("no maintenance inputs"))?;
        let records = DeviceState::for_collection(&inputs.path).maintenance_records();
        let sizes = match &inputs.status {
            Some(provider) => provider.media_sizes(&inputs.path),
            None => meguri_storage::media::recorded_sizes(&inputs.path).map_err(Into::into),
        };
        let remedies = if corrupt {
            match &inputs.status {
                Some(provider) => provider
                    .restore_available()
                    .map(|available| if available { vec![Remedy::Restore] } else { vec![] }),
                None => Ok(vec![]),
            }
        } else {
            Ok(vec![])
        };
        Ok(MaintenanceStatus {
            last_backup: records.backup,
            last_check_database: records.check_database,
            last_optimize: records.cadence_start.and(records.optimize),
            last_quick_check: records.quick_check,
            collection_bytes: collection_size(&inputs.path),
            media_bytes: sizes.clone().map(|s| s.0),
            trash_bytes: sizes.map(|s| s.1),
            remedies,
        })
    }
}
