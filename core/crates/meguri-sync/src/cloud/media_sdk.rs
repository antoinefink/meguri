//! One-engine media send bridge. Scheduling and file decisions stay in Runtime.
use super::*;
use crate::cloud::policy::PolicyWork;

fn check_requested_action(work: &Work, requested: workflow::PendingChange) -> Result<(), RuntimeError> {
    match (&work.request, requested) {
        (Request::Upload { .. }, workflow::PendingChange::Save)
        | (Request::Delete { .. }, workflow::PendingChange::Delete) => Ok(()),
        _ => Err(Error::InvalidData.into()),
    }
}

impl Runtime {
    pub fn validate_cloud_media_admission(
        &self,
        sdk: &str,
        collection: &Collection,
        work: &Work,
    ) -> Result<(), RuntimeError> {
        self.validate_sdk(sdk)?;
        self.validate_cloud_media_work(collection, work)?;
        {
            let s = self.state();
            if s.media.enabled == Some(false)
                || s.media.paused
                || !matches!(work.request, Request::Root) && !s.media.root_checked
            {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        self.workflow_gate()
    }
    pub fn validate_cloud_media_batch(
        &self,
        sdk: &str,
        collection: &Collection,
        batch: u64,
    ) -> Result<Work, RuntimeError> {
        let _ = self.adapter_now();
        self.validate_sdk(sdk)?;
        let retained = self
            .state()
            .actions
            .media_batches
            .get(&(sdk.to_owned(), batch))
            .cloned()
            .ok_or(RuntimeError::StaleIncarnation)?;
        check_requested_action(&retained.work, retained.requested)?;
        let work = retained.work;
        self.validate_cloud_media_admission(sdk, collection, &work)?;
        if let Request::Delete { name } = &work.request {
            let now = self.adapter_now()?;
            let mut s = self.state();
            s.media.observed_now_ms = now;
            let allowed = s.root_confirmed
                && !s.media.paused
                && s.media.enabled != Some(false)
                && !Self::publication_allows_send_locked(&s);
            let admitted = with_tracking_access(|| {
                s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .media_deletions(work.authority.binding, now, allowed)
            })?
            .contains(name);
            if !admitted {
                s.actions.media_batches.remove(&(sdk.to_owned(), batch));
                self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, BTreeSet::new())?;
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        let s = self.state();
        if let Request::Upload { name, .. } = &work.request
            && s.media.enabled.is_some()
            && with_tracking_access(|| Self::sendables_locked(&s))?
                .0
                .keys()
                .any(|pending| pending != name)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(work)
    }
    pub fn queue_cloud_media_upload(
        &self,
        sdk: &str,
        collection: &Collection,
        work: &Work,
    ) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        self.validate_sdk(sdk)?;
        self.validate_cloud_media_work(collection, work)?;
        if !matches!(work.request, Request::Upload { .. }) {
            return Err(Error::InvalidData.into());
        }
        let mut s = self.state();
        Self::caller_locked(&s, sdk)?;
        Self::check_media_locked(&s, &work.authority)?;
        Self::gate_locked(&s)?;
        if s.media.pending.as_ref() != Some(work)
            && !s
                .actions
                .media_upload
                .as_ref()
                .is_some_and(|(_, original)| original == work)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        s.actions.media_upload = Some((sdk.to_owned(), work.clone()));
        s.media.pending = None;
        self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, BTreeSet::new())?;
        Ok(())
    }

    /// Called by the same builder used for collection batches. One original
    /// Work is outstanding today; this stays within P13.3's 8/128 MiB bounds.
    pub fn build_cloud_media_batch(
        &self,
        sdk: &str,
        collection: &Collection,
        requested: &BTreeMap<String, workflow::PendingChange>,
    ) -> Result<Option<(u64, Work)>, RuntimeError> {
        let _ = self.adapter_now();
        self.validate_sdk(sdk)?;
        // Selection is only a hint. Re-read eligibility with a fresh clock and
        // retain this exact work through final admission and the SDK callback.
        self.consume_media_feed(collection, &self.media_service(collection)?)?;
        let queued = self
            .state()
            .actions
            .media_upload
            .clone()
            .filter(|(owner, _)| owner == sdk);
        if let Some((_, work)) = &queued
            && let Request::Upload { name, .. } = &work.request
            && let Some(action) = requested.get(name)
        {
            check_requested_action(work, *action)?;
        }
        let now = self.adapter_now()?;
        let authority = self.media_authority(collection, true)?;
        {
            let mut s = self.state();
            Self::sdk_locked(&s, sdk)?;
            Self::gate_locked(&s)?;
            s.media.observed_now_ms = now;
            let allowed = s.root_confirmed
                && !s.media.paused
                && s.media.enabled != Some(false)
                && s.media.root_checked
                && !Self::publication_allows_send_locked(&s);
            let due = with_tracking_access(|| {
                s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .media_deletions(authority.binding, now, allowed)
            })?;
            if let Some(name) = due
                .iter()
                .filter(|name| requested.contains_key(*name))
                .find(|name| !s.policy.parked.contains(*name))
                .cloned()
            {
                let action = requested[&name];
                if action != workflow::PendingChange::Delete {
                    return Err(Error::InvalidData.into());
                }
                for ((owner, id), retained) in &s.actions.media_batches {
                    if matches!(&retained.work.request,Request::Delete{name:n} if n==&name) {
                        check_requested_action(&retained.work, action)?;
                        check_requested_action(&retained.work, retained.requested)?;
                        return if owner == sdk {
                            Ok(Some((*id, retained.work.clone())))
                        } else {
                            Err(RuntimeError::StaleIncarnation)
                        };
                    }
                }
                s.actions.batch = s.actions.batch.checked_add(1).ok_or(Error::InvalidData)?;
                let id = s.actions.batch;
                let work = Work {
                    id,
                    authority: authority.clone(),
                    request: Request::Delete { name: name.clone() },
                };
                s.actions.media_batches.insert(
                    (sdk.into(), id),
                    workflow::MediaBatch {
                        requested: action,
                        work: work.clone(),
                    },
                );
                s.actions.observations.push(workflow::Observation::BatchBuilt {
                    names: BTreeSet::from([name]),
                    batch_id: id,
                });
                return Ok(Some((id, work)));
            }
            self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, BTreeSet::new())?;
        }
        let Some((owner, work)) = queued else {
            return Ok(None);
        };
        self.validate_cloud_media_work(collection, &work)?;
        let Request::Upload { name, envelope, staged } = &work.request else {
            return Err(Error::InvalidData.into());
        };
        let Some(action) = requested.get(name).copied() else {
            return Ok(None);
        };
        check_requested_action(&work, action)?;
        {
            let s = self.state();
            if s.media.enabled.is_some()
                && with_tracking_access(|| Self::sendables_locked(&s))?
                    .0
                    .keys()
                    .any(|pending| pending != name)
            {
                return Ok(None);
            }
        }

        let metadata = blob::Blob::decode(envelope).map_err(|_| Error::InvalidData)?;
        let actual = self.media_io(collection, &work.authority, || Ok(m::io::hash(m::io::open(staged)?)?))?;
        if actual != metadata {
            return Err(Error::InvalidData.into());
        }
        let mut s = self.state();
        Self::caller_locked(&s, sdk)?;
        Self::check_media_locked(&s, &work.authority)?;
        Self::gate_locked(&s)?;
        if s.actions.media_upload.as_ref() != Some(&(owner, work.clone())) || s.policy.parked.contains(name) {
            return Ok(None);
        }
        for ((original_sdk, id), retained) in &s.actions.media_batches {
            let original = &retained.work;
            if let Request::Upload {
                name: original_name, ..
            } = &original.request
                && original_name == name
            {
                check_requested_action(original, action)?;
                check_requested_action(original, retained.requested)?;
                if original_sdk == sdk && original == &work {
                    return Ok(Some((*id, original.clone())));
                }
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        s.actions.batch = s.actions.batch.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.actions.batch;
        s.actions.media_batches.insert(
            (sdk.into(), id),
            workflow::MediaBatch {
                requested: action,
                work: work.clone(),
            },
        );
        s.actions.observations.push(workflow::Observation::BatchBuilt {
            names: BTreeSet::from([name.clone()]),
            batch_id: id,
        });
        Ok(Some((id, work)))
    }

    /// False means a collection batch. Matching media batches retain their
    /// ORIGINAL Work through transient SDK failures, never reconstructed ids.
    #[allow(clippy::too_many_arguments)]
    pub fn cloud_media_sdk_sent(
        &self,
        sdk: &str,
        collection: &Collection,
        request_id: u64,
        saved: &[workflow::Saved],
        blob_conflicts: &[workflow::Saved],
        failed: &[(String, workflow::SendError)],
        deleted: &[String],
        failed_delete: &[(String, workflow::SendError)],
        retrying: &[String],
    ) -> Result<bool, RuntimeError> {
        let _ = self.adapter_now();
        self.validate_sdk(sdk)?;
        let key = (sdk.to_owned(), request_id);
        let Some(retained) = self.state().actions.media_batches.get(&key).cloned() else {
            return Ok(false);
        };
        check_requested_action(&retained.work, retained.requested)?;
        let work = retained.work;
        self.validate_cloud_media_work(collection, &work)?;
        if let Request::Delete { name } = &work.request {
            if !saved.is_empty()
                || !blob_conflicts.is_empty()
                || !failed.is_empty()
                || deleted.len() + failed_delete.len() + retrying.len() != 1
                || deleted.iter().any(|n| n != name)
                || failed_delete.iter().any(|(n, _)| n != name)
                || retrying.iter().any(|n| n != name)
                || self.state().actions.handler != Some(request_id)
            {
                return Err(Error::InvalidData.into());
            }
            let done = !deleted.is_empty()
                || failed_delete
                    .iter()
                    .any(|(_, e)| *e == workflow::SendError::UnknownItem);
            if done {
                self.with_media(&work.authority, |store| {
                    store.media_deleted(work.authority.binding, name)
                })?;
            }
            let now = self.adapter_now()?;
            let mut s = self.state();
            Self::sdk_locked(&s, sdk)?;
            Self::check_media_locked(&s, &work.authority)?;
            s.media.observed_now_ms = now;
            // Preserve the separate callback categories: only actual Deleted
            // names participate in the SDK's successful-result removal list.
            s.actions
                .saved_callbacks
                .insert(request_id, deleted.iter().cloned().collect());
            if s.media
                .pending
                .as_ref()
                .is_some_and(|pending| matches!(&pending.request,Request::Delete{name:n} if n==name))
            {
                s.media.pending = None;
            }
            if done {
                s.actions.media_batches.remove(&key);
            } else if let Some((_, error)) = failed_delete.first() {
                s.actions.media_batches.remove(&key);
                if *error != workflow::SendError::BatchRequestFailed {
                    // This bridge builds one deletion per batch. A one-record
                    // limit failure is parked, exactly as collection sends.
                    s.policy.parked.insert(name.clone());
                }
            }
            let force = if failed_delete
                .iter()
                .any(|(_, error)| *error == workflow::SendError::BatchRequestFailed)
            {
                BTreeSet::from([name.clone()])
            } else {
                BTreeSet::new()
            };
            self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, force)?;
            return Ok(true);
        }
        if !deleted.is_empty() || !failed_delete.is_empty() {
            return Err(Error::InvalidData.into());
        }
        let Request::Upload { name, envelope, staged } = &work.request else {
            return Err(Error::InvalidData.into());
        };
        if saved.len() + blob_conflicts.len() > 1
            || failed.len() > 1
            || retrying.len() > 1
            || saved.len() + blob_conflicts.len() + failed.len() + retrying.len() != 1
            || saved
                .iter()
                .chain(blob_conflicts)
                .any(|r| r.name != *name || r.envelope != *envelope)
            || failed.iter().any(|(n, _)| n != name)
            || retrying.iter().any(|n| n != name)
            || self.state().actions.handler != Some(request_id)
        {
            return Err(Error::InvalidData.into());
        }
        if let Some(record) = saved.first().or_else(|| blob_conflicts.first()) {
            meguri_model::faultpoint!("cloud.media.after_blob_save").map_err(Error::from)?;
            self.cloud_media_reply(
                collection,
                &self.media_service(collection)?,
                &work,
                Reply::Saved {
                    name: record.name.clone(),
                    envelope: record.envelope.clone(),
                },
            )?;
            // Commit removal of the ledger reference before releasing this
            // original SDK file. Stale replies retain ownership for recovery.
            let mut ledger = self.with_media(&work.authority, |store| store.media_ledger(work.authority.binding))?;
            if let Some(blob) = ledger.blobs.get_mut(name) {
                blob.staged = None;
            }
            self.put_media_state(&work.authority, &ledger)?;
            let _ = std::fs::remove_file(staged);
        }
        let mut s = self.state();
        Self::caller_locked(&s, sdk)?;
        Self::check_media_locked(&s, &work.authority)?;
        s.actions
            .saved_callbacks
            .insert(request_id, saved.iter().map(|r| r.name.clone()).collect());
        if !saved.is_empty() || !blob_conflicts.is_empty() {
            s.actions.media_upload = None;
            s.actions.media_batches.remove(&key);
        } else if !failed.is_empty() {
            s.policy.parked.insert(name.clone());
            s.actions.media_batches.remove(&key);
        }
        self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, BTreeSet::new())?;
        Ok(true)
    }
}

impl Runtime {
    pub(super) fn retained_upload_authority(s: &State, authority: &Authority) -> bool {
        s.actions
            .media_upload
            .as_ref()
            .is_some_and(|(_, work)| &work.authority == authority)
            || s.actions
                .media_batches
                .values()
                .any(|batch| &batch.work.authority == authority)
    }

    pub fn cloud_media_downloads_blocked(&self) -> bool {
        let s = self.state();
        let Some(b) = &s.bound else { return false };
        let zone = crate::cloud::adapter::collection_zone(b.binding.facts.epoch);
        let fetching =
            |boundary: &crate::cloud::adapter::FetchBoundary| boundary.zones.get(&zone).is_some_and(|(_, done)| !done);
        s.adapter.fetch.as_ref().is_some_and(|call| {
            call.scope.contains(&zone) && (call.boundary.as_ref().is_none_or(fetching) || !call.drains.contains(&zone))
        }) || s
            .adapter
            .scheduled
            .as_ref()
            .is_some_and(|boundary| boundary.zones.contains_key(&zone))
    }

    pub fn prepare_cloud_media_sdk(&self, sdk: &str) -> Result<(), RuntimeError> {
        self.prepare_cloud_media_sdk_with_work(sdk, &mut PolicyWork::default())
    }
    pub fn prepare_cloud_media_sdk_with_work(&self, sdk: &str, work: &mut PolicyWork) -> Result<(), RuntimeError> {
        self.validate_sdk(sdk)?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        work.check()?;
        if s.media.sdk.as_deref() != Some(sdk) {
            work.effect()?;
            // A replacement is registered only after the old native object has
            // drained. Ledger paths survive; callbacks cannot acquire its Work.
            s.actions.media_upload = None;
            s.actions.media_batches.clear();
            s.media.pending = None;
            s.media.metadata_pending = None;
            s.media.recovered = false;
            s.media.root_checked = false;
            s.media.round_reads.clear();
            s.media.attempted.clear();
            s.media.sdk = Some(sdk.into());
        }
        Ok(())
    }

    pub(crate) fn policy_begin_media_with_work(
        &self,
        guard: &crate::cloud::adapter::Guard,
        sdk: Option<&str>,
        action: &crate::cloud::policy::ActionKind,
        work: &mut PolicyWork,
    ) -> Result<bool, RuntimeError> {
        let _ = self.adapter_now();
        if let Some(sdk) = sdk {
            self.prepare_cloud_media_sdk_with_work(sdk, work)?;
            #[cfg(feature = "test-hooks")]
            self.incoming_hooks
                .hit(crate::cloud::testing::IncomingStage::PolicyMediaPrepared);
        }
        let mut s = self.state();
        work.check()?;
        if Self::adapter_guard_locked(&s) != *guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        match action {
            crate::cloud::policy::ActionKind::PauseMedia => {
                work.effect()?;
                s.media.paused = true;
                s.media.epoch = s.media.epoch.checked_add(1).ok_or(Error::InvalidData)?;
                s.media.pending = None;
                s.media.metadata_pending = None;
                self.emit_pending_locked(&mut s, workflow::UpdateKind::Full, BTreeSet::new())?;
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(crate::cloud::testing::IncomingStage::PolicyMediaSelected);
                Ok(false)
            }
            crate::cloud::policy::ActionKind::PrioritizeMedia { names } => {
                work.effect()?;
                s.media.visible = names.iter().map(|name| name.to_lowercase()).collect();
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(crate::cloud::testing::IncomingStage::PolicyMediaSelected);
                Ok(false)
            }
            crate::cloud::policy::ActionKind::ResumeMedia | crate::cloud::policy::ActionKind::ReadBlobs { .. } => {
                Self::sdk_locked(&s, sdk.ok_or(RuntimeError::StaleIncarnation)?)?;
                Self::gate_locked(&s)?;
                work.effect()?;
                s.media.paused = false;
                s.media.round_reads.clear();
                s.media.attempted.clear();
                s.media.no_space = false;
                s.media.due_loaded = false;
                s.media.batch_reads = true;
                s.media.suppress_uploads = false;
                s.media.metadata_pending = None;
                match action {
                    crate::cloud::policy::ActionKind::ResumeMedia => {
                        s.media.selection = Selection::Resume;
                        s.media.root_checked = false;
                        if !s
                            .media
                            .pending
                            .as_ref()
                            .is_some_and(|work| matches!(work.request, Request::Upload { .. }))
                        {
                            s.media.pending = None;
                        }
                    }
                    crate::cloud::policy::ActionKind::ReadBlobs { include_missing } => {
                        s.media.selection = Selection::Read {
                            include_missing: *include_missing,
                        };
                        s.media.root_checked = true;
                    }
                    _ => unreachable!(),
                }
                #[cfg(feature = "test-hooks")]
                self.incoming_hooks
                    .hit(crate::cloud::testing::IncomingStage::PolicyMediaSelected);
                Ok(true)
            }
            _ => Err(Error::InvalidData.into()),
        }
    }
}

impl Runtime {
    pub(crate) fn policy_media_setting_with_work(
        &self,
        enabled: bool,
        work: &mut PolicyWork,
    ) -> Result<(), RuntimeError> {
        let mut s = self.state();
        work.effect()?;
        s.media.enabled = Some(enabled);
        if !enabled {
            s.media.paused = true;
            s.media.root_checked = false;
            s.media.epoch = s.media.epoch.checked_add(1).ok_or(Error::InvalidData)?;
            s.media.pending = None;
            s.media.metadata_pending = None;
        }
        Ok(())
    }
}

impl Runtime {
    pub(crate) fn policy_media_quota(
        &self,
        guard: &crate::cloud::adapter::Guard,
        sdk: &str,
    ) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
        let mut s = self.state();
        if Self::adapter_guard_locked(&s) != *guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        Self::sdk_locked(&s, sdk)?;
        s.media.suppress_uploads = true;
        if let Some((_, work)) = &s.actions.media_upload
            && let Request::Upload { name, .. } = &work.request
        {
            let name = name.clone();
            s.policy.parked.insert(name);
        }
        self.emit_pending_locked(&mut s, workflow::UpdateKind::Changes, BTreeSet::new())?;
        Ok(())
    }
}
