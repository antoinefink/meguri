//! Reconciliation of an already admitted manual media stage (media-sync Y3–Y14).
//!
//! The caller starts a separate engine MediaSync job after OneWaySync drains,
//! checks the operation's selected scope/identity, and supplies its resulting
//! generation. This module grants no permission, schedules nothing and retains
//! no account configuration. Use the blocking entry point on the engine worker:
//! it drains cancellation and file work before releasing media-job ownership.

use crate::{ErrorKind, Job, JobError, Method, Payload, transport::runtime, wire};
use meguri_engine::{
    CancelToken, EngineError, StopCause, ThreadRegistry,
    media::{MediaJobKind, MediaService},
};
use meguri_storage::media::{
    MediaBaselineOwner, MediaEntry, MediaError, MediaPageDecision, MediaStore, MediaSyncBaseline, MediaSyncState,
    PendingChange, MediaBoundReset, MediaBoundCheckError, UnsupportedMedia,
};
use std::{
    collections::{BTreeMap, BTreeSet, VecDeque},
    fs::Metadata,
    os::unix::fs::MetadataExt,
    sync::Arc,
    time::Duration,
};

const BATCH: u32 = 25;
const FILL: u64 = 2_621_440;
// Limit the received archive, allowing one MiB for its directory and _meta
// in addition to the payload budget defined by W17.1.
const DOWNLOAD_REPLY_LIMIT: u64 = wire::media::MAX_DOWNLOAD_BYTES + (1 << 20);
const REBUILD_LIMIT: usize = 3;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MediaPhase {
    #[default]
    Checking,
    Listing,
    Downloading,
    Uploading,
    Confirming,
    Resyncing,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaSyncResult {
    pub uploaded_files: u64,
    pub uploaded_deletions: u64,
    pub downloaded_files: u64,
    pub downloaded_deletions: u64,
    pub checked: u64,
    pub bytes_downloaded: u64,
    pub bytes_uploaded: u64,
    pub resynced: bool,
    pub oversized: Vec<String>,
    pub empty: Vec<String>,
    pub awaiting: u64,
}

/// Media-specific values for a later host adapter. No phase-average ETA or
/// completion inference is taken from the generic engine ProgressSink.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MediaSnapshot {
    /// Retained evidence of an actual transfer request, not a prepared batch.
    pub transfer_started: bool,
    pub phase: MediaPhase,
    pub totals: MediaSyncResult,
    pub listed: u64,
    pub download_files: u64,
    pub download_received: u64,
    pub published_files: u64,
    pub files_left: u64,
    pub estimate: Option<Duration>,
}

/// A failed or cancelled stage still has real effects. The caller retains this
/// snapshot alongside the already completed collection-stage result.
#[derive(Clone, Debug)]
pub struct MediaFailure {
    pub error: JobError,
    pub snapshot: MediaSnapshot,
}

/// Last twenty saved batches, available to deterministic Rust progress tests.
#[derive(Default, Debug)]
pub struct DownloadWindow(VecDeque<(Duration, u64)>);
impl DownloadWindow {
    pub fn saved_batch(&mut self, elapsed: Duration, received: u64) {
        if received == 0 {
            return;
        }
        self.0.push_back((elapsed, received));
        if self.0.len() > 20 {
            self.0.pop_front();
        }
    }
    pub fn estimate(&self, left: u64) -> Option<Duration> {
        if self.0.len() < 3 {
            return None;
        }
        let files: u64 = self.0.iter().map(|(_, files)| files).sum();
        let seconds: f64 = self.0.iter().map(|(elapsed, _)| elapsed.as_secs_f64()).sum();
        Duration::try_from_secs_f64(left as f64 * seconds / files as f64).ok()
    }
}

/// Run only on the admitted engine job's worker. `job` must use that job's
/// cancellation token. Callbacks run synchronously outside all store/lifecycle
/// locks and may inspect media or submit collection operations.
///
/// The outer runtime token deliberately never fires: the actual token is
/// checked by every request, file helper and checkpoint. Dropping the whole
/// future on cancel would release the media lease before file workers drain.
pub fn reconcile(
    threads: &ThreadRegistry,
    job: &mut Job,
    service: &MediaService,
    expected_generation: u64,
    owner: &MediaBaselineOwner,
    progress: &mut dyn FnMut(&MediaSnapshot),
) -> Result<MediaSyncResult, Box<MediaFailure>> {
    let cancel = job.cancellation();
    let mut stage = Stage {
        job,
        service,
        generation: expected_generation,
        cancel,
        baseline: None,
        snapshot: MediaSnapshot::default(),
        progress,
        window: DownloadWindow::default(),
        prebound: None, initial: None, resets: Vec::new(), binding_changed: std::cell::Cell::new(false),
    };
    let keep_draining = CancelToken::new();
    let result = runtime::block_on(threads, &keep_draining, async { Ok(stage.run(owner).await) });
    let result = match result {
        Ok(result) => result,
        Err(error) => Err(stage.job.error_for(Method::MediaBegin, error)),
    };
    let result = if stage.cancel.is_cancelled() {
        Err(stopped(&stage.cancel))
    } else {
        result
    };
    match result {
        Ok(()) => Ok(stage.snapshot.totals),
        Err(error) => Err(Box::new(MediaFailure {
            error,
            snapshot: stage.snapshot,
        })),
    }
}

/// Explicit local data; names are never included in diagnostics or Debug.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AffectedMediaKind { Empty, OverLimit, InvalidName, NotRegularFile, Pending, Awaiting }
#[derive(Clone)]
pub struct AffectedMedia { pub name: String, pub kind: AffectedMediaKind }
#[derive(Clone, Default)]
pub struct RoundMediaAccounting {
    pub excluded: u64, pub pending: u64, pub awaiting: u64,
    pub affected: Vec<AffectedMedia>,
}
/// Constructed only by the actual Stage before relinquishing its Sync lease.
/// Reset authority comes from storage receipts, never resynced or a later read.
pub struct BoundMediaReceipt {
    initial: MediaSyncBaseline, terminal: MediaSyncBaseline, resets: Vec<MediaBoundReset>,
}
impl BoundMediaReceipt {
    pub(crate) fn initial(&self) -> &MediaSyncBaseline { &self.initial }
    pub(crate) fn terminal(&self) -> &MediaSyncBaseline { &self.terminal }
    pub(crate) fn owner(&self) -> &MediaBaselineOwner { self.initial.owner.as_ref().expect("prebound owner") }
    pub fn reset_occurred(&self) -> bool { !self.resets.is_empty() }
    pub(crate) fn with_current<T>(&self, store: &MediaStore, publish: impl FnOnce() -> T)
        -> Result<T, MediaBoundCheckError> {
        let mut epoch = self.initial.epoch;
        for reset in &self.resets {
            if !reset.is_from(store) || reset.before().owner != self.initial.owner
                || reset.before().epoch != epoch || reset.after().owner != self.initial.owner {
                return Err(MediaBoundCheckError::Changed);
            }
            epoch = reset.after().epoch;
        }
        if self.terminal.epoch != epoch || self.terminal.owner != self.initial.owner {
            return Err(MediaBoundCheckError::Changed);
        }
        store.with_bound_sync_baseline(&self.terminal, publish)
    }
}
pub struct BoundMediaTerminal {
    outcome: Result<MediaSyncResult, Box<MediaFailure>>,
    receipt: Option<BoundMediaReceipt>,
    accounting: Result<RoundMediaAccounting, JobError>,
    binding_changed: bool,
    // Kept with the result through guarded Host metadata finalization/retry.
    _lease: Option<meguri_engine::media::MediaJobLease>,
}
impl BoundMediaTerminal {
    pub fn outcome(&self) -> &Result<MediaSyncResult, Box<MediaFailure>> { &self.outcome }
    pub fn release_finalized_lease(&mut self) { self._lease = None; }
    pub fn binding_changed(&self) -> bool { self.binding_changed }
    pub fn receipt(&self) -> Option<&BoundMediaReceipt> { self.receipt.as_ref() }
    pub fn accounting(&self) -> &Result<RoundMediaAccounting, JobError> { &self.accounting }
}
fn routine_accounting(stage: &Stage<'_>) -> Result<RoundMediaAccounting, JobError> {
    let inventory = local(|| stage.store().promised_inventory(wire::media::MAX_DOWNLOAD_ENTRY_BYTES,
        &mut || !stage.cancel.is_cancelled()))?;
    let mut result = RoundMediaAccounting::default();
    for file in inventory.files() {
        let kind = match file.unsupported {
            Some(UnsupportedMedia::Empty) => Some(AffectedMediaKind::Empty),
            Some(UnsupportedMedia::OverLimit) => Some(AffectedMediaKind::OverLimit),
            Some(UnsupportedMedia::InvalidName) => Some(AffectedMediaKind::InvalidName),
            Some(UnsupportedMedia::NotRegularFile) => Some(AffectedMediaKind::NotRegularFile),
            None => {
                let entry = local(|| stage.store().sync_entry(&file.name))?;
                if entry.as_ref().is_none_or(|e| !file.matches_journal(e) || e.dirty || e.pending_download) {
                    Some(AffectedMediaKind::Pending)
                } else { None }
            }
        };
        if let Some(kind) = kind {
            if kind == AffectedMediaKind::Pending { result.pending += 1; } else { result.excluded += 1; }
            result.affected.push(AffectedMedia { name: file.name.clone(), kind });
        }
    }
    // Missing remote files and pending deletions also belong to the scope.
    for name in local(|| stage.store().awaiting_names())? {
        result.awaiting += 1;
        result.affected.push(AffectedMedia { name, kind: AffectedMediaKind::Awaiting });
    }
    for pending in local(|| stage.store().pending())? {
        if !result.affected.iter().any(|a| a.name == pending.name) {
            result.pending += 1;
            result.affected.push(AffectedMedia { name: pending.name, kind: AffectedMediaKind::Pending });
        }
    }
    Ok(result)
}
/// Prebound provider execution. Acquire Sync FIRST; mismatches cannot invoke
/// bind_sync_owner, scan, recovery, reset or a request. This owns the same Stage
/// and file-worker drain as legacy reconcile.
pub fn reconcile_bound(
    threads: &ThreadRegistry, job: &mut Job, service: &MediaService,
    expected_generation: u64, admission: crate::provider::MeguriRoundAdmission,
    progress: &mut dyn FnMut(&MediaSnapshot),
) -> BoundMediaTerminal {
    let lease = match service.try_job(MediaJobKind::Sync) {
        Ok(lease) => lease,
        Err(error) => return BoundMediaTerminal { outcome: Err(Box::new(MediaFailure {
            error: engine_error(error), snapshot: MediaSnapshot::default() })), receipt: None,
            accounting: Err(local_failure("media Sync lease unavailable")), binding_changed: false, _lease: None },
    };
    let owner = admission.binding().media_owner();
    let prebound = admission.binding().media_epoch().clone();
    job.admit_meguri(admission);
    let cancel = job.cancellation();
    let mut stage = Stage { job, service, generation: expected_generation, cancel,
        baseline: None, snapshot: MediaSnapshot::default(), progress, window: DownloadWindow::default(),
        prebound: Some(prebound), initial: None, resets: Vec::new(), binding_changed: std::cell::Cell::new(false) };
    let keep_draining = CancelToken::new();
    let run = runtime::block_on(threads, &keep_draining, async { Ok(stage.run(&owner).await) });
    let mut result = match run { Ok(result) => result,
        Err(error) => Err(stage.job.error_for(Method::MediaBegin, error)) };
    // A genuine failure keeps its own error and committed-effect snapshot.
    if matches!(&result, Err(error) if error.kind == ErrorKind::Cancelled)
        && stage.cancel.stop_cause() == Some(StopCause::System) { result = Err(stopped(&stage.cancel)); }
    if result.is_ok() { result = stage.checkpoint(); }
    let accounting = if stage.initial.is_some() { routine_accounting(&stage) }
        else { Err(state_changed()) };
    if result.is_ok() { result = stage.checkpoint(); }
    let receipt = stage.initial.take().zip(stage.baseline.take()).map(|(initial, terminal)|
        BoundMediaReceipt { initial, terminal, resets: std::mem::take(&mut stage.resets) });
    let binding_changed = stage.binding_changed.get();
    let outcome = match result { Ok(()) => Ok(stage.snapshot.totals),
        Err(error) => Err(Box::new(MediaFailure { error, snapshot: stage.snapshot })) };
    BoundMediaTerminal { outcome, receipt, accounting, binding_changed, _lease: Some(lease) }
}

struct Stage<'a> {
    job: &'a mut Job,
    service: &'a MediaService,
    generation: u64,
    cancel: Arc<CancelToken>,
    baseline: Option<MediaSyncBaseline>,
    snapshot: MediaSnapshot,
    progress: &'a mut dyn FnMut(&MediaSnapshot),
    window: DownloadWindow,
    prebound: Option<crate::provider::MediaEpochEvidence>,
    initial: Option<MediaSyncBaseline>,
    resets: Vec<MediaBoundReset>,
    binding_changed: std::cell::Cell<bool>,
}
impl Stage<'_> {
    fn store(&self) -> &MediaStore {
        self.service.store()
    }
    // The provider-issued witness is the only nonlegacy boundary. Storage
    // receives a refusal callback, never a caller's claimed readiness value.
    fn effect_boundary(&self) -> impl FnMut() -> Result<(), MediaError> + 'static + use<> {
        let admission = self.job.meguri_admission();
        let cancel = self.cancel.clone();
        move || {
            if let Some(admission) = &admission
                && (cancel.is_cancelled() || admission.check_media_effect().is_err()) {
                return Err(MediaError::Cancelled);
            }
            Ok(())
        }
    }
    fn bound_check_error(&self, error: MediaBoundCheckError) -> JobError {
        match error {
            MediaBoundCheckError::Changed => { self.binding_changed.set(true); state_changed() },
            MediaBoundCheckError::Storage(error) => self.boundary_error(error),
        }
    }
    fn boundary_error(&self, error: MediaError) -> JobError {
        if matches!(error, MediaError::Cancelled) && self.job.admission_refused() {
            return self.job.check_admission().expect_err("retained admission refusal");
        }
        if let Some(expected) = &self.baseline
            && let Some(owner) = &expected.owner {
            match self.store().bound_sync_baseline(owner, expected.epoch) {
                Ok(current) if &current != expected => { self.binding_changed.set(true); return state_changed(); }
                Err(MediaBoundCheckError::Changed) => { self.binding_changed.set(true); return state_changed(); }
                _ => {} // A read failure cannot replace the genuine original storage error.
            }
        }
        if matches!(error, MediaError::Cancelled) && self.cancel.is_cancelled() { return stopped(&self.cancel); }
        storage_error(error)
    }
    fn guarded_local<T>(&self, work: impl FnOnce() -> Result<T, MediaError>) -> Result<T, JobError> {
        runtime::blocking_work(work).map_err(|error| self.boundary_error(error))
    }
    fn emit(&mut self, phase: MediaPhase) {
        self.snapshot.phase = phase;
        (self.progress)(&self.snapshot);
    }
    fn checkpoint(&self) -> Result<(), JobError> {
        if self.cancel.is_cancelled() {
            return Err(stopped(&self.cancel));
        }
        self.job.check_admission()?;
        self.service.check_generation(self.generation).map_err(engine_error)?;
        if let Some(expected) = &self.baseline {
            let current = local(|| self.store().sync_baseline())?;
            if &current != expected {
                self.binding_changed.set(true);
                return Err(state_changed());
            }
        }
        Ok(())
    }
    fn baseline(&self) -> &MediaSyncBaseline {
        self.baseline.as_ref().expect("bound media stage")
    }
    fn advanced(&mut self, usn: u64) {
        self.baseline.as_mut().expect("bound media stage").usn = usn;
    }
    async fn json(&mut self, method: Method, body: Vec<u8>) -> Result<Vec<u8>, JobError> {
        self.checkpoint()?;
        let response = self
            .job
            .request(method, Payload::Json(body))
            .await
            .map_err(|e| self.job.error_for(method, e))?;
        response.bytes().await.map_err(|e| self.job.error_for(method, e))
    }
    fn exclusions(&mut self) -> Result<(), JobError> {
        let entries = local(|| self.store().entries())?;
        self.snapshot.totals.oversized = entries
            .iter()
            .filter(|e| e.live && e.size > wire::media::MAX_DOWNLOAD_ENTRY_BYTES)
            .map(|e| e.name.clone())
            .collect();
        self.snapshot.totals.empty = entries
            .iter()
            .filter(|e| e.live && e.size == 0)
            .map(|e| e.name.clone())
            .collect();
        self.snapshot.totals.oversized.sort();
        self.snapshot.totals.empty.sort();
        self.snapshot.totals.awaiting = local(|| self.store().awaiting_names())?.len() as u64;
        Ok(())
    }
    fn check_changes(&mut self) -> Result<(), JobError> {
        self.emit(MediaPhase::Checking);
        self.checkpoint()?;
        let checked_before = self.snapshot.totals.checked;
        let service = self.service;
        let cancel = self.cancel.clone();
        let admission = self.job.meguri_admission();
        let mut before_effect = self.effect_boundary();
        let snapshot = &mut self.snapshot;
        let progress = &mut self.progress;
        // Storage owns the marker fast path; changed-folder callbacks and
        // hashing remain outside the journal guard, with cancellation.
        let report = if admission.is_some() {
            runtime::blocking_work(|| service.store().check_changes_with_boundary(&mut before_effect))
        } else { runtime::blocking_work(|| {
            service.store().check_changes_report_with_progress(|seen, _| {
                snapshot.totals.checked = checked_before + seen as u64;
                progress(snapshot);
                !cancel.is_cancelled() && admission.as_ref().is_none_or(|a| a.check().is_ok())
            }).map(|_| ())
        }) };
        if let Ok(Some(report)) = service.store().last_check_report() {
            self.snapshot.totals.checked = checked_before + report.seen;
        }
        if self.job.admission_refused() { self.job.check_admission()?; }
        report.map_err(|error| self.boundary_error(error))?;
        self.exclusions()?;
        self.checkpoint()
    }
    async fn run(&mut self, owner: &MediaBaselineOwner) -> Result<(), JobError> {
        self.checkpoint()?;
        if self.job.kind() != crate::ServerKind::SelfHosted {
            return Err(JobError::new(
                ErrorKind::Interrupted,
                None,
                "manual media stage requires its configured self-hosted job",
            ));
        }
        let _lease = if self.prebound.is_none() {
            Some(self.service.try_job(MediaJobKind::Sync).map_err(engine_error)?)
        } else { None };
        if let Some(recorded) = &self.prebound {
            // The provider caller already owns the original Sync lease. This
            // read cannot bind, reset, recover or scan an inadmissible journal.
            let baseline = self.store().bound_sync_baseline(owner, recorded.epoch())
                .map_err(|error| self.bound_check_error(error))?;
            if !recorded.matches(&baseline) { self.binding_changed.set(true); return Err(state_changed()); }
            self.initial = Some(baseline.clone());
            self.baseline = Some(baseline);
            self.check_changes()?;
        } else {
        // Do not bind/reset a different account before checking the caller's
        // generation. The opaque configured identity is never put in progress.
        self.check_changes()?;
        let rebound = local(|| self.store().bind_sync_owner(owner))?;
        self.baseline = Some(local(|| self.store().sync_baseline())?);
        if rebound {
            self.check_changes()?;
        }
        }
        for attempt in 0..2 {
            if attempt != 0 {
                self.check_changes()?;
            }
            let begin = self.json(Method::MediaBegin, wire::encode_begin()).await?;
            let server_usn = positive_usn(wire::decode_begin(&begin)?, Method::MediaBegin)?;
            let mut requested = false;
            if server_usn != self.baseline().usn {
                self.list().await?;
                requested = true;
            }
            requested |= !local(|| self.store().download_intents())?.is_empty();
            self.download().await?;
            loop {
                requested |= self.push().await?;
                self.checkpoint()?;
                if local(|| self.store().clean_live_count())?.is_none() {
                    continue;
                }
                if !requested {
                    self.exclusions()?;
                    self.checkpoint()?;
                    return Ok(());
                }
                self.emit(MediaPhase::Confirming);
                self.checkpoint()?;
                let Some(count) = local(|| self.store().clean_live_count())? else {
                    continue;
                };
                let count = u32::try_from(count).map_err(|_| local_failure("media count exceeds wire range"))?;
                let body = self.json(Method::MediaSanity, wire::encode_media_sanity(count)).await?;
                match wire::decode_media_sanity(&body)? {
                    wire::MediaSanity::Ok => {
                        self.exclusions()?;
                        self.checkpoint()?;
                        // A local add can race the request itself. Its old
                        // successful answer cannot confirm the new work.
                        if local(|| self.store().clean_live_count())? != Some(u64::from(count)) {
                            continue;
                        }
                        return Ok(());
                    }
                    wire::MediaSanity::Mismatch => break,
                }
            }
            self.checkpoint()?;
            if self.prebound.is_some() {
                let reset = self.store().reset_bound_sync_with_boundary(self.baseline(), &mut self.effect_boundary()).map_err(|error| self.bound_check_error(error))?;
                // Carry the real reset even if a later provider check refuses.
                self.baseline = Some(reset.after().clone());
                let accepted = self.job.meguri_admission().expect("prebound provider witness").accept_media_reset(&reset);
                self.resets.push(reset);
                if accepted.is_err() { self.binding_changed.set(true); return Err(state_changed()); }
            } else {
                local(|| self.store().reset_sync())?;
                self.baseline = Some(local(|| self.store().sync_baseline())?);
            }
            if attempt != 0 {
                self.exclusions()?;
                let failure = JobError::new(
                    ErrorKind::MediaResyncFailed,
                    Some(Method::MediaSanity),
                    "media sanity failed on the second reconciliation",
                );
                return Err(failure);
            }
            self.snapshot.totals.resynced = true;
            self.emit(MediaPhase::Resyncing);
        }
        unreachable!("bounded media loop returns on its second sanity answer")
    }
    async fn list(&mut self) -> Result<(), JobError> {
        self.emit(MediaPhase::Listing);
        let mut cursor = self.baseline().usn;
        // Last server fact for each name. A later deletion must also retire an
        // earlier download intent while preserving a pending local live file.
        let mut listed = BTreeSet::new();
        loop {
            let last = i32::try_from(cursor).map_err(|_| local_failure("media cursor exceeds wire range"))?;
            let body = self
                .json(Method::MediaChanges, wire::encode_media_changes(last))
                .await?;
            let page = wire::decode_media_changes(&body)?;
            self.checkpoint()?;
            if page.is_empty() {
                let usn = self.guarded_local(|| self.store().finish_media_listing_with_boundary(self.baseline(), &listed, &mut self.effect_boundary()))?;
                self.advanced(usn);
                return Ok(());
            }
            let mut high = cursor;
            let mut latest = BTreeMap::new();
            for change in &page {
                let usn = positive_usn(change.usn, Method::MediaChanges)?;
                if usn <= high || (!change.sha1.is_empty() && digest(&change.sha1).is_none()) {
                    return Err(JobError::bad_response(
                        Method::MediaChanges,
                        "media page order, USN or digest is invalid",
                    ));
                }
                high = usn;
                if change.name.is_empty() {
                    continue;
                }
                if change.sha1.is_empty() {
                    listed.remove(&change.name);
                } else {
                    listed.insert(change.name.clone());
                }
                latest.insert(change.name.clone(), change);
            }
            let changes: Vec<_> = latest.into_values().collect();
            let mut committed = false;
            for _ in 0..REBUILD_LIMIT {
                self.checkpoint()?;
                let mut decisions = Vec::new();
                let mut captured = Vec::new();
                for change in &changes {
                    let entry = local(|| self.store().sync_entry(&change.name))?;
                    if let Some(decision) = decide(change, entry.as_ref()) {
                        decisions.push(decision);
                    }
                    captured.push((change.name.as_str(), entry));
                }
                let applied =
                    runtime::blocking_work(|| self.store().apply_media_page_with_boundary(self.baseline(), high, &decisions, &mut self.effect_boundary()));
                match applied {
                    Ok((usn, effects)) => {
                        self.snapshot.totals.downloaded_deletions += effects.downloaded_deletions;
                        self.advanced(usn);
                        committed = true;
                        break;
                    }
                    Err(failure) => {
                        let removed = failure.effects.downloaded_deletions;
                        self.snapshot.totals.downloaded_deletions += removed;
                        let error = self.boundary_error(failure.error);
                        // Y7 counts filesystem effects even when the journal
                        // transaction fails. Once files moved, retain the error.
                        if removed > 0 {
                            return Err(error);
                        }
                        self.checkpoint()?;
                        // A replacement decision requires evidence that its
                        // captured row changed; storage errors alone cannot retry.
                        let mut changed = false;
                        for (name, before) in &captured {
                            if local(|| self.store().sync_entry(name))? != *before {
                                changed = true;
                                break;
                            }
                        }
                        if !changed {
                            return Err(error);
                        }
                    }
                }
            }
            if !committed {
                return Err(local_failure("media page kept changing"));
            }
            cursor = high; // Listing cursor is independent of the durable USN.
            self.snapshot.listed += page.len() as u64;
            self.snapshot.totals.checked += page.len() as u64;
            self.snapshot.totals.awaiting = local(|| self.store().awaiting_names())?.len() as u64;
            self.emit(MediaPhase::Listing);
        }
    }
    async fn download(&mut self) -> Result<(), JobError> {
        let initial = local(|| self.store().download_intents())?;
        self.snapshot.download_files = initial.len() as u64;
        self.snapshot.download_received = 0;
        self.snapshot.published_files = 0;
        self.snapshot.files_left = initial.len() as u64;
        self.snapshot.estimate = self.window.estimate(initial.len() as u64);
        if initial.is_empty() {
            return Ok(());
        }
        self.emit(MediaPhase::Downloading);
        let staging = owned_staging(self.store())?;
        loop {
            self.checkpoint()?;
            let intents: Vec<_> = local(|| self.store().download_intents())?
                .into_iter()
                .take(BATCH as usize)
                .collect();
            if intents.is_empty() {
                return Ok(());
            }
            let names: Vec<_> = intents.iter().map(|d| d.name.clone()).collect();
            let started = meguri_engine::monotonic::now();
            self.snapshot.transfer_started = true;
            self.emit(MediaPhase::Downloading);
            let response = self
                .job
                .request(
                    Method::DownloadFiles,
                    Payload::Json(wire::encode_download_files(&names)),
                )
                .await
                .map_err(|e| self.job.error_for(Method::DownloadFiles, e))?;
            // W17.1/Y8.2 validation finishes for every member in the private
            // staging directory before any file can become visible to readers.
            let files = wire::spool_media_reply(
                response,
                staging.path().to_path_buf(),
                names,
                DOWNLOAD_REPLY_LIMIT,
                self.job.kind(),
                Arc::clone(&self.cancel),
            )
            .await?;
            self.snapshot.totals.downloaded_files += files.len() as u64;
            self.snapshot.download_received += files.len() as u64;
            self.snapshot.totals.bytes_downloaded += files.iter().map(|f| f.file.size()).sum::<u64>();
            // Keep every received file and the staging owner through all
            // rebuilds. A stale receipt requires the complete answer to be
            // observed/saved again, never a weakened batch commit.
            let mut order: Vec<_> = (0..files.len()).collect();
            let mut changed_names = BTreeSet::new();
            let mut committed = false;
            'repair: for attempt in 0..REBUILD_LIMIT {
                let mut records = vec![None; files.len()];
                for &index in &order {
                    let file = &files[index];
                    self.checkpoint()?;
                    let intent = intents
                        .iter()
                        .find(|d| d.name == file.name)
                        .ok_or_else(|| JobError::bad_response(Method::DownloadFiles, "unrequested media file"))?;
                    let saved = match runtime::blocking_work(|| {
                        if self.prebound.is_some() {
                            self.store().save_downloaded_file_with_boundary(self.baseline(), intent, file.file.path(), &mut self.effect_boundary())
                        } else { self.store().save_downloaded_file(self.baseline(), intent, file.file.path()) }
                    }) {
                        Ok(saved) => saved,
                        Err(MediaError::StaleDownload) if attempt + 1 < REBUILD_LIMIT => {
                            self.checkpoint()?;
                            // A save can publish bytes and then lose its receipt
                            // to an external edit. Discard this pass's records
                            // and observe every staged member again.
                            continue 'repair;
                        }
                        Err(error) => return Err(self.boundary_error(error)),
                    };
                    if saved.changed {
                        if let Some(name) = &saved.record.stored_name {
                            changed_names.insert(name.clone());
                        }
                        self.snapshot.published_files += 1;
                    }
                    records[index] = Some(saved.record);
                    self.emit(MediaPhase::Downloading);
                }
                let records: Vec<_> = records
                    .into_iter()
                    .map(|r| r.expect("complete staged answer"))
                    .collect();
                self.checkpoint()?;
                match runtime::blocking_work(|| self.store().commit_media_downloads_with_boundary(self.baseline(), &records, &mut self.effect_boundary())) {
                    Ok(usn) => {
                        self.advanced(usn);
                        let arrived = records
                            .iter()
                            .filter_map(|r| r.stored_name.as_ref())
                            .filter(|name| changed_names.contains(*name))
                            .cloned()
                            .collect::<BTreeSet<_>>();
                        self.service
                            .media_arrived(arrived.into_iter().collect())
                            .map_err(engine_error)?;
                        committed = true;
                        break;
                    }
                    Err(MediaError::StaleDownload) if attempt + 1 < REBUILD_LIMIT => {
                        self.checkpoint()?;
                        // An exact server name owns its destination. Save it
                        // first on repair so a transformed name can choose a
                        // hash fallback rather than overwrite it each pass.
                        order.sort_by_key(|&i| records[i].stored_name.as_ref() != Some(&records[i].intent.name));
                    }
                    Err(error) => return Err(self.boundary_error(error)),
                }
            }
            if !committed {
                return Err(local_failure("download answer kept changing"));
            }
            self.snapshot.files_left = local(|| self.store().download_intents())?.len() as u64;
            self.snapshot.totals.awaiting = local(|| self.store().awaiting_names())?.len() as u64;
            self.window
                .saved_batch(meguri_engine::monotonic::now() - started, files.len() as u64);
            self.snapshot.estimate = self.window.estimate(self.snapshot.files_left);
            self.emit(MediaPhase::Downloading);
        }
    }
    fn register(&self, captured: &PendingChange) -> Result<(), JobError> {
        for _ in 0..REBUILD_LIMIT {
            self.checkpoint()?;
            self.guarded_local(|| {
                self.store()
                    .register_sync_file_with_boundary(self.baseline(), captured, || !self.cancel.is_cancelled()
                        && self.job.check_admission().is_ok(), &mut self.effect_boundary())
            })?;
            if local(|| self.store().sync_entry(&captured.name))?
                .as_ref()
                .map(|e| e.change_counter)
                != Some(captured.change_counter)
            {
                return Ok(());
            }
        }
        Err(local_failure("upload source kept changing during registration"))
    }
    async fn push(&mut self) -> Result<bool, JobError> {
        let mut requested = false;
        let mut rebuilds = 0;
        let mut directory = UploadDirectory::default();
        loop {
            self.checkpoint()?;
            let pending = local(|| self.store().pending_batch(BATCH))?;
            if pending.is_empty() {
                return Ok(requested);
            }
            self.emit(MediaPhase::Uploading);
            self.checkpoint()?;
            let mut selection = Vec::new();
            let mut captured = Vec::new();
            let mut identities = Vec::new();
            let mut size = 0;
            let mut rebuild = false;
            for change in pending {
                if size > FILL {
                    break;
                }
                self.checkpoint()?;
                let input = if let Some(hash) = &change.sha1 {
                    let entry = local(|| self.store().sync_entry(&change.name))?;
                    let source = runtime::blocking_work(|| directory.source(self.store(), &change.name))?;
                    match (entry, source, digest(hash)) {
                        (Some(entry), Some((file, metadata)), Some(sha1))
                            if entry.change_counter == change.change_counter
                                && entry.sha1.as_ref() == Some(hash)
                                && entry.size == metadata.len()
                                && entry.mtime_secs == metadata.mtime()
                                && i64::from(entry.mtime_nanos) == metadata.mtime_nsec()
                                && (1..=wire::media::MAX_DOWNLOAD_ENTRY_BYTES).contains(&metadata.len()) =>
                        {
                            size += metadata.len();
                            identities.push(Some(metadata.clone()));
                            wire::MediaUploadInput::Put {
                                name: change.name.clone(),
                                file,
                                stamp: change.change_counter,
                                expected: wire::MediaDigest {
                                    length: entry.size,
                                    sha1,
                                },
                            }
                        }
                        _ => {
                            self.register(&change)?;
                            // Y10.3 rebuilds from folder facts. Scan the folder
                            // here so the next selection sees all external edits.
                            self.check_changes()?;
                            rebuild = true;
                            break;
                        }
                    }
                } else {
                    identities.push(None);
                    wire::MediaUploadInput::Remove {
                        name: change.name.clone(),
                        stamp: change.change_counter,
                    }
                };
                selection.push(input);
                captured.push(change);
            }
            if rebuild {
                rebuilds += 1;
                if rebuilds >= REBUILD_LIMIT {
                    return Err(local_failure("upload selection kept changing"));
                }
                continue;
            }
            let staging = owned_staging(self.store())?;
            let upload = match wire::prepare_media_upload(selection, staging.path().into(), self.cancel.clone()).await {
                Ok(upload) => upload,
                Err(error) if error.kind == ErrorKind::LocalStorage && rebuilds < REBUILD_LIMIT => {
                    for change in captured.iter().filter(|c| c.sha1.is_some()) {
                        self.register(change)?;
                    }
                    self.check_changes()?;
                    rebuilds += 1;
                    continue;
                }
                Err(error) => return Err(error),
            };
            self.checkpoint()?;
            let (payload, receipt) = upload.stream_payload(self.cancel.clone());
            requested = true;
            self.snapshot.transfer_started = true;
            self.emit(MediaPhase::Uploading);
            let response = self.job.request(Method::UploadChanges, payload).await;
            // Read the answer while the source may still be in flight; always
            // drain, including a rejected status or an invalid success body.
            let answer = match response {
                Ok(response) => response
                    .bytes()
                    .await
                    .map_err(|e| self.job.error_for(Method::UploadChanges, e)),
                Err(error) => Err(self.job.error_for(Method::UploadChanges, error)),
            };
            // Sample before draining: a later stop cannot replace an already
            // observed provider failure. Legacy cancellation precedence stays.
            let retain_answer_error = self.prebound.is_some() && !self.cancel.is_cancelled()
                && answer.as_ref().is_err_and(|error| !matches!(error.kind, ErrorKind::Cancelled | ErrorKind::Interrupted));
            let mut drain = std::pin::pin!(receipt.drain());
            let sent = std::future::poll_fn(|cx| {
                let result = std::future::Future::poll(drain.as_mut(), cx);
                #[cfg(feature = "test-hooks")]
                if result.is_pending() {
                    // Observation only: it must never bypass the real drain.
                    let _ = meguri_model::faultpoint!("media.sync.upload.drain_pending");
                }
                result
            }).await;
            if sent.is_ok() {
                self.snapshot.totals.bytes_uploaded += upload
                    .records()
                    .iter()
                    .filter_map(|r| r.encoded.as_ref())
                    .map(|d| d.length)
                    .sum::<u64>();
            }
            if retain_answer_error { return Err(answer.expect_err("observed provider answer error")); }
            if self.cancel.is_cancelled() {
                return Err(stopped(&self.cancel));
            }
            let answer = answer?;
            sent?;
            let (processed, usn) = wire::decode_upload_changes(&answer)?;
            let usn = positive_usn(usn, Method::UploadChanges)?;
            if processed == 0 {
                return Err(JobError::bad_response(
                    Method::UploadChanges,
                    "nonempty media batch accepted no changes",
                ));
            }
            let processed = (processed as usize).min(captured.len());
            self.checkpoint()?;
            for ((record, change), identity) in upload.records().iter().zip(&captured).zip(&identities) {
                let changed = if let Some(before) = identity {
                    runtime::blocking_work(|| directory.source(self.store(), &change.name))?
                        .is_none_or(|(_, after)| !same_identity(before, &after))
                } else {
                    runtime::blocking_work(|| directory.source(self.store(), &change.name))?
                        .is_some_and(|(_, after)| (1..=wire::media::MAX_DOWNLOAD_ENTRY_BYTES).contains(&after.len()))
                };
                if !record.matches_capture() || changed {
                    self.register(change)?;
                }
            }
            // Include every accepted POSITION in the USN calculation. Changed
            // bytes were re-registered above, so their old stamps cannot clear.
            let accepted = &captured[..processed];
            let usn = self.guarded_local(|| self.store().commit_media_upload_with_boundary(self.baseline(), accepted, usn, &mut self.effect_boundary()))?;
            self.advanced(usn);
            self.snapshot.totals.uploaded_files += accepted.iter().filter(|c| c.sha1.is_some()).count() as u64;
            self.snapshot.totals.uploaded_deletions += accepted.iter().filter(|c| c.sha1.is_none()).count() as u64;
            rebuilds = 0;
            self.emit(MediaPhase::Uploading);
        }
    }
}

fn decide(change: &wire::MediaChange, entry: Option<&MediaEntry>) -> Option<MediaPageDecision> {
    if change.sha1.is_empty() {
        entry
            .filter(|e| {
                matches!(e.sync_state, MediaSyncState::Deleted | MediaSyncState::Awaiting)
                    || (e.sync_state == MediaSyncState::Live && !e.dirty)
            })
            .map(|e| MediaPageDecision::Remove {
                name: change.name.clone(),
                stamp: e.change_counter,
            })
    } else if let Some(entry) =
        entry.filter(|e| e.sync_state == MediaSyncState::Live && e.sha1.as_ref() == Some(&change.sha1))
    {
        Some(MediaPageDecision::Clean {
            name: change.name.clone(),
            stamp: entry.change_counter,
        })
    } else {
        Some(MediaPageDecision::Download {
            name: change.name.clone(),
            sha1: change.sha1.clone(),
            usn: change.usn as u64,
            stamp: entry.map(|e| e.change_counter),
        })
    }
}
fn positive_usn(usn: i32, method: Method) -> Result<u64, JobError> {
    u64::try_from(usn).map_err(|_| JobError::bad_response(method, "negative media USN"))
}
fn digest(hash: &str) -> Option<[u8; 20]> {
    if hash.len() != 40 || !hash.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let mut result = [0; 20];
    for (i, pair) in hash.as_bytes().as_chunks::<2>().0.iter().enumerate() {
        let digit = |b: u8| if b.is_ascii_digit() { b - b'0' } else { b - b'a' + 10 };
        result[i] = digit(pair[0]) * 16 + digit(pair[1]);
    }
    Some(result)
}
/// One spelling inventory per unchanged directory, shared across bounded
/// upload batches. Namespace changes invalidate it; in-place edits still get
/// current descriptor/path identity and journal-stamp checks for every file.
/// For N files and M pending changes: expected O(N + M) spelling work without
/// namespace churn, O(N) names; each observed namespace change costs O(N).
#[derive(Default)]
struct UploadDirectory {
    stamp: Option<Metadata>,
    names: std::collections::HashSet<std::ffi::OsString>,
}
impl UploadDirectory {
    fn current(&self, store: &MediaStore) -> Result<bool, JobError> {
        let after = std::fs::metadata(store.media_dir()).map_err(|e| storage_error(e.into()))?;
        Ok(self.stamp.as_ref().is_some_and(|before| same_identity(before, &after)))
    }
    fn observe(&mut self, store: &MediaStore) -> Result<(), JobError> {
        if self.current(store)? {
            return Ok(());
        }
        for _ in 0..REBUILD_LIMIT {
            let before = std::fs::metadata(store.media_dir()).map_err(|e| storage_error(e.into()))?;
            let mut names = std::collections::HashSet::new();
            meguri_model::faultpoint!("media.sync.upload.spelling_listing").map_err(|e| storage_error(e.into()))?;
            for entry in std::fs::read_dir(store.media_dir()).map_err(|e| storage_error(e.into()))? {
                let entry = entry.map_err(|e| storage_error(e.into()))?;
                // The per-file source check below decides regularness. Saving
                // every spelling also lets nonregular replacements invalidate
                // a previous exact regular source without a second listing.
                names.insert(entry.file_name());
            }
            let after = std::fs::metadata(store.media_dir()).map_err(|e| storage_error(e.into()))?;
            if same_identity(&before, &after) {
                self.names = names;
                self.stamp = Some(after);
                return Ok(());
            }
        }
        Err(local_failure("upload directory kept changing"))
    }
    fn source(&mut self, store: &MediaStore, name: &str) -> Result<Option<(std::fs::File, Metadata)>, JobError> {
        self.observe(store)?;
        if !self.names.contains(std::ffi::OsStr::new(name)) {
            return Ok(None);
        }
        let path = store.media_dir().join(name);
        let before = match std::fs::symlink_metadata(&path) {
            Ok(m) if m.is_file() => m,
            Ok(_) => return Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(e) => return Err(storage_error(e.into())),
        };
        let opened = local(|| store.open_file(name));
        // A namespace race may let lookup choose a case variant. Never use
        // that descriptor as evidence for the captured exact spelling.
        if !self.current(store)? {
            return Ok(None);
        }
        let Some(file) = opened? else { return Ok(None) };
        let metadata = file.metadata().map_err(|e| storage_error(e.into()))?;
        match std::fs::symlink_metadata(&path) {
            Ok(after)
                if after.is_file()
                    && same_identity(&before, &after)
                    && same_identity(&metadata, &after)
                    && self.current(store)? =>
            {
                Ok(Some((file, metadata)))
            }
            Ok(_) => Ok(None),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(storage_error(e.into())),
        }
    }
}
fn same_identity(a: &Metadata, b: &Metadata) -> bool {
    a.dev() == b.dev()
        && a.ino() == b.ino()
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}
fn owned_staging(store: &MediaStore) -> Result<tempfile::TempDir, JobError> {
    meguri_model::faultpoint!("media.sync.before_staging").map_err(|e| storage_error(e.into()))?;
    meguri_storage::leftovers::staging_directory(store.staging_dir()).map_err(|e| storage_error(e.into()))
}
fn local<T>(work: impl FnOnce() -> Result<T, MediaError>) -> Result<T, JobError> {
    runtime::blocking_work(work).map_err(storage_error)
}
fn storage_error(error: MediaError) -> JobError {
    match error {
        MediaError::Cancelled => cancelled(),
        MediaError::DiskFull => JobError::new(
            ErrorKind::DiskFull {
                needed_bytes: None,
                available_bytes: None,
            },
            None,
            "media storage is full",
        ),
        _ => local_failure("media file or journal operation failed"),
    }
}
fn local_failure(detail: &'static str) -> JobError {
    JobError::new(ErrorKind::LocalStorage, None, detail)
}
fn state_changed() -> JobError {
    JobError::new(ErrorKind::Interrupted, None, "media owner, epoch or cursor changed")
}
fn stopped(cancel: &CancelToken) -> JobError {
    match cancel.stop_cause() {
        Some(StopCause::System) => JobError::new(ErrorKind::Interrupted, None, "media stage stopped by lifecycle"),
        _ => cancelled(),
    }
}
fn cancelled() -> JobError {
    JobError::new(ErrorKind::Cancelled, None, "media stage cancelled")
}
fn engine_error(error: EngineError) -> JobError {
    let kind = match error {
        EngineError::Busy { .. } => ErrorKind::ExclusiveBusy,
        EngineError::Cancelled => ErrorKind::Cancelled,
        EngineError::Media(MediaError::DiskFull) => ErrorKind::DiskFull {
            needed_bytes: None,
            available_bytes: None,
        },
        EngineError::StaleGeneration { .. } | EngineError::CollectionUnavailable { .. } => ErrorKind::Interrupted,
        _ => ErrorKind::LocalStorage,
    };
    JobError::new(kind, None, "media service admission or lifecycle check failed")
}
