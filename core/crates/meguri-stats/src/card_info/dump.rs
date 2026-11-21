//! The debug dump (`stats/card-info` C8): the raw values, the card row, the
//! review-log rows with their raw values, the decks and the preset, as JSON
//! with two-space indent. It holds no name or text a user typed but custom
//! data and a filtered deck's search terms.

use meguri_model::{
    card_data::MemoryState,
    json_text::{push_f32, push_string},
    schema11::{deck_config_to_json, deck_to_json},
};
use meguri_storage::{Cell, Table};

use super::{CardInfo, HistoryEntry};

/// A JSON value whose scalars are kept as their text.
enum Json {
    Raw(String),
    Array(Vec<Json>),
    /// Keys as JSON string text, quotes included.
    Object(Vec<(String, Json)>),
}

fn key(name: &str) -> String {
    let mut out = String::new();
    push_string(&mut out, name);
    out
}

fn string(text: &str) -> Json {
    Json::Raw(key(text))
}

fn int(value: impl Into<i128>) -> Json {
    Json::Raw(value.into().to_string())
}

fn f32_value(value: f32) -> Json {
    let mut out = String::new();
    push_f32(&mut out, value);
    Json::Raw(out)
}

fn null() -> Json {
    Json::Raw("null".into())
}

fn optional<T>(value: Option<T>, write: impl FnOnce(T) -> Json) -> Json {
    value.map_or_else(null, write)
}

fn object(members: Vec<(&str, Json)>) -> Json {
    Json::Object(members.into_iter().map(|(name, value)| (key(name), value)).collect())
}

fn memory(state: Option<MemoryState>) -> Json {
    optional(state, |state| {
        object(vec![
            ("stability", f32_value(state.stability)),
            ("difficulty", f32_value(state.difficulty)),
        ])
    })
}

/// A stored cell as JSON: integers as integers, text as a string.
fn cell(value: &Cell) -> Json {
    match value {
        Cell::Null => null(),
        Cell::Integer(value) => int(*value),
        Cell::Real(value) if value.is_finite() => Json::Raw(format!("{value:?}")),
        Cell::Real(_) => null(),
        Cell::Text(bytes) | Cell::Blob(bytes) => string(&String::from_utf8_lossy(bytes)),
    }
}

fn row(table: Table, image: &[Cell]) -> Json {
    object(table.columns().iter().copied().zip(image.iter().map(cell)).collect())
}

/// Reads JSON text that Meguri's own writers made, keeping every scalar's
/// text and every key's spelling.
struct Reader<'a> {
    text: &'a str,
    at: usize,
    depth: usize,
}

/// The nesting the dump reads (`model/json-numbers`' 128 levels); deeper
/// text is written as a string, so neither walk recurses past it.
const MAX_DEPTH: usize = 128;

impl Reader<'_> {
    fn space(&mut self) {
        while self.text[self.at..].starts_with([' ', '\t', '\n', '\r']) {
            self.at += 1;
        }
    }

    fn string_end(&self) -> Option<usize> {
        let bytes = self.text.as_bytes();
        let mut index = self.at + 1;
        while index < bytes.len() {
            match bytes[index] {
                b'\\' => index += 2,
                b'"' => return Some(index + 1),
                _ => index += 1,
            }
        }
        None
    }

    fn value(&mut self) -> Option<Json> {
        self.space();
        let first = self.text[self.at..].chars().next()?;
        if matches!(first, '{' | '[') {
            if self.depth == MAX_DEPTH {
                return None;
            }
            self.depth += 1;
        }
        let value = match first {
            '{' => {
                self.at += 1;
                let mut members = Vec::new();
                self.space();
                if self.text[self.at..].starts_with('}') {
                    self.at += 1;
                } else {
                    loop {
                        self.space();
                        let end = self.string_end()?;
                        let name = self.text[self.at..end].to_owned();
                        self.at = end;
                        self.space();
                        self.text[self.at..].starts_with(':').then_some(())?;
                        self.at += 1;
                        members.push((name, self.value()?));
                        self.space();
                        let next = self.text[self.at..].chars().next()?;
                        self.at += 1;
                        match next {
                            ',' => continue,
                            '}' => break,
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
                Json::Object(members)
            }
            '[' => {
                self.at += 1;
                let mut items = Vec::new();
                self.space();
                if self.text[self.at..].starts_with(']') {
                    self.at += 1;
                } else {
                    loop {
                        items.push(self.value()?);
                        self.space();
                        let next = self.text[self.at..].chars().next()?;
                        self.at += 1;
                        match next {
                            ',' => continue,
                            ']' => break,
                            _ => return None,
                        }
                    }
                }
                self.depth -= 1;
                Json::Array(items)
            }
            '"' => {
                let end = self.string_end()?;
                let text = self.text[self.at..end].to_owned();
                self.at = end;
                Json::Raw(text)
            }
            _ => {
                let rest = &self.text[self.at..];
                let length = rest
                    .find([',', ']', '}', ' ', '\t', '\n', '\r'])
                    .unwrap_or(rest.len());
                if length == 0 {
                    return None;
                }
                self.at += length;
                Json::Raw(rest[..length].to_owned())
            }
        };
        Some(value)
    }
}

fn read(text: &str) -> Option<Json> {
    let mut reader = Reader { text, at: 0, depth: 0 };
    let value = reader.value()?;
    reader.space();
    (reader.at == text.len()).then_some(value)
}

/// A schema-11 object without the members named in `drop`.
fn schema11(text: &str, drop: &[&str]) -> Json {
    match read(text) {
        Some(Json::Object(members)) => Json::Object(
            members
                .into_iter()
                .filter(|(name, _)| !drop.iter().any(|dropped| *name == key(dropped)))
                .collect(),
        ),
        Some(other) => other,
        None => string(text),
    }
}

fn entry_info(entry: &HistoryEntry) -> Json {
    object(vec![
        ("time", int(entry.instant)),
        ("review_kind", int(entry.kind.stored())),
        ("button_chosen", int(entry.rating)),
        ("interval", int(entry.interval_secs)),
        ("last_interval", int(entry.last_interval_secs)),
        ("ease", int(entry.factor)),
        ("taken_secs", f32_value(entry.taken_secs)),
        ("memory_state", memory(entry.memory_state)),
    ])
}

fn pretty(out: &mut String, value: &Json, depth: usize) {
    let indent = |out: &mut String, depth: usize| out.extend(std::iter::repeat_n("  ", depth));
    match value {
        Json::Raw(text) => out.push_str(text),
        Json::Array(items) if items.is_empty() => out.push_str("[]"),
        Json::Object(members) if members.is_empty() => out.push_str("{}"),
        Json::Array(items) => {
            out.push_str("[\n");
            for (index, item) in items.iter().enumerate() {
                indent(out, depth + 1);
                pretty(out, item, depth + 1);
                out.push_str(if index + 1 < items.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push(']');
        }
        Json::Object(members) => {
            out.push_str("{\n");
            for (index, (name, member)) in members.iter().enumerate() {
                indent(out, depth + 1);
                out.push_str(name);
                out.push_str(": ");
                pretty(out, member, depth + 1);
                out.push_str(if index + 1 < members.len() { ",\n" } else { "\n" });
            }
            indent(out, depth);
            out.push('}');
        }
    }
}

pub(super) fn write(info: &CardInfo) -> String {
    let raw = &info.raw;
    let stats = object(vec![
        ("card_id", int(raw.card_id.0)),
        ("note_id", int(raw.note_id.0)),
        ("custom_data", string(&raw.custom_data)),
        ("added", int(raw.added)),
        ("first_review", optional(raw.first_review, int)),
        ("latest_review", optional(raw.latest_review, int)),
        ("due_date", optional(raw.due_date, int)),
        ("due_position", optional(raw.due_position, int)),
        ("interval", int(raw.interval)),
        ("ease", int(raw.ease)),
        ("reviews", int(raw.reviews)),
        ("lapses", int(raw.lapses)),
        ("average_secs", f32_value(raw.average_secs)),
        ("total_secs", f32_value(raw.total_secs)),
        ("desired_retention", optional(raw.desired_retention, f32_value)),
        ("fsrs_retrievability", optional(raw.fsrs_retrievability, f32_value)),
        ("memory_state", memory(raw.memory_state)),
        (
            "fsrs_params",
            Json::Array(raw.fsrs_params.iter().copied().map(f32_value).collect()),
        ),
        (
            "memory_state_source",
            optional(raw.memory_state_source, |source| string(source.as_str())),
        ),
    ]);
    let source = &info.source;
    // The history is newest first, the stored rows oldest first.
    let revlog = info
        .history
        .iter()
        .zip(source.revlog_images.iter().rev())
        .map(|(entry, image)| object(vec![("row", row(Table::Revlog, image)), ("info", entry_info(entry))]))
        .collect();
    let mut members = vec![
        ("stats", stats),
        ("card", row(Table::Cards, &source.card_image)),
        ("revlog", Json::Array(revlog)),
        ("deck", schema11(&deck_to_json(&source.deck), &["name", "desc", "usn"])),
    ];
    if let Some(home) = &source.home_deck {
        members.push(("homeDeck", schema11(&deck_to_json(home), &["name", "desc", "usn"])));
    }
    members.push(("preset", schema11(&deck_config_to_json(&source.preset), &["name"])));
    members.push((
        "rollover",
        match &source.rollover {
            None => null(),
            Some(bytes) => {
                let text = String::from_utf8_lossy(bytes);
                meguri_model::json_text::parse(&text)
                    .and_then(|_| read(&text))
                    .unwrap_or_else(|| string(&text))
            }
        },
    ));
    let mut out = String::new();
    pretty(&mut out, &object(members), 0);
    out
}
