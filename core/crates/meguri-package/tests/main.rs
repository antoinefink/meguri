//! This crate's integration tests in one binary (`[[test]] package_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "apkg_import.rs"]
pub mod apkg_import;
#[path = "apkg_merge.rs"]
pub mod apkg_merge;
#[path = "apkg_rules.rs"]
pub mod apkg_rules;
#[path = "backup_retention.rs"]
pub mod backup_retention;
#[path = "backup_service.rs"]
pub mod backup_service;
#[path = "backup_writer.rs"]
pub mod backup_writer;
#[path = "collection_package.rs"]
pub mod collection_package;
#[path = "deck_export.rs"]
pub mod deck_export;
#[path = "inspection.rs"]
pub mod inspection;
#[path = "live_writer.rs"]
pub mod live_writer;
#[path = "restore.rs"]
pub mod restore;
