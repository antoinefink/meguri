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
use sha2::{Digest, Sha256};

use super::clock::ScenarioClock;
use super::{ExecError, Executor, Handoff, Needs, SegmentExecutor, SegmentOutput, SegmentRequest};
use crate::scenario::{ActorName, LoadedScenario, NEW_COLLECTION, ObservationRequest, Setup, Step};

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
/// Runs when a run or the last segment of a chain ends on Meguri, after its
/// steps: the checks an area makes over the whole run (the storage area's
/// schema guard).
pub type FinishFn = for<'a> fn(&mut Session<'a>) -> Result<(), ExecError>;
/// The raw dump of a Meguri actor's collection (sync-ops.md S7.2): the
/// `db_dump` value of all twelve tables without O8's normalization of random
/// values.
pub type RawDumpFn = for<'a> fn(&mut Session<'a>) -> Result<Value, ExecError>;
/// Makes the session's new collection at the session clock's instant, which
/// is `clock.start` (dsl.md D6, D8), before the setup runs.
pub type NewCollectionFn = for<'a> fn(&mut Session<'a>) -> Result<(), ExecError>;

#[derive(Debug, Clone, Copy)]
struct OpAdapter {
    run: OpFn,
    rejects: &'static [&'static str],
}

/// Adapters by setup section, op and observation kind: the executor's
/// capabilities.
#[derive(Debug, Default)]
pub struct Registry {
    new_collection: Option<NewCollectionFn>,
    setup: BTreeMap<&'static str, SetupFn>,
    ops: BTreeMap<&'static str, OpAdapter>,
    observations: BTreeMap<&'static str, ObserveFn>,
    handoff: Option<(HandoffFn, HandoffFn)>,
    actor: Option<RawDumpFn>,
    finish: Vec<FinishFn>,
}

impl Registry {
    /// What makes the new collection a scenario starts from (dsl.md D8):
    /// U28's bootstrap, or before it an area's stand-in. It counts as the
    /// setup capability `new_collection`.
    pub fn new_collection(&mut self, f: NewCollectionFn) {
        assert!(self.new_collection.is_none(), "new_collection registered twice");
        self.new_collection = Some(f);
    }

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
            Step::NAMES.contains(&name) && !matches!(name, "observe" | "handoff" | "actor"),
            "op {name} is not in the language"
        );
        // The worker runs these for every actor (sync-ops.md S10.2).
        assert!(
            !matches!(name, "net_fault" | "converge"),
            "op {name} runs on the oracle worker, never on Meguri"
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

    /// A check that runs when a run on Meguri ends, after its steps.
    pub fn finish(&mut self, f: FinishFn) {
        self.finish.push(f);
    }

    /// What makes Meguri an actor of actor scenarios, through the parties
    /// executor (sync-ops.md S10.2, S10.4): each Meguri actor is a session
    /// with a collection of its own and its seat (`Session::actor`), and
    /// `raw_dump` gives its collection's raw dump for `converge`.
    pub fn actor(&mut self, raw_dump: RawDumpFn) {
        assert!(self.actor.is_none(), "actor registered twice");
        self.actor = Some(raw_dump);
    }

    /// Everything registered; the handoff adapters count as the `handoff` op,
    /// the actor adapter as the `actor` op and the new collection as the
    /// setup capability `new_collection`.
    pub fn capabilities(&self) -> Needs {
        let mut ops: BTreeSet<String> = self.ops.keys().map(|s| (*s).to_owned()).collect();
        if self.handoff.is_some() {
            ops.insert("handoff".to_owned());
        }
        if self.actor.is_some() {
            ops.insert("actor".to_owned());
        }
        let mut setup: BTreeSet<String> = self.setup.keys().map(|s| (*s).to_owned()).collect();
        if self.new_collection.is_some() {
            setup.insert(NEW_COLLECTION.to_owned());
        }
        Needs {
            setup,
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
    /// The actor this session plays in a mixed run of an actor scenario;
    /// `None` elsewhere.
    pub actor: Option<Seat>,
    /// The session directory and the open collection (`exec/storage.rs`).
    pub storage: super::storage::Storage,
    /// The day clock's cached day (`sched/day-clock` D6; `exec/sched.rs`).
    pub day: meguri_engine::DayCache,
}

/// A Meguri actor's place in a mixed run (sync-ops.md S10.4): its name, the
/// endpoint it syncs with (the Meguri edge, S8.1; `None` while the runner
/// has none) and the scenario's account, test credentials the worker
/// reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seat {
    pub name: ActorName,
    pub endpoint: Option<String>,
    pub account: SyncAccount,
}

/// The scenario's sync account (protocol.json `sync_info.account`).
#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SyncAccount {
    pub username: String,
    pub password: String,
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

/// The seed of a Meguri actor's random source (sync-ops.md S2.5): the first
/// eight bytes, big-endian, of the SHA-256 of `<scenario_sha256>/<actor>`, so
/// two Meguri actors never share a sequence.
pub fn actor_seed(scenario: &LoadedScenario, actor: &str) -> u64 {
    let digest = Sha256::digest(format!("{}/{actor}", scenario.sha256).as_bytes());
    let mut first = [0u8; 8];
    first.copy_from_slice(&digest[..8]);
    u64::from_be_bytes(first)
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

    /// The areas that have adapters, by name.
    pub const AREAS: [&'static str; 8] = [
        "browser", "clock", "render", "sched", "search", "storage", "tags", "text",
    ];

    /// Every area adapter that exists.
    pub fn core_registry() -> Registry {
        MeguriExecutor::area_registry(&MeguriExecutor::AREAS)
    }

    /// The adapters of these areas only (runner tests that stand in for an
    /// area, such as the storage area's handoff).
    ///
    /// # Panics
    ///
    /// On a name that is not in [`MeguriExecutor::AREAS`].
    pub fn area_registry(areas: &[&str]) -> Registry {
        let mut registry = Registry::default();
        for area in areas {
            match *area {
                "browser" => super::browser::register(&mut registry),
                "clock" => super::clock::register(&mut registry),
                "render" => super::render::register(&mut registry),
                "sched" => super::sched::register(&mut registry),
                "search" => super::search::register(&mut registry),
                "storage" => super::storage::register(&mut registry),
                "tags" => super::tags::register(&mut registry),
                "text" => super::text::register(&mut registry),
                other => panic!("no area {other} has Meguri adapters"),
            }
        }
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
            actor: None,
            storage: Default::default(),
            day: Default::default(),
        })
    }

    /// D8: the new collection, when the scenario starts from one and an
    /// adapter makes it.
    fn new_collection(&self, session: &mut Session<'_>) -> Result<(), ExecError> {
        match self.registry.new_collection {
            Some(f) => f(session),
            None => Ok(()),
        }
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

    fn finish(&self, session: &mut Session<'_>) -> Result<(), ExecError> {
        self.registry.finish.iter().try_for_each(|f| f(session))
    }

    fn step(&self, session: &mut Session<'_>, step: &Step) -> Result<Value, ExecError> {
        // storage-ops.md P1.2, P1.3: a scenario that opens fixtures starts
        // with no collection, and a refused open leaves none; only `open`
        // may run then.
        if session.scenario.scenario.opens_fixtures() && !session.storage.is_open() && !matches!(step, Step::Open(_)) {
            return Err(ExecError::new(
                "invalid_scenario",
                format!(
                    "{} with no collection open: only open may follow a refused open",
                    step.name()
                ),
            ));
        }
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

/// Real ids a step or a setup created, in creation order
/// (protocol.json `new_ids`).
#[derive(Debug, Clone, Default, PartialEq, Eq, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
pub struct NewIds {
    pub notes: Vec<i64>,
    pub cards: Vec<i64>,
}

impl NewIds {
    pub fn extend(&mut self, more: &NewIds) {
        self.notes.extend_from_slice(&more.notes);
        self.cards.extend_from_slice(&more.cards);
    }
}

/// The Meguri actors of a mixed run (sync-ops.md S10.4): one session each,
/// with its own random source (S2.5) and one clock they share, as every
/// actor and the server share the scenario clock.
pub(super) struct Actors<'a> {
    executor: &'a MeguriExecutor,
    raw_dump: RawDumpFn,
    sessions: BTreeMap<ActorName, Session<'a>>,
}

impl MeguriExecutor {
    /// Sessions for these actors of an actor scenario, each with its new
    /// collection made at `clock.start` (sync-ops.md S2.2, dsl.md D8);
    /// `unsupported_op` when Meguri cannot play actors.
    pub(super) fn actors<'a>(
        &'a self,
        loaded: &'a LoadedScenario,
        names: &[ActorName],
    ) -> Result<Actors<'a>, ExecError> {
        let raw_dump = self
            .registry
            .actor
            .ok_or_else(|| ExecError::new("unsupported_op", "no adapter for op actor"))?;
        let first = self.session(loaded, None)?;
        let clock = Arc::clone(&first.clock);
        let mut sessions = BTreeMap::new();
        for name in names {
            let mut session = Session {
                scenario: loaded,
                clock: Arc::clone(&clock),
                random: Arc::new(SeededRandom::new(actor_seed(loaded, name.as_str()))),
                notes: Vec::new(),
                cards: Vec::new(),
                actor: None,
                storage: Default::default(),
                day: Default::default(),
            };
            self.new_collection(&mut session)?;
            sessions.insert(name.clone(), session);
        }
        Ok(Actors {
            executor: self,
            raw_dump,
            sessions,
        })
    }
}

impl<'a> Actors<'a> {
    fn session(&mut self, actor: &ActorName) -> Result<&mut Session<'a>, ExecError> {
        self.sessions
            .get_mut(actor)
            .ok_or_else(|| ExecError::new("internal_error", format!("Meguri does not play {actor}")))
    }

    /// The actors, in name order.
    pub(super) fn names(&self) -> Vec<ActorName> {
        self.sessions.keys().cloned().collect()
    }

    /// Gives every actor its seat once the worker has reported the account.
    pub(super) fn seat(&mut self, endpoint: Option<&str>, account: &SyncAccount) {
        for (name, session) in &mut self.sessions {
            session.actor = Some(Seat {
                name: name.clone(),
                endpoint: endpoint.map(str::to_owned),
                account: account.clone(),
            });
        }
    }

    /// Runs the setup on this actor's new collection; the ids it created.
    pub(super) fn setup(&mut self, actor: &ActorName) -> Result<NewIds, ExecError> {
        let executor = self.executor;
        let session = self.session(actor)?;
        executor.run_setup(session)?;
        Ok(NewIds {
            notes: session.notes.clone(),
            cards: session.cards.clone(),
        })
    }

    /// Runs one step on this actor, with the scenario's id table so far (all
    /// actors' ids, S2.4); its output and the ids it created.
    pub(super) fn step(
        &mut self,
        actor: &ActorName,
        step: &Step,
        table: &NewIds,
    ) -> Result<(Value, NewIds), ExecError> {
        let executor = self.executor;
        let session = self.session(actor)?;
        session.notes.clone_from(&table.notes);
        session.cards.clone_from(&table.cards);
        let output = executor.step(session, step)?;
        let created = NewIds {
            notes: session.notes.get(table.notes.len()..).unwrap_or_default().to_vec(),
            cards: session.cards.get(table.cards.len()..).unwrap_or_default().to_vec(),
        };
        Ok((output, created))
    }

    /// Applies `advance` or `set_clock` to the actors' shared clock.
    pub(super) fn clock(&mut self, step: &Step) -> Result<(), ExecError> {
        let executor = self.executor;
        if let Some(session) = self.sessions.values_mut().next() {
            executor.step(session, step)?;
        }
        Ok(())
    }

    /// This actor's raw dump (S7.2).
    pub(super) fn raw_dump(&mut self, actor: &ActorName) -> Result<Value, ExecError> {
        let raw_dump = self.raw_dump;
        raw_dump(self.session(actor)?)
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
        if loaded.scenario.has_actors() {
            return Err(ExecError::new(
                "invalid_scenario",
                "actor scenarios run only in the parties executor (sync-ops.md S10.3; tools/mg conformance --live)",
            ));
        }
        let mut session = self.session(loaded, None)?;
        if loaded.scenario.starts_from_new_collection() {
            self.new_collection(&mut session)?;
        }
        self.run_setup(&mut session)?;
        let outputs = self.run_steps(&mut session, 0..loaded.scenario.steps.len())?;
        self.finish(&mut session)?;
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
            None => {
                // D9.2: the first segment starts from a new collection unless
                // it opens a fixture.
                if !loaded.scenario.starts_with_open() {
                    self.new_collection(&mut session)?;
                }
                self.run_setup(&mut session)?;
            }
            Some(handoff) => into(&mut session, &request.root.join(&handoff.dir))?,
        }
        let steps = self.run_steps(&mut session, request.steps.clone())?;
        let handoff = match &request.hand_off_to {
            None => {
                self.finish(&mut session)?;
                None
            }
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
