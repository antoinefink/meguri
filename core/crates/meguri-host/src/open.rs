use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use meguri_engine::{
    Collection, DerivedHost, Engine, EngineConfig, EngineError, OpenConfig, Ports, media::MediaService,
    ports::BackupService,
};
use meguri_model::clock::Clock;
use meguri_storage::paths::CollectionPaths;

use crate::{
    HostConfig,
    wiring::{self, MediaPort},
};

/// Process-owned providers. Engine configuration is fixed at construction;
/// opening supplies only the ordinary HostConfig platform inputs.
#[derive(Clone)]
pub struct ProcessCore {
    engine: Engine,
    queues: Arc<meguri_engine::sched::queue::QueueProvider>,
    review: Arc<meguri_engine::session::ReviewService>,
    job_workers: usize,
    opened: Arc<Mutex<bool>>,
}

impl ProcessCore {
    pub fn new(config: EngineConfig) -> Result<Self, EngineError> {
        let job_workers = config.job_workers;
        let engine = Engine::new(config)?;
        let queues = Arc::new(meguri_engine::sched::queue::QueueProvider::default());
        let review = meguri_engine::session::ReviewService::new(&engine, queues.clone())?;
        Ok(Self {
            engine,
            queues,
            review,
            job_workers,
            opened: Arc::new(Mutex::new(false)),
        })
    }
    /// Explicit recovery/import entry. Returns a live provider host even if
    /// opening the existing file failed; callers show engine.state(). No request
    /// is made here. Initial activation reuses these providers exactly once.
    pub fn open_for_manual_import(&self, config: HostConfig) -> Result<CoreHost, EngineError> {
        {
            let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
            if *opened || config.engine.job_workers != self.job_workers {
                return Err(EngineError::Busy {
                    running_op: "process collection".into(),
                });
            }
            *opened = true;
        }
        struct Reset<'a>(&'a ProcessCore, bool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                if !self.1 {
                    *self.0.opened.lock().unwrap_or_else(PoisonError::into_inner) = false;
                }
            }
        }
        let mut reset = Reset(self, false);
        let (host, paths, options) = CoreHost::compose_on(self, config)?;
        // Active means host-owned lifecycle, including NoCollection/Failed.
        *host.closed.lock().unwrap_or_else(PoisonError::into_inner) = CloseState::Active;
        if paths.collection().symlink_metadata().is_ok() {
            let _ = host.engine.open_raw(&paths.collection(), options);
        }
        reset.1 = true;
        Ok(host)
    }
    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn review(&self) -> &meguri_engine::session::ReviewService {
        &self.review
    }
    pub fn open(&self, config: HostConfig) -> Result<CoreHost, EngineError> {
        let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
        if *opened {
            return Err(EngineError::CollectionUnavailable {
                reason: "process collection is already open".into(),
            });
        }
        if config.engine.job_workers != self.job_workers {
            return Err(EngineError::InvalidArgument {
                message: "process engine configuration is fixed at construction".into(),
            });
        }
        *opened = true;
        drop(opened); // Host inputs can call Swift; never keep the ownership lock.
        struct Reservation<'a> {
            process: &'a ProcessCore,
            committed: bool,
        }
        impl Drop for Reservation<'_> {
            fn drop(&mut self) {
                if !self.committed {
                    *self.process.opened.lock().unwrap_or_else(PoisonError::into_inner) = false;
                }
            }
        }
        let mut reservation = Reservation {
            process: self,
            committed: false,
        };
        let host = CoreHost::open_on(self, config)?;
        reservation.committed = true;
        Ok(host)
    }
}

/// Preactivation failure returns Err. After activation, providers and ownership
/// travel with the typed outcome even when reopening/recovery reported failure.
pub struct InitialImportCompletion {
    pub host: CoreHost,
    pub outcome: Result<meguri_package::entry::Activation, meguri_package::entry::EntryError>,
}
impl ProcessCore {
    pub fn import_initial(
        &self,
        config: HostConfig,
        inspection: meguri_package::entry::EntryInspection,
        slot: &meguri_engine::SlotGuard,
        cancel: &Arc<meguri_engine::CancelToken>,
        progress: &meguri_engine::ProgressSink,
    ) -> Result<InitialImportCompletion, meguri_package::entry::EntryError> {
        use meguri_package::entry::EntryError;
        // Reserve without retaining a mutex across provider or platform work.
        {
            let mut opened = self.opened.lock().unwrap_or_else(PoisonError::into_inner);
            if *opened {
                return Err(EngineError::Busy {
                    running_op: "collection open".into(),
                }
                .into());
            }
            if config.engine.job_workers != self.job_workers {
                return Err(EngineError::InvalidArgument {
                    message: "process engine configuration is fixed at construction".into(),
                }
                .into());
            }
            *opened = true;
        }
        struct Reset<'a>(&'a ProcessCore, bool);
        impl Drop for Reset<'_> {
            fn drop(&mut self) {
                if !self.1 {
                    *self.0.opened.lock().unwrap_or_else(PoisonError::into_inner) = false;
                }
            }
        }
        let mut reset = Reset(self, false);
        let paths = CollectionPaths::new(&config.collection_directory, &config.backups_directory)?;
        if paths.collection() != inspection.facts().destination {
            return Err(EntryError::StaleConfirmation);
        }
        let (host, _, options) = CoreHost::compose_on(self, config)?;
        let outcome = inspection.activate_initial(slot, options, cancel, progress);
        let activated = outcome.is_ok() || outcome.as_ref().is_err_and(|e| e.candidate_active());
        if !activated {
            // Prepared host cleanup never closes/waits for its own import job.
            host.close()?;
            return Err(outcome.expect_err("preactivation failure"));
        }
        *host.closed.lock().unwrap_or_else(PoisonError::into_inner) = CloseState::Active;
        reset.1 = true;
        Ok(InitialImportCompletion { host, outcome })
    }
}

enum CloseState {
    Prepared,
    Active,
    Closing,
    Closed,
}

#[path = "cloud_adoption.rs"]
mod cloud_adoption;

pub struct CoreHost {
    cloud: Option<Arc<meguri_sync::cloud::runtime::Runtime>>,
    pub(crate) manual_transfers: meguri_sync::oneway::ManualTransfers,
    pub(crate) export: crate::collection_export::ExportInputs,
    process: ProcessCore,
    closed: Mutex<CloseState>,
    engine: Engine,
    derived: DerivedHost,
    media: Arc<MediaPort>,
    clock: Arc<dyn Clock>,
    backup: Arc<dyn BackupService>,
    backup_inputs: meguri_package::entry::EntryInputs,
    initial_open: OpenConfig,
    queues: Arc<meguri_engine::sched::queue::QueueProvider>,
    review: Arc<meguri_engine::session::ReviewService>,
}

impl CoreHost {
    pub fn open(config: HostConfig) -> Result<Self, EngineError> {
        ProcessCore::new(config.engine.clone())?.open(config)
    }

    fn open_on(process: &ProcessCore, config: HostConfig) -> Result<Self, EngineError> {
        let (host, paths, options) = Self::compose_on(process, config)?;
        let result = host.engine.open_raw(&paths.collection(), options);
        if let Err(error) = result {
            if let Err(cleanup) = host.close() {
                return Err(EngineError::Internal {
                    message: format!("open failed: {error}; provider cleanup failed: {cleanup}"),
                });
            }
            return Err(error);
        }
        *host.closed.lock().unwrap_or_else(PoisonError::into_inner) = CloseState::Active;
        Ok(host)
    }

    fn compose_on(
        process: &ProcessCore,
        config: HostConfig,
    ) -> Result<(Self, CollectionPaths, OpenConfig), EngineError> {
        wiring::validate()?;
        let paths = CollectionPaths::new(&config.collection_directory, &config.backups_directory)?;
        let engine = process.engine.clone();
        let cloud = config.cloud.clone().map(|inputs| {
            Arc::new(meguri_sync::cloud::runtime::Runtime::new(
                config.collection_directory.clone(),
                inputs,
            ))
        });
        let derived = DerivedHost::builder(&engine).start()?;
        let media = Arc::new(MediaPort::new(config.clock.clone()));
        let queues = process.queues.clone();
        let review = process.review.clone();
        if let Some(cloud) = &cloud {
            cloud.attach_sources(config.random.clone(), Arc::downgrade(&review));
            cloud.attach_adapter_clock(config.clock.clone());
            if let Some(keys) = &config.cloud_key_bytes {
                cloud.attach_key_bytes(keys.clone());
            }
        }
        let mut ports = derived
            .ports(Ports::new())
            .open_step(media.clone())
            .suspend_participant(media.clone())
            .observer(queues.clone())
            .generation_listener(queues.clone())
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone());
        if let Some(cloud) = &cloud {
            ports = ports
                .cloud_publication_authority(cloud.clone())
                .open_step(cloud.clone())
                .observer(cloud.clone())
                .generation_listener(cloud.clone())
                .suspend_participant(cloud.clone());
        }
        if let Err(error) = wiring::validate_ports(&ports) {
            let media_cleanup = media.close_until(meguri_engine::monotonic::now() + Duration::from_secs(5));
            let derived_cleanup = derived.close();
            if media_cleanup.is_err() || derived_cleanup.is_err() {
                return Err(EngineError::Internal {
                    message: format!(
                        "port validation failed: {error}; cleanup: {media_cleanup:?}, {derived_cleanup:?}"
                    ),
                });
            }
            return Err(error);
        }
        let backup: Arc<dyn BackupService> = Arc::new(meguri_package::ForcedBackupProvider::new(
            engine.clone(),
            paths.collection(),
            paths.backups.clone(),
            config.clock.clone(),
            config.random.clone(),
            config.disk_space_provider.clone(),
        ));
        let export = crate::collection_export::ExportInputs::new(&config);
        let backup_inputs = config.entry_inputs();
        #[cfg(feature = "test-hooks")]
        let backup = config.test_backup.unwrap_or(backup);
        let mut options = OpenConfig::new(config.clock.clone(), config.random.clone())
            .ports(ports.backup_service(backup.clone()))
            .backups_directory(paths.backups.clone())
            .disk_space_provider(config.disk_space_provider.clone());
        options.publication_binding = config.publication_binding.clone();
        // The platform provider is the sole current-account authority. Call it
        // before engine ownership/recovery and outside Runtime/tracking locks.
        let mut startup_identity_read_failed = false;
        options.startup_publication_binding = config.cloud.as_ref().map(|inputs| {
            use meguri_engine::device_state::AccountBinding;
            use meguri_sync::cloud::runtime::IdentityStatus;
            match inputs.identity.read_identity(&inputs.scope) {
                IdentityStatus::Available(account) => match String::from_utf8(account.0) {
                    Ok(account) if !account.is_empty() => AccountBinding::Connected {
                        account,
                        configuration: inputs.scope.container.as_str().to_owned(),
                        environment: match inputs.scope.environment {
                            meguri_sync::cloud::Environment::Development => "development",
                            meguri_sync::cloud::Environment::Production => "production",
                        }
                        .into(),
                    },
                    _ => AccountBinding::Unknown,
                },
                IdentityStatus::ReadFailed => {
                    startup_identity_read_failed = true;
                    AccountBinding::Unknown
                }
                _ => AccountBinding::Unknown,
            }
        });
        options.startup_identity_read_failed = startup_identity_read_failed;
        options.backup_entry_times = config.backup_entry_times.clone();
        options.maintenance_status = Some(Arc::new(MaintenanceStatusSource {
            backups: paths.backups.clone(),
        }));
        let manual_transfers = meguri_sync::oneway::ManualTransfers::new(meguri_sync::oneway::TransferInputs {
            engine: engine.clone(),
            collection_directory: config.collection_directory.clone(),
            backups_directory: config.backups_directory.clone(),
            state_directory: config.manual_transfer_directory.clone(),
            clock: config.clock.clone(),
            random: config.random.clone(),
            space: config.disk_space_provider.clone(),
            backup: backup.clone(),
            initial_open: options.clone(),
        });
        let host = Self {
            cloud,
            manual_transfers,
            backup_inputs,
            initial_open: options.clone(),
            export,
            process: process.clone(),
            closed: Mutex::new(CloseState::Prepared),
            engine,
            derived,
            media,
            clock: config.clock,
            backup,
            queues,
            review,
        };
        Ok((host, paths, options))
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn collection(&self) -> Result<Collection, EngineError> {
        self.engine.collection()
    }
    /// Sync remains unavailable until whole-binding reconciliation and the
    /// guarded root/send/ack integration land.
    #[cfg(feature = "test-hooks")]
    pub fn cloud_fixture_sources(&self) -> (Arc<dyn Clock>, Arc<dyn meguri_model::rng::RandomSource>) {
        (self.clock.clone(), self.initial_open.random.clone())
    }

    pub fn cloud_runtime(&self) -> Option<&Arc<meguri_sync::cloud::runtime::Runtime>> {
        self.cloud.as_ref()
    }

    /// Ordinary incoming transport and native callers share the production
    /// Runtime's durable queue and engine operation path.
    pub fn receive_cloud_collection_records(
        &self,
        instance: &str,
        records: &[meguri_sync::cloud::workflow::Saved],
        timeout: Duration,
    ) -> Result<usize, meguri_sync::cloud::runtime::RuntimeError> {
        let runtime = self
            .cloud
            .as_ref()
            .ok_or(meguri_sync::cloud::runtime::RuntimeError::IdentityUnverified)?;
        runtime.receive_collection_records(instance, records)?;
        runtime.drain_incoming(&self.collection()?, timeout)
    }

    pub fn apply_cloud_incoming(&self, timeout: Duration) -> Result<usize, meguri_sync::cloud::runtime::RuntimeError> {
        let runtime = self
            .cloud
            .as_ref()
            .ok_or(meguri_sync::cloud::runtime::RuntimeError::IdentityUnverified)?;
        runtime.drain_incoming(&self.collection()?, timeout)
    }
    /// Reach one bounded native apply, moving past parked work without receiving again.
    pub fn apply_native_cloud_incoming(
        &self,
        sdk: &str,
        timeout: Duration,
    ) -> Result<meguri_sync::cloud::incoming::IncomingDisposition, meguri_sync::cloud::runtime::RuntimeError> {
        let runtime = self
            .cloud
            .as_ref()
            .ok_or(meguri_sync::cloud::runtime::RuntimeError::IdentityUnverified)?;
        runtime.drain_native_incoming(&self.collection()?, sdk, timeout)
    }
    pub fn derived(&self) -> &DerivedHost {
        &self.derived
    }
    pub fn media(&self) -> Result<MediaService, EngineError> {
        MediaService::open(&self.collection()?, self.clock.clone())
    }
    pub fn queues(&self) -> &meguri_engine::sched::queue::QueueProvider {
        &self.queues
    }

    pub fn review(&self) -> &meguri_engine::session::ReviewService {
        &self.review
    }

    pub fn restore_backup(
        &self,
        name: &str,
        cancel: &Arc<meguri_engine::CancelToken>,
        progress: &meguri_engine::ProgressSink,
    ) -> Result<meguri_package::restore::RestoredBackup, meguri_package::restore::RestoreError> {
        meguri_package::restore::restore_backup(
            &self.engine,
            &self.backup_inputs,
            self.initial_open.clone(),
            self.backup.as_ref(),
            name,
            cancel,
            progress,
        )
    }

    pub fn backup(&self) -> Arc<dyn BackupService> {
        self.backup.clone()
    }

    pub fn close(&self) -> Result<(), EngineError> {
        let prepared = {
            let mut closed = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
            match *closed {
                CloseState::Closed => return Ok(()),
                CloseState::Closing => {
                    return Err(EngineError::Busy {
                        running_op: "collection close".into(),
                    });
                }
                CloseState::Active => {
                    *closed = CloseState::Closing;
                    false
                }
                CloseState::Prepared => {
                    *closed = CloseState::Closing;
                    true
                }
            }
        };
        struct Restore<'a>(&'a Mutex<CloseState>, bool);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(*state, CloseState::Closing) {
                    *state = if self.1 {
                        CloseState::Prepared
                    } else {
                        CloseState::Active
                    };
                }
            }
        }
        let _restore = Restore(&self.closed, prepared);
        self.review.try_close_host(
            || {
                if !prepared {
                    // A retained startup row has no live adoption slot. Keep
                    // that recovery evidence while closing the host normally.
                    if self.engine.slot_holder() == Some(meguri_engine::LongOp::CloudAdopt)
                        && let Some(runtime) = &self.cloud
                        && let Some(op) = runtime.adoption_op().map_err(|_| EngineError::CollectionUnavailable {
                            reason: "cloud recovery close failed".into(),
                        })?
                    {
                        runtime
                            .cancel_cloud_adoption(op.id, &self.collection()?)
                            .map_err(|_| EngineError::Busy {
                                running_op: "cloud adoption".into(),
                            })?;
                    }
                    self.engine.close()?;
                }
                self.media
                    .close_until(meguri_engine::monotonic::now() + Duration::from_secs(5))?;
                self.derived.close()?;
                Ok(())
            },
            || {
                *self.closed.lock().unwrap_or_else(PoisonError::into_inner) = CloseState::Closed;
                *self.process.opened.lock().unwrap_or_else(PoisonError::into_inner) = false;
            },
        )
    }
}

impl Drop for CoreHost {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(%error, "closing the core host");
        }
    }
}

/// Read-only adapters; reuse B7's typed listing without changing backup policy.
struct MaintenanceStatusSource {
    backups: std::path::PathBuf,
}
impl meguri_engine::maintenance::status::StatusProvider for MaintenanceStatusSource {
    fn media_sizes(&self, collection: &std::path::Path) -> Result<(u64, u64), EngineError> {
        meguri_storage::media::recorded_sizes(collection).map_err(Into::into)
    }
    fn restore_available(&self) -> Result<bool, EngineError> {
        use meguri_package::backup::{listing, naming::BackupKind};
        Ok(listing::list(&self.backups)
            .map_err(meguri_storage::StorageError::from)?
            .iter()
            .any(|entry| matches!(entry.kind, BackupKind::Backup | BackupKind::ServerCopy)))
    }
}
