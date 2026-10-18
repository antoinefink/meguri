//! The tags area's adapters on Meguri's executor
//! (`specs/conformance/tag-ops.md` G1 to G9): the eight ops and `tag_tree`
//! on a session's collection, their counts, rejections and stamps. The
//! committed tags scenarios need the new collection, the note type setup and
//! `add_note` (U27 to U29), so these runs open the schema-18 fixture and add
//! notes through a stand-in `add_note` that stores tags as a note save does
//! (`tags/tags` T4). The scenarios' goldens are checked against the engine's
//! ops by meguri-engine's `tests/tags_conformance.rs`.

use std::fs;
use std::path::{Path, PathBuf};

use meguri_engine::tags::note_tags_for_save;
use meguri_model::card::{Card, CardQueue, CardType};
use meguri_model::card_data::CardData;
use meguri_model::ids::{CardId, DeckId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_testkit::exec::{ExecError, Executor, MeguriExecutor, OpResult, Session};
use meguri_testkit::scenario::{self, Fields, LoadedScenario, Step};
use serde_json::{Value, json};

fn repo_fixture() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../conformance/fixtures/storage/schema-18.anki2")
}

fn load(dir: &Path, id: &str, steps: &str) -> LoadedScenario {
    let storage = dir.join("fixtures/storage");
    fs::create_dir_all(&storage).unwrap();
    fs::copy(repo_fixture(), storage.join("schema-18.anki2")).unwrap();
    let path = dir.join("scenarios").join(format!("{id}.yaml"));
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        format!(
            "id: {id}\nclock: {{tz: UTC, start: \"2030-01-15T10:00:00\"}}\nsteps:\n  \
             - open: {{fixture: storage/schema-18.anki2}}\n  - db_exec: {{sql: \"DELETE FROM notes\"}}\n  \
             - db_exec: {{sql: \"DELETE FROM cards\"}}\n  - db_exec: {{sql: \"DELETE FROM tags\"}}\n{steps}"
        ),
    )
    .unwrap();
    scenario::load(&path).unwrap()
}

/// A stand-in for U27's `add_note`: one note with the list's fields, its
/// tags as a note save stores them, and one card at the next position.
fn add_note(session: &mut Session<'_>, step: &Step) -> Result<OpResult, ExecError> {
    let Step::AddNote(spec) = step else {
        return Err(ExecError::new("internal_error", "add_note stand-in"));
    };
    let Fields::List(fields) = &spec.fields else {
        return Err(ExecError::new("internal_error", "the stand-in takes a field list"));
    };
    let fields = fields.clone();
    let tags = spec.tags.clone();
    let position = i32::try_from(session.notes.len() + 1).unwrap();
    let collection = session.engine_collection()?;
    let (nid, cid) = collection
        .run(
            meguri_engine::OpSpec::named("test_note", meguri_engine::OpSource::UserNonUndoable).skip_undo(),
            move |ctx| {
                let scope = ctx.scope();
                let note = Note {
                    guid: format!("stand-in-{position}"),
                    notetype_id: NotetypeId(1),
                    fields: fields.join("\u{1f}"),
                    sort_field: fields[0].clone(),
                    tags: note_tags_for_save(scope, &tags).unwrap(),
                    ..Note::default()
                };
                let nid = scope.add_note(&note).unwrap();
                let cid = scope
                    .add_card(&Card {
                        id: CardId(0),
                        note_id: nid,
                        deck_id: DeckId(1),
                        ord: 0,
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
                    })
                    .unwrap();
                Ok((nid, cid))
            },
        )
        .map_err(|e| ExecError::new("test_standin", e.to_string()))?
        .value;
    session.notes.push(nid.0);
    session.cards.push(cid.0);
    Ok(OpResult::Output(json!({
        "note": format!("#n{}", session.notes.len()),
        "cards": [format!("#c{}", session.cards.len())],
    })))
}

fn executor() -> MeguriExecutor {
    let mut registry = MeguriExecutor::area_registry(&["clock", "storage", "tags"]);
    registry.op("add_note", add_note);
    MeguriExecutor::with(registry)
}

fn run(id: &str, steps: &str) -> Vec<Value> {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(dir.path(), id, steps);
    let out = executor().run(&loaded).unwrap();
    // The fixture's open and the three deletes come first.
    out["steps"].as_array().unwrap()[4..].to_vec()
}

#[test]
fn the_tag_ops_count_reject_and_stamp_as_g1_to_g8_say() {
    let steps = run(
        "tags/adapters",
        r##"  - add_note: {notetype: X, fields: [a], tags: [foo, "foo::kid"]}
  - add_note: {notetype: X, fields: [b], tags: [Bar]}
  - advance: 1h
  - rename_tag: {from: foo, to: "a b"}
  - rename_tag: {from: FOO, to: bar}
  - reparent_tags: {tags: ["Bar::kid"], parent: ""}
  - add_note_tags: {notes: ["#n2"], tags: "z q"}
  - remove_note_tags: {notes: ["#n1", "#n2"], tags: bar}
  - set_tag_collapsed: {tags: [q, "new::x"], collapsed: false}
  - delete_tags: {tags: z}
  - clear_unused_tags: {}
  - set_flag: {cards: ["#c1"], flag: 9}
  - set_flag: {cards: ["#c1", "#c2"], flag: 3}
  - db_exec: {sql: "SELECT tags, mod, usn FROM notes ORDER BY id"}
  - db_exec: {sql: "SELECT flags, mod, usn FROM cards ORDER BY id"}
  - db_exec: {sql: "SELECT mod FROM col"}
  - observe: [tag_tree]
"##,
    );
    let later: i64 = 1_894_705_200;
    assert_eq!(
        steps[3..13],
        [
            json!({"rename_tag": {"error": "invalid_name"}}),
            json!({"rename_tag": {"count": 1}}),
            json!({"reparent_tags": {"count": 1}}),
            json!({"add_note_tags": {"count": 1}}),
            json!({"remove_note_tags": {"count": 2}}),
            json!({}),
            json!({"delete_tags": {"count": 1}}),
            json!({"clear_unused_tags": {"count": 2}}),
            json!({"set_flag": {"error": "invalid_flag"}}),
            json!({"set_flag": {"count": 2}}),
        ]
    );
    assert_eq!(
        steps[13],
        json!({"db_exec": {"rows": [[" kid ", later, -1], [" q ", later, -1]]}})
    );
    // A flag change keeps the cards' mod.
    assert_eq!(
        steps[14],
        json!({"db_exec": {"rows": [[3, 1_894_701_600, -1], [3, 1_894_701_600, -1]]}})
    );
    assert_eq!(steps[15], json!({"db_exec": {"rows": [[later * 1000]]}}));
    assert_eq!(
        steps[16],
        json!({"tag_tree": [
            {"name": "kid", "collapsed": true, "children": []},
            {"name": "q", "collapsed": false, "children": []},
        ]})
    );
}

#[test]
fn an_unknown_reference_is_a_run_error() {
    let dir = tempfile::tempdir().unwrap();
    let loaded = load(
        dir.path(),
        "tags/unknown",
        "  - add_note_tags: {notes: [\"#n1\"], tags: x}\n",
    );
    let error = executor().run(&loaded).unwrap_err();
    assert_eq!((error.kind.as_str(), error.step), ("unknown_ref", Some(4)));
}

#[test]
fn the_tags_area_registers_every_tag_op_and_tag_tree() {
    let needs = MeguriExecutor::area_registry(&["tags"]).capabilities();
    assert_eq!(
        needs.ops.iter().map(String::as_str).collect::<Vec<_>>(),
        [
            "add_note_tags",
            "clear_unused_tags",
            "delete_tags",
            "remove_note_tags",
            "rename_tag",
            "reparent_tags",
            "set_flag",
            "set_tag_collapsed"
        ]
    );
    assert_eq!(
        needs.observations.iter().map(String::as_str).collect::<Vec<_>>(),
        ["tag_tree"]
    );
}
