//! Test-only observations; no fixture construction, writes or SDK admission.
use crate::{CoreError, runtime, sync::cloud_error};
use meguri_storage::{ReadRows, ReaderRole, RowKey};
use meguri_sync::cloud::selected_observation as selected;
use meguri_sync::cloud::{entity_codec as c, row_payload};

#[derive(Clone, Copy, uniffi::Enum)]
pub enum CloudSelectedMissing {
    Unbound,
    CurrentCollectionChanged,
    AccountUnavailable,
    BindingUnusable,
    OperationInProgress,
    ActiveUnavailable,
    ControlsMismatch,
    MissingAfterRestart,
    PublicationNotCommitted,
    AdoptionNotActivated,
    ActivationUnavailable,
    OwnedBackupUnavailable,
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum CloudSelectedProof {
    Proven,
    Missing { reason: CloudSelectedMissing },
}
impl From<selected::Proof> for CloudSelectedProof {
    fn from(v: selected::Proof) -> Self {
        match v {
            selected::Proof::Proven => Self::Proven,
            selected::Proof::Missing(reason) => Self::Missing {
                reason: match reason {
                    selected::Missing::Unbound => CloudSelectedMissing::Unbound,
                    selected::Missing::CurrentCollectionChanged => CloudSelectedMissing::CurrentCollectionChanged,
                    selected::Missing::AccountUnavailable => CloudSelectedMissing::AccountUnavailable,
                    selected::Missing::BindingUnusable => CloudSelectedMissing::BindingUnusable,
                    selected::Missing::OperationInProgress => CloudSelectedMissing::OperationInProgress,
                    selected::Missing::ActiveUnavailable => CloudSelectedMissing::ActiveUnavailable,
                    selected::Missing::ControlsMismatch => CloudSelectedMissing::ControlsMismatch,
                    selected::Missing::MissingAfterRestart => CloudSelectedMissing::MissingAfterRestart,
                    selected::Missing::PublicationNotCommitted => CloudSelectedMissing::PublicationNotCommitted,
                    selected::Missing::AdoptionNotActivated => CloudSelectedMissing::AdoptionNotActivated,
                    selected::Missing::ActivationUnavailable => CloudSelectedMissing::ActivationUnavailable,
                    selected::Missing::OwnedBackupUnavailable => CloudSelectedMissing::OwnedBackupUnavailable,
                },
            },
        }
    }
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum CloudSelectedProvenance {
    CurrentAcceptedActive,
    RuntimeAcceptedPublication,
    RetainedAdoption,
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum CloudSelectedReplyOrigin {
    Saved,
    Conflict,
}
#[derive(Clone, Copy, uniffi::Enum)]
pub enum CloudSelectedPublicationKind {
    CompleteEpoch,
    DoneActive,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudSelectedPublicationEvent {
    pub origin: CloudSelectedReplyOrigin,
    pub attempt: u64,
    pub kind: CloudSelectedPublicationKind,
    pub correlation: Vec<u8>,
}
impl TryFrom<selected::PublicationEvent> for CloudSelectedPublicationEvent {
    type Error = ();
    fn try_from(v: selected::PublicationEvent) -> Result<Self, Self::Error> {
        Ok(Self {
            origin: match v.origin {
                selected::ReplyOrigin::Saved => CloudSelectedReplyOrigin::Saved,
                selected::ReplyOrigin::Conflict => CloudSelectedReplyOrigin::Conflict,
            },
            attempt: v.attempt,
            kind: match v.kind {
                meguri_sync::cloud::publication::OwnKind::SaveCompleteEpoch => {
                    CloudSelectedPublicationKind::CompleteEpoch
                }
                meguri_sync::cloud::publication::OwnKind::SaveActive => CloudSelectedPublicationKind::DoneActive,
                _ => return Err(()),
            },
            correlation: v.correlation.to_vec(),
        })
    }
}
#[derive(Clone, uniffi::Record)]
pub struct CloudSelectedControlObservation {
    pub generation: u64,
    pub status: CloudSelectedProof,
    pub provenance: Option<CloudSelectedProvenance>,
    pub publication_complete: Option<CloudSelectedPublicationEvent>,
    pub publication_done_active: Option<CloudSelectedPublicationEvent>,
    pub active_sha256: Option<Vec<u8>>,
    pub epoch_sha256: Option<Vec<u8>>,
    pub binding_matches_epoch: CloudSelectedProof,
    pub activation: CloudSelectedProof,
    pub owned_backup: CloudSelectedProof,
    pub required_scan_complete: bool,
}

pub fn selected_control() -> Result<CloudSelectedControlObservation, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("selected runtime absent"))?;
    let collection = host.collection()?;
    let generation = collection.generation();
    let observed = core.selected_control_for_testing(&collection).map_err(cloud_error)?;
    let status = core.status().map_err(cloud_error)?;
    let scan = core.note_scan_proof();
    let evidence = collection.open_evidence()?;
    // Whole/note scan coverage belongs to this open generation and native file.
    // Durable scan_drained is deliberately absent: it describes suspend.
    let current_owner = matches!(
        observed.status,
        selected::Proof::Proven | selected::Proof::Missing(selected::Missing::OperationInProgress)
    );
    let scan_complete = current_owner
        && status.stored_usable
        && status.identity_verified
        && !status.scan_required
        && !status.scan_running
        && !status.tracking_stopped
        && status.scan_completed_this_open
        && status.note_scan_completed_this_open
        && status.generation == Some(generation)
        && scan.as_ref().is_some_and(|p| {
            p.snapshot_generation == generation
                && (p.snapshot_native_file.device, p.snapshot_native_file.inode) == evidence.native_file
                && p.receipt.identity().generation() == generation
                && p.receipt.identity().native_file() == evidence.native_file
                && p.receipt.through() >= p.snapshot_seq
        });
    let final_status = core.status().map_err(cloud_error)?;
    let final_scan = core.note_scan_proof();
    if !collection.is_current()
        || collection.open_evidence()? != evidence
        || core.selected_control_for_testing(&collection).map_err(cloud_error)? != observed
        || final_status.generation != status.generation
        || final_status.scan_required != status.scan_required
        || final_status.scan_running != status.scan_running
        || final_status.scan_completed_this_open != status.scan_completed_this_open
        || final_status.note_scan_completed_this_open != status.note_scan_completed_this_open
        || final_status.tracking_stopped != status.tracking_stopped
        || final_status.stored_usable != status.stored_usable
        || final_status.identity_verified != status.identity_verified
        || scan
            .as_ref()
            .map(|p| (p.snapshot_generation, p.snapshot_native_file, p.snapshot_seq, p.receipt))
            != final_scan
                .as_ref()
                .map(|p| (p.snapshot_generation, p.snapshot_native_file, p.snapshot_seq, p.receipt))
    {
        return Err(cloud_error("selected current owner changed"));
    }
    Ok(CloudSelectedControlObservation {
        generation: generation.0,
        status: observed.status.into(),
        provenance: observed.provenance.map(|p| match p {
            selected::Provenance::CurrentAcceptedActive => CloudSelectedProvenance::CurrentAcceptedActive,
            selected::Provenance::RuntimeAcceptedPublication => CloudSelectedProvenance::RuntimeAcceptedPublication,
            selected::Provenance::RetainedAdoption => CloudSelectedProvenance::RetainedAdoption,
        }),
        publication_complete: observed.publication_complete.and_then(|v| v.try_into().ok()),
        publication_done_active: observed.publication_done_active.and_then(|v| v.try_into().ok()),
        active_sha256: observed.active_sha256.map(|v| v.to_vec()),
        epoch_sha256: observed.epoch_sha256.map(|v| v.to_vec()),
        binding_matches_epoch: observed.binding_matches_epoch.into(),
        activation: observed.activation.into(),
        owned_backup: observed.owned_backup.into(),
        required_scan_complete: scan_complete,
    })
}

#[derive(Clone, uniffi::Record)]
pub struct SmokeIntegrityObservation {
    pub generation: u64,
    pub seq: u64,
    pub integrity_ok: bool,
    pub schema_ok: bool,
    pub reference_failures: u64,
}
pub fn integrity() -> Result<SmokeIntegrityObservation, CoreError> {
    let collection = runtime::host()?.collection()?;
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let r = snapshot.smoke_integrity()?;
    Ok(SmokeIntegrityObservation {
        generation: snapshot.generation().0,
        seq: snapshot.seq().0,
        integrity_ok: r.integrity_ok,
        schema_ok: r.schema_ok,
        reference_failures: r.missing_card_notes
            + r.missing_card_decks
            + r.missing_note_types
            + r.missing_field_types
            + r.wrong_field_counts,
    })
}

#[derive(Clone, uniffi::Record)]
pub struct CloudSmokeEntity {
    pub label: String,
    pub record_name: String,
    pub revision: u64,
    pub acked_revision: u64,
    pub digest: Vec<u8>,
    pub acked_envelope_digest: Vec<u8>,
    pub acknowledged: bool,
    pub media_bytes: Option<u64>,
    pub media_sha256: Option<Vec<u8>>,
    pub blob_present: bool,
    pub media_change_counter: Option<u64>,
}
#[derive(Clone, uniffi::Record)]
pub struct CloudSmokeObservation {
    pub generation: u64,
    pub active_session: Option<u64>,
    pub seq: u64,
    pub publication_id: Option<Vec<u8>>,
    pub publication_reason: Option<u8>,
    pub publication_completing: bool,
    pub publication_binding_id: Option<Vec<u8>>,
    pub binding_id: Option<Vec<u8>>,
    pub publication_initial: bool,
    pub publication_complete: bool,
    pub binding_usable: bool,
    pub engine: Option<String>,
    pub entities: Vec<CloudSmokeEntity>,
    pub card_note_id: Option<i64>,
    pub last_review_id: Option<i64>,
}

#[derive(Clone, uniffi::Record)]
pub struct SmokeReviewImage {
    pub name: String,
    pub envelope_digest: Option<Vec<u8>>,
    pub digest: Option<Vec<u8>>,
    pub image_seq: u64,
    pub held: bool,
}
impl From<meguri_sync::cloud::workflow::SmokeReviewImage> for SmokeReviewImage {
    fn from(i: meguri_sync::cloud::workflow::SmokeReviewImage) -> Self {
        Self {
            name: i.name,
            envelope_digest: i.envelope_digest.map(|d| d.to_vec()),
            digest: i.digest.map(|d| d.to_vec()),
            image_seq: i.image_seq,
            held: i.held,
        }
    }
}
#[derive(Clone, uniffi::Record)]
pub struct SmokeBatchProof {
    pub generation: u64,
    pub seq: u64,
    pub session: Option<u64>,
    pub saves: u64,
    pub images: Vec<SmokeReviewImage>,
    pub save_images: Vec<SmokeReviewImage>,
}
#[derive(uniffi::Record)]
pub struct CloudSmokeHistoricalAck {
    pub sdk: String,
    pub name: String,
    pub envelope_digest: Vec<u8>,
    pub image_seq: u64,
    pub held: bool,
}
pub fn acknowledgements(callback: Option<(&str, u64)>) -> Result<Vec<CloudSmokeHistoricalAck>, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("cloud fixture unavailable"))?;
    Ok(core
        .smoke_acknowledgements_for_testing(callback)
        .map_err(cloud_error)?
        .into_iter()
        .map(|ack| CloudSmokeHistoricalAck {
            sdk: ack.sdk,
            name: ack.name,
            envelope_digest: ack.envelope_digest.to_vec(),
            image_seq: ack.image_seq,
            held: ack.held,
        })
        .collect())
}
impl From<meguri_sync::cloud::workflow::SmokeBatchProof> for SmokeBatchProof {
    fn from(v: meguri_sync::cloud::workflow::SmokeBatchProof) -> Self {
        Self {
            generation: v.generation,
            seq: v.seq,
            session: v.session,
            saves: v.saves,
            images: v.images.into_iter().map(Into::into).collect(),
            save_images: v.save_images.into_iter().map(Into::into).collect(),
        }
    }
}
pub fn batch(sdk: String, id: u64) -> Result<SmokeBatchProof, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("smoke runtime absent"))?;
    core.validate_sdk(&sdk).map_err(cloud_error)?;
    Ok(core.smoke_batch_for_testing(&sdk, id).map_err(cloud_error)?.into())
}
pub fn review(sdk: String, card: i64, review: i64) -> Result<SmokeBatchProof, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("smoke runtime absent"))?;
    core.validate_sdk(&sdk).map_err(cloud_error)?;
    let collection = host.collection()?;
    Ok(core
        .smoke_review_for_testing(&sdk, &collection, card, review)
        .map_err(cloud_error)?
        .into())
}

pub fn observe(
    note_id: Option<i64>,
    card_id: Option<i64>,
    revlog_id: Option<i64>,
    media_name: Option<String>,
) -> Result<CloudSmokeObservation, CoreError> {
    let host = runtime::host()?;
    let core = host
        .cloud_runtime()
        .ok_or_else(|| cloud_error("smoke cloud runtime absent"))?;
    let collection = host.collection()?;
    let media = if let Some(name) = &media_name {
        // A stored media basename only; never a caller-selected file or query.
        if name.is_empty()
            || name.len() > 255
            || name.contains('/')
            || name.contains('\\')
            || name.contains('\0')
            || name == "."
            || name == ".."
        {
            return Err(cloud_error("invalid smoke media basename"));
        }
        // Opening the service needs the Query slot; do it before retaining its lease.
        Some(host.media()?)
    } else {
        None
    };
    let lease = collection.reader(ReaderRole::Query)?;
    let snapshot = lease.snapshot()?;
    let status = core.status().map_err(cloud_error)?;
    let op = core.publication_op().map_err(cloud_error)?;
    let projection = core
        .policy_projection(meguri_sync::cloud::policy::Lifecycle::Ready, snapshot.generation().0)
        .map_err(cloud_error)?;
    let engine = projection.engine.filter(|e| !e.stopping).map(|e| e.instance);
    if let Some(engine) = &engine {
        core.validate_sdk(engine).map_err(cloud_error)?;
    }
    let mut result = CloudSmokeObservation {
        active_session: host.review().active_session().map(|s| s.0),
        generation: snapshot.generation().0,
        seq: snapshot.seq().0,
        publication_id: op.as_ref().map(|p| p.id.to_vec()),
        publication_reason: op.as_ref().map(|p| p.reason),
        publication_completing: op.as_ref().is_some_and(|p| {
            p.step == meguri_sync::cloud::publication::PublishStep::Complete
                && p.complete_system_fields.is_some()
                && p.root_acked
        }),
        publication_binding_id: op.as_ref().and_then(|p| p.binding_after.map(|b| b.0.to_vec())),
        binding_id: status.binding.as_ref().map(|b| b.id.0.to_vec()),
        publication_initial: op
            .as_ref()
            .is_some_and(|p| p.reason == 1 && p.active.is_none() && p.active_epoch.is_none()),
        publication_complete: op.is_none()
            && status
                .binding
                .as_ref()
                .is_some_and(|b| b.facts.state == meguri_sync::cloud::BindingState::Active)
            && core.sdk_root_confirmed(),
        binding_usable: status.stored_usable && status.identity_verified,
        engine,
        entities: Vec::new(),
        card_note_id: card_id
            .map(|id| snapshot.card(meguri_model::ids::CardId(id)))
            .transpose()?
            .flatten()
            .map(|c| c.note_id.0),
        last_review_id: card_id
            .map(|id| snapshot.max_revlog_id(meguri_model::ids::CardId(id)))
            .transpose()?,
    };
    if note_id.is_none() && card_id.is_none() && revlog_id.is_none() && media_name.is_none() {
        return Ok(result);
    }
    let binding = status.binding.ok_or_else(|| cloud_error("smoke mutation is unbound"))?;
    let notetypes = core.entity_facts_page(c::Kind::Notetype, None).map_err(cloud_error)?;
    let blobs = core.media_blobs_for_testing().map_err(cloud_error)?;
    let mut keys = Vec::new();
    for (label, kind, id) in [
        ("n1", c::Kind::Note, note_id),
        ("c1", c::Kind::Card, card_id),
        ("r1", c::Kind::Revlog, revlog_id),
    ] {
        if let Some(id) = id {
            keys.push((label, kind, c::id_key(id).to_vec(), Some(id)));
        }
    }
    if let Some(name) = &media_name {
        keys.push(("m1", c::Kind::MediaName, name.to_lowercase().into_bytes(), None));
    }
    for (label, kind, key, id) in keys {
        let name = c::entity_name(&binding.facts.record_key.0, kind, &key).map_err(cloud_error)?;
        let e = core
            .entity_facts(&name)
            .map_err(cloud_error)?
            .ok_or_else(|| cloud_error("smoke entity not captured"))?;
        let payload = if let Some(id) = id {
            let row = snapshot
                .row_image(
                    row_payload::table(kind).ok_or_else(|| cloud_error("smoke row kind"))?,
                    &RowKey::Id(id),
                )?
                .ok_or_else(|| cloud_error("smoke row absent"))?;
            row_payload::project(&row_payload::ReadView(&*snapshot), kind, &row, |ntid| {
                notetypes
                    .iter()
                    .find_map(|t| match &t.payload {
                        c::Payload::Notetype(nt) if nt.id == ntid => Some(nt.clone()),
                        _ => None,
                    })
                    .ok_or(meguri_sync::cloud::Error::InvalidData)
            })
            .map_err(cloud_error)?
        } else {
            e.payload.clone()
        };
        if let c::Payload::Revlog(review) = &payload
            && (Some(review.cid) != card_id || review.ease != 3)
        {
            return Err(cloud_error("smoke review row does not match the Good answer"));
        }
        let digest = payload.payload_digest().map_err(cloud_error)?;
        if e.payload.payload_digest().map_err(cloud_error)? != digest || e.tombstone_at.is_some() {
            return Err(cloud_error("smoke tracked payload differs from actual row"));
        }
        let (media_bytes, media_sha256, blob_present) = match &payload {
            c::Payload::MediaName(m) => {
                let blob = meguri_sync::cloud::media::blob::Blob::from(m);
                // Persisted Present is set only after validated blob metadata or
                // an actual successful upload; selection/queue admission cannot set it.
                let present = blobs
                    .values()
                    .any(|b| b.metadata == blob && b.server == meguri_sync::cloud::media::ServerState::Present);
                (Some(m.size), Some(m.sha256.to_vec()), present)
            }
            _ => (None, None, false),
        };
        let media_change_counter =
            if let Some((name, media)) = media_name.as_ref().filter(|_| label == "m1").zip(media.as_ref()) {
                let feed = media.store().media_feed_snapshot().map_err(cloud_error)?;
                let item = feed
                    .items
                    .iter()
                    .find(|i| &i.name == name)
                    .ok_or_else(|| cloud_error("smoke media journal fact absent"))?;
                if !feed.status.complete || !matches!(item.state, meguri_storage::media::MediaFileState::Present(_)) {
                    return Err(cloud_error("smoke media journal is incomplete"));
                }
                Some(item.change_counter)
            } else {
                None
            };
        result.entities.push(CloudSmokeEntity {
            label: label.into(),
            record_name: name,
            revision: e.local_rev,
            acked_revision: e.acked_rev,
            digest: digest.to_vec(),
            acked_envelope_digest: c::sha256(&e.acked_envelope.0).to_vec(),
            acknowledged: e.local_rev > 0
                && e.acked_rev == e.local_rev
                && e.acked_payload_digest == digest
                && e.mark.is_none()
                && e.in_flight.is_empty()
                && e.server_erev > 0
                && !e.acked_system_fields.is_empty()
                && !e.acked_envelope.0.is_empty(),
            media_bytes,
            media_sha256,
            blob_present,
            media_change_counter,
        });
    }
    Ok(result)
}
