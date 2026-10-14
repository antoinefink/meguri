//! Deck config (preset) records (`specs/model/row-records.md` R7).

use crate::ids::DeckConfigId;
use crate::proto::ProtoBlob;
use crate::proto::deck_config::DeckConfigMessage;
use crate::time::TimestampSecs;
use crate::usn::Usn;

/// A `deck_config` row with its blob.
#[derive(Clone, Debug)]
pub struct DeckConfig {
    pub id: DeckConfigId,
    pub name: String,
    pub mtime: TimestampSecs,
    pub usn: Usn,
    pub config: ProtoBlob<DeckConfigMessage>,
}
