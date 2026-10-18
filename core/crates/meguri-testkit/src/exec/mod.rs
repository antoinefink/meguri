//! Executors run scenarios and return their observations (dsl.md D8). The
//! oracle executor drives oracle workers; Meguri's runs the core in-process
//! through per-area adapters registered in a capabilities registry. Live
//! scenarios run as a chain of segments on both (dsl.md D9, `live`), actor
//! scenarios through the parties executor (sync-ops.md S10, `parties`).

mod browser;
mod cards;
pub mod clock;
mod decks;
pub mod live;
mod media;
mod meguri;
mod model;
mod notes;
mod notetypes;
mod oracle;
pub mod parties;
mod render;
pub mod sched;
mod search;
mod storage;
pub mod sync_guard;
mod tags;
mod text;

use std::fmt;
use std::ops::Range;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::scenario::LoadedScenario;
pub use crate::scenario::Needs;
pub use meguri::{
    FinishFn, HandoffFn, MeguriExecutor, NewCollectionFn, NewIds, ObserveFn, OpFn, OpResult, RawDumpFn, Registry, Seat,
    Session, SetupFn, SyncAccount, actor_seed,
};
pub use oracle::{Hello, OracleExecutor};
pub use storage::{Storage, db_dump_table, db_exec_rows, normalize_sql};
pub use tags::tag_tree_value;

/// A failed run: a protocol error kind (`unknown_ref`, `invalid_scenario`,
/// `unsupported_op`, `anki_error`, `worker_died`, ...), the step when known and
/// text for people, which is never compared. A rejection recorded as an op's
/// output (dsl.md D7.3) is not an error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecError {
    pub kind: String,
    pub step: Option<usize>,
    pub detail: String,
}

impl ExecError {
    pub fn new(kind: impl Into<String>, detail: impl Into<String>) -> ExecError {
        ExecError {
            kind: kind.into(),
            step: None,
            detail: detail.into(),
        }
    }

    pub fn at_step(mut self, step: usize) -> ExecError {
        self.step.get_or_insert(step);
        self
    }
}

impl fmt::Display for ExecError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.kind)?;
        if let Some(step) = self.step {
            write!(f, " at step {step}")?;
        }
        if !self.detail.is_empty() {
            write!(f, ": {}", self.detail)?;
        }
        Ok(())
    }
}

impl std::error::Error for ExecError {}

/// Something that runs scenarios.
pub trait Executor: Sync {
    /// `meguri` or `oracle`.
    fn name(&self) -> &str;

    /// What of `needs` this executor cannot do; empty when it can run it.
    fn missing(&self, needs: &Needs) -> Needs;

    /// Runs the scenario on a fresh collection: `{"steps": [...]}`.
    fn run(&self, scenario: &LoadedScenario) -> Result<Value, ExecError>;
}

/// What crosses a handoff (dsl.md D9): the session directory copied to
/// `dir` under the handoff directory (the collection file
/// `collection.anki2`, its media and every file an op wrote), the clock and
/// the real ids behind the logical ones. protocol.json `handoff_state`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Handoff {
    /// Relative to the handoff directory, `/`-separated.
    pub dir: String,
    /// Epoch seconds.
    pub now: i64,
    pub notes: Vec<i64>,
    pub cards: Vec<i64>,
}

/// One segment of a live chain for one side to run.
#[derive(Debug, Clone)]
pub struct SegmentRequest {
    /// Where the previous segment handed over; `None` for the first, which
    /// runs the setup on a fresh collection.
    pub resume: Option<Handoff>,
    /// The scenario steps to run.
    pub steps: Range<usize>,
    /// When set, the segment ends by handing the session over into this
    /// directory (relative to the handoff directory).
    pub hand_off_to: Option<String>,
    /// The handoff directory as this process sees it. An oracle worker sees
    /// the same directory at its own mount.
    pub root: PathBuf,
}

/// What a segment produced: one output per step it ran, and the handoff
/// state when it handed over.
#[derive(Debug, Clone)]
pub struct SegmentOutput {
    pub steps: Vec<Value>,
    pub handoff: Option<Handoff>,
}

/// An executor that can run one segment of a live scenario.
pub trait SegmentExecutor: Executor {
    fn segment(&self, scenario: &LoadedScenario, request: &SegmentRequest) -> Result<SegmentOutput, ExecError>;

    /// A worker of the zone `tz`, held for one interactive session
    /// (protocol.json `begin`, `step`, `end`): how the parties executor runs
    /// actor scenarios (sync-ops.md S10.4). Only the oracle has them.
    fn interactive(&self, tz: &str) -> Result<Box<dyn Interactive + '_>, ExecError> {
        Err(ExecError::new(
            "unsupported_op",
            format!("the {} executor runs no interactive sessions ({tz})", self.name()),
        ))
    }
}

/// One worker for one interactive session. Dropping it before a successful
/// `end` stops the worker instead of reusing it, so no session stays open.
pub trait Interactive {
    /// Sends one request; an error response is an [`ExecError`] with the
    /// protocol's kind and step.
    fn request(&mut self, request: &Value) -> Result<Value, ExecError>;
}
