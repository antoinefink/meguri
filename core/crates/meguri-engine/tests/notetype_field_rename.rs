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
                ctx.scope().put_notetype(&nt)?;
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
        "{{ Back }}|{{text:hint:Front}}|{{# Back }}{{^Front}}s{{/Front}}{{/ Back }}|{{front}}|{{text: Front}}<!--{{Front}}-->"
    );
    assert_eq!(t.a_format, "{{Front}} / {{{Back}}} / {{::Back}}");
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
