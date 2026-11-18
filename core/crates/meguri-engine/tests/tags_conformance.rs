//! Every tags scenario (`conformance/scenarios/tags/`; `specs/tags/tags.md`,
//! `specs/tags/flags-and-marked.md`, `specs/conformance/tag-ops.md`) played
//! on the engine's tag and flag ops and compared with the oracle's golden,
//! the deviations of `conformance/deviations.yaml` applied (dsl.md D13).
//!
//! T0 runs them once Meguri's executor has the new collection (U28), the
//! note type setup (U29) and `add_note`/`edit_note` (U27); its tags adapters
//! already run the tag ops (`meguri-testkit/src/exec/tags.rs`). Until then
//! this file plays each scenario here with stand-ins for what those units
//! build:
//!
//! - **The new collection** (dsl.md D6): a copy of the schema-18 fixture
//!   whose notes, cards, review log, graves and tags are removed and whose
//!   `col.mod` is 0.
//! - **Setup**: note types live here only (the scenarios set up nothing
//!   else).
//! - **`add_note`, `edit_note`**: ops that store the note's fields as saved,
//!   its tags as a note save stores them (`tags/tags` T4, this unit's
//!   `note_tags_for_save`), and for a new note one card per template whose
//!   front shows a non-empty field, at the note's new position in the
//!   default deck.
//!
//! The tag ops, `set_flag` and `tag_tree` are the engine's own (`tags`,
//! `flags`), through the same calls the app makes.

mod common;

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::Duration;

use common::Harness;
use common::tagging::setup_spec;
use meguri_engine::executor::catalog;
use meguri_engine::flags::{self, InvalidFlag};
use meguri_engine::tags::{self, InvalidName, RunOptions};
use meguri_engine::{ClearCause, Collection};
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::clock::Clock;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::raw::{RawRows, SqlValue};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::diff::{self, CompareOptions};
use meguri_testkit::exec::{Executor, MeguriExecutor, db_dump_table, db_exec_rows, tag_tree_value};
use meguri_testkit::scenario::{
    self, EditNoteArgs, Fields, LoadedScenario, LogicalRef, NoteSpec, ObservationArgs, ObservationKind,
    ObservationRequest, RefKind, SqlParam, Step,
};
use meguri_testkit::suite::{Status, Suite};
use meguri_text::checksum::{note_checksum, sort_text_of};
use meguri_text::field_empty::counts_as_empty;
use meguri_text::normalize::field_as_saved;
use serde_json::{Map, Value, json};

fn conformance() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance")
}

/// A note type of the scenario's setup, kept here only.
#[derive(Clone, Debug)]
struct Notetype {
    id: NotetypeId,
    fields: Vec<String>,
    fronts: Vec<String>,
}

impl Notetype {
    /// The field values as saved (text C1) after `fields` replaces from the
    /// first field (a list) or the named fields (a mapping).
    fn values(&self, fields: &Fields) -> Vec<String> {
        let mut values = vec![String::new(); self.fields.len()];
        match fields {
            Fields::List(list) => {
                for (slot, value) in values.iter_mut().zip(list) {
                    slot.clone_from(value);
                }
            }
            Fields::Map(map) => {
                for (name, value) in map {
                    let i = self.fields.iter().position(|f| f == name).expect("a field");
                    values[i].clone_from(value);
                }
            }
        }
        values.iter().map(|v| field_as_saved(v, true).into_owned()).collect()
    }

    /// The ordinals of the templates whose front shows a non-empty field.
    /// The scenarios' fronts are `{{Name}}` replacements only.
    fn card_ords(&self, values: &[String]) -> Vec<u16> {
        (0..self.fronts.len())
            .filter(|&ord| {
                let name = self.fronts[ord].trim_start_matches("{{").trim_end_matches("}}");
                self.fields
                    .iter()
                    .position(|f| f == name)
                    .is_some_and(|i| !counts_as_empty(&values[i]))
            })
            .map(|ord| u16::try_from(ord).unwrap())
            .collect()
    }
}

fn new_card(note_id: NoteId, ord: u16, position: i32) -> Card {
    Card {
        id: CardId(0),
        note_id,
        deck_id: DeckId(1),
        ord,
        mtime: EpochSecs(0),
        usn: Usn::PENDING,
        card_type: CardType::New,
        queue: CardQueue::New,
        due: position,
        interval: 0,
        ease_factor: 0,
        reps: 0,
        lapses: 0,
        remaining_steps: 0,
        original_due: 0,
        original_deck_id: DeckId(0),
        flags: 0,
        data: CardData::default(),
    }
}

struct Player {
    h: Harness,
    scenario: LoadedScenario,
    notetypes: BTreeMap<String, Notetype>,
    notes: Vec<NoteId>,
    cards: Vec<CardId>,
    next_position: i32,
}

impl Player {
    fn new(scenario: LoadedScenario) -> Player {
        Player {
            h: Harness::open(),
            scenario,
            notetypes: BTreeMap::new(),
            notes: Vec::new(),
            cards: Vec::new(),
            next_position: 1,
        }
    }

    fn c(&self) -> &Collection {
        &self.h.collection
    }

    /// W1: one statement on the writer, committed at once.
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

    fn new_collection(&self) {
        for table in ["notes", "cards", "revlog", "graves", "tags"] {
            self.db_exec(&format!("DELETE FROM {table}"), Vec::new());
        }
        self.db_exec("UPDATE col SET mod = 0", Vec::new());
    }

    fn setup(&mut self) {
        let setup = self.scenario.scenario.setup.clone();
        assert_eq!(
            setup.sections(),
            ["notetypes"],
            "the tags scenarios set up note types only"
        );
        let first = self.h.clock.now_ms() + 6;
        for (i, notetype) in setup.notetypes.iter().flatten().enumerate() {
            self.notetypes.insert(
                notetype.name.clone(),
                Notetype {
                    id: NotetypeId(first + i64::try_from(i).unwrap()),
                    fields: notetype.fields.clone(),
                    fronts: notetype.templates.iter().map(|t| t.front.clone()).collect(),
                },
            );
        }
    }

    /// D7.2 `add_note` in the default deck: the note, its tags as a note
    /// save stores them, and its cards at the next new position.
    fn add_note(&mut self, spec: &NoteSpec) -> Value {
        assert!(spec.deck.is_none(), "the tags scenarios add to the default deck");
        let notetype = self.notetypes[&spec.notetype].clone();
        let values = notetype.values(&spec.fields);
        let items = spec.tags.clone();
        let position = self.next_position;
        self.next_position += 1;
        let guid = format!("u30-{}", self.notes.len() + 1);
        let (nid, cids) = self
            .c()
            .run(setup_spec(), move |ctx| {
                let tags = tags::note_tags_for_save(ctx.scope(), &items)?;
                let note = Note {
                    id: NoteId(0),
                    guid,
                    notetype_id: notetype.id,
                    mtime: EpochSecs(0),
                    usn: Usn::PENDING,
                    tags,
                    fields: values.join("\u{1f}"),
                    sort_field: sort_text_of(&values, 0).into_owned(),
                    checksum: note_checksum(&values[0]),
                    flags: 0,
                    data: String::new(),
                };
                let nid = ctx.scope().add_note(&note)?;
                let mut cids = Vec::new();
                for ord in notetype.card_ords(&values) {
                    cids.push(ctx.scope().add_card(&new_card(nid, ord, position))?);
                }
                Ok((nid, cids))
            })
            .unwrap()
            .value;
        self.notes.push(nid);
        let first = self.cards.len();
        self.cards.extend(&cids);
        let cards: Vec<String> = (first..self.cards.len()).map(|i| format!("#c{}", i + 1)).collect();
        json!({ "add_note": { "note": format!("#n{}", self.notes.len()), "cards": cards } })
    }

    /// W2 for a tags-only edit: the note saved with the new tag list (T4).
    fn edit_note(&self, args: &EditNoteArgs) -> Value {
        assert!(args.fields.is_none(), "the tags scenarios edit tags only");
        let nid = self.note(args.note.0);
        let items = args.tags.clone().unwrap_or_default();
        self.c()
            .run(setup_spec(), move |ctx| {
                let mut note = ctx.scope().note(nid)?.expect("the note exists");
                note.tags = tags::note_tags_for_save(ctx.scope(), &items)?;
                ctx.scope().put(&note)?;
                Ok(())
            })
            .unwrap();
        json!({})
    }

    fn note(&self, r: LogicalRef) -> NoteId {
        assert_eq!(r.kind, RefKind::Note);
        *self.notes.get(r.index()).expect("a note the scenario created")
    }

    fn card(&self, r: LogicalRef) -> CardId {
        assert_eq!(r.kind, RefKind::Card);
        *self.cards.get(r.index()).expect("a card the scenario created")
    }

    fn query(&self, sql: &str, params: &[SqlValue]) -> RawRows {
        self.c()
            .reader(ReaderRole::Query)
            .unwrap()
            .execute_raw(sql, params)
            .unwrap()
    }

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

    /// O4 `card`: every card's stored row.
    fn card_observation(&self, request: &ObservationRequest) -> Value {
        assert_eq!(
            request.args,
            ObservationArgs::None,
            "the tags scenarios observe every card"
        );
        let mut out = Map::new();
        for n in 1..=self.cards.len() {
            let r = LogicalRef {
                kind: RefKind::Card,
                n: u32::try_from(n).unwrap(),
            };
            let rows = self.query(
                "SELECT id, nid, did, ord, mod, usn, type, queue, due, ivl, factor, reps, lapses, \
                 left, odue, odid, flags, data FROM cards WHERE id = ?",
                &[SqlValue::Integer(self.card(r).0)],
            );
            let [row] = &rows.rows[..] else {
                panic!("one row for {r}")
            };
            let mut card = Map::new();
            for (column, cell) in rows.columns.iter().zip(row) {
                let value = match (column.as_str(), cell) {
                    ("id", cell) => self.logical(RefKind::Card, cell),
                    ("nid", cell) => self.logical(RefKind::Note, cell),
                    ("did" | "odid", cell) => self.deck_name(cell),
                    ("data", SqlValue::Text(text)) if text.is_empty() => json!({}),
                    ("data", SqlValue::Text(text)) => serde_json::from_str(text).unwrap(),
                    (_, SqlValue::Integer(v)) => json!(v),
                    (column, other) => panic!("{column}: {other:?}"),
                };
                card.insert(column.clone(), value);
            }
            out.insert(r.to_string(), Value::Object(card));
        }
        Value::Object(out)
    }

    fn db_dump(&self, request: &ObservationRequest) -> Value {
        let ObservationArgs::Mapping(map) = &request.args else {
            panic!("the tags scenarios name their tables")
        };
        let mut out = Map::new();
        for table in map["tables"].as_array().unwrap() {
            let table = table.as_str().unwrap();
            let rows = self.query(&format!("SELECT * FROM {table}"), &[]);
            out.insert(table.to_owned(), db_dump_table(table, &rows).unwrap());
        }
        Value::Object(out)
    }

    fn observe(&self, requests: &[ObservationRequest]) -> Value {
        let mut out = Map::new();
        for request in requests {
            let value = match request.kind {
                ObservationKind::DbDump => self.db_dump(request),
                ObservationKind::Card => self.card_observation(request),
                ObservationKind::TagTree => tag_tree_value(&tags::tag_tree(self.c()).unwrap()),
                other => panic!("this test observes no {}", other.name()),
            };
            assert!(out.insert(request.kind.name().to_owned(), value).is_none());
        }
        Value::Object(out)
    }

    fn count(op: &str, value: usize) -> Value {
        json!({ op: { "count": value } })
    }

    fn step(&mut self, step: &Step) -> Value {
        let c = self.h.collection.clone();
        let options = RunOptions::default();
        match step {
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
            Step::AddNote(spec) => self.add_note(spec),
            Step::EditNote(args) => self.edit_note(args),
            Step::Observe(requests) => self.observe(requests),
            Step::AddNoteTags(args) => {
                let notes: Vec<NoteId> = args.notes.iter().map(|r| self.note(r.0)).collect();
                Player::count(
                    "add_note_tags",
                    tags::add_to_notes(&c, &notes, &args.tags, &options).unwrap().value,
                )
            }
            Step::RemoveNoteTags(args) => {
                let notes: Vec<NoteId> = args.notes.iter().map(|r| self.note(r.0)).collect();
                let removed = tags::remove_from_notes(&c, &notes, &args.tags, &options).unwrap();
                Player::count("remove_note_tags", removed.value)
            }
            Step::DeleteTags(args) => {
                Player::count("delete_tags", tags::delete(&c, &args.tags, &options).unwrap().value)
            }
            Step::RenameTag(args) => {
                if tags::check_new_name(&args.to).is_err() {
                    return json!({ "rename_tag": { "error": InvalidName::KIND } });
                }
                Player::count(
                    "rename_tag",
                    tags::rename(&c, &args.from, &args.to, &options).unwrap().value,
                )
            }
            Step::ReparentTags(args) => {
                let target = (!args.parent.is_empty()).then_some(args.parent.as_str());
                let moved = tags::reparent(&c, &args.tags, target, &options).unwrap();
                Player::count("reparent_tags", moved.value)
            }
            Step::ClearUnusedTags(_) => {
                Player::count("clear_unused_tags", tags::clear_unused(&c, &options).unwrap().value)
            }
            Step::SetTagCollapsed(args) => {
                tags::set_collapsed(&c, &args.tags, args.collapsed).unwrap();
                json!({})
            }
            Step::SetFlag(args) => {
                if flags::check_flag(args.flag).is_err() {
                    return json!({ "set_flag": { "error": InvalidFlag::KIND } });
                }
                let cards: Vec<CardId> = args.cards.iter().map(|r| self.card(r.0)).collect();
                Player::count(
                    "set_flag",
                    flags::set_flag(&c, &cards, args.flag, &options).unwrap().value,
                )
            }
            other => panic!("this test does not play {}", other.name()),
        }
    }
}

/// Plays `tags/<name>` and compares its observations with the golden's, as
/// T0 does (dsl.md D10, observations.md O7).
fn play(name: &str) {
    let id = format!("tags/{name}");
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
    assert_eq!(golden["clock"]["start"], json!("2030-01-15T10:00:00"));
    assert_eq!(golden["clock"]["mode"], json!("frozen"));
    assert_eq!(golden["tz"], json!("UTC"));

    let mut player = Player::new(loaded);
    player.new_collection();
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
fn bulk_add() {
    play("bulk/add");
}

#[test]
fn bulk_remove() {
    play("bulk/remove");
}

#[test]
fn canon_normalize() {
    play("canon/normalize");
}

#[test]
fn canon_spelling() {
    play("canon/spelling");
}

#[test]
fn clear_unused_parents() {
    play("clear-unused/parents");
}

#[test]
fn collapse_set() {
    play("collapse/set");
}

#[test]
fn delete_children() {
    play("delete/children");
}

#[test]
fn flags_set() {
    play("flags/set");
}

#[test]
fn marked_mark() {
    play("marked/mark");
}

#[test]
fn rename_basic() {
    play("rename/basic");
}

#[test]
fn rename_case_only() {
    play("rename/case-only");
}

#[test]
fn rename_existing_spelling() {
    play("rename/existing-spelling");
}

#[test]
fn rename_implicit_parent() {
    play("rename/implicit-parent");
}

#[test]
fn rename_new_casing() {
    play("rename/new-casing");
}

#[test]
fn rename_refused() {
    play("rename/refused");
}

#[test]
fn rename_rows() {
    play("rename/rows");
}

#[test]
fn reparent_existing_spelling() {
    play("reparent/existing-spelling");
}

#[test]
fn reparent_rules() {
    play("reparent/rules");
}

#[test]
fn tree_structure() {
    play("tree/structure");
}

/// Every tags scenario has a play test above or explicit prepared pending coverage.
/// Pending completion cases are not parity passes or U84 acceptance.
#[test]
fn every_tags_scenario_has_play_or_explicit_pending_coverage() {
    let played: [&str; 19] = [
        "bulk/add",
        "bulk/remove",
        "canon/normalize",
        "canon/spelling",
        "clear-unused/parents",
        "collapse/set",
        "delete/children",
        "flags/set",
        "marked/mark",
        "rename/basic",
        "rename/case-only",
        "rename/existing-spelling",
        "rename/implicit-parent",
        "rename/new-casing",
        "rename/refused",
        "rename/rows",
        "reparent/existing-spelling",
        "reparent/rules",
        "tree/structure",
    ];
    // Only these six prepared cases await the tag_complete adapter (U84).
    let pending: [&str; 6] = [
        "complete/components",
        "complete/filter-sequence",
        "complete/folding",
        "complete/order-limit",
        "complete/registry",
        "complete/separators",
    ];
    let suite = Suite::open(&conformance()).unwrap();
    let files = suite.discover(&["tags".to_owned()]).unwrap();
    let mut expected: Vec<String> = played
        .iter()
        .chain(&pending)
        .map(|name| format!("tags/{name}"))
        .collect();
    expected.sort();
    let names: Vec<&str> = files.iter().map(|file| file.rel.as_str()).collect();
    assert_eq!(names, expected, "the exact 25 tags scenario IDs");

    let this =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/tags_conformance.rs")).unwrap();
    for name in played {
        assert!(this.contains(&format!("play(\"{name}\")")), "tags/{name} is not played");
    }

    let executor = MeguriExecutor::default();
    for name in pending {
        let id = format!("tags/{name}");
        let file = files.iter().find(|file| file.rel == id).unwrap();
        let loaded = scenario::load(&file.path).unwrap();
        scenario::check_id(&loaded, &id).unwrap();
        let needs = loaded.scenario.needs();
        assert!(
            needs.observations.contains("tag_complete"),
            "{id}: typed completion request required"
        );
        // Check missing adapter capabilities before Suite preflight.
        let missing = executor.missing(&needs);
        assert_eq!(
            missing,
            scenario::Needs {
                observations: ["tag_complete".to_owned()].into(),
                ..Default::default()
            },
            "{id}: pending coverage requires missing tag_complete; add a play test when it becomes executable"
        );
        // Suite preflight checks the parsed ID, golden hash and oracle pin
        // before classifying the scenario; no setup or steps run while pending.
        let outcome = suite.check(file, &executor);
        assert_eq!(outcome.status, Status::Pending, "{id}: {}", outcome.message);
        assert!(!outcome.active, "{id}: prepared completion must remain inactive");
        assert_eq!(outcome.scenario_sha256.as_deref(), Some(loaded.sha256.as_str()));
        assert_eq!(outcome.missing.as_ref(), Some(&missing));
    }
}
