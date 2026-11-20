//! This crate's integration tests in one binary (`[[test]] template_it`).
//! Each `tests/*.rs` file is a module, loaded by `#[path]` so its own `mod`
//! lines resolve as they did at its crate root; `pub` keeps its public items
//! exported. Add a new test file here, or give it its own `[[test]]` when it
//! cannot share a binary (`tools/mg static-checks --check test-targets`).

// Each file keeps its own copy of the helper modules it declares (`mod
// common;`), as it did in a binary of its own.
#![allow(clippy::duplicate_mod)]

#[path = "av_tags.rs"]
pub mod av_tags;
#[path = "card_pipeline.rs"]
pub mod card_pipeline;
#[path = "cloze_unit.rs"]
pub mod cloze_unit;
#[path = "display_transform.rs"]
pub mod display_transform;
#[path = "image_occlusion.rs"]
pub mod image_occlusion;
#[path = "image_occlusion_render.rs"]
pub mod image_occlusion_render;
#[path = "template_unit.rs"]
pub mod template_unit;
#[path = "type_answer.rs"]
pub mod type_answer;
