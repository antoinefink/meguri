//! The scheduler area's adapters. So far the pure `fsrs_next` observation
//! (float-determinism.md F3, F5): the FSRS answer states of a stored review
//! card per input, computed by meguri-sched without a collection (F5.5).
//! `meguri-cli fp-probe` evaluates the probe corpus with the same functions
//! (F10.1), and also the fuzz table (`sched/fuzz` Z6).

use std::collections::BTreeMap;

use fsrs::FSRS;
use meguri_sched::states::fuzz;
use meguri_sched::states::review::{self, ReviewAnswers, ReviewCard};
use serde_json::{Value, json};

use super::{ExecError, Registry, Session};
use crate::observe;
use crate::scenario::{FsrsNextInput, FsrsNextRequest, ObservationArgs, ObservationRequest};

/// Registers the `fsrs_next` kind.
pub(super) fn register(registry: &mut Registry) {
    registry.observation("fsrs_next", observe);
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
