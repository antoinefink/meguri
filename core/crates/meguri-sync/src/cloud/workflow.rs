//! Production pending actions and conditional sends for a verified stable binding.
use super::{Error, OpaqueBytes, entity_codec as c, runtime::DirtyMark};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, VecDeque};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PendingChange {
    Save,
    Delete,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum UpdateKind {
    Full,
    Changes,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingSnapshot(pub BTreeMap<String, PendingChange>);
impl PendingSnapshot {
    pub fn empty() -> Self {
        Self::default()
    }
    pub fn saves(names: impl IntoIterator<Item = String>) -> Self {
        Self(names.into_iter().map(|n| (n, PendingChange::Save)).collect())
    }
    pub fn deletions(names: impl IntoIterator<Item = String>) -> Self {
        Self(names.into_iter().map(|n| (n, PendingChange::Delete)).collect())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PendingUpdate {
    pub id: u64,
    pub kind: UpdateKind,
    pub adds: BTreeMap<String, PendingChange>,
    pub removes: BTreeSet<String>,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Observation {
    ReconcileKindCompleted {
        kind: c::Kind,
        row_count: usize,
    },
    MediaFeedDrained {
        entry_count: usize,
        feed_id: [u8; 16],
        cursor: u64,
    },
    SessionEnded {
        session: u64,
    },
    CorePendingEmitted {
        kind: UpdateKind,
        action_id: u64,
    },
    FullComparisonSkipped {
        action_id: u64,
    },
    BatchBuilt {
        names: BTreeSet<String>,
        batch_id: u64,
    },
    PendingAdded {
        name: String,
        action_id: u64,
    },
    HandlerReturned {
        request_id: u64,
    },
    SavedPendingRemoved {
        name: String,
        request_id: u64,
    },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Flight {
    pub local_rev: u64,
    pub payload_digest: [u8; 32],
    pub row_digest: [u8; 32],
    pub payload: c::Payload,
    pub erev: u64,
    #[serde(default)]
    pub image_seq: u64,
    #[serde(default)]
    pub held: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EntityFacts {
    pub name: String,
    pub kind: c::Kind,
    pub key: Vec<u8>,
    pub origin: [u8; 16],
    pub local_rev: u64,
    pub acked_rev: u64,
    pub payload: c::Payload,
    pub base_payload: Option<c::Payload>,
    pub mark: Option<DirtyMark>,
    pub server_erev: u64,
    pub acked_system_fields: Vec<u8>,
    pub acked_row_digest: [u8; 32],
    pub acked_payload_digest: [u8; 32],
    pub acked_payload: Option<c::Payload>,
    pub acked_envelope: OpaqueBytes,
    pub in_flight: Vec<Flight>,
    pub observer_open: u64,
    pub observed_seq: u64,
    pub observed_generation: u64,
}
impl EntityFacts {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.kind != self.payload.kind()
            || super::row_payload::payload_key(&self.payload)? != self.key
            || self.in_flight.len() > 250
        {
            return Err(Error::InvalidData);
        }
        for flight in &self.in_flight {
            super::runtime_store::checked(flight.image_seq)?;
        }
        for n in [
            self.local_rev,
            self.acked_rev,
            self.server_erev,
            self.observer_open,
            self.observed_seq,
            self.observed_generation,
        ] {
            super::runtime_store::checked(n)?;
        }
        if self.acked_rev > self.local_rev {
            return Err(Error::InvalidData);
        }
        if self.server_erev > 0 {
            let p = self.acked_payload.as_ref().ok_or(Error::InvalidData)?;
            if p.kind() != self.kind
                || p.row_digest().map_err(|_| Error::InvalidData)? != self.acked_row_digest
                || p.payload_digest().map_err(|_| Error::InvalidData)? != self.acked_payload_digest
                || self.acked_system_fields.is_empty()
            {
                return Err(Error::InvalidData);
            }
        }
        if let Some(m) = &self.mark
            && m.payload_digest != self.payload.payload_digest().map_err(|_| Error::InvalidData)?
        {
            return Err(Error::InvalidData);
        }
        Ok(())
    }
    pub(crate) fn observe(
        &mut self,
        p: c::Payload,
        source: super::runtime::DirtySource,
        counter: &mut u64,
    ) -> Result<bool, Error> {
        if self.payload == p {
            return Ok(false);
        }
        self.local_rev = self.local_rev.checked_add(1).ok_or(Error::InvalidData)?;
        self.payload = p;
        if self.payload.row_digest().map_err(|_| Error::InvalidData)? == self.acked_row_digest && self.server_erev > 0 {
            self.mark = None;
            self.base_payload = None;
        } else {
            *counter = counter.checked_add(1).ok_or(Error::InvalidData)?;
            if self.mark.is_none() {
                self.base_payload = self.acked_payload.clone();
            }
            self.mark = Some(DirtyMark {
                source,
                change_seq: *counter,
                payload_digest: self.payload.payload_digest().map_err(|_| Error::InvalidData)?,
            });
        }
        Ok(true)
    }
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SaveInput {
    pub name: String,
    pub envelope: Vec<u8>,
    pub system_fields: Option<Vec<u8>>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuiltBatch {
    pub id: u64,
    pub saves: Vec<SaveInput>,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Saved {
    pub name: String,
    pub envelope: Vec<u8>,
    pub system_fields: Vec<u8>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum SendError {
    LimitExceeded,
    BatchRequestFailed,
    ZoneMissing,
    Other,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct SendCall {
    pub id: u64,
    pub exchange: Option<u64>,
    pub automatic: bool,
    pub request_ids: Vec<u64>,
    pub returned: bool,
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExchangeResult {
    pub boundary_seq: u64,
    pub selected_names: BTreeSet<String>,
    pub acked_names: BTreeSet<String>,
    pub outstanding_names: BTreeSet<String>,
}
/// Opening-local undo authority. No Collection/Runtime strong references.
#[derive(Default)]
pub(crate) struct CloudFrontier {
    pub generation: Option<meguri_engine::Generation>,
    pub native_file: Option<super::runtime::NativeFileIdentity>,
    pub detached: Vec<meguri_engine::undo::holdback::HeldBackRun>,
    pub incoming_held: BTreeSet<String>,
    pub deferred: BTreeSet<String>,
    pub flagged: BTreeMap<meguri_engine::EntryId, BTreeSet<String>>,
    pub acked_images: BTreeMap<String, u64>,
    // A persisted flight from an earlier process has no authority over this
    // opening's undo ids, even if its numeric image seq happens to match.
    pub handed_images: BTreeMap<(String, u64, [u8; 32]), u64>,
    pub incoming_seq: u64,
}

/// Action ordering lives in production. SDK implementations execute complete lists
/// only after the sent event returns; a later full update cannot overtake one.
#[derive(Default)]
pub(crate) struct Actions {
    pub desired: BTreeMap<String, PendingChange>,
    pub queue: VecDeque<PendingUpdate>,
    pub emitted: Vec<u64>,
    pub observations: Vec<Observation>,
    pub handed_updates: BTreeMap<u64, PendingUpdate>,
    pub saved_callbacks: BTreeMap<u64, BTreeSet<String>>,
    pub handler: Option<u64>,
    pub next: u64,
    pub batch: u64,
    pub record_limit: usize,
    pub byte_limit: usize,
    pub automatic: bool,
    pub exchange_phase: Option<FetchPhase>,
    pub selected_revs: BTreeMap<String, u64>,
    pub calls: Vec<SendCall>,
    pub exchange: Option<(u64, ExchangeResult)>,
    pub exchanges: BTreeMap<u64, ExchangeResult>,
}
impl Actions {
    pub fn new() -> Self {
        Self {
            record_limit: 250,
            byte_limit: 1_572_864,
            ..Self::default()
        }
    }
    pub fn update(&mut self, kind: UpdateKind, snapshot: PendingSnapshot, force: BTreeSet<String>) -> PendingUpdate {
        self.next += 1;
        let id = self.next;
        let removes = self
            .desired
            .keys()
            .filter(|n| !snapshot.0.contains_key(*n))
            .cloned()
            .collect();
        let skip = kind == UpdateKind::Full && snapshot.0.is_empty() && self.desired.is_empty() && force.is_empty();
        let adds = if skip {
            BTreeMap::new()
        } else {
            snapshot
                .0
                .iter()
                .filter(|(n, v)| kind == UpdateKind::Full || self.desired.get(*n) != Some(*v) || force.contains(*n))
                .map(|(n, v)| (n.clone(), *v))
                .collect()
        };
        self.desired = snapshot.0;
        let update = PendingUpdate {
            id,
            kind,
            adds,
            removes,
        };
        self.observations
            .push(Observation::CorePendingEmitted { kind, action_id: id });
        if skip {
            self.observations
                .push(Observation::FullComparisonSkipped { action_id: id });
        }
        self.emitted.push(id);
        self.queue.push_back(update.clone());
        update
    }
    pub fn take(&mut self) -> Vec<PendingUpdate> {
        if self.handler.is_some() {
            Vec::new()
        } else {
            {
                let updates: Vec<_> = self.queue.drain(..).collect();
                for u in &updates {
                    self.handed_updates.insert(u.id, u.clone());
                }
                updates
            }
        }
    }
    pub fn begin_handler(&mut self, id: u64) -> Result<(), Error> {
        if self.handler.is_some() {
            return Err(Error::InvalidData);
        }
        self.handler = Some(id);
        Ok(())
    }
    pub fn end_handler(&mut self) -> Result<u64, Error> {
        let id = self.handler.take().ok_or(Error::InvalidData)?;
        self.observations.push(Observation::HandlerReturned { request_id: id });
        Ok(id)
    }
}

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TraceEvent {
    PublicationBinding {
        binding: super::BindingId,
        publish_step: Option<super::publication::PublishStep>,
    },
    IdentityVerified {
        instance: String,
        account: String,
    },
    ScanCommitted {
        instance: String,
        native_file: super::runtime::NativeFileIdentity,
        generation: u64,
        covered_seq: u64,
        recovered_names: Vec<String>,
    },
    EngineStarted {
        instance: String,
        account: String,
    },
    RootReadRecorded {
        instance: String,
        request_id: u64,
    },
    RootConfirmed {
        instance: String,
    },
    CollectionBatchBuilt {
        instance: String,
        names: BTreeSet<String>,
    },
    PublishStarted,
    AdoptStarted,
    CollectionReplaced,
    ManualTransferStarted,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoverageLedger {
    pub open: u64,
    pub cursor: super::runtime::NoteScanCursor,
    pub counts: Vec<(c::Kind, u64)>,
    pub media_feed_id: [u8; 16],
    pub media_cursor: u64,
}
impl CoverageLedger {
    pub(crate) fn validate(&self) -> Result<(), Error> {
        if self.cursor.covered_through < self.cursor.seq || self.cursor.native_file.created_nanos >= 1_000_000_000 {
            return Err(Error::InvalidData);
        }
        if self.counts.iter().map(|(k, _)| *k).collect::<Vec<_>>() != super::row_payload::KINDS {
            return Err(Error::InvalidData);
        }
        super::runtime_store::checked(self.open)?;
        super::runtime_store::checked(self.media_cursor)?;
        for (_, n) in &self.counts {
            super::runtime_store::checked(*n)?;
        }
        Ok(())
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RootAcknowledgement {
    pub open: u64,
    pub request_id: u64,
    pub epoch: super::EpochId,
    pub envelope: OpaqueBytes,
    pub system_fields: OpaqueBytes,
}

impl std::fmt::Debug for TraceEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CloudTraceEvent")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FetchPhase {
    BeforeSend,
    AfterSend,
}

macro_rules! opaque_debug {($($name:ty),+)=>{$(impl std::fmt::Debug for $name {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result{f.write_str(stringify!($name))}})+};}
opaque_debug!(Flight, EntityFacts, SaveInput, BuiltBatch, Saved);
