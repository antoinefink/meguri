pub mod blob;
pub(crate) mod io;
pub(crate) mod store;
use super::{entity_codec as c, runtime::NativeFileIdentity, *};
use serde::{Deserialize, Serialize};
use std::{collections::BTreeMap, path::PathBuf};

pub const MAX_ASSET_BYTES: u64 = 104_857_600;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ServerState {
    #[default]
    Unknown,
    Present,
    Deleted,
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Transfer {
    #[default]
    None,
    Queued,
    Running,
    Staged,
    Complete,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlobState {
    pub metadata: blob::Blob,
    pub server: ServerState,
    pub upload: Transfer,
    pub download: Transfer,
    pub staged: Option<PathBuf>,
    pub recheck_at: Option<i64>,
    pub rechecked: bool,
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct NameState {
    pub local: Option<c::MediaName>,
    pub source: Option<(String, u64, i64, u32)>,
    pub file_missing: bool,
    pub unsyncable: bool,
    pub withheld: Option<[u8; 32]>,
    pub superseded: Option<[u8; 32]>,
    pub removal_held: bool,
    pub tombstone_at: Option<i64>,
    pub media_changed: Option<[u8; 32]>,
}
impl NameState {
    pub(crate) fn sendable(&self) -> bool {
        !self.file_missing
            && !self.unsyncable
            && self.withheld.is_none()
            && self.superseded.is_none()
            && !self.removal_held
            && self.media_changed.is_none()
    }
}
#[derive(Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct Ledger {
    #[serde(default)]
    pub revision: u64,
    pub feed: Option<([u8; 16], u64)>,
    pub names: BTreeMap<String, NameState>,
    pub blobs: BTreeMap<String, BlobState>,
    pub adoption: bool,
    pub zone_missing: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Authority {
    pub binding: BindingId,
    pub open: u64,
    pub incarnation: u64,
    pub generation: u64,
    pub file: NativeFileIdentity,
    pub account: OpaqueBytes,
    pub adapter_token: u64,
    pub media_epoch: u64,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    Root,
    Read {
        name: String,
    },
    Upload {
        name: String,
        envelope: Vec<u8>,
        staged: PathBuf,
    },
    Download {
        name: String,
    },
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Work {
    pub id: u64,
    pub authority: Authority,
    pub request: Request,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reply {
    Record {
        name: String,
        record_type: String,
        envelope: Vec<u8>,
        asset: Option<PathBuf>,
    },
    Saved {
        name: String,
        envelope: Vec<u8>,
    },
    UnknownItem,
    ZoneMissing,
    Failed {
        code: i64,
    },
}
#[derive(Default)]
pub(crate) struct Process {
    pub epoch: u64,
    pub pending: Option<Work>,
    pub next: u64,
    pub root_checked: bool,
    pub recovered: bool,
    pub paused: bool,
    pub no_space: bool,
    pub attempted: std::collections::BTreeSet<String>,
    pub pending_notification: bool,
}
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Apply {
    pub incoming: workflow::Saved,
    pub server: Option<c::MediaName>,
    pub old_name: String,
    pub before: Option<c::MediaName>,
    pub moved: Option<c::MediaName>,
    pub moved_before: Option<c::MediaName>,
    #[serde(default)]
    pub move_evidence: Option<meguri_storage::media::CloudMoveEvidence>,
    #[serde(default)]
    pub install_absent: bool,
    pub staged: Option<PathBuf>,
    pub before_referenced_new: BTreeMap<String, bool>,
    #[serde(default)]
    pub local_wins: bool,
    #[serde(default)]
    pub delete_wins: bool,
    pub installed: bool,
    pub abandoned: bool,
}
macro_rules! opaque {($($t:ty),*)=>{$(impl std::fmt::Debug for $t {fn fmt(&self,f:&mut std::fmt::Formatter<'_>)->std::fmt::Result {f.write_str(stringify!($t))}})*};}
opaque!(BlobState, NameState, Authority, Request, Work, Reply, Apply);
