//! Meguri's executor: the core in-process, one area adapter at a time (dsl.md
//! D8). Each area registers its setup sections, ops and observation kinds in
//! `MeguriExecutor::new` from its own module (`exec/<area>.rs`); a scenario
//! that needs anything unregistered is pending, never run.

use std::collections::BTreeMap;
use std::sync::Arc;

use meguri_model::rng::SeededRandom;
use serde_json::{Map, Value};

use super::clock::ScenarioClock;
use super::{ExecError, Executor, Needs};
use crate::scenario::{LoadedScenario, ObservationRequest, Setup, Step};

/// Runs one op step: `Some(output)` is recorded as `{<op>: output}`.
pub type OpFn = for<'a> fn(&mut Session<'a>, &Step) -> Result<Option<Value>, ExecError>;
/// Produces one observation's value.
pub type ObserveFn = for<'a> fn(&mut Session<'a>, &ObservationRequest) -> Result<Value, ExecError>;
/// Runs one setup section.
pub type SetupFn = for<'a> fn(&mut Session<'a>, &Setup) -> Result<(), ExecError>;

/// Adapters by setup section, op and observation kind: the executor's
/// capabilities.
#[derive(Debug, Default)]
pub struct Registry {
    setup: BTreeMap<&'static str, SetupFn>,
    ops: BTreeMap<&'static str, OpFn>,
    observations: BTreeMap<&'static str, ObserveFn>,
}

impl Registry {
    pub fn setup(&mut self, section: &'static str, f: SetupFn) {
        assert!(
            self.setup.insert(section, f).is_none(),
            "setup section {section} registered twice"
        );
    }

    pub fn op(&mut self, name: &'static str, f: OpFn) {
        assert!(self.ops.insert(name, f).is_none(), "op {name} registered twice");
    }

    pub fn observation(&mut self, kind: &'static str, f: ObserveFn) {
        assert!(
            self.observations.insert(kind, f).is_none(),
            "observation {kind} registered twice"
        );
    }

    /// Everything registered.
    pub fn capabilities(&self) -> Needs {
        Needs {
            setup: self.setup.keys().map(|s| (*s).to_owned()).collect(),
            ops: self.ops.keys().map(|s| (*s).to_owned()).collect(),
            observations: self.observations.keys().map(|s| (*s).to_owned()).collect(),
        }
    }
}

/// One scenario run: the scenario, its clock and random source, and the real
/// ids behind the logical ones in creation order. Areas add the state they
/// need (the collection handle, ...) as they land.
#[derive(Debug)]
pub struct Session<'a> {
    pub scenario: &'a LoadedScenario,
    pub clock: Arc<ScenarioClock>,
    pub random: Arc<SeededRandom>,
    pub notes: Vec<i64>,
    pub cards: Vec<i64>,
}

/// The seed of a scenario's random source: the first eight bytes of its hash,
/// big-endian (dsl.md D5).
pub fn seed(scenario: &LoadedScenario) -> u64 {
    scenario
        .sha256
        .get(..16)
        .and_then(|hex| u64::from_str_radix(hex, 16).ok())
        .unwrap_or(0)
}

#[derive(Debug)]
pub struct MeguriExecutor {
    registry: Registry,
}

impl MeguriExecutor {
    /// The executor with every area adapter that exists.
    pub fn new() -> MeguriExecutor {
        let mut registry = Registry::default();
        super::clock::register(&mut registry);
        MeguriExecutor { registry }
    }

    pub fn capabilities(&self) -> Needs {
        self.registry.capabilities()
    }
}

impl Default for MeguriExecutor {
    fn default() -> MeguriExecutor {
        MeguriExecutor::new()
    }
}

impl Executor for MeguriExecutor {
    fn name(&self) -> &str {
        "meguri"
    }

    fn missing(&self, needs: &Needs) -> Needs {
        let have = self.registry.capabilities();
        Needs {
            setup: needs.setup.difference(&have.setup).cloned().collect(),
            ops: needs.ops.difference(&have.ops).cloned().collect(),
            observations: needs.observations.difference(&have.observations).cloned().collect(),
        }
    }

    fn run(&self, loaded: &LoadedScenario) -> Result<Value, ExecError> {
        let scenario = &loaded.scenario;
        let clock = ScenarioClock::new(&scenario.clock.tz, scenario.clock.start.as_str())
            .map_err(|e| ExecError::new("invalid_scenario", e.to_string()))?;
        let mut session = Session {
            scenario: loaded,
            clock: Arc::new(clock),
            random: Arc::new(SeededRandom::new(seed(loaded))),
            notes: Vec::new(),
            cards: Vec::new(),
        };
        for section in scenario.setup.sections() {
            let f = self
                .registry
                .setup
                .get(section)
                .ok_or_else(|| ExecError::new("unsupported_setup", format!("no adapter for setup.{section}")))?;
            f(&mut session, &scenario.setup)?;
        }
        let mut outputs = Vec::with_capacity(scenario.steps.len());
        for (index, step) in scenario.steps.iter().enumerate() {
            let output = self.step(&mut session, step).map_err(|e| e.at_step(index))?;
            outputs.push(output);
        }
        let mut observations = Map::new();
        observations.insert("steps".to_owned(), Value::Array(outputs));
        Ok(Value::Object(observations))
    }
}

impl MeguriExecutor {
    fn step(&self, session: &mut Session<'_>, step: &Step) -> Result<Value, ExecError> {
        let mut out = Map::new();
        match step {
            Step::Observe(requests) => {
                for request in requests {
                    let kind = request.kind.name();
                    let f = self.registry.observations.get(kind).ok_or_else(|| {
                        ExecError::new("unsupported_observation", format!("no adapter for observation {kind}"))
                    })?;
                    let value = f(session, request)?;
                    merge(&mut out, kind, value)?;
                }
            }
            other => {
                let name = other.name();
                let f = self
                    .registry
                    .ops
                    .get(name)
                    .ok_or_else(|| ExecError::new("unsupported_op", format!("no adapter for op {name}")))?;
                if let Some(output) = f(session, other)? {
                    out.insert(name.to_owned(), output);
                }
            }
        }
        Ok(Value::Object(out))
    }
}

/// Two requests of one kind in a step merge when they name different targets
/// (observations.md O2).
fn merge(out: &mut Map<String, Value>, kind: &str, value: Value) -> Result<(), ExecError> {
    let twice = || {
        ExecError::new(
            "invalid_scenario",
            format!("observation {kind} is requested twice for the same target"),
        )
    };
    match (out.get_mut(kind), value) {
        (None, value) => {
            out.insert(kind.to_owned(), value);
            Ok(())
        }
        (Some(Value::Object(existing)), Value::Object(more)) => {
            for (key, v) in more {
                if existing.insert(key, v).is_some() {
                    return Err(twice());
                }
            }
            Ok(())
        }
        _ => Err(twice()),
    }
}
