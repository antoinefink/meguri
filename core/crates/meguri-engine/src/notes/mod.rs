//! Note operations from specs/notes/note-records. Composite callers use the
//! `_in` forms in their own journal; public operations use the executor.
pub mod add;
pub mod check;
pub mod delete;
pub mod deleted_log;
pub mod fields;
pub mod guid;
pub mod update;

pub use add::{AddNote, Added, add, add_in, bulk_add, bulk_add_in};
pub use check::{FieldCheck, FieldState, check};
pub use delete::{delete, delete_in};
pub use update::{UpdateNote, Updated, update, update_in};

use crate::executor::catalog;
use crate::undo::entry::StepKind;
use crate::{EngineError, OpSpec};
use meguri_model::changes::OpSource;
use std::time::Duration;

pub(crate) fn invalid(message: impl Into<String>) -> EngineError {
    EngineError::InvalidArgument {
        message: message.into(),
    }
}

pub(crate) fn op_spec(name: &'static str, source: OpSource, count: usize) -> OpSpec {
    let spec = if count > 500 {
        OpSpec::new(catalog::BULK_OP, source).declare_long_write(Duration::from_secs(1))
    } else {
        OpSpec::named(name, source)
    };
    spec.undo_step(name, StepKind::Other)
}
