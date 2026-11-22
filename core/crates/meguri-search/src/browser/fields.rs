//! Field columns `fld:<field name>` (`specs/browser/workspace.md` V3,
//! V4.2): a field column's cell and its sort key, from the stored field
//! value. Nothing here reads the collection or renders a template.

use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_text::field_empty::counts_as_empty;
use meguri_text::{entities, fold, html, media_refs};

use super::cells::Cell;

/// The value of the field named exactly `name` in `note`, of note type
/// `kind`: `None` when the note type has no such field (V3). A note that
/// holds no value at the field's position (a damaged note) reads as the
/// empty text. Of two fields with that name (a damaged note type) the first
/// position counts.
pub fn value<'n>(note: &'n Note, kind: &Notetype, name: &str) -> Option<&'n str> {
    Some(field_at(&note.fields, ord_of(kind, name)?))
}

/// The position of the field named exactly `name` in note type `kind`, the
/// first of two with that name; `None` when it has none.
pub fn ord_of(kind: &Notetype, name: &str) -> Option<u32> {
    kind.fields
        .iter()
        .filter(|field| field.name == name)
        .map(|field| field.ord)
        .min()
}

/// The value at position `ord` of a note's stored fields; the empty text
/// past the last one.
pub fn field_at(fields: &str, ord: u32) -> &str {
    fields
        .split(meguri_model::note::FIELD_SEPARATOR)
        .nth(ord as usize)
        .unwrap_or("")
}

/// V3's one-line text: `text/strip-html` H6 with media names kept, as the
/// Sort Field cell computes it (`browser/columns` C7.1), trimmed.
pub fn line(value: &str) -> String {
    // Every H6 step starts at one of these characters; without them the
    // line is the value trimmed. Sorting computes it for every row.
    if !value.contains(['<', '&', '[', '\n']) {
        return value.trim().to_owned();
    }
    html::text_line_keep_media(value)
}

/// M2's first `<img>` tag, by its decoded file name.
fn first_image(value: &str) -> Option<String> {
    media_refs::media_tags(value)
        .find(|tag| tag.matched.get(..4).is_some_and(|name| name.eq_ignore_ascii_case("<img")))
        .map(|tag| entities::decode_text(tag.raw_name).into_owned())
}

/// V3: the cell of a field column for a row whose note's field has `value`
/// (`None`: its note type has no such field, shown as an en dash). An
/// empty field (`text/field-emptiness` E1) shows nothing. A field holding a
/// sound tag or an image shows its first sound and its first image by file
/// name (decoded, M1), never the reference text; the app reads the sound's
/// length and the thumbnail. Otherwise the field's one-line text.
pub fn cell(value: Option<&str>) -> Cell {
    let Some(value) = value else {
        return Cell::NoField;
    };
    if counts_as_empty(value) {
        return Cell::Empty;
    }
    let sound = media_refs::sound_tags(value)
        .next()
        .map(|tag| entities::decode_text(tag.raw_name).into_owned());
    let image = first_image(value);
    if sound.is_some() || image.is_some() {
        return Cell::Media { sound, image };
    }
    Cell::Text(line(value))
}

/// V4.2 step 1 and 2's ranks, the first byte of a key.
const HAS_VALUE: u8 = 0;
const EMPTY: u8 = 1;
const NO_FIELD: u8 = 2;
/// V4.2 step 3: a number before a text, the second byte of a key.
const NUMBER: u8 = 0;
const TEXT: u8 = 1;

/// V4.2 step 3: the value of a text that is `^-?[0-9]+(\.[0-9]+)?$` (ASCII
/// digits), as the nearest binary64.
pub fn number(text: &str) -> Option<f64> {
    let unsigned = text.strip_prefix('-').unwrap_or(text);
    let (whole, fraction) = match unsigned.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (unsigned, None),
    };
    let digits = |part: &str| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit());
    if !digits(whole) || fraction.is_some_and(|fraction| !digits(fraction)) {
        return None;
    }
    text.parse().ok()
}

/// The bytes of `x` whose byte order is its numeric order; −0 is 0.
fn number_bytes(x: f64) -> [u8; 8] {
    let x = if x == 0.0 { 0.0 } else { x };
    let bits = x.to_bits();
    let ordered = if bits >> 63 == 1 { !bits } else { bits | (1 << 63) };
    ordered.to_be_bytes()
}

/// `bytes` with each byte raised by one, so that 0 can end the run: UTF-8
/// never holds 0xFF, so nothing overflows, and a proper prefix (followed by
/// the 0) orders first.
fn push_raised(key: &mut Vec<u8>, bytes: &[u8]) {
    key.extend(bytes.iter().map(|b| b + 1));
}

/// V4.2's key of one row as bytes whose order (SQLite's blob order:
/// `memcmp`, a proper prefix first) is V4.2 steps 1 to 3: rows whose note
/// type has the field, non-empty values first (numbers, by value, before
/// texts), then empty values; then rows without the field (`value` `None`).
/// Equal numbers and texts compare by the `unicase` folding of the text
/// (`text/unicode-primitives` U2) and then by its code points. The caller
/// adds the final id key (S8).
pub fn sort_key(value: Option<&str>) -> Vec<u8> {
    let Some(value) = value else {
        return vec![NO_FIELD];
    };
    if counts_as_empty(value) {
        return vec![EMPTY];
    }
    let text = line(value);
    let folded = fold::folded(&text);
    let mut key = Vec::with_capacity(12 + folded.len() + text.len());
    key.push(HAS_VALUE);
    match number(&text) {
        Some(number) => {
            key.push(NUMBER);
            key.extend(number_bytes(number));
        }
        None => key.push(TEXT),
    }
    // U2 compares the folded characters code point by code point, which is
    // their UTF-8 byte order.
    push_raised(&mut key, folded.as_bytes());
    key.push(0);
    push_raised(&mut key, text.as_bytes());
    key
}
