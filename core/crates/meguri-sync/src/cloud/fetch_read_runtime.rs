//! P3/V26.1 SDK-bound record-ID inference. A read never stands in for a
//! fetched zone, and a false drain cannot finish an absence pass.
use super::*;
use crate::cloud::{adapter::*, publication::RecordRead};

enum FetchReadCompletion {
    Record(RecordRead),
    NativeMalformed,
}

impl Runtime {
    pub fn adapter_epoch_crt_known(&self) -> bool {
        self.state().bound.as_ref().is_some_and(|b| b.epoch_crt.is_some())
    }
    pub fn epoch_preparation_failure_class(&self, read: &FetchRead) -> Option<policy::OperationError> {
        let s = self.state();
        let b = s.bound.as_ref()?;
        if s.tracking_stopped { return Some(policy::OperationError::TrackingError); }
        if b.binding.id != read.scan.binding || (!b.verified && b.binding.facts.state == BindingState::Active)
            || matches!(b.binding.facts.state, BindingState::Isolated(IsolationReason::SignedOut | IsolationReason::AccountChanged))
        { return Some(policy::OperationError::NoAccount); }
        if matches!(b.binding.facts.state, BindingState::Isolated(IsolationReason::ZoneMissing | IsolationReason::ActiveMissing
            | IsolationReason::EpochSwitched | IsolationReason::ZoneDeletedByUser | IsolationReason::EncryptedDataReset)) {
            return Some(policy::OperationError::ConflictRecovery);
        }
        if b.epoch_crt.is_some_and(|crt| crt != b.current_crt.1) { return Some(policy::OperationError::ConflictRecovery); }
        None
    }
    /// P12.3/V26.1: this capability cannot qualify a fetch/no-state read.
    pub fn adapter_sync_now_epoch_read(&self, sdk: &str, collection: &Collection) -> Result<Option<FetchRead>, RuntimeError> {
        let request = self.admit_sync_now(sdk, collection)?;
        self.sync_now_epoch_read_owned(&request, collection)
    }
    pub fn admit_sync_now(&self, sdk: &str, collection: &Collection) -> Result<SyncNowRequest, RuntimeError> {
        if self.active_review().is_some() {
            return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
        }
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if s.adapter.paused || !b.usable || !b.verified || b.binding.facts.state != BindingState::Active
            || b.epoch_crt.is_some_and(|crt| crt != b.current_crt.1)
        { return Err(RuntimeError::StaleIncarnation); }
        let request = SyncNowRequest { sdk: sdk.into(), guard: Self::adapter_guard_locked(&s),
            scan: ScanGuard { invalidation: s.invalidation, binding: b.binding.id, open: b.open_counter,
                generation: b.evidence.generation, file: b.file, incarnation: s.incarnation } };
        drop(s);
        self.validate_sync_now_request(&request, collection)?;
        Ok(request)
    }
    pub fn validate_sync_now_request(&self, request: &SyncNowRequest, collection: &Collection) -> Result<(), RuntimeError> {
        self.with_scan(&request.scan, collection, |s| {
            Self::sdk_locked(s, &request.sdk)?;
            if Self::adapter_guard_locked(s) != request.guard || s.adapter.paused {
                return Err(RuntimeError::StaleIncarnation);
            }
            Ok(())
        })
    }
    pub fn sync_now_epoch_read_owned(&self, request: &SyncNowRequest, collection: &Collection) -> Result<Option<FetchRead>, RuntimeError> {
        self.validate_sync_now_request(request, collection)?;
        self.sync_now_epoch_read_for_exchange(&request.sdk, collection)
    }
    pub(crate) fn sync_now_epoch_read_for_exchange(&self, sdk: &str, collection: &Collection) -> Result<Option<FetchRead>, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if b.epoch_crt.is_some() { return Ok(None); }
        let read = FetchRead {
            sdk: sdk.into(), scan: ScanGuard {
                invalidation: s.invalidation, binding: b.binding.id, open: b.open_counter,
                generation: b.evidence.generation, file: b.file, incarnation: s.incarnation,
            }, guard: Self::adapter_guard_locked(&s), fetch: None,
            point: s.adapter.no_state_point, zone: collection_zone(b.binding.facts.epoch),
            lost_collection: false, a7_failure: false,
            purpose: FetchReadPurpose::SyncNowEpoch(std::sync::Arc::new(std::sync::Mutex::new(EpochPreparationOutcome::Unknown))),
            after_identity: s.adapter.next_identity, loss_identity_after: s.adapter.next_identity,
        };
        drop(s);
        self.validate_fetch_read(&read, collection)?;
        Ok(Some(read))
    }
    fn epoch_preparation_locked(s: &State, read: &FetchRead) -> Result<(), RuntimeError> {
        if Self::adapter_guard_locked(s) != read.guard || s.tracking_stopped || s.adapter.paused
            || !s.whole_complete || s.scan_required || s.scan_running || s.incoming.running
            || s.actions.exchange.is_some() || Self::publication_allows_send_locked(s)
            || s.adapter.known_loss.is_some()
            || s.adapter.fetch.as_ref().is_some_and(|call| !call.completed) || s.adapter.scheduled.is_some()
        { return Err(RuntimeError::StaleIncarnation); }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        // The missing-zone followup retains this purpose and original guard.
        if read.lost_collection {
            if !read.is_active() || !s.adapter.pending_collection_loss.as_ref().is_some_and(|pending| {
                pending.sdk == read.sdk && pending.guard == read.guard && pending.scan.file == read.scan.file
                    && pending.is_epoch_preparation()
            }) { return Err(RuntimeError::StaleIncarnation); }
        } else if b.epoch_crt.is_some() || read.zone != collection_zone(b.binding.facts.epoch)
            || s.adapter.durable.collection_loss.is_some() || s.adapter.durable.epoch_verification.is_some()
        { return Err(RuntimeError::StaleIncarnation); }
        Ok(())
    }
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
        self.adapter_finalize_fetch(sdk, fetch)?;
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
                purpose: FetchReadPurpose::Fetch,
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
                            root_intake: false,
                            epoch_readability: None,
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
        if read.is_epoch_preparation() {
            return Self::epoch_preparation_locked(s, read);
        }
        if read.is_known_loss() {
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
    pub fn validate_fetch_read_enqueue(&self, read: &FetchRead, collection: &Collection) -> Result<(), RuntimeError> {
        if !collection.is_current() { return Err(RuntimeError::StaleIncarnation); }
        let authority = self.native_authority();
        if authority.guard != read.guard { return Err(RuntimeError::StaleIncarnation); }
        authority.validate_sdk(&read.sdk)
    }
    pub fn validate_fetch_read(&self, read: &FetchRead, collection: &Collection) -> Result<(), RuntimeError> {
        self.with_scan(&read.scan, collection, |s| {
            Self::fetch_read_locked(s, read)?;
            if read.is_epoch_preparation() {
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                let store = s.store.as_ref().ok_or(Error::Database)?;
                if store.publication_op()?.is_some()
                    || store.apply_intents(b.binding.id)?.iter().any(|intent| intent.result_seq.is_none())
                { return Err(RuntimeError::StaleIncarnation); }
            }
            Ok(())
        })
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
        self.complete_fetch_read(read, collection, FetchReadCompletion::Record(result), discard, failure, terminal)
    }
    /// Refusal of an original native reply with an invalid record shape. It
    /// shares ordinary completion's guards and cannot supply record authority.
    pub fn adapter_complete_malformed_fetch_read(
        &self,
        read: &FetchRead,
        collection: &Collection,
        discard: bool,
        failure: Option<crate::cloud::zone_deletion::FailureFacts>,
        terminal: Option<crate::cloud::zone_deletion::FailureFacts>,
    ) -> Result<Option<FetchRead>, RuntimeError> {
        self.complete_fetch_read(read, collection, FetchReadCompletion::NativeMalformed, discard, failure, terminal)
    }
    fn complete_fetch_read(
        &self,
        read: &FetchRead,
        collection: &Collection,
        completion: FetchReadCompletion,
        discard: bool,
        failure: Option<crate::cloud::zone_deletion::FailureFacts>,
        terminal: Option<crate::cloud::zone_deletion::FailureFacts>,
    ) -> Result<Option<FetchRead>, RuntimeError> {
        let (result, native_malformed) = match completion {
            FetchReadCompletion::Record(result) => (result, false),
            FetchReadCompletion::NativeMalformed => (RecordRead::Failed, true),
        };
        // Retain the original owned boundary's cutoff before file validation.
        // Neither network/provider delay can add arrivals to that earlier fetch.
        let reference_through = {
            let s = self.state();
            Self::fetch_read_locked(&s, read).map(|()| {
                s.adapter.fetch.as_ref().filter(|c| Some(c.id) == read.fetch).and_then(|c| c.reference_through)
            })
        };
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
        let reference_through = reference_through?;
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
        if read.is_epoch_preparation() && (native_loss.is_some() || matches!(&result, RecordRead::Missing | RecordRead::ZoneMissing)) {
            read.set_preparation_outcome(EpochPreparationOutcome::Conflict);
        }
        if let Some(reason) = native_loss {
            self.isolate_fetch_read(read, collection, reason)?;
            return Ok(None);
        }
        if native_malformed && read.is_epoch_preparation() {
            // Recheck the original file, account, binding, SDK and read point
            // under the same acceptance scope used for record bodies.
            self.with_scan(&read.scan, collection, |s| {
                Self::fetch_read_locked(s, read)?;
                read.set_preparation_outcome(EpochPreparationOutcome::Conflict);
                Ok(())
            })?;
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
                        root_intake: false,
                        epoch_readability: Some(false),
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
        let mut epoch_readability = None;
        match result {
            RecordRead::Found(record) if read.is_active() => {
                let readable = record
                    .encrypted
                    .as_ref()
                    .is_some_and(|envelope| crate::cloud::control_codec::Active::decode(envelope).is_ok());
                if !readable {
                    self.record_fetch_readability(read, collection, Some(workflow::Saved {
                        name: "active".into(), envelope: record.encrypted.unwrap_or_default(),
                        system_fields: record.system_fields.0,
                    }), None)?;
                    if read.lost_collection {
                        self.isolate_fetch_read(read, collection, IsolationReason::ZoneMissing)?;
                        return Ok(None);
                    }
                    // The typed record establishes zone existence only. The
                    // false drain below grants neither an ACK nor absence.
                } else {
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
            }
            RecordRead::Found(record) => {
                // The issued target already fixes zone/name/type. Qualify its
                // complete encrypted body against the original live binding.
                let s = self.state();
                Self::fetch_read_locked(&s, read)?;
                let binding = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
                if read.is_epoch_preparation() {
                    use crate::cloud::control_codec::{DecodeOutcome, Epoch, EpochState};
                    if let Some(envelope) = &record.encrypted {
                        let outcome = match Epoch::decode(envelope) {
                            Ok(DecodeOutcome::UnsupportedFormat(_)) => EpochPreparationOutcome::Unsupported,
                            Ok(DecodeOutcome::Supported(epoch)) if epoch.epoch_id != binding.facts.epoch.0
                                || epoch.record_key != binding.facts.record_key.0 || record.system_fields.0.is_empty() => EpochPreparationOutcome::Conflict,
                            Ok(DecodeOutcome::Supported(epoch)) if epoch.state == EpochState::Publishing => EpochPreparationOutcome::Unknown,
                            Ok(DecodeOutcome::Supported(_)) => EpochPreparationOutcome::Ready,
                            Err(_) if Epoch::schema_version(envelope).is_ok_and(|ver| ver > 18) => EpochPreparationOutcome::Unsupported,
                            Err(_) => EpochPreparationOutcome::Conflict,
                        };
                        if outcome != EpochPreparationOutcome::Ready {
                            read.set_preparation_outcome(outcome);
                            drop(s);
                            self.record_fetch_readability(read, collection, None, Some(false))?;
                            return Ok(None);
                        }
                    }
                }
                if let Some(envelope) = record.encrypted {
                    fetched_epoch = super::super::epoch_crt_store::fetched(binding, &[workflow::Saved {
                        name: "epoch".into(), envelope, system_fields: record.system_fields.0,
                    }]);
                }
                epoch_readability = Some(fetched_epoch.is_some());
                // P3: typed epoch existence decides the no-state zone read even
                // without authority. Preparation still needs a complete body.
                if read.is_epoch_preparation() && fetched_epoch.is_none() {
                    drop(s);
                    self.record_fetch_readability(read, collection, None, epoch_readability)?;
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
            if read.is_epoch_preparation() {
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                let store = s.store.as_ref().ok_or(Error::Database)?;
                if store.publication_op()?.is_some()
                    || store.apply_intents(b.binding.id)?.iter().any(|intent| intent.result_seq.is_none())
                { return Err(RuntimeError::StaleIncarnation); }
            }
            let mut durable = s.adapter.durable.clone();
            // Only the qualified complete body resolves ordinary verification.
            let resolved_epoch = if fetched_epoch.is_some() {
                durable.epoch_verification.take()
            } else {
                None
            };
            let resolved = resolved_epoch.is_some();
            // A root read can be due for a drain already emitted by a real fetch.
            let new_drain = !read.is_epoch_preparation() && !s
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
            if new_drain || resolved_epoch.is_some() || fetched_epoch.is_some() || epoch_readability.is_some() {
                let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
                #[cfg(feature = "test-hooks")]
                let fail = read.is_epoch_preparation() && std::mem::take(&mut s.adapter.fail_inbox);
                #[cfg(not(feature = "test-hooks"))]
                let fail = false;
                let options = super::super::adapter_store::AdapterCommitOptions {
                    deleted_collection_zones: Vec::new(),
                    at_ms,
                    fail,
                    start_absence: false,
                    resolved_epoch,
                    fetched_epoch,
                    root_intake: false,
                    epoch_readability,
                };
                let store = s.store.as_mut().ok_or(Error::Database)?;
                let committed = if new_drain && !read.is_active() {
                    store.adapter_commit_reference_drain(binding, &durable, options, reference_through.ok_or(Error::InvalidData)?)
                } else {
                    store.adapter_commit(binding, &[], &[], None, &durable, options)
                };
                if let Err(error) = committed {
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
            if read.is_epoch_preparation() {
                let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
                read.set_preparation_outcome(if b.crt_equal() { EpochPreparationOutcome::Ready } else { EpochPreparationOutcome::Conflict });
            } else {
                s.adapter.decided_zones.insert(read.zone.clone());
            }
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
    fn record_fetch_readability(
        &self, read: &FetchRead, collection: &Collection,
        active: Option<workflow::Saved>, epoch_readability: Option<bool>,
    ) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        self.with_scan(&read.scan, collection, |s| {
            Self::fetch_read_locked(s, read)?;
            let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            let mut durable = s.adapter.durable.clone();
            let root_intake = active.is_some();
            if let Some(active) = active { durable.active = Some(active); }
            #[cfg(feature = "test-hooks")]
            let fail = std::mem::take(&mut s.adapter.fail_inbox);
            #[cfg(not(feature = "test-hooks"))]
            let fail = false;
            if let Err(error) = s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                binding, &[], &[], None, &durable,
                super::super::adapter_store::AdapterCommitOptions {
                    deleted_collection_zones: Vec::new(), at_ms, fail, start_absence: false,
                    resolved_epoch: None, fetched_epoch: None, root_intake, epoch_readability,
                },
            ) {
                Self::invalidate_tracking_locked(s);
                return Err(error.into());
            }
            s.adapter.durable = durable;
            Ok(())
        })
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
