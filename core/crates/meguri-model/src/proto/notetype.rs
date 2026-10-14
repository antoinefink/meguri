//! Note type blobs: the notetype, field and template configs
//! (`specs/model/protobuf-records.md` P6.1 to P6.3).

use super::{Decode, Encode, Reader, Result, UnknownFields, WireType, Writer, blob_message};
use crate::error::BlobColumn;

/// P6.1, `notetypes.config`.
#[derive(Clone, Debug, Default)]
pub struct NotetypeConfig {
    /// 0 normal, 1 cloze; any other number is kept.
    pub kind: i32,
    pub sort_field_idx: u32,
    pub css: String,
    /// Legacy last-used deck; not read by Anki, kept as is.
    pub target_deck_id_unused: i64,
    pub latex_pre: String,
    pub latex_post: String,
    pub latex_svg: bool,
    /// Legacy card requirements.
    pub reqs: Vec<CardRequirement>,
    /// 0 unknown, 1 basic, 2 basic and reversed, 3 basic with optional
    /// reverse, 4 basic with typing, 5 cloze, 6 image occlusion.
    pub original_stock_kind: i32,
    /// The note type's id in the collection it was imported from.
    pub original_id: Option<i64>,
    pub(crate) other: Vec<u8>,
    pub(crate) unknown: UnknownFields,
}

impl NotetypeConfig {
    /// Legacy JSON keys (P7), opaque bytes.
    pub fn other(&self) -> &[u8] {
        &self.other
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for NotetypeConfig {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.kind = r.int32(wire)?,
            2 => self.sort_field_idx = r.uint32(wire)?,
            3 => self.css = r.string(wire)?,
            4 => self.target_deck_id_unused = r.int64(wire)?,
            5 => self.latex_pre = r.string(wire)?,
            6 => self.latex_post = r.string(wire)?,
            7 => self.latex_svg = r.bool(wire)?,
            8 => {
                let mut req = CardRequirement::default();
                r.message(wire, &mut req)?;
                self.reqs.push(req);
            }
            9 => self.original_stock_kind = r.int32(wire)?,
            10 => self.original_id = Some(r.int64(wire)?),
            255 => self.other = r.bytes(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for NotetypeConfig {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.int32(1, self.kind);
        w.uint32(2, self.sort_field_idx);
        w.string(3, &self.css);
        w.int64(4, self.target_deck_id_unused);
        w.string(5, &self.latex_pre);
        w.string(6, &self.latex_post);
        w.bool(7, self.latex_svg);
        for req in &self.reqs {
            w.message(8, req);
        }
        w.int32(9, self.original_stock_kind);
        w.opt_int64(10, self.original_id);
        w.bytes(255, &self.other);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

blob_message!(NotetypeConfig, BlobColumn::NotetypeConfig);

/// P6.1.1, one legacy card requirement.
#[derive(Clone, Debug, Default)]
pub struct CardRequirement {
    pub card_ord: u32,
    /// 0 none, 1 any, 2 all.
    pub kind: i32,
    pub field_ords: Vec<u32>,
    pub(crate) unknown: UnknownFields,
}

impl CardRequirement {
    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for CardRequirement {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.card_ord = r.uint32(wire)?,
            2 => self.kind = r.int32(wire)?,
            3 => r.uint32s(wire, &mut self.field_ords)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for CardRequirement {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.uint32(1, self.card_ord);
        w.int32(2, self.kind);
        w.uint32s(3, &self.field_ords);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

/// P6.2, `fields.config`. The field's `ord` and `name` are row columns.
#[derive(Clone, Debug, Default)]
pub struct FieldConfig {
    pub sticky: bool,
    pub rtl: bool,
    pub font_name: String,
    pub font_size: u32,
    pub description: String,
    pub plain_text: bool,
    pub collapsed: bool,
    pub exclude_from_search: bool,
    /// Stable field id, matched when merging note types on import.
    pub id: Option<i64>,
    /// Image-occlusion field tag.
    pub tag: Option<u32>,
    pub prevent_deletion: bool,
    pub(crate) other: Vec<u8>,
    pub(crate) unknown: UnknownFields,
}

impl FieldConfig {
    /// Legacy JSON keys (P7), opaque bytes.
    pub fn other(&self) -> &[u8] {
        &self.other
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for FieldConfig {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.sticky = r.bool(wire)?,
            2 => self.rtl = r.bool(wire)?,
            3 => self.font_name = r.string(wire)?,
            4 => self.font_size = r.uint32(wire)?,
            5 => self.description = r.string(wire)?,
            6 => self.plain_text = r.bool(wire)?,
            7 => self.collapsed = r.bool(wire)?,
            8 => self.exclude_from_search = r.bool(wire)?,
            9 => self.id = Some(r.int64(wire)?),
            10 => self.tag = Some(r.uint32(wire)?),
            11 => self.prevent_deletion = r.bool(wire)?,
            255 => self.other = r.bytes(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for FieldConfig {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.bool(1, self.sticky);
        w.bool(2, self.rtl);
        w.string(3, &self.font_name);
        w.uint32(4, self.font_size);
        w.string(5, &self.description);
        w.bool(6, self.plain_text);
        w.bool(7, self.collapsed);
        w.bool(8, self.exclude_from_search);
        w.opt_int64(9, self.id);
        w.opt_uint32(10, self.tag);
        w.bool(11, self.prevent_deletion);
        w.bytes(255, &self.other);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

blob_message!(FieldConfig, BlobColumn::FieldConfig);

/// P6.3, `templates.config`. `ord`, `name`, `mtime_secs` and `usn` are row
/// columns.
#[derive(Clone, Debug, Default)]
pub struct TemplateConfig {
    pub q_format: String,
    pub a_format: String,
    pub q_format_browser: String,
    pub a_format_browser: String,
    /// Deck override; 0 means none.
    pub target_deck_id: i64,
    pub browser_font_name: String,
    pub browser_font_size: u32,
    /// Stable template id, matched when merging note types on import.
    pub id: Option<i64>,
    pub(crate) other: Vec<u8>,
    pub(crate) unknown: UnknownFields,
}

impl TemplateConfig {
    /// Legacy JSON keys (P7), opaque bytes.
    pub fn other(&self) -> &[u8] {
        &self.other
    }

    pub fn unknown_fields(&self) -> &UnknownFields {
        &self.unknown
    }
}

impl Decode for TemplateConfig {
    fn merge_field(&mut self, field: u32, wire: WireType, r: &mut Reader<'_>) -> Result<bool> {
        match field {
            1 => self.q_format = r.string(wire)?,
            2 => self.a_format = r.string(wire)?,
            3 => self.q_format_browser = r.string(wire)?,
            4 => self.a_format_browser = r.string(wire)?,
            5 => self.target_deck_id = r.int64(wire)?,
            6 => self.browser_font_name = r.string(wire)?,
            7 => self.browser_font_size = r.uint32(wire)?,
            8 => self.id = Some(r.int64(wire)?),
            255 => self.other = r.bytes(wire)?,
            _ => return Ok(false),
        }
        Ok(true)
    }

    fn unknown_mut(&mut self) -> &mut UnknownFields {
        &mut self.unknown
    }
}

impl Encode for TemplateConfig {
    fn encode_fields(&self, w: &mut Writer<'_>) {
        w.string(1, &self.q_format);
        w.string(2, &self.a_format);
        w.string(3, &self.q_format_browser);
        w.string(4, &self.a_format_browser);
        w.int64(5, self.target_deck_id);
        w.string(6, &self.browser_font_name);
        w.uint32(7, self.browser_font_size);
        w.opt_int64(8, self.id);
        w.bytes(255, &self.other);
    }

    fn unknown(&self) -> &UnknownFields {
        &self.unknown
    }
}

blob_message!(TemplateConfig, BlobColumn::TemplateConfig);
