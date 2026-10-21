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
    registry.op("answer", answer_op);
    registry.observation("revlog", revlog_observation);
    registry.observation("fsrs_next", observe);
    registry.observation("timing", timing);
    registry.observation("queue", queue_observation);
    registry.observation("deck_counts", queue_observation);
    registry.observation("congrats", queue_observation);
    registry.observation("fuzz_table", fuzz_table);
    registry.observation("next_states", scheduling_states);
    registry.observation("answer_labels", scheduling_states);
    registry.observation("ignored_before_count", ignored_before_count);
    registry.op("reopen", reopen);
    registry.op("enable_fsrs", enable_fsrs);
    registry.op("enable_v3", enable_v3);
}

fn enable_fsrs(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    let collection = session.engine_collection()?;
    meguri_engine::sched::fsrs_policy::enable_fsrs(&collection, &Default::default())
        .map_err(|error| ExecError::new("core_error", error.to_string()))?;
    Ok(OpResult::Done)
}

fn enable_v3(session: &mut Session<'_>, _step: &Step) -> Result<OpResult, ExecError> {
    // Promotion runs the actual engine open step; Raw reopen remains Raw.
    session.engine_collection()?;
    Ok(OpResult::Done)
}

fn ignored_before_count(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    let ObservationArgs::Mapping(map) = &request.args else {
        return Err(invalid("ignored_before_count takes {preset, dates}"));
    };
    let request =
        crate::scenario::IgnoredBeforeCountRequest::from_mapping(map).map_err(|error| invalid(error.to_string()))?;
    let day = day_query_now(session)?.day;
    let collection = session.engine_collection()?;
    let core = |error: StorageError| ExecError::new("core_error", error.to_string());
    let rows = collection
        .reader(meguri_storage::ReaderRole::Interactive)
        .map_err(|error| ExecError::new("core_error", error.to_string()))?;
    let preset = rows
        .deck_config_rows()
        .map_err(core)?
        .into_iter()
        .find(|preset| preset.name == request.preset)
        .ok_or_else(|| invalid("unknown preset"))?;
    let preset = meguri_sched::model::preset::PresetView::new(&preset);
    let config = meguri_sched::model::sched_config::SchedConfig::new(&*rows);
    let bool_config = |key: &str, default: bool| -> Result<bool, ExecError> {
        Ok(rows
            .config_value(key)
            .map_err(core)?
            .and_then(|value| value.as_bool())
            .unwrap_or(default))
    };
    let search = meguri_search::SearchContext {
        now: session.clock.now_secs(),
        today: day.today,
        next_day_at: day.next_day_at,
        learn_ahead_secs: config.collapse_time().map_err(core)?,
        current_deck: meguri_model::ids::DeckId(
            rows.config_value("curDeck")
                .map_err(core)?
                .and_then(|value| value.as_i64())
                .unwrap_or(0),
        ),
        normalize_note_text: bool_config("normalize_note_text", true)?,
        ignore_accents: bool_config("ignoreAccentsInSearch", false)?,
        fold: meguri_search::FoldMode::Unicode,
    };
    request
        .dates
        .iter()
        .map(
            |date| match meguri_engine::sched::memory::ignored_before_count(&*rows, &preset, date, &search) {
                Ok(count) => Ok(json!({"included":count.included,"total":count.total})),
                Err(meguri_engine::EngineError::InvalidIgnoreBeforeDate { .. }) => Ok(json!({"error":"invalid_date"})),
                Err(error) => Err(ExecError::new("core_error", error.to_string())),
            },
        )
        .collect::<Result<Vec<_>, _>>()
        .map(Value::Array)
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
            memory_state: meguri_model::card_data::MemoryState {
                stability: input.s,
                difficulty: input.d,
            },
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
        for x in [answer.memory_state.stability, answer.memory_state.difficulty] {
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

/// O5/O10 share the pure engine; no answer or review-log writes occur here.
fn scheduling_states(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use meguri_model::{ids::CardId, proto::deck::DeckKind};
    use meguri_sched::{
        model::{deck_sched::DeckSched, preset::PresetView, sched_config::SchedConfig},
        states::{
            engine::{StateInput, next_states},
            fsrs_glue::FsrsCache,
        },
    };
    let collection = session.engine_collection()?;
    let day = day_query_now(session)?.day;
    let now = session.clock.now_ms().div_euclid(1000);
    let rows = collection
        .reader(meguri_storage::ReaderRole::Interactive)
        .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    let config = SchedConfig::new(&*rows);
    let core = |e: StorageError| ExecError::new("core_error", e.to_string());
    let learn_ahead = config.collapse_time().map_err(core)?;
    let short_term_with_steps = config.fsrs_short_term_with_steps().map_err(core)?;
    let references = match &request.args {
        ObservationArgs::Refs(refs) => refs.clone(),
        _ => (0..session.cards.len())
            .map(|i| LogicalRef {
                kind: RefKind::Card,
                n: i as u32 + 1,
            })
            .collect(),
    };
    let mut cache = FsrsCache::default();
    let mut out = serde_json::Map::new();
    for reference in references {
        let id = *session
            .cards
            .get(reference.index())
            .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?;
        let card = rows
            .card(CardId(id))
            .map_err(core)?
            .ok_or_else(|| ExecError::new("unknown_ref", reference.to_string()))?;
        let preset = PresetView::for_card(&*rows, &card)
            .map_err(core)?
            .ok_or_else(|| invalid("next states need a normal home deck"))?;
        let deck = rows
            .deck(meguri_sched::model::card_sched::CardSched::new(&card).home_deck())
            .map_err(core)?
            .ok_or_else(|| invalid("missing home deck"))?;
        let normal = match &deck.kind.kind {
            DeckKind::Normal(n) => Some(n),
            _ => None,
        };
        let desired_retention = DeckSched::new(normal, &deck.common).retention(preset.fields().desired_retention);
        let mut history = Vec::new();
        if card.card_type != meguri_model::card::CardType::New && card.data.memory_state.is_none() {
            rows.visit_fsrs_histories(
                &meguri_storage::queries::revlog_fsrs::HistorySelection::Cards(vec![card.id]),
                |_, found| -> Result<(), StorageError> {
                    history.extend_from_slice(found);
                    Ok(())
                },
            )
            .map_err(core)?;
        }
        let memory_state =
            meguri_sched::memory::resolve::on_the_fly(&mut cache, &preset, &card, &history, day.next_day_at)
                .map_err(|error| ExecError::new("core_error", error.to_string()))?;
        let last_review_time = match card.data.last_review_secs {
            Some(time) => Some(time),
            None => rows.fsrs_last_review_time(card.id).map_err(core)?,
        };
        let input = StateInput {
            card: &card,
            preset: &preset,
            memory_state,
            last_review_time,
            desired_retention,
            today: day.today,
            next_day_at: day.next_day_at,
            now,
            short_term_with_steps,
            learn_ahead,
        };
        let states = if let Some(balance) = &session.answer_cache {
            balance.with_chooser(&collection, &card, &preset, |chooser| {
                next_states(&mut cache, &input, chooser)
            })
        } else {
            next_states(&mut cache, &input, None)
        }
        .map_err(|e| ExecError::new("core_error", e.to_string()))?;
        let value = if request.kind.name() == "answer_labels" {
            let label = |l: &meguri_sched::states::labels::AnswerLabel| json!({"amount":l.amount,"less_than":l.less_than,"unit":l.unit.name()});
            json!({"again":label(&states.again.label),"hard":label(&states.hard.label),"good":label(&states.good.label),"easy":label(&states.easy.label)})
        } else {
            json!({"current":state_value(states.current)?,"again":state_value(states.again.state)?,"hard":state_value(states.hard.state)?,"good":state_value(states.good.state)?,"easy":state_value(states.easy.state)?})
        };
        out.insert(reference.to_string(), value);
    }
    Ok(Value::Object(out))
}

fn answer_op(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::Answer(spec) = step else {
        return Err(invalid("expected answer"));
    };
    let id = *session
        .cards
        .get(spec.card.0.index())
        .ok_or_else(|| invalid("unknown answer card"))?;
    let collection = session.engine_collection()?;
    let rating = match spec.rating {
        crate::scenario::Rating::Again => meguri_sched::answer::Rating::Again,
        crate::scenario::Rating::Hard => meguri_sched::answer::Rating::Hard,
        crate::scenario::Rating::Good => meguri_sched::answer::Rating::Good,
        crate::scenario::Rating::Easy => meguri_sched::answer::Rating::Easy,
    };
    let prediction = meguri_engine::sched::answer::predict(
        &collection,
        meguri_model::ids::CardId(id),
        &mut meguri_sched::states::fsrs_glue::FsrsCache::default(),
        session.answer_cache.clone(),
    )
    .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    let mut request = prediction.answer(rating, spec.time_ms);
    // H4 deliberately has no mutable presentation queue or head validation.
    // The kind exists solely for U25/W9's queue-side metadata.
    if request.cache.is_some() {
        request.from_queue = Some(match request.current {
            meguri_sched::states::engine::State::New { .. } => meguri_sched::queue::Kind::New,
            meguri_sched::states::engine::State::Review(_) => meguri_sched::queue::Kind::Review,
            _ => meguri_sched::queue::Kind::Learning,
        });
    }
    meguri_engine::sched::answer::answer(&collection, request)
        .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    Ok(OpResult::Done)
}

fn revlog_observation(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use meguri_storage::raw::SqlValue;
    let collection = session.engine_collection()?;
    let rows = collection
        .reader(meguri_storage::ReaderRole::Query)
        .map_err(|e| ExecError::new("core_error", e.to_string()))?;
    let refs = match &request.args {
        ObservationArgs::Refs(refs) => refs.clone(),
        _ => (0..session.cards.len())
            .map(|i| LogicalRef {
                kind: RefKind::Card,
                n: i as u32 + 1,
            })
            .collect(),
    };
    let mut out = serde_json::Map::new();
    for reference in refs {
        let id = *session
            .cards
            .get(reference.index())
            .ok_or_else(|| invalid("unknown revlog card"))?;
        let raw = rows
            .execute_raw("SELECT * FROM revlog WHERE cid=? ORDER BY id", &[SqlValue::Integer(id)])
            .map_err(|e| ExecError::new("core_error", e.to_string()))?;
        let mut values = Vec::new();
        for row in raw.rows {
            let mut value = serde_json::Map::new();
            for (col, cell) in raw.columns.iter().zip(row) {
                let cell = if col == "cid" {
                    json!(reference.to_string())
                } else {
                    match cell {
                        SqlValue::Integer(v) => json!(v),
                        _ => return Err(invalid("invalid stored revlog column")),
                    }
                };
                value.insert(col.clone(), cell);
            }
            values.push(Value::Object(value));
        }
        out.insert(reference.to_string(), Value::Array(values));
    }
    Ok(Value::Object(out))
}
fn state_value(state: meguri_sched::states::engine::State) -> Result<Value, ExecError> {
    use meguri_sched::states::engine::State;
    let (kind, mut value) = match state {
        State::New { position } => ("new", json!({"position":position})),
        State::Learning(l) => ("learning", learning_value(l)?),
        State::Review(r) => ("review", review_value(r)?),
        State::Relearning { learning, review } => (
            "relearning",
            json!({"learning":learning_value(learning)?,"review":review_value(review)?}),
        ),
    };
    value
        .as_object_mut()
        .expect("state object")
        .insert("kind".into(), json!(kind));
    Ok(value)
}
fn memory_value(value: &mut Value, memory: Option<meguri_model::card_data::MemoryState>) -> Result<(), ExecError> {
    if let Some(memory) = memory {
        value.as_object_mut().expect("state object").insert(
            "memory_state".into(),
            json!({"stability":state_float(memory.stability)?,"difficulty":state_float(memory.difficulty)?}),
        );
    }
    Ok(())
}
fn state_float(value: f32) -> Result<Value, ExecError> {
    observe::f32_value(value).map_err(|e| ExecError::new("core_error", e.to_string()))
}
fn learning_value(l: meguri_sched::states::learning::LearningState) -> Result<Value, ExecError> {
    let mut value =
        json!({"remaining_steps":l.remaining_steps,"scheduled_secs":l.scheduled_secs,"elapsed_secs":l.elapsed_secs});
    memory_value(&mut value, l.memory_state)?;
    Ok(value)
}
fn review_value(r: meguri_sched::states::engine::ReviewState) -> Result<Value, ExecError> {
    let mut value = json!({"scheduled_days":r.scheduled_days,"elapsed_days":r.elapsed_days,"ease_factor":state_float(r.ease_factor)?,"lapses":r.lapses,"leeched":r.leeched});
    memory_value(&mut value, r.memory_state)?;
    Ok(value)
}

/// Q2-Q4: complete typed dispatch. Only deck_counts requests the explicit
/// compatibility prelude. queue/congrats never promote a raw collection or
/// run implicit timing writes. Product reads use the host's QueueProvider.
fn queue_observation(session: &mut Session<'_>, request: &ObservationRequest) -> Result<Value, ExecError> {
    use crate::scenario::ObservationKind;
    use meguri_sched::{
        counts::{deck_tree, finished},
        queue::{self, Environment},
    };
    if request.kind == ObservationKind::Queue {
        let collection = session.engine_collection()?;
        if session.answer_cache_held
            && (session.clock.now_secs() >= session.answer_cache_boundary
                || session
                    .answer_cache
                    .as_ref()
                    .is_some_and(|c| !c.is_current(&collection)))
        {
            if let Some(c) = session.answer_cache.take() {
                c.invalidate();
            }
            session.answer_cache_held = false;
        }
        if !session.answer_cache_held {
            let cache = meguri_engine::sched::answer::AnswerCache::build_scenario_held(&collection)
                .map_err(|e| ExecError::new("core_error", e.to_string()))?;
            // A disabled chooser still registers H3's real-change witness.
            session.answer_cache_boundary = cache.next_day_at();
            session.answer_cache = Some(cache);
            session.answer_cache_held = true;
        }
    }
    if request.kind == ObservationKind::DeckCounts {
        day_query_now(session)?;
    }
    let now = session.clock.now_secs();
    let offset = session.clock.utc_offset_at(session.clock.now_ms());
    let cached = session.day.valid_at(now, offset);
    let core = |e: StorageError| ExecError::new("core_error", e.to_string());
    let collection = session
        .storage
        .collection()
        .ok_or_else(|| invalid("no collection is open"))?;
    collection.with_snapshot(|rows| {
        // The explicit cache is already cleared by raw writes, opens and
        // timing-key writes. A read with no prior query computes day math only.
        let day=if let Some(day)=cached {day} else {
            let config=meguri_sched::model::sched_config::SchedConfig::new(rows);
            meguri_sched::timing::sched_day(&meguri_sched::timing::DayInputs {
                crt:rows.col().map_err(core)?.created.0, creation_offset:config.creation_offset().map_err(core)?,
                rollover_hour:config.rollover_hour().map_err(core)?,offset,now,
            })
        };
        let env=Environment::read(rows,day.today,now,day.next_day_at).map_err(core)?;
        if request.kind==ObservationKind::Queue {
            let q=queue::build(rows,&env,env.current).map_err(core)?;
            let presented=q.presentation();
            let cards=presented.iter().map(|c|session.cards.iter().position(|id|*id==c.id.0)
                .map_or_else(||c.id.0.to_string(),|i|format!("#c{}",i+1))).collect::<Vec<_>>();
            return Ok(json!({"cards":cards,"kinds":presented.iter().map(|c|c.kind.name()).collect::<Vec<_>>(),
                "counts":{"new":q.counts.new,"learning":q.counts.learning,"review":q.counts.review}}));
        }
        let groups=rows.sched_deck_aggregates(env.today,env.now,env.next_day_at,env.ahead).map_err(core)?;
        let list=deck_tree::build(&env,&groups,false);
        if request.kind==ObservationKind::DeckCounts {
            fn node(n:&deck_tree::Node)->Value { json!({"name":n.name,"new":n.counts.new,"learning":n.counts.learning,"review":n.counts.review,"children":n.children.iter().map(node).collect::<Vec<_>>()}) }
            return Ok(Value::Array(list.nodes.iter().map(node).collect()));
        }
        let q=queue::build(rows,&env,env.current).map_err(core)?;
        let tomorrow=deck_tree::build(&env,&groups,true);
        let empty=rows.sched_subtree_empty(&env.subtree(env.current,false)).map_err(core)?;
        let state=finished::build(&env,&q,&list,&tomorrow,&groups,empty);
        Ok(json!({"learn_remaining":state.learning_remaining,"secs_until_next_learn":state.secs_until_next_learn,
            "review_remaining":state.review_remaining,"new_remaining":state.new_remaining,
            "have_sched_buried":state.sibling_buried>0,"have_user_buried":state.user_buried>0}))
    })
}
