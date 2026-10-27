//! Local backup/recovery composition for package integration tests. Opens no
//! endpoint, server, transport or acceptance harness.
use meguri_model::{clock::Clock, rng::SeededRandom};
use meguri_storage::space::SpaceProvider;
use std::{path::Path, sync::Arc};
pub fn open_restore_host(
    path: &Path,
    backups: &Path,
    clock: Arc<dyn Clock>,
    space: Arc<dyn SpaceProvider>,
) -> Result<meguri_host::CoreHost, meguri_engine::EngineError> {
    let directory = path.parent().expect("isolated collection directory");
    let config = meguri_host::HostConfig::new(
        directory.into(),
        backups.into(),
        directory.join("snapshot"),
        clock,
        Arc::new(SeededRandom::new(127)),
        space,
    );
    meguri_host::ProcessCore::new(Default::default())?.open_for_manual_import(config)
}
