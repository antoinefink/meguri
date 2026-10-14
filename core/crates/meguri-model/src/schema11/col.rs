//! The legacy JSON columns of `col` (`specs/model/schema11-json.md` S12):
//! `conf`, `models`, `decks`, `dconf` and `tags` at schema 11. At schema 18
//! all five are empty strings (R1456).

use serde_json::value::RawValue;

use super::Result;
use super::deck::deck_from_raw;
use super::deck_config::deck_config_from_raw;
use super::notetype::notetype_from_raw;
use super::{deck_config_to_json, deck_to_json, notetype_to_json};
use crate::config::ConfigEntry;
use crate::deck::Deck;
use crate::deck_config::DeckConfig;
use crate::error::{ModelError, Schema11Item};
use crate::ids::DeckConfigId;
use crate::json::{self, Member, ObjectWriter};
use crate::notetype::Notetype;
use crate::usn::Usn;

fn refuse(column: &'static str, key: &str) -> ModelError {
    ModelError::invalid_json(Schema11Item::Column(column), key)
}

/// A column's members, repeated keys reduced (N9); anything but a JSON object
/// refuses the column.
fn members<'a>(column: &'static str, text: &'a str) -> Result<Vec<Member<'a>>> {
    json::parse_value(text)
        .and_then(json::object)
        .ok_or_else(|| refuse(column, ""))
}

/// An id key: an optional `-` and decimal digits, within `i64`.
fn id_key(column: &'static str, key: &str) -> Result<i64> {
    let digits = key.strip_prefix('-').unwrap_or(key);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err(refuse(column, key));
    }
    key.parse().map_err(|_| refuse(column, key))
}

/// A member's refusal, named by its column and key.
fn in_member(column: &'static str, key: &str, error: ModelError) -> ModelError {
    match error {
        ModelError::InvalidJson { key: inner, .. } if !inner.is_empty() => refuse(column, &format!("{key}.{inner}")),
        _ => refuse(column, key),
    }
}

/// `conf` output: config keys in ascending byte order with their stored
/// JSON values. A value that is not JSON refuses the column.
pub fn conf_to_json(entries: &[ConfigEntry]) -> Result<String> {
    let mut sorted: Vec<&ConfigEntry> = entries.iter().collect();
    sorted.sort_by(|a, b| a.key.as_bytes().cmp(b.key.as_bytes()));
    let mut out = ObjectWriter::new();
    for entry in sorted {
        let value = std::str::from_utf8(&entry.value)
            .ok()
            .and_then(json::parse_value)
            .ok_or_else(|| refuse("conf", &entry.key))?;
        out.raw(&entry.key, value.get());
    }
    Ok(out.finish())
}

/// `conf` input: each key with its value as exact JSON text (N7), in input
/// order.
pub fn conf_from_json(text: &str) -> Result<Vec<(String, Vec<u8>)>> {
    Ok(members("conf", text)?
        .into_iter()
        .map(|(key, raw)| (key, raw.get().as_bytes().to_vec()))
        .collect())
}

/// `models` output: note type objects by id, ids ascending.
pub fn notetypes_to_json(notetypes: &[Notetype]) -> String {
    let mut sorted: Vec<&Notetype> = notetypes.iter().collect();
    sorted.sort_by_key(|notetype| notetype.id);
    let mut out = ObjectWriter::new();
    for notetype in sorted {
        out.raw(&notetype.id.to_string(), &notetype_to_json(notetype));
    }
    out.finish()
}

/// `models` input: the id key is checked, the object's own `id` is the note
/// type's id.
pub fn notetypes_from_json(text: &str) -> Result<Vec<Notetype>> {
    from_column("models", text, |_, raw| notetype_from_raw(raw))
}

/// `decks` output: deck objects by id, ids ascending.
pub fn decks_to_json(decks: &[Deck]) -> String {
    let mut sorted: Vec<&Deck> = decks.iter().collect();
    sorted.sort_by_key(|deck| deck.id);
    let mut out = ObjectWriter::new();
    for deck in sorted {
        out.raw(&deck.id.to_string(), &deck_to_json(deck));
    }
    out.finish()
}

/// `decks` input: the id key is checked, the object's own `id` is the deck's
/// id.
pub fn decks_from_json(text: &str) -> Result<Vec<Deck>> {
    from_column("decks", text, |_, raw| deck_from_raw(raw))
}

/// `dconf` output: preset objects by id, ids ascending.
pub fn deck_configs_to_json(presets: &[DeckConfig]) -> String {
    let mut sorted: Vec<&DeckConfig> = presets.iter().collect();
    sorted.sort_by_key(|preset| preset.id);
    let mut out = ObjectWriter::new();
    for preset in sorted {
        out.raw(&preset.id.to_string(), &deck_config_to_json(preset));
    }
    out.finish()
}

/// `dconf` input: the member key, not the object's own `id`, is the preset's
/// id.
pub fn deck_configs_from_json(text: &str) -> Result<Vec<DeckConfig>> {
    from_column("dconf", text, |id, raw| {
        deck_config_from_raw(raw).map(|preset| DeckConfig {
            id: DeckConfigId(id),
            ..preset
        })
    })
}

fn from_column<T>(column: &'static str, text: &str, convert: impl Fn(i64, &RawValue) -> Result<T>) -> Result<Vec<T>> {
    members(column, text)?
        .into_iter()
        .map(|(key, raw)| {
            let id = id_key(column, &key)?;
            convert(id, raw).map_err(|error| in_member(column, &key, error))
        })
        .collect()
}

/// `tags` output: each tag with its `usn`, in the given order (the order of
/// the `tags` table).
pub fn tags_to_json(tags: &[(String, Usn)]) -> String {
    let mut out = ObjectWriter::new();
    for (name, usn) in tags {
        out.int(name, usn.0);
    }
    out.finish()
}

/// `tags` input: each tag with its `usn` (a strict `i32`), in input order.
pub fn tags_from_json(text: &str) -> Result<Vec<(String, Usn)>> {
    members("tags", text)?
        .into_iter()
        .map(|(name, raw)| match json::i32_value(raw) {
            Some(usn) => Ok((name, Usn(usn))),
            None => Err(refuse("tags", &name)),
        })
        .collect()
}
