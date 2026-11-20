//! The custom data value C(data, key) of `specs/search/properties.md` P12,
//! behind `meguri_custom_data` (P15) and the `has-cd:`, `prop:cdn:` and
//! `prop:cds:` terms.
//!
//! The `cd` text is read as Anki's reader reads it, serde_json with its
//! default features (P12 step 2): the out-of-range numbers, unpaired
//! surrogate escapes, nesting over 127 levels and trailing text that make
//! the whole text fail are that parser's refusals, and its numbers are the
//! floats of `model/json-numbers` N11. The workspace never enables
//! `float_roundtrip` or `preserve_order`, which would change both. Values are
//! written back by this module (steps 4 and 5), not by serde_json's writer.

use meguri_model::card_data::extractors::Fields;
use serde_json::{Number, Value};

/// C(data, key), or `None` when it is missing.
pub fn custom_data(data: &str, key: &str) -> Option<String> {
    let text = Fields::custom_data(data)?;
    let Value::Object(members) = serde_json::from_str::<Value>(&text).ok()? else {
        return None;
    };
    // serde_json keeps the last member of a repeated name.
    Some(match members.get(key)? {
        Value::String(content) => content.clone(),
        other => {
            let mut out = String::new();
            write_value(&mut out, other);
            out
        }
    })
}

/// Step 4: compact JSON, with members in the parser's (sorted) order. The
/// parser refused nesting deeper than 127 levels, so this recursion is
/// bounded.
fn write_value(out: &mut String, value: &Value) {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(true) => out.push_str("true"),
        Value::Bool(false) => out.push_str("false"),
        Value::Number(number) => write_number(out, number),
        Value::String(text) => write_string(out, text),
        Value::Array(items) => {
            out.push('[');
            for (index, item) in items.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_value(out, item);
            }
            out.push(']');
        }
        Value::Object(members) => {
            out.push('{');
            for (index, (name, item)) in members.iter().enumerate() {
                if index > 0 {
                    out.push(',');
                }
                write_string(out, name);
                out.push(':');
                write_value(out, item);
            }
            out.push('}');
        }
    }
}

fn write_string(out: &mut String, text: &str) {
    out.push_str(&serde_json::to_string(text).expect("a string always serializes"));
}

/// Step 5: integers that fit 64 bits keep their digits; every other number
/// is its float, written in its shortest form.
fn write_number(out: &mut String, number: &Number) {
    if let Some(value) = number.as_u64() {
        out.push_str(&value.to_string());
    } else if let Some(value) = number.as_i64() {
        out.push_str(&value.to_string());
    } else if let Some(value) = number.as_f64() {
        out.push_str(&shortest(value));
    }
}

/// Step 5's layout of F's shortest digits d1...dn with exponent e.
fn shortest(value: f64) -> String {
    // `{:e}` writes the shortest digits that read back as the value.
    let written = format!("{value:e}");
    let (mantissa, exponent) = written.split_once('e').expect("LowerExp writes an exponent");
    let exponent: i32 = exponent.parse().expect("LowerExp writes a decimal exponent");
    let (sign, mantissa) = match mantissa.strip_prefix('-') {
        Some(rest) => ("-", rest),
        None => ("", mantissa),
    };
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let n = digits.len() as i32;
    let body = if (0..=15).contains(&exponent) && n <= exponent + 1 {
        format!("{digits}{}.0", "0".repeat((exponent + 1 - n) as usize))
    } else if (0..=15).contains(&exponent) {
        let (whole, fraction) = digits.split_at(exponent as usize + 1);
        format!("{whole}.{fraction}")
    } else if (-5..=-1).contains(&exponent) {
        format!("0.{}{digits}", "0".repeat((-exponent - 1) as usize))
    } else {
        let (first, rest) = digits.split_at(1);
        if rest.is_empty() {
            format!("{first}e{exponent}")
        } else {
            format!("{first}.{rest}e{exponent}")
        }
    };
    format!("{sign}{body}")
}

#[cfg(test)]
mod tests {
    use super::{custom_data, shortest};

    fn value(cd: &str, key: &str) -> Option<String> {
        let data = format!("{{\"cd\":{}}}", serde_json::to_string(cd).unwrap());
        custom_data(&data, key)
    }

    #[test]
    fn values_are_strings_or_compact_json() {
        for (cd, expected) in [
            (r#"{"k":"7","v":"abc"}"#, Some("7")),
            (r#"{"k":5}"#, Some("5")),
            (r#"{"k":1.5}"#, Some("1.5")),
            (r#"{"k":true}"#, Some("true")),
            (r#"{"k":null}"#, Some("null")),
            (r#"{"k":""}"#, Some("")),
            (r#"{"k":"1","k":"9"}"#, Some("9")),
            (r#"{"k":[1, 2, [3]]}"#, Some("[1,2,[3]]")),
            (r#"{"k":{"b":1,"a":"x"}}"#, Some(r#"{"a":"x","b":1}"#)),
            (r#"{"k":1e2}"#, Some("100.0")),
            (r#"{"k":-0}"#, Some("-0.0")),
            (r#"{"k":18446744073709551615}"#, Some("18446744073709551615")),
            (r#"{"k":-9223372036854775808}"#, Some("-9223372036854775808")),
            (r#"{"k":18446744073709551616}"#, Some("1.8446744073709552e19")),
            (r#"{"k":-9223372036854775809}"#, Some("-9.223372036854776e18")),
            (r#"{"k":8e-28}"#, Some("8.000000000000001e-28")),
            (r#"{"k":123456789012345678901}"#, Some("1.2345678901234567e20")),
            (r#"{"k":0e400}"#, Some("0.0")),
            (r#"{"k":1e-400}"#, Some("0.0")),
            (r#"{"k":-1e-9999999999}"#, Some("-0.0")),
            // P12 step 5's recorded results where F is not the nearest float.
            (r#"{"k":2.62e-21}"#, Some("2.6200000000000003e-21")),
            (r#"{"k":9007199254740993.0}"#, Some("9007199254740994.0")),
            (r#"{"k":9007199254740992.99999999999}"#, Some("9007199254740994.0")),
            (
                r#"{"k":1.00000000000000011102230246251565404236316680908203125}"#,
                Some("1.0000000000000002"),
            ),
            (r#"{"k":573e-309}"#, Some("5.7299999999999996e-307")),
            (r#"{"k":"\ud83d\ude00"}"#, Some("\u{1F600}")),
            (r#"{"k":1,"z":1.7976931348623158e308}"#, None),
            (r#"{"k":1,"z":1e9999999999}"#, None),
            (r#"{"K":1}"#, None),
            ("[1,2]", None),
            (r#"{"k":1,"z":1e400}"#, None),
            (r#"{"k":1,"z":"\ud800"}"#, None),
            (r#"{"k":1} x"#, None),
        ] {
            assert_eq!(value(cd, "k").as_deref(), expected, "{cd}");
        }
        assert_eq!(custom_data(r#"{"s":5}"#, "k"), None);
        assert_eq!(custom_data(r#"{"cd":5}"#, "k"), None);
    }

    #[test]
    fn nesting_beyond_127_levels_fails_the_whole_text() {
        let nested = |depth: usize| format!(r#"{{"k":1,"z":{}{}}}"#, "[".repeat(depth), "]".repeat(depth));
        assert_eq!(value(&nested(126), "k").as_deref(), Some("1"));
        assert_eq!(value(&nested(127), "k"), None);
    }

    #[test]
    fn shortest_forms_follow_the_exponent() {
        for (number, text) in [
            (1e15, "1000000000000000.0"),
            (1234567890123456.7, "1234567890123456.8"),
            (0.0001, "0.0001"),
            (1e-5, "0.00001"),
            (1e16, "1e16"),
            (1e-6, "1e-6"),
            (1.5e-6, "1.5e-6"),
            (5e-324, "5e-324"),
            (1.5e300, "1.5e300"),
            (2500.0, "2500.0"),
            (-0.0, "-0.0"),
        ] {
            assert_eq!(shortest(number), text, "{number}");
        }
    }
}
