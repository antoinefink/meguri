//! The bulk action framework (U86, `specs/browser/bulk-framework.md`): how
//! Browse's actions over many notes or cards run.
//!
//! - **Targets.** Explicit ids, every note, or a selection of a search
//!   handle's index ranges (`browser/result-sets` H12), which the job
//!   resolves to ids in the core, so large selections never cross the FFI.
//! - **Job.** A short [`start`] returns a [`BulkJob`] at once; the work runs
//!   on the engine's job pool (`engine/jobs` J1) and ends with exactly one
//!   `JobFinished`. The job's result ([`BulkOutcome`]: the counts, the
//!   commit's seq and change set, the undo step) is in the job before that
//!   event is published.
//! - **Phases.** A compute phase on a maintenance snapshot reads and plans
//!   (cancellable, progress in `Planning`), then one write transaction
//!   commits (`engine/op-model` O6, O7): progress shows `Committing` and a
//!   cancel no longer stops it. Above [`LONG_WRITE_ROWS`] the write is the
//!   catalog's `bulk_op` with a declared long write (O8, O9). A cancel or a
//!   refusal before the commit writes nothing and leaves both undo
//!   histories as they were (`engine/undo` M2).
//! - **Undo.** The commit is one op, so one undo step named after the
//!   action (`browser/bulk-actions` B16). An action that writes nothing
//!   records no step and keeps `col.mod`, and like every user op it empties
//!   the redo history (`engine/undo` A2).
//! - **Counts.** `{affected, considered}` (B16): the rows written and the
//!   ids the action was given.
//!
//! Find and Replace (`browser/find-replace`) is [`find_replace`]; the field
//! names and note types of a target are [`summary`].

pub mod find_replace;
pub mod summary;

use std::sync::{Arc, Condvar, Mutex, PoisonError};
use std::time::Duration;

use meguri_model::changes::{ChangeSet, OpSource};
use meguri_model::ids::{CardId, NoteId};

use crate::browse::search::{BrowseError, HandleSelection, SearchMode};
use crate::cancel::CancelToken;
use crate::changes::stream::Seq;
use crate::collection::Collection;
use crate::error::EngineError;
use crate::executor::catalog;
use crate::executor::op::{OpResult, OpSpec, UndoMeta};
use crate::jobs::{JobContext, JobHandle, JobId, JobKind};
use crate::lifecycle::Engine;
use crate::progress::ProgressSink;
use crate::undo::entry::StepKind;

/// Above this many rows to write, a bulk action runs as the catalog's
/// `bulk_op` and declares its write (`engine/op-model` O8, O9): the tag
/// ops' threshold (`tags::LONG_WRITE_ROWS`).
pub const LONG_WRITE_ROWS: usize = crate::tags::LONG_WRITE_ROWS;

/// The write time declared per row: a note read again, rewritten, compared,
/// stamped, journalled and given its missing cards.
const WRITE_PER_ROW: Duration = Duration::from_micros(60);

/// What every bulk action reports (`browser/bulk-actions` B16).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct BulkCounts {
    /// The rows the action wrote (notes for note actions).
    pub affected: u64,
    /// The ids the action was given, deleted ones included; for an
    /// all-notes target, the notes it read.
    pub considered: u64,
}

/// A finished bulk action's result.
#[derive(Clone, Debug)]
pub struct BulkOutcome {
    pub counts: BulkCounts,
    /// The commit's seq: the step's id when one was recorded.
    pub seq: Seq,
    /// What the commit changed, for the table's refresh (R1185).
    pub changes: Arc<ChangeSet>,
    /// The undo step, `None` when the action wrote nothing.
    pub undo: Option<UndoMeta>,
    /// A cancel came after the write phase started; the action completed
    /// anyway (O5).
    pub past_cancel_point: bool,
}

impl BulkOutcome {
    pub fn of(result: OpResult<BulkCounts>) -> BulkOutcome {
        BulkOutcome {
            counts: result.value,
            seq: result.seq,
            changes: result.changes,
            undo: result.undo,
            past_cancel_point: result.past_cancel_point,
        }
    }
}

/// The notes a note action works on.
#[derive(Clone, Debug)]
pub enum NoteTarget {
    /// These notes, in this order.
    Notes(Vec<NoteId>),
    /// Every note of the collection when the compute phase reads it.
    AllNotes,
    /// A selection of a search handle: its notes (H12's target notes).
    Selection(HandleSelection),
}

/// A note target with its selection resolved.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResolvedNotes {
    Notes(Vec<NoteId>),
    AllNotes,
}

impl NoteTarget {
    /// Resolves a selection to its note ids (H12), on the query reader; a
    /// selection of a handle of another collection is refused.
    pub fn resolve(&self, collection: &Collection) -> Result<ResolvedNotes, EngineError> {
        Ok(match self {
            NoteTarget::Notes(ids) => ResolvedNotes::Notes(ids.clone()),
            NoteTarget::AllNotes => ResolvedNotes::AllNotes,
            NoteTarget::Selection(selection) => ResolvedNotes::Notes(
                selection
                    .resolve_for(collection, SearchMode::Notes)
                    .map_err(selection_error)?
                    .into_iter()
                    .map(NoteId)
                    .collect(),
            ),
        })
    }
}

/// The cards a card action works on (U87's actions).
#[derive(Clone, Debug)]
pub enum CardTarget {
    Cards(Vec<CardId>),
    /// A selection of a search handle: its cards (H12's target cards, every
    /// card of each selected note in notes mode).
    Selection(HandleSelection),
}

impl CardTarget {
    /// Resolves a selection to its card ids (H12), on the query reader; a
    /// selection of a handle of another collection is refused.
    pub fn resolve(&self, collection: &Collection) -> Result<Vec<CardId>, EngineError> {
        Ok(match self {
            CardTarget::Cards(ids) => ids.clone(),
            CardTarget::Selection(selection) => selection
                .resolve_for(collection, SearchMode::Cards)
                .map_err(selection_error)?
                .into_iter()
                .map(CardId)
                .collect(),
        })
    }
}

fn selection_error(error: BrowseError) -> EngineError {
    match error {
        BrowseError::Engine(error) => error,
        other => EngineError::InvalidArgument {
            message: format!("the selection cannot be resolved: {other:?}"),
        },
    }
}

/// `InvalidArgument` unless `ids` is non-empty and names each id once (the
/// engine invariant of `browser/bulk-actions` B7.2 and `browser/find-replace`
/// F11).
pub fn check_ids<T: Ord + Copy>(ids: &[T], what: &str) -> Result<(), EngineError> {
    if ids.is_empty() {
        return Err(EngineError::InvalidArgument {
            message: format!("a bulk action needs at least one {what}"),
        });
    }
    let mut sorted = ids.to_vec();
    sorted.sort_unstable();
    if sorted.windows(2).any(|pair| pair[0] == pair[1]) {
        return Err(EngineError::InvalidArgument {
            message: format!("a bulk action names a {what} twice"),
        });
    }
    Ok(())
}

/// The spec of a bulk action's commit, which writes about `rows` rows and
/// records the undo step `step`: the plain op `name`, or above
/// [`LONG_WRITE_ROWS`] the catalog's `bulk_op` declaring its write.
pub fn op_spec(
    name: &'static str,
    step: &'static str,
    rows: usize,
    cancel: &Arc<CancelToken>,
    progress: &ProgressSink,
) -> OpSpec {
    let spec = if rows > LONG_WRITE_ROWS {
        let expected = WRITE_PER_ROW.saturating_mul(u32::try_from(rows).unwrap_or(u32::MAX));
        OpSpec::new(catalog::BULK_OP, OpSource::UserUndoable).declare_long_write(expected)
    } else {
        OpSpec::named(name, OpSource::UserUndoable)
    };
    spec.undo_step(step, StepKind::Other)
        .cancel(Arc::clone(cancel))
        .progress(progress.clone())
}

type Slot = Arc<(Mutex<Option<Result<BulkOutcome, EngineError>>>, Condvar)>;

/// A started bulk action: its job handle (id, cancel token, progress) and,
/// once the work has ended, its result. The result is set before the job's
/// `JobFinished` is published.
#[derive(Clone, Debug)]
pub struct BulkJob {
    handle: JobHandle,
    slot: Slot,
}

impl BulkJob {
    pub fn handle(&self) -> &JobHandle {
        &self.handle
    }

    pub fn id(&self) -> JobId {
        self.handle.id
    }

    /// Requests cancellation: honoured until the commit starts (O5).
    pub fn cancel(&self) {
        self.handle.cancel.cancel();
    }

    /// The result, once the work has ended.
    pub fn outcome(&self) -> Option<Result<BulkOutcome, EngineError>> {
        self.slot.0.lock().unwrap_or_else(PoisonError::into_inner).clone()
    }

    /// Waits at most `timeout` for the result.
    pub fn wait(&self, timeout: Duration) -> Option<Result<BulkOutcome, EngineError>> {
        let (lock, ready) = &*self.slot;
        let guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        let (guard, _) = ready
            .wait_timeout_while(guard, timeout, |result| result.is_none())
            .unwrap_or_else(PoisonError::into_inner);
        guard.clone()
    }
}

/// Fills the slot once. Dropped unfilled, it records why: a job cancelled
/// before it started (the registry then drops its work unrun, also while
/// admission is paused) is `Cancelled`; work that started and ended without
/// a result panicked.
struct Completion {
    slot: Slot,
    started: bool,
}

impl Completion {
    fn set(&self, result: Result<BulkOutcome, EngineError>) {
        let (lock, ready) = &*self.slot;
        let mut guard = lock.lock().unwrap_or_else(PoisonError::into_inner);
        if guard.is_none() {
            *guard = Some(result);
        }
        ready.notify_all();
    }
}

impl Drop for Completion {
    fn drop(&mut self) {
        let error = if self.started {
            EngineError::internal("the bulk job ended without a result")
        } else {
            EngineError::Cancelled
        };
        self.set(Err(error));
    }
}

/// Starts a bulk action as a job of kind `BulkOp` and returns at once.
/// `work` runs on the job pool with the job's cancel token and progress
/// sink; its result becomes the job's outcome (`Cancelled` for a cancel)
/// and is kept for [`BulkJob::outcome`].
pub fn start(
    engine: &Engine,
    work: impl FnOnce(&JobContext) -> Result<BulkOutcome, EngineError> + Send + 'static,
) -> Result<BulkJob, EngineError> {
    let slot: Slot = Arc::new((Mutex::new(None), Condvar::new()));
    let mut completion = Completion {
        slot: Arc::clone(&slot),
        started: false,
    };
    let handle = engine.start_job(JobKind::BulkOp, move |context| {
        completion.started = true;
        let result = work(context);
        completion.set(result.clone());
        result.map(|_| ())
    })?;
    Ok(BulkJob { handle, slot })
}
