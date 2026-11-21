//! Browser data queries: row records and card excerpts, whose requests take
//! now and today from their caller (`browser/columns` C1), and the search
//! sessions that hold result handles (`search`, `browser/result-sets`).
mod excerpts;
pub mod name_index;
mod rows;
pub mod search;
pub use excerpts::{CardExcerpt, card_excerpts};
pub use meguri_search::browser::cells::{Cell, Font, Percent, RowState, RowTime, Tint};
pub use meguri_search::browser::qa::{Answer, Question};
pub use name_index::NameIndexProvider;
pub use rows::{Row, RowFailure, RowRequest, RowResult, rows};
pub(crate) use rows::rows_in;
