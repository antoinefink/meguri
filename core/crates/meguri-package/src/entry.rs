//! RF2 entry orchestration. Only owned inspection candidates reach verified
//! replacement; the host supplies providers, never an alternate lifecycle.
use crate::{
    CollectionPackageFacts, CollectionPackageInspectContext, CollectionPackageInspectError, InspectedCollection,
};
use meguri_engine::{
    CancelToken, Engine, EngineError, Generation, OpenConfig, ProgressPhase, ProgressSink, Seq, SlotGuard,
    ports::{BackupReceipt, BackupService},
    replace::{ReplaceError, ReplaceReason},
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
    AfterBackup {
        source: Box<EntryError>,
        backup: BackupReceipt,
    },
    MediaOutcome {
        source: Box<EntryError>,
        media: meguri_storage::media::ImportMediaCounts,
        retained_reference: String,
    },
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
impl From<std::io::Error> for EntryError {
    fn from(e: std::io::Error) -> Self {
        StorageError::from(e).into()
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
impl EntryError {
    pub fn candidate_active(&self) -> bool {
        match self {
            Self::Replacement {
                error: ReplaceError::AfterActivation { .. },
                ..
            } => true,
            Self::AfterBackup { source, .. } | Self::MediaOutcome { source, .. } => source.candidate_active(),
            _ => false,
        }
    }
    fn with_media(
        self,
        media: &std::sync::Mutex<meguri_storage::media::ImportMediaCounts>,
        reference: &std::sync::Mutex<Option<String>>,
    ) -> Self {
        match reference.lock().unwrap().clone() {
            Some(retained_reference) => Self::MediaOutcome {
                source: Box::new(self),
                media: media.lock().unwrap().clone(),
                retained_reference,
            },
            None => self,
        }
    }
}

#[derive(Clone, Debug)]
pub struct EntryFacts {
    pub source: CollectionPackageFacts,
    pub destination: PathBuf,
    pub replacing_existing: bool,
    pub source_sha256: [u8; 32],
    pub media_count: u64,
    pub media_decoded_bytes: u64,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activation {
    pub generation: Generation,
    pub backup: Option<BackupReceipt>,
    pub media: meguri_storage::media::ImportMediaCounts,
    pub media_retention_reference: Option<String>,
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
    space: Arc<dyn SpaceProvider>,
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
            (Some(seq), Ok(c)) if c.last_seq() == *seq && c.path() == self.facts.destination => self.check_source(),
            (None, Err(_)) => self.check_source(),
            _ => Err(EntryError::StaleConfirmation),
        }
    }
    fn check_source(&self) -> Result<(), EntryError> {
        use sha2::{Digest, Sha256};
        use std::io::Read;
        let mut file = std::fs::File::open(self._source.path().join("source.colpkg")).map_err(StorageError::from)?;
        let mut digest = Sha256::new();
        let mut chunk = [0; 65536];
        loop {
            self.source_cancel.check()?;
            let n = file.read(&mut chunk).map_err(StorageError::from)?;
            if n == 0 {
                break;
            }
            digest.update(&chunk[..n]);
        }
        let actual: [u8; 32] = digest.finalize().into();
        if actual != self.facts.source_sha256 {
            return Err(EntryError::StaleConfirmation);
        }
        Ok(())
    }

    pub fn activate_ready(
        self,
        slot: &SlotGuard,
        backup: &dyn BackupService,
        cancel: &Arc<CancelToken>,
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
        self.check(slot, cancel).map_err(|e| EntryError::AfterBackup {
            source: Box::new(e),
            backup: receipt.clone(),
        })?;
        let mut stage = self.inspected.into_replacement().map_err(|e| EntryError::AfterBackup {
            source: Box::new(e.into()),
            backup: receipt.clone(),
        })?;
        stage.media_space = Some(self.space.clone());
        stage.media_backup = Some(meguri_storage::media::ImportMediaBackup {
            name: receipt.name.clone(),
            size_bytes: receipt.size_bytes,
            instant_ms: receipt.instant.0,
            snapshot_modified_ms: receipt.snapshot_modified.0,
        });
        let source_cancel = self.source_cancel.clone();
        let activation_cancel = cancel.clone();
        stage.media_cancel = Some(Arc::new(move || {
            source_cancel.is_cancelled() || activation_cancel.is_cancelled()
        }));
        let media = stage.media_counts();
        let reference = stage.media_reference();
        let current_media = media.clone();
        exclusive
            .replace_verified_before_commit(
                stage,
                ReplaceReason::ColpkgImport,
                |_| Ok(()),
                || {
                    self.source_cancel.check()?;
                    cancel.check()?;
                    let counts = current_media.lock().unwrap().clone();
                    progress.report_collection_media_prepared(&counts);
                    self.source_cancel.check()?;
                    cancel.check()?;
                    progress.set_phase(ProgressPhase::Committing);
                    Ok(())
                },
            )
            .map(|c| Activation {
                generation: c.generation(),
                backup: Some(receipt.clone()),
                media: media.lock().unwrap().clone(),
                media_retention_reference: reference.lock().unwrap().clone(),
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
            .map_err(|e| e.with_media(&media, &reference))
    }

    /// Host passes its actual composed OpenConfig. This calls the one initial
    /// replacement seam, whose retained ownership protects the missing-file test.
    pub fn activate_initial(
        self,
        slot: &SlotGuard,
        config: OpenConfig,
        cancel: &Arc<CancelToken>,
        progress: &ProgressSink,
    ) -> Result<Activation, EntryError> {
        progress.set_phase(ProgressPhase::Named("activation"));
        self.check(slot, cancel)?;
        if !self.is_initial() {
            return Err(EntryError::StaleConfirmation);
        }
        let mut stage = self.inspected.into_replacement()?;
        stage.media_space = Some(self.space.clone());
        let source_cancel = self.source_cancel.clone();
        let activation_cancel = cancel.clone();
        stage.media_cancel = Some(Arc::new(move || {
            source_cancel.is_cancelled() || activation_cancel.is_cancelled()
        }));
        let media = stage.media_counts();
        let reference = stage.media_reference();
        let current_media = media.clone();
        slot.replace_verified_initial_before_commit(
            &self.facts.destination,
            config,
            stage,
            ReplaceReason::ColpkgImport,
            |_| Ok(()),
            || {
                self.source_cancel.check()?;
                cancel.check()?;
                let counts = current_media.lock().unwrap().clone();
                progress.report_collection_media_prepared(&counts);
                self.source_cancel.check()?;
                cancel.check()?;
                progress.set_phase(ProgressPhase::Committing);
                Ok(())
            },
        )
        .map(|c| Activation {
            generation: c.generation(),
            backup: None,
            media: media.lock().unwrap().clone(),
            media_retention_reference: reference.lock().unwrap().clone(),
        })
        .map_err(|error| {
            if error == ReplaceError::Lifecycle(EngineError::Cancelled) {
                EntryError::Cancelled
            } else {
                EntryError::Replacement { error, backup: None }
            }
        })
        .map_err(|e| e.with_media(&media, &reference))
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
    use std::os::unix::fs::MetadataExt;
    let backup_parent = crate::writer::existing_directory(&paths.backups)?;
    let shared_volume = std::fs::metadata(&backup_parent)?.dev() == std::fs::metadata(&paths.directory)?.dev();
    if backup_budget != 0 && !shared_volume {
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
        reserved_space_bytes: if shared_volume { backup_budget } else { 0 },
        clock: inputs.clock.as_ref(),
        random: inputs.random.as_ref(),
        space: inputs.space.as_ref(),
        is_cancelled: &is_cancelled,
    };
    let (inspected, source_copy, digest) =
        crate::inspection::copy_and_inspect_local_collection_package(source, &context, progress)?;
    let facts = EntryFacts {
        source: inspected.facts().clone(),
        destination,
        replacing_existing: existing.is_some(),
        source_sha256: digest,
        media_count: inspected.media_count(),
        media_decoded_bytes: inspected.media_bytes(),
    };
    Ok(EntryInspection {
        engine: engine.clone(),
        generation,
        committed,
        inspected,
        _source: source_copy,
        source_cancel: cancel,
        facts,
        space: inputs.space.clone(),
    })
}
