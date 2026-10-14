//! The JSON Meguri shares with Anki (`specs/model/json-numbers.md`): reading
//! values with their declared types (N2, N6), keeping untyped values as their
//! exact text (N7), and writing numbers, strings and objects (N3, N4, N8).
//!
//! serde_json checks the text (RFC 8259, N9) and hands every value over as its
//! raw text; the conversions below work on that text, so numbers never go
//! through serde_json's own float parser.

use std::collections::HashMap;
use std::fmt::{self, Write as _};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

/// One member of an object: the key (unescaped) and the value's exact text.
pub(crate) type Member<'a> = (String, &'a RawValue);

/// The JSON type of a raw value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

/// A raw value's type, from its first character (raw text is never empty and
/// has no leading whitespace).
pub(crate) fn kind(raw: &RawValue) -> Kind {
    match raw.get().as_bytes().first() {
        Some(b'{') => Kind::Object,
        Some(b'[') => Kind::Array,
        Some(b'"') => Kind::String,
        Some(b't' | b'f') => Kind::Bool,
        Some(b'n') => Kind::Null,
        _ => Kind::Number,
    }
}

/// The members of an object in text order, every occurrence of a repeated key
/// included.
struct ObjectMembers<'a>(Vec<Member<'a>>);

impl<'de> Deserialize<'de> for ObjectMembers<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct MembersVisitor;

        impl<'de> Visitor<'de> for MembersVisitor {
            type Value = ObjectMembers<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value: &'de RawValue = map.next_value()?;
                    members.push((key, value));
                }
                Ok(ObjectMembers(members))
            }
        }

        deserializer.deserialize_map(MembersVisitor)
    }
}

/// Any JSON text: `None` when it is not valid JSON. Every other reader works
/// on values this one returned.
pub(crate) fn parse_value(text: &str) -> Option<&RawValue> {
    let raw = serde_json::from_str::<&RawValue>(text).ok()?;
    strings_decode(raw).then_some(raw)
}

/// Whether every string in the value, object keys included, decodes. serde_json
/// checks the syntax when it captures raw text but not escapes, and N9 makes
/// an unpaired surrogate escape anywhere invalid.
fn strings_decode(raw: &RawValue) -> bool {
    match kind(raw) {
        Kind::String => serde_json::from_str::<String>(raw.get()).is_ok(),
        Kind::Array => {
            serde_json::from_str::<Vec<&RawValue>>(raw.get()).is_ok_and(|items| items.into_iter().all(strings_decode))
        }
        // Keys are decoded as the members are read.
        Kind::Object => {
            parse_members(raw.get()).is_some_and(|members| members.into_iter().all(|(_, v)| strings_decode(v)))
        }
        Kind::Number | Kind::Bool | Kind::Null => true,
    }
}

/// JSON text holding an object: its members in text order, repeated keys
/// included. `None` for invalid JSON or another type.
pub(crate) fn parse_members(text: &str) -> Option<Vec<Member<'_>>> {
    serde_json::from_str::<ObjectMembers<'_>>(text).ok().map(|m| m.0)
}

/// N9: each repeated key reduced to its last occurrence, kept at that
/// occurrence's place.
pub(crate) fn last_wins(members: Vec<Member<'_>>) -> Vec<Member<'_>> {
    let keep: Vec<bool> = {
        let mut last: HashMap<&str, usize> = HashMap::with_capacity(members.len());
        for (i, (key, _)) in members.iter().enumerate() {
            last.insert(key.as_str(), i);
        }
        members
            .iter()
            .enumerate()
            .map(|(i, (key, _))| last.get(key.as_str()) == Some(&i))
            .collect()
    };
    members
        .into_iter()
        .zip(keep)
        .filter_map(|(member, keep)| keep.then_some(member))
        .collect()
}

/// An object value's members, repeated keys reduced (N9).
pub(crate) fn object(raw: &RawValue) -> Option<Vec<Member<'_>>> {
    if kind(raw) != Kind::Object {
        return None;
    }
    parse_members(raw.get()).map(last_wins)
}

/// An array value's elements.
pub(crate) fn array(raw: &RawValue) -> Option<Vec<&RawValue>> {
    if kind(raw) != Kind::Array {
        return None;
    }
    serde_json::from_str::<Vec<&RawValue>>(raw.get()).ok()
}

/// A string value, unescaped.
pub(crate) fn string(raw: &RawValue) -> Option<String> {
    if kind(raw) != Kind::String {
        return None;
    }
    serde_json::from_str::<String>(raw.get()).ok()
}

pub(crate) fn is_null(raw: &RawValue) -> bool {
    kind(raw) == Kind::Null
}

/// N2: `true` or `false`.
pub(crate) fn boolean(raw: &RawValue) -> Option<bool> {
    match raw.get() {
        "true" => Some(true),
        "false" => Some(false),
        _ => None,
    }
}

/// N2: a number written without a fraction or an exponent. Literals too long
/// for `i128` are out of every declared range anyway.
fn integer(raw: &RawValue) -> Option<i128> {
    let text = raw.get();
    if kind(raw) != Kind::Number || text.contains(['.', 'e', 'E']) {
        return None;
    }
    text.parse().ok()
}

/// N2 integer readers, one per declared type.
pub(crate) fn i64_value(raw: &RawValue) -> Option<i64> {
    integer(raw).and_then(|v| i64::try_from(v).ok())
}

pub(crate) fn i32_value(raw: &RawValue) -> Option<i32> {
    integer(raw).and_then(|v| i32::try_from(v).ok())
}

pub(crate) fn u32_value(raw: &RawValue) -> Option<u32> {
    integer(raw).and_then(|v| u32::try_from(v).ok())
}

pub(crate) fn u16_value(raw: &RawValue) -> Option<u16> {
    integer(raw).and_then(|v| u16::try_from(v).ok())
}

pub(crate) fn u8_value(raw: &RawValue) -> Option<u8> {
    integer(raw).and_then(|v| u8::try_from(v).ok())
}

/// N2 `f32`: an integer literal becomes the nearest `f32`; a literal with a
/// fraction or exponent becomes the nearest `f64` first, then the nearest
/// `f32`. Out-of-range magnitudes become infinities.
pub(crate) fn f32_value(raw: &RawValue) -> Option<f32> {
    if kind(raw) != Kind::Number {
        return None;
    }
    let text = raw.get();
    if text.contains(['.', 'e', 'E']) {
        text.parse::<f64>().ok().map(|v| v as f32)
    } else {
        text.parse::<f32>().ok()
    }
}

/// An array whose elements all read as `f32` (N2).
pub(crate) fn f32_array(raw: &RawValue) -> Option<Vec<f32>> {
    array(raw)?.into_iter().map(f32_value).collect()
}

/// N6 integer or numeric string: a JSON integer, or a string of an optional
/// sign and one or more ASCII digits.
fn integer_or_string(raw: &RawValue) -> Option<i128> {
    match kind(raw) {
        Kind::Number => integer(raw),
        Kind::String => {
            let text = string(raw)?;
            let digits = text.strip_prefix(['+', '-']).unwrap_or(&text);
            if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
                return None;
            }
            text.parse().ok()
        }
        _ => None,
    }
}

pub(crate) fn i64_or_string(raw: &RawValue) -> Option<i64> {
    integer_or_string(raw).and_then(|v| i64::try_from(v).ok())
}

pub(crate) fn i32_or_string(raw: &RawValue) -> Option<i32> {
    integer_or_string(raw).and_then(|v| i32::try_from(v).ok())
}

/// N6 bool from anything.
pub(crate) fn bool_any(raw: &RawValue) -> Option<bool> {
    fn one_or_zero_int(value: i128) -> Option<bool> {
        match value {
            1 => Some(true),
            0 => Some(false),
            _ => None,
        }
    }
    fn one_or_zero_float(value: f64) -> Option<bool> {
        if (value - 1.0).abs() < f64::EPSILON {
            Some(true)
        } else if value == 0.0 {
            Some(false)
        } else {
            None
        }
    }
    match kind(raw) {
        Kind::Bool => boolean(raw),
        Kind::Number => match integer(raw) {
            Some(value) => one_or_zero_int(value),
            None => one_or_zero_float(raw.get().parse().ok()?),
        },
        Kind::String => {
            let text = string(raw)?;
            if text.eq_ignore_ascii_case("true") {
                Some(true)
            } else if text.eq_ignore_ascii_case("false") {
                Some(false)
            } else if let Ok(value) = text.parse::<i64>() {
                one_or_zero_int(value.into())
            } else if let Ok(value) = text.parse::<f64>() {
                one_or_zero_float(value)
            } else {
                None
            }
        }
        _ => None,
    }
}

/// N8: a JSON string with only the escapes JSON requires.
pub(crate) fn push_string(out: &mut String, text: &str) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\u{c}' => out.push_str("\\f"),
            '\r' => out.push_str("\\r"),
            c if c < '\u{20}' => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

/// N4: the shortest text that reads back as the same `f32`; `null` for NaN
/// and the infinities.
pub(crate) fn push_f32(out: &mut String, value: f32) {
    if value.is_finite() {
        out.push_str(ryu::Buffer::new().format_finite(value));
    } else {
        out.push_str("null");
    }
}

/// N3.
pub(crate) fn push_int(out: &mut String, value: impl Into<i128>) {
    let _ = write!(out, "{}", value.into());
}

pub(crate) fn push_f32_array(out: &mut String, values: &[f32]) {
    out.push('[');
    for (i, value) in values.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        push_f32(out, *value);
    }
    out.push(']');
}

/// A compact JSON object written member by member (N8).
pub(crate) struct ObjectWriter {
    out: String,
    empty: bool,
}

impl ObjectWriter {
    pub(crate) fn new() -> ObjectWriter {
        ObjectWriter {
            out: String::from("{"),
            empty: true,
        }
    }

    /// Starts a member; the caller writes its value into the returned text.
    pub(crate) fn key(&mut self, key: &str) -> &mut String {
        if !self.empty {
            self.out.push(',');
        }
        self.empty = false;
        push_string(&mut self.out, key);
        self.out.push(':');
        &mut self.out
    }

    pub(crate) fn raw(&mut self, key: &str, text: &str) {
        self.key(key).push_str(text);
    }

    pub(crate) fn int(&mut self, key: &str, value: impl Into<i128>) {
        push_int(self.key(key), value);
    }

    pub(crate) fn f32(&mut self, key: &str, value: f32) {
        push_f32(self.key(key), value);
    }

    pub(crate) fn f32_array(&mut self, key: &str, values: &[f32]) {
        push_f32_array(self.key(key), values);
    }

    pub(crate) fn bool(&mut self, key: &str, value: bool) {
        self.key(key).push_str(if value { "true" } else { "false" });
    }

    pub(crate) fn string(&mut self, key: &str, value: &str) {
        push_string(self.key(key), value);
    }

    pub(crate) fn null(&mut self, key: &str) {
        self.key(key).push_str("null");
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.empty
    }

    pub(crate) fn finish(mut self) -> String {
        self.out.push('}');
        self.out
    }
}
