//! The schema-change gate and the storage guard
//! (`specs/notetypes/schema-changes.md` G1 to G6, AE39, R951, R1475 to
//! R1477). U34's structure ops do not exist yet, so the ops here are
//! test-only: they change a note type's rows directly and declare their
//! reasons to the gate as an op of G2 would.

mod common;

use std::time::Duration;

use common::{Harness, drain};
use meguri_engine::notetypes::{self, Draft, NewNotetype, NewTemplate, SaveOptions};
use meguri_engine::schema_gate::{SchemaReason, is_modified};
use meguri_engine::undo::names::UPDATE_NOTE_TYPE;
use meguri_engine::{EngineError, EventKind, OpContext, OpSpec, StepKind, UndoScope, UndoStatus, mark_schema_change};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::col::Col;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, StampMode};

fn col(h: &Harness) -> Col {
    h.collection.reader(ReaderRole::Query).unwrap().col().unwrap()
}

fn notetype(h: &Harness, id: NotetypeId) -> Notetype {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(id)
        .unwrap()
        .expect("the note type exists")
}

/// A note type with fields Front and Back and two templates.
fn pair(h: &Harness) -> NotetypeId {
    let new = NewNotetype {
        name: "Pair".to_owned(),
        fields: vec!["Front".to_owned(), "Back".to_owned()],
        templates: vec![
            NewTemplate {
                name: "Forward".to_owned(),
                front: "{{Front}}".to_owned(),
                back: "{{Back}}".to_owned(),
                ..Default::default()
            },
            NewTemplate {
                name: "Reverse".to_owned(),
                front: "{{Back}}".to_owned(),
                back: "{{Front}}".to_owned(),
                ..Default::default()
            },
        ],
        ..NewNotetype::default()
    };
    notetypes::add(&h.collection, new).unwrap().value.id
}

/// G1, T9: the state right after a sync (`ls` = `scm`), then a minute later,
/// so that a change made now sets `scm` above `ls`.
fn just_synced(h: &Harness) {
    h.collection
        .run(
            OpSpec::named("test_sync_finish", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            |ctx| {
                let mut col = ctx.scope().col()?;
                col.last_sync = col.schema_modified;
                ctx.scope().put_col(&col)?;
                Ok(())
            },
        )
        .unwrap();
    h.clock.step_ms(60_000);
}

/// A test-only structure op declares its effect before safety admission.
fn structure_op(
    h: &Harness,
    id: NotetypeId,
    reasons: &'static [SchemaReason],
    change: fn(&mut Notetype, &OpContext<'_, '_>),
) -> Result<(), EngineError> {
    h.collection
        .run(
            OpSpec::named("test_structure_change", OpSource::UserUndoable)
                .schema_changes(reasons.iter().copied())
                .undo_step(UPDATE_NOTE_TYPE, StepKind::Other),
            move |ctx| {
                let mut notetype = ctx.scope().notetype(id)?.expect("the note type exists");
                mark_schema_change(ctx.scope(), reasons)?;
                change(&mut notetype, ctx);
                ctx.scope().put_notetype(&notetype)?;
                Ok(())
            },
        )
        .map(|_| ())
}

fn rename_back(notetype: &mut Notetype, _: &OpContext<'_, '_>) {
    notetype.fields[1].name = "Answer".to_owned();
}

fn add_field(notetype: &mut Notetype, ctx: &OpContext<'_, '_>) {
    let name = format!("Extra {}", notetype.fields.len());
    let mut field = notetypes::new_field(&name, ctx.random());
    field.ord = u32::try_from(notetype.fields.len()).unwrap();
    notetype.fields.push(field);
}

fn add_template(notetype: &mut Notetype, ctx: &OpContext<'_, '_>) {
    let mut template = notetypes::new_template("Both", "{{Front}}{{Back}}", "{{Back}}", ctx.random());
    template.ord = u32::try_from(notetype.templates.len()).unwrap();
    notetype.templates.push(template);
}

fn swap_templates(notetype: &mut Notetype, _: &OpContext<'_, '_>) {
    notetype.templates.swap(0, 1);
    for (ord, template) in notetype.templates.iter_mut().enumerate() {
        template.ord = u32::try_from(ord).unwrap();
    }
}

const FIELDS: &[SchemaReason] = &[SchemaReason::FieldsChanged];
const TEMPLATES: &[SchemaReason] = &[SchemaReason::TemplatesChanged];

#[test]
fn a_schema_op_commits_at_once_while_scm_is_at_ls() {
    let h = Harness::open();
    let id = pair(&h);
    just_synced(&h);
    let synced = col(&h);
    assert!(!is_modified(&synced));
    structure_op(&h, id, &[], rename_back).unwrap();
    assert_eq!(notetype(&h, id).fields[1].name, "Answer");
    assert_eq!(col(&h).schema_modified, synced.schema_modified);
    let now = h.clock.now_ms();
    structure_op(&h, id, FIELDS, add_field).unwrap();
    assert_eq!(notetype(&h, id).fields.len(), 3);
    assert_eq!(col(&h).schema_modified.0, now);
    assert!(is_modified(&col(&h)));
    h.clock.step_ms(1_000);
    structure_op(&h, id, TEMPLATES, swap_templates).unwrap();
    assert_eq!(notetype(&h, id).templates[0].name, "Reverse");
    assert_eq!(col(&h).schema_modified.0, h.clock.now_ms());
}

#[test]
fn a_plain_save_cannot_move_templates_that_have_no_id() {
    // R949, notetype-ops.md T5: a template keeps its identity wherever a
    // draft moves it, also without the optional ids of legacy note types.
    // After a sync, two such templates with the same texts swapped in a
    // draft look like two renames; the plain save must refuse the move,
    // which rewrites card ordinals (U34), and write nothing.
    let h = Harness::open();
    let new = NewNotetype {
        name: "Twins".to_owned(),
        fields: vec!["Front".to_owned(), "Back".to_owned()],
        templates: ["One", "Two"]
            .into_iter()
            .map(|name| NewTemplate {
                name: name.to_owned(),
                front: "{{Front}}{{Card}}".to_owned(),
                back: "{{Back}}".to_owned(),
                ..Default::default()
            })
            .collect(),
        ..NewNotetype::default()
    };
    let id = notetypes::add(&h.collection, new).unwrap().value.id;
    let mut legacy = notetype(&h, id);
    for template in &mut legacy.templates {
        template.config.id = None;
    }
    for field in &mut legacy.fields {
        field.config.id = None;
    }
    h.collection
        .run(
            OpSpec::named("test_strip_ids", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                ctx.scope().put_notetype(&legacy)?;
                Ok(())
            },
        )
        .unwrap();
    just_synced(&h);
    let before = (col(&h), notetype(&h, id));

    let mut moved = Draft::of(notetype(&h, id));
    moved.templates.swap(0, 1);
    let refused = notetypes::save(&h.collection, moved, SaveOptions::default());
    assert!(
        matches!(refused, Err(EngineError::InvalidArgument { .. })),
        "{refused:?}"
    );
    let mut fields_moved = Draft::of(notetype(&h, id));
    fields_moved.fields.swap(0, 1);
    let refused = notetypes::save(&h.collection, fields_moved, SaveOptions::default());
    assert!(
        matches!(refused, Err(EngineError::InvalidArgument { .. })),
        "{refused:?}"
    );

    let after = (col(&h), notetype(&h, id));
    assert_eq!(after.0, before.0);
    assert_eq!(
        after.1.templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["One", "Two"]
    );
    assert_eq!(after.1.mtime, before.1.mtime);

    // Renaming the same templates in place is a plain save, and no schema
    // change.
    let mut renamed = Draft::of(notetype(&h, id));
    renamed.templates[0].name = "Two".to_owned();
    renamed.templates[1].name = "One".to_owned();
    notetypes::save(&h.collection, renamed, SaveOptions::default()).unwrap();
    assert_eq!(col(&h).schema_modified, before.0.schema_modified);
}

#[test]
fn a_second_field_add_needs_no_confirmation_and_sets_scm_again() {
    let h = Harness::open();
    let id = pair(&h);
    just_synced(&h);
    structure_op(&h, id, FIELDS, add_field).unwrap();
    h.clock.step_ms(5_000);
    let now = h.clock.now_ms();
    structure_op(&h, id, FIELDS, add_field).unwrap();
    assert_eq!(notetype(&h, id).fields.len(), 4);
    assert_eq!(col(&h).schema_modified.0, now);
}

#[test]
fn a_change_set_and_the_undo_step_carry_the_schema_stamp() {
    // G5: the `scm` write moves `col.mod`, is in the op's change set and in
    // its undo step.
    let h = Harness::open();
    let id = pair(&h);
    just_synced(&h);
    let synced = col(&h);
    let events = h.engine.stream().subscribe();
    structure_op(&h, id, FIELDS, add_field).unwrap();
    let after = col(&h);
    assert_eq!(after.modified.0, h.clock.now_ms());
    let changes: Vec<_> = drain(&events, Duration::from_millis(50))
        .into_iter()
        .filter_map(|e| match &e.kind {
            EventKind::Changes(changes) => Some(changes.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(changes.len(), 1);
    assert!(changes[0].collection_row);
    assert_eq!(changes[0].notetypes.len(), 1);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col(&h).schema_modified, synced.schema_modified);
}

#[test]
fn the_storage_guard_refuses_a_count_change_that_leaves_scm_at_ls() {
    // G6: a non-schema op that changes a field count while `scm <= ls`.
    let h = Harness::open();
    let id = pair(&h);
    just_synced(&h);
    let before = (col(&h), notetype(&h, id).fields.len());
    let refused = structure_op(&h, id, &[], add_field);
    assert_eq!(refused, Err(EngineError::SchemaNotMarked { notetype_id: id }));
    assert_eq!((col(&h), notetype(&h, id).fields.len()), before);
    // A template count too.
    let refused = structure_op(&h, id, &[], add_template);
    assert_eq!(refused, Err(EngineError::SchemaNotMarked { notetype_id: id }));
    assert_eq!(notetype(&h, id).templates.len(), 2);
    // A reorder keeps the counts: the guard lets it through (G2 and G3 are
    // the ops' to declare).
    structure_op(&h, id, &[], swap_templates).unwrap();
}

#[test]
fn the_guard_lets_count_changes_through_once_the_schema_is_modified() {
    // G6 is defined on the state the transaction leaves: with `scm > ls`
    // already, any count change commits.
    let h = Harness::open();
    let id = pair(&h);
    // Never synced: `ls` 0 (G1).
    assert_eq!(col(&h).last_sync.0, 0);
    assert!(is_modified(&col(&h)));
    structure_op(&h, id, &[], add_field).unwrap();
    assert_eq!(notetype(&h, id).fields.len(), 3);
}

#[test]
fn note_types_added_or_removed_do_not_count() {
    let h = Harness::open();
    just_synced(&h);
    // Adding is no schema change (R950) and passes the guard.
    let id = pair(&h);
    assert_eq!(col(&h).schema_modified.0, col(&h).last_sync.0);
    // Removing the whole note type: its rows go, and the guard ignores it.
    h.collection
        .run(
            OpSpec::named("test_remove_notetype", OpSource::UserNonUndoable),
            move |ctx| {
                ctx.scope().remove_notetype(id)?;
                Ok(())
            },
        )
        .unwrap();
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .notetype(id)
            .unwrap()
            .is_none()
    );
}

#[test]
fn undo_and_redo_of_a_template_add_both_commit() {
    let h = Harness::open();
    let id = pair(&h);
    just_synced(&h);
    let synced = col(&h);
    structure_op(&h, id, TEMPLATES, add_template).unwrap();
    let added = col(&h);
    assert_eq!(notetype(&h, id).templates.len(), 3);
    assert!(matches!(
        h.collection.peek_undo(UndoScope::Global).unwrap(),
        UndoStatus::Available(_)
    ));

    // The undo restores the count with the `scm` its step set: the one
    // exception to the guard.
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(notetype(&h, id).templates.len(), 2);
    assert_eq!(col(&h).schema_modified, synced.schema_modified);
    assert_eq!(col(&h).last_sync, synced.last_sync);

    // The redo sets `scm` to the step's value again, which passes.
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(notetype(&h, id).templates.len(), 3);
    assert_eq!(col(&h).schema_modified, added.schema_modified);
}

#[test]
fn no_op_takes_a_schema_confirmation_input() {
    // A function-pointer assertion freezes the confirmation-free public signature.
    let stamp: fn(&mut meguri_storage::WriteScope<'_>, &[SchemaReason]) -> Result<(), EngineError> = mark_schema_change;
    for reason in SchemaReason::ALL {
        let h = Harness::open();
        just_synced(&h);
        let now = h.clock.now_ms();
        h.collection
            .run(
                OpSpec::named("test_schema_op", OpSource::UserUndoable).schema_changes([reason]),
                move |ctx| stamp(ctx.scope(), &[reason]),
            )
            .unwrap();
        assert_eq!(col(&h).schema_modified.0, now, "{reason:?}");
    }
}

#[test]
fn an_op_with_several_reasons_stamps_once() {
    let h = Harness::open();
    just_synced(&h);
    let before = col(&h);
    let reasons = [
        SchemaReason::TemplatesChanged,
        SchemaReason::FieldsChanged,
        SchemaReason::FieldsChanged,
    ];
    h.collection
        .run(
            OpSpec::named("test_schema_op", OpSource::UserUndoable).schema_changes(reasons),
            move |ctx| mark_schema_change(ctx.scope(), &reasons),
        )
        .unwrap();
    assert_eq!(col(&h).schema_modified.0, h.clock.now_ms());
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(col(&h).schema_modified, before.schema_modified);
}

#[test]
fn the_classification_is_the_table_of_g2() {
    // R949, R950, R1475: every reason of G2 with its structural-edit column,
    // read from the spec.
    let spec = std::fs::read_to_string(
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../specs/notetypes/schema-changes.md"),
    )
    .unwrap();
    let section = spec
        .split("\n## G2.")
        .nth(1)
        .and_then(|rest| rest.split("\n## ").next())
        .expect("schema-changes.md has a G2 section");
    let listed: Vec<(String, bool)> = section
        .lines()
        .filter(|line| line.starts_with("| `"))
        .map(|line| {
            let cells: Vec<&str> = line.split('|').map(str::trim).collect();
            (cells[1].trim_matches('`').to_owned(), cells[4].starts_with("yes"))
        })
        .collect();
    let ours: Vec<(String, bool)> = SchemaReason::ALL
        .iter()
        .map(|r| (r.name().to_owned(), r.is_structural()))
        .collect();
    assert_eq!(ours, listed);
}
