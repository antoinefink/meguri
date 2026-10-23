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
    pub(crate) fn requires_publication_decision(self) -> bool {
        matches!(self, Self::Restore | Self::ColpkgImport | Self::FullDownload)
    }
    pub(crate) fn refuses_active_review(self) -> bool {
        matches!(self, Self::ColpkgImport | Self::FullDownload | Self::FullUpload)
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
    /// Minimal verified entry. Collection import and manual transfers refuse an
    /// active review before closing admission, and after the exclusive drain.
    /// The legacy closure entry retains its ability to replace a session.
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
        self.replace_verified_before_commit(stage, reason, change, || Ok(()))
    }

    /// Runs `before_commit` after candidate preparation, outside core locks and
    /// reader leases, immediately before closing the live connections for swap.
    /// Once it succeeds, replacement and recovery run to completion.
    pub fn replace_verified_before_commit(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        change: impl FnOnce(&mut WriteScope<'_>) -> Result<(), StorageError>,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
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
        self.activate_finalised(stage, reason, before_commit, None)
    }

    /// Activates an O6.2 artifact after independently verifying its immutable
    /// identity, device, finalisation, gate and integrity. This path never
    /// opens a candidate writer or runs preparation a second time.
    pub fn activate_prepared_upload(
        &mut self,
        artifact: PreparedUpload,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
    ) -> Result<Collection, ReplaceError> {
        self.verified_environment(ReplaceReason::FullUpload)?;
        artifact.check(self.collection().path())?;
        self.activate_finalised(
            artifact.stage,
            ReplaceReason::FullUpload,
            before_commit,
            Some(artifact.identity),
        )
    }

    fn activate_finalised(
        &mut self,
        stage: ReplacementStage,
        reason: ReplaceReason,
        before_commit: impl FnOnce() -> Result<(), EngineError>,
        identity: Option<crate::device_state::FileIdentity>,
    ) -> Result<Collection, ReplaceError> {
        let environment = self.verified_environment(reason)?;
        before_commit()?;
        // Admission is privileged and the writer drained. Recheck under the
        // lifecycle/session locks after preparation and the unlocked hook.
        self.verified_environment(reason)?;
        let error = Arc::new(Mutex::new(None));
        let record = error.clone();
        let activated = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let tracked = activated.clone();
        let outcome = activated.clone();
        let old_generation = self.collection().generation();
        let result = self.replace_tracked(
            move |live| {
                if let Some(expected) = &identity {
                    meguri_storage::one_way::verify_prepared_upload(&stage, live).map_err(|e| {
                        *record.lock().unwrap() = Some(e.clone());
                        EngineError::Storage(StorageError::Io {
                            detail: "prepared upload verification failed".into(),
                        })
                    })?;
                    if device_state::fingerprint(&stage.path())?.as_ref() != Some(expected) {
                        return Err(EngineError::Storage(StorageError::Io {
                            detail: "prepared upload identity changed".into(),
                        }));
                    }
                }
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
    // Every replacement retires queued tokens. Manual Import requires a new
    // publication decision; Export preserves the exact prior decision.
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

/// Owned, verified prepared bytes. Construction verifies the caller's recorded
/// byte identity; no arbitrary unchecked path or caller transform is accepted.
pub struct PreparedUpload {
    stage: ReplacementStage,
    identity: crate::device_state::FileIdentity,
}
impl PreparedUpload {
    pub fn verify(stage: ReplacementStage, live: &Path, length: u64, sha256: [u8; 32]) -> Result<Self, ReplaceError> {
        let artifact = Self {
            stage,
            identity: crate::device_state::FileIdentity { length, sha256 },
        };
        artifact.check(live)?;
        Ok(artifact)
    }
    fn check(&self, live: &Path) -> Result<(), ReplaceError> {
        meguri_storage::one_way::verify_prepared_upload(&self.stage, live)?;
        if device_state::fingerprint(&self.stage.path())
            .map_err(StagingError::from)?
            .as_ref()
            != Some(&self.identity)
        {
            return Err(StagingError::StagingNotFinalised.into());
        }
        Ok(())
    }
}
