//! Benchmark support (U12, R1864). Area units write the suites; this module
//! gives them one way to measure and report:
//!
//! - [`timing`]: every operation timed on its own into an HDR histogram,
//!   from criterion's `iter_custom`, so p50/p95/p99 are per operation, never
//!   percentiles of batch means;
//! - [`memory`]: the process's memory as R1864 defines it (phys_footprint on
//!   Apple hosts, the resident set in Docker), sampled while a bench runs;
//! - [`peak_alloc`]: a counting global allocator, reported beside memory
//!   only as a breakdown (SQLite allocates outside Rust's allocator);
//! - [`fixtures`]: the generated collections benches run on, shared with
//!   `tools/mg fixtures`;
//! - [`report`]: the perf result JSON (`tools/perf/perf-result.schema.json`)
//!   that U241's gate reads.

pub mod fixtures;
pub mod memory;
pub mod peak_alloc;
pub mod report;
pub mod timing;
