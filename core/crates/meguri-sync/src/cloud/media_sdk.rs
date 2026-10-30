//! One-engine media send bridge. Scheduling and file decisions stay in Runtime.
use super::*;

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
    ) -> Result<(), RuntimeError> {
        self.validate_sdk(sdk)?;
        let work = self
            .state()
            .actions
            .media_batches
            .get(&(sdk.to_owned(), batch))
            .cloned()
            .ok_or(RuntimeError::StaleIncarnation)?;
        self.validate_cloud_media_admission(sdk, collection, &work)?;
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
        Ok(())
    }
    pub fn queue_cloud_media_upload(
        &self,
        sdk: &str,
        collection: &Collection,
        work: &Work,
    ) -> Result<(), RuntimeError> {
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
        requested: &BTreeSet<String>,
    ) -> Result<Option<(u64, Work)>, RuntimeError> {
        self.validate_sdk(sdk)?;
        let queued = self.state().actions.media_upload.clone();
        let Some((owner, work)) = queued.filter(|(owner, _)| owner == sdk) else {
            return Ok(None);
        };
        self.validate_cloud_media_work(collection, &work)?;
        let Request::Upload { name, envelope, staged } = &work.request else {
            return Err(Error::InvalidData.into());
        };
        if !requested.contains(name) {
            return Ok(None);
        }
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
        for ((original_sdk, id), original) in &s.actions.media_batches {
            if let Request::Upload {
                name: original_name, ..
            } = &original.request
                && original_name == name
            {
                if original_sdk == sdk && original == &work {
                    return Ok(Some((*id, original.clone())));
                }
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        s.actions.batch = s.actions.batch.checked_add(1).ok_or(Error::InvalidData)?;
        let id = s.actions.batch;
        s.actions.media_batches.insert((sdk.into(), id), work.clone());
        s.actions.observations.push(workflow::Observation::BatchBuilt {
            names: BTreeSet::from([name.clone()]),
            batch_id: id,
        });
        Ok(Some((id, work)))
    }

    /// False means a collection batch. Matching media batches retain their
    /// ORIGINAL Work through transient SDK failures, never reconstructed ids.
    pub fn cloud_media_sdk_sent(
        &self,
        sdk: &str,
        collection: &Collection,
        request_id: u64,
        saved: &[workflow::Saved],
        failed: &[(String, workflow::SendError)],
        retrying: &[String],
    ) -> Result<bool, RuntimeError> {
        self.validate_sdk(sdk)?;
        let key = (sdk.to_owned(), request_id);
        let Some(work) = self.state().actions.media_batches.get(&key).cloned() else {
            return Ok(false);
        };
        self.validate_cloud_media_work(collection, &work)?;
        let Request::Upload { name, envelope, staged } = &work.request else {
            return Err(Error::InvalidData.into());
        };
        if saved.len() > 1
            || failed.len() > 1
            || retrying.len() > 1
            || saved.len() + failed.len() + retrying.len() != 1
            || saved.iter().any(|r| r.name != *name || r.envelope != *envelope)
            || failed.iter().any(|(n, _)| n != name)
            || retrying.iter().any(|n| n != name)
            || self.state().actions.handler != Some(request_id)
        {
            return Err(Error::InvalidData.into());
        }
        if let Some(record) = saved.first() {
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
        if !saved.is_empty() {
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
        self.validate_sdk(sdk)?;
        let mut s = self.state();
        Self::sdk_locked(&s, sdk)?;
        if s.media.sdk.as_deref() != Some(sdk) {
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

    pub(crate) fn policy_begin_media(
        &self,
        guard: &crate::cloud::adapter::Guard,
        sdk: Option<&str>,
        action: &crate::cloud::policy::ActionKind,
    ) -> Result<bool, RuntimeError> {
        if let Some(sdk) = sdk {
            self.prepare_cloud_media_sdk(sdk)?;
        }
        let mut s = self.state();
        if Self::adapter_guard_locked(&s) != *guard {
            return Err(RuntimeError::StaleIncarnation);
        }
        match action {
            crate::cloud::policy::ActionKind::PauseMedia => {
                s.media.paused = true;
                s.media.epoch = s.media.epoch.checked_add(1).ok_or(Error::InvalidData)?;
                s.media.pending = None;
                s.media.metadata_pending = None;
                self.emit_pending_locked(&mut s, workflow::UpdateKind::Full, BTreeSet::new())?;
                Ok(false)
            }
            crate::cloud::policy::ActionKind::PrioritizeMedia { names } => {
                s.media.visible = names.iter().map(|name| name.to_lowercase()).collect();
                Ok(false)
            }
            crate::cloud::policy::ActionKind::ResumeMedia | crate::cloud::policy::ActionKind::ReadBlobs { .. } => {
                Self::sdk_locked(&s, sdk.ok_or(RuntimeError::StaleIncarnation)?)?;
                Self::gate_locked(&s)?;
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
                Ok(true)
            }
            _ => Err(Error::InvalidData.into()),
        }
    }
}

impl Runtime {
    pub(crate) fn policy_media_setting(&self, enabled: bool) -> Result<(), RuntimeError> {
        let mut s = self.state();
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
