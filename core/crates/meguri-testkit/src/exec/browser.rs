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
    registry.observation("browser_rows", browser_rows);
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

/// B3: the rows of the references in the mode `browserTableShowNotesMode`
/// says, with the scenario clock's now and today.
fn browser_rows(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use crate::scenario::{LogicalRef, ObservationArgs, RefKind};
    use meguri_engine::browse::{self, RowRequest, RowTime};
    use meguri_search::config::{Column, ColumnKey};
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("browser_rows takes a mapping"));
    };
    let columns = map
        .get("columns")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("browser_rows needs columns"))?
        .iter()
        .map(|key| {
            key.as_str()
                .and_then(Column::from_key)
                .ok_or_else(|| invalid(format!("{key} is not one of the 19 columns")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let refs = map
        .get("rows")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("browser_rows needs rows"))?
        .iter()
        .map(|reference| {
            reference
                .as_str()
                .and_then(|text| text.parse::<LogicalRef>().ok())
                .ok_or_else(|| invalid(format!("{reference} is not a logical reference")))
        })
        .collect::<Result<Vec<_>, _>>()?;
    // B3: non-empty lists of distinct items.
    let distinct = |items: &[String]| {
        let mut sorted = items.to_vec();
        sorted.sort();
        sorted.dedup();
        !items.is_empty() && sorted.len() == items.len()
    };
    if !distinct(&columns.iter().map(|c| c.key().to_owned()).collect::<Vec<_>>())
        || !distinct(&refs.iter().map(ToString::to_string).collect::<Vec<_>>())
    {
        return Err(invalid("browser_rows takes non-empty lists of distinct columns and rows"));
    }
    // Rebind the session's handles first: after a hosted sync the stored
    // reader may name the previous generation.
    let engine = session.engine_collection()?;
    let notes_mode = BrowserConfig::read(&*collection(session)?.reader()?)
        .map_err(core)?
        .notes_mode();
    let (mode, kind, named) = if notes_mode {
        (SearchMode::Notes, RefKind::Note, &session.notes)
    } else {
        (SearchMode::Cards, RefKind::Card, &session.cards)
    };
    let ids = refs
        .iter()
        .map(|reference| {
            if reference.kind != kind {
                return Err(invalid(format!("{reference} is not a row of the Browse mode")));
            }
            named
                .get(reference.index())
                .copied()
                .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let time = RowTime {
        now: session.clock.now_secs(),
        today: super::sched::day_without_writes(session)?.today,
    };
    let keys: Vec<ColumnKey> = columns.iter().copied().map(ColumnKey::Column).collect();
    let rows = browse::rows(
        &engine,
        &RowRequest {
            mode,
            ids: &ids,
            columns: &keys,
            time,
        },
    )
    .map_err(core)?;
    let clock = &*session.clock;
    rows.into_iter()
        .map(|row| match row {
            Ok(row) => {
                let cells = columns
                    .iter()
                    .zip(&row.cells)
                    .map(|(column, cell)| Ok((column.key().to_owned(), cell_value(cell, clock)?)))
                    .collect::<Result<Map<_, _>, ExecError>>()?;
                Ok(serde_json::json!({"tint": tint_name(row.tint), "cells": cells}))
            }
            Err(browse::RowFailure::Deleted) => Ok(serde_json::json!({"error": "deleted"})),
            Err(browse::RowFailure::Damaged) => Ok(serde_json::json!({"error": "damaged"})),
            Err(failure @ browse::RowFailure::Unreadable { .. }) => {
                Err(ExecError::new("core_error", format!("unreadable row: {failure:?}")))
            }
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// B3's tint names.
fn tint_name(tint: meguri_engine::browse::Tint) -> String {
    use meguri_engine::browse::Tint;
    match tint {
        Tint::None => "none".to_owned(),
        Tint::Flag(flag) => format!("flag_{flag}"),
        Tint::Marked => "marked".to_owned(),
        Tint::Suspended => "suspended".to_owned(),
        Tint::Buried => "buried".to_owned(),
    }
}

/// B4: a typed cell in its observed form; dates in the scenario's zone.
fn cell_value(cell: &meguri_engine::browse::Cell, clock: &super::clock::ScenarioClock) -> Result<Value, ExecError> {
    use meguri_engine::browse::{Answer, Cell, Percent, Question};
    use meguri_search::browser::cells::{span_amount_text, span_parts};
    use serde_json::json;
    let wrap = |mut value: Value, wrapped: bool| {
        if wrapped {
            value["wrapped"] = Value::Bool(true);
        }
        value
    };
    Ok(match cell {
        Cell::Text(text) | Cell::Question(Question::Text(text)) | Cell::Answer(Answer::Text(text)) => {
            json!({"text": text})
        }
        Cell::Count(count) => json!({"count": count}),
        Cell::DeckCount(count) => json!({"text": format!("({count})")}),
        Cell::Percent(Percent::Integer(value)) => json!({"percent": value}),
        Cell::Percent(Percent::Real(value)) => {
            // C12: the exact value rounded to an integer, a half to even.
            let rounded = f64::from(*value).round_ties_even();
            if !rounded.is_finite() || rounded.abs() > 9.0e15 {
                return Err(ExecError::new(
                    "unsupported_observation",
                    format!("percent {value} has no B4 form"),
                ));
            }
            json!({"percent": rounded as i64})
        }
        Cell::Position(position) => json!({"position": position}),
        Cell::Instant { secs, wrapped } => wrap(json!({"date": local(*secs, clock).0}), *wrapped),
        Cell::DateTime(secs) => {
            let (date, time) = local(*secs, clock);
            json!({"datetime": format!("{date} {time}")})
        }
        Cell::NewPosition { position, wrapped } => wrap(json!({"new_position": position}), *wrapped),
        Cell::Filtered { wrapped } => wrap(json!({"filtered": true}), *wrapped),
        Cell::NewMarker => json!({"marker": "new"}),
        Cell::LearningMarker => json!({"marker": "learning"}),
        Cell::TimeSpan(seconds) => {
            let (unit, amount) = span_parts(*seconds);
            json!({"span": {"unit": unit.name(), "amount": span_amount_text(amount)}})
        }
        Cell::Empty => Value::Null,
        Cell::Question(other) => {
            return Err(ExecError::new(
                "unsupported_observation",
                format!("a Question outcome with no B4 form: {other:?}"),
            ));
        }
        Cell::Answer(other) => {
            return Err(ExecError::new(
                "unsupported_observation",
                format!("an Answer outcome with no B4 form: {other:?}"),
            ));
        }
        // B3 takes only the 19 columns of C2: no field column reaches here.
        Cell::NoField | Cell::Media { .. } => {
            return Err(ExecError::new(
                "unsupported_observation",
                format!("a field column cell with no B4 form: {cell:?}"),
            ));
        }
    })
}

/// C12: the local date `YYYY-MM-DD` and time `HH:MM` of an instant, in the
/// zone's offset at that instant (UTC where the zone database has none);
/// four-digit years zero-padded, others signed; `invalid date` outside
/// −262143-01-01 to +262142-12-31.
fn local(secs: i64, clock: &super::clock::ScenarioClock) -> (String, String) {
    use meguri_model::clock::Clock;
    const INVALID: &str = "invalid date";
    let offset = secs
        .checked_mul(1000)
        .map_or(0, |ms| i64::from(clock.utc_offset_at(ms).seconds()));
    let Some(local) = secs.checked_add(offset) else {
        return (INVALID.to_owned(), INVALID.to_owned());
    };
    let (days, seconds) = (local.div_euclid(86_400), local.rem_euclid(86_400));
    // Civil date from days since 1970-01-01, proleptic Gregorian.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    if !(-262_143..=262_142).contains(&year) {
        return (INVALID.to_owned(), INVALID.to_owned());
    }
    let year = if (0..=9999).contains(&year) {
        format!("{year:04}")
    } else if year > 0 {
        format!("+{year:04}")
    } else {
        format!("-{:04}", -year)
    };
    (
        format!("{year}-{month:02}-{day:02}"),
        format!("{:02}:{:02}", seconds / 3600, seconds % 3600 / 60),
    )
}

#[cfg(test)]
mod tests {
    use super::super::clock::ScenarioClock;
    use super::local;

    // browser/columns C12: four-digit years zero-padded, others signed, and
    // `invalid date` outside -262143-01-01 to +262142-12-31.
    #[test]
    fn local_dates_write_years_and_invalid_instants_as_c12_says() {
        let utc = ScenarioClock::new("UTC", "2030-01-15T10:00:00").unwrap();
        assert_eq!(local(1_894_701_600, &utc), ("2030-01-15".into(), "10:00".into()));
        assert_eq!(local(-60, &utc), ("1969-12-31".into(), "23:59".into()));
        let day = 86_400i64;
        // 1,894,701,600 + 3,000,000 days: the year 10243.
        assert!(local(1_894_701_600 + 3_000_000 * day, &utc).0.starts_with("+10243-"));
        // 0000-01-01 and the day before it.
        let year_zero = -62_167_219_200;
        assert_eq!(local(year_zero, &utc).0, "0000-01-01");
        assert_eq!(local(year_zero - day, &utc).0, "-0001-12-31");
        let invalid = ("invalid date".to_owned(), "invalid date".to_owned());
        assert_eq!(local(1_894_701_600 + 100_000_000 * day, &utc), invalid);
        assert_eq!(local(i64::MAX, &utc), invalid);
        // The zone's offset at the instant: 2030-03-11 04:30 UTC is 00:30
        // EDT, after the change to daylight-saving time.
        let new_york = ScenarioClock::new("America/New_York", "2030-01-15T10:00:00").unwrap();
        assert_eq!(local(1_899_433_800, &new_york), ("2030-03-11".into(), "00:30".into()));
    }
}
