//! The parties executor (sync-ops.md S10.3, S10.4): the only executor of
//! actor scenarios. It drives one oracle worker per scenario through an
//! interactive session (`begin`, one `step` per scenario step, `end`). In the
//! replay the worker plays every actor; in a mixed run Meguri's executor plays
//! the Meguri actors and the worker the others. Before the first step it
//! passes every endpoint the worker reports, and the Meguri edge, through
//! [`check_endpoint`] (R1876), so no client connects to an endpoint the guard
//! refuses.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value, json};

use super::meguri::{Actors, MeguriExecutor, NewIds, SyncAccount};
use super::parties_edge::{EdgeLease, unavailable};
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
    #[serde(deserialize_with = "route_object")]
    pub route: Option<Route>,
}

/// Caller-owned mixed route; strict shape, then checked against checkout identity.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Route {
    pub proxy: String,
    pub worker: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Endpoints {
    /// What the oracle actors sync with: the fault proxy, or the server.
    pub actors: String,
    /// The converge client's: the server, on loopback.
    pub converge: String,
    /// The fault proxy's upstream (`host:port`); null without a fault proxy.
    /// Required, like `recorder`: an endpoint the worker leaves out would
    /// escape the check (S8.3).
    #[serde(deserialize_with = "Option::deserialize")]
    pub upstream: Option<String>,
    /// The recorder (`host:port`) in a recording run, else null.
    #[serde(deserialize_with = "Option::deserialize")]
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
    /// Native runner capability, never supplied by scenario data (S8.4).
    pub meguri_edge_cmd: Option<&'a [String]>,
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
        // The guard owns every resource after a successful begin, including
        // validation failures and unwinding. Stop actors, release, then end.
        let mut session = PartySession {
            worker,
            actors,
            lease: None,
            ended: false,
        };
        let result = (|| {
            let sync = begun
                .get("sync")
                .cloned()
                .ok_or_else(|| ExecError::new("protocol_error", "begin reported no sync endpoints"))
                .and_then(sync_info)?;
            check(&sync)?;
            if session.actors.is_some() {
                let route = sync
                    .route
                    .as_ref()
                    .ok_or_else(|| unavailable("mixed begin needs route ownership"))?;
                let expected = session
                    .worker
                    .worker_identity()
                    .filter(|s| lower_hex(s))
                    .ok_or_else(|| unavailable("missing trusted worker identity"))?;
                if route.worker != expected || !proxy_name(&route.proxy) {
                    return Err(unavailable("mixed route ownership mismatch"));
                }
                let command = self
                    .meguri_edge_cmd
                    .ok_or_else(|| unavailable("mixed execution needs an edge helper"))?;
                session.lease = Some(EdgeLease::acquire(command, &sync.endpoints.actors, route, expected)?);
                let lease = session.lease.as_ref().expect("acquired lease");
                lease.check_endpoint()?;
                session
                    .actors
                    .as_mut()
                    .expect("mixed actors")
                    .seat(Some(&lease.endpoint), &sync.account);
            }
            if session.actors.is_none() && sync.route.is_some() {
                return Err(unavailable("replay begin must have null route"));
            }
            table.extend(&ids_in(&begun, "begin")?);
            let mut steps = Steps {
                worker: session.worker.as_mut(),
                actors: session.actors.take(),
                table,
            };
            // Keep actors inside this scope even when a step fails.
            for ((index, step), by) in scenario.steps.iter().enumerate().zip(scenario.step_actors()) {
                steps.step(index, step, by).map_err(|e| e.at_step(index))?;
            }
            Ok(())
        })();
        let ended = session.finish(result)?;
        let decoded = (|| {
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
        })();
        if decoded.is_err() {
            session.worker.quarantine();
        }
        decoded
    }
}

fn route_object<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Option<Route>, D::Error> {
    match Value::deserialize(d)? {
        Value::Null => Ok(None),
        value @ Value::Object(_) => serde_json::from_value(value)
            .map(Some)
            .map_err(serde::de::Error::custom),
        _ => Err(serde::de::Error::custom("route must be an object or null")),
    }
}

fn lower_hex(token: &str) -> bool {
    token.len() == 32 && token.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn proxy_name(name: &str) -> bool {
    name.strip_prefix("meguri-edge-")
        .and_then(|s| s.split_once('-'))
        .is_some_and(|(owner, session)| lower_hex(owner) && lower_hex(session))
}

// Invalid account values must never be interpolated into diagnostics. Keep
// missing endpoint names useful without forwarding serde's offending values.
fn sync_info(value: Value) -> Result<SyncInfo, ExecError> {
    serde_json::from_value(value.clone()).map_err(|_| {
        let missing = ["actors", "converge", "upstream", "recorder"]
            .into_iter()
            .find(|name| value.get("endpoints").and_then(|v| v.get(*name)).is_none());
        let detail = missing.map_or_else(
            || "begin's sync is malformed".to_owned(),
            |name| format!("begin's sync: missing field `{name}`"),
        );
        ExecError::new("protocol_error", detail)
    })
}

/// Every reported endpoint through the guard before allocation (S8.3).
fn check(sync: &SyncInfo) -> Result<(), ExecError> {
    for (what, endpoint) in sync.endpoints() {
        check_endpoint(endpoint)
            .map_err(|_| ExecError::new("endpoint_refused", format!("the {what} endpoint was refused (R1876)")))?;
    }
    Ok(())
}

/// Cleanup is attempted once on normal/error returns; Drop handles unwinding.
struct PartySession<'s, 'a> {
    worker: Box<dyn Interactive + 's>,
    actors: Option<Actors<'a>>,
    lease: Option<EdgeLease<'s>>,
    ended: bool,
}

impl PartySession<'_, '_> {
    fn cleanup(&mut self) -> Result<Value, ExecError> {
        drop(self.actors.take());
        let released = self.lease.take().map_or(Ok(()), |mut lease| lease.release());
        self.ended = true; // Never retry an end or release and hide its first error.
        let ended = self.worker.request(&json!({"cmd": "end"}));
        if released.is_err() || ended.is_err() {
            self.worker.quarantine();
        }
        combine(ended, released)
    }

    fn finish(&mut self, result: Result<(), ExecError>) -> Result<Value, ExecError> {
        let ended = self.cleanup();
        if result.is_err() || ended.is_err() {
            self.worker.quarantine();
        }
        match result {
            Ok(()) => ended,
            Err(error) => combine(Err(error), ended.map(|_| ())),
        }
    }
}

impl Drop for PartySession<'_, '_> {
    fn drop(&mut self) {
        if !self.ended {
            self.worker.quarantine();
            let _ = self.cleanup();
        }
    }
}

fn combine<T>(result: Result<T, ExecError>, cleanup: Result<(), ExecError>) -> Result<T, ExecError> {
    match (result, cleanup) {
        (Ok(value), Ok(())) => Ok(value),
        (Err(mut error), Err(cleanup)) => {
            error.detail.push_str(&format!("; cleanup: {cleanup}"));
            Err(error)
        }
        (Err(error), _) | (_, Err(error)) => Err(error),
    }
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
