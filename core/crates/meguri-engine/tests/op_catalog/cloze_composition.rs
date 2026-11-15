//! K19.2/S2.5/M9.11 executor regressions, not U35 product-op coverage.
//! Each primitive contribution finishes before the next one starts.
use super::*;
use meguri_engine::{SchemaReason, executor::catalog};
use meguri_model::{
    clock::Clock,
    grave::GraveKind,
    ids::{CardId, NoteId},
    proto::ProtoBlob,
    time::EpochSecs,
    usn::Usn,
};
use meguri_storage::{Cell, RowImage, StampMode, StorageError};

const FRONT: &str = "{{c1::one}} {{c2::two}} {{c3::three}}";

fn standard(h: &Harness, name: &str, count: usize) -> NotetypeId {
    notetypes::add(&h.collection, NewNotetype {
        name: name.into(),
        fields: vec!["Front".into(), "Back".into()],
        templates: (0..count).map(|i| NewTemplate {
            name: format!("View {i}"),
            front: format!("View {i}: {{{{Front}}}}"),
            back: "{{Front}}<hr>{{Back}}".into(),
            ..Default::default()
        }).collect(),
        ..Default::default()
    }).unwrap().value.id
}

// Explicit framework rows: all three cards are required by the cloze text,
// and by standard3's nonempty templates. Historical stamps and opaque config
// bytes make restoration stronger than a mid/ordinal-only comparison.
fn seed(h: &Harness, types: &[NotetypeId]) -> (NoteId, [CardId; 3]) {
    let types = types.to_vec();
    let source = types[0];
    let result = h.collection.run(
        OpSpec::named("seed_joined_cloze_framework", OpSource::UserNonUndoable)
            .stamp(StampMode::CallerSupplied),
        move |ctx| {
            for id in types {
                let mut nt = ctx.scope().notetype(id)?.unwrap();
                nt.mtime = EpochSecs(42);
                nt.usn = Usn(8);
                let mut config = nt.config.to_bytes().into_owned();
                config.extend_from_slice(&[0xa0, 0x01, 0x07]);
                nt.config = ProtoBlob::decode(&config).unwrap();
                for (i, field) in nt.fields.iter_mut().enumerate() {
                    field.config.id = Some(-101 - i as i64);
                    let mut config = field.config.to_bytes().into_owned();
                    config.extend_from_slice(&[0xa0, 0x01, 0x07]);
                    field.config = ProtoBlob::decode(&config).unwrap();
                }
                for (i, template) in nt.templates.iter_mut().enumerate() {
                    template.mtime = EpochSecs(321 + i as i64);
                    template.usn = Usn(19);
                    template.config.id = Some(-201 - i as i64);
                    let mut config = template.config.to_bytes().into_owned();
                    config.extend_from_slice(&[0xa0, 0x01, 0x07]);
                    template.config = ProtoBlob::decode(&config).unwrap();
                }
                ctx.scope().put_notetype(&nt)?;
            }
            let mut note = common::note(&[FRONT, "answer"]);
            note.notetype_id = source;
            note.sort_field = meguri_text::checksum::sort_text(FRONT).into_owned();
            note.checksum = meguri_text::checksum::note_checksum(FRONT);
            note.mtime = EpochSecs(43);
            note.usn = Usn(9);
            note.data = "framework-note-data".into();
            let nid = ctx.scope().add_note(&note)?;
            let mut ids = [CardId(0); 3];
            for (ord, id) in ids.iter_mut().enumerate() {
                let mut card = common::card(nid, ord as u16);
                card.mtime = EpochSecs(44 + ord as i64);
                card.usn = Usn(10 + ord as i32);
                card.due = 11 + ord as i32;
                card.flags = ord as u8;
                *id = ctx.scope().add_card(&card)?;
            }
            Ok((nid, ids))
        },
    ).unwrap().value;
    h.collection.clear_undo(meguri_engine::ClearCause::ExternalChange).unwrap();
    h.clock.set_now_ms(common::NOW_MS + 120_000);
    result
}

#[derive(Clone, Debug, PartialEq)]
struct Aggregate {
    head: RowImage,
    fields: Vec<RowImage>,
    templates: Vec<RowImage>,
}

#[derive(Clone, Debug, PartialEq)]
struct Rows {
    types: Vec<Aggregate>,
    note: RowImage,
    cards: [Option<RowImage>; 3],
    graves: [Option<RowImage>; 3],
    live: Vec<(CardId, u16)>,
    col: RowImage,
}

fn rows(h: &Harness, types: &[NotetypeId], nid: NoteId, ids: [CardId; 3]) -> Rows {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    Rows {
        types: types.iter().map(|id| Aggregate {
            head: r.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap(),
            fields: r.notetype_fields_page(*id, None).unwrap(),
            templates: r.notetype_templates_page(*id, None).unwrap(),
        }).collect(),
        note: r.row_image(Table::Notes, &RowKey::Id(nid.0)).unwrap().unwrap(),
        cards: std::array::from_fn(|i| r.row_image(Table::Cards, &RowKey::Id(ids[i].0)).unwrap()),
        graves: std::array::from_fn(|i| r.row_image(Table::Graves,
            &RowKey::Pair(ids[i].0, GraveKind::Card.stored())).unwrap()),
        live: r.cards_of_note(nid).unwrap().iter().map(|c| (c.id, c.ord)).collect(),
        col: r.row_image(Table::Col, &RowKey::Id(1)).unwrap().unwrap(),
    }
}

fn expected_forward(before: &Rows, ids: [CardId; 3], destination: NotetypeId, now: i64, swap: bool) -> Rows {
    let mut after = before.clone();
    after.note[2] = Cell::Integer(destination.0);
    after.note[3] = Cell::Integer(now / 1000);
    after.note[4] = Cell::Integer(-1);
    after.cards[2] = None;
    after.graves[2] = Some(vec![Cell::Integer(ids[2].0), Cell::Integer(0), Cell::Integer(-1)]);
    after.live = vec![(ids[0], 0), (ids[1], 1)];
    after.col[2] = Cell::Integer(now);
    after.col[3] = Cell::Integer(now);
    if swap {
        let b = &mut after.types[1];
        b.head[2] = Cell::Integer(now / 1000);
        b.head[3] = Cell::Integer(-1);
        b.templates.swap(0, 1);
        for (ord, template) in b.templates.iter_mut().enumerate() {
            template[1] = Cell::Integer(ord as i64);
        }
        for i in 0..2 {
            let card = after.cards[i].as_mut().unwrap();
            card[3] = Cell::Integer(1 - i as i64);
            card[4] = Cell::Integer(now / 1000);
            card[5] = Cell::Integer(-1);
        }
        after.live = vec![(ids[0], 1), (ids[1], 0)];
    }
    after
}

fn op(swap: bool) -> OpSpec {
    let reasons = if swap {
        vec![SchemaReason::NotesNotetypeChanged, SchemaReason::TemplatesChanged]
    } else {
        vec![SchemaReason::NotesNotetypeChanged]
    };
    OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable).schema_changes(reasons)
}

fn primitive(ctx: &mut meguri_engine::OpContext<'_, '_>, nid: NoteId,
    old: NotetypeId, new: NotetypeId, prefix: Vec<i32>, drop: Option<CardId>,
) -> Result<(), EngineError> {
    let mut report = StructureReport::default();
    report.reasons.insert(R::NotesNotetypeChanged);
    report.type_changes.insert((old.0, new.0), TypeChangeReport {
        before: LayoutShape::of(&ctx.scope().notetype(old)?.unwrap())?,
        after: LayoutShape::of(&ctx.scope().notetype(new)?.unwrap())?,
        field_map: vec![0, 1], template_map: prefix,
    });
    let token = ctx.scope().start_structure_report(report)?;
    let mut note = ctx.scope().note(nid)?.unwrap();
    assert_eq!(note.notetype_id, old);
    note.notetype_id = new;
    ctx.scope().put(&note)?;
    if let Some(id) = drop {
        ctx.scope().remove_card(id)?;
        ctx.scope().add_grave(id.0, GraveKind::Card, Usn(-1))?;
    }
    ctx.scope().complete_structure_report(token)?;
    Ok(())
}

fn swap_b(ctx: &mut meguri_engine::OpContext<'_, '_>, b: NotetypeId, ids: [CardId; 3]) -> Result<(), EngineError> {
    let mut nt = ctx.scope().notetype(b)?.unwrap();
    let shape = LayoutShape::of(&nt)?;
    let mut report = StructureReport::default();
    report.reasons.insert(R::TemplatesChanged);
    report.layouts.insert(b.0, LayoutReport {
        before: shape, after: shape, field_map: vec![0, 1], template_map: vec![1, 0],
    });
    let token = ctx.scope().start_structure_report(report)?;
    nt.templates.swap(0, 1);
    for (ord, template) in nt.templates.iter_mut().enumerate() { template.ord = ord as u32; }
    ctx.scope().put_notetype(&nt)?;
    for (ord, id) in ids[..2].iter().enumerate() {
        let mut card = ctx.scope().card(*id)?.unwrap();
        assert_eq!(card.ord, ord as u16);
        card.ord = 1 - ord as u16;
        ctx.scope().put(&card)?;
    }
    ctx.scope().complete_structure_report(token)?;
    Ok(())
}

fn pair(old: LayoutShape, new: LayoutShape, templates: &[i32]) -> TypeChangeReport {
    TypeChangeReport { before: old, after: new, field_map: vec![0, 1], template_map: templates.to_vec() }
}

// The second test deliberately leaves the inverse-report assertion to the
// first: wrong inverse metadata must not hide retained Redo admission.
fn cloze_then_swap(check_inverse: bool) {
    let (h, seen) = rig();
    let a = add(&h, "Joined cloze A", true);
    let b = standard(&h, "Joined standard B", 2);
    let types = [a, b];
    let (nid, ids) = seed(&h, &types);
    let before = rows(&h, &types, nid, ids);
    assert_eq!(before.live, vec![(ids[0], 0), (ids[1], 1), (ids[2], 2)]);
    assert!(before.graves.iter().all(Option::is_none));
    let expected = expected_forward(&before, ids, b, h.clock.now_ms(), true);
    let sa = LayoutShape::of(&read(&h, a)).unwrap();
    let sb = LayoutShape::of(&read(&h, b)).unwrap();
    assert!(sa.cloze && !sb.cloze);
    assert_eq!((sa.templates, sb.templates), (1, 2));
    let count = seen.0.lock().unwrap().len();
    h.collection.run(op(true), move |ctx| {
        meguri_engine::mark_schema_change(ctx.scope(),
            &[SchemaReason::NotesNotetypeChanged, SchemaReason::TemplatesChanged])?;
        primitive(ctx, nid, a, b, vec![], Some(ids[2]))?;
        swap_b(ctx, b, ids)
    }).expect("canonical sequential contributions must reach executor COMMIT");
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    assert_eq!(rows(&h, &types, nid, ids), expected);
    let retained = last(&seen);
    let original = retained.structure_report().unwrap();
    let mut wanted = StructureReport::default();
    wanted.reasons.extend([R::NotesNotetypeChanged, R::TemplatesChanged]);
    wanted.layouts.insert(b.0, LayoutReport {
        before: sb, after: sb, field_map: vec![0, 1], template_map: vec![1, 0],
    });
    wanted.type_changes.insert((a.0, b.0), pair(sa, sb, &[1, 0]));
    assert_eq!(original, &wanted);
    let mut inverse = wanted.clone();
    inverse.type_changes.clear();
    inverse.type_changes.insert((b.0, a.0), pair(sb, sa, &[1, 0]));
    for cycle in 0..2 {
        h.collection.undo(UndoScope::Global).expect("Undo must restore the journal's physical rows");
        assert_eq!(rows(&h, &types, nid, ids), before, "Undo cycle {cycle}");
        if check_inverse {
            assert_eq!(last(&seen).structure_report(), Some(&inverse),
                "Undo must preserve the reverse authored template swap, cycle {cycle}");
        }
        h.collection.redo(UndoScope::Global)
            .expect("retained joined A-to-B [1,0] must be admitted on Redo");
        assert_eq!(last(&seen).structure_report(), Some(original));
        assert_eq!(rows(&h, &types, nid, ids), expected, "Redo cycle {cycle}");
        assert_eq!(retained.structure_report(), Some(&wanted));
    }
    assert_eq!(seen.0.lock().unwrap().len(), count + 5);
}

#[test]
fn cloze_then_template_swap_undo_preserves_reverse_authored_movement() {
    cloze_then_swap(true);
}

#[test]
fn cloze_then_template_swap_retained_redo_admits_original_joined_prefix() {
    cloze_then_swap(false);
}

#[test]
fn standard_three_through_cloze_to_standard_two_has_exact_final_arity() {
    let (h, seen) = rig();
    let a = standard(&h, "Joined standard A3", 3);
    let m = add(&h, "Joined middle cloze", true);
    let c = standard(&h, "Joined standard C2", 2);
    let types = [a, m, c];
    let (nid, ids) = seed(&h, &types);
    let before = rows(&h, &types, nid, ids);
    assert_eq!(before.live, vec![(ids[0], 0), (ids[1], 1), (ids[2], 2)]);
    assert!(before.graves.iter().all(Option::is_none));
    let expected = expected_forward(&before, ids, c, h.clock.now_ms(), false);
    let sa = LayoutShape::of(&read(&h, a)).unwrap();
    let sm = LayoutShape::of(&read(&h, m)).unwrap();
    let sc = LayoutShape::of(&read(&h, c)).unwrap();
    assert_eq!((sa.templates, sm.templates, sc.templates), (3, 1, 2));
    assert!(!sa.cloze && sm.cloze && !sc.cloze);
    let count = seen.0.lock().unwrap().len();
    h.collection.run(op(false), move |ctx| {
        meguri_engine::mark_schema_change(ctx.scope(), &[SchemaReason::NotesNotetypeChanged])?;
        primitive(ctx, nid, a, m, vec![0, 1, 2], None)?;
        assert_eq!(ctx.scope().note(nid)?.unwrap().notetype_id, m);
        assert_eq!(ctx.scope().cards_of_note(nid)?.iter().map(|c| (c.id, c.ord)).collect::<Vec<_>>(),
            vec![(ids[0], 0), (ids[1], 1), (ids[2], 2)], "M9.3 leaves all original cards untouched");
        primitive(ctx, nid, m, c, vec![], Some(ids[2]))
    }).expect("canonical standard3-to-cloze-to-standard2 contributions must COMMIT");
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    assert_eq!(rows(&h, &types, nid, ids), expected);
    let retained = last(&seen);
    let original = retained.structure_report().unwrap();
    assert_eq!(original.type_changes.keys().copied().collect::<Vec<_>>(), [(a.0, c.0)],
        "S2.5 keeps only the actual transaction-start/final pair");
    assert_eq!(original.type_changes[&(a.0, c.0)].template_map.len(), 2,
        "a final standard destination must not retain the intermediate prefix's third entry");
    let mut wanted = StructureReport::default();
    wanted.reasons.insert(R::NotesNotetypeChanged);
    wanted.type_changes.insert((a.0, c.0), pair(sa, sc, &[0, 1]));
    assert_eq!(original, &wanted);
    let mut inverse = StructureReport::default();
    inverse.reasons.insert(R::NotesNotetypeChanged);
    inverse.type_changes.insert((c.0, a.0), pair(sc, sa, &[0, 1, -1]));
    for cycle in 0..2 {
        h.collection.undo(UndoScope::Global).expect("Undo restores original card 2 from the journal");
        assert_eq!(rows(&h, &types, nid, ids), before, "Undo cycle {cycle}");
        assert_eq!(last(&seen).structure_report(), Some(&inverse));
        h.collection.redo(UndoScope::Global).expect("Redo admits the exact two-destination report");
        assert_eq!(rows(&h, &types, nid, ids), expected, "Redo cycle {cycle}");
        assert_eq!(last(&seen).structure_report(), Some(original));
        assert_eq!(retained.structure_report(), Some(&wanted));
    }
    assert_eq!(seen.0.lock().unwrap().len(), count + 5);
}

#[test]
fn noncanonical_user_primitive_cloze_prefix_is_refused_at_admission() {
    let (h, seen) = rig();
    let a = add(&h, "Primitive cloze A", true);
    let b = standard(&h, "Primitive standard B", 2);
    let types = [a, b];
    let (nid, ids) = seed(&h, &types);
    let before = rows(&h, &types, nid, ids);
    let retained = last(&seen);
    let original = retained.structure_report().cloned();
    let count = seen.0.lock().unwrap().len();
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let result = h.collection.run(op(false), move |ctx| -> Result<(), EngineError> {
        let mut report = StructureReport::default();
        report.reasons.insert(R::NotesNotetypeChanged);
        report.type_changes.insert((a.0, b.0), pair(
            LayoutShape::of(&ctx.scope().notetype(a)?.unwrap())?,
            LayoutShape::of(&ctx.scope().notetype(b)?.unwrap())?, &[1, 0]));
        // Must fail here, before any row writer or completion/commit guard.
        ctx.scope().start_structure_report(report)?;
        panic!("noncanonical primitive unexpectedly passed authored admission");
    });
    assert!(matches!(&result, Err(EngineError::Storage(StorageError::Mode { detail }))
        if detail == "invalid structure report: cloze type change must preserve card ordinals"),
        "primitive admission must retain its own precise typed refusal: {result:?}");
    assert_eq!(rows(&h, &types, nid, ids), before);
    assert_eq!(seen.0.lock().unwrap().len(), count);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(retained.structure_report(), original.as_ref());
}

#[test]
fn retained_entrypoint_cannot_commit_a_fresh_op_for_any_caller_source() {
    for source in OpSource::ALL {
        let (h, seen) = rig();
        let a = add(&h, "Retained guard cloze", true);
        let b = standard(&h, "Retained guard standard", 2);
        let types = [a, b];
        let (nid, ids) = seed(&h, &types);
        let before = rows(&h, &types, nid, ids);
        let retained = last(&seen);
        let original = retained.structure_report().cloned();
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let count = seen.0.lock().unwrap().len();
        let result = h.collection.run(
            OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, source)
                .schema_changes([SchemaReason::NotesNotetypeChanged, SchemaReason::TemplatesChanged]),
            move |ctx| {
                meguri_engine::mark_schema_change(ctx.scope(),
                    &[SchemaReason::NotesNotetypeChanged, SchemaReason::TemplatesChanged])?;
                let mut nt = ctx.scope().notetype(b)?.unwrap();
                let sb = LayoutShape::of(&nt)?;
                let mut report = StructureReport::default();
                report.reasons.extend([R::NotesNotetypeChanged, R::TemplatesChanged]);
                report.layouts.insert(b.0, LayoutReport {
                    before: sb, after: sb, field_map: vec![0, 1], template_map: vec![1, 0],
                });
                report.type_changes.insert((a.0, b.0), pair(
                    LayoutShape::of(&ctx.scope().notetype(a)?.unwrap())?, sb, &[1, 0]));
                // Valid joined domains and real writes must reach the final
                // capability fence. A caller-selected source is insufficient.
                let token = ctx.scope().start_retained_structure_report(report)?;
                let mut note = ctx.scope().note(nid)?.unwrap();
                note.notetype_id = b;
                ctx.scope().put(&note)?;
                ctx.scope().remove_card(ids[2])?;
                ctx.scope().add_grave(ids[2].0, GraveKind::Card, Usn(-1))?;
                nt.templates.swap(0, 1);
                for (ord, template) in nt.templates.iter_mut().enumerate() { template.ord = ord as u32; }
                ctx.scope().put_notetype(&nt)?;
                for (ord, id) in ids[..2].iter().enumerate() {
                    let mut card = ctx.scope().card(*id)?.unwrap();
                    card.ord = 1 - ord as u16;
                    ctx.scope().put(&card)?;
                }
                ctx.scope().complete_structure_report(token)?;
                Ok(())
            },
        );
        assert!(matches!(&result, Err(EngineError::Storage(StorageError::Mode { detail }))
            if detail == "retained structure report requires the completed queued history restore"),
            "fresh retained admission must fail at the executor capability fence for {source:?}: {result:?}");
        assert_eq!(rows(&h, &types, nid, ids), before);
        assert_eq!(seen.0.lock().unwrap().len(), count);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        assert_eq!(retained.structure_report(), original.as_ref());
    }
}

fn same_id_cloze_to_standard(prefix: Vec<i32>, reverse: Vec<i32>, move_last: bool) {
    let (h, seen) = rig();
    let id = add(&h, "Same ID cloze layout", true);
    let types = [id];
    let (nid, ids) = seed(&h, &types);
    let before = rows(&h, &types, nid, ids);
    assert_eq!(before.live, vec![(ids[0], 0), (ids[1], 1), (ids[2], 2)]);
    assert!(before.graves.iter().all(Option::is_none));
    let mut nt = read(&h, id);
    let sa = LayoutShape::of(&nt).unwrap();
    assert_eq!(sa, LayoutShape { fields: 2, templates: 1, cloze: true, sort_field: 0 });
    nt.config.kind = 0;
    let first = nt.templates[0].clone();
    nt.templates = (0..2).map(|ord| {
        let mut template = first.clone();
        template.ord = ord;
        template.name = format!("Layout view {ord}");
        template.config.id = Some(-201 - i64::from(ord));
        template.config.q_format = format!("Layout view {ord}: {{{{Front}}}}");
        template.config.a_format = "{{Front}}<hr>{{Back}}".into();
        template
    }).collect();
    let sb = LayoutShape::of(&nt).unwrap();
    assert_eq!(sb, LayoutShape { fields: 2, templates: 2, cloze: false, sort_field: 0 });
    assert!(prefix.len() < sb.templates as usize,
        "the finite destination must extend beyond the explicit prefix");
    let mut wanted = StructureReport::default();
    wanted.reasons.insert(R::TemplatesChanged);
    wanted.layouts.insert(id.0, LayoutReport {
        before: sa, after: sb, field_map: vec![0, 1], template_map: prefix,
    });
    let mut inverse = StructureReport::default();
    inverse.reasons.insert(R::TemplatesChanged);
    inverse.layouts.insert(id.0, LayoutReport {
        before: sb, after: sa, field_map: vec![0, 1], template_map: reverse,
    });

    let now = h.clock.now_ms();
    let dropped = if move_last { 0 } else { 2 };
    let mut expected = before.clone();
    expected.types[0].head[2] = Cell::Integer(now / 1000);
    expected.types[0].head[3] = Cell::Integer(-1);
    expected.types[0].head[4] = Cell::Blob(nt.config.to_bytes().into_owned());
    expected.types[0].templates = nt.templates.iter().map(|template| {
        let mut row = before.types[0].templates[0].clone();
        row[1] = Cell::Integer(i64::from(template.ord));
        row[2] = Cell::text(template.name.clone());
        row[5] = Cell::Blob(template.config.to_bytes().into_owned());
        row
    }).collect();
    expected.cards[dropped] = None;
    expected.graves[dropped] = Some(vec![Cell::Integer(ids[dropped].0), Cell::Integer(0), Cell::Integer(-1)]);
    expected.live = if move_last {
        let card = expected.cards[2].as_mut().unwrap();
        card[3] = Cell::Integer(0);
        card[4] = Cell::Integer(now / 1000);
        card[5] = Cell::Integer(-1);
        vec![(ids[1], 1), (ids[2], 0)]
    } else { vec![(ids[0], 0), (ids[1], 1)] };
    expected.col[2] = Cell::Integer(now);
    expected.col[3] = Cell::Integer(now);
    let count = seen.0.lock().unwrap().len();
    let authored = wanted.clone();
    h.collection.run(
        OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
            .schema_changes([SchemaReason::TemplatesChanged]),
        move |ctx| {
            meguri_engine::mark_schema_change(ctx.scope(), &[SchemaReason::TemplatesChanged])?;
            let token = ctx.scope().start_structure_report(authored)?;
            ctx.scope().put_notetype(&nt)?;
            ctx.scope().remove_card(ids[dropped])?;
            ctx.scope().add_grave(ids[dropped].0, GraveKind::Card, Usn(-1))?;
            if move_last {
                let mut card = ctx.scope().card(ids[2])?.unwrap();
                assert_eq!(card.ord, 2);
                card.ord = 0;
                ctx.scope().put(&card)?;
            }
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        },
    ).expect("admitted same-ID cloze-to-standard layout must reach executor COMMIT");
    assert_eq!(seen.0.lock().unwrap().len(), count + 1);
    assert_eq!(rows(&h, &types, nid, ids), expected);
    assert_eq!(LayoutShape::of(&read(&h, id)).unwrap(), sb);
    let retained = last(&seen);
    let original = retained.structure_report().unwrap();
    assert_eq!(original, &wanted);
    for cycle in 0..2 {
        h.collection.undo(UndoScope::Global)
            .expect("Undo must admit the endpoint-aware inverse and restore removed card rows");
        assert_eq!(rows(&h, &types, nid, ids), before, "Undo cycle {cycle}");
        assert_eq!(LayoutShape::of(&read(&h, id)).unwrap(), sa);
        assert_eq!(last(&seen).structure_report(), Some(&inverse), "Undo cycle {cycle}");
        h.collection.redo(UndoScope::Global)
            .expect("Redo must admit the original retained cloze prefix");
        assert_eq!(rows(&h, &types, nid, ids), expected, "Redo cycle {cycle}");
        assert_eq!(LayoutShape::of(&read(&h, id)).unwrap(), sb);
        assert_eq!(last(&seen).structure_report(), Some(original), "Redo cycle {cycle}");
        assert_eq!(retained.structure_report(), Some(&wanted));
    }
    assert_eq!(seen.0.lock().unwrap().len(), count + 5);
}

#[test]
fn same_id_cloze_to_standard_empty_prefix_inverts_full_finite_destination() {
    same_id_cloze_to_standard(vec![], vec![0, 1], false);
}

#[test]
fn same_id_cloze_to_standard_short_prefix_keeps_tail_origin_and_missing_origin() {
    same_id_cloze_to_standard(vec![2], vec![-1, 1, 0], true);
}
