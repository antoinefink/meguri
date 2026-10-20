//! Browser data queries. These do not need search state or the day clock.
mod excerpts;
pub use excerpts::{CardExcerpt, card_excerpts};
pub use meguri_search::browser::qa::Question;
