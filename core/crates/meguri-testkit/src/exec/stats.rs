//! The stats area's adapters (`specs/conformance/stats-ops.md`): the
//! `card_stats` observation (H9) reads Card Info's raw values from
//! meguri-stats' snapshot, the product read (`stats/card-info` C3, C6.1).
//! A snapshot that fails is a run error named by its C11 kind.
//!
//! The `stats` observation (H10, `stats/graphs-common` G14) runs the
//! product's graph request (G8) once per listed request, each its own
//! refresh, and writes the records with `observe::stats`.

use meguri_engine::CancelToken;
use meguri_model::{card_data::MemoryState, ids::CardId};
use meguri_stats::graphs::{Columns, Graph, GraphRequest, GraphResult, History, Scope, ScopeError};
use meguri_stats::{CardInfoOutcome, card_info::RawValues};
use meguri_storage::{ReadRows, ReaderRole};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::observe::f32_value;
use crate::observe::stats::{context_part, graph_part};
use crate::scenario::{ObservationArgs, ObservationRequest, StatsHistory, StatsPart, StatsRequest, StatsScope};

pub(super) fn register(registry: &mut Registry) {
    registry.observation("card_stats", card_stats);
    registry.observation("stats", stats);
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

/// The graph a part names; `context` names none (G14).
fn graph_of(part: StatsPart) -> Option<Graph> {
    Some(match part {
        StatsPart::Reviews => Graph::Reviews,
        StatsPart::TrueRetention => Graph::Retention,
        StatsPart::Hours => Graph::Hourly,
        StatsPart::Buttons => Graph::AnswerButtons,
        StatsPart::FutureDue => Graph::FutureDue,
        StatsPart::CardCounts => Graph::CardCounts,
        StatsPart::Intervals => Graph::Intervals,
        StatsPart::Stability => Graph::Stability,
        StatsPart::Difficulty => Graph::Difficulty,
        StatsPart::Retrievability => Graph::Retrievability,
        StatsPart::Added => Graph::Added,
        StatsPart::Context => return None,
    })
}

/// H10's Meguri side: the deck named with `::` by its id; a name that
/// names no deck is a run error.
fn typed_scope(collection: &meguri_engine::Collection, scope: &StatsScope) -> Result<Scope, ExecError> {
    Ok(match scope {
        StatsScope::Collection => Scope::Collection,
        StatsScope::Search(text) => Scope::Search(text.clone()),
        StatsScope::Deck(name) => {
            let lease = collection.reader(ReaderRole::Query).map_err(core)?;
            let decks = lease.snapshot().map_err(core)?.deck_rows().map_err(core)?;
            let deck = decks
                .iter()
                .find(|deck| deck.human_name() == *name)
                .ok_or_else(|| ExecError::new("unknown_ref", format!("no deck {name:?}")))?;
            Scope::Deck(deck.id)
        }
    })
}

/// H10: one output per request, in order: its parts, or the scope's search
/// error (`conformance/search-ops` S3), or `unreadable_row`.
fn stats(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::List(items) = &request.args else {
        return Err(ExecError::new("invalid_scenario", "stats needs a request list"));
    };
    let wanted = StatsRequest::from_list(items).map_err(|error| ExecError::new("invalid_scenario", error.to_string()))?;
    let collection = session.engine_collection()?;
    let host = session.storage.host()?;
    let mut outputs = Vec::with_capacity(wanted.len());
    for (refresh, one) in (1..).zip(&wanted) {
        let graph_request = GraphRequest {
            scope: typed_scope(&collection, &one.scope)?,
            history: match one.history {
                StatsHistory::Year => History::Last12Months,
                StatsHistory::All => History::AllHistory,
            },
            graphs: one.parts.iter().copied().filter_map(graph_of).collect(),
            refresh,
            context: None,
            cancel: CancelToken::new(),
            columns: Columns::Cached,
        };
        let output = match host.stats().request(&collection, host.derived(), &graph_request).map_err(core)? {
            GraphResult::Data(data) => {
                let mut parts = Map::new();
                for record in &data.records {
                    let value = graph_part(record.as_ref()).map_err(core)?;
                    parts.insert(record.graph().part().to_owned(), value);
                }
                if one.parts.contains(&StatsPart::Context) {
                    parts.insert("context".to_owned(), context_part(&data.context, &data.preferences));
                }
                Value::Object(parts)
            }
            GraphResult::ScopeError(ScopeError::Search(error)) => super::search::error_value(&error),
            GraphResult::UnreadableRow(_) => json!({"error": "unreadable_row"}),
            GraphResult::ScopeError(ScopeError::UnknownDeck) => {
                return Err(ExecError::new("unknown_ref", "the deck went before its request ran"));
            }
            other @ (GraphResult::ContextChanged | GraphResult::Cancelled) => {
                return Err(core(format!("a first request gave {other:?}")));
            }
        };
        outputs.push(output);
    }
    Ok(Value::Array(outputs))
}
