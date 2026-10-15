//! The JSON the upgrade reads from schema-11 `col` columns, member by member
//! as exact text (`specs/model/json-numbers.md` N7, N9), and the compact JSON
//! that step 14 writes into `config.val` (`storage/schema-upgrade` U9.2).
//! Record conversions themselves are meguri-model's (U21).

use std::collections::{BTreeMap, HashMap};
use std::fmt;
use std::fmt::Write as _;

use serde::de::{Deserialize, Deserializer, MapAccess, Visitor};
use serde_json::value::RawValue;

/// An object's members in text order, a repeated key reduced to its last
/// occurrence at that occurrence's place (N9). `None` when `text` is not a
/// JSON object.
pub(crate) fn members(text: &str) -> Option<Vec<(String, &RawValue)>> {
    let all = serde_json::from_str::<Members<'_>>(text).ok()?.0;
    let mut last: HashMap<&str, usize> = HashMap::with_capacity(all.len());
    for (i, (key, _)) in all.iter().enumerate() {
        last.insert(key.as_str(), i);
    }
    let keep: Vec<bool> = all
        .iter()
        .enumerate()
        .map(|(i, (key, _))| last.get(key.as_str()) == Some(&i))
        .collect();
    Some(
        all.into_iter()
            .zip(keep)
            .filter_map(|(member, keep)| keep.then_some(member))
            .collect(),
    )
}

/// The members of a JSON object with repeated keys kept.
struct Members<'a>(Vec<(String, &'a RawValue)>);

impl<'de> Deserialize<'de> for Members<'de> {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Members<'de>, D::Error> {
        struct MembersVisitor;

        impl<'de> Visitor<'de> for MembersVisitor {
            type Value = Members<'de>;

            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a JSON object")
            }

            fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<Members<'de>, A::Error> {
                let mut members = Vec::new();
                while let Some(entry) = map.next_entry::<String, &'de RawValue>()? {
                    members.push(entry);
                }
                Ok(Members(members))
            }
        }

        deserializer.deserialize_map(MembersVisitor)
    }
}

/// S12's id key: an optional `-` and decimal digits, within `i64`.
pub(crate) fn id_key(key: &str) -> Option<i64> {
    let digits = key.strip_prefix('-').unwrap_or(key);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    key.parse().ok()
}

/// The string value of member `key` of a JSON object (the last occurrence).
pub(crate) fn string_member(object: &str, key: &str) -> Option<String> {
    let (_, raw) = members(object)?.into_iter().find(|(k, _)| k == key)?;
    serde_json::from_str(raw.get()).ok()
}

/// Nesting deeper than this refuses the value. The writer below recurses
/// once per level, and raw capture has no nesting limit of its own.
const MAX_DEPTH: usize = 128;

/// U9.2: the value written back without whitespace, members sorted by name
/// in code point order at every depth (a repeated name keeps its last
/// value), integers that fit 64 bits as integers, every other number as the
/// shortest decimal that reads back as the same 64-bit float, and strings
/// with only the escapes JSON requires. `None` when the text is not JSON or
/// a number is beyond the 64-bit float range.
pub(crate) fn compact(text: &str) -> Option<String> {
    let raw: &RawValue = serde_json::from_str(text).ok()?;
    let mut out = String::with_capacity(text.len());
    write_value(raw, 0, &mut out)?;
    Some(out)
}

fn write_value(raw: &RawValue, depth: usize, out: &mut String) -> Option<()> {
    let text = raw.get();
    match text.as_bytes().first()? {
        b'{' => {
            if depth >= MAX_DEPTH {
                return None;
            }
            // BTreeMap: sorted by code point (String's order is UTF-8 byte
            // order); inserting a repeated name keeps the last value.
            let object: BTreeMap<String, &RawValue> = serde_json::from_str(text).ok()?;
            out.push('{');
            for (i, (key, value)) in object.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_value(value, depth + 1, out)?;
            }
            out.push('}');
        }
        b'[' => {
            if depth >= MAX_DEPTH {
                return None;
            }
            let items: Vec<&RawValue> = serde_json::from_str(text).ok()?;
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_value(item, depth + 1, out)?;
            }
            out.push(']');
        }
        b'"' => write_string(&serde_json::from_str::<String>(text).ok()?, out),
        b't' | b'f' | b'n' => out.push_str(text),
        _ => write_number(text, out)?,
    }
    Some(())
}

/// A number token, read as serde_json reads it: an integer within `u64` or
/// `i64` stays an integer; `-0`, a fraction, an exponent or a larger integer
/// becomes an `f64` from serde_json's own parser (not the exactly rounded
/// one: the spec's `123456789012345678901` gives `1.2345678901234567e20`, as
/// it does for Anki), written by ryu as serde_json writes it.
fn write_number(token: &str, out: &mut String) -> Option<()> {
    let number: serde_json::Number = serde_json::from_str(token).ok()?;
    if let Some(value) = number.as_u64() {
        // Writing to a String cannot fail.
        let _ = write!(out, "{value}");
    } else if let Some(value) = number.as_i64() {
        let _ = write!(out, "{value}");
    } else {
        let value = number.as_f64().filter(|v| v.is_finite())?;
        out.push_str(ryu::Buffer::new().format_finite(value));
    }
    Some(())
}

fn write_string(text: &str, out: &mut String) {
    out.push('"');
    for c in text.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `storage/schema-upgrade` U9.2's examples.
    #[test]
    fn compact_json_follows_the_spec_examples() {
        let cases = [
            (
                r#"{"b": 1, "a": 2.50, "c": [1.0, 2]}"#,
                r#"{"a":2.5,"b":1,"c":[1.0,2]}"#,
            ),
            (r#"{"z": {"b": 1, "a": 2}}"#, r#"{"z":{"a":2,"b":1}}"#),
            ("2", "2"),
            ("12345678901234567890", "12345678901234567890"),
            ("123456789012345678901", "1.2345678901234567e20"),
            ("123.0", "123.0"),
            ("1E2", "100.0"),
            ("-0", "-0.0"),
            ("1e15", "1000000000000000.0"),
            ("1e16", "1e16"),
            ("0.0001", "0.0001"),
            ("0.000001", "1e-6"),
            ("1e300", "1e300"),
            (r#""a/bé\u0001\"""#, r#""a/bé\u0001\"""#),
            ("true", "true"),
            ("null", "null"),
        ];
        for (input, expected) in cases {
            assert_eq!(compact(input).as_deref(), Some(expected), "{input}");
        }
    }

    #[test]
    fn compact_json_keeps_the_last_of_repeated_names_and_refuses_bad_input() {
        assert_eq!(
            compact(r#"{"a": 1, "b": 2, "a": 3}"#).as_deref(),
            Some(r#"{"a":3,"b":2}"#)
        );
        assert_eq!(compact("-9223372036854775808").as_deref(), Some("-9223372036854775808"));
        assert_eq!(
            compact("-9223372036854775809").as_deref(),
            Some("-9.223372036854776e18")
        );
        assert_eq!(compact("1e400"), None);
        assert_eq!(compact("[1,"), None);
        assert_eq!(compact(r#""\ud800""#), None);
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert_eq!(compact(&deep), None);
        assert_eq!(compact(r#""\u007f\t\u001F""#).as_deref(), Some("\"\u{7f}\\t\\u001f\""));
        // A raw control character is not JSON.
        assert_eq!(compact("\"\t\""), None);
    }

    #[test]
    fn members_keep_text_order_and_the_last_repeated_key() {
        let got = members(r#"{"b": 1, "a": [2], "b": 3}"#).unwrap();
        let got: Vec<(&str, &str)> = got.iter().map(|(k, v)| (k.as_str(), v.get())).collect();
        assert_eq!(got, [("a", "[2]"), ("b", "3")]);
        assert!(members("[1]").is_none());
        assert!(members("{").is_none());
    }

    #[test]
    fn id_keys_are_decimal_i64() {
        assert_eq!(id_key("1700000000000"), Some(1_700_000_000_000));
        assert_eq!(id_key("-5"), Some(-5));
        assert_eq!(id_key("0"), Some(0));
        for bad in ["", "-", "1.0", "x", "+1", "99999999999999999999"] {
            assert_eq!(id_key(bad), None, "{bad}");
        }
    }
}
