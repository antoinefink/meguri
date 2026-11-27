//! Lossless lifecycle values. Availability waits use the engine's bounded wait.
use crate::{callback, error::CoreError, runtime};
use meguri_engine as engine;
use meguri_storage::{OpenError, UpgradeStage};

#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionOpenFailure {
    OpenInAnotherProgram,
    TooOld {
        version: i64,
    },
    TooNew {
        version: i64,
    },
    V1Scheduler,
    NotACollection {
        missing_tables: Vec<String>,
    },
    UpgradeFailed {
        version: i64,
        stage: CollectionUpgradeStage,
        detail: String,
    },
    Corrupt {
        detail: String,
    },
    ConnectionInit {
        detail: String,
    },
    NotFound,
    Io {
        detail: String,
    },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionUpgradeStage {
    Copy,
    Step { step: u32 },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionFailedReason {
    Open { error: CollectionOpenFailure },
    OpenStep { step: String, detail: String },
    Replace { detail: String },
    Corrupt { detail: String },
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionLongOp {
    OneWaySync,
    Restore,
    ColpkgImport,
    ApkgApply,
    CheckDatabase,
    CollectionExportHandoff,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionExclusivePhase {
    Draining,
    Working,
    Replacing,
}
#[derive(Clone, Debug, PartialEq, Eq, uniffi::Enum)]
pub enum CollectionLifecycle {
    NoCollection,
    Opening,
    Upgrading {
        from: i64,
        step: u32,
    },
    Ready {
        generation: u64,
    },
    ExclusiveBusy {
        op: CollectionLongOp,
        phase: CollectionExclusivePhase,
        cancellable: bool,
    },
    Failed {
        reason: CollectionFailedReason,
    },
}
impl From<OpenError> for CollectionOpenFailure {
    fn from(value: OpenError) -> Self {
        match value {
            OpenError::OpenInAnotherProgram => Self::OpenInAnotherProgram,
            OpenError::TooOld { version } => Self::TooOld { version },
            OpenError::TooNew { version } => Self::TooNew { version },
            OpenError::V1Scheduler => Self::V1Scheduler,
            OpenError::NotACollection { missing_tables } => Self::NotACollection { missing_tables },
            OpenError::UpgradeFailed { version, stage, detail } => Self::UpgradeFailed {
                version,
                detail,
                stage: match stage {
                    UpgradeStage::Copy => CollectionUpgradeStage::Copy,
                    UpgradeStage::Step(step) => CollectionUpgradeStage::Step { step },
                },
            },
            OpenError::Corrupt { detail } => Self::Corrupt { detail },
            OpenError::ConnectionInit { detail } => Self::ConnectionInit { detail },
            OpenError::NotFound => Self::NotFound,
            OpenError::Io { detail } => Self::Io { detail },
        }
    }
}
impl From<engine::LifecycleState> for CollectionLifecycle {
    fn from(value: engine::LifecycleState) -> Self {
        match value {
            engine::LifecycleState::NoCollection => Self::NoCollection,
            engine::LifecycleState::Opening => Self::Opening,
            engine::LifecycleState::Upgrading { from, step } => Self::Upgrading { from, step },
            engine::LifecycleState::Ready { generation } => Self::Ready {
                generation: generation.0,
            },
            engine::LifecycleState::ExclusiveBusy { op, phase, cancellable } => Self::ExclusiveBusy {
                cancellable,
                op: match op {
                    engine::LongOp::OneWaySync => CollectionLongOp::OneWaySync,
                    engine::LongOp::Restore => CollectionLongOp::Restore,
                    engine::LongOp::ColpkgImport => CollectionLongOp::ColpkgImport,
                    engine::LongOp::ApkgApply => CollectionLongOp::ApkgApply,
                    engine::LongOp::CheckDatabase => CollectionLongOp::CheckDatabase,
                    engine::LongOp::CollectionExportHandoff => CollectionLongOp::CollectionExportHandoff,
                    engine::LongOp::CloudAdopt => CollectionLongOp::ColpkgImport,
                },
                phase: match phase {
                    engine::ExclusivePhase::Draining => CollectionExclusivePhase::Draining,
                    engine::ExclusivePhase::Working => CollectionExclusivePhase::Working,
                    engine::ExclusivePhase::Replacing => CollectionExclusivePhase::Replacing,
                },
            },
            engine::LifecycleState::Failed { reason } => Self::Failed {
                reason: match reason {
                    engine::FailedReason::Open(error) => CollectionFailedReason::Open { error: error.into() },
                    engine::FailedReason::OpenStep { step, detail } => {
                        CollectionFailedReason::OpenStep { step, detail }
                    }
                    engine::FailedReason::Replace { detail } => CollectionFailedReason::Replace { detail },
                    engine::FailedReason::Corrupt { detail } => CollectionFailedReason::Corrupt { detail },
                },
            },
        }
    }
}
#[uniffi::export]
pub fn close_collection() -> Result<(), CoreError> {
    callback::enter("close_collection");
    runtime::close()
}
#[uniffi::export]
pub fn collection_lifecycle() -> Result<CollectionLifecycle, CoreError> {
    callback::enter("collection_lifecycle");
    Ok(runtime::engine()?.state().into())
}
#[uniffi::export]
pub fn wait_collection_available(timeout_ms: u32) -> Result<u64, CoreError> {
    callback::enter("wait_collection_available");
    Ok(runtime::engine()?
        .wait_available(std::time::Duration::from_millis(timeout_ms.into()))?
        .generation()
        .0)
}
