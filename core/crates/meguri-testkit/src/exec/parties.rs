//! The parties executor (sync-ops.md S10.3, S10.4): the only executor of
//! actor scenarios. It drives one oracle worker per scenario through an
//! interactive session (`begin`, one `step` per scenario step, `end`). In the
//! replay the worker plays every actor; in a mixed run Meguri's executor plays
//! the Meguri actors and the worker the others. Before the first step it
//! passes every endpoint the worker reports, and the Meguri edge, through
//! [`check_endpoint`] (R1876), so no client connects to an endpoint the guard
//! refuses.

use serde::Deserialize;
use serde_json::{Map, Value, json};

use super::meguri::{Actors, MeguriExecutor, NewIds, SyncAccount};
use super::sync_guard::check_endpoint;
use super::{ExecError, Interactive, SegmentExecutor};
use crate::scenario::{ActorName, LoadedScenario, Step};

/// The run's endpoints and account a worker reports when an actor scenario's
/// session begins (protocol.json `sync_info`).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncInfo {
    pub endpoints: Endpoints,
    pub account: SyncAccount,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    /// What the oracle actors sync with: the fault proxy, or the server.
    pub actors: String,
    /// The converge client's: the server, on loopback.
    pub converge: String,
    /// The fault proxy's upstream (`host:port`); `None` without a fault proxy.
    pub upstream: Option<String>,
    /// The recorder (`host:port`) in a recording run.
    pub recorder: Option<String>,
}

impl SyncInfo {
    /// Every endpoint the check covers (S8.3), named for reports.
    fn endpoints(&self) -> Vec<(&'static str, &str)> {
        let e = &self.endpoints;
        let mut out = vec![("actors", e.actors.as_str()), ("converge", e.converge.as_str())];
        out.extend(e.upstream.as_deref().map(|u| ("upstream", u)));
        out.extend(e.recorder.as_deref().map(|r| ("recorder", r)));
        out
    }
}

pub struct Parties<'a> {
    pub oracle: &'a dyn SegmentExecutor,
    pub meguri: &'a MeguriExecutor,
    /// The Meguri edge (S8.1): the endpoint on the runner's host through
    /// which Meguri actors reach the fault proxy. `None` until the first
    /// Meguri sync adapter wires it (U105).
    pub meguri_edge: Option<&'a str>,
}

impl Parties<'_> {
    /// The replay (S10.3): the oracle plays every actor. `{"steps": [...]}`.
    pub fn replay(&self, loaded: &LoadedScenario) -> Result<Value, ExecError> {
        self.run(loaded, &[])
    }

    /// The mixed run (S10.3): Meguri's executor plays the Meguri actors, the
    /// oracle the others. `{"steps": [...]}`.
    pub fn mixed(&self, loaded: &LoadedScenario) -> Result<Value, ExecError> {
        let mine: Vec<ActorName> = loaded
            .scenario
            .actors()
            .iter()
            .filter(|a| a.is_meguri())
            .cloned()
            .collect();
        if mine.is_empty() {
            return Err(ExecError::new(
                "invalid_scenario",
                "a mixed run needs a Meguri actor (sync-ops.md S10.3)",
            ));
        }
        self.run(loaded, &mine)
    }

    fn run(&self, loaded: &LoadedScenario, mine: &[ActorName]) -> Result<Value, ExecError> {
        let scenario = &loaded.scenario;
        if !scenario.has_actors() {
            return Err(ExecError::new(
                "invalid_scenario",
                "the parties executor runs actor scenarios only (sync-ops.md S10.3)",
            ));
        }
        let mut actors = if mine.is_empty() {
            None
        } else {
            Some(self.meguri.actors(loaded, mine)?)
        };
        let mut table = NewIds::default();
        let mut begin = json!({"cmd": "begin", "id": scenario.id, "scenario": loaded.value});
        if let Some(actors) = actors.as_mut() {
            begin["caller_actors"] = json!(mine);
            let first = &scenario.actors()[0];
            if mine.contains(first) {
                // The caller runs the setup first and hands its ids over.
                table = actors.setup(first)?;
                begin["ids"] = to_value(&table)?;
            }
        }

        let mut worker = self.oracle.interactive(&scenario.clock.tz)?;
        let begun = worker.request(&begin)?;
        let sync: SyncInfo = begun
            .get("sync")
            .cloned()
            .ok_or_else(|| ExecError::new("protocol_error", "begin reported no sync endpoints"))
            .and_then(|v| from_value(v, "begin's sync"))?;
        let edge = actors.as_ref().and(self.meguri_edge);
        if let Err(refused) = check(&sync, edge) {
            // No step has run, so nothing has connected (S8.3); `end` stops
            // the server before the session is reported.
            let _ = worker.request(&json!({"cmd": "end"}));
            return Err(refused);
        }
        table.extend(&ids_in(&begun, "begin")?);
        if let Some(actors) = actors.as_mut() {
            actors.seat(edge, &sync.account);
        }

        let mut session = Steps {
            worker: worker.as_mut(),
            actors,
            table,
        };
        for ((index, step), by) in scenario.steps.iter().enumerate().zip(scenario.step_actors()) {
            session.step(index, step, by).map_err(|e| e.at_step(index))?;
        }
        let ended = worker.request(&json!({"cmd": "end"}))?;
        let steps = ended
            .pointer("/observations/steps")
            .and_then(Value::as_array)
            .cloned()
            .ok_or_else(|| ExecError::new("protocol_error", "end returned no observations"))?;
        if steps.len() != scenario.steps.len() {
            return Err(ExecError::new(
                "protocol_error",
                format!(
                    "end returned {} outputs for {} steps",
                    steps.len(),
                    scenario.steps.len()
                ),
            ));
        }
        Ok(json!({"steps": steps}))
    }
}

/// Every reported endpoint, and the Meguri edge when Meguri plays an actor,
/// through the guard (S8.3). A refusal is `endpoint_refused`.
fn check(sync: &SyncInfo, edge: Option<&str>) -> Result<(), ExecError> {
    let mut endpoints = sync.endpoints();
    endpoints.extend(edge.map(|e| ("meguri edge", e)));
    for (what, endpoint) in endpoints {
        check_endpoint(endpoint)
            .map_err(|refused| ExecError::new("endpoint_refused", format!("the {what} endpoint: {refused}")))?;
    }
    Ok(())
}

/// The steps of one session: which side runs each, and the id table they
/// share (S2.4).
struct Steps<'s, 'a> {
    worker: &'s mut dyn Interactive,
    actors: Option<Actors<'a>>,
    table: NewIds,
}

impl Steps<'_, '_> {
    fn step(&mut self, index: usize, step: &Step, by: Option<&ActorName>) -> Result<(), ExecError> {
        let mut request = Map::new();
        request.insert("cmd".to_owned(), json!("step"));
        let mut done = None;
        if let Some(actors) = self.actors.as_mut() {
            match step {
                Step::Advance(_) | Step::SetClock(_) => actors.clock(step)?,
                Step::Converge(_) => {
                    let mut dumps = Map::new();
                    for name in actors.names() {
                        dumps.insert(name.to_string(), actors.raw_dump(&name)?);
                    }
                    request.insert("dumps".to_owned(), Value::Object(dumps));
                }
                _ => {}
            }
            if let Some(actor) = by.filter(|a| a.is_meguri()) {
                let (output, created) = actors.step(actor, step, &self.table)?;
                request.insert(
                    "done".to_owned(),
                    json!({"output": output, "notes": created.notes, "cards": created.cards}),
                );
                done = Some(created);
            }
        }
        let response = self.worker.request(&Value::Object(request))?;
        if response.get("index").and_then(Value::as_u64) != Some(index as u64) {
            return Err(ExecError::new(
                "protocol_error",
                format!("the step response has index {:?}", response.get("index")),
            ));
        }
        let created = ids_in(&response, "step")?;
        if let Some(mine) = done
            && mine != created
        {
            return Err(ExecError::new(
                "protocol_error",
                format!("the worker recorded ids {created:?} for a step that created {mine:?}"),
            ));
        }
        self.table.extend(&created);
        Ok(())
    }
}

/// The `ids` member of a response; none when it has none.
fn ids_in(response: &Value, what: &str) -> Result<NewIds, ExecError> {
    match response.get("ids") {
        None => Ok(NewIds::default()),
        Some(ids) => from_value(ids.clone(), &format!("{what}'s ids")),
    }
}

fn from_value<T: serde::de::DeserializeOwned>(value: Value, what: &str) -> Result<T, ExecError> {
    serde_json::from_value(value).map_err(|e| ExecError::new("protocol_error", format!("{what}: {e}")))
}

fn to_value<T: serde::Serialize>(value: &T) -> Result<Value, ExecError> {
    serde_json::to_value(value).map_err(|e| ExecError::new("internal_error", e.to_string()))
}
