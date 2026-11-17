//! V9.2 authority is retained from V13's real reads, separate from scan authority.
use super::*;
use crate::cloud::policy::PolicyWork;
use crate::cloud::{
    control_codec::{Active, DecodeOutcome, Epoch, EpochState},
    publication::*,
};
use std::collections::{BTreeMap, BTreeSet};

type ResumeUnsentCountsWithExactnessAndDependencyHolds = (BTreeMap<codec::Kind, u64>, bool, BTreeSet<String>);

impl Runtime {
    /// R11: a completed matching process read can reaffirm the original stopped
    /// owner's ready preview. This grants no usability or SDK verification.
    pub(super) fn resume_account_reaffirmation_locked(
        &self,
        s: &State,
        proof: &super::super::adapter::AcceptedIdentityRead,
    ) -> Result<bool, RuntimeError> {
        if proof.no_collection
            || proof.request != s.adapter.next_identity
            || proof.incarnation != s.incarnation
            || proof.guard != Self::adapter_guard_locked(s)
            || s.adapter.identity_read.is_some()
            || s.adapter.sdk.is_some()
            || s.bound.as_ref().is_none_or(|b| b.usable || b.verified)
        {
            return Ok(false);
        }
        let Some((preview, true)) = s.publication.preview.as_ref() else {
            return Ok(false);
        };
        let Some(context) = s.publication.resume.as_ref() else {
            return Ok(false);
        };
        if preview.decision != ConnectDecision::ConfirmResume
            || preview.resume_binding != Some(context.binding.id)
            || preview.account != context.binding.facts.account
            || !matches!(&proof.identity, IdentityStatus::Available(a) if a == &preview.account)
            || preview.native_file != Some(context.scan.file)
            || preview.decision_before.as_ref() != Some(&context.decision)
            || !Self::resume_reads_match(context, preview)
        {
            return Ok(false);
        }
        match self.validate_resume_context_locked(s, context) {
            Ok(()) => Ok(true),
            Err(RuntimeError::Publication(PublicationError::NotOffered)) => Ok(false),
            Err(error) => Err(error),
        }
    }

    fn resumable(binding: &Binding) -> bool {
        matches!(
            binding.facts.state,
            BindingState::Isolated(
                IsolationReason::SignedOut | IsolationReason::AccountChanged | IsolationReason::Stopped
            )
        )
    }

    fn resume_decision_matches(&self, decision: &DecisionSnapshot, binding: &Binding) -> bool {
        use meguri_engine::device_state::AccountBinding;
        match &decision.publication.binding {
            AccountBinding::Connected {
                account,
                configuration,
                environment,
            } => {
                account.as_bytes() == binding.facts.account.0.as_slice()
                    && configuration == self.inputs.scope.container.as_str()
                    && environment
                        == match self.inputs.scope.environment {
                            Environment::Development => "development",
                            Environment::Production => "production",
                        }
            }
            AccountBinding::LocalOnly => true,
            AccountBinding::Unknown => false,
        }
    }

    pub(super) fn retain_resume_context_locked(
        &self,
        s: &State,
        collection: &Collection,
        file: NativeFileIdentity,
        decision: &DecisionSnapshot,
        account: &OpaqueBytes,
    ) -> Result<Option<ResumeContext>, RuntimeError> {
        let Some(b) = s.bound.as_ref().filter(|b| Self::resumable(&b.binding)) else {
            return Ok(None);
        };
        if Self::layout_refused_locked(s) || b.binding.scope != self.inputs.scope
            || &b.binding.facts.account != account
            || b.binding.facts.publication != decision.publication.identity
            || decision.publication.decision != meguri_engine::device_state::PublicationDecision::Clear
            || !self.resume_decision_matches(decision, &b.binding)
            || b.file != file
            || b.evidence.native_file != file.pair()
            || b.evidence.generation != collection.generation()
            || !collection.is_current()
            || s.invalidation_exhausted
            || s.scan_running
        {
            return Ok(None);
        }
        let context = with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            Ok(ResumeContext {
                collection: collection.clone(),
                scan: ScanGuard {
                    invalidation: s.invalidation,
                    binding: b.binding.id,
                    open: b.open_counter,
                    generation: collection.generation(),
                    file,
                    incarnation: s.incarnation,
                },
                adapter: Self::adapter_guard_locked(s),
                binding: b.binding.clone(),
                decision: decision.clone(),
                publish: store.publication_op()?,
                adopt: store.adoption_op()?,
                active: None,
                epoch: None,
            })
        })?;
        if self.validate_resume_context_locked(s, &context).is_err() {
            return Ok(None);
        }
        Ok(Some(context))
    }

    /// Caller holds state; providers and sidecar access must precede this check.
    pub(super) fn validate_resume_context_locked(&self, s: &State, c: &ResumeContext) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(PublicationError::NotOffered)?;
        if Self::layout_refused_locked(s) || s.incarnation != c.scan.incarnation
            || s.invalidation != c.scan.invalidation
            || s.invalidation_exhausted
            || s.scan_running
            || !Self::resumable(&b.binding)
            || b.binding != c.binding
            || b.binding.scope != self.inputs.scope
            || (b.open_counter, b.evidence.generation, b.file) != (c.scan.open, c.scan.generation, c.scan.file)
            || Self::adapter_guard_locked(s) != c.adapter
            || !c.collection.is_current()
            || c.collection.generation() != c.scan.generation
        {
            return Err(PublicationError::NotOffered.into());
        }
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let publish = store.publication_op()?;
            let adopt = store.adoption_op()?;
            if store.binding(c.binding.id)?.as_ref() != Some(&c.binding)
                || store
                    .runtime_binding(c.binding.id)?
                    .file
                    .is_some_and(|f| f != c.scan.file)
                || publish != c.publish
                || adopt != c.adopt
                || publish.as_ref().is_some_and(|p| {
                    p.binding_after == Some(c.binding.id)
                        || p.binding_before.as_ref().is_some_and(|b| b.id == c.binding.id)
                })
                || adopt.as_ref().is_some_and(|p| {
                    p.binding_after == c.binding.id || p.binding_before.as_ref().is_some_and(|b| b.id == c.binding.id)
                })
                || store
                    .publication_bindings()?
                    .iter()
                    .any(|b| b.facts.state == BindingState::Active)
            {
                return Err(PublicationError::NotOffered.into());
            }
            Ok(())
        })
    }

    fn validate_resume_providers(&self, c: &ResumeContext, p: &Preview) -> Result<(), RuntimeError> {
        if self.active_review().is_some() {
            return Err(PublicationError::NotOffered.into());
        }
        let account = self.publication_account().map_err(|e| match e {
            RuntimeError::Publication(PublicationError::AccountUnavailable) => PublicationError::NotOffered.into(),
            e => e,
        })?;
        if account != c.binding.facts.account {
            return Err(PublicationError::AccountMismatch.into());
        }
        self.check_preview_authority(p, &c.collection).map_err(|e| match e {
            RuntimeError::Publication(PublicationError::AuthorityChanged) => PublicationError::NotOffered.into(),
            e => e,
        })?;
        if p.decision_before.as_ref() != Some(&c.decision)
            || c.decision.publication.identity != c.binding.facts.publication
            || c.decision.publication.decision != meguri_engine::device_state::PublicationDecision::Clear
            || !self.resume_decision_matches(&c.decision, &c.binding)
            || p.native_file != Some(c.scan.file)
        {
            return Err(PublicationError::NotOffered.into());
        }
        Ok(())
    }

    pub(super) fn validate_connection_resume(&self, id: u64) -> Result<(), RuntimeError> {
        let retained = {
            let s = self.state();
            s.publication.resume.clone().zip(
                s.publication
                    .preview
                    .as_ref()
                    .filter(|(p, _)| p.id == id)
                    .map(|(p, _)| p.clone()),
            )
        };
        if let Some((context, preview)) = retained {
            self.validate_resume_providers(&context, &preview)?;
            self.validate_resume_context_locked(&self.state(), &context)?;
        }
        Ok(())
    }

    pub(super) fn resume_reads_match(c: &ResumeContext, p: &Preview) -> bool {
        c.active.as_ref() == p.active.as_ref()
            && c.active.as_ref().is_some_and(|r| {
                r.encrypted
                    .as_deref()
                    .and_then(|b| Active::decode(b).ok())
                    .is_some_and(|a| a.epoch == c.binding.facts.epoch.0)
            })
            && c.epoch.as_ref().is_some_and(|r| {
                !r.system_fields.0.is_empty()
                    && matches!(r.encrypted.as_deref().and_then(|b| Epoch::decode(b).ok()),
                    Some(DecodeOutcome::Supported(e)) if e.epoch_id == c.binding.facts.epoch.0
                        && e.record_key == c.binding.facts.record_key.0 && e.state == EpochState::Complete)
            })
    }

    /// Observational offer, revalidated each time. No expiry heuristic grants authority.
    pub fn resume_binding_offer(&self) -> Option<BindingId> {
        self.resume_binding_offer_with_work(&mut PolicyWork::default())
            .ok()
            .flatten()
    }

    pub(crate) fn resume_binding_offer_with_work(
        &self,
        work: &mut PolicyWork,
    ) -> Result<Option<BindingId>, RuntimeError> {
        let (context, preview) = {
            let s = self.state();
            work.check()?;
            let Some((p, ready)) = s.publication.preview.as_ref() else {
                return Ok(None);
            };
            if !ready || p.decision != ConnectDecision::ConfirmResume {
                return Ok(None);
            }
            let Some(context) = s.publication.resume.clone() else {
                return Ok(None);
            };
            (context, p.clone())
        };
        if self.validate_resume_providers(&context, &preview).is_err() {
            let mut s = self.state();
            work.check()?;
            if s.publication.preview.as_ref().is_some_and(|(p, _)| p.id == preview.id) && s.publication.resume.is_some()
            {
                work.effect()?;
                s.publication.resume = None;
            }
            return Ok(None);
        }
        let s = self.state();
        work.check()?;
        if s.publication.preview.as_ref() != Some(&(preview.clone(), true))
            || !Self::resume_reads_match(&context, &preview)
        {
            return Ok(None);
        }
        if self.validate_resume_context_locked(&s, &context).is_err() {
            return Ok(None);
        }
        Ok(Some(context.binding.id))
    }

    /// Explicit V26.2 admission consumes the retained V13 proof, never UI booleans.
    /// Activation is durable even if subsequent settle/scan fails; startup stays gated.
    pub fn resume_binding(&self, binding: BindingId, collection: &Collection) -> Result<(), RuntimeError> {
        self.resume_binding_retained(binding, None, collection)
    }

    pub fn confirm_connection_resume(&self, preview: &Preview, collection: &Collection) -> Result<(), RuntimeError> {
        self.resume_binding_retained(
            preview.resume_binding.ok_or(PublicationError::NotOffered)?,
            Some(preview),
            collection,
        )
    }

    fn resume_binding_retained(
        &self,
        binding: BindingId,
        expected: Option<&Preview>,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        let (context, preview) = {
            let mut s = self.state();
            let c = s.publication.resume.as_ref().ok_or(PublicationError::NotOffered)?;
            let (p, ready) = s.publication.preview.as_ref().ok_or(PublicationError::NotOffered)?;
            if !ready
                || p.decision != ConnectDecision::ConfirmResume
                || c.binding.id != binding
                || p.resume_binding != Some(binding)
                || !Self::resume_reads_match(c, p)
                || expected.is_some_and(|expected| expected != p)
            {
                return Err(PublicationError::NotOffered.into());
            }
            let retained = (c.clone(), p.clone());
            // Consume admission even on failure. A retry needs fresh actual reads.
            s.publication.preview = Some((retained.1.clone(), false));
            s.publication.resume = None;
            retained
        };
        self.validate_resume_providers(&context, &preview)?;
        self.validate_resume_context_locked(&self.state(), &context)?;
        self.wait_incoming_idle();
        let lease = collection.reader(ReaderRole::Cloud)?;
        if !collection.is_current()
            || collection.generation() != context.scan.generation
            || collection.path() != context.collection.path()
        {
            return Err(PublicationError::NotOffered.into());
        }
        // Immediate precommit provider reads run outside both guards.
        self.validate_resume_providers(&context, &preview)?;
        let durable_file =
            DurableFileIdentity::from_evidence(collection.open_evidence()?.cloud_file, context.scan.file)?;
        {
            let mut s = self.state();
            self.validate_resume_context_locked(&s, &context)?;
            if s.publication.preview.as_ref() != Some(&(preview, false)) || s.incoming.running {
                return Err(PublicationError::NotOffered.into());
            }
            let incarnation = s.incarnation.checked_add(1).ok_or(Error::InvalidData)?;
            let token = s.adapter.token.checked_add(1).ok_or(Error::InvalidData)?;
            let resumed = with_tracking_access(|| {
                s.store.as_mut().ok_or(Error::Database)?.resume_isolated_binding(
                    &context.binding,
                    context.scan.file,
                    durable_file,
                    &context.publish,
                    &context.adopt,
                )
            })
            .map_err(|e| {
                if matches!(e, Error::RecoveryRefused) {
                    PublicationError::NotOffered.into()
                } else {
                    RuntimeError::from(e)
                }
            })?;
            Self::adapter_close_exchange_locked(&mut s);
            s.incarnation = incarnation;
            s.adapter.token = token;
            // Keep the installed native object until its cancellation completion;
            // changed authority rejects every old callback and proof immediately.
            if let Some(sdk) = s.adapter.sdk.as_mut() {
                sdk.stopping = true;
            }
            s.publish_authority();
            s.adapter.identity_read = None;
            s.adapter.accepted_identity_read = None;
            s.adapter.fetch = None;
            s.adapter.scheduled = None;
            s.publication.resume = None;
            s.publication.preview = None;
            s.root_confirmed = false;
            s.root_fetch_qualified = false;
            s.root_reads.clear();
            s.policy = crate::cloud::policy::RuntimeEvidenceState::default();
            let retained_sdk = s.adapter.sdk.is_some();
            s.actions.rebind(retained_sdk);
            s.cloud.flagged.clear();
            s.cloud.acked_images.clear();
            s.scan_required = true;
            s.whole_complete = false;
            s.note_scan = None;
            s.last_capture = None;
            s.tracking_stopped = false;
            let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
            b.binding = resumed;
            b.file = context.scan.file;
            b.usable = true;
            b.verified = true;
        }
        drop(lease);
        self.qualify_provider_binding()?;
        // V2.4 step 8: reconcile_binding first settles re-key, ordinary apply,
        // and media apply intents, then takes the whole held-snapshot scan.
        self.reconcile_binding(collection, &self.media_service(collection)?, Duration::from_secs(30))?;
        Ok(())
    }

    /// Stored pending entities are a lower bound when native projection is
    /// unproved. Read bounded header pages without loading payload bodies.
    pub(super) fn known_unsent_lower_bounds(&self) -> Result<BTreeMap<codec::Kind, u64>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let mut counts = BTreeMap::new();
            for kind in crate::cloud::row_payload::KINDS {
                let mut count = 0u64;
                let mut after = None;
                loop {
                    let page = store.entity_count_page(b.binding.id, kind, after.as_deref())?;
                    if page.is_empty() { break; }
                    for e in page {
                        after = Some(e.key.clone());
                        if e.pending {
                            count = count.checked_add(1).ok_or(Error::InvalidData)?;
                        }
                    }
                }
                counts.insert(kind, count);
            }
            Ok(counts)
        })
    }

    /// Preview counts include actual isolated local edits which capture skipped.
    /// Read-only: no ACK, dirty mark, cursor, retry budget or file is changed.
    /// Reuse the preview's held snapshot; the Cloud reader cannot be leased twice.
    pub(super) fn resume_unsent_counts(
        &self,
        snapshot: &(impl ReadRows + ?Sized),
        media: &meguri_engine::media::MediaService,
    ) -> Result<ResumeUnsentCountsWithExactnessAndDependencyHolds, RuntimeError> {
        // Keep only keys/digests and the required notetype authority. Bodies
        // for notes, bases, ACKs and flights are never preview count inputs.
        let (binding, entities, mut layouts, expected) = {
            let s = self.state();
            let Some(b) = s.bound.as_ref().filter(|b| b.binding.facts.state == BindingState::Active || Self::resumable(&b.binding) || Self::layout_refused_locked(&s)) else {
                return Ok((BTreeMap::new(), true, BTreeSet::new()));
            };
            with_tracking_access(|| -> Result<_, RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                let mut entities = BTreeMap::new();
                let mut layouts = BTreeMap::new();
                for kind in crate::cloud::row_payload::KINDS {
                    let mut after = None;
                    loop {
                        let page = store.entity_count_page(b.binding.id, kind, after.as_deref())?;
                        if page.is_empty() { break; }
                        for e in page {
                            after = Some(e.key.clone());
                            let header = store.entity_header(b.binding.id, &e.name)?.ok_or(Error::InvalidData)?;
                            if kind == codec::Kind::Notetype {
                                let codec::Payload::Notetype(n) = header.local_payload(&store.connection, b.binding.id)? else {
                                    return Err(Error::InvalidData.into());
                                };
                                layouts.insert(n.id, n);
                            }
                            entities.insert((kind, e.key), (e.name, e.pending, header.facts.acked_row_digest,
                                header.facts.acked_tombstone(), header.facts.server_erev > 0 && header.facts.acked_payload.is_some()));
                        }
                    }
                }
                let expected = super::super::incoming_store::intent_layouts(&store.connection, b.binding.id,
                    b.file, &b.binding.facts.record_key.0, &store.directory, &store.apply_intents(b.binding.id)?)?;
                Ok((b.binding.id, entities, layouts, expected))
            })?
        };
        let mut exact = true;
        match incoming_impl::select_intent_layouts(snapshot, &mut layouts, expected) {
            Ok(()) => {},
            Err(Error::ValidatedUnprovedLayout { .. }) => exact = false,
            Err(e) => return Err(e.into()),
        }
        let mut pending: BTreeSet<_> = entities
            .iter()
            .filter(|(_, e)| e.1)
            .map(|(key, _)| key.clone())
            .collect();
        let view = crate::cloud::row_payload::ReadView(snapshot);
        let mut seen = BTreeSet::new();
        let mut dependency_held = BTreeSet::new();
        for kind in crate::cloud::row_payload::KINDS {
            let Some(table) = crate::cloud::row_payload::table(kind) else {
                continue;
            };
            let mut after = None;
            loop {
                let page = snapshot.row_page(table, after.as_ref())?;
                if page.is_empty() {
                    break;
                }
                for row in page {
                    after = Some(RowKey::from_stored(table, &row).map_err(meguri_storage::StorageError::from)?);
                    let key = (kind, crate::cloud::row_payload::key(kind, &row)?);
                    let projected = crate::cloud::row_payload::project_ordinary(&view, kind, &row, |id| {
                        layouts.get(&id).cloned().map(Ok)
                            .unwrap_or_else(|| crate::cloud::row_payload::missing_layout(&view, id))
                    });
                    let payload = match projected {
                        Ok(payload) => payload,
                        Err(Error::ValidatedUnprovedLayout { .. }) => {
                            exact = false;
                            seen.insert(key);
                            continue;
                        }
                        Err(e) => return Err(e.into()),
                    };
                    if entities.get(&key).is_some_and(|e| e.1) {
                        for reference in crate::cloud::publication_store::references(&payload)? {
                            if entities.get(&reference).is_none_or(|e| !e.4 || e.3) {
                                dependency_held.insert(entities[&key].0.clone());
                                break;
                            }
                        }
                    }
                    if entities
                        .get(&key)
                        .is_none_or(|e| payload.row_digest().ok() != Some(e.2))
                    {
                        pending.insert(key.clone());
                    }
                    seen.insert(key);
                }
            }
        }
        for (key, e) in &entities {
            if key.0 != codec::Kind::MediaName && !seen.contains(key) && !e.3 {
                pending.insert(key.clone());
            }
        }
        let feed = media.store().media_feed_snapshot().map_err(|_| Error::InvalidData)?;
        for item in feed.items {
            let key = (codec::Kind::MediaName, item.name.to_lowercase().into_bytes());
            let ack = if entities.contains_key(&key) {
                let s = self.state();
                with_tracking_access(|| -> Result<_, RuntimeError> {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    Ok(store.entity_image(binding, codec::Kind::MediaName, &key.1)?
                        .ok_or(Error::InvalidData)?.acked_payload)
                })?
            } else { None };
            let same = match (&item.state, ack.as_ref()) {
                (meguri_storage::media::MediaFileState::Present(f), Some(codec::Payload::MediaName(m))) => {
                    crate::cloud::media::io::sha1_text(&m.sha1) == f.sha1 && m.size == f.size
                }
                (meguri_storage::media::MediaFileState::Absent(_), _) => {
                    entities.get(&key).is_none_or(|e| e.3)
                }
                _ => false,
            };
            if !same {
                pending.insert(key);
            }
        }
        let mut counts = crate::cloud::row_payload::KINDS
            .into_iter()
            .map(|k| (k, 0))
            .collect::<BTreeMap<_, _>>();
        for (kind, _) in pending {
            *counts.get_mut(&kind).ok_or(Error::InvalidData)? += 1;
        }
        Ok((counts, exact, dependency_held))
    }
}
