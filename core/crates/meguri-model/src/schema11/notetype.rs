//! Note type objects (`specs/model/schema11-json.md` S4 to S6).

use serde_json::value::RawValue;

use super::{ObjectReader, Result, nullable, other_from_members, other_members, write_other};
use crate::error::{ModelError, Schema11Item};
use crate::ids::NotetypeId;
use crate::json::{self, ObjectWriter};
use crate::notetype::{Notetype, NotetypeField, NotetypeTemplate};
use crate::proto::ProtoBlob;
use crate::proto::notetype::{CardRequirement, FieldConfig, NotetypeConfig, TemplateConfig};
use crate::time::TimestampSecs;
use crate::usn::Usn;

const ITEM: Schema11Item = Schema11Item::Notetype;

/// S3 reserved keys.
const RESERVED: [&str; 16] = [
    "css",
    "did",
    "flds",
    "id",
    "latexPost",
    "latexPre",
    "latexsvg",
    "mod",
    "name",
    "originalId",
    "originalStockKind",
    "req",
    "sortf",
    "tmpls",
    "type",
    "usn",
];
const FIELD_RESERVED: [&str; 13] = [
    "collapsed",
    "description",
    "excludeFromSearch",
    "font",
    "id",
    "name",
    "ord",
    "plainText",
    "preventDeletion",
    "rtl",
    "size",
    "sticky",
    "tag",
];
const TEMPLATE_RESERVED: [&str; 10] = [
    "afmt", "bafmt", "bfont", "bqfmt", "bsize", "did", "id", "name", "ord", "qfmt",
];

/// S4 output.
pub fn notetype_to_json(notetype: &Notetype) -> String {
    let config: &NotetypeConfig = &notetype.config;
    let mut out = ObjectWriter::new();
    out.int("id", notetype.id.0);
    out.string("name", &notetype.name);
    out.int("type", if config.kind == 1 { 1 } else { 0 });
    out.int("mod", notetype.mtime.0);
    out.int("usn", notetype.usn.0);
    out.int("sortf", config.sort_field_idx as u16);
    match config.target_deck_id_unused {
        0 => out.null("did"),
        did => out.int("did", did),
    }
    let templates = out.key("tmpls");
    templates.push('[');
    for (i, template) in notetype.templates.iter().enumerate() {
        if i > 0 {
            templates.push(',');
        }
        templates.push_str(&template_to_json(template));
    }
    templates.push(']');
    let fields = out.key("flds");
    fields.push('[');
    for (i, field) in notetype.fields.iter().enumerate() {
        if i > 0 {
            fields.push(',');
        }
        fields.push_str(&field_to_json(field));
    }
    fields.push(']');
    out.string("css", &config.css);
    out.string("latexPre", &config.latex_pre);
    out.string("latexPost", &config.latex_post);
    out.bool("latexsvg", config.latex_svg);
    write_reqs(out.key("req"), &config.reqs);
    let mut written = vec!["id", "name", "type", "mod", "usn", "sortf", "did", "tmpls", "flds"];
    written.extend(["css", "latexPre", "latexPost", "latexsvg", "req"]);
    if config.original_stock_kind != 0 {
        out.int("originalStockKind", config.original_stock_kind);
        written.push("originalStockKind");
    }
    if let Some(id) = config.original_id {
        out.int("originalId", id);
        written.push("originalId");
    }
    write_other(&mut out, &other_members(config.other()), &RESERVED, &written);
    out.finish()
}

/// S4.1 output: `[card_ord, kind, field_ords]` per requirement.
fn write_reqs(out: &mut String, reqs: &[CardRequirement]) {
    out.push('[');
    for (i, req) in reqs.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        out.push('[');
        json::push_int(out, req.card_ord as u16);
        out.push(',');
        out.push_str(match req.kind {
            1 => "\"any\"",
            2 => "\"all\"",
            _ => "\"none\"",
        });
        out.push_str(",[");
        for (j, ord) in req.field_ords.iter().enumerate() {
            if j > 0 {
                out.push(',');
            }
            json::push_int(out, *ord as u16);
        }
        out.push_str("]]");
    }
    out.push(']');
}

/// S5 output.
fn field_to_json(field: &NotetypeField) -> String {
    let config: &FieldConfig = &field.config;
    let mut out = ObjectWriter::new();
    out.string("name", &field.name);
    out.int("ord", field.ord as u16);
    out.bool("sticky", config.sticky);
    out.bool("rtl", config.rtl);
    out.string("font", &config.font_name);
    out.int("size", config.font_size as u16);
    out.string("description", &config.description);
    out.bool("plainText", config.plain_text);
    out.bool("collapsed", config.collapsed);
    out.bool("excludeFromSearch", config.exclude_from_search);
    match config.id {
        Some(id) => out.int("id", id),
        None => out.null("id"),
    }
    match config.tag {
        Some(tag) => out.int("tag", tag),
        None => out.null("tag"),
    }
    out.bool("preventDeletion", config.prevent_deletion);
    write_other(
        &mut out,
        &other_members(config.other()),
        &FIELD_RESERVED,
        &FIELD_RESERVED,
    );
    out.finish()
}

/// S6 output.
fn template_to_json(template: &NotetypeTemplate) -> String {
    let config: &TemplateConfig = &template.config;
    let mut out = ObjectWriter::new();
    out.string("name", &template.name);
    out.int("ord", template.ord as u16);
    out.string("qfmt", &config.q_format);
    out.string("afmt", &config.a_format);
    out.string("bqfmt", &config.q_format_browser);
    out.string("bafmt", &config.a_format_browser);
    if config.target_deck_id > 0 {
        out.int("did", config.target_deck_id);
    } else {
        out.null("did");
    }
    out.string("bfont", &config.browser_font_name);
    out.int("bsize", config.browser_font_size as u8);
    match config.id {
        Some(id) => out.int("id", id),
        None => out.null("id"),
    }
    write_other(
        &mut out,
        &other_members(config.other()),
        &TEMPLATE_RESERVED,
        &TEMPLATE_RESERVED,
    );
    out.finish()
}

/// S4 input. The record's blobs are new (no original bytes).
pub fn notetype_from_json(text: &str) -> Result<Notetype> {
    let raw = json::parse_value(text).ok_or_else(|| ModelError::invalid_json(ITEM, ""))?;
    notetype_from_raw(raw)
}

pub(crate) fn notetype_from_raw(raw: &RawValue) -> Result<Notetype> {
    let mut r = ObjectReader::new(raw, ITEM, "")?;
    let id = r.strict("id", json::i64_or_string, None)?;
    let name = r.strict("name", json::string, None)?;
    let kind = r.strict("type", |v| json::i32_value(v).filter(|k| matches!(k, 0 | 1)), None)?;
    let mtime = r.strict("mod", json::i64_value, None)?;
    let usn = r.strict("usn", json::i32_value, None)?;
    let sort_field_idx = r.strict("sortf", json::u16_value, None)?;
    let did = r.lenient("did", nullable(json::i64_value), None)?;
    let templates = r.strict("tmpls", json::array, None)?;
    let fields = r.strict("flds", json::array, None)?;
    let css = r.lenient("css", json::string, None)?;
    let latex_pre = r.strict("latexPre", json::string, Some(String::new()))?;
    let latex_post = r.strict("latexPost", json::string, Some(String::new()))?;
    let latex_svg = r.lenient("latexsvg", json::boolean, Some(false))?;
    let reqs = r.lenient("req", reqs_from_raw, Some(Vec::new()))?;
    let original_stock_kind = r.strict("originalStockKind", json::i32_value, Some(0))?;
    let original_id = r.strict("originalId", nullable(json::i64_value), Some(None))?;
    let other = other_from_members(&r.unread());

    let templates = templates
        .into_iter()
        .enumerate()
        .map(|(ord, raw)| template_from_raw(raw, ord))
        .collect::<Result<Vec<_>>>()?;
    let fields = fields
        .into_iter()
        .enumerate()
        .map(|(ord, raw)| field_from_raw(raw, ord))
        .collect::<Result<Vec<_>>>()?;
    let config = NotetypeConfig {
        kind,
        sort_field_idx: sort_field_idx.into(),
        css,
        target_deck_id_unused: did.unwrap_or(0),
        latex_pre,
        latex_post,
        latex_svg,
        reqs,
        original_stock_kind,
        original_id,
        other,
        ..NotetypeConfig::default()
    };
    Ok(Notetype {
        id: NotetypeId(id),
        name,
        mtime: TimestampSecs(mtime),
        usn: Usn(usn),
        config: ProtoBlob::new(config),
        fields,
        templates,
    })
}

/// S4.1 input: anything but an array of `[u16, "any" | "all" | "none",
/// [u16, ...]]` makes the whole key invalid.
fn reqs_from_raw(raw: &RawValue) -> Option<Vec<CardRequirement>> {
    json::array(raw)?
        .into_iter()
        .map(|entry| {
            let parts = json::array(entry)?;
            let [card_ord, kind, field_ords] = parts.as_slice() else {
                return None;
            };
            let kind = match json::string(kind)?.as_str() {
                "none" => 0,
                "any" => 1,
                "all" => 2,
                _ => return None,
            };
            Some(CardRequirement {
                card_ord: json::u16_value(card_ord)?.into(),
                kind,
                field_ords: json::array(field_ords)?
                    .into_iter()
                    .map(|ord| json::u16_value(ord).map(u32::from))
                    .collect::<Option<_>>()?,
                ..CardRequirement::default()
            })
        })
        .collect()
}

/// S5 input; `ord` is the field's position in `flds`.
fn field_from_raw(raw: &RawValue, ord: usize) -> Result<NotetypeField> {
    let mut r = ObjectReader::new(raw, ITEM, &format!("flds[{ord}]"))?;
    let name = r.strict("name", json::string, None)?;
    let prior_ord = r.strict("ord", nullable(json::u16_value), Some(None))?;
    let sticky = r.strict("sticky", json::bool_any, None)?;
    let rtl = r.strict("rtl", json::bool_any, None)?;
    let font_name = r.strict("font", json::string, None)?;
    let font_size = r.strict("size", json::u16_value, None)?;
    let description = r.lenient("description", json::string, Some(String::new()))?;
    let plain_text = r.lenient("plainText", json::boolean, Some(false))?;
    let collapsed = r.lenient("collapsed", json::boolean, Some(false))?;
    let exclude_from_search = r.lenient("excludeFromSearch", json::boolean, Some(false))?;
    let id = r.lenient("id", nullable(json::i64_value), Some(None))?;
    let tag = r.lenient("tag", nullable(json::u32_value), Some(None))?;
    let prevent_deletion = r.lenient("preventDeletion", json::boolean, Some(false))?;
    let other = other_from_members(&r.unread());
    Ok(NotetypeField {
        ord: ord as u32,
        name,
        config: ProtoBlob::new(FieldConfig {
            sticky,
            rtl,
            font_name,
            font_size: font_size.into(),
            description,
            plain_text,
            collapsed,
            exclude_from_search,
            id,
            tag,
            prevent_deletion,
            other,
            ..FieldConfig::default()
        }),
        prior_ord: prior_ord.map(u32::from),
    })
}

/// S6 input; `ord` is the template's position in `tmpls`.
fn template_from_raw(raw: &RawValue, ord: usize) -> Result<NotetypeTemplate> {
    let mut r = ObjectReader::new(raw, ITEM, &format!("tmpls[{ord}]"))?;
    let name = r.strict("name", json::string, None)?;
    let prior_ord = r.strict("ord", nullable(json::u16_value), Some(None))?;
    let q_format = r.strict("qfmt", json::string, None)?;
    let a_format = r.strict("afmt", json::string, Some(String::new()))?;
    let q_format_browser = r.strict("bqfmt", json::string, Some(String::new()))?;
    let a_format_browser = r.strict("bafmt", json::string, Some(String::new()))?;
    let did = r.lenient("did", nullable(json::i64_value), Some(None))?;
    let browser_font_name = r.lenient("bfont", json::string, Some(String::new()))?;
    let browser_font_size = r.lenient("bsize", json::u8_value, Some(0))?;
    let id = r.lenient("id", nullable(json::i64_value), Some(None))?;
    let other = other_from_members(&r.unread());
    Ok(NotetypeTemplate {
        ord: ord as u32,
        name,
        mtime: TimestampSecs(0),
        usn: Usn(0),
        config: ProtoBlob::new(TemplateConfig {
            q_format,
            a_format,
            q_format_browser,
            a_format_browser,
            target_deck_id: did.unwrap_or(0),
            browser_font_name,
            browser_font_size: browser_font_size.into(),
            id,
            other,
            ..TemplateConfig::default()
        }),
        prior_ord: prior_ord.map(u32::from),
    })
}
