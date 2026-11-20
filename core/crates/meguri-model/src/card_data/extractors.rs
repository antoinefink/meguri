//! Read-only card-data fields for search/card-extractors X3. These deliberately
//! differ from CardData's paired memory and lenient custom-data codec rules.
use serde_json::value::RawValue;

use super::KNOWN_KEYS;
use crate::json::{self, Kind};

/// Independently readable fields; invalid input reads as all absent.
#[derive(Clone, Copy, Debug, Default)]
pub struct Fields {
    pub position: Option<u32>,
    pub stability: Option<f32>,
    pub difficulty: Option<f32>,
    pub desired_retention: Option<f32>,
    pub decay: Option<f32>,
    pub last_review_secs: Option<i64>,
}

impl Fields {
    pub fn parse(text: &str) -> Self {
        Self::read(text).map(|(fields, _)| fields).unwrap_or_default()
    }

    /// X3's `cd`: the custom data text, when the whole text reads and holds
    /// it (`search/properties` P12 step 1).
    pub fn custom_data(text: &str) -> Option<String> {
        Self::read(text)?.1
    }

    fn read(text: &str) -> Option<(Self, Option<String>)> {
        // Raw capture checks JSON syntax iteratively, including control
        // characters, but permits surrogate escapes in skipped values.
        let raw: &RawValue = serde_json::from_str(text).ok()?;
        let mut values = [None; 7];
        match json::kind(raw) {
            Kind::Object => {
                for (key, value) in json::parse_members(raw.get())? {
                    if let Some(index) = KNOWN_KEYS.iter().position(|known| *known == key) {
                        if values[index].is_some() || !json::in_range(value) || !json::within_depth(value, 126) {
                            return None;
                        }
                        values[index] = Some(value);
                    }
                }
            }
            Kind::Array => {
                if !json::in_range(raw) || !json::within_depth(raw, 127) {
                    return None;
                }
                let items = json::array(raw)?;
                if items.len() > values.len() {
                    return None;
                }
                for (slot, value) in values.iter_mut().zip(items) {
                    *slot = Some(value);
                }
            }
            _ => return None,
        }
        let custom_data = match values[6] {
            Some(value) => Some(json::string(value)?),
            None => None,
        };
        // JSON -0 has float syntax for the integer readers of X3, without
        // changing the ordinary model's integer conversion behavior.
        fn integer(value: Option<&RawValue>) -> Option<&RawValue> {
            value.filter(|v| v.get() != "-0")
        }
        Some((
            Self {
                position: integer(values[0]).and_then(json::u32_value),
                stability: values[1].and_then(json::f32_value),
                difficulty: values[2].and_then(json::f32_value),
                desired_retention: values[3].and_then(json::f32_value),
                decay: values[4].and_then(json::f32_value),
                last_review_secs: integer(values[5]).and_then(json::i64_value),
            },
            custom_data,
        ))
    }
}
