//! This crate's integration tests in one binary (`[[test]] cli_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "anonymize.rs"]
pub mod anonymize;
#[path = "cli.rs"]
pub mod cli;
#[path = "cloud_service.rs"]
pub mod cloud_service;
#[path = "fp_probe.rs"]
pub mod fp_probe;
#[path = "generate.rs"]
pub mod generate;
#[path = "native_review.rs"]
pub mod native_review;
#[path = "private.rs"]
pub mod private;
#[path = "timing.rs"]
pub mod timing;
