//! P3/V26.1 SDK-bound record-ID inference. A read never stands in for a
//! fetched zone, and a false drain cannot finish an absence pass.
use super::*;
use crate::cloud::{adapter::*, publication::RecordRead};

impl Runtime {
    pub fn adapter_fetch_qualified(&self, sdk: &str, fetch: u64) -> Result<bool, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        Ok(s.adapter
            .fetch
            .as_ref()
            .filter(|c| c.id == fetch)
            .ok_or(Error::InvalidData)?
            .completed)
    }
    pub fn adapter_fetch_reads(
        &self,
        sdk: &str,
        fetch: u64,
        collection: &Collection,
    ) -> Result<Vec<FetchRead>, RuntimeError> {
        if !self.adapter_fetch_qualified(sdk, fetch)? {
            return Ok(Vec::new());
        }
        // Reuse the runtime's existing qualification/due path. This fact is
        // permission to read a record, never confirmation of the root.
        self.sdk_fetch_completed_without_root(sdk)?;
        if !self.root_read_due()? {
            return Ok(Vec::new());
        }
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let scan = ScanGuard {
            invalidation: s.invalidation,
            binding: b.binding.id,
            open: b.open_counter,
            generation: b.evidence.generation,
            file: b.file,
            incarnation: s.incarnation,
        };
        let call = s
            .adapter
            .fetch
            .as_ref()
            .filter(|c| c.id == fetch && c.completed)
            .ok_or(Error::InvalidData)?;
        let bound_zone = collection_zone(b.binding.facts.epoch);
        let revalidate_epoch = s.adapter.durable.epoch_verification.is_some();
        let zones: Vec<_> = call
            .scope
            .iter()
            .filter(|zone| {
                (zone.as_str() == "meguri-root" || zone.as_str() == bound_zone.as_str())
                    && !call.read_zones.contains(*zone)
                    && !call.failed_zones.contains(*zone)
                    && (!revalidate_epoch || zone.as_str() == bound_zone.as_str())
                    && ((revalidate_epoch && zone.as_str() == bound_zone.as_str())
                        || (s.adapter.no_state
                            && !s.adapter.decided_zones.contains(*zone)
                            && !call.drains.contains(*zone))
                        || (zone.as_str() == "meguri-root" && !s.root_confirmed && call.drains.contains(*zone)))
            })
            .cloned()
            .collect();
        let reads = zones
            .iter()
            .map(|zone| FetchRead {
                sdk: sdk.into(),
                guard: Self::adapter_guard_locked(&s),
                scan: scan.clone(),
                fetch: Some(fetch),
                point: s.adapter.no_state_point,
                zone: zone.clone(),
                lost_collection: false,
                a7_failure: s.adapter.no_state_creation && !s.adapter.zone_wills.contains(zone),
                known_loss: false,
                after_identity: s.adapter.next_identity,
                loss_identity_after: s.adapter.next_identity,
            })
            .collect::<Vec<_>>();
        s.adapter
            .fetch
            .as_mut()
            .ok_or(Error::InvalidData)?
            .read_zones
            .extend(zones);
        drop(s);
        let at_ms = self.adapter_now()?;
        for read in &reads {
            self.with_scan(&read.scan, collection, |s| {
                Self::fetch_read_locked(s, read)?;
                if read.is_active() {
                    return Ok(());
                }
                let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
                let pending = EpochVerification {
                    binding: binding.clone(),
                    file: read.scan.file,
                };
                if let Some(prior) = &s.adapter.durable.epoch_verification {
                    if prior != &pending {
                        return Err(RuntimeError::StaleIncarnation);
                    }
                } else {
                    let mut durable = s.adapter.durable.clone();
                    durable.epoch_verification = Some(pending);
                    // A separate committed fence precedes the owned probe.
                    // Its outcome transaction cannot roll this obligation back.
                    if let Err(error) = s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                        binding.id,
                        &[],
                        &[],
                        None,
                        &durable,
                        super::super::adapter_store::AdapterCommitOptions {
                            deleted_collection_zones: Vec::new(),
                            at_ms,
                            fail: false,
                            start_absence: false,
                            resolved_epoch: None,
                            fetched_epoch: None,
                        },
                    ) {
                        Self::invalidate_tracking_locked(s);
                        return Err(error.into());
                    }
                    s.adapter.durable = durable;
                }
                Ok(())
            })?;
        }
        Ok(reads)
    }
    pub(super) fn fetch_read_locked(s: &State, read: &FetchRead) -> Result<(), RuntimeError> {
        Self::sdk_locked(s, &read.sdk)?;
        if read.known_loss {
            if Self::adapter_guard_locked(s) != read.guard
                || !read.lost_collection
                || !read.is_active()
                || !s.adapter.pending_collection_loss.as_ref().is_some_and(|pending| {
                    pending.sdk == read.sdk
                        && pending.guard == read.guard
                        && pending.scan.binding == read.scan.binding
                        && pending.scan.incarnation == read.scan.incarnation
                        && pending.scan.file == read.scan.file
                })
                || s.tracking_stopped
                || s.adapter.paused
                || Self::publication_allows_send_locked(s)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            return Ok(());
        }
        let call = s.adapter.fetch.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if Self::adapter_guard_locked(s) != read.guard
            || s.adapter.no_state_point != read.point
            || Some(call.id) != read.fetch
            || !call.completed
            || s.tracking_stopped
            || s.adapter.paused
            || !s.whole_complete
            || s.scan_required
            || s.scan_running
            || Self::publication_allows_send_locked(s)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if !read.is_active() {
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if read.zone != collection_zone(b.binding.facts.epoch)
                || s.adapter
                    .durable
                    .epoch_verification
                    .as_ref()
                    .is_some_and(|pending| pending.binding != b.binding || pending.file != read.scan.file)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        Ok(())
    }
    pub fn validate_fetch_read(&self, read: &FetchRead, collection: &Collection) -> Result<(), RuntimeError> {
        self.with_scan(&read.scan, collection, |s| Self::fetch_read_locked(s, read))
    }
    pub fn adapter_complete_fetch_read(
        &self,
        read: &FetchRead,
        collection: &Collection,
        result: RecordRead,
        discard: bool,
        failure: Option<crate::cloud::zone_deletion::FailureFacts>,
        terminal: Option<crate::cloud::zone_deletion::FailureFacts>,
    ) -> Result<Option<FetchRead>, RuntimeError> {
        let validation = self.validate_fetch_read(read, collection);
        let accepted = !discard && validation.is_ok();
        let outcome = match &result {
            RecordRead::Found(_) => "found",
            RecordRead::Missing => "unknownItem",
            RecordRead::ZoneMissing => "zoneNotFound",
            RecordRead::Failed => "failed",
        };
        {
            let mut s = self.state();
            if s.adapter.root_reads.len() == 64 {
                s.adapter.root_reads.remove(0);
            }
            s.adapter.root_reads.push(RootReadDiagnostic {
                active: read.is_active(),
                lost_collection: read.lost_collection,
                outcome: outcome.into(),
                a7_failure: accepted && !read.is_active() && read.a7_failure && matches!(&result, RecordRead::Found(_)),
                accepted,
                failure: failure.clone(),
                terminal: terminal.clone(),
            });
        }
        if discard {
            return Ok(None);
        }
        validation?;
        #[cfg(feature = "test-hooks")]
        if read.lost_collection {
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::KnownLossBeforeActiveResult);
        }
        let native_loss = failure.iter().chain(terminal.iter()).find_map(|f| match f.code {
            Some(26) if f.encrypted_data_reset => Some(IsolationReason::EncryptedDataReset),
            Some(28) => Some(IsolationReason::ZoneDeletedByUser),
            _ => None,
        });
        if read.is_active() || native_loss.is_some() {
            let s = self.state();
            let request = s
                .adapter
                .accepted_identity_read
                .as_ref()
                .ok_or(RuntimeError::IdentityUnverified)?
                .request;
            Self::fresh_loss_identity_locked(&s, request, read.after_identity)?;
        }
        if let Some(reason) = native_loss {
            self.isolate_fetch_read(read, collection, reason)?;
            return Ok(None);
        }
        if !read.is_active() && matches!(&result, RecordRead::Missing | RecordRead::ZoneMissing) {
            let at_ms = self.adapter_now()?;
            self.with_scan(&read.scan, collection, |s| {
                Self::fetch_read_locked(s, read)?;
                let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
                let mut durable = s.adapter.durable.clone();
                let resolved_epoch = durable.epoch_verification.take();
                durable.collection_loss = Some(CollectionLoss {
                    binding: binding.clone(),
                    file: read.scan.file,
                });
                // with_scan already holds tracking access and rechecks the
                // actual file/publication. Commit before returning the followup;
                // neither SDK serialization nor root ACK is changed here.
                if let Err(error) = s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                    binding.id,
                    &[],
                    &[],
                    None,
                    &durable,
                    super::super::adapter_store::AdapterCommitOptions {
                        deleted_collection_zones: Vec::new(),
                        at_ms,
                        fail: false,
                        start_absence: false,
                        resolved_epoch,
                        fetched_epoch: None,
                    },
                ) {
                    Self::invalidate_tracking_locked(s);
                    return Err(error.into());
                }
                s.adapter.durable = durable;
                s.adapter.pending_collection_loss = Some(read.clone());
                s.root_confirmed = false;
                Ok(())
            })?;
            // K12.5 requires an actual new active read even if this call already
            // read active. No successful root result is invented for this path.
            let mut followup = read.clone();
            followup.zone = "meguri-root".into();
            followup.lost_collection = true;
            followup.a7_failure = false;
            followup.after_identity = self.state().adapter.next_identity;
            followup.loss_identity_after = read.after_identity;
            return Ok(Some(followup));
        }
        let mut fetched_epoch = None;
        match result {
            RecordRead::Found(record) if read.is_active() => {
                let readable = record
                    .encrypted
                    .as_ref()
                    .is_some_and(|envelope| crate::cloud::control_codec::Active::decode(envelope).is_ok());
                if !readable {
                    if read.lost_collection {
                        self.isolate_fetch_read(read, collection, IsolationReason::ZoneMissing)?;
                    }
                    return Ok(None);
                }
                let envelope = record.encrypted.ok_or(Error::InvalidData)?;
                self.accept_fetch_root_read(read, &envelope, &record.system_fields.0)?;
                #[cfg(feature = "test-hooks")]
                if read.lost_collection {
                    self.incoming_hooks
                        .hit(super::super::testing::IncomingStage::KnownLossAfterActiveResult);
                }
                // A lost collection result may isolate even with the same
                // active epoch, and an epoch switch must never create a drain.
                if read.lost_collection || self.validate_sdk(&read.sdk).is_err() {
                    return Ok(None);
                }
            }
            RecordRead::Found(record) => {
                // The issued target already fixes zone/name/type. Qualify its
                // complete encrypted body against the original live binding.
                let s = self.state();
                Self::fetch_read_locked(&s, read)?;
                let binding = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
                if let Some(envelope) = record.encrypted {
                    fetched_epoch = super::super::epoch_crt_store::fetched(binding, &[workflow::Saved {
                        name: "epoch".into(), envelope, system_fields: record.system_fields.0,
                    }]);
                }
                if fetched_epoch.is_none() {
                    return Ok(None);
                }
            }
            RecordRead::Missing | RecordRead::ZoneMissing => {
                self.isolate_fetch_read(read, collection, IsolationReason::ActiveMissing)?;
                return Ok(None);
            }
            RecordRead::Failed if read.lost_collection => {
                self.isolate_fetch_read(read, collection, IsolationReason::ZoneMissing)?;
                return Ok(None);
            }
            RecordRead::Failed => return Ok(None),
        }
        let at_ms = self.adapter_now()?;
        let resolved = self.with_scan(&read.scan, collection, |s| {
            Self::fetch_read_locked(s, read)?;
            let mut durable = s.adapter.durable.clone();
            // Only the qualified complete body resolves ordinary verification.
            let resolved_epoch = if read.is_active() {
                None
            } else {
                durable.epoch_verification.take()
            };
            let resolved = resolved_epoch.is_some();
            // A root read can be due for a drain already emitted by a real fetch.
            let new_drain = !s
                .adapter
                .fetch
                .as_ref()
                .ok_or(Error::InvalidData)?
                .drains
                .contains(&read.zone);
            if new_drain {
                durable.drains.push(FetchDrain {
                    zone: read.zone.clone(),
                    fetched: false,
                });
            }
            if new_drain || resolved_epoch.is_some() || fetched_epoch.is_some() {
                let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
                if let Err(error) = s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                    binding,
                    &[],
                    &[],
                    None,
                    &durable,
                    super::super::adapter_store::AdapterCommitOptions {
                        deleted_collection_zones: Vec::new(),
                        at_ms,
                        fail: false,
                        start_absence: false,
                        resolved_epoch,
                        fetched_epoch,
                    },
                ) {
                    Self::invalidate_tracking_locked(s);
                    return Err(error.into());
                }
                s.adapter.durable = durable;
                Self::refresh_epoch_crt_locked(s)?;
            }
            if new_drain {
                s.adapter
                    .fetch
                    .as_mut()
                    .ok_or(Error::InvalidData)?
                    .drains
                    .insert(read.zone.clone());
                if !read.is_active() {
                    s.publication.cleanup_drain = Some((Self::adapter_guard_locked(s), read.sdk.clone(), at_ms));
                    s.media.observed_now_ms = at_ms;
                }
            }
            s.adapter.decided_zones.insert(read.zone.clone());
            Ok(resolved)
        })?;
        if resolved {
            let mut s = self.state();
            Self::sdk_locked(&s, &read.sdk)?;
            if s.root_confirmed {
                self.emit_pending_locked(&mut s, crate::cloud::workflow::UpdateKind::Full, Default::default())?;
            }
        }
        Ok(None)
    }
    fn isolate_fetch_read(
        &self,
        read: &FetchRead,
        collection: &Collection,
        reason: IsolationReason,
    ) -> Result<(), RuntimeError> {
        self.with_scan(&read.scan, collection, |s| {
            Self::fetch_read_locked(s, read)?;
            let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            Self::commit_service_isolation_locked(s, id, reason)?;
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.binding.facts.state = BindingState::Isolated(reason);
            b.verified = false;
            b.usable = false;
            if read.lost_collection {
                s.adapter.pending_collection_loss = None;
            }
            s.root_confirmed = false;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
            if matches!(
                reason,
                IsolationReason::EncryptedDataReset | IsolationReason::ZoneDeletedByUser
            ) {
                let live = s.store.as_ref().ok_or(Error::Database)?.inbox(id)?;
                super::super::payload_asset::sweep(&self.directory, id, &live)?;
            }
            Ok(())
        })
    }
    pub fn adapter_root_read_diagnostics(&self) -> Vec<RootReadDiagnostic> {
        self.state().adapter.root_reads.clone()
    }
}
