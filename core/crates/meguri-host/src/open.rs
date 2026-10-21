use std::{
    sync::{Arc, Mutex, PoisonError},
    time::Duration,
};

use meguri_engine::{
    Collection, DerivedHost, Engine, EngineConfig, EngineError, OpenConfig, Ports,
    media::MediaService,
    ports::{BackupService, ServicePort},
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

enum CloseState {
    Active,
    Closing,
    Closed,
}

pub struct CoreHost {
    process: ProcessCore,
    closed: Mutex<CloseState>,
    engine: Engine,
    derived: DerivedHost,
    media: Arc<MediaPort>,
    clock: Arc<dyn Clock>,
    backup: Arc<dyn BackupService>,
    queues: Arc<meguri_engine::sched::queue::QueueProvider>,
    review: Arc<meguri_engine::session::ReviewService>,
}

impl CoreHost {
    pub fn open(config: HostConfig) -> Result<Self, EngineError> {
        ProcessCore::new(config.engine.clone())?.open(config)
    }

    fn open_on(process: &ProcessCore, config: HostConfig) -> Result<Self, EngineError> {
        wiring::validate()?;
        let paths = CollectionPaths::new(&config.collection_directory, &config.backups_directory)?;
        let engine = process.engine.clone();
        let derived = DerivedHost::builder(&engine).start()?;
        let media = Arc::new(MediaPort::new(config.clock.clone()));
        let queues = process.queues.clone();
        let review = process.review.clone();
        let ports = derived
            .ports(Ports::new())
            .open_step(media.clone())
            .suspend_participant(media.clone())
            .observer(queues.clone())
            .generation_listener(queues.clone())
            .open_step(review.clone())
            .observer(review.clone())
            .generation_listener(review.clone())
            .suspend_participant(review.clone());
        wiring::validate_ports(&ports)?;
        let options = OpenConfig::new(config.clock.clone(), config.random)
            .ports(ports)
            .backups_directory(paths.backups.clone())
            .disk_space_provider(config.disk_space_provider);
        if let Err(error) = engine.open_raw(&paths.collection(), options) {
            let cleanup: Vec<_> = [
                // A failed raw open owns no live collection and already drains
                // its writer. Calling close here would wait for the open job
                // that is currently performing this very cleanup.
                if engine.collection().is_ok() {
                    engine.close()
                } else {
                    Ok(())
                },
                media.close_until(meguri_engine::monotonic::now() + Duration::from_secs(5)),
                derived.close(),
            ]
            .into_iter()
            .filter_map(Result::err)
            .collect();
            if !cleanup.is_empty() {
                return Err(EngineError::Internal {
                    message: format!("open failed: {error}; provider cleanup failed: {cleanup:?}"),
                });
            }
            return Err(error);
        }
        let backup = wiring::pending::PENDING
            .iter()
            .find(|(port, _)| *port == ServicePort::Backup)
            .map(|(_, unavailable)| Arc::new(*unavailable) as Arc<dyn BackupService>)
            .ok_or_else(|| EngineError::Internal {
                message: "backup provider is not wired".into(),
            })?;
        #[cfg(feature = "test-hooks")]
        let backup = config.test_backup.unwrap_or(backup);
        Ok(Self {
            process: process.clone(),
            closed: Mutex::new(CloseState::Active),
            engine,
            derived,
            media,
            clock: config.clock,
            backup,
            queues,
            review,
        })
    }

    pub fn engine(&self) -> &Engine {
        &self.engine
    }
    pub fn collection(&self) -> Result<Collection, EngineError> {
        self.engine.collection()
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

    pub fn backup(&self) -> Arc<dyn BackupService> {
        self.backup.clone()
    }

    pub fn close(&self) -> Result<(), EngineError> {
        {
            let mut closed = self.closed.lock().unwrap_or_else(PoisonError::into_inner);
            match *closed {
                CloseState::Closed => return Ok(()),
                CloseState::Closing => {
                    return Err(EngineError::Busy {
                        running_op: "collection close".into(),
                    });
                }
                CloseState::Active => *closed = CloseState::Closing,
            }
        }
        struct Restore<'a>(&'a Mutex<CloseState>);
        impl Drop for Restore<'_> {
            fn drop(&mut self) {
                let mut state = self.0.lock().unwrap_or_else(PoisonError::into_inner);
                if matches!(*state, CloseState::Closing) {
                    *state = CloseState::Active;
                }
            }
        }
        let _restore = Restore(&self.closed);
        if self.engine.collection().is_ok()
            && let Some(id) = self.review.active_session()
        {
            self.review.end(id)?;
        }
        self.engine.close()?;
        self.media
            .close_until(meguri_engine::monotonic::now() + Duration::from_secs(5))?;
        self.derived.close()?;
        *self.closed.lock().unwrap_or_else(PoisonError::into_inner) = CloseState::Closed;
        *self.process.opened.lock().unwrap_or_else(PoisonError::into_inner) = false;
        Ok(())
    }
}

impl Drop for CoreHost {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(%error, "closing the core host");
        }
    }
}
