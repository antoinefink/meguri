//! The stats area's adapters (`specs/conformance/stats-ops.md`): the
//! `card_stats` observation (H9) reads Card Info's raw values from
//! meguri-stats' snapshot, the product read (`stats/card-info` C3, C6.1).
//! A snapshot that fails is a run error named by its C11 kind.

use meguri_model::{card_data::MemoryState, ids::CardId};
use meguri_stats::{CardInfoOutcome, card_info::RawValues};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::observe::f32_value;
use crate::scenario::{ObservationArgs, ObservationRequest};

pub(super) fn register(registry: &mut Registry) {
    registry.observation("card_stats", card_stats);
}

fn core(error: impl std::fmt::Display) -> ExecError {
    ExecError::new("core_error", error.to_string())
}

fn f32_json(value: f32) -> Result<Value, ExecError> {
    f32_value(value).map_err(|error| core(error.to_string()))
}

fn optional_f32(value: Option<f32>) -> Result<Value, ExecError> {
    value.map_or(Ok(Value::Null), f32_json)
}

fn memory(state: Option<MemoryState>) -> Result<Value, ExecError> {
    Ok(match state {
        None => Value::Null,
        Some(state) => json!({
            "stability": f32_json(state.stability)?,
            "difficulty": f32_json(state.difficulty)?,
        }),
    })
}

/// A logical reference when the scenario names the id, else the id.
fn reference(ids: &[i64], id: i64, prefix: &str) -> Value {
    ids.iter()
        .position(|named| *named == id)
        .map_or_else(|| json!(id), |index| json!(format!("{prefix}{}", index + 1)))
}

fn raw_value(session: &Session<'_>, raw: &RawValues) -> Result<Map<String, Value>, ExecError> {
    let mut out = Map::new();
    out.insert("card_id".into(), reference(&session.cards, raw.card_id.0, "#c"));
    out.insert("note_id".into(), reference(&session.notes, raw.note_id.0, "#n"));
    out.insert("deck".into(), json!(raw.deck));
    out.insert("original_deck".into(), json!(raw.original_deck));
    out.insert("card_type".into(), json!(raw.card_type));
    out.insert("notetype".into(), json!(raw.notetype));
    out.insert("preset".into(), json!(raw.preset));
    out.insert("custom_data".into(), json!(raw.custom_data));
    out.insert("added".into(), json!(raw.added));
    out.insert("first_review".into(), json!(raw.first_review));
    out.insert("latest_review".into(), json!(raw.latest_review));
    out.insert("due_date".into(), json!(raw.due_date));
    out.insert("due_position".into(), json!(raw.due_position));
    out.insert("interval".into(), json!(raw.interval));
    out.insert("ease".into(), json!(raw.ease));
    out.insert("reviews".into(), json!(raw.reviews));
    out.insert("lapses".into(), json!(raw.lapses));
    out.insert("average_secs".into(), f32_json(raw.average_secs)?);
    out.insert("total_secs".into(), f32_json(raw.total_secs)?);
    out.insert("desired_retention".into(), optional_f32(raw.desired_retention)?);
    out.insert("fsrs_retrievability".into(), optional_f32(raw.fsrs_retrievability)?);
    out.insert("memory_state".into(), memory(raw.memory_state)?);
    out.insert(
        "fsrs_params".into(),
        Value::Array(raw.fsrs_params.iter().map(|p| f32_json(*p)).collect::<Result<_, _>>()?),
    );
    Ok(out)
}

/// H9: one product read per listed card, in request order.
fn card_stats(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Refs(refs) = &request.args else {
        return Err(ExecError::new("invalid_scenario", "card_stats needs a card list"));
    };
    let ids = refs
        .iter()
        .map(|reference| {
            session
                .cards
                .get(reference.index())
                .copied()
                .map(|id| (reference.to_string(), CardId(id)))
                .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let collection = session.engine_collection()?;
    let mut out = Map::new();
    for (name, id) in ids {
        let info = match meguri_stats::card_info(&collection, id).map_err(core)? {
            CardInfoOutcome::Found(info) => info,
            CardInfoOutcome::Unavailable(kind) => {
                return Err(ExecError::new(kind.as_str(), format!("card_stats {name}")));
            }
        };
        let mut value = raw_value(session, &info.raw)?;
        let revlog = info
            .history
            .iter()
            .map(|entry| {
                Ok(json!({
                    "time": entry.instant,
                    "review_kind": entry.kind.stored(),
                    "button_chosen": entry.rating,
                    "interval": entry.interval_secs,
                    "ease": entry.factor,
                    "taken_secs": f32_json(entry.taken_secs)?,
                    "memory_state": memory(entry.memory_state)?,
                    "last_interval": entry.last_interval_secs,
                }))
            })
            .collect::<Result<Vec<_>, ExecError>>()?;
        value.insert("revlog".into(), Value::Array(revlog));
        out.insert(name, Value::Object(value));
    }
    Ok(Value::Object(out))
}
