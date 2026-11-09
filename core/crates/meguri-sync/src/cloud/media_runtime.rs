use super::*;
use crate::cloud::policy::PolicyWork;
use crate::cloud::{
    incoming::{ApplyIntent, ApplyKey, ApplyKeyRole},
    media::{self as m, *},
    workflow::EntityFacts,
};
use meguri_engine::media::{MediaJobKind, MediaService};
use meguri_storage::media::{MediaAbsenceCause, MediaFeedId, MediaFileState};
use std::collections::{BTreeMap, BTreeSet};

#[path = "media_sdk.rs"]
mod sdk;

#[derive(Clone)]
struct MediaRecoveryAuthority(Authority);
impl std::ops::Deref for MediaRecoveryAuthority {
    type Target = Authority;
    fn deref(&self) -> &Authority {
        &self.0
    }
}

type MediaFeedApplyCommit = (
    MediaRecoveryAuthority,
    ApplyIntent,
    Ledger,
    Vec<(Option<EntityFacts>, EntityFacts)>,
    bool,
);

struct PendingMediaApply {
    incoming: workflow::Saved,
    inbox: Option<i64>,
    server: Option<codec::MediaName>,
    staged: Option<PathBuf>,
}

fn media_error(_: impl std::fmt::Display) -> RuntimeError {
    Error::Io(std::io::ErrorKind::Other).into()
}
fn text(bytes: &[u8]) -> Result<&str, RuntimeError> {
    std::str::from_utf8(bytes).map_err(|_| Error::InvalidData.into())
}
fn same(a: Option<&codec::MediaName>, b: Option<&codec::MediaName>) -> bool {
    a == b
}
fn same_media_content(a: &codec::MediaName, b: &codec::MediaName) -> bool {
    let (Ok(a_name), Ok(b_name)) = (text(&a.name), text(&b.name)) else {
        return false;
    };
    a_name.to_lowercase() == b_name.to_lowercase() && a.sha256 == b.sha256 && a.sha1 == b.sha1 && a.size == b.size
}
impl Runtime {
    #[cfg(feature = "test-hooks")]
    pub fn cleanup_observation_for_testing(
        &self,
        sdk: &str,
    ) -> Result<crate::cloud::testing::CleanupObservation, RuntimeError> {
        use crate::cloud::testing::{CleanupBlobObservation, CleanupObservation};
        let s = self.state();
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        let ledger = with_tracking_access(|| s.store.as_ref().ok_or(Error::Database)?.media_ledger(binding))?;
        let drain = s.publication.cleanup_drain.as_ref();
        Ok(CleanupObservation {
            sdk_current: Self::sdk_locked(&s, sdk).is_ok(),
            drain_present: drain.is_some(),
            drain_sdk_matches: drain.is_some_and(|(_, owner, _)| owner == sdk),
            drain_guard_matches: drain.is_some_and(|(guard, _, _)| *guard == Self::adapter_guard_locked(&s)),
            drain_at_ms: drain.map(|(_, _, at)| *at),
            cached_now_ms: self.pending_clock.load(std::sync::atomic::Ordering::Relaxed),
            media_observed_now_ms: s.media.observed_now_ms,
            last_cleanup_ms: s.publication.last_cleanup_ms,
            root_confirmed: s.root_confirmed,
            gate_open: Self::gate_locked(&s).is_ok(),
            publication_sending: Self::publication_allows_send_locked(&s),
            media_enabled: s.media.enabled,
            media_paused: s.media.paused,
            media_root_checked: s.media.root_checked,
            ledger_cleanup_drain_ms: ledger.cleanup_drain_ms,
            zone_missing: ledger.zone_missing,
            blobs: ledger
                .blobs
                .values()
                .map(|blob| CleanupBlobObservation {
                    server: blob.server,
                    upload: blob.upload,
                    download: blob.download,
                    staged: blob.staged.is_some(),
                    unreferenced_since_ms: blob.unreferenced_since_ms,
                    deletion_due_ms: blob.deletion_due_ms,
                })
                .collect(),
        })
    }
    /// Read the persisted blob ledger without advancing media work or policy.
    #[cfg(feature = "test-hooks")]
    pub fn media_blobs_for_testing(&self) -> Result<BTreeMap<String, BlobState>, RuntimeError> {
        let s = self.state();
        let binding = s.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?.binding.id;
        with_tracking_access(|| Ok(s.store.as_ref().ok_or(Error::Database)?.media_ledger(binding)?.blobs))
    }
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
        self.media_service_with_work(collection, &mut PolicyWork::default())
    }
    pub(super) fn media_service_with_work(
        &self,
        collection: &Collection,
        work: &mut PolicyWork,
    ) -> Result<MediaService, RuntimeError> {
        let clock = self
            .adapter_clock
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
            .ok_or(Error::InvalidData)?;
        let result = MediaService::open_with_boundary(collection, clock, &mut || work.media_effect());
        match result {
            Err(_) if work.was_rejected() => Err(meguri_engine::EngineError::Cancelled.into()),
            result => Ok(result?),
        }
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
            || s.media.epoch != a.media_epoch && !Self::retained_upload_authority(s, a)
            || s.adapter.paused
            || s.media.paused && !Self::retained_upload_authority(s, a)
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
    fn check_media_recovery_locked(s: &State, a: &MediaRecoveryAuthority) -> Result<(), RuntimeError> {
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
            || s.tracking_stopped
            || s.adapter
                .sdk
                .as_ref()
                .is_some_and(|sdk| sdk.stopping && sdk.guard == Self::adapter_guard_locked(s))
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        Ok(())
    }
    fn with_media_recovery<T>(
        &self,
        a: &MediaRecoveryAuthority,
        work: impl FnOnce(&mut TrackingStore) -> Result<T, Error>,
    ) -> Result<T, RuntimeError> {
        let mut s = self.state();
        Self::check_media_recovery_locked(&s, a)?;
        with_tracking_access(|| work(s.store.as_mut().ok_or(Error::Database)?)).map_err(Into::into)
    }
    // Durable intent replay is local recovery even while device sync is disabled.
    // Selecting READY input and persisting a NEW intent still require admission.
    fn with_media_admission<T>(
        &self,
        a: &MediaRecoveryAuthority,
        work: impl FnOnce(&mut TrackingStore) -> Result<T, Error>,
    ) -> Result<T, RuntimeError> {
        let mut s = self.state();
        Self::check_media_recovery_locked(&s, a)?;
        if s.adapter.paused {
            return Err(RuntimeError::StaleIncarnation);
        }
        with_tracking_access(|| work(s.store.as_mut().ok_or(Error::Database)?)).map_err(Into::into)
    }
    fn media_recovery_io<T>(
        &self,
        collection: &Collection,
        a: &MediaRecoveryAuthority,
        work: impl FnOnce() -> Result<T, RuntimeError>,
    ) -> Result<T, RuntimeError> {
        if self.media_authority(collection, true)? != a.0 {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        Self::check_media_recovery_locked(&s, a)?;
        work()
    }
    fn put_media_state(&self, a: &Authority, ledger: &Ledger) -> Result<(), RuntimeError> {
        let _ = self.adapter_now();
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
        let _ = self.adapter_now();
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
        let mut current = self.media_authority(collection, true)?;
        if Self::retained_upload_authority(&self.state(), a) {
            current.media_epoch = a.media_epoch;
        }
        if current != *a {
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
        apply_commit: Option<MediaFeedApplyCommit>,
        notification: Option<&Authority>,
    ) -> Result<usize, RuntimeError> {
        self.consume_media_feed_inner_with_work(
            collection,
            media,
            apply_commit,
            notification,
            &mut PolicyWork::default(),
        )
    }
    pub(crate) fn consume_media_feed_with_work(
        &self,
        collection: &Collection,
        media: &MediaService,
        work: &mut PolicyWork,
    ) -> Result<usize, RuntimeError> {
        self.consume_media_feed_inner_with_work(collection, media, None, None, work)
    }
    fn consume_media_feed_inner_with_work(
        &self,
        collection: &Collection,
        media: &MediaService,
        apply_commit: Option<MediaFeedApplyCommit>,
        notification: Option<&Authority>,
        work: &mut PolicyWork,
    ) -> Result<usize, RuntimeError> {
        let a = self.media_authority(collection, false)?;
        if notification.is_some_and(|issued| issued != &a) {
            return Err(RuntimeError::StaleIncarnation);
        }
        media.check_generation(a.generation)?;
        media
            .store()
            .check_changes_with_boundary(&mut || work.media_effect())
            .map_err(|error| work.media_error(error))?;
        let (mut feed_state, intents) = {
            let state = self.state();
            with_tracking_access(|| -> Result<_, Error> {
                let store = state.store.as_ref().ok_or(Error::Database)?;
                Ok((store.media_ledger(a.binding)?, store.apply_intents(a.binding)?))
            })?
        };
        if let Some((authority, _, proposed_ledger, _, _)) = &apply_commit {
            if authority.0 != a || feed_state.revision != proposed_ledger.revision {
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
            // A case-only rename can register the new spelling before the
            // old spelling's absence. That retired spelling must not erase
            // the actual M4 destination already observed for this case key.
            if matches!(item.state, MediaFileState::Absent(_))
                && previous.withheld.is_none()
                && previous.superseded.is_none()
                && !previous.file_missing
                && !previous.removal_held
                && previous.tombstone_at.is_none()
                && let Some(physical) = &previous.local
                && physical.name != item.name.as_bytes()
                && Self::current_media(media, &item.name)?.as_ref() == Some(physical)
            {
                continue;
            }
            let mut flags = previous.clone();
            let source = match &item.state {
                MediaFileState::Present(f) => Some((f.sha1.clone(), f.size, f.mtime_secs, f.mtime_nanos)),
                _ => None,
            };
            let payload = match &item.state {
                MediaFileState::Present(f) if (1..=MAX_ASSET_BYTES).contains(&f.size) => {
                    if source == flags.source && flags.local.as_ref().is_some_and(|p| p.name == item.name.as_bytes()) {
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
            // K15.9 may retain a canonical ACK spelling different from the
            // physical M4 name. Only the exact facts already observed by that
            // settle are exempt; a later name, content or journal change goes
            // through the ordinary observer path.
            let retained_spelling = previous.local == payload
                && previous.source == flags.source
                && previous.withheld.is_none()
                && previous.superseded.is_none()
                && !previous.file_missing
                && !previous.removal_held
                && previous.tombstone_at.is_none()
                && tracked.as_ref().is_some_and(|e| {
                    e.server_erev > 0
                        && e.acked_rev > 0
                        && !e.acked_media_tombstone()
                        && e.mark.is_none()
                        && e.acked_payload.as_ref() == Some(&e.payload)
                })
                && ack.zip(payload.as_ref()).is_some_and(|(canonical, physical)| {
                    canonical.name != physical.name && same_media_content(canonical, physical)
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
                    if tracked.as_ref().is_some_and(|e| e.acked_media_tombstone()) {
                        flags.withheld = Some(p.sha256);
                        flags.superseded = None;
                    } else if let Some(acked) = ack {
                        if acked.sha256 != p.sha256 {
                            flags.superseded = Some(p.sha256);
                        }
                    } else {
                        flags.withheld = Some(p.sha256);
                    }
                }
                if flags.withheld.is_none() && flags.superseded.is_none() {
                    flags.tombstone_at = None;
                    if !retained_spelling {
                        file_changes.push((codec::Payload::MediaName(p.clone()), self.publication_random_id()?));
                    }
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
                        && (tracked.as_ref().is_some_and(|e| e.acked_media_tombstone())
                            || ack.is_none_or(|v| v.sha256 != p.sha256))
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
        let now_ms = self.media_now()?;
        let mut state = self.state();
        let bound = state.bound.as_ref().ok_or(RuntimeError::StaleIncarnation)?;
        if bound.binding.id != a.binding
            || bound.open_counter != a.open
            || state.incarnation != a.incarnation
            || bound.file != a.file
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        if let Some((authority, ..)) = &apply_commit {
            Self::check_media_recovery_locked(&state, authority)?;
        }
        state.media.observed_now_ms = now_ms;
        let (changed_work, retained_floor) = with_tracking_access(|| -> Result<_, Error> {
            let store = state.store.as_mut().ok_or(Error::Database)?;
            let changed_work = store.commit_media_feed(
                a.binding,
                &feed_state,
                &file_changes,
                (a.open, collection.last_seq().0, a.generation),
                now_ms,
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
        self.adapter_now()
    }
    pub fn resume_cloud_media(&self) {
        let mut s = self.state();
        s.media.paused = false;
        s.media.root_checked = false;
        s.media.recovered = false;
        s.media.attempted.clear();
        s.media.round_reads.clear();
        s.media.selection = Selection::Resume;
        s.media.due_loaded = false;
    }
    pub fn pause_cloud_media(&self) {
        let mut s = self.state();
        s.media.paused = true;
        s.media.epoch = s.media.epoch.saturating_add(1);
        s.media.pending = None;
        s.media.metadata_pending = None;
    }
    pub fn cloud_media_work(
        &self,
        collection: &Collection,
        media: &MediaService,
    ) -> Result<Option<Work>, RuntimeError> {
        self.cloud_media_work_with_work(collection, media, None, &mut PolicyWork::default())
    }
    fn cloud_media_work_with_work(
        &self,
        collection: &Collection,
        media: &MediaService,
        sdk: Option<&str>,
        work: &mut PolicyWork,
    ) -> Result<Option<Work>, RuntimeError> {
        self.consume_media_feed_with_work(collection, media, work)?;
        let a = self.media_authority(collection, true)?;
        {
            let mut s = self.state();
            if let Some(sdk) = sdk {
                Self::sdk_locked(&s, sdk)?;
            }
            if s.media.enabled == Some(false)
                || s.media.paused
                || s.adapter.paused
                || s.scan_running
                || s.tracking_stopped
                || !s.whole_complete
                || s.scan_required
            {
                return Ok(None);
            }
            // Selection clears stale work, recovers staging and reserves new
            // work. These are effects even before any transport is issued.
            work.effect()?;
            if s.media.pending.as_ref().is_some_and(|w| w.authority != a) {
                s.media.pending = None;
                s.media.recovered = false;
            }
            if s.media.metadata_pending.as_ref().is_some_and(|w| w.authority != a) {
                s.media.metadata_pending = None;
            }
            if let Some(w) = &s.media.metadata_pending {
                return Ok(Some(w.clone()));
            }
            if s.media.selection == Selection::Resume
                && s.media.root_checked
                && let Some(w) = &s.media.pending
            {
                let w = w.clone();
                drop(s);
                if !matches!(&w.request, Request::Download { name }
                    if self.media_download_blocked_by_unsyncable(&a, name)?)
                {
                    return Ok(Some(w));
                }
                let mut s = self.state();
                Self::check_media_locked(&s, &a)?;
                if s.media.pending.as_ref() != Some(&w) {
                    return Ok(None);
                }
                // Retire only this original work under its authority. Read and
                // revision-check the ledger in the same tracking write scope.
                let Request::Download { name } = &w.request else {
                    return Err(Error::InvalidData.into());
                };
                let changed = with_tracking_access(|| -> Result<_, Error> {
                    let store = s.store.as_mut().ok_or(Error::Database)?;
                    let mut ledger = store.media_ledger(a.binding)?;
                    let blob = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                    if blob.download == Transfer::Running {
                        blob.download = Transfer::None;
                    }
                    store.put_media(a.binding, &ledger)
                })?;
                s.media.pending = None;
                self.notify_media_pending_locked(&mut s, changed)?;
            }
        }
        let (mut ledger, key) = self.with_media(&a, |store| {
            store.materialize_media_rechecks(a.binding)?;
            Ok((store.media_ledger(a.binding)?, store.publication_media(a.binding)?))
        })?;
        if ledger.zone_missing {
            return Ok(None);
        }
        let key = key.and_then(|(key, acked, _)| acked.then_some(key));
        if self.state().media.selection == Selection::Resume && !self.state().media.recovered {
            for blob in ledger.blobs.values_mut() {
                if blob.download == Transfer::Running {
                    blob.download = Transfer::Queued;
                }
                if blob.upload == Transfer::Running {
                    blob.upload = Transfer::None;
                    // The ledger retains this file until a new original SDK
                    // adopts it or a real Saved callback releases it.
                }
            }
            self.put_media_state(&a, &ledger)?;
            ledger = self.with_media(&a, |s| s.media_ledger(a.binding))?;
            self.state().media.recovered = true;
        }
        if self.state().media.selection == Selection::Resume && self.state().media.root_checked {
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
        let load_due = {
            let s = self.state();
            s.media.root_checked && !s.media.due_loaded
        };
        if load_due {
            let due = Self::cloud_due_media_names(collection)?;
            let mut s = self.state();
            Self::check_media_locked(&s, &a)?;
            s.media.due = due;
            s.media.due_loaded = true;
        }
        let request = if !self.state().media.root_checked {
            Some(Request::Root)
        } else {
            let key = key.ok_or(Error::InvalidData)?;
            let mut live_blobs = BTreeSet::new();
            for flags in ledger.names.values() {
                if let Some(local) = &flags.local {
                    if flags.sendable() {
                        live_blobs.insert(blob::Blob::from(local).name(&key.0));
                    }
                    let metadata = blob::Blob::from(local);
                    ledger.blobs.entry(metadata.name(&key.0)).or_insert_with(|| BlobState {
                        metadata,
                        ..Default::default()
                    });
                }
            }
            let (visible, due) = {
                let s = self.state();
                (s.media.visible.clone(), s.media.due.clone())
            };
            let mut visible_blobs = BTreeSet::new();
            let mut due_content = BTreeSet::new();
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
                        if ledger
                            .names
                            .get(&text(&p.name)?.to_lowercase())
                            .is_some_and(|f| f.unsyncable)
                        {
                            continue;
                        }
                        let metadata = blob::Blob::from(&p);
                        let name = metadata.name(&key.0);
                        live_blobs.insert(name.clone());
                        if visible.contains(&text(&p.name)?.to_lowercase()) {
                            visible_blobs.insert(name.clone());
                        }
                        if due.contains(&text(&p.name)?.to_lowercase()) {
                            due_content.insert(name.clone());
                        }
                        ledger
                            .blobs
                            .entry(name)
                            .and_modify(|b| {
                                if matches!(b.download, Transfer::None | Transfer::Complete) {
                                    b.download = Transfer::Queued;
                                }
                            })
                            .or_insert_with(|| BlobState {
                                metadata,
                                download: Transfer::Queued,
                                ..Default::default()
                            });
                    }
                }
            }
            for e in self.with_media(&a, |store| Self::cloud_media_entities(store, a.binding))? {
                if e.acked_media_tombstone() {
                    continue;
                }
                if let codec::Payload::MediaName(p) = e.payload {
                    let flags = ledger.names.get(&text(&p.name)?.to_lowercase());
                    if flags.is_none_or(|f| !f.removal_held && f.tombstone_at.is_none() && f.withheld.is_none()) {
                        live_blobs.insert(blob::Blob::from(&p).name(&key.0));
                    }
                    if e.mark.is_none()
                        && flags.is_none_or(|f| !f.unsyncable && f.local.as_ref().is_none_or(|l| l.sha256 != p.sha256))
                    {
                        let metadata = blob::Blob::from(&p);
                        let name = metadata.name(&key.0);
                        live_blobs.insert(name.clone());
                        if visible.contains(&text(&p.name)?.to_lowercase()) {
                            visible_blobs.insert(name.clone());
                        }
                        if due.contains(&text(&p.name)?.to_lowercase()) {
                            due_content.insert(name.clone());
                        }
                        ledger
                            .blobs
                            .entry(name)
                            .and_modify(|b| {
                                if matches!(b.download, Transfer::None | Transfer::Complete) {
                                    b.download = Transfer::Queued;
                                }
                            })
                            .or_insert_with(|| BlobState {
                                metadata,
                                download: Transfer::Queued,
                                ..Default::default()
                            });
                    }
                }
            }
            let (attempted, round_reads, selection, visible) = {
                let s = self.state();
                (
                    s.media.attempted.clone(),
                    s.media.round_reads.clone(),
                    s.media.selection,
                    s.media.visible.clone(),
                )
            };
            visible_blobs.extend(
                ledger
                    .names
                    .iter()
                    .filter(|(name, _)| visible.contains(*name))
                    .filter_map(|(_, f)| f.local.as_ref().map(|p| blob::Blob::from(p).name(&key.0))),
            );
            let mut ordered: Vec<_> = ledger
                .blobs
                .keys()
                .filter(|name| live_blobs.contains(*name))
                .cloned()
                .collect();
            for (filename, state) in &ledger.names {
                if due.contains(filename)
                    && let Some(payload) = &state.local
                {
                    due_content.insert(blob::Blob::from(payload).name(&key.0));
                }
            }
            ordered.sort_by_key(|name| (!visible_blobs.contains(name), !due_content.contains(name), name.clone()));
            let include_missing = !matches!(selection, Selection::Read { include_missing: false });
            let reads: Vec<_> = ordered
                .iter()
                .filter(|name| {
                    let b = &ledger.blobs[*name];
                    !round_reads.contains(*name)
                        && (b.server == ServerState::Unknown
                            || include_missing && b.server == ServerState::Deleted && b.download == Transfer::Queued)
                })
                .take(if self.state().media.batch_reads { 200 } else { 1 })
                .cloned()
                .collect();
            let mut request = if reads.is_empty() {
                None
            } else if self.state().media.batch_reads {
                Some(Request::Reads { names: reads })
            } else {
                Some(Request::Read { name: reads[0].clone() })
            };
            if request.is_none() && selection == Selection::Resume {
                let original = self.state().actions.media_upload.as_ref().map(|(_, work)| work.clone());
                if let Some(original) = original.filter(|work| {
                    let s = self.state();
                    !s.media.suppress_uploads
                        && !matches!(&work.request, Request::Upload { name, .. } if s.policy.parked.contains(name))
                }) {
                    self.validate_cloud_media_work(collection, &original)?;
                    self.put_media_state(&a, &ledger)?;
                    return Ok(Some(original));
                }
            }
            for name in &ordered {
                if selection != Selection::Resume
                    || matches!(request, Some(Request::Read { .. } | Request::Reads { .. }))
                {
                    break;
                }
                let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                if attempted.contains(name) {
                    continue;
                }
                if selection == Selection::Resume
                    && b.server == ServerState::Present
                    && b.download == Transfer::Queued
                    && !self.media_download_blocked_by_unsyncable(&a, name)?
                    && !self.cloud_media_downloads_blocked()
                {
                    if request.is_none() {
                        request = Some(Request::Download { name: name.clone() });
                    }
                    continue;
                }
                if b.server == ServerState::Deleted
                    && b.upload != Transfer::Complete
                    && !self.state().media.suppress_uploads
                {
                    let candidate = ledger
                        .names
                        .values()
                        .find(|f| f.sendable() && f.local.as_ref().is_some_and(|p| p.sha256 == b.metadata.sha256))
                        .and_then(|f| f.local.clone());
                    if let Some(local) = candidate {
                        let (staged, actual) = if let Some(path) = b.staged.as_ref().filter(|p| {
                            m::io::open(p)
                                .and_then(m::io::hash)
                                .is_ok_and(|found| found == b.metadata)
                        }) {
                            (path.clone(), b.metadata.clone())
                        } else {
                            let file = media
                                .store()
                                .open_file(text(&local.name)?)
                                .map_err(media_error)?
                                .ok_or(Error::InvalidData)?;
                            self.media_io(collection, &a, || Ok(m::io::stage(file, &self.cloud_staging())?))?
                        };
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
            if request.is_none() && selection == Selection::Resume {
                let now = self.media_now()?;
                let due = self.with_media(&a, |store| store.media_deletions(a.binding, now, true))?;
                let s = self.state();
                if let Some(name) = due.into_iter().find(|name| !s.policy.parked.contains(name)) {
                    request = Some(Request::Delete { name });
                }
            }
            if let Some(Request::Download { name }) = &request {
                ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?.download = Transfer::Running;
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
        if matches!(
            work.request,
            Request::Root | Request::Read { .. } | Request::Reads { .. }
        ) {
            s.media.metadata_pending = Some(work.clone());
        } else {
            s.media.pending = Some(work.clone());
        }
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
    fn media_recovery_record_key(&self, a: &MediaRecoveryAuthority) -> Result<[u8; 32], RuntimeError> {
        self.with_media_recovery(a, |store| {
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
        #[cfg(feature = "test-hooks")]
        let request_label = match &work.request {
            Request::Delete { .. } => "Delete",
            Request::Root => "Root",
            Request::Read { .. } => "Read",
            Request::Reads { .. } => "Reads",
            Request::Upload { .. } => "Upload",
            Request::Download { .. } => "Download",
        };
        #[cfg(feature = "test-hooks")]
        let reply_label = match &reply {
            Reply::BlobReads { .. } => "BlobReads",
            Reply::Record { .. } => "Record",
            Reply::Saved { .. } => "Saved",
            Reply::UnknownItem => "UnknownItem",
            Reply::ZoneMissing => "ZoneMissing",
            Reply::Failed { .. } => "Failed",
        };
        #[cfg(feature = "test-hooks")]
        let mut stage = "authority_validation";
        let result = (|| {
            let a = &work.authority;
            #[cfg(feature = "test-hooks")]
            if matches!(work.request, Request::Download { .. }) {
                self.incoming_hooks
                    .hit(crate::cloud::testing::IncomingStage::MediaReplyBeforeApply);
            }
            self.validate_cloud_media_work(collection, work)?;
            #[cfg(feature = "test-hooks")]
            {
                stage = "account_validation";
            }
            if self.publication_account()? != a.account {
                return Err(RuntimeError::IdentityUnverified);
            }
            #[cfg(feature = "test-hooks")]
            {
                stage = "authority_snapshot";
            }
            let mut current_authority = self.media_authority(collection, true)?;
            if matches!(work.request, Request::Upload { .. }) {
                current_authority.media_epoch = a.media_epoch;
            }
            if current_authority != *a {
                return Err(RuntimeError::StaleIncarnation);
            }
            #[cfg(feature = "test-hooks")]
            {
                stage = "pending_validation";
            }
            {
                let s = self.state();
                if s.media.pending.as_ref() != Some(work)
                    && s.media.metadata_pending.as_ref() != Some(work)
                    && !s
                        .actions
                        .media_upload
                        .as_ref()
                        .is_some_and(|(_, original)| original == work)
                {
                    return Err(RuntimeError::StaleIncarnation);
                }
            }
            #[cfg(feature = "test-hooks")]
            {
                stage = "ledger_read";
            }
            let (mut ledger, key) = self.with_media(a, |store| {
                Ok((store.media_ledger(a.binding)?, store.publication_media(a.binding)?))
            })?;
            let failed = matches!(&reply, Reply::Failed { .. })
                || matches!(&reply, Reply::BlobReads { results } if results.iter().any(|r| matches!(r.result, BlobReadResult::Failed { .. })));
            let mut staged_download = None;
            #[cfg(feature = "test-hooks")]
            {
                stage = "reply_shape";
            }
            match (&work.request, reply) {
                (Request::Reads { names }, Reply::BlobReads { results }) => {
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_reads_shape";
                    }
                    let expected: BTreeSet<_> = names.iter().cloned().collect();
                    if names.is_empty()
                        || names.len() > 200
                        || expected.len() != names.len()
                        || results.len() != names.len()
                        || results.iter().map(|r| r.name.clone()).collect::<BTreeSet<_>>() != expected
                    {
                        return Err(Error::InvalidData.into());
                    }
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_reads_key";
                    }
                    let key = key.as_ref().ok_or(Error::InvalidData)?.0.0;
                    for read in results {
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "blob_reads_ledger";
                        }
                        let b = ledger.blobs.get_mut(&read.name).ok_or(Error::InvalidData)?;
                        match read.result {
                            BlobReadResult::Present { envelope } => {
                                #[cfg(feature = "test-hooks")]
                                {
                                    stage = "blob_reads_decode";
                                }
                                let metadata = blob::Blob::decode(&envelope).map_err(|_| Error::InvalidData)?;
                                #[cfg(feature = "test-hooks")]
                                {
                                    stage = "blob_reads_validation";
                                }
                                if metadata != b.metadata || metadata.name(&key) != read.name {
                                    return Err(Error::InvalidData.into());
                                }
                                b.server = ServerState::Present;
                            }
                            BlobReadResult::UnknownItem => {
                                b.server = ServerState::Deleted;
                                if !matches!(b.upload, Transfer::Running | Transfer::Queued) {
                                    b.upload = Transfer::None;
                                }
                            }
                            BlobReadResult::ZoneMissing => ledger.zone_missing = true,
                            BlobReadResult::Failed { .. } => {}
                        }
                    }
                }
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
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "root_decode";
                    }
                    let crate::cloud::control_codec::DecodeOutcome::Supported(root) =
                        crate::cloud::control_codec::MediaRoot::decode(&envelope).map_err(|_| Error::InvalidData)?
                    else {
                        return Err(Error::InvalidData.into());
                    };
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "root_key_validation";
                    }
                    if key
                        .as_ref()
                        .is_some_and(|(key, acked, _)| *acked && root.media_key != key.0)
                    {
                        ledger.zone_missing = true;
                    } else if key.as_ref().is_none_or(|(_, acked, _)| !acked) {
                        let crt = collection.reader(ReaderRole::Cloud)?.col()?.created.0;
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "root_commit";
                        }
                        self.with_media(a, |store| store.accept_media_root(a.binding, root.media_key, crt))?;
                        ledger = self.with_media(a, |store| store.media_ledger(a.binding))?;
                    }
                }
                (Request::Read { name } | Request::Download { name }, Reply::UnknownItem) => {
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_ledger";
                    }
                    let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                    b.server = ServerState::Deleted;
                    if !matches!(b.upload, Transfer::Running | Transfer::Queued) {
                        b.upload = Transfer::None;
                    }
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
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_record_shape";
                    }
                    if name != &actual || record_type != "MGBlob" {
                        return Err(Error::InvalidData.into());
                    }
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_decode";
                    }
                    let metadata = blob::Blob::decode(&envelope).map_err(|_| Error::InvalidData)?;
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_ledger";
                    }
                    let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_validation";
                    }
                    if metadata != b.metadata || metadata.name(&key.as_ref().ok_or(Error::InvalidData)?.0.0) != *name {
                        return Err(Error::InvalidData.into());
                    }
                    b.server = ServerState::Present;
                    if matches!(work.request, Request::Download { .. }) {
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "download_asset";
                        }
                        let asset = asset.ok_or(Error::InvalidData)?;
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "download_staging";
                        }
                        let (staged, found) = self.media_io(collection, a, || {
                            Ok(m::io::stage(m::io::open(&asset)?, &self.cloud_staging())?)
                        })?;
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "download_validation";
                        }
                        if metadata != found {
                            std::fs::remove_file(staged).map_err(Error::from)?;
                            return Err(Error::InvalidData.into());
                        }
                        b.download = Transfer::Staged;
                        b.staged = Some(staged.clone());
                        staged_download = Some((name.clone(), staged));
                    } else if asset.is_some() {
                        #[cfg(feature = "test-hooks")]
                        {
                            stage = "read_asset_validation";
                        }
                        return Err(Error::InvalidData.into());
                    }
                }
                (Request::Upload { name, .. }, Reply::Saved { name: actual, envelope }) => {
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_decode";
                    }
                    let metadata = blob::Blob::decode(&envelope).map_err(|_| Error::InvalidData)?;
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "blob_ledger";
                    }
                    let b = ledger.blobs.get_mut(name).ok_or(Error::InvalidData)?;
                    #[cfg(feature = "test-hooks")]
                    {
                        stage = "upload_validation";
                    }
                    if *name != actual
                        || metadata != b.metadata
                        || metadata.name(&key.as_ref().ok_or(Error::InvalidData)?.0.0) != *name
                    {
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
            #[cfg(feature = "test-hooks")]
            {
                stage = "ledger_commit";
            }
            self.put_media_state(a, &ledger)?;
            {
                let mut s = self.state();
                #[cfg(feature = "test-hooks")]
                {
                    stage = "pending_commit";
                }
                Self::check_media_locked(&s, a)?;
                if s.media.pending.as_ref() == Some(work) {
                    s.media.pending = None;
                }
                if s.media.metadata_pending.as_ref() == Some(work) {
                    s.media.metadata_pending = None;
                }
                if let Request::Read { name } = &work.request {
                    s.media.round_reads.insert(name.clone());
                }
                if let Request::Reads { names } = &work.request {
                    s.media.round_reads.extend(names.iter().cloned());
                }
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
                    _ => {}
                }
            }
            if failed {
                #[cfg(feature = "test-hooks")]
                {
                    stage = "request_failed";
                }
                return Err(Error::Io(std::io::ErrorKind::Other).into());
            }
            if let Some((name, _)) = staged_download {
                #[cfg(feature = "test-hooks")]
                {
                    stage = "download_apply";
                }
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
                #[cfg(feature = "test-hooks")]
                {
                    stage = "apply_commit";
                }
                self.finish_media_pending(a)?;
            }
            Ok(())
        })();
        #[cfg(feature = "test-hooks")]
        if result.is_err() {
            eprintln!("cloud-media reply_failed request={request_label} reply={reply_label} stage={stage}");
        }
        result
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
            if server.sha256 != b.metadata.sha256
                || ledger
                    .names
                    .get(&text(&server.name)?.to_lowercase())
                    .is_some_and(|f| f.unsyncable)
            {
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
                PendingMediaApply {
                    incoming: saved,
                    inbox: Some(entry.id),
                    server: Some(server),
                    staged: Some(staged.clone()),
                },
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
                    if let codec::Payload::MediaName(p) = e.acked_payload.as_ref().unwrap_or(&e.payload)
                        && p.sha256 == b.metadata.sha256
                        && !ledger
                            .names
                            .get(&text(&p.name)?.to_lowercase())
                            .is_some_and(|f| f.unsyncable)
                        && Self::current_media(media, text(&p.name)?)?.as_ref() != Some(p)
                    {
                        self.begin_media_apply(
                            collection,
                            media,
                            a,
                            PendingMediaApply {
                                incoming: workflow::Saved {
                                    name: e.name.clone(),
                                    envelope: e.acked_envelope.0.clone(),
                                    system_fields: e.acked_system_fields.clone(),
                                },
                                inbox: None,
                                server: Some(p.clone()),
                                staged: Some(staged.clone()),
                            },
                        )?;
                    }
                }
            }
        }
        let mut ledger = self.with_media(a, |s| s.media_ledger(a.binding))?;
        let retained = self.with_media(a, |store| {
            Ok(store.apply_intents(a.binding)?.iter().any(|intent| {
                intent
                    .media
                    .as_ref()
                    .is_some_and(|apply| apply.staged.as_ref() == Some(&staged))
            }) || store
                .conflict_copies()?
                .iter()
                .any(|copy| copy.copy.staged_path.as_deref() == staged.to_str()))
        })?;
        if let Some(b) = ledger.blobs.get_mut(blob_name) {
            b.download = Transfer::Complete;
            if !retained {
                b.staged = None;
            }
        }
        self.put_media_state(a, &ledger)?;
        if !retained {
            let _ = std::fs::remove_file(staged);
        }
        Ok(())
    }
    fn begin_media_apply(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &Authority,
        pending: PendingMediaApply,
    ) -> Result<(), RuntimeError> {
        self.with_media(a, |_| Ok(()))?;
        self.begin_local_media_apply(collection, media, &MediaRecoveryAuthority(a.clone()), pending)
    }
    fn begin_local_media_apply(
        &self,
        collection: &Collection,
        media: &MediaService,
        a: &MediaRecoveryAuthority,
        pending: PendingMediaApply,
    ) -> Result<(), RuntimeError> {
        self.media_recovery_io(collection, a, || Ok(()))?;
        self.with_media_admission(a, |_| Ok(()))?;
        let PendingMediaApply {
            incoming,
            inbox,
            server,
            staged,
        } = pending;
        let received = codec::receive(
            &self.media_recovery_record_key(a)?,
            &incoming.name,
            &incoming.envelope,
            codec::AssetInput::Absent,
        )
        .map_err(|_| Error::InvalidData)?;
        let existing = self.with_media_recovery(a, |s| s.entity_facts(a.binding, &incoming.name))?;
        let name = match server.as_ref() {
            Some(p) => text(&p.name)?.to_owned(),
            None => text(&received.entity.fields.key)?.to_owned(),
        };
        let before = Self::current_media(media, &name)?;
        let ledger = self.with_media_recovery(a, |s| s.media_ledger(a.binding))?;
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
            originating_open: Some(a.open),
            planned_at_ms: self.media_now()?,
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
        // Recheck after provider reads/planning, under the same lock as persistence.
        intent.id = self.with_media_admission(a, |s| s.persist_apply_intent(&intent, None))?;
        self.execute_media_apply(collection, media, a, intent)
    }
    fn plan_media_rewrite(
        &self,
        collection: &Collection,
        a: &MediaRecoveryAuthority,
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
                let name = codec::entity_name(
                    &self.media_recovery_record_key(a)?,
                    codec::Kind::Note,
                    &codec::id_key(id),
                )
                .map_err(|_| Error::InvalidData)?;
                let e = self
                    .with_media_recovery(a, |s| s.entity_facts(a.binding, &name))?
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
        a: &MediaRecoveryAuthority,
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
        planned.originating_open = Some(a.open);
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
                Self::check_media_recovery_locked(&s, &authority).map_err(engine_error)?;
                with_tracking_access(|| s.store.as_mut().ok_or(Error::Database)?.update_media_intent(&planned))
                    .map_err(engine_error)?;
                Ok(planned)
            },
        )?;
        *intent = result.value;
        intent.result_seq = Some(result.seq.0);
        self.with_media_recovery(a, |s| s.update_media_intent(intent))?;
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
        a: &MediaRecoveryAuthority,
        mut intent: ApplyIntent,
    ) -> Result<(), RuntimeError> {
        if intent.binding != a.binding {
            return Err(RuntimeError::StaleIncarnation);
        }
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
                        apply.move_evidence = self.media_recovery_io(collection, a, || {
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
                        self.with_media_recovery(a, |s| s.update_media_intent(&intent))?;
                    }
                    moved_ran = if let Some(proof) = &apply.move_evidence {
                        self.media_recovery_io(collection, a, || {
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
                self.with_media_recovery(a, |_| Ok(()))?;
                media.check_generation(a.generation)?;
                if let Some(server) = &apply.server {
                    let staged = apply.staged.as_ref().ok_or(Error::InvalidData)?;
                    if m::io::hash(m::io::open(staged)?)? != blob::Blob::from(server) {
                        return Err(Error::InvalidData.into());
                    }
                    if current.is_some() {
                        apply.install_absent = true;
                        intent.media = Some(apply.clone());
                        self.with_media_recovery(a, |s| s.update_media_intent(&intent))?;
                        let before = current.as_ref().ok_or(Error::InvalidData)?;
                        let removed = self.media_recovery_io(collection, a, || {
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
                        match self.media_recovery_io(collection, a, || {
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
                        self.media_recovery_io(collection, a, || {
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
        self.with_media_recovery(a, |s| s.update_media_intent(&intent))?;
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
        a: &MediaRecoveryAuthority,
        intent: &ApplyIntent,
        moved_ran: bool,
        snapshot: &meguri_engine::Snapshot<'_>,
    ) -> Result<(), RuntimeError> {
        let apply = intent.media.as_ref().ok_or(Error::InvalidData)?;
        let received = codec::receive(
            &self.with_media_recovery(a, |s| {
                Ok(s.binding(a.binding)?.ok_or(Error::BindingNotFound)?.facts.record_key.0)
            })?,
            &apply.incoming.name,
            &apply.incoming.envelope,
            codec::AssetInput::Absent,
        )
        .map_err(|_| Error::InvalidData)?;
        let mut ledger = self.with_media_recovery(a, |s| s.media_ledger(a.binding))?;
        let mut updates = Vec::new();
        let existing = self.with_media_recovery(a, |s| s.entity_facts(a.binding, &apply.incoming.name))?;
        if apply.installed || apply.local_wins {
            if let Some(server) = &apply.server {
                let payload = codec::Payload::MediaName(server.clone());
                let mut entity = existing.clone().unwrap_or_else(|| {
                    fresh_media_entity(
                        &apply.incoming.name,
                        received.entity.fields.key.clone(),
                        received.entity.fields.origin,
                        payload.clone(),
                        true,
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
                        true,
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
            let record_key = self.with_media_recovery(a, |s| {
                Ok(s.binding(a.binding)?.ok_or(Error::BindingNotFound)?.facts.record_key.0)
            })?;
            let name = codec::entity_name(&record_key, codec::Kind::MediaName, &key).map_err(|_| Error::InvalidData)?;
            let old = self.with_media_recovery(a, |s| s.entity_facts(a.binding, &name))?;
            let mut e = old
                .clone()
                .unwrap_or_else(|| fresh_media_entity(&name, key, [0; 16], payload.clone(), false, a));
            if e.origin == [0; 16] {
                e.origin = self.publication_random_id()?;
            }
            let stays_clean = e.server_erev > 0
                && e.acked_rev > 0
                && !e.acked_media_tombstone()
                && matches!(&e.acked_payload, Some(codec::Payload::MediaName(acked))
                    if same_media_content(acked, moved))
                && ledger
                    .names
                    .get(text(&moved.name)?.to_lowercase().as_str())
                    .is_none_or(|f| f.withheld.is_none() && f.tombstone_at.is_none() && !f.removal_held);
            if stays_clean {
                // The Service ACK remains canonical. Observing M4's physical
                // spelling is not another save or an acknowledgement.
                e.payload = e.acked_payload.clone().ok_or(Error::InvalidData)?;
                e.mark = None;
                e.base_payload = None;
            } else {
                e.payload = payload;
                e.local_rev = e.local_rev.checked_add(1).ok_or(Error::InvalidData)?;
            }
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
            if stays_clean {
                let sha1 = m::io::sha1_text(&moved.sha1);
                let observed = media
                    .store()
                    .sync_entry(text(&moved.name)?)
                    .map_err(media_error)?
                    .filter(|entry| {
                        entry.live && entry.size == moved.size && entry.sha1.as_deref() == Some(sha1.as_str())
                    })
                    .ok_or(Error::RecoveryRequired)?;
                f.source = Some((
                    observed.sha1.ok_or(Error::InvalidData)?,
                    observed.size,
                    observed.mtime_secs,
                    observed.mtime_nanos,
                ));
            }
        }
        if moved_ran {
            for key in &intent.keys {
                let id = i64::from_be_bytes(key.key.as_slice().try_into().map_err(|_| Error::InvalidData)?);
                let Some(row) = snapshot.row_image(Table::Notes, &RowKey::Id(id))? else {
                    continue;
                };
                let mut e = self
                    .with_media_recovery(a, |s| s.entity_facts(a.binding, &key.name))?
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
            let (owned, found) = self.media_recovery_io(collection, a, || {
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
            self.with_media_recovery(a, |s| s.store_conflict(a.binding, &copy))?;
        }
        if let Some(losing) = &apply.before
            && apply.server.as_ref().is_none_or(|s| s.sha256 != losing.sha256)
            && (apply.installed || moved_ran)
        {
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
                    .rfind(|t| t.sha1 == m::io::sha1_text(&losing.sha1))
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
            self.with_media_recovery(a, |s| s.store_conflict(a.binding, &copy))?;
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
        let a = MediaRecoveryAuthority(self.media_authority(collection, true)?);
        let intents = self.with_media_recovery(&a, |s| s.apply_intents(a.binding))?;
        if intents.iter().all(|intent| intent.media.is_none()) {
            return Ok(0);
        }
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
    sent: bool,
    a: &Authority,
) -> EntityFacts {
    EntityFacts {
        name: name.into(),
        kind: codec::Kind::MediaName,
        key,
        origin,
        sent,
        local_rev: 1,
        acked_rev: 0,
        tombstone_at: None,
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
    fn media_download_blocked_by_unsyncable(&self, a: &Authority, name: &str) -> Result<bool, RuntimeError> {
        let (ledger, entities, inbox) = self.with_media(a, |store| {
            Ok((
                store.media_ledger(a.binding)?,
                Self::cloud_media_entities(store, a.binding)?,
                store.inbox(a.binding)?,
            ))
        })?;
        let metadata = &ledger.blobs.get(name).ok_or(Error::InvalidData)?.metadata;
        let key = self.media_record_key(a)?;
        let mut blocked = false;
        let mut allowed = false;
        let mut target = |p: &codec::MediaName| -> Result<(), RuntimeError> {
            if blob::Blob::from(p) == *metadata {
                if ledger
                    .names
                    .get(&text(&p.name)?.to_lowercase())
                    .is_some_and(|f| f.unsyncable)
                {
                    blocked = true;
                } else {
                    allowed = true;
                }
            }
            Ok(())
        };
        for entity in entities {
            if !entity.acked_media_tombstone()
                && let codec::Payload::MediaName(p) = &entity.payload
            {
                target(p)?;
            }
        }
        for entry in inbox {
            let InboxValue::Record { envelope, .. } = &entry.entry.value else {
                continue;
            };
            let received = codec::receive(&key, &entry.entry.record_name, &envelope.0, codec::AssetInput::Absent)
                .map_err(|_| Error::InvalidData)?;
            if !received.entity.fields.tombstone
                && let Some(codec::Payload::MediaName(p)) = received.payload
            {
                target(&p)?;
            }
        }
        Ok(blocked && !allowed)
    }
    pub fn validate_cloud_media_work(&self, collection: &Collection, work: &Work) -> Result<(), RuntimeError> {
        let mut current = self.media_authority(collection, true)?;
        if matches!(work.request, Request::Upload { .. } | Request::Delete { .. }) {
            current.media_epoch = work.authority.media_epoch;
        }
        if self.publication_account()? != work.authority.account || current != work.authority {
            return Err(RuntimeError::IdentityUnverified);
        }
        if let Request::Download { name } = &work.request
            && self.media_download_blocked_by_unsyncable(&work.authority, name)?
        {
            return Err(RuntimeError::StaleIncarnation);
        }
        let s = self.state();
        Self::check_media_locked(&s, &work.authority)?;
        if s.media.pending.as_ref() != Some(work)
            && s.media.metadata_pending.as_ref() != Some(work)
            && !s.actions.media_batches.values().any(|original| &original.work == work)
            && !s
                .actions
                .media_upload
                .as_ref()
                .is_some_and(|(_, original)| original == work)
        {
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
        let a = MediaRecoveryAuthority(self.media_authority(collection, true)?);
        self.media_recovery_io(collection, &a, || Ok(()))?;
        let entries = self.with_media_admission(&a, |s| s.inbox(a.binding))?;
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
            if !matches!(codec::Entity::decode(&envelope.0),
                Ok(codec::DecodeOutcome::Supported(entity)) if entity.fields.kind == codec::Kind::MediaName)
            {
                continue;
            }
            let r = codec::receive(
                &self.media_recovery_record_key(&a)?,
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
                let ledger = self.with_media_recovery(&a, |s| s.media_ledger(a.binding))?;
                if ledger
                    .names
                    .get(text(&r.entity.fields.key)?)
                    .is_some_and(|n| n.withheld.is_some())
                {
                    continue;
                }
                self.begin_local_media_apply(
                    collection,
                    media,
                    &a,
                    PendingMediaApply {
                        incoming: saved,
                        inbox: Some(entry.id),
                        server: None,
                        staged: None,
                    },
                )?;
                count += 1;
            } else if let Some(codec::Payload::MediaName(server)) = r.payload
                && Self::current_media(media, text(&server.name)?)?.as_ref() == Some(&server)
            {
                self.begin_local_media_apply(
                    collection,
                    media,
                    &a,
                    PendingMediaApply {
                        incoming: saved,
                        inbox: Some(entry.id),
                        server: Some(server),
                        staged: None,
                    },
                )?;
                count += 1;
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
        self.cloud_media_work_with_space_and_work(collection, media, space, None, &mut PolicyWork::default())
    }
    /// Complete preparation for the retained SDK, including media open/feed.
    /// Every blocking step stays on the registered engine worker.
    pub fn prepare_cloud_media_work_with_space(
        self: &Arc<Self>,
        collection: &Collection,
        sdk: &str,
        space: &dyn meguri_storage::space::SpaceProvider,
        work: &mut PolicyWork,
    ) -> Result<Option<Work>, RuntimeError> {
        self.prepare_cloud_media_sdk_with_work(sdk, work)?;
        let media = self.media_service_with_work(collection, work)?;
        let consumer: Arc<dyn meguri_engine::media::MediaFeedConsumer> = self.clone();
        media.set_feed_consumer(Arc::downgrade(&consumer));
        // Revalidate after media-open contention; selection checks the original
        // SDK again under its state guard after SDK-independent local capture.
        self.validate_sdk(sdk)?;
        work.check()?;
        self.cloud_media_work_with_space_and_work(collection, &media, space, Some(sdk), work)
    }
    fn cloud_media_work_with_space_and_work(
        &self,
        collection: &Collection,
        media: &MediaService,
        space: &dyn meguri_storage::space::SpaceProvider,
        sdk: Option<&str>,
        authority: &mut PolicyWork,
    ) -> Result<Option<Work>, RuntimeError> {
        let work = self.cloud_media_work_with_work(collection, media, sdk, authority)?;
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

impl Runtime {
    pub(super) fn policy_media_facts_locked(
        s: &State,
        store: &TrackingStore,
        binding: BindingId,
    ) -> Result<crate::cloud::policy::MediaFacts, RuntimeError> {
        let ledger = store.media_ledger(binding)?;
        let key = store.publication_media(binding)?;
        let mut live = BTreeMap::new();
        for entity in Self::cloud_media_entities(store, binding)? {
            if !entity.acked_media_tombstone()
                && let codec::Payload::MediaName(p) = entity.payload
            {
                live.insert(text(&p.name)?.to_lowercase(), p);
            }
        }
        let record_key = store.binding(binding)?.ok_or(Error::BindingNotFound)?.facts.record_key;
        for entry in store.inbox(binding)? {
            if let InboxValue::Record { envelope, .. } = &entry.entry.value
                && let Ok(record) = codec::receive(
                    &record_key.0,
                    &entry.entry.record_name,
                    &envelope.0,
                    codec::AssetInput::Absent,
                )
                && !record.entity.fields.tombstone
                && let Some(codec::Payload::MediaName(p)) = record.payload
            {
                live.insert(text(&p.name)?.to_lowercase(), p);
            }
        }
        let mut blobs_by_metadata = BTreeMap::new();
        for state in ledger.blobs.values() {
            #[cfg(feature = "test-hooks")]
            super::testing::media_policy_blob_candidate_visit();
            let metadata = &state.metadata;
            // Keep the first row in ledger record-name order, just as find did.
            blobs_by_metadata
                .entry((metadata.sha256, metadata.sha1, metadata.size))
                .or_insert(state);
        }
        let lookup_blob = |metadata: &blob::Blob| {
            blobs_by_metadata
                .get(&(metadata.sha256, metadata.sha1, metadata.size))
                .copied()
                .inspect(|_| {
                    #[cfg(feature = "test-hooks")]
                    super::testing::media_policy_blob_candidate_visit();
                })
        };
        let mut uploads = BTreeMap::new();
        let mut downloads = BTreeMap::new();
        let mut unknown = BTreeSet::new();
        let mut missing = 0u64;
        for (name, payload) in &live {
            let metadata = blob::Blob::from(payload);
            let tracked = lookup_blob(&metadata);
            let flags = ledger.names.get(name);
            if flags.is_some_and(|f| f.removal_held || f.withheld.is_some() || f.tombstone_at.is_some()) {
                continue;
            }
            if tracked.is_none_or(|b| b.server == ServerState::Unknown) {
                unknown.insert(metadata.sha256);
            }
            let local_matches = flags
                .and_then(|f| f.local.as_ref())
                .is_some_and(|p| p.sha256 == metadata.sha256);
            if !local_matches || flags.is_some_and(|f| f.file_missing || f.superseded.is_some()) {
                downloads.insert(metadata.sha256, metadata.size);
                if tracked.is_some_and(|b| b.server == ServerState::Deleted) {
                    missing += 1;
                }
            }
        }
        for flags in ledger.names.values().filter(|f| f.sendable()) {
            if let Some(local) = &flags.local {
                let metadata = blob::Blob::from(local);
                let tracked = lookup_blob(&metadata);
                if tracked.is_none_or(|b| b.server != ServerState::Present && b.upload != Transfer::Complete) {
                    uploads.insert(metadata.sha256, metadata.size);
                }
                if tracked.is_none_or(|b| b.server == ServerState::Unknown) {
                    unknown.insert(metadata.sha256);
                }
            }
        }
        let bytes = |files: &BTreeMap<[u8; 32], u64>| -> Result<u64, RuntimeError> {
            files.values().try_fold(0u64, |total, size| {
                total.checked_add(*size).ok_or(Error::InvalidData.into())
            })
        };
        Ok(crate::cloud::policy::MediaFacts {
            upload_files: Some(uploads.len() as u64),
            upload_bytes: Some(bytes(&uploads)?),
            download_files: Some(downloads.len() as u64),
            download_bytes: Some(bytes(&downloads)?),
            unknown_blobs: Some(unknown.len() as u64),
            missing_names: Some(missing),
            root_acked: Some(key.is_some_and(|(_, acked, _)| acked)),
            zone_missing: Some(ledger.zone_missing),
            no_space: Some(s.media.no_space),
            // Durable Running includes prepared and interrupted work. It is
            // insufficient evidence for a live native transfer or an estimate.
            uploads_running: None,
            downloads_running: None,
            remaining_download_ms: None,
        })
    }
}

impl Runtime {
    fn cloud_due_media_names(collection: &Collection) -> Result<BTreeSet<String>, RuntimeError> {
        let day = collection.check_rollover()?;
        let lease = collection.reader(ReaderRole::Cloud)?;
        let snapshot = lease.snapshot()?;
        let mut notes = BTreeSet::new();
        let mut names = BTreeSet::new();
        for row in snapshot.cards_in_queues(&[1, 2, 3, 4])? {
            let due = match row.queue {
                1 | 4 => row.due < day.next_day_at,
                2 | 3 => row.due <= i64::from(day.today),
                _ => false,
            };
            if due && let Some(card) = snapshot.card(row.id)? {
                notes.insert(card.note_id);
            }
        }
        for note in notes {
            if let Some(note) = snapshot.note(note)? {
                names.extend(
                    meguri_text::media_refs::local_references(&note.fields)
                        .into_iter()
                        .map(|name| name.to_lowercase()),
                );
            }
        }
        Ok(names)
    }
}

impl Runtime {
    fn cloud_media_entities(store: &TrackingStore, binding: BindingId) -> Result<Vec<EntityFacts>, Error> {
        let mut after = None;
        let mut entities = Vec::new();
        loop {
            let page = store.entity_page(binding, codec::Kind::MediaName, after.as_deref())?;
            let Some(last) = page.last() else { return Ok(entities) };
            after = Some(last.key.clone());
            entities.extend(page);
        }
    }
}

#[cfg(test)]
mod recovery_authority_tests {
    use super::*;

    // Storage-only semantic seam for metadata duplicates/mismatches and held
    // flags that cannot safely be fabricated through public SDK callbacks.
    // The real Runtime.policy_projection cost proof lives in cloud_media_cleanup.
    fn policy_facts_fixture(ledger: Ledger, payloads: Vec<codec::MediaName>) -> crate::cloud::policy::MediaFacts {
        use meguri_model::{
            clock::{FixedClock, UtcOffset},
            rng::SeededRandom,
        };
        let directory = tempfile::tempdir().unwrap();
        let (mut state, mut authority) = paused_owner();
        let binding = state.bound.as_ref().unwrap().binding.clone();
        let mut store = match TrackingStore::open(directory.path(), binding.scope).unwrap() {
            OpenOutcome::Missing(store) => store,
            _ => panic!("new synthetic store"),
        };
        store.install_runtime().unwrap();
        let binding = store
            .create_binding(
                binding.facts,
                &FixedClock::new(1000, UtcOffset::UTC),
                &SeededRandom::new(1915),
            )
            .unwrap();
        authority.0.binding = binding.id;
        let tx = store.connection.transaction().unwrap();
        m::store::put_ledger(&tx, binding.id, &ledger).unwrap();
        for payload in payloads {
            let key = text(&payload.name).unwrap().to_lowercase().into_bytes();
            let name = codec::entity_name(&binding.facts.record_key.0, codec::Kind::MediaName, &key).unwrap();
            let entity = fresh_media_entity(
                &name,
                key,
                [1; 16],
                codec::Payload::MediaName(payload),
                false,
                &authority,
            );
            workflow_store::put(&tx, binding.id, &entity).unwrap();
        }
        tx.commit().unwrap();
        state.media.no_space = true;
        Runtime::policy_media_facts_locked(&state, &store, binding.id).unwrap()
    }

    fn policy_metadata(id: u8, size: u64) -> blob::Blob {
        blob::Blob {
            sha256: [id; 32],
            sha1: [id; 20],
            size,
        }
    }

    fn expected_policy_facts(
        uploads: (u64, u64),
        downloads: (u64, u64),
        unknown: u64,
        missing: u64,
    ) -> crate::cloud::policy::MediaFacts {
        crate::cloud::policy::MediaFacts {
            upload_files: Some(uploads.0),
            upload_bytes: Some(uploads.1),
            download_files: Some(downloads.0),
            download_bytes: Some(downloads.1),
            unknown_blobs: Some(unknown),
            missing_names: Some(missing),
            root_acked: Some(false),
            zone_missing: Some(true),
            no_space: Some(true),
            uploads_running: None,
            downloads_running: None,
            remaining_download_ms: None,
        }
    }

    #[test]
    fn media_policy_lookup_cost_exact_metadata_and_duplicate_first_winner_in_both_loops() {
        let shared = policy_metadata(1, 7);
        let sha1_mismatch = policy_metadata(3, 11);
        let size_mismatch = policy_metadata(4, 13);
        let mut ledger = Ledger {
            zone_missing: true,
            ..Default::default()
        };
        for (key, metadata, server) in [
            ("a-first", shared.clone(), ServerState::Deleted),
            ("z-duplicate", shared.clone(), ServerState::Present),
            (
                "0-wrong-sha1",
                blob::Blob {
                    sha1: [9; 20],
                    ..sha1_mismatch.clone()
                },
                ServerState::Present,
            ),
            (
                "0-wrong-size",
                blob::Blob {
                    size: 14,
                    ..size_mismatch.clone()
                },
                ServerState::Present,
            ),
        ] {
            ledger.blobs.insert(
                key.into(),
                BlobState {
                    metadata,
                    server,
                    ..Default::default()
                },
            );
        }
        for (name, metadata) in [
            ("upload.bin", &shared),
            ("sha1-upload.bin", &sha1_mismatch),
            ("size-upload.bin", &size_mismatch),
        ] {
            ledger.names.insert(
                name.into(),
                NameState {
                    local: Some(metadata.media(name)),
                    ..Default::default()
                },
            );
        }
        let facts = policy_facts_fixture(
            ledger,
            vec![
                shared.media("remote.bin"),
                shared.media("shared.bin"),
                policy_metadata(2, 9).media("unknown.bin"),
                sha1_mismatch.media("sha1.bin"),
                size_mismatch.media("size.bin"),
            ],
        );
        // Two missing names share one download; SHA256 alone must not match
        // either mismatched Present row. Duplicate metadata selects a-first.
        assert_eq!(facts, expected_policy_facts((3, 31), (4, 40), 3, 2));
    }

    #[test]
    fn media_policy_lookup_cost_case_folded_flags_are_preserved() {
        let mut ledger = Ledger {
            zone_missing: true,
            ..Default::default()
        };
        let mut payloads = Vec::new();
        for (id, name, mut flags) in [
            (
                1,
                "WithHeld.bin",
                NameState {
                    withheld: Some([1; 32]),
                    ..Default::default()
                },
            ),
            (
                2,
                "Removal.bin",
                NameState {
                    removal_held: true,
                    ..Default::default()
                },
            ),
            (
                3,
                "Tombstone.bin",
                NameState {
                    tombstone_at: Some(1),
                    ..Default::default()
                },
            ),
            (
                4,
                "Missing.bin",
                NameState {
                    file_missing: true,
                    ..Default::default()
                },
            ),
            (
                5,
                "Superseded.bin",
                NameState {
                    superseded: Some([5; 32]),
                    ..Default::default()
                },
            ),
            (
                6,
                "Changed.bin",
                NameState {
                    media_changed: Some([6; 32]),
                    ..Default::default()
                },
            ),
            (
                7,
                "Unsyncable.bin",
                NameState {
                    unsyncable: true,
                    ..Default::default()
                },
            ),
        ] {
            let payload = policy_metadata(id, 2 * u64::from(id) + 3).media(name);
            if flags.tombstone_at.is_none() {
                flags.local = Some(payload.clone());
            }
            ledger.names.insert(name.to_lowercase(), flags);
            payloads.push(payload);
        }
        assert_eq!(
            policy_facts_fixture(ledger, payloads),
            expected_policy_facts((0, 0), (2, 24), 4, 0)
        );
    }

    fn paused_owner() -> (State, MediaRecoveryAuthority) {
        let authority = Authority {
            binding: BindingId([1; 16]),
            open: 2,
            incarnation: 3,
            generation: 4,
            file: NativeFileIdentity {
                device: 5,
                inode: 6,
                created_seconds: 7,
                created_nanos: 8,
            },
            account: OpaqueBytes(b"synthetic-local-recovery".to_vec()),
            adapter_token: 0,
            media_epoch: 0,
        };
        let mut state = State {
            incarnation: authority.incarnation,
            bound: Some(Bound {
                binding: Binding {
                    id: authority.binding,
                    created_at_ms: 0,
                    scope: BuildScope {
                        container: ContainerId::new("synthetic-recovery").unwrap(),
                        environment: Environment::Development,
                    },
                    facts: BindingFacts {
                        account: authority.account.clone(),
                        epoch: EpochId([2; 16]),
                        record_key: RecordKey([3; 32]),
                        publication: meguri_engine::device_state::PublicationId::from_bytes([4; 16]),
                        state: BindingState::Active,
                    },
                },
                evidence: OpenEvidence {
                    generation: Generation(authority.generation),
                    previous_clean_shutdown: false,
                    opened_mod: 0,
                    opened_scm: 0,
                    native_file: authority.file.pair(),
                    cloud_file: None,
                },
                file: authority.file,
                open_counter: authority.open,
                usable: true,
                verified: true,
                latest_mod: 0,
                latest_scm: 0,
            }),
            ..State::default()
        };
        state.adapter.paused = true;
        state.media.paused = true;
        state.media.enabled = Some(false);
        (state, MediaRecoveryAuthority(authority))
    }

    #[test]
    fn local_recovery_keeps_exact_authority_and_does_not_open_network_gate() {
        let (state, original) = paused_owner();
        assert!(Runtime::check_media_recovery_locked(&state, &original).is_ok());
        assert!(Runtime::check_media_locked(&state, &original).is_err());
        let mutations: [fn(&mut Authority); 8] = [
            |a| a.binding.0[0] ^= 1,
            |a| a.open += 1,
            |a| a.incarnation += 1,
            |a| a.generation += 1,
            |a| a.file.created_nanos += 1,
            |a| a.account.0.push(0),
            |a| a.adapter_token += 1,
            |a| a.media_epoch += 1,
        ];
        for mutate in mutations {
            let mut stale = original.clone();
            mutate(&mut stale.0);
            assert!(matches!(
                Runtime::check_media_recovery_locked(&state, &stale),
                Err(RuntimeError::StaleIncarnation)
            ));
        }
        let refusals: [fn(&mut State); 6] = [
            |s| s.bound.as_mut().unwrap().usable = false,
            |s| s.bound.as_mut().unwrap().verified = false,
            |s| s.bound.as_mut().unwrap().binding.facts.state = BindingState::Isolated(IsolationReason::FileMismatch),
            |s| s.invalidation_exhausted = true,
            |s| s.tracking_stopped = true,
            |s| {
                s.adapter.sdk = Some(crate::cloud::adapter::SDKAuthority {
                    id: "synthetic-stopping-sdk".into(),
                    guard: Runtime::adapter_guard_locked(s),
                    stopping: true,
                })
            },
        ];
        for refuse in refusals {
            let (mut state, original) = paused_owner();
            refuse(&mut state);
            assert!(Runtime::check_media_recovery_locked(&state, &original).is_err());
        }
    }

    #[test]
    fn retained_invalidated_sdk_allows_only_current_local_recovery() {
        let (mut state, old) = paused_owner();
        state.adapter.sdk = Some(crate::cloud::adapter::SDKAuthority {
            id: "synthetic-retained-sdk".into(),
            guard: Runtime::adapter_guard_locked(&state),
            stopping: true,
        });
        assert!(matches!(
            Runtime::check_media_recovery_locked(&state, &old),
            Err(RuntimeError::StaleIncarnation)
        ));

        state.incarnation += 1;
        state.adapter.token += 1;
        let mut current = old.clone();
        current.0.incarnation = state.incarnation;
        current.0.adapter_token = Runtime::adapter_guard_locked(&state).token;
        assert!(Runtime::check_media_recovery_locked(&state, &current).is_ok());
        assert!(matches!(
            Runtime::check_media_recovery_locked(&state, &old),
            Err(RuntimeError::StaleIncarnation)
        ));
        // Even with media unpaused, the retained object admits no network work.
        state.adapter.paused = false;
        state.media.paused = false;
        state.media.enabled = Some(true);
        assert!(matches!(
            Runtime::check_media_locked(&state, &current),
            Err(RuntimeError::StaleIncarnation)
        ));

        let guard = Runtime::adapter_guard_locked(&state);
        state.adapter.sdk.as_mut().unwrap().guard = guard;
        assert!(matches!(
            Runtime::check_media_recovery_locked(&state, &current),
            Err(RuntimeError::StaleIncarnation)
        ));
    }
}
