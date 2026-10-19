//! K1/K2 request grammar, shared by validation and the real adapters.
use super::{FormatError, LogicalRef, RefKind};
use serde_json::{Map, Value};
use std::collections::BTreeSet;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum TypeAnswerFunction {
    Compare,
    CompareNc,
}
impl TypeAnswerFunction {
    pub fn name(self) -> &'static str {
        match self {
            Self::Compare => "compare",
            Self::CompareNc => "compare_nc",
        }
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TypeAnswerRequest {
    pub fns: Vec<TypeAnswerFunction>,
    pub inputs: Vec<(String, String)>,
}
impl TypeAnswerRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        let bad = || {
            FormatError::new("type_answer takes distinct nonempty fns [compare, compare_nc] and string pairs inputs")
        };
        if map.len() != 2 || !map.contains_key("fns") || !map.contains_key("inputs") {
            return Err(bad());
        }
        let names = map["fns"].as_array().filter(|v| !v.is_empty()).ok_or_else(bad)?;
        let mut seen = BTreeSet::new();
        let mut fns = Vec::new();
        for name in names {
            let function = match name.as_str() {
                Some("compare") => TypeAnswerFunction::Compare,
                Some("compare_nc") => TypeAnswerFunction::CompareNc,
                _ => return Err(bad()),
            };
            if !seen.insert(function) {
                return Err(bad());
            }
            fns.push(function);
        }
        let mut inputs = Vec::new();
        for value in map["inputs"].as_array().ok_or_else(bad)? {
            let pair = value.as_array().filter(|p| p.len() == 2).ok_or_else(bad)?;
            inputs.push((
                pair[0].as_str().ok_or_else(bad)?.to_owned(),
                pair[1].as_str().ok_or_else(bad)?.to_owned(),
            ));
        }
        Ok(Self { fns, inputs })
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ReviewHtmlCase {
    pub card: LogicalRef,
    pub typed: String,
}
impl ReviewHtmlCase {
    pub fn from_list(items: &[Value]) -> Result<Vec<Self>, FormatError> {
        let bad = || FormatError::new("review_html takes nonempty cases {card: #cN, typed?: string}");
        if items.is_empty() {
            return Err(bad());
        }
        items
            .iter()
            .map(|value| {
                let map = value.as_object().ok_or_else(bad)?;
                if map.keys().any(|k| k != "card" && k != "typed") {
                    return Err(bad());
                }
                let card: LogicalRef = map.get("card").and_then(Value::as_str).ok_or_else(bad)?.parse()?;
                if card.kind != RefKind::Card {
                    return Err(bad());
                }
                let typed = match map.get("typed") {
                    None => String::new(),
                    Some(v) => v.as_str().ok_or_else(bad)?.to_owned(),
                };
                Ok(Self { card, typed })
            })
            .collect()
    }
}
