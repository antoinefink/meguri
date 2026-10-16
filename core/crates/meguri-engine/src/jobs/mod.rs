//! Jobs (KTD9, `specs/engine/jobs.md`): long work started by a short call
//! that returns a [`JobHandle`] at once, run on the bounded pool, and ended by
//! exactly one `JobFinished` on the change stream.

pub mod pool;
pub mod registry;

use std::fmt;
use std::sync::Arc;

use crate::cancel::CancelToken;
use crate::error::EngineError;
use crate::lifecycle::{LongOp, SlotGuard};
use crate::progress::ProgressSink;

/// A job's id: unique for the life of the engine, from 1.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct JobId(pub u64);

impl fmt::Display for JobId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}

/// How a job ended.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum JobOutcome {
    Succeeded,
    /// The work returned `Cancelled`, or was cancelled before it started.
    Cancelled,
    /// The work failed; a panic is `Internal` with its message.
    Failed {
        error: EngineError,
    },
}

/// What a start call returns.
#[derive(Clone, Debug)]
pub struct JobHandle {
    pub id: JobId,
    pub cancel: Arc<CancelToken>,
    pub progress: ProgressSink,
}

/// What a job's work sees.
pub struct JobContext {
    pub id: JobId,
    pub cancel: Arc<CancelToken>,
    pub progress: ProgressSink,
    pub(crate) slot: Option<SlotGuard>,
}

impl fmt::Debug for JobContext {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("JobContext")
            .field("id", &self.id)
            .field("slot", &self.slot)
            .finish_non_exhaustive()
    }
}

impl JobContext {
    /// The long-operation slot the job took at its start (`engine/jobs` J3);
    /// replacement jobs enter their exclusive phase through it.
    pub fn slot(&self) -> Option<&SlotGuard> {
        self.slot.as_ref()
    }
}

/// What a job is, for arbitration (`engine/lifecycle` L9).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum JobKind {
    Open,
    Login,
    MetaProbe,
    NormalSync,
    MediaSync,
    OneWaySync,
    Restore,
    ColpkgImport,
    ApkgImport,
    Export,
    Backup,
    CheckDatabase,
    CheckMedia,
    EmptyCards,
    Optimizer,
    BulkOp,
}

impl JobKind {
    pub fn name(self) -> &'static str {
        match self {
            JobKind::Open => "open",
            JobKind::Login => "login",
            JobKind::MetaProbe => "meta",
            JobKind::NormalSync => "normal sync",
            JobKind::MediaSync => "media sync",
            JobKind::OneWaySync => "one-way sync",
            JobKind::Restore => "restore",
            JobKind::ColpkgImport => ".colpkg import",
            JobKind::ApkgImport => ".apkg import",
            JobKind::Export => "export",
            JobKind::Backup => "backup",
            JobKind::CheckDatabase => "Check Database",
            JobKind::CheckMedia => "Check Media",
            JobKind::EmptyCards => "Empty Cards",
            JobKind::Optimizer => "optimizer",
            JobKind::BulkOp => "bulk op",
        }
    }

    /// The slot the job takes when it starts, if any.
    pub fn long_op(self) -> Option<LongOp> {
        match self {
            JobKind::OneWaySync => Some(LongOp::OneWaySync),
            JobKind::Restore => Some(LongOp::Restore),
            JobKind::ColpkgImport => Some(LongOp::ColpkgImport),
            JobKind::ApkgImport => Some(LongOp::ApkgApply),
            JobKind::CheckDatabase => Some(LongOp::CheckDatabase),
            JobKind::Open
            | JobKind::Login
            | JobKind::MetaProbe
            | JobKind::NormalSync
            | JobKind::MediaSync
            | JobKind::Export
            | JobKind::Backup
            | JobKind::CheckMedia
            | JobKind::EmptyCards
            | JobKind::Optimizer
            | JobKind::BulkOp => None,
        }
    }
}
