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
    /// Finer capabilities an op's spec splits it into (D8:
    /// `update_notetype.<edit>`, notetype-ops.md T10.1).
    op_parts: BTreeSet<&'static str>,
    observations: BTreeMap<&'static str, ObserveFn>,
    observation_parts: BTreeSet<&'static str>,
    handoff: Option<(HandoffFn, HandoffFn)>,
    actor: Option<RawDumpFn>,
    finish: Vec<FinishFn>,
    record_changes: bool,
    fixture_backup_witness: bool,
}

impl Registry {
    /// Use a counted test witness on isolated fixture copies, never user data.
    /// This proves hook invocation, not production backup safety (host H6).
    /// Scenarios requiring `force_backup` or `backup` retain CoreHost's production provider.
    pub fn fixture_backup_witness(&mut self) {
        self.fixture_backup_witness = true;
    }

    /// Capture engine changes and backup-hook witnesses for parity tests.
    pub fn record_changes(&mut self) {
        self.record_changes = true;
    }
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

    /// A finer capability of a registered op (dsl.md D8), `<op>.<part>`:
    /// it counts with the ops, and scenarios whose steps use that part need
    /// it (`update_notetype.css`, notetype-ops.md T10.1).
    pub fn op_part(&mut self, name: &'static str) {
        let op = name.split_once('.').map(|(op, _)| op);
        assert!(
            op.is_some_and(|op| self.ops.contains_key(op)),
            "op part {name} of an op that is not registered"
        );
        assert!(self.op_parts.insert(name), "op part {name} registered twice");
    }

    pub fn observation(&mut self, kind: &'static str, f: ObserveFn) {
        assert!(
            self.observations.insert(kind, f).is_none(),
            "observation {kind} registered twice"
        );
    }

    /// A part of an observation that later units may attach (for example template.card).
    pub fn observation_part(&mut self, name: &'static str) {
        let kind = name.split_once('.').map(|(kind, _)| kind);
        assert!(
            kind.is_some_and(|kind| self.observations.contains_key(kind)),
            "observation part {name} has no registered observation"
        );
        assert!(
            self.observation_parts.insert(name),
            "observation part {name} registered twice"
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
        let mut ops: BTreeSet<String> = self.ops.keys().chain(&self.op_parts).map(|s| (*s).to_owned()).collect();
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
            observations: self
                .observations
                .keys()
                .chain(&self.observation_parts)
                .map(|s| (*s).to_owned())
                .collect(),
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
    /// Setup deck identities survive raw deletion and reopen (note-ops N1).
    pub decks: BTreeMap<String, i64>,
    /// The actor this session plays in a mixed run of an actor scenario;
    /// `None` elsewhere.
    pub actor: Option<Seat>,
    pub(super) review_session: Option<meguri_engine::session::UnqueuedSession>,
    /// The session directory and the open collection (`exec/storage.rs`).
    pub storage: super::storage::Storage,
    /// The day clock's cached day (`sched/day-clock` D6; `exec/sched.rs`).
    pub day: meguri_engine::DayCache,
    /// E2 owns the real engine-held presentation and U44 chooser cache.
    pub held_queue: Option<meguri_engine::session::held::HeldQueue>,
    pub(super) sync: super::sync::State,
}

impl Session<'_> {
    /// Promote to the normal hosted engine before an engine-owned adapter runs.
    pub fn engine_collection(&mut self) -> Result<meguri_engine::Collection, ExecError> {
        if self.storage.promote(self.clock.clone(), self.random.clone())? {
            self.day.clear();
        }
        if let Some(session) = &self.review_session {
            return session
                .collection()
                .map_err(|e| ExecError::new("core_error", e.to_string()));
        }
        self.storage.current_collection()
    }
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

    /// Explicit harness for generated/copied conformance fixtures. The default
    /// executor instead retains CoreHost's real or unavailable backup provider.
    pub fn fixture_harness() -> MeguriExecutor {
        let mut registry = Self::core_registry();
        registry.fixture_backup_witness();
        Self::with(registry)
    }

    /// The areas that have adapters, by name.
    pub const AREAS: [&'static str; 18] = [
        "package",
        "maintenance",
        "sync",
        "undo",
        "media",
        "browser",
        "decks",
        "cards",
        "model",
        "clock",
        "notetypes",
        "notes",
        "render",
        "sched",
        "search",
        "storage",
        "tags",
        "text",
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
                "package" => super::package::register(&mut registry),
                "maintenance" => super::maintenance::register(&mut registry),
                "sync" => super::sync::register(&mut registry),
                "undo" => super::undo::register(&mut registry),
                "media" => super::media::register(&mut registry),
                "cards" => super::cards::register(&mut registry),
                "model" => super::model::register(&mut registry),
                "decks" => super::decks::register(&mut registry),
                "browser" => super::browser::register(&mut registry),
                "clock" => super::clock::register(&mut registry),
                "notetypes" => super::notetypes::register(&mut registry),
                "notes" => super::notes::register(&mut registry),
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
        let mut session = Session {
            scenario: loaded,
            clock: Arc::new(clock),
            random: Arc::new(SeededRandom::new(seed(loaded))),
            notes: resume.map(|h| h.notes.clone()).unwrap_or_default(),
            cards: resume.map(|h| h.cards.clone()).unwrap_or_default(),
            decks: BTreeMap::new(),
            actor: None,
            storage: Default::default(),
            day: Default::default(),
            held_queue: None,
            sync: Default::default(),
            review_session: None,
        };
        self.configure_storage(&mut session);
        Ok(session)
    }

    /// Choose before setup can promote the writer. F3 and H1 need actual publication;
    /// other fixture scenarios keep the hook-only witness they requested.
    fn configure_storage(&self, session: &mut Session<'_>) {
        let needs = session.scenario.scenario.needs();
        if self.registry.fixture_backup_witness && !needs.ops.contains("force_backup") && !needs.ops.contains("backup")
        {
            session.storage.fixture_backup_witness();
        }
        if self.registry.record_changes {
            session.storage.record_changes();
        }
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
        super::notes::capture_decks(session)?;
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
                let result = (adapter.run)(session, other);
                // A failed undo/redo leaves its held presentation intact;
                // successful scheduling restores invalidate in undo::restore.
                let failed = !matches!(other, Step::Undo(_) | Step::Redo(_))
                    && (result.is_err() || matches!(&result, Ok(OpResult::Rejected("changed_externally"))));
                if (failed
                    || matches!(other, Step::Reopen(_))
                    || matches!(other,Step::DbExec(args) if !args.sql.trim_start().to_ascii_lowercase().starts_with("select")))
                    && let Some(held) = session.held_queue.take()
                {
                    held.invalidate();
                }
                match result? {
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
                decks: BTreeMap::new(),
                actor: None,
                storage: Default::default(),
                day: Default::default(),
                held_queue: None,
                sync: Default::default(),
                review_session: None,
            };
            self.configure_storage(&mut session);
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

#[cfg(test)]
mod host_tests {
    use super::*;
    use meguri_engine::UndoScope;
    use meguri_storage::ReadRows;

    #[test]
    fn backup_adapters_share_the_disjoint_directory_before_and_after_promotion() {
        use super::super::storage::SessionCollection;
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup-adapters.yaml");
        fs::write(
            &path,
            concat!(
                "id: host/backup-adapters\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\n",
                "setup:\n",
                "  config: {backups: {daily: 0, weekly: 0, monthly: 0, minimum_interval_mins: 30}}\n",
                "  notes: [{notetype: Basic, fields: [current front, current back]}]\n",
                "steps:\n",
                "  - add_backup_files: {names: ['backup-2029-01-01-00.00.00.colpkg', 'foreign seed', 'é']}\n",
                "  - observe: [backup_files]\n",
                "  - force_backup: {copy_as: copy.colpkg}\n",
                "  - observe: [backup_files]\n",
            ),
        )
        .unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let executor = MeguriExecutor::fixture_harness();
        let mut session = executor.session(&loaded, None).unwrap();
        executor.new_collection(&mut session).unwrap();
        assert!(matches!(session.storage.collection(), Some(SessionCollection::Raw(_))));
        executor.step(&mut session, &loaded.scenario.steps[0]).unwrap();
        let backups = session.storage.backup_directory_for_testing();
        let live = session.storage.dir().unwrap();
        assert!(!backups.starts_with(&live));
        assert!(!live.starts_with(&backups));
        assert!(!live.join("backups").exists());
        assert_eq!(fs::read(backups.join("foreign seed")).unwrap(), b"");
        assert_eq!(fs::read(backups.join("é")).unwrap(), b"");
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[1]).unwrap(),
            json!({"backup_files": ["backup-2029-01-01-00.00.00.colpkg", "foreign seed", "é"]})
        );

        // Note setup promotes before F3: provider selection cannot wait until
        // the force_backup adapter runs. F2's directory must survive promotion.
        executor.run_setup(&mut session).unwrap();
        let current_rows = |reader: &meguri_storage::Reader| {
            ["notes", "cards", "decks", "config"].map(|table| {
                reader
                    .execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[])
                    .unwrap()
            })
        };
        let (before, snapshot_mod) = {
            let reader = session.storage.collection().unwrap().reader().unwrap();
            (current_rows(&reader), reader.col().unwrap().modified)
        };
        let name = "backup-2030-01-15-10.00.00.colpkg";
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[2]).unwrap(),
            json!({"force_backup": {"name": name}})
        );
        assert!(matches!(
            session.storage.collection(),
            Some(SessionCollection::Hosted { .. })
        ));
        assert_eq!(session.storage.backup_directory_for_testing(), backups);
        assert_eq!(
            session.storage.collection().unwrap().path(),
            live.join("collection.anki2").canonicalize().unwrap()
        );
        let produced = fs::read(backups.join(name)).unwrap();
        assert!(!produced.is_empty());
        assert_eq!(fs::read(live.join("copy.colpkg")).unwrap(), produced);
        assert!(!backups.join("backup-2029-01-01-00.00.00.colpkg").exists());
        assert_eq!(session.storage.backup_calls(), 0);
        let metadata =
            crate::observe::parse_json(&fs::read_to_string(live.join("meguri-sidecar.json")).unwrap()).unwrap();
        assert_eq!(
            metadata["deviceState"]["lastBackup"],
            json!({"name": name, "snapshotMod": snapshot_mod.0, "instant": session.clock.now_secs() * 1000})
        );
        let captured = meguri_package::inspect_collection_package(
            &live.join("copy.colpkg"),
            &meguri_package::CollectionPackageInspectContext {
                collection_directory: dir.path(),
                live_collection_size: 0,
                reserved_space_bytes: 0,
                clock: &*session.clock,
                random: &*session.random,
                space: &meguri_storage::space::StatvfsSpace,
                is_cancelled: &|| false,
            },
        )
        .unwrap();
        assert_eq!((captured.facts().notes, captured.facts().cards), (1, 1));
        let recovered = meguri_storage::open_collection(
            &captured.collection_path(),
            &meguri_storage::OpenOptions::new(&*session.clock, &*session.random),
        )
        .unwrap();
        {
            let reader = recovered.open_reader(meguri_storage::ReaderRole::Query).unwrap();
            assert_eq!(current_rows(&reader), before);
            assert_eq!(reader.col().unwrap().modified, snapshot_mod);
        }
        recovered.close().unwrap();
        assert_eq!(fs::read(backups.join("foreign seed")).unwrap(), b"");
        assert_eq!(fs::read(backups.join("é")).unwrap(), b"");
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[3]).unwrap(),
            json!({"backup_files": [name, "foreign seed", "é"]})
        );
        assert!(!live.join("backups").exists());
        executor.finish(&mut session).unwrap();
    }

    #[test]
    fn fixture_actor_sessions_select_real_forced_backups_and_keep_hook_only_witnesses() {
        for forced in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("actor-backup.yaml");
            let op = if forced {
                "force_backup: {}"
            } else {
                "delete_decks: {decks: [Protected]}"
            };
            std::fs::write(
                &path,
                format!(
                    "id: host/actor-backup\nclock: {{tz: UTC, start: '2030-01-15T10:00:00'}}\n\
                     actors: [meguri]\nsetup:\n  decks: [Protected]\n  \
                     notes: [{{notetype: Basic, deck: Protected, fields: [front, back]}}]\nsteps:\n  - {op}\n"
                ),
            )
            .unwrap();
            let loaded = crate::scenario::load(&path).unwrap();
            let executor = MeguriExecutor::fixture_harness();
            let actor = loaded.scenario.actors()[0].clone();
            let mut actors = executor.actors(&loaded, std::slice::from_ref(&actor)).unwrap();
            let ids = actors.setup(&actor).unwrap();
            assert_eq!(ids.cards.len(), 1);
            let (output, _) = actors.step(&actor, &loaded.scenario.steps[0], &ids).unwrap();
            let session = actors.session(&actor).unwrap();
            let backups = session.storage.backup_directory_for_testing();
            if forced {
                let name = "backup-2030-01-15-10.00.00.colpkg";
                assert_eq!(output, json!({"force_backup": {"name": name}}));
                assert!(!std::fs::read(backups.join(name)).unwrap().is_empty());
                assert_eq!(session.storage.backup_calls(), 0);
            } else {
                assert_eq!(output, json!({"delete_decks": {"count": 1}}));
                assert_eq!(session.storage.backup_calls(), 1);
                assert_eq!(std::fs::read_dir(backups).unwrap().count(), 0);
            }
            executor.finish(session).unwrap();
        }
    }

    #[test]
    fn backup_handoffs_replace_owned_snapshots_without_nesting_or_resurrecting_files() {
        use std::fs;

        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup-handoff.yaml");
        fs::write(
            &path,
            concat!(
                "id: host/backup-handoff\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsteps:\n",
                "  - add_backup_files: {names: ['é backup.colpkg', 'remove me']}\n",
                "  - export: {format: colpkg, name: flat.colpkg}\n",
                "  - observe: [backup_files]\n",
            ),
        )
        .unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let executor = MeguriExecutor::new();
        let mut session = executor.session(&loaded, None).unwrap();
        executor.new_collection(&mut session).unwrap();
        executor.step(&mut session, &loaded.scenario.steps[0]).unwrap();
        let backups = session.storage.backup_directory_for_testing();
        let live = session.storage.dir().unwrap();
        let bytes = b"\0\xffexact backup bytes\r\n";
        fs::write(backups.join("é backup.colpkg"), bytes).unwrap();
        fs::write(backups.join("remove me"), b"deleted later").unwrap();
        fs::write(backups.parent().unwrap().join("host-private"), b"private").unwrap();
        executor.step(&mut session, &loaded.scenario.steps[1]).unwrap();
        let exported = fs::read(live.join("flat.colpkg")).unwrap();
        fs::create_dir_all(live.join("collection.media")).unwrap();
        fs::write(live.join("collection.media/attachment"), b"media bytes").unwrap();
        let (out, into) = executor.registry.handoff.unwrap();
        let wire = dir.path().join("wire");
        out(&mut session, &wire).unwrap();
        assert!(!session.storage.is_open());
        assert!(wire.join("collection.anki2").is_file());
        assert_eq!(fs::read(wire.join("flat.colpkg")).unwrap(), exported);
        assert_eq!(
            fs::read(wire.join("collection.media/attachment")).unwrap(),
            b"media bytes"
        );
        assert_eq!(fs::read(wire.join("backups/é backup.colpkg")).unwrap(), bytes);
        assert_eq!(fs::read(wire.join("backups/remove me")).unwrap(), b"deleted later");
        assert!(!wire.join("host-private").exists());
        assert!(!wire.join("app-group").exists());

        // Taking the wire snapshot replaces a populated runtime directory.
        fs::remove_file(wire.join("backups/remove me")).unwrap();
        into(&mut session, &wire).unwrap();
        assert!(!live.join("backups").exists());
        assert_eq!(session.storage.backup_directory_for_testing(), backups);
        assert_eq!(fs::read(backups.join("é backup.colpkg")).unwrap(), bytes);
        assert!(!backups.join("remove me").exists());
        session.engine_collection().unwrap();
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[2]).unwrap(),
            json!({"backup_files": ["é backup.colpkg"]})
        );
        assert_eq!(fs::read(live.join("flat.colpkg")).unwrap(), exported);

        // Publishing again replaces the prior wire snapshot, including files
        // removed from the runtime since the previous publication.
        fs::remove_file(backups.join("é backup.colpkg")).unwrap();
        out(&mut session, &wire).unwrap();
        assert_eq!(fs::read_dir(wire.join("backups")).unwrap().count(), 0);
        assert_eq!(
            fs::read(backups.parent().unwrap().join("host-private")).unwrap(),
            b"private"
        );

        // Missing incoming backups mean empty even when runtime has old files.
        // Repeat across close/open/promotion to catch accidental resurrection.
        for _ in 0..2 {
            fs::write(backups.join("stale"), b"must disappear").unwrap();
            fs::remove_dir_all(wire.join("backups")).unwrap();
            into(&mut session, &wire).unwrap();
            assert_eq!(
                executor.step(&mut session, &loaded.scenario.steps[2]).unwrap(),
                json!({"backup_files": []})
            );
            assert!(!live.join("backups").exists());
            session.engine_collection().unwrap();
            out(&mut session, &wire).unwrap();
            assert_eq!(fs::read_dir(wire.join("backups")).unwrap().count(), 0);
            assert_eq!(fs::read(wire.join("flat.colpkg")).unwrap(), exported);
        }
    }

    #[test]
    fn injected_unavailable_backup_refuses_deletion_without_mutation_or_history_events() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("refusal.yaml");
        std::fs::write(&path, "id: host/refusal\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsetup:\n  decks: [Protected]\n  notes: [{notetype: Basic, deck: Protected, fields: [front, back]}]\nsteps:\n  - delete_decks: {decks: [Protected]}\n").unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let mut executor = MeguriExecutor::new();
        executor.registry.record_changes();
        let mut session = executor.session(&loaded, None).unwrap();
        session
            .storage
            .override_backup_for_testing(Arc::new(meguri_engine::ports::Unavailable {
                feature: "testkit injected safety backup",
                provider_unit: "testkit-injected-backup-provider",
            }));
        executor.new_collection(&mut session).unwrap();
        executor.run_setup(&mut session).unwrap();
        let collection = session.engine_collection().unwrap();
        let data = |session: &Session<'_>| {
            let reader = session.storage.collection().unwrap().reader().unwrap();
            ["col", "decks", "notes", "cards", "graves"].map(|table| {
                reader
                    .execute_raw(&format!("SELECT * FROM {table} ORDER BY 1"), &[])
                    .unwrap()
            })
        };
        let before = data(&session);
        let undo = collection.peek_undo(UndoScope::Global).unwrap();
        session.storage.take_changes();
        let error = executor.step(&mut session, &loaded.scenario.steps[0]).unwrap_err();
        assert_eq!(error.kind, "core_error");
        assert_eq!(
            error.detail,
            "collection unavailable: testkit injected safety backup requires provider testkit-injected-backup-provider"
        );
        assert_eq!(data(&session), before);
        assert_eq!(collection.peek_undo(UndoScope::Global).unwrap(), undo);
        assert!(session.storage.take_changes().is_empty());
        assert_eq!(session.storage.backup_calls(), 0);
    }

    #[test]
    fn default_executor_deletion_publishes_a_recoverable_pre_delete_backup() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("backup.yaml");
        std::fs::write(&path, "id: host/backup\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsetup:\n  decks: [Protected]\n  notes: [{notetype: Basic, deck: Protected, fields: [front, back]}]\nsteps:\n  - delete_decks: {decks: [Protected]}\n").unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let executor = MeguriExecutor::new();
        let mut session = executor.session(&loaded, None).unwrap();
        executor.new_collection(&mut session).unwrap();
        executor.run_setup(&mut session).unwrap();
        let protected_rows = |reader: &meguri_storage::Reader| {
            [
                "SELECT * FROM decks WHERE name = 'Protected' ORDER BY id",
                "SELECT * FROM notes ORDER BY id",
                "SELECT * FROM cards ORDER BY id",
            ]
            .map(|sql| reader.execute_raw(sql, &[]).unwrap())
        };
        let before = {
            let reader = session.storage.collection().unwrap().reader().unwrap();
            protected_rows(&reader)
        };
        assert!(before.iter().all(|table| table.rows.len() == 1));
        let backups = session.storage.backup_directory_for_testing();
        assert_eq!(std::fs::read_dir(&backups).unwrap().count(), 0);
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[0]).unwrap(),
            json!({"delete_decks": {"count": 1}})
        );
        {
            let reader = session.storage.collection().unwrap().reader().unwrap();
            assert!(protected_rows(&reader).iter().all(|table| table.rows.is_empty()));
        }
        assert_eq!(session.storage.backup_calls(), 0);
        let published = std::fs::read_dir(&backups)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect::<Vec<_>>();
        assert_eq!(published.len(), 1);
        assert_eq!(published[0].file_name().unwrap(), "backup-2030-01-15-10.00.00.colpkg");
        // T6.2: the real provider must publish before deletion commits. P12
        // inspection extracts and validates the saved pre-delete collection.
        let captured = meguri_package::inspect_collection_package(
            &published[0],
            &meguri_package::CollectionPackageInspectContext {
                collection_directory: dir.path(),
                live_collection_size: 0,
                reserved_space_bytes: 0,
                clock: &*session.clock,
                random: &*session.random,
                space: &meguri_storage::space::StatvfsSpace,
                is_cancelled: &|| false,
            },
        )
        .unwrap();
        assert_eq!((captured.facts().notes, captured.facts().cards), (1, 1));
        let recovered = meguri_storage::open_collection(
            &captured.collection_path(),
            &meguri_storage::OpenOptions::new(&*session.clock, &*session.random),
        )
        .unwrap();
        {
            let reader = recovered.open_reader(meguri_storage::ReaderRole::Query).unwrap();
            assert_eq!(protected_rows(&reader), before);
        }
        recovered.close().unwrap();
        executor.finish(&mut session).unwrap();
    }

    #[test]
    fn unavailable_and_failed_restores_preserve_held_owners_and_cached_day() {
        use meguri_engine::executor::op::OpSpec;
        use meguri_engine::undo::{entry::StepKind, names};
        use meguri_model::{changes::OpSource, clock::UtcOffset};
        use meguri_storage::ConfigValue;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("restore.yaml");
        std::fs::write(
            &path,
            "id: undo/restore\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\nsteps: [{undo: {}}, {redo: {}}]\n",
        )
        .unwrap();
        let loaded = crate::scenario::load(&path).unwrap();
        let executor = MeguriExecutor::new();
        let mut session = executor.session(&loaded, None).unwrap();
        executor.new_collection(&mut session).unwrap();
        let collection = session.engine_collection().unwrap();
        session.held_queue = Some(meguri_engine::session::held::HeldQueue::build(&collection, None).unwrap());
        super::super::sched::day_query_now(&mut session).unwrap();
        let day = session.day.valid_at(session.clock.now_secs(), UtcOffset::UTC);
        assert!(day.is_some());
        let owner = Arc::downgrade(&session.held_queue.as_ref().unwrap().cache());
        for (step, error) in loaded.scenario.steps.iter().zip(["nothing_to_undo", "nothing_to_redo"]) {
            assert_eq!(
                executor.step(&mut session, step).unwrap(),
                json!({(step.name()): {"error": error}})
            );
            assert!(session.held_queue.is_some() && owner.upgrade().is_some());
            assert_eq!(session.day.valid_at(session.clock.now_secs(), UtcOffset::UTC), day);
        }
        // The named step is still visible to peek, but its saved row no longer
        // matches. This exercises the failed restore after a successful peek.
        collection
            .run(
                OpSpec::named("record", OpSource::UserUndoable).undo_step(names::SUSPEND, StepKind::Other),
                |ctx| {
                    ctx.scope().set_config("restore-test", &ConfigValue::Integer(1))?;
                    Ok(())
                },
            )
            .unwrap();
        collection
            .run(
                OpSpec::named("outside-history", OpSource::UserNonUndoable).skip_undo(),
                |ctx| {
                    ctx.scope().set_config("restore-test", &ConfigValue::Integer(2))?;
                    Ok(())
                },
            )
            .unwrap();
        assert_eq!(
            collection.peek_undo(UndoScope::Global).unwrap().name(),
            Some(names::SUSPEND)
        );
        assert_eq!(
            executor.step(&mut session, &loaded.scenario.steps[0]).unwrap(),
            json!({"undo": {"error": "changed_externally"}})
        );
        assert!(session.held_queue.is_some() && owner.upgrade().is_some());
        assert_eq!(session.day.valid_at(session.clock.now_secs(), UtcOffset::UTC), day);
    }

    #[test]
    fn flags_tags_and_same_card_count_edits_keep_held_owners_through_undo_and_redo() {
        use meguri_model::clock::UtcOffset;
        for op in [
            "set_flag: {cards: ['#c1'], flag: 2}",
            "add_note_tags: {notes: ['#n1'], tags: example}",
            "edit_note: {note: '#n1', fields: [changed, back]}",
        ] {
            let dir = tempfile::tempdir().unwrap();
            let path = dir.path().join("other.yaml");
            std::fs::write(&path, format!("id: undo/other\nclock: {{tz: UTC, start: '2030-01-15T10:00:00'}}\nsetup:\n  notes: [{{notetype: Basic, fields: [front, back]}}]\nsteps:\n  - observe: [queue]\n  - {op}\n  - undo: {{}}\n  - redo: {{}}\n")).unwrap();
            let loaded = crate::scenario::load(&path).unwrap();
            let executor = MeguriExecutor::new();
            let mut session = executor.session(&loaded, None).unwrap();
            executor.new_collection(&mut session).unwrap();
            executor.run_setup(&mut session).unwrap();
            executor.step(&mut session, &loaded.scenario.steps[0]).unwrap();
            super::super::sched::day_query_now(&mut session).unwrap();
            let day = session.day.valid_at(session.clock.now_secs(), UtcOffset::UTC);
            assert!(day.is_some());
            let owner = Arc::downgrade(&session.held_queue.as_ref().unwrap().cache());
            for step in &loaded.scenario.steps[1..] {
                executor.step(&mut session, step).unwrap();
                assert!(
                    session.held_queue.is_some() && owner.upgrade().is_some(),
                    "{op}: {}",
                    step.name()
                );
                assert_eq!(session.day.valid_at(session.clock.now_secs(), UtcOffset::UTC), day);
            }
        }
    }
}

#[cfg(test)]
mod media_adapter_tests {
    use super::super::storage::SessionCollection;
    use super::*;
    use meguri_storage::media::{MediaBaselineOwner, MediaSyncState};

    fn load(text: &str) -> (tempfile::TempDir, LoadedScenario) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("media.yaml");
        std::fs::write(&path, text).unwrap();
        (dir, crate::scenario::load(&path).unwrap())
    }

    #[test]
    fn media_quick_check_preserves_marker_fast_path_and_exact_pending_values() {
        let (_dir, loaded) = load(
            "id: media/quick-adapter\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\n\
             setup: {media_files: [{name: x, text: x}]}\nsteps:\n\
             - observe: [media_quick_check]\n\
             - media_outside: [{name: x, text: hello}]\n\
             - observe: [media_quick_check]\n\
             - observe: [media_pending]\n\
             - advance: 1s\n\
             - media_outside: [{name: x, remove: true}]\n\
             - observe: [media_quick_check]\n",
        );
        for hosted in [false, true] {
            let executor = MeguriExecutor::new();
            let mut session = executor.session(&loaded, None).unwrap();
            executor.new_collection(&mut session).unwrap();
            if hosted {
                session.engine_collection().unwrap();
            }
            executor.run_setup(&mut session).unwrap();
            let steps = &loaded.scenario.steps;
            let original = json!({"media_quick_check":{"x":"11f6ad8ec52a2984abaafd7c3b516503785c2072"}});
            assert_eq!(executor.step(&mut session, &steps[0]).unwrap(), original);
            let store = session
                .storage
                .media(session.clock.clone(), session.random.clone())
                .unwrap();
            let before = store.diagnostics();
            executor.step(&mut session, &steps[1]).unwrap();
            assert_eq!(executor.step(&mut session, &steps[2]).unwrap(), original);
            let after = store.diagnostics();
            assert_eq!((after.scans, after.hashes), (before.scans, before.hashes));
            assert_eq!(
                executor.step(&mut session, &steps[3]).unwrap(),
                json!({"media_pending":{"x":"aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"}})
            );
            assert!(store.diagnostics().scans > after.scans);
            assert!(store.diagnostics().hashes > after.hashes);
            executor.step(&mut session, &steps[4]).unwrap();
            executor.step(&mut session, &steps[5]).unwrap();
            assert_eq!(
                executor.step(&mut session, &steps[6]).unwrap(),
                json!({"media_quick_check":{"x":null}})
            );
        }
    }

    #[test]
    fn media_reset_retains_files_facts_owner_and_trash_without_network_and_obeys_host_job_exclusion() {
        let (_dir, loaded) = load(
            "id: media/reset-adapter\nclock: {tz: UTC, start: '2030-01-15T10:00:00'}\n\
             setup: {media_files: [{name: keep, text: hello}, {name: gone, text: x}]}\nsteps:\n\
             - observe: [media_quick_check]\n\
             - trash_media: {names: [gone]}\n\
             - media_reset: {}\n\
             - observe: [media_quick_check]\n",
        );
        let executor = MeguriExecutor::new();
        let mut session = executor.session(&loaded, None).unwrap();
        executor.new_collection(&mut session).unwrap();
        // trash_media promotes, so exercise reset against that same hosted owner.
        session.engine_collection().unwrap();
        let store = session
            .storage
            .media(session.clock.clone(), session.random.clone())
            .unwrap();
        store
            .bind_sync_owner(&MediaBaselineOwner::new("synthetic-owner"))
            .unwrap();
        executor.run_setup(&mut session).unwrap();
        let steps = &loaded.scenario.steps;
        executor.step(&mut session, &steps[0]).unwrap();
        executor.step(&mut session, &steps[1]).unwrap();
        let facts = store.sync_entry("keep").unwrap().unwrap();
        let trash = store.trash_entries().unwrap();
        assert_eq!(trash.len(), 1);
        assert_eq!(
            store.sync_entry("gone").unwrap().unwrap().sync_state,
            MediaSyncState::Deleted
        );
        let initial_baseline = store.sync_baseline().unwrap();
        // Seed a prior cursor through the production page-commit API, offline.
        store.commit_media_page(&initial_baseline, 7, &[]).unwrap();
        let baseline = store.sync_baseline().unwrap();
        assert_eq!(baseline.usn, 7);
        let SessionCollection::Hosted { host, .. } = session.storage.collection().unwrap() else {
            panic!("expected hosted media");
        };
        let media = host.media().unwrap();
        for kind in [
            meguri_engine::media::MediaJobKind::Sync,
            meguri_engine::media::MediaJobKind::CheckMedia,
        ] {
            let lease = media.try_job(kind).unwrap();
            assert!(executor.step(&mut session, &steps[2]).is_err());
            assert!(executor.step(&mut session, &steps[3]).is_err());
            assert_eq!(store.sync_baseline().unwrap(), baseline);
            assert_eq!(store.sync_entry("keep").unwrap().unwrap(), facts);
            drop(lease);
        }
        assert_eq!(executor.step(&mut session, &steps[2]).unwrap(), json!({}));
        let reset_baseline = store.sync_baseline().unwrap();
        assert_eq!(reset_baseline.owner, baseline.owner);
        assert_eq!(reset_baseline.usn, 0);
        assert!(reset_baseline.epoch > baseline.epoch);
        let reset_facts = store.sync_entry("keep").unwrap().unwrap();
        assert_eq!(
            (
                &reset_facts.sha1,
                reset_facts.size,
                reset_facts.mtime_secs,
                reset_facts.mtime_nanos
            ),
            (&facts.sha1, facts.size, facts.mtime_secs, facts.mtime_nanos)
        );
        assert_eq!(reset_facts.sync_state, MediaSyncState::None);
        assert!(!reset_facts.dirty);
        assert!(store.pending().unwrap().is_empty());
        assert!(store.sync_entry("gone").unwrap().is_none());
        assert_eq!(std::fs::read(store.media_dir().join("keep")).unwrap(), b"hello");
        assert!(!store.media_dir().join("gone").exists());
        assert_eq!(store.trash_entries().unwrap(), trash);
        let scans = store.diagnostics().scans;
        assert_eq!(
            executor.step(&mut session, &steps[3]).unwrap(),
            json!({"media_quick_check":{"keep":"aaf4c61ddcc5e8a2dabede0f3b482cd9aea9434d"}})
        );
        assert!(store.diagnostics().scans > scans, "reset must unset the folder marker");
        assert_eq!(store.sync_baseline().unwrap(), reset_baseline);
        assert!(!executor.capabilities().ops.contains("media_sync"));
    }
}
