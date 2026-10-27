//! The shared automatic/scenario primitive; no cadence or success recording here.
use crate::{CancelToken, Collection, EngineError, executor::catalog};
use std::{sync::Arc, time::Duration};

pub fn optimize(collection: &Collection, cancel: &Arc<CancelToken>) -> Result<(), EngineError> {
    for (def, statement) in [
        (catalog::VACUUM, 0),
        (catalog::ANALYZE, 1),
        (catalog::WAL_CHECKPOINT, 2),
    ] {
        cancel.check()?;
        let token = cancel.clone();
        let exec = collection.core.exec.clone();
        let result = collection
            .submit_maintenance(def, Duration::from_secs(30), move |writer| {
                token.check()?;
                let result = match statement {
                    0 => writer.vacuum(&|| token.is_cancelled()),
                    1 => writer.analyze(&|| token.is_cancelled()),
                    // Busy is a successful, incomplete checkpoint (M8.3). Never
                    // wait behind a reader while a session is trying to start.
                    _ => writer.checkpoint_with_timeout(Duration::ZERO).map(drop),
                };
                if super::integrity::is_corruption(result.as_ref().err()) {
                    exec.set_admission(crate::executor::exclusive::Admission::Corrupt);
                    return result.map_err(Into::into);
                }
                token.check()?;
                result.map_err(Into::into)
            })
            .wait();
        if matches!(&result, Err(EngineError::Storage(error)) if super::integrity::is_corruption(Some(error)))
            && let Some(owner) = collection
                .core
                .exec
                .maintenance
                .get()
                .and_then(std::sync::Weak::upgrade)
        {
            owner.report_corruption(collection);
        }
        result?;
    }
    cancel.check()
}
