//! The Custom Data row's text (`stats/card-info` C5.5): the `cd` JSON read
//! with the readings of a JavaScript JSON parser, then written as
//! `<key>=<value>` parts. Strings are UTF-16 code units throughout, as in a
//! JavaScript page; an unpaired surrogate is written as U+FFFD (rule 5).

/// The nesting Meguri reads; deeper text is written unchanged (rule 1).
const MAX_DEPTH: usize = 128;

#[derive(Debug)]
enum Value {
    Null,
    Bool(bool),
    Number(f64),
    String(Vec<u16>),
    Array(Vec<Value>),
    /// Members in JavaScript's own-key order: array-index keys first,
    /// ascending, then the others by first appearance (rule 2).
    Object(Vec<(Vec<u16>, Value)>),
}

struct Parser<'a> {
    text: &'a str,
    bytes: &'a [u8],
    at: usize,
}

impl Parser<'_> {
    fn skip_space(&mut self) {
        while matches!(self.bytes.get(self.at), Some(b' ' | b'\t' | b'\n' | b'\r')) {
            self.at += 1;
        }
    }

    fn eat(&mut self, byte: u8) -> bool {
        if self.bytes.get(self.at) == Some(&byte) {
            self.at += 1;
            true
        } else {
            false
        }
    }

    fn literal(&mut self, word: &str) -> Option<()> {
        if self.bytes[self.at..].starts_with(word.as_bytes()) {
            self.at += word.len();
            Some(())
        } else {
            None
        }
    }

    fn value(&mut self, depth: usize) -> Option<Value> {
        self.skip_space();
        let value = match *self.bytes.get(self.at)? {
            b'{' => {
                if depth == MAX_DEPTH {
                    return None;
                }
                self.at += 1;
                self.object(depth + 1)?
            }
            b'[' => {
                if depth == MAX_DEPTH {
                    return None;
                }
                self.at += 1;
                self.array(depth + 1)?
            }
            b'"' => {
                self.at += 1;
                Value::String(self.string()?)
            }
            b't' => self.literal("true").map(|()| Value::Bool(true))?,
            b'f' => self.literal("false").map(|()| Value::Bool(false))?,
            b'n' => self.literal("null").map(|()| Value::Null)?,
            _ => Value::Number(self.number()?),
        };
        self.skip_space();
        Some(value)
    }

    fn object(&mut self, depth: usize) -> Option<Value> {
        let mut members: Vec<(Vec<u16>, Value)> = Vec::new();
        self.skip_space();
        if !self.eat(b'}') {
            loop {
                self.skip_space();
                if !self.eat(b'"') {
                    return None;
                }
                let key = self.string()?;
                self.skip_space();
                if !self.eat(b':') {
                    return None;
                }
                let value = self.value(depth)?;
                match members.iter_mut().find(|(existing, _)| *existing == key) {
                    Some(member) => member.1 = value,
                    None => members.push((key, value)),
                }
                if self.eat(b'}') {
                    break;
                }
                if !self.eat(b',') {
                    return None;
                }
            }
        }
        let (mut indexes, others): (Vec<_>, Vec<_>) = members.into_iter().partition(|(key, _)| array_index(key).is_some());
        indexes.sort_by_key(|(key, _)| array_index(key));
        indexes.extend(others);
        Some(Value::Object(indexes))
    }

    fn array(&mut self, depth: usize) -> Option<Value> {
        let mut items = Vec::new();
        self.skip_space();
        if self.eat(b']') {
            return Some(Value::Array(items));
        }
        loop {
            items.push(self.value(depth)?);
            if self.eat(b']') {
                return Some(Value::Array(items));
            }
            if !self.eat(b',') {
                return None;
            }
        }
    }

    fn hex4(&mut self) -> Option<u16> {
        let digits = self.bytes.get(self.at..self.at + 4)?;
        let text = std::str::from_utf8(digits).ok()?;
        if !text.bytes().all(|b| b.is_ascii_hexdigit()) {
            return None;
        }
        self.at += 4;
        u16::from_str_radix(text, 16).ok()
    }

    /// The rest of a string after its opening quote, as UTF-16 code units;
    /// escapes may give unpaired surrogates (rule 1).
    fn string(&mut self) -> Option<Vec<u16>> {
        let mut units = Vec::new();
        loop {
            let byte = *self.bytes.get(self.at)?;
            match byte {
                b'"' => {
                    self.at += 1;
                    return Some(units);
                }
                b'\\' => {
                    self.at += 1;
                    let escape = *self.bytes.get(self.at)?;
                    self.at += 1;
                    let unit = match escape {
                        b'"' => u16::from(b'"'),
                        b'\\' => u16::from(b'\\'),
                        b'/' => u16::from(b'/'),
                        b'b' => 0x08,
                        b'f' => 0x0c,
                        b'n' => u16::from(b'\n'),
                        b'r' => u16::from(b'\r'),
                        b't' => u16::from(b'\t'),
                        b'u' => self.hex4()?,
                        _ => return None,
                    };
                    units.push(unit);
                }
                0x00..=0x1f => return None,
                _ => {
                    // `at` is always on a character boundary of the source.
                    let character = self.text[self.at..].chars().next()?;
                    self.at += character.len_utf8();
                    let mut buffer = [0u16; 2];
                    units.extend_from_slice(character.encode_utf16(&mut buffer));
                }
            }
        }
    }

    /// RFC 8259's number grammar; the value is the nearest binary64, an
    /// overflow is ±Infinity and an underflow ±0 (rule 1).
    fn number(&mut self) -> Option<f64> {
        let start = self.at;
        self.eat(b'-');
        let digits = |parser: &mut Self| {
            let from = parser.at;
            while parser.bytes.get(parser.at).is_some_and(u8::is_ascii_digit) {
                parser.at += 1;
            }
            parser.at - from
        };
        if self.eat(b'0') {
            if self.bytes.get(self.at).is_some_and(u8::is_ascii_digit) {
                return None;
            }
        } else if digits(self) == 0 {
            return None;
        }
        if self.eat(b'.') && digits(self) == 0 {
            return None;
        }
        if self.eat(b'e') || self.eat(b'E') {
            if !self.eat(b'+') {
                self.eat(b'-');
            }
            if digits(self) == 0 {
                return None;
            }
        }
        std::str::from_utf8(&self.bytes[start..self.at]).ok()?.parse().ok()
    }
}

/// A canonical array index, 0 to 4,294,967,294 (rule 2).
fn array_index(key: &[u16]) -> Option<u32> {
    if key.is_empty() || key.len() > 10 || !key.iter().all(|unit| (u16::from(b'0')..=u16::from(b'9')).contains(unit)) {
        return None;
    }
    if key.len() > 1 && key[0] == u16::from(b'0') {
        return None;
    }
    let value: u64 = key.iter().fold(0, |n, unit| n * 10 + u64::from(*unit - u16::from(b'0')));
    (value <= 4_294_967_294).then_some(value as u32)
}

fn parse(text: &str) -> Option<Value> {
    let mut parser = Parser {
        text,
        bytes: text.as_bytes(),
        at: 0,
    };
    let value = parser.value(0)?;
    (parser.at == parser.bytes.len()).then_some(value)
}

/// UTF-16 code units as text, every unpaired surrogate as U+FFFD (rule 5).
fn units_text(units: &[u16]) -> String {
    char::decode_utf16(units.iter().copied())
        .map(|unit| unit.unwrap_or(char::REPLACEMENT_CHARACTER))
        .collect()
}

/// Rule 4: a value as JavaScript's string conversion writes it.
fn value_text(value: &Value, nested: bool) -> String {
    match value {
        Value::Null if nested => String::new(),
        Value::Null => "null".into(),
        Value::Bool(flag) => flag.to_string(),
        Value::Number(number) => super::js_number(*number),
        Value::String(units) => units_text(units),
        Value::Array(items) => items
            .iter()
            .map(|item| value_text(item, true))
            .collect::<Vec<_>>()
            .join(","),
        Value::Object(_) => "[object Object]".into(),
    }
}

/// The Custom Data row's value for a `cd` text that is not `""`.
pub fn custom_data_text(cd: &str) -> String {
    let part = |key: String, value: String| format!("{key}={value}");
    match parse(cd) {
        None | Some(Value::Null) => cd.to_owned(),
        Some(Value::Object(members)) => members
            .iter()
            .map(|(key, value)| part(units_text(key), value_text(value, false)))
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::Array(items)) => items
            .iter()
            .enumerate()
            .map(|(index, value)| part(index.to_string(), value_text(value, false)))
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::String(units)) => units
            .iter()
            .enumerate()
            .map(|(index, unit)| part(index.to_string(), units_text(std::slice::from_ref(unit))))
            .collect::<Vec<_>>()
            .join(" "),
        Some(Value::Number(_) | Value::Bool(_)) => String::new(),
    }
}
