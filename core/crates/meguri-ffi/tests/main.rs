//! This crate's integration tests in one binary (`[[test]] ffi_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

/// `meguri_testkit::cloud::child_driver` starts its child process as this
/// binary with `--exact cloud_child_entry`. Here the entries the files define
/// are `<file>::cloud_child_entry`, so the child runs this one, the same call.
/// Outside such a child it returns at once, as theirs do.
#[test]
fn cloud_child_entry() {
    meguri_testkit::cloud::child_driver::child_entry().unwrap();
}

#[path = "browse_search_ffi.rs"]
pub mod browse_search_ffi;
#[path = "cloud_composition_ffi.rs"]
pub mod cloud_composition_ffi;
#[path = "cloud_initial_app_fixture.rs"]
pub mod cloud_initial_app_fixture;
#[path = "cloud_initial_import_ffi.rs"]
pub mod cloud_initial_import_ffi;
#[path = "cloud_original_observation.rs"]
pub mod cloud_original_observation;
#[path = "cloud_recovered_cancel.rs"]
pub mod cloud_recovered_cancel;
#[path = "cold_job_finished.rs"]
pub mod cold_job_finished;
#[path = "collection_export_memory.rs"]
pub mod collection_export_memory;
#[path = "deck_options_ffi.rs"]
pub mod deck_options_ffi;
#[path = "headless_parity.rs"]
pub mod headless_parity;
#[path = "notetype_reads_ffi.rs"]
pub mod notetype_reads_ffi;
#[cfg(feature = "test-hooks")]
#[path = "maintenance_ffi.rs"]
pub mod maintenance_ffi;
#[cfg(feature = "test-hooks")]
#[path = "physical_review_rows.rs"]
pub mod physical_review_rows;
