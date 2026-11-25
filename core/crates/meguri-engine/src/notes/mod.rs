//! Note operations from specs/notes/note-records. Composite callers use the
//! `_in` forms in their own journal; public operations use the executor.
pub mod add;
pub mod check;
pub mod delete;
pub mod deleted_log;
pub mod fields;
pub mod guid;
pub mod mining;
pub mod update;

pub use add::{AddFieldIdentity, AddNote, AddScreenNote, Added, add, add_for_screen, add_in, bulk_add, bulk_add_in};
pub use check::{FieldCheck, FieldState, check};
pub use delete::{delete, delete_controlled, delete_in};
pub use update::{NotePatch, UpdateNote, Updated, patch, patch_for, patch_in, update, update_in};

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

/// N6.3: sync removes controls without NFC or local save preferences.
pub fn sync_field(field: &str) -> String {
    meguri_text::normalize::field_as_saved(field, false).into_owned()
}
/// Compute only the incoming note's saved field data; never generate cards or
/// register tags. The caller validated the note-type field count.
pub fn prepare_sync_note(
    note: &mut meguri_model::note::Note,
    nt: &meguri_model::notetype::Notetype,
    fields: &[String],
) {
    note.fields = fields.join("\u{1f}");
    note.sort_field = meguri_text::checksum::sort_text_of(fields, nt.config.sort_field_idx as usize).into_owned();
    note.checksum = meguri_text::checksum::note_checksum(fields.first().map_or("", String::as_str));
    note.flags = 0;
    note.data.clear();
}
