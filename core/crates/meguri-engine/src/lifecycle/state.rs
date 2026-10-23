//! Lifecycle states (C7, `engine/lifecycle` L1) and the long operations of
//! the slot (L5, L9).

use std::fmt;

use meguri_storage::OpenError;

use crate::lifecycle::Generation;

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LifecycleState {
    NoCollection,
    /// The open gate or the open steps are running.
    Opening,
    /// The gate's upgrade runs the step that sets `col.ver` to `step`.
    Upgrading {
        from: i64,
        step: u32,
    },
    Ready {
        generation: Generation,
    },
    /// A replacement op's exclusive phase (L7).
    ExclusiveBusy {
        op: LongOp,
        phase: ExclusivePhase,
        cancellable: bool,
    },
    Failed {
        reason: FailedReason,
    },
}

impl LifecycleState {
    /// A short description for refusals and logs.
    pub fn describe(&self) -> String {
        match self {
            LifecycleState::NoCollection => "no collection is open".to_owned(),
            LifecycleState::Opening => "the collection is opening".to_owned(),
            LifecycleState::Upgrading { .. } => "the collection is being upgraded".to_owned(),
            LifecycleState::Ready { .. } => "the collection is ready".to_owned(),
            LifecycleState::ExclusiveBusy { op, .. } => format!("{} is running", op.name()),
            LifecycleState::Failed { reason } => format!("the collection failed: {reason}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FailedReason {
    /// The open gate refused or failed.
    Open(OpenError),
    /// An open step failed.
    OpenStep { step: String, detail: String },
    /// The file could not be reopened after a swap.
    Replace { detail: String },
    /// The collection is corrupt (set by U132).
    Corrupt { detail: String },
}

impl fmt::Display for FailedReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FailedReason::Open(error) => write!(f, "{error}"),
            FailedReason::OpenStep { step, detail } => write!(f, "open step {step}: {detail}"),
            FailedReason::Replace { detail } => write!(f, "reopening after a swap: {detail}"),
            FailedReason::Corrupt { detail } => write!(f, "corrupt: {detail}"),
        }
    }
}

/// Where an exclusive phase is (L7, L8).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExclusivePhase {
    /// Ops queued before the phase began are committing.
    Draining,
    /// The exclusive job works; only its ops commit.
    Working,
    /// The collection file is being swapped.
    Replacing,
}

/// The long operations of the slot (L5, R171).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum LongOp {
    OneWaySync,
    Restore,
    ColpkgImport,
    ApkgApply,
    CheckDatabase,
    /// Native Files selection/copy owns source admission until delegate completion.
    CollectionExportHandoff,
}

struct LongOpDeclaration {
    name: &'static str,
    replaces_collection: bool,
    allowed_when_corrupt: bool,
}

impl LongOp {
    fn declaration(self) -> LongOpDeclaration {
        match self {
            Self::OneWaySync => LongOpDeclaration {
                name: "one-way sync",
                replaces_collection: true,
                allowed_when_corrupt: true,
            },
            Self::Restore => LongOpDeclaration {
                name: "restore",
                replaces_collection: true,
                allowed_when_corrupt: true,
            },
            Self::ColpkgImport => LongOpDeclaration {
                name: ".colpkg import",
                replaces_collection: true,
                allowed_when_corrupt: false,
            },
            Self::ApkgApply => LongOpDeclaration {
                name: ".apkg import",
                replaces_collection: false,
                allowed_when_corrupt: false,
            },
            Self::CheckDatabase => LongOpDeclaration {
                name: "Check Database",
                replaces_collection: false,
                allowed_when_corrupt: false,
            },
            Self::CollectionExportHandoff => LongOpDeclaration {
                name: "collection export destination",
                replaces_collection: false,
                allowed_when_corrupt: false,
            },
        }
    }

    pub fn name(self) -> &'static str {
        self.declaration().name
    }

    /// One-way sync, restore and `.colpkg` import replace the collection and
    /// may enter the exclusive phase (L5, KTD17).
    pub fn replaces_collection(self) -> bool {
        self.declaration().replaces_collection
    }

    /// From `Failed{Corrupt}` only restore and the recovery download may
    /// run (L4).
    pub fn allowed_when_corrupt(self) -> bool {
        self.declaration().allowed_when_corrupt
    }
}
