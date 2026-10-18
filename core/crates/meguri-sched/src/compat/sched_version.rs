//! The engine calls this only at open, after the storage scheduler gate.
use meguri_storage::{ReadRows, StorageError};

pub fn needs_current_scheduler(rows: &impl ReadRows) -> Result<bool, StorageError> {
    Ok(!crate::model::sched_config::SchedConfig::new(rows).sched2021()?)
}
