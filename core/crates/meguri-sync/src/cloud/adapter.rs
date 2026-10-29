//! SDK delivery authority and durable adapter observations. Network scheduling
//! stays with the SDK; pending and exchange decisions stay with Runtime.
use super::{workflow::*, *};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Guard {
    pub generation: u64,
    pub binding: Vec<u8>,
    pub token: u64,
}
#[derive(Clone)]
pub(crate) struct SDKAuthority {
    pub id: String,
    pub guard: Guard,
    pub stopping: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FetchDrain {
    pub zone: String,
    pub fetched: bool,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum CollectionOutcome {
    Succeeded,
    WaitingNoAccount,
    Incomplete,
    Failed,
    Cancelled,
}
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompletedExchange {
    pub id: u64,
    pub collection: CollectionOutcome,
    pub result: ExchangeResult,
}
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub(crate) struct Durable {
    pub active: Option<Saved>,
    pub active_deleted: bool,
    pub absence: Option<AbsenceLedger>,
    pub drains: Vec<FetchDrain>,
    pub exchanges: BTreeMap<u64, CompletedExchange>,
    pub exchange_ended_ms: BTreeMap<u64, i64>,
}
#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct AbsenceLedger {
    pub pass: [u8; 16],
    pub completed: bool,
}
#[derive(Clone, PartialEq, Eq)]
pub struct Snapshot {
    pub binding: Vec<u8>,
    pub tracking_content: Vec<u8>,
    pub serialization: Option<Vec<u8>>,
    pub entities: Vec<EntityFacts>,
    pub inbox: Vec<InboxEntry>,
    pub root: Option<RootAcknowledgement>,
    pub tracking_stopped: bool,
    pub absence_pass_completed: bool,
}
#[derive(Default)]
pub(crate) struct FetchBoundary {
    pub group: String,
    pub manual: bool,
    pub zones: BTreeMap<String, (u64, bool)>,
}
#[derive(Default)]
pub(crate) struct FetchCall {
    pub id: u64,
    pub scope: Vec<String>,
    pub boundary: Option<FetchBoundary>,
    pub completed: bool,
    pub drains: BTreeSet<String>,
}
#[derive(Default)]
pub(crate) struct AdapterState {
    pub sdk: Option<SDKAuthority>,
    pub sdk_retrying: bool,
    pub next_sdk: u64,
    pub token: u64,
    pub paused: bool,
    pub durable: Durable,
    pub fetch: Option<FetchCall>,
    pub scheduled: Option<FetchBoundary>,
    pub next_fetch: u64,
    pub exchange_owner: Option<(u64, String, u64)>,
    pub closed_exchanges: BTreeMap<u64, (String, CompletedExchange)>,
    pub next_identity: u64,
    pub identity_read: Option<(u64, Guard, u64, Option<String>)>,
    pub no_state: bool,
    pub no_state_point: u64,
    pub missing_did: u64,
    pub retain_inspection: bool,
    pub closed_snapshot: Option<Snapshot>,
    #[cfg(feature = "test-hooks")]
    pub fail_inbox: bool,
    #[cfg(feature = "test-hooks")]
    pub fail_state: bool,
}

/// K2/K4.1 collection zone, using the epoch bytes without changing identity.
pub fn collection_zone(epoch: EpochId) -> String {
    format!(
        "c-{}",
        data_encoding::BASE32_NOPAD.encode(&epoch.0).to_ascii_lowercase()
    )
}
