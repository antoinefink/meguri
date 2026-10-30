use super::{publication::*, runtime::NativeFileIdentity, *};
use meguri_engine::device_state::{Publication, PublicationId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdoptStep {
    Staging,
    Staged,
    Ready,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptOp {
    pub id: [u8; 16],
    pub step: AdoptStep,
    pub binding_before: Option<BindingBefore>,
    pub binding_after: BindingId,
    pub account: OpaqueBytes,
    pub before: Option<DecisionSnapshot>,
    pub file_before: Option<NativeFileIdentity>,
    pub candidate: Publication,
    pub active: ControlRecord,
    pub epoch: EpochId,
    pub epoch_record: Option<ControlRecord>,
    pub media_root: Option<ControlRecord>,
    pub staging_name: String,
    pub started_at_ms: i64,
}
impl std::fmt::Debug for AdoptOp {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AdoptOp")
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum AdoptionTask {
    ReadActive,
    ReadEpoch,
    ReadMediaRoot,
    Fetch { cursor: Option<OpaqueBytes> },
    Validate,
    RecheckActive,
    Activate,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptionWork {
    pub op: [u8; 16],
    pub task: AdoptionTask,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct StagingRecord {
    pub name: String,
    pub record_type: String,
    pub record: ControlRecord,
}
#[derive(Clone, Serialize, Deserialize)]
pub enum AdoptionReply {
    Read(RecordRead),
    Page {
        records: Vec<StagingRecord>,
        deleted: Vec<String>,
        next: Option<OpaqueBytes>,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStep {
    Publish(PublishStep),
    Adopt(AdoptStep),
}
impl PartialEq<PublishStep> for RecoveryStep {
    fn eq(&self, step: &PublishStep) -> bool {
        matches!(self, Self::Publish(p) if p == step)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryKind {
    Publish(Box<PublishOp>),
    Adopt(Box<AdoptOp>),
}
/// Common presentation fields retain publication clients' read surface. The
/// complete durable row and its distinct step are carried by `kind`/`step`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecoveryOpFacts {
    pub id: [u8; 16],
    pub step: RecoveryStep,
    pub kind: RecoveryKind,
    pub account: OpaqueBytes,
    pub epoch: EpochId,
    pub binding_after: Option<BindingId>,
    pub confirmed_publication: Option<PublicationId>,
    pub confirmed_file: Option<NativeFileIdentity>,
    pub switch_sent: bool,
    pub active_attempt: Option<OpaqueBytes>,
    pub switch_conflict: Option<OpaqueBytes>,
}
impl From<PublishOp> for RecoveryOpFacts {
    fn from(p: PublishOp) -> Self {
        Self {
            id: p.id,
            step: RecoveryStep::Publish(p.step),
            account: p.account.clone(),
            epoch: p.epoch,
            binding_after: p.binding_after,
            confirmed_publication: Some(p.confirmed_publication),
            confirmed_file: Some(p.confirmed_file),
            switch_sent: p.switch_sent,
            active_attempt: p.active_attempt.clone(),
            switch_conflict: p.switch_conflict.clone(),
            kind: RecoveryKind::Publish(Box::new(p)),
        }
    }
}
impl From<AdoptOp> for RecoveryOpFacts {
    fn from(p: AdoptOp) -> Self {
        Self {
            id: p.id,
            step: RecoveryStep::Adopt(p.step),
            account: p.account.clone(),
            epoch: p.epoch,
            binding_after: (p.step == AdoptStep::Ready).then_some(p.binding_after),
            confirmed_publication: p.before.as_ref().map(|d| d.publication.identity),
            confirmed_file: p.file_before,
            switch_sent: false,
            active_attempt: None,
            switch_conflict: None,
            kind: RecoveryKind::Adopt(Box::new(p)),
        }
    }
}
