//! Browser data queries. These do not need search state; row requests take
//! now and today from their caller (`browser/columns` C1).
mod excerpts;
pub mod name_index;
mod rows;
pub use excerpts::{CardExcerpt, card_excerpts};
pub use meguri_search::browser::cells::{Cell, Font, Percent, RowState, RowTime, Tint};
pub use meguri_search::browser::qa::{Answer, Question};
pub use name_index::NameIndexProvider;
pub use rows::{Row, RowFailure, RowRequest, RowResult, rows};
