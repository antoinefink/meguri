//! S2.5/O3 executor composition: transaction-start births and reason ownership.
use super::*;
use meguri_engine::{OpContext, changes::observers::RowAfter};
use meguri_model::{clock::Clock, ids::DeckConfigId, proto::ProtoBlob};
use meguri_storage::{Cell, Record, RowImage, StorageError};

type NotetypeRows = (RowImage, Vec<RowImage>, Vec<RowImage>);

fn rows_in(rows: &impl ReadRows, id: NotetypeId) -> Result<NotetypeRows, StorageError> {
    Ok((
        rows.row_image(Table::Notetypes, &RowKey::Id(id.0))?.unwrap(),
        rows.notetype_fields_page(id, None)?,
        rows.notetype_templates_page(id, None)?,
    ))
}

fn rows(h: &Harness, id: NotetypeId) -> NotetypeRows {
    rows_in(&*h.collection.reader(ReaderRole::Query).unwrap(), id).unwrap()
}

fn preset_report() -> StructureReport {
    let mut report = StructureReport::default();
    report.reasons.insert(R::PresetRemoved);
    report
}

fn save_css(ctx: &mut OpContext<'_, '_>, id: NotetypeId, css: &str) -> Result<(), EngineError> {
    let mut draft = notetypes::Draft::of(ctx.scope().notetype(id)?.unwrap());
    draft.config.css = css.into();
    assert!(notetypes::save_in(ctx.scope(), &draft, notetypes::SaveOptions::default())?.written);
    Ok(())
}

#[derive(Clone, Debug, PartialEq)]
struct Snapshot {
    notetype: NotetypeRows,
    presets: Vec<RowImage>,
    decks: Vec<RowImage>,
    col: RowImage,
    current: Option<RowImage>,
}

fn snapshot(h: &Harness, id: NotetypeId) -> Snapshot {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    Snapshot {
        notetype: rows_in(&*r, id).unwrap(),
        presets: r.deck_config_rows().unwrap().iter().map(Record::to_cells).collect(),
        decks: r.deck_rows().unwrap().iter().map(Record::to_cells).collect(),
        col: r.col().unwrap().to_cells(),
        current: r.row_image(Table::Config, &RowKey::text(notetypes::CURRENT_NOTETYPE_KEY)).unwrap(),
    }
}

fn setup() -> (Harness, Arc<Capture>, NotetypeId, DeckConfigId) {
    let (h, seen) = rig();
    let id = add(&h, "Scope ownership basic", false);
    let preset = meguri_engine::deck_config::add(&h.collection, "Unassigned custom preset".into()).unwrap().value;
    let mut draft = notetypes::Draft::of(read(&h, id));
    let mut bytes = draft.config.to_bytes().into_owned();
    bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
    draft.config = ProtoBlob::decode(&bytes).unwrap();
    draft.config.css = ".card { line-height: 1.4; }".into();
    notetypes::save(&h.collection, draft, notetypes::SaveOptions::default()).unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_ne!(preset, DeckConfigId(1));
    assert_eq!(meguri_engine::deck_config::use_count(&*reader, preset).unwrap(), 0);
    assert!(reader.deck_config(DeckConfigId(1)).unwrap().is_some());
    drop(reader);
    (h, seen, id, preset)
}

fn expected_css(h: &Harness, id: NotetypeId, css: &str) -> NotetypeRows {
    let mut expected = rows(h, id);
    let mut config = read(h, id).config;
    config.css = css.into();
    expected.0[2] = Cell::Integer(h.clock.now_ms() / 1000);
    expected.0[3] = Cell::Integer(-1);
    expected.0[4] = Cell::Blob(config.to_bytes().into_owned());
    expected
}

fn preset_row(before: &Snapshot, preset: DeckConfigId) -> RowImage {
    before.presets.iter().find(|row| row[0] == Cell::Integer(preset.0)).unwrap().clone()
}

fn assert_deleted(payload: &CommitPayload, preset: DeckConfigId, before: RowImage) {
    let delta = payload.row(Table::DeckConfig, &RowKey::Id(preset.0)).unwrap().unwrap();
    assert_eq!(delta.before, Some(before));
    assert_eq!(delta.after, RowAfter::Absent);
}

fn assert_mode(result: Result<(), EngineError>) {
    assert!(matches!(&result, Err(EngineError::Storage(StorageError::Mode { .. }))),
        "an invalid report token must be refused with typed Mode: {result:?}");
}

fn assert_no_changes(events: &meguri_engine::Subscription) {
    while let Some(event) = events.try_recv() {
        assert!(!matches!(&event.kind, meguri_engine::EventKind::Changes(_)));
    }
}

#[test]
fn native_add_then_plain_save_has_no_transaction_start_origins() {
    let (h, seen) = rig();
    let current_before = h.collection.reader(ReaderRole::Query).unwrap()
        .row_image(Table::Config, &RowKey::text(notetypes::CURRENT_NOTETYPE_KEY)).unwrap();
    let count = seen.0.lock().unwrap().len();
    let result = h.collection.run(spec(), |ctx| {
        let new = NewNotetype {
            name: "Composite new type".into(),
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![NewTemplate {
                name: "Card".into(), front: "{{Front}}".into(), back: "{{Back}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let (scope, random) = ctx.scope_and_random();
        let added = notetypes::add_in(scope, random, &new)?;
        assert!(added.written);
        let mut expected = rows_in(ctx.scope(), added.id)?;
        let mut draft = notetypes::Draft::of(ctx.scope().notetype(added.id)?.unwrap());
        draft.name = "Composite new type saved".into();
        draft.config.css = ".card { line-height: 1.6; }".into();
        let mut config = draft.config.to_bytes().into_owned();
        config.extend_from_slice(&[0xa0, 0x01, 0x07]);
        draft.config = ProtoBlob::decode(&config).unwrap();
        expected.0[1] = Cell::text(draft.name.clone());
        expected.0[4] = Cell::Blob(draft.config.to_bytes().into_owned());
        let saved = notetypes::save_in(ctx.scope(), &draft, notetypes::SaveOptions::default())?;
        assert!(saved.written);
        assert_eq!(saved.id, added.id);
        Ok((added.id, expected))
    }).expect("native add and checked plain save must COMMIT together");
    let (id, expected) = result.value;
    assert_eq!(result.changes.source, OpSource::UserUndoable);
    assert_eq!(result.changes.notetypes.len(), 1);
    assert_eq!(result.changes.notetypes[0].id, id);
    assert_eq!(result.changes.notetypes[0].kind, meguri_model::changes::ChangeKind::Added);
    assert!(result.changes.notetypes[0].schema_changed);
    assert!(result.undo.is_some());
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    assert_eq!(rows(&h, id), expected);
    let retained = last(&seen);
    assert_eq!(retained.seq(), result.seq);
    assert_eq!(retained.generation(), result.generation);
    assert!(retained.structure_report().is_none(), "a new type has no old layout or reason");
    assert!(retained.identity_layout(id).is_none());
    for (table, images) in [
        (Table::Notetypes, vec![expected.0.clone()]),
        (Table::Fields, expected.1.clone()),
        (Table::Templates, expected.2.clone()),
    ] {
        let deltas = retained.rows(table).collect::<Result<Vec<_>, _>>().unwrap();
        assert_eq!(deltas.len(), images.len());
        for (delta, image) in deltas.iter().zip(images) {
            assert!(delta.before.is_none());
            assert_eq!(delta.after, RowAfter::Row(image));
        }
    }
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.notetype(id).unwrap().is_none());
    assert!(r.notetype_fields_page(id, None).unwrap().is_empty());
    assert!(r.notetype_templates_page(id, None).unwrap().is_empty());
    assert_eq!(r.row_image(Table::Config, &RowKey::text(notetypes::CURRENT_NOTETYPE_KEY)).unwrap(), current_before);
    drop(r);
    let mut removed = StructureReport::default();
    removed.reasons.insert(R::NotetypeRemoved);
    removed.removed.insert(id.0);
    assert_eq!(last(&seen).structure_report(), Some(&removed));
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), expected);
    assert!(last(&seen).structure_report().is_none());
    assert!(last(&seen).identity_layout(id).is_none());
    assert!(retained.structure_report().is_none());
    assert_eq!(retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap().after,
        RowAfter::Row(expected.0));
}

#[test]
fn parent_reason_survives_completed_inner_native_save() {
    let (h, seen, id, preset) = setup();
    let before = snapshot(&h, id);
    let mut expected = before.clone();
    expected.notetype = expected_css(&h, id, ".card { line-height: 1.8; }");
    expected.presets.retain(|row| row[0] != Cell::Integer(preset.0));
    let mut report = preset_report();
    report.layouts.insert(id.0, LayoutReport::identity(&read(&h, id)).unwrap());
    let count = seen.0.lock().unwrap().len();
    let result = h.collection.run(spec(), move |ctx| {
        let outer = ctx.scope().start_structure_report(preset_report())?;
        save_css(ctx, id, ".card { line-height: 1.8; }")?;
        // Explicit framework authoring: this unused valid preset needs no
        // deck rewrites or SCM change to exercise report-token ownership.
        assert!(ctx.scope().remove_deck_config(preset)?.changed());
        ctx.scope().complete_structure_report(outer)?;
        Ok(())
    }).expect("a reason-only outer token must survive a completed native identity contribution");
    assert_eq!(snapshot(&h, id), expected);
    assert_eq!(result.changes.deck_configs.len(), 1);
    assert_eq!(result.changes.deck_configs[0].id, preset);
    assert_eq!(result.changes.deck_configs[0].kind, meguri_model::changes::ChangeKind::Removed);
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    assert_eq!(retained.seq(), result.seq);
    assert_eq!(retained.generation(), result.generation);
    assert_eq!(retained.structure_report(), Some(&report));
    assert_eq!(retained.identity_layout(id), report.layouts.get(&id.0));
    assert_deleted(&retained, preset, preset_row(&before, preset));
    let delta = retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap();
    assert_eq!(delta.before, Some(before.notetype.0.clone()));
    assert_eq!(delta.after, RowAfter::Row(expected.notetype.0.clone()));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), expected);
    assert_eq!(retained.structure_report(), Some(&report));
    assert_eq!(retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap(), delta);
}

#[test]
fn parent_reason_survives_failed_child_and_discards_completed_child_native_report() {
    let (h, seen, id, preset) = setup();
    let before = snapshot(&h, id);
    let mut expected = before.clone();
    expected.presets.retain(|row| row[0] != Cell::Integer(preset.0));
    let count = seen.0.lock().unwrap().len();
    let result = h.collection.run(spec(), move |ctx| {
        let outer = ctx.scope().start_structure_report(preset_report())?;
        let failed: Result<(), EngineError> = ctx.step(|ctx| {
            save_css(ctx, id, ".card { line-height: 2; }")?;
            assert!(ctx.scope().journal().unwrap().identity_layout(id.0).is_some());
            Err(EngineError::InvalidArgument { message: "rollback completed native save".into() })
        });
        assert!(matches!(failed, Err(EngineError::InvalidArgument { ref message })
            if message == "rollback completed native save"));
        assert!(ctx.scope().journal().unwrap().structure_report().is_none());
        assert!(ctx.scope().remove_deck_config(preset)?.changed());
        ctx.scope().complete_structure_report(outer)?;
        Ok(())
    }).expect("failed child rollback must preserve a parent reason-only token");
    assert_eq!(snapshot(&h, id), expected);
    assert!(result.changes.notetypes.is_empty());
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    assert_eq!(retained.seq(), result.seq);
    assert_eq!(retained.generation(), result.generation);
    assert_eq!(retained.structure_report(), Some(&preset_report()));
    assert!(retained.identity_layout(id).is_none());
    for table in [Table::Notetypes, Table::Fields, Table::Templates] {
        assert_eq!(retained.row_keys(table).count(), 0);
    }
    assert_deleted(&retained, preset, preset_row(&before, preset));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), expected);
    assert_eq!(retained.structure_report(), Some(&preset_report()));
}

#[test]
fn reason_token_born_in_failed_child_cannot_escape_rollback() {
    let (h, seen, id, preset) = setup();
    let before = snapshot(&h, id);
    let retained = last(&seen);
    let evidence = retained.structure_report().unwrap().clone();
    let retained_row = retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap();
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let count = seen.0.lock().unwrap().len();
    let events = h.engine.stream().subscribe();
    let result = h.collection.run(spec(), move |ctx| {
        // If an invalid token were accepted, this real parent edit would
        // commit together with a ghost reason for the restored deletion.
        save_css(ctx, id, ".card { line-height: 2.2; }")?;
        let old_preset = ctx.scope().deck_config(preset)?.unwrap().to_cells();
        let mut escaped = None;
        let failed: Result<(), EngineError> = ctx.step(|ctx| {
            escaped = Some(ctx.scope().start_structure_report(preset_report())?);
            assert!(ctx.scope().remove_deck_config(preset)?.changed());
            Err(EngineError::InvalidArgument { message: "rollback token birthplace".into() })
        });
        assert!(matches!(failed, Err(EngineError::InvalidArgument { ref message })
            if message == "rollback token birthplace"));
        assert_eq!(ctx.scope().deck_config(preset)?.unwrap().to_cells(), old_preset);
        ctx.scope().complete_structure_report(escaped.unwrap())?;
        Ok(())
    });
    assert_mode(result.map(|_| ()));
    assert_eq!(snapshot(&h, id), before);
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_no_changes(&events);
    assert_eq!(retained.structure_report(), Some(&evidence));
    assert_eq!(retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap(), retained_row);
}

#[test]
fn reason_token_born_in_successful_child_remains_valid_after_release() {
    let (h, seen, id, preset) = setup();
    let before = snapshot(&h, id);
    let mut expected = before.clone();
    expected.presets.retain(|row| row[0] != Cell::Integer(preset.0));
    let count = seen.0.lock().unwrap().len();
    let result = h.collection.run(spec(), move |ctx| {
        let token = ctx.step(|ctx| {
            let token = ctx.scope().start_structure_report(preset_report())?;
            assert!(ctx.scope().remove_deck_config(preset)?.changed());
            Ok(token)
        })?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).expect("savepoint release must retain its token and actual preset deletion");
    assert_eq!(snapshot(&h, id), expected);
    assert!(result.changes.notetypes.is_empty());
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    assert_eq!(retained.seq(), result.seq);
    assert_eq!(retained.generation(), result.generation);
    assert_eq!(retained.structure_report(), Some(&preset_report()));
    assert!(retained.identity_layout(id).is_none());
    assert_deleted(&retained, preset, preset_row(&before, preset));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(snapshot(&h, id), expected);
    assert_eq!(retained.structure_report(), Some(&preset_report()));
}

#[test]
fn reason_token_from_another_operation_cannot_complete_here() {
    let (h, seen, id, preset) = setup();
    let retained = last(&seen);
    let evidence = retained.structure_report().unwrap().clone();
    let retained_row = retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap();
    // Deliberately leave an uncompleted plan in a different, unchanged op.
    // Neither journal has completed a report: owner isolation, not revision
    // mismatch, must refuse its attempted use for the next op's deletion.
    let token = h.collection.run(spec(), |ctx| {
        Ok(ctx.scope().start_structure_report(preset_report())?)
    }).unwrap().value;
    let before = snapshot(&h, id);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let count = seen.0.lock().unwrap().len();
    let events = h.engine.stream().subscribe();
    let result = h.collection.run(spec(), move |ctx| {
        assert!(ctx.scope().remove_deck_config(preset)?.changed());
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    });
    assert_mode(result.map(|_| ()));
    assert_eq!(snapshot(&h, id), before);
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_no_changes(&events);
    assert_eq!(retained.structure_report(), Some(&evidence));
    assert_eq!(retained.row(Table::Notetypes, &RowKey::Id(id.0)).unwrap(), retained_row);
}
