//! Same-operation media admission. No collection preparation or replacement.
use super::*;
use crate::media::{MediaSnapshot, MediaSyncResult};
use meguri_engine::{StopCause, media::MediaService};

#[derive(Clone, Debug)]
pub struct MediaConfirmation {
    id: OperationId,
    revision: u64,
    generation: u64,
    publication: meguri_engine::device_state::PublicationId,
    categories: u32,
    account: AccountRef,
    configured_endpoint: String,
}
impl MediaConfirmation {
    pub fn operation_id(&self) -> &OperationId {
        &self.id
    }
}
#[derive(Clone, Debug)]
pub struct MediaPreview {
    pub operation: Operation,
    pub confirmation: MediaConfirmation,
}
impl ManualTransfers {
    fn media_operation(&self, id: &OperationId, account: &AccountRef, configured: &str) -> Result<Operation> {
        let mut op = self.policy.get(id)?;
        if self.policy.active()?.as_ref().map(|a| &a.id) != Some(id)
            || op.scope != Scope::CollectionAndMedia
            || !op.collection_complete
            || op.receipt.is_some()
            || matches!(op.phase, Phase::Complete | Phase::Superseded)
        {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        if op.acceptance == Acceptance::PossiblyAccepted
            || (op.direction == Direction::Export && op.acceptance != Acceptance::ExactOk)
        {
            return Err(TransferError::NeedsDecision(Boundary::RemoteCompletionUnknown));
        }
        if op.account != *account || op.configured_endpoint != configured {
            return Err(TransferError::Stale(Boundary::IdentityChanged));
        }
        let current = DeviceState::for_collection(&self.live())
            .publication()?
            .map(|p| p.identity);
        // Every admission, including a saved identity, needs the full durable
        // R6 proof of THIS activation, binding and publication decision. Reviews
        // may change live bytes; neither their hash nor a process counter proves
        // or disproves this activation across restart.
        if !self.activation_proven(&op)? {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        if op.resulting_publication.is_none() && current.is_some() {
            op.resulting_publication = current;
        }
        if op.resulting_publication.is_none() || op.resulting_publication != current {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        self.inputs.engine.collection()?;
        Ok(op)
    }
    /// Callback fencing is independent of a reusable process generation. The
    /// reader lease keeps publication fixed through this local-only CAS/write.
    fn with_media_authority<T>(&self, op: &Operation, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let collection = self.inputs.engine.collection()?;
        let _reader = collection.reader(meguri_storage::ReaderRole::Query)?;
        let current = self.policy.get(&op.id)?;
        if self.policy.active()?.as_ref().map(|a| &a.id) != Some(&op.id)
            || current.revision != op.revision
            || current.resulting_publication != op.resulting_publication
            || matches!(current.phase, Phase::Complete | Phase::Superseded)
            || op.resulting_publication.is_none()
            || DeviceState::for_collection(collection.path())
                .publication()?
                .map(|p| p.identity)
                != op.resulting_publication
        {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        action()
    }
    fn save_media(&self, op: &mut Operation) -> Result<()> {
        let captured = op.clone();
        self.with_media_authority(&captured, || Ok(self.policy.save(op)?))
    }
    /// Offline category preview. Credentials prove only the captured identity;
    /// constructing this value issues zero requests and leaves media untouched.
    pub fn prepare_media(&self, id: &OperationId, access: &TransferAccess) -> Result<MediaPreview> {
        let _flight = Flight::acquire()?;
        let _admission = self.inputs.engine.acquire_slot(meguri_engine::LongOp::OneWaySync)?;
        let mut op = self.media_operation(id, &access.account, access.configured.as_str())?;
        op.media.stage = MediaStage::AwaitingConsent;
        op.media.consent = None;
        clear_exclusions(&mut op.media);
        // An explicit new decision may bind a reopened instance of the SAME
        // persistent R6 identity to its actual current engine generation.
        op.actual_generation = Some(self.inputs.engine.collection()?.generation().0);
        // Retain recorded effects; current exclusions require a fresh check.
        self.policy.save(&mut op)?;
        Ok(MediaPreview {
            confirmation: MediaConfirmation {
                id: op.id.clone(),
                revision: op.revision,
                generation: op.actual_generation.ok_or(TransferError::Unavailable)?,
                publication: op.resulting_publication.ok_or(TransferError::Unavailable)?,
                categories: MEDIA_CATEGORIES,
                account: op.account.clone(),
                configured_endpoint: op.configured_endpoint.clone(),
            },
            operation: op,
        })
    }
    /// Cancel this prepared permission decision offline. Collection completion,
    /// selected scope and every recorded media effect remain available to Resume.
    pub fn cancel_prepared_media(&self, confirmation: MediaConfirmation) -> Result<Operation> {
        let _flight = Flight::acquire()?;
        let _admission = self.inputs.engine.acquire_slot(meguri_engine::LongOp::OneWaySync)?;
        let mut op = self.media_operation(
            &confirmation.id,
            &confirmation.account,
            &confirmation.configured_endpoint,
        )?;
        if op.revision != confirmation.revision
            || op.resulting_publication != Some(confirmation.publication)
            || op.actual_generation != Some(confirmation.generation)
            || self.inputs.engine.collection()?.generation().0 != confirmation.generation
            || confirmation.categories != MEDIA_CATEGORIES
            || op.media.stage != MediaStage::AwaitingConsent
        {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        // Declined is the durable permission outcome, distinct from stopping an
        // authorized media worker. Do not finish the operation or clear its facts.
        op.media.stage = MediaStage::Declined;
        op.phase = Phase::MediaIncomplete;
        self.policy.save(&mut op)?;
        Ok(op)
    }
    /// Called only by the separate MediaSync host job after collection drain and
    /// native adoption. The reconciler owns the engine-bound media lease/drain.
    pub fn run_media(
        &self,
        mut access: TransferAccess,
        confirmation: MediaConfirmation,
        service: &MediaService,
        progress: &ProgressSink,
        mut snapshot: impl FnMut(&MediaSnapshot),
    ) -> Result<Operation> {
        let _flight = Flight::acquire()?;
        let _admission = self.inputs.engine.acquire_slot(meguri_engine::LongOp::OneWaySync)?;
        let mut op = self.media_operation(&confirmation.id, &access.account, access.configured.as_str())?;
        if op.revision != confirmation.revision
            || op.resulting_publication != Some(confirmation.publication)
            || confirmation.categories != MEDIA_CATEGORIES
            || op.actual_generation != Some(confirmation.generation)
            || self.inputs.engine.collection()?.generation().0 != confirmation.generation
            || op.media.stage != MediaStage::AwaitingConsent
        {
            return Err(TransferError::Stale(Boundary::LocalChanged));
        }
        service.check_generation(confirmation.generation)?;
        if let Some(cause) = access.options.cancel.stop_cause() {
            return Err(match cause {
                StopCause::User => TransferError::Cancelled,
                StopCause::System => TransferError::Interrupted,
            });
        }
        op.media.consent = Some(MediaConsent {
            revision: confirmation.revision,
            generation: confirmation.generation,
            categories: confirmation.categories,
            account: op.account.clone(),
            configured_endpoint: op.configured_endpoint.clone(),
        });
        op.media.stage = MediaStage::Running;
        let historical_effects_unknown = op.media.remote_outcome_unknown;
        op.media.remote_outcome_unknown = true;
        clear_exclusions(&mut op.media);
        op.media.failure_kind = None;
        op.phase = Phase::MediaIncomplete;
        op.boundary = None;
        self.save_media(&mut op)?;
        drop(_admission);
        // Continue at the destination approved and recorded during collection
        // preparation, preserving configured identity through that redirect.
        let target =
            Endpoint::parse(&op.target_endpoint).map_err(|_| TransferError::Stale(Boundary::IdentityChanged))?;
        if target != access.configured {
            let approved = crate::follow_redirect(&access.configured, ServerKind::SelfHosted, target.as_str())
                .map_err(|_| TransferError::Stale(Boundary::IdentityChanged))?;
            if approved != target {
                return Err(TransferError::Stale(Boundary::IdentityChanged));
            }
        }
        access.options.base = target;
        let prior = op.media.counters.clone();
        let owner = op.account.media_owner();
        let mut job = access.job(self.inputs.random.as_ref(), progress)?;
        let mut callback_error = None;
        let mut durable_snapshot = op.clone();
        let cancel = access.options.cancel.clone();
        let outcome = crate::media::reconcile(
            self.inputs.engine.threads(),
            &mut job,
            service,
            confirmation.generation,
            &owner,
            &mut |value| {
                op.media.counters = accumulated(&prior, &value.totals);
                op.media.exclusions_verified = value.phase != crate::media::MediaPhase::Checking;
                if !op.media.exclusions_verified {
                    clear_exclusions(&mut op.media);
                }
                // Checking can report once per file; the media journal owns
                // recovery. Persist category/batch snapshots, not 28k sidecars.
                if callback_error.is_some() {
                    return;
                }
                let captured = op.clone();
                let saved = self.with_media_authority(&captured, || {
                    if value.phase != crate::media::MediaPhase::Checking {
                        self.policy.save_snapshot(&mut op, &mut durable_snapshot)?;
                    }
                    Ok(())
                });
                if let Err(error) = saved {
                    callback_error = Some(error);
                    cancel.cancel();
                    return;
                }
                // Storage and lifecycle locks have been released before delivery.
                snapshot(value);
            },
        );
        if let Some(error) = callback_error {
            return Err(error);
        }
        match outcome {
            Ok(totals) => {
                op.media.counters = accumulated(&prior, &totals);
                op.media.stage = MediaStage::Complete;
                // A fresh reconciliation proves completion, not how many
                // earlier effects were accepted without a saved answer.
                op.media.remote_outcome_unknown = historical_effects_unknown;
                op.media.exclusions_verified = true;
                // Completion evidence must be durable BEFORE the receipt transition.
                self.save_media(&mut op)?;
                let captured = op.clone();
                self.with_media_authority(&captured, || {
                    Ok(self.policy.finish(&mut op, self.inputs.clock.now_ms())?)
                })?;
            }
            Err(failure) => {
                op.media.counters = accumulated(&prior, &failure.snapshot.totals);
                op.media.exclusions_verified = failure.snapshot.phase != crate::media::MediaPhase::Checking;
                if !op.media.exclusions_verified {
                    clear_exclusions(&mut op.media);
                }
                let kind = failure.error.kind;
                op.media.stage = match kind {
                    crate::ErrorKind::Cancelled => MediaStage::Cancelled,
                    crate::ErrorKind::Interrupted => MediaStage::Interrupted,
                    _ => MediaStage::Failed,
                };
                op.boundary = Some(match op.media.stage {
                    MediaStage::Interrupted => Boundary::Interrupted,
                    MediaStage::Cancelled => Boundary::Cancelled,
                    _ => Boundary::Server,
                });
                op.media.failure_kind = Some(if op.media.stage == MediaStage::Interrupted {
                    "interrupted".into()
                } else {
                    kind.id().into()
                });
                self.save_media(&mut op)?;
            }
        }
        Ok(op)
    }
}
fn accumulated(prior: &MediaCounters, now: &MediaSyncResult) -> MediaCounters {
    MediaCounters {
        uploaded_files: prior.uploaded_files.saturating_add(now.uploaded_files),
        uploaded_deletions: prior.uploaded_deletions.saturating_add(now.uploaded_deletions),
        downloaded_files: prior.downloaded_files.saturating_add(now.downloaded_files),
        downloaded_deletions: prior.downloaded_deletions.saturating_add(now.downloaded_deletions),
        checked: prior.checked.saturating_add(now.checked),
        bytes_downloaded: prior.bytes_downloaded.saturating_add(now.bytes_downloaded),
        bytes_uploaded: prior.bytes_uploaded.saturating_add(now.bytes_uploaded),
        resynced: prior.resynced || now.resynced,
        oversized: now.oversized.clone(),
        empty: now.empty.clone(),
        awaiting: now.awaiting,
    }
}

// These are current-check facts, never a union of old exclusions. An interrupted
// scan cannot claim an empty result or that yesterday's exclusions still apply.
fn clear_exclusions(outcome: &mut MediaOutcome) {
    outcome.exclusions_verified = false;
    outcome.counters.oversized.clear();
    outcome.counters.empty.clear();
}
