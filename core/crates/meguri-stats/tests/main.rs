//! This crate's integration tests in one binary (`[[test]] stats_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]`. Add a new test
//! file here (`tools/mg static-checks --check test-targets`).

#[path = "by_deck.rs"]
pub mod by_deck;
#[path = "card_info.rs"]
pub mod card_info;
#[path = "graphs_cards.rs"]
pub mod graphs_cards;
#[path = "graphs_common.rs"]
pub mod graphs_common;
#[path = "graphs_common_cache.rs"]
pub mod graphs_common_cache;
#[path = "graphs_revlog.rs"]
pub mod graphs_revlog;
#[path = "history_parts.rs"]
pub mod history_parts;
#[path = "history_request.rs"]
pub mod history_request;
#[path = "history_states.rs"]
pub mod history_states;
