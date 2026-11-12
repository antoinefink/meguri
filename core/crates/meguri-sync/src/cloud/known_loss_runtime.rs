//! K12.5 known native losses. Unaccepted callbacks keep the old opaque cursor;
//! only the existing CollectionLoss shape is needed after a durable acceptance.
use super::*;
use crate::cloud::{adapter::*, adapter_store::AdapterCommitOptions};

impl Runtime {
    // Caller holds State and tracking access. A committed isolation revokes
    // native work before any readback; a tracking failure closes intake too.
    pub(super) fn commit_service_isolation_locked(
        s: &mut State,
        id: BindingId,
        reason: IsolationReason,
    ) -> Result<(), RuntimeError> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .isolate_runtime_binding(id, reason)
        }))
        .unwrap_or(Err(Error::Database));
        if let Err(error) = result {
            Self::invalidate_tracking_locked(s);
            return Err(error.into());
        }
        let b = s
            .bound
            .as_mut()
            .filter(|b| b.binding.id == id)
            .ok_or(RuntimeError::StaleIncarnation)?;
        b.binding.facts.state = BindingState::Isolated(reason);
        b.usable = false;
        b.verified = false;
        s.root_confirmed = false;
        s.publish_authority();
        Ok(())
    }
    pub fn adapter_observe_zone_losses(&self, sdk: &str, losses: &[ZoneLoss]) -> Result<bool, RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let bound = collection_zone(b.binding.facts.epoch);
        // Validate the entire input before changing even unrelated tracking.
        if losses.iter().any(|l| {
            l.zone != "meguri-root"
                && l.zone != "meguri-media"
                && !super::super::publication_store::valid_collection_zone(&l.zone)
        }) {
            return Err(Error::InvalidData.into());
        }
        if losses.iter().all(|l| {
            l.cause != ZoneLossCause::EncryptedDataReset
                && l.zone != "meguri-root"
                && l.zone != "meguri-media"
                && l.zone != bound
        }) {
            drop(s);
            self.adapter_fetched_collection_zone_deletions(
                sdk,
                &losses.iter().map(|l| l.zone.clone()).collect::<Vec<_>>(),
            )?;
            return Ok(false);
        }
        let scan = ScanGuard {
            invalidation: s.invalidation,
            binding: b.binding.id,
            open: b.open_counter,
            generation: b.evidence.generation,
            file: b.file,
            incarnation: s.incarnation,
        };
        let mut all = s
            .adapter
            .known_loss
            .as_ref()
            .map_or_else(Vec::new, |l| l.losses.clone());
        all.extend_from_slice(losses);
        // Every newly delivered fact requires a lookup begun after this delivery,
        // including when an earlier loss already has a resolver in flight.
        let after_read = s.adapter.next_identity;
        s.adapter.known_loss = Some(KnownLoss {
            sdk: sdk.into(),
            scan,
            guard: Self::adapter_guard_locked(&s),
            after_read,
            losses: all,
        });
        // Account-lane completion is a separate admission. No callback waits
        // for an SDK operation or invents a fetch/drain to obtain that proof.
        Ok(true)
    }

    pub fn adapter_accept_zone_losses(&self, request: u64, collection: &Collection) -> Result<(), RuntimeError> {
        let pending = self.state().adapter.known_loss.clone();
        let Some(pending) = pending else {
            return self.recover_collection_loss(request, collection);
        };
        let at_ms = self.adapter_now()?;
        self.with_scan(&pending.scan, collection, |s| {
            Self::sdk_locked(s, &pending.sdk)?;
            let current = s.adapter.known_loss.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if current.guard != pending.guard
                || current.after_read != pending.after_read
                || current.losses != pending.losses
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            Self::fresh_loss_identity_locked(s, request, pending.after_read)?;
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            let binding = b.binding.clone();
            let bound = collection_zone(binding.facts.epoch);
            let own: Vec<_> = pending
                .losses
                .iter()
                .filter(|l| l.zone == "meguri-root" || l.zone == "meguri-media" || l.zone == bound)
                .collect();
            let publisher = Self::publication_allows_send_locked(s);
            let reset = pending
                .losses
                .iter()
                .any(|l| l.cause == ZoneLossCause::EncryptedDataReset);
            let purged = own
                .iter()
                .any(|l| l.zone != "meguri-media" && l.cause == ZoneLossCause::Purged);
            let root = own.iter().any(|l| l.zone == "meguri-root");
            let lost_collection = own.iter().any(|l| l.zone == bound);
            let reason = if reset {
                Some(IsolationReason::EncryptedDataReset)
            } else if purged {
                Some(IsolationReason::ZoneDeletedByUser)
            } else if root && !publisher {
                Some(IsolationReason::ActiveMissing)
            } else if lost_collection && publisher {
                Some(IsolationReason::ZoneMissing)
            } else {
                None
            };
            let other = pending
                .losses
                .iter()
                .filter(|l| l.zone.starts_with("c-") && l.zone != bound)
                .map(|l| l.zone.clone())
                .collect::<Vec<_>>();
            let media = own.iter().any(|l| l.zone == "meguri-media");
            let mut durable = s.adapter.durable.clone();
            if reason == Some(IsolationReason::ActiveMissing) {
                durable.active = None;
                durable.active_deleted = true;
            }
            if lost_collection && reason.is_none() {
                durable.collection_loss = Some(CollectionLoss {
                    binding: binding.clone(),
                    file: pending.scan.file,
                });
            }
            #[cfg(feature = "test-hooks")]
            let fail = std::mem::take(&mut s.adapter.fail_inbox);
            #[cfg(not(feature = "test-hooks"))]
            let fail = false;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::KnownLossBeforeAcceptance);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                s.store.as_mut().ok_or(Error::Database)?.adapter_commit_loss(
                    binding.id,
                    &[],
                    &[],
                    None,
                    &durable,
                    AdapterCommitOptions {
                        deleted_collection_zones: other.clone(),
                        at_ms,
                        fail,
                        start_absence: false,
                        resolved_epoch: None,
                    },
                    Some((reason, media, publisher && root && reason.is_none())),
                )
            }))
            .unwrap_or(Err(Error::Database));
            if let Err(error) = result {
                Self::invalidate_tracking_locked(s);
                return Err(error.into());
            }
            if media {
                s.media.zone_missing = true;
                s.publish_authority();
            }
            if let Some(reason) = reason {
                let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                b.binding.facts.state = BindingState::Isolated(reason);
                b.usable = false;
                b.verified = false;
                s.root_confirmed = false;
                s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
                s.publish_authority();
            }
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::KnownLossAfterAcceptance);
            s.adapter.durable = s.store.as_ref().ok_or(Error::Database)?.adapter_durable(binding.id)?;
            s.adapter.known_loss = None;
            if !other.is_empty() {
                s.zone_listing_fence = Arc::new(());
            }
            if reason.is_some() {
                let updated = s
                    .store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .binding(binding.id)?
                    .ok_or(Error::BindingNotFound)?;
                s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?.binding = updated;
            } else if lost_collection {
                s.root_confirmed = false;
                s.adapter.pending_collection_loss = Some(FetchRead {
                    sdk: pending.sdk.clone(),
                    scan: pending.scan.clone(),
                    guard: pending.guard.clone(),
                    fetch: None,
                    point: s.adapter.no_state_point,
                    zone: "meguri-root".into(),
                    lost_collection: true,
                    a7_failure: false,
                    known_loss: true,
                    after_identity: request,
                    loss_identity_after: pending.after_read,
                });
            }
            // The accepted account read also authorizes its policy projection
            // after this same read committed a service isolation/token change.
            let guard = Self::adapter_guard_locked(s);
            if let Some(proof) = s.adapter.accepted_identity_read.as_mut() {
                proof.guard = guard;
            }
            if matches!(
                reason,
                Some(IsolationReason::EncryptedDataReset | IsolationReason::ZoneDeletedByUser)
            ) {
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(super::super::testing::IncomingStage::KnownLossBeforeCacheSweep);
                let live = s.store.as_ref().ok_or(Error::Database)?.inbox(binding.id)?;
                super::super::payload_asset::sweep(&self.directory, binding.id, &live)?;
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(super::super::testing::IncomingStage::KnownLossAfterCacheSweep);
            }
            Ok(())
        })
    }

    pub(super) fn fresh_loss_identity_locked(s: &State, request: u64, after: u64) -> Result<(), RuntimeError> {
        let proof = s
            .adapter
            .accepted_identity_read
            .as_ref()
            .ok_or(RuntimeError::IdentityUnverified)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if request <= after
            || proof.request != request
            || s.adapter.next_identity != request
            || s.adapter.identity_read.is_some()
            || proof.guard != Self::adapter_guard_locked(s)
            || proof.incarnation != s.incarnation
            || !matches!(&proof.identity, IdentityStatus::Available(account) if account == &b.binding.facts.account)
            || b.binding.facts.state != BindingState::Active
        {
            return Err(RuntimeError::IdentityUnverified);
        }
        Ok(())
    }
    pub fn adapter_known_loss_read_after_identity(
        &self,
        sdk: &str,
        request: u64,
    ) -> Result<Option<FetchRead>, RuntimeError> {
        self.with_accepted_identity_read(request, |_| ())?;
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let proof = s
            .adapter
            .accepted_identity_read
            .as_ref()
            .ok_or(RuntimeError::IdentityUnverified)?;
        if proof.request != request
            || s.adapter.next_identity != request
            || s.adapter.identity_read.is_some()
            || proof.guard != Self::adapter_guard_locked(&s)
            || proof.incarnation != s.incarnation
            || s.adapter.known_loss.is_some()
        {
            return Err(RuntimeError::IdentityUnverified);
        }
        Ok(s.adapter
            .pending_collection_loss
            .as_ref()
            .filter(|r| r.sdk == sdk)
            .cloned())
    }
    pub fn adapter_known_loss_read(&self, sdk: &str) -> Result<Option<FetchRead>, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        Ok(s.adapter
            .pending_collection_loss
            .as_ref()
            .filter(|r| r.sdk == sdk && r.known_loss)
            .cloned())
    }
    fn recover_collection_loss(&self, request: u64, collection: &Collection) -> Result<(), RuntimeError> {
        if self.state().adapter.durable.collection_loss.is_none() {
            return Ok(());
        }
        let file = self.inputs.files.identity(collection.path())?;
        let publication = meguri_engine::device_state::DeviceState::for_collection(collection.path()).publication()?;
        let mut s = self.state();
        let Some(loss) = s.adapter.durable.collection_loss.as_ref() else {
            return Ok(());
        };
        // Live SDK recovery belongs to its issued Active read, never this path.
        if s.adapter.pending_collection_loss.is_some() {
            return Ok(());
        }
        Self::fresh_loss_identity_locked(&s, request, 0)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if loss.binding != b.binding
            || loss.file != file
            || b.file != file
            || !collection.is_current()
            || collection.generation() != b.evidence.generation
            || !publication.is_some_and(|p| {
                p.identity == b.binding.facts.publication
                    && p.decision == meguri_engine::device_state::PublicationDecision::Clear
            })
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let id = b.binding.id;
        with_tracking_access(|| Self::commit_service_isolation_locked(&mut s, id, IsolationReason::ZoneMissing))?;
        let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
        b.binding.facts.state = BindingState::Isolated(IsolationReason::ZoneMissing);
        b.usable = false;
        b.verified = false;
        s.root_confirmed = false;
        s.publish_authority();
        let binding = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.binding(id))?
            .ok_or(Error::BindingNotFound)?;
        s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?.binding = binding;
        Ok(())
    }
}
