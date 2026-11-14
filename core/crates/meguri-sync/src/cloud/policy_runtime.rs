//! Projection of real tracking and execution helpers; never owns collection or publication decisions.
use super::*;
use crate::cloud::policy::*;
use std::collections::BTreeSet;

impl Runtime {
    pub(super) fn exchange_work_locked(
        state: &super::State,
        store: &TrackingStore,
        binding: BindingId,
    ) -> Result<crate::cloud::adapter::ExchangeWork, RuntimeError> {
        let pending = store.policy_pending_names(binding)?;
        let held: BTreeSet<_> = state
            .cloud
            .deferred
            .iter()
            .chain(&state.cloud.incoming_held)
            .chain(&state.policy.held_images)
            .cloned()
            .collect();
        Ok(crate::cloud::adapter::ExchangeWork {
            held_pending_names: pending.intersection(&held).cloned().collect(),
            pending_unheld_names: pending.difference(&held).cloned().collect(),
            media: Self::policy_media_facts_locked(state, store, binding)?,
        })
    }
    /// Sample this Runtime's original platform provider, never a UI identity or
    /// the process-global configuration for a later Host. No core lock is held.
    pub fn policy_account_identity(&self) -> Result<(u64, IdentityStatus), RuntimeError> {
        self.policy_account_identity_with_work(&mut PolicyWork::default())
    }
    pub fn policy_account_identity_with_work(
        &self,
        work: &mut PolicyWork,
    ) -> Result<(u64, IdentityStatus), RuntimeError> {
        let request = self.adapter_begin_identity_read_with_work(false, None, work)?;
        let identity = self.inputs.identity.read_identity(&self.inputs.scope);
        self.adapter_validate_identity_read(request)?;
        Ok((request, identity))
    }

    pub(crate) fn policy_engine(&self, guard: &crate::cloud::adapter::Guard) -> Result<Option<Engine>, RuntimeError> {
        let s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(s.adapter.sdk.as_ref().map(|sdk| Engine {
            instance: sdk.id.clone(),
            binding: s.bound.as_ref().map_or(BindingId([0; 16]), |b| b.binding.id),
            stopping: sdk.stopping || &sdk.guard != guard,
        }))
    }

    pub fn policy_projection(&self, lifecycle: Lifecycle, generation: u64) -> Result<Projection, RuntimeError> {
        self.policy_projection_with_work(lifecycle, generation, &mut PolicyWork::default())
    }
    pub fn policy_projection_with_work(
        &self,
        lifecycle: Lifecycle,
        generation: u64,
        work: &mut PolicyWork,
    ) -> Result<Projection, RuntimeError> {
        let session = self.active_review();
        let resume_binding = self.resume_binding_offer_with_work(work)?;
        // Read the existing sidecar fact, outside Runtime/tracking locks. This grants no authority.
        let publication =
            meguri_engine::device_state::DeviceState::for_collection(&self.directory.join("collection.anki2"))
                .publication()?;
        let mut fence = match publication.as_ref() {
            Some(p) if p.decision == meguri_engine::device_state::PublicationDecision::Pending => {
                Fence::DecisionRequired
            }
            Some(p) if p.binding == meguri_engine::device_state::AccountBinding::LocalOnly => Fence::LocalOnly,
            _ => Fence::Clear,
        };
        let s = self.state();
        let mut summary = Summary {
            scan: if s.scan_running {
                Scan::Running
            } else if s.scan_required {
                Scan::Required
            } else {
                Scan::None
            },
            ..Summary::default()
        };
        let mut binding = None;
        let mut evidence = Evidence::default();
        let mut held_pending = BTreeSet::new();
        with_tracking_access(|| -> Result<(), RuntimeError> {
            // V8.1 kept sources span all original build scopes and outlive bindings.
            // Only finalized main files count; enumeration failures are not zero.
            if let Some(store) = &s.store {
                let unreadable = store.retained_damaged_sources()?.len() as u64;
                if unreadable > 0 {
                    summary.attention.insert(Attention::ConflictsUnreadable, unreadable);
                }
            }
            if let (Some(b), Some(store)) = (&s.bound, &s.store) {
                let publish = store.publication_op()?;
                let publish_step = publish
                    .as_ref()
                    .filter(|op| op.binding_after == Some(b.binding.id))
                    .map(|op| match op.step {
                        crate::cloud::publication::PublishStep::Confirmed => PublishStep::Bound,
                        crate::cloud::publication::PublishStep::Bound => PublishStep::Bound,
                        crate::cloud::publication::PublishStep::Zones => PublishStep::Zones,
                        crate::cloud::publication::PublishStep::Sent => PublishStep::Sent,
                        crate::cloud::publication::PublishStep::Complete => PublishStep::Complete,
                        crate::cloud::publication::PublishStep::Cancelling => PublishStep::Cancelling,
                    });
                binding = Some(BindingView {
                    id: b.binding.id,
                    state: b.binding.facts.state,
                    account: b.binding.facts.account.clone(),
                    usable: b.usable,
                    verified: b.verified,
                    publish_step,
                });
                for (code, count) in store.row_failure_counts(b.binding.id)? {
                    let attention = match code {
                        1 => Attention::Oversize,
                        2 => Attention::Unreadable,
                        _ => return Err(Error::InvalidData.into()),
                    };
                    *summary.attention.entry(attention).or_default() += count;
                }
                if store.root_unreadable(b.binding.id)?.is_some() {
                    summary.attention.insert(Attention::RootUnreadable, 1);
                }
                if store.exchange_incomplete(b.binding.id)? {
                    summary.attention.insert(Attention::ExchangeIncomplete, 1);
                }
                summary.crt_known = b.epoch_crt.is_some();
                summary.crt_mismatch = b.epoch_crt.is_some_and(|crt| crt != b.current_crt.1);
                let work = Self::exchange_work_locked(&s, store, b.binding.id)?;
                held_pending = work.held_pending_names.clone();
                summary.held = held_pending.len() as u64;
                summary.pending_unheld = work.pending_unheld_names.len() as u64;
                let failed_names = store.row_failure_names(b.binding.id)?;
                summary.sendable = store
                    .dirty_names(b.binding.id)?
                    .0
                    .keys()
                    .filter(|n| !failed_names.contains(*n) && !held_pending.contains(*n) && !s.policy.parked.contains(*n))
                    .count() as u64;
                summary.parked = held_pending
                    .iter()
                    .chain(&work.pending_unheld_names)
                    .filter(|name| s.policy.parked.contains(*name))
                    .count() as u64;
                summary.zone_deletions_due = store
                    .retained_zones(b.binding.id)?
                    .values()
                    .filter(|row| row.deletion_due)
                    .count() as u64;
                let current = s
                    .adapter
                    .sdk
                    .as_ref()
                    .filter(|sdk| !sdk.stopping && sdk.guard == Self::adapter_guard_locked(&s))
                    .map(|sdk| sdk.id.as_str());
                let flights: BTreeSet<_> = s
                    .policy
                    .flights
                    .iter()
                    .filter(|((owner, _), _)| Some(owner.as_str()) == current)
                    .flat_map(|(_, names)| names.iter())
                    .collect();
                summary.in_flight = flights.len() as u64;
                let reference_waiting = store.reference_waiting(b.binding.id)?;
                for entry in store.inbox(b.binding.id)? {
                    if entry.entry.reason == InboxReason::Ready {
                        summary.ready += 1;
                    } else {
                        summary.incoming_waiting += 1;
                    }
                    let attention = match entry.entry.reason {
                        InboxReason::Reference if reference_waiting.contains(&entry.id) => Some(Attention::ReferenceWaiting),
                        InboxReason::Asset if entry.entry.retry_count >= 5 => Some(Attention::AssetUnavailable),
                        InboxReason::Asset => {
                            summary.asset_retryable += 1;
                            None
                        }
                        InboxReason::Format => Some(Attention::UnsupportedSchema),
                        InboxReason::ApplyFailed { failure, .. } => {
                            summary.apply_failed += 1;
                            match failure {
                                ApplyFailure::Rekey => Some(Attention::RekeyFailed),
                                ApplyFailure::Apply if entry.entry.retry_count >= 5 => Some(Attention::ApplyFailed),
                                _ => None,
                            }
                        }
                        _ => None,
                    };
                    if let Some(item) = attention {
                        *summary.attention.entry(item).or_default() += 1;
                    }
                }
                let intents = store.apply_intents(b.binding.id)?;
                summary.apply_in_progress = s.incoming.running || intents.iter().any(|i| i.result_seq.is_some());
                summary.unsettled_intents = intents.iter().filter(|i| i.result_seq.is_none()).count() as u64;
                let refused = store.refused_entries(b.binding.id)?.len() as u64;
                if refused > 0 {
                    summary.attention.insert(Attention::Refused, refused);
                }
                let conflicts = store
                    .conflict_copies()?
                    .iter()
                    .filter(|c| c.binding == b.binding.id && c.copy.resolved_at_ms.is_none())
                    .count() as u64;
                if conflicts > 0 {
                    summary.attention.insert(Attention::Conflicts, conflicts);
                }
                if publish.is_some() && s.publication.waiting || store.adoption_op()?.is_some() {
                    summary.attention.insert(Attention::RecoveryOpWaiting, 1);
                }
                summary.media = work.media;
                let media_ledger = store.media_ledger(b.binding.id)?;
                for (attention, count) in [
                    (
                        Attention::Unsyncable,
                        media_ledger.names.values().filter(|f| f.unsyncable).count(),
                    ),
                    (
                        Attention::MediaChanged,
                        media_ledger
                            .names
                            .values()
                            .filter(|f| f.media_changed.is_some())
                            .count(),
                    ),
                ] {
                    if count > 0 {
                        summary.attention.insert(attention, count as u64);
                    }
                }
                summary.collection_known = s.whole_complete;
                // Other K10 flag/root/generation producers remain incomplete.
                // Those missing facts may never be presented as zero attention.
                summary.attention_known = false;
                evidence = store.cloud_policy_evidence(b.binding.id)?;
            } else if let Some(store) = &s.store {
                // A binding from another build is local-only, never a choose-collection candidate here.
                if store.selected_binding()?.is_none() && publication.as_ref().is_some_and(|p| matches!(&p.binding,
                    meguri_engine::device_state::AccountBinding::Connected { configuration, environment, .. }
                    if configuration.as_str() != s_scope_container(self) || environment.as_str() != s_scope_environment(self)))
                { fence = Fence::EnvironmentChanged; }
            }
            Ok(())
        })?;
        Ok(Projection {
            held_pending,
            facts: CoreFacts {
                lifecycle,
                generation,
                binding,
                session,
                fence,
                summary,
                resume_binding_offered: resume_binding
                    .filter(|id| {
                        s.publication
                            .resume
                            .as_ref()
                            .is_some_and(|c| c.binding.id == *id && self.validate_resume_context_locked(&s, c).is_ok())
                    })
                    .map(|_| true),
            },
            evidence,
            engine: s.adapter.sdk.as_ref().map(|sdk| Engine {
                instance: sdk.id.clone(),
                binding: s.bound.as_ref().map_or(BindingId([0; 16]), |b| b.binding.id),
                stopping: sdk.stopping,
            }),
            tracking_stopped: s.tracking_stopped,
            root_confirmed: s.root_confirmed,
            drains: s.adapter.durable.drains.clone(),
            cleanup_drain: s.publication.cleanup_drain.clone(),
            backend_guard: Self::adapter_guard_locked(&s),
        })
    }
    pub(crate) fn policy_update_pending_with_work(
        &self,
        guard: &crate::cloud::adapter::Guard,
        update: Update,
        work: &mut PolicyWork,
    ) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        let mut s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        work.effect()?;
        if update == Update::Full {
            s.policy.parked.clear();
        }
        self.emit_pending_locked(
            &mut s,
            match update {
                Update::Full => crate::cloud::workflow::UpdateKind::Full,
                Update::Changes => crate::cloud::workflow::UpdateKind::Changes,
            },
            BTreeSet::new(),
        )?;
        drop(s);
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::PolicyPendingUpdated);
        Ok(())
    }
    pub(crate) fn policy_tracking_failed_with_work(&self, work: &mut PolicyWork) -> Result<(), RuntimeError> {
        let mut s = self.state();
        work.effect()?;
        s.tracking_stopped = true;
        Ok(())
    }
    pub(crate) fn policy_tracking_stopped(&self) -> bool {
        self.state().tracking_stopped
    }
    pub(crate) fn policy_probe_with_work(
        &self,
        guard: &crate::cloud::adapter::Guard,
        work: &mut PolicyWork,
    ) -> Result<(), RuntimeError> {
        let at_ms = self.adapter_now()?;
        let mut s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| -> Result<_, RuntimeError> {
            work.effect()?;
            Ok(s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_cloud_probe(binding, at_ms))
        })?;
        match result {
            Ok(()) if !s.invalidation_exhausted => {
                s.tracking_stopped = false;
                Ok(())
            }
            Ok(()) => Err(Error::InvalidData.into()),
            Err(error) => {
                s.tracking_stopped = true;
                Err(error.into())
            }
        }
    }
    pub(crate) fn policy_record_quiet_with_work(
        &self,
        guard: &crate::cloud::adapter::Guard,
        at_ms: i64,
        work: &mut PolicyWork,
    ) -> Result<Evidence, RuntimeError> {
        let mut s = self.state();
        if &Self::adapter_guard_locked(&s) != guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        Self::gate_locked(&s)?;
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| -> Result<_, RuntimeError> {
            work.effect()?;
            Ok(s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_cloud_quiet(binding, at_ms))
        })?;
        if result.is_err() {
            s.tracking_stopped = true;
        }
        Ok(result?)
    }
    /// Returns no stamp for old rows written before exchange clocks were available.
    pub(crate) fn policy_completed_exchange(&self, id: u64) -> Option<(crate::cloud::adapter::CompletedExchange, i64)> {
        let s = self.state();
        Some((
            s.adapter.durable.exchanges.get(&id)?.clone(),
            *s.adapter.durable.exchange_ended_ms.get(&id)?,
        ))
    }
    /// Only a successful day check leaves a receipt. A concurrent check is refused;
    /// a failed check releases its reservation so an explicit later call can try it.
    pub(crate) fn policy_record_exchange_with_work(
        &self,
        collection: &Collection,
        guard: &crate::cloud::adapter::Guard,
        owner: &str,
        record: &ExchangeRecord,
        native: Option<u64>,
        work: &mut PolicyWork,
    ) -> Result<Evidence, RuntimeError> {
        let key = (owner.to_owned(), record.exchange);
        let rollover = {
            let mut s = self.state();
            if &Self::adapter_guard_locked(&s) != guard
                || !collection.is_current()
                || collection.generation().0 != guard.generation
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            let recorded = with_tracking_access(|| {
                s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .policy_exchange_recorded(binding, owner, record.exchange)
            })?;
            if recorded {
                let evidence = with_tracking_access(|| -> Result<_, RuntimeError> {
                    Ok(s.store
                        .as_ref()
                        .ok_or(Error::Database)?
                        .cloud_policy_evidence(binding)?)
                })?;
                work.check()?;
                return Ok(evidence);
            }
            if s.policy.rollovers.contains(&key) {
                false
            } else {
                work.effect()?;
                let inserted = s.policy.rollovers_running.insert(key.clone());
                if inserted {
                    true
                } else {
                    return Err(meguri_engine::EngineError::Busy {
                        running_op: "cloud policy rollover".into(),
                    }
                    .into());
                }
            }
        };
        if rollover {
            let checked = collection.check_rollover();
            let mut s = self.state();
            s.policy.rollovers_running.remove(&key);
            checked?;
            s.policy.rollovers.insert(key);
        }
        let mut s = self.state();
        if &Self::adapter_guard_locked(&s) != guard || !collection.is_current() {
            return Err(RuntimeError::StaleIncarnation);
        }
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let result = with_tracking_access(|| -> Result<_, RuntimeError> {
            work.effect()?;
            Ok(s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_cloud_policy_exchange_original(binding, owner, record, native))
        })?;
        if result.is_err() {
            s.tracking_stopped = true;
        }
        Ok(result?)
    }
    pub(crate) fn policy_apply_ready_with_work(
        &self,
        collection: &Collection,
        guard: &crate::cloud::adapter::Guard,
        retry_failed: bool,
        timeout: Duration,
        work: &mut PolicyWork,
    ) -> Result<crate::cloud::incoming::IncomingDisposition, RuntimeError> {
        let sdk = {
            let mut s = self.state();
            if &Self::adapter_guard_locked(&s) != guard {
                return Err(RuntimeError::StaleIncarnation);
            }
            let sdk = s.adapter.sdk.as_ref().ok_or(RuntimeError::StaleIncarnation)?.id.clone();
            Self::sdk_locked(&s, &sdk)?;
            if retry_failed {
                work.effect()?;
                s.incoming.retry_granted = true;
            }
            sdk
        };
        let authority = work.clone();
        #[cfg(feature = "test-hooks")]
        let hooks = self.incoming_hooks.clone();
        collection.check_rollover_with_boundary(move || {
            #[cfg(feature = "test-hooks")]
            hooks.hit(crate::cloud::testing::IncomingStage::PolicyApplyWriterAdmission);
            authority.engine_effect()
        })?;
        work.check()?;
        self.drain_native_incoming_with_work(collection, &sdk, timeout, work)
    }
}

#[cfg(feature = "test-hooks")]
impl Runtime {
    /// Installs a real SQLite constraint in the disposable policy evidence store.
    pub fn policy_evidence_constraint_for_testing(&self, enabled: bool) -> Result<(), RuntimeError> {
        let s = self.state();
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            store
                .connection
                .execute_batch(if enabled {
                    "CREATE TEMP TRIGGER reject_policy_evidence_insert BEFORE INSERT ON cloud_policy_evidence
                 BEGIN SELECT RAISE(ABORT,'policy evidence constraint'); END;
                 CREATE TEMP TRIGGER reject_policy_evidence_update BEFORE UPDATE ON cloud_policy_evidence
                 BEGIN SELECT RAISE(ABORT,'policy evidence constraint'); END;"
                } else {
                    "DROP TRIGGER reject_policy_evidence_insert; DROP TRIGGER reject_policy_evidence_update;"
                })
                .map_err(Error::from)?;
            Ok(())
        })
    }
}
fn s_scope_container(runtime: &Runtime) -> &str {
    runtime.inputs.scope.container.as_str()
}
fn s_scope_environment(runtime: &Runtime) -> &str {
    match runtime.inputs.scope.environment {
        Environment::Development => "development",
        Environment::Production => "production",
    }
}
