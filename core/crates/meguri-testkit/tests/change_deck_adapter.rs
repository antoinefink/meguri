//! The `change_deck` adapter (`specs/conformance/bulk-ops.md` X2) on a
//! Meguri-only run, until the held `notes/change-deck/` scenarios land: the
//! output is the cards moved, a card of a deleted note is passed on by its
//! real id and skipped, a filtered target is the rejection `filtered_deck`,
//! and an unknown deck name is `unknown_ref`.
use meguri_testkit::{
    exec::{Executor, MeguriExecutor},
    scenario,
};
use serde_json::{Value, json};

fn run(steps: &str) -> Result<Value, String> {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("case.yaml");
    std::fs::write(
        &path,
        format!(
            r#"id: notes/change-deck/adapter-probe
clock: {{tz: UTC, start: '2030-01-15T10:00:00'}}
setup:
  config: {{fsrs: false}}
  notetypes:
    - name: Single
      fields: [Front, Back]
      templates: [{{name: One, front: "{{{{Front}}}}", back: "{{{{Back}}}}"}}]
  decks: [Home, Target]
  notes:
    - {{notetype: Single, deck: Home, fields: [a, x]}}
    - {{notetype: Single, deck: Home, fields: [b, x]}}
    - {{notetype: Single, deck: Target, fields: [c, x]}}
steps:
{steps}"#
        ),
    )
    .unwrap();
    MeguriExecutor::default()
        .run(&scenario::load(&path).unwrap())
        .map_err(|error| format!("{error:?}"))
}

#[test]
fn change_deck_outputs_the_cards_moved_and_rejects_a_filtered_deck() {
    // The filtered deck first: a raw write empties the undo history.
    let out = run(r#"  - db_exec: {sql: "INSERT INTO decks (id, name, mtime_secs, usn, common, kind) VALUES (100, 'Cram', 0, 0, X'08011001', X'12130801120a0a066465636b3a78106428d804383c')"}
  - change_deck: {cards: ['#c1', '#c3'], deck: Target}
  - delete_notes: {notes: ['#n2']}
  - change_deck: {cards: ['#c2', '#c1'], deck: Home}
  - change_deck: {cards: ['#c1'], deck: Cram}
  - observe: [undo_status]
"#)
    .unwrap();
    let steps = &out["steps"];
    assert_eq!(steps[1]["change_deck"], json!({"count": 1}), "#c3 is in Target already");
    assert_eq!(steps[3]["change_deck"], json!({"count": 1}), "#c2's note was deleted");
    assert_eq!(steps[4]["change_deck"], json!({"error": "filtered_deck"}));
    assert_eq!(steps[5]["undo_status"]["undo"], json!("change_deck"), "the refusal keeps the history: {steps}");
    let unknown = run("  - change_deck: {cards: ['#c1'], deck: Nowhere}\n").unwrap_err();
    assert!(unknown.contains("unknown_ref"), "{unknown}");
}
