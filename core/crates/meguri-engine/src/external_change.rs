//! Conservative cache invalidation, serialized on the writer (suspend S3).
use crate::changes::{
    observers::{self, ObservedCommit},
    stream::EventKind,
};
use crate::executor::{ExecShared, preconditions::TouchIndex};
use crate::{DisabledReason, EngineError};
use meguri_model::changes::{ChangeSet, OpSource};
use meguri_storage::{ReadRows, Writer};
use std::sync::Arc;

pub(crate) fn notify(shared: &ExecShared, writer: &Writer, touched: &mut TouchIndex) -> Result<(), EngineError> {
    let col = writer.col()?;
    shared.undo.reset(DisabledReason::ChangedExternally);
    shared.day_clock.drop_cached();
    let mut changes = ChangeSet::empty(OpSource::Maintenance);
    changes.whole_collection = true;
    let changes = Arc::new(changes);
    let reservation = shared.stream.reserve();
    let seq = reservation.seq();
    *shared.lock_commit() = seq;
    touched.reset(seq);
    observers::notify(
        &shared.observers,
        &ObservedCommit {
            seq,
            generation: shared.generation(),
            changes: &changes,
            cards: &[],
            revlog_added: &[],
            revlog_removed: &[],
            config: &[],
            collection: Some(&col),
        },
    );
    reservation.fill(shared.generation(), None, EventKind::Changes(changes));
    Ok(())
}
