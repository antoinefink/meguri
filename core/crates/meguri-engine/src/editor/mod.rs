//! Read-only services for the production editor. Add bookkeeping remains
//! inside the existing note-add transaction.
mod defaults;

pub use defaults::{AddDefaults, add_defaults, notetype_last_deck};
