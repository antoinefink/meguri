use std::{path::PathBuf, sync::Arc};

use meguri_engine::{DiskSpaceProvider, EngineConfig};
use meguri_model::{clock::Clock, rng::RandomSource};

/// Platform inputs only: callers cannot register their own engine ports.
#[derive(Clone)]
pub struct HostConfig {
    pub collection_directory: PathBuf,
    pub backups_directory: PathBuf,
    pub app_group_snapshot: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub disk_space_provider: Arc<dyn DiskSpaceProvider>,
    pub engine: EngineConfig,
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
        }
    }
}
