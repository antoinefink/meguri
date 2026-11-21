//! The engine: op executor, change stream, jobs, ports, undo, lifecycle,
//! review session and the collection operations built on them.
//!
//! This crate's core (U24) is the single serialized write path (C1), the
//! ordered change stream and in-core observers (C2), the collection
//! lifecycle with generations and exclusive-op arbitration (C7), the job
//! registry, the ports, cancellation and progress; U25 adds undo and redo
//! (C3); U26 the derived store's host (`specs/storage/derived-store.md`);
//! U30 the tags, flags and marked notes (`specs/tags/`); U40 the day clock
//! (C6, `sched`); U83 the browser and search config writes
//! (`specs/browser/config-keys.md`); U29 note types and the schema-change
//! gate (`notetypes`, `schema_gate`). Specs: `specs/engine/op-model.md`,
//! `change-stream.md`, `lifecycle.md`, `jobs.md`, `undo.md`,
//! `specs/sched/day-clock.md`, `specs/notetypes/`.

pub mod activity;
pub mod backup_limits;
pub mod bootstrap;
pub mod browse;
pub mod browser_config;
pub mod cancel;
pub mod cardgen;
pub mod cards;
pub mod changes;
pub mod collection;
pub mod deck_config;
pub mod decks;
pub mod derived_host;
pub mod device_state;
pub mod editor;
pub mod error;
pub mod executor;
pub mod external_change;
pub mod flags;
pub mod image_occlusion;
pub mod jobs;
pub mod lifecycle;
pub mod maintenance;
pub mod marked;
pub mod media;
pub mod monotonic;
pub mod notes;
pub mod notetypes;
pub mod ports;
pub mod progress;
pub mod readers;
pub mod rendering;
pub mod replace;
mod safety;
pub mod sched;
pub mod schema_gate;
pub mod session;
pub mod studied;
pub mod suspend;
pub mod tags;
pub mod threads;
pub mod undo;

pub use cancel::{CancelToken, StopCause};
pub use changes::observers::{CardDelta, CommitPayload, ConfigDelta, ObservedCommit, ObserverRows};
pub use changes::stream::{ChangeStream, Event, EventKind, ExtensionEvent, Reservation, Seq, Subscription};
pub use collection::{Collection, OpenEvidence};
pub use derived_host::{DerivedApplied, DerivedHost, DerivedReadiness, Readiness};
pub use error::EngineError;
pub use executor::catalog::{OpDef, UndoClass};
pub use executor::op::{OpContext, OpResult, OpSpec, OpTiming, Pending, UndoMeta, WriteDelay};
pub use executor::plan_commit::{Plan, PlanContext, plan_and_commit, plan_and_commit_with, plan_and_commit_with_replans};
pub use executor::preconditions::Precondition;
pub use jobs::registry::{JobRegistry, RunningJob};
pub use jobs::{JobContext, JobHandle, JobId, JobKind, JobOutcome};
pub use lifecycle::{
    Engine, EngineConfig, Exclusive, ExclusivePhase, FailedReason, Generation, LifecycleState, LongOp, OpenConfig,
    SlotGuard, WeakEngine,
};
/// The change value types (KTD2), for crates that reach the engine only.
pub use meguri_model::changes::{ChangeKind, ChangeSet, OpSource};
pub use ports::{ChangeObserver, GenerationListener, OpenStep, PortKind, Ports, SuspendParticipant};
pub use progress::{Progress, ProgressListener, ProgressPhase, ProgressSink};
pub use readers::{ReaderLease, ReaderLifetimePin, Snapshot};
pub use sched::{DayCache, DayClock, DayQuery, DayReport, day_query};
pub use schema_gate::{SchemaReason, mark_schema_change};
pub use threads::{Qos, RegisteredThread, ThreadInfo, ThreadRegistry};
pub use undo::entry::{EntryId, RestorerState, SessionId, StepInfo, StepKind};
pub use undo::holdback::{CloudHold, HeldBackRun, HeldRow, SyncHold, SyncMerge};
pub use undo::reasons::{ClearCause, DisabledReason, SourceEffect, source_effect};
pub use undo::restorers::{Direction, RestoredStep, UndoRestorer};
pub use undo::{UndoScope, UndoStatus};

pub use ports::DiskSpaceProvider;
pub use suspend::{ResumeReport, SuspendOutcome};

mod wal_coverage;
pub use wal_coverage::{WalCoverage, WalIdentity, WalReceipt};
