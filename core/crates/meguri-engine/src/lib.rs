//! The engine: op executor, change stream, jobs, ports, undo, lifecycle,
//! review session and the collection operations built on them.
//!
//! This crate's core (U24) is the single serialized write path (C1), the
//! ordered change stream and in-core observers (C2), the collection
//! lifecycle with generations and exclusive-op arbitration (C7), the job
//! registry, the ports, cancellation and progress; U25 adds undo and redo
//! (C3); U30 the tags, flags and marked notes (`specs/tags/`); U83 the
//! browser and search config writes (`specs/browser/config-keys.md`). Specs:
//! `specs/engine/op-model.md`, `change-stream.md`, `lifecycle.md`,
//! `jobs.md`, `undo.md`.

pub mod browser_config;
pub mod cancel;
pub mod changes;
pub mod collection;
pub mod error;
pub mod executor;
pub mod flags;
pub mod jobs;
pub mod lifecycle;
pub mod marked;
pub mod monotonic;
pub mod ports;
pub mod progress;
pub mod readers;
pub mod tags;
pub mod threads;
pub mod undo;

pub use cancel::CancelToken;
pub use changes::observers::{CardDelta, ConfigDelta, ObservedCommit};
pub use changes::stream::{ChangeStream, Event, EventKind, ExtensionEvent, Reservation, Seq, Subscription};
pub use collection::Collection;
pub use error::EngineError;
pub use executor::catalog::{OpDef, UndoClass};
pub use executor::op::{OpContext, OpResult, OpSpec, OpTiming, Pending, UndoMeta, WriteDelay};
pub use executor::plan_commit::{Plan, PlanContext, plan_and_commit, plan_and_commit_with};
pub use executor::preconditions::Precondition;
pub use jobs::registry::{JobRegistry, RunningJob};
pub use jobs::{JobContext, JobHandle, JobId, JobKind, JobOutcome};
pub use lifecycle::{
    Engine, EngineConfig, Exclusive, ExclusivePhase, FailedReason, Generation, LifecycleState, LongOp, OpenConfig,
    SlotGuard,
};
/// The change value types (KTD2), for crates that reach the engine only.
pub use meguri_model::changes::{ChangeKind, ChangeSet, OpSource};
pub use ports::{ChangeObserver, GenerationListener, OpenStep, PortKind, Ports, SuspendParticipant};
pub use progress::{Progress, ProgressListener, ProgressPhase, ProgressSink};
pub use readers::{ReaderLease, Snapshot};
pub use threads::{Qos, RegisteredThread, ThreadInfo, ThreadRegistry};
pub use undo::entry::{EntryId, RestorerState, SessionId, StepInfo, StepKind};
pub use undo::holdback::{HeldBackRun, HeldRow, SyncHold, SyncMerge};
pub use undo::reasons::{ClearCause, DisabledReason, SourceEffect, source_effect};
pub use undo::restorers::{Direction, RestoredStep, UndoRestorer};
pub use undo::{UndoScope, UndoStatus};
