//! This crate's integration tests in one binary (`[[test]] model_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "card_data_json.rs"]
pub mod card_data_json;
#[path = "memory_fields.rs"]
pub mod memory_fields;
#[path = "proto_roundtrip.rs"]
pub mod proto_roundtrip;
#[path = "row_records.rs"]
pub mod row_records;
#[path = "sched_params.rs"]
pub mod sched_params;
#[path = "schema11_roundtrip.rs"]
pub mod schema11_roundtrip;
#[path = "seam.rs"]
pub mod seam;
#[cfg(feature = "test-hooks")]
#[path = "step_clock.rs"]
pub mod step_clock;
