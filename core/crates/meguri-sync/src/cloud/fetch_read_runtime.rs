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
                fetch,
                point: s.adapter.no_state_point,
                zone: zone.clone(),
                lost_collection: false,
                a7_failure: s.adapter.no_state_creation && !s.adapter.zone_wills.contains(zone),
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
                            at_ms,
                            fail: false,
                            start_absence: false,
                            resolved_epoch: None,
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
        let call = s.adapter.fetch.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if Self::adapter_guard_locked(s) != read.guard
            || s.adapter.no_state_point != read.point
            || call.id != read.fetch
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
                failure,
                terminal,
            });
        }
        if discard {
            return Ok(None);
        }
        validation?;
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
                        at_ms,
                        fail: false,
                        start_absence: false,
                        resolved_epoch,
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
            return Ok(Some(followup));
        }
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
                // A lost collection result may isolate even with the same
                // active epoch, and an epoch switch must never create a drain.
                if read.lost_collection || self.validate_sdk(&read.sdk).is_err() {
                    return Ok(None);
                }
            }
            RecordRead::Found(_) => {}
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
            // Found on this exact owned epoch target proves existence, even
            // without encrypted fields. Failed/discarded replies never get here.
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
            if new_drain || resolved_epoch.is_some() {
                let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
                if let Err(error) = s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                    binding,
                    &[],
                    &[],
                    None,
                    &durable,
                    super::super::adapter_store::AdapterCommitOptions {
                        at_ms,
                        fail: false,
                        start_absence: false,
                        resolved_epoch,
                    },
                ) {
                    Self::invalidate_tracking_locked(s);
                    return Err(error.into());
                }
                s.adapter.durable = durable;
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
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .isolate_runtime_binding(id, reason)?;
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
            Ok(())
        })
    }
    pub fn adapter_root_read_diagnostics(&self) -> Vec<RootReadDiagnostic> {
        self.state().adapter.root_reads.clone()
    }
}
