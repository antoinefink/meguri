//! Phase 2 of the open gate, the refusals (`storage/open-and-connections`
//! C1, C2.2): decided from the probe alone, before anything is written.

use crate::error::{OpenError, UpgradeStage};
use crate::schema::SCHEMA_VERSION;
use crate::schema::version::{Probed, Scheduler};

/// The versions the upgrade starts from.
pub(crate) const UPGRADABLE: [i64; 5] = [11, 14, 15, 16, 17];

/// C1: the refusal for a version, if any.
pub(crate) fn version(version: i64) -> Option<OpenError> {
    match version {
        v if v < 11 => Some(OpenError::TooOld { version: v }),
        v if v == SCHEMA_VERSION || UPGRADABLE.contains(&v) => None,
        v => Some(OpenError::TooNew { version: v }),
    }
}

/// C1, then C2.2.
pub(crate) fn refuse(probed: &Probed) -> Result<(), OpenError> {
    refuse_as(probed, false)
}

/// [`refuse`], where `package_v1` lets the v1 marker pass: the staged
/// collection of a deck package (`package/apkg-import` I3 step 3).
pub(crate) fn refuse_as(probed: &Probed, package_v1: bool) -> Result<(), OpenError> {
    if let Some(error) = version(probed.version) {
        return Err(error);
    }
    match probed.scheduler {
        Scheduler::V2 => Ok(()),
        Scheduler::V1 if package_v1 => Ok(()),
        Scheduler::V1 | Scheduler::NotRead => Err(OpenError::V1Scheduler),
        // The upgrade could not read it either (step 14 converts it).
        Scheduler::ConfNotAnObject => Err(OpenError::UpgradeFailed {
            version: probed.version,
            stage: UpgradeStage::Step(14),
            detail: "col.conf is not a JSON object".to_owned(),
        }),
    }
}
