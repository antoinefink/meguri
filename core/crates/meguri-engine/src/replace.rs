//! Verified replacement is the swap step of the existing lifecycle, never a
//! second writer lifecycle. Backup/inspection/confirmation belong to consumers.
use crate::device_state::{self, AccountBinding};
use crate::{Collection, EngineError, Exclusive, SlotGuard};
pub use meguri_storage::verified_replace::{ReplaceError as StagingError, ReplacementStage};
use meguri_storage::{OpenOptions, StorageError, WriteScope};
use serde::{Deserialize, Serialize};
use std::{
    path::Path,
    sync::{Arc, Mutex},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplaceReason {
    Restore,
    ColpkgImport,
    FullDownload,
    FullUpload,
}
impl ReplaceReason {
    pub(crate) fn package(self) -> bool {
        matches!(self, Self::Restore | Self::ColpkgImport)
    }
    pub(crate) fn long_op(self) -> crate::LongOp {
        match self {
            Self::Restore => crate::LongOp::Restore,
            Self::ColpkgImport => crate::LongOp::ColpkgImport,
            Self::FullDownload | Self::FullUpload => crate::LongOp::OneWaySync,
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReplaceError {
    NotExclusive,
    Staging(StagingError),
    Lifecycle(EngineError),
    /// The rename succeeded. The candidate file stands even if opening a
    /// writer failed; Some names the successfully reopened process generation.
    AfterActivation {
        generation: Option<crate::Generation>,
        source: Box<ReplaceError>,
    },
}
impl std::fmt::Display for ReplaceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}
impl std::error::Error for ReplaceError {}
impl From<EngineError> for ReplaceError {
    fn from(e: EngineError) -> Self {
        Self::Lifecycle(e)
    }
}
impl From<StagingError> for ReplaceError {
    fn from(e: StagingError) -> Self {
        Self::Staging(e)
    }
}

impl SlotGuard {
    /// Minimal verified entry. Import refuses an active review before closing
    /// admission, and again after the exclusive drain. The legacy closure entry
    /// deliberately retains its ability to replace a session.
    pub fn replace_verified(
        &self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
    ) -> Result<Collection, ReplaceError> {
        if self.op() != reason.long_op() {
            return Err(ReplaceError::NotExclusive);
        }
        self.check_verified_review(reason)?;
        let mut exclusive = self.enter_exclusive(true)?;
        exclusive.replace_verified(stage, reason, change)
    }
}
impl Exclusive<'_> {
    /// Validates while the live collection remains open, then reuses L8's
    /// drain/close/swap/reopen/generation/event/provider protocol. An Err after
    /// rename can leave the new generation active: inspect collection()/state.
    pub fn replace_verified(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
    ) -> Result<Collection, ReplaceError> {
        let environment = self.verified_environment(reason)?;
        let live = self.collection().path().to_owned();
        let options = OpenOptions::new(environment.clock.as_ref(), environment.random.as_ref())
            .initializers(environment.initializers.clone());
        meguri_storage::verified_replace::prepare(
            &stage,
            &live,
            &options,
            environment.clock.as_ref(),
            reason.package(),
            change,
        )?;
        // Recheck after validation too; a closed/stale exclusive cannot swap.
        self.verified_environment(reason)?;
        let error = Arc::new(Mutex::new(None));
        let record = error.clone();
        let activated = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tracked = activated.clone();
        let outcome = activated.clone();
        let old_generation = self.collection().generation();
        let result = self.replace_tracked(
            move |live| {
                swap(
                    live,
                    stage,
                    reason,
                    &environment.binding,
                    environment.random.as_ref(),
                    &tracked,
                )
                .map_err(|e| {
                    *record.lock().unwrap() = Some(e.clone());
                    EngineError::Storage(StorageError::Io { detail: e.to_string() })
                })
            },
            activated,
        );
        let failure = match (result, error.lock().unwrap().take()) {
            (Ok(collection), None) => return Ok(collection),
            (_, Some(error)) => ReplaceError::Staging(error),
            (Err(error), None) => ReplaceError::Lifecycle(error),
        };
        if outcome.load(std::sync::atomic::Ordering::Acquire) {
            let generation =
                (self.collection().generation() != old_generation).then_some(self.collection().generation());
            Err(ReplaceError::AfterActivation {
                generation,
                source: Box::new(failure),
            })
        } else {
            Err(failure)
        }
    }
}

pub(crate) fn swap(
    live: &Path,
    stage: ReplacementStage,
    reason: ReplaceReason,
    binding: &AccountBinding,
    random: &dyn meguri_model::rng::RandomSource,
    activated: &std::sync::atomic::AtomicBool,
) -> Result<(), StagingError> {
    use meguri_storage::{atomic_file, verified_replace as storage};
    storage::check_device(&stage, live)?;
    meguri_model::faultpoint!("replace.before_live_wal_check")?;
    storage::check_live_wal(live)?;
    // Manual one-way replacements also retire queued tokens. Their new
    // identity retains the exact cloud decision rather than resolving it.
    device_state::begin(live, &stage.path(), reason, binding, random)?;
    meguri_model::faultpoint!("replace.after_pause_recorded")?;
    storage::remove_empty_live_sidecars(live)?;
    meguri_model::faultpoint!("replace.after_live_wal_removed")?;
    std::fs::rename(stage.path(), live)?;
    activated.store(true, std::sync::atomic::Ordering::Release);
    meguri_model::faultpoint!("replace.after_rename")?;
    atomic_file::sync_directory(live.parent().unwrap_or(Path::new(".")))?;
    meguri_model::faultpoint!("replace.after_dir_sync")?;
    device_state::activate(live, binding)?;
    // TempDir removes only the operation-owned directory, best effort.
    drop(stage);
    Ok(())
}
