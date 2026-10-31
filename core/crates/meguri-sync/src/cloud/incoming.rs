//! Ordinary incoming work. Intents describe collection writes, never a second collection.
use super::{entity_codec as c, *};
use serde::{Deserialize, Serialize};

/// One bounded native apply. Deferred is produced only after rollback and its
/// tracking commit succeed; the original writer error stays available locally.
#[derive(Debug)]
pub enum IncomingDisposition {
    Progress(usize),
    Deferred(meguri_engine::EngineError),
}
impl IncomingDisposition {
    pub(super) fn into_result(self) -> Result<usize, runtime::RuntimeError> {
        match self {
            Self::Progress(count) => Ok(count),
            Self::Deferred(error) => Err(error.into()),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyKeyRole {
    Entry,
    SideEffect,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyKey {
    pub name: String,
    pub kind: c::Kind,
    pub key: Vec<u8>,
    pub role: ApplyKeyRole,
    pub before_digest: Option<[u8; 32]>,
    pub after_digest: Option<[u8; 32]>,
    pub before_payload: Option<c::Payload>,
    pub after_payload: Option<c::Payload>,
    pub own_flight: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyIntent {
    #[serde(default)]
    pub media: Option<super::media::Apply>,
    pub id: i64,
    pub binding: BindingId,
    pub inbox_ids: Vec<i64>,
    pub keys: Vec<ApplyKey>,
    pub result_seq: Option<u64>,
    /// Opening that created this intent. Legacy counters carry no open identity.
    #[serde(default)]
    pub originating_open: Option<u64>,
    #[serde(default)]
    pub planned_at_ms: i64,
    pub native_file: runtime::NativeFileIdentity,
    pub generation: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RefusedEntry {
    pub record_name: String,
    pub reason: String,
    pub envelope: OpaqueBytes,
    pub origin: Option<[u8; 16]>,
    pub erev: Option<u64>,
    pub seen_at_ms: i64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApplyRun {
    pub intent_id: i64,
    pub seq: u64,
    pub native_file: runtime::NativeFileIdentity,
    pub generation: u64,
    pub entry_ids: Vec<i64>,
    pub changed_keys: std::collections::BTreeSet<String>,
}

#[derive(Default)]
pub(crate) struct ProcessState {
    pub running: bool,
    pub retry_granted: bool,
    pub failed_this_open: std::collections::BTreeSet<i64>,
    pub runs: Vec<ApplyRun>,
    pub receipts: Vec<meguri_engine::WalReceipt>,
    pub last_deferred_error: Option<meguri_engine::EngineError>,
}

pub(crate) fn digest(p: Option<&c::Payload>) -> Result<Option<[u8; 32]>, Error> {
    p.map(|p| p.row_digest().map_err(|_| Error::InvalidData)).transpose()
}

/// Decode without touching live rows; retain generation controls separately from Ready entities.
pub(crate) fn classify(key: &[u8; 32], records: &[workflow::Saved], at: i64) -> (Vec<InboxEntry>, Vec<RefusedEntry>) {
    let mut entries = Vec::new();
    let mut refused = Vec::new();
    for r in records {
        if r.name.starts_with('g') {
            // Generation manifests are control work. This ordinary worker
            // retains them for the generation phase and never makes them Ready.
            entries.push(InboxEntry {
                record_type: RecordType::Generation,
                record_name: r.name.clone(),
                value: InboxValue::Record {
                    erev: 0,
                    system_fields: OpaqueBytes(r.system_fields.clone()),
                    envelope: OpaqueBytes(r.envelope.clone()),
                    staged_asset: None,
                },
                reason: InboxReason::Generation,
                source: InboxSource::Fetch,
                absence_pass: None,
                retry_count: 0,
                fetched_at_ms: at,
            });
            continue;
        }
        let decoded = c::Entity::decode(&r.envelope).ok();
        let fields = match &decoded {
            Some(c::DecodeOutcome::Supported(e)) => Some(&e.fields),
            _ => None,
        };
        let reason = match c::receive(key, &r.name, &r.envelope, c::AssetInput::Absent) {
            Ok(received) => {
                if (!received.entity.fields.tombstone && received.payload.is_none())
                    || received.payload.as_ref().is_some_and(|p| {
                        p.kind() != received.entity.fields.kind
                            || (p.kind() != c::Kind::MediaName
                                && super::row_payload::payload_key(p).ok().as_ref()
                                    != Some(&received.entity.fields.key))
                    })
                {
                    None
                } else if received.entity.fields.generation.is_some() {
                    Some(InboxReason::Generation)
                } else {
                    Some(InboxReason::Ready)
                }
            }
            Err(c::ReceiveError::UnsupportedFormat(_)) => Some(InboxReason::Format),
            Err(c::ReceiveError::RetryableAsset(_)) => Some(InboxReason::Asset),
            Err(c::ReceiveError::Refused(_)) => None,
        };
        if let Some(reason) = reason {
            entries.push(InboxEntry {
                record_type: RecordType::Entity,
                record_name: r.name.clone(),
                value: InboxValue::Record {
                    erev: fields.map_or(0, |f| f.erev),
                    system_fields: OpaqueBytes(r.system_fields.clone()),
                    envelope: OpaqueBytes(r.envelope.clone()),
                    staged_asset: None,
                },
                reason,
                source: InboxSource::Fetch,
                absence_pass: None,
                retry_count: 0,
                fetched_at_ms: at,
            });
        } else {
            refused.push(RefusedEntry {
                record_name: r.name.clone(),
                reason: "invalid_body".into(),
                envelope: OpaqueBytes(r.envelope.clone()),
                origin: fields.map(|f| f.origin),
                erev: fields.map(|f| f.erev),
                seen_at_ms: at,
            });
        }
    }
    (entries, refused)
}
