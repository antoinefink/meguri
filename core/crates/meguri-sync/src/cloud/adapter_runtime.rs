use super::{super::adapter_store::AdapterCommitOptions, *};
use crate::cloud::policy::PolicyWork;
use crate::cloud::{adapter::*, workflow::*};
use std::collections::{BTreeMap, BTreeSet};
impl Runtime {
    pub fn attach_adapter_clock(&self, clock: Arc<dyn meguri_model::clock::Clock>) {
        *self.adapter_clock.lock().unwrap_or_else(PoisonError::into_inner) = Some(clock);
    }
    pub(super) fn adapter_now(&self) -> Result<i64, RuntimeError> {
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let now = clock.now_ms();
        self.pending_clock.store(now, std::sync::atomic::Ordering::Relaxed);
        Ok(now)
    }
    pub(super) fn adapter_guard_locked(s: &State) -> Guard {
        Guard {
            generation: s.bound.as_ref().map_or(0, |b| b.evidence.generation.0),
            binding: s.bound.as_ref().map_or_else(Vec::new, |b| b.binding.id.0.to_vec()),
            token: s.adapter.token.saturating_add(s.invalidation),
        }
    }
    pub fn adapter_guard(&self) -> Guard {
        Self::adapter_guard_locked(&self.state())
    }
    pub fn register_sdk(&self, automatic: bool) -> Result<(String, Option<Vec<u8>>, String), RuntimeError> {
        let _ = self.adapter_now();
        let mut s = self.state();
        self.register_sdk_locked(&mut s, automatic, &mut PolicyWork::default())
    }
    /// Registration is the linearization point, after all native preparation awaits.
    /// The issued guard is compared in the same critical section as installation.
    pub fn register_sdk_guarded(
        &self,
        guard: &Guard,
        automatic: bool,
    ) -> Result<(String, Option<Vec<u8>>, String), RuntimeError> {
        self.register_sdk_guarded_with_work(guard, automatic, &mut PolicyWork::default())
    }
    pub fn register_sdk_guarded_with_work(
        &self,
        guard: &Guard,
        automatic: bool,
        work: &mut PolicyWork,
    ) -> Result<(String, Option<Vec<u8>>, String), RuntimeError> {
        let _ = self.adapter_now();
        let mut s = self.state();
        work.check()?;
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        self.register_sdk_locked(&mut s, automatic, work)
    }
    fn register_sdk_locked(
        &self,
        s: &mut State,
        automatic: bool,
        work: &mut PolicyWork,
    ) -> Result<(String, Option<Vec<u8>>, String), RuntimeError> {
        if s.adapter.sdk.is_some() {
            return Err(Error::InvalidData.into());
        }
        let host = Self::instance_locked(s)?;
        Self::caller_locked(s, &host)?;
        if !s.whole_complete || s.scan_required || s.tracking_stopped || s.adapter.paused {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.clone();
        let (serialization, durable) = with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            // All registration preconditions precede the first durable effect.
            // In particular, waiting for State or tracking access is cancellable.
            work.effect()?;
            store.adapter_install()?;
            let live = store.inbox(b.id)?;
            super::super::payload_asset::sweep(&self.directory, b.id, &live)?;
            let leased = s
                .actions
                .payload_uploads
                .values()
                .flatten()
                .map(|path| path.to_path_buf())
                .collect();
            super::super::payload_asset::sweep_uploads(&self.directory, b.id, &leased)?;
            Ok((
                store.engine_state(b.id)?.map(|v| v.serialization.0),
                store.adapter_durable(b.id)?,
            ))
        })?;
        s.adapter.durable = durable;
        if let Some(pending) = &s.adapter.durable.epoch_verification {
            let bound = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if pending.binding != bound.binding || pending.file != bound.file {
                return Err(RuntimeError::StaleIncarnation);
            }
            s.root_confirmed = false;
        }
        s.adapter.next_sdk = s.adapter.next_sdk.checked_add(1).ok_or(Error::InvalidData)?;
        let id = format!("sdk:{host}:{}", s.adapter.next_sdk);
        if s.adapter.next_sdk > 1 {
            s.root_confirmed = false;
        }
        s.adapter.cursorless_first_zone = serialization.is_none();
        s.adapter.no_state = serialization.is_none();
        s.adapter.no_state_creation = serialization.is_none();
        s.adapter.zone_wills.clear();
        s.adapter.decided_zones.clear();
        s.adapter.fetch_groups.clear();
        s.adapter.no_state_point += 1;
        s.adapter.fetch = None;
        s.adapter.scheduled = None;
        s.policy.flights.clear();
        s.actions.request_flights.clear();
        s.policy.parked.clear();
        s.policy.held_images.clear();
        s.actions.automatic = automatic;
        s.actions.handler = None;
        #[cfg(feature = "test-hooks")]
        meguri_model::faultpoint!("cloud.adapter.register_sdk.before_pending").map_err(Error::from)?;
        self.emit_pending_locked(s, UpdateKind::Full, BTreeSet::new())?;
        // Failed pending discovery must leave the SDK slot available for retry.
        s.adapter.sdk = Some(SDKAuthority {
            id: id.clone(),
            guard: Self::adapter_guard_locked(s),
            stopping: false,
        });
        Ok((id, serialization, collection_zone(b.facts.epoch)))
    }
    pub(crate) fn policy_sdk_origin(&self, guard: &Guard) -> Result<Option<String>, RuntimeError> {
        let s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(s.adapter.sdk.as_ref().map(|sdk| sdk.id.clone()))
    }
    pub(crate) fn policy_fetch_scope(
        &self,
        guard: &Guard,
        zones: &[crate::cloud::policy::Zone],
    ) -> Result<Vec<String>, RuntimeError> {
        let s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        let binding = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
        zones
            .iter()
            .map(|zone| match zone {
                crate::cloud::policy::Zone::Root => Ok("meguri-root".to_owned()),
                crate::cloud::policy::Zone::Collection => Ok(collection_zone(binding.facts.epoch)),
                crate::cloud::policy::Zone::Media => Err(Error::InvalidData.into()),
            })
            .collect()
    }
    pub fn sdk_root_confirmed(&self) -> bool {
        self.state().root_confirmed
    }
    pub fn validate_sdk(&self, sdk: &str) -> Result<(), RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)
    }
    pub(super) fn sdk_locked(s: &State, sdk: &str) -> Result<(), RuntimeError> {
        if !s
            .adapter
            .sdk
            .as_ref()
            .is_some_and(|a| a.id == sdk && !a.stopping && a.guard == Self::adapter_guard_locked(s))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Self::caller_locked(s, sdk)
    }
    /// Re-creation invalidates the SDK authority without changing the policy guard.
    pub fn retire_sdk(&self, sdk: &str, policy_stop: bool) -> Result<(), RuntimeError> {
        self.retire_sdk_with_work(sdk, policy_stop, &mut PolicyWork::default())
    }
    pub fn retire_sdk_with_work(
        &self,
        sdk: &str,
        policy_stop: bool,
        work: &mut PolicyWork,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        work.check()?;
        if !s.adapter.sdk.as_ref().is_some_and(|a| a.id == sdk) {
            return Ok(());
        }
        if s.adapter
            .pending_collection_loss
            .as_ref()
            .is_some_and(|read| read.sdk == sdk)
        {
            // Probe outside both writer guards, then recheck the original
            // binding/incarnation under the same lock that retires authority.
            drop(s);
            let path = self.directory.join("collection.anki2");
            let file = self.inputs.files.identity(&path)?;
            let publication = meguri_engine::device_state::DeviceState::for_collection(&path).publication()?;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::SDKRetireBeforeReacquisition);
            s = self.state();
            work.check()?;
            if !s.adapter.sdk.as_ref().is_some_and(|a| a.id == sdk) {
                return Ok(());
            }
            if let Some(read) = s
                .adapter
                .pending_collection_loss
                .as_ref()
                .filter(|read| read.sdk == sdk)
                .cloned()
            {
                let scan = &read.scan;
                let original = s.incarnation == scan.incarnation
                    && s.invalidation == scan.invalidation
                    && !s.invalidation_exhausted
                    && file == scan.file
                    && file.created_nanos < 1_000_000_000
                    && s.bound.as_ref().is_some_and(|b| {
                        (b.binding.id, b.open_counter, b.evidence.generation, b.file)
                            == (scan.binding, scan.open, scan.generation, scan.file)
                            && b.binding.facts.state == BindingState::Active
                            && publication.as_ref().is_some_and(|p| {
                                p.identity == b.binding.facts.publication
                                    && p.decision == meguri_engine::device_state::PublicationDecision::Clear
                            })
                    });
                if original {
                    // Existing isolation precedence and triggers preserve pending
                    // local rows and clear absence state. Never overwrite a
                    // stronger current reason with this service fallback.
                    let (binding, durable) = with_tracking_access(|| {
                        let store = s.store.as_mut().ok_or(Error::Database)?;
                        work.effect()?;
                        store.isolate_runtime_binding(scan.binding, IsolationReason::ZoneMissing)?;
                        // Isolation has committed. Failed readback cannot restore
                        // authority to this already isolated binding.
                        let b = s.bound.as_mut().ok_or(Error::BindingNotFound)?;
                        b.usable = false;
                        b.verified = false;
                        s.publish_authority();
                        #[cfg(feature = "test-hooks")]
                        self.incoming_hooks
                            .hit(super::super::testing::IncomingStage::SDKIsolationPublished);
                        #[cfg(feature = "test-hooks")]
                        if self
                            .incoming_hooks
                            .retire_readback_failure
                            .swap(false, std::sync::atomic::Ordering::SeqCst)
                        {
                            return Err(RuntimeError::from(Error::Database));
                        }
                        let store = s.store.as_ref().ok_or(Error::Database)?;
                        Ok::<_, RuntimeError>((
                            store.binding(scan.binding)?.ok_or(Error::BindingNotFound)?,
                            store.adapter_durable(scan.binding)?,
                        ))
                    })?;
                    s.adapter.durable = durable;
                    let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                    b.binding = binding;
                    b.usable = false;
                    b.verified = false;
                    s.root_confirmed = false;
                }
                work.effect()?;
                s.adapter.pending_collection_loss = None;
            }
        }
        let authority = s
            .adapter
            .sdk
            .as_mut()
            .filter(|a| a.id == sdk)
            .ok_or(RuntimeError::StaleIncarnation)?;
        work.effect()?;
        authority.stopping = true;
        s.publish_authority();
        s.policy.flights.retain(|(owner, _), _| owner != sdk);
        if policy_stop {
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
        }
        Ok(())
    }
    pub fn release_sdk(&self, sdk: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.adapter.sdk.as_ref().is_some_and(|a| a.id == sdk && a.stopping) {
            // Release follows the original SDK's cancellation/drain, including
            // any late callbacks. Retiring alone cannot release upload files.
            s.actions.request_flights.retain(|(owner, _), _| owner != sdk);
            s.actions.payload_uploads.retain(|(owner, _), _| owner != sdk);
            s.actions.payload_upload_sends.retain(|(owner, _), _| owner != sdk);
            s.actions.payload_handlers_returned.retain(|(owner, _)| owner != sdk);
            // This sole installed SDK has completed cancellation/drain; its
            // explicit sends cannot become dependencies of a successor batch.
            for call in &mut s.actions.calls {
                call.returned = true;
            }
            s.adapter.sdk = None;
            s.actions.handler = None;
            s.adapter.fetch = None;
            s.adapter.scheduled = None;
        }
        Ok(())
    }
    pub fn adapter_pause(&self, paused: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if s.adapter.paused != paused {
            s.adapter.paused = paused;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
        }
        Ok(())
    }
    pub fn adapter_stop_syncing(&self) -> Result<(), RuntimeError> {
        let mut s = self.state();
        let id = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
        s.publish_authority();
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .isolate_runtime_binding(id, IsolationReason::Stopped)
        })?;
        if let Some(b) = s.bound.as_mut() {
            b.usable = false;
            b.verified = false;
            b.binding.facts.state = BindingState::Isolated(IsolationReason::Stopped);
        }
        Ok(())
    }
    pub fn adapter_account(&self, sdk: Option<&str>, identity: IdentityStatus) -> Result<(), RuntimeError> {
        self.adapter_account_with_work(sdk, identity, &mut PolicyWork::default())
    }
    pub fn adapter_account_with_work(
        &self,
        sdk: Option<&str>,
        identity: IdentityStatus,
        work: &mut PolicyWork,
    ) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        self.wait_incoming_idle();
        let mut s = self.state();
        if let Some(sdk) = sdk {
            Self::sdk_locked(&s, sdk)?;
        }
        work.effect()?;
        Self::adapter_account_locked(self, &mut s, identity, sdk.is_some(), false)
    }
    fn adapter_account_locked(
        &self,
        s: &mut State,
        identity: IdentityStatus,
        sdk_event: bool,
        no_collection_authorized: bool,
    ) -> Result<(), RuntimeError> {
        // Connection discovery has no binding yet. Accepted account events
        // still invalidate its preview; no account or binding is invented.
        if s.bound.is_none() {
            if s.store.is_none() && !no_collection_authorized {
                return Err(RuntimeError::StaleIncarnation);
            }
            s.publication.preview = None;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
            return Ok(());
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        // A local resume rebinds this same owner as unverified. An actual
        // same-account proof restores verification without revoking the SDK
        // guard whose original serial callback is held across suspension.
        let same = matches!(&identity, IdentityStatus::Available(a) if a == &b.binding.facts.account)
            && (b.verified
                || s.adapter
                    .sdk
                    .as_ref()
                    .is_some_and(|sdk| !sdk.stopping && sdk.guard == Self::adapter_guard_locked(s)));
        if !same {
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
        }
        let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
        let isolation = match identity {
            IdentityStatus::Available(a) if a == b.binding.facts.account => {
                b.verified = b.usable;
                None
            }
            IdentityStatus::Available(_) => Some(IsolationReason::AccountChanged),
            IdentityStatus::SignedOut => Some(IsolationReason::SignedOut),
            IdentityStatus::SwitchedAccounts => Some(IsolationReason::AccountChanged),
            _ => {
                b.verified = false;
                None
            }
        };
        if let Some(reason) = isolation {
            b.verified = false;
            b.usable = false;
            b.binding.facts.state = BindingState::Isolated(reason);
            let id = b.binding.id;
            s.publish_authority();
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::AccountIsolationPublished);
            with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .isolate_runtime_binding(id, reason)
            })?;
        }
        s.publish_authority();
        if same && sdk_event {
            s.adapter.no_state = true;
            s.adapter.no_state_creation = false;
            s.adapter.decided_zones.clear();
            if let Some(call) = s.adapter.fetch.as_mut() {
                call.drains.clear();
                call.read_zones.clear();
            }
            s.adapter.no_state_point += 1;
            self.emit_pending_locked(s, UpdateKind::Full, BTreeSet::new())?;
        }
        Ok(())
    }
    pub(super) fn adapter_snapshot_locked(s: &State) -> Result<Snapshot, RuntimeError> {
        let Some(b) = s.bound.as_ref() else {
            return s.adapter.closed_snapshot.clone().ok_or(RuntimeError::StaleIncarnation);
        };
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut entities = Vec::new();
            for kind in crate::cloud::row_payload::KINDS {
                let mut after = None;
                loop {
                    let page = store.entity_page(b.binding.id, kind, after.as_deref())?;
                    if page.is_empty() {
                        break;
                    }
                    after = page.last().map(|e| e.key.clone());
                    entities.extend(page);
                }
            }
            let runtime = store.stored_runtime_state(b.binding.id)?;
            let durable = store.adapter_durable(b.binding.id)?;
            let inbox = store.inbox(b.binding.id)?;
            let indexed_inbox = inbox.iter().map(|e| (e.id, &e.entry)).collect::<Vec<_>>();
            let tracking_content = crate::cloud::store::encode(&(
                store.binding(b.binding.id)?.map(|b| (b.id, b.created_at_ms, b.facts)),
                &runtime,
                &entities,
                indexed_inbox,
                &durable,
                store
                    .engine_state(b.binding.id)?
                    .map(|e| (e.serialization, e.saved_at_ms)),
            ))?
            .into_bytes();
            Ok(Snapshot {
                binding: b.binding.id.0.to_vec(),
                tracking_content,
                serialization: store.engine_state(b.binding.id)?.map(|s| s.serialization.0),
                entities,
                inbox: inbox.into_iter().map(|e| e.entry).collect(),
                root: runtime.and_then(|v| v.root_ack),
                tracking_stopped: s.tracking_stopped,
                absence_pass_completed: store.absence_completed(b.binding.id, &durable)?,
            })
        })
    }
    pub fn retain_adapter_inspection(&self) {
        self.state().adapter.retain_inspection = true;
    }
    pub fn adapter_snapshot(&self) -> Result<Snapshot, RuntimeError> {
        Self::adapter_snapshot_locked(&self.state())
    }
    pub(super) fn adapter_commit_locked(
        s: &mut State,
        entries: &[InboxEntry],
        refused: &[super::super::incoming::RefusedEntry],
        serialization: Option<&[u8]>,
        options: AdapterCommitOptions,
    ) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| {
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                s.store.as_mut().ok_or(Error::Database)?.adapter_commit(
                    b,
                    entries,
                    refused,
                    serialization,
                    &s.adapter.durable,
                    options,
                )
            }))
            .unwrap_or(Err(Error::Database))
        });
        if result.is_err() {
            Self::invalidate_locked(s);
        }
        result.map_err(Into::into)
    }
    pub fn adapter_state_update(&self, sdk: &str, serialization: &[u8]) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        #[cfg(feature = "test-hooks")]
        let fail = std::mem::take(&mut s.adapter.fail_state);
        #[cfg(not(feature = "test-hooks"))]
        let fail = false;
        Self::adapter_commit_locked(
            &mut s,
            &[],
            &[],
            Some(serialization),
            AdapterCommitOptions {
                at_ms,
                fail,
                start_absence: false,
                resolved_epoch: None,
            },
        )
    }
    pub fn adapter_fetched(
        &self,
        sdk: &str,
        zone: &str,
        records: &[Saved],
        deleted: &[String],
    ) -> Result<(), RuntimeError> {
        let inputs: Vec<_> = records
            .iter()
            .map(|record| FetchedRecord {
                record: record.clone(),
                asset_path: None,
            })
            .collect();
        self.adapter_fetched_assets(sdk, zone, &inputs, deleted)
    }
    pub fn adapter_fetched_assets(
        &self,
        sdk: &str,
        zone: &str,
        records: &[FetchedRecord],
        deleted: &[String],
    ) -> Result<(), RuntimeError> {
        let guard = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            Self::adapter_guard_locked(&s)
        };
        let result = self.adapter_fetched_owned(sdk, zone, records, deleted);
        if result.is_err() {
            let mut s = self.state();
            // A failed intake closes only the adapter that supplied it. A
            // successor must never inherit this callback's failure authority.
            if Self::adapter_guard_locked(&s) == guard && Self::sdk_locked(&s, sdk).is_ok() {
                Self::invalidate_locked(&mut s);
            }
        }
        result
    }
    fn adapter_fetched_owned(
        &self,
        sdk: &str,
        zone: &str,
        inputs: &[FetchedRecord],
        deleted: &[String],
    ) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let (key, epoch, binding, guard) = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            let b = &s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding;
            (
                b.facts.record_key.0,
                b.facts.epoch.0,
                b.id,
                Self::adapter_guard_locked(&s),
            )
        };
        let records: Vec<_> = inputs.iter().map(|input| input.record.clone()).collect();
        if zone == "meguri-root" {
            // A root proof is accepted at its drained fetch, never from an old callback.
            if inputs.iter().any(|input| input.asset_path.is_some()) {
                return Err(Error::InvalidData.into());
            }
            for r in &records {
                if r.name != "active" {
                    return Err(RuntimeError::Incomplete(Incomplete::RootConfirmationAndSending));
                }
                crate::cloud::control_codec::Active::decode(&r.envelope).map_err(|_| Error::InvalidData)?;
            }
        } else if zone != collection_zone(EpochId(epoch)) {
            return Err(RuntimeError::Incomplete(Incomplete::WholeBindingReconciliation));
        }
        if zone != "meguri-root" {
            meguri_model::faultpoint!("cloud.fetch.before_inbox").map_err(Error::from)?;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::FetchBeforeInbox);
        }
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        let intake = if zone != "meguri-root" {
            Some(super::super::payload_asset::intake(
                &self.directory,
                binding,
                &key,
                inputs,
                at_ms,
            )?)
        } else {
            None
        };
        let mut entries = intake.as_ref().map_or_else(Vec::new, |intake| intake.entries.clone());
        let refused = intake.as_ref().map_or_else(Vec::new, |intake| intake.refused.clone());
        if zone != "meguri-root" {
            for name in deleted {
                entries.push(InboxEntry {
                    record_type: RecordType::Entity,
                    record_name: name.clone(),
                    value: InboxValue::Deletion,
                    reason: InboxReason::Ready,
                    source: InboxSource::Fetch,
                    absence_pass: None,
                    retry_count: 0,
                    fetched_at_ms: at_ms,
                });
            }
        }
        #[cfg(feature = "test-hooks")]
        let fail = std::mem::take(&mut s.adapter.fail_inbox);
        #[cfg(not(feature = "test-hooks"))]
        let fail = false;
        let previous = s.adapter.durable.clone();
        if zone == "meguri-root" {
            if deleted.iter().any(|name| name == "active") {
                s.adapter.durable.active = None;
                s.adapter.durable.active_deleted = true;
            } else if !s.adapter.durable.active_deleted
                && let Some(record) = records.last()
            {
                s.adapter.durable.active = Some(record.clone());
            }
        }
        if let Err(error) = Self::adapter_commit_locked(
            &mut s,
            &entries,
            &refused,
            None,
            AdapterCommitOptions {
                at_ms,
                fail,
                start_absence: false,
                resolved_epoch: None,
            },
        ) {
            s.adapter.durable = previous;
            return Err(error);
        }
        if let Some(intake) = intake {
            with_tracking_access(|| -> Result<(), Error> {
                let live = s.store.as_ref().ok_or(Error::Database)?.inbox(binding)?;
                intake.commit(&live)?;
                super::super::payload_asset::sweep(&self.directory, binding, &live)
            })?;
        }
        if zone != "meguri-root" {
            s.incoming.retry_granted = true;
        }
        if s.adapter.durable.active_deleted {
            s.root_confirmed = false;
            s.adapter.token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            s.publish_authority();
            if let Some(b) = s.bound.as_mut() {
                b.verified = false;
                b.usable = false;
                b.binding.facts.state = BindingState::Isolated(IsolationReason::ActiveMissing);
            }
        }
        drop(s);
        if zone != "meguri-root" {
            meguri_model::faultpoint!("cloud.fetch.after_inbox").map_err(Error::from)?;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(super::super::testing::IncomingStage::FetchAfterInbox);
        }
        Ok(())
    }
    pub fn adapter_begin_fetch(&self, sdk: &str, zones: Vec<String>) -> Result<u64, RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if s.adapter.fetch.is_some() {
            return Err(Error::InvalidData.into());
        }
        s.adapter.next_fetch += 1;
        let id = s.adapter.next_fetch;
        let boundary = s.adapter.scheduled.take();
        s.adapter.fetch_groups.insert(format!("meguri-{id}"));
        s.adapter.fetch = Some(FetchCall {
            id,
            scope: zones,
            own_eligible: boundary.is_none(),
            boundary,
            ..Default::default()
        });
        Ok(id)
    }
    pub fn adapter_will_fetch(&self, sdk: &str, group: String, manual: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let owns_call = s.adapter.fetch.as_ref().is_some_and(|call| {
            manual
                && call.own_eligible
                && (group == format!("meguri-{}", call.id) || !s.adapter.fetch_groups.contains(&group))
        });
        let boundary = FetchBoundary {
            group,
            manual,
            owns_call,
            ..Default::default()
        };
        if let Some(call) = s.adapter.fetch.as_mut() {
            if call.boundary.take().is_some() {
                s.adapter.missing_did += 1;
            }
            s.adapter.fetch.as_mut().ok_or(Error::InvalidData)?.boundary = Some(boundary);
        } else {
            s.adapter.scheduled = Some(boundary);
        }
        Ok(())
    }
    pub fn adapter_fetch_zone(&self, sdk: &str, zone: String, will: bool, failed: bool) -> Result<(), RuntimeError> {
        // Foreign clock/random providers may reenter: read them outside both
        // guards, then recheck the original SDK and creation fact before writing.
        let starts_pass = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            will && s.adapter.cursorless_first_zone
                && s.bound
                    .as_ref()
                    .is_some_and(|b| collection_zone(b.binding.facts.epoch) == zone)
                && (s.adapter.fetch.as_ref().is_some_and(|c| c.boundary.is_some()) || s.adapter.scheduled.is_some())
        };
        let fresh = if starts_pass {
            Some((self.adapter_now()?, self.publication_random_id()?))
        } else {
            None
        };
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if s.adapter.cursorless_first_zone
            && let Some((at_ms, random)) = fresh
        {
            use sha2::{Digest, Sha256};
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            let prior = s.adapter.durable.clone();
            s.adapter.durable.absence_sequence = s
                .adapter
                .durable
                .absence_sequence
                .checked_add(1)
                .ok_or(Error::InvalidData)?;
            let mut hash = Sha256::new();
            hash.update(b.0);
            hash.update(random);
            hash.update(s.adapter.durable.absence_sequence.to_be_bytes());
            let mut pass = [0; 16];
            pass.copy_from_slice(&hash.finalize()[..16]);
            s.adapter.durable.absence = Some(AbsenceLedger {
                pass,
                completed: false,
                zone: zone.clone(),
                started_at_ms: at_ms,
            });
            if let Err(e) = Self::adapter_commit_locked(
                &mut s,
                &[],
                &[],
                None,
                AdapterCommitOptions {
                    at_ms,
                    fail: false,
                    start_absence: true,
                    resolved_epoch: None,
                },
            ) {
                s.adapter.durable = prior;
                return Err(e);
            }
            s.adapter.cursorless_first_zone = false;
        }
        if will {
            s.adapter.zone_wills.insert(zone.clone());
        }
        if !will
            && failed
            && let Some(call) = s.adapter.fetch.as_mut()
        {
            call.failed_zones.insert(zone.clone());
        }
        let point = s.adapter.no_state_point;
        let boundary = if let Some(call) = s.adapter.fetch.as_mut() {
            call.boundary.as_mut()
        } else {
            s.adapter.scheduled.as_mut()
        };
        if let Some(boundary) = boundary {
            if will {
                boundary.zones.insert(zone, (point, false));
            } else if let Some((started, done)) = boundary.zones.get_mut(&zone) {
                *done = !failed && *started == point;
            }
        }
        Ok(())
    }
    pub fn adapter_did_fetch(&self, sdk: &str, group: &str, manual: bool) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let verification_guard = {
            let s = self.state();
            Self::sdk_locked(&s, sdk)?;
            s.adapter
                .durable
                .epoch_verification
                .as_ref()
                .map(|_| (Self::adapter_guard_locked(&s), s.adapter.no_state_point))
        };
        let verification_file = if verification_guard.is_some() {
            let path = self.directory.join("collection.anki2");
            Some((
                self.inputs.files.identity(&path)?,
                meguri_engine::device_state::DeviceState::for_collection(&path).publication()?,
            ))
        } else {
            None
        };
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if verification_guard.as_ref().is_some_and(|(guard, point)| {
            *guard != Self::adapter_guard_locked(&s) || *point != s.adapter.no_state_point
        }) {
            return Err(RuntimeError::StaleIncarnation);
        }
        // While the loss obligation awaits its Active reply, a cached root or
        // unchanged fetch cannot confirm it, record a drain, or finish absence.
        if s.adapter.durable.collection_loss.is_some() {
            return Ok(());
        }
        let boundary = if s.adapter.fetch.is_some() {
            s.adapter.fetch.as_mut().and_then(|c| c.boundary.take())
        } else {
            s.adapter.scheduled.take()
        };
        let Some(boundary) = boundary else { return Ok(()) };
        if boundary.group != group || boundary.manual != manual {
            return Ok(());
        }
        let scope = s.adapter.fetch.as_ref().map(|c| c.scope.clone());
        let owns_call = boundary.owns_call;
        let drains = if let Some(scope) = scope.filter(|_| owns_call) {
            scope
                .into_iter()
                .filter_map(|zone| match boundary.zones.get(&zone) {
                    Some((started, true)) if *started == s.adapter.no_state_point => {
                        Some(FetchDrain { zone, fetched: true })
                    }
                    None if !s.adapter.no_state || s.adapter.decided_zones.contains(&zone) => {
                        Some(FetchDrain { zone, fetched: false })
                    }
                    _ => None,
                })
                .collect::<Vec<_>>()
        } else {
            boundary
                .zones
                .into_iter()
                .filter_map(|(zone, (started, done))| {
                    (done && started == s.adapter.no_state_point).then_some(FetchDrain { zone, fetched: true })
                })
                .collect()
        };
        let resolved_epoch = if let Some(pending) = &s.adapter.durable.epoch_verification {
            let bound = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            let zone = collection_zone(bound.binding.facts.epoch);
            if !drains.iter().any(|d| d.zone == zone && d.fetched) {
                if let Some(call) = s.adapter.fetch.as_mut() {
                    call.completed = owns_call;
                }
                return Ok(());
            }
            if pending.binding != bound.binding
                || pending.file != bound.file
                || !verification_file.as_ref().is_some_and(|(file, publication)| {
                    *file == pending.file
                        && file.created_nanos < 1_000_000_000
                        && publication.as_ref().is_some_and(|p| {
                            p.identity == bound.binding.facts.publication
                                && p.decision == meguri_engine::device_state::PublicationDecision::Clear
                        })
                })
                || s.tracking_stopped
                || s.adapter.paused
                || !s.whole_complete
                || s.scan_required
                || s.scan_running
                || Self::publication_allows_send_locked(&s)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            Some(pending.clone())
        } else {
            None
        };
        if resolved_epoch.is_none() && drains.iter().any(|d| d.zone == "meguri-root") {
            let root = if s.adapter.durable.active_deleted {
                None
            } else {
                s.adapter.durable.active.clone().or(with_tracking_access(|| {
                    s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .stored_runtime_state(s.bound.as_ref().ok_or(Error::Database)?.binding.id)
                })?
                .and_then(|v| v.root_ack)
                .map(|r| Saved {
                    name: "active".into(),
                    envelope: r.envelope.0,
                    system_fields: r.system_fields.0,
                }))
            };
            let request = s.adapter.fetch.as_ref().map_or(0, |c| c.id);
            drop(s);
            if let Some(root) = root {
                self.accept_root_fetch(sdk, Some(&root), true, request)?;
            }
            s = self.state();
            Self::sdk_locked(&s, sdk)?;
        }
        let old = s.adapter.durable.clone();
        if resolved_epoch.is_some() {
            s.adapter.durable.epoch_verification = None;
        }
        s.adapter.durable.drains.extend(drains.clone());
        if let Err(e) = Self::adapter_commit_locked(
            &mut s,
            &[],
            &[],
            None,
            AdapterCommitOptions {
                at_ms,
                fail: false,
                start_absence: false,
                resolved_epoch,
            },
        ) {
            s.adapter.durable = old;
            return Err(e);
        }
        s.adapter
            .decided_zones
            .extend(drains.iter().filter(|d| d.fetched).map(|d| d.zone.clone()));
        if let Some(binding) = s.bound.as_ref().map(|b| b.binding.clone()) {
            let zone = collection_zone(binding.facts.epoch);
            if drains.iter().any(|d| d.zone == zone) {
                s.publication.cleanup_drain = Some((Self::adapter_guard_locked(&s), sdk.into(), at_ms));
                s.media.observed_now_ms = at_ms;
            }
            if drains.iter().any(|d| d.zone == zone && d.fetched) {
                let completed = with_tracking_access(|| {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .complete_absence(binding.id, &zone, at_ms)
                });
                match completed {
                    Ok(Some((durable, event))) => {
                        s.adapter.durable = durable;
                        // A trace can describe only a committed receipt.
                        s.trace.push(event);
                    }
                    Ok(None) => {}
                    Err(e) => {
                        Self::invalidate_locked(&mut s);
                        return Err(e.into());
                    }
                }
            }
        }
        if let Some(call) = s.adapter.fetch.as_mut() {
            call.completed = owns_call;
            if owns_call {
                call.drains.extend(drains.into_iter().map(|d| d.zone));
            }
        }
        Ok(())
    }
    pub fn adapter_finish_fetch(&self, sdk: &str, id: u64) -> Result<bool, RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let call = s.adapter.fetch.take().ok_or(Error::InvalidData)?;
        if call.id != id {
            return Err(Error::InvalidData.into());
        }
        if call.boundary.is_some() {
            s.adapter.missing_did += 1;
        }
        Ok(call.completed && call.scope.iter().all(|z| call.drains.contains(z)))
    }
    pub fn adapter_fetch_drains(&self) -> Vec<FetchDrain> {
        self.state().adapter.durable.drains.clone()
    }
    pub fn adapter_missing_did(&self) -> u64 {
        self.state().adapter.missing_did
    }
    pub fn adapter_exchange_fetch(&self, sdk: &str, complete: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if complete {
            s.actions.exchange_phase = None;
        }
        Ok(())
    }
    pub fn adapter_complete_exchange(
        &self,
        sdk: &str,
        id: u64,
        fetched: bool,
        terminal: Option<CollectionOutcome>,
    ) -> Result<CompletedExchange, RuntimeError> {
        let owner = {
            let s = self.state();
            if let Some(completed) = s.adapter.durable.exchanges.get(&id) {
                return Ok(completed.clone());
            }
            if let Some((owner, completed)) = s.adapter.closed_exchanges.get(&id)
                && owner == sdk
            {
                return Ok(completed.clone());
            }
            let owner = (id, sdk.to_owned(), s.incarnation);
            if s.adapter.exchange_owner.as_ref() != Some(&owner) {
                return Err(RuntimeError::StaleIncarnation);
            }
            if s.actions.exchange.as_ref().map(|(active, _)| *active) != Some(id) {
                return Err(Error::InvalidData.into());
            }
            owner
        };
        // The clock is a foreign provider and can reenter or change Runtime.
        // Read it without either guard; all completion facts are read again below.
        let ended_ms = self.adapter_now()?;
        let mut s = self.state();
        if let Some(completed) = s.adapter.durable.exchanges.get(&id) {
            return Ok(completed.clone());
        }
        if let Some((owner, completed)) = s.adapter.closed_exchanges.get(&id)
            && owner == sdk
        {
            return Ok(completed.clone());
        }
        if s.incarnation != owner.2 || s.adapter.exchange_owner.as_ref() != Some(&owner) {
            return Err(RuntimeError::StaleIncarnation);
        }
        let (active_id, result) = s.actions.exchange.clone().ok_or(Error::InvalidData)?;
        if active_id != id {
            return Err(Error::InvalidData.into());
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let live = Self::sdk_locked(&s, sdk).is_ok();
        let inbox_empty = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.inbox(binding))?.is_empty();
        let outcome = if s
            .bound
            .as_ref()
            .is_some_and(|b| matches!(b.binding.facts.state, BindingState::Isolated(_)))
        {
            CollectionOutcome::Failed
        } else if !live {
            CollectionOutcome::Cancelled
        } else if let Some(outcome @ (CollectionOutcome::Failed | CollectionOutcome::Cancelled)) = terminal {
            outcome
        } else if !fetched {
            CollectionOutcome::WaitingNoAccount
        } else if result.outstanding_names.is_empty() && inbox_empty {
            CollectionOutcome::Succeeded
        } else {
            CollectionOutcome::Incomplete
        };
        let completed = CompletedExchange {
            id,
            collection: outcome,
            result,
        };
        let mut durable = s.adapter.durable.clone();
        durable.exchanges.insert(id, completed.clone());
        durable.exchange_ended_ms.insert(id, ended_ms);
        if let Err(error) = with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .adapter_record_exchange(binding, &durable)
        }) {
            Self::invalidate_locked(&mut s);
            return Err(error.into());
        }
        s.adapter.durable = durable;
        s.actions.exchange = None;
        s.actions.exchange_phase = None;
        s.actions.selected_revs.clear();
        s.adapter.exchange_owner = None;
        Ok(completed)
    }
    pub fn adapter_exchange_result(&self, id: u64) -> Option<CompletedExchange> {
        self.state().adapter.durable.exchanges.get(&id).cloned()
    }
    pub fn adapter_exchange_active(&self) -> bool {
        self.state().actions.exchange.is_some()
    }
    /// Compare after actual send return (A23 false), without a full-policy trigger.
    pub fn adapter_repair_pending(
        &self,
        sdk: &str,
        pending: BTreeMap<String, PendingChange>,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let desired = s.actions.desired.clone();
        let missing = desired
            .iter()
            .filter(|(n, v)| pending.get(*n) != Some(*v))
            .map(|(n, _)| n.clone())
            .collect::<BTreeSet<_>>();
        if !missing.is_empty() {
            s.actions.update(UpdateKind::Changes, PendingSnapshot(desired), missing);
        }
        Ok(())
    }
}
impl Runtime {
    pub fn adapter_send_due(&self, sdk: &str) -> Result<bool, RuntimeError> {
        let s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let Some((id, exchange)) = &s.actions.exchange else {
            return Ok(false);
        };
        Ok(!s.adapter.sdk_retrying
            && !exchange.outstanding_names.is_empty()
            && s.actions.calls.iter().filter(|c| c.exchange == Some(*id)).count() < 8)
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Preparation only: a synthetic previously confirmed service baseline.
    /// All later edits, review commits, scans, flights and acks use normal paths.
    pub fn prepare_adapter_baseline_for_testing(
        &self,
        collection: &Collection,
        clock: &dyn meguri_model::clock::Clock,
        random: &dyn meguri_model::rng::RandomSource,
        saved_state: Option<Vec<u8>>,
    ) -> Result<(Vec<Saved>, Saved, Saved), RuntimeError> {
        use crate::cloud::row_payload::{self, View};
        if self.inputs.scope.container.as_str() != "synthetic-u114" {
            return Err(Error::InvalidData.into());
        }
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let view = row_payload::ReadView(&*snapshot);
        let mut layouts = BTreeMap::new();
        let mut after = None;
        loop {
            let page = view.page(Table::Notetypes, after.as_ref())?;
            if page.is_empty() {
                break;
            }
            for row in page {
                let id = row_payload::int(&row, 0)?;
                after = Some(RowKey::Id(id));
                layouts.insert(id, row_payload::notetype(&view, &row, 1)?);
            }
        }
        let record_key = [114; 32];
        let epoch = [114; 16];
        let mut records = Vec::new();
        for kind in row_payload::KINDS {
            let Some(table) = row_payload::table(kind) else {
                continue;
            };
            let mut after = None;
            loop {
                let page = view.page(table, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?);
                    let key = row_payload::key(kind, &row)?;
                    let payload = row_payload::project(&view, kind, &row, |id| {
                        layouts.get(&id).cloned().ok_or(Error::InvalidData)
                    })?;
                    let name = codec::entity_name(&record_key, kind, &key).map_err(|_| Error::InvalidData)?;
                    let envelope = codec::Entity::build(
                        codec::EntityFields {
                            kind,
                            key,
                            origin: [31; 16],
                            erev: 1,
                            generation: None,
                            tombstone: false,
                            deleted_at_ms: 0,
                            writer: [41; 16],
                            base_sha256: None,
                        },
                        &payload.encode().map_err(|_| Error::InvalidData)?,
                    )
                    .map_err(|_| Error::InvalidData)?
                    .encode()
                    .map_err(|_| Error::InvalidData)?;
                    records.push(Saved {
                        name,
                        envelope,
                        system_fields: b"synthetic-confirmed-system-fields".to_vec(),
                    });
                }
            }
        }
        let root = Saved {
            name: "active".into(),
            envelope: crate::cloud::control_codec::Active {
                epoch,
                activated_at_ms: clock.now_ms(),
                previous: None,
                replaced_through_ms: None,
            }
            .encode()
            .map_err(|_| Error::InvalidData)?,
            system_fields: b"synthetic-root-system-fields".to_vec(),
        };
        let incoming_payload = codec::Payload::Config(codec::Config {
            key: b"synthetic-incoming".to_vec(),
            mtime_secs: clock.now_ms() / 1000,
            val: b"true".to_vec(),
        });
        let incoming_key = row_payload::payload_key(&incoming_payload)?;
        let incoming = Saved {
            name: codec::entity_name(&record_key, codec::Kind::Config, &incoming_key)
                .map_err(|_| Error::InvalidData)?,
            envelope: codec::Entity::build(
                codec::EntityFields {
                    kind: codec::Kind::Config,
                    key: incoming_key,
                    origin: [51; 16],
                    erev: 2,
                    generation: None,
                    tombstone: false,
                    deleted_at_ms: 0,
                    writer: [61; 16],
                    base_sha256: None,
                },
                &incoming_payload.encode().map_err(|_| Error::InvalidData)?,
            )
            .map_err(|_| Error::InvalidData)?
            .encode()
            .map_err(|_| Error::InvalidData)?,
            system_fields: b"synthetic-incoming-system-fields".to_vec(),
        };
        let publication = meguri_engine::device_state::DeviceState::for_collection(collection.path())
            .publication()?
            .ok_or(Error::InvalidData)?
            .identity;
        {
            let mut s = self.state();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_mut().ok_or(Error::Database)?;
                if store.selected_binding()?.is_some() {
                    return Err(Error::InvalidData.into());
                }
                let binding = store.create_binding(
                    BindingFacts {
                        account: OpaqueBytes(b"synthetic-account-A".to_vec()),
                        epoch: EpochId(epoch),
                        record_key: RecordKey(record_key),
                        publication,
                        state: BindingState::Active,
                    },
                    clock,
                    random,
                )?;
                for record in &records {
                    store.prepare_entity_baseline_for_testing(
                        binding.id,
                        &record.name,
                        &record.envelope,
                        OpaqueBytes(record.system_fields.clone()),
                    )?;
                }
                store.adapter_install()?;
                store.adapter_commit(
                    binding.id,
                    &[],
                    &[],
                    saved_state.as_deref(),
                    &Durable::default(),
                    super::super::adapter_store::AdapterCommitOptions {
                        at_ms: clock.now_ms(),
                        fail: false,
                        start_absence: false,
                        resolved_epoch: None,
                    },
                )?;
                Ok(())
            })?;
        }
        drop(snapshot);
        drop(lease);
        self.bind(collection)?;
        self.adapter_account(
            None,
            IdentityStatus::Available(OpaqueBytes(b"synthetic-account-A".to_vec())),
        )?;
        Ok((records, root, incoming))
    }
    pub fn arm_adapter_faults_for_testing(&self, inbox: bool, state: bool) {
        let mut s = self.state();
        s.adapter.fail_inbox = inbox;
        s.adapter.fail_state = state;
    }
}

impl Runtime {
    pub fn adapter_tracking_failure(&self, sdk: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        Self::invalidate_locked(&mut s);
        Ok(())
    }
}

impl Runtime {
    pub fn adapter_sdk_retrying(&self, sdk: &str, retained_error: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        s.adapter.sdk_retrying |= retained_error;
        Ok(())
    }
}

impl Runtime {
    /// Only composition of a real retained NoCollection Host issues this lease.
    /// It names this Runtime incarnation and never opens tracking or a collection.
    pub fn attach_no_collection_identity_owner(
        &self,
        live: std::sync::Weak<Mutex<bool>>,
        engine: meguri_engine::WeakEngine,
        generation: Generation,
    ) {
        let mut owner = self
            .no_collection_identity_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        if owner.is_none() {
            *owner = Some(NoCollectionIdentityOwner {
                live,
                engine,
                generation,
                incarnation: self.state().incarnation,
            });
        }
    }

    fn with_identity_read_state<T>(
        &self,
        no_collection: bool,
        apply: impl FnOnce(&mut State) -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if !no_collection {
            let mut s = self.state();
            if s.store.is_none() {
                return Err(RuntimeError::StaleIncarnation);
            }
            return apply(&mut s);
        }
        let owner = self
            .no_collection_identity_owner
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(RuntimeError::StaleIncarnation)?;
        let live = owner.live.upgrade().ok_or(RuntimeError::StaleIncarnation)?;
        // Same ordering as Host close: original-owner lease before Runtime state.
        // No platform provider is ever called while either lock is held.
        let live = live.lock().unwrap_or_else(PoisonError::into_inner);
        let engine = owner.engine.upgrade().ok_or(RuntimeError::StaleIncarnation)?;
        if !*live
            || engine.generation() != owner.generation
            || !matches!(engine.state(), meguri_engine::LifecycleState::NoCollection)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let mut s = self.state();
        if s.bound.is_some()
            || s.cloud.generation.is_some()
            || s.incarnation != owner.incarnation
            || engine.generation() != owner.generation
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        match self.directory.join("collection.anki2").symlink_metadata() {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
            Ok(_) => return Err(RuntimeError::StaleIncarnation),
        }
        apply(&mut s)
    }

    fn identity_read_current_locked(s: &State, id: u64) -> bool {
        s.adapter
            .identity_read
            .as_ref()
            .is_some_and(|(request, guard, incarnation, origin_sdk, _)| {
                *request == id
                    && *guard == Self::adapter_guard_locked(s)
                    && *incarnation == s.incarnation
                    && !origin_sdk
                        .as_deref()
                        .is_some_and(|sdk| Self::sdk_locked(s, sdk).is_err())
            })
    }
    /// Recheck the original authority after the provider returns, without
    /// consuming it. Completion repeats the check at its mutation boundary.
    pub(super) fn adapter_validate_identity_read(&self, id: u64) -> Result<(), RuntimeError> {
        let no_collection = self
            .state()
            .adapter
            .identity_read
            .as_ref()
            .filter(|(request, ..)| *request == id)
            .map(|v| v.4)
            .ok_or(RuntimeError::StaleIncarnation)?;
        self.with_identity_read_state(no_collection, |s| {
            if !Self::identity_read_current_locked(s, id) {
                return Err(RuntimeError::StaleIncarnation);
            }
            Ok(())
        })
    }
    pub fn adapter_begin_identity_read(
        &self,
        _transition: bool,
        origin_sdk: Option<&str>,
    ) -> Result<u64, RuntimeError> {
        self.adapter_begin_identity_read_with_work(_transition, origin_sdk, &mut PolicyWork::default())
    }
    pub fn adapter_begin_identity_read_with_work(
        &self,
        _transition: bool,
        origin_sdk: Option<&str>,
        work: &mut PolicyWork,
    ) -> Result<u64, RuntimeError> {
        let _ = self.adapter_now();
        let no_collection = self.state().store.is_none();
        self.with_identity_read_state(no_collection, |s| {
            if let Some(sdk) = origin_sdk {
                Self::sdk_locked(s, sdk)?;
            }
            work.effect()?;
            if origin_sdk.is_some() {
                s.adapter.no_state = true;
                s.adapter.no_state_creation = false;
                s.adapter.decided_zones.clear();
                if let Some(call) = s.adapter.fetch.as_mut() {
                    call.drains.clear();
                    call.read_zones.clear();
                }
                s.adapter.no_state_point += 1;
                self.emit_pending_locked(s, UpdateKind::Full, BTreeSet::new())?;
            }
            s.adapter.next_identity = s.adapter.next_identity.checked_add(1).ok_or(Error::InvalidData)?;
            let id = s.adapter.next_identity;
            s.adapter.accepted_identity_read = None;
            // A pending lookup is not evidence of a different account. Keep the
            // verified owner accepting records and their following cursor (V9.1).
            s.adapter.identity_read = Some((
                id,
                Self::adapter_guard_locked(s),
                s.incarnation,
                origin_sdk.map(str::to_owned),
                no_collection,
            ));
            Ok(id)
        })
    }
    pub fn adapter_complete_identity_read(&self, id: u64, identity: IdentityStatus) -> Result<bool, RuntimeError> {
        self.adapter_complete_identity_read_with_work(id, identity, &mut PolicyWork::default())
    }
    pub fn adapter_complete_identity_read_with_work(
        &self,
        id: u64,
        identity: IdentityStatus,
        work: &mut PolicyWork,
    ) -> Result<bool, RuntimeError> {
        let Some(no_collection) = self
            .state()
            .adapter
            .identity_read
            .as_ref()
            .filter(|(request, ..)| *request == id)
            .map(|v| v.4)
        else {
            return Ok(false);
        };
        let result = self.with_identity_read_state(no_collection, |s| {
            if !Self::identity_read_current_locked(s, id) {
                return Ok(false);
            }
            work.effect()?;
            s.adapter.identity_read = None;
            // A failed later read preserves an already verified process owner (V9.1).
            if !matches!(identity, IdentityStatus::ReadFailed) {
                self.adapter_account_locked(s, identity.clone(), false, no_collection)?;
            }
            s.adapter.accepted_identity_read = Some(crate::cloud::adapter::AcceptedIdentityRead {
                request: id,
                guard: Self::adapter_guard_locked(s),
                incarnation: s.incarnation,
                no_collection,
                identity,
            });
            Ok(true)
        });
        match result {
            Err(RuntimeError::StaleIncarnation) => Ok(false),
            result => result,
        }
    }

    pub(crate) fn with_accepted_identity_read<T>(
        &self,
        request: u64,
        commit: impl FnOnce(&IdentityStatus) -> T,
    ) -> Result<T, RuntimeError> {
        let no_collection = self
            .state()
            .adapter
            .accepted_identity_read
            .as_ref()
            .filter(|proof| proof.request == request)
            .map(|proof| proof.no_collection)
            .ok_or(RuntimeError::StaleIncarnation)?;
        self.with_identity_read_state(no_collection, |s| {
            let proof = s
                .adapter
                .accepted_identity_read
                .as_ref()
                .ok_or(RuntimeError::StaleIncarnation)?;
            if proof.request != request
                || s.adapter.next_identity != request
                || s.adapter.identity_read.is_some()
                || proof.incarnation != s.incarnation
                || proof.guard != Self::adapter_guard_locked(s)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            Ok(commit(&proof.identity))
        })
    }
}

impl Runtime {
    pub(super) fn adapter_close_exchange_locked(s: &mut State) {
        if let Some((id, result)) = s.actions.exchange.take()
            && let Some((owned_id, owner, _)) = s.adapter.exchange_owner.take()
            && id == owned_id
        {
            // P12.7: close has no durable row or successor-generation write.
            s.adapter.closed_exchanges.insert(
                id,
                (
                    owner,
                    CompletedExchange {
                        id,
                        collection: CollectionOutcome::Cancelled,
                        result,
                    },
                ),
            );
        }
        s.actions.exchange_phase = None;
        s.actions.selected_revs.clear();
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Exercise the real engine_state deletion boundary without injecting pass facts.
    pub fn forget_adapter_state_for_testing(&self, sdk: &str) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .forget_adapter_state_for_testing(b)
        })?;
        Ok(())
    }
}
