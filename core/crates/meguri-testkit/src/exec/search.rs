//! The search area's observation kinds (`specs/conformance/search-ops.md`):
//! `search` parses each query, builds its plan for the mode and runs the
//! plan's statement on the session's collection, sorted when the request
//! has an `order` (`browser-ops.md` B1, B2); `canonical_search` writes
//! canonical texts and never touches the collection.

use meguri_search::config::SortDirection;
use meguri_search::{
    BuildError, ContextReads, ContextValue, FoldMode, Param, Plan, SearchContext, SearchError, SearchMode,
    SearchNotetypes, Sort, SortError, SortedQuery, parse,
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
    registry.observation("search_build", search_build);
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

/// S6 `search_build`: each S5 item mapped to the core's built item (J3)
/// and written by `build`; a parse error of a parsed-text part is the
/// entry's value. Scenarios hold no item J3 refuses, so a refusal is a run
/// error of the executor.
fn search_build(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use meguri_search::builder::{BuilderError, build};
    let items = crate::scenario::SearchBuildRequest::from_list(list(request)?).map_err(|e| invalid(e.to_string()))?;
    items
        .iter()
        .map(|item| match build(&built_item(item)) {
            Ok(text) => Ok(json!({ "text": text })),
            Err(BuilderError::Parse { error, .. }) => Ok(error_value(&error)),
            Err(BuilderError::InvalidItem { reason, path }) => Err(ExecError::new(
                "invalid_item",
                format!("the builder refused the item at {path:?}: {}", reason.reason()),
            )),
        })
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
}

/// S5's neutral item as J3's built item. Groups and negations are mapped
/// with an explicit stack, children before parents.
fn built_item(top: &crate::scenario::BuiltSearchItem) -> meguri_search::BuiltItem {
    use crate::scenario::{BuiltSearchItem as S, SearchFlag, SearchJoiner, SearchState, SearchWord};
    use meguri_model::ids::NotetypeId;
    use meguri_search::ast::{CardState, IdKind};
    use meguri_search::{BuiltItem as B, Joiner};
    enum Step<'a> {
        Visit(&'a S),
        Group(SearchJoiner, usize),
        Negate,
    }
    let mut steps = vec![Step::Visit(top)];
    let mut made: Vec<B> = Vec::new();
    while let Some(step) = steps.pop() {
        let item = match step {
            Step::Group(joiner, count) => {
                let items = made.split_off(made.len() - count);
                let joiner = match joiner {
                    SearchJoiner::And => Joiner::And,
                    SearchJoiner::Or => Joiner::Or,
                };
                B::Group { joiner, items }
            }
            Step::Negate => {
                let child = made.pop().expect("a negation's child is made first");
                B::Negated(Box::new(child))
            }
            Step::Visit(S::Group { joiner, items }) => {
                steps.push(Step::Group(*joiner, items.len()));
                steps.extend(items.iter().rev().map(Step::Visit));
                continue;
            }
            Step::Visit(S::Negated(child)) => {
                steps.push(Step::Negate);
                steps.push(Step::Visit(child));
                continue;
            }
            Step::Visit(leaf) => match leaf {
                S::Word(SearchWord::WholeCollection) => B::WholeCollection,
                S::Word(SearchWord::CurrentDeck) => B::CurrentDeck,
                S::Word(SearchWord::AnyDeck) => B::AnyDeck,
                S::Word(SearchWord::AnyTag) => B::AnyTag,
                S::Word(SearchWord::Untagged) => B::Untagged,
                S::Word(SearchWord::AnyNotetype) => B::AnyNotetype,
                S::Deck(name) => B::Deck(name.clone()),
                S::Tag(name) => B::Tag(name.clone()),
                S::Notetype(name) => B::Notetype(name.clone()),
                S::Template(ordinal) => B::Template(u32::from(*ordinal)),
                S::FieldPresent(name) => B::FieldPresent(name.clone()),
                S::Field { name, text } => B::FieldText {
                    field: name.clone(),
                    text: text.clone(),
                },
                S::Literal(text) => B::Literal(text.clone()),
                S::Flag(SearchFlag::Any) => B::AnyFlag,
                S::Flag(SearchFlag::None) => B::NoFlag,
                S::Flag(SearchFlag::Color(flag)) => B::Flag(*flag),
                S::State(state) => B::State(match state {
                    SearchState::New => CardState::New,
                    SearchState::Learn => CardState::Learn,
                    SearchState::Review => CardState::Review,
                    SearchState::Due => CardState::Due,
                    SearchState::Suspended => CardState::Suspended,
                    SearchState::Buried => CardState::Buried,
                }),
                S::Added(days) => B::Added(u64::from(*days)),
                S::Edited(days) => B::Edited(u64::from(*days)),
                S::Introduced(days) => B::Introduced(u64::from(*days)),
                S::Answered { days, button } => B::Answered {
                    days: u64::from(*days),
                    button: *button,
                },
                S::Rescheduled(days) => B::Rescheduled(u64::from(*days)),
                S::DueOn(day) => B::DueOn(i64::from(*day)),
                S::DueWithin(day) => B::DueWithin(i64::from(*day)),
                // S5 bounds ids to 0..2^53-1, so they fit.
                S::NoteIds(ids) => B::Ids {
                    kind: IdKind::Notes,
                    ids: ids.iter().map(|id| *id as i64).collect(),
                },
                S::Dupe { notetype_id, text } => B::Duplicates {
                    notetype: NotetypeId(*notetype_id as i64),
                    text: text.clone(),
                },
                S::Text { text, field_text } => B::Parsed {
                    text: text.clone(),
                    field_text: *field_text,
                },
                S::Group { .. } | S::Negated(_) => unreachable!("visited above"),
            },
        };
        made.push(item);
    }
    made.pop().expect("the top item is made last")
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
/// after planning, before execution. The current deck is `curDeck` read as
/// a 64-bit integer, 1 when it is missing or reads otherwise
/// (`search/names-and-ids` N3.4 step 1).
fn context(session: &Session<'_>) -> Result<SearchContext, ExecError> {
    let current_deck = config(session, "curDeck")?
        .as_ref()
        .and_then(ConfigValue::as_i64)
        .unwrap_or(1);
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
    Ok(context)
}

/// T15: the note types, read on the collection the searches run on; no step
/// runs between this read and the searches of one observation.
fn notetypes(session: &Session<'_>) -> Result<SearchNotetypes, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    SearchNotetypes::read(&*collection.reader()?).map_err(|e| ExecError::new("core_error", e.to_string()))
}

/// S1: one output per request; a request with `order` is a sorted search
/// (browser-ops.md B1, B2).
fn search(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let requests = SearchRequest::from_list(list(request)?).map_err(|e| invalid(e.to_string()))?;
    let mut context = context(session)?;
    let notetypes = notetypes(session)?;
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
        // B1: a parse error first, then `unsortable`, then the run errors.
        let sort = match &request.order {
            None => None,
            Some(order) => {
                let direction = if order.reverse == Some(true) {
                    SortDirection::Descending
                } else {
                    SortDirection::Ascending
                };
                match Sort::new(mode, &order.column, direction) {
                    Ok(sort) => Some(sort),
                    Err(_) => {
                        outputs.push(json!({"error": "unsortable"}));
                        continue;
                    }
                }
            }
        };
        let build = |context: &SearchContext| -> Result<Result<Built, Value>, ExecError> {
            let plan = match Plan::build(&query, mode, context, &notetypes) {
                Ok(plan) => plan,
                Err(BuildError::Search(error)) => return Ok(Err(error_value(&error))),
            };
            let sorted = match &sort {
                None => None,
                Some(sort) => Some(SortedQuery::for_plan(&plan, sort, context).map_err(|error| match error {
                    SortError::FieldColumnNotYetSupported { name } => ExecError::new(
                        "unsupported_observation",
                        format!("the sort key of field column {name:?} is U261's"),
                    ),
                    SortError::ModeMismatch => ExecError::new("core_error", "sorted plan of another mode"),
                })?),
            };
            Ok(Ok(Built { plan, sorted }))
        };
        let mut built = match build(&context)? {
            Ok(built) => built,
            Err(value) => {
                outputs.push(value);
                continue;
            }
        };
        if !day_supplied && built.reads_day() {
            // A search never writes, sorted or not: the day is read without
            // the day query's unbury and timing-key writes (browser-ops B1).
            let day = super::sched::day_without_writes(session)?;
            context.today = day.today;
            context.next_day_at = day.next_day_at;
            day_supplied = true;
            // Only context changed; the same parsed query must still compile.
            built = build(&context)?
                .map_err(|e| ExecError::new("core_error", format!("search context replan: {e}")))?;
        }
        outputs.push(match &built.sorted {
            None => run(session, &built.plan, mode)?,
            Some(sorted) => run_sorted(session, sorted)?,
        });
    }
    Ok(Value::Array(outputs))
}

/// A request's plan and, for a sorted search, its sorted statement.
struct Built {
    plan: Plan,
    sorted: Option<SortedQuery>,
}

impl Built {
    fn reads_day(&self) -> bool {
        reads_day(self.plan.reads()) || self.sorted.as_ref().is_some_and(|sorted| reads_day(sorted.reads()))
    }
}

fn reads_day(reads: ContextReads) -> bool {
    reads.contains(ContextValue::Today) || reads.contains(ContextValue::NextDayAt)
}

fn sql_params(params: &[Param]) -> Vec<SqlValue> {
    params
        .iter()
        .map(|param| match param {
            Param::Integer(v) => SqlValue::Integer(*v),
            Param::Real(v) => SqlValue::Real(*v),
            Param::Text(v) => SqlValue::Text(v.clone()),
        })
        .collect()
}

/// Runs a plan: `{ids}` with logical references first, sorted by number,
/// then the real ids no reference names, ascending (S1). A database error is
/// `search_failed` (errors.md E3).
fn run(session: &Session<'_>, plan: &Plan, mode: SearchMode) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let rows = match collection.reader()?.execute_raw(plan.sql(), &sql_params(plan.params())) {
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

/// B1, B2: the sorted ids, each equal-key run listed as S1 orders an
/// unsorted result, everything else at its sorted place. A database error is
/// `search_failed`.
fn run_sorted(session: &Session<'_>, sorted: &SortedQuery) -> Result<Value, ExecError> {
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let rows = match collection.reader()?.execute_raw(sorted.runs_sql(), &sql_params(sorted.params())) {
        Ok(rows) => rows,
        Err(_) => return Ok(json!({"error": "search_failed"})),
    };
    let named = match sorted.mode() {
        SearchMode::Cards => (&session.cards, "#c"),
        SearchMode::Notes => (&session.notes, "#n"),
    };
    let mut ids = Vec::with_capacity(rows.rows.len());
    let mut run: Vec<i64> = Vec::new();
    let mut run_key = None;
    for row in &rows.rows {
        let (Some(SqlValue::Integer(id)), Some(SqlValue::Integer(key))) = (row.first(), row.get(1)) else {
            return Err(ExecError::new(
                "core_error",
                format!("sorted search returned {row:?}, not an id and a run"),
            ));
        };
        if run_key != Some(*key) {
            ids.extend(run_value(&run, named.0, named.1));
            run.clear();
            run_key = Some(*key);
        }
        run.push(*id);
    }
    ids.extend(run_value(&run, named.0, named.1));
    Ok(json!({ "ids": ids }))
}

/// One equal-key run in S1's order.
fn run_value(run: &[i64], named: &[i64], prefix: &str) -> Vec<Value> {
    match ids_value(run, named, prefix) {
        Value::Object(mut map) => match map.remove("ids") {
            Some(Value::Array(ids)) => ids,
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
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
