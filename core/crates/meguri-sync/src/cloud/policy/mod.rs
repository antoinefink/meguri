//! CloudKit P3/P4 policy. SDK scheduling, capture, undo and apply remain Runtime-owned.
//! Partial and unaccepted: complete attention/media projection and cleanup remain outstanding.
//! Unknown attention withholds UpToDate; P24/F5 and whole-policy acceptance are still pending.
mod driver;
mod status;
mod transition;
mod types;
pub use driver::{Command, DeviceEvent, Driver, DriverError, PolicyWork};
#[cfg(feature = "test-hooks")]
pub use driver::{ExchangeObservation, OriginalExchangeWork};
pub use transition::step;
pub use types::*;

#[derive(Default)]
pub(crate) struct RuntimeEvidenceState {
    pub flights: std::collections::BTreeMap<(String, u64), std::collections::BTreeSet<String>>,
    pub parked: std::collections::BTreeSet<String>,
    pub held_images: std::collections::BTreeSet<String>,
    pub rollovers: std::collections::BTreeSet<(String, u64)>,
    pub rollovers_running: std::collections::BTreeSet<(String, u64)>,
}
/// Atomically read Runtime/tracking projection. Session is the real ReviewService registry value.
pub struct Projection {
    pub facts: CoreFacts,
    pub(crate) held_pending: std::collections::BTreeSet<String>,
    pub evidence: Evidence,
    pub engine: Option<Engine>,
    pub tracking_stopped: bool,
    pub root_confirmed: bool,
    pub drains: Vec<crate::cloud::adapter::FetchDrain>,
    pub(crate) cleanup_drain: Option<crate::cloud::publication::CleanupDrain>,
    pub backend_guard: crate::cloud::adapter::Guard,
}
