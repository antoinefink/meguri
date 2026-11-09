//! Shared fake CloudKit service foundation (conformance/cloudkit-acceptance Q2,
//! Q3.1, Q3.2, Q3.6, Q3.7). The Service itself models server state only.
//!
//! Each request commits scoped rows, change references and an outcome reference
//! in one transaction in generated `fake-cloud.db`. Payload bodies are shared
//! by content hash. Versions and snapshot memberships remain available for replay;
//! unique writes and fresh snapshots retain metadata, without copying bodies.
//!
//! Direct replies contain records. Outcome inspection returns typed references,
//! with log/outcome pages and explicit scoped single-version body resolution.
//! Compatibility inspection caps materialization at 2048 rows / 8 MiB; it returns
//! `InspectionLimit` rather than silently truncating. Log/outcome pages cap rows
//! at 2048. These inspection caps do not lower service or asset limits.
//! Assets are bounded synthetic bytes; device staging and file IO are deferred.
//! No result from these service tests constitutes P24 or full fake-lane acceptance.

mod service;
mod storage;
mod types;

pub use service::{Service, record_non_asset_bytes};
pub use types::*;

pub mod adapter;
pub mod checks;
pub mod child_driver;
pub mod coordinator;
pub mod devices;
pub mod media_observation;
