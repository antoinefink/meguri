//! Shared helpers for the engine tests: an engine over a copy of the
//! oracle-made schema-18 fixture, row builders, ops that add and edit rows,
//! and raw reads through the role readers (test hooks). Fixtures are read
//! only to copy them; SQLite only ever opens the copies.

#![allow(dead_code)]

pub mod day;
pub mod note_scenarios;
pub mod notes;
pub mod steps;
pub mod tagging;

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use meguri_engine::{
    Collection, Engine, EngineConfig, Event, EventKind, OpResult, OpSpec, OpenConfig, Ports, Subscription,
};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::note::Note;
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole};
use tempfile::TempDir;

/// 2030-01-15T10:00:00Z, the scenarios' clock.
pub const NOW_MS: i64 = 1_894_701_600_000;

/// The note type every test note names; storage does not check it.
pub const MID: NotetypeId = NotetypeId(1);

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../../conformance/fixtures/storage")
        .join(name)
}

/// A temporary directory holding a copy of the fixture as
/// `collection.anki2`.
pub fn copy_fixture(name: &str) -> (TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    fs::copy(fixture(name), &path).unwrap();
    (dir, path)
}

/// An engine with one collection open.
pub struct Harness {
    pub dir: TempDir,
    pub path: PathBuf,
    pub clock: Arc<StepClock>,
    pub engine: Engine,
    pub collection: Collection,
}

impl Harness {
    pub fn open() -> Harness {
        Harness::open_with(Ports::default())
    }

    pub fn open_with(ports: Ports) -> Harness {
        Harness::open_fixture("schema-18.anki2", ports)
    }

    pub fn open_fixture(name: &str, ports: Ports) -> Harness {
        let (dir, path) = copy_fixture(name);
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine.open_raw(&path, open_config(&clock).ports(ports)).unwrap();
        Harness {
            dir,
            path,
            clock,
            engine,
            collection,
        }
    }

    /// Opens an existing file (a working copy) with a fresh engine.
    pub fn open_path(dir: TempDir, path: PathBuf) -> Harness {
        let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
        let engine = Engine::new(EngineConfig::default()).unwrap();
        let collection = engine.open_raw(&path, open_config(&clock)).unwrap();
        Harness {
            dir,
            path,
            clock,
            engine,
            collection,
        }
    }
}

pub fn open_config(clock: &Arc<StepClock>) -> OpenConfig {
    OpenConfig::new(clock.clone(), Arc::new(SeededRandom::new(7)))
}

pub fn note(fields: &[&str]) -> Note {
    Note {
        guid: format!("g{}", fields.join("")),
        notetype_id: MID,
        fields: fields.join("\u{1f}"),
        sort_field: fields.first().copied().unwrap_or_default().to_owned(),
        ..Note::default()
    }
}

pub fn card(note: NoteId, ord: u16) -> Card {
    Card {
        id: CardId(0),
        note_id: note,
        deck_id: DeckId(1),
        ord,
        mtime: EpochSecs(0),
        usn: Usn(0),
        card_type: CardType::New,
        queue: CardQueue::New,
        due: 1,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: meguri_model::card_data::CardData::default(),
    }
}

pub fn revlog(card: CardId) -> RevlogEntry {
    RevlogEntry {
        id: RevlogId(0),
        card_id: card,
        usn: Usn(0),
        button: 3,
        interval: 1,
        last_interval: 0,
        ease_factor: 2500,
        answer_millis: 4000,
        kind: RevlogKind::Review,
    }
}

/// Adds a note with one card in one op.
pub fn add_note_with_card(collection: &Collection, source: OpSource, fields: &[&str]) -> OpResult<(NoteId, CardId)> {
    let note = note(fields);
    collection
        .run(OpSpec::named("add_note", source), move |ctx| {
            let nid = ctx.scope().add_note(&note)?;
            let cid = ctx.scope().add_card(&card(nid, 0))?;
            Ok((nid, cid))
        })
        .unwrap()
}

/// Adds `count` notes (no cards) in one op.
pub fn add_notes(collection: &Collection, count: usize, prefix: &str) -> Vec<NoteId> {
    let prefix = prefix.to_owned();
    collection
        .run(OpSpec::named("add_notes", OpSource::UserNonUndoable), move |ctx| {
            (0..count)
                .map(|i| Ok(ctx.scope().add_note(&note(&[&format!("{prefix}{i}"), "back"]))?))
                .collect()
        })
        .unwrap()
        .value
}

/// An op that sets a note's first field.
pub fn set_first_field(
    collection: &Collection,
    source: OpSource,
    nid: NoteId,
    value: &str,
) -> Result<OpResult<()>, meguri_engine::EngineError> {
    let value = value.to_owned();
    collection.run(OpSpec::named("update_note_fields", source), move |ctx| {
        let mut note = ctx.scope().note(nid)?.expect("the note exists");
        let mut fields: Vec<String> = note.field_values().map(str::to_owned).collect();
        fields[0] = value.clone();
        note.fields = fields.join("\u{1f}");
        note.sort_field = value;
        ctx.scope().put(&note)?;
        Ok(())
    })
}

pub fn note_fields(collection: &Collection, nid: NoteId) -> String {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader.note(nid).unwrap().expect("the note exists").fields
}

pub fn col_mod(collection: &Collection) -> i64 {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    reader.col().unwrap().modified.0
}

/// One value of a one-row, one-column raw query on the query reader.
pub fn query_int(collection: &Collection, sql: &str) -> i64 {
    let reader = collection.reader(ReaderRole::Query).unwrap();
    match &reader.execute_raw(sql, &[]).unwrap().rows[0][0] {
        SqlValue::Integer(v) => *v,
        other => panic!("{sql}: not an integer: {other:?}"),
    }
}

/// Every event the subscription holds now or receives within `wait`.
pub fn drain(subscription: &Subscription, wait: Duration) -> Vec<Arc<Event>> {
    let mut events = Vec::new();
    while let Some(event) = subscription.recv_timeout(wait) {
        events.push(event);
    }
    events
}

/// The seqs of the `Changes` events among `events`.
pub fn change_seqs(events: &[Arc<Event>]) -> Vec<u64> {
    events
        .iter()
        .filter(|e| matches!(e.kind, EventKind::Changes(_)))
        .map(|e| e.seq.0)
        .collect()
}

/// A gate an op waits at on the writer, so tests can queue ops behind it.
#[derive(Clone, Default)]
pub struct Gate {
    state: Arc<(Mutex<(bool, bool)>, std::sync::Condvar)>,
}

impl Gate {
    pub fn new() -> Gate {
        Gate::default()
    }

    /// Called inside the blocking op: marks it entered and waits for open.
    pub fn enter_and_wait(&self) {
        let (lock, cv) = &*self.state;
        let mut state = lock.lock().unwrap();
        state.0 = true;
        cv.notify_all();
        while !state.1 {
            state = cv.wait(state).unwrap();
        }
    }

    /// Waits until the blocking op is on the writer.
    pub fn wait_entered(&self) {
        let (lock, cv) = &*self.state;
        let mut state = lock.lock().unwrap();
        while !state.0 {
            state = cv.wait(state).unwrap();
        }
    }

    pub fn open(&self) {
        let (lock, cv) = &*self.state;
        lock.lock().unwrap().1 = true;
        cv.notify_all();
    }

    /// Waits until the gate is open.
    pub fn wait_open(&self) {
        let (lock, cv) = &*self.state;
        let mut state = lock.lock().unwrap();
        while !state.1 {
            state = cv.wait(state).unwrap();
        }
    }
}

/// Submits an op of `source` that holds the writer until `gate` opens, and
/// waits until it does.
pub fn block_writer(collection: &Collection, source: OpSource, gate: &Gate) -> meguri_engine::Pending<OpResult<()>> {
    let inner = gate.clone();
    let pending = collection.submit(OpSpec::named("blocker", source), move |_| {
        inner.enter_and_wait();
        Ok(())
    });
    gate.wait_entered();
    pending
}
