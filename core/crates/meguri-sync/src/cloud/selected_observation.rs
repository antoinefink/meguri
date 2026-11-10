//! Fixed J3.2 inspection of the current owned collection. Test hooks only.
//! Nothing returned here admits a write or reconstructs a lost original reply.
use super::*;
use crate::cloud::{
    adoption::{AdoptOp, AdoptStep},
    control_codec::{Active, DecodeOutcome, Epoch, EpochState, MAX_CONTROL_BYTES},
    publication::{ControlRecord, OwnKind, OwnOperation, OwnRequest, OwnResult, PublishOp},
};
use meguri_engine::{
    device_state::{DeviceState, PublicationId},
    replace::ReplaceReason,
};
use sha2::{Digest, Sha256};
use std::io::{self, Write};

const MAX_PUBLICATION_OBSERVATION_BYTES: usize = 8 * MAX_CONTROL_BYTES;

// Only the fixed publication correlation uses this writer. An error discards
// the entire event, including the hash of any prefix already written.
#[derive(Default)]
struct PublicationHash {
    hash: Sha256,
    bytes: usize,
}
impl Write for PublicationHash {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        if bytes.len() > MAX_PUBLICATION_OBSERVATION_BYTES - self.bytes {
            return Err(io::Error::other("publication observation limit"));
        }
        self.hash.update(bytes);
        self.bytes += bytes.len();
        Ok(bytes.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Missing {
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Proof {
    Proven,
    Missing(Missing),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Provenance {
    /// Current Active ACK and adapter bytes; not immutable publication history.
    CurrentAcceptedActive,
    /// Original SaveCompleteEpoch reply, observed after its successful commit,
    /// joined to the subsequent successful publication_done Active transaction.
    RuntimeAcceptedPublication,
    /// Original Ready AdoptOp retained by end_adoption with activated_file.
    RetainedAdoption,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReplyOrigin {
    Saved,
    Conflict,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicationEvent {
    pub origin: ReplyOrigin,
    pub attempt: u64,
    pub kind: OwnKind,
    pub correlation: [u8; 32],
}
impl PublicationEvent {
    fn capture(request: &OwnRequest, record: &ControlRecord, origin: ReplyOrigin) -> Option<Self> {
        if !matches!(request.kind, OwnKind::SaveCompleteEpoch | OwnKind::SaveActive) {
            return None;
        }
        // serde_json scans a string before writing its unescaped span. Bound
        // every variable string first; byte sequences stop at the writer cap.
        let OwnOperation::Save {
            zone,
            name,
            record_type,
            ..
        } = &request.operation
        else {
            return None;
        };
        if [zone, name, record_type, &request.instance]
            .iter()
            .any(|s| s.len() > MAX_PUBLICATION_OBSERVATION_BYTES)
        {
            return None;
        }
        for decision in [&request.decision_before, &request.decision_current] {
            if let meguri_engine::device_state::AccountBinding::Connected {
                account,
                configuration,
                environment,
            } = &decision.publication.binding
                && [account, configuration, environment]
                    .iter()
                    .any(|s| s.len() > MAX_PUBLICATION_OBSERVATION_BYTES)
            {
                return None;
            }
        }
        let mut writer = PublicationHash::default();
        serde_json::to_writer(&mut writer, &(request, record, origin)).ok()?;
        Some(Self {
            origin,
            attempt: request.attempt,
            kind: request.kind,
            correlation: writer.hash.finalize().into(),
        })
    }
    /// Project the actual transport result, before Core accepts or rejects it.
    pub fn from_reply(request: &OwnRequest, reply: &OwnResult) -> Option<Self> {
        match reply {
            OwnResult::Saved(record) => Self::capture(request, record, ReplyOrigin::Saved),
            OwnResult::Conflict(record) => Self::capture(request, record, ReplyOrigin::Conflict),
            _ => None,
        }
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SelectedControlObservation {
    pub status: Proof,
    pub provenance: Option<Provenance>,
    pub publication_complete: Option<PublicationEvent>,
    pub publication_done_active: Option<PublicationEvent>,
    pub active_sha256: Option<[u8; 32]>,
    pub epoch_sha256: Option<[u8; 32]>,
    pub binding_matches_epoch: Proof,
    pub activation: Proof,
    pub owned_backup: Proof,
}
impl std::fmt::Debug for SelectedControlObservation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("SelectedControlObservation")
    }
}
impl SelectedControlObservation {
    fn missing(reason: Missing) -> Self {
        Self {
            status: Proof::Missing(reason),
            provenance: None,
            publication_complete: None,
            publication_done_active: None,
            active_sha256: None,
            epoch_sha256: None,
            binding_matches_epoch: Proof::Missing(reason),
            activation: Proof::Missing(reason),
            owned_backup: Proof::Missing(reason),
        }
    }
}

/// One bounded process-local observation. Never persisted, restored, or used by
/// production decisions. A retained Complete response alone proves no completion.
#[derive(Clone, PartialEq, Eq)]
pub(super) struct AcceptedPublication {
    op: [u8; 16],
    binding: BindingId,
    epoch: EpochId,
    record_key: RecordKey,
    file: NativeFileIdentity,
    generation: Generation,
    incarnation: u64,
    complete: Vec<u8>,
    complete_request: OwnRequest,
    complete_event: PublicationEvent,
    done_active: Option<(OwnRequest, PublicationEvent, Vec<u8>, [u8; 32])>,
}
impl AcceptedPublication {
    pub(super) fn new(
        op: &PublishOp,
        request: &OwnRequest,
        record: &ControlRecord,
        origin: ReplyOrigin,
        s: &State,
    ) -> Option<Self> {
        let bound = s.bound.as_ref()?;
        if request.op != op.id
            || request.kind != OwnKind::SaveCompleteEpoch
            || record.encrypted.as_ref()?.len() > MAX_CONTROL_BYTES
        {
            return None;
        }
        let complete_event = PublicationEvent::capture(request, record, origin)?;
        Some(Self {
            op: op.id,
            binding: op.binding_after?,
            epoch: op.epoch,
            record_key: op.record_key.clone(),
            file: op.confirmed_file,
            generation: bound.evidence.generation,
            incarnation: s.incarnation,
            complete: record.encrypted.as_ref()?.clone(),
            complete_request: request.clone(),
            complete_event,
            done_active: None,
        })
    }
    pub(super) fn mark_done(
        &mut self,
        op: &PublishOp,
        request: &OwnRequest,
        active: &ControlRecord,
        origin: ReplyOrigin,
    ) {
        if self.op == op.id
            && request.op == self.complete_request.op
            && request.incarnation == self.complete_request.incarnation
            && request.generation == self.complete_request.generation
            && request.instance == self.complete_request.instance
            && request.kind == OwnKind::SaveActive
            && Some(self.binding) == op.binding_after
            && self.epoch == op.epoch
            && self.record_key == op.record_key
            && self.file == op.confirmed_file
            && active.encrypted.as_ref().is_some_and(|v| v.len() <= MAX_CONTROL_BYTES)
            && let Some(event) = PublicationEvent::capture(request, active, origin)
        {
            self.done_active = active.encrypted.as_ref().map(|payload| {
                (
                    request.clone(),
                    event,
                    payload.clone(),
                    codec::sha256(&active.system_fields.0),
                )
            });
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
struct Snapshot {
    bound: Option<Bound>,
    incarnation: u64,
    invalidation: u64,
    stopped: bool,
    // These are private correlation facts, never serialized to the caller.
    binding: Option<Binding>,
    runtime: Option<StoredRuntimeState>,
    active: Option<super::super::workflow::Saved>,
    adoption: Option<(AdoptOp, Option<NativeFileIdentity>)>,
    live_operation: bool,
    collection_crt: Option<i64>,
    publication: Option<AcceptedPublication>,
}
impl Runtime {
    fn selected_snapshot(&self) -> Result<Snapshot, RuntimeError> {
        let s = self.state();
        with_tracking_access(|| {
            let store = s.store.as_ref().ok_or(Error::Database)?;
            let id = s.bound.as_ref().map(|b| b.binding.id);
            Ok(Snapshot {
                bound: s.bound.clone(),
                incarnation: s.incarnation,
                invalidation: s.invalidation,
                stopped: s.tracking_stopped,
                binding: id.map(|id| store.binding(id)).transpose()?.flatten(),
                runtime: id.map(|id| store.stored_runtime_state(id)).transpose()?.flatten(),
                active: id
                    .map(|id| store.adapter_durable(id))
                    .transpose()?
                    .and_then(|d| d.active),
                adoption: id
                    .map(|id| store.selected_adoption_authority(id))
                    .transpose()?
                    .flatten(),
                live_operation: store.publication_op()?.is_some() || store.adoption_op()?.is_some(),
                collection_crt: id
                    .map(|id| store.publication_media(id))
                    .transpose()?
                    .flatten()
                    .map(|(_, _, crt)| crt),
                publication: s.selected_publication.clone(),
            })
        })
    }

    /// No caller-selected binding, operation, key, path, verdict, or remote read.
    /// The Collection must be the runtime's actual current owned generation.
    pub fn selected_control_for_testing(
        &self,
        collection: &Collection,
    ) -> Result<SelectedControlObservation, RuntimeError> {
        use Missing::*;
        let missing = SelectedControlObservation::missing;
        if collection.path() != self.directory.join("collection.anki2") {
            eprintln!("selected-control:path");
            return Ok(missing(CurrentCollectionChanged));
        }
        let _lease = collection.reader(ReaderRole::Cloud).map_err(|_| Error::InvalidData)?;
        // All provider/sidecar reads precede runtime/tracking lock acquisition.
        let file = self
            .inputs
            .files
            .identity(collection.path())
            .map_err(|_| RuntimeError::FileIdentityUnavailable)?;
        let evidence = collection.open_evidence().map_err(|_| Error::InvalidData)?;
        let account = self.inputs.identity.read_identity(&self.inputs.scope);
        let device = DeviceState::for_collection(collection.path());
        let decision = device.publication_decision().map_err(|_| Error::InvalidData)?;
        let activation = collection.cloud_adopt_activation().map_err(|_| Error::InvalidData)?;
        let latest_replacement = device.replacement_evidence().map_err(|_| Error::InvalidData)?;
        let replacement = latest_replacement
            .as_ref()
            .map(|e| device.replacement_evidence_for(e.prior.identity))
            .transpose()
            .map_err(|_| Error::InvalidData)?
            .flatten();
        let snapshot = self.selected_snapshot()?;
        let Some(bound) = &snapshot.bound else {
            return Ok(missing(Unbound));
        };
        let mut result = (|| -> Result<SelectedControlObservation, Missing> {
            if snapshot.stopped || !bound.usable || !bound.verified {
                return Err(BindingUnusable);
            }
            let binding = snapshot.binding.as_ref().ok_or(BindingUnusable)?;
            if binding != &bound.binding
                || binding.facts.state != BindingState::Active
                || binding.scope != self.inputs.scope
            {
                return Err(BindingUnusable);
            }
            if !matches!(&account, IdentityStatus::Available(a) if a == &binding.facts.account) {
                return Err(AccountUnavailable);
            }
            if file != bound.file
                || evidence != bound.evidence
                || !collection.is_current()
                || collection.generation() != bound.evidence.generation
                || evidence.native_file != file.pair()
                || snapshot.runtime.as_ref().and_then(|r| r.file) != Some(file)
            {
                eprintln!("selected-control:file-open-current-generation-runtime-file");
                return Err(CurrentCollectionChanged);
            }
            let decision = decision.as_ref().ok_or(CurrentCollectionChanged).inspect_err(|_| {
                eprintln!("selected-control:publication-fence-absent");
            })?;
            let account_binding_matches = matches!(&decision.publication.binding,
            meguri_engine::device_state::AccountBinding::Connected { account, configuration, environment }
            if account.as_bytes() == binding.facts.account.0.as_slice()
                && configuration == self.inputs.scope.container.as_str()
                && environment == match self.inputs.scope.environment {
                    Environment::Development => "development", Environment::Production => "production",
                });
            if decision.publication.identity != binding.facts.publication
                || decision.pending_reason.is_some()
                || !account_binding_matches
            {
                eprintln!("selected-control:publication-fence");
                return Err(CurrentCollectionChanged);
            }
            if snapshot.live_operation {
                return Err(OperationInProgress);
            }
            let (active_record, epoch_record, provenance) = if let Some((op, activated_file)) = &snapshot.adoption {
                if op.step != AdoptStep::Ready
                    || *activated_file != Some(file)
                    || op.binding_after != binding.id
                    || op.account != binding.facts.account
                    || op.epoch != binding.facts.epoch
                    || op.candidate != decision.publication
                {
                    return Err(AdoptionNotActivated);
                }
                let epoch_record = op.epoch_record.as_ref().ok_or(ControlsMismatch)?;
                if op.active.system_fields.0.is_empty() || epoch_record.system_fields.0.is_empty() {
                    return Err(ControlsMismatch);
                }
                (
                    op.active.encrypted.as_deref().ok_or(ActiveUnavailable)?,
                    epoch_record.encrypted.as_deref().ok_or(ControlsMismatch)?,
                    Provenance::RetainedAdoption,
                )
            } else {
                let ack = snapshot
                    .runtime
                    .as_ref()
                    .and_then(|r| r.root_ack.as_ref())
                    .ok_or(ActiveUnavailable)?;
                let adapter = snapshot.active.as_ref().ok_or(ActiveUnavailable)?;
                if ack.epoch != binding.facts.epoch
                    || ack.envelope.0 != adapter.envelope
                    || ack.system_fields.0 != adapter.system_fields
                    || ack.system_fields.0.is_empty()
                {
                    return Err(ControlsMismatch);
                }
                // This is current accepted Active, not a fabricated original op.
                let active = Active::decode(&ack.envelope.0).map_err(|_| ControlsMismatch)?;
                if active.epoch != binding.facts.epoch.0 {
                    return Err(ControlsMismatch);
                }
                let mut partial = missing(MissingAfterRestart);
                partial.provenance = Some(Provenance::CurrentAcceptedActive);
                partial.active_sha256 = Some(codec::sha256(&active.encode().map_err(|_| ControlsMismatch)?));
                let Some(receipt) = &snapshot.publication else {
                    return Ok(partial);
                };
                if receipt.binding != binding.id
                    || receipt.epoch != binding.facts.epoch
                    || receipt.record_key != binding.facts.record_key
                    || receipt.file != file
                    || receipt.generation != evidence.generation
                    || receipt.incarnation != snapshot.incarnation
                    || receipt.complete_request.op != receipt.op
                    || receipt.complete_request.incarnation != snapshot.incarnation
                    || receipt.complete_request.generation != evidence.generation.0
                {
                    eprintln!("selected-control:accepted-receipt-identity");
                    return Err(CurrentCollectionChanged);
                }
                let Some(done) = &receipt.done_active else {
                    return Err(PublicationNotCommitted);
                };
                if done.0.op != receipt.op
                    || done.0.incarnation != snapshot.incarnation
                    || done.0.generation != evidence.generation.0
                    || done.0.instance != receipt.complete_request.instance
                    || done.2 != ack.envelope.0
                    || done.3 != codec::sha256(&ack.system_fields.0)
                {
                    return Err(ControlsMismatch);
                }
                (
                    done.2.as_slice(),
                    receipt.complete.as_slice(),
                    Provenance::RuntimeAcceptedPublication,
                )
            };
            let active = Active::decode(active_record).map_err(|_| ControlsMismatch)?;
            let DecodeOutcome::Supported(epoch) = Epoch::decode(epoch_record).map_err(|_| ControlsMismatch)? else {
                return Err(ControlsMismatch);
            };
            if active.epoch != binding.facts.epoch.0
                || epoch.epoch_id != active.epoch
                || epoch.state != EpochState::Complete
                || epoch.record_key != binding.facts.record_key.0
                || epoch.ver != 18
                || snapshot.collection_crt != Some(epoch.crt)
            {
                return Err(ControlsMismatch);
            }
            let mut output = SelectedControlObservation {
                status: Proof::Proven,
                provenance: Some(provenance),
                publication_complete: snapshot
                    .publication
                    .as_ref()
                    .filter(|_| provenance == Provenance::RuntimeAcceptedPublication)
                    .map(|r| r.complete_event.clone()),
                publication_done_active: snapshot
                    .publication
                    .as_ref()
                    .filter(|_| provenance == Provenance::RuntimeAcceptedPublication)
                    .and_then(|r| r.done_active.as_ref().map(|d| d.1.clone())),
                active_sha256: Some(codec::sha256(&active.encode().map_err(|_| ControlsMismatch)?)),
                epoch_sha256: Some(codec::sha256(&epoch.encode().map_err(|_| ControlsMismatch)?)),
                binding_matches_epoch: Proof::Proven,
                activation: Proof::Missing(ActivationUnavailable),
                owned_backup: Proof::Missing(OwnedBackupUnavailable),
            };
            if let Some((op, _)) = &snapshot.adoption {
                let before = op.before.as_ref().ok_or(ActivationUnavailable)?;
                let proof = activation.as_ref().ok_or(ActivationUnavailable)?;
                if !proof.matches(PublicationId::from_bytes(op.id), &before.publication, &op.candidate)
                    || proof.native_file() != file.pair()
                {
                    return Err(ActivationUnavailable);
                }
                let replacement = replacement.as_ref().ok_or(OwnedBackupUnavailable)?;
                // Engine's activation getter independently joins its private
                // operation/receipt to the retained latest replacement and native file.
                if latest_replacement.as_ref() != Some(replacement)
                    || replacement.reason != ReplaceReason::CloudAdopt
                    || replacement.prior != before.publication
                    || replacement.candidate != op.candidate
                    || replacement.old_file.is_none()
                    || op.file_before.is_none()
                    || replacement.safety_backup.is_none()
                {
                    return Err(OwnedBackupUnavailable);
                }
                output.activation = Proof::Proven;
                output.owned_backup = Proof::Proven;
            }
            Ok(output)
        })()
        .unwrap_or_else(missing);
        // Validate all sidecar/provider facts without either runtime/store lock.
        let final_file = self
            .inputs
            .files
            .identity(collection.path())
            .map_err(|_| RuntimeError::FileIdentityUnavailable)?;
        let final_account = self.inputs.identity.read_identity(&self.inputs.scope);
        let final_decision = device.publication_decision().map_err(|_| Error::InvalidData)?;
        let final_activation = collection.cloud_adopt_activation().map_err(|_| Error::InvalidData)?;
        let final_activation_matches = match (&snapshot.adoption, &final_activation) {
            (Some((op, _)), Some(proof)) => op.before.as_ref().is_some_and(|before| {
                proof.matches(PublicationId::from_bytes(op.id), &before.publication, &op.candidate)
            }),
            _ => false,
        };
        if final_file != file
            || final_account != account
            || final_decision != decision
            || !collection.is_current()
            || collection.open_evidence().map_err(|_| Error::InvalidData)? != evidence
            || (result.activation == Proof::Proven && !final_activation_matches)
            || final_activation.as_ref().map(|a| a.native_file()) != activation.as_ref().map(|a| a.native_file())
            || device.replacement_evidence().map_err(|_| Error::InvalidData)? != latest_replacement
            || self.selected_snapshot()? != snapshot
        {
            eprintln!("selected-control:final-provider-snapshot-recheck");
            result = missing(CurrentCollectionChanged);
        }
        Ok(result)
    }
}

// Kept only across ordinary scanning in this new Runtime. The values were read
// from its owned store before open_runtime_binding; they are never restored.
#[derive(Clone)]
pub(super) struct ReopenNote {
    pub binding: Binding,
    pub file: NativeFileIdentity,
    pub open: u64,
    pub entity: super::super::workflow::EntityFacts,
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoricalEdit {
    pub original_generation: u64,
    pub original_seq: u64,
    pub local_revision: u64,
    pub digest: [u8; 32],
}
// In-process projection only. This is neither an original callback receipt nor
// authorization to complete a recovered exchange.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HistoricalAck {
    pub edit: HistoricalEdit,
    pub acked_revision: u64,
    pub acked_envelope_digest: [u8; 32],
}
/// The exact live original-role ACK predicate, shared with its FFI observation.
pub fn original_note_acknowledged_for_testing(e: &super::super::workflow::EntityFacts, digest: [u8; 32]) -> bool {
    e.local_rev > 0
        && e.acked_rev == e.local_rev
        && e.acked_payload_digest == digest
        && e.mark.is_none()
        && e.in_flight.is_empty()
        && e.server_erev > 0
        && !e.acked_system_fields.is_empty()
        && !e.acked_envelope.0.is_empty()
}
fn original_ack_pair(
    original: &super::super::workflow::EntityFacts,
    current: &super::super::workflow::EntityFacts,
    digest: [u8; 32],
) -> Result<HistoricalAck, RuntimeError> {
    if !original_note_acknowledged_for_testing(original, digest)
        || !original_note_acknowledged_for_testing(current, digest)
        || original.local_rev != current.local_rev
        || original.acked_rev != current.acked_rev
        || original.acked_payload_digest != current.acked_payload_digest
        || original.server_erev != current.server_erev
        || original.acked_system_fields != current.acked_system_fields
        || original.acked_envelope != current.acked_envelope
        || original.tombstone_at.is_some()
        || current.tombstone_at.is_some()
    {
        return Err(Error::RecoveryRefused.into());
    }
    Ok(HistoricalAck {
        edit: HistoricalEdit {
            original_generation: original.observed_generation,
            original_seq: original.observed_seq,
            local_revision: original.local_rev,
            digest,
        },
        acked_revision: original.acked_rev,
        acked_envelope_digest: codec::sha256(&original.acked_envelope.0),
    })
}
#[derive(Clone)]
pub struct RestoredControl {
    pub generation: u64,
    pub active_sha256: [u8; 32],
    pub epoch_sha256: [u8; 32],
}
fn correlation<T: Serialize>(domain: &[u8], value: &T) -> Result<[u8; 32], RuntimeError> {
    let mut writer = PublicationHash::default();
    writer.write_all(domain).map_err(|_| Error::InvalidData)?;
    serde_json::to_writer(&mut writer, value).map_err(|_| Error::InvalidData)?;
    Ok(writer.hash.finalize().into())
}
impl Runtime {
    // Reuse the live inspector's native file, publication fence, account,
    // binding and final-provider rechecks. MissingAfterRestart is accepted only
    // as current ownership here; it NEVER becomes original reply evidence.
    fn selected_owned_snapshot(
        &self,
        collection: &Collection,
    ) -> Result<(Snapshot, SelectedControlObservation), RuntimeError> {
        let control = self.selected_control_for_testing(collection)?;
        if !matches!(
            control.status,
            Proof::Proven | Proof::Missing(Missing::MissingAfterRestart)
        ) {
            return Err(Error::RecoveryRefused.into());
        }
        let snapshot = self.selected_snapshot()?;
        let b = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        if b.evidence != collection.open_evidence()? || !collection.is_current() || !b.verified || !b.usable {
            return Err(Error::RecoveryRefused.into());
        }
        Ok((snapshot, control))
    }
    fn selected_recheck(
        &self,
        collection: &Collection,
        snapshot: &Snapshot,
        control: &SelectedControlObservation,
    ) -> Result<(), RuntimeError> {
        if &self.selected_control_for_testing(collection)? != control || &self.selected_snapshot()? != snapshot {
            return Err(Error::RecoveryRefused.into());
        }
        Ok(())
    }
    fn control_correlation(
        snapshot: &Snapshot,
        active: [u8; 32],
        epoch: [u8; 32],
        generation: u64,
    ) -> Result<[u8; 32], RuntimeError> {
        let bound = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        // Stable, actually owned identity; opening counter and current generation
        // are deliberately absent. The original generation stays historical.
        correlation(
            b"meguri-selected-original-control-v1",
            &(&bound.binding, bound.file, active, epoch, generation),
        )
    }
    /// Emitted only after the real original Complete/Active or adoption proof.
    /// The journal stores this correlation together with the original canonical
    /// digests, not an invented durable copy of the process-local reply.
    pub fn selected_original_control_witness_for_testing(
        &self,
        collection: &Collection,
    ) -> Result<[u8; 32], RuntimeError> {
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        if control.status != Proof::Proven || control.binding_matches_epoch != Proof::Proven {
            return Err(Error::RecoveryRefused.into());
        }
        let witness = Self::control_correlation(
            &snapshot,
            control.active_sha256.ok_or(Error::RecoveryRefused)?,
            control.epoch_sha256.ok_or(Error::RecoveryRefused)?,
            collection.generation().0,
        )?;
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(witness)
    }
    /// Historical journal proof and current acceptance are separate. This method
    /// does not change selected_control_for_testing or resurrect its old reply.
    pub fn selected_restored_control_for_testing(
        &self,
        collection: &Collection,
        original_generation: u64,
        active: [u8; 32],
        epoch: [u8; 32],
        witness: [u8; 32],
    ) -> Result<RestoredControl, RuntimeError> {
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        if original_generation == 0
            || control.active_sha256 != Some(active)
            || control.epoch_sha256.is_some_and(|current| current != epoch)
            || Self::control_correlation(&snapshot, active, epoch, original_generation)? != witness
        {
            return Err(Error::RecoveryRefused.into());
        }
        if snapshot.adoption.is_some() && (control.activation != Proof::Proven || control.owned_backup != Proof::Proven)
        {
            return Err(Error::RecoveryRefused.into());
        }
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(RestoredControl {
            generation: collection.generation().0,
            active_sha256: active,
            epoch_sha256: epoch,
        })
    }
    fn edit_correlation(
        binding: &Binding,
        file: NativeFileIdentity,
        entity: &super::super::workflow::EntityFacts,
        baseline: [u8; 32],
    ) -> Result<[u8; 32], RuntimeError> {
        correlation(
            b"meguri-selected-original-edit-v1",
            &(
                binding,
                file,
                baseline,
                &entity.name,
                &entity.key,
                entity.origin,
                entity.observer_open,
                entity.observed_generation,
                entity.observed_seq,
                entity.local_rev,
                entity.payload.payload_digest().map_err(|_| Error::InvalidData)?,
            ),
        )
    }
    /// Only the actual fixed-edit post-flush path calls this with its actual
    /// edit seq, baseline capability and held snapshot facts. The caller retains
    /// its reader/snapshot through this read. No product decision consumes it.
    pub fn original_edit_witness_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        baseline: [u8; 32],
        held_snapshot: (meguri_engine::WalIdentity, Seq),
        edit_seq: Seq,
        digest: [u8; 32],
    ) -> Result<[u8; 32], RuntimeError> {
        let (identity, snapshot_seq) = held_snapshot;
        let (entity, _) = self.original_note_observation_for_testing(collection, id, identity, snapshot_seq, digest)?;
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        let b = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        if entity.observed_seq != edit_seq.0 || entity.local_rev == 0 || entity.observer_open != b.open_counter {
            return Err(Error::RecoveryRefused.into());
        }
        let witness = Self::edit_correlation(&b.binding, b.file, &entity, baseline)?;
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(witness)
    }
    fn original_edit_entities(
        &self,
        snapshot: &Snapshot,
        id: i64,
        baseline: [u8; 32],
        witness: [u8; 32],
    ) -> Result<(ReopenNote, super::super::workflow::EntityFacts), RuntimeError> {
        let b = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        let pair = {
            let s = self.state();
            let original = s.selected_reopen_note.as_ref().ok_or(Error::RecoveryRefused)?;
            if original.binding != b.binding
                || original.file != b.file
                || original.open.checked_add(1) != Some(b.open_counter)
                || original.entity.observer_open != original.open
                || original.entity.key != codec::id_key(id)
                || original.entity.kind != codec::Kind::Note
                || original.entity.tombstone_at.is_some()
                || original.entity.local_rev == 0
                || original.entity.observed_generation == 0
                || original.entity.observed_seq == 0
                || Self::edit_correlation(&b.binding, b.file, &original.entity, baseline)? != witness
            {
                return Err(Error::RecoveryRefused.into());
            }
            let current = with_tracking_access(|| {
                s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .entity_dump(b.binding.id, &original.entity.name)
            })?
            .ok_or(Error::RecoveryRefused)?;
            if current.kind != original.entity.kind
                || current.key != original.entity.key
                || current.origin != original.entity.origin
                || current.payload != original.entity.payload
                || current.local_rev != original.entity.local_rev
                || current.tombstone_at.is_some()
            {
                return Err(Error::RecoveryRefused.into());
            }
            (original.clone(), current)
        };
        Ok(pair)
    }
    pub fn read_original_edit_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        baseline: [u8; 32],
        witness: [u8; 32],
    ) -> Result<HistoricalEdit, RuntimeError> {
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        let (original, _) = self.original_edit_entities(&snapshot, id, baseline, witness)?;
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(HistoricalEdit {
            original_generation: original.entity.observed_generation,
            original_seq: original.entity.observed_seq,
            local_revision: original.entity.local_rev,
            digest: original
                .entity
                .payload
                .payload_digest()
                .map_err(|_| Error::InvalidData)?,
        })
    }
    /// The caller retains its Query snapshot and current WAL coverage. The
    /// original edit sequence is historical, never substituted for this frontier.
    pub fn read_original_ack_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        baseline: [u8; 32],
        witness: [u8; 32],
        held_snapshot: (meguri_engine::WalIdentity, Seq),
        digest: [u8; 32],
    ) -> Result<HistoricalAck, RuntimeError> {
        self.read_original_ack_with_recheck_for_testing(
            collection,
            id,
            (baseline, witness),
            held_snapshot,
            digest,
            || {},
        )
    }
    /// Test-only read boundary; supplies no row or authority and holds no locks.
    pub fn read_original_ack_with_recheck_for_testing(
        &self,
        collection: &Collection,
        id: i64,
        edit_correlation: ([u8; 32], [u8; 32]),
        held_snapshot: (meguri_engine::WalIdentity, Seq),
        digest: [u8; 32],
        after_read: impl FnOnce(),
    ) -> Result<HistoricalAck, RuntimeError> {
        let (baseline, witness) = edit_correlation;
        let (identity, seq) = held_snapshot;
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        let (original, current) = self.original_edit_entities(&snapshot, id, baseline, witness)?;
        let (owned, _) = self.original_note_observation_for_testing(collection, id, identity, seq, digest)?;
        if original
            .entity
            .payload
            .payload_digest()
            .map_err(|_| Error::InvalidData)?
            != digest
            || owned != current
        {
            return Err(Error::RecoveryRefused.into());
        }
        let ack = original_ack_pair(&original.entity, &current, digest)?;
        after_read();
        let (reread, _) = self.original_note_observation_for_testing(collection, id, identity, seq, digest)?;
        let (retained, current_again) = self.original_edit_entities(&snapshot, id, baseline, witness)?;
        if reread != current
            || current_again != current
            || retained.entity != original.entity
            || original_ack_pair(&retained.entity, &current_again, digest)? != ack
        {
            return Err(Error::RecoveryRefused.into());
        }
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(ack)
    }
    pub fn original_exchange_witness_for_testing(
        &self,
        collection: &Collection,
        owner: &str,
        exchange: u64,
        source: super::super::policy::Source,
    ) -> Result<[u8; 32], RuntimeError> {
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        let b = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        if owner.is_empty() || owner.len() > 1024 || exchange == 0 {
            return Err(Error::RecoveryRefused.into());
        }
        let witness = exchange_correlation(&b.binding, b.file, owner, exchange, source)?;
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(witness)
    }
    pub fn read_original_exchange_for_testing(
        &self,
        collection: &Collection,
        exchange: u64,
        witness: [u8; 32],
    ) -> Result<super::super::policy::ExchangeRecord, RuntimeError> {
        let (snapshot, control) = self.selected_owned_snapshot(collection)?;
        let b = snapshot.bound.as_ref().ok_or(Error::RecoveryRefused)?;
        let record = {
            let s = self.state();
            with_tracking_access(|| {
                s.store
                    .as_ref()
                    .ok_or(Error::Database)?
                    .selected_original_exchange(&b.binding, b.file, exchange, witness)
            })?
            .ok_or(Error::RecoveryRefused)?
        };
        self.selected_recheck(collection, &snapshot, &control)?;
        Ok(record)
    }
}
pub(crate) fn exchange_correlation(
    binding: &Binding,
    file: NativeFileIdentity,
    owner: &str,
    exchange: u64,
    source: super::super::policy::Source,
) -> Result<[u8; 32], RuntimeError> {
    correlation(
        b"meguri-selected-original-exchange-v1",
        &(binding, file, owner, exchange, source),
    )
}

#[cfg(test)]
mod original_ack_tests {
    use super::*;
    use crate::cloud::workflow::{EntityFacts, Flight};

    // Predicate-only values; never installed as tracking rows or receipts.
    fn acked() -> EntityFacts {
        let payload = codec::Payload::Note(codec::Note {
            id: 1,
            ..Default::default()
        });
        let digest = payload.payload_digest().unwrap();
        EntityFacts {
            name: "predicate-only".into(),
            kind: codec::Kind::Note,
            key: codec::id_key(1).to_vec(),
            origin: [1; 16],
            sent: true,
            local_rev: 3,
            acked_rev: 3,
            tombstone_at: None,
            payload: payload.clone(),
            base_payload: None,
            mark: None,
            server_erev: 2,
            acked_system_fields: vec![1],
            acked_row_digest: [2; 32],
            acked_payload_digest: digest,
            acked_payload: Some(payload),
            acked_envelope: OpaqueBytes(vec![3]),
            in_flight: vec![],
            observer_open: 4,
            observed_seq: 5,
            observed_generation: 6,
        }
    }
    #[test]
    fn original_ack_requires_each_live_predicate_on_both_rows() {
        let healthy = acked();
        let digest = healthy.payload.payload_digest().unwrap();
        for side in ["original", "current"] {
            for failure in [
                "zero",
                "unacked",
                "digest",
                "dirty",
                "flight",
                "server",
                "systems",
                "envelope",
                "tombstone",
            ] {
                let mut bad = healthy.clone();
                match failure {
                    "zero" => {
                        bad.local_rev = 0;
                        bad.acked_rev = 0;
                    }
                    "unacked" => bad.acked_rev -= 1,
                    "digest" => bad.acked_payload_digest = [9; 32],
                    "dirty" => {
                        bad.mark = Some(DirtyMark {
                            source: DirtySource::Observer,
                            change_seq: bad.observed_seq,
                            payload_digest: digest,
                        })
                    }
                    "flight" => bad.in_flight.push(Flight {
                        local_rev: bad.local_rev,
                        payload_digest: digest,
                        row_digest: [2; 32],
                        payload: bad.payload.clone().into(),
                        erev: 2,
                        image_seq: 5,
                        held: false,
                        tombstone: false,
                        tombstone_envelope_digest: None,
                    }),
                    "server" => bad.server_erev = 0,
                    "systems" => bad.acked_system_fields.clear(),
                    "envelope" => bad.acked_envelope.0.clear(),
                    "tombstone" => bad.tombstone_at = Some(1),
                    _ => unreachable!(),
                }
                let result = if side == "original" {
                    original_ack_pair(&bad, &healthy, digest)
                } else {
                    original_ack_pair(&healthy, &bad, digest)
                };
                assert!(result.is_err(), "{side}: {failure}");
            }
        }
        let projected = original_ack_pair(&healthy, &healthy, digest).unwrap();
        assert_eq!(projected.edit.original_seq, healthy.observed_seq);
        assert_eq!(projected.acked_revision, healthy.local_rev);
        assert_eq!(
            projected.acked_envelope_digest,
            codec::sha256(&healthy.acked_envelope.0)
        );
    }
    #[test]
    fn independently_acked_rows_still_need_identical_revision_system_fields_and_envelope() {
        let original = acked();
        let digest = original.payload.payload_digest().unwrap();
        for changed in ["revision", "server_revision", "system_fields", "envelope"] {
            let mut current = original.clone();
            match changed {
                "revision" => {
                    current.local_rev += 1;
                    current.acked_rev += 1;
                }
                "server_revision" => current.server_erev += 1,
                "system_fields" => current.acked_system_fields.push(7),
                "envelope" => current.acked_envelope.0.push(7),
                _ => unreachable!(),
            }
            assert!(original_note_acknowledged_for_testing(&current, digest));
            assert!(
                original_ack_pair(&original, &current, digest).is_err(),
                "current: {changed}"
            );
            assert!(
                original_ack_pair(&current, &original, digest).is_err(),
                "original: {changed}"
            );
        }
    }
}
