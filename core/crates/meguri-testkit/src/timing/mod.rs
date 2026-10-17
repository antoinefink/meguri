//! The Meguri side of the timed operations (`specs/conformance/timed-operations.md`,
//! U18): the catalogue (T6), the image and the collection summary (T2), the
//! protocol and statistic (T3, T4), the shared inputs (T5.4, T5.5) and the
//! report (T7). `meguri-cli timing run` drives it with the operations
//! Meguri's engine can run (T10); every other operation is `unsupported`.
//!
//! Timings are never goldens (T12). A report holds counts, times, operation
//! names and build identifiers only (R1940).

pub mod catalogue;
pub mod image;
pub mod package;
pub mod protocol;
pub mod report;
pub mod run;
pub mod stub;

pub use catalogue::Operation;
pub use image::Summary;
pub use protocol::{ErrorKind, Outcome, RunContext, RunFailure, Stage, TimedOperation, TimedRun};
pub use report::{MeguriBuild, Report};
pub use run::{RunError, Session, TimingRequest};
