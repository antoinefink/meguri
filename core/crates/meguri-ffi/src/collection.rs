//! U37 platform inputs and the short open starter. Operations stay in CoreHost.
use crate::{
    callback,
    error::CoreError,
    jobs::{self, JobHandle},
    runtime,
};
use meguri_host::HostConfig;
use meguri_model::{
    clock::{Clock, UtcOffset},
    rng::SeededRandom,
};
use meguri_storage::StorageError;
use std::{path::Path, sync::Arc};

#[derive(Clone, Debug, uniffi::Record)]
pub struct CollectionLocation {
    pub collection_directory: String,
    pub backups_directory: String,
    pub app_group_snapshot: String,
    pub manual_transfer_directory: String,
}

/// Trusted bounded synchronous platform values only; no core reentry, UI or async
/// work. These reads alone may run under core locks (E-20261021T142014Z-9c3d).
#[uniffi::export(foreign)]
pub trait HostClock: Send + Sync {
    fn now_ms(&self) -> i64;
    fn offset_from_utc(&self, instant_ms: i64) -> i32;
    fn zone_id(&self) -> String;
}

pub(crate) struct ForeignClock(pub(crate) Arc<dyn HostClock>);
impl std::fmt::Debug for ForeignClock {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ForeignClock")
    }
}
impl Clock for ForeignClock {
    fn now_ms(&self) -> i64 {
        callback::invoke("HostClock.now_ms", || self.0.now_ms())
    }
    fn utc_offset_at(&self, at_ms: i64) -> UtcOffset {
        let seconds = callback::invoke("HostClock.offset_from_utc", || self.0.offset_from_utc(at_ms));
        UtcOffset::from_seconds(seconds).expect("host clock offset must be less than one day")
    }
    fn zone_id(&self) -> String {
        callback::invoke("HostClock.zone_id", || self.0.zone_id())
    }
}

/// Synchronous: absence and I/O failure must never look like unlimited capacity.
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum DiskCapacity {
    Available { bytes: u64 },
    Failed { detail: String },
}
/// Same bounded value-read exception as HostClock; no delivery callback is exempt.
#[uniffi::export(foreign)]
pub trait DiskSpaceProvider: Send + Sync {
    fn available_bytes(&self, directory: String) -> DiskCapacity;
}
pub(crate) struct ForeignSpace(pub(crate) Arc<dyn DiskSpaceProvider>);
impl meguri_engine::DiskSpaceProvider for ForeignSpace {
    fn available_bytes(&self, directory: &Path) -> Result<u64, StorageError> {
        let directory = directory.to_str().ok_or_else(|| StorageError::Io {
            detail: "non-UTF-8 capacity path".into(),
        })?;
        match callback::invoke("DiskSpaceProvider.available_bytes", || {
            self.0.available_bytes(directory.to_owned())
        }) {
            DiskCapacity::Available { bytes } => Ok(bytes),
            DiskCapacity::Failed { detail } => Err(StorageError::Io { detail }),
        }
    }
}

#[uniffi::export]
pub fn start_open(
    location: CollectionLocation,
    clock: Arc<dyn HostClock>,
    disk_space: Arc<dyn DiskSpaceProvider>,
    random_seed: u64,
) -> Result<JobHandle, CoreError> {
    callback::enter("start_open");
    let reservation = runtime::reserve_open()?;
    let manual_transfer_directory = location.manual_transfer_directory.clone();
    let mut config = HostConfig::new(
        location.collection_directory.into(),
        location.backups_directory.into(),
        location.app_group_snapshot.into(),
        Arc::new(ForeignClock(clock)),
        Arc::new(SeededRandom::new(random_seed)),
        Arc::new(ForeignSpace(disk_space)),
    );
    config.manual_transfer_directory = manual_transfer_directory.into();
    config.cloud = crate::sync::platform_inputs();
    jobs::start("open collection", runtime::engine()?.generation().0, move |job| {
        job.cancel.check()?;
        reservation.open(config)?;
        // The completed open stays owned by runtime. Report a late cancellation
        // as failure so the caller can explicitly close that collection.
        if job.cancel.is_cancelled()? {
            return Err(CoreError::Unavailable {
                reason: "open completed after cancellation; collection remains open; close it explicitly".into(),
            });
        }
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use meguri_engine::EngineError;
    use meguri_model::clock::FixedClock;
    use std::sync::Mutex;

    struct Capacity {
        answer: Mutex<DiskCapacity>,
        paths: Mutex<Vec<String>>,
    }
    impl DiskSpaceProvider for Capacity {
        fn available_bytes(&self, directory: String) -> DiskCapacity {
            // A synchronous reentrant export must be rejected by invoke.
            #[cfg(debug_assertions)]
            assert!(std::panic::catch_unwind(crate::core_version).is_err());
            self.paths.lock().unwrap().push(directory);
            self.answer.lock().unwrap().clone()
        }
    }
    #[test]
    fn actual_host_capacity_callback_refuses_low_space_and_propagates_failure() {
        let dir = tempfile::tempdir().unwrap();
        let collection = dir.path().join("collection");
        let backups = dir.path().join("backups");
        std::fs::create_dir_all(&collection).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        let clock = Arc::new(FixedClock::new(1_894_701_600_000, UtcOffset::UTC));
        meguri_engine::bootstrap::create(
            &collection.join("collection.anki2"),
            clock.as_ref(),
            &SeededRandom::new(37),
        )
        .unwrap();
        let capacity = Arc::new(Capacity {
            answer: Mutex::new(DiskCapacity::Available { bytes: 41 }),
            paths: Mutex::new(vec![]),
        });
        let host = meguri_host::CoreHost::open(HostConfig::new(
            collection.clone(),
            backups,
            dir.path().join("widget"),
            clock,
            Arc::new(SeededRandom::new(37)),
            Arc::new(ForeignSpace(capacity.clone())),
        ))
        .unwrap();
        let c = host.collection().unwrap();
        assert_eq!(
            c.require_space(42),
            Err(EngineError::Storage(StorageError::InsufficientSpace {
                required: 42,
                available: 41
            }))
        );
        assert_eq!(
            c.require_space(42).map_err(CoreError::from),
            Err(CoreError::InsufficientSpace {
                required: 42,
                available: 41
            })
        );
        assert!(c.require_space(41).is_ok());
        *capacity.answer.lock().unwrap() = DiskCapacity::Failed {
            detail: "capacity unavailable".into(),
        };
        assert_eq!(
            c.require_space(0),
            Err(EngineError::Storage(StorageError::Io {
                detail: "capacity unavailable".into()
            }))
        );
        assert_eq!(*capacity.paths.lock().unwrap(), vec![collection.to_str().unwrap(); 4]);
        host.close().unwrap();
    }
}
