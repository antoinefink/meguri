//! S2.5/M9.11 framework tests. Authored test operations below are not U34,
//! U35, import or repair feature coverage. All observations use real commits.
use std::sync::{Arc, Mutex};
use super::common::{self, Harness};
use meguri_engine::{CommitPayload, EngineError, OpSpec, Ports, UndoScope};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate};
use meguri_model::{changes::OpSource, ids::NotetypeId};
use meguri_storage::{ReadRows, ReaderRole, RowKey, Table};
use meguri_storage::journal::{LayoutReport, LayoutShape, StructureReason as R, StructureReport, TypeChangeReport};

#[derive(Default)]
struct Capture(Mutex<Vec<CommitPayload>>);
impl meguri_engine::ports::ChangeObserver for Capture {
    fn name(&self) -> &str {
        "structure_reports_capture"
    }
    fn observe(&self, commit: &meguri_engine::ObservedCommit<'_>) -> Result<(), EngineError> {
        self.0.lock().unwrap().push(commit.payload.clone());
        Ok(())
    }
}
fn rig() -> (Harness, Arc<Capture>) {
    let capture = Arc::new(Capture::default());
    let h = Harness::open_with(Ports::new().observer(capture.clone()));
    h.collection.run(OpSpec::named("settle_structure_observer", OpSource::Rollover)
        .stamp(meguri_storage::StampMode::Raw), |_| Ok(())).unwrap();
    capture.0.lock().unwrap().clear();
    (h, capture)
}
fn add(h: &Harness, name: &str, cloze: bool) -> NotetypeId {
    notetypes::add(&h.collection, NewNotetype {
        name: name.into(),
        kind: if cloze { notetypes::NotetypeKind::Cloze } else { notetypes::NotetypeKind::Standard },
        fields: vec!["Front".into(), "Back".into()],
        templates: vec![NewTemplate { name: "Card".into(), front: if cloze { "{{cloze:Front}}" } else { "{{Front}}" }.into(), back: if cloze { "{{cloze:Front}}{{Back}}" } else { "{{Back}}" }.into(), ..Default::default() }],
        ..Default::default()
    }).unwrap().value.id
}
fn read(h: &Harness, id: NotetypeId) -> meguri_model::notetype::Notetype {
    h.collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap()
}
fn last(c: &Capture) -> CommitPayload { c.0.lock().unwrap().last().unwrap().clone() }
fn spec() -> OpSpec { OpSpec::named("authored_structure_framework", OpSource::UserUndoable) }

#[test]
fn sparse_large_stored_keys_have_logical_arities_and_identity_maps() {
    let (h, _) = rig();
    let id = add(&h, "Sparse logical shape", false);
    let mut nt = read(&h, id);
    nt.fields[1].ord = u32::MAX - 1;
    nt.templates[0].ord = u32::MAX - 2;
    // N1: ordered list positions are ordinals, not physical row keys.
    assert_eq!((nt.fields.len(), nt.templates.len()), (2, 1));
    let shape = LayoutShape::of(&nt).expect("two fields and one template fit logical arities");
    // Check counts before identity can allocate any map from the shape.
    assert_eq!((shape.fields, shape.templates), (2, 1));
    let identity = LayoutReport::identity(&nt).unwrap();
    assert_eq!((identity.before, identity.after), (shape, shape));
    assert_eq!(identity.field_map, [0, 1]);
    assert_eq!(identity.template_map, [0]);
    assert!(identity.is_identity());
}

#[test]
fn native_rename_of_sparse_large_stored_keys_reports_logical_identity_through_undo_redo() {
    use meguri_engine::notetypes::{AggregateDigest, FieldIdentity, FieldRename, RenameFields};
    use meguri_model::{clock::Clock, proto::ProtoBlob, time::EpochSecs, usn::Usn};
    use meguri_storage::{Cell, StampMode};

    let (h, seen) = rig();
    let id = add(&h, "Sparse native rename", false);
    let mut nt = read(&h, id);
    nt.fields[1].ord = u32::MAX - 1;
    nt.templates[0].ord = u32::MAX - 2;
    nt.mtime = EpochSecs(42);
    nt.usn = Usn(8);
    nt.config.css = ".card { color: inherit; }".into();
    let mut config = nt.config.to_bytes().into_owned();
    config.extend_from_slice(&[0xa0, 0x01, 0x07]);
    nt.config = ProtoBlob::decode(&config).unwrap();
    for (i, field) in nt.fields.iter_mut().enumerate() {
        field.config.id = if i == 0 { None } else { Some(-17) };
        field.config.prevent_deletion = true;
        let mut config = field.config.to_bytes().into_owned();
        config.extend_from_slice(&[0xa0, 0x01, 0x07]);
        field.config = ProtoBlob::decode(&config).unwrap();
    }
    let template = &mut nt.templates[0];
    template.mtime = EpochSecs(321);
    template.usn = Usn(19);
    template.config.id = Some(-29);
    template.config.q_format_browser = "{{Front}}".into();
    template.config.a_format_browser = "{{Back}}".into();
    let mut config = template.config.to_bytes().into_owned();
    config.extend_from_slice(&[0xa0, 0x01, 0x07]);
    template.config = ProtoBlob::decode(&config).unwrap();
    // Seed legacy stored rows only: no authored map and no CloudKit network.
    h.collection.run(OpSpec::named("seed_sparse_legacy_rows", OpSource::CloudIncoming)
        .stamp(StampMode::CallerSupplied), move |ctx| {
        ctx.scope().put_notetype(&nt)?;
        Ok(())
    }).unwrap();
    let rows = || {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        (
            reader.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap(),
            reader.notetype_fields_page(id, None).unwrap(),
            reader.notetype_templates_page(id, None).unwrap(),
        )
    };
    let before = rows();
    assert_eq!((before.1.len(), before.2.len()), (2, 1));
    assert_eq!(before.1[0][1], Cell::Integer(0));
    assert_eq!(before.1[1][1], Cell::Integer(i64::from(u32::MAX - 1)));
    assert_eq!(before.2[0][1], Cell::Integer(i64::from(u32::MAX - 2)));
    let old = read(&h, id);
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().notetype_note_count(id).unwrap(), 0);
    let mut expected = before.clone();
    expected.0[2] = Cell::Integer(h.clock.now_ms() / 1000);
    expected.0[3] = Cell::Integer(-1);
    expected.1[0][2] = Cell::text("Prompt");
    expected.1[1][2] = Cell::text("Answer");
    let mut config = old.templates[0].config.clone();
    config.q_format = "{{Prompt}}".into();
    config.a_format = "{{Answer}}".into();
    config.q_format_browser = "{{Prompt}}".into();
    config.a_format_browser = "{{Answer}}".into();
    expected.2[0][5] = Cell::Blob(config.to_bytes().into_owned());
    seen.0.lock().unwrap().clear();

    // The target is the ordinary user operation. With no notes this isolates
    // the ordinal domain; it does not claim card generation coverage.
    let renamed = notetypes::rename_fields(&h.collection, RenameFields {
        notetype: id, base: AggregateDigest::of(&old),
        fields: old.fields.iter().zip(["Prompt", "Answer"]).map(|(field, name)| FieldRename {
            field: FieldIdentity::of(field), name: name.into(),
        }).collect(),
    }, meguri_engine::CancelToken::new(), meguri_engine::ProgressSink::new())
        .expect("native rename must accept sparse legacy stored keys");
    assert!(renamed.value.written);
    let retained = last(&seen);
    let report = retained.structure_report().unwrap();
    assert!(report.reasons.is_empty());
    assert!(report.removed.is_empty() && report.type_changes.is_empty());
    assert_eq!(report.layouts.len(), 1);
    let layout = &report.layouts[&id.0];
    assert_eq!((layout.before.fields, layout.before.templates), (2, 1));
    assert_eq!(layout.field_map, [0, 1]);
    assert_eq!(layout.template_map, [0]);
    assert!(layout.is_identity());
    assert_eq!(rows(), expected);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(), before);
    assert_eq!(last(&seen).structure_report(), Some(report));
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(), expected);
    assert_eq!(last(&seen).structure_report(), Some(report));
    assert_eq!(retained.structure_report(), Some(report));
}

#[test]
fn authored_reorder_retains_original_and_inverse_with_the_commit_owner() {
    let (h, seen) = rig();
    let id = add(&h, "Reorder", false);
    h.collection.run(spec(), move |ctx| {
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        let shape = LayoutShape::of(&nt)?;
        let mut report = StructureReport::default();
        report.reasons.extend([R::TemplatesChanged, R::FieldsChanged, R::FieldsChanged]);
        report.layouts.insert(id.0, LayoutReport { before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0] });
        let token = ctx.scope().start_structure_report(report)?;
        nt.fields.swap(0, 1);
        for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
        ctx.scope().put_notetype(&nt)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
    let retained = last(&seen);
    let original = retained.structure_report().unwrap().clone();
    assert_eq!(original.reasons.iter().map(|r| *r as u32).collect::<Vec<_>>(), [1, 2]);
    assert_eq!(original.layouts[&id.0].field_map, [1, 0]);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report().unwrap().layouts[&id.0].field_map, [1, 0]);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report(), Some(&original));
    h.collection.clear_undo(meguri_engine::ClearCause::ExternalChange).unwrap();
    h.engine.close().unwrap();
    assert_eq!(retained.structure_report(), Some(&original));
    assert!(retained.row(Table::Fields, &RowKey::Pair(id.0, 0)).unwrap().is_some());
}

#[test]
fn an_unreported_swap_back_cannot_leave_stale_authored_origins_when_endpoints_match() {
    let (h, seen) = rig();
    let id = add(&h, "Cancelled unreported swap", false);
    let before = h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap();
    assert_eq!(before.len(), 2);
    let count = seen.0.lock().unwrap().len();
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let result = h.collection.run(spec(), move |ctx| {
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        let shape = LayoutShape::of(&nt)?;
        let mut report = StructureReport::default();
        report.reasons.insert(R::FieldsChanged);
        report.layouts.insert(id.0, LayoutReport { before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0] });
        let token = ctx.scope().start_structure_report(report)?;
        nt.fields.swap(0, 1);
        for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
        ctx.scope().put_notetype(&nt)?;
        ctx.scope().complete_structure_report(token)?;
        // This second swap has no authored contribution, despite cancelling
        // the first swap's physical endpoint changes in the same operation.
        nt.fields.swap(0, 1);
        for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
        ctx.scope().put_notetype(&nt)?;
        Ok(())
    });
    assert!(matches!(result, Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap(), before);
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
}

#[test]
fn two_completed_authored_swaps_retain_the_composed_identity_map() {
    let (h, seen) = rig();
    let id = add(&h, "Cancelled authored swaps", false);
    let before = h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap();
    assert_eq!(before.len(), 2);
    let shape = LayoutShape::of(&read(&h, id)).unwrap();
    let count = seen.0.lock().unwrap().len();
    h.collection.run(spec(), move |ctx| {
        for _ in 0..2 {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.reasons.insert(R::FieldsChanged);
            report.layouts.insert(id.0, LayoutReport { before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0] });
            let token = ctx.scope().start_structure_report(report)?;
            nt.fields.swap(0, 1);
            for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
        }
        Ok(())
    }).unwrap();
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap(), before);
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    let report = retained.structure_report().unwrap();
    assert_eq!(report.reasons, [R::FieldsChanged].into());
    assert_eq!(report.layouts[&id.0], LayoutReport { before: shape, after: shape, field_map: vec![0, 1], template_map: vec![0] });
}

#[test]
fn aggregate_name_write_after_completed_authored_swaps_retains_the_composed_identity_map() {
    let (h, seen) = rig();
    let id = add(&h, "Authored swaps then rename", false);
    let before = h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap();
    assert_eq!(before.len(), 2);
    let shape = LayoutShape::of(&read(&h, id)).unwrap();
    let count = seen.0.lock().unwrap().len();
    h.collection.run(spec(), move |ctx| {
        for _ in 0..2 {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.reasons.insert(R::FieldsChanged);
            report.layouts.insert(id.0, LayoutReport { before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0] });
            let token = ctx.scope().start_structure_report(report)?;
            nt.fields.swap(0, 1);
            for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
        }
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        nt.name = "Renamed aggregate".into();
        ctx.scope().put_notetype(&nt)?;
        Ok(())
    }).unwrap();
    assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap(), before);
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    let report = retained.structure_report().unwrap();
    assert_eq!(report.reasons, [R::FieldsChanged].into());
    assert_eq!(report.layouts[&id.0], LayoutReport { before: shape, after: shape, field_map: vec![0, 1], template_map: vec![0] });
    assert_eq!(read(&h, id).name, "Renamed aggregate");
}

#[test]
fn template_format_write_after_completed_authored_swaps_retains_the_composed_identity_map() {
    let (h, seen) = rig();
    let id = add(&h, "Authored swaps then format", false);
    let before = h.collection.reader(ReaderRole::Query).unwrap().notetype_fields_page(id, None).unwrap();
    assert_eq!(before.len(), 2);
    let nt = read(&h, id);
    let shape = LayoutShape::of(&nt).unwrap();
    let key = RowKey::Pair(id.0, 0);
    let mut expected = h.collection.reader(ReaderRole::Query).unwrap().row_image(Table::Templates, &key).unwrap().unwrap();
    let mut config = nt.templates[0].config.clone();
    config.q_format = "<p>{{Front}}</p>".into();
    expected[5] = meguri_storage::Cell::Blob(config.to_bytes().into_owned());
    let count = seen.0.lock().unwrap().len();
    h.collection.run(spec(), move |ctx| {
        for _ in 0..2 {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.reasons.insert(R::FieldsChanged);
            report.layouts.insert(id.0, LayoutReport { before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0] });
            let token = ctx.scope().start_structure_report(report)?;
            nt.fields.swap(0, 1);
            for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
        }
        // Both contributions are complete before this same-name child edit.
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        nt.templates[0].config = config;
        ctx.scope().put_notetype(&nt)?;
        Ok(())
    }).unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.notetype_fields_page(id, None).unwrap(), before);
    assert_eq!(reader.row_image(Table::Templates, &key).unwrap(), Some(expected));
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    let retained = last(&seen);
    let report = retained.structure_report().unwrap();
    assert_eq!(report.reasons, [R::FieldsChanged].into());
    assert_eq!(report.layouts[&id.0], LayoutReport { before: shape, after: shape, field_map: vec![0, 1], template_map: vec![0] });
}

#[test]
fn unreported_rename_rolls_back_for_every_local_source_and_cloud_sources_are_exempt() {
    for source in &OpSource::ALL {
        let (h, seen) = rig();
        let id = add(&h, "Guard", false);
        let before = read(&h, id);
        let count = seen.0.lock().unwrap().len();
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let result = h.collection.run(OpSpec::named("unreported_rename", *source), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            nt.fields[0].name = "Unreported".into();
            ctx.scope().put_notetype(&nt)?;
            Ok(())
        });
        if matches!(source, OpSource::CloudIncoming | OpSource::CloudRekey) {
            result.unwrap();
        } else {
            assert!(matches!(result, Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
            assert_eq!(read(&h, id).fields[0].name, before.fields[0].name);
            assert_eq!(seen.0.lock().unwrap().len(), count);
            assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        }
    }
}

#[test]
fn mid_guard_and_authored_pairs_include_cloze_no_template_map_rules() {
    for (old_cloze, new_cloze) in [(false, false), (true, false), (false, true)] {
        let (h, seen) = rig();
        let old = add(&h, "Old", old_cloze);
        let new = add(&h, "New", new_cloze);
        let mut note = common::note(&["front", "back"]);
        note.notetype_id = old;
        let nid = h.collection.run(spec(), move |ctx| Ok(ctx.scope().add_note(&note)?)).unwrap().value;
        let count = seen.0.lock().unwrap().len();
        let result = h.collection.run(spec(), move |ctx| {
            let mut note = ctx.scope().note(nid)?.unwrap();
            note.notetype_id = new;
            ctx.scope().put(&note)?;
            Ok(())
        });
        assert!(matches!(result, Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == old));
        assert_eq!(seen.0.lock().unwrap().len(), count);
        h.collection.run(spec(), move |ctx| {
            let before = LayoutShape::of(&ctx.scope().notetype(old)?.unwrap())?;
            let after = LayoutShape::of(&ctx.scope().notetype(new)?.unwrap())?;
            let mut report = StructureReport::default();
            report.reasons.insert(R::NotesNotetypeChanged);
            report.type_changes.insert((old.0, new.0), TypeChangeReport { before, after, field_map: vec![1, 0], template_map: if old_cloze { vec![] } else { vec![0] } });
            let token = ctx.scope().start_structure_report(report)?;
            let mut note = ctx.scope().note(nid)?.unwrap();
            note.notetype_id = new;
            note.fields = "back\u{1f}front".into();
            note.sort_field = "back".into();
            ctx.scope().put(&note)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        }).unwrap();
        let original = last(&seen).structure_report().unwrap().clone();
        assert!(original.type_changes.contains_key(&(old.0, new.0)));
        h.collection.undo(UndoScope::Global).unwrap();
        let inverse = last(&seen);
        let pair = &inverse.structure_report().unwrap().type_changes[&(new.0, old.0)];
        assert_eq!(pair.template_map, if new_cloze { vec![] } else { vec![0] });
        h.collection.redo(UndoScope::Global).unwrap();
        assert_eq!(last(&seen).structure_report(), Some(&original));
    }
}

#[test]
fn failed_steps_and_whole_ops_discard_contributions_and_undo_addition_reports_four() {
    let (h, seen) = rig();
    let id = add(&h, "Added", false);
    assert!(last(&seen).structure_report().is_none());
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report().unwrap().reasons, [R::NotetypeRemoved].into());
    assert!(last(&seen).structure_report().unwrap().removed.contains(&id.0));
    h.collection.redo(UndoScope::Global).unwrap();
    assert!(last(&seen).structure_report().is_none());
    h.collection.run(spec(), move |ctx| {
        let failed: Result<(), EngineError> = ctx.step(|ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.reasons.insert(R::ApkgApply);
            report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
            let token = ctx.scope().start_structure_report(report)?;
            nt.fields[0].name = "Rolled back".into();
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
            Err(EngineError::InvalidArgument { message: "failed structural step".into() })
        });
        assert!(failed.is_err());
        let nt = ctx.scope().notetype(id)?.unwrap();
        let mut draft = notetypes::Draft::of(nt);
        draft.name = "After failed step".into();
        notetypes::save_in(ctx.scope(), &draft, notetypes::SaveOptions::default())?;
        Ok(())
    }).unwrap();
    assert!(last(&seen).structure_report().unwrap().reasons.is_empty());
    let count = seen.0.lock().unwrap().len();
    let failed = h.collection.run(spec(), move |ctx| {
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        let mut report = StructureReport::default();
        report.reasons.insert(R::FieldsChanged);
        report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
        let token = ctx.scope().start_structure_report(report)?;
        nt.fields[0].name = "Whole rollback".into();
        ctx.scope().put_notetype(&nt)?;
        ctx.scope().complete_structure_report(token)?;
        Err::<(), _>(EngineError::InvalidArgument { message: "whole op".into() })
    });
    assert!(failed.is_err());
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert_eq!(read(&h, id).fields[0].name, "Front");
}

// The maps drive these minimal framework writes; none is inferred from the
// resulting row order. This does not implement a product field/template editor.
fn mapped_edit(h: &Harness, id: NotetypeId, fields: Vec<i32>, templates: Vec<i32>, sort: u32, reasons: Vec<R>) {
    h.collection.run(spec(), move |ctx| {
        let old = ctx.scope().notetype(id)?.unwrap();
        let mut new = old.clone();
        new.fields = fields.iter().enumerate().map(|(i, from)| {
            let mut field = old.fields[if *from < 0 { 0 } else { *from as usize }].clone();
            field.ord = i as u32;
            if *from < 0 { field.name = format!("Added {i}"); field.config.id = None; }
            field
        }).collect();
        new.templates = templates.iter().enumerate().map(|(i, from)| {
            let mut template = old.templates[if *from < 0 { 0 } else { *from as usize }].clone();
            template.ord = i as u32;
            if *from < 0 { template.name = format!("Added template {i}"); template.config.id = None; }
            template
        }).collect();
        new.config.sort_field_idx = sort;
        let mut report = StructureReport::default();
        report.reasons.extend(reasons);
        report.layouts.insert(id.0, LayoutReport { before: LayoutShape::of(&old)?, after: LayoutShape::of(&new)?, field_map: fields, template_map: templates });
        // Schema guard precedence/SCM restoration has its own existing tests.
        meguri_engine::mark_schema_change(ctx.scope(), &[meguri_engine::SchemaReason::FieldsChanged])?;
        let token = ctx.scope().start_structure_report(report)?;
        ctx.scope().put_notetype(&new)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
}

#[test]
fn field_and_template_add_remove_reorder_and_sort_keep_old_arities_for_inverse() {
    let (h, seen) = rig();
    let id = add(&h, "Mapped", false);
    mapped_edit(&h, id, vec![0, 1, -1], vec![0, -1], 0, vec![R::FieldsChanged, R::TemplatesChanged]);
    mapped_edit(&h, id, vec![2, 0, 1], vec![1, 0], 0, vec![R::FieldsChanged, R::TemplatesChanged]);
    let original = last(&seen).structure_report().unwrap().clone();
    h.collection.undo(UndoScope::Global).unwrap();
    let inverse = last(&seen);
    assert_eq!(inverse.structure_report().unwrap().layouts[&id.0].field_map, [1, 2, 0]);
    assert_eq!(inverse.structure_report().unwrap().layouts[&id.0].template_map, [1, 0]);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report(), Some(&original));
    mapped_edit(&h, id, vec![0, 1], vec![0], 0, vec![R::FieldsChanged, R::TemplatesChanged]);
    h.collection.undo(UndoScope::Global).unwrap();
    let inverse = last(&seen);
    assert_eq!(inverse.structure_report().unwrap().layouts[&id.0].field_map, [0, 1, -1]);
    assert_eq!(inverse.structure_report().unwrap().layouts[&id.0].template_map, [0, -1]);
    h.collection.redo(UndoScope::Global).unwrap();
    mapped_edit(&h, id, vec![0, 1], vec![0], 1, vec![R::SortFieldChanged]);
    assert_eq!(last(&seen).structure_report().unwrap().reasons, [R::SortFieldChanged].into());
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(read(&h, id).config.sort_field_idx, 0);
}

#[test]
fn native_rename_plain_save_and_preset_removal_report_executed_facts() {
    use meguri_engine::notetypes::{Draft, FieldIdentity, FieldRename, RenameFields, SaveOptions};
    use meguri_model::ids::DeckConfigId;
    for cloze in [false, true] {
        let (h, seen) = rig();
        let id = add(&h, "Native", cloze);
        let old = read(&h, id);
        notetypes::rename_fields(&h.collection, RenameFields {
            notetype: id, base: notetypes::AggregateDigest::of(&old),
            fields: vec![FieldRename { field: FieldIdentity::of(&old.fields[0]), name: "Prompt".into() }],
        }, meguri_engine::CancelToken::new(), meguri_engine::ProgressSink::new()).unwrap();
        let rename = last(&seen);
        let report = rename.structure_report().unwrap();
        assert!(report.reasons.is_empty());
        assert_eq!(report.layouts[&id.0].field_map, [0, 1]);
        assert!(report.layouts[&id.0].is_identity());
        assert!(rename.identity_layout(id).is_some());
        let mut draft = Draft::of(read(&h, id));
        draft.name = "Native saved".into();
        notetypes::save(&h.collection, draft, SaveOptions::default()).unwrap();
        assert!(last(&seen).structure_report().unwrap().reasons.is_empty());
        assert!(last(&seen).identity_layout(id).is_some());
        let preset = meguri_engine::deck_config::add(&h.collection, "Temporary".into()).unwrap().value;
        meguri_engine::deck_config::remove(&h.collection, preset, DeckConfigId(1)).unwrap();
        let deleted = last(&seen);
        let report = deleted.structure_report().unwrap();
        assert_eq!(report.reasons, [R::PresetRemoved].into());
        assert!(report.layouts.is_empty() && report.removed.is_empty() && report.type_changes.is_empty());
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(last(&seen).structure_report().unwrap().reasons, [R::PresetRemoved].into());
        h.collection.redo(UndoScope::Global).unwrap();
        assert_eq!(last(&seen).structure_report(), Some(report));
    }
}

#[test]
fn a_later_native_plain_save_cannot_authorize_an_earlier_unreported_layout_write() {
    let (h, seen) = rig();
    let id = add(&h, "Authority", false);
    let before = read(&h, id);
    let count = seen.0.lock().unwrap().len();
    let events = h.engine.stream().subscribe();
    let result = h.collection.run(spec(), move |ctx| {
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        nt.fields[0].name = "Sneaked".into();
        nt.templates[0].config.q_format = "{{Sneaked}}".into();
        ctx.scope().put_notetype(&nt)?;
        let mut draft = notetypes::Draft::of(ctx.scope().notetype(id)?.unwrap());
        draft.name = "Later plain save".into();
        notetypes::save_in(ctx.scope(), &draft, notetypes::SaveOptions::default())?;
        Ok(())
    });
    assert!(matches!(result, Err(EngineError::Storage(meguri_storage::StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
    assert_eq!(read(&h, id).fields[0].name, before.fields[0].name);
    assert_eq!(read(&h, id).name, before.name);
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert!(!common::drain(&events, std::time::Duration::from_millis(10)).iter().any(|e| matches!(&e.kind, meguri_engine::EventKind::Changes(_))));
}

#[test]
fn a_missing_old_type_has_no_invented_layout_and_two_valid_destinations_keep_two_pairs() {
    let (h, seen) = rig();
    let old = add(&h, "Source", false);
    let a = add(&h, "Destination A", false);
    let b = add(&h, "Destination B", false);
    let mut note = common::note(&["front", "back"]);
    note.notetype_id = old;
    let (n1, n2, missing) = h.collection.run(spec(), move |ctx| {
        let n1 = ctx.scope().add_note(&note)?;
        note.guid = "second".into();
        let n2 = ctx.scope().add_note(&note)?;
        note.guid = "missing-type".into();
        note.notetype_id = NotetypeId(-9999);
        Ok((n1, n2, ctx.scope().add_note(&note)?))
    }).unwrap().value;
    h.collection.run(spec(), move |ctx| {
        let before = LayoutShape::of(&ctx.scope().notetype(old)?.unwrap())?;
        let mut report = StructureReport::default();
        report.reasons.extend([R::NotesNotetypeChanged, R::CheckDatabaseRepair]);
        for destination in [a, b] {
            report.type_changes.insert((old.0, destination.0), TypeChangeReport { before,
                after: LayoutShape::of(&ctx.scope().notetype(destination)?.unwrap())?, field_map: vec![0, 1], template_map: vec![0] });
        }
        let token = ctx.scope().start_structure_report(report)?;
        for (nid, destination) in [(n1, a), (n2, b), (missing, a)] {
            let mut note = ctx.scope().note(nid)?.unwrap();
            note.notetype_id = destination;
            ctx.scope().put(&note)?;
        }
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
    let retained = last(&seen);
    let report = retained.structure_report().unwrap();
    assert_eq!(report.type_changes.keys().copied().collect::<Vec<_>>(), [(old.0, a.0), (old.0, b.0)]);
    assert!(report.layouts.is_empty());
}

#[test]
fn composite_maps_follow_authored_call_order_and_failed_nested_writes_leave_no_map() {
    let (h, seen) = rig();
    let id = add(&h, "Composite", false);
    mapped_edit(&h, id, vec![0, 1, -1], vec![0], 0, vec![R::FieldsChanged]);
    h.collection.run(spec(), move |ctx| {
        for origins in [vec![1, 2, 0], vec![0, 2, 1]] {
            let old = ctx.scope().notetype(id)?.unwrap();
            let mut new = old.clone();
            new.fields = origins.iter().enumerate().map(|(ord, origin)| {
                let mut field = old.fields[*origin as usize].clone(); field.ord = ord as u32; field
            }).collect();
            let mut report = StructureReport::default();
            report.reasons.insert(R::FieldsChanged);
            report.layouts.insert(id.0, LayoutReport { before: LayoutShape::of(&old)?, after: LayoutShape::of(&new)?, field_map: origins, template_map: vec![0] });
            let token = ctx.scope().start_structure_report(report)?;
            ctx.scope().put_notetype(&new)?;
            ctx.scope().complete_structure_report(token)?;
        }
        let failed: Result<(), EngineError> = ctx.step(|ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.reasons.insert(R::NotetypeRestored);
            report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
            let token = ctx.scope().start_structure_report(report)?;
            nt.fields[0].name = "Nested failure".into();
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
            Err(EngineError::InvalidArgument { message: "nested".into() })
        });
        assert!(failed.is_err());
        Ok(())
    }).unwrap();
    let committed = last(&seen);
    let report = committed.structure_report().unwrap();
    assert_eq!(report.layouts[&id.0].field_map, [1, 0, 2]);
    assert_eq!(report.reasons, [R::FieldsChanged].into());
    assert_ne!(read(&h, id).fields[0].name, "Nested failure");
}

#[test]
fn authored_removal_undo_addition_and_redo_reuse_the_original_report() {
    let (h, seen) = rig();
    let id = add(&h, "Removed", false);
    h.collection.run(spec().schema_changes([meguri_engine::SchemaReason::NotetypeRemoved]), move |ctx| {
        let mut report = StructureReport::default();
        report.reasons.insert(R::NotetypeRemoved);
        report.removed.insert(id.0);
        let token = ctx.scope().start_structure_report(report)?;
        meguri_engine::mark_schema_change(ctx.scope(), &[meguri_engine::SchemaReason::NotetypeRemoved])?;
        ctx.scope().remove_notetype(id)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
    let removed = last(&seen);
    let original = removed.structure_report().unwrap();
    assert!(original.removed.contains(&id.0));
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(last(&seen).structure_report().unwrap().removed.is_empty());
    assert!(last(&seen).structure_report().unwrap().layouts.is_empty());
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report(), Some(original));
}

#[test]
fn deck_options_removal_also_reports_seven() {
    use meguri_engine::sched::deck_options as options;
    use meguri_model::ids::DeckId;
    let (h, seen) = rig();
    let preset = meguri_engine::deck_config::add(&h.collection, "Options temporary".into()).unwrap().value;
    let mut draft = options::Draft::new(options::snapshot(&h.collection, DeckId(1)).unwrap());
    draft.remove(preset.0).unwrap();
    options::save(&h.collection, draft, &Default::default()).unwrap();
    let committed = last(&seen);
    let report = committed.structure_report().unwrap();
    assert_eq!(report.reasons, [R::PresetRemoved].into());
    assert!(report.layouts.is_empty());
}

#[test]
fn inverse_type_change_can_restore_its_removed_destination_type() {
    // Framework coverage for the approved future merge/Change Note Type
    // report shape. No product merge or Change Note Type implementation.
    let (h, seen) = rig();
    let old = add(&h, "Removed source", false);
    let new = add(&h, "Kept destination", false);
    let mut note = common::note(&["front", "back"]);
    note.notetype_id = old;
    let nid = h.collection.run(spec(), move |ctx| Ok(ctx.scope().add_note(&note)?)).unwrap().value;
    h.collection.run(spec(), move |ctx| {
        let mut report = StructureReport::default();
        report.reasons.extend([R::NotetypeRemoved, R::NotesNotetypeChanged]);
        report.removed.insert(old.0);
        report.type_changes.insert((old.0, new.0), TypeChangeReport {
            before: LayoutShape::of(&ctx.scope().notetype(old)?.unwrap())?,
            after: LayoutShape::of(&ctx.scope().notetype(new)?.unwrap())?,
            field_map: vec![0, 1], template_map: vec![0],
        });
        let token = ctx.scope().start_structure_report(report)?;
        let mut note = ctx.scope().note(nid)?.unwrap();
        note.notetype_id = new;
        ctx.scope().put(&note)?;
        ctx.scope().remove_notetype(old)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
    let retained = last(&seen);
    let original = retained.structure_report().unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(last(&seen).structure_report().unwrap().type_changes.contains_key(&(new.0, old.0)));
    assert!(last(&seen).structure_report().unwrap().removed.is_empty());
    assert_eq!(read(&h, old).name, "Removed source");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(last(&seen).structure_report(), Some(original));
}

// Primitive coverage only. The production normal-sync/local-stub and host
// CloudKit recovery suites remain separate acceptance requirements.
mod normal_transfer {
    use super::*;
    use meguri_model::notetype::Notetype;
    use meguri_storage::{StampMode, StorageError};

    fn incoming() -> OpSpec {
        OpSpec::new(meguri_engine::executor::catalog::SYNC_INCOMING_MERGE, OpSource::SyncIncoming)
            .stamp(StampMode::CallerSupplied)
    }
    fn swap(nt: &mut Notetype) {
        nt.fields.swap(0, 1);
        for (ord, field) in nt.fields.iter_mut().enumerate() { field.ord = ord as u32; }
    }
    fn rows(h: &Harness, id: NotetypeId) -> Vec<meguri_storage::RowImage> {
        let r = h.collection.reader(ReaderRole::Query).unwrap();
        let mut rows = vec![r.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap()];
        rows.extend(r.notetype_fields_page(id, None).unwrap());
        rows.extend(r.notetype_templates_page(id, None).unwrap());
        rows
    }
    fn missing<T>(result: Result<T, EngineError>, id: NotetypeId) {
        assert!(matches!(result, Err(EngineError::Storage(StorageError::StructureNotReported { notetype_id })) if notetype_id == id));
    }
    fn authored_rename(ctx: &mut meguri_engine::OpContext<'_, '_>, id: NotetypeId, name: &str) -> Result<(), EngineError> {
        let mut nt = ctx.scope().notetype(id)?.unwrap();
        let mut report = StructureReport::default();
        report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
        let token = ctx.scope().start_structure_report(report)?;
        nt.fields[0].name = name.into();
        ctx.scope().put_notetype(&nt)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }

    #[test]
    fn accepted_sync_rename_and_swap_commit_without_report_or_identity_authority() {
        for swapped in [false, true] {
            let (h, seen) = rig();
            let id = add(&h, "Accepted incoming", false);
            let mut nt = read(&h, id);
            if swapped { swap(&mut nt); } else { nt.fields[0].name = "Incoming prompt".into(); }
            // Equal counts and deliberately unchanged optional IDs are not maps.
            let expected = nt.fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>();
            let count = seen.0.lock().unwrap().len();
            h.collection.run(incoming(), move |ctx| {
                ctx.put_accepted_sync_notetype(&nt)?;
                assert!(ctx.scope().journal().unwrap().structure_report().is_none());
                assert!(ctx.scope().journal().unwrap().identity_layout(id.0).is_none());
                Ok(())
            }).unwrap();
            assert_eq!(read(&h, id).fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), expected);
            assert_eq!(seen.0.lock().unwrap().len(), count + 1);
            let payload = last(&seen);
            assert!(payload.structure_report().is_none()); // no map, reason or structural generation input
            assert!(payload.identity_layout(id).is_none());
            assert!(payload.row(Table::Fields, &RowKey::Pair(id.0, 0)).unwrap().is_some());
        }
    }

    #[test]
    fn accepted_sync_helper_rejects_every_other_source_including_direct_storage_calls() {
        for source in OpSource::ALL.iter().copied().filter(|s| *s != OpSource::SyncIncoming) {
            for direct_storage in [false, true] {
                let (h, seen) = rig();
                let id = add(&h, "Wrong source", false);
                let before = rows(&h, id);
                let count = seen.0.lock().unwrap().len();
                let result = h.collection.run(OpSpec::named("wrong_incoming_source", source), move |ctx| {
                    let mut nt = ctx.scope().notetype(id)?.unwrap();
                    swap(&mut nt);
                    if direct_storage { ctx.scope().put_accepted_sync_notetype(&nt)?; }
                    else { ctx.put_accepted_sync_notetype(&nt)?; }
                    Ok(())
                });
                if direct_storage { assert!(matches!(result, Err(EngineError::Storage(StorageError::Mode { .. })))); }
                else { assert!(matches!(result, Err(EngineError::InvalidArgument { .. }))); }
                assert_eq!(rows(&h, id), before);
                assert_eq!(seen.0.lock().unwrap().len(), count);
            }
        }
    }

    #[test]
    fn unreported_writes_around_receipt_fail_with_equal_endpoints() {
        for raw_before in [false, true] {
            for restore_initial in [false, true] {
                let (h, seen) = rig();
                let id = add(&h, "Scoped incoming", false);
                let before = rows(&h, id);
                let count = seen.0.lock().unwrap().len();
                let retained = last(&seen);
                let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
                let result = h.collection.run(incoming(), move |ctx| {
                    let raw = |ctx: &mut meguri_engine::OpContext<'_, '_>| -> Result<(), EngineError> {
                        let base = ctx.scope().notetype(id)?.unwrap();
                        let mut changed = base.clone();
                        swap(&mut changed);
                        ctx.scope().put_notetype(&changed)?;
                        if restore_initial { ctx.scope().put_notetype(&base)?; }
                        Ok(())
                    };
                    if raw_before { raw(ctx)?; }
                    let mut nt = ctx.scope().notetype(id)?.unwrap();
                    nt.fields[0].name = "Finally accepted".into();
                    ctx.put_accepted_sync_notetype(&nt)?;
                    if !raw_before { raw(ctx)?; }
                    Ok(())
                });
                missing(result, id);
                assert_eq!(rows(&h, id), before);
                assert_eq!(seen.0.lock().unwrap().len(), count);
                assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
                assert!(retained.row(Table::Fields, &RowKey::Pair(id.0, 0)).unwrap().is_some());
            }
        }
    }

    #[test]
    fn earlier_native_report_is_discarded_and_later_native_report_cannot_bless_incoming() {
        let (h, seen) = rig();
        let id = add(&h, "Native then incoming", false);
        h.collection.run(incoming(), move |ctx| {
            authored_rename(ctx, id, "Native prompt")?;
            assert!(ctx.scope().journal().unwrap().identity_layout(id.0).is_some());
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            swap(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            Ok(())
        }).unwrap();
        assert!(last(&seen).structure_report().is_none());
        assert!(last(&seen).identity_layout(id).is_none());
        let before = rows(&h, id);
        let count = seen.0.lock().unwrap().len();
        let result = h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            swap(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            authored_rename(ctx, id, "Later native")
        });
        missing(result, id);
        assert_eq!(rows(&h, id), before);
        assert_eq!(seen.0.lock().unwrap().len(), count);
    }

    #[test]
    fn incoming_replacement_supersedes_a_preexisting_report_token() {
        let (h, _) = rig();
        let id = add(&h, "Spanning token", false);
        let before = rows(&h, id);
        let result = h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
            let token = ctx.scope().start_structure_report(report)?;
            swap(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        });
        assert!(matches!(result, Err(EngineError::Storage(StorageError::Mode { .. }))));
        assert_eq!(rows(&h, id), before);
    }

    #[test]
    fn savepoint_removes_incoming_receipt_and_restores_prior_native_map() {
        let (h, seen) = rig();
        let id = add(&h, "Incoming savepoint", false);
        h.collection.run(incoming(), move |ctx| {
            authored_rename(ctx, id, "Kept native")?;
            let report = ctx.scope().journal().unwrap().structure_report().unwrap().clone();
            let failed: Result<(), EngineError> = ctx.step(|ctx| {
                let mut nt = ctx.scope().notetype(id)?.unwrap();
                swap(&mut nt);
                ctx.put_accepted_sync_notetype(&nt)?;
                assert!(ctx.scope().journal().unwrap().structure_report().is_none());
                Err(EngineError::InvalidArgument { message: "rollback incoming step".into() })
            });
            assert!(failed.is_err());
            assert_eq!(ctx.scope().journal().unwrap().structure_report(), Some(&report));
            assert!(ctx.scope().journal().unwrap().identity_layout(id.0).is_some());
            // If a receipt survived, this native contribution would be refused.
            authored_rename(ctx, id, "Native after rollback")
        }).unwrap();
        assert!(last(&seen).identity_layout(id).is_some());
        assert_eq!(read(&h, id).fields[0].name, "Native after rollback");
    }

    #[test]
    fn savepoint_restores_exact_incoming_coverage_without_covering_another_id() {
        let (h, seen) = rig();
        let id = add(&h, "Kept incoming", false);
        let other = add(&h, "Rolled back incoming", false);
        h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            swap(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            let failed: Result<(), EngineError> = ctx.step(|ctx| {
                // Invalidate the earlier receipt, add a second one, then roll both back.
                let mut raw = ctx.scope().notetype(id)?.unwrap();
                swap(&mut raw);
                ctx.scope().put_notetype(&raw)?;
                let mut incoming = ctx.scope().notetype(other)?.unwrap();
                swap(&mut incoming);
                ctx.put_accepted_sync_notetype(&incoming)?;
                Err(EngineError::InvalidArgument { message: "rollback exact receipt".into() })
            });
            assert!(failed.is_err());
            authored_rename(ctx, other, "Other native after rollback")?;
            Ok(())
        }).unwrap();
        let payload = last(&seen);
        let report = payload.structure_report().unwrap();
        assert!(!report.layouts.contains_key(&id.0));
        assert!(payload.identity_layout(id).is_none());
        assert!(payload.identity_layout(other).is_some());
        assert_eq!(read(&h, id).fields[0].name, "Back");
    }

    #[test]
    fn whole_op_rollback_keeps_old_rows_and_retained_native_evidence() {
        let (h, seen) = rig();
        let id = add(&h, "Whole incoming rollback", false);
        h.collection.run(spec(), move |ctx| authored_rename(ctx, id, "Native evidence")).unwrap();
        let retained = last(&seen);
        let report = retained.structure_report().unwrap().clone();
        let before = rows(&h, id);
        let count = seen.0.lock().unwrap().len();
        let result = h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            swap(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            Err::<(), _>(EngineError::InvalidArgument { message: "abort accepted aggregate".into() })
        });
        assert!(matches!(result, Err(EngineError::InvalidArgument { .. })));
        assert_eq!(rows(&h, id), before);
        assert_eq!(seen.0.lock().unwrap().len(), count);
        assert_eq!(retained.structure_report(), Some(&report));
        assert!(retained.identity_layout(id).is_some());
    }

    #[test]
    fn incoming_receipt_exempts_neither_note_mid_changes_nor_removal() {
        for removal in [false, true] {
            let (h, seen) = rig();
            let id = add(&h, "Receipt boundary fixture", false);
            let other = add(&h, "Other note type", false);
            let mut note = common::note(&["front", "back"]);
            note.notetype_id = id;
            let nid = h.collection.run(spec(), move |ctx| Ok(ctx.scope().add_note(&note)?)).unwrap().value;
            let before = rows(&h, id);
            let count = seen.0.lock().unwrap().len();
            let result = h.collection.run(incoming(), move |ctx| {
                let mut nt = ctx.scope().notetype(id)?.unwrap();
                swap(&mut nt);
                ctx.put_accepted_sync_notetype(&nt)?;
                if removal { ctx.scope().remove_notetype(id)?; }
                else {
                    let mut note = ctx.scope().note(nid)?.unwrap();
                    note.notetype_id = other;
                    ctx.scope().put(&note)?;
                }
                Ok(())
            });
            missing(result, id);
            assert_eq!(rows(&h, id), before);
            assert_eq!(seen.0.lock().unwrap().len(), count);
            assert_eq!(h.collection.reader(ReaderRole::Query).unwrap().note(nid).unwrap().unwrap().notetype_id, id);
        }
    }

    #[test]
    fn accepted_metadata_only_write_cannot_authorize_a_prior_raw_swap() {
        for restore_initial in [false, true] {
            let (h, _) = rig();
            let id = add(&h, "Metadata is not structural coverage", false);
            let before = rows(&h, id);
            let result = h.collection.run(incoming(), move |ctx| {
                let mut nt = ctx.scope().notetype(id)?.unwrap();
                swap(&mut nt);
                ctx.scope().put_notetype(&nt)?;
                if restore_initial {
                    swap(&mut nt);
                    ctx.scope().put_notetype(&nt)?;
                }
                nt.name = "Incoming metadata".into();
                nt.config.css = ".card { color: inherit; }".into();
                nt.templates[0].config.q_format = "<p>{{Front}}</p>".into();
                nt.mtime.0 += 1;
                ctx.put_accepted_sync_notetype(&nt)?;
                Ok(())
            });
            missing(result, id);
            assert_eq!(rows(&h, id), before);
        }
    }

    // An accepted aggregate has unknown origins even when every layout name,
    // count, kind and sort index matches. This is deliberately not a raw native
    // metadata update (whose reviewed control cases above retain their maps).
    fn replace_same_name_data(nt: &mut Notetype) {
        nt.mtime.0 += 1;
        nt.usn.0 += 1;
        nt.config.css = ".card { color: inherit; }".into();
        nt.config.original_id = Some(nt.config.original_id.map_or(-303, |id| id ^ 1));
        for (ord, field) in nt.fields.iter_mut().enumerate() {
            field.config.id = Some(field.config.id.map_or(-101 - ord as i64, |id| id ^ 1));
            field.config.sticky = !field.config.sticky;
        }
        for (ord, template) in nt.templates.iter_mut().enumerate() {
            template.mtime.0 += 1;
            template.usn.0 += 1;
            template.config.id = Some(template.config.id.map_or(-201 - ord as i64, |id| id ^ 1));
            template.config.q_format = "<p>{{Front}}</p>".into();
            template.config.a_format = "<p>{{Back}}</p>".into();
        }
    }

    #[test]
    fn accepted_same_name_replacement_discards_an_earlier_nonidentity_native_map() {
        let (h, seen) = rig();
        let id = add(&h, "Same-name replacement after native swap", false);
        let count = seen.0.lock().unwrap().len();
        h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let shape = LayoutShape::of(&nt)?;
            let mut report = StructureReport::default();
            report.reasons.insert(R::FieldsChanged);
            report.layouts.insert(id.0, LayoutReport {
                before: shape, after: shape, field_map: vec![1, 0], template_map: vec![0],
            });
            let token = ctx.scope().start_structure_report(report)?;
            swap(&mut nt);
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
            assert_eq!(ctx.scope().journal().unwrap().structure_report().unwrap().layouts[&id.0].field_map,
                [1, 0], "the completed native contribution must really carry nonidentity origins");

            // The incoming replacement preserves the current swapped names;
            // IDs and non-layout data cannot certify those native origins.
            replace_same_name_data(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            Ok(())
        }).expect("a same-name accepted aggregate must COMMIT without requiring a fabricated map");
        assert_eq!(seen.0.lock().unwrap().len(), count + 1,
            "the accepted replacement must reach the commit observer");
        assert_eq!(read(&h, id).fields.iter().map(|f| f.name.clone()).collect::<Vec<_>>(), ["Back", "Front"],
            "the replacement must preserve the names after the authored native swap");
        let payload = last(&seen);
        // The original field-change reason can survive the authored swap.
        // This asserts only map authority; generation integration is separate.
        if let Some(report) = payload.structure_report() {
            assert!(!report.layouts.contains_key(&id.0),
                "accepted same-name replacement retained an earlier native map with unknown incoming origins: {report:?}");
        }
        assert!(payload.identity_layout(id).is_none(),
            "accepted same-name replacement must expose no native identity authority");
    }

    #[test]
    fn accepted_same_name_replacement_cannot_supply_origins_to_a_later_type_change() {
        let (h, seen) = rig();
        let id = notetypes::add_stock(&h.collection, notetypes::StockKind::Basic,
            Some("Same-name incoming source".into())).unwrap().value.id;
        let other = notetypes::add_stock(&h.collection, notetypes::StockKind::Basic,
            Some("Same-name incoming destination".into())).unwrap().value.id;
        let mut note = common::note(&["front", "back"]);
        note.notetype_id = id;
        let nid = h.collection.run(spec(), move |ctx| Ok(ctx.scope().add_note(&note)?)).unwrap().value;
        let before = (rows(&h, id), rows(&h, other));
        let persisted = || {
            let r = h.collection.reader(ReaderRole::Query).unwrap();
            (r.row_image(Table::Notes, &RowKey::Id(nid.0)).unwrap(),
                r.row_image(Table::Col, &RowKey::Id(1)).unwrap())
        };
        let before_persisted = persisted();
        let count = seen.0.lock().unwrap().len();
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let result = h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            replace_same_name_data(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            let before = LayoutShape::of(&nt)?;
            let after = LayoutShape::of(&ctx.scope().notetype(other)?.unwrap())?;
            let mut report = StructureReport::default();
            report.reasons.insert(R::NotesNotetypeChanged);
            report.type_changes.insert((id.0, other.0), TypeChangeReport {
                before, after, field_map: vec![1, 0], template_map: vec![0],
            });
            let token = ctx.scope().start_structure_report(report)?;
            let mut note = ctx.scope().note(nid)?.unwrap();
            note.notetype_id = other;
            note.fields = "back\u{1f}front".into();
            note.sort_field = "back".into();
            ctx.scope().put(&note)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        });
        assert!(matches!(&result, Err(EngineError::Storage(StorageError::StructureNotReported { notetype_id })) if *notetype_id == id),
            "a later type-change map must not gain origin authority from a same-name incoming replacement: {result:?}");
        assert_eq!((rows(&h, id), rows(&h, other)), before,
            "refused type change must roll back both note-type aggregates byte for byte");
        assert_eq!(persisted(), before_persisted,
            "refused type change must restore the complete note row, including mid/fields, and collection row");
        assert_eq!(seen.0.lock().unwrap().len(), count,
            "refused type change must emit no observer commit");
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo,
            "refused type change must leave undo state unchanged");
    }

    #[test]
    fn accepted_same_name_replacement_supersedes_an_open_native_contribution_token() {
        let (h, seen) = rig();
        let id = add(&h, "Same-name replacement spanning token", false);
        let before = rows(&h, id);
        let collection_row = || h.collection.reader(ReaderRole::Query).unwrap()
            .row_image(Table::Col, &RowKey::Id(1)).unwrap();
        let before_collection = collection_row();
        let count = seen.0.lock().unwrap().len();
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let result = h.collection.run(incoming(), move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            let mut report = StructureReport::default();
            report.layouts.insert(id.0, LayoutReport::identity(&nt)?);
            let token = ctx.scope().start_structure_report(report)?;
            replace_same_name_data(&mut nt);
            ctx.put_accepted_sync_notetype(&nt)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        });
        assert!(matches!(&result, Err(EngineError::Storage(StorageError::Mode { detail }))
            if detail == "invalid structure report: foreign or superseded contribution token"),
            "a same-name accepted replacement must supersede the already-open native token: {result:?}");
        assert_eq!(rows(&h, id), before,
            "superseded-token refusal must restore every aggregate row byte for byte");
        assert_eq!(collection_row(), before_collection,
            "superseded-token refusal must restore the complete collection row");
        assert_eq!(seen.0.lock().unwrap().len(), count,
            "superseded-token refusal must emit no observer commit");
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo,
            "superseded-token refusal must leave undo state unchanged");
    }
}
#[path = "scope_ownership.rs"]
mod scope_ownership;

#[path = "cloze_composition.rs"]
mod cloze_composition;
