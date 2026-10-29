use super::*;
use crate::cloud::{
    incoming::{ApplyIntent, ApplyKey, ApplyKeyRole},
    media::{self as m, *},
    workflow::EntityFacts,
};
use meguri_engine::media::{MediaJobKind, MediaService};
use meguri_storage::media::{MediaAbsenceCause, MediaFeedId, MediaFileState};
use std::collections::{BTreeMap, BTreeSet};

fn media_error(_: impl std::fmt::Display) -> RuntimeError {
    Error::Io(std::io::ErrorKind::Other).into()
}
fn text(bytes: &[u8]) -> Result<&str, RuntimeError> {
    std::str::from_utf8(bytes).map_err(|_| Error::InvalidData.into())
}
fn same(a: Option<&codec::MediaName>, b: Option<&codec::MediaName>) -> bool {
    a == b
}
impl Runtime {
    #[cfg(feature = "test-hooks")]
    pub fn media_conflicts_for_testing(&self) -> Result<Vec<ConflictCopy>, RuntimeError> {
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| {
            Ok(s.store
                .as_ref()
                .ok_or(Error::Database)?
                .conflict_copies()?
                .into_iter()
                .filter(|c| c.binding == b && c.copy.kind == ConflictKind::MediaName)
                .map(|c| c.copy)
                .collect())
        })
    }
    pub(super) fn media_service(&self, collection: &Collection) -> Result<MediaService, RuntimeError> {
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        Ok(MediaService::open(collection, clock)?)
    }
    fn media_authority(&self, collection: &Collection, verified: bool) -> Result<Authority, RuntimeError> {
        let evidence = collection.open_evidence()?;
        let file = self.inputs.files.identity(collection.path())?;
        let s = self.state();
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if !b.usable
            || verified && !b.verified
            || evidence.generation != b.evidence.generation
            || file != b.file
            || evidence.native_file != file.pair()
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(Authority {
            binding: b.binding.id,
            open: b.open_counter,
            incarnation: s.incarnation,
            generation: evidence.generation.0,
            file,
            account: b.binding.facts.account.clone(),
            adapter_token: Self::adapter_guard_locked(&s).token,
            media_epoch: s.media.epoch,
        })
    }
    fn check_media_locked(s: &State, a: &Authority) -> Result<(), RuntimeError> {
        let b = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if b.binding.id != a.binding
            || b.open_counter != a.open
            || s.incarnation != a.incarnation
            || b.evidence.generation.0 != a.generation
            || b.file != a.file
            || b.binding.facts.account != a.account
            || !b.usable
            || !b.verified
            || b.binding.facts.state != BindingState::Active
            || s.invalidation_exhausted
            || Self::adapter_guard_locked(s).token != a.adapter_token
            || s.media.epoch != a.media_epoch
            || s.adapter.paused
            || s.media.paused
            || s.tracking_stopped
            || s.adapter.sdk.as_ref().is_some_and(|sdk| sdk.stopping)
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
    fn with_media<T>(
        &self,
        a: &Authority,
        work: impl FnOnce(&mut TrackingStore) -> Result<T, Error>,
    ) -> Result<T, RuntimeError> {
        let mut s = self.state();
        Self::check_media_locked(&s, a)?;
        with_tracking_access(|| work(s.store.as_mut().ok_or(Error::Database)?)).map_err(Into::into)
    }
    fn put_media_state(&self, a: &Authority, ledger: &Ledger) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::check_media_locked(&s, a)?;
        let changed = with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.put_media(a.binding, ledger))?;
        self.notify_media_pending_locked(&mut s, changed)?;
        Ok(())
    }
    fn notify_media_pending_locked(&self, s: &mut State, changed: bool) -> Result<(), RuntimeError> {
        s.media.pending_notification |= changed;
        if s.media.pending_notification && !s.incoming.running {
            self.emit_pending_locked(s, workflow::UpdateKind::Full, BTreeSet::new())?;
            s.media.pending_notification = false;
        }
        Ok(())
    }
    fn finish_media_pending(&self, a: &Authority) -> Result<(), RuntimeError> {
        let mut s = self.state();
        Self::check_media_locked(&s, a)?;
        self.notify_media_pending_locked(&mut s, false)
    }
    fn cloud_staging(&self) -> PathBuf {
        self.directory.join("meguri-cloud-staging")
    }
    fn media_io<T>(
        &self,
        collection: &Collection,
        a: &Authority,
        work: impl FnOnce() -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if self.media_authority(collection, true)? != *a {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        Self::check_media_locked(&s, a)?;
        work()
    }
    pub fn consume_media_feed(&self, collection: &Collection, media: &MediaService) -> Result<usize, RuntimeError> {
        self.consume_media_feed_inner(collection, media, None, None)
    }
    fn consume_media_feed_inner(
        &self,
        collection: &Collection,
        media: &MediaService,
        apply_commit: Option<(
            Authority,
            ApplyIntent,
            Ledger,
            Vec<(Option<EntityFacts>, EntityFacts)>,
            bool,
        )>,
        notification: Option<&Authority>,
    ) -> Result<usize, RuntimeError> {
        let a = self.media_authority(collection, false)?;
        if notification.is_some_and(|issued| issued != &a) {
            return Err(RuntimeError::StaleIncarnation);
        }
        media.check_generation(a.generation)?;
        media.store().check_changes().map_err(media_error)?;
        let (mut feed_state, intents) = {
            let state = self.state();
            with_tracking_access(|| -> Result<_, Error> {
                let store = state.store.as_ref().ok_or(Error::Database)?;
                Ok((store.media_ledger(a.binding)?, store.apply_intents(a.binding)?))
            })?
        };
        if let Some((authority, _, proposed_ledger, _, _)) = &apply_commit {
            if authority != &a || feed_state.revision != proposed_ledger.revision {
                return Err(Error::RecoveryRequired.into());
            }
            feed_state = proposed_ledger.clone();
        }
        let status = media.store().media_feed_status().map_err(media_error)?;
        if status.floor.is_none() {
            media
                .store()
                .set_media_feed_floor(status.feed_id, Some(status.counter))
                .map_err(media_error)?;
        }
        let cursor_start = feed_state.feed.filter(|(id, cursor)| {
            *id == status.feed_id.0
                && *cursor <= status.counter
                && status.floor.is_none_or(|retained_floor| *cursor >= retained_floor)
        });
        let feed = match cursor_start {
            Some((_, cursor)) => media.store().media_feed(cursor),
            None => media.store().media_feed_snapshot(),
        }
        .map_err(media_error)?;
        let complete = feed.status.complete;
        let mut file_changes = Vec::new();
        for item in &feed.items {
            let case = item.name.to_lowercase();
            let previous = feed_state.names.get(&case).cloned().unwrap_or_default();
            let mut flags = previous.clone();
            let source = match &item.state {
                MediaFileState::Present(f) => Some((f.sha1.clone(), f.size, f.mtime_secs, f.mtime_nanos)),
                _ => None,
            };
            let payload = match &item.state {
                MediaFileState::Present(f) if (1..=MAX_ASSET_BYTES).contains(&f.size) => {
                    if source == flags.source {
                        flags.local.clone()
                    } else {
                        let file = media
                            .store()
                            .open_file(&item.name)
                            .map_err(media_error)?
                            .ok_or(Error::InvalidData)?;
                        let found = m::io::hash(file)?;
                        if found.size != f.size || m::io::sha1_text(&found.sha1) != f.sha1 {
                            return Err(Error::InvalidData.into());
                        }
                        Some(found.media(&item.name))
                    }
                }
                _ => None,
            };
            let own = intents.iter().filter_map(|i| i.media.as_ref()).any(|i| {
                let expected = if i.old_name.to_lowercase() == case {
                    vec![i.before.as_ref(), None, i.server.as_ref()]
                } else if i
                    .moved
                    .as_ref()
                    .is_some_and(|v| text(&v.name).ok().is_some_and(|n| n.to_lowercase() == case))
                {
                    vec![i.moved_before.as_ref(), i.moved.as_ref()]
                } else {
                    return false;
                };
                expected.into_iter().any(|e| same(e, payload.as_ref()))
            });
            flags.local = payload.clone();
            flags.source = source;
            if own {
                feed_state.names.insert(case, flags);
                continue;
            }
            flags.media_changed = None;
            let tracked = {
                let state = self.state();
                with_tracking_access(|| {
                    let store = state.store.as_ref().ok_or(Error::Database)?;
                    let binding = store.binding(a.binding)?.ok_or(Error::BindingNotFound)?;
                    let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::MediaName, case.as_bytes())
                        .map_err(|_| Error::InvalidData)?;
                    store.entity_facts(a.binding, &name)
                })?
            };
            let ack = tracked.as_ref().and_then(|e| e.acked_payload.as_ref()).and_then(|p| {
                if let codec::Payload::MediaName(v) = p {
                    Some(v)
                } else {
                    None
                }
            });
            if let Some(p) = &payload {
                flags.file_missing = false;
                flags.unsyncable = false;
                flags.removal_held = false;
                if flags.withheld.is_some_and(|h| h != p.sha256) {
                    flags.withheld = None;
                }
                if flags.superseded.is_some_and(|h| h != p.sha256) || ack.is_some_and(|v| v.sha256 == p.sha256) {
                    flags.superseded = None;
                }
                if feed_state.adoption {
                    if let Some(acked) = ack {
                        if acked.sha256 != p.sha256 {
                            flags.superseded = Some(p.sha256);
                        }
                    } else {
                        flags.withheld = Some(p.sha256);
                    }
                }
                if flags.withheld.is_none() && flags.superseded.is_none() {
                    flags.tombstone_at = None;
                    file_changes.push((codec::Payload::MediaName(p.clone()), self.publication_random_id()?));
                } else if tracked.is_none() {
                    file_changes.push((codec::Payload::MediaName(p.clone()), self.publication_random_id()?));
                }
                let metadata = blob::Blob::from(p);
                let root = self.publication_media_state()?;
                if let Some((key, true, _)) = root {
                    let name = metadata.name(&key.0);
                    let bound = feed_state.blobs.entry(name).or_insert_with(|| BlobState {
                        metadata: metadata.clone(),
                        ..Default::default()
                    });
                    if previous.local.as_ref().is_none_or(|v| v.sha256 != p.sha256)
                        && ack.is_none_or(|v| v.sha256 != p.sha256)
                    {
                        bound.server = ServerState::Unknown;
                    }
                }
            } else if matches!(item.state, MediaFileState::Present(_)) {
                flags.unsyncable = true;
            } else if flags.withheld.is_some() {
                if matches!(item.state, MediaFileState::Absent(MediaAbsenceCause::Removed)) {
                    flags.withheld = None;
                }
            } else if flags.superseded.take().is_some() {
                flags.file_missing = true;
            } else if matches!(item.state, MediaFileState::Absent(MediaAbsenceCause::Removed)) {
                flags.removal_held = !complete;
                if complete {
                    flags.file_missing = false;
                    flags.unsyncable = false;
                    flags.tombstone_at = Some(self.media_now()?);
                    if let Some(e) = &tracked {
                        file_changes.push((e.payload.clone(), e.origin));
                    }
                }
            } else {
                flags.file_missing = tracked.is_some();
            }
            feed_state.names.insert(case, flags);
        }
        if cursor_start.is_none() && complete {
            let seen: BTreeSet<_> = feed.items.iter().map(|i| i.name.to_lowercase()).collect();
            for (name, f) in &mut feed_state.names {
                if !seen.contains(name) && f.withheld.is_none() {
                    f.local = None;
                    f.source = None;
                    f.file_missing = true;
                }
            }
        }
        if complete {
            feed_state.adoption = false;
        }
        feed_state.feed = Some((feed.status.feed_id.0, feed.status.counter));
        media.check_generation(a.generation)?;
        let mut state = self.state();
        let bound = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if bound.binding.id != a.binding
            || bound.open_counter != a.open
            || state.incarnation != a.incarnation
            || bound.file != a.file
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if apply_commit.is_some() {
            Self::check_media_locked(&state, &a)?;
        }
        let (changed_work, retained_floor) = with_tracking_access(|| -> Result<_, Error> {
            let store = state.store.as_mut().ok_or(Error::Database)?;
            let changed_work = store.commit_media_feed(
                a.binding,
                &feed_state,
                &file_changes,
                (a.open, collection.last_seq().0, a.generation),
                apply_commit
                    .as_ref()
                    .map(|(_, intent, _, proposals, clear)| (intent, proposals.as_slice(), *clear)),
            )?;
            Ok((changed_work, store.media_floor(feed.status.feed_id.0)?))
        })?;
        self.notify_media_pending_locked(&mut state, changed_work)?;
        drop(state);
        if let Some(retained_floor) = retained_floor {
            media
                .store()
                .set_media_feed_floor(MediaFeedId(feed.status.feed_id.0), Some(retained_floor))
                .map_err(media_error)?;
        }
        Ok(feed.items.len())
    }
    fn media_now(&self) -> Result<i64, RuntimeError> {
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        Ok(clock.now_ms())
    }
    pub fn resume_cloud_media(&self) {
        let mut s = self.state();
        s.media.paused = false;
        s.media.root_checked = false;
        s.media.recovered = false;
        s.media.attempted.clear();
    }
    pub fn pause_cloud_media(&self) {
        let mut s = self.state();
        s.media.paused = true;
        s.media.epoch = s.media.epoch.saturating_add(1);
        s.media.pending = None;
    }
    pub fn cloud_media_work(
        &self,
        collection: &Collection,
        media: &MediaService,
    ) -> Result<Option<Work>, RuntimeError> {
        self.consume_media_feed(collection, media)?;
        let a = self.media_authority(collection, true)?;
        {
            let mut s = self.state();
            if s.media.paused
                || s.adapter.paused
                || s.scan_running
                || s.tracking_stopped
                || !s.whole_complete
                || s.scan_required
            {
                return Ok(None);
            }
            if s.media.pending.as_ref().is_some_and(|w| w.authority != a) {
                s.media.pending = None;
                s.media.recovered = false;
            }
            if let Some(w) = &s.media.pending {
                return Ok(Some(w.clone()));
            }
        }
        let (mut ledger, key) = self.with_media(&a, |store| {
            Ok((store.media_ledger(a.binding)?, store.publication_media(a.binding)?))
        })?;
        if ledger.zone_missing {
            return Ok(None);
        }
        let Some((key, true, _)) = key else { return Ok(None) };
        if !self.state().media.recovered {
            for blob in ledger.blobs.values_mut() {
                if blob.download == Transfer::Running {
                    blob.download = Transfer::Queued;
                }
                if blob.upload == Transfer::Running {
                    blob.upload = Transfer::None;
                    blob.staged = None;
                }
            }
            self.put_media_state(&a, &ledger)?;
            ledger = self.with_media(&a, |s| s.media_ledger(a.binding))?;
            self.state().media.recovered = true;
        }
        if self.state().media.root_checked {
            let staged: Vec<_> = ledger
                .blobs
                .iter()
                .filter(|(_, b)| b.download == Transfer::Staged)
                .map(|(name, b)| (name.clone(), b.staged.clone(), b.metadata.clone()))
                .collect();
            for (name, path, metadata) in staged {
                let valid = path.as_ref().is_some_and(|p| {
                    m::io::open(p)
                        .and_then(m::io::hash)
                        .is_ok_and(|found| found == metadata)
                });
                if valid {
                    let running = {
                        let mut s = self.state();
                        if s.incoming.running {
                            return Ok(None);
                        }
                        s.incoming.running = true;
                        MediaRunning(self.state.clone(), self.incoming_idle.clone())
                    };
                    self.apply_staged_media(collection, media, &a, &name)?;
                    drop(running);
                    self.finish_media_pending(&a)?;
                    ledger = self.with_media(&a, |s| s.media_ledger(a.binding))?;
                } else if let Some(blob) = ledger.blobs.get_mut(&name) {
                    if let Some(path) = blob.staged.take() {
                        let _ = std::fs::remove_file(path);
                    }
                    blob.download = Transfer::Queued;
                }
            }
        }
        let request = if !self.state().media.root_checked {
            Some(Request::Root)
        } else {
            let entries = self.with_media(&a, |store| store.inbox(a.binding))?;
            for entry in entries {
                let InboxValue::Record { envelope, .. } = &entry.entry.value else {
                    continue;
                };
                let r = codec::receive(
                    &self.media_record_key(&a)?,
                    &entry.entry.record_name,
                    &envelope.0,
                    codec::AssetInput::Absent,
                );
                if let Ok(r) = r {
                    if r.entity.fields.tombstone {
                        continue;
                    }
                    if let Some(codec::Payload::MediaName(p)) = r.payload {
                        let metadata = blob::Blob::from(&p);
                        let name = metadata.name(&key.0);
                        ledger.blobs.entry(name).or_insert_with(|| BlobState {
                            metadata,
                            download: Transfer::Queued,
                            ..Default::default()
                        });
                    }
                }
            }
            for e in self.with_media(&a, |store| store.entity_page(a.binding, codec::Kind::MediaName, None))? {
                if e.acked_media_tombstone() {
                    continue;
                }
                if let codec::Payload::MediaName(p) = e.payload {
                    let flags = ledger.names.get(&text(&p.name)?.to_lowercase());
                    if e.mark.is_none() && flags.is_none_or(|f| f.local.as_ref().is_none_or(|l| l.sha256 != p.sha256)) {
                        let metadata = blob::Blob::from(&p);
                        let name = metadata.name(&key.0);
                        ledger.blobs.entry(name).or_insert_with(|| BlobState {
                            metadata,
                            download: Transfer::Queued,
                            ..Default::default()
                        });
                    }
                }
            }
            let attempted = self.state().media.attempted.clone();
            let mut request = None;
            for (name, b) in &mut ledger.blobs {
                if attempted.contains(name) {
                    continue;
                }
                if b.server == ServerState::Unknown
                    || b.server == ServerState::Deleted && b.download == Transfer::Queued
                {
                    request = Some(Request::Read { name: name.clone() });
                    break;
                }
                if b.server == ServerState::Present && b.download == Transfer::Queued {
                    b.download = Transfer::Running;
                    request = Some(Request::Download { name: name.clone() });
                    break;
                }
                if b.server == ServerState::Deleted && b.upload != Transfer::Complete {
                    let candidate = ledger
                        .names
                        .values()
                        .find(|f| f.sendable() && f.local.as_ref().is_some_and(|p| p.sha256 == b.metadata.sha256))
                        .and_then(|f| f.local.clone());
                    if let Some(local) = candidate {
                        let file = media
                            .store()
                            .open_file(text(&local.name)?)
                            .map_err(media_error)?
                            .ok_or(Error::InvalidData)?;
                        let (staged, actual) =
                            self.media_io(collection, &a, || Ok(m::io::stage(file, &self.cloud_staging())?))?;
                        if actual != b.metadata {
                            let _ = std::fs::remove_file(&staged);
                            if let Some(flags) = ledger.names.get_mut(&text(&local.name)?.to_lowercase()) {
                                flags.media_changed = Some(actual.sha256);
                            }
                            continue;
                        }
                        b.upload = Transfer::Running;
                        b.staged = Some(staged.clone());
                        request = Some(Request::Upload {
                            name: name.clone(),
                            envelope: actual.encode().map_err(|_| Error::InvalidData)?,
                            staged,
                        });
                        break;
                    }
                }
            }
            request
        };
        self.put_media_state(&a, &ledger)?;
        let Some(request) = request else { return Ok(None) };
        let mut s = self.state();
        Self::check_media_locked(&s, &a)?;
        s.media.next = s.media.next.checked_add(1).ok_or(Error::InvalidData)?;
        let work = Work {
            id: s.media.next,
            authority: a,
            request,
        };
        s.media.pending = Some(work.clone());
        Ok(Some(work))
    }
    fn media_record_key(&self, a: &Authority) -> Result<[u8; 32], RuntimeError> {
        self.with_media(a, |store| {
            Ok(store
                .binding(a.binding)?
                .ok_or(Error::BindingNotFound)?
                .facts
                .record_key
                .0)
        })
    }
    pub fn cloud_media_reply(
        &self,
        collection: &Collection,
        media: &MediaService,
        work: &Work,
        reply: Reply,
    ) -> Result<(), RuntimeError> {
        let a = &work.authority;
        #[cfg(feature = "test-hooks")]
        if matches!(work.request, Request::Download { .. }) {
            self.incoming_hooks
                .hit(crate::cloud::testing::IncomingStage::MediaReplyBeforeApply);
        }
        self.validate_cloud_media_work(collection, work)?;
        if self.publication_account()? != a.account {
            return Err(RuntimeError::IdentityUnverified);
        }
        if self.media_authority(collection, true)? != *a {
            return Err(RuntimeError::StaleIncarnation);
        }
        {
            let s = self.state();
            if s.media.pending.as_ref() != Some(work) {
                return Err(RuntimeError::StaleIncarnation);
            }
        }
        let (mut ledger, key) = self.with_media(a, |store| {
            Ok((
                store.media_ledger(a.binding)?,
                store.publication_media(a.binding)?.ok_or(Error::InvalidData)?.0,
            ))
        })?;
        let failed = matches!(&reply, Reply::Failed { .. });
        let mut staged_download = None;
        match (&work.request, reply) {
            (_, Reply::ZoneMissing) => ledger.zone_missing = true,
            (Request::Root, Reply::UnknownItem) => ledger.zone_missing = true,
            (
                Request::Root,
                Reply::Record {
                    name,
                    record_type,
                    envelope,
                    asset: None,
                },
            ) if name == "root" && record_type == "MGMediaRoot" => {
                let crate::cloud::control_codec::DecodeOutcome::Supported(root) =
                    crate::cloud::control_codec::MediaRoot::decode(&envelope).map_err(|_| Error::InvalidData)?
                else {
                    return Err(Error::InvalidData.into());
                };
                if root.media_key != key.0 {
                    ledger.zone_missing = true;
                }
            }
            (Request::Read { name } | Request::Download { name }, Reply::UnknownItem) => {
                let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                b.server = ServerState::Deleted;
                if b.download == Transfer::Running {
                    b.download = Transfer::Queued;
                }
            }
            (
                Request::Read { name } | Request::Download { name },
                Reply::Record {
                    name: actual,
                    record_type,
                    envelope,
                    asset,
                },
            ) => {
                if name != &actual || record_type != "MGBlob" {
                    return Err(Error::InvalidData.into());
                }
                let metadata = blob::Blob::decode(&envelope).map_err(|_| Error::InvalidData)?;
                let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                if metadata != b.metadata || metadata.name(&key.0) != *name {
                    return Err(Error::InvalidData.into());
                }
                b.server = ServerState::Present;
                if matches!(work.request, Request::Download { .. }) {
                    let asset = asset.ok_or(Error::InvalidData)?;
                    let (staged, found) = self.media_io(collection, a, || {
                        Ok(m::io::stage(m::io::open(&asset)?, &self.cloud_staging())?)
                    })?;
                    if metadata != found {
                        std::fs::remove_file(staged).map_err(Error::from)?;
                        return Err(Error::InvalidData.into());
                    }
                    b.download = Transfer::Staged;
                    b.staged = Some(staged.clone());
                    staged_download = Some((name.clone(), staged));
                } else if asset.is_some() {
                    return Err(Error::InvalidData.into());
                }
            }
            (Request::Upload { name, .. }, Reply::Saved { name: actual, envelope }) => {
                let metadata = blob::Blob::decode(&envelope).map_err(|_| Error::InvalidData)?;
                let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                if *name != actual || metadata != b.metadata || metadata.name(&key.0) != *name {
                    return Err(Error::InvalidData.into());
                }
                b.server = ServerState::Present;
                b.upload = Transfer::Complete;
            }
            (_, Reply::Failed { .. }) => {}
            _ => return Err(Error::InvalidData.into()),
        }
        #[cfg(feature = "test-hooks")]
        if matches!(work.request, Request::Download { .. }) {
            self.incoming_hooks
                .hit(crate::cloud::testing::IncomingStage::MediaReplyBeforeTracking);
        }
        self.put_media_state(a, &ledger)?;
        {
            let mut s = self.state();
            Self::check_media_locked(&s, a)?;
            s.media.pending = None;
            if matches!(work.request, Request::Root) && !failed {
                s.media.root_checked = true;
            }
            if failed {
                s.media.paused = true;
            }
            match &work.request {
                Request::Download { name } | Request::Upload { name, .. } => {
                    s.media.attempted.insert(name.clone());
                }
                Request::Read { name }
                    if ledger
                        .blobs
                        .get(name)
                        .is_some_and(|b| b.server == ServerState::Deleted && b.download == Transfer::Queued) =>
                {
                    s.media.attempted.insert(name.clone());
                }
                _ => {}
            }
        }
        if failed {
            return Err(Error::Io(std::io::ErrorKind::Other).into());
        }
        if let Some((name, _)) = staged_download {
            meguri_model::faultpoint!("cloud.media.after_download").map_err(Error::from)?;
            let running = {
                let mut s = self.state();
                if s.incoming.running {
                    return Err(EngineError::Busy {
                        running_op: "cloud incoming".into(),
                    }
                    .into());
                }
                s.incoming.running = true;
                MediaRunning(self.state.clone(), self.incoming_idle.clone())
            };
            self.apply_staged_media(collection, media, a, &name)?;
            drop(running);
            self.finish_media_pending(a)?;
        }
        Ok(())
    }
    fn current_media(media: &MediaService, name: &str) -> Result<Option<codec::MediaName>, RuntimeError> {
        media.store().check_changes().map_err(media_error)?;
        let Some(actual) = media.store().lookup(name).map_err(media_error)? else {
            return Ok(None);
        };
        let file = media
            .store()
            .open_file(&actual)
            .map_err(media_error)?
            .ok_or(Error::InvalidData)?;
        Ok(Some(m::io::hash(file)?.media(&actual)))
    }
    fn apply_staged_media(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &Authority,
        blob_name: &str,
    ) -> Result<(), RuntimeError> {
        let ledger = self.with_media(a, |s| s.media_ledger(a.binding))?;
        let b = ledger.blobs.get(blob_name).ok_or(Error::InvalidData)?;
        let staged = b.staged.clone().ok_or(Error::InvalidData)?;
        if m::io::hash(m::io::open(&staged)?)? != b.metadata {
            return Err(Error::InvalidData.into());
        }
        let entries = self.with_media(a, |store| store.inbox(a.binding))?;
        let mut applied = false;
        for entry in entries {
            let InboxValue::Record {
                envelope,
                system_fields,
                ..
            } = &entry.entry.value
            else {
                continue;
            };
            let r = codec::receive(
                &self.media_record_key(a)?,
                &entry.entry.record_name,
                &envelope.0,
                codec::AssetInput::Absent,
            )
            .map_err(|_| Error::InvalidData)?;
            let Some(codec::Payload::MediaName(server)) = r.payload else {
                continue;
            };
            if r.entity.fields.tombstone {
                continue;
            }
            if server.sha256 != b.metadata.sha256 {
                continue;
            }
            let saved = workflow::Saved {
                name: entry.entry.record_name.clone(),
                envelope: envelope.0.clone(),
                system_fields: system_fields.0.clone(),
            };
            self.begin_media_apply(
                collection,
                media,
                a,
                saved,
                Some(entry.id),
                Some(server),
                Some(staged.clone()),
            )?;
            applied = true;
        }
        if !applied {
            let mut after = None;
            loop {
                let page = self.with_media(a, |s| {
                    s.entity_page(a.binding, codec::Kind::MediaName, after.as_deref())
                })?;
                if page.is_empty() {
                    break;
                }
                for e in page {
                    after = Some(e.key.clone());
                    if let codec::Payload::MediaName(p) = e.acked_payload.as_ref().unwrap_or(&e.payload) {
                        if p.sha256 == b.metadata.sha256
                            && Self::current_media(media, text(&p.name)?)?.as_ref() != Some(p)
                        {
                            self.begin_media_apply(
                                collection,
                                media,
                                a,
                                workflow::Saved {
                                    name: e.name.clone(),
                                    envelope: e.acked_envelope.0.clone(),
                                    system_fields: e.acked_system_fields.clone(),
                                },
                                None,
                                Some(p.clone()),
                                Some(staged.clone()),
                            )?;
                        }
                    }
                }
            }
        }
        let mut ledger = self.with_media(a, |s| s.media_ledger(a.binding))?;
        if let Some(b) = ledger.blobs.get_mut(blob_name) {
            b.download = Transfer::Complete;
        }
        self.put_media_state(a, &ledger)?;
        Ok(())
    }
    fn begin_media_apply(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &Authority,
        incoming: workflow::Saved,
        inbox: Option<i64>,
        server: Option<codec::MediaName>,
        staged: Option<PathBuf>,
    ) -> Result<(), RuntimeError> {
        let received = codec::receive(
            &self.media_record_key(a)?,
            &incoming.name,
            &incoming.envelope,
            codec::AssetInput::Absent,
        )
        .map_err(|_| Error::InvalidData)?;
        let existing = self.with_media(a, |s| s.entity_facts(a.binding, &incoming.name))?;
        let name = match server.as_ref() {
            Some(p) => text(&p.name)?.to_owned(),
            None => text(&received.entity.fields.key)?.to_owned(),
        };
        let before = Self::current_media(media, &name)?;
        if before.as_ref() == server.as_ref() && server.is_some() {}
        let ledger = self.with_media(a, |s| s.media_ledger(a.binding))?;
        let flags = ledger.names.get(&name.to_lowercase());
        let changed = existing.as_ref().is_some_and(|e| e.mark.is_some())
            && flags.is_none_or(|f| f.withheld.is_none() && f.superseded.is_none() && !f.file_missing);
        let different = before
            .as_ref()
            .zip(server.as_ref())
            .is_some_and(|(l, s)| l.sha256 != s.sha256);
        let collision = before.as_ref().is_some_and(|v| v.name != name.as_bytes());
        let both_created = existing.as_ref().is_some_and(|e| e.server_erev == 0);
        let moved = if changed && different && (both_created || collision) {
            let old = before.as_ref().ok_or(Error::InvalidData)?;
            let new = media
                .store()
                .cloud_hash_name(text(&old.name)?, &m::io::sha1_text(&old.sha1))
                .map_err(media_error)?;
            let mut p = old.clone();
            p.name = new.into_bytes();
            Some(p)
        } else {
            None
        };
        let moved_before = moved
            .as_ref()
            .map(|p| Self::current_media(media, text(&p.name)?))
            .transpose()?
            .flatten();
        let delete_wins = flags.is_some_and(|f| f.tombstone_at.is_some())
            && existing.as_ref().is_some_and(|e| !e.acked_media_tombstone())
            && server.is_some();
        let local_wins = delete_wins
            || changed
                && different
                && !both_created
                && !collision
                && before.as_ref().zip(server.as_ref()).is_some_and(|(l, s)| {
                    codec::Payload::MediaName(l.clone()).payload_digest().ok()
                        > codec::Payload::MediaName(s.clone()).payload_digest().ok()
                });
        let mut intent = ApplyIntent {
            id: 0,
            binding: a.binding,
            inbox_ids: inbox.into_iter().collect(),
            keys: Vec::new(),
            result_seq: None,
            native_file: a.file,
            generation: a.generation,
            media: Some(Apply {
                incoming,
                server,
                old_name: before
                    .as_ref()
                    .map(|p| text(&p.name).map(str::to_owned))
                    .transpose()?
                    .unwrap_or(name),
                local_wins,
                delete_wins,
                before,
                moved,
                moved_before,
                move_evidence: None,
                install_absent: false,
                staged,
                before_referenced_new: BTreeMap::new(),
                installed: false,
                abandoned: false,
            }),
        };
        if intent.media.as_ref().is_some_and(|i| i.moved.is_some()) {
            self.plan_media_rewrite(collection, a, &mut intent)?;
        }
        intent.id = self.with_media(a, |s| s.persist_apply_intent(&intent, None))?;
        self.execute_media_apply(collection, media, a, intent)
    }
    fn plan_media_rewrite(
        &self,
        collection: &Collection,
        a: &Authority,
        intent: &mut ApplyIntent,
    ) -> Result<(), RuntimeError> {
        let apply = intent.media.as_ref().ok_or(Error::InvalidData)?;
        let new = text(&apply.moved.as_ref().ok_or(Error::InvalidData)?.name)?.to_owned();
        let old = apply.old_name.clone();
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut after = None;
        loop {
            let page = snapshot.row_page(Table::Notes, after.as_ref())?;
            if page.is_empty() {
                break;
            }
            for row in page {
                let key = RowKey::from_stored(Table::Notes, &row).map_err(meguri_storage::StorageError::from)?;
                after = Some(key.clone());
                let fields = match &row[6] {
                    Cell::Text(v) => text(v)?,
                    _ => return Err(Error::InvalidData.into()),
                };
                if !meguri_text::media_refs::local_references(fields).contains(&old) {
                    continue;
                }
                let RowKey::Id(id) = key else {
                    return Err(Error::InvalidData.into());
                };
                let name = codec::entity_name(&self.media_record_key(a)?, codec::Kind::Note, &codec::id_key(id))
                    .map_err(|_| Error::InvalidData)?;
                let e = self
                    .with_media(a, |s| s.entity_facts(a.binding, &name))?
                    .ok_or(Error::InvalidData)?;
                let codec::Payload::Note(n) = &e.payload else {
                    return Err(Error::InvalidData.into());
                };
                let before = note_image(&row, n.nt_struct_gen)?;
                let rewritten = rewrite_note(before.clone(), &old, &new, self.media_now()?, 0)?;
                intent
                    .media
                    .as_mut()
                    .ok_or(Error::InvalidData)?
                    .before_referenced_new
                    .insert(
                        name.clone(),
                        meguri_text::media_refs::local_references(fields).contains(&new),
                    );
                let before = codec::Payload::Note(before);
                let rewritten = codec::Payload::Note(rewritten);
                intent.keys.push(ApplyKey {
                    name,
                    kind: codec::Kind::Note,
                    key: codec::id_key(id).to_vec(),
                    role: ApplyKeyRole::SideEffect,
                    before_digest: Some(before.row_digest().map_err(|_| Error::InvalidData)?),
                    after_digest: Some(rewritten.row_digest().map_err(|_| Error::InvalidData)?),
                    before_payload: Some(before),
                    after_payload: Some(rewritten),
                    own_flight: None,
                });
            }
        }
        Ok(())
    }
    fn rewrite_media_notes(
        &self,
        collection: &Collection,
        a: &Authority,
        intent: &mut ApplyIntent,
    ) -> Result<(), RuntimeError> {
        let apply = intent.media.as_ref().ok_or(Error::InvalidData)?;
        let old = apply.old_name.clone();
        let new = text(&apply.moved.as_ref().ok_or(Error::InvalidData)?.name)?.to_owned();
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut ran = false;
        for k in &intent.keys {
            let Some(row) = snapshot.row_image(
                Table::Notes,
                &RowKey::Id(i64::from_be_bytes(
                    k.key.as_slice().try_into().map_err(|_| Error::InvalidData)?,
                )),
            )?
            else {
                continue;
            };
            let Some(codec::Payload::Note(n)) = &k.before_payload else {
                return Err(Error::InvalidData.into());
            };
            let current = codec::Payload::Note(note_image(&row, n.nt_struct_gen)?);
            let fields = match &current {
                codec::Payload::Note(n) => text(&n.flds)?,
                _ => unreachable!(),
            };
            ran |= Some(current.row_digest().map_err(|_| Error::InvalidData)?) == k.after_digest
                || !apply.before_referenced_new.get(&k.name).copied().unwrap_or(true)
                    && meguri_text::media_refs::local_references(fields).contains(&new);
        }
        drop(snapshot);
        drop(lease);
        if ran {
            return Ok(());
        }
        let mut planned = intent.clone();
        planned.result_seq = None;
        let shared = self.state.clone();
        let authority = a.clone();
        let now = self.media_now()?;
        let result = collection.run(
            meguri_engine::OpSpec::named("cloud_incoming_apply", meguri_engine::OpSource::CloudIncoming),
            move |ctx| {
                if ctx.generation().0 != authority.generation {
                    return Err(engine_error("media rewrite generation"));
                }
                for k in &mut planned.keys {
                    let id = i64::from_be_bytes(
                        k.key
                            .as_slice()
                            .try_into()
                            .map_err(|_| engine_error("media rewrite key"))?,
                    );
                    let Some(row) = ctx.scope().row_image(Table::Notes, &RowKey::Id(id))? else {
                        k.before_digest = None;
                        k.after_digest = None;
                        k.before_payload = None;
                        k.after_payload = None;
                        continue;
                    };
                    let Some(codec::Payload::Note(layout)) = &k.before_payload else {
                        return Err(engine_error("media rewrite layout"));
                    };
                    let before = note_image(&row, layout.nt_struct_gen).map_err(engine_error)?;
                    let fields = text(&before.flds).map_err(engine_error)?;
                    planned
                        .media
                        .as_mut()
                        .ok_or_else(|| engine_error("media intent"))?
                        .before_referenced_new
                        .insert(
                            k.name.clone(),
                            meguri_text::media_refs::local_references(fields).contains(&new),
                        );
                    let sort = ctx
                        .scope()
                        .notetype(meguri_model::ids::NotetypeId(before.mid))?
                        .ok_or_else(|| engine_error("media rewrite notetype"))?
                        .config
                        .sort_field_idx as usize;
                    let after = rewrite_note(before.clone(), &old, &new, now, sort).map_err(engine_error)?;
                    let before = codec::Payload::Note(before);
                    let after = codec::Payload::Note(after);
                    k.before_digest = Some(before.row_digest().map_err(engine_error)?);
                    k.after_digest = Some(after.row_digest().map_err(engine_error)?);
                    k.before_payload = Some(before);
                    k.after_payload = Some(after.clone());
                    crate::cloud::row_payload::stage_payload(ctx.scope(), &after).map_err(engine_error)?;
                }
                let mut s = shared.lock().unwrap_or_else(PoisonError::into_inner);
                Self::check_media_locked(&s, &authority).map_err(engine_error)?;
                with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.update_media_intent(&planned))
                    .map_err(engine_error)?;
                Ok(planned)
            },
        )?;
        *intent = result.value;
        intent.result_seq = Some(result.seq.0);
        self.with_media(a, |s| s.update_media_intent(intent))?;
        meguri_model::faultpoint!("cloud.media.rewrite_after_commit").map_err(Error::from)?;
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::ResolutionRewriteCommitted);
        Ok(())
    }
    fn execute_media_apply(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &Authority,
        mut intent: ApplyIntent,
    ) -> Result<(), RuntimeError> {
        if intent.native_file != a.file {
            return Err(RuntimeError::FileIdentityUnavailable);
        }
        let _job = media.try_job(MediaJobKind::CheckMedia)?;
        let mut apply = intent.media.clone().ok_or(Error::InvalidData)?;
        let mut moved_ran = false;
        if let Some(moved) = &apply.moved {
            let current_new = Self::current_media(media, text(&moved.name)?)?;
            let current_old = Self::current_media(media, &apply.old_name)?;
            moved_ran = match &apply.move_evidence {
                Some(proof) => media
                    .store()
                    .cloud_move_completed(
                        &apply.old_name,
                        text(&moved.name)?,
                        &m::io::sha1_text(&moved.sha1),
                        proof,
                    )
                    .map_err(media_error)?,
                None => false,
            };
            if !moved_ran {
                if !same(current_old.as_ref(), apply.before.as_ref())
                    || !same(current_new.as_ref(), apply.moved_before.as_ref())
                {
                    apply.abandoned = true;
                } else {
                    #[cfg(feature = "test-hooks")]
                    self.incoming_hooks
                        .hit(crate::cloud::testing::IncomingStage::ResolutionBeforeMove);
                    if apply.move_evidence.is_none() {
                        apply.move_evidence = self.media_io(collection, a, || {
                            media
                                .store()
                                .cloud_move_evidence(
                                    &apply.old_name,
                                    text(&moved.name)?,
                                    &m::io::sha1_text(&moved.sha1),
                                )
                                .map_err(media_error)
                        })?;
                        intent.media = Some(apply.clone());
                        self.with_media(a, |s| s.update_media_intent(&intent))?;
                    }
                    moved_ran = if let Some(proof) = &apply.move_evidence {
                        self.media_io(collection, a, || {
                            media
                                .store()
                                .cloud_move_guarded(
                                    &apply.old_name,
                                    text(&moved.name)?,
                                    &m::io::sha1_text(&moved.sha1),
                                    proof,
                                )
                                .map_err(media_error)
                        })?
                    } else {
                        false
                    };
                    if !moved_ran {
                        apply.abandoned = true;
                    } else {
                        meguri_model::faultpoint!("cloud.media.after_hash_move").map_err(Error::from)?;
                        #[cfg(feature = "test-hooks")]
                        self.incoming_hooks
                            .hit(crate::cloud::testing::IncomingStage::ResolutionMoveBeforeRewrite);
                    }
                }
            }
            if moved_ran {
                intent.media = Some(apply.clone());
                if let Some(staged) = &apply.staged {
                    let _verified = m::io::hash(m::io::open(staged)?)?;
                }
                self.rewrite_media_notes(collection, a, &mut intent)?;
                apply = intent.media.clone().ok_or(Error::InvalidData)?;
            }
        }
        if !apply.abandoned && !apply.local_wins {
            let expected = if moved_ran || apply.install_absent {
                None
            } else {
                apply.before.as_ref()
            };
            let current = Self::current_media(media, &apply.old_name)?;
            let already = same(current.as_ref(), apply.server.as_ref());
            if already {
                apply.installed = true;
            } else if !same(current.as_ref(), expected) && current.is_some() {
                apply.abandoned = true;
            } else {
                self.with_media(a, |_| Ok(()))?;
                media.check_generation(a.generation)?;
                if let Some(server) = &apply.server {
                    let staged = apply.staged.as_ref().ok_or(Error::InvalidData)?;
                    if m::io::hash(m::io::open(staged)?)? != blob::Blob::from(server) {
                        return Err(Error::InvalidData.into());
                    }
                    if current.is_some() {
                        apply.install_absent = true;
                        intent.media = Some(apply.clone());
                        self.with_media(a, |s| s.update_media_intent(&intent))?;
                        let before = current.as_ref().ok_or(Error::InvalidData)?;
                        let removed = self.media_io(collection, a, || {
                            media
                                .store()
                                .cloud_trash(
                                    &apply.old_name,
                                    &m::io::sha1_text(&before.sha1),
                                    &before.sha256,
                                    before.size,
                                )
                                .map_err(media_error)
                        })?;
                        if removed {
                            self.consume_media_feed(collection, media)?;
                            meguri_model::faultpoint!("cloud.media.after_trash").map_err(Error::from)?;
                            #[cfg(feature = "test-hooks")]
                            self.incoming_hooks
                                .hit(crate::cloud::testing::IncomingStage::MediaTrashBeforeInstall);
                        } else {
                            apply.abandoned = true;
                        }
                    }
                    if !apply.abandoned {
                        if m::io::hash(m::io::open(staged)?)? != blob::Blob::from(server) {
                            return Err(Error::InvalidData.into());
                        }
                        meguri_model::faultpoint!("cloud.media.during_install").map_err(Error::from)?;
                        match self.media_io(collection, a, || {
                            media
                                .store()
                                .cloud_install(text(&server.name)?, staged, None)
                                .map_err(media_error)
                        })? {
                            meguri_storage::media::CloudInstall::Installed => {
                                apply.installed = true;
                                media.media_arrived(vec![text(&server.name)?.into()])?;
                            }
                            meguri_storage::media::CloudInstall::Changed => apply.abandoned = true,
                            meguri_storage::media::CloudInstall::CaseCollision { .. } => {
                                return Err(Error::RecoveryRequired.into());
                            }
                        }
                    }
                } else {
                    apply.installed = if let Some(before) = current.as_ref() {
                        self.media_io(collection, a, || {
                            media
                                .store()
                                .cloud_trash(
                                    &apply.old_name,
                                    &m::io::sha1_text(&before.sha1),
                                    &before.sha256,
                                    before.size,
                                )
                                .map_err(media_error)
                        })?
                    } else {
                        true
                    };
                    apply.abandoned = !apply.installed;
                }
            }
        }
        intent.media = Some(apply.clone());
        self.with_media(a, |s| s.update_media_intent(&intent))?;
        if apply.installed {
            meguri_model::faultpoint!("cloud.media.after_resolution_install").map_err(Error::from)?;
        }
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let receipt = {
            match collection.wait_wal_covered(snapshot.wal_identity(), snapshot.seq(), Duration::from_secs(30))? {
                WalCoverage::Covered(r) if r.identity() == snapshot.wal_identity() && r.through() >= snapshot.seq() => {
                    r
                }
                _ => return Err(RuntimeError::StaleIncarnation),
            }
        };
        self.settle_media_apply(collection, media, a, &intent, moved_ran, &snapshot)?;
        self.state().incoming.receipts.push(receipt);
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::IntentSettlementComplete);
        Ok(())
    }
    fn settle_media_apply(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &Authority,
        intent: &ApplyIntent,
        moved_ran: bool,
        snapshot: &meguri_engine::Snapshot<'_>,
    ) -> Result<(), RuntimeError> {
        let apply = intent.media.as_ref().ok_or(Error::InvalidData)?;
        let received = codec::receive(
            &self.media_record_key(a)?,
            &apply.incoming.name,
            &apply.incoming.envelope,
            codec::AssetInput::Absent,
        )
        .map_err(|_| Error::InvalidData)?;
        let mut ledger = self.with_media(a, |s| s.media_ledger(a.binding))?;
        let mut updates = Vec::new();
        let existing = self.with_media(a, |s| s.entity_facts(a.binding, &apply.incoming.name))?;
        if apply.installed || apply.local_wins {
            if let Some(server) = &apply.server {
                let payload = codec::Payload::MediaName(server.clone());
                let mut entity = existing.clone().unwrap_or_else(|| {
                    fresh_media_entity(
                        &apply.incoming.name,
                        received.entity.fields.key.clone(),
                        received.entity.fields.origin,
                        payload.clone(),
                        a,
                    )
                });
                entity.origin = received.entity.fields.origin;
                entity.server_erev = received.entity.fields.erev;
                entity.acked_system_fields = apply.incoming.system_fields.clone();
                entity.acked_envelope = OpaqueBytes(apply.incoming.envelope.clone());
                entity.acked_payload = Some(payload.clone());
                entity.acked_payload_digest = received.entity.payload_sha256;
                entity.acked_row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
                entity.payload = if apply.local_wins {
                    if apply.delete_wins {
                        payload
                    } else {
                        codec::Payload::MediaName(apply.before.clone().ok_or(Error::InvalidData)?)
                    }
                } else {
                    payload
                };
                entity.mark = None;
                entity.base_payload = None;
                if !apply.local_wins {
                    entity.acked_rev = entity.local_rev;
                }
                entity.in_flight.retain(|f| f.erev > entity.server_erev);
                updates.push((existing.clone(), entity));
                let f = ledger.names.entry(text(&server.name)?.to_lowercase()).or_default();
                f.file_missing = false;
                f.superseded = None;
                f.withheld = None;
                if !apply.delete_wins {
                    f.tombstone_at = None;
                }
                f.local = if apply.local_wins {
                    apply.before.clone()
                } else {
                    Some(server.clone())
                };
            } else {
                let payload = received.payload.clone().ok_or(Error::InvalidData)?;
                let mut e = existing.clone().unwrap_or_else(|| {
                    fresh_media_entity(
                        &apply.incoming.name,
                        received.entity.fields.key.clone(),
                        received.entity.fields.origin,
                        payload.clone(),
                        a,
                    )
                });
                e.origin = received.entity.fields.origin;
                e.server_erev = received.entity.fields.erev;
                e.acked_system_fields = apply.incoming.system_fields.clone();
                e.acked_envelope = OpaqueBytes(apply.incoming.envelope.clone());
                e.payload = payload.clone();
                e.acked_payload = Some(payload.clone());
                e.acked_row_digest = payload.row_digest().map_err(|_| Error::InvalidData)?;
                e.acked_payload_digest = received.entity.payload_sha256;
                e.mark = None;
                e.base_payload = None;
                e.acked_rev = e.local_rev;
                e.in_flight.clear();
                updates.push((existing.clone(), e));
                let f = ledger
                    .names
                    .entry(text(&received.entity.fields.key)?.into())
                    .or_default();
                f.local = None;
                f.file_missing = false;
                f.superseded = None;
                f.tombstone_at = Some(received.entity.fields.deleted_at_ms);
            }
        }
        if moved_ran {
            let moved = apply.moved.as_ref().ok_or(Error::InvalidData)?;
            let payload = codec::Payload::MediaName(moved.clone());
            let key = crate::cloud::row_payload::payload_key(&payload)?;
            let name = codec::entity_name(&self.media_record_key(a)?, codec::Kind::MediaName, &key)
                .map_err(|_| Error::InvalidData)?;
            let old = self.with_media(a, |s| s.entity_facts(a.binding, &name))?;
            let mut e = old
                .clone()
                .unwrap_or_else(|| fresh_media_entity(&name, key, [0; 16], payload.clone(), a));
            if e.origin == [0; 16] {
                e.origin = self.publication_random_id()?;
            }
            e.payload = payload;
            e.local_rev = e.local_rev.checked_add(1).ok_or(Error::InvalidData)?;
            updates.push((old, e));
            let key = self.publication_media_state()?.ok_or(Error::InvalidData)?.0;
            let metadata = blob::Blob::from(moved);
            ledger.blobs.entry(metadata.name(&key.0)).or_insert_with(|| BlobState {
                metadata,
                ..Default::default()
            });
            let f = ledger.names.entry(text(&moved.name)?.to_lowercase()).or_default();
            f.withheld = None;
            f.superseded = None;
            f.file_missing = false;
            f.local = Some(moved.clone());
        }
        if moved_ran {
            for key in &intent.keys {
                let id = i64::from_be_bytes(key.key.as_slice().try_into().map_err(|_| Error::InvalidData)?);
                let Some(row) = snapshot.row_image(Table::Notes, &RowKey::Id(id))? else {
                    continue;
                };
                let mut e = self
                    .with_media(a, |s| s.entity_facts(a.binding, &key.name))?
                    .ok_or(Error::InvalidData)?;
                if e.observer_open == a.open && e.observed_seq > snapshot.seq().0 {
                    return Err(Error::RecoveryRequired.into());
                }
                let expected = e.clone();
                let codec::Payload::Note(n) = &e.payload else {
                    return Err(Error::InvalidData.into());
                };
                e.payload = codec::Payload::Note(note_image(&row, n.nt_struct_gen)?);
                e.local_rev = e.local_rev.checked_add(1).ok_or(Error::InvalidData)?;
                updates.push((Some(expected), e));
            }
        }
        if apply.local_wins && !apply.abandoned {
            let server = apply.server.as_ref().ok_or(Error::InvalidData)?;
            let path = apply.staged.as_ref().ok_or(Error::InvalidData)?;
            let (owned, found) = self.media_io(collection, a, || {
                Ok(m::io::stage(m::io::open(path)?, &self.cloud_staging())?)
            })?;
            if found != blob::Blob::from(server) {
                return Err(Error::InvalidData.into());
            }
            let winner = codec::Payload::MediaName(if apply.delete_wins {
                server.clone()
            } else {
                apply.before.clone().ok_or(Error::InvalidData)?
            });
            let copy = ConflictCopy {
                id: CopyId(self.publication_random_id()?),
                kind: ConflictKind::MediaName,
                key: OpaqueBytes(received.entity.fields.key.clone()),
                reason: if apply.delete_wins {
                    ConflictReason::EditVsDelete
                } else {
                    ConflictReason::MediaContent
                },
                losing_envelope: OpaqueBytes(apply.incoming.envelope.clone()),
                losing_payload_digest: PayloadDigest(received.entity.payload_sha256),
                winning_payload_digest: PayloadDigest(winner.payload_digest().map_err(|_| Error::InvalidData)?),
                field_names: None,
                template_names: None,
                note_deck: None,
                staged_path: Some(owned.to_string_lossy().into_owned()),
                created_at_ms: self.media_now()?,
                resolved_at_ms: None,
            };
            self.with_media(a, |s| s.store_conflict(a.binding, &copy))?;
        }
        if let Some(losing) = &apply.before {
            if apply.server.as_ref().is_none_or(|s| s.sha256 != losing.sha256) && (apply.installed || moved_ran) {
                let staged_path = if let Some(moved) = &apply.moved {
                    Some(
                        media
                            .store()
                            .media_dir()
                            .join(text(&moved.name)?)
                            .to_string_lossy()
                            .into_owned(),
                    )
                } else {
                    media
                        .store()
                        .trash_entries()
                        .map_err(media_error)?
                        .into_iter()
                        .filter(|t| t.sha1 == m::io::sha1_text(&losing.sha1))
                        .last()
                        .map(|t| {
                            media
                                .store()
                                .cloud_trash_path(t.id)
                                .map(|p| p.to_string_lossy().into_owned())
                                .unwrap_or_default()
                        })
                };
                let payload = codec::Payload::MediaName(losing.clone());
                let bytes = payload.encode().map_err(|_| Error::InvalidData)?;
                let envelope = codec::Entity::build(
                    codec::EntityFields {
                        kind: codec::Kind::MediaName,
                        key: received.entity.fields.key.clone(),
                        origin: existing.as_ref().map_or([0; 16], |e| e.origin),
                        erev: existing.as_ref().map_or(1, |e| e.server_erev.max(1)),
                        generation: None,
                        tombstone: false,
                        deleted_at_ms: 0,
                        writer: a.binding.0,
                        base_sha256: None,
                    },
                    &bytes,
                )
                .map_err(|_| Error::InvalidData)?
                .encode()
                .map_err(|_| Error::InvalidData)?;
                let copy = ConflictCopy {
                    id: CopyId(self.publication_random_id()?),
                    kind: ConflictKind::MediaName,
                    key: OpaqueBytes(received.entity.fields.key.clone()),
                    reason: ConflictReason::MediaContent,
                    losing_envelope: OpaqueBytes(envelope),
                    losing_payload_digest: PayloadDigest(codec::sha256(&bytes)),
                    winning_payload_digest: PayloadDigest(received.entity.payload_sha256),
                    field_names: None,
                    template_names: None,
                    note_deck: None,
                    staged_path,
                    created_at_ms: self.media_now()?,
                    resolved_at_ms: None,
                };
                self.with_media(a, |s| s.store_conflict(a.binding, &copy))?;
            }
        }
        #[cfg(feature = "test-hooks")]
        self.incoming_hooks
            .hit(crate::cloud::testing::IncomingStage::MediaSettlementBeforeCommit);
        self.consume_media_feed_inner(
            collection,
            media,
            Some((
                a.clone(),
                intent.clone(),
                ledger,
                updates,
                apply.installed || apply.local_wins,
            )),
            None,
        )?;
        if apply.abandoned {
            self.consume_media_feed(collection, media)?;
        }
        Ok(())
    }
    pub(super) fn replay_media_intents(
        &self,
        collection: &Collection,
        media: &MediaService,
    ) -> Result<usize, RuntimeError> {
        let a = self.media_authority(collection, true)?;
        let intents = self.with_media(&a, |s| s.apply_intents(a.binding))?;
        let mut count = 0;
        for intent in intents.into_iter().filter(|i| i.media.is_some()) {
            self.execute_media_apply(collection, media, &a, intent)?;
            count += 1;
        }
        Ok(count)
    }
}
fn fresh_media_entity(
    name: &str,
    key: Vec<u8>,
    origin: [u8; 16],
    payload: codec::Payload,
    a: &Authority,
) -> EntityFacts {
    EntityFacts {
        name: name.into(),
        kind: codec::Kind::MediaName,
        key,
        origin,
        local_rev: 1,
        acked_rev: 0,
        payload,
        base_payload: None,
        mark: None,
        server_erev: 0,
        acked_system_fields: Vec::new(),
        acked_row_digest: [0; 32],
        acked_payload_digest: [0; 32],
        acked_payload: None,
        acked_envelope: OpaqueBytes::default(),
        in_flight: Vec::new(),
        observer_open: a.open,
        observed_seq: 0,
        observed_generation: a.generation,
    }
}
fn rewrite_note(
    mut note: codec::Note,
    old: &str,
    new: &str,
    now: i64,
    sort: usize,
) -> Result<codec::Note, RuntimeError> {
    let fields = text(&note.flds)?;
    let mut spans = Vec::new();
    for (start, matched, raw) in meguri_text::media_refs::media_tags(fields)
        .map(|t| (t.start, t.matched, t.raw_name))
        .chain(meguri_text::media_refs::sound_tags(fields).map(|t| (t.start, t.matched, t.raw_name)))
    {
        if !meguri_text::media_refs::is_remote(raw) && meguri_text::entities::decode_text(raw) == old {
            let offset = raw.as_ptr() as usize - matched.as_ptr() as usize;
            spans.push((start + offset, raw.len()));
        }
    }
    let mut rewritten = fields.to_owned();
    spans.sort_unstable();
    for (start, len) in spans.into_iter().rev() {
        rewritten.replace_range(start..start + len, new);
    }
    let fields: Vec<String> = rewritten.split('\u{1f}').map(str::to_owned).collect();
    note.sfld = codec::Stored::Text(
        meguri_text::checksum::sort_text_of(&fields, sort)
            .into_owned()
            .into_bytes(),
    );
    note.csum = i64::from(meguri_text::checksum::note_checksum(
        fields.first().map_or("", String::as_str),
    ));
    note.flds = rewritten.into_bytes();
    note.modified = now / 1000;
    Ok(note)
}
impl Runtime {
    pub fn validate_cloud_media_work(&self, collection: &Collection, work: &Work) -> Result<(), RuntimeError> {
        if self.publication_account()? != work.authority.account
            || self.media_authority(collection, true)? != work.authority
        {
            return Err(RuntimeError::IdentityUnverified);
        }
        let s = self.state();
        Self::check_media_locked(&s, &work.authority)?;
        if s.media.pending.as_ref() != Some(work) || s.media.paused {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
}
impl Runtime {
    pub(super) fn drain_media_metadata(
        &self,
        collection: &Collection,
        media: &MediaService,
    ) -> Result<usize, RuntimeError> {
        self.consume_media_feed(collection, media)?;
        let a = self.media_authority(collection, true)?;
        let entries = self.with_media(&a, |s| s.inbox(a.binding))?;
        let mut count = 0;
        for entry in entries {
            if entry.entry.reason != InboxReason::Ready {
                continue;
            }
            let InboxValue::Record {
                envelope,
                system_fields,
                ..
            } = &entry.entry.value
            else {
                continue;
            };
            let r = codec::receive(
                &self.media_record_key(&a)?,
                &entry.entry.record_name,
                &envelope.0,
                codec::AssetInput::Absent,
            )
            .map_err(|_| Error::InvalidData)?;
            if r.entity.fields.kind != codec::Kind::MediaName {
                continue;
            }
            let saved = workflow::Saved {
                name: entry.entry.record_name,
                envelope: envelope.0.clone(),
                system_fields: system_fields.0.clone(),
            };
            if r.entity.fields.tombstone {
                let ledger = self.with_media(&a, |s| s.media_ledger(a.binding))?;
                if ledger
                    .names
                    .get(text(&r.entity.fields.key)?)
                    .is_some_and(|n| n.withheld.is_some())
                {
                    continue;
                }
                self.begin_media_apply(collection, media, &a, saved, Some(entry.id), None, None)?;
                count += 1;
            } else if let Some(codec::Payload::MediaName(server)) = r.payload {
                if Self::current_media(media, text(&server.name)?)?.as_ref() == Some(&server) {
                    self.begin_media_apply(collection, media, &a, saved, Some(entry.id), Some(server), None)?;
                    count += 1;
                }
            }
        }
        Ok(count)
    }
}
impl meguri_engine::media::MediaFeedConsumer for Runtime {
    fn committed(&self, collection: &Collection, media: &MediaService, generation: Generation) {
        let Ok(issued) = self.media_authority(collection, false) else {
            return;
        };
        if issued.generation != generation.0 || media.check_generation(generation.0).is_err() {
            return;
        }
        if self
            .consume_media_feed_inner(collection, media, None, Some(&issued))
            .is_err()
        {
            if media.check_generation(generation.0).is_err() {
                return;
            }
            let mut state = self.state();
            // A delayed error belongs only to the opening that issued this notification.
            if state.incarnation == issued.incarnation
                && collection.current_generation() == generation
                && state.bound.as_ref().is_some_and(|b| {
                    b.usable
                        && b.binding.id == issued.binding
                        && b.open_counter == issued.open
                        && b.evidence.generation == generation
                        && b.file == issued.file
                        && b.binding.facts.account == issued.account
                })
            {
                Self::invalidate_locked(&mut state);
            }
        }
    }
}
struct MediaRunning(Arc<Mutex<State>>, Arc<std::sync::Condvar>);
impl Drop for MediaRunning {
    fn drop(&mut self) {
        self.0.lock().unwrap_or_else(PoisonError::into_inner).incoming.running = false;
        self.1.notify_all();
    }
}
impl Runtime {
    pub fn cloud_media_work_with_space(
        &self,
        collection: &Collection,
        media: &MediaService,
        space: &dyn meguri_storage::space::SpaceProvider,
    ) -> Result<Option<Work>, RuntimeError> {
        let work = self.cloud_media_work(collection, media)?;
        if let Some(Work {
            authority,
            request: Request::Download { name },
            ..
        }) = &work
        {
            let ledger = self.with_media(authority, |s| s.media_ledger(authority.binding))?;
            let size = ledger.blobs.get(name).ok_or(Error::InvalidData)?.metadata.size;
            let staged: u64 = ledger
                .blobs
                .values()
                .filter(|b| b.download == Transfer::Staged)
                .map(|b| b.metadata.size)
                .sum();
            let available = space.available_bytes(media.store().media_dir())?;
            if staged.checked_add(size).is_none_or(|n| n > 256 * 1024 * 1024) || available < size + 512 * 1024 * 1024 {
                self.state().media.no_space = true;
                return Ok(None);
            }
            self.state().media.no_space = false;
        }
        Ok(work)
    }
}
