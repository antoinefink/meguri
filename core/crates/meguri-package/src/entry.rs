//! RF2 entry orchestration. Only owned inspection candidates reach verified
//! replacement; the host supplies providers, never an alternate lifecycle.
use crate::{
    CollectionPackageFacts, CollectionPackageInspectContext, CollectionPackageInspectError, InspectedCollection,
};
use meguri_engine::{
    CancelToken, Engine, EngineError, Generation, OpenConfig, ProgressPhase, ProgressSink, Seq, SlotGuard,
    ports::{BackupReceipt, BackupService},
    replace::{ReplaceError, ReplaceReason, ReplacementStage},
};
use meguri_model::{clock::Clock, rng::RandomSource};
use meguri_storage::{StorageError, paths::CollectionPaths, space::SpaceProvider};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

#[derive(Clone)]
pub struct EntryInputs {
    pub collection_directory: PathBuf,
    pub backups_directory: PathBuf,
    pub clock: Arc<dyn Clock>,
    pub random: Arc<dyn RandomSource>,
    pub space: Arc<dyn SpaceProvider>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EntryError {
    Package(CollectionPackageInspectError),
    Engine(EngineError),
    StaleConfirmation,
    Cancelled,
    CancelledAfterBackup(BackupReceipt),
    Replacement {
        error: ReplaceError,
        backup: Option<Box<BackupReceipt>>,
    },
}
impl From<EngineError> for EntryError {
    fn from(e: EngineError) -> Self {
        if e == EngineError::Cancelled {
            Self::Cancelled
        } else {
            Self::Engine(e)
        }
    }
}
impl From<StorageError> for EntryError {
    fn from(e: StorageError) -> Self {
        EngineError::from(e).into()
    }
}
impl From<CollectionPackageInspectError> for EntryError {
    fn from(e: CollectionPackageInspectError) -> Self {
        if e == CollectionPackageInspectError::Cancelled {
            Self::Cancelled
        } else {
            Self::Package(e)
        }
    }
}
impl std::fmt::Display for EntryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for EntryError {}

#[derive(Clone, Debug)]
pub struct EntryFacts {
    pub source: CollectionPackageFacts,
    pub destination: PathBuf,
    pub replacing_existing: bool,
    pub source_sha256: [u8; 32],
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activation {
    pub generation: Generation,
    pub backup: Option<BackupReceipt>,
}

/// No public path or candidate accessor. Moving this value consumes its sole
/// authority, including after a stale/cancelled/refused activation.
pub struct EntryInspection {
    engine: Engine,
    generation: Generation,
    committed: Option<Seq>,
    inspected: InspectedCollection,
    _source: tempfile::TempDir,
    source_cancel: Arc<CancelToken>,
    facts: EntryFacts,
}
impl EntryInspection {
    pub fn facts(&self) -> &EntryFacts {
        &self.facts
    }
    pub fn is_initial(&self) -> bool {
        self.committed.is_none()
    }

    fn check(&self, slot: &SlotGuard, cancel: &CancelToken) -> Result<(), EntryError> {
        self.source_cancel.check()?;
        cancel.check()?;
        if !slot.belongs_to(&self.engine) || self.engine.generation() != self.generation {
            return Err(EntryError::StaleConfirmation);
        }
        match (&self.committed, self.engine.collection()) {
            (Some(seq), Ok(c)) if c.last_seq() == *seq && c.path() == self.facts.destination => Ok(()),
            (None, Err(_)) => Ok(()),
            _ => Err(EntryError::StaleConfirmation),
        }
    }

    pub fn activate_ready(
        self,
        slot: &SlotGuard,
        backup: &dyn BackupService,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> Result<Activation, EntryError> {
        self.check(slot, cancel)?;
        if self.is_initial() {
            return Err(EntryError::StaleConfirmation);
        }
        slot.check_verified_review(ReplaceReason::ColpkgImport)?;
        progress.set_phase(ProgressPhase::Named("activation"));
        let mut exclusive = slot.enter_exclusive(true)?;
        // Participant flush may commit an old answer or race a new session.
        // Check the committed operation sequence, never the stream's event seq.
        slot.check_verified_review(ReplaceReason::ColpkgImport)?;
        self.check(slot, cancel)?;
        progress.set_phase(ProgressPhase::Named("backup"));
        let receipt = backup.before_colpkg_import(exclusive.collection(), cancel)?;
        if self.source_cancel.is_cancelled() || cancel.is_cancelled() {
            return Err(EntryError::CancelledAfterBackup(receipt));
        }
        let stage = ReplacementStage::from_staging(self.inspected.into_staged().into_directory())?;
        exclusive
            .replace_verified_before_commit(
                stage,
                ReplaceReason::ColpkgImport,
                |_| Ok(()),
                || {
                    self.source_cancel.check()?;
                    cancel.check()?;
                    progress.set_phase(ProgressPhase::Committing);
                    Ok(())
                },
            )
            .map(|c| Activation {
                generation: c.generation(),
                backup: Some(receipt.clone()),
            })
            .map_err(|error| {
                if error == ReplaceError::Lifecycle(EngineError::Cancelled) {
                    EntryError::CancelledAfterBackup(receipt)
                } else {
                    EntryError::Replacement {
                        error,
                        backup: Some(Box::new(receipt)),
                    }
                }
            })
    }

    /// Host passes its actual composed OpenConfig. This calls the one initial
    /// replacement seam, whose retained ownership protects the missing-file test.
    pub fn activate_initial(
        self,
        slot: &SlotGuard,
        config: OpenConfig,
        cancel: &CancelToken,
        progress: &ProgressSink,
    ) -> Result<Activation, EntryError> {
        progress.set_phase(ProgressPhase::Named("activation"));
        self.check(slot, cancel)?;
        if !self.is_initial() {
            return Err(EntryError::StaleConfirmation);
        }
        let stage = ReplacementStage::from_staging(self.inspected.into_staged().into_directory())?;
        slot.replace_verified_initial_before_commit(
            &self.facts.destination,
            config,
            stage,
            ReplaceReason::ColpkgImport,
            |_| Ok(()),
            || {
                self.source_cancel.check()?;
                cancel.check()?;
                progress.set_phase(ProgressPhase::Committing);
                Ok(())
            },
        )
        .map(|c| Activation {
            generation: c.generation(),
            backup: None,
        })
        .map_err(|error| {
            if error == ReplaceError::Lifecycle(EngineError::Cancelled) {
                EntryError::Cancelled
            } else {
                EntryError::Replacement { error, backup: None }
            }
        })
    }
}

/// Stable source access is a caller precondition: on Apple keep security scope
/// plus NSFileCoordinator's synchronous read accessor until the inspect job's
/// terminal event, including cancellation. No timeout may release access early.
pub fn inspect_entry(
    engine: &Engine,
    source: &Path,
    inputs: &EntryInputs,
    cancel: Arc<CancelToken>,
    progress: &ProgressSink,
) -> Result<EntryInspection, EntryError> {
    cancel.check()?;
    // The latest container is shared with deck packages; the entry flow accepts
    // only the picker-selected collection-package type, never a deck import.
    if !source
        .extension()
        .and_then(|e| e.to_str())
        .is_some_and(|e| e.eq_ignore_ascii_case("colpkg"))
    {
        return Err(CollectionPackageInspectError::UnsupportedFormat.into());
    }
    let paths = CollectionPaths::new(&inputs.collection_directory, &inputs.backups_directory)?;
    let destination = paths.collection();
    let existing = match destination.symlink_metadata() {
        Ok(m) if m.is_file() && !m.file_type().is_symlink() => Some(m.len()),
        Ok(_) => {
            return Err(EngineError::InvalidArgument {
                message: "destination is not a regular collection file".into(),
            }
            .into());
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(StorageError::from(e).into()),
    };
    let (generation, committed) = match engine.collection() {
        Ok(c) if c.path() == destination && existing.is_some() => (c.generation(), Some(c.last_seq())),
        Ok(_) => return Err(EntryError::StaleConfirmation),
        Err(_) if existing.is_none() => (engine.generation(), None),
        Err(_) => {
            return Err(EngineError::Unsupported {
                feature: "pre-import safety backup of an unopened collection (T7)".into(),
            }
            .into());
        }
    };
    let mut live_size = existing.unwrap_or(0);
    if existing.is_some() {
        let wal = destination.with_file_name("collection.anki2-wal");
        match wal.symlink_metadata() {
            Ok(m) if m.is_file() && !m.file_type().is_symlink() => {
                live_size = live_size
                    .checked_add(m.len())
                    .ok_or_else(|| crate::corrupt("live size overflow"))?
            }
            Ok(_) => {
                return Err(EngineError::InvalidArgument {
                    message: "live WAL is not a regular file".into(),
                }
                .into());
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(StorageError::from(e).into()),
        }
    }
    let backup_budget = if existing.is_some() {
        live_size
            .checked_mul(2)
            .and_then(|n| n.checked_add(4 * 1024 * 1024))
            .ok_or_else(|| crate::corrupt("backup budget overflow"))?
    } else {
        0
    };
    if backup_budget != 0 {
        // The backup may live on another filesystem. Check it before copy too.
        let directory = crate::writer::existing_directory(&paths.backups)?;
        meguri_storage::space::require_space(inputs.space.as_ref(), &directory, backup_budget).map_err(
            |e| match e {
                StorageError::InsufficientSpace { required, available } => {
                    EntryError::Package(CollectionPackageInspectError::InsufficientSpace {
                        needed: required,
                        available,
                    })
                }
                other => other.into(),
            },
        )?;
    }
    let is_cancelled = || cancel.is_cancelled();
    let context = CollectionPackageInspectContext {
        collection_directory: &paths.directory,
        live_collection_size: live_size,
        reserved_space_bytes: backup_budget,
        clock: inputs.clock.as_ref(),
        random: inputs.random.as_ref(),
        space: inputs.space.as_ref(),
        is_cancelled: &is_cancelled,
    };
    let (inspected, source_copy, digest) = crate::copy_and_inspect_collection_package(source, &context, progress)?;
    let facts = EntryFacts {
        source: inspected.facts().clone(),
        destination,
        replacing_existing: existing.is_some(),
        source_sha256: digest,
    };
    Ok(EntryInspection {
        engine: engine.clone(),
        generation,
        committed,
        inspected,
        _source: source_copy,
        source_cancel: cancel,
        facts,
    })
}
