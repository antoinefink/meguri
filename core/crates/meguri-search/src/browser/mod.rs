//! Browse cells (`browser/columns` C4 to C13): the typed values the core
//! computes from decoded records, shared by Browse rows and card excerpts;
//! and Find Duplicates' key (`browser/workspace` V10).
pub mod cells;
pub mod deck_cell;
pub mod duplicates;
pub mod fields;
pub mod qa;
