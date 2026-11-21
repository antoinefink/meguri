//! Browser data queries. These do not need search state or the day clock.
mod excerpts;
pub mod name_index;
pub use excerpts::{CardExcerpt, card_excerpts};
pub use name_index::NameIndexProvider;
pub use meguri_search::browser::qa::Question;
