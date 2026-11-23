// SPDX-License-Identifier: MIT
//! Test-only synchronization. Entire module is absent with default features.
use crate::{Error, Result};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};
use tokio::sync::Semaphore;
/// Expire one ephemeral upload without clock sleeps or a production control.
pub fn expire_media_upload(service: &crate::Service, id: &str) {
    service.mcp_uploads.expire_fixture(id);
}
#[derive(Default)]
pub struct Hooks(Mutex<HashMap<&'static str, Arc<Pause>>>);
pub struct Pause {
    arrived: Semaphore,
    release: Semaphore,
}
impl Hooks {
    pub fn arm(&self, name: &'static str) -> Arc<Pause> {
        let pause = Arc::new(Pause {
            arrived: Semaphore::new(0),
            release: Semaphore::new(0),
        });
        self.0.lock().unwrap().insert(name, pause.clone());
        pause
    }
    pub async fn hit(&self, name: &'static str) {
        let pause = self.0.lock().unwrap().remove(name);
        if let Some(pause) = pause {
            pause.arrived.add_permits(1);
            pause.release.acquire().await.unwrap().forget();
        }
    }
}
impl Pause {
    pub async fn arrived(&self) {
        self.arrived.acquire().await.unwrap().forget();
    }
    pub fn resume(&self) {
        self.release.add_permits(1);
    }
}
/// Child process helper: report a completed durable phase, then park until the
/// test sends SIGKILL. No timing-based crash assumptions. Never built by default.
pub(crate) fn process_phase(name: &str) -> Result<()> {
    #[cfg(test)]
    if STORAGE_FAILURE.with(|p| p.get() == Some(name)) {
        return Err(Error::new("provision_unavailable"));
    }
    if std::env::var("MEGURI_TEST_FAIL_PHASE").ok().as_deref() == Some(name) {
        return Err(Error::new("provision_unavailable"));
    }
    if std::env::var("MEGURI_TEST_KILL_PHASE").ok().as_deref() == Some(name) {
        use std::io::Write;
        let path = std::path::PathBuf::from(
            std::env::var_os("MEGURI_TEST_SIGNAL_FILE").ok_or(Error::new("internal_error"))?,
        );
        let mut pending_name = path
            .file_name()
            .ok_or(Error::new("internal_error"))?
            .to_os_string();
        pending_name.push(".pending");
        let pending = path.with_file_name(pending_name);
        let mut signal = std::fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&pending)?;
        let publication = (|| -> std::io::Result<()> {
            signal.write_all(name.as_bytes())?;
            signal.sync_all()?;
            // Existence must imply complete synced bytes, without replacing a marker.
            std::fs::hard_link(&pending, &path)
        })();
        drop(signal);
        // Only our successfully created sibling is ours to remove. Cleanup is
        // best effort so a published signal cannot be followed by an error.
        let _ = std::fs::remove_file(&pending);
        publication?;
        loop {
            std::thread::park();
        }
    }
    Ok(())
}

// Unit fixtures call synchronous storage operations on their own thread. This
// seam never affects integration processes, other test threads or production.
#[cfg(test)]
std::thread_local! {
    static STORAGE_FAILURE: std::cell::Cell<Option<&'static str>> = const { std::cell::Cell::new(None) };
}
#[cfg(test)]
pub(crate) fn storage_failure<T>(name: &'static str, operation: impl FnOnce() -> T) -> T {
    struct Reset;
    impl Drop for Reset {
        fn drop(&mut self) {
            STORAGE_FAILURE.with(|p| p.set(None));
        }
    }
    STORAGE_FAILURE.with(|p| {
        assert!(p.get().is_none());
        p.set(Some(name));
    });
    let _reset = Reset;
    operation()
}
