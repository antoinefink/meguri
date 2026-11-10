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
