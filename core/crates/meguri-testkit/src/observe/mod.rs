//! Observation values: canonical JSON text, f32 formatting and tolerances
//! (specs/conformance/observations.md O6, O7; dsl.md D3, D11).

use std::collections::BTreeMap;

use serde_json::value::RawValue;
use serde_json::{Map, Number, Value};

use crate::scenario::FormatError;

/// Parses JSON text with every fractional number read as the nearest double,
/// as Python's `json` and std's parser do (scenario files, goldens, oracle
/// responses). serde_json without its `float_roundtrip` feature is off by one
/// ULP for some 17-digit numbers, which would change scenario hashes and
/// fail exact f64 comparisons.
pub fn parse_json(text: &str) -> Result<Value, serde_json::Error> {
    let raw: &RawValue = serde_json::from_str(text)?;
    exact(raw)
}

fn exact(raw: &RawValue) -> Result<Value, serde_json::Error> {
    let text = raw.get();
    match text.as_bytes().first() {
        Some(b'{') => {
            let members: BTreeMap<String, &RawValue> = serde_json::from_str(text)?;
            members
                .into_iter()
                .map(|(key, value)| Ok((key, exact(value)?)))
                .collect::<Result<Map<String, Value>, serde_json::Error>>()
                .map(Value::Object)
        }
        Some(b'[') => {
            let items: Vec<&RawValue> = serde_json::from_str(text)?;
            items.into_iter().map(exact).collect::<Result<_, _>>().map(Value::Array)
        }
        Some(b'-' | b'0'..=b'9') => {
            let value: Value = serde_json::from_str(text)?;
            match &value {
                // Integers are exact already; re-read fractions and exponents.
                Value::Number(n) if n.is_f64() => text
                    .parse::<f64>()
                    .ok()
                    .and_then(Number::from_f64)
                    .map(Value::Number)
                    .ok_or_else(|| serde::de::Error::custom(format!("{text} is not a finite number"))),
                _ => Ok(value),
            }
        }
        _ => serde_json::from_str(text),
    }
}

/// RFC 8785 (JCS) text of a value (dsl.md D3): sorted keys by UTF-16 code
/// units, no whitespace, ECMAScript number text.
pub fn jcs(value: &Value) -> Result<String, FormatError> {
    let mut out = String::new();
    write_jcs(value, &mut out)?;
    Ok(out)
}

fn write_jcs(value: &Value, out: &mut String) -> Result<(), FormatError> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => out.push_str(&jcs_number(n)?),
        Value::String(s) => write_string(s, out),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_jcs(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
            out.push('{');
            for (i, key) in keys.into_iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write_string(key, out);
                out.push(':');
                write_jcs(&map[key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}

const MAX_SAFE: u64 = 1 << 53;

fn jcs_number(n: &Number) -> Result<String, FormatError> {
    if let Some(i) = n.as_i64() {
        if i.unsigned_abs() > MAX_SAFE {
            return Err(FormatError::new(format!(
                "integer {i} is outside the exact JSON number range"
            )));
        }
        return Ok(i.to_string());
    }
    if let Some(u) = n.as_u64() {
        if u > MAX_SAFE {
            return Err(FormatError::new(format!(
                "integer {u} is outside the exact JSON number range"
            )));
        }
        return Ok(u.to_string());
    }
    let x = n
        .as_f64()
        .ok_or_else(|| FormatError::new("a number has no JSON form"))?;
    if !x.is_finite() {
        return Err(FormatError::new("infinities and NaN have no JSON form"));
    }
    if x == 0.0 {
        return Ok("0".to_owned());
    }
    let (negative, digits, n) = shortest_digits(x);
    let sign = if negative { "-" } else { "" };
    let k = len_i64(&digits);
    let text = if k <= n && n <= 21 {
        format!("{digits}{}", zeros(n - k))
    } else if 0 < n && n <= 21 {
        let (a, b) = digits.split_at(usize::try_from(n).unwrap_or(0));
        format!("{a}.{b}")
    } else if -6 < n && n <= 0 {
        format!("0.{}{digits}", zeros(-n))
    } else {
        let e = n - 1;
        let exp = if e >= 0 { format!("e+{e}") } else { format!("e-{}", -e) };
        with_exponent(&digits, &exp)
    };
    Ok(format!("{sign}{text}"))
}

fn len_i64(s: &str) -> i64 {
    i64::try_from(s.len()).unwrap_or(i64::MAX)
}

fn zeros(n: i64) -> String {
    "0".repeat(usize::try_from(n).unwrap_or(0))
}

fn with_exponent(digits: &str, exp: &str) -> String {
    if digits.len() == 1 {
        format!("{digits}{exp}")
    } else {
        format!("{}.{}{exp}", &digits[..1], &digits[1..])
    }
}

/// Shortest round-trip digits of a finite non-zero double: (negative, digits
/// without leading or trailing zeros, n) with |x| = 0.digits × 10^n.
fn shortest_digits(x: f64) -> (bool, String, i64) {
    let mut buffer = ryu::Buffer::new();
    let text = buffer.format_finite(x.abs());
    let (mantissa, exp) = match text.split_once(['e', 'E']) {
        Some((m, e)) => (m, e.parse::<i64>().unwrap_or(0)),
        None => (text, 0),
    };
    let (int_part, frac_part) = mantissa.split_once('.').unwrap_or((mantissa, ""));
    let all = format!("{int_part}{frac_part}");
    let leading = all.len() - all.trim_start_matches('0').len();
    let digits = all[leading..].trim_end_matches('0').to_owned();
    let n = len_i64(int_part) + exp - len_i64(&all[..leading]);
    (x.is_sign_negative(), digits, n)
}

/// JSON string text: `"` `\` and controls escaped, everything else as itself.
/// JCS and the golden layout (Python's `json.dumps(ensure_ascii=False)`)
/// escape the same set.
fn write_string(s: &str, out: &mut String) {
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The golden-file layout of a value (dsl.md D11): sorted keys, two-space
/// indents, arrays of scalars on one line, numbers as Python writes them.
/// Comparisons use parsed values; the layout lets text diffs line up.
pub fn golden_text(value: &Value) -> String {
    let mut out = String::new();
    dump(value, 0, &mut out);
    out.push('\n');
    out
}

fn is_scalar(value: &Value) -> bool {
    !matches!(value, Value::Array(_) | Value::Object(_))
}

fn dump(value: &Value, indent: usize, out: &mut String) {
    let pad = "  ".repeat(indent);
    match value {
        Value::Array(items) if items.iter().all(is_scalar) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push_str(", ");
                }
                dump(item, indent, out);
            }
            out.push(']');
        }
        Value::Array(items) => {
            out.push_str("[\n");
            for (i, item) in items.iter().enumerate() {
                out.push_str(&pad);
                out.push_str("  ");
                dump(item, indent + 1, out);
                out.push_str(if i + 1 < items.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad);
            out.push(']');
        }
        Value::Object(map) if map.is_empty() => out.push_str("{}"),
        Value::Object(map) => {
            out.push_str("{\n");
            for (i, (key, item)) in map.iter().enumerate() {
                out.push_str(&pad);
                out.push_str("  ");
                write_string(key, out);
                out.push_str(": ");
                dump(item, indent + 1, out);
                out.push_str(if i + 1 < map.len() { ",\n" } else { "\n" });
            }
            out.push_str(&pad);
            out.push('}');
        }
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::String(s) => write_string(s, out),
        Value::Number(n) => out.push_str(&python_number(n)),
    }
}

/// A number as Python's `json.dumps` writes it: integers in decimal, floats
/// as `repr` (fixed notation for decimal exponents -4 to 15, else `1e-05`,
/// `1e+22`).
fn python_number(n: &Number) -> String {
    if n.is_i64() || n.is_u64() {
        return n.to_string();
    }
    let x = n.as_f64().unwrap_or(0.0);
    if x == 0.0 {
        return if x.is_sign_negative() {
            "-0.0".to_owned()
        } else {
            "0.0".to_owned()
        };
    }
    let (negative, digits, n) = shortest_digits(x);
    let sign = if negative { "-" } else { "" };
    let k = len_i64(&digits);
    let text = if (-3..=16).contains(&n) {
        if n <= 0 {
            format!("0.{}{digits}", zeros(-n))
        } else if n >= k {
            format!("{digits}{}.0", zeros(n - k))
        } else {
            let (a, b) = digits.split_at(usize::try_from(n).unwrap_or(0));
            format!("{a}.{b}")
        }
    } else {
        let e = n - 1;
        let exp = if e >= 0 {
            format!("e+{e:02}")
        } else {
            format!("e-{:02}", -e)
        };
        with_exponent(&digits, &exp)
    };
    format!("{sign}{text}")
}

/// The JSON number for an f32: its shortest round-trip decimal (O6), never the
/// widened double.
pub fn f32_value(x: f32) -> Result<Value, FormatError> {
    if !x.is_finite() {
        return Err(FormatError::new(format!("non-finite f32 {x} has no JSON form")));
    }
    let mut buffer = ryu::Buffer::new();
    let text = buffer.format_finite(x);
    let widened: f64 = text
        .parse()
        .map_err(|_| FormatError::new(format!("cannot read back {text}")))?;
    Number::from_f64(widened)
        .map(Value::Number)
        .ok_or_else(|| FormatError::new("non-finite"))
}

/// Relative tolerance on memory states inside `next_states` (O7).
pub const MEMORY_STATE_TOLERANCE: f64 = 1e-5;
/// Relative tolerance on fractional numbers inside `stats` (O7).
pub const STATS_TOLERANCE: f64 = 1e-6;

/// The relative tolerance declared for the value at this pointer (unescaped
/// segments of `/steps/<i>/<kind>/...`), if any (O7).
pub fn tolerance(segments: &[String]) -> Option<f64> {
    match segments.get(2)?.as_str() {
        "next_states" => {
            let n = segments.len();
            let last = segments.last()?;
            (n >= 2 && segments[n - 2] == "memory_state" && (last == "stability" || last == "difficulty"))
                .then_some(MEMORY_STATE_TOLERANCE)
        }
        "stats" => Some(STATS_TOLERANCE),
        _ => None,
    }
}
