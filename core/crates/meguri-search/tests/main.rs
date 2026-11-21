//! This crate's integration tests in one binary (`[[test]] search_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "browser_cells.rs"]
pub mod browser_cells;
#[path = "builder.rs"]
pub mod builder;
#[path = "canonical_roundtrip.rs"]
pub mod canonical_roundtrip;
#[path = "card_extractors.rs"]
pub mod card_extractors;
#[path = "config_keys.rs"]
pub mod config_keys;
#[path = "edit_services.rs"]
pub mod edit_services;
#[path = "name_index.rs"]
pub mod name_index;
#[path = "names_and_ids.rs"]
pub mod names_and_ids;
#[path = "parse_props.rs"]
pub mod parse_props;
#[path = "props_and_history.rs"]
pub mod props_and_history;
#[path = "state_and_flags.rs"]
pub mod state_and_flags;
