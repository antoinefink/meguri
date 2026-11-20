//! This crate's integration tests in one binary (`[[test]] sched_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "answer.rs"]
pub mod answer;
#[path = "card_data_roundtrip.rs"]
pub mod card_data_roundtrip;
#[path = "card_sched.rs"]
pub mod card_sched;
#[path = "deck_options.rs"]
pub mod deck_options;
#[path = "deck_sched.rs"]
pub mod deck_sched;
#[path = "deck_sched_goldens.rs"]
pub mod deck_sched_goldens;
#[path = "fsrs_glue.rs"]
pub mod fsrs_glue;
#[path = "fsrs_policy.rs"]
pub mod fsrs_policy;
#[path = "fuzz.rs"]
pub mod fuzz;
#[path = "interval_limits.rs"]
pub mod interval_limits;
#[path = "labels.rs"]
pub mod labels;
#[path = "learning_current.rs"]
pub mod learning_current;
#[path = "leech.rs"]
pub mod leech;
#[path = "load_balancer.rs"]
pub mod load_balancer;
#[path = "memory.rs"]
pub mod memory;
#[path = "next_states.rs"]
pub mod next_states;
#[path = "preset_view.rs"]
pub mod preset_view;
#[path = "queue.rs"]
pub mod queue;
#[path = "review.rs"]
pub mod review;
#[path = "sched_config.rs"]
pub mod sched_config;
#[path = "sched_ops.rs"]
pub mod sched_ops;
#[path = "session_queue.rs"]
pub mod session_queue;
#[path = "session_tokens.rs"]
pub mod session_tokens;
#[path = "steps.rs"]
pub mod steps;
#[path = "timing.rs"]
pub mod timing;
