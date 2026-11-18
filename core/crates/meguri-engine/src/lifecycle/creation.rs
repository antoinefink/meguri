//! Explicit first-local creation. Opening excludes competing lifecycle/slot
//! mutations; the ordinary initial-open path adopts the published file.
use super::*;

impl Engine {
    /// Host-only entry, with the original Host lease held by the caller.
    /// `admit` checks retained provider work without changing its evidence.
    pub fn create_empty_raw(
        &self,
        live: &Path,
        mut config: OpenConfig,
        admit: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<Collection, EngineError> {
        {
            let _control = self.job_control_lane()?;
            let mut state = self.lock_state();
            if !matches!(state.state, LifecycleState::NoCollection)
                || state.current.is_some() || state.slot.is_some() || state.suspended
                || state.recovery_ownership.is_some()
            {
                return Err(EngineError::unavailable("new collection requires an idle NoCollection owner"));
            }
            self.set_state_locked(&mut state, LifecycleState::Opening);
        }
        let directory = live.parent().unwrap_or_else(|| Path::new("."));
        let mut attempted = false;
        let prepared = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let ownership = Arc::new(meguri_storage::lockfile::CollectionLock::acquire(directory)?);
            require_absent(live)?;
            admit()?;
            // An absent file with an old publication/intent is recovery, not a
            // fresh installation. Never reconcile it into new local authority.
            if crate::device_state::DeviceState::for_collection(live).settled_publication()?.is_some() {
                return Err(EngineError::unavailable("new collection has retained publication evidence"));
            }
            config.publication_binding = crate::device_state::AccountBinding::LocalOnly;
            config.startup_publication_binding = None;
            config.startup_identity_read_failed = false;
            self.lock_state().recovery_ownership = Some(ownership.clone());
            attempted = true;
            crate::bootstrap::create(live, config.clock.as_ref(), config.random.as_ref())?;
            Ok::<_, EngineError>(ownership)
        })).unwrap_or_else(|payload| Err(EngineError::from_panic(payload.as_ref())));
        let ownership = match prepared {
            Ok(ownership) => ownership,
            Err(error) => {
                // A failed directory sync may follow successful publication.
                // Never unlink, retry, or claim absence on an unknown outcome.
                let mut state = self.lock_state();
                if !attempted && require_absent(live).is_ok() {
                    state.recovery_ownership = None;
                    self.set_state_locked(&mut state, LifecycleState::NoCollection);
                } else {
                    self.set_state_locked(&mut state, LifecycleState::Failed {
                        reason: FailedReason::OpenStep { step: "create collection".into(), detail: error.to_string() },
                    });
                }
                return Err(error);
            }
        };
        // Retain the advisory lock even if normal open or an open step fails.
        // Successful open transfers it to Current and supplies the generation.
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.open_raw_owned(live, config, Some(ownership))
        })).unwrap_or_else(|payload| {
            let error = EngineError::from_panic(payload.as_ref());
            self.set_state(LifecycleState::Failed {
                reason: FailedReason::OpenStep { step: "create collection open".into(), detail: error.to_string() },
            });
            Err(error)
        })
    }
}

fn require_absent(live: &Path) -> Result<(), EngineError> {
    for suffix in ["", "-wal", "-shm", "-journal"] {
        let path = live.with_file_name(format!("{}{}", live.file_name().unwrap_or_default().to_string_lossy(), suffix));
        match path.symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
            Ok(_) => return Err(EngineError::unavailable("new collection destination is not absent")),
        }
    }
    Ok(())
}
