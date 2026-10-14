//! Note type records (`specs/model/row-records.md` R7): the `notetypes` row
//! with its `fields` and `templates` rows.

use crate::ids::NotetypeId;
use crate::proto::ProtoBlob;
use crate::proto::notetype::{FieldConfig, NotetypeConfig, TemplateConfig};
use crate::time::TimestampSecs;
use crate::usn::Usn;

#[derive(Clone, Debug)]
pub struct Notetype {
    pub id: NotetypeId,
    pub name: String,
    pub mtime: TimestampSecs,
    pub usn: Usn,
    pub config: ProtoBlob<NotetypeConfig>,
    /// In `ord` order, from 0 without gaps.
    pub fields: Vec<NotetypeField>,
    /// In `ord` order, from 0 without gaps.
    pub templates: Vec<NotetypeTemplate>,
}

/// A `fields` row.
#[derive(Clone, Debug)]
pub struct NotetypeField {
    /// The field's position.
    pub ord: u32,
    pub name: String,
    pub config: ProtoBlob<FieldConfig>,
    /// The `ord` a schema-11 object carried: the field's previous position,
    /// for the note type structure-change logic (`model/schema11-json` S4).
    /// Not a column.
    pub prior_ord: Option<u32>,
}

/// A `templates` row.
#[derive(Clone, Debug)]
pub struct NotetypeTemplate {
    /// The template's position.
    pub ord: u32,
    pub name: String,
    /// Anki keeps it 0.
    pub mtime: TimestampSecs,
    /// Anki keeps it 0.
    pub usn: Usn,
    pub config: ProtoBlob<TemplateConfig>,
    /// The `ord` a schema-11 object carried (S4). Not a column.
    pub prior_ord: Option<u32>,
}
