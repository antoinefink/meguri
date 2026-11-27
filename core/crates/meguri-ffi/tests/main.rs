//! This crate's integration tests in one binary (`[[test]] ffi_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "browse_query_ffi.rs"]
pub mod browse_query_ffi;
#[path = "browse_search_ffi.rs"]
pub mod browse_search_ffi;
#[path = "bulk_actions_ffi.rs"]
pub mod bulk_actions_ffi;
#[path = "cold_job_finished.rs"]
pub mod cold_job_finished;
#[path = "collection_export_memory.rs"]
pub mod collection_export_memory;
#[path = "deck_options_ffi.rs"]
pub mod deck_options_ffi;
#[path = "find_replace_ffi.rs"]
pub mod find_replace_ffi;
#[path = "headless_parity.rs"]
pub mod headless_parity;
#[path = "notetype_reads_ffi.rs"]
pub mod notetype_reads_ffi;
#[path = "stats_page_ffi.rs"]
pub mod stats_page_ffi;
#[cfg(feature = "test-hooks")]
#[path = "deck_import_ffi.rs"]
pub mod deck_import_ffi;
#[cfg(feature = "test-hooks")]
#[path = "maintenance_ffi.rs"]
pub mod maintenance_ffi;
#[cfg(feature = "test-hooks")]
#[path = "physical_review_rows.rs"]
pub mod physical_review_rows;
#[cfg(feature = "test-hooks")]
#[path = "provider_structure_production.rs"]
pub mod provider_structure_production;
