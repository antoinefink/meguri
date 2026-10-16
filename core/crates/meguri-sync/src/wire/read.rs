//! The reading rules every decoded body follows (specs/sync/wire-format.md
//! W1.2, W11): valid JSON per `model/json-numbers` N9 and N10, at most 127
//! levels of nesting, envelope members read once, strict and loose numbers,
//! and tuples read by position. A break is a [`DecodeError`] that names where
//! it happened, never the body; decoding allocates only in proportion to the
//! bytes read and never recurses deeper than the nesting limit (R1562).

use std::collections::HashMap;
use std::fmt::{self, Write as _};

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

/// The deepest nesting a body may have, the outermost value counting as
/// level 1 (W1.2).
pub(crate) const MAX_DEPTH: usize = 127;

/// Where in a body a rule broke, and which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct DecodeError {
    /// A path such as `decks[0][3].conf`, empty for the whole body.
    pub(crate) at: String,
    pub(crate) why: String,
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.at.is_empty() {
            write!(f, "the body {}", self.why)
        } else {
            write!(f, "{} {}", self.at, self.why)
        }
    }
}

pub(crate) type Result<T> = std::result::Result<T, DecodeError>;

/// A place in a body, rendered only when something breaks there.
#[derive(Clone, Copy, Debug)]
pub(crate) enum At<'a> {
    Root,
    Member(&'a At<'a>, &'a str),
    Index(&'a At<'a>, usize),
}

impl<'a> At<'a> {
    pub(crate) fn member(&'a self, key: &'a str) -> At<'a> {
        At::Member(self, key)
    }

    pub(crate) fn index(&'a self, index: usize) -> At<'a> {
        At::Index(self, index)
    }

    pub(crate) fn fail(&self, why: impl Into<String>) -> DecodeError {
        DecodeError {
            at: self.render(),
            why: why.into(),
        }
    }

    fn render(&self) -> String {
        let mut steps = Vec::new();
        let mut current = self;
        loop {
            match current {
                At::Root => break,
                At::Member(parent, key) => {
                    steps.push(Step::Key(key));
                    current = parent;
                }
                At::Index(parent, index) => {
                    steps.push(Step::Index(*index));
                    current = parent;
                }
            }
        }
        let mut out = String::new();
        for step in steps.iter().rev() {
            match step {
                Step::Key(key) if out.is_empty() => out.push_str(key),
                Step::Key(key) => {
                    out.push('.');
                    out.push_str(key);
                }
                Step::Index(index) => {
                    let _ = write!(out, "[{index}]");
                }
            }
        }
        out
    }
}

enum Step<'a> {
    Key(&'a str),
    Index(usize),
}

/// A whole body as one JSON value: UTF-8, nested at most [`MAX_DEPTH`]
/// levels, valid per N9 and N10.
pub(crate) fn body(bytes: &[u8]) -> Result<&RawValue> {
    let text = std::str::from_utf8(bytes).map_err(|_| At::Root.fail("is not UTF-8"))?;
    if depth_exceeds(text.as_bytes(), MAX_DEPTH) {
        return Err(At::Root.fail(format!("nests deeper than {MAX_DEPTH} levels")));
    }
    meguri_model::json_text::parse(text).ok_or_else(|| At::Root.fail("is not valid JSON"))
}

/// Whether the arrays and objects of `text` nest deeper than `limit`. One
/// pass with no recursion; brackets inside strings do not count. On text
/// that is not JSON the answer means nothing, and the parser refuses it.
pub(crate) fn depth_exceeds(text: &[u8], limit: usize) -> bool {
    let mut depth = 0usize;
    let mut in_string = false;
    let mut escaped = false;
    for &byte in text {
        if in_string {
            if escaped {
                escaped = false;
            } else if byte == b'\\' {
                escaped = true;
            } else if byte == b'"' {
                in_string = false;
            }
            continue;
        }
        match byte {
            b'"' => in_string = true,
            b'[' | b'{' => {
                depth += 1;
                if depth > limit {
                    return true;
                }
            }
            b']' | b'}' => depth = depth.saturating_sub(1),
            _ => {}
        }
    }
    false
}

/// The JSON type of a raw value, from its first character (raw text never
/// starts with whitespace).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Object,
    Array,
    String,
    Number,
    Bool,
    Null,
}

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

/// An object's members in text order, every occurrence of a repeated key
/// kept.
pub(crate) struct Object<'a> {
    members: Vec<(String, &'a RawValue)>,
}

struct Members<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> std::result::Result<Self, D::Error> {
        struct MembersVisitor;

        impl<'de> Visitor<'de> for MembersVisitor {
            type Value = Members<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> std::result::Result<Self::Value, A::Error> {
                let mut members = Vec::new();
                while let Some(key) = map.next_key::<String>()? {
                    let value: &'de RawValue = map.next_value()?;
                    members.push((key, value));
                }
                Ok(Members(members))
            }
        }

        deserializer.deserialize_map(MembersVisitor)
    }
}

/// An object value.
pub(crate) fn object<'a>(raw: &'a RawValue, at: At<'_>) -> Result<Object<'a>> {
    if kind(raw) != Kind::Object {
        return Err(at.fail("is not an object"));
    }
    serde_json::from_str::<Members<'a>>(raw.get())
        .map(|members| Object { members: members.0 })
        .map_err(|_| at.fail("is not an object"))
}

impl<'a> Object<'a> {
    /// A member this file reads (W1.2): `None` when absent; a repeated one
    /// makes the object invalid.
    pub(crate) fn get(&self, key: &str, at: At<'_>) -> Result<Option<&'a RawValue>> {
        let mut found = None;
        for (name, value) in &self.members {
            if name == key {
                if found.is_some() {
                    return Err(at.member(key).fail("is repeated"));
                }
                found = Some(*value);
            }
        }
        Ok(found)
    }

    /// A required member.
    pub(crate) fn required(&self, key: &str, at: At<'_>) -> Result<&'a RawValue> {
        self.get(key, at)?.ok_or_else(|| at.member(key).fail("is missing"))
    }

    /// The members of an object that maps names to values (N9): a repeated
    /// name keeps its last value, at the place of its last occurrence.
    pub(crate) fn last_wins(self) -> Vec<(String, &'a RawValue)> {
        let mut last: HashMap<&str, usize> = HashMap::with_capacity(self.members.len());
        for (index, (name, _)) in self.members.iter().enumerate() {
            last.insert(name.as_str(), index);
        }
        let keep: Vec<bool> = self
            .members
            .iter()
            .enumerate()
            .map(|(index, (name, _))| last.get(name.as_str()) == Some(&index))
            .collect();
        self.members
            .into_iter()
            .zip(keep)
            .filter_map(|(member, keep)| keep.then_some(member))
            .collect()
    }
}

/// An array value's elements.
pub(crate) fn array<'a>(raw: &'a RawValue, at: At<'_>) -> Result<Vec<&'a RawValue>> {
    if kind(raw) != Kind::Array {
        return Err(at.fail("is not an array"));
    }
    serde_json::from_str::<Vec<&'a RawValue>>(raw.get()).map_err(|_| at.fail("is not an array"))
}

/// A tuple (W1.2): an array with `min` to `max` elements.
pub(crate) fn tuple<'a>(raw: &'a RawValue, min: usize, max: usize, at: At<'_>) -> Result<Vec<&'a RawValue>> {
    let items = array(raw, at)?;
    if items.len() < min || items.len() > max {
        let wanted = if min == max {
            format!("{min}")
        } else {
            format!("{min} to {max}")
        };
        return Err(at.fail(format!("has {} elements, not {wanted}", items.len())));
    }
    Ok(items)
}

/// A JSON string, unescaped.
pub(crate) fn string(raw: &RawValue, at: At<'_>) -> Result<String> {
    if kind(raw) != Kind::String {
        return Err(at.fail("is not a string"));
    }
    serde_json::from_str::<String>(raw.get()).map_err(|_| at.fail("is not a string"))
}

/// `true` or `false`.
pub(crate) fn boolean(raw: &RawValue, at: At<'_>) -> Result<bool> {
    match raw.get() {
        "true" => Ok(true),
        "false" => Ok(false),
        _ => Err(at.fail("is not true or false")),
    }
}

pub(crate) fn is_null(raw: &RawValue) -> bool {
    kind(raw) == Kind::Null
}

/// A number written without a fraction or an exponent, as an `i128`;
/// `None` for anything else or a literal too long for `i128`.
fn integer_literal(raw: &RawValue) -> Option<i128> {
    let text = raw.get();
    if kind(raw) != Kind::Number || text.contains(['.', 'e', 'E']) {
        return None;
    }
    text.parse().ok()
}

/// The strict reader (W1.2, N2): an integer literal within `T`'s range.
pub(crate) fn strict<T: TryFrom<i128>>(raw: &RawValue, at: At<'_>) -> Result<T> {
    integer_literal(raw)
        .and_then(|value| T::try_from(value).ok())
        .ok_or_else(|| at.fail(format!("is not an integer of type {}", short_type_name::<T>())))
}

fn short_type_name<T>() -> &'static str {
    let name = std::any::type_name::<T>();
    name.rsplit("::").next().unwrap_or(name)
}

/// The loose reader of W11, steps 1, 2 and 4: any JSON number as an `i64`.
/// An integer literal counts as its value, clamped to the `i64` range; one
/// with a fraction or exponent is read as the nearest 64-bit float, then
/// truncated toward zero and clamped. Anything but a number refuses.
pub(crate) fn loose(raw: &RawValue, at: At<'_>) -> Result<i64> {
    if kind(raw) != Kind::Number {
        return Err(at.fail("is not a number"));
    }
    let text = raw.get();
    if text.contains(['.', 'e', 'E']) {
        // Rust's float-to-integer `as` truncates toward zero and saturates.
        let value: f64 = text.parse().map_err(|_| at.fail("is not a number"))?;
        return Ok(value as i64);
    }
    Ok(match text.parse::<i128>() {
        Ok(value) => value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64,
        // A literal longer than i128 holds: beyond either bound.
        Err(_) if text.starts_with('-') => i64::MIN,
        Err(_) => i64::MAX,
    })
}

/// W11 step 3 for `i32`: the low 32 bits, two's complement.
pub(crate) fn loose_i32(value: i64) -> i32 {
    value as i32
}

/// W11 step 3 for `u32`: 0 when negative, otherwise the low 32 bits.
pub(crate) fn loose_u32(value: i64) -> u32 {
    if value < 0 { 0 } else { value as u32 }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(text: &str) -> &RawValue {
        serde_json::from_str(text).unwrap()
    }

    #[test]
    fn paths_render_members_and_indexes() {
        let root = At::Root;
        let decks = root.member("decks");
        let first = decks.index(0);
        let third = first.index(3);
        let conf = third.member("conf");
        assert_eq!(conf.fail("x").at, "decks[0][3].conf");
        assert_eq!(At::Root.fail("x").at, "");
    }

    #[test]
    fn brackets_inside_strings_do_not_nest() {
        assert!(!depth_exceeds(br#"["[[[[\"]]]]", {"a]": "{"}]"#, 2));
        assert!(depth_exceeds(b"[[[]]]", 2));
        assert!(!depth_exceeds(b"[[]][[]]", 2));
    }

    #[test]
    fn loose_reads_clamp_and_truncate() {
        // W11's table, through the full reader.
        let at = At::Root;
        assert_eq!(loose(raw("-1e300"), at).ok(), Some(i64::MIN));
        assert_eq!(loose(raw("1e30"), at).ok(), Some(i64::MAX));
        assert_eq!(loose(raw(&format!("-{}", "9".repeat(60))), at).ok(), Some(i64::MIN));
    }
}
