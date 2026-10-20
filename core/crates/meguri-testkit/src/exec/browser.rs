//! The browser area's adapters (`specs/conformance/browser-config-ops.md`
//! B1, B2): `browser_config_set` runs meguri-engine's whole-value write of
//! one browser config key (`browser/config-keys` K10.1) in a write scope of
//! the session's collection, as one operation with no undo entry; the
//! `browser_config` observation reports the 23 keys as Anki's own reads give
//! them, from meguri-search's accessor (U83).

use meguri_engine::browser_config;
use meguri_search::SearchMode;
use meguri_search::config::{BrowserConfig, Key, Shape, default_columns};
use meguri_storage::ConfigValue;
use serde_json::{Map, Number, Value};

use super::storage::config_value;
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{ObservationRequest, Step};

/// Registers `browser_config_set` and `browser_config`.
pub(super) fn register(registry: &mut Registry) {
    registry.op("browser_config_set", browser_config_set);
    registry.observation("browser_config", browser_config);
    registry.observation("card_excerpts", card_excerpts);
}

fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn collection<'s>(session: &'s Session<'_>) -> Result<&'s super::storage::SessionCollection, ExecError> {
    session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open"))
}

/// B1: the key's whole value, as one local operation (C4: nothing written
/// when the stored value is the same value).
fn browser_config_set(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::BrowserConfigSet(args) = step else {
        return Err(ExecError::new(
            "internal_error",
            "browser_config_set adapter called for another op",
        ));
    };
    let key = Key::from_name(&args.key)
        .filter(|key| key.writable())
        .ok_or_else(|| invalid(format!("{:?} is not one of the 22 browser config keys", args.key)))?;
    let value = config_value(&args.value)?;
    if !key.shape().accepts(&value) {
        return Err(invalid(format!("{key} takes no value {}", args.value)));
    }
    let collection = session.engine_collection()?;
    browser_config::set_value(&collection, key, &value).map_err(core)?;
    Ok(OpResult::Done)
}

/// B2's value for one key: a boolean or text key's typed read (K2.1,
/// K2.2); for the column, sort, `savedFilters` and `flagLabels` keys the
/// stored value as C3 reads it, or the default the browser passes when it
/// is unreadable.
fn reported(config: &BrowserConfig, key: Key) -> Value {
    let raw_default = match key {
        Key::ActiveCols => Some(columns_value(SearchMode::Cards)),
        Key::ActiveNoteCols => Some(columns_value(SearchMode::Notes)),
        Key::SortType | Key::NoteSortType => Some(Value::Null),
        Key::SortBackwards | Key::BrowserNoteSortBackwards => Some(Value::Bool(false)),
        Key::SavedFilters | Key::FlagLabels => Some(Value::Object(Map::new())),
        _ => None,
    };
    if let Some(default) = raw_default {
        return config.stored(key).map_or(default, json_value);
    }
    match key.shape() {
        Shape::Boolean(_) => config.boolean(key).map_or(Value::Null, Value::Bool),
        _ => config.text(key).map_or(Value::Null, Value::String),
    }
}

fn columns_value(mode: SearchMode) -> Value {
    Value::Array(
        default_columns(mode)
            .iter()
            .map(|column| Value::from(column.key()))
            .collect(),
    )
}

/// A config value as an observed value: numbers keep their kind
/// (observations.md O6), an object's repeated names keep the last.
fn json_value(value: &ConfigValue) -> Value {
    match value {
        ConfigValue::Null => Value::Null,
        ConfigValue::Bool(b) => Value::Bool(*b),
        ConfigValue::Integer(v) => match (i64::try_from(*v), u64::try_from(*v)) {
            (Ok(i), _) => Value::from(i),
            (_, Ok(u)) => Value::from(u),
            // C3 reads no integer outside the two ranges.
            _ => Value::Null,
        },
        ConfigValue::Float(f) => Number::from_f64(*f).map_or(Value::Null, Value::Number),
        ConfigValue::String(text) => Value::String(text.clone()),
        ConfigValue::Array(items) => Value::Array(items.iter().map(json_value).collect()),
        ConfigValue::Object(_) => Value::Object(
            value
                .members()
                .into_iter()
                .map(|(name, member)| (name.to_owned(), json_value(member)))
                .collect(),
        ),
    }
}

/// B2: the 23 keys; reading writes nothing (R1138).
fn browser_config(session: &mut Session<'_>, _request: &ObservationRequest) -> Result<Value, ExecError> {
    let config = BrowserConfig::read(&*collection(session)?.reader()?).map_err(core)?;
    Ok(Value::Object(
        Key::ALL
            .into_iter()
            .map(|key| (key.name().to_owned(), reported(&config, key)))
            .collect(),
    ))
}

/// E4: the product entry point, once for the entire reference list.
fn card_excerpts(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use crate::scenario::ObservationArgs;
    use meguri_engine::browse::{self, CardExcerpt, Question};
    let ObservationArgs::Refs(refs) = &request.args else {
        return Err(invalid("card_excerpts needs a card list"));
    };
    let ids = refs
        .iter()
        .map(|reference| {
            session
                .cards
                .get(reference.index())
                .copied()
                .map(meguri_model::ids::CardId)
                .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let collection = session.engine_collection()?;
    let entries = browse::card_excerpts(&collection, &ids)
        .map_err(core)?
        .into_iter()
        .map(|entry| match entry {
            CardExcerpt::Found {
                question: Question::Text(question),
                deck,
                ..
            } => Ok(serde_json::json!({"question":question,"deck":deck})),
            CardExcerpt::Missing { .. } => Ok(serde_json::json!({"error":"missing"})),
            CardExcerpt::Damaged { .. } => Ok(serde_json::json!({"error":"damaged"})),
            CardExcerpt::Found { question, .. } => Err(ExecError::new(
                "unsupported_observation",
                format!("card_excerpts has a non-text question outcome: {question:?}"),
            )),
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(Value::Array(entries))
}
