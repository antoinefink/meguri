use std::{
    path::PathBuf,
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use meguri_engine::{
    Collection, DerivedHost, Engine, EngineConfig, EngineError, OpenConfig, Ports,
    jobs::registry::{OriginalHostLiveness, OriginalHostState},
    media::MediaService,
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
    /// V13 discovery owns the existing process/providers without opening or
    /// bootstrapping a collection. A file that appears must use ordinary open.
    pub fn prepare_cloud_connection(&self, config: HostConfig) -> Result<CoreHost, EngineError> {
        match config.collection_directory.join("collection.anki2").symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
            Ok(_) => {
                return Err(EngineError::Busy {
                    running_op: "existing collection requires open".into(),
                });
            }
        }
        self.prepare_host(config, false)
    }

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
        self.prepare_host(config, true)
    }
    fn prepare_host(&self, config: HostConfig, open_existing: bool) -> Result<CoreHost, EngineError> {
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
        let (mut host, paths, options) = CoreHost::compose_on(self, config)?;
        // Active means host-owned lifecycle, including NoCollection/Failed.
        host.set_close_state(CloseState::Active);
        if !open_existing && let Some(runtime) = &host.cloud {
            let live = Arc::new(Mutex::new(true));
            runtime.attach_no_collection_identity_owner(
                Arc::downgrade(&live),
                host.engine.downgrade(),
                host.engine.generation(),
            );
            host.no_collection_identity_owner = Some(live);
            if let Err(error) = runtime.recover_initial_cloud_adoption(&host.engine, &host.initial_open) {
                host.close()?;
                return Err(EngineError::CollectionUnavailable {
                    reason: error.to_string(),
                });
            }
        }
        if open_existing && paths.collection().symlink_metadata().is_ok() {
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
        host.set_close_state(CloseState::Active);
        reset.1 = true;
        Ok(InitialImportCompletion { host, outcome })
    }
}

#[derive(Clone, Copy)]
enum CloseState {
    Prepared,
    Active,
    Closing,
    Closed,
}

impl CloseState {
    fn compact(self) -> OriginalHostState {
        match self {
            Self::Prepared => OriginalHostState::Prepared,
            Self::Active => OriginalHostState::Active,
            Self::Closing => OriginalHostState::Closing,
            Self::Closed => OriginalHostState::Closed,
        }
    }
}

// Restores the canonical state and its same-Host mirror together on error or unwind.
struct RestoreCloseState<'a>(&'a CoreHost, bool);
impl Drop for RestoreCloseState<'_> {
    fn drop(&mut self) {
        let mut state = self.0.closed.lock().unwrap_or_else(PoisonError::into_inner);
        if matches!(*state, CloseState::Closing) {
            self.0.set_close_state_locked(
                &mut state,
                if self.1 {
                    CloseState::Prepared
                } else {
                    CloseState::Active
                },
            );
        }
    }
}

#[path = "cloud_adoption.rs"]
mod cloud_adoption;

pub struct CoreHost {
    pub(crate) provider: Arc<meguri_sync::provider::Authority>,
    pub(crate) cloud_policy: Option<meguri_sync::cloud::policy::Driver>,
    cloud: Option<Arc<meguri_sync::cloud::runtime::Runtime>>,
    pub(crate) manual_transfers: meguri_sync::oneway::ManualTransfers,
    pub(crate) export: crate::collection_export::ExportInputs,
    process: ProcessCore,
    closed: Mutex<CloseState>,
    original_liveness: OriginalHostLiveness,
    no_collection_identity_owner: Option<Arc<Mutex<bool>>>,
    engine: Engine,
    derived: DerivedHost,
    media: Arc<MediaPort>,
    clock: Arc<dyn Clock>,
    backup: Arc<dyn BackupService>,
    backup_inputs: meguri_package::entry::EntryInputs,
    // The same composition-time authority used by ordinary engine open.
    // Metadata publication must not resolve the caller's directory again.
    provider_collection_path: PathBuf,
    initial_open: OpenConfig,
    queues: Arc<meguri_engine::sched::queue::QueueProvider>,
    review: Arc<meguri_engine::session::ReviewService>,
}

impl CoreHost {
    fn set_close_state_locked(&self, state: &mut CloseState, next: CloseState) {
        *state = next;
        self.original_liveness.publish(next.compact());
    }

    fn set_close_state(&self, next: CloseState) {
        let mut state = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
        self.set_close_state_locked(&mut state, next);
    }

    /// Internal Rust authority for a capability already retaining this Host and
    /// its original collection. No current/successor Host is resolved at start.
    pub fn original_resource_authority(
        &self,
        collection: &Collection,
    ) -> meguri_engine::jobs::registry::OriginalResourceAuthority {
        self.engine
            .original_resource_authority(collection, self.original_liveness.clone())
    }

    #[cfg(feature = "test-hooks")]
    pub fn with_close_state_held_for_testing<T>(&self, body: impl FnOnce() -> T) -> T {
        let _state = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
        body()
    }

    /// Only local provider metadata runs under these original-owner gates.
    /// Match close's identity-owner -> close-state order, then try the engine
    /// lane; never wait for lifecycle cleanup while retaining Host ownership.
    /// The body must not reenter Host/engine controls or invoke host callbacks.
    pub(crate) fn publish_provider_metadata<T>(
        &self,
        generation: meguri_engine::Generation,
        publish: impl FnOnce(Option<meguri_engine::device_state::Publication>) -> Result<T, EngineError>,
    ) -> Result<T, EngineError> {
        fn lease<T>(lock: &Mutex<T>) -> Result<std::sync::MutexGuard<'_, T>, EngineError> {
            match lock.try_lock() {
                Ok(guard) => Ok(guard),
                Err(std::sync::TryLockError::Poisoned(error)) => Ok(error.into_inner()),
                Err(std::sync::TryLockError::WouldBlock) => Err(EngineError::Busy {
                    running_op: "provider metadata original host owner".into(),
                }),
            }
        }
        let identity_owner = self.no_collection_identity_owner.as_ref().map(|live| lease(live)).transpose()?;
        let closed = lease(&self.closed)?;
        if !matches!(*closed, CloseState::Active) || identity_owner.as_ref().is_some_and(|live| !**live) {
            return Err(EngineError::CollectionUnavailable { reason: "provider metadata original host closed".into() });
        }
        self.engine.with_provider_metadata_publication(generation, |open_path| {
            let path = &self.provider_collection_path;
            if let Some(open_path) = open_path {
                if open_path != path {
                    return Err(EngineError::CollectionUnavailable { reason: "provider metadata collection owner changed".into() });
                }
            } else {
                // NoCollection is not permission to bootstrap or ignore a file
                // that appeared after preparation. Ordinary open owns it.
                match path.symlink_metadata() {
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
                    Ok(_) => return Err(EngineError::CollectionUnavailable { reason: "existing collection requires open".into() }),
                }
            }
            let publication = meguri_engine::device_state::DeviceState::for_collection(path).settled_publication()?;
            publish(publication)
        })
    }

    pub(crate) fn media_space_provider(&self) -> &dyn meguri_storage::space::SpaceProvider {
        self.backup_inputs.space.as_ref()
    }
    pub(crate) fn provider_random(&self) -> &dyn meguri_model::rng::RandomSource {
        self.backup_inputs.random.as_ref()
    }

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
        host.set_close_state(CloseState::Active);
        host.refresh_provider_admission();
        Ok(host)
    }

    fn compose_on(
        process: &ProcessCore,
        mut config: HostConfig,
    ) -> Result<(Self, CollectionPaths, OpenConfig), EngineError> {
        wiring::validate()?;
        let paths = CollectionPaths::new(&config.collection_directory, &config.backups_directory)?;
        // A failed migration/load is retained as blocked network admission. It
        // must not prevent local collection open or repair.
        let _ = meguri_sync::provider::Authority::migrate(&config.manual_transfer_directory, None);
        let provider = meguri_sync::provider::Authority::load(&config.manual_transfer_directory);
        if !meguri_sync::policy::TransferPolicy::new(&config.manual_transfer_directory)
            .active().is_ok_and(|active| active.is_none()) {
            provider.close_admission();
        }
        if let Some(inputs) = config.cloud.as_mut() {
            inputs.identity = Arc::new(crate::provider::AdmittedIdentity {
                original: inputs.identity.clone(), authority: provider.clone(),
            });
        }
        let engine = process.engine.clone();
        let cloud = config.cloud.clone().map(|inputs| {
            Arc::new(meguri_sync::cloud::runtime::Runtime::new(
                paths.directory.clone(),
                inputs,
            ))
        });
        let derived = DerivedHost::builder(&engine).start()?;
        let media = Arc::new(MediaPort::new(config.clock.clone()));
        let queues = process.queues.clone();
        let review = process.review.clone();
        if let Some(cloud) = &cloud {
            cloud.attach_provider_authority(provider.clone());
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
                IdentityStatus::ReadFailed | IdentityStatus::TemporarilyUnavailable => {
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
        let cloud_policy = cloud
            .as_ref()
            .map(|runtime| meguri_sync::cloud::policy::Driver::new(runtime.clone(), config.clock.clone()));
        let host = Self {
            provider,
            cloud_policy,
            cloud,
            manual_transfers,
            backup_inputs,
            provider_collection_path: paths.collection(),
            initial_open: options.clone(),
            export,
            process: process.clone(),
            closed: Mutex::new(CloseState::Prepared),
            original_liveness: OriginalHostLiveness::default(),
            no_collection_identity_owner: None,
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
        runtime.require_provider_collection_admission()?;
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
        let media = MediaService::open(&self.collection()?, self.clock.clone())?;
        if let Some(runtime) = &self.cloud {
            let consumer: Arc<dyn meguri_engine::media::MediaFeedConsumer> = runtime.clone();
            media.set_feed_consumer(Arc::downgrade(&consumer));
        }
        Ok(media)
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
        // Identity provider calls hold no lease. Only begin/result/apply hold
        // this original Host lease, so close never waits for a platform read.
        let identity_owner = self
            .no_collection_identity_owner
            .as_ref()
            .map(|live| live.lock().unwrap_or_else(PoisonError::into_inner));
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
                    self.set_close_state_locked(&mut closed, CloseState::Closing);
                    false
                }
                CloseState::Prepared => {
                    self.set_close_state_locked(&mut closed, CloseState::Closing);
                    true
                }
            }
        };
        let _restore = RestoreCloseState(self, prepared);
        // Publish Closing under the original identity lease, then release it:
        // an already running initial-adoption reply needs it to finish. The
        // engine closes registry admission and drains before we reacquire.
        drop(identity_owner);
        let identity_owner = std::cell::RefCell::new(None);
        self.review.try_close_host(
            || {
                if !prepared {
                    self.engine.close_with_host_cleanup(|collection| {
                        *identity_owner.borrow_mut() = self
                            .no_collection_identity_owner
                            .as_ref()
                            .map(|live| live.lock().unwrap_or_else(PoisonError::into_inner));
                        // A retained startup row has no live adoption slot. Keep
                        // that recovery evidence while closing the host normally.
                        if self.engine.slot_holder() == Some(meguri_engine::LongOp::CloudAdopt)
                            && let Some(runtime) = &self.cloud
                            && let Some(op) = runtime.adoption_op_for_host_close(&self.engine).map_err(|_| {
                                EngineError::CollectionUnavailable {
                                    reason: "cloud recovery close failed".into(),
                                }
                            })?
                        {
                            runtime
                                .cancel_cloud_adoption_for_host_close(op.id, &self.engine, collection.as_ref())
                                .map_err(|_| EngineError::Busy {
                                    running_op: "cloud adoption".into(),
                                })?;
                        }
                        Ok(())
                    })?;
                } else {
                    *identity_owner.borrow_mut() = self
                        .no_collection_identity_owner
                        .as_ref()
                        .map(|live| live.lock().unwrap_or_else(PoisonError::into_inner));
                }
                self.media
                    .close_until(meguri_engine::monotonic::now() + Duration::from_secs(5))?;
                self.derived.close()?;
                Ok(())
            },
            || {
                if let Some(live) = identity_owner.borrow_mut().as_mut() {
                    **live = false;
                }
                self.set_close_state(CloseState::Closed);
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

#[cfg(test)]
mod resource_liveness_tests {
    use super::*;
    use meguri_engine::jobs::registry::ResourceMode;
    use meguri_model::{
        clock::{StepClock, UtcOffset},
        rng::SeededRandom,
    };
    use std::sync::mpsc;

    fn config_for_prepared(root: &std::path::Path) -> HostConfig {
        let collection = root.join("collection");
        let backups = root.join("backups");
        std::fs::create_dir_all(&collection).unwrap();
        std::fs::create_dir_all(&backups).unwrap();
        std::fs::copy(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("../../../conformance/fixtures/storage/schema-18.anki2"),
            collection.join("collection.anki2"),
        )
        .unwrap();
        HostConfig::new(
            collection,
            backups,
            root.join("widget"),
            Arc::new(StepClock::new(1_894_701_600_000, UtcOffset::UTC, "UTC")),
            Arc::new(SeededRandom::new(37)),
            Arc::new(meguri_storage::space::StatvfsSpace),
        )
    }

    fn sample_locked(host: &CoreHost, collection: &Collection, expected: ResourceMode) {
        let closed = host.closed.lock().unwrap();
        let (sent, result) = mpsc::channel();
        // Hold canonical close state throughout registration and sampling.
        // Unlock before the scoped thread is joined, including on timeout,
        // so this witness can report an unexpected dependency on that lock.
        std::thread::scope(|scope| {
            let start = scope.spawn(|| {
                host.engine.jobs().start_original_resource(
                    "mirror while host locked",
                    host.original_resource_authority(collection),
                    move |_, mut authority| {
                        sent.send(authority.recheck()).unwrap();
                        Ok(())
                    },
                );
            });
            let sampled = result.recv_timeout(Duration::from_secs(5));
            drop(closed);
            start.join().unwrap();
            assert_eq!(sampled.unwrap(), expected);
        });
    }

    #[test]
    fn resource_jobs_no_collection_successor_close_drains_registered_work_and_busy_tail() {
        for busy_tail in [false, true] {
            let first_root = tempfile::tempdir().unwrap();
            let first_config = config_for_prepared(first_root.path());
            let process = ProcessCore::new(first_config.engine.clone()).unwrap();
            let first = process.open(first_config).unwrap();
            first.close().unwrap();
            let next_root = tempfile::tempdir().unwrap();
            let next_config = config_for_prepared(next_root.path());
            std::fs::remove_file(next_config.collection_directory.join("collection.anki2")).unwrap();
            let next = process.prepare_cloud_connection(next_config).unwrap();
            assert!(matches!(
                next.engine.state(),
                meguri_engine::LifecycleState::NoCollection
            ));
            assert!(matches!(*next.closed.lock().unwrap(), CloseState::Active));
            let (entered, ready) = mpsc::channel();
            let (release, wait) = mpsc::channel();
            if busy_tail {
                next.engine.jobs().hold_next_busy_tail_for_testing(entered, wait);
                next.engine
                    .jobs()
                    .start("connection busy tail", next.engine.generation(), None, |_| Ok(()));
            } else {
                next.engine
                    .jobs()
                    .start("ordinary connection work", next.engine.generation(), None, move |_| {
                        entered.send(()).unwrap();
                        let _ = wait.recv_timeout(Duration::from_secs(10));
                        Ok(())
                    });
            }
            ready.recv_timeout(Duration::from_secs(5)).unwrap();
            assert_eq!(next.engine.jobs().running().is_empty(), busy_tail);
            assert!(!next.engine.threads().busy_threads().is_empty());
            std::thread::scope(|scope| {
                let (sent, closed) = mpsc::channel();
                let host = &next;
                let close = scope.spawn(move || {
                    let result = host.close();
                    sent.send(result).unwrap();
                });
                let early = closed.recv_timeout(Duration::from_millis(40));
                let still_open = !matches!(*next.closed.lock().unwrap(), CloseState::Closed);
                // Release the worker before assertions or the scoped join.
                drop(release);
                close.join().unwrap();
                assert!(matches!(early, Err(mpsc::RecvTimeoutError::Timeout)));
                assert!(still_open);
                closed.recv_timeout(Duration::from_secs(5)).unwrap().unwrap();
            });
            assert!(matches!(*next.closed.lock().unwrap(), CloseState::Closed));
        }
    }

    #[test]
    fn resource_jobs_reopen_initializer_keeps_old_collection_sealed_during_open_and_failure() {
        let root = tempfile::tempdir().unwrap();
        let config = config_for_prepared(root.path());
        let path = config.collection_directory.join("collection.anki2");
        let process = ProcessCore::new(config.engine.clone()).unwrap();
        let host = process.open(config).unwrap();
        let original = host.engine.collection().unwrap();
        sample_locked(&host, &original, ResourceMode::Full);
        host.engine.close().unwrap();
        assert_eq!(original.generation(), host.engine.generation());
        assert!(matches!(*host.closed.lock().unwrap(), CloseState::Active));
        sample_locked(&host, &original, ResourceMode::DisposalOnly);
        let (entered, ready) = mpsc::channel();
        let (release, wait) = mpsc::channel::<()>();
        let wait = Arc::new(Mutex::new(wait));
        let mut options = host.initial_open.clone();
        options.initializers.register(move |connection| {
            entered.send(()).unwrap();
            let _ = wait.lock().unwrap().recv_timeout(Duration::from_secs(10));
            connection.execute_batch("SELECT missing_resource_reopen_initializer()")
        });
        std::thread::scope(|scope| {
            let opening = scope.spawn(|| host.engine.open_raw(&path, options));
            // Keep all assertions after release and join, including barrier failure.
            let reached = ready.recv_timeout(Duration::from_secs(5));
            let state = host.engine.state();
            let generation = host.engine.generation();
            let (sent, mode) = mpsc::channel();
            let job = host.engine.jobs().start_original_resource(
                "old resource during initializer",
                host.original_resource_authority(&original),
                move |_, mut authority| {
                    sent.send(authority.recheck()).unwrap();
                    Ok(())
                },
            );
            let sampled = mode.recv_timeout(Duration::from_secs(5));
            drop(release);
            let failed = opening.join().unwrap();
            reached.unwrap();
            assert!(matches!(state, meguri_engine::LifecycleState::Opening));
            assert_eq!(generation, original.generation());
            assert_eq!(sampled.unwrap(), ResourceMode::DisposalOnly);
            assert!(!job.cancel.is_cancelled());
            assert!(matches!(
                failed,
                Err(EngineError::Open(meguri_storage::OpenError::ConnectionInit { .. }))
            ));
        });
        assert!(matches!(
            host.engine.state(),
            meguri_engine::LifecycleState::Failed { .. }
        ));
        assert_eq!(host.engine.generation(), original.generation());
        sample_locked(&host, &original, ResourceMode::DisposalOnly);
        let current = host.engine.open_raw(&path, host.initial_open.clone()).unwrap();
        sample_locked(&host, &original, ResourceMode::DisposalOnly);
        sample_locked(&host, &current, ResourceMode::Full);
        host.close().unwrap();
    }

    #[test]
    fn resource_jobs_host_mirror_covers_prepared_activation_closing_closed_and_failed_close_restore() {
        let root = tempfile::tempdir().unwrap();
        let config = config_for_prepared(root.path());
        let process = ProcessCore::new(config.engine.clone()).unwrap();
        let (host, paths, options) = CoreHost::compose_on(&process, config).unwrap();
        let collection = host.engine.open_raw(&paths.collection(), options).unwrap();
        sample_locked(&host, &collection, ResourceMode::DisposalOnly);
        // Use the same canonical transition helper as all activation paths.
        host.set_close_state(CloseState::Active);
        sample_locked(&host, &collection, ResourceMode::Full);
        // A real close failure restores Active. A handoff slot refuses before
        // engine teardown, so it neither advances generation nor releases stores.
        let slot = host.engine.reserve_collection_export_handoff().unwrap();
        assert!(host.close().is_err());
        sample_locked(&host, &collection, ResourceMode::Full);
        drop(slot);
        host.set_close_state(CloseState::Closing);
        sample_locked(&host, &collection, ResourceMode::DisposalOnly);
        host.set_close_state(CloseState::Active);
        host.close().unwrap();
        sample_locked(&host, &collection, ResourceMode::DisposalOnly);
        // The exact production close guard also restores Prepared on unwind.
        let prepared_root = tempfile::tempdir().unwrap();
        let prepared_config = config_for_prepared(prepared_root.path());
        let prepared_process = ProcessCore::new(prepared_config.engine.clone()).unwrap();
        let (prepared, paths, options) = CoreHost::compose_on(&prepared_process, prepared_config).unwrap();
        let prepared_collection = prepared.engine.open_raw(&paths.collection(), options).unwrap();
        let unwound = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _restore = RestoreCloseState(&prepared, true);
            prepared.set_close_state(CloseState::Closing);
            sample_locked(&prepared, &prepared_collection, ResourceMode::DisposalOnly);
            panic!("prepared close unwind");
        }));
        assert!(unwound.is_err());
        assert!(matches!(*prepared.closed.lock().unwrap(), CloseState::Prepared));
        sample_locked(&prepared, &prepared_collection, ResourceMode::DisposalOnly);
        // This fixture deliberately opened through raw engine while Prepared;
        // restore the ordinary ownership boundary before dropping the Host.
        prepared.set_close_state(CloseState::Active);
        prepared.close().unwrap();
    }
}
