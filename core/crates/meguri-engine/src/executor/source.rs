//! What an op's source decides (`engine/op-model` O1, O2): its queue and its
//! default stamping mode. Undo effects per source are U25's.

use meguri_model::changes::OpSource;
use meguri_storage::StampMode;

/// The stamping mode an op of `source` writes with unless its spec sets
/// another: incoming sync rows keep the server's `usn` and mtime; every other
/// write is a local change.
pub fn default_stamp(source: OpSource) -> StampMode {
    match source {
        OpSource::SyncIncoming | OpSource::CloudIncoming => StampMode::CallerSupplied,
        OpSource::UserUndoable
        | OpSource::UserNonUndoable
        | OpSource::LocalApi
        | OpSource::Optimizer
        | OpSource::Rollover
        | OpSource::Maintenance
        | OpSource::CloudRekey => StampMode::Local,
    }
}

/// Which queue an op of `source` waits in.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QueueKind {
    /// User writes and local-server writes, one FIFO, taken first (R1790).
    Interactive,
    /// Sync merges, optimizer, rollover and maintenance, and maintenance
    /// tasks: taken only when no interactive op waits.
    Background,
}

pub fn queue_of(source: OpSource) -> QueueKind {
    if source.is_interactive() {
        QueueKind::Interactive
    } else {
        QueueKind::Background
    }
}
