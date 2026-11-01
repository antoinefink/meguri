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

    pub(crate) fn compute_policy_local_cleanup(
        &self,
        guard: &crate::cloud::adapter::Guard,
        sdk: &str,
        collection: &meguri_engine::Collection,
        receipt: &crate::cloud::publication::CleanupDrain,
    ) -> Result<(), RuntimeError> {
        let now_ms = self.adapter_now()?;
        let at_ms = receipt.2;
        let file = self.inputs.files.identity(collection.path())?;
        let account = self.publication_account()?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        Self::gate_locked(&s)?;
        if &Self::adapter_guard_locked(&s) != guard || !s.root_confirmed || Self::publication_allows_send_locked(&s) {
            return Err(RuntimeError::StaleIncarnation);
        }
        if &receipt.0 != guard || receipt.1 != sdk || s.publication.cleanup_drain.as_ref() != Some(receipt) {
            return Err(RuntimeError::StaleIncarnation);
        }
        // Consume only this issued drain; a newer accepted receipt belongs to
        // its own command. Policy projection cannot renew the one-hour window.
        s.publication.cleanup_drain = None;
        if now_ms
            .checked_sub(at_ms)
            .is_none_or(|age| !(0..media::cleanup::FRESH_MS).contains(&age))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if file != b.file
            || account != b.binding.facts.account
            || !collection.is_current()
            || b.evidence.generation != collection.generation()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let binding = b.binding.id;
        if s.publication
            .last_cleanup_ms
            .is_some_and(|last| at_ms.checked_sub(last).is_none_or(|age| age < 86_400_000))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let media = s.media.enabled != Some(false) && !s.media.paused;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .compute_cleanup(binding, at_ms, media)
        })?;
        s.publication.last_cleanup_ms = Some(at_ms);
        s.media.observed_now_ms = now_ms;
        self.emit_pending_locked(&mut s, super::super::workflow::UpdateKind::Full, Default::default())?;
        with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.finish_conflict_expiry())?;
        Ok(())
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
