//! Rename-only production proof: F1–F5, N3/N4/N6/N8, V1/V5,
//! template-syntax T2–T6/T12 and notes/card-generation G9.
//! Setup may seed legacy/malformed records; every tested rename uses the
//! typed production API. Full U34 edits and normal sync belong elsewhere.

mod common;

use std::sync::Arc;
use std::time::Duration;

use common::{Harness, fresh};
use meguri_engine::executor::catalog;
use meguri_engine::notetypes::{
    self, AggregateDigest, Draft, FieldIdentity, FieldRename, NewNotetype, NewTemplate, NotetypeKind, RenameFields,
    SaveOptions, rename,
};
use meguri_engine::notetypes::{TemplateFormatsUpdate, TemplateIdentity, template_formats};
use meguri_engine::{
    CancelToken, EngineError, EventKind, OpSpec, Progress, ProgressListener, ProgressPhase, ProgressSink, UndoScope,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::ids::{CardId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::proto::{BlobMessage, ProtoBlob};
use meguri_model::time::EpochSecs;
use meguri_model::usn::Usn;
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, StampMode, Table};

fn make(h: &Harness, cloze: bool, fronts: &[&str]) -> NotetypeId {
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: "Rename fixture".to_owned(),
            kind: if cloze {
                NotetypeKind::Cloze
            } else {
                NotetypeKind::Standard
            },
            fields: vec!["Front".to_owned(), "Back".to_owned()],
            templates: fronts
                .iter()
                .enumerate()
                .map(|(i, front)| NewTemplate {
                    name: format!("View {i}"),
                    front: (*front).to_owned(),
                    back: if cloze { "{{cloze:Front}}" } else { "{{Back}}" }.to_owned(),
                    ..Default::default()
                })
                .collect(),
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id
}

fn read(h: &Harness, id: NotetypeId) -> Notetype {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(id)
        .unwrap()
        .unwrap()
}

fn request(nt: &Notetype, edits: &[(usize, &str)]) -> RenameFields {
    RenameFields {
        notetype: nt.id,
        base: AggregateDigest::of(nt),
        fields: edits
            .iter()
            .map(|(at, name)| FieldRename {
                field: FieldIdentity::of(&nt.fields[*at]),
                name: (*name).to_owned(),
            })
            .collect(),
    }
}

fn run(h: &Harness, input: RenameFields) -> Result<meguri_engine::OpResult<notetypes::Saved>, EngineError> {
    notetypes::rename_fields(&h.collection, input, CancelToken::new(), ProgressSink::new())
}

// Fixture setup only, preserving caller-supplied stamps and unknown data.
fn seed(h: &Harness, nt: Notetype) {
    h.collection
        .run(
            OpSpec::named("rename_fixture", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                // Fixture author keeps each field/template's positional
                // identity while seeding explicit legacy ordinal gaps.
                // Appended rows have no origin; stored keys do not index maps.
                let old = ctx.scope().notetype(nt.id)?.unwrap();
                let before = meguri_storage::journal::LayoutShape::of(&old)?;
                let after = meguri_storage::journal::LayoutShape::of(&nt)?;
                let mut field_map = vec![-1; after.fields as usize];
                let mut template_map = if after.cloze { Vec::new() } else { vec![-1; after.templates as usize] };
                for (position, _) in old.fields.iter().zip(&nt.fields).enumerate() { field_map[position] = position as i32; }
                if !after.cloze {
                    for (position, _) in old.templates.iter().zip(&nt.templates).enumerate() { template_map[position] = position as i32; }
                }
                let mut report = meguri_storage::journal::StructureReport::default();
                report.layouts.insert(nt.id.0, meguri_storage::journal::LayoutReport { before, after, field_map, template_map });
                let token = ctx.scope().start_structure_report(report)?;
                ctx.scope().put_notetype(&nt)?;
                ctx.scope().complete_structure_report(token)?;
                Ok(())
            },
        )
        .unwrap();
}

fn rows(h: &Harness, id: NotetypeId) -> Vec<RowImage> {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let nt = reader.notetype(id).unwrap().unwrap();
    let mut rows = vec![reader.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap()];
    for field in &nt.fields {
        rows.push(
            reader
                .row_image(Table::Fields, &RowKey::Pair(id.0, i64::from(field.ord)))
                .unwrap()
                .unwrap(),
        );
    }
    for template in &nt.templates {
        rows.push(
            reader
                .row_image(Table::Templates, &RowKey::Pair(id.0, i64::from(template.ord)))
                .unwrap()
                .unwrap(),
        );
    }
    rows
}

fn col(h: &Harness) -> (i64, i64) {
    let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    (col.modified.0, col.schema_modified.0)
}

fn names(nt: &Notetype) -> Vec<&str> {
    nt.fields.iter().map(|f| f.name.as_str()).collect()
}

fn commit_spec() -> OpSpec {
    OpSpec::new(catalog::NOTETYPE_STRUCTURE_CHANGE, OpSource::UserUndoable)
        .undo_step(meguri_engine::undo::names::UPDATE_NOTE_TYPE, Default::default())
        .declare_long_write(Duration::from_secs(1))
}

#[test]
fn single_case_only_simultaneous_swap_and_complete_list_normalization() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}{{text:Back}}"]);
    run(&h, request(&read(&h, id), &[(0, "Prompt")])).unwrap();
    let nt = read(&h, id);
    assert_eq!(names(&nt), ["Prompt", "Back"]);
    assert_eq!(nt.templates[0].config.q_format, "{{Prompt}}{{text:Back}}");
    run(&h, request(&nt, &[(0, "PROMPT")])).unwrap();
    let nt = read(&h, id);
    assert_eq!(nt.templates[0].config.q_format, "{{PROMPT}}{{text:Back}}");
    run(&h, request(&nt, &[(0, "Back"), (1, "PROMPT")])).unwrap();
    let nt = read(&h, id);
    assert_eq!(names(&nt), ["Back", "PROMPT"]);
    assert_eq!(nt.templates[0].config.q_format, "{{Back}}{{text:PROMPT}}");
    // N4 uniquing happens once, in field order, over the final list.
    run(&h, request(&nt, &[(0, "  # /^ e\u{301}:{}\" "), (1, "É")])).unwrap();
    let nt = read(&h, id);
    assert_eq!(names(&nt), ["é", "É+"]);
    assert_eq!(nt.templates[0].config.q_format, "{{é}}{{text:É+}}");
    assert_eq!(nt.config.reqs[0].kind, 1);
    assert_eq!(nt.config.reqs[0].field_ords, [0, 1]);

    // A collision with an unchanged field is resolved in that same final
    // list, including a suffix for the later, untargeted field.
    run(&h, request(&nt, &[(0, "É+")])).unwrap();
    let nt = read(&h, id);
    assert_eq!(names(&nt), ["É+", "É++"]);
    assert_eq!(nt.templates[0].config.q_format, "{{É+}}{{text:É++}}");
}

#[test]
fn normalized_noop_precedes_fallback_and_preserves_rows_stamps_undo_and_publication() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}"]);
    let mut nt = read(&h, id);
    nt.templates[0].config.q_format = "literal with no replacements".to_owned();
    nt.templates[0].config.q_format_browser = "browser literal".to_owned();
    nt.mtime = EpochSecs(123);
    nt.usn = Usn(7);
    seed(&h, nt);
    let nt = read(&h, id);
    let before = rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let events = h.engine.stream().subscribe();
    h.clock.step_ms(60_000);
    let result = run(&h, request(&nt, &[(0, "  # /^ Fr:o{n}t\" ")])).unwrap();
    assert!(!result.value.written);
    assert!(result.changes.is_empty());
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert!(common::drain(&events, Duration::from_millis(10)).is_empty());
    // Invalid normalized input also refuses without repairing the texts.
    assert!(matches!(
        run(&h, request(&nt, &[(0, "::{}\"")])),
        Err(EngineError::NotetypeRefused(_))
    ));
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
}

#[test]
fn all_four_texts_use_original_exact_keys_spans_filters_sections_and_delimiters() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}"]);
    let mut nt = read(&h, id);
    let t = &mut nt.templates[0].config;
    t.q_format = "{{ Front }}|{{text:hint:Back}}|{{# Front }}{{^Back}}s{{/Back}}{{/ Front }}|{{front}}|{{text: Front}}<!--{{Front}}-->".to_owned();
    t.a_format = "{{Back}} / {{{Front}}} / {{::Front}}".to_owned();
    t.q_format_browser = " \n{{=<% %>=}}{{=<% %>=}}<%text:Front%> {{Back}}<!--<%Front%>-->".to_owned();
    t.a_format_browser = "{{#Back}}x{{/Back}} {{Front}}".to_owned();
    seed(&h, nt);
    run(&h, request(&read(&h, id), &[(0, "Back"), (1, "Front")])).unwrap();
    let nt = read(&h, id);
    let t = &nt.templates[0].config;
    assert_eq!(
        t.q_format,
        "{{Back}}|{{text:hint:Front}}|{{#Back}}{{^Front}}s{{/Front}}{{/Back}}|{{front}}|{{text: Front}}<!--{{Front}}-->"
    );
    // S4.1: a rewritten tag is written in canonical form over its whole
    // span, so its spaces and the extra opening brace go; `}` after the
    // tag's closer is text.
    assert_eq!(t.a_format, "{{Front}} / {{Back}}} / {{::Back}}");
    assert_eq!(
        t.q_format_browser,
        " \n{{=<% %>=}}{{=<% %>=}}<%text:Back%> {{Back}}<!--<%Front%>-->"
    );
    assert_eq!(t.a_format_browser, "{{#Front}}x{{/Front}} {{Back}}");
    // Req values use final fronts AND final names, rather than the old
    // front evaluated against the swapped field list.
    assert_eq!(nt.config.reqs[0].kind, 1);
    assert_eq!(nt.config.reqs[0].field_ords, [0, 1]);
}

#[test]
fn malformed_texts_are_independent_and_standard_fallback_excludes_sections_and_backs() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}", "{{text:Front}}"]);
    let mut nt = read(&h, id);
    let t = &mut nt.templates[0].config;
    t.q_format = "{{#Front}}literal{{/Front}}".to_owned();
    t.a_format = "{{#Front}}broken".to_owned();
    t.q_format_browser = "browser".to_owned();
    t.a_format_browser = "<!--{{Front}}-->".to_owned();
    let t = &mut nt.templates[1].config;
    t.q_format = "{{#Front}}bad{{/Back}}".to_owned();
    t.a_format = "{{text:Front}}".to_owned();
    let t = &mut nt.templates[2].config;
    t.q_format = "{{=<% %>=}}<%#Back%>x<%/Back%>".to_owned();
    t.a_format = "answer".to_owned();
    seed(&h, nt);
    run(&h, request(&read(&h, id), &[(0, "Primary")])).unwrap();
    let nt = read(&h, id);
    let t = &nt.templates[0].config;
    assert_eq!(t.q_format, "{{#Primary}}literal{{/Primary}}{{Primary}}");
    assert_eq!(t.a_format, "{{#Front}}broken");
    assert_eq!(t.q_format_browser, "browser{{Primary}}");
    assert_eq!(t.a_format_browser, "<!--{{Front}}-->");
    let t = &nt.templates[1].config;
    assert_eq!(t.q_format, "{{#Front}}bad{{/Back}}");
    assert_eq!(t.a_format, "{{text:Primary}}");
    assert_eq!(t.q_format_browser, "");
    assert_eq!(t.a_format_browser, "");
    assert_eq!(
        nt.templates[2].config.q_format,
        "{{=<% %>=}}<%#Back%>x<%/Back%><%Primary%>"
    );
    assert_eq!(nt.templates[2].config.a_format, "answer");
    assert_eq!(nt.config.reqs[0].kind, 1);
    assert_eq!(nt.config.reqs[0].field_ords, [0]);
    assert_eq!(nt.config.reqs[1].kind, 0);
    assert!(nt.config.reqs[1].field_ords.is_empty());
}

#[test]
fn cloze_fallback_covers_each_processed_side_keeps_alternate_mode_and_skips_empty_browser() {
    let h = fresh();
    let id = make(&h, true, &["{{cloze:Front}}", "{{text:cloze:Front}}"]);
    let mut nt = read(&h, id);
    let t = &mut nt.templates[0].config;
    t.q_format = "{{=<% %>=}}<%Front%>".to_owned();
    t.a_format = "{{#Front}}x{{/Front}}".to_owned();
    t.a_format_browser = "{{=<% %>=}}browser".to_owned();
    let t = &mut nt.templates[1].config;
    t.q_format = "{{text:cloze:Front}}{{cloze:text:Back}}".to_owned();
    t.a_format = "{{#Front}}bad".to_owned();
    t.q_format_browser = "<!--{{cloze:Front}}-->".to_owned();
    seed(&h, nt);
    run(&h, request(&read(&h, id), &[(0, "Body")])).unwrap();
    let nt = read(&h, id);
    let t = &nt.templates[0].config;
    assert_eq!(t.q_format, "{{=<% %>=}}<%Body%><%cloze:Body%>");
    assert_eq!(t.a_format, "{{#Body}}x{{/Body}}{{cloze:Body}}");
    assert_eq!(t.q_format_browser, "");
    assert_eq!(t.a_format_browser, "{{=<% %>=}}browser<%cloze:Body%>");
    let t = &nt.templates[1].config;
    assert_eq!(t.q_format, "{{text:cloze:Body}}{{cloze:text:Back}}");
    assert_eq!(t.a_format, "{{#Front}}bad");
    assert_eq!(t.q_format_browser, "<!--{{cloze:Front}}-->{{cloze:Body}}");
    assert_eq!(t.a_format_browser, "");
}

#[test]
fn identities_reject_wrong_duplicate_and_out_of_range_but_allow_legacy_ids() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}{{Back}}"]);
    for optional_id in [None, Some(77)] {
        let mut nt = read(&h, id);
        for field in &mut nt.fields {
            field.config.id = optional_id;
        }
        seed(&h, nt);
        let nt = read(&h, id);
        let before = rows(&h, id);
        let before_col = col(&h);
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        let mut bad = request(&nt, &[(0, "X")]);
        bad.fields[0].field.id = Some(78);
        let mut out_of_range = request(&nt, &[(0, "X")]);
        out_of_range.fields[0].field.ord = u32::MAX;
        let mut empty = request(&nt, &[(0, "X")]);
        empty.fields.clear();
        for bad in [bad, out_of_range, request(&nt, &[(0, "X"), (0, "Y")]), empty] {
            assert!(matches!(run(&h, bad), Err(EngineError::InvalidArgument { .. })));
            assert_eq!(rows(&h, id), before);
            assert_eq!(col(&h), before_col);
            assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        }
        // Distinct captured ordinals disambiguate the same optional ID.
        run(&h, request(&nt, &[(0, &nt.fields[1].name), (1, &nt.fields[0].name)])).unwrap();
        let nt = read(&h, id);
        assert_eq!(
            nt.fields.iter().map(|f| f.config.id).collect::<Vec<_>>(),
            [optional_id; 2]
        );
    }
    // With distinct IDs, an ordinal captured for another field refuses.
    let mut nt = read(&h, id);
    nt.fields[0].config.id = Some(11);
    nt.fields[1].config.id = Some(12);
    seed(&h, nt);
    let nt = read(&h, id);
    let mut bad = request(&nt, &[(0, "X")]);
    bad.fields[0].field.ord = nt.fields[1].ord;
    assert!(matches!(run(&h, bad), Err(EngineError::InvalidArgument { .. })));
}

#[test]
fn stale_base_is_refused_at_plan_and_writer_while_parent_usn_marking_is_accepted() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}"]);
    let captured = request(&read(&h, id), &[(0, "Prompt")]);
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = rename::plan(&*reader, captured.clone(), &CancelToken::new(), &ProgressSink::new()).unwrap();
    drop(reader);
    let mut draft = Draft::of(read(&h, id));
    draft.fields[0].config.sticky = true;
    notetypes::save(&h.collection, draft, SaveOptions { skip_checks: true }).unwrap();
    let before = rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    assert!(matches!(
        run(&h, captured),
        Err(EngineError::Stale {
            table: Table::Notetypes,
            ..
        })
    ));
    let error = h
        .collection
        .run(commit_spec(), move |ctx| rename::commit_in(ctx.scope(), &planned));
    assert!(matches!(
        error,
        Err(EngineError::Stale {
            table: Table::Notetypes,
            ..
        })
    ));
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);

    let nt = read(&h, id);
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = rename::plan(
        &*reader,
        request(&nt, &[(0, "Prompt")]),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    drop(reader);
    let mut marked = nt.clone();
    marked.usn = Usn(24);
    seed(&h, marked);
    assert_eq!(AggregateDigest::of(&read(&h, id)), AggregateDigest::of(&nt));
    h.collection
        .run(commit_spec(), move |ctx| rename::commit_in(ctx.scope(), &planned))
        .unwrap();
    assert_eq!(names(&read(&h, id)), ["Prompt", "Back"]);
    assert!(read(&h, id).fields[0].config.sticky);
}

#[test]
fn plain_save_still_refuses_rename_and_other_structure_edits() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}"]);
    let nt = read(&h, id);
    let before = rows(&h, id);
    for edit in 0..7 {
        let mut draft = Draft::of(nt.clone());
        match edit {
            0 => draft.fields[0].name = "Prompt".to_owned(),
            1 => draft.fields.swap(0, 1),
            2 => {
                draft.fields.pop();
            }
            3 => draft.config.sort_field_idx = 1,
            4 => draft.config.kind = 1,
            5 => draft.templates[0].config.q_format = "{{Back}}".to_owned(),
            _ => {
                let field = draft.fields[0].clone();
                draft.fields.push(field);
            }
        }
        assert!(matches!(
            notetypes::save(&h.collection, draft, SaveOptions { skip_checks: true }),
            Err(EngineError::InvalidArgument { .. })
        ));
        assert_eq!(rows(&h, id), before);
    }
}

#[test]
fn gapped_legacy_ordinals_preserve_children_metadata_and_atomic_undo_redo_rollback() {
    // F4 preserves stored row identities even when legacy ordinals have gaps.
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let mut nt = read(&h, id);
    nt.fields[1].ord = 4;
    nt.fields[0].config.id = None;
    nt.fields[1].config.id = Some(-17);
    nt.templates[1].ord = 7;
    nt.templates[0].config.a_format = "{{Front}}".to_owned();
    nt.templates[0].config.id = None;
    nt.templates[1].config.id = Some(-29);
    nt.mtime = EpochSecs(42);
    nt.usn = Usn(8);
    nt.config = with_unknown(&with_other(&nt.config, b"kept parent"));
    for field in &mut nt.fields {
        field.config.prevent_deletion = true;
        field.config = with_unknown(&with_other(&field.config, b"kept field"));
    }
    for template in &mut nt.templates {
        template.mtime = EpochSecs(321);
        template.usn = Usn(19);
        template.config = with_unknown(&with_other(&template.config, b"kept template"));
    }
    seed(&h, nt);
    let before = rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let nt = read(&h, id);
    let events = h.engine.stream().subscribe();
    h.clock.step_ms(60_000);
    let noop = run(&h, request(&nt, &[(1, "Back")])).unwrap();
    assert!(!noop.value.written && noop.changes.is_empty());
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert!(common::drain(&events, Duration::from_millis(10)).is_empty());

    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = rename::plan(
        &*reader,
        request(&nt, &[(0, "Prompt")]),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    drop(reader);
    let failed: Result<meguri_engine::OpResult<()>, EngineError> = h.collection.run(commit_spec(), move |ctx| {
        assert!(rename::commit_in(ctx.scope(), &planned)?.written);
        assert_eq!(ctx.scope().notetype(id)?.unwrap().fields[0].name, "Prompt");
        Err(EngineError::InvalidArgument {
            message: "rollback gapped rename".to_owned(),
        })
    });
    assert!(matches!(failed, Err(EngineError::InvalidArgument { .. })));
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert!(
        common::drain(&events, Duration::from_millis(10))
            .iter()
            .all(|event| !matches!(event.kind, EventKind::Changes(_)))
    );

    // First retain the unchanged high keys, then edit those very same keys.
    for (index, old_name, new_name) in [(0, "Front", "Prompt"), (1, "Back", "Answer")] {
        let old = read(&h, id);
        let before = rows(&h, id);
        let before_col = col(&h);
        let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
        h.clock.step_ms(60_000);
        let result = run(&h, request(&old, &[(index, new_name)])).unwrap();
        assert!(result.value.written);
        let new = read(&h, id);
        assert_eq!(new.fields.iter().map(|f| f.ord).collect::<Vec<_>>(), [0, 4]);
        assert_eq!(new.templates.iter().map(|t| t.ord).collect::<Vec<_>>(), [0, 7]);
        assert_eq!(new.fields[index].name, new_name);
        assert_eq!(
            (
                new.fields[1 - index].ord,
                &new.fields[1 - index].name,
                new.fields[1 - index].config.id,
                new.fields[1 - index].config.to_bytes(),
            ),
            (
                old.fields[1 - index].ord,
                &old.fields[1 - index].name,
                old.fields[1 - index].config.id,
                old.fields[1 - index].config.to_bytes(),
            )
        );
        for (before, after) in old.fields.iter().zip(&new.fields) {
            assert_eq!(before.config.to_bytes(), after.config.to_bytes());
        }
        for (before, after) in old.templates.iter().zip(&new.templates) {
            let mut expected = before.clone();
            expected.config.q_format = expected.config.q_format.replace(old_name, new_name);
            expected.config.a_format = expected.config.a_format.replace(old_name, new_name);
            expected.config.q_format_browser = expected.config.q_format_browser.replace(old_name, new_name);
            expected.config.a_format_browser = expected.config.a_format_browser.replace(old_name, new_name);
            assert_eq!(
                (
                    after.ord,
                    &after.name,
                    after.mtime,
                    after.usn,
                    after.config.id,
                    after.config.to_bytes()
                ),
                (
                    expected.ord,
                    &expected.name,
                    expected.mtime,
                    expected.usn,
                    expected.config.id,
                    expected.config.to_bytes(),
                )
            );
            assert_eq!(after.config.to_bytes(), expected.config.to_bytes());
        }
        assert_eq!(new.config.other(), old.config.other());
        assert_eq!(new.config.unknown_fields(), old.config.unknown_fields());
        assert_eq!((new.mtime.0, new.usn), (h.clock.now_ms() / 1000, Usn::PENDING));
        assert_eq!(col(&h), (h.clock.now_ms(), before_col.1));
        let after = rows(&h, id);
        let after_col = col(&h);
        assert_eq!(
            h.collection.peek_undo(UndoScope::Global).unwrap().name(),
            Some(meguri_engine::undo::names::UPDATE_NOTE_TYPE)
        );
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(rows(&h, id), before);
        assert_eq!(col(&h), before_col);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        h.collection.redo(UndoScope::Global).unwrap();
        assert_eq!(rows(&h, id), after);
        assert_eq!(col(&h), after_col);
    }
}

fn with_unknown<M: BlobMessage>(blob: &ProtoBlob<M>) -> ProtoBlob<M> {
    let mut bytes = blob.to_bytes().into_owned();
    bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
    ProtoBlob::decode(&bytes).unwrap()
}

fn with_other<M: BlobMessage>(blob: &ProtoBlob<M>, other: &[u8]) -> ProtoBlob<M> {
    assert!(other.len() < 128);
    let mut bytes = blob.to_bytes().into_owned();
    // P6's opaque field 255, through the public protobuf decoder.
    bytes.extend_from_slice(&[0xfa, 0x0f, other.len() as u8]);
    bytes.extend_from_slice(other);
    ProtoBlob::decode(&bytes).unwrap()
}

#[test]
fn unknown_members_ids_options_metadata_notes_cards_and_stamps_survive_one_step_undo_redo() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let added = common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    let mut nt = read(&h, id);
    nt.usn = Usn(8);
    nt.mtime = EpochSecs(42);
    nt.config.latex_pre = "kept header".to_owned();
    nt.config = with_other(&nt.config, b"{\"extension\":17}");
    nt.config = with_unknown(&nt.config);
    // Retained requirement records also carry unknown data (P4). Supply
    // the two known requirements with an extra varint on each record.
    nt.config.reqs.clear();
    let mut bytes = nt.config.to_bytes().into_owned();
    bytes.extend_from_slice(&[
        0x42, 9, 0x08, 0, 0x10, 1, 0x18, 0, 0xa0, 0x01, 7, 0x42, 9, 0x08, 1, 0x10, 1, 0x18, 1, 0xa0, 0x01, 7,
    ]);
    nt.config = ProtoBlob::decode(&bytes).unwrap();
    for field in &mut nt.fields {
        field.config.sticky = true;
        field.config.prevent_deletion = true;
        field.config.tag = Some(9);
        field.config = with_other(&field.config, b"kept field");
        field.config = with_unknown(&field.config);
    }
    for template in &mut nt.templates {
        template.mtime = EpochSecs(321);
        template.usn = Usn(19);
        template.config = with_other(&template.config, b"kept template");
        template.config = with_unknown(&template.config);
    }
    seed(&h, nt);
    let before_nt = read(&h, id);
    let before = rows(&h, id);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let note = reader
        .row_image(Table::Notes, &RowKey::Id(added.id.0))
        .unwrap()
        .unwrap();
    let cards: Vec<_> = added
        .cards
        .iter()
        .map(|card| reader.row_image(Table::Cards, &RowKey::Id(card.0)).unwrap().unwrap())
        .collect();
    drop(reader);
    let schema = col(&h).1;
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    h.clock.step_ms(60_000);
    let result = run(&h, request(&before_nt, &[(0, "Prompt")])).unwrap();
    assert!(result.value.written);
    assert_eq!(result.changes.notetypes.len(), 1);
    assert_eq!(result.changes.notetypes[0].id, id);
    assert!(result.changes.notetypes[0].schema_changed);
    assert!(result.changes.notes.is_empty());
    assert!(result.changes.cards.is_empty());
    let after_nt = read(&h, id);
    assert_eq!(after_nt.mtime.0, h.clock.now_ms() / 1000);
    assert_eq!(after_nt.usn, Usn(-1));
    assert_eq!(col(&h), (h.clock.now_ms(), schema));
    assert_eq!(after_nt.config.other(), before_nt.config.other());
    assert_eq!(after_nt.config.latex_pre, before_nt.config.latex_pre);
    assert!(!after_nt.config.unknown_fields().is_empty());
    assert!(after_nt.config.reqs.iter().all(|req| !req.unknown_fields().is_empty()));
    for (old, new) in before_nt.fields.iter().zip(&after_nt.fields) {
        assert_eq!((old.ord, old.config.id), (new.ord, new.config.id));
        assert_eq!(old.config.to_bytes(), new.config.to_bytes());
    }
    for (old, new) in before_nt.templates.iter().zip(&after_nt.templates) {
        assert_eq!(
            (old.ord, old.config.id, old.mtime, old.usn),
            (new.ord, new.config.id, new.mtime, new.usn)
        );
        let mut expected = old.config.clone();
        expected.q_format = expected.q_format.replace("Front", "Prompt");
        expected.a_format = expected.a_format.replace("Front", "Prompt");
        assert_eq!(expected.to_bytes(), new.config.to_bytes());
    }
    assert_eq!(after_nt.config.reqs[0].kind, 1);
    assert_eq!(after_nt.config.reqs[0].field_ords, [0]);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        reader
            .row_image(Table::Notes, &RowKey::Id(added.id.0))
            .unwrap()
            .unwrap(),
        note
    );
    for (id, before) in added.cards.iter().zip(&cards) {
        assert_eq!(
            &reader.row_image(Table::Cards, &RowKey::Id(id.0)).unwrap().unwrap(),
            before
        );
    }
    drop(reader);
    let after = rows(&h, id);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some(meguri_engine::undo::names::UPDATE_NOTE_TYPE)
    );
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), after);
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
}

// Deliberately incomplete populations for G9: setup removes cards without
// graves; production rename must only add missing cards and leave notes.
fn remove_fixture_card(h: &Harness, card: CardId) {
    common::tagging::raw(
        &h.collection,
        "DELETE FROM cards WHERE id=?",
        &[meguri_storage::raw::SqlValue::Integer(card.0)],
    );
}

#[test]
fn production_rename_generates_missing_cards_in_same_journal_and_undo_without_graves() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let kept = common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    let cardless = common::notes::add(&h, common::notes::input(id, &["other", "meaning"]));
    remove_fixture_card(&h, kept.cards[1]);
    for card in cardless.cards {
        remove_fixture_card(&h, card);
    }
    let sibling = kept.cards[0];
    h.collection
        .run(common::tagging::setup_spec(), move |ctx| {
            let mut card = ctx.scope().card(sibling)?.unwrap();
            card.due = 42;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    let before = rows(&h, id);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let note = reader.row_image(Table::Notes, &RowKey::Id(kept.id.0)).unwrap().unwrap();
    let sibling_row = reader.row_image(Table::Cards, &RowKey::Id(sibling.0)).unwrap().unwrap();
    let next_pos = reader.config_value("nextPos").unwrap();
    drop(reader);
    let result = run(&h, request(&read(&h, id), &[(0, "Prompt"), (1, "Answer")])).unwrap();
    assert_eq!(result.changes.cards.len(), 1);
    let generated = result.changes.cards[0].id;
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let generated_card = reader.card(generated).unwrap().unwrap();
    assert_eq!(
        (generated_card.note_id, generated_card.ord, generated_card.due),
        (kept.id, 1, 42)
    );
    assert_eq!(generated_card.deck_id, reader.card(sibling).unwrap().unwrap().deck_id);
    assert_eq!(reader.config_value("nextPos").unwrap(), next_pos);
    assert_eq!(
        reader.row_image(Table::Notes, &RowKey::Id(kept.id.0)).unwrap().unwrap(),
        note
    );
    assert_eq!(
        reader.row_image(Table::Cards, &RowKey::Id(sibling.0)).unwrap().unwrap(),
        sibling_row
    );
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
    let generated_row = reader
        .row_image(Table::Cards, &RowKey::Id(generated.0))
        .unwrap()
        .unwrap();
    drop(reader);
    let after = rows(&h, id);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), before);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.card(generated).unwrap().is_none());
    assert_eq!(reader.cards_of_note(kept.id).unwrap().len(), 1);
    drop(reader);
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), after);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(
        reader
            .row_image(Table::Cards, &RowKey::Id(generated.0))
            .unwrap()
            .unwrap(),
        generated_row
    );
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
}

#[test]
fn commit_rederives_live_membership_fields_siblings_and_standard_card_count() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let moved = common::notes::add(&h, common::notes::input(id, &["a", "back"]));
    let changed = common::notes::add(&h, common::notes::input(id, &["b", "back"]));
    let cardless = common::notes::add(&h, common::notes::input(id, &["c", "back"]));
    let complete = common::notes::add(&h, common::notes::input(id, &["d", "back"]));
    let orphaned = common::notes::add(&h, common::notes::input(id, &["e", "back"]));
    for card in [moved.cards[1], changed.cards[1], complete.cards[1], orphaned.cards[1]] {
        remove_fixture_card(&h, card);
    }
    for card in cardless.cards {
        remove_fixture_card(&h, card);
    }
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = rename::plan(
        &*reader,
        request(&read(&h, id), &[(0, "Prompt")]),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    drop(reader);
    let late = common::notes::add(&h, common::notes::input(id, &["late", "back"]));
    remove_fixture_card(&h, late.cards[1]);
    let moved_card = moved.cards[0];
    let changed_note = changed.id;
    let complete_note = complete.id;
    let orphaned_card = orphaned.cards[0];
    h.collection
        .run(common::tagging::setup_spec(), move |ctx| {
            let mut card = ctx.scope().card(moved_card)?.unwrap();
            card.due = 88;
            ctx.scope().put(&card)?;
            let mut note = ctx.scope().note(changed_note)?.unwrap();
            note.fields = "b\u{1f}".to_owned();
            ctx.scope().put(&note)?;
            // G9's count-based completion rule includes orphan ordinals.
            ctx.scope().add_card(&common::card(complete_note, 5))?;
            let mut card = ctx.scope().card(orphaned_card)?.unwrap();
            card.ord = 5;
            ctx.scope().put(&card)?;
            Ok(())
        })
        .unwrap();
    let result = h
        .collection
        .run(commit_spec(), move |ctx| rename::commit_in(ctx.scope(), &planned))
        .unwrap();
    assert_eq!(result.changes.cards.len(), 4); // moved, late, orphaned's 0 and 1
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let cards = reader.cards_of_note(moved.id).unwrap();
    let mut ordinal_dues = cards.iter().map(|c| (c.ord, c.due)).collect::<Vec<_>>();
    ordinal_dues.sort_unstable();
    assert_eq!(ordinal_dues, [(0, 88), (1, 88)]);
    assert_eq!(reader.cards_of_note(changed.id).unwrap().len(), 1);
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
    let mut ordinals = reader
        .cards_of_note(complete.id)
        .unwrap()
        .iter()
        .map(|c| c.ord)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [0, 5]);
    assert_eq!(reader.cards_of_note(late.id).unwrap().len(), 2);
    let mut ordinals = reader
        .cards_of_note(orphaned.id)
        .unwrap()
        .iter()
        .map(|c| c.ord)
        .collect::<Vec<_>>();
    ordinals.sort_unstable();
    assert_eq!(ordinals, [0, 1, 5]);
}

#[test]
fn error_after_aggregate_and_generated_card_writes_rolls_back_the_entire_rename() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let added = common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    remove_fixture_card(&h, added.cards[1]);
    let before = rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let events = h.engine.stream().subscribe();
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = rename::plan(
        &*reader,
        request(&read(&h, id), &[(0, "Prompt")]),
        &CancelToken::new(),
        &ProgressSink::new(),
    )
    .unwrap();
    drop(reader);
    let error: Result<meguri_engine::OpResult<()>, EngineError> = h.collection.run(commit_spec(), move |ctx| {
        assert!(rename::commit_in(ctx.scope(), &planned)?.written);
        assert_eq!(ctx.scope().cards_of_note(added.id)?.len(), 2);
        Err(EngineError::InvalidArgument {
            message: "failure after composed rename".to_owned(),
        })
    });
    assert!(matches!(error, Err(EngineError::InvalidArgument { .. })));
    assert_eq!(rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .cards_of_note(added.id)
            .unwrap()
            .len(),
        1
    );
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
    assert!(
        common::drain(&events, Duration::from_millis(10))
            .iter()
            .all(|event| !matches!(event.kind, EventKind::Changes(_)))
    );
}

struct CancelOnPhase {
    token: Arc<CancelToken>,
    phase: ProgressPhase,
}

impl ProgressListener for CancelOnPhase {
    fn on_progress(&self, progress: &Progress) {
        if progress.phase == self.phase {
            self.token.cancel();
        }
    }
}

#[test]
fn cancellation_before_or_during_planning_writes_nothing_and_commit_finishes_past_cancel_point() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}"]);
    common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    let before = rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    for pre_cancel in [true, false] {
        let token = CancelToken::new();
        let progress = ProgressSink::new();
        if pre_cancel {
            token.cancel();
        } else {
            progress.set_listener(Arc::new(CancelOnPhase {
                token: token.clone(),
                phase: ProgressPhase::Planning,
            }));
        }
        assert!(matches!(
            notetypes::rename_fields(&h.collection, request(&read(&h, id), &[(0, "Prompt")]), token, progress),
            Err(EngineError::Cancelled)
        ));
        assert_eq!(rows(&h, id), before);
        assert_eq!(col(&h), before_col);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    }
    let token = CancelToken::new();
    let progress = ProgressSink::new();
    progress.set_listener(Arc::new(CancelOnPhase {
        token: token.clone(),
        phase: ProgressPhase::Committing,
    }));
    let result =
        notetypes::rename_fields(&h.collection, request(&read(&h, id), &[(0, "Prompt")]), token, progress).unwrap();
    assert!(result.past_cancel_point);
    assert!(result.value.written);
    assert_eq!(names(&read(&h, id)), ["Prompt", "Back"]);
}

// The review-format slice shares these existing aggregate, legacy-metadata,
// row-image and card-generation fixtures with the field-rename prerequisite.
fn formats_request(nt: &Notetype, at: usize, front: &str, back: &str, css: Option<&str>) -> TemplateFormatsUpdate {
    TemplateFormatsUpdate {
        notetype: nt.id,
        base: AggregateDigest::of(nt),
        template: TemplateIdentity::of(&nt.templates[at]),
        front: front.to_owned(),
        back: back.to_owned(),
        css: css.map(str::to_owned),
    }
}

fn formats_run(
    h: &Harness,
    request: TemplateFormatsUpdate,
) -> Result<meguri_engine::OpResult<notetypes::TemplateFormatsSaved>, EngineError> {
    notetypes::update_template_formats(&h.collection, request, CancelToken::new(), ProgressSink::new())
}

// Before/after evidence includes full aggregate, note and card rows and the
// persisted allocation counter, in addition to collection stamps and history.
fn formats_rows(h: &Harness, id: NotetypeId) -> Vec<RowImage> {
    let mut images = rows(h, id);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for note in reader.notetype_notes_after(id, None, 500).unwrap() {
        images.push(reader.row_image(Table::Notes, &RowKey::Id(note.id.0)).unwrap().unwrap());
        for card in reader.cards_of_note(note.id).unwrap() {
            images.push(reader.row_image(Table::Cards, &RowKey::Id(card.id.0)).unwrap().unwrap());
        }
    }
    if let Some(counter) = reader.row_image(Table::Config, &RowKey::text("nextPos")).unwrap() {
        images.push(counter);
    }
    images.push(reader.row_image(Table::Col, &RowKey::Id(1)).unwrap().unwrap());
    images
}

#[test]
fn template_formats_checked_success_preserves_metadata_ordinals_stamps_and_noop() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let mut nt = read(&h, id);
    nt.mtime = EpochSecs(42);
    nt.usn = Usn(8);
    nt.config.latex_pre = "kept header".to_owned();
    nt.config = with_unknown(&with_other(&nt.config, b"{\"extension\":17}"));
    nt.config.reqs.clear();
    let mut bytes = nt.config.to_bytes().into_owned();
    bytes.extend_from_slice(&[
        0x42, 9, 0x08, 0, 0x10, 1, 0x18, 0, 0xa0, 0x01, 7,
        0x42, 9, 0x08, 1, 0x10, 1, 0x18, 1, 0xa0, 0x01, 7,
    ]);
    nt.config = ProtoBlob::decode(&bytes).unwrap();
    nt.fields[1].ord = 4;
    nt.templates[1].ord = 7;
    for field in &mut nt.fields {
        field.config = with_unknown(&with_other(&field.config, b"kept field"));
    }
    for template in &mut nt.templates {
        template.config.id = None; // Legacy missing IDs are valid crosschecks.
        template.mtime = EpochSecs(321);
        template.usn = Usn(19);
        template.config.q_format_browser = "{{NoBrowserCheck}}".to_owned();
        template.config.a_format_browser = "kept browser back".to_owned();
        template.config = with_unknown(&with_other(&template.config, b"kept template"));
    }
    seed(&h, nt);
    let before = rows(&h, id);
    let old = read(&h, id);
    let schema = col(&h).1;
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let depth = h.collection.undo_depth_for_testing().unwrap();
    h.clock.step_ms(60_000);
    let result = formats_run(&h, formats_request(&old, 1, "{{Front}} edited", "{{Back}} edited", Some(".card { color: teal; }"))).unwrap();
    assert_eq!(result.value.id, id);
    assert!(result.value.written);
    assert!(result.value.cards_added.is_empty());
    assert_eq!(result.changes.notetypes.len(), 1);
    assert!(!result.changes.notetypes[0].schema_changed);
    assert!(result.changes.notes.is_empty());
    assert!(result.changes.cards.is_empty());
    assert!(!result.changes.collection_row);
    assert_eq!(col(&h), (h.clock.now_ms(), schema));
    let new = read(&h, id);
    assert_eq!((new.mtime.0, new.usn), (h.clock.now_ms() / 1000, Usn::PENDING));
    assert_eq!((&new.name, new.id), (&old.name, old.id));
    for (before, after) in old.fields.iter().zip(&new.fields) {
        assert_eq!((before.ord, &before.name), (after.ord, &after.name));
        assert_eq!(before.config.to_bytes(), after.config.to_bytes());
    }
    for (at, (before, after)) in old.templates.iter().zip(&new.templates).enumerate() {
        assert_eq!((before.ord, &before.name, before.mtime, before.usn), (after.ord, &after.name, after.mtime, after.usn));
        let mut expected = before.config.clone();
        if at == 1 {
            expected.q_format = "{{Front}} edited".to_owned();
            expected.a_format = "{{Back}} edited".to_owned();
        }
        assert_eq!(expected.to_bytes(), after.config.to_bytes());
    }
    let mut expected = old.config.clone();
    expected.css = ".card { color: teal; }".to_owned();
    expected.reqs[1].field_ords = vec![0];
    assert_eq!(new.config.to_bytes(), expected.to_bytes());
    assert!(new.config.reqs.iter().all(|req| !req.unknown_fields().is_empty()));
    assert_eq!(h.collection.undo_depth_for_testing().unwrap(), depth + 1);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap().name(), Some(meguri_engine::undo::names::UPDATE_NOTE_TYPE));
    let after = rows(&h, id);
    let after_col = col(&h);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), after);
    assert_eq!(col(&h), after_col);

    let events = h.engine.stream().subscribe();
    let depth = h.collection.undo_depth_for_testing().unwrap();
    let result = formats_run(&h, formats_request(&new, 1, "{{Front}} edited", "{{Back}} edited", Some(&new.config.css))).unwrap();
    assert!(!result.value.written);
    assert!(result.value.cards_added.is_empty());
    assert!(result.changes.is_empty());
    assert_eq!(rows(&h, id), after);
    assert_eq!(col(&h), after_col);
    assert_eq!(h.collection.undo_depth_for_testing().unwrap(), depth);
    assert!(common::drain(&events, Duration::from_millis(10)).is_empty());
}

#[test]
fn template_formats_plain_save_guard_and_checked_refusals_remain() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let nt = read(&h, id);
    let before = formats_rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    for skip_checks in [false, true] {
        for front in [false, true] {
            let mut draft = Draft::of(nt.clone());
            if front {
                draft.templates[0].config.q_format.push_str(" changed");
            } else {
                draft.templates[0].config.a_format.push_str(" changed");
            }
            assert!(matches!(notetypes::save(&h.collection, draft, SaveOptions { skip_checks }), Err(EngineError::InvalidArgument { .. })));
        }
    }
    for (front, back, kind) in [
        ("{{Back}}", "", notetypes::CheckKind::DuplicateFront),
        ("{{#Front}}", "", notetypes::CheckKind::TemplateParseError),
        ("plain", "", notetypes::CheckKind::NoFrontField),
        ("{{Nope}}", "", notetypes::CheckKind::UnknownField),
        ("{{Front}}", "{{Nope}}", notetypes::CheckKind::UnknownField),
    ] {
        assert!(matches!(
            formats_run(&h, formats_request(&nt, 0, front, back, Some("also refused"))),
            Err(EngineError::NotetypeRefused(notetypes::NotetypeRefusal::Template(failure))) if failure.kind == kind
        ));
    }
    assert_eq!(formats_rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    // The existing CSS-only plain save is still supported.
    let mut draft = Draft::of(nt);
    draft.config.css = "plain CSS save".to_owned();
    assert!(notetypes::save(&h.collection, draft, SaveOptions::default()).unwrap().value.written);

    let id = make(&h, true, &["{{cloze:Front}}"]);
    let nt = read(&h, id);
    let before = formats_rows(&h, id);
    assert!(matches!(
        formats_run(&h, formats_request(&nt, 0, "{{cloze:Front}}", "{{Front}}", None)),
        Err(EngineError::NotetypeRefused(notetypes::NotetypeRefusal::Template(failure))) if failure.kind == notetypes::CheckKind::MissingCloze
    ));
    assert_eq!(formats_rows(&h, id), before);
}

#[test]
fn template_formats_front_generates_missing_only_and_one_step_undo_redo_exact_ids() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let kept = common::notes::add(&h, common::notes::input(id, &["word", ""]));
    let cardless = common::notes::add(&h, common::notes::input(id, &["other", ""]));
    remove_fixture_card(&h, cardless.cards[0]);
    let sibling = kept.cards[0];
    h.collection.run(common::tagging::setup_spec(), move |ctx| {
        let mut card = ctx.scope().card(sibling)?.unwrap();
        card.card_type = meguri_model::card::CardType::Review;
        card.queue = meguri_model::card::CardQueue::Review;
        card.due = 42;
        card.interval = 17;
        card.reps = 8;
        ctx.scope().put(&card)?;
        ctx.scope().set_config("nextPos", &meguri_storage::ConfigValue::from(99_i64))?;
        Ok(())
    }).unwrap();
    let before = formats_rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let depth = h.collection.undo_depth_for_testing().unwrap();
    let sibling_row = h.collection.reader(ReaderRole::Query).unwrap().row_image(Table::Cards, &RowKey::Id(sibling.0)).unwrap().unwrap();
    let result = formats_run(&h, formats_request(&read(&h, id), 1, "{{Front}} reverse", "{{Back}}", None)).unwrap();
    assert!(result.value.written);
    assert_eq!(result.value.cards_added.len(), 1);
    let generated = result.value.cards_added[0];
    assert_eq!(result.changes.cards.len(), 1);
    assert_eq!(result.changes.cards[0].id, generated);
    assert!(result.changes.notes.is_empty());
    assert!(!result.changes.notetypes[0].schema_changed);
    assert_eq!(col(&h).1, before_col.1);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.card(generated).unwrap().unwrap();
    assert_eq!((card.note_id, card.ord, card.due), (kept.id, 1, 99));
    assert_eq!(reader.config_value("nextPos").unwrap().unwrap().as_i64(), Some(100));
    assert_eq!(reader.row_image(Table::Cards, &RowKey::Id(sibling.0)).unwrap().unwrap(), sibling_row);
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
    drop(reader);
    let after = formats_rows(&h, id);
    let after_col = col(&h);
    assert_eq!(h.collection.undo_depth_for_testing().unwrap(), depth + 1);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(formats_rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(formats_rows(&h, id), after);
    assert_eq!(col(&h), after_col);
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), 0);
    // Disabling an already populated front never removes or restamps cards.
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let cards: Vec<_> = reader.cards_of_note(kept.id).unwrap().into_iter().map(|c| {
        reader.row_image(Table::Cards, &RowKey::Id(c.id.0)).unwrap().unwrap()
    }).collect();
    drop(reader);
    let disabled = formats_run(&h, formats_request(&read(&h, id), 1, "{{#Back}}{{Back}}{{/Back}}", "{{Back}}", None)).unwrap();
    assert!(disabled.value.cards_added.is_empty());
    assert!(disabled.changes.cards.is_empty());
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for row in cards {
        let id = row[0].as_i64().unwrap();
        assert_eq!(reader.row_image(Table::Cards, &RowKey::Id(id)).unwrap().unwrap(), row);
    }
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
}

#[test]
fn template_formats_back_and_css_keep_requirements_and_never_generate() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let added = common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    remove_fixture_card(&h, added.cards[1]);
    let mut nt = read(&h, id);
    nt.config.reqs[0].field_ords = vec![1]; // Stale requirements are not repaired.
    seed(&h, nt);
    for (back, css) in [("changed {{Back}}", None), ("changed {{Back}}", Some("new CSS"))] {
        let nt = read(&h, id);
        let reqs = nt.config.to_bytes().into_owned();
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let card = reader.row_image(Table::Cards, &RowKey::Id(added.cards[0].0)).unwrap().unwrap();
        let counter = reader.config_value("nextPos").unwrap();
        drop(reader);
        let result = formats_run(&h, formats_request(&nt, 0, &nt.templates[0].config.q_format, back, css)).unwrap();
        assert!(result.value.written);
        assert!(result.value.cards_added.is_empty());
        assert!(result.changes.cards.is_empty());
        let mut expected = nt.config.clone();
        if let Some(css) = css { expected.css = css.to_owned(); }
        assert_eq!(read(&h, id).config.to_bytes(), expected.to_bytes());
        if css.is_none() { assert_eq!(read(&h, id).config.to_bytes().as_ref(), reqs.as_slice()); }
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        assert_eq!(reader.cards_of_note(added.id).unwrap().len(), 1);
        assert_eq!(reader.row_image(Table::Cards, &RowKey::Id(added.cards[0].0)).unwrap().unwrap(), card);
        assert_eq!(reader.config_value("nextPos").unwrap(), counter);
    }
}

#[test]
fn template_formats_same_second_stale_plan_writer_and_identity_refusals_are_atomic() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let nt = read(&h, id);
    let captured = formats_request(&nt, 0, "{{Front}} edited", "{{Back}}", Some("draft CSS"));
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = template_formats::plan(&*reader, captured.clone(), &CancelToken::new(), &ProgressSink::new()).unwrap();
    drop(reader);
    let mut draft = Draft::of(nt.clone());
    draft.config.css = "concurrent same-second edit".to_owned();
    notetypes::save(&h.collection, draft, SaveOptions::default()).unwrap();
    assert_eq!(read(&h, id).mtime, nt.mtime);
    assert_eq!(read(&h, id).usn, nt.usn);
    let before = formats_rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    assert!(matches!(formats_run(&h, captured), Err(EngineError::Stale { table: Table::Notetypes, .. })));
    assert!(matches!(h.collection.run(commit_spec(), move |ctx| template_formats::commit_in(ctx.scope(), &planned)), Err(EngineError::Stale { table: Table::Notetypes, .. })));
    let live = read(&h, id);
    for (ord, template_id) in [(99, live.templates[0].config.id), (0, Some(-17)), (0, None)] {
        let mut request = formats_request(&live, 0, "{{Front}} edited", "{{Back}}", None);
        request.template = TemplateIdentity { ord, id: template_id };
        assert!(matches!(formats_run(&h, request), Err(EngineError::InvalidArgument { .. })));
    }
    assert_eq!(formats_rows(&h, id), before);
    assert_eq!(col(&h), before_col);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);

    // A valid captured identity becomes stale if its stored optional ID
    // changes, both when planning and when committing an existing plan.
    common::notes::add(&h, common::notes::input(id, &["word", ""]));
    let nt = read(&h, id);
    let captured = formats_request(&nt, 1, "{{Front}} reverse", "{{Back}}", Some("draft CSS"));
    assert!(captured.template.id.is_some());
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = template_formats::plan(&*reader, captured.clone(), &CancelToken::new(), &ProgressSink::new()).unwrap();
    drop(reader);
    let mut concurrent = nt.clone();
    concurrent.templates[1].config.id = None;
    seed(&h, concurrent);
    let live = read(&h, id);
    assert_eq!((live.mtime, live.usn), (nt.mtime, nt.usn));
    assert_eq!(live.templates[1].config.id, None);
    assert_ne!(AggregateDigest::of(&live), captured.base);
    let before = formats_rows(&h, id);
    let before_col = col(&h);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let redo = h.collection.peek_redo(UndoScope::Global).unwrap();
    let depth = h.collection.undo_depth_for_testing().unwrap();
    let graves = common::query_int(&h.collection, "SELECT count(*) FROM graves");
    let revlog = common::query_int(&h.collection, "SELECT count(*) FROM revlog");
    let events = h.engine.stream().subscribe();
    for writer in [false, true] {
        let error = if writer {
            let planned = planned.clone();
            h.collection.run(commit_spec(), move |ctx| template_formats::commit_in(ctx.scope(), &planned)).unwrap_err()
        } else {
            let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
            template_formats::plan(&*reader, captured.clone(), &CancelToken::new(), &ProgressSink::new()).unwrap_err()
        };
        assert!(matches!(error, EngineError::Stale { table: Table::Notetypes, key } if key == id.0.to_string()));
        assert_eq!(formats_rows(&h, id), before);
        assert_eq!(col(&h), before_col);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
        assert_eq!(h.collection.peek_redo(UndoScope::Global).unwrap(), redo);
        assert_eq!(h.collection.undo_depth_for_testing().unwrap(), depth);
        assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), graves);
        assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM revlog"), revlog);
        assert!(common::drain(&events, Duration::from_millis(10)).iter().all(|event| !matches!(event.kind, EventKind::Changes(_))));
    }

    // Repeated optional IDs are crosschecks, never lookup identities.
    let mut nt = read(&h, id);
    for template in &mut nt.templates {
        template.config.id = Some(11);
    }
    seed(&h, nt);
    let nt = read(&h, id);
    formats_run(&h, formats_request(&nt, 1, "{{Back}} edited", "{{Front}}", None)).unwrap();
    let saved = read(&h, id);
    assert_eq!(saved.templates[0].config.to_bytes(), nt.templates[0].config.to_bytes());
    assert_eq!(saved.templates[1].config.id, Some(11));
    assert_eq!(saved.templates[1].config.q_format, "{{Back}} edited");
}

#[test]
fn template_formats_generation_rederives_live_notes_cards_and_positions() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let changed = common::notes::add(&h, common::notes::input(id, &["word", "meaning"]));
    remove_fixture_card(&h, changed.cards[1]);
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = template_formats::plan(
        &*reader,
        formats_request(&read(&h, id), 1, "{{Front}} reverse", "{{Back}}", None),
        &CancelToken::new(),
        &ProgressSink::new(),
    ).unwrap();
    drop(reader);
    let late = common::notes::add(&h, common::notes::input(id, &["late", ""]));
    let cardless = common::notes::add(&h, common::notes::input(id, &["cardless", ""]));
    remove_fixture_card(&h, cardless.cards[0]);
    let late_sibling = late.cards[0];
    let changed_note = changed.id;
    h.collection.run(common::tagging::setup_spec(), move |ctx| {
        let mut note = ctx.scope().note(changed_note)?.unwrap();
        note.fields = "\u{1f}meaning".to_owned();
        ctx.scope().put(&note)?;
        let mut card = ctx.scope().card(late_sibling)?.unwrap();
        card.due = 88;
        ctx.scope().put(&card)?;
        Ok(())
    }).unwrap();
    let result = h.collection.run(commit_spec(), move |ctx| template_formats::commit_in(ctx.scope(), &planned)).unwrap();
    assert_eq!(result.value.cards_added.len(), 1);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let generated = reader.card(result.value.cards_added[0]).unwrap().unwrap();
    assert_eq!((generated.note_id, generated.ord, generated.due), (late.id, 1, 88));
    assert_eq!(reader.cards_of_note(changed.id).unwrap().len(), 1);
    assert!(reader.cards_of_note(cardless.id).unwrap().is_empty());
}

#[derive(Default)]
struct FormatsObserver {
    // seq, generated-card count, structure-report presence
    seen: std::sync::Mutex<Vec<(u64, usize, bool)>>,
}

impl meguri_engine::ChangeObserver for FormatsObserver {
    fn name(&self) -> &str {
        "template formats observer"
    }

    fn observe(&self, commit: &meguri_engine::ObservedCommit<'_>) -> Result<(), EngineError> {
        self.seen.lock().unwrap().push((
            commit.seq.0,
            commit.changes.cards.len(),
            commit.payload.structure_report().is_some(),
        ));
        Ok(())
    }
}

#[test]
fn template_formats_real_precommit_fault_rolls_back_aggregate_cards_counter_and_publication() {
    use meguri_model::faultpoint::{self, Fault};
    let observer = Arc::new(FormatsObserver::default());
    let h = Harness::open_with(meguri_engine::Ports::new().observer(observer.clone()));
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    let added = common::notes::add(&h, common::notes::input(id, &["word", ""]));
    let sibling = added.cards[0];
    h.collection.run(common::tagging::setup_spec(), move |ctx| {
        let mut card = ctx.scope().card(sibling)?.unwrap();
        card.card_type = meguri_model::card::CardType::Review;
        card.queue = meguri_model::card::CardQueue::Review;
        ctx.scope().put(&card)?;
        ctx.scope().set_config("nextPos", &meguri_storage::ConfigValue::from(123_i64))?;
        Ok(())
    }).unwrap();
    let request = formats_request(&read(&h, id), 1, "{{Front}} reverse", "{{Back}}", Some("draft CSS"));
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = template_formats::plan(&*reader, request.clone(), &CancelToken::new(), &ProgressSink::new()).unwrap();
    drop(reader);
    let before = formats_rows(&h, id);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let redo = h.collection.peek_redo(UndoScope::Global).unwrap();
    let depth = h.collection.undo_depth_for_testing().unwrap();
    let graves = common::query_int(&h.collection, "SELECT count(*) FROM graves");
    observer.seen.lock().unwrap().clear();
    let events = h.engine.stream().subscribe();
    let error = h.collection.run(commit_spec(), move |ctx| {
        let result = template_formats::commit_in(ctx.scope(), &planned)?;
        assert!(result.written);
        assert_eq!(result.cards_added.len(), 1);
        assert_eq!(ctx.scope().notetype(id)?.unwrap().config.css, "draft CSS");
        assert_eq!(ctx.scope().config_value("nextPos")?.unwrap().as_i64(), Some(124));
        // This existing storage hook fires after both the aggregate and G9
        // writes, while the executor can still roll the transaction back.
        faultpoint::arm("storage.commit.before", Fault::IoError(std::io::ErrorKind::Other));
        Ok(result)
    });
    faultpoint::disarm("storage.commit.before");
    assert!(error.unwrap_err().to_string().contains("storage.commit.before"));
    assert_eq!(formats_rows(&h, id), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(h.collection.peek_redo(UndoScope::Global).unwrap(), redo);
    assert_eq!(h.collection.undo_depth_for_testing().unwrap(), depth);
    assert_eq!(common::query_int(&h.collection, "SELECT count(*) FROM graves"), graves);
    assert!(observer.seen.lock().unwrap().is_empty());
    assert!(common::drain(&events, Duration::from_millis(10)).iter().all(|event| !matches!(event.kind, EventKind::Changes(_))));

    let result = formats_run(&h, request).unwrap();
    let seen = observer.seen.lock().unwrap();
    assert_eq!(seen.len(), 1);
    assert_eq!(seen[0].1, 1);
    assert!(!seen[0].2); // Formats/CSS supply no layout or generation reason.
    let changes = common::drain(&events, Duration::from_millis(10)).into_iter().filter(|event| matches!(event.kind, EventKind::Changes(_))).collect::<Vec<_>>();
    assert_eq!(changes.len(), 1);
    assert_eq!(changes[0].seq.0, seen[0].0);
    assert_eq!(result.value.cards_added.len(), 1);
}

#[test]
fn template_formats_cancel_before_commit_writes_nothing_and_commit_finishes_past_cancel_point() {
    let h = fresh();
    let id = make(&h, false, &["{{Front}}", "{{Back}}"]);
    common::notes::add(&h, common::notes::input(id, &["word", ""]));
    let before = formats_rows(&h, id);
    let undo = h.collection.peek_undo(UndoScope::Global).unwrap();
    let request = formats_request(&read(&h, id), 1, "{{Front}} reverse", "{{Back}}", None);
    for pre_cancel in [true, false] {
        let token = CancelToken::new();
        let progress = ProgressSink::new();
        if pre_cancel {
            token.cancel();
        } else {
            progress.set_listener(Arc::new(CancelOnPhase { token: token.clone(), phase: ProgressPhase::Planning }));
        }
        assert!(matches!(notetypes::update_template_formats(&h.collection, request.clone(), token, progress), Err(EngineError::Cancelled)));
        assert_eq!(formats_rows(&h, id), before);
        assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    }
    let token = CancelToken::new();
    let reader = h.collection.reader(ReaderRole::Maintenance).unwrap();
    let planned = template_formats::plan(&*reader, request.clone(), &token, &ProgressSink::new()).unwrap();
    drop(reader);
    token.cancel();
    let events = h.engine.stream().subscribe();
    assert!(matches!(h.collection.run(commit_spec().cancel(token), move |ctx| template_formats::commit_in(ctx.scope(), &planned)), Err(EngineError::Cancelled)));
    assert_eq!(formats_rows(&h, id), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), undo);
    assert!(common::drain(&events, Duration::from_millis(10)).iter().all(|event| !matches!(event.kind, EventKind::Changes(_))));

    let token = CancelToken::new();
    let progress = ProgressSink::new();
    progress.set_listener(Arc::new(CancelOnPhase { token: token.clone(), phase: ProgressPhase::Committing }));
    let result = notetypes::update_template_formats(&h.collection, request, token, progress).unwrap();
    assert!(result.past_cancel_point);
    assert!(result.value.written);
    assert_eq!(result.value.cards_added.len(), 1);
}
