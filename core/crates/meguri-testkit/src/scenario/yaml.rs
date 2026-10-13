//! YAML 1.2 core schema, restricted to JSON values (specs/conformance/dsl.md D2).
//!
//! granit-parser (serde-saphyr's parser) supplies the events. Scalars are
//! resolved here rather than through serde-saphyr's typeless deserializer,
//! which also accepts YAML 1.1 forms (`yes`, `1_000`, `0b101`, octal `017`),
//! aliases, merge keys and tags: the oracle side reads none of those as this
//! spec does, and the two must hash every scenario alike.

use std::collections::BTreeSet;

use granit_parser::{Event, Parser, ScalarStyle, Span};
use serde_json::{Map, Number, Value};

use super::FormatError;

/// The largest integer magnitude a scenario may hold (exact in a double).
const MAX_SAFE: i128 = 1 << 53;

pub(super) fn parse(text: &str) -> Result<Value, FormatError> {
    let mut events = Events {
        parser: Parser::new_from_str(text),
    };
    match events.next()? {
        Some((Event::StreamStart, _)) => {}
        _ => return Err(FormatError::new("not a YAML stream")),
    }
    let value = match events.next()? {
        Some((Event::DocumentStart(..), _)) => match events.next()? {
            Some((Event::DocumentEnd, _)) => Value::Null,
            Some((event, span)) => {
                let value = node(&mut events, event, span)?;
                match events.next()? {
                    Some((Event::DocumentEnd, _)) => value,
                    _ => return Err(FormatError::new("a scenario file holds one YAML document")),
                }
            }
            None => return Err(FormatError::new("unexpected end of the YAML stream")),
        },
        // An empty stream is the null document.
        Some((Event::StreamEnd, _)) => return Ok(Value::Null),
        _ => return Err(FormatError::new("not a YAML document")),
    };
    match events.next()? {
        Some((Event::StreamEnd, _)) => Ok(value),
        _ => Err(FormatError::new("a scenario file holds one YAML document")),
    }
}

struct Events<'a> {
    parser: Parser<'a, granit_parser::StrInput<'a>>,
}

impl<'a> Events<'a> {
    /// The next data event; comments are skipped.
    fn next(&mut self) -> Result<Option<(Event<'a>, Span)>, FormatError> {
        loop {
            match self.parser.next() {
                None => return Ok(None),
                Some(Err(e)) => return Err(FormatError::new(format!("YAML syntax: {e}"))),
                Some(Ok((Event::Comment(..), _))) => continue,
                Some(Ok(event)) => return Ok(Some(event)),
            }
        }
    }
}

fn at(span: Span, message: impl std::fmt::Display) -> FormatError {
    FormatError::new(format!(
        "line {} column {}: {message}",
        span.start.line(),
        span.start.col() + 1
    ))
}

fn node<'a>(events: &mut Events<'a>, event: Event<'a>, span: Span) -> Result<Value, FormatError> {
    match event {
        Event::Alias(_) => Err(at(span, "aliases are not allowed in scenarios")),
        Event::Scalar(text, style, _anchor, tag) => {
            if tag.is_some() {
                return Err(at(span, "tags are not allowed in scenarios"));
            }
            if style == ScalarStyle::Plain {
                resolve_plain(&text).map_err(|m| at(span, m))
            } else {
                Ok(Value::String(text.into_owned()))
            }
        }
        Event::SequenceStart(_, _anchor, tag) => {
            if tag.is_some() {
                return Err(at(span, "tags are not allowed in scenarios"));
            }
            let mut items = Vec::new();
            loop {
                match events.next()? {
                    Some((Event::SequenceEnd, _)) => return Ok(Value::Array(items)),
                    Some((event, span)) => items.push(node(events, event, span)?),
                    None => return Err(at(span, "unterminated sequence")),
                }
            }
        }
        Event::MappingStart(_, _anchor, tag) => {
            if tag.is_some() {
                return Err(at(span, "tags are not allowed in scenarios"));
            }
            let mut map = Map::new();
            let mut seen = BTreeSet::new();
            loop {
                let key = match events.next()? {
                    Some((Event::MappingEnd, _)) => return Ok(Value::Object(map)),
                    Some((Event::Scalar(text, style, _, tag), key_span)) => {
                        if tag.is_some() {
                            return Err(at(key_span, "tags are not allowed in scenarios"));
                        }
                        if style == ScalarStyle::Plain {
                            if text == "<<" {
                                return Err(at(key_span, "merge keys are not allowed in scenarios"));
                            }
                            match resolve_plain(&text).map_err(|m| at(key_span, m))? {
                                Value::String(s) => s,
                                _ => return Err(at(key_span, format!("mapping keys must be strings, not {text}"))),
                            }
                        } else {
                            text.into_owned()
                        }
                    }
                    Some((Event::Alias(_), key_span)) => {
                        return Err(at(key_span, "aliases are not allowed in scenarios"));
                    }
                    Some((_, key_span)) => return Err(at(key_span, "mapping keys must be strings")),
                    None => return Err(at(span, "unterminated mapping")),
                };
                if !seen.insert(key.clone()) {
                    return Err(at(span, format!("duplicate key {key:?}")));
                }
                let value = match events.next()? {
                    Some((event, value_span)) => node(events, event, value_span)?,
                    None => return Err(at(span, "unterminated mapping")),
                };
                map.insert(key, value);
            }
        }
        _ => Err(at(span, "unexpected YAML event")),
    }
}

/// The core schema's resolution of a plain scalar (D2).
fn resolve_plain(text: &str) -> Result<Value, String> {
    match text {
        "true" | "True" | "TRUE" => return Ok(Value::Bool(true)),
        "false" | "False" | "FALSE" => return Ok(Value::Bool(false)),
        "" | "~" | "null" | "Null" | "NULL" => return Ok(Value::Null),
        _ => {}
    }
    if let Some(n) = core_int(text) {
        return integer(n?);
    }
    if is_core_float(text) {
        let lower = text.to_ascii_lowercase();
        if lower.ends_with("inf") || lower.ends_with("nan") {
            return Err(format!("{text} has no JSON form"));
        }
        let x: f64 = text.parse().map_err(|_| format!("{text} is not a number"))?;
        return float(x).ok_or_else(|| format!("{text} has no JSON form"));
    }
    Ok(Value::String(text.to_owned()))
}

/// `[-+]?[0-9]+`, `0o[0-7]+` or `0x[0-9a-fA-F]+`: Some(value) when it matches.
fn core_int(text: &str) -> Option<Result<i128, String>> {
    let too_big = || format!("integer {text} is outside ±2^53");
    if let Some(digits) = text.strip_prefix("0o") {
        if !digits.is_empty() && digits.bytes().all(|b| (b'0'..=b'7').contains(&b)) {
            return Some(i128::from_str_radix(digits, 8).map_err(|_| too_big()));
        }
        return None;
    }
    if let Some(digits) = text.strip_prefix("0x") {
        if !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Some(i128::from_str_radix(digits, 16).map_err(|_| too_big()));
        }
        return None;
    }
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if !unsigned.is_empty() && unsigned.bytes().all(|b| b.is_ascii_digit()) {
        return Some(text.parse::<i128>().map_err(|_| too_big()));
    }
    None
}

/// `[-+]?(\.[0-9]+|[0-9]+(\.[0-9]*)?)([eE][-+]?[0-9]+)?`, or an infinity or
/// NaN spelling (rejected by the caller).
fn is_core_float(text: &str) -> bool {
    let unsigned = text.strip_prefix(['-', '+']).unwrap_or(text);
    if matches!(unsigned, ".inf" | ".Inf" | ".INF") || matches!(text, ".nan" | ".NaN" | ".NAN") {
        return true;
    }
    let (mantissa, exponent) = match unsigned.find(['e', 'E']) {
        Some(i) => (&unsigned[..i], Some(&unsigned[i + 1..])),
        None => (unsigned, None),
    };
    let mantissa_ok = match mantissa.split_once('.') {
        Some(("", frac)) => !frac.is_empty() && frac.bytes().all(|b| b.is_ascii_digit()),
        Some((int, frac)) => {
            !int.is_empty() && int.bytes().all(|b| b.is_ascii_digit()) && frac.bytes().all(|b| b.is_ascii_digit())
        }
        None => !mantissa.is_empty() && mantissa.bytes().all(|b| b.is_ascii_digit()),
    };
    let exponent_ok = exponent.is_none_or(|e| {
        let digits = e.strip_prefix(['-', '+']).unwrap_or(e);
        !digits.is_empty() && digits.bytes().all(|b| b.is_ascii_digit())
    });
    mantissa_ok && exponent_ok
}

fn integer(n: i128) -> Result<Value, String> {
    if n.abs() > MAX_SAFE {
        return Err(format!("integer {n} is outside ±2^53"));
    }
    let n = i64::try_from(n).map_err(|_| format!("integer {n} is outside ±2^53"))?;
    Ok(Value::Number(Number::from(n)))
}

fn float(x: f64) -> Option<Value> {
    Number::from_f64(x).map(Value::Number)
}

/// Generated scenarios: JSON with no duplicate keys, numbers within the
/// scenario range.
pub(super) fn parse_json(text: &str) -> Result<Value, FormatError> {
    let value: Value = serde_json::from_str(text).map_err(|e| FormatError::new(format!("JSON: {e}")))?;
    // serde_json keeps the last of two equal keys: count the keys of the
    // text's objects by re-reading it as a stream of map entries.
    check_duplicates(text)?;
    check_numbers(&value)?;
    Ok(value)
}

fn check_numbers(value: &Value) -> Result<(), FormatError> {
    match value {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                integer(i128::from(i)).map(|_| ()).map_err(FormatError::new)
            } else if let Some(u) = n.as_u64() {
                integer(i128::from(u)).map(|_| ()).map_err(FormatError::new)
            } else {
                Ok(())
            }
        }
        Value::Array(items) => items.iter().try_for_each(check_numbers),
        Value::Object(map) => map.values().try_for_each(check_numbers),
        _ => Ok(()),
    }
}

fn check_duplicates(text: &str) -> Result<(), FormatError> {
    use serde::de::{self, Deserializer, MapAccess, SeqAccess, Visitor};

    struct NoDuplicates;

    impl<'de> Visitor<'de> for NoDuplicates {
        type Value = ();

        fn expecting(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            f.write_str("any JSON value")
        }

        fn visit_bool<E>(self, _: bool) -> Result<(), E> {
            Ok(())
        }
        fn visit_i64<E>(self, _: i64) -> Result<(), E> {
            Ok(())
        }
        fn visit_u64<E>(self, _: u64) -> Result<(), E> {
            Ok(())
        }
        fn visit_f64<E>(self, _: f64) -> Result<(), E> {
            Ok(())
        }
        fn visit_str<E>(self, _: &str) -> Result<(), E> {
            Ok(())
        }
        fn visit_unit<E>(self) -> Result<(), E> {
            Ok(())
        }

        fn visit_seq<A: SeqAccess<'de>>(self, mut seq: A) -> Result<(), A::Error> {
            while seq.next_element_seed(Seed)?.is_some() {}
            Ok(())
        }

        fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
            let mut seen = BTreeSet::new();
            while let Some(key) = map.next_key::<String>()? {
                if !seen.insert(key.clone()) {
                    return Err(de::Error::custom(format!("duplicate key {key:?}")));
                }
                map.next_value_seed(Seed)?;
            }
            Ok(())
        }
    }

    struct Seed;

    impl<'de> de::DeserializeSeed<'de> for Seed {
        type Value = ();
        fn deserialize<D: Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
            deserializer.deserialize_any(NoDuplicates)
        }
    }

    let mut de = serde_json::Deserializer::from_str(text);
    de::DeserializeSeed::deserialize(Seed, &mut de).map_err(|e| FormatError::new(format!("JSON: {e}")))
}
