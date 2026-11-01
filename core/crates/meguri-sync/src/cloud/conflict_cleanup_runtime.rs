//! Local open recovery and the reusable P10.3 item 4 Runtime helper.
use super::*;

impl Runtime {
    /// Only resolved-copy retention. This is not full RunCleanup, which stays
    /// unavailable until its other work and native inventory are implemented.
    /// No bound owner, verified account, SDK, media setting or sync gate needed.
    pub fn cleanup_resolved_conflicts(&self) -> Result<usize, RuntimeError> {
        let now = self.adapter_now()?;
        let mut state = self.state();
        with_tracking_access(|| {
            let store = state.store.as_mut().ok_or(Error::Database)?;
            let removed = store.expire_resolved_conflicts(now)?;
            store.finish_conflict_expiry()?;
            Ok(removed)
        })
    }

    pub(super) fn recover_conflict_staging_at_open(&self) -> Result<(), RuntimeError> {
        {
            let mut state = self.state();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                state.store.as_mut().ok_or(Error::Database)?.sweep_conflict_staging()?;
                Ok(())
            })?;
        }
        // V2.4: step 6 precedes expiry, before any identity read or engine start.
        self.cleanup_resolved_conflicts()?;
        Ok(())
    }
}
