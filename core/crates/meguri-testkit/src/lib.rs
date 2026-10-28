//! Test support: the conformance runner (scenarios, executors, golden
//! comparison, reports, harvest catalog), plus fixtures, generators and fault
//! injection as later units add them. Development only.
//!
//! The scenario language, observations and statuses are specified in
//! `specs/conformance/dsl.md` and `specs/conformance/observations.md`.

pub mod anonymize;
pub mod bench;
pub mod catalog;
pub mod cloud;
pub mod diff;
pub mod dump;
pub mod exec;
pub mod generate;
pub mod observe;
pub mod private;
pub mod report;
pub mod scenario;
pub mod suite;
pub mod sync_stub;
pub mod timing;
pub mod wallclock;

pub mod native_review;
pub mod normal_support;

/// Local backup/recovery composition helpers.
pub mod backup_support;
