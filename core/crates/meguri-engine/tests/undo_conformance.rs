//! Undo scenarios through real U27 note add, update, generation and deletion.
//! Uses U28 bootstrap and deck operations. The single answer remains a test
//! stand-in until U44, and U38 owns the production undo session adapter.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::Harness;
use meguri_engine::executor::catalog;
use meguri_engine::undo::names::{ANSWER_CARD, UPDATE_CONFIG, UPDATE_NOTE};
use meguri_engine::{ClearCause, Collection, DisabledReason, EngineError, OpSpec, StepKind, UndoScope, UndoStatus};
use meguri_model::card::{CardQueue, CardType};
use meguri_model::card_data::MemoryState;
use meguri_model::changes::OpSource;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId, RevlogId};
use meguri_model::revlog::{RevlogEntry, RevlogKind};
use meguri_model::usn::Usn;
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};
use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::{db_dump_table, db_exec_rows};
use meguri_testkit::scenario::{
    self, AnswerSpec, DUMP_TABLES, DeleteNotesArgs, EditNoteArgs, Fields, LoadedScenario, LogicalRef, NoteSpec,
    ObservationArgs, ObservationKind, ObservationRequest, Rating, RefKind, SqlParam, Step,
};
use meguri_testkit::suite::Suite;
use meguri_text::normalize::field_as_saved;
use serde_json::{Map, Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// A note type of the scenario's setup (dsl.md D6.4), kept here only.
#[derive(Clone, Debug)]
struct Notetype {
    id: NotetypeId,
    fields: Vec<String>,
}

impl Notetype {
    /// The field values a note of this type stores after `fields` is
    /// applied to `current` (dsl.md D7.2: a list replaces from the first
    /// field, a mapping the named fields), as saved (text C1).
    fn apply(&self, current: &[String], fields: &Fields) -> Vec<String> {
        let mut values: Vec<String> = (0..self.fields.len())
            .map(|i| current.get(i).cloned().unwrap_or_default())
            .collect();
        match fields {
            Fields::List(list) => {
                assert!(list.len() <= values.len(), "more values than fields");
                for (slot, value) in values.iter_mut().zip(list) {
                    slot.clone_from(value);
                }
            }
            Fields::Map(map) => {
                for (name, value) in map {
                    let i = self
                        .fields
                        .iter()
                        .position(|f| f == name)
                        .expect("a field of the note type");
                    values[i].clone_from(value);
                }
            }
        }
        values
            .iter()
            .map(|value| field_as_saved(value, true).into_owned())
            .collect()
    }
}

/// One scenario on one engine.
struct Player {
    h: Harness,
    scenario: LoadedScenario,
    decks: BTreeMap<String, DeckId>,
    notetypes: BTreeMap<String, Notetype>,
    notes: Vec<NoteId>,
    cards: Vec<CardId>,
}

impl Player {
    fn new(scenario: LoadedScenario) -> Player {
        let mut decks = BTreeMap::new();
        decks.insert("Default".to_owned(), DeckId(1));
        Player {
            h: common::fresh(),
            scenario,
            decks,
            notetypes: BTreeMap::new(),
            notes: Vec::new(),
            cards: Vec::new(),
        }
    }

    fn c(&self) -> &Collection {
        &self.h.collection
    }

    /// W1: one statement on the writer connection, committed at once. One
    /// that does not begin with `select` empties both histories.
    fn db_exec(&self, sql: &str, params: Vec<SqlValue>) -> Value {
        let statement = sql.to_owned();
        let rows = self
            .c()
            .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |writer| {
                Ok(writer.execute_raw(&statement, &params)?)
            })
            .wait()
            .unwrap();
        if !sql.trim_start().to_ascii_lowercase().starts_with("select") {
            self.c().clear_undo(ClearCause::ExternalChange).unwrap();
        }
        json!({ "rows": db_exec_rows(&rows) })
    }

    fn setup_op() -> OpSpec {
        OpSpec::named("scenario_setup", OpSource::UserNonUndoable).skip_undo()
    }

    /// dsl.md D6 in section order: config, decks, note types, notes.
    fn setup(&mut self) {
        let setup = self.scenario.scenario.setup.clone();
        for section in setup.sections() {
            assert!(
                ["config", "decks", "notetypes", "notes"].contains(&section),
                "this test plays no setup.{section}"
            );
        }
        for (key, value) in setup.config.iter().flatten() {
            self.set_config(key, value, false);
        }
        for deck in setup.decks.iter().flatten() {
            let name = deck.name().to_owned();
            let id = meguri_engine::decks::add(self.c(), name.clone()).unwrap().value.id;
            self.decks.insert(name, id);
        }
        for notetype in setup.notetypes.iter().flatten() {
            let new = meguri_engine::notetypes::NewNotetype {
                name: notetype.name.clone(),
                kind: match notetype.kind {
                    scenario::NotetypeKind::Cloze => meguri_engine::notetypes::NotetypeKind::Cloze,
                    _ => meguri_engine::notetypes::NotetypeKind::Standard,
                },
                fields: notetype.fields.clone(),
                templates: notetype
                    .templates
                    .iter()
                    .map(|t| meguri_engine::notetypes::NewTemplate {
                        name: t.name.clone(),
                        front: t.front.clone(),
                        back: t.back.clone(),
                    })
                    .collect(),
                css: Some(notetype.css.clone()),
            };
            let id = self
                .c()
                .run(Player::setup_op(), move |ctx| {
                    let (scope, random) = ctx.scope_and_random();
                    meguri_engine::notetypes::add_in(scope, random, &new).map(|a| a.id)
                })
                .unwrap()
                .value;
            self.notetypes.insert(
                notetype.name.clone(),
                Notetype {
                    id,
                    fields: notetype.fields.clone(),
                },
            );
        }
        for note in setup.notes.iter().flatten() {
            self.add_note(note);
        }
    }

    /// D6.5: a note as `add_note` adds it, its cards in template order.
    fn add_note(&mut self, spec: &NoteSpec) {
        let notetype = self.notetypes[&spec.notetype].clone();
        let deck = *self
            .decks
            .get(spec.deck.as_deref().unwrap_or("Default"))
            .expect("a setup deck");
        let values = notetype.apply(&[], &spec.fields);
        let input = meguri_engine::notes::AddNote {
            notetype_id: notetype.id,
            fields: values,
            tags: spec.tags.clone(),
            deck_id: deck,
            guid: None,
            update_add_defaults: true,
        };
        let added = meguri_engine::notes::add(self.c(), input, OpSource::UserUndoable)
            .unwrap()
            .value;
        let (nid, cids) = (added.id, added.cards);
        self.notes.push(nid);
        self.cards.extend(cids);
    }

    /// W4, or with `undoable` V3.1: one config key through the config API.
    fn set_config(&self, key: &str, value: &Value, undoable: bool) {
        let key = key.to_owned();
        let value = ConfigValue::parse(value.to_string().as_bytes()).expect("a JSON value");
        let spec = if undoable {
            OpSpec::named("set_config", OpSource::UserUndoable).undo_step(UPDATE_CONFIG, StepKind::Other)
        } else {
            OpSpec::named("set_config", OpSource::UserNonUndoable).skip_undo()
        };
        self.c()
            .run(spec, move |ctx| {
                ctx.scope().set_config(&key, &value)?;
                Ok(())
            })
            .unwrap();
    }

    /// V3.2: the key removed through the config API, an "Update Config" step.
    fn remove_config(&self, key: &str) {
        let key = key.to_owned();
        self.c()
            .run(
                OpSpec::named("remove_config", OpSource::UserUndoable).undo_step(UPDATE_CONFIG, StepKind::Other),
                move |ctx| {
                    ctx.scope().remove_config(&key)?;
                    Ok(())
                },
            )
            .unwrap();
    }

    /// W2: the note loaded, changed and saved as one "Update Note" op. A save
    /// that changes nothing writes nothing; new tags are registered and the
    /// cards the new content shows are added, in the deck and at the new
    /// position of the note's first card, with no logical id (undo-ops V5).
    fn edit_note(&self, args: &EditNoteArgs) {
        let nid = self.note(args.note.0);
        let notetypes: Vec<Notetype> = self.notetypes.values().cloned().collect();
        let fields = args.fields.clone();
        let tags = args.tags.clone();
        self.c()
            .run(
                OpSpec::named("update_note", OpSource::UserUndoable).undo_step(UPDATE_NOTE, StepKind::Other),
                move |ctx| {
                    let note = ctx.scope().note(nid)?.unwrap();
                    let nt = notetypes.iter().find(|t| t.id == note.notetype_id).unwrap();
                    let current = note.field_values().map(str::to_owned).collect::<Vec<_>>();
                    let values = fields.as_ref().map_or(current.clone(), |f| nt.apply(&current, f));
                    meguri_engine::notes::update_in(
                        ctx.scope(),
                        &meguri_engine::notes::UpdateNote {
                            id: nid,
                            fields: values,
                            tags: tags.unwrap_or_else(|| note.tags.split_whitespace().map(str::to_owned).collect()),
                        },
                    )
                    .map(|_| ())
                },
            )
            .unwrap();
    }

    /// W3: the listed notes that still exist removed with their cards, their
    /// graves added, as one "Delete Note" op; none left, no op at all.
    fn delete_notes(&self, args: &DeleteNotesArgs) {
        let existing: Vec<NoteId> = {
            let reader = self.c().reader(ReaderRole::Query).unwrap();
            args.notes
                .iter()
                .map(|r| self.note(r.0))
                .filter(|nid| reader.note(*nid).unwrap().is_some())
                .collect()
        };
        if existing.is_empty() {
            return;
        }
        meguri_engine::notes::delete(self.c(), existing, OpSource::UserUndoable).unwrap();
    }

    /// `sched/answering` A2 to A5 for its A8 example, the only answer the
    /// scenarios give: a new card answered Good with the default preset and
    /// FSRS on, whose chosen state is learning with one step left, a delay
    /// of 600 s and the memory state (2.3065, 2.118104). Its learning fuzz is
    /// `sched/fuzz` Z7's first example (card 1894701600000, no review yet:
    /// 739 s).
    fn answer(&self, spec: &AnswerSpec) {
        assert_eq!(spec.rating, Rating::Good, "the stand-in answers Good only");
        let cid = self.card(spec.card.0);
        assert_eq!(cid, CardId(1_894_701_600_000), "Z7's example card");
        let time_ms = u32::try_from(spec.time_ms.min(60_000)).unwrap();
        self.c()
            .run(
                OpSpec::named("answer_card", OpSource::UserUndoable).undo_step(ANSWER_CARD, StepKind::Answer),
                move |ctx| {
                    let now = ctx.now().whole_secs().0;
                    let mut card = ctx.scope().card(cid)?.expect("the card exists");
                    assert_eq!(
                        (card.card_type, card.reps),
                        (CardType::New, 0),
                        "Z7's example: a new card"
                    );
                    let (delay, fuzzed, stability, difficulty) = (600, 739, 2.3065_f32, 2.118_104_f32);
                    let position = u32::try_from(card.due.max(0)).unwrap();
                    card.card_type = CardType::Learning;
                    card.queue = CardQueue::Learning;
                    card.due = i32::try_from(now + fuzzed).unwrap();
                    card.remaining_steps = 1;
                    card.reps += 1;
                    card.data.new_position = Some(position);
                    card.data.memory_state = Some(MemoryState { stability, difficulty });
                    card.data.desired_retention = Some(0.9);
                    card.data.decay = Some(0.1542);
                    card.data.last_review_secs = Some(now);
                    ctx.scope().put(&card)?;
                    // A5: the difficulty shifted, in f32, halves away from zero.
                    let factor = (((difficulty - 1.0) / 9.0 + 0.1) * 1000.0).round();
                    ctx.scope().add_revlog(&RevlogEntry {
                        id: RevlogId(0),
                        card_id: cid,
                        usn: Usn::PENDING,
                        button: 3,
                        interval: -delay,
                        last_interval: 0,
                        ease_factor: factor as u32,
                        answer_millis: time_ms,
                        kind: RevlogKind::Learning,
                    })?;
                    Ok(())
                },
            )
            .unwrap();
    }

    /// V1: undo or redo with scope `Global`. Only a refusal by the step's own
    /// check is `changed_externally`; every other refusal is the empty one.
    fn restore(&self, op: &str) -> Value {
        let c = self.c();
        let (status, empty) = match op {
            "undo" => (c.peek_undo(UndoScope::Global), "nothing_to_undo"),
            _ => (c.peek_redo(UndoScope::Global), "nothing_to_redo"),
        };
        if status.unwrap().name().is_none() {
            return json!({ op: { "error": empty } });
        }
        let done = match op {
            "undo" => c.undo(UndoScope::Global).map(drop),
            _ => c.redo(UndoScope::Global).map(drop),
        };
        match done {
            Ok(()) => json!({}),
            Err(EngineError::UndoUnavailable {
                reason: DisabledReason::ChangedExternally,
            }) => json!({ op: { "error": "changed_externally" } }),
            Err(EngineError::UndoUnavailable { .. }) => json!({ op: { "error": empty } }),
            Err(error) => panic!("{op} failed: {error}"),
        }
    }

    /// V2, V4: the step ids an undo and a redo would apply now. A V4 id is
    /// its name's words in lower case joined by `_` ("Update Config":
    /// `update_config`).
    fn undo_status(&self) -> Value {
        let id = |status: UndoStatus| match status.name() {
            Some(name) => json!(name.to_lowercase().replace([' ', '/'], "_")),
            None => Value::Null,
        };
        json!({
            "undo": id(self.c().peek_undo(UndoScope::Global).unwrap()),
            "redo": id(self.c().peek_redo(UndoScope::Global).unwrap()),
        })
    }

    fn note(&self, r: LogicalRef) -> NoteId {
        assert_eq!(r.kind, RefKind::Note);
        *self.notes.get(r.index()).expect("a note the scenario created")
    }

    fn card(&self, r: LogicalRef) -> CardId {
        assert_eq!(r.kind, RefKind::Card);
        *self.cards.get(r.index()).expect("a card the scenario created")
    }

    /// The references a request names, or every note and card of `kinds`.
    fn refs(&self, request: &ObservationRequest, kinds: &[RefKind]) -> Vec<LogicalRef> {
        if let ObservationArgs::Refs(refs) = &request.args {
            return refs.clone();
        }
        let mut refs = Vec::new();
        for kind in kinds {
            let count = match kind {
                RefKind::Note => self.notes.len(),
                RefKind::Card => self.cards.len(),
            };
            refs.extend((1..=count).map(|n| LogicalRef {
                kind: *kind,
                n: u32::try_from(n).unwrap(),
            }));
        }
        refs
    }

    fn query(&self, sql: &str, params: &[SqlValue]) -> RawRows {
        let reader = self.c().reader(ReaderRole::Query).unwrap();
        reader.execute_raw(sql, params).unwrap()
    }

    /// A deck id as O4 shows it: the deck's name, `null` for 0.
    fn deck_name(&self, id: &SqlValue) -> Value {
        if *id == SqlValue::Integer(0) {
            return Value::Null;
        }
        match &self
            .query("SELECT name FROM decks WHERE id = ?", std::slice::from_ref(id))
            .rows[..]
        {
            [row] => match &row[0] {
                SqlValue::Text(name) => json!(name.replace('\u{1f}', "::")),
                other => panic!("a deck name: {other:?}"),
            },
            _ => panic!("no deck {id:?}"),
        }
    }

    /// A note or card id as its logical id.
    fn logical(&self, kind: RefKind, id: &SqlValue) -> Value {
        let SqlValue::Integer(id) = id else {
            panic!("an id: {id:?}")
        };
        let (ids, letter): (Vec<i64>, char) = match kind {
            RefKind::Note => (self.notes.iter().map(|n| n.0).collect(), 'n'),
            RefKind::Card => (self.cards.iter().map(|c| c.0).collect(), 'c'),
        };
        let i = ids.iter().position(|x| x == id).expect("an id the scenario created");
        json!(format!("#{letter}{}", i + 1))
    }

    /// O4 `card`: the stored rows, `null` for a card that is gone.
    fn card_observation(&self, request: &ObservationRequest) -> Value {
        let mut out = Map::new();
        for r in self.refs(request, &[RefKind::Card]) {
            let rows = self.query(
                "SELECT id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, \
                 left, odue, odid, flags, data FROM cards WHERE id = ?",
                &[SqlValue::Integer(self.card(r).0)],
            );
            let value = match &rows.rows[..] {
                [] => Value::Null,
                [row] => {
                    let mut card = Map::new();
                    for (column, cell) in rows.columns.iter().zip(row) {
                        let value = match (column.as_str(), cell) {
                            ("id", cell) => self.logical(RefKind::Card, cell),
                            ("nid", cell) => self.logical(RefKind::Note, cell),
                            ("did" | "odid", cell) => self.deck_name(cell),
                            ("data", SqlValue::Text(text)) if text.is_empty() => json!({}),
                            ("data", SqlValue::Text(text)) => {
                                serde_json::from_str(text).unwrap_or_else(|_| json!(text))
                            }
                            (_, SqlValue::Integer(v)) => json!(v),
                            (column, other) => panic!("{column}: {other:?}"),
                        };
                        card.insert(column.clone(), value);
                    }
                    Value::Object(card)
                }
                _ => unreachable!("one row per id"),
            };
            out.insert(r.to_string(), value);
        }
        Value::Object(out)
    }

    /// O4 `revlog`: each card's review-log rows in id order.
    fn revlog_observation(&self, request: &ObservationRequest) -> Value {
        let mut out = Map::new();
        for r in self.refs(request, &[RefKind::Card]) {
            let rows = self.query(
                "SELECT id, cid, usn, ease, ivl, lastIvl, factor, time, type FROM revlog WHERE cid = ? ORDER BY id",
                &[SqlValue::Integer(self.card(r).0)],
            );
            let entries: Vec<Value> = rows
                .rows
                .iter()
                .map(|row| {
                    let mut entry = Map::new();
                    for (column, cell) in rows.columns.iter().zip(row) {
                        let value = match (column.as_str(), cell) {
                            ("cid", cell) => self.logical(RefKind::Card, cell),
                            (_, SqlValue::Integer(v)) => json!(v),
                            (column, other) => panic!("{column}: {other:?}"),
                        };
                        entry.insert(column.clone(), value);
                    }
                    Value::Object(entry)
                })
                .collect();
            out.insert(r.to_string(), Value::Array(entries));
        }
        Value::Object(out)
    }

    /// O4 `ids`: the real ids behind the references, or behind every one.
    fn ids_observation(&self, request: &ObservationRequest) -> Value {
        let mut out = Map::new();
        for r in self.refs(request, &[RefKind::Note, RefKind::Card]) {
            let id = match r.kind {
                RefKind::Note => self.note(r).0,
                RefKind::Card => self.card(r).0,
            };
            out.insert(r.to_string(), json!(id));
        }
        Value::Object(out)
    }

    /// O8 `db_dump`, through the runner's own formatting.
    fn db_dump(&self, request: &ObservationRequest) -> Value {
        let tables: Vec<String> = match &request.args {
            ObservationArgs::Mapping(map) => map["tables"]
                .as_array()
                .expect("a table list")
                .iter()
                .map(|t| t.as_str().unwrap().to_owned())
                .collect(),
            _ => DUMP_TABLES.iter().map(|t| (*t).to_owned()).collect(),
        };
        let mut out = Map::new();
        for table in tables {
            let rows = self.query(&format!("SELECT * FROM {table}"), &[]);
            out.insert(table.clone(), db_dump_table(&table, &rows).unwrap());
        }
        Value::Object(out)
    }

    fn observe(&self, requests: &[ObservationRequest]) -> Value {
        let mut out = Map::new();
        for request in requests {
            let value = match request.kind {
                ObservationKind::UndoStatus => self.undo_status(),
                ObservationKind::DbDump => self.db_dump(request),
                ObservationKind::Card => self.card_observation(request),
                ObservationKind::Revlog => self.revlog_observation(request),
                ObservationKind::Ids => self.ids_observation(request),
                other => panic!("this test observes no {}", other.name()),
            };
            assert!(out.insert(request.kind.name().to_owned(), value).is_none());
        }
        Value::Object(out)
    }

    fn step(&mut self, step: &Step) -> Value {
        match step {
            Step::AddNote(spec) => {
                let note = self.notes.len() + 1;
                let first = self.cards.len() + 1;
                self.add_note(spec);
                json!({"add_note": {"note": format!("#n{note}"), "cards": (first..=self.cards.len()).map(|i| format!("#c{i}")).collect::<Vec<_>>()}})
            }
            Step::Advance(duration) => {
                self.h.clock.step_ms(duration.seconds() * 1000);
                json!({})
            }
            Step::DbExec(args) => {
                let params = args
                    .params
                    .iter()
                    .flatten()
                    .map(|param| match param {
                        SqlParam::Text(text) => SqlValue::Text(text.clone()),
                        SqlParam::Integer(value) => SqlValue::Integer(*value),
                        SqlParam::Real(value) => SqlValue::Real(*value),
                        SqlParam::Null => SqlValue::Null,
                    })
                    .collect();
                json!({ "db_exec": self.db_exec(&args.sql, params) })
            }
            Step::SetConfig(args) => {
                self.set_config(&args.key, &args.value, args.undoable);
                json!({})
            }
            Step::RemoveConfig(args) => {
                self.remove_config(&args.key);
                json!({})
            }
            Step::EditNote(args) => {
                self.edit_note(args);
                json!({})
            }
            Step::DeleteNotes(args) => {
                self.delete_notes(args);
                json!({})
            }
            Step::Answer(spec) => {
                self.answer(spec);
                json!({})
            }
            Step::Undo(_) => self.restore("undo"),
            Step::Redo(_) => self.restore("redo"),
            Step::Observe(requests) => self.observe(requests),
            other => panic!("this test does not play {}", other.name()),
        }
    }
}

/// Plays `undo/<name>` and compares its observations with the golden's, as
/// T0 does (dsl.md D10, observations.md O7).
fn play(name: &str) {
    let id = if name.contains("/") {
        name.to_owned()
    } else {
        format!("undo/{name}")
    };
    let loaded = scenario::load(&conformance().join(format!("scenarios/{id}.yaml"))).unwrap();
    let golden: Value = meguri_testkit::observe::parse_json(
        &std::fs::read_to_string(conformance().join(format!("goldens/{id}.json"))).unwrap(),
    )
    .unwrap();
    assert_eq!(
        golden["scenario_sha256"].as_str(),
        Some(loaded.sha256.as_str()),
        "the golden is fresh"
    );
    // The harness clock: 2030-01-15T10:00:00 UTC, frozen.
    assert_eq!(golden["clock"]["start"], json!("2030-01-15T10:00:00"));
    assert_eq!(golden["clock"]["mode"], json!("frozen"));
    assert_eq!(golden["tz"], json!("UTC"));

    let mut player = Player::new(loaded);
    player.setup();
    let steps: Vec<Step> = player.scenario.scenario.steps.clone();
    let observed: Vec<Value> = steps.iter().map(|step| player.step(step)).collect();

    let suite = Suite::open(&conformance()).unwrap();
    let comparison = diff::compare(
        &id,
        &golden["observations"],
        &json!({ "steps": observed }),
        &CompareOptions {
            set: &player.scenario.scenario.compare.set,
            deviations: &suite.deviations,
            overlay: &[],
        },
    );
    assert!(
        comparison.differences.is_empty(),
        "{id}: differences from the golden: {:#?}",
        comparison
            .differences
            .iter()
            .map(|d| (&d.pointer, &d.expected, &d.actual))
            .collect::<Vec<_>>()
    );
    // Exactly the deviations listed for the scenario applied (`apple_overlay`
    // does nothing here).
    let listed: Vec<&str> = suite
        .deviations
        .iter()
        .filter(|d| d.id != "fp-libm-boundary" && d.scenarios.iter().any(|p| diff::pointer::glob(p, &id)))
        .map(|d| d.id.as_str())
        .collect();
    let applied: Vec<&str> = comparison
        .deviations_applied
        .iter()
        .map(String::as_str)
        .filter(|d| *d != "fp-libm-boundary")
        .collect();
    assert_eq!(applied, listed, "{id}: the deviations that applied");
}

#[test]
fn col_mod() {
    play("col-mod");
}

#[test]
fn config_writes() {
    play("config-writes");
}

#[test]
fn delete_note_existing_grave() {
    play("delete-note-existing-grave");
}

#[test]
fn delete_notes() {
    play("delete-notes");
}

#[test]
fn edit_note() {
    play("edit-note");
}

#[test]
fn edit_note_merge() {
    play("edit-note-merge");
}

#[test]
fn edit_note_new_card() {
    play("edit-note-new-card");
}

#[test]
fn redo_cleared() {
    play("redo-cleared");
}

#[test]
fn revlog_taken_id() {
    play("revlog-taken-id");
}

#[test]
fn thirty_steps() {
    play("thirty-steps");
}

/// No undo scenario goes unplayed: each has its test above.
#[test]
fn every_undo_scenario_is_played_here() {
    let this =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/undo_conformance.rs")).unwrap();
    let mut names: Vec<String> = std::fs::read_dir(conformance().join("scenarios/undo"))
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter_map(|f| f.strip_suffix(".yaml").map(str::to_owned))
        .filter(|f| f != "area")
        .collect();
    names.sort();
    assert_eq!(names.len(), 10, "{names:?}");
    for name in names {
        assert!(this.contains(&format!("play(\"{name}\")")), "undo/{name} is not played");
    }
}

#[test]
fn undo_add_note() {
    play("notes/records/undo-add");
}
