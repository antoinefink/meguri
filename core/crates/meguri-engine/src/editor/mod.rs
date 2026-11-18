//! Read-only services for the production editor. Add bookkeeping remains
//! inside the existing note-add transaction.
mod defaults;
mod tag_complete;

pub use defaults::{AddDefaults, add_defaults, notetype_last_deck};
pub use tag_complete::tag_complete;
