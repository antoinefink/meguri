use std::{sync::Arc, time::Duration};

use meguri_engine::{
    Collection, DerivedHost, Engine, EngineError, OpenConfig, Ports,
    media::MediaService,
    ports::{BackupService, ServicePort},
};
use meguri_model::clock::Clock;
use meguri_storage::paths::CollectionPaths;

use crate::{
    HostConfig,
    wiring::{self, MediaPort},
};

pub struct CoreHost {
    engine: Engine,
    derived: DerivedHost,
    media: Arc<MediaPort>,
    clock: Arc<dyn Clock>,
    backup: Arc<dyn BackupService>,
}

impl CoreHost {
    pub fn open(config: HostConfig) -> Result<Self, EngineError> {
        wiring::validate()?;
        let paths = CollectionPaths::new(&config.collection_directory, &config.backups_directory)?;
        let engine = Engine::new(config.engine)?;
        let derived = DerivedHost::builder(&engine).start()?;
        let media = Arc::new(MediaPort::new(config.clock.clone()));
        let ports = derived
            .ports(Ports::new())
            .open_step(media.clone())
            .suspend_participant(media.clone());
        wiring::validate_ports(&ports)?;
        let options = OpenConfig::new(config.clock.clone(), config.random)
            .ports(ports)
            .backups_directory(paths.backups.clone())
            .disk_space_provider(config.disk_space_provider);
        if let Err(error) = engine.open_raw(&paths.collection(), options) {
            let cleanup: Vec<_> = [
                engine.close(),
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
        Ok(Self {
            engine,
            derived,
            media,
            clock: config.clock,
            backup,
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
    pub fn backup(&self) -> Arc<dyn BackupService> {
        self.backup.clone()
    }

    pub fn close(&self) -> Result<(), EngineError> {
        self.engine.close()?;
        self.media
            .close_until(meguri_engine::monotonic::now() + Duration::from_secs(5))?;
        self.derived.close()
    }
}

impl Drop for CoreHost {
    fn drop(&mut self) {
        if let Err(error) = self.close() {
            tracing::warn!(%error, "closing the core host");
        }
    }
}
