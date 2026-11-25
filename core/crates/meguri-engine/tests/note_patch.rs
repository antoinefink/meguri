//! `editor/field-html` S2: the field patch form of the note update (U27).
mod common;
use common::notes::{add, input, kind};
use common::tagging::{fresh, raw};
use meguri_engine::{EngineError, UndoScope, notes};
use meguri_model::{changes::OpSource, ids::NoteId, ids::NotetypeId};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole, Record, Table};

fn layout(h: &common::Harness, nt: NotetypeId) -> Vec<notes::AddFieldIdentity> {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    r.notetype(nt)
        .unwrap()
        .unwrap()
        .fields
        .iter()
        .map(|field| notes::AddFieldIdentity {
            ord: field.ord,
            id: field.config.id,
            name: field.name.clone(),
        })
        .collect()
}

fn patch(id: NoteId, nt: NotetypeId, layout: Vec<notes::AddFieldIdentity>, fields: &[(u32, &str)]) -> notes::NotePatch {
    notes::NotePatch {
        id,
        notetype_id: nt,
        field_layout: layout,
        fields: fields.iter().map(|(ord, value)| (*ord, (*value).to_owned())).collect(),
        tags: None,
    }
}

fn stored(h: &common::Harness, id: NoteId) -> meguri_model::note::Note {
    h.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap()
}

#[test]
fn sparse_stored_ordinals_patch_a_later_position_and_survive_reopen_without_schema_writes() {
    for ords in [[0, 4, 12], [7, 15, 90]] {
        let h = fresh();
        let nt = kind(&h, false, &["Front", "Back", "Extra"], &["{{Front}}"]);
        let mut draft = input(nt, &["<span  class='kept'>front</span><br/>", "back", "[sound:kept.mp3]"]);
        draft.tags = vec!["kept".to_owned()];
        let added = add(&h, draft);
        let unrelated = add(&h, input(nt, &["unrelated", "<b>exact</b>", "other"])).id;
        // Persist genuinely sparse rows, as a sync/import may supply them.
        // Move the later rows first so their primary keys never collide.
        for position in (0..ords.len()).rev() {
            raw(
                &h.collection,
                "UPDATE fields SET ord = ? WHERE ntid = ? AND ord = ?",
                &[SqlValue::Integer(ords[position]), SqlValue::Integer(nt.0), SqlValue::Integer(position as i64)],
            );
        }
        let schema_rows = |h: &common::Harness| {
            let reader = h.collection.reader(ReaderRole::Query).unwrap();
            ["notetypes", "fields", "templates"].map(|table| {
                reader.execute_raw(&format!("SELECT * FROM {table} ORDER BY 1, 2"), &[]).unwrap()
            })
        };
        let before_schema = schema_rows(&h);
        let before_note = stored(&h, added.id);
        let before_unrelated = stored(&h, unrelated);
        let expected = layout(&h, nt);
        assert_eq!(expected.iter().map(|f| i64::from(f.ord)).collect::<Vec<_>>(), ords);
        // The fingerprint still uses raw ord/id/name. Normalizing it would
        // weaken the protection against a layout change under the editor.
        let mut normalized = expected.clone();
        for (position, field) in normalized.iter_mut().enumerate() {
            field.ord = position as u32;
        }
        assert!(matches!(
            notes::patch(&h.collection, patch(added.id, nt, normalized, &[(1, "wrong")]), OpSource::UserUndoable),
            Err(EngineError::Stale { table: Table::Notetypes, .. })
        ));
        assert_eq!(stored(&h, added.id).to_cells(), before_note.to_cells());
        // Patch key 1 is the second logical value, never stored ordinal 15/4.
        let result = notes::patch(
            &h.collection, patch(added.id, nt, expected, &[(1, "later field edited")]), OpSource::UserUndoable,
        ).unwrap();
        assert!(result.value.written);
        let mut expected_note = before_note;
        expected_note.fields = "<span  class='kept'>front</span><br/>\u{1f}later field edited\u{1f}[sound:kept.mp3]".to_owned();
        let saved = stored(&h, added.id);
        // Only ordinary note stamps may differ besides the selected value.
        expected_note.mtime = saved.mtime;
        expected_note.usn = saved.usn;
        assert_eq!(saved.to_cells(), expected_note.to_cells());
        assert_eq!(schema_rows(&h), before_schema);
        let common::Harness { dir, path, engine, collection, .. } = h;
        engine.close().unwrap();
        drop(collection);
        drop(engine);
        let reopened = common::Harness::open_path(dir, path);
        assert_eq!(stored(&reopened, added.id).to_cells(), expected_note.to_cells());
        assert_eq!(stored(&reopened, unrelated).to_cells(), before_unrelated.to_cells());
        assert_eq!(schema_rows(&reopened), before_schema);
    }
}

#[test]
fn a_patch_writes_only_its_fields_and_keeps_another_writers_and_the_tags() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back", "Extra"], &["{{Front}}"]);
    let mut draft = input(nt, &["front", "back", "extra"]);
    draft.tags = vec!["kept".to_owned()];
    let added = add(&h, draft);
    // Another writer changes Back after the editor loaded the note.
    notes::update(
        &h.collection,
        notes::UpdateNote {
            id: added.id,
            fields: vec!["front".to_owned(), "theirs".to_owned(), "extra".to_owned()],
            tags: vec!["kept".to_owned()],
        },
        OpSource::SyncIncoming,
    )
    .unwrap();
    let result = notes::patch(
        &h.collection,
        patch(added.id, nt, layout(&h, nt), &[(0, "mine")]),
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(result.value.written);
    let note = stored(&h, added.id);
    assert_eq!(note.fields, "mine\u{1f}theirs\u{1f}extra");
    assert_eq!(note.tags, " kept ");
    assert_eq!(note.sort_field, "mine");
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some("Update Note")
    );
}

#[test]
fn an_unchanged_or_empty_patch_writes_nothing() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let added = add(&h, input(nt, &["same", "back"]));
    let before = stored(&h, added.id);
    for fields in [vec![(0, "same")], vec![]] {
        let result = notes::patch(&h.collection, patch(added.id, nt, layout(&h, nt), &fields), OpSource::UserUndoable)
            .unwrap();
        assert!(!result.value.written);
        assert_eq!(stored(&h, added.id), before);
    }
}

#[test]
fn tags_are_replaced_only_when_listed() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    let mut draft = input(nt, &["front"]);
    draft.tags = vec!["old".to_owned()];
    let added = add(&h, draft);
    let mut request = patch(added.id, nt, layout(&h, nt), &[]);
    request.tags = Some(vec!["new".to_owned()]);
    assert!(notes::patch(&h.collection, request, OpSource::UserUndoable).unwrap().value.written);
    assert_eq!(stored(&h, added.id).tags, " new ");
}

#[test]
fn refusals_name_a_deleted_note_a_changed_layout_and_a_bad_ordinal() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let other = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let added = add(&h, input(nt, &["front", "back"]));
    let before = stored(&h, added.id);
    let attempt = |request| notes::patch(&h.collection, request, OpSource::UserUndoable);
    assert!(matches!(
        attempt(patch(NoteId(-99), nt, layout(&h, nt), &[(0, "x")])),
        Err(EngineError::NoteNotFound { .. })
    ));
    assert!(matches!(
        attempt(patch(added.id, other, layout(&h, other), &[(0, "x")])),
        Err(EngineError::Stale { table: Table::Notes, .. })
    ));
    let mut renamed = layout(&h, nt);
    renamed[1].name = "Answer".to_owned();
    assert!(matches!(
        attempt(patch(added.id, nt, renamed, &[(0, "x")])),
        Err(EngineError::Stale { table: Table::Notetypes, .. })
    ));
    let mut shorter = layout(&h, nt);
    shorter.pop();
    assert!(matches!(
        attempt(patch(added.id, nt, shorter, &[(0, "x")])),
        Err(EngineError::Stale { table: Table::Notetypes, .. })
    ));
    for fields in [vec![(2, "x")], vec![(0, "x"), (0, "y")]] {
        assert!(matches!(
            attempt(patch(added.id, nt, layout(&h, nt), &fields)),
            Err(EngineError::InvalidArgument { .. })
        ));
    }
    raw(
        &h.collection,
        "UPDATE notes SET flds = 'one' WHERE id = ?",
        &[SqlValue::Integer(added.id.0)],
    );
    assert!(matches!(
        attempt(patch(added.id, nt, layout(&h, nt), &[(0, "x")])),
        Err(EngineError::InvalidArgument { .. })
    ));
    raw(
        &h.collection,
        "UPDATE notes SET flds = ? WHERE id = ?",
        &[SqlValue::Text(before.fields.clone()), SqlValue::Integer(added.id.0)],
    );
    assert_eq!(stored(&h, added.id), before);
}

#[test]
fn consecutive_patches_merge_into_one_undo_step() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let added = add(&h, input(nt, &["front", "back"]));
    let original = stored(&h, added.id).fields;
    for value in ["f", "fr"] {
        notes::patch(&h.collection, patch(added.id, nt, layout(&h, nt), &[(0, value)]), OpSource::UserUndoable)
            .unwrap();
        h.clock.step_ms(1_000);
    }
    assert_eq!(stored(&h, added.id).fields, "fr\u{1f}back");
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(stored(&h, added.id).fields, original);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some("Add Note")
    );
}

#[test]
fn a_patch_that_fills_another_template_generates_its_card() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let added = add(&h, input(nt, &["front", ""]));
    assert_eq!(added.cards.len(), 1);
    let result = notes::patch(&h.collection, patch(added.id, nt, layout(&h, nt), &[(1, "back")]), OpSource::UserUndoable)
        .unwrap();
    assert_eq!(result.value.cards.len(), 1);
}
