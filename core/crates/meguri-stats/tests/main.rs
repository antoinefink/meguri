//! This crate's integration tests in one binary (`[[test]] stats_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]`. Add a new test
//! file here (`tools/mg static-checks --check test-targets`).

#[path = "card_info.rs"]
pub mod card_info;
#[path = "graphs_common.rs"]
pub mod graphs_common;
#[path = "graphs_common_cache.rs"]
pub mod graphs_common_cache;
