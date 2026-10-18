//! The scheduler area's adapters: the pure `fsrs_next` observation
//! (float-determinism.md F3, F5): the FSRS answer states of a stored review
//! card per input, computed by meguri-sched without a collection (F5.5);
//! `meguri-cli fp-probe` evaluates the probe corpus with the same functions
//! (F10.1), and also the fuzz table (`sched/fuzz` Z6). And the day clock's
//! `timing` observation and `reopen` op (`sched/day-clock` D14.2): one day
//! query on the session's collection, with the engine's `day_query` in a
//! `Raw` write scope and the session's cached day, until the session runs
//! the engine (U38).

use std::collections::BTreeMap;

use fsrs::FSRS;
use meguri_engine::{DayQuery, day_query};
use meguri_model::clock::Clock;
use meguri_sched::states::fuzz;
use meguri_sched::states::review::{self, ReviewAnswers, ReviewCard};
use meguri_storage::{ReadRows, StampMode, StorageError};
use serde_json::{Value, json};

use super::{ExecError, OpResult, Registry, Session};
use crate::observe;
use crate::scenario::{FsrsNextInput, FsrsNextRequest, LogicalRef, ObservationArgs, ObservationRequest, RefKind, Step};

/// Registers the `fsrs_next` and `timing` kinds and the `reopen` op.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("fsrs_next", observe);
    registry.observation("timing", timing);
    registry.observation("fuzz_table", fuzz_table);
    registry.op("reopen", reopen);
}

/// O4/Z6: synthetic ids/counts are pure inputs, never temporary card edits.
fn fuzz_table(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("fuzz_table takes {card, pairs, intervals}"));
    };
    let reference: LogicalRef = map
        .get("card")
        .and_then(Value::as_str)
        .ok_or_else(|| invalid("fuzz_table needs a card reference"))?
        .parse()
        .map_err(|e| invalid(format!("fuzz_table card: {e}")))?;
    if reference.kind != RefKind::Card {
        return Err(invalid("fuzz_table needs a card reference"));
    }
    let id = *session
        .cards
        .get(reference.index())
        .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?;
    let core = |e: StorageError| ExecError::new("core_error", e.to_string());
    let rows = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open"))?
        .reader()?;
    let card = rows
        .card(meguri_model::ids::CardId(id))
        .map_err(core)?
        .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?;
    let preset = meguri_sched::model::preset::PresetView::for_card(&*rows, &card)
        .map_err(core)?
        .ok_or_else(|| invalid("fuzz_table card has no normal home deck"))?;
    let maximum = preset.fields().maximum_review_interval;
    let pairs = map
        .get("pairs")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("fuzz_table needs pairs"))?
        .iter()
        .map(|value| {
            let pair = value
                .as_array()
                .filter(|p| p.len() == 2)
                .ok_or_else(|| invalid("fuzz_table pair must have two integers"))?;
            let id = pair[0]
                .as_u64()
                .ok_or_else(|| invalid("fuzz_table id must be an unsigned integer"))?;
            let reps = pair[1]
                .as_u64()
                .filter(|v| *v < u64::from(u32::MAX))
                .ok_or_else(|| invalid("fuzz_table count exceeds 4294967294"))?;
            Ok((id, reps))
        })
        .collect::<Result<Vec<_>, ExecError>>()?;
    // The oracle lends this one card's identity. Reject collisions even
    // though Meguri's pure computation has no reason to rewrite that row.
    let occupied: std::collections::BTreeSet<u64> = rows
        .ids_after(meguri_storage::Table::Cards, None, usize::MAX)
        .map_err(core)?
        .into_iter()
        .filter(|other| *other != id)
        .map(|v| v as u64)
        .collect();
    if pairs.iter().any(|(id, _)| occupied.contains(id)) {
        return Err(invalid("fuzz_table id belongs to another card"));
    }
    let intervals = map
        .get("intervals")
        .and_then(Value::as_array)
        .filter(|v| !v.is_empty())
        .ok_or_else(|| invalid("fuzz_table needs intervals"))?
        .iter()
        .map(|v| {
            v.as_u64()
                .and_then(|v| u32::try_from(v).ok())
                .ok_or_else(|| invalid("fuzz_table intervals must fit u32"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"intervals":intervals,"rows":fuzz_table_rows(&pairs,&intervals,maximum)}))
}

/// One day query now (`sched/day-clock` D5 to D7) on the session's
/// collection: the engine's `checkRollover()` without its events, which no
/// observation reads.
pub(super) fn day_query_now(session: &mut Session<'_>) -> Result<DayQuery, ExecError> {
    let Session {
        storage, day, clock, ..
    } = session;
    let collection = storage
        .collection()
        .ok_or_else(|| invalid("no collection is open (storage-ops.md P1.2)"))?;
    let now_ms = clock.now_ms();
    let core = |e: StorageError| ExecError::new("core_error", e.to_string());
    match collection {
        super::storage::SessionCollection::Raw(raw) => {
            let mut scope = raw
                .writer()
                .begin_write(StampMode::Raw, clock.as_ref(), None)
                .map_err(core)?;
            let query =
                day_query(&mut scope, day, now_ms.div_euclid(1000), clock.utc_offset_at(now_ms)).map_err(core)?;
            scope.commit().map_err(core)?;
            Ok(query)
        }
        super::storage::SessionCollection::Hosted { collection, .. } => {
            let mut cache = day.clone();
            let offset = clock.utc_offset_at(now_ms);
            let (query, cache) = collection
                .run(
                    meguri_engine::OpSpec::named("timing", meguri_engine::OpSource::Rollover).stamp(StampMode::Raw),
                    move |ctx| {
                        let query = day_query(ctx.scope(), &mut cache, now_ms.div_euclid(1000), offset)?;
                        Ok((query, cache))
                    },
                )
                .map_err(|e| ExecError::new("core_error", e.to_string()))?
                .value;
            *day = cache;
            Ok(query)
        }
    }
}

/// observations.md O4: the day number, the next boundary, now and the
/// offset now, after one day query.
fn timing(session: &mut Session<'_>, _request: &ObservationRequest) -> Result<Value, ExecError> {
    let query = day_query_now(session)?;
    let now_ms = session.clock.now_ms();
    Ok(json!({
        "days_elapsed": query.day.today,
        "next_day_at": query.day.next_day_at,
        "now": now_ms.div_euclid(1000),
        "utc_offset_secs": session.clock.utc_offset_at(now_ms).seconds(),
    }))
}

/// sync-ops.md S6: closes the collection and opens it again from its file.
/// This raw fixture adapter invalidates its cached day; an explicit timing
/// observation performs the query (D14.2). CoreHost owns real lifecycle steps.
fn reopen(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    super::storage::reopen_collection(session)?;
    session.day.clear();
    Ok(OpResult::Done)
}

fn invalid(detail: impl Into<String>) -> ExecError {
    ExecError::new("invalid_scenario", detail)
}

fn observe(_session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("observation fsrs_next takes {param_sets, inputs}"));
    };
    let request = FsrsNextRequest::from_mapping(map).map_err(|e| invalid(e.to_string()))?;
    let models = FsrsModels::new(&request.param_sets)?;
    let rows = request
        .inputs
        .iter()
        .map(|input| models.evaluate(input).and_then(|answers| row(&answers)))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({ "rows": rows }))
}

/// One FSRS model per parameter set, made once per request.
#[derive(Debug, Clone)]
pub struct FsrsModels(BTreeMap<String, FSRS>);

impl FsrsModels {
    /// `FSRS::new` with each set's values as they are (sched/fsrs-transitions
    /// T1): the crate fills and clips them itself.
    pub fn new(param_sets: &BTreeMap<String, Vec<f32>>) -> Result<FsrsModels, ExecError> {
        param_sets
            .iter()
            .map(|(name, values)| {
                FSRS::new(values)
                    .map(|model| (name.clone(), model))
                    .map_err(|e| ExecError::new("core_error", format!("parameter set {name}: fsrs refused it: {e:?}")))
            })
            .collect::<Result<_, _>>()
            .map(FsrsModels)
    }

    /// F3.2: the answer states of the stored review card of F3.1 that this
    /// input describes.
    pub fn evaluate(&self, input: &FsrsNextInput) -> Result<ReviewAnswers, ExecError> {
        let model = self
            .0
            .get(&input.params)
            .ok_or_else(|| invalid(format!("input {}: no parameter set {:?}", input.id, input.params)))?;
        let card = ReviewCard {
            stability: input.s,
            difficulty: input.d,
            days_elapsed: input.elapsed_days,
            interval: input.scheduled_days,
            desired_retention: input.dr,
            maximum_interval: input.max_ivl,
            fuzz_seed: fuzz::seed(input.card_id, input.reps),
        };
        review::answers(model, &card)
            .map_err(|e| ExecError::new("core_error", format!("input {}: fsrs: {e:?}", input.id)))
    }
}

/// An F3.3 row: days, stability and difficulty for again, hard, good and
/// easy, the floats as their shortest `f32` text (O6).
pub fn row(answers: &ReviewAnswers) -> Result<Value, ExecError> {
    let mut values = Vec::with_capacity(observe::FSRS_NEXT_COLUMNS);
    for answer in [answers.again, answers.hard, answers.good, answers.easy] {
        values.push(json!(answer.days));
        for x in [answer.stability, answer.difficulty] {
            values.push(observe::f32_value(x).map_err(|e| ExecError::new("core_error", e.to_string()))?);
        }
    }
    Ok(Value::Array(values))
}

/// The rows of a `fuzz_table` value (observations.md O4, `sched/fuzz` Z6):
/// `[card_id, reps, delta, ...]` per pair, one delta per interval, for a card
/// whose home preset has the maximum interval `maximum`.
pub fn fuzz_table_rows(pairs: &[(u64, u64)], intervals: &[u32], maximum: u32) -> Vec<Value> {
    pairs
        .iter()
        .map(|&(card_id, reps)| {
            let mut row = vec![json!(card_id), json!(reps)];
            row.extend(
                intervals
                    .iter()
                    .map(|&interval| json!(fuzz::stored_card_delta(card_id, reps, interval, maximum))),
            );
            Value::Array(row)
        })
        .collect()
}
