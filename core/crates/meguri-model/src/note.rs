//! The `notes` row (`specs/model/row-records.md` R3).

use crate::ids::{NoteId, NotetypeId};
use crate::time::EpochSecs;
use crate::usn::Usn;

/// Scalar normal-sync preparation faults, carried without received text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SyncNoteFault {
    MissingNotetype { note_id: NoteId, notetype_id: NotetypeId },
    FieldsMismatch { note_id: NoteId, notetype_id: NotetypeId },
}

/// U+001F, which joins the field values in `flds` and deck name components.
pub const FIELD_SEPARATOR: char = '\u{1f}';

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Note {
    pub id: NoteId,
    /// Stored byte for byte (`specs/text/guid`).
    pub guid: String,
    /// `mid`.
    pub notetype_id: NotetypeId,
    /// `mod`, epoch seconds.
    pub mtime: EpochSecs,
    pub usn: Usn,
    /// Space-separated with a leading and a trailing space, or empty.
    pub tags: String,
    /// `flds`: the field values joined by U+001F.
    pub fields: String,
    /// `sfld`, read as text.
    pub sort_field: String,
    /// `csum`: an unsigned 32-bit checksum.
    pub checksum: u32,
    /// Unused: a new note gets 0, updates keep the stored value.
    pub flags: i64,
    /// Unused: a new note gets the empty string, updates keep the stored value.
    pub data: String,
}

impl Note {
    /// The field values in order.
    pub fn field_values(&self) -> impl Iterator<Item = &str> {
        self.fields.split(FIELD_SEPARATOR)
    }

    /// R3: a stored `csum` outside 0 to 4294967295 reads as 0.
    pub fn checksum_from_stored(stored: i64) -> u32 {
        u32::try_from(stored).unwrap_or(0)
    }
}
