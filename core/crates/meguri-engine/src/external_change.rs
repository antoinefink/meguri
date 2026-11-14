//! Conservative cache invalidation, serialized on the writer (suspend S3).
use crate::changes::{
    observers::{self, CommitPayload, ObservedCommit},
    stream::EventKind,
};
use crate::executor::{ExecShared, preconditions::TouchIndex};
use crate::{DisabledReason, EngineError};
use meguri_model::changes::{ChangeSet, OpSource};
use meguri_storage::{ReadRows, Writer};
use std::sync::Arc;

pub(crate) fn notify(shared: &ExecShared, writer: &Writer, touched: &mut TouchIndex) -> Result<(), EngineError> {
    let col = writer.col()?;
    if let Some(maintenance) = shared.maintenance.get().and_then(std::sync::Weak::upgrade) {
        maintenance.committed(shared.generation(), col.modified.0);
    }
    shared.undo.reset(DisabledReason::ChangedExternally);
    shared.day_clock.drop_cached();
    shared.answer_caches.invalidate_all();
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
            collection: Some(meguri_storage::derived::CollectionMark::of(&col)),
            payload: CommitPayload::external(seq, shared.generation(), col),
        },
        writer,
        None,
    );
    reservation.fill(shared.generation(), None, EventKind::Changes(changes));
    Ok(())
}
