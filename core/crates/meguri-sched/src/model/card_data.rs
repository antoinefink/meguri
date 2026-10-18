//! Reuse the one lossless codec (model/card-data-json C3/C4). Known fields
//! canonicalize on write; unknown raw values and custom-data contents survive.
pub use meguri_model::card_data::{CardData, MemoryState};
