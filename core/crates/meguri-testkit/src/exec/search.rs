//! The search area's observation kinds (`specs/conformance/search-ops.md`):
//! `search` parses each query, builds its plan for the mode and runs the
//! plan's statement on the session's collection; `canonical_search` writes
//! canonical texts and never touches the collection.

use meguri_search::{
    BuildError, ContextReads, ContextValue, FoldMode, Param, Plan, SearchContext, SearchError, SearchMode, parse,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ConfigValue, ReadRows};
use serde_json::{Map, Value, json};

use super::{ExecError, Registry, Session};
use crate::scenario::{ObservationArgs, ObservationRequest, SearchKind, SearchRequest, canonical_search_texts};

/// Registers search, canonical text and the pure card-data observations.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("search", search);
    registry.observation("canonical_search", canonical_search);
    registry.observation("card_extractors", card_extractors);
}

/// E5: a scratch connection only; never consult the scenario collection,
/// clock, fixture or golden. All six results come from the real SQL functions.
fn card_extractors(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use crate::scenario::{CardExtractorsRequest, ExtractorValue};
    use rusqlite::{
        ToSql,
        types::{ToSqlOutput, Value as SqlResult, ValueRef},
    };

    struct Bound<'a>(&'a ExtractorValue);
    impl ToSql for Bound<'_> {
        fn to_sql(&self) -> rusqlite::Result<ToSqlOutput<'_>> {
            Ok(ToSqlOutput::Borrowed(match self.0 {
                ExtractorValue::Null => ValueRef::Null,
                ExtractorValue::Integer(v) => ValueRef::Integer(*v),
                ExtractorValue::Real(v) => ValueRef::Real(*v),
                ExtractorValue::Text(v) => ValueRef::Text(v),
                ExtractorValue::Blob(v) => ValueRef::Blob(v),
            }))
        }
    }
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("card_extractors takes a mapping"));
    };
    let inputs = CardExtractorsRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let core = |e: rusqlite::Error| ExecError::new("core_error", e.to_string());
    let conn = rusqlite::Connection::open_in_memory().map_err(core)?;
    meguri_search::sqlfns::register(&conn).map_err(core)?;
    let mut statement = conn.prepare("SELECT meguri_card_f32(?1,'s'),meguri_card_f32(?1,'d'),meguri_card_f32(?1,'dr'),meguri_retrievability(?1,?2,?3,?4,?5),meguri_relative_retrievability(?1,?2,?3,?4,?5),meguri_original_position(?1)").map_err(core)?;
    let mut rows = Vec::with_capacity(inputs.inputs.len());
    for input in inputs.inputs {
        let values = statement
            .query_row(rusqlite::params_from_iter(input.arguments.iter().map(Bound)), |row| {
                Ok([
                    row.get::<_, SqlResult>(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ])
            })
            .map_err(core)?;
        let mut output = Map::new();
        output.insert("id".into(), json!(input.id));
        for (key, value) in ["s", "d", "dr", "r", "rr", "pos"].into_iter().zip(values) {
            let value = match value {
                SqlResult::Null => Value::Null,
                SqlResult::Integer(v) if key == "pos" => json!(v),
                SqlResult::Real(v) if key != "pos" => {
                    if v.is_nan() {
                        Value::Null
                    } else if v == f64::INFINITY {
                        json!("inf")
                    } else if v == f64::NEG_INFINITY {
                        json!("-inf")
                    } else if f64::from(v as f32) == v {
                        crate::observe::f32_value(v as f32).map_err(|e| ExecError::new("core_error", e.to_string()))?
                    } else {
                        return Err(ExecError::new("core_error", "extractor REAL is not a widened f32"));
                    }
                }
                _ => {
                    return Err(ExecError::new(
                        "core_error",
                        "unexpected extractor result storage class",
                    ));
                }
            };
            output.insert(key.into(), value);
        }
        rows.push(Value::Object(output));
    }
    Ok(json!({"rows": rows}))
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn list(request: &ObservationRequest) -> Result<&[Value], ExecError> {
    match &request.args {
        ObservationArgs::List(items) => Ok(items),
        _ => Err(invalid(format!("observation {} takes a list", request.kind.name()))),
    }
}

/// S3: the kind, then `provided` and `context` for the kinds that carry
/// them, backticks turned to apostrophes. Run errors carry neither.
fn error_value(error: &SearchError) -> Value {
    let mut value = Map::new();
    value.insert("error".to_owned(), json!(error.code()));
    if error.kind().is_parse_error() {
        if let Some(provided) = error.kind().provided() {
            value.insert(
                "provided".to_owned(),
                json!(meguri_search::error::apostrophes(provided)),
            );
        }
        if let Some(context) = error.kind().context() {
            value.insert("context".to_owned(), json!(meguri_search::error::apostrophes(context)));
        }
    }
    Value::Object(value)
}

/// S2.
fn canonical_search(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let texts = canonical_search_texts(list(request)?).map_err(|e| invalid(e.to_string()))?;
    Ok(Value::Array(
        texts
            .iter()
            .map(|text| match meguri_search::canonical_text(text) {
                Ok(canonical) => json!({ "text": canonical }),
                Err(error) => error_value(&error),
            })
            .collect(),
    ))
}

/// A config value of the collection, if it is set.
fn config(session: &Session<'_>, key: &str) -> Result<Option<ConfigValue>, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    collection
        .reader()?
        .config_value(key)
        .map_err(|e| ExecError::new("core_error", e.to_string()))
}

/// Q8 from the scenario clock and config. Day values are filled on demand
/// after planning, before execution; a missing `curDeck` remains unavailable.
fn context(session: &Session<'_>) -> Result<(SearchContext, ContextReads), ExecError> {
    let mut unknown = ContextReads::NONE;
    let current_deck = match config(session, "curDeck")?.as_ref().and_then(ConfigValue::as_i64) {
        Some(id) => id,
        None => {
            unknown = unknown.with(ContextValue::CurrentDeck);
            0
        }
    };
    let flag = |key: &str, missing: bool| -> Result<bool, ExecError> {
        Ok(config(session, key)?
            .as_ref()
            .and_then(ConfigValue::as_bool)
            .unwrap_or(missing))
    };
    let learn_ahead_secs = config(session, "collapseTime")?
        .as_ref()
        .and_then(ConfigValue::as_u64)
        .and_then(|secs| u32::try_from(secs).ok())
        .unwrap_or(SearchContext::DEFAULT_LEARN_AHEAD_SECS);
    let context = SearchContext {
        now: session.clock.now_secs(),
        today: 0,
        next_day_at: 0,
        learn_ahead_secs,
        current_deck: meguri_model::ids::DeckId(current_deck),
        normalize_note_text: flag("normalize_note_text", true)?,
        ignore_accents: flag("ignoreAccentsInSearch", false)?,
        fold: FoldMode::Unicode,
    };
    Ok((context, unknown))
}

/// S1: one output per request.
fn search(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let requests = SearchRequest::from_list(list(request)?).map_err(|e| invalid(e.to_string()))?;
    if requests.iter().any(|request| request.order.is_some()) {
        return Err(ExecError::new("unsupported_observation", "sorted search is pending"));
    }
    let (mut context, unknown) = context(session)?;
    let mut day_supplied = false;
    let mut outputs = Vec::with_capacity(requests.len());
    for request in &requests {
        let mode = match request.kind {
            SearchKind::Cards => SearchMode::Cards,
            SearchKind::Notes => SearchMode::Notes,
        };
        let query = match parse(&request.query) {
            Ok(query) => query,
            Err(error) => {
                outputs.push(error_value(&error));
                continue;
            }
        };
        let mut plan = match Plan::build(&query, mode, &context) {
            Ok(plan) => plan,
            Err(BuildError::Search(error)) => {
                outputs.push(error_value(&error));
                continue;
            }
            Err(BuildError::NotYetSupported { term, .. }) => {
                return Err(ExecError::new(
                    "unsupported_observation",
                    format!(
                        "search {:?}: Meguri cannot match the term {term:?} yet (U81, U82)",
                        request.query
                    ),
                ));
            }
        };
        if !day_supplied
            && (plan.reads().contains(ContextValue::Today) || plan.reads().contains(ContextValue::NextDayAt))
        {
            let day = super::sched::day_query_now(session)?.day;
            context.today = day.today;
            context.next_day_at = day.next_day_at;
            day_supplied = true;
            // Only context changed; the same parsed query must still compile.
            plan = Plan::build(&query, mode, &context)
                .map_err(|e| ExecError::new("core_error", format!("search context replan: {e:?}")))?;
        }
        if let Some(value) = plan.reads().iter().find(|v| unknown.contains(*v)) {
            return Err(ExecError::new(
                "unsupported_observation",
                format!(
                    "search {:?} reads {value:?}, which the search adapter cannot give yet",
                    request.query
                ),
            ));
        }
        outputs.push(run(session, &plan, mode)?);
    }
    Ok(Value::Array(outputs))
}

/// Runs a plan: `{ids}` with logical references first, sorted by number,
/// then the real ids no reference names, ascending (S1). A database error is
/// `search_failed` (errors.md E3).
fn run(session: &Session<'_>, plan: &Plan, mode: SearchMode) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let params: Vec<SqlValue> = plan
        .params()
        .iter()
        .map(|param| match param {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect();
    let rows = match collection.reader()?.execute_raw(plan.sql(), &params) {
        Ok(rows) => rows,
        Err(_) => return Ok(json!({"error": "search_failed"})),
    };
    let mut found = Vec::with_capacity(rows.rows.len());
    for row in &rows.rows {
        let Some(SqlValue::Integer(id)) = row.first() else {
            return Err(ExecError::new(
                "core_error",
                format!("search returned {row:?}, not an id"),
            ));
        };
        found.push(*id);
    }
    Ok(match mode {
        SearchMode::Cards => ids_value(&found, &session.cards, "#c"),
        SearchMode::Notes => ids_value(&found, &session.notes, "#n"),
    })
}

/// S1's `{ids}`: the found ids that a logical reference names (`named[k]` is
/// `<prefix><k + 1>`) as references sorted by number, then the others as
/// integers, ascending. The order the search returned them in is dropped.
fn ids_value(found: &[i64], named: &[i64], prefix: &str) -> Value {
    let mut refs = Vec::new();
    let mut unnamed = Vec::new();
    for id in found {
        match named.iter().position(|real| real == id) {
            Some(index) => refs.push(index + 1),
            None => unnamed.push(*id),
        }
    }
    refs.sort_unstable();
    unnamed.sort_unstable();
    let ids: Vec<Value> = refs
        .into_iter()
        .map(|n| json!(format!("{prefix}{n}")))
        .chain(unnamed.into_iter().map(|id| json!(id)))
        .collect();
    json!({ "ids": ids })
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    // S1: references by number (#c2 before #c10), then unnamed real ids
    // ascending, whatever order the rows came in.
    #[test]
    fn found_ids_are_references_by_number_then_real_ids() {
        let named: Vec<i64> = (1..=10).map(|k| 1000 + k).collect();
        let found = [1010, 77, 1002, 5, 1001];
        assert_eq!(
            super::ids_value(&found, &named, "#c"),
            json!({"ids": ["#c1", "#c2", "#c10", 5, 77]})
        );
        assert_eq!(super::ids_value(&[], &named, "#n"), json!({"ids": []}));
    }
}
