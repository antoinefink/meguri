//! Stored card observations (observations O4, record-ops W3).
use super::{ExecError, Registry, Session};
use crate::scenario::{LogicalRef, ObservationArgs, ObservationRequest, RefKind};
use meguri_model::ids::DeckId;
use meguri_storage::{
    ReadRows,
    raw::{RawRows, SqlValue},
};
use serde_json::{Map, Value, json};
pub(super) fn register(r: &mut Registry) {
    r.observation("card", observe);
    r.observation("ids", observe_ids);
}

fn observe_ids(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let refs: Vec<_> = match &request.args {
        ObservationArgs::Refs(refs) => refs.clone(),
        _ => [(RefKind::Note, s.notes.len()), (RefKind::Card, s.cards.len())]
            .into_iter()
            .flat_map(|(kind, count)| (0..count).map(move |i| LogicalRef { kind, n: i as u32 + 1 }))
            .collect(),
    };
    let mut out = Map::new();
    for reference in refs {
        let ids = match reference.kind {
            RefKind::Note => &s.notes,
            RefKind::Card => &s.cards,
        };
        let id = ids
            .get(reference.index())
            .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?;
        out.insert(reference.to_string(), json!(id));
    }
    Ok(out.into())
}
fn core(e: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", e.to_string())
}
pub(super) fn card_value(rows: &impl ReadRows, s: &Session<'_>, raw: &RawRows) -> Result<Value, ExecError> {
    let Some(row) = raw.rows.first() else {
        return Ok(Value::Null);
    };
    let mut out = Map::new();
    for (column, cell) in raw.columns.iter().zip(row) {
        let value = match (column.as_str(), cell) {
            ("id" | "nid", SqlValue::Integer(id)) => {
                let (list, kind) = if column == "id" {
                    (&s.cards, "c")
                } else {
                    (&s.notes, "n")
                };
                list.iter()
                    .position(|v| v == id)
                    .map(|i| json!(format!("#{kind}{}", i + 1)))
                    .ok_or_else(|| core("card row has unknown logical id"))?
            }
            ("did" | "odid", SqlValue::Integer(0)) => Value::Null,
            ("did" | "odid", SqlValue::Integer(id)) => rows
                .deck(DeckId(*id))
                .map_err(core)?
                .map_or(Value::Null, |d| json!(d.human_name())),
            ("data", SqlValue::Text(text)) if text.is_empty() => json!({}),
            ("data", SqlValue::Text(text)) => crate::observe::parse_json(text).unwrap_or_else(|_| json!(text)),
            (_, SqlValue::Integer(v)) => json!(v),
            _ => return Err(core(format!("invalid card column {column}"))),
        };
        out.insert(column.clone(), value);
    }
    Ok(out.into())
}
fn observe(s: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let writer = s.storage.collection().ok_or_else(|| core("no collection"))?.reader()?;
    let refs: Vec<_> = match &request.args {
        ObservationArgs::Refs(refs) => refs.clone(),
        _ => (0..s.cards.len())
            .map(|i| LogicalRef {
                kind: RefKind::Card,
                n: i as u32 + 1,
            })
            .collect(),
    };
    let mut out = Map::new();
    for r in refs {
        let id = s
            .cards
            .get(r.index())
            .ok_or_else(|| ExecError::new("unknown_ref", r.to_string()))?;
        let raw = writer
            .execute_raw("SELECT * FROM cards WHERE id = ?", &[SqlValue::Integer(*id)])
            .map_err(core)?;
        out.insert(r.to_string(), card_value(&*writer, s, &raw)?);
    }
    Ok(out.into())
}
