//! The `config` table (`specs/storage/config-table.md`): rows, values read
//! the way Anki reads them (C3), the compact form Meguri writes (C5), and
//! the "same value" test that keeps a stored value's bytes when a write
//! would not change it (C4).
//!
//! Values are parsed here from the stored bytes, not through serde_json:
//! numbers keep their kind (an integer stays an integer, C6) and floats are
//! read exactly (std's parser), and nesting deeper than [`MAX_DEPTH`] reads
//! as invalid JSON instead of growing the stack.

use std::collections::BTreeMap;
use std::fmt::Write as _;

use meguri_model::config::ConfigEntry;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;

use super::{Cell, Record, RowCells, RowError, RowImage, Table};

/// The prefix of Meguri's own synced keys (C8, R1468), followed by a
/// lower-camel-case name.
pub const MEGURI_PREFIX: &str = "meguri";

/// Nesting deeper than this reads as invalid JSON.
pub const MAX_DEPTH: usize = 128;

impl Record for ConfigEntry {
    const TABLE: Table = Table::Config;

    /// The value is stored as a BLOB (C2).
    fn to_cells(&self) -> RowImage {
        vec![
            Cell::text(self.key.clone()),
            Cell::Integer(i64::from(self.usn.0)),
            Cell::Integer(self.mtime.0),
            Cell::Blob(self.value.clone()),
        ]
    }

    /// `val` of any storage class reads as its bytes (R9's exception to
    /// R1); [`stored_value`] tells a BLOB from the rest.
    fn from_cells(cells: &[Cell]) -> Result<ConfigEntry, RowError> {
        let row = RowCells {
            table: Table::Config,
            cells,
        };
        let value = match &cells[3] {
            Cell::Blob(bytes) | Cell::Text(bytes) => bytes.clone(),
            Cell::Integer(v) => v.to_string().into_bytes(),
            Cell::Real(v) => v.to_string().into_bytes(),
            Cell::Null => Vec::new(),
        };
        Ok(ConfigEntry {
            key: row.text(0)?,
            usn: Usn(row.int(1)?),
            mtime: EpochSecs(row.int(2)?),
            value,
        })
    }
}

/// A stored `val` as C3 reads it: `None` unless it is a BLOB holding valid
/// JSON (Anki treats a TEXT value as missing).
pub fn stored_value(val: &Cell) -> Option<ConfigValue> {
    match val {
        Cell::Blob(bytes) => ConfigValue::parse(bytes),
        _ => None,
    }
}

/// C4: whether the stored `val` holds the same value as `new`: a BLOB of
/// valid JSON with no repeated member name whose compact form equals
/// `new`'s.
pub fn same_value(val: &Cell, new: &ConfigValue) -> bool {
    stored_value(val).is_some_and(|stored| !stored.has_repeated_names() && stored.compact() == new.compact())
}

/// One JSON value (RFC 8259) with C3's number kinds.
#[derive(Clone, Debug, PartialEq)]
pub enum ConfigValue {
    Null,
    Bool(bool),
    /// From -9223372036854775808 to 18446744073709551615.
    Integer(i128),
    /// Finite.
    Float(f64),
    String(String),
    Array(Vec<ConfigValue>),
    /// Members in stored order, repeated names included.
    Object(Vec<(String, ConfigValue)>),
}

const INTEGER_MIN: i128 = i64::MIN as i128;
const INTEGER_MAX: i128 = u64::MAX as i128;

impl From<i64> for ConfigValue {
    fn from(value: i64) -> ConfigValue {
        ConfigValue::Integer(i128::from(value))
    }
}

impl From<u64> for ConfigValue {
    fn from(value: u64) -> ConfigValue {
        ConfigValue::Integer(i128::from(value))
    }
}

impl From<bool> for ConfigValue {
    fn from(value: bool) -> ConfigValue {
        ConfigValue::Bool(value)
    }
}

impl From<&str> for ConfigValue {
    fn from(value: &str) -> ConfigValue {
        ConfigValue::String(value.to_owned())
    }
}

impl ConfigValue {
    /// A float; `None` when it is not finite (JSON has no such number).
    pub fn float(value: f64) -> Option<ConfigValue> {
        value.is_finite().then_some(ConfigValue::Float(value))
    }

    /// Reads JSON text (C3): `None` when it is not UTF-8, not valid JSON,
    /// nested deeper than [`MAX_DEPTH`], or holds a number beyond the 64-bit
    /// float range.
    pub fn parse(bytes: &[u8]) -> Option<ConfigValue> {
        let text = std::str::from_utf8(bytes).ok()?;
        let mut parser = Parser {
            text: text.as_bytes(),
            at: 0,
        };
        parser.whitespace();
        let value = parser.value(0)?;
        parser.whitespace();
        (parser.at == parser.text.len()).then_some(value)
    }

    /// C5: the bytes Meguri (and Anki) write for this value.
    pub fn compact(&self) -> String {
        let mut out = String::new();
        self.write(&mut out);
        out
    }

    /// C5 with a requested root-member prefix; all nested objects stay sorted.
    pub fn compact_ordered(&self, first: &[&str]) -> String {
        if !matches!(self, Self::Object(_)) {
            return self.compact();
        }
        let mut members = self.members();
        let mut ordered = Vec::new();
        for name in first {
            if let Some(index) = members.iter().position(|(key, _)| key == name) {
                ordered.push(members.remove(index));
            }
        }
        ordered.extend(members);
        let mut out = String::from("{");
        for (index, (name, value)) in ordered.into_iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write_string(name, &mut out);
            out.push(':');
            value.write(&mut out);
        }
        out.push('}');
        out
    }

    fn write(&self, out: &mut String) {
        match self {
            ConfigValue::Null => out.push_str("null"),
            ConfigValue::Bool(true) => out.push_str("true"),
            ConfigValue::Bool(false) => out.push_str("false"),
            ConfigValue::Integer(v) => {
                // Writing to a String cannot fail.
                let _ = write!(out, "{v}");
            }
            ConfigValue::Float(v) => out.push_str(ryu::Buffer::new().format_finite(*v)),
            ConfigValue::String(text) => write_string(text, out),
            ConfigValue::Array(items) => {
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    item.write(out);
                }
                out.push(']');
            }
            ConfigValue::Object(_) => {
                out.push('{');
                for (i, (name, value)) in self.members().into_iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    write_string(name, out);
                    out.push(':');
                    value.write(out);
                }
                out.push('}');
            }
        }
    }

    /// An object's members, each name once with its last value, sorted by
    /// name in code point order; empty for other values.
    pub fn members(&self) -> Vec<(&str, &ConfigValue)> {
        let ConfigValue::Object(members) = self else {
            return Vec::new();
        };
        let last: BTreeMap<&str, &ConfigValue> = members.iter().map(|(n, v)| (n.as_str(), v)).collect();
        last.into_iter().collect()
    }

    /// Whether some object in the value repeats a member name.
    pub fn has_repeated_names(&self) -> bool {
        match self {
            ConfigValue::Array(items) => items.iter().any(ConfigValue::has_repeated_names),
            ConfigValue::Object(members) => {
                let mut names: Vec<&str> = members.iter().map(|(n, _)| n.as_str()).collect();
                names.sort_unstable();
                names.windows(2).any(|w| w[0] == w[1]) || members.iter().any(|(_, v)| v.has_repeated_names())
            }
            _ => false,
        }
    }

    /// An object member by name, the last occurrence counting (C3).
    pub fn get(&self, name: &str) -> Option<&ConfigValue> {
        match self {
            ConfigValue::Object(members) => members.iter().rev().find(|(n, _)| n == name).map(|(_, v)| v),
            _ => None,
        }
    }

    /// C6: the object with `name` set to `value`, every other member kept
    /// with its value and kind; a value that is not an object counts as an
    /// empty one.
    pub fn with_member(&self, name: &str, value: ConfigValue) -> ConfigValue {
        let mut members = match self {
            ConfigValue::Object(members) => members.clone(),
            _ => Vec::new(),
        };
        members.retain(|(n, _)| n != name);
        members.push((name.to_owned(), value));
        ConfigValue::Object(members)
    }

    pub fn as_i64(&self) -> Option<i64> {
        match self {
            ConfigValue::Integer(v) => i64::try_from(*v).ok(),
            _ => None,
        }
    }

    pub fn as_u64(&self) -> Option<u64> {
        match self {
            ConfigValue::Integer(v) => u64::try_from(*v).ok(),
            _ => None,
        }
    }

    /// A number of either kind.
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ConfigValue::Integer(v) => Some(*v as f64),
            ConfigValue::Float(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            ConfigValue::Bool(v) => Some(*v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            ConfigValue::String(v) => Some(v),
            _ => None,
        }
    }
}

/// C5's string form.
fn write_string(text: &str, out: &mut String) {
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
            c if u32::from(c) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", u32::from(c));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

struct Parser<'a> {
    text: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn peek(&self) -> Option<u8> {
        self.text.get(self.at).copied()
    }

    fn whitespace(&mut self) {
        while matches!(self.peek(), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> Option<()> {
        (self.peek()? == byte).then(|| self.at += 1)
    }

    fn literal(&mut self, word: &str, value: ConfigValue) -> Option<ConfigValue> {
        let end = self.at.checked_add(word.len())?;
        (self.text.get(self.at..end)? == word.as_bytes()).then(|| {
            self.at = end;
            value
        })
    }

    fn value(&mut self, depth: usize) -> Option<ConfigValue> {
        match self.peek()? {
            b'{' => {
                if depth >= MAX_DEPTH {
                    return None;
                }
                self.at += 1;
                let mut members = Vec::new();
                self.whitespace();
                if self.eat(b'}').is_some() {
                    return Some(ConfigValue::Object(members));
                }
                loop {
                    self.whitespace();
                    if self.peek()? != b'"' {
                        return None;
                    }
                    let name = self.string()?;
                    self.whitespace();
                    self.eat(b':')?;
                    self.whitespace();
                    let value = self.value(depth + 1)?;
                    members.push((name, value));
                    self.whitespace();
                    match self.peek()? {
                        b',' => self.at += 1,
                        b'}' => {
                            self.at += 1;
                            return Some(ConfigValue::Object(members));
                        }
                        _ => return None,
                    }
                }
            }
            b'[' => {
                if depth >= MAX_DEPTH {
                    return None;
                }
                self.at += 1;
                let mut items = Vec::new();
                self.whitespace();
                if self.eat(b']').is_some() {
                    return Some(ConfigValue::Array(items));
                }
                loop {
                    self.whitespace();
                    items.push(self.value(depth + 1)?);
                    self.whitespace();
                    match self.peek()? {
                        b',' => self.at += 1,
                        b']' => {
                            self.at += 1;
                            return Some(ConfigValue::Array(items));
                        }
                        _ => return None,
                    }
                }
            }
            b'"' => self.string().map(ConfigValue::String),
            b't' => self.literal("true", ConfigValue::Bool(true)),
            b'f' => self.literal("false", ConfigValue::Bool(false)),
            b'n' => self.literal("null", ConfigValue::Null),
            b'-' | b'0'..=b'9' => self.number(),
            _ => None,
        }
    }

    fn digits(&mut self) -> usize {
        let start = self.at;
        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.at += 1;
        }
        self.at - start
    }

    /// C3's reading of a number token.
    fn number(&mut self) -> Option<ConfigValue> {
        let start = self.at;
        let _ = self.eat(b'-');
        match self.peek()? {
            b'0' => self.at += 1,
            b'1'..=b'9' => {
                self.digits();
            }
            _ => return None,
        }
        let mut integral = true;
        if self.eat(b'.').is_some() {
            integral = false;
            if self.digits() == 0 {
                return None;
            }
        }
        if matches!(self.peek(), Some(b'e' | b'E')) {
            self.at += 1;
            integral = false;
            if matches!(self.peek(), Some(b'+' | b'-')) {
                self.at += 1;
            }
            if self.digits() == 0 {
                return None;
            }
        }
        // The token is ASCII.
        let token = std::str::from_utf8(&self.text[start..self.at]).ok()?;
        if integral
            && token != "-0"
            && let Ok(v) = token.parse::<i128>()
            && (INTEGER_MIN..=INTEGER_MAX).contains(&v)
        {
            return Some(ConfigValue::Integer(v));
        }
        ConfigValue::float(token.parse::<f64>().ok()?)
    }

    fn hex4(&mut self) -> Option<u32> {
        let end = self.at.checked_add(4)?;
        let digits = std::str::from_utf8(self.text.get(self.at..end)?).ok()?;
        if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at = end;
        u32::from_str_radix(digits, 16).ok()
    }

    fn string(&mut self) -> Option<String> {
        self.eat(b'"')?;
        let mut out = Vec::new();
        loop {
            let byte = self.peek()?;
            self.at += 1;
            match byte {
                b'"' => return String::from_utf8(out).ok(),
                b'\\' => {
                    let escape = self.peek()?;
                    self.at += 1;
                    let c = match escape {
                        b'"' => '"',
                        b'\\' => '\\',
                        b'/' => '/',
                        b'b' => '\u{8}',
                        b'f' => '\u{c}',
                        b'n' => '\n',
                        b'r' => '\r',
                        b't' => '\t',
                        b'u' => {
                            let unit = self.hex4()?;
                            let code = match unit {
                                0xD800..=0xDBFF => {
                                    self.eat(b'\\')?;
                                    self.eat(b'u')?;
                                    let low = self.hex4()?;
                                    if !(0xDC00..=0xDFFF).contains(&low) {
                                        return None;
                                    }
                                    0x10000 + ((unit - 0xD800) << 10) + (low - 0xDC00)
                                }
                                0xDC00..=0xDFFF => return None,
                                unit => unit,
                            };
                            char::from_u32(code)?
                        }
                        _ => return None,
                    };
                    let mut buffer = [0; 4];
                    out.extend_from_slice(c.encode_utf8(&mut buffer).as_bytes());
                }
                0x00..=0x1f => return None,
                other => out.push(other),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn compact(text: &str) -> Option<String> {
        ConfigValue::parse(text.as_bytes()).map(|v| v.compact())
    }

    #[test]
    fn deep_nesting_reads_as_invalid() {
        let ok = format!("{}{}", "[".repeat(MAX_DEPTH), "]".repeat(MAX_DEPTH));
        assert!(ConfigValue::parse(ok.as_bytes()).is_some());
        let deep = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert_eq!(ConfigValue::parse(deep.as_bytes()), None);
    }

    #[test]
    fn invalid_text_is_refused() {
        for bad in [
            "",
            " ",
            "01",
            "-",
            "1.",
            ".5",
            "1e",
            "+1",
            "[1,]",
            "{\"a\"}",
            "{\"a\":1,}",
            "\"\t\"",
            "\"\\x\"",
            "\"\\ud800\"",
            "\"\\udc00\"",
            "\"\\ud800\\u0041\"",
            "tru",
            "nul",
            "1 2",
            "1e400",
            "\u{feff}1",
        ] {
            assert_eq!(compact(bad), None, "{bad:?}");
        }
        assert_eq!(ConfigValue::parse(&[b'"', 0xff, b'"']), None);
    }

    #[test]
    fn surrogate_pairs_and_whitespace_read() {
        assert_eq!(compact(" \"\\ud83d\\ude00\" ").as_deref(), Some("\"\u{1f600}\""));
        assert_eq!(compact("\r\n[ 1 ,\t2 ]\n").as_deref(), Some("[1,2]"));
    }
}
