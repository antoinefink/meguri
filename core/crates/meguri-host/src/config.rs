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
    /// Installation-private stable root, shared across endpoints/accounts and
    /// collections. Never choose a disposable/staging directory here.
    pub manual_transfer_directory: PathBuf,
    pub publication_binding: meguri_engine::device_state::AccountBinding,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub disk_space_provider: Arc<dyn DiskSpaceProvider>,
    pub engine: EngineConfig,
    /// Optional bounded capture; platform providers supply real account facts.
    pub cloud: Option<meguri_sync::cloud::runtime::CloudInputs>,
    pub cloud_key_bytes: Option<Arc<dyn meguri_sync::cloud::runtime::KeyByteSource>>,
    pub backup_entry_times: Arc<dyn meguri_engine::maintenance::BackupEntryTimes>,
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
        let manual_transfer_directory = collection_directory
            .parent()
            .unwrap_or(std::path::Path::new("."))
            .join("ManualTransfers");
        Self {
            manual_transfer_directory,
            publication_binding: meguri_engine::device_state::AccountBinding::LocalOnly,
            collection_directory,
            backups_directory,
            app_group_snapshot,
            clock,
            random,
            disk_space_provider,
            engine: EngineConfig::default(),
            cloud: None,
            cloud_key_bytes: None,
            backup_entry_times: Arc::new(meguri_engine::maintenance::OsBackupEntryTimes),
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

impl HostConfig {
    pub fn entry_inputs(&self) -> meguri_package::entry::EntryInputs {
        meguri_package::entry::EntryInputs {
            collection_directory: self.collection_directory.clone(),
            backups_directory: self.backups_directory.clone(),
            clock: self.clock.clone(),
            random: self.random.clone(),
            space: self.disk_space_provider.clone(),
        }
    }
}
