//! `applyChanges` (specs/sync/wire-format.md W6; R1523, R1464): note types,
//! decks, presets and tags as schema-11 JSON (`model/schema11-json`, with
//! their `other` members), and the config table and `crt` from the newer
//! side. Which objects are pending, and who applies `conf` and `crt`, is
//! U107's.

use meguri_model::ModelError;
use meguri_model::deck::Deck;
use meguri_model::deck_config::DeckConfig;
use meguri_model::notetype::Notetype;
use meguri_model::schema11::{
    deck_config_from_json, deck_config_to_json, deck_from_json, deck_to_json, notetype_from_json, notetype_to_json,
};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::StoredConfig;
use serde_json::value::RawValue;

use super::read::{self, At, DecodeError, Kind};
use super::write;
use crate::error::{DbCheckOrigin, ErrorKind, JobError};
use crate::transport::Method;

/// What the client sends (W6.1). Every object goes out with the server's
/// `usn` from this sync's `meta` in place of its stored one (W6.2).
#[derive(Debug, Clone, Copy)]
pub struct OutgoingChanges<'a> {
    pub notetypes: &'a [Notetype],
    pub decks: &'a [Deck],
    pub deck_configs: &'a [DeckConfig],
    /// Tag names as stored in the `tags` table.
    pub tags: &'a [String],
    /// `conf`: every row of the config table, only when the local collection
    /// is the newer one (`lnewer` true in `start`); `None` otherwise.
    pub config: Option<&'a [StoredConfig]>,
    /// `crt`: `col.crt`, sent with `config`.
    pub created: Option<EpochSecs>,
}

/// The `applyChanges` request: `{"changes":<changes object>}`. A config row
/// whose `val` is not a BLOB holding valid JSON cannot be sent: the sync
/// fails before the request with `db_check_required`, origin local (W6.4,
/// errors.md E7).
pub fn encode_apply_changes(changes: &OutgoingChanges<'_>, server_usn: Usn) -> Result<Vec<u8>, JobError> {
    let mut out = String::from("{\"changes\":{\"models\":[");
    for (index, notetype) in changes.notetypes.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let mut sent = notetype.clone();
        sent.usn = server_usn;
        out.push_str(&notetype_to_json(&sent));
    }
    out.push_str("],\"decks\":[[");
    for (index, deck) in changes.decks.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let mut sent = deck.clone();
        sent.usn = server_usn;
        out.push_str(&deck_to_json(&sent));
    }
    out.push_str("],[");
    for (index, preset) in changes.deck_configs.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        let mut sent = preset.clone();
        sent.usn = server_usn;
        out.push_str(&deck_config_to_json(&sent));
    }
    out.push_str("]],\"tags\":[");
    for (index, tag) in changes.tags.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        write::string(&mut out, tag);
    }
    out.push(']');
    if let Some(config) = changes.config {
        out.push_str(",\"conf\":{");
        for (index, row) in config.iter().enumerate() {
            if index > 0 {
                out.push(',');
            }
            write::string(&mut out, &row.entry.key);
            out.push(':');
            out.push_str(config_text(row)?);
        }
        out.push('}');
    }
    if let Some(created) = changes.created {
        out.push_str(",\"crt\":");
        write::int(&mut out, created.0);
    }
    out.push_str("}}");
    Ok(out.into_bytes())
}

/// W6.4: the row's `val` as stored, which must be a BLOB holding valid JSON.
fn config_text(row: &StoredConfig) -> Result<&str, JobError> {
    let text = std::str::from_utf8(&row.entry.value)
        .ok()
        .filter(|text| row.is_blob && meguri_model::json_text::parse(text).is_some());
    text.ok_or_else(|| {
        JobError::new(
            ErrorKind::DbCheckRequired {
                origin: DbCheckOrigin::Local,
            },
            None,
            format!(
                "config key {:?} cannot be sent: its value is not a BLOB holding valid JSON",
                row.entry.key
            ),
        )
    })
}

/// What a changes object carries (W6.1), decoded.
#[derive(Debug, Clone)]
pub struct IncomingChanges {
    pub notetypes: Vec<Notetype>,
    pub decks: Vec<Deck>,
    pub deck_configs: Vec<DeckConfig>,
    pub tags: Vec<String>,
    /// `conf`: each key with its value's JSON text as received, a repeated key
    /// keeping its last value; `None` when not sent.
    pub config: Option<Vec<(String, String)>>,
    /// `crt`; `None` when not sent.
    pub created: Option<EpochSecs>,
}

/// The `applyChanges` answer: the server's changes object. A refused
/// schema-11 object makes the whole body a decode failure (W6.1).
pub fn decode_changes(body: &[u8]) -> Result<IncomingChanges, JobError> {
    decode(body).map_err(|err| JobError::bad_response(Method::ApplyChanges, err.to_string()))
}

fn decode(body: &[u8]) -> read::Result<IncomingChanges> {
    let at = At::Root;
    let root = read::object(read::body(body)?, at)?;

    let models_at = at.member("models");
    let mut notetypes = Vec::new();
    for (index, raw) in read::array(root.required("models", at)?, models_at)?
        .into_iter()
        .enumerate()
    {
        notetypes.push(notetype_from_json(raw.get()).map_err(|err| refused(models_at.index(index), &err))?);
    }

    let decks_at = at.member("decks");
    let (deck_list, preset_list, object_form) = decks_and_presets(root.required("decks", at)?, decks_at)?;
    let (decks_list_at, presets_list_at) = if object_form {
        (decks_at.member("decks"), decks_at.member("config"))
    } else {
        (decks_at.index(0), decks_at.index(1))
    };
    let mut decks = Vec::with_capacity(deck_list.len());
    for (index, raw) in deck_list.into_iter().enumerate() {
        decks.push(deck_from_json(raw.get()).map_err(|err| refused(decks_list_at.index(index), &err))?);
    }
    let mut deck_configs = Vec::with_capacity(preset_list.len());
    for (index, raw) in preset_list.into_iter().enumerate() {
        deck_configs.push(deck_config_from_json(raw.get()).map_err(|err| refused(presets_list_at.index(index), &err))?);
    }

    let tags_at = at.member("tags");
    let tags = read::array(root.required("tags", at)?, tags_at)?
        .into_iter()
        .enumerate()
        .map(|(index, raw)| read::string(raw, tags_at.index(index)))
        .collect::<read::Result<Vec<String>>>()?;

    let config = match root.get("conf", at)? {
        Some(raw) if !read::is_null(raw) => Some(
            read::object(raw, at.member("conf"))?
                .last_wins()
                .into_iter()
                .map(|(key, value)| (key, value.get().to_owned()))
                .collect(),
        ),
        _ => None,
    };
    let created = match root.get("crt", at)? {
        Some(raw) if !read::is_null(raw) => Some(EpochSecs(read::strict::<i64>(raw, at.member("crt"))?)),
        _ => None,
    };
    Ok(IncomingChanges {
        notetypes,
        decks,
        deck_configs,
        tags,
        config,
        created,
    })
}

/// W6.3: `[[decks…],[presets…]]`, or the object form
/// `{"decks":[…],"config":[…]}` that Meguri accepts and never writes (then
/// the third value is true).
type DeckLists<'a> = (Vec<&'a RawValue>, Vec<&'a RawValue>, bool);

fn decks_and_presets<'a>(raw: &'a RawValue, at: At<'_>) -> read::Result<DeckLists<'a>> {
    if read::kind(raw) == Kind::Object {
        let object = read::object(raw, at)?;
        let decks = read::array(object.required("decks", at)?, at.member("decks"))?;
        let presets = read::array(object.required("config", at)?, at.member("config"))?;
        return Ok((decks, presets, true));
    }
    let pair = read::tuple(raw, 2, 2, at)?;
    let decks = read::array(pair[0], at.index(0))?;
    let presets = read::array(pair[1], at.index(1))?;
    Ok((decks, presets, false))
}

/// A schema-11 object the model refused (S1), at its place in the body.
fn refused(at: At<'_>, error: &ModelError) -> DecodeError {
    match error {
        ModelError::InvalidJson { key, .. } if !key.is_empty() => at.member(key).fail("is refused (schema-11 S1)"),
        _ => at.fail("is refused (schema-11 S1)"),
    }
}
