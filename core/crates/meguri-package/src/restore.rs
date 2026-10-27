//! Local T8 restore. Validation precedes safety work; activation remains the
//! engine's verified replacement, including its durable R6 publication fence.
use crate::{
    CollectionPackageInspectContext, CollectionPackageInspectError,
    backup::naming::{BackupKind, parse_name},
    entry::EntryInputs,
};
use meguri_engine::{
    CancelToken, Engine, EngineError, Generation, LifecycleState, LongOp, OpenConfig, ProgressPhase, ProgressSink,
    ports::{BackupService, GuardedTransferBackup, TransferBackup},
    replace::{ReplaceError, ReplaceReason},
};
use meguri_storage::{StorageError, paths::CollectionPaths};
use std::{fs, path::Path, sync::Arc};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RestoredBackup {
    pub name: String,
    pub safety_backup: Option<String>,
    pub generation: Generation,
    pub cloud_publication_pending: bool,
    pub deadline: Option<meguri_model::time::EpochMillis>,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RestoreError {
    Engine(EngineError),
    Package(CollectionPackageInspectError),
    Replacement {
        error: ReplaceError,
        safety_backup: Option<String>,
    },
    AfterActivation {
        error: EngineError,
        restored: RestoredBackup,
    },
    AfterSafety {
        error: Box<RestoreError>,
        safety_backup: Option<String>,
    },
}
impl From<EngineError> for RestoreError {
    fn from(e: EngineError) -> Self {
        Self::Engine(e)
    }
}
impl From<StorageError> for RestoreError {
    fn from(e: StorageError) -> Self {
        EngineError::from(e).into()
    }
}
impl From<std::io::Error> for RestoreError {
    fn from(e: std::io::Error) -> Self {
        StorageError::from(e).into()
    }
}
impl From<CollectionPackageInspectError> for RestoreError {
    fn from(e: CollectionPackageInspectError) -> Self {
        Self::Package(e)
    }
}
impl std::fmt::Display for RestoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for RestoreError {}
impl RestoreError {
    pub fn candidate_active(&self) -> bool {
        match self {
            Self::Replacement {
                error: ReplaceError::AfterActivation { .. },
                ..
            } => true,
            Self::AfterActivation { .. } => true,
            Self::AfterSafety { error, .. } => error.candidate_active(),
            _ => false,
        }
    }
}
fn invalid(message: &str) -> RestoreError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
    .into()
}
fn file_size(path: &Path) -> Result<Option<u64>, RestoreError> {
    match path.symlink_metadata() {
        Ok(m) if m.file_type().is_file() => Ok(Some(m.len())),
        Ok(_) => Err(invalid("restore requires regular local collection files")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e.into()),
    }
}
fn safety_name(backup: &TransferBackup) -> Option<String> {
    match backup {
        TransferBackup::Collection(receipt) => Some(receipt.name.clone()),
        TransferBackup::Raw { main, .. } => Some(main.clone()),
        TransferBackup::Absent => None,
    }
}

/// Worker entry point. `initial_open` is the host's already-composed provider
/// configuration, used only when no collection writer exists. There is no UI
/// confirmation or publication decision in this local operation.
pub fn restore_backup(
    engine: &Engine,
    inputs: &EntryInputs,
    initial_open: OpenConfig,
    backup: &dyn BackupService,
    name: &str,
    cancel: &Arc<CancelToken>,
    progress: &ProgressSink,
) -> Result<RestoredBackup, RestoreError> {
    // Check the *whole* name before slot admission, metadata reads or package IO.
    match parse_name(name) {
        Some((BackupKind::Backup | BackupKind::ServerCopy, _)) => {}
        _ => return Err(invalid("unrecognized restore backup name")),
    }
    let slot = engine.acquire_slot(LongOp::Restore)?;
    cancel.check()?;
    let paths = CollectionPaths::new(&inputs.collection_directory, &inputs.backups_directory)?;
    let live = paths.collection();
    if let Ok(current) = engine.collection()
        && current.path() != live
    {
        return Err(invalid("restore destination differs from the open collection"));
    }
    let source = paths.backups.join(name);
    if file_size(&source)?.is_none() {
        return Err(invalid("backup is absent"));
    }
    let existing = file_size(&live)?;
    let wal = if existing.is_some() {
        file_size(&live.with_file_name("collection.anki2-wal"))?.unwrap_or(0)
    } else {
        0
    };
    let live_size = existing
        .unwrap_or(0)
        .checked_add(wal)
        .ok_or_else(|| invalid("restore size overflow"))?;
    let damaged = matches!(
        engine.state(),
        LifecycleState::Failed { .. } | LifecycleState::NoCollection
    );
    let safety_need = if existing.is_some() {
        live_size
            .checked_mul(if damaged { 1 } else { 2 })
            .and_then(|n| n.checked_add(4 * 1024 * 1024))
            .ok_or_else(|| invalid("restore safety size overflow"))?
    } else {
        0
    };
    // Same-volume inspection reserves both needs in its one preflight. On
    // separate volumes each volume must independently hold its own artifact.
    use std::os::unix::fs::MetadataExt;
    let backup_parent = crate::writer::existing_directory(&paths.backups)?;
    let shared_volume = fs::metadata(&backup_parent)?.dev() == fs::metadata(&paths.directory)?.dev();
    if safety_need > 0 && !shared_volume {
        meguri_storage::space::require_space(inputs.space.as_ref(), &backup_parent, safety_need)?;
    }
    progress.set_phase(ProgressPhase::Named("inspect"));
    let inspected = crate::inspect_collection_package(
        &source,
        &CollectionPackageInspectContext {
            collection_directory: &paths.directory,
            live_collection_size: live_size,
            reserved_space_bytes: if shared_volume { safety_need } else { 0 },
            clock: inputs.clock.as_ref(),
            random: inputs.random.as_ref(),
            space: inputs.space.as_ref(),
            is_cancelled: &|| cancel.is_cancelled(),
        },
    )?;
    // Retention can remove source now: the staged file owns all incoming bytes.
    let mut stage = inspected.into_replacement()?;
    let activation_cancel = cancel.clone();
    stage.media_cancel = Some(Arc::new(move || activation_cancel.is_cancelled()));
    cancel.check()?;
    slot.check_restore_review()?;
    let mut safety: Option<GuardedTransferBackup> = None;
    let replaced = if engine.collection().is_ok() {
        let mut exclusive = slot.enter_exclusive(true)?;
        slot.check_restore_review()?;
        cancel.check()?;
        progress.set_phase(ProgressPhase::Named("backup"));
        safety = Some(backup.before_restore(Some(exclusive.collection()), damaged, cancel)?);
        let safety_backup = safety.as_ref().and_then(|s| safety_name(&s.backup));
        exclusive
            .replace_verified_before_commit(
                stage,
                ReplaceReason::Restore,
                |_| Ok(()),
                || {
                    cancel.check()?;
                    progress.set_phase(ProgressPhase::Committing);
                    Ok(())
                },
            )
            .map_err(|error| RestoreError::Replacement { error, safety_backup })
    } else {
        // Initial replacement acquires advisory ownership before this hook.
        // It validates/finalises the candidate before raw safety work and keeps
        // the same ownership through intent, rename, recovery and managed open.
        slot.restore_verified_initial_before_commit(&live, initial_open, stage, || {
            cancel.check()?;
            progress.set_phase(ProgressPhase::Named("backup"));
            safety = Some(backup.before_restore(None, true, cancel)?);
            cancel.check()?;
            progress.set_phase(ProgressPhase::Committing);
            Ok(safety.as_mut().and_then(|s| s.raw_restore.take()))
        })
        .map_err(|error| RestoreError::Replacement {
            error,
            safety_backup: safety.as_ref().and_then(|s| safety_name(&s.backup)),
        })
    };
    let safety_backup = safety.as_ref().and_then(|s| safety_name(&s.backup));
    let collection = replaced?;
    // Exclusive has ended. Re-evaluate from the restored col.mod and the
    // durable safety reference before returning the new host timer deadline.
    let mut restored = RestoredBackup {
        name: name.into(),
        safety_backup,
        generation: collection.generation(),
        cloud_publication_pending: true,
        deadline: engine.maintenance().deadline(),
    };
    restored.deadline = (|| {
        meguri_model::faultpoint!("restore.before_maintenance_reevaluation")
            .map_err(StorageError::from)
            .map_err(EngineError::from)?;
        engine.maintenance().reevaluate()
    })()
    .map_err(|error| RestoreError::AfterActivation {
        error,
        restored: restored.clone(),
    })?;
    Ok(restored)
}
