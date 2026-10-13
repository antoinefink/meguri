//! The sync endpoint guard (R1876) for harnesses: every sync endpoint a test,
//! harness or adapter connects to passes [`check_endpoint`] first. The rule
//! itself is `meguri_sync::endpoint_guard` (feature `test-hooks`), which the
//! sync transport applies too, so there is one copy of it.

pub use meguri_sync::endpoint_guard::{ANKIWEB_HOST, Allowed, Reason, Refused, check_endpoint};
