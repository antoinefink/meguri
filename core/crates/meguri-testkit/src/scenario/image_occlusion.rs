//! conformance/image-occlusion-ops J1: requests for pure IO observations.
use super::FormatError;
use serde_json::{Map, Value};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageOcclusionRequest {
    Parse(Vec<String>),
    Fields(Vec<Vec<Option<u32>>>),
}

impl ImageOcclusionRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        let bad = || {
            FormatError::new(
                "observation image_occlusion: takes {fns: [parse|fields], inputs}; parse inputs must be NFC text without controls other than TAB/LF, fields inputs nonempty lists of plain u32 integers or null",
            )
        };
        if map.len() != 2 {
            return Err(bad());
        }
        let fns = map.get("fns").and_then(Value::as_array).ok_or_else(bad)?;
        let [Value::String(function)] = fns.as_slice() else {
            return Err(bad());
        };
        let inputs = map.get("inputs").and_then(Value::as_array).ok_or_else(bad)?;
        match function.as_str() {
            "parse" => {
                let texts = inputs
                    .iter()
                    .map(|v| {
                        let text = v.as_str()?;
                        if meguri_text::normalize::nfc(text) != text
                            || text.chars().any(|c| c < ' ' && c != '\t' && c != '\n')
                        {
                            return None;
                        }
                        Some(text.to_owned())
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(bad)?;
                Ok(Self::Parse(texts))
            }
            "fields" => {
                let fields = inputs
                    .iter()
                    .map(|v| {
                        let tags = v.as_array().filter(|a| !a.is_empty())?;
                        tags.iter()
                            .map(|tag| {
                                if tag.is_null() {
                                    Some(None)
                                } else {
                                    Some(Some(u32::try_from(tag.as_u64()?).ok()?))
                                }
                            })
                            .collect::<Option<Vec<_>>>()
                    })
                    .collect::<Option<Vec<_>>>()
                    .ok_or_else(bad)?;
                Ok(Self::Fields(fields))
            }
            _ => Err(bad()),
        }
    }
}
