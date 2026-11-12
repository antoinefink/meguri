use super::*;
use crate::cloud::{
    publication::{RecordRead, ZoneDeletionGuard},
    zone_deletion::{Diagnostic, ZoneResult},
};

impl Runtime {
    pub fn zone_deletion_due(&self, instance: &str) -> Result<bool, RuntimeError> {
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        Ok(
            with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.retained_zones(b.binding.id))?
                .values()
                .any(|row| row.deletion_due),
        )
    }
    /// Recheck the WHOLE original selection at final synchronous queue admission.
    /// No provider or callback runs with Runtime/tracking authority held.
    pub fn submit_zone_deletion_batch<T>(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        read: RecordRead,
        zones: &[String],
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        self.submit_zone_deletion(guard, collection, || {
            let current = self.zone_deletion_check(guard, read, collection)?;
            if zones.is_empty() || current != zones || zones.iter().any(|z| !z.starts_with("c-")) {
                return Err(RuntimeError::StaleIncarnation);
            }
            Ok(submit())
        })?
    }
    pub fn zone_deletion_diagnostics(&self) -> Vec<Diagnostic> {
        self.state().adapter.zone_deletions.clone()
    }
    pub fn record_zone_deletion_diagnostic(&self, diagnostic: Diagnostic) {
        let mut s = self.state();
        // Bounded process-local debug evidence, never record payloads/accounts.
        if s.adapter.zone_deletions.len() == 64 {
            s.adapter.zone_deletions.remove(0);
        }
        s.adapter.zone_deletions.push(diagnostic);
    }
    pub fn zone_deletion_read_reset(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.zone_deletion_read_loss(guard, collection, IsolationReason::EncryptedDataReset)
    }
    pub fn zone_deletion_read_loss(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        reason: IsolationReason,
    ) -> Result<(), RuntimeError> {
        if !matches!(
            reason,
            IsolationReason::EncryptedDataReset | IsolationReason::ZoneDeletedByUser | IsolationReason::ActiveMissing
        ) {
            return Err(Error::InvalidData.into());
        }
        self.validate_zone_deletion_guard(guard, collection)?;
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let proof = s
            .adapter
            .accepted_identity_read
            .as_ref()
            .ok_or(RuntimeError::IdentityUnverified)?
            .request;
        Self::fresh_loss_identity_locked(&s, proof, guard.after_identity)?;
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| Self::commit_service_isolation_locked(&mut s, id, reason))?;
        let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
        b.binding.facts.state = BindingState::Isolated(reason);
        b.usable = false;
        b.verified = false;
        s.root_confirmed = false;
        s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        s.publish_authority();
        let live = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.inbox(id))?;
        super::super::payload_asset::sweep(&self.directory, id, &live)?;
        Ok(())
    }
    pub fn zone_deletion_batch_result(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        results: &[ZoneResult],
    ) -> Result<(), RuntimeError> {
        self.zone_deletion_batch_result_with_terminal(guard, collection, results, None)
    }
    pub fn zone_deletion_batch_result_with_terminal(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        results: &[ZoneResult],
        terminal: Option<&crate::cloud::zone_deletion::FailureFacts>,
    ) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        self.validate_zone_deletion_guard(guard, collection)?;
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let reset = terminal.is_some_and(|f| f.code == Some(26) && f.encrypted_data_reset)
            || results.iter().any(|r| {
                r.failure
                    .as_ref()
                    .is_some_and(|f| f.code == Some(26) && f.encrypted_data_reset)
            });
        if reset {
            let proof = s
                .adapter
                .accepted_identity_read
                .as_ref()
                .ok_or(RuntimeError::IdentityUnverified)?
                .request;
            Self::fresh_loss_identity_locked(&s, proof, guard.after_identity)?;
        }
        let zones = results
            .iter()
            .filter(|r| r.failure.as_ref().is_none_or(|f| f.code == Some(26)))
            .map(|r| r.zone.clone())
            .collect::<Vec<_>>();
        let durable = s.adapter.durable.clone();
        with_tracking_access(|| {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                s.store.as_mut().ok_or(Error::Database)?.adapter_commit_loss(
                    id,
                    &[],
                    &[],
                    None,
                    &durable,
                    super::super::adapter_store::AdapterCommitOptions {
                        deleted_collection_zones: zones.clone(),
                        at_ms,
                        fail: false,
                        start_absence: false,
                        resolved_epoch: None,
                        fetched_epoch: None,
                    },
                    Some((reset.then_some(IsolationReason::EncryptedDataReset), false, false)),
                )
            }))
            .unwrap_or(Err(Error::Database));
            if let Err(error) = result {
                Self::invalidate_tracking_locked(&mut s);
                return Err(RuntimeError::from(error));
            }
            if reset {
                let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                b.binding.facts.state = BindingState::Isolated(IsolationReason::EncryptedDataReset);
                b.usable = false;
                b.verified = false;
                s.root_confirmed = false;
                s.publish_authority();
            }
            Ok(())
        })?;
        if !zones.is_empty() {
            s.zone_listing_fence = Arc::new(());
        }
        if reset {
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.binding.facts.state = BindingState::Isolated(IsolationReason::EncryptedDataReset);
            b.usable = false;
            b.verified = false;
            s.root_confirmed = false;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
            let live = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.inbox(id))?;
            super::super::payload_asset::sweep(&self.directory, id, &live)?;
        }
        Ok(())
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Synthetic SDK tests use real retained rows, with an elapsed retention
    /// interval. No binding, ACK or production due-query result is supplied.
    pub fn prepare_zone_deletion_fixture_for_testing(&self, instance: &str) -> Result<Vec<String>, RuntimeError> {
        let now = self.adapter_now()?;
        let seen = now.checked_sub(30 * 86_400_000).ok_or(Error::InvalidData)?;
        let mut zones: Vec<_> = [61, 62, 63]
            .map(|b| super::super::adapter::collection_zone(EpochId([b; 16])))
            .into();
        zones.sort();
        let mut s = self.state();
        Self::caller_locked(&s, instance)?;
        Self::gate_locked(&s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if self.inputs.scope.container.as_str() != "synthetic-u114"
            || zones
                .iter()
                .any(|z| z == &super::super::adapter::collection_zone(b.binding.facts.epoch))
        {
            return Err(Error::InvalidData.into());
        }
        let id = b.binding.id;
        with_tracking_access(|| {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            store.record_publication_zones(id, &zones, seen)?;
            store.mark_publication_zones_due(id, now)
        })?;
        Ok(zones)
    }
}
