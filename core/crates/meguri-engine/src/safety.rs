//! T6 admission lives outside writer/undo locks. Reuse evidence lasts only
//! for this opening; undo/history changes conservatively discard it.
use crate::ports::{BackupFailureCause, BackupPublication, GuardedBackup, SafetyBackupFailure};
use crate::{Collection, EngineError, OpSource, OpSpec, SchemaReason};

impl Collection {
    pub(crate) fn admit_safety(&self, spec: &OpSpec) -> Result<GuardedBackup, EngineError> {
        if crate::executor::writer::on_writer_thread() {
            return Err(EngineError::InvalidArgument {
                message: "safety admission must run off the writer".into(),
            });
        }
        self.check_current()?;
        if let Some(cancel) = &spec.cancel {
            cancel.check()?;
        }
        let missing = || {
            EngineError::SafetyBackupFailed(Box::new(SafetyBackupFailure {
                cause: BackupFailureCause::Io {
                    detail: "safety backup provider unavailable".into(),
                },
                publication: BackupPublication::Unpublished,
            }))
        };
        let provider = self.core.backup.as_ref().ok_or_else(missing)?;
        let reusable = spec.source == OpSource::UserUndoable
            && !spec.def.undo_class().is_some_and(|class| class.requires_backup())
            && !spec.schema_reasons.contains(&SchemaReason::NotetypeRemoved);
        let now = self.core.exec.clock.now_ms();
        let (revision, previous) = {
            let _commit = self.core.exec.lock_commit();
            self.core.exec.undo.safety_state(now)
        };
        if reusable && let Some(receipt) = previous {
            // Pin before trusting the candidate; retention may have removed it
            // since the preceding operation released its pin.
            if let Some(protection) = provider.protect_backup(&receipt.name)? {
                let still_current = {
                    let _commit = self.core.exec.lock_commit();
                    self.core.exec.undo.safety_state(now).0 == revision
                };
                if still_current {
                    return Ok(GuardedBackup {
                        receipt: Some(receipt),
                        protection: Some(protection),
                    });
                }
            }
        }
        let cancel = spec.cancel.clone().unwrap_or_default();
        let backup = provider.before_safety_operation(self, &cancel).map_err(|error| {
            if cancel.is_cancelled() {
                return EngineError::Cancelled;
            }
            match error {
                EngineError::SafetyBackupFailed(_) | EngineError::Cancelled => error,
                other => EngineError::SafetyBackupFailed(Box::new(SafetyBackupFailure {
                    cause: BackupFailureCause::Engine(Box::new(other)),
                    publication: BackupPublication::Unpublished,
                })),
            }
        })?;
        cancel.check()?;
        self.check_current()?;
        if reusable && let Some(receipt) = &backup.receipt {
            // Completion time is deliberately separate from lastBackup.instant,
            // which the existing writer captures before making its snapshot.
            let completed = self.core.exec.clock.now_ms();
            let _commit = self.core.exec.lock_commit();
            if completed >= now {
                self.core
                    .exec
                    .undo
                    .safety_completed(revision, completed, receipt.clone());
            }
        }
        Ok(backup)
    }
}
