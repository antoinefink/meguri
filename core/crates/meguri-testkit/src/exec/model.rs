//! Stored model observations and patches (model-ops L1, L2, L4).
use super::{ExecError, OpResult, Registry, Session};
use crate::scenario::{ObservationArgs, ObservationRequest, Step};
use meguri_model::{
    BlobError, ModelError,
    proto::{
        BlobMessage,
        deck::{DeckCommon, DeckKindMessage},
        deck_config::DeckConfigMessage,
        notetype::{FieldConfig, NotetypeConfig, TemplateConfig},
    },
    schema11,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, StampMode};
use serde_json::{Map, Value};

pub(super) fn register(r: &mut Registry) {
    r.op_rejecting("legacy_update", &["invalid_json"], update);
    r.observation("legacy_json", observe);
    r.observation("blob_decode", observe_blobs);
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
fn object<R: ReadRows>(rows: &R, kind: &str, name: &str) -> Result<Value, ExecError> {
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
    observe_named(s, request, object::<meguri_storage::Writer>)
}
fn observe_blobs(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    observe_named(s, request, blob_status)
}
fn observe_named(
    s: &mut Session<'_>,
    request: &ObservationRequest,
    value: fn(&meguri_storage::Writer, &str, &str) -> Result<Value, ExecError>,
) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(args) = &request.args else {
        return Err(core("legacy_json expects mapping"));
    };
    let mut out = Map::new();
    for (kind, names) in args {
        let mut values = Map::new();
        for name in names.as_array().ok_or_else(|| core("legacy names must be a list"))? {
            let name = name.as_str().ok_or_else(|| core("legacy name must be string"))?;
            values.insert(name.into(), value(writer(s)?, kind, name)?);
        }
        out.insert(kind.clone(), Value::Object(values));
    }
    Ok(out.into())
}

// Read names and raw columns first: one malformed record must not hide another.
fn blob_status(writer: &meguri_storage::Writer, kind: &str, name: &str) -> Result<Value, ExecError> {
    let (table, columns, stored_name) = match kind {
        "notetypes" => ("notetypes", "id, config", name.to_owned()),
        "decks" => ("decks", "id, common, kind", name.replace("::", "\u{1f}")),
        "presets" => ("deck_config", "id, config", name.to_owned()),
        _ => return Err(core("unknown blob kind")),
    };
    let rows = writer
        .execute_raw(
            &format!("SELECT {columns} FROM {table} WHERE name = ? COLLATE BINARY"),
            &[SqlValue::Text(stored_name)],
        )
        .map_err(core)?;
    let row = match rows.rows.as_slice() {
        [] => return Err(unknown(name)),
        [row] => row,
        _ => return Err(ExecError::new("invalid_scenario", "ambiguous record name")),
    };
    let Some(SqlValue::Integer(id)) = row.first() else {
        return Err(core("invalid record identity"));
    };
    let valid = match kind {
        "presets" => decodes::<DeckConfigMessage>(&row[1])?,
        "decks" => {
            let common = decodes::<DeckCommon>(&row[1])?;
            let kind = decodes::<DeckKindMessage>(&row[2])?;
            common && kind
        }
        _ => {
            let mut valid = decodes::<NotetypeConfig>(&row[1])?;
            for table in ["fields", "templates"] {
                let children = writer
                    .execute_raw(
                        &format!("SELECT config FROM {table} WHERE ntid = ? ORDER BY ord"),
                        &[SqlValue::Integer(*id)],
                    )
                    .map_err(core)?;
                for child in &children.rows {
                    valid &= if table == "fields" {
                        decodes::<FieldConfig>(&child[0])?
                    } else {
                        decodes::<TemplateConfig>(&child[0])?
                    };
                }
            }
            valid
        }
    };
    Ok(Value::String(if valid { "ok" } else { "invalid_blob" }.into()))
}
fn decodes<M: BlobMessage>(value: &SqlValue) -> Result<bool, ExecError> {
    let SqlValue::Blob(bytes) = value else {
        return Err(core("a blob column is not stored as BLOB"));
    };
    match M::decode(bytes) {
        Ok(_) => Ok(true),
        Err(
            e @ ModelError::Blob {
                reason: BlobError::NoDeckKind,
                ..
            },
        ) => Err(core(e)),
        Err(ModelError::Blob { .. }) => Ok(false),
        Err(e) => Err(core(e)),
    }
}
