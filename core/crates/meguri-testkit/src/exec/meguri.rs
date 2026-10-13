//! Meguri's executor: the core in-process, one area adapter at a time (dsl.md
//! D8). Each area registers its setup sections, ops and observation kinds in
//! `MeguriExecutor::core_registry` from its own module (`exec/<area>.rs`); a
//! scenario that needs anything unregistered is pending, never run. The area
//! that owns the collection files registers the handoff adapters live chains
//! need (dsl.md D9).

use std::collections::{BTreeMap, BTreeSet};
use std::ops::Range;
use std::path::Path;
use std::sync::Arc;

use meguri_model::rng::SeededRandom;
use serde_json::{Map, Value, json};

use super::clock::ScenarioClock;
use super::{ExecError, Executor, Handoff, Needs, SegmentExecutor, SegmentOutput, SegmentRequest};
use crate::scenario::{LoadedScenario, ObservationRequest, Setup, Step};

/// What an op adapter reports (dsl.md D7.2, D7.3).
#[derive(Debug, Clone, PartialEq)]
pub enum OpResult {
    /// Recorded as `{}`.
    Done,
    /// Recorded as `{<op>: output}`.
    Output(Value),
    /// Anki refuses the op: recorded as `{<op>: {"error": kind}}`, nothing
    /// changes and the run goes on. `kind` must be one the op declared.
    Rejected(&'static str),
}

/// Runs one op step.
pub type OpFn = for<'a> fn(&mut Session<'a>, &Step) -> Result<OpResult, ExecError>;
/// Produces one observation's value.
pub type ObserveFn = for<'a> fn(&mut Session<'a>, &ObservationRequest) -> Result<Value, ExecError>;
/// Runs one setup section.
pub type SetupFn = for<'a> fn(&mut Session<'a>, &Setup) -> Result<(), ExecError>;
/// Writes the session's collection and files into a handoff directory (out),
/// or opens the ones handed over in it (in). The executor carries the clock
/// and the logical ids itself.
pub type HandoffFn = for<'a> fn(&mut Session<'a>, &Path) -> Result<(), ExecError>;

#[derive(Debug, Clone, Copy)]
struct OpAdapter {
    run: OpFn,
    rejects: &'static [&'static str],
}

/// Adapters by setup section, op and observation kind: the executor's
/// capabilities.
#[derive(Debug, Default)]
pub struct Registry {
    setup: BTreeMap<&'static str, SetupFn>,
    ops: BTreeMap<&'static str, OpAdapter>,
    observations: BTreeMap<&'static str, ObserveFn>,
    handoff: Option<(HandoffFn, HandoffFn)>,
}

impl Registry {
    pub fn setup(&mut self, section: &'static str, f: SetupFn) {
        assert!(
            Setup::ORDER.contains(&section),
            "setup.{section} is not in the language"
        );
        assert!(
            self.setup.insert(section, f).is_none(),
            "setup section {section} registered twice"
        );
    }

    /// An op that Anki never refuses.
    pub fn op(&mut self, name: &'static str, f: OpFn) {
        self.op_rejecting(name, &[], f);
    }

    /// An op and the rejection kinds it may record (its closed list in
    /// dsl.md D7.2).
    pub fn op_rejecting(&mut self, name: &'static str, rejects: &'static [&'static str], f: OpFn) {
        assert!(
            Step::NAMES.contains(&name) && !matches!(name, "observe" | "handoff"),
            "op {name} is not in the language"
        );
        let adapter = OpAdapter { run: f, rejects };
        assert!(self.ops.insert(name, adapter).is_none(), "op {name} registered twice");
    }

    pub fn observation(&mut self, kind: &'static str, f: ObserveFn) {
        assert!(
            self.observations.insert(kind, f).is_none(),
            "observation {kind} registered twice"
        );
    }

    /// The handoff adapters: what makes Meguri a side of live chains.
    pub fn handoff(&mut self, out: HandoffFn, into: HandoffFn) {
        assert!(self.handoff.is_none(), "handoff registered twice");
        self.handoff = Some((out, into));
    }

    /// Everything registered; the handoff adapters count as the `handoff` op.
    pub fn capabilities(&self) -> Needs {
        let mut ops: BTreeSet<String> = self.ops.keys().map(|s| (*s).to_owned()).collect();
        if self.handoff.is_some() {
            ops.insert("handoff".to_owned());
        }
        Needs {
            setup: self.setup.keys().map(|s| (*s).to_owned()).collect(),
            ops,
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
        MeguriExecutor::with(MeguriExecutor::core_registry())
    }

    /// Every area adapter that exists.
    pub fn core_registry() -> Registry {
        let mut registry = Registry::default();
        super::clock::register(&mut registry);
        super::text::register(&mut registry);
        registry
    }

    /// An executor with these adapters (runner tests, area prototypes).
    pub fn with(registry: Registry) -> MeguriExecutor {
        MeguriExecutor { registry }
    }

    pub fn capabilities(&self) -> Needs {
        self.registry.capabilities()
    }

    /// A session at the scenario's start, or at a handoff's instant and ids.
    fn session<'a>(&self, loaded: &'a LoadedScenario, resume: Option<&Handoff>) -> Result<Session<'a>, ExecError> {
        let scenario = &loaded.scenario;
        let clock = ScenarioClock::new(&scenario.clock.tz, scenario.clock.start.as_str())
            .map_err(|e| ExecError::new("invalid_scenario", e.to_string()))?;
        if let Some(handoff) = resume {
            clock.set_secs(handoff.now);
        }
        Ok(Session {
            scenario: loaded,
            clock: Arc::new(clock),
            random: Arc::new(SeededRandom::new(seed(loaded))),
            notes: resume.map(|h| h.notes.clone()).unwrap_or_default(),
            cards: resume.map(|h| h.cards.clone()).unwrap_or_default(),
        })
    }

    fn run_setup(&self, session: &mut Session<'_>) -> Result<(), ExecError> {
        let setup = &session.scenario.scenario.setup;
        for section in setup.sections() {
            let f = self
                .registry
                .setup
                .get(section)
                .ok_or_else(|| ExecError::new("unsupported_setup", format!("no adapter for setup.{section}")))?;
            f(session, setup)?;
        }
        Ok(())
    }

    fn run_steps(&self, session: &mut Session<'_>, steps: Range<usize>) -> Result<Vec<Value>, ExecError> {
        let scenario = session.scenario;
        let mut outputs = Vec::with_capacity(steps.len());
        for index in steps {
            let step = scenario
                .scenario
                .steps
                .get(index)
                .ok_or_else(|| ExecError::new("invalid_scenario", format!("no step {index}")))?;
            outputs.push(self.step(session, step).map_err(|e| e.at_step(index))?);
        }
        Ok(outputs)
    }

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
            Step::Handoff(_) => {
                return Err(ExecError::new(
                    "invalid_scenario",
                    "a handoff ends a segment; live scenarios run as chains (tools/mg conformance --live)",
                ));
            }
            other => {
                let name = other.name();
                let adapter = self
                    .registry
                    .ops
                    .get(name)
                    .ok_or_else(|| ExecError::new("unsupported_op", format!("no adapter for op {name}")))?;
                match (adapter.run)(session, other)? {
                    OpResult::Done => {}
                    OpResult::Output(output) => {
                        out.insert(name.to_owned(), output);
                    }
                    OpResult::Rejected(kind) if adapter.rejects.contains(&kind) => {
                        out.insert(name.to_owned(), json!({"error": kind}));
                    }
                    OpResult::Rejected(kind) => {
                        return Err(ExecError::new(
                            "internal_error",
                            format!("op {name} recorded rejection {kind}, which is not in its list"),
                        ));
                    }
                }
            }
        }
        Ok(Value::Object(out))
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
        let mut session = self.session(loaded, None)?;
        self.run_setup(&mut session)?;
        let outputs = self.run_steps(&mut session, 0..loaded.scenario.steps.len())?;
        Ok(json!({"steps": outputs}))
    }
}

impl SegmentExecutor for MeguriExecutor {
    fn segment(&self, loaded: &LoadedScenario, request: &SegmentRequest) -> Result<SegmentOutput, ExecError> {
        let (out, into) = self
            .registry
            .handoff
            .ok_or_else(|| ExecError::new("unsupported_op", "no adapter for op handoff"))?;
        let mut session = self.session(loaded, request.resume.as_ref())?;
        match &request.resume {
            None => self.run_setup(&mut session)?,
            Some(handoff) => into(&mut session, &request.root.join(&handoff.dir))?,
        }
        let steps = self.run_steps(&mut session, request.steps.clone())?;
        let handoff = match &request.hand_off_to {
            None => None,
            Some(rel) => {
                let dir = request.root.join(rel);
                std::fs::create_dir_all(&dir)
                    .map_err(|e| ExecError::new("handoff_error", format!("creating {}: {e}", dir.display())))?;
                out(&mut session, &dir)?;
                Some(Handoff {
                    dir: rel.clone(),
                    now: session.clock.now_secs(),
                    notes: session.notes.clone(),
                    cards: session.cards.clone(),
                })
            }
        };
        Ok(SegmentOutput { steps, handoff })
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
