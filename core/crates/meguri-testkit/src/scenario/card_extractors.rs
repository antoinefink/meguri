//! Typed inputs for conformance/extractor-ops E1/E2. Keep SQL storage classes
//! (including arbitrary TEXT bytes) intact until binding to SQLite.
use super::FormatError;
use serde_json::{Map, Value};
use std::collections::BTreeSet;

#[derive(Clone, Debug)]
pub enum ExtractorValue {
    Null,
    Integer(i64),
    Real(f64),
    Text(Vec<u8>),
    Blob(Vec<u8>),
}

impl ExtractorValue {
    fn parse(value: &Value) -> Result<Self, FormatError> {
        let bad = || FormatError::new("card_extractors: invalid SQL value (E2)");
        match value {
            Value::Null => Ok(Self::Null),
            Value::String(s) => Ok(Self::Text(s.as_bytes().to_vec())),
            Value::Number(n) if n.is_f64() => n.as_f64().filter(|n| n.is_finite()).map(Self::Real).ok_or_else(bad),
            Value::Number(n) => n
                .as_i64()
                .filter(|n| (-9_007_199_254_740_992..=9_007_199_254_740_992).contains(n))
                .map(Self::Integer)
                .ok_or_else(bad),
            Value::Object(map) if map.len() == 1 => {
                if let Some(text) = map.get("int").and_then(Value::as_str) {
                    let digits = text.strip_prefix('-').unwrap_or(text);
                    if digits.is_empty() || digits.len() > 19 || !digits.bytes().all(|b| b.is_ascii_digit()) {
                        return Err(bad());
                    }
                    return text.parse().map(Self::Integer).map_err(|_| bad());
                }
                for (key, blob) in [("text_hex", false), ("blob_hex", true)] {
                    if let Some(hex) = map.get(key).and_then(Value::as_str) {
                        let bytes = hex.as_bytes();
                        if !bytes.len().is_multiple_of(2) || !bytes.iter().all(u8::is_ascii_hexdigit) {
                            return Err(bad());
                        }
                        let digit = |b: u8| {
                            if b.is_ascii_digit() {
                                b - b'0'
                            } else {
                                b.to_ascii_lowercase() - b'a' + 10
                            }
                        };
                        let decoded = bytes
                            .as_chunks::<2>()
                            .0
                            .iter()
                            .map(|p| digit(p[0]) * 16 + digit(p[1]))
                            .collect();
                        return Ok(if blob { Self::Blob(decoded) } else { Self::Text(decoded) });
                    }
                }
                Err(bad())
            }
            _ => Err(bad()),
        }
    }
}

#[derive(Clone, Debug)]
pub struct CardExtractorInput {
    pub id: String,
    /// data, due, ivl, today, now, in function argument order.
    pub arguments: [ExtractorValue; 5],
}

#[derive(Clone, Debug)]
pub struct CardExtractorsRequest {
    pub inputs: Vec<CardExtractorInput>,
}

impl CardExtractorsRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        let bad = || FormatError::new("card_extractors takes {inputs: [1..1000 uniquely labelled SQL inputs]} (E1)");
        if map.len() != 1 {
            return Err(bad());
        }
        let inputs = map
            .get("inputs")
            .and_then(Value::as_array)
            .filter(|v| (1..=1000).contains(&v.len()))
            .ok_or_else(bad)?;
        let mut ids = BTreeSet::new();
        let mut parsed = Vec::with_capacity(inputs.len());
        for input in inputs {
            let input = input.as_object().filter(|v| v.len() == 6).ok_or_else(bad)?;
            let id = input
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.is_empty())
                .ok_or_else(bad)?;
            if !ids.insert(id) {
                return Err(bad());
            }
            let value = |key| ExtractorValue::parse(input.get(key).ok_or_else(bad)?);
            parsed.push(CardExtractorInput {
                id: id.to_owned(),
                arguments: [
                    value("data")?,
                    value("due")?,
                    value("ivl")?,
                    value("today")?,
                    value("now")?,
                ],
            });
        }
        Ok(Self { inputs: parsed })
    }
}
