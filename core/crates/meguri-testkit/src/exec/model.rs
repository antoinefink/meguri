//! Schema-11 observations and patches needed by U28 (model-ops L1-L2).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{ObservationArgs, ObservationRequest, Step};
use meguri_model::schema11;
use meguri_storage::{ReadRows, StampMode};
use serde_json::{Map, Value};

pub(super) fn register(r: &mut Registry) {
    r.op_rejecting("legacy_update", &["invalid_json"], update);
    r.observation("legacy_json", observe);
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
fn unknown(s: &str) -> ExecError {
    ExecError::new("unknown_ref", format!("no record {s:?}"))
}
fn writer<'a>(s: &'a Session<'_>) -> Result<&'a meguri_storage::Writer, ExecError> {
    s.storage
        .collection()
        .map(|c| c.writer())
        .ok_or_else(|| core("no collection"))
}
fn object(rows: &impl ReadRows, kind: &str, name: &str) -> Result<Value, ExecError> {
    let text = match kind {
        "decks" => schema11::deck_to_json(
            &rows
                .deck_rows()
                .map_err(core)?
                .into_iter()
                .find(|d| d.human_name() == name)
                .ok_or_else(|| unknown(name))?,
        ),
        "presets" => {
            let mut matches = rows
                .deck_config_rows()
                .map_err(core)?
                .into_iter()
                .filter(|p| p.name == name);
            let p = matches.next().ok_or_else(|| unknown(name))?;
            if matches.next().is_some() {
                return Err(ExecError::new("invalid_scenario", "ambiguous preset"));
            }
            schema11::deck_config_to_json(&p)
        }
        "notetypes" => schema11::notetype_to_json(
            &meguri_engine::notetypes::read::by_name(rows, name)
                .map_err(core)?
                .ok_or_else(|| unknown(name))?,
        ),
        _ => return Err(core("unknown legacy kind")),
    };
    crate::observe::parse_json(&text).map_err(core)
}
fn merge(value: &mut Value, patch: &Value) {
    if let Value::Object(patch) = patch {
        if !value.is_object() {
            *value = Value::Object(Map::new());
        }
        for (key, patch) in patch {
            if patch.is_null() {
                value.as_object_mut().expect("object").remove(key);
            } else {
                merge(&mut value[key], patch);
            }
        }
    } else {
        *value = patch.clone();
    }
}
fn update(s: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::LegacyUpdate(args) = step else {
        return Err(core("wrong legacy op"));
    };
    let (kind, name) = if let Some(n) = &args.deck {
        ("decks", n)
    } else if let Some(n) = &args.preset {
        ("presets", n)
    } else {
        (
            "notetypes",
            args.notetype.as_ref().ok_or_else(|| core("no legacy target"))?,
        )
    };
    let mut object = object(writer(s)?, kind, name)?;
    merge(&mut object, &Value::Object(args.patch.clone()));
    let text = object.to_string();
    let mode = if kind == "presets" {
        StampMode::Local
    } else {
        StampMode::CallerSupplied
    };
    let mut scope = writer(s)?.begin_write(mode, s.clock.as_ref(), None).map_err(core)?;
    let result = match kind {
        "decks" => schema11::deck_from_json(&text).map(|mut d| {
            d.name = meguri_model::deck::normalize_native_name(&d.name);
            scope.put(&d)
        }),
        "presets" => schema11::deck_config_from_json(&text).map(|p| scope.put(&p)),
        _ => schema11::notetype_from_json(&text).map(|mut n| {
            n.config.reqs = meguri_engine::notetypes::write::card_requirements(&n);
            scope.put_notetype(&n)
        }),
    };
    match result {
        Err(_) => Ok(OpResult::Rejected("invalid_json")),
        Ok(written) => {
            written.map_err(core)?;
            scope.commit().map_err(core)?;
            Ok(OpResult::Done)
        }
    }
}
fn observe(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(args) = &request.args else {
        return Err(core("legacy_json expects mapping"));
    };
    let mut out = Map::new();
    for (kind, names) in args {
        let mut values = Map::new();
        for name in names.as_array().ok_or_else(|| core("legacy names must be a list"))? {
            let name = name.as_str().ok_or_else(|| core("legacy name must be string"))?;
            values.insert(name.into(), object(writer(s)?, kind, name)?);
        }
        out.insert(kind.clone(), Value::Object(values));
    }
    Ok(out.into())
}
