//! Test support: the conformance runner (scenarios, executors, golden
//! comparison, reports, harvest catalog), plus fixtures, generators and fault
//! injection as later units add them. Development only.
//!
//! The scenario language, observations and statuses are specified in
//! `specs/conformance/dsl.md` and `specs/conformance/observations.md`.

pub mod catalog;
pub mod diff;
pub mod exec;
pub mod generate;
pub mod observe;
pub mod report;
pub mod scenario;
pub mod suite;
pub mod wallclock;
