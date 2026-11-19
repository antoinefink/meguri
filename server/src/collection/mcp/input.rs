// SPDX-License-Identifier: MIT
use super::*;
use serde::de::{MapAccess, Visitor};
use std::fmt;

/// Exact field names; reject duplicate map keys before they are overwritten.
#[derive(Debug, Default, Serialize)]
#[serde(transparent)]
pub struct Fields(pub BTreeMap<String, String>);
impl<'de> Deserialize<'de> for Fields {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        struct FieldVisitor;
        impl<'de> Visitor<'de> for FieldVisitor {
            type Value = Fields;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { f.write_str("unique named fields") }
            fn visit_map<M: MapAccess<'de>>(self, mut map: M) -> std::result::Result<Fields, M::Error> {
                let mut out = BTreeMap::new();
                let mut bytes = 0;
                while let Some((k,v)) = map.next_entry::<String,String>()? {
                    bytes += k.len() + v.len();
                    if out.len() >= MAX_FIELDS || bytes > MAX_REQUEST_BYTES || out.insert(k,v).is_some() {
                        return Err(serde::de::Error::custom("invalid fields"));
                    }
                }
                Ok(Fields(out))
            }
        }
        d.deserialize_map(FieldVisitor)
    }
}
fn non_null<'de, D: serde::Deserializer<'de>, T: Deserialize<'de>>(d: D) -> std::result::Result<Option<T>, D::Error> {
    T::deserialize(d).map(Some)
}
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SearchInput {
    #[serde(default, deserialize_with="non_null")] pub query: Option<String>,
    #[serde(default, deserialize_with="non_null")] pub deck_id: Option<String>,
    #[serde(default, deserialize_with="non_null")] pub tag: Option<String>,
    #[serde(default, deserialize_with="non_null")] pub note_id: Option<String>,
    #[serde(default, deserialize_with="non_null")] pub limit: Option<usize>,
    #[serde(default, deserialize_with="non_null")] pub cursor: Option<String>,
}
#[derive(Debug, Default, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all="snake_case")]
pub enum CatalogKind { #[default] All, Deck, NoteType }
#[derive(Debug, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CatalogInput {
    #[serde(default, deserialize_with="non_null")] pub kind: Option<CatalogKind>,
    #[serde(default, deserialize_with="non_null")] pub limit: Option<usize>,
    #[serde(default, deserialize_with="non_null")] pub cursor: Option<String>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CreateInput {
    pub deck_id: String, pub note_type_id: String, pub fields: Fields,
    #[serde(default, deserialize_with="non_null")] pub tags: Option<Vec<String>>,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EditInput {
    pub note_id: String, pub revision: String,
    #[serde(default, deserialize_with="non_null")] pub fields: Option<Fields>,
    #[serde(default, deserialize_with="non_null")] pub add_tags: Option<Vec<String>>,
    #[serde(default, deserialize_with="non_null")] pub remove_tags: Option<Vec<String>>,
}
pub(super) fn id(text: &str) -> Result<i64> {
    if text.is_empty() || text.len() > 19 || !text.bytes().all(|b| b.is_ascii_digit()) { return Err(invalid()); }
    text.parse::<i64>().ok().filter(|v| *v > 0).ok_or_else(invalid)
}
pub(super) fn limit(value: Option<usize>) -> Result<usize> {
    match value.unwrap_or(20) { n @ 1..=50 => Ok(n), _ => Err(invalid()) }
}
pub(super) fn bounded_input(value: &impl Serialize) -> Result<()> {
    if size(value)? > MAX_REQUEST_BYTES { return Err(invalid()); }
    Ok(())
}
pub(super) fn tag_inputs(tags: &[String]) -> Result<()> {
    if tags.len() > MAX_TAGS || tags.iter().any(|t| t.len() > 1024) { return Err(invalid()); }
    Ok(())
}
