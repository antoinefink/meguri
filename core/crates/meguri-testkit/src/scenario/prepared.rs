//! Typed prerequisites for prepared observations. These parsers register no adapters.
use serde::Deserialize;
use serde_json::{Map, Value};

use super::{FormatError, LogicalRef, RefKind};

fn invalid(why: &str) -> FormatError {
    FormatError::new(why)
}
fn object(value: &Value) -> Result<&Map<String, Value>, FormatError> {
    value.as_object().ok_or_else(|| invalid("expected a mapping"))
}
fn keys(map: &Map<String, Value>, required: &[&str], optional: &[&str]) -> Result<(), FormatError> {
    if required.iter().all(|k| map.contains_key(*k))
        && map
            .keys()
            .all(|k| required.contains(&k.as_str()) || optional.contains(&k.as_str()))
    {
        Ok(())
    } else {
        Err(invalid("missing or unknown argument"))
    }
}
fn read<T: serde::de::DeserializeOwned>(value: &Value) -> Result<T, FormatError> {
    T::deserialize(value.clone()).map_err(|e| FormatError::new(e.to_string()))
}
fn nonempty<T>(items: &[T]) -> Result<(), FormatError> {
    if items.is_empty() {
        Err(invalid("expected a non-empty list"))
    } else {
        Ok(())
    }
}
fn card(value: &Value) -> Result<LogicalRef, FormatError> {
    let r: LogicalRef = read(value)?;
    if r.kind != RefKind::Card {
        return Err(invalid("expected a card reference"));
    }
    Ok(r)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AddDefaultsRequest {
    pub review_card: Option<LogicalRef>,
}
impl AddDefaultsRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        keys(map, &[], &["review_card"])?;
        Ok(Self {
            review_card: map.get("review_card").map(card).transpose()?,
        })
    }
    /// The resolver checks assignment, retaining references to deleted cards (D7).
    pub fn resolve<T>(&self, assigned: impl FnOnce(&LogicalRef) -> Option<T>) -> Result<Option<T>, FormatError> {
        self.review_card
            .as_ref()
            .map(|r| assigned(r).ok_or_else(|| invalid("unknown_ref")))
            .transpose()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotetypeLastDeckRequest {
    Notetype(String),
    NotetypeId(i64),
}
impl NotetypeLastDeckRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        if map.len() != 1 {
            return Err(invalid("expected exactly one notetype or notetype_id"));
        }
        if let Some(v) = map.get("notetype") {
            let name: String = read(v)?;
            if name.is_empty() {
                return Err(invalid("expected a non-empty notetype name"));
            }
            return Ok(Self::Notetype(name));
        }
        if let Some(v) = map.get("notetype_id") {
            return Ok(Self::NotetypeId(read(v)?));
        }
        Err(invalid("expected notetype or notetype_id"))
    }
    /// The adapter resolves names and refuses ids present in the collection.
    pub fn resolve(
        &self,
        by_name: impl FnOnce(&str) -> Option<i64>,
        exists: impl FnOnce(i64) -> bool,
    ) -> Result<i64, FormatError> {
        match self {
            Self::Notetype(name) => by_name(name).ok_or_else(|| invalid("unknown_ref")),
            Self::NotetypeId(id) => {
                if exists(*id) {
                    Err(invalid("invalid_scenario: raw notetype_id exists"))
                } else {
                    Ok(*id)
                }
            }
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagCompleteCase {
    pub input: String,
    pub limit: u64,
}
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TagCompleteRequest {
    pub cases: Vec<TagCompleteCase>,
}
impl TagCompleteRequest {
    pub fn from_mapping(map: &Map<String, Value>) -> Result<Self, FormatError> {
        keys(map, &["cases"], &[])?;
        let cases = map["cases"].as_array().ok_or_else(|| invalid("expected cases list"))?;
        nonempty(cases)?;
        for case in cases {
            object(case)?;
        }
        let request: Self = read(&Value::Object(map.clone()))?;
        if request.cases.iter().any(|c| !(1..=4_294_967_296).contains(&c.limit)) {
            return Err(invalid("tag completion limit outside 1..2^32"));
        }
        Ok(request)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardStatsRequest {
    pub cards: Vec<LogicalRef>,
}
impl CardStatsRequest {
    pub fn from_list(items: &[Value]) -> Result<Self, FormatError> {
        nonempty(items)?;
        let cards = items.iter().map(card).collect::<Result<Vec<_>, _>>()?;
        if cards.iter().collect::<std::collections::BTreeSet<_>>().len() != cards.len() {
            return Err(invalid("card_stats needs distinct card references"));
        }
        Ok(Self { cards })
    }
    pub fn resolve<T>(&self, mut assigned: impl FnMut(&LogicalRef) -> Option<T>) -> Result<Vec<T>, FormatError> {
        self.cards
            .iter()
            .map(|r| assigned(r).ok_or_else(|| invalid("unknown_ref")))
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsScope {
    Collection,
    Deck(String),
    Search(String),
}
impl StatsScope {
    pub fn from_value(value: &Value) -> Result<Self, FormatError> {
        if value.as_str() == Some("collection") {
            return Ok(Self::Collection);
        }
        let map = object(value)?;
        if map.len() == 1 {
            if let Some(v) = map.get("deck") {
                let name: String = read(v)?;
                if !name.is_empty() {
                    return Ok(Self::Deck(name));
                }
            }
            if let Some(v) = map.get("search") {
                return Ok(Self::Search(read(v)?));
            }
        }
        Err(invalid("expected collection, {deck}, or {search} scope"))
    }
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsHistory {
    Year,
    All,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StatsPart {
    Reviews,
    TrueRetention,
    Hours,
    Buttons,
    FutureDue,
    CardCounts,
    Intervals,
    Stability,
    Difficulty,
    Retrievability,
    Added,
    Context,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatsRequest {
    pub scope: StatsScope,
    pub history: StatsHistory,
    pub parts: Vec<StatsPart>,
}
impl StatsRequest {
    pub fn from_list(items: &[Value]) -> Result<Vec<Self>, FormatError> {
        nonempty(items)?;
        items
            .iter()
            .map(|item| {
                let map = object(item)?;
                keys(map, &["scope", "history", "parts"], &[])?;
                let parts: Vec<StatsPart> = read(&map["parts"])?;
                nonempty(&parts)?;
                Ok(Self {
                    scope: StatsScope::from_value(&map["scope"])?,
                    history: read(&map["history"])?,
                    parts,
                })
            })
            .collect()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchJoiner {
    And,
    Or,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchWord {
    WholeCollection,
    CurrentDeck,
    AnyDeck,
    AnyTag,
    Untagged,
    AnyNotetype,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SearchState {
    New,
    Learn,
    Review,
    Due,
    Suspended,
    Buried,
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SearchFlag {
    Any,
    None,
    Color(u8),
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BuiltSearchItem {
    Word(SearchWord),
    Deck(String),
    Tag(String),
    Notetype(String),
    Template(u16),
    FieldPresent(String),
    Field { name: String, text: String },
    Literal(String),
    Flag(SearchFlag),
    State(SearchState),
    Added(u32),
    Edited(u32),
    Introduced(u32),
    Answered { days: u32, button: Option<u8> },
    Rescheduled(u32),
    DueOn(i32),
    DueWithin(i32),
    NoteIds(Vec<u64>),
    Dupe { notetype_id: u64, text: String },
    Text { text: String, field_text: bool },
    Group { joiner: SearchJoiner, items: Vec<Self> },
    Negated(Box<Self>),
}
fn whitespace(text: &str) -> bool {
    text.chars()
        .any(|c| (c.is_whitespace() && c != ' ') || ('\u{1c}'..='\u{1f}').contains(&c))
}
fn name(text: String, kind: &str) -> Result<String, FormatError> {
    if text.is_empty() || text == "_*" || whitespace(&text) {
        return Err(invalid("unrepresentable search item text"));
    }
    if kind == "deck"
        && (matches!(text.as_str(), "current" | "filtered")
            || text.split("::").any(|level| {
                level.is_empty()
                    || level.chars().any(|c| c.is_ascii_control())
                    || level.starts_with(|c: char| c.is_whitespace() || c == ':')
                    || level.ends_with(|c: char| c.is_whitespace() || c == ':')
                    || meguri_text::normalize::nfc(level).as_ref() != level
            }))
    {
        return Err(invalid("unrepresentable deck name"));
    }
    if kind == "notetype" && meguri_text::normalize::nfc(&text).as_ref() != text.as_str() {
        return Err(invalid("unrepresentable notetype name"));
    }
    if kind == "tag" && text == "none" {
        return Err(invalid("reserved tag name"));
    }
    if kind == "field"
        && [
            "deck",
            "note",
            "tag",
            "card",
            "flag",
            "resched",
            "prop",
            "added",
            "edited",
            "introduced",
            "rated",
            "is",
            "did",
            "mid",
            "nid",
            "cid",
            "re",
            "nc",
            "sc",
            "w",
            "dupe",
            "has-cd",
            "preset",
        ]
        .contains(&text.to_ascii_lowercase().as_str())
    {
        return Err(invalid("reserved field name"));
    }
    Ok(text)
}
fn days(value: &Value) -> Result<u32, FormatError> {
    let days: u32 = read(value)?;
    if days == 0 {
        Err(invalid("days must be positive"))
    } else {
        Ok(days)
    }
}
impl BuiltSearchItem {
    pub fn from_value(value: &Value) -> Result<Self, FormatError> {
        if value.is_string() {
            return read(value).map(Self::Word);
        }
        let map = object(value)?;
        if let Some(text) = map.get("text") {
            keys(map, &["text"], &["field_text"])?;
            return Ok(Self::Text {
                text: read(text)?,
                field_text: map.get("field_text").map(read).transpose()?.unwrap_or(false),
            });
        }
        if map.len() != 1 {
            return Err(invalid("a built item has exactly one kind"));
        }
        let (key, v) = map.iter().next().expect("one key");
        Ok(match key.as_str() {
            "deck" => Self::Deck(name(read(v)?, "deck")?),
            "tag" => Self::Tag(name(read(v)?, "tag")?),
            "notetype" => Self::Notetype(name(read(v)?, "notetype")?),
            "template" => {
                let n: u16 = read(v)?;
                if n == u16::MAX {
                    return Err(invalid("template ordinal exceeds 65534"));
                }
                Self::Template(n)
            }
            "field_present" => Self::FieldPresent(name(read(v)?, "field")?),
            "literal" => {
                let t: String = read(v)?;
                if t.is_empty() || whitespace(&t) {
                    return Err(invalid("invalid literal"));
                }
                Self::Literal(t)
            }
            "field" => {
                let m = object(v)?;
                keys(m, &["name", "text"], &[])?;
                let text: String = read(&m["text"])?;
                if whitespace(&text) {
                    return Err(invalid("invalid field text"));
                }
                Self::Field {
                    name: name(read(&m["name"])?, "field")?,
                    text,
                }
            }
            "flag" => Self::Flag(match v.as_str() {
                Some("any") => SearchFlag::Any,
                Some("none") => SearchFlag::None,
                _ => {
                    let n: u8 = read(v)?;
                    if !(1..=7).contains(&n) {
                        return Err(invalid("invalid flag"));
                    }
                    SearchFlag::Color(n)
                }
            }),
            "state" => Self::State(read(v)?),
            "added" => Self::Added(days(v)?),
            "edited" => Self::Edited(days(v)?),
            "introduced" => Self::Introduced(days(v)?),
            "answered" => {
                let m = object(v)?;
                keys(m, &["days"], &["button"])?;
                let button: Option<u8> = m.get("button").map(read).transpose()?;
                if button.is_some_and(|b| !(1..=4).contains(&b)) {
                    return Err(invalid("invalid answer button"));
                }
                Self::Answered {
                    days: days(&m["days"])?,
                    button,
                }
            }
            "rescheduled" => Self::Rescheduled(read(v)?),
            "due_on" => Self::DueOn(read(v)?),
            "due_within" => Self::DueWithin(read(v)?),
            "note_ids" => {
                let ids: Vec<u64> = read(v)?;
                nonempty(&ids)?;
                if ids.iter().any(|n| *n > 9_007_199_254_740_991) {
                    return Err(invalid("note id outside exact JSON range"));
                }
                Self::NoteIds(ids)
            }
            "dupe" => {
                let m = object(v)?;
                keys(m, &["notetype_id", "text"], &[])?;
                let id: u64 = read(&m["notetype_id"])?;
                let text: String = read(&m["text"])?;
                if id > 9_007_199_254_740_991 || whitespace(&text) {
                    return Err(invalid("invalid duplicates item"));
                }
                Self::Dupe { notetype_id: id, text }
            }
            "group" => {
                let m = object(v)?;
                keys(m, &["joiner", "items"], &[])?;
                Self::Group {
                    joiner: read(&m["joiner"])?,
                    items: Self::from_list(m["items"].as_array().ok_or_else(|| invalid("expected group items"))?)?,
                }
            }
            "negated" => Self::Negated(Box::new(Self::from_value(v)?)),
            _ => return Err(invalid("unknown built search item")),
        })
    }
    pub fn from_list(items: &[Value]) -> Result<Vec<Self>, FormatError> {
        nonempty(items)?;
        items.iter().map(Self::from_value).collect()
    }
}
pub struct SearchBuildRequest;
impl SearchBuildRequest {
    pub fn from_list(items: &[Value]) -> Result<Vec<BuiltSearchItem>, FormatError> {
        BuiltSearchItem::from_list(items)
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchJoinRequest {
    pub existing: String,
    pub item: BuiltSearchItem,
    pub joiner: SearchJoiner,
    pub negate: bool,
}
impl SearchJoinRequest {
    pub fn from_list(items: &[Value]) -> Result<Vec<Self>, FormatError> {
        nonempty(items)?;
        items
            .iter()
            .map(|v| {
                let m = object(v)?;
                keys(m, &["existing", "item", "joiner", "negate"], &[])?;
                Ok(Self {
                    existing: read(&m["existing"])?,
                    item: BuiltSearchItem::from_value(&m["item"])?,
                    joiner: read(&m["joiner"])?,
                    negate: read(&m["negate"])?,
                })
            })
            .collect()
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchReplaceRequest {
    pub existing: String,
    pub item: BuiltSearchItem,
}
impl SearchReplaceRequest {
    pub fn from_list(items: &[Value]) -> Result<Vec<Self>, FormatError> {
        nonempty(items)?;
        items
            .iter()
            .map(|v| {
                let m = object(v)?;
                keys(m, &["existing", "item"], &[])?;
                Ok(Self {
                    existing: read(&m["existing"])?,
                    item: BuiltSearchItem::from_value(&m["item"])?,
                })
            })
            .collect()
    }
}
