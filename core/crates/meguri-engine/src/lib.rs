//! The engine: op executor, change stream, jobs, ports, undo, lifecycle,
//! review session and the collection operations built on them.
//!
//! This crate's core (U24) is the single serialized write path (C1), the
//! ordered change stream and in-core observers (C2), the collection
//! lifecycle with generations and exclusive-op arbitration (C7), the job
//! registry, the ports, cancellation and progress. Specs:
//! `specs/engine/op-model.md`, `change-stream.md`, `lifecycle.md`, `jobs.md`.

pub mod cancel;
pub mod changes;
pub mod collection;
pub mod error;
pub mod executor;
pub mod jobs;
pub mod lifecycle;
pub mod monotonic;
pub mod ports;
pub mod progress;
pub mod readers;
pub mod threads;

pub use cancel::CancelToken;
pub use changes::observers::{CardDelta, ConfigDelta, ObservedCommit};
pub use changes::stream::{ChangeStream, Event, EventKind, ExtensionEvent, Reservation, Seq, Subscription};
pub use collection::Collection;
pub use error::EngineError;
pub use executor::catalog::OpDef;
pub use executor::op::{OpContext, OpResult, OpSpec, OpTiming, Pending, UndoMeta, WriteDelay};
pub use executor::plan_commit::{Plan, PlanContext, plan_and_commit};
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
