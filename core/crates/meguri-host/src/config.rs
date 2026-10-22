use std::{path::PathBuf, sync::Arc};

use meguri_engine::{DiskSpaceProvider, EngineConfig};
use meguri_model::{clock::Clock, rng::RandomSource};

/// Platform inputs only: callers cannot register their own engine ports.
#[derive(Clone)]
pub struct HostConfig {
    pub collection_directory: PathBuf,
    /// May be missing; the forced writer preflights its existing ancestor before creation.
    pub backups_directory: PathBuf,
    pub app_group_snapshot: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub disk_space_provider: Arc<dyn DiskSpaceProvider>,
    pub engine: EngineConfig,
    #[cfg(feature = "test-hooks")]
    pub(crate) test_backup: Option<Arc<dyn meguri_engine::ports::BackupService>>,
}

impl HostConfig {
    pub fn new(
        collection_directory: PathBuf,
        backups_directory: PathBuf,
        app_group_snapshot: PathBuf,
        clock: Arc<dyn Clock>,
        random: Arc<dyn RandomSource>,
        disk_space_provider: Arc<dyn DiskSpaceProvider>,
    ) -> Self {
        Self {
            collection_directory,
            backups_directory,
            app_group_snapshot,
            clock,
            random,
            disk_space_provider,
            engine: EngineConfig::default(),
            #[cfg(feature = "test-hooks")]
            test_backup: None,
        }
    }

    /// A harness witness is never counted as a production provider at a gate.
    #[cfg(feature = "test-hooks")]
    pub fn with_backup_for_testing(mut self, backup: Arc<dyn meguri_engine::ports::BackupService>) -> Self {
        self.test_backup = Some(backup);
        self
    }
}
