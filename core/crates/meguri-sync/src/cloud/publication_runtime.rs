use super::*;
use crate::cloud::{
    control_codec::{self, DecodeOutcome, Epoch, EpochReason, EpochState, KindCount},
    publication::*,
    row_payload,
    workflow::UpdateKind,
};
use meguri_engine::device_state::{DeviceState, PendingDecisionReason};
use std::collections::BTreeMap;

impl Runtime {
    pub fn local_publication_tracking_information(
        &self,
    ) -> Result<Option<super::super::local_publication::LocalTrackingInformation>, RuntimeError> {
        Ok(self.local_publication_tracking()?.1)
    }
    /// Local metadata only. No account verification, provider or callback.
    /// Incarnation is a private consistency guard, never returned across FFI.
    pub(crate) fn local_publication_tracking(
        &self,
    ) -> Result<(u64, Option<super::super::local_publication::LocalTrackingInformation>), RuntimeError> {
        use super::super::local_publication::*;
        let s = self.state();
        let facts = with_tracking_access(|| -> Result<_, Error> {
            let Some(store) = s.store.as_ref() else {
                return Ok(None);
            };
            let bindings = store.publication_bindings()?;
            let mut isolated = Vec::new();
            for binding in &bindings {
                let BindingState::Isolated(reason) = binding.facts.state else {
                    continue;
                };
                let mut pending_by_kind = BTreeMap::new();
                for kind in row_payload::KINDS {
                    let mut count = 0u64;
                    let mut after = None;
                    loop {
                        let page = store.entity_count_page(binding.id, kind, after.as_deref())?;
                        if page.is_empty() {
                            break;
                        }
                        for entity in page {
                            after = Some(entity.key.clone());
                            if entity.pending {
                                count = count.checked_add(1).ok_or(Error::InvalidData)?;
                            }
                        }
                    }
                    pending_by_kind.insert(kind, count);
                }
                isolated.push(IsolatedLocalInformation {
                    pending_by_kind,
                    content: match reason {
                        IsolationReason::ReplacedLocally => IsolatedLocalContent::ReplacedLocally,
                        IsolationReason::FileMismatch => IsolatedLocalContent::FileMismatch,
                        _ => IsolatedLocalContent::Unverified,
                    },
                    backup: if reason == IsolationReason::FileMismatch {
                        LocalBackupKnowledge::NotTakenForExternalReplacement
                    } else {
                        LocalBackupKnowledge::Unknown
                    },
                });
            }
            let unresolved_conflict_copies = store.publication_unresolved_conflict_count()?;
            Ok(Some(LocalTrackingInformation {
                isolated,
                unresolved_conflict_copies,
            }))
        })?;
        Ok((s.incarnation, facts))
    }
    fn preview_retained_work_locked(s: &State, dependency_held: &std::collections::BTreeSet<String>) -> Result<RetainedWork, Error> {
        let Some(store) = s.store.as_ref() else { return Ok(RetainedWork::default()); };
        let mut work = RetainedWork { conflict_copies: store.publication_unresolved_conflict_count()?, ..RetainedWork::default() };
        let held: std::collections::BTreeSet<_> = s.cloud.deferred.iter()
            .chain(&s.cloud.incoming_held).chain(&s.policy.held_images).chain(dependency_held).collect();
        for binding in store.publication_bindings()? {
            let fetched: i64 = store.connection.query_row(
                "SELECT count(*) FROM inbox WHERE binding=?1", [binding.id.0.as_slice()], |row| row.get(0))?;
            work.fetched_unverified = work.fetched_unverified.checked_add(u64::try_from(fetched).map_err(|_| Error::InvalidData)?)
                .ok_or(Error::InvalidData)?;
            for kind in row_payload::KINDS {
                let mut after = None;
                loop {
                    let page = store.entity_count_page(binding.id, kind, after.as_deref())?;
                    if page.is_empty() { break; }
                    for entity in page {
                        after = Some(entity.key.clone());
                        if !entity.pending { continue; }
                        if matches!(binding.facts.state, BindingState::Isolated(_)) {
                            work.isolated_unsent = work.isolated_unsent.checked_add(1).ok_or(Error::InvalidData)?;
                        }
                        if s.bound.as_ref().is_some_and(|b| b.binding.id == binding.id) && held.contains(&entity.name) {
                            work.held = work.held.checked_add(1).ok_or(Error::InvalidData)?;
                        }
                    }
                }
            }
        }
        Ok(work)
    }
    fn check_absent_preview_file(&self, id: u64) -> Result<(), RuntimeError> {
        let absent = self
            .state()
            .publication
            .preview
            .as_ref()
            .is_some_and(|(preview, _)| preview.id == id && preview.native_file.is_none());
        if absent {
            match self.directory.join("collection.anki2").symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => return Err(meguri_storage::StorageError::from(error).into()),
                Ok(_) => return Err(PublicationError::AuthorityChanged.into()),
            }
        }
        Ok(())
    }
    /// V13 discovery never bootstraps a file, sidecar publication or binding.
    /// The Host checks NoCollection and retains its real generation authority.
    pub fn begin_connection_without_collection(&self) -> Result<PreviewWork, RuntimeError> {
        let path = self.directory.join("collection.anki2");
        let absent = || -> Result<(), RuntimeError> {
            match path.symlink_metadata() {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
                Err(error) => Err(meguri_storage::StorageError::from(error).into()),
                Ok(_) => Err(PublicationError::AuthorityChanged.into()),
            }
        };
        absent()?;
        let account = self.publication_account()?;
        let decision_before = DeviceState::for_collection(&path).publication_decision()?;
        absent()?;
        let mut s = self.state();
        if s.bound.is_some() || s.cloud.generation.is_some() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        s.publication.next_preview = s.publication.next_preview.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.publication.next_preview;
        s.publication.resume = None;
        let retained_work = with_tracking_access(|| Self::preview_retained_work_locked(&s, &Default::default()))?;
        s.publication.preview = Some((
            Preview {
                id,
                local: Counts {
                    counts: BTreeMap::new(),
                    empty: false,
                },
                cloud: None,
                published_at_ms: None,
                decision: ConnectDecision::NoCollection,
                account,
                native_file: None,
                decision_before,
                active: None,
                active_epoch: None,
                unpublished_by_kind: BTreeMap::new(),
                resume_binding: None,
                unsent_by_kind: BTreeMap::new(),
                unsent_counts_exact: true,
                retained_work,
            },
            false,
        ));
        Ok(PreviewWork::ReadActive { id })
    }
    pub(super) fn require_supported_publication(op: &PublishOp) -> Result<(), RuntimeError> {
        // Reset publication supports only the non-destructive first-root cases.
        // Unsupported root outcomes stay at Bound; adoption has its own guard.
        op.validate().map_err(Into::into)
    }
    fn reset_root_missing_locked(s: &State, op: &PublishOp) -> bool {
        s.publication.reset_root_missing.as_ref().is_some_and(|(read, guard)| {
            read.kind == OwnKind::ReadResetMediaRoot
                && read.op == op.id
                && read.incarnation == s.incarnation
                && read.generation == s.bound.as_ref().map_or(0, |b| b.evidence.generation.0)
                && read.decision_before == op.fence_before
                && Self::instance_locked(s).is_ok_and(|instance| instance == read.instance)
                && *guard == Self::adapter_guard_locked(s)
        })
    }
    pub fn attach_key_bytes(&self, source: Arc<dyn KeyByteSource>) {
        *self.key_bytes.lock().unwrap_or_else(PoisonError::into_inner) = Some(source);
    }
    pub(super) fn publication_decision(&self) -> Result<DecisionSnapshot, RuntimeError> {
        DeviceState::for_collection(&self.directory.join("collection.anki2"))
            .publication_decision()?
            .ok_or_else(|| PublicationError::AuthorityChanged.into())
    }
    pub(super) fn publication_account(&self) -> Result<OpaqueBytes, RuntimeError> {
        match self.inputs.identity.read_identity(&self.inputs.scope) {
            IdentityStatus::Available(account) if !account.0.is_empty() => Ok(account),
            _ => Err(PublicationError::AccountUnavailable.into()),
        }
    }
    pub(super) fn publication_random_id(&self) -> Result<[u8; 16], RuntimeError> {
        let random = self
            .random
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let mut id = [0; 16];
        id[..8].copy_from_slice(&random.next_u64().to_be_bytes());
        id[8..].copy_from_slice(&random.next_u64().to_be_bytes());
        Ok(id)
    }
    /// Read actual local rows. Counts do not prepare tracking, acknowledge records,
    /// merge collections, or change the sidecar decision.
    pub fn begin_connection_preview(
        &self,
        collection: &Collection,
        media: &meguri_engine::media::MediaService,
    ) -> Result<PreviewWork, RuntimeError> {
        if self.state().tracking_stopped {
            return Err(Error::Database.into());
        }
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let incarnation = self.state().incarnation;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let local = super::super::local_publication::count_local_rows(&*snapshot, media.tracked_names()?.names.len())?;
        let layout_refused = match self.preview_native_layouts(&*snapshot, file, &account, &decision) {
            Ok(()) => false,
            Err(RuntimeError::Tracking(Error::ValidatedUnprovedLayout { .. })) => true,
            Err(error) => return Err(error),
        };
        let (unsent_by_kind, counted_exact, dependency_held) = if layout_refused {
            (self.known_unsent_lower_bounds()?, false, Default::default())
        } else { self.resume_unsent_counts(&*snapshot, media)? };
        let unsent_counts_exact = counted_exact && !layout_refused;
        if self.inputs.files.identity(collection.path())? != file
            || self.publication_decision()? != decision
            || self.publication_account()? != account
            || !collection.is_current()
            || collection.last_seq() != snapshot.seq()
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let mut s = self.state();
        if s.incarnation != incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.publication.next_preview = s.publication.next_preview.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.publication.next_preview;
        s.publication.resume = if unsent_counts_exact {
            self.retain_resume_context_locked(&s, collection, file, &decision, &account)?
        } else {
            let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
            s.layout_refusal = Some(binding);
            s.scan_required = true;
            s.whole_complete = false;
            s.publish_authority();
            None
        };
        let mut unpublished_by_kind = row_payload::KINDS
            .into_iter()
            .map(|k| (k, 0))
            .collect::<BTreeMap<_, _>>();
        with_tracking_access(|| -> Result<(), Error> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            if let Some(b) = store.publication_binding_for_account(&account)?
                && matches!(
                    b.facts.state,
                    BindingState::Isolated(IsolationReason::ReplacedLocally | IsolationReason::FileMismatch)
                )
            {
                for kind in row_payload::KINDS {
                    let mut after = None;
                    loop {
                        let page = store.entity_count_page(b.id, kind, after.as_deref())?;
                        if page.is_empty() {
                            break;
                        }
                        for entity in page {
                            after = Some(entity.key.clone());
                            if entity.pending {
                                *unpublished_by_kind.get_mut(&kind).ok_or(Error::InvalidData)? += 1;
                            }
                        }
                    }
                }
            }
            Ok(())
        })?;
        let retained_work = with_tracking_access(|| Self::preview_retained_work_locked(&s, &dependency_held))?;
        s.publication.preview = Some((
            Preview {
                id,
                local,
                cloud: None,
                published_at_ms: None,
                decision: ConnectDecision::ConfirmUpload,
                account,
                native_file: Some(file),
                decision_before: Some(decision),
                active: None,
                active_epoch: None,
                unpublished_by_kind,
                resume_binding: None,
                unsent_by_kind,
                unsent_counts_exact,
                retained_work,
            },
            false,
        ));
        Ok(PreviewWork::ReadActive { id })
    }
    /// Deliver only an actual record-ID read of Active. Missing and unreadable are
    /// distinct; unreadable Active retains its conditional-save system fields.
    pub fn connection_active_read(&self, id: u64, read: RecordRead) -> Result<PreviewWork, RuntimeError> {
        self.validate_connection_resume(id)?;
        let account = self.publication_account()?;
        self.check_absent_preview_file(id)?;
        let mut s = self.state();
        let (p, ready) = s
            .publication
            .preview
            .as_mut()
            .filter(|(p, _)| p.id == id && p.account == account)
            .ok_or(PublicationError::AuthorityChanged)?;
        if *ready || p.active.is_some() {
            return Err(PublicationError::AuthorityChanged.into());
        }
        match read {
            RecordRead::Missing | RecordRead::ZoneMissing => {
                *ready = true;
                Ok(PreviewWork::Ready(Box::new(p.clone())))
            }
            RecordRead::Found(record) => {
                if record.system_fields.0.is_empty() {
                    return Err(PublicationError::UnreadableControl.into());
                }
                let active = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::Active::decode(e).ok());
                p.active = Some(record);
                if let Some(active) = active {
                    let epoch = EpochId(active.epoch);
                    p.active_epoch = Some(epoch);
                    Ok(PreviewWork::ReadEpoch { id, epoch })
                } else {
                    p.decision = ConnectDecision::CloudUnreadable;
                    *ready = true;
                    Ok(PreviewWork::Ready(Box::new(p.clone())))
                }
            }
            RecordRead::Failed => Err(PublicationError::AccountUnavailable.into()),
        }
    }
    pub fn connection_epoch_read(&self, id: u64, read: RecordRead) -> Result<PreviewWork, RuntimeError> {
        self.validate_connection_resume(id)?;
        let account = self.publication_account()?;
        self.check_absent_preview_file(id)?;
        let mut s = self.state();
        let (p, ready) = s
            .publication
            .preview
            .as_mut()
            .filter(|(p, _)| p.id == id && p.account == account)
            .ok_or(PublicationError::AuthorityChanged)?;
        if *ready {
            return Err(PublicationError::AuthorityChanged.into());
        }
        let RecordRead::Found(record) = read else {
            return Err(PublicationError::EpochIncomplete.into());
        };
        if record.system_fields.0.is_empty() {
            return Err(PublicationError::UnreadableControl.into());
        }
        let Some(DecodeOutcome::Supported(epoch)) = record.encrypted.as_deref().and_then(|b| Epoch::decode(b).ok())
        else {
            return Err(PublicationError::UnreadableControl.into());
        };
        if Some(EpochId(epoch.epoch_id)) != p.active_epoch || epoch.state != EpochState::Complete {
            return Err(PublicationError::EpochIncomplete.into());
        }
        let mut counts = row_payload::KINDS
            .into_iter()
            .map(|k| (k, 0))
            .collect::<BTreeMap<_, _>>();
        for count in epoch.counts {
            counts.insert(count.kind, count.live);
        }
        p.cloud = Some(Counts { counts, empty: false });
        p.published_at_ms = Some(epoch.published_at_ms);
        p.decision = if p.native_file.is_none() {
            ConnectDecision::NoCollection
        } else if p.local.empty && p.unsent_counts_exact {
            ConnectDecision::ConfirmAdoption
        } else {
            ConnectDecision::ChooseCollection
        };
        *ready = true;
        let mut preview = p.clone();
        if let Some(mut context) = s.publication.resume.clone() {
            context.active = preview.active.clone();
            context.epoch = Some(record);
            if self.validate_resume_context_locked(&s, &context).is_ok() && Self::resume_reads_match(&context, &preview)
            {
                preview.decision = ConnectDecision::ConfirmResume;
                preview.resume_binding = Some(context.binding.id);
                s.publication.resume = Some(context);
            }
        }
        s.publication.preview = Some((preview.clone(), true));
        Ok(PreviewWork::Ready(Box::new(preview)))
    }
    /// Cancellation of an older native sheet cannot dismiss a newer preview.
    pub fn cancel_connection_preview_id(&self, id: u64) -> Result<(), RuntimeError> {
        let mut s = self.state();
        if !s.publication.preview.as_ref().is_some_and(|(p, _)| p.id == id) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        s.publication.preview = None;
        s.publication.resume = None;
        Ok(())
    }
    pub fn cancel_connection_preview(&self) {
        let mut s = self.state();
        s.publication.preview = None;
        s.publication.resume = None;
    }
    pub fn publication_op(&self) -> Result<Option<PublishOp>, RuntimeError> {
        self.read_recovery_op(TrackingStore::publication_op)
    }
    pub(super) fn check_preview_authority(&self, p: &Preview, collection: &Collection) -> Result<(), RuntimeError> {
        if self.publication_account()? != p.account
            || Some(self.publication_decision()?) != p.decision_before
            || Some(self.inputs.files.identity(collection.path())?) != p.native_file
            || !collection.is_current()
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        Ok(())
    }
    /// Stop preserves every retained row, intent, image and acknowledgement.
    /// The retained Host supplies collection/generation ownership.
    pub fn stop_using_icloud(&self, collection: &Collection) -> Result<(), RuntimeError> {
        self.stop_using_icloud_checked(collection, None)
    }
    fn stop_using_icloud_checked(&self, collection: &Collection, preview: Option<&Preview>) -> Result<(), RuntimeError> {
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let identity = self.inputs.identity.read_identity(&self.inputs.scope);
        {
            let mut s = self.state();
            let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
            if let Some(p) = preview
                && (s.publication.preview.as_ref() != Some(&(p.clone(), true))
                    || p.native_file != Some(file) || p.decision_before.as_ref() != Some(&decision)
                    || p.account != b.binding.facts.account
                    || !matches!(&identity, IdentityStatus::Available(account) if account == &p.account))
            { return Err(PublicationError::AuthorityChanged.into()); }
            if matches!(&identity, IdentityStatus::Available(account) if account != &b.binding.facts.account) {
                return Err(PublicationError::AccountMismatch.into());
            }
            if !collection.is_current() || b.binding.scope != self.inputs.scope || b.file != file
                || b.evidence.generation != collection.generation()
                || b.binding.facts.publication != decision.publication.identity
            { return Err(PublicationError::AuthorityChanged.into()); }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                if store.publication_op()?.is_some() || store.adoption_op()?.is_some() {
                    return Err(PublicationError::Busy.into());
                }
                Ok(())
            })?;
            Self::stop_syncing_locked(&mut s)?;
        }
        self.wait_incoming_idle();
        let mut s = self.state();
        s.publication.preview = None;
        s.publication.resume = None;
        Ok(())
    }
    pub fn keep_local_only(&self, collection: &Collection, preview: Option<&Preview>) -> Result<(), RuntimeError> {
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        let owner = {
            let s = self.state();
            (s.incarnation, s.bound.as_ref().map(|b| b.binding.id))
        };
        if let Some(p) = preview {
            self.check_preview_authority(p, collection)?;
            if self.state().publication.preview.as_ref() != Some(&(p.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
        } else if decision.publication.decision != meguri_engine::device_state::PublicationDecision::Pending {
            let lease = collection.reader(ReaderRole::Cloud)?;
            let snapshot = lease.snapshot()?;
            let account = self.publication_account()?;
            match self.preview_native_layouts(&*snapshot, file, &account, &decision) {
                Err(RuntimeError::Tracking(Error::ValidatedUnprovedLayout { .. })) => {},
                Err(error) => return Err(error),
                Ok(()) => return Err(PublicationError::NotOffered.into()),
            }
        }
        {
            let s = self.state();
            with_tracking_access(|| -> Result<(), RuntimeError> {
                if let Some(store) = &s.store
                    && (store.publication_op()?.is_some() || store.adoption_op()?.is_some())
                { return Err(PublicationError::Busy.into()); }
                Ok(())
            })?;
        }
        // Stop first: a sidecar failure remains a stopped connection, with all
        // original evidence retained, rather than reopening cloud admission.
        if self.state().bound.is_some() { self.stop_using_icloud_checked(collection, preview)?; }
        let guard = {
            let s = self.state();
            if (s.incarnation, s.bound.as_ref().map(|b| b.binding.id)) != owner {
                return Err(PublicationError::AuthorityChanged.into());
            }
            (s.incarnation, s.adapter.token, s.bound.as_ref().map(|b| b.binding.id))
        };
        if self.inputs.files.identity(collection.path())? != file || self.publication_decision()? != decision {
            return Err(PublicationError::AuthorityChanged.into());
        }
        collection.choose_cloud_local_only(&decision, file.pair(), |apply| -> Result<(), RuntimeError> {
            let mut s = self.state();
            if (s.incarnation, s.adapter.token, s.bound.as_ref().map(|b| b.binding.id)) != guard
                || s.bound.as_ref().is_some_and(|b| b.usable || b.verified || b.file != file
                    || b.binding.scope != self.inputs.scope || b.evidence.generation != collection.generation()
                    || b.binding.facts.publication != decision.publication.identity)
            { return Err(PublicationError::AuthorityChanged.into()); }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                if let Some(store) = &s.store
                    && (store.publication_op()?.is_some() || store.adoption_op()?.is_some())
                { return Err(PublicationError::Busy.into()); }
                Ok(())
            })?;
            if !apply()? { return Err(PublicationError::AuthorityChanged.into()); }
            s.publication.preview = None;
            s.publication.resume = None;
            s.publish_authority();
            Ok(())
        })
    }
    /// Explicit user confirmation. Both full-width keys are obtained before the
    /// first publication commit, outside engine, runtime-state and tracking locks.
    pub fn confirm_connection_publication(
        &self,
        p: &Preview,
        collection: &Collection,
    ) -> Result<[u8; 16], RuntimeError> {
        // Confirmed only records authorization. Bind settles old intents and
        // waits for incoming work before isolating the prior binding (V17.6).
        self.check_preview_authority(p, collection)?;
        let decision_before = p.decision_before.as_ref().ok_or(PublicationError::NotOffered)?;
        let native_file = p.native_file.ok_or(PublicationError::NotOffered)?;
        {
            let s = self.state();
            if s.publication.preview.as_ref() != Some(&(p.clone(), true)) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            if with_tracking_access(|| -> Result<bool, Error> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                Ok(store.publication_op()?.is_some() || store.adoption_op()?.is_some())
            })? {
                return Err(PublicationError::Busy.into());
            }
        }
        let source = self
            .key_bytes
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(PublicationError::KeyUnavailable)?;
        let record_key = source.draw_key().map_err(|_| PublicationError::KeyUnavailable)?;
        let media_key = source.draw_key().map_err(|_| PublicationError::KeyUnavailable)?;
        let id = self.publication_random_id()?;
        let epoch = EpochId(self.publication_random_id()?);
        let now = self.adapter_now()?;
        let crt = {
            let lease = collection.reader(ReaderRole::Cloud)?;
            lease.col()?.created.0
        };
        self.check_preview_authority(p, collection)?;
        let mut s = self.state();
        if s.publication.preview.as_ref() != Some(&(p.clone(), true)) {
            return Err(PublicationError::AuthorityChanged.into());
        }
        with_tracking_access(|| -> Result<(), RuntimeError> {
            let store = s.store.as_mut().ok_or(Error::Database)?;
            let before = store.publication_binding_for_account(&p.account)?;
            let reason = match decision_before.pending_reason {
                Some(PendingDecisionReason::Restore) => EpochReason::Restore,
                Some(PendingDecisionReason::ColpkgImport) => EpochReason::CollectionPackageImport,
                Some(PendingDecisionReason::ManualImport) => EpochReason::ManualAnkiImport,
                None if p.active.is_some() => EpochReason::ReplaceCloud,
                None => EpochReason::Initial,
            };
            store.confirm_publication(&PublishOp {
                id,
                step: PublishStep::Confirmed,
                binding_before: before.map(|b| BindingBefore {
                    id: b.id,
                    state: b.facts.state,
                }),
                binding_after: None,
                account: p.account.clone(),
                confirmed_publication: decision_before.publication.identity,
                confirmed_file: native_file,
                fence_before: decision_before.clone(),
                epoch,
                record_key: RecordKey(record_key),
                media_key: RecordKey(media_key),
                active: p.active.clone(),
                active_epoch: p.active_epoch,
                epoch_system_fields: None,
                complete_system_fields: None,
                root_acked: false,
                switch_sent: false,
                switch_conflict: None,
                active_attempt: None,
                started_at_ms: now,
                retry_after_ms: None,
                crt,
                reason: reason as u8,
            })?;
            Ok(())
        })?;
        s.publication.preview = None;
        s.publication.resume = None;
        s.publication.waiting = false;
        s.publication.reset_root_missing = None;
        Ok(id)
    }
    /// Event trigger, never a retry timer. One failed attempt remains waiting
    /// until a new policy/platform event explicitly reaches this entry point.
    pub fn publication_trigger(&self) {
        self.state().publication.waiting = false;
    }
    fn publication_epoch(op: &PublishOp, state: EpochState, counts: Vec<KindCount>) -> Result<Vec<u8>, RuntimeError> {
        Epoch {
            epoch_id: op.epoch.0,
            record_key: op.record_key.0,
            crt: op.crt,
            ver: 18,
            state,
            reason: EpochReason::try_from(u64::from(op.reason)).map_err(|_| Error::InvalidData)?,
            published_at_ms: op.started_at_ms,
            counts,
        }
        .encode()
        .map_err(|_| Error::InvalidData.into())
    }
    fn publication_exempt_locked(s: &State) -> bool {
        let Some(store) = &s.store else {
            return false;
        };
        let Ok(Some(op)) = store.publication_op() else {
            return false;
        };
        Self::require_supported_publication(&op).is_ok()
            && op.binding_after == s.bound.as_ref().map(|b| b.binding.id)
            && matches!(op.step, PublishStep::Zones | PublishStep::Sent | PublishStep::Complete)
    }
    pub(super) fn publication_allows_send_locked(s: &State) -> bool {
        Self::publication_exempt_locked(s)
    }
    pub(super) fn publication_holds_binding_locked(s: &State) -> bool {
        // Caller holds tracking access. No nested gate acquisition here.
        let Some(store) = &s.store else {
            return false;
        };
        if store.adoption_op().map_or(true, |op| op.is_some()) {
            return true;
        }
        let op = match store.publication_op() {
            Ok(Some(op)) => op,
            Ok(None) => return false,
            Err(_) => return true,
        };
        Self::require_supported_publication(&op).is_err()
            || (op.step == PublishStep::Confirmed && s.publication.restarted_confirmed)
            || matches!(op.step, PublishStep::Bound | PublishStep::Cancelling)
    }
    /// One actual local transition or one transport request at a time. Returning
    /// Scan/SendEntities grants existing Runtime/SDK work; it does not model it.
    pub fn next_publication_work(&self, collection: &Collection) -> Result<PublicationWork, RuntimeError> {
        self.next_publication_work_with_work(collection, &mut crate::cloud::policy::PolicyWork::default())
    }
    pub fn next_publication_work_with_work(
        &self,
        collection: &Collection,
        work: &mut crate::cloud::policy::PolicyWork,
    ) -> Result<PublicationWork, RuntimeError> {
        work.check()?;
        if let Some(work) = self.next_adoption_work_with_work(Some(collection), work)? {
            return Ok(PublicationWork::Adoption(work));
        }
        let Some(mut op) = self.publication_op()? else {
            return Ok(PublicationWork::Idle);
        };
        Self::require_supported_publication(&op)?;
        if op.step == PublishStep::Cancelling {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Cancel,
            });
        }
        let account = match self.publication_account() {
            Ok(a) => a,
            Err(_) => return Ok(PublicationWork::Idle),
        };
        self.refresh_identity_with_work(work)?;
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        if op.step == PublishStep::Confirmed {
            let before_matches = {
                let s = self.state();
                with_tracking_access(|| -> Result<bool, Error> {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    Ok(match &op.binding_before {
                        Some(before) => store.binding(before.id)?.is_some_and(|b| b.facts.state == before.state),
                        None => true,
                    })
                })?
            };
            if account != op.account
                || decision != op.fence_before
                || decision.publication.identity != op.confirmed_publication
                || file != op.confirmed_file
                || !before_matches
            {
                let mut s = self.state();
                with_tracking_access(|| -> Result<(), RuntimeError> {
                    work.effect()?;
                    Ok(s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .discard_confirmed_publication(&op)?)
                })?;
                return Ok(PublicationWork::Idle);
            }
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Bind,
            });
        }
        let desired = op.decision_written(&self.inputs.scope)?;
        if op.step == PublishStep::Bound && decision != op.fence_before && decision != desired {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::Cancel,
            });
        }
        if account != op.account
            || file != op.confirmed_file
            || decision.publication.identity != op.confirmed_publication
            || !collection.is_current()
            || (op.step != PublishStep::Bound && decision != desired)
        {
            return Ok(PublicationWork::Idle);
        }
        if op.step == PublishStep::Bound && decision != desired {
            return Ok(PublicationWork::Local {
                op: op.id,
                work: LocalWork::WriteDecision,
            });
        }
        let now = self.adapter_now()?;
        let mut s = self.state();
        if s.adapter.paused
            || s.tracking_stopped
            || s.publication.waiting
            || s.publication.running.is_some()
            || op.switch_conflict.is_some()
            || op.retry_after_ms.is_some_and(|t| now < t)
            || !s
                .bound
                .as_ref()
                .is_some_and(|b| Some(b.binding.id) == op.binding_after && b.usable && b.verified)
        {
            return Ok(PublicationWork::Idle);
        }
        let current = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.publication_op())?;
        // Entity results may advance this same operation while the provider
        // reads run. Only that step change preserves the validated snapshot.
        if op.step == PublishStep::Zones && current.as_ref().is_some_and(|op| op.step == PublishStep::Sent) {
            op.step = PublishStep::Sent;
        }
        if current.as_ref() != Some(&op) {
            return Err(RuntimeError::StaleIncarnation);
        }
        if matches!(op.step, PublishStep::Zones | PublishStep::Sent | PublishStep::Complete)
            && (s.scan_required || !s.whole_complete)
        {
            return Ok(PublicationWork::Scan);
        }
        let zone = crate::cloud::adapter::collection_zone(op.epoch);
        let save = |zone: &str, name: &str, record_type: &str, encrypted: Vec<u8>, if_unchanged| OwnOperation::Save {
            zone: zone.to_owned(),
            name: name.to_owned(),
            record_type: record_type.to_owned(),
            encrypted,
            if_unchanged,
        };
        let (kind, operation) = match op.step {
            PublishStep::Complete if s.publication.recreate_active_root == Some(op.id) => (
                OwnKind::CreateZones,
                OwnOperation::CreateZones(vec!["meguri-root".into()]),
            ),
            PublishStep::Bound if !s.publication.zones_created => (
                OwnKind::CreateZones,
                OwnOperation::CreateZones(vec![zone.clone(), "meguri-root".into(), "meguri-media".into()]),
            ),
            PublishStep::Bound
                if !op.root_acked && op.is_encrypted_reset() && !Self::reset_root_missing_locked(&s, &op) =>
            {
                s.publication.reset_root_missing = None;
                (
                    OwnKind::ReadResetMediaRoot,
                    OwnOperation::Read {
                        zone: "meguri-media".into(),
                        name: "root".into(),
                    },
                )
            }
            PublishStep::Bound if !op.root_acked => (
                OwnKind::SaveMediaRoot,
                save(
                    "meguri-media",
                    "root",
                    "MGMediaRoot",
                    control_codec::MediaRoot {
                        media_key: op.media_key.0,
                        created_at_ms: op.started_at_ms,
                    }
                    .encode()
                    .map_err(|_| Error::InvalidData)?,
                    None,
                ),
            ),
            PublishStep::Bound => (
                OwnKind::SavePublishingEpoch,
                save(
                    &zone,
                    "epoch",
                    "MGEpoch",
                    Self::publication_epoch(&op, EpochState::Publishing, Vec::new())?,
                    None,
                ),
            ),
            PublishStep::Zones => {
                if s.scan_required || !s.whole_complete {
                    return Ok(PublicationWork::Scan);
                }
                return Ok(PublicationWork::SendEntities);
            }
            PublishStep::Sent => {
                let mut counts = Vec::new();
                with_tracking_access(|| -> Result<(), RuntimeError> {
                    let store = s.store.as_ref().ok_or(Error::Database)?;
                    for kind in row_payload::KINDS {
                        let mut after = None;
                        let mut live = 0;
                        loop {
                            let page = store.entity_page(
                                op.binding_after.ok_or(Error::InvalidData)?,
                                kind,
                                after.as_deref(),
                            )?;
                            if page.is_empty() {
                                break;
                            }
                            for e in page {
                                after = Some(e.key);
                                if e.server_erev > 0 {
                                    live += 1;
                                }
                            }
                        }
                        counts.push(KindCount { kind, live });
                    }
                    Ok(())
                })?;
                (
                    OwnKind::SaveCompleteEpoch,
                    save(
                        &zone,
                        "epoch",
                        "MGEpoch",
                        Self::publication_epoch(&op, EpochState::Complete, counts)?,
                        op.epoch_system_fields.clone(),
                    ),
                )
            }
            PublishStep::Complete if !s.publication.own_epoch_read => (
                OwnKind::ReadOwnEpoch,
                OwnOperation::Read {
                    zone,
                    name: "epoch".into(),
                },
            ),
            PublishStep::Complete => {
                let bytes = control_codec::Active {
                    epoch: op.epoch.0,
                    activated_at_ms: now,
                    previous: op.active_epoch.map(|e| e.0),
                    replaced_through_ms: None,
                }
                .encode()
                .map_err(|_| Error::InvalidData)?;
                // Holding this request has not submitted a cloud mutation.
                work.effect()?;
                s.publication.own_epoch_read = false;
                (
                    OwnKind::SaveActive,
                    save(
                        "meguri-root",
                        "active",
                        "MGActive",
                        bytes,
                        op.active.as_ref().map(|r| r.system_fields.clone()),
                    ),
                )
            }
            _ => return Ok(PublicationWork::Idle),
        };
        work.effect()?;
        s.publication.next_attempt = s.publication.next_attempt.checked_add(1).ok_or(Error::InvalidData)?;
        let request = OwnRequest {
            op: op.id,
            attempt: s.publication.next_attempt,
            kind,
            operation,
            incarnation: s.incarnation,
            generation: collection.generation().0,
            instance: Self::instance_locked(&s)?,
            decision_before: op.fence_before.clone(),
            decision_current: desired,
        };
        if kind == OwnKind::ReadResetMediaRoot {
            s.publication.reset_root_read_guard = Some(Self::adapter_guard_locked(&s));
        }
        s.publication.submitted = false;
        s.publication.running = Some(request.clone());
        s.publication.own_identity_after = s.adapter.next_identity;
        Ok(PublicationWork::Own(Box::new(request)))
    }
    pub fn perform_publication_local(
        &self,
        id: [u8; 16],
        work: LocalWork,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        let incarnation = self.state().incarnation;
        let Some(mut op) = self.publication_op()?.filter(|o| o.id == id) else {
            return Err(PublicationError::NotOffered.into());
        };
        Self::require_supported_publication(&op)?;
        if work == LocalWork::Cancel {
            return self.cancel_publication(id, collection);
        }
        let account = self.publication_account()?;
        let decision = self.publication_decision()?;
        let file = self.inputs.files.identity(collection.path())?;
        if account != op.account
            || file != op.confirmed_file
            || decision.publication.identity != op.confirmed_publication
        {
            return Err(PublicationError::AuthorityChanged.into());
        }
        match work {
            LocalWork::Bind if op.step == PublishStep::Confirmed && decision == op.fence_before => {
                let checked = self.check_replacement_work(collection)?;
                let id = BindingId(self.publication_random_id()?);
                let now = self.adapter_now()?;
                // Providers can suspend, reopen or replace the collection. Read
                // their final values before pinning the open and taking guards.
                let account = self.publication_account()?;
                let file = self.inputs.files.identity(collection.path())?;
                let decision = self.publication_decision()?;
                if account != op.account || file != op.confirmed_file || decision != op.fence_before {
                    return Err(PublicationError::AuthorityChanged.into());
                }
                {
                    let _lease = collection.reader(ReaderRole::Cloud)?;
                    let evidence = collection.open_evidence()?;
                    let durable_file = DurableFileIdentity::from_evidence(evidence.cloud_file, file)?;
                    let mut s = self.state();
                    if s.incarnation != incarnation
                        || s.incoming.running || collection.last_seq() != checked.seq || file != checked.file
                        || !collection.is_current()
                        || evidence.generation != collection.generation()
                    {
                        return Err(RuntimeError::StaleIncarnation);
                    }
                    with_tracking_access(|| {
                        s.store
                            .as_mut()
                            .ok_or(Error::Database)?
                            .bind_publication(&mut op, id, now, durable_file, Some(&checked))
                    })?;
                }
                self.bind(collection)?;
                self.refresh_identity()?;
            }
            LocalWork::WriteDecision if op.step == PublishStep::Bound => {
                let desired = op.decision_written(&self.inputs.scope)?;
                if decision != desired && !collection.resolve_cloud_publication_decision(op.id, &op.fence_before)? {
                    return Err(PublicationError::AuthorityChanged.into());
                }
                meguri_model::faultpoint!("cloud.publish.after_fence_clear").map_err(Error::from)?;
                self.bind(collection)?;
                self.refresh_identity()?;
            }
            _ => return Err(PublicationError::NotOffered.into()),
        }
        // Consent precedes these guarded local transitions. Only their actual
        // active binding and matching settled decision can qualify the required
        // publication SDK work; discovery cannot reach this path. This grants
        // no completed scan, ACK or ready baseline.
        self.qualify_provider_binding()?;
        Ok(())
    }
    /// The executor invokes this at EACH synchronous transport enqueue, after
    /// any suspension. The callback must return acknowledged queue acceptance,
    /// never a deferred operation. Wait for replies after this method returns.
    /// No provider or transport callback runs under Runtime/tracking locks.
    pub fn submit_publication_request<T>(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        self.submit_publication_acknowledged(request, collection, submit, |_| true)
    }
    /// Native queues report synchronous admission separately from their later
    /// reply. False proves this operation was never added to the queue.
    pub fn submit_publication_queue(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> bool,
    ) -> Result<bool, RuntimeError> {
        self.submit_publication_acknowledged(request, collection, submit, |accepted| *accepted)
    }
    fn submit_publication_acknowledged<T>(
        &self,
        request: &OwnRequest,
        collection: &Collection,
        submit: impl FnOnce() -> T,
        accepted: impl FnOnce(&T) -> bool,
    ) -> Result<T, RuntimeError> {
        let _lease = collection.reader(ReaderRole::Cloud)?;
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let mut prior_active = None;
        {
            let mut s = self.state();
            if s.publication.running.as_ref() != Some(request)
                || s.incarnation != request.incarnation
                || Self::instance_locked(&s)? != request.instance
                || collection.generation().0 != request.generation
                || decision != request.decision_current
                || s.adapter.paused
                || s.tracking_stopped
                || (s.publication.submitted && request.kind != OwnKind::CreateZones)
            {
                return Err(RuntimeError::StaleIncarnation);
            }
            with_tracking_access(|| -> Result<(), RuntimeError> {
                let store = s.store.as_ref().ok_or(Error::Database)?;
                let mut op = store
                    .publication_op()?
                    .filter(|o| o.id == request.op)
                    .ok_or(RuntimeError::StaleIncarnation)?;
                Self::require_supported_publication(&op)?;
                if op.fence_before != request.decision_before
                    || op.decision_written(&self.inputs.scope)? != decision
                    || op.account != account
                    || op.confirmed_file != file
                    || !s.bound.as_ref().is_some_and(|b| {
                        Some(b.binding.id) == op.binding_after
                            && b.evidence.generation.0 == request.generation
                            && b.usable
                            && b.verified
                    })
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                if request.kind == OwnKind::ReadResetMediaRoot
                    && s.publication.reset_root_read_guard.as_ref() != Some(&Self::adapter_guard_locked(&s))
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                if request.kind == OwnKind::SaveMediaRoot
                    && op.is_encrypted_reset()
                    && !Self::reset_root_missing_locked(&s, &op)
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
                if request.kind == OwnKind::SaveActive {
                    let OwnOperation::Save { encrypted, .. } = &request.operation else {
                        return Err(Error::InvalidData.into());
                    };
                    prior_active = Some((op.switch_sent, op.active_attempt.clone()));
                    op.switch_sent = true;
                    op.active_attempt = Some(OpaqueBytes(encrypted.clone()));
                    s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op)?;
                }
                Ok(())
            })?;
            if matches!(request.kind, OwnKind::ReadResetMediaRoot | OwnKind::SaveMediaRoot) {
                // Each permission is single-attempt, including a rejected enqueue
                // or a save with a lost/failed reply. The next attempt rereads.
                s.publication.reset_root_missing = None;
            }
            s.publication.submitted = true;
        }
        // A submitted mutation whose reply is lost remains durably distinct
        // from a request rejected above, which never calls the transport.
        let result = submit();
        if !accepted(&result) {
            let mut s = self.state();
            // Never overwrite a successor's evidence if lifecycle invalidation
            // raced this acknowledgment. Such an operation remains recoverable.
            if s.publication.running.as_ref() != Some(request) {
                return Err(RuntimeError::StaleIncarnation);
            }
            // Restore only the evidence provisionally changed for this exact
            // enqueue. Earlier accepted attempts must remain uncertain.
            if let Some((switch_sent, active_attempt)) = prior_active {
                with_tracking_access(|| -> Result<(), RuntimeError> {
                    if let Some(store) = s.store.as_mut()
                        && let Some(mut op) = store.publication_op()?.filter(|op| op.id == request.op)
                    {
                        op.switch_sent = switch_sent;
                        op.active_attempt = active_attempt;
                        store.write_publication_op(&op)?;
                    }
                    Ok(())
                })?;
            }
            s.publication.submitted = false;
            s.publication.running = None;
            s.publication.waiting = true;
        }
        Ok(result)
    }
    pub fn publication_result(
        &self,
        request: &OwnRequest,
        result: OwnResult,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        if result == OwnResult::NotStarted {
            let mut s = self.state();
            if s.publication.running.as_ref() != Some(request) || s.publication.submitted {
                return Err(RuntimeError::StaleIncarnation);
            }
            s.publication.running = None;
            s.publication.waiting = true;
            return Ok(());
        }
        // Only this exact original delivery settles its outstanding attempt.
        // Account/lifecycle invalidation can refuse its facts, never supply a
        // completion. Keep ownership across provider reads and recheck it after.
        {
            let s = self.state();
            if s.publication.running.as_ref() != Some(request) || !s.publication.submitted {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        let account = self.publication_account();
        let file = self.inputs.files.identity(collection.path());
        let decision = self.publication_decision();
        let now = self.adapter_now();
        let mut s = self.state();
        if s.publication.running.as_ref() != Some(request) || !s.publication.submitted {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.publication.running = None;
        s.publication.submitted = false;
        s.publication.waiting = true;
        if s.incarnation != request.incarnation
            || Self::instance_locked(&s).ok().as_ref() != Some(&request.instance)
            || collection.generation().0 != request.generation
            || !collection.is_current()
            || s.tracking_stopped
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let account = account?;
        let file = file?;
        let decision = decision?;
        let now = now?;
        if request.kind == OwnKind::ReadResetMediaRoot
            && s.publication.reset_root_read_guard.as_ref() != Some(&Self::adapter_guard_locked(&s))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let mut op = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.publication_op())?
            .filter(|o| o.id == request.op)
            .ok_or(RuntimeError::StaleIncarnation)?;
        if account != op.account
            || file != op.confirmed_file
            || decision != op.decision_written(&self.inputs.scope)?
            || !s.bound.as_ref().is_some_and(|b| {
                Some(b.binding.id) == op.binding_after && b.usable && b.verified
            })
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.publication.waiting = false;
        if let OwnResult::ZoneLosses(losses) = &result {
            use crate::cloud::adapter::ZoneLossCause;
            if losses.is_empty()
                || losses.iter().any(|loss| match &request.operation {
                    OwnOperation::Read { zone, .. } | OwnOperation::Save { zone, .. } => &loss.zone != zone,
                    OwnOperation::CreateZones(zones) => !zones.contains(&loss.zone),
                })
            {
                return Err(Error::InvalidData.into());
            }
            let proof = s
                .adapter
                .accepted_identity_read
                .as_ref()
                .ok_or(RuntimeError::IdentityUnverified)?
                .request;
            Self::fresh_loss_identity_locked(&s, proof, s.publication.own_identity_after)?;
            let bound = crate::cloud::adapter::collection_zone(op.epoch);
            if losses
                .iter()
                .any(|l| l.zone != bound && l.zone != "meguri-root" && l.zone != "meguri-media")
            {
                return Err(Error::InvalidData.into());
            }
            let reset = losses.iter().any(|l| l.cause == ZoneLossCause::EncryptedDataReset);
            let purged = losses
                .iter()
                .any(|l| l.zone != "meguri-media" && l.cause == ZoneLossCause::Purged);
            let collection_lost = losses.iter().any(|l| l.zone == bound);
            let root_lost = losses.iter().any(|l| l.zone == "meguri-root");
            if !reset && !purged && !collection_lost && root_lost && request.kind == OwnKind::SaveActive {
                // V26.1's publisher exception only: no root remains to replace.
                // Existing fields retain the crash obligation. A restart tries
                // the new-record save, then meets zoneNotFound again if needed.
                op.active = None;
                op.active_attempt = None;
                with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op))?;
                s.publication.recreate_active_root = Some(op.id);
                s.publication.waiting = false;
                return Ok(());
            }
            let reason = if reset {
                Some(IsolationReason::EncryptedDataReset)
            } else if purged {
                Some(IsolationReason::ZoneDeletedByUser)
            } else if collection_lost {
                Some(IsolationReason::ZoneMissing)
            } else {
                None
            };
            let id = op.binding_after.ok_or(Error::InvalidData)?;
            if let Some(reason) = reason {
                with_tracking_access(|| Self::commit_service_isolation_locked(&mut s, id, reason))?;
                // Commit has removed network authority. Publish before any
                // fallible readback, hook, sweep or unwind can leave this path.
                let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                b.binding.facts.state = BindingState::Isolated(reason);
                b.usable = false;
                b.verified = false;
                s.root_confirmed = false;
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
                    return Err(Error::Database.into());
                }
                let binding = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.binding(id))?
                    .ok_or(Error::BindingNotFound)?;
                s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?.binding = binding;
                let live = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.inbox(id))?;
                super::super::payload_asset::sweep(&self.directory, id, &live)?;
            } else if losses.iter().any(|l| l.zone == "meguri-media") {
                with_tracking_access(|| {
                    let mut ledger = s.store.as_ref().ok_or(Error::Database)?.media_ledger(id)?;
                    ledger.zone_missing = true;
                    self.commit_media_state_locked(&mut s, id, &ledger).map(|_| ())
                })?;
            }
            s.publication.waiting = true;
            return Ok(());
        }
        if let OwnResult::Failed { .. } | OwnResult::RetryAfter { .. } = result {
            op.retry_after_ms = match result {
                OwnResult::Failed { retry_after_ms } => retry_after_ms,
                OwnResult::RetryAfter { duration_ms } => {
                    Some(i64::try_from(duration_ms).map_or(i64::MAX, |duration| now.saturating_add(duration)))
                }
                _ => unreachable!(),
            };
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op))?;
            s.publication.waiting = true;
            return Ok(());
        }
        let mut accepted = false;
        let mut accepted_epoch = None;
        #[cfg(feature = "test-hooks")]
        let selected_origin = match &result {
            OwnResult::Saved(_) => Some(selected_observation::ReplyOrigin::Saved),
            OwnResult::Conflict(_) => Some(selected_observation::ReplyOrigin::Conflict),
            _ => None,
        };
        #[cfg(feature = "test-hooks")]
        let mut selected_complete = None;
        match (request.kind, result) {
            (OwnKind::CreateZones, OwnResult::ZonesCreated)
                if op.step == PublishStep::Complete
                    && s.publication.recreate_active_root == Some(op.id)
                    && request.operation == OwnOperation::CreateZones(vec!["meguri-root".into()]) =>
            {
                s.publication.recreate_active_root = None;
                accepted = true;
            }
            (OwnKind::CreateZones, OwnResult::ZonesCreated) if op.step == PublishStep::Bound => {
                s.publication.zones_created = true;
                accepted = true;
            }
            (OwnKind::ReadResetMediaRoot, OwnResult::Read(read))
                if op.step == PublishStep::Bound && op.is_encrypted_reset() && !op.root_acked =>
            {
                match read {
                    RecordRead::Missing => {
                        s.publication.reset_root_missing = Some((request.clone(), Self::adapter_guard_locked(&s)));
                        accepted = true;
                    }
                    RecordRead::Found(record) => {
                        if let Some(DecodeOutcome::Supported(root)) = record
                            .encrypted
                            .as_deref()
                            .and_then(|e| control_codec::MediaRoot::decode(e).ok())
                            && !record.system_fields.0.is_empty()
                        {
                            op.media_key = RecordKey(root.media_key);
                            op.root_acked = true;
                            accepted = true;
                        }
                        // An absent e requires the own-epoch/second-root protocol.
                        // That destructive phase is unsupported here: wait, save nothing.
                    }
                    RecordRead::ZoneMissing | RecordRead::Failed => {}
                }
            }
            (OwnKind::SaveMediaRoot, OwnResult::Saved(record) | OwnResult::Conflict(record))
                if op.step == PublishStep::Bound =>
            {
                if let Some(DecodeOutcome::Supported(root)) = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::MediaRoot::decode(e).ok())
                    && !record.system_fields.0.is_empty()
                {
                    op.media_key = RecordKey(root.media_key);
                    op.root_acked = true;
                    accepted = true;
                }
            }
            (
                OwnKind::SavePublishingEpoch | OwnKind::SaveCompleteEpoch,
                OwnResult::Saved(record) | OwnResult::Conflict(record),
            ) => {
                let expected_state = if request.kind == OwnKind::SavePublishingEpoch {
                    EpochState::Publishing
                } else {
                    EpochState::Complete
                };
                if let Some(DecodeOutcome::Supported(epoch)) =
                    record.encrypted.as_deref().and_then(|e| Epoch::decode(e).ok())
                    && epoch.epoch_id == op.epoch.0
                    && epoch.record_key == op.record_key.0
                    && epoch.state == expected_state
                    && epoch.crt == op.crt
                    && !record.system_fields.0.is_empty()
                {
                    if expected_state == EpochState::Publishing && op.step == PublishStep::Bound {
                        op.epoch_system_fields = Some(record.system_fields);
                        if op.root_acked {
                            op.step = PublishStep::Zones;
                        }
                        accepted = true;
                        accepted_epoch = Some(epoch);
                    } else if expected_state == EpochState::Complete && op.step == PublishStep::Sent {
                        #[cfg(feature = "test-hooks")]
                        {
                            selected_complete = selected_origin.and_then(|origin| {
                                selected_observation::AcceptedPublication::new(&op, request, &record, origin, &s)
                            });
                        }
                        op.complete_system_fields = Some(record.system_fields);
                        op.step = PublishStep::Complete;
                        accepted = true;
                        accepted_epoch = Some(epoch);
                    }
                }
            }
            (OwnKind::ReadOwnEpoch, OwnResult::Read(read)) if op.step == PublishStep::Complete => match read {
                RecordRead::Found(record) => {
                    if let Some(DecodeOutcome::Supported(epoch)) =
                        record.encrypted.as_deref().and_then(|e| Epoch::decode(e).ok())
                        && epoch.epoch_id == op.epoch.0
                        && epoch.record_key == op.record_key.0
                        && epoch.state == EpochState::Complete
                    {
                        s.publication.own_epoch_read = true;
                        accepted = true;
                    }
                }
                RecordRead::Missing | RecordRead::ZoneMissing => {
                    let proof = s
                        .adapter
                        .accepted_identity_read
                        .as_ref()
                        .ok_or(RuntimeError::IdentityUnverified)?
                        .request;
                    Self::fresh_loss_identity_locked(&s, proof, s.publication.own_identity_after)?;
                    let id = op.binding_after.ok_or(Error::InvalidData)?;
                    with_tracking_access(|| {
                        s.store
                            .as_mut()
                            .ok_or(Error::Database)?
                            .isolate_runtime_binding(id, IsolationReason::ZoneMissing)
                    })?;
                    let b = s.bound.as_mut().ok_or(RuntimeError::StaleIncarnation)?;
                    b.binding.facts.state = BindingState::Isolated(IsolationReason::ZoneMissing);
                    b.usable = false;
                    b.verified = false;
                    s.publish_authority();
                }
                _ => {}
            },
            (OwnKind::SaveActive, OwnResult::Saved(record) | OwnResult::Conflict(record))
                if op.step == PublishStep::Complete =>
            {
                if let Some(active) = record
                    .encrypted
                    .as_deref()
                    .and_then(|e| control_codec::Active::decode(e).ok())
                {
                    if active.epoch == op.epoch.0 && !record.system_fields.0.is_empty() {
                        let open = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.open_counter;
                        with_tracking_access(|| {
                            s.store
                                .as_mut()
                                .ok_or(Error::Database)?
                                .publication_done(&op, &record, open, now)
                        })?;
                        #[cfg(feature = "test-hooks")]
                        if let Some(receipt) = &mut s.selected_publication
                            && let Some(origin) = selected_origin
                        {
                            receipt.mark_done(&op, request, &record, origin);
                        }
                        s.root_confirmed = true;
                        s.adapter.durable.active = Some(crate::cloud::workflow::Saved {
                            name: "active".into(),
                            envelope: record.encrypted.ok_or(Error::InvalidData)?,
                            system_fields: record.system_fields.0,
                        });
                        s.publication = ProcessState::default();
                        self.emit_pending_locked(&mut s, UpdateKind::Full, Default::default())?;
                        return Ok(());
                    }
                    // A valid competing Active is a durable choice, never a retry.
                    op.switch_conflict = Some(record.system_fields);
                    accepted = true;
                }
            }
            _ => {}
        }
        if accepted {
            op.retry_after_ms = None;
            if let Err(error) =
                with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.publication_ack_control(&op, accepted_epoch.as_ref()))
            {
                s.publication.reset_root_missing = None;
                return Err(error.into());
            }
            with_tracking_access(|| Self::refresh_epoch_crt_locked(&mut s))?;
            #[cfg(feature = "test-hooks")]
            if request.kind == OwnKind::SaveCompleteEpoch {
                s.selected_publication = selected_complete;
            }
        } else {
            s.publication.waiting = true;
        }
        Ok(())
    }
    pub fn publication_cancellable(&self, id: [u8; 16]) -> Result<bool, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| -> Result<bool, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            Ok(store.publication_op()?.is_some_and(|op| op.id == id)
                && !s.incoming.running
                && !s.publication.running.as_ref().is_some_and(|r| r.kind == OwnKind::SaveActive))
        })
    }

    pub fn publication_conflict_cancellable(&self, id: [u8; 16]) -> Result<bool, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| -> Result<bool, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let Some(op) = store.publication_op()?.filter(|op| op.id == id) else { return Ok(false); };
            Ok(op.step == PublishStep::Complete && op.switch_sent
                && op.switch_conflict.as_ref().is_some_and(|tag| !tag.0.is_empty())
                && s.publication.running.is_none() && !s.incoming.running)
        })
    }

    pub fn cancel_publication(&self, id: [u8; 16], collection: &Collection) -> Result<(), RuntimeError> {
        let Some(mut op) = self.publication_op()?.filter(|o| o.id == id) else {
            return Err(PublicationError::NotOffered.into());
        };
        {
            let mut s = self.state();
            if with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.publication_op())?.as_ref() != Some(&op) {
                return Err(PublicationError::AuthorityChanged.into());
            }
            if s.incoming.running || s.publication.running.as_ref().is_some_and(|r| r.kind == OwnKind::SaveActive)
            {
                return Err(PublicationError::Busy.into());
            }
            if op.step == PublishStep::Confirmed {
                with_tracking_access(|| {
                    s.store
                        .as_mut()
                        .ok_or(Error::Database)?
                        .discard_confirmed_publication(&op)
                })?;
                return Ok(());
            }
            op.step = PublishStep::Cancelling;
            with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.write_publication_op(&op))?;
            s.publication.running = None;
            Self::adapter_close_exchange_locked(&mut s);
            s.root_confirmed = false;
        }
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let desired = op.decision_written(&self.inputs.scope)?;
        if op.fence_before != desired && decision == desired {
            // A concurrent decision wins. Never restore a fence over it.
            if collection.resolve_cloud_publication_decision(op.id, &desired)? {
                meguri_model::faultpoint!("cloud.publish.after_cancel_fence").map_err(Error::from)?;
            }
        }
        let decision = self.publication_decision()?;
        let restored = match op.binding_before.as_ref().map(|b| b.state) {
            Some(BindingState::Active) if decision.publication.identity != op.confirmed_publication => {
                BindingState::Isolated(IsolationReason::ReplacedLocally)
            }
            Some(BindingState::Active) if file != op.confirmed_file => {
                BindingState::Isolated(IsolationReason::FileMismatch)
            }
            Some(state) => state,
            None => BindingState::Isolated(IsolationReason::Republished),
        };
        {
            let mut s = self.state();
            with_tracking_access(|| {
                s.store
                    .as_mut()
                    .ok_or(Error::Database)?
                    .cancel_publication(&op, restored)
            })?;
            s.publication = ProcessState::default();
        }
        self.bind(collection)?;
        self.refresh_identity()?;
        if restored == BindingState::Active {
            match self.reconcile_binding(collection, &self.media_service(collection)?, Duration::from_secs(30)) {
                Err(RuntimeError::Tracking(Error::ValidatedUnprovedLayout { .. })) => {},
                Err(error) => return Err(error),
                Ok(_) => {},
            }
        }
        Ok(())
    }
}

impl Runtime {
    /// Capture before the actual own inventory fetch, retaining its original
    /// engine, account, binding, file and lifecycle authority across the await.
    pub fn begin_zone_listing(
        &self,
        instance: &str,
        collection: &Collection,
    ) -> Result<ZoneListingRequest, RuntimeError> {
        let _lease = collection.reader(ReaderRole::Cloud)?;
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        let request = ZoneListingRequest {
            owner: Arc::downgrade(&self.state),
            collection: collection.clone(),
            scan: ScanGuard {
                invalidation: s.invalidation,
                binding: b.binding.id,
                open: b.open_counter,
                generation: b.evidence.generation,
                file,
                incarnation: s.incarnation,
            },
            adapter: Self::adapter_guard_locked(&s),
            instance: instance.into(),
            account,
            decision,
            deletion_fence: s.zone_listing_fence.clone(),
        };
        self.validate_zone_listing_locked(&s, &request, collection)?;
        Ok(request)
    }
    fn validate_zone_listing_locked(
        &self,
        s: &State,
        request: &ZoneListingRequest,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        Self::caller_locked(s, &request.instance)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !request.owner.ptr_eq(&Arc::downgrade(&self.state))
            || s.incarnation != request.scan.incarnation
            || (b.binding.id, b.open_counter, b.evidence.generation, b.file)
                != (
                    request.scan.binding,
                    request.scan.open,
                    request.scan.generation,
                    request.scan.file,
                )
            || b.binding.facts.account != request.account
            || s.invalidation != request.scan.invalidation
            || s.invalidation_exhausted
            || Self::adapter_guard_locked(s) != request.adapter
            || s.tracking_stopped
            || s.adapter.paused
            || s.store.is_none()
            || request.scan.file.created_nanos >= 1_000_000_000
            || !collection.is_current()
            || !request.collection.is_current()
            || collection.generation() != request.scan.generation
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
    /// Only an actual successful inventory supplies names; None is a failed
    /// fetch. False also discards a listing superseded by a zone deletion. A
    /// request begun after that deletion can observe a genuinely recreated zone.
    pub fn complete_zone_listing(
        &self,
        request: ZoneListingRequest,
        collection: &Collection,
        zones: Option<&[String]>,
    ) -> Result<bool, RuntimeError> {
        if !request.owner.ptr_eq(&Arc::downgrade(&self.state))
            || !collection.is_current()
            || !request.collection.is_current()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let _lease = request.collection.reader(ReaderRole::Cloud)?;
        let account = self.publication_account()?;
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let now = self.adapter_now()?;
        if account != request.account || file != request.scan.file || decision != request.decision {
            return Err(RuntimeError::StaleIncarnation);
        }
        let mut s = self.state();
        self.validate_zone_listing_locked(&s, &request, collection)?;
        if !Arc::ptr_eq(&s.zone_listing_fence, &request.deletion_fence) {
            return Ok(false);
        }
        let Some(zones) = zones else { return Ok(false) };
        with_tracking_access(|| {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .record_publication_zones(request.scan.binding, zones, now)
        })?;
        Ok(true)
    }
    /// Caller holds state then tracking access. Fence each committed removal,
    /// including a successful member preceding a failed batch member.
    pub(super) fn remove_publication_zone_locked(s: &mut State, b: BindingId, zone: &str) -> Result<(), Error> {
        s.store
            .as_mut()
            .ok_or(Error::Database)?
            .remove_publication_zone(b, zone)?;
        s.zone_listing_fence = Arc::new(());
        Ok(())
    }
    /// Called from a real collection drain; no timer makes a zone due.
    pub fn publication_collection_drained(&self, instance: &str, collection: &Collection) -> Result<(), RuntimeError> {
        let now_ms = self.adapter_now()?;
        let file = self.inputs.files.identity(collection.path())?;
        let account = self.publication_account()?;
        let mut s = self.state();
        Self::sdk_locked(&s, instance)?;
        let Some((guard, sdk, at_ms)) = s.publication.cleanup_drain.take() else {
            return Ok(());
        };
        if guard != Self::adapter_guard_locked(&s)
            || sdk != instance
            || now_ms
                .checked_sub(at_ms)
                .is_none_or(|age| !(0..media::cleanup::FRESH_MS).contains(&age))
            || !s.root_confirmed
            || Self::publication_allows_send_locked(&s)
            || Self::gate_locked(&s).is_err()
        {
            return Ok(());
        }
        let bound = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if bound.file != file
            || bound.binding.facts.account != account
            || !collection.is_current()
            || bound.evidence.generation != collection.generation()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if s.publication
            .last_cleanup_ms
            .is_some_and(|last| at_ms.checked_sub(last).is_none_or(|age| age < 86_400_000))
        {
            return Ok(());
        }
        let b = bound.binding.id;
        let media = s.media.enabled != Some(false) && !s.media.paused;
        with_tracking_access(|| -> Result<(), RuntimeError> {
            s.store
                .as_mut()
                .ok_or(Error::Database)?
                .compute_cleanup(b, at_ms, media)?;
            Ok(())
        })?;
        s.publication.last_cleanup_ms = Some(at_ms);
        s.media.observed_now_ms = now_ms;
        self.emit_pending_locked(&mut s, UpdateKind::Full, Default::default())?;
        // Durable unlink replay follows the single committed computation.
        with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.finish_conflict_expiry())?;
        Ok(())
    }
    pub fn begin_zone_deletion_read(
        &self,
        instance: &str,
        collection: &Collection,
    ) -> Result<ZoneDeletionGuard, RuntimeError> {
        let file = self.inputs.files.identity(collection.path())?;
        let decision = self.publication_decision()?;
        let account = self.publication_account()?;
        let s = self.state();
        Self::caller_locked(&s, instance)?;
        Self::gate_locked(&s)?;
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !s.root_confirmed
            || Self::publication_allows_send_locked(&s)
            || file != b.file
            || account != b.binding.facts.account
        {
            return Err(PublicationError::NotOffered.into());
        }
        Ok(ZoneDeletionGuard {
            instance: instance.into(),
            incarnation: s.incarnation,
            file,
            decision,
            account,
            after_identity: s.adapter.next_identity,
        })
    }
    pub(super) fn validate_zone_deletion_guard(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        if self.inputs.files.identity(collection.path())? != guard.file
            || self.publication_decision()? != guard.decision
            || self.publication_account()? != guard.account
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation || !s.root_confirmed || Self::publication_allows_send_locked(&s) {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
    /// A fresh record-ID Active read must still name this binding, immediately
    /// before the platform submits any deletion. Failure/unreadable deletes none.
    pub fn zone_deletion_check(
        &self,
        guard: &ZoneDeletionGuard,
        read: RecordRead,
        collection: &Collection,
    ) -> Result<Vec<String>, RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        let RecordRead::Found(record) = read else {
            return Ok(Vec::new());
        };
        let Some(active) = record
            .encrypted
            .as_deref()
            .and_then(|e| control_codec::Active::decode(e).ok())
        else {
            return Ok(Vec::new());
        };
        let s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if active.epoch != b.binding.facts.epoch.0 {
            return Ok(Vec::new());
        }
        let zones = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.retained_zones(b.binding.id))?;
        let active_zone = crate::cloud::adapter::collection_zone(EpochId(active.epoch));
        Ok(zones
            .into_iter()
            .filter_map(|(name, state)| (state.deletion_due && name != active_zone).then_some(name))
            .collect())
    }
    /// `deleted` means a successful deletion or an actual zone-not-found result.
    /// Other results keep the durable due mark for the next drained fetch.
    pub fn zone_deletion_result(
        &self,
        guard: &ZoneDeletionGuard,
        zone: &str,
        deleted: bool,
        collection: &Collection,
    ) -> Result<(), RuntimeError> {
        self.validate_zone_deletion_guard(guard, collection)?;
        if !deleted {
            return Ok(());
        }
        let mut s = self.state();
        Self::caller_locked(&s, &guard.instance)?;
        Self::gate_locked(&s)?;
        if s.incarnation != guard.incarnation {
            return Err(RuntimeError::StaleIncarnation);
        }
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| Self::remove_publication_zone_locked(&mut s, b, zone))?;
        Ok(())
    }
}

impl Runtime {
    /// Actual durable media-root authority, independent of the in-progress op.
    pub fn publication_media_state(&self) -> Result<Option<(RecordKey, bool, i64)>, RuntimeError> {
        let s = self.state();
        let Some(bound) = &s.bound else {
            return Ok(None);
        };
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .publication_media(bound.binding.id)?)
        })
    }
}

impl meguri_engine::ports::CloudPublicationAuthority for Runtime {
    fn current_account(&self) -> meguri_engine::device_state::AccountBinding {
        use meguri_engine::device_state::AccountBinding;
        // Read the platform directly: neither local ownership nor a recovery
        // row is evidence of the current account. No Runtime/tracking guard.
        #[cfg(feature = "test-hooks")]
        if let Some(hook) = self.take_current_account_hook_for_test() {
            hook();
        }
        match self.inputs.identity.read_identity(&self.inputs.scope) {
            IdentityStatus::Available(account) => match String::from_utf8(account.0) {
                Ok(account) if !account.is_empty() => AccountBinding::Connected {
                    account,
                    configuration: self.inputs.scope.container.as_str().to_owned(),
                    environment: match self.inputs.scope.environment {
                        Environment::Development => "development",
                        Environment::Production => "production",
                    }
                    .into(),
                },
                _ => AccountBinding::Unknown,
            },
            _ => AccountBinding::Unknown,
        }
    }

    fn with_resolution(
        &self,
        collection: &Collection,
        operation: [u8; 16],
        expected: &DecisionSnapshot,
        apply: &mut dyn FnMut(&DecisionSnapshot) -> Result<bool, EngineError>,
    ) -> Result<bool, EngineError> {
        // These providers may reenter platform code. No Runtime, tracking,
        // lifecycle-binding or sidecar mutex is held while they run.
        let account = self.inputs.identity.read_identity(&self.inputs.scope);
        let file = self.inputs.files.identity(collection.path()).map_err(engine_error)?;
        let evidence = collection.open_evidence()?;
        let current = self.publication_decision().map_err(engine_error)?;
        if &current != expected
            || !collection.is_current()
            || evidence.generation != collection.generation()
            || evidence.native_file != file.pair()
            || file.created_nanos >= 1_000_000_000
        {
            return Err(engine_error("stale decision"));
        }
        let s = self.state();
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or_else(|| engine_error("missing store"))?;
            let op = store
                .publication_op()
                .map_err(engine_error)?
                .filter(|o| o.id == operation)
                .ok_or_else(|| engine_error("missing recovery authority"))?;
            let after_id = op
                .binding_after
                .ok_or_else(|| engine_error("missing publication binding"))?;
            let after = store
                .binding(after_id)
                .map_err(engine_error)?
                .ok_or_else(|| engine_error("missing publication binding"))?;
            if op.confirmed_publication != current.publication.identity
                || after.facts.publication != op.confirmed_publication
                || after.facts.account != op.account
                || after.facts.epoch != op.epoch
            {
                return Err(engine_error("stale recovery authority"));
            }
            if op.step == PublishStep::Bound {
                Self::require_supported_publication(&op).map_err(engine_error)?;
                if account != IdentityStatus::Available(op.account.clone())
                    || op.confirmed_file != file
                    || !s.bound.as_ref().is_some_and(|b| {
                        Some(b.binding.id) == op.binding_after
                            && b.evidence.generation == collection.generation()
                            && b.file == file
                    })
                {
                    return Err(engine_error("stale publication authority"));
                }
            }
            let desired = op.decision_written(&self.inputs.scope).map_err(engine_error)?;
            let replacement = match op.step {
                PublishStep::Bound if expected == &op.fence_before => desired,
                // Cancel grants no new publication authority. Current native
                // evidence and the exact scoped row/CAS suffice even after an
                // account event or a file change since confirmation (V15.3).
                PublishStep::Cancelling if expected == &desired => op.fence_before,
                _ => return Err(engine_error("decision authority refused")),
            };
            // Serialize the actual durable row through the sidecar CAS. No
            // writer work or Runtime method is invoked by the engine callback.
            apply(&replacement)
        })
    }
}

impl Runtime {
    #[cfg(feature = "test-hooks")]
    pub fn publication_control_for_testing(&self) -> Result<super::testing::PublicationControl, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let current = s.bound.as_ref().map(|b| b.binding.clone());
            let zones = match &current {
                Some(binding) => store.retained_zones(binding.id)?,
                None => BTreeMap::new(),
            };
            Ok(super::testing::PublicationControl {
                current,
                recovery_ops: store
                    .publication_op()?
                    .into_iter()
                    .map(Into::into)
                    .chain(store.adoption_op()?.into_iter().map(Into::into))
                    .collect(),
                zones,
            })
        })
    }
    pub fn publication_facts(&self, collection: &Collection) -> Result<PublicationFacts, RuntimeError> {
        // Provider reads precede locks; a snapshot is observational, never authority
        // for a later write (the mutating APIs revalidate independently).
        let decision = self.publication_decision()?;
        let native_file = self.inputs.files.identity(collection.path())?;
        let s = self.state();
        with_tracking_access(|| -> Result<_, RuntimeError> {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let current = s.bound.as_ref().map(|b| b.binding.clone());
            let mut entities = BTreeMap::new();
            let mut zones = BTreeMap::new();
            let mut media_key = None;
            let mut root_acked = false;
            if let Some(binding) = &current {
                for kind in row_payload::KINDS {
                    let mut after = None;
                    loop {
                        let page = store.entity_page(binding.id, kind, after.as_deref())?;
                        if page.is_empty() {
                            break;
                        }
                        for entity in page {
                            after = Some(entity.key.clone());
                            let entity = store.entity_dump(binding.id, &entity.name)?.ok_or(Error::InvalidData)?;
                            entities.insert(entity.name.clone(), entity);
                        }
                    }
                }
                zones = store.retained_zones(binding.id)?;
                if let Some((key, acked, _)) = store.publication_media(binding.id)? {
                    media_key = Some(key);
                    root_acked = acked;
                }
            }
            Ok(PublicationFacts {
                current,
                bindings: store.publication_bindings()?,
                recovery_ops: store
                    .publication_op()?
                    .into_iter()
                    .map(Into::into)
                    .chain(store.adoption_op()?.into_iter().map(Into::into))
                    .collect(),
                entities,
                zones,
                decision,
                native_file,
                media_key,
                root_acked,
            })
        })
    }
    /// Submit/enqueue while the deletion guard is current, after any platform hold.
    pub fn submit_zone_deletion<T>(
        &self,
        guard: &ZoneDeletionGuard,
        collection: &Collection,
        submit: impl FnOnce() -> T,
    ) -> Result<T, RuntimeError> {
        let _lease = collection.reader(ReaderRole::Cloud)?;
        self.validate_zone_deletion_guard(guard, collection)?;
        Ok(submit())
    }
    pub(super) fn publication_automatic_locked(s: &State) -> bool {
        // The saved user preference is unchanged; only zones temporarily forces
        // automatic scheduling (P6/V15.2). No transport fixture grants permission.
        with_tracking_access(|| {
            s.store
                .as_ref()
                .and_then(|store| store.publication_op().ok().flatten())
                .is_some_and(|op| {
                    op.step == PublishStep::Zones && op.binding_after == s.bound.as_ref().map(|b| b.binding.id)
                })
        })
    }
}
