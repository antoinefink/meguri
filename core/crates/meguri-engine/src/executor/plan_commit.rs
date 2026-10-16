//! Plan and commit (`engine/op-model` O6, O7): heavy ops compute on a reader
//! snapshot off the writer, cancellable and with progress, then commit in one
//! short write phase guarded by preconditions. A `Stale` precondition makes
//! the op plan again on a new snapshot, at most [`REPLANS`] more times.

use std::sync::Arc;

use meguri_storage::ReaderRole;

use crate::cancel::CancelToken;
use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::op::{OpContext, OpResult, OpSpec};
use crate::executor::preconditions::Precondition;
use crate::monotonic;
use crate::progress::{ProgressPhase, ProgressSink};
use crate::readers::Snapshot;

/// How many times a stale op is planned again before it returns `Stale`.
pub const REPLANS: u32 = 3;

/// What a plan step sees.
pub struct PlanContext<'a> {
    snapshot: &'a Snapshot<'a>,
    cancel: Option<&'a Arc<CancelToken>>,
    progress: Option<&'a ProgressSink>,
    attempt: u32,
}

impl<'a> PlanContext<'a> {
    pub fn snapshot(&self) -> &Snapshot<'a> {
        self.snapshot
    }

    /// The seq of the last commit the snapshot sees: the `since` of the
    /// plan's `Untouched` and `TableUntouched` preconditions.
    pub fn seq(&self) -> Seq {
        self.snapshot.seq()
    }

    /// `Err(Cancelled)` once the op's token was cancelled.
    pub fn check_cancel(&self) -> Result<(), EngineError> {
        self.cancel.map_or(Ok(()), |token| token.check())
    }

    pub fn progress(&self) -> Option<&ProgressSink> {
        self.progress
    }

    /// 0 for the first plan, then 1, 2, ... after each `Stale`.
    pub fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// A plan: the value the write phase applies and what must still hold then.
#[derive(Debug)]
pub struct Plan<P> {
    pub value: P,
    pub preconditions: Vec<Precondition>,
}

/// Plans on a snapshot of the `role` reader (the maintenance reader for most
/// ops, the sync reader for a sync) and commits through `collection`. The
/// spec's cancel token and progress sink serve both phases.
pub fn plan_and_commit<P, T, PF, CF>(
    collection: &Collection,
    role: ReaderRole,
    spec: OpSpec,
    mut plan: PF,
    commit: CF,
) -> Result<OpResult<T>, EngineError>
where
    P: Send + 'static,
    T: Send + 'static,
    PF: FnMut(&PlanContext<'_>) -> Result<Plan<P>, EngineError>,
    CF: Fn(&mut OpContext<'_, '_>, P) -> Result<T, EngineError> + Send + Sync + 'static,
{
    let commit = Arc::new(commit);
    let mut attempt = 0;
    loop {
        if let Some(token) = &spec.cancel {
            token.check()?;
        }
        if let Some(progress) = &spec.progress {
            progress.set_phase(ProgressPhase::Planning);
        }
        let (planned, plan_time) = {
            let lease = collection.reader(role)?;
            let snapshot = lease.snapshot()?;
            let started = monotonic::now();
            let context = PlanContext {
                snapshot: &snapshot,
                cancel: spec.cancel.as_ref(),
                progress: spec.progress.as_ref(),
                attempt,
            };
            let planned = plan(&context)?;
            (planned, monotonic::now().saturating_sub(started))
        };
        if let Some(token) = &spec.cancel {
            token.check()?;
        }
        let mut attempt_spec = spec.clone().preconditions(planned.preconditions);
        attempt_spec.plan_time = Some(plan_time);
        let commit = Arc::clone(&commit);
        let value = planned.value;
        match collection.run(attempt_spec, move |context| commit(context, value)) {
            Err(EngineError::Stale { .. }) if attempt < REPLANS => attempt += 1,
            other => return other,
        }
    }
}
