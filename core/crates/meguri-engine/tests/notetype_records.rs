//! Note type records (`specs/notetypes/records.md` N1 to N10) and the save
//! checks (`specs/notetypes/validation.md` V1 to V5) through meguri-engine's
//! note type ops: what a save writes row by row, its stamps and undo
//! images, name normalization, new and cloned note types, the on-demand
//! Image Occlusion note type, and what the scenarios cannot show.

mod common;

use std::time::Duration;

use common::{Harness, drain};
use meguri_engine::executor::catalog;
use meguri_engine::notetypes::{
    self, CheckKind, Draft, FieldNameCheck, FieldNameRefusal, InvalidNotetype, NewNotetype, NewTemplate, NotetypeKind,
    NotetypeRefusal, SaveOptions, StockKind, TemplateFailure, TemplateSide, check_typed_field_name, template_failures,
};
use meguri_engine::undo::names::{ADD_NOTE_TYPE, UPDATE_NOTE_TYPE};
use meguri_engine::{EngineError, EventKind, OpSpec, UndoScope};
use meguri_model::changes::OpSource;
use meguri_model::clock::Clock;
use meguri_model::ids::NotetypeId;
use meguri_model::notetype::Notetype;
use meguri_model::proto::ProtoBlob;
use meguri_model::schema11::notetype_to_json;
use meguri_model::time::{EpochMillis, EpochSecs};
use meguri_model::usn::Usn;
use meguri_storage::journal::{
    LayoutReport as AuthoredLayoutReport, LayoutShape, StructureReason, StructureReport as AuthoredStructureReport,
};
use meguri_storage::raw::SqlValue;
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, StampMode, Table};

fn template(name: &str, front: &str, back: &str) -> NewTemplate {
    NewTemplate {
        name: name.to_owned(),
        front: front.to_owned(),
        back: back.to_owned(),
        ..Default::default()
    }
}

fn new_notetype(name: &str, fields: &[&str], templates: Vec<NewTemplate>) -> NewNotetype {
    NewNotetype {
        name: name.to_owned(),
        kind: NotetypeKind::Standard,
        fields: fields.iter().map(|f| (*f).to_owned()).collect(),
        templates,
        css: Some(String::new()),
    }
}

/// Pair: fields Front and Back, templates Forward and Reverse.
fn pair(name: &str) -> NewNotetype {
    new_notetype(
        name,
        &["Front", "Back"],
        vec![
            template("Forward", "{{Front}}", "{{Back}}"),
            template("Reverse", "{{Back}}", "{{Front}}"),
        ],
    )
}

fn add(h: &Harness, new: NewNotetype) -> Result<notetypes::Added, EngineError> {
    notetypes::add(&h.collection, new).map(|r| r.value)
}

fn read(h: &Harness, id: NotetypeId) -> Notetype {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(id)
        .unwrap()
        .expect("the note type exists")
}

/// A draft of the stored note type (records N3, engine/op-model O7).
fn edit(h: &Harness, id: NotetypeId) -> Draft {
    Draft::of(read(h, id))
}

fn save(h: &Harness, draft: Draft) -> Result<notetypes::Saved, EngineError> {
    notetypes::save(&h.collection, draft, SaveOptions::default()).map(|r| r.value)
}

fn refusal(result: Result<impl std::fmt::Debug, EngineError>) -> NotetypeRefusal {
    match result {
        Err(EngineError::NotetypeRefused(refusal)) => refusal,
        other => panic!("expected a refused save, got {other:?}"),
    }
}

fn kind_of(result: Result<impl std::fmt::Debug, EngineError>) -> &'static str {
    refusal(result).kind()
}

/// The stored rows of a note type: its `notetypes` row, then its `fields`
/// and `templates` rows by ord.
fn rows(h: &Harness, id: NotetypeId) -> Vec<RowImage> {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let mut images = vec![reader.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap()];
    for table in [Table::Fields, Table::Templates] {
        for ord in 0..20 {
            if let Some(image) = reader.row_image(table, &RowKey::Pair(id.0, ord)).unwrap() {
                images.push(image);
            }
        }
    }
    images
}

fn col_mod(h: &Harness) -> i64 {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .col()
        .unwrap()
        .modified
        .0
}

fn scm(h: &Harness) -> i64 {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .col()
        .unwrap()
        .schema_modified
        .0
}

/// Writes the note type's rows as given (a sync would): stamps included.
fn put_as_given(h: &Harness, notetype: Notetype) {
    h.collection
        .run(
            OpSpec::named("test_put_notetype", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                ctx.scope().put_notetype(&notetype)?;
                Ok(())
            },
        )
        .unwrap();
}

/// The note type as a sync left it: `mtime_secs` 1700000000, `usn` 5.
fn synced(h: &Harness, id: NotetypeId) {
    let mut notetype = read(h, id);
    notetype.mtime = EpochSecs(1_700_000_000);
    notetype.usn = Usn(5);
    put_as_given(h, notetype);
}

#[test]
fn a_save_that_changes_nothing_writes_nothing() {
    // N3.1 (R906, R1472): no row, no stamp, no col.mod, no change set, no
    // undo step.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    synced(&h, id);
    h.clock.step_ms(60_000);
    let (before, modified) = (rows(&h, id), col_mod(&h));
    let top = h.collection.peek_undo(UndoScope::Global).unwrap();
    let events = h.engine.stream().subscribe();
    let saved = save(&h, edit(&h, id)).unwrap();
    assert!(!saved.written);
    assert_eq!(rows(&h, id), before);
    assert_eq!(col_mod(&h), modified);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), top);
    assert!(
        drain(&events, Duration::from_millis(50))
            .iter()
            .all(|e| !matches!(e.kind, EventKind::Changes(_)))
    );
}

#[test]
fn a_styling_change_stamps_the_notetype_row_and_rewrites_no_field_or_template() {
    // N3.2, N8, R950: the notetypes row is stamped as a local change; the
    // fields and templates rows stay; scm does not move.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    synced(&h, id);
    h.clock.step_ms(60_000);
    let before = rows(&h, id);
    let schema = scm(&h);
    let mut draft = edit(&h, id);
    draft.config.css = ".card { color: teal; }".to_owned();
    let saved = save(&h, draft).unwrap();
    assert!(saved.written);
    let after = read(&h, id);
    assert_eq!(after.mtime.0, h.clock.now_ms() / 1000);
    assert_eq!(after.usn, Usn(-1));
    assert_eq!(after.config.css, ".card { color: teal; }");
    assert_eq!(rows(&h, id)[1..], before[1..]);
    assert_eq!(scm(&h), schema);
    assert_eq!(col_mod(&h), h.clock.now_ms());
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some(UPDATE_NOTE_TYPE)
    );
}

/// Stands in for the end of a normal sync with server usn 5, which U107 owns:
/// every note type marked as sent (`usn` 5), then the `col` row finalised as
/// R1531 gives (`mod` and `ls` the server's time, here now; `usn` 6). Then a
/// minute later, so a schema change made now would move `scm` past `ls`.
fn all_synced(h: &Harness) {
    let ids = notetypes::read::ids(&*h.collection.reader(ReaderRole::Query).unwrap()).unwrap();
    for id in ids {
        synced(h, id);
    }
    let now = h.clock.now_ms();
    h.collection
        .run(
            OpSpec::named("test_sync_finish", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                let mut col = ctx.scope().col()?;
                col.modified = EpochMillis(now);
                col.last_sync = EpochMillis(now);
                col.usn = Usn(6);
                ctx.scope().put_col(&col)?;
                Ok(())
            },
        )
        .unwrap();
    h.clock.step_ms(60_000);
}

/// The `col` row's `mod`, `scm` and `ls`, which the offline sync status
/// reads (`sync/meta-status` M5).
fn sync_stamps(h: &Harness) -> (i64, i64, i64) {
    let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    (col.modified.0, col.schema_modified.0, col.last_sync.0)
}

/// The pending note types (`usn` -1, `storage/ids-usn-mtime` S2), each whole
/// in its schema-11 form (`model/schema11-json`, the form `applyChanges`
/// carries, `sync/wire-format` W6.1), by id. Which rows a normal sync sends
/// and the upload itself are U107's.
fn pending_notetypes(h: &Harness) -> Vec<serde_json::Value> {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let pending = reader
        .execute_raw("SELECT id FROM notetypes WHERE usn = -1 ORDER BY id", &[])
        .unwrap();
    pending
        .rows
        .iter()
        .map(|row| {
            let SqlValue::Integer(id) = row[0] else {
                panic!("{row:?}")
            };
            let notetype = reader.notetype(NotetypeId(id)).unwrap().unwrap();
            serde_json::from_str(&notetype_to_json(&notetype)).unwrap()
        })
        .collect()
}

/// `sync/meta-status` M5 right after a sync: `mod` = `ls` and `scm` not
/// past `ls`, so nothing is pending locally.
#[track_caller]
fn assert_nothing_pending(h: &Harness) {
    let (modified, schema, last_sync) = sync_stamps(h);
    assert_ne!(last_sync, 0);
    assert!(schema <= last_sync, "scm {schema} ls {last_sync}");
    assert_eq!(modified, last_sync);
    assert!(pending_notetypes(h).is_empty());
}

/// M5 after a local change that is not a schema change: `scm` as it was and
/// not past `ls`, `mod` past `ls`: local changes pending, which a normal
/// sync sends (M5's one-way row does not match).
#[track_caller]
fn assert_changes_pending_without_one_way(h: &Harness, schema_before: i64) {
    let (modified, schema, last_sync) = sync_stamps(h);
    assert_eq!(schema, schema_before);
    assert!(schema <= last_sync, "scm {schema} ls {last_sync}");
    assert!(modified > last_sync, "mod {modified} ls {last_sync}");
}

#[test]
fn a_field_option_save_or_a_field_rename_leaves_the_whole_note_type_pending_for_a_normal_sync() {
    // N3.2, N8, R950: after a sync, a field option save and a field rename
    // each make the note type pending as a whole, its schema-11 form carries
    // every edit, and `scm` stays, so the collection needs a normal sync and
    // not a one-way one (meta-status M5). That the normal sync then uploads
    // it is U107's (normal_sync.rs): U107 depends on this unit.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    all_synced(&h);
    assert_nothing_pending(&h);
    let schema = scm(&h);

    let mut draft = edit(&h, id);
    draft.fields[0].config.sticky = true;
    draft.fields[1].config.description = "the answer".to_owned();
    save(&h, draft).unwrap();
    let pending = pending_notetypes(&h);
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0]["id"], id.0);
    assert_eq!(pending[0]["usn"], -1);
    assert_eq!(pending[0]["mod"], h.clock.now_ms() / 1000);
    assert_eq!(pending[0]["flds"][0]["sticky"], true);
    assert_eq!(pending[0]["flds"][1]["description"], "the answer");
    assert_changes_pending_without_one_way(&h, schema);

    // A sync sends it and marks it sent. A field rename is U34's op; the
    // aggregate rule is the record's, so a write of the renamed aggregate
    // through the scope stamps the notetypes row too.
    all_synced(&h);
    assert_nothing_pending(&h);
    let schema = scm(&h);
    let mut renamed = read(&h, id);
    h.collection
        .run(OpSpec::named("test_rename_field", OpSource::UserUndoable), move |ctx| {
            let mut report = AuthoredStructureReport::default();
            // This fixture renames one field without moving any ordered position.
            report.layouts.insert(id.0, AuthoredLayoutReport::identity(&renamed)?);
            let token = ctx.scope().start_structure_report(report)?;
            renamed.fields[1].name = "Answer".to_owned();
            ctx.scope().put_notetype(&renamed)?;
            ctx.scope().complete_structure_report(token)?;
            Ok(())
        })
        .unwrap();
    let pending = pending_notetypes(&h);
    assert_eq!(pending.len(), 1, "{pending:?}");
    assert_eq!(pending[0]["id"], id.0);
    assert_eq!(pending[0]["usn"], -1);
    let fields: Vec<(&str, bool, &str)> = pending[0]["flds"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| {
            (
                f["name"].as_str().unwrap(),
                f["sticky"].as_bool().unwrap(),
                f["description"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(fields, [("Front", true, ""), ("Answer", false, "the answer")]);
    assert_changes_pending_without_one_way(&h, schema);
}

#[test]
fn a_draft_made_before_another_save_is_stale_and_writes_nothing() {
    // engine/op-model O7: two drafts of one note type are open; the first
    // saves a field option, then the older second one saves the styling. It
    // must not write back the field option the first one changed, even in
    // the same second, where the notetypes row's stamps are equal.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    let mut first = edit(&h, id);
    let mut second = edit(&h, id);
    first.fields[0].config.sticky = true;
    save(&h, first).unwrap();
    let saved = rows(&h, id);
    let top = h.collection.peek_undo(UndoScope::Global).unwrap();
    second.config.css = ".card { color: teal; }".to_owned();
    let stale = save(&h, second);
    assert!(
        matches!(&stale, Err(EngineError::Stale { table: Table::Notetypes, key }) if *key == id.0.to_string()),
        "{stale:?}"
    );
    assert_eq!(rows(&h, id), saved);
    assert!(read(&h, id).fields[0].config.sticky);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), top);

    // A draft of the note type as it is now saves. A sync that only marks
    // the row as sent (its stamps) leaves such a draft current.
    let mut third = edit(&h, id);
    synced(&h, id);
    third.config.css = ".card { color: teal; }".to_owned();
    save(&h, third).unwrap();
    let stored = read(&h, id);
    assert!(stored.fields[0].config.sticky);
    assert_eq!(stored.config.css, ".card { color: teal; }");
}

#[test]
fn template_rows_keep_their_stamps_wherever_they_move() {
    // N3.3: templates.mtime_secs and usn are data, never stamps.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    let mut stamped = read(&h, id);
    stamped.templates[1].mtime = EpochSecs(123);
    stamped.templates[1].usn = Usn(7);
    put_as_given(&h, stamped);
    let mut draft = edit(&h, id);
    draft.templates[1].name = "Backwards".to_owned();
    save(&h, draft).unwrap();
    let stored = read(&h, id);
    assert_eq!(stored.templates[1].name, "Backwards");
    assert_eq!(
        (stored.templates[1].mtime, stored.templates[1].usn),
        (EpochSecs(123), Usn(7))
    );
    assert_eq!(
        (stored.templates[0].mtime, stored.templates[0].usn),
        (EpochSecs(0), Usn(0))
    );
}

#[test]
fn template_names_swapped_in_one_save_pass_the_unique_index() {
    // N3.2: the unique (name, ntid) index under unicase never sees two rows
    // of one name halfway through the save.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    let mut draft = edit(&h, id);
    draft.templates[0].name = "Reverse".to_owned();
    draft.templates[1].name = "FORWARD".to_owned();
    save(&h, draft).unwrap();
    let stored = read(&h, id);
    assert_eq!(
        stored.templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Reverse", "FORWARD"]
    );
}

#[test]
fn undo_restores_every_row_of_the_aggregate_and_redo_the_saved_ones() {
    // N3.4: the notetypes row's mtime_secs and usn included.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    synced(&h, id);
    h.clock.step_ms(60_000);
    let before = rows(&h, id);
    let mut draft = edit(&h, id);
    draft.config.css = "x".to_owned();
    draft.templates[0].name = "Ahead".to_owned();
    draft.fields[1].config.description = "the answer".to_owned();
    save(&h, draft).unwrap();
    let after = rows(&h, id);
    assert_ne!(after, before);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h, id), after);
}

#[test]
fn a_note_type_with_gaps_in_its_ords_is_saved_without_them() {
    // N1: read in ord order, positions as ordinals; written back from 0
    // without gaps.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    for sql in [
        "UPDATE fields SET ord = 4 WHERE ntid = ?1 AND ord = 1",
        "UPDATE templates SET ord = 3 WHERE ntid = ?1 AND ord = 1",
    ] {
        h.collection
            .submit_maintenance(catalog::ANALYZE, Duration::from_millis(10), move |writer| {
                Ok(writer.execute_raw(sql, &[SqlValue::Integer(id.0)])?)
            })
            .wait()
            .unwrap();
    }
    let ords = |table| match &h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .execute_raw(
            &format!("SELECT group_concat(ord) FROM (SELECT ord FROM {table} WHERE ntid = ?1 ORDER BY ord)"),
            &[SqlValue::Integer(id.0)],
        )
        .unwrap()
        .rows[0][0]
    {
        SqlValue::Text(text) => text.clone(),
        other => panic!("{other:?}"),
    };
    assert_eq!(
        (ords("fields"), ords("templates")),
        ("0,4".to_owned(), "0,3".to_owned())
    );
    let mut draft = edit(&h, id);
    assert_eq!(draft.fields[1].name, "Back");
    draft.config.css = "y".to_owned();
    save(&h, draft).unwrap();
    assert_eq!(
        (ords("fields"), ords("templates")),
        ("0,1".to_owned(), "0,1".to_owned())
    );
    assert_eq!(read(&h, id).fields[1].name, "Back");
}

#[test]
fn a_new_notetype_holds_the_values_of_n5() {
    let h = Harness::open();
    let schema = scm(&h);
    let added = add(
        &h,
        NewNotetype {
            css: None,
            ..pair("Fresh")
        },
    )
    .unwrap();
    let stored = read(&h, added.id);
    let now_s = h.clock.now_ms() / 1000;
    assert_eq!(stored.name, "Fresh");
    assert_eq!((stored.mtime.0, stored.usn), (now_s, Usn(-1)));
    let config = &stored.config;
    assert_eq!(config.kind, 0);
    assert_eq!(config.sort_field_idx, 0);
    assert_eq!(config.css, notetypes::BASE_CARD_CSS);
    assert_eq!(
        config.latex_pre,
        "\\documentclass[12pt]{article}\n\\special{papersize=3in,5in}\n\\usepackage[utf8]{inputenc}\n\
         \\usepackage{amssymb,amsmath}\n\\pagestyle{empty}\n\\setlength{\\parindent}{0in}\n\\begin{document}\n"
    );
    assert_eq!(config.latex_pre.len(), 178);
    assert_eq!(config.latex_post, "\\end{document}");
    assert!(!config.latex_svg);
    assert_eq!(config.original_stock_kind, 1);
    assert_eq!(config.original_id, None);
    assert_eq!(config.target_deck_id_unused, 0);
    assert!(config.other().is_empty());
    let mut ids = Vec::new();
    for (ord, field) in stored.fields.iter().enumerate() {
        let c = &field.config;
        assert_eq!(field.ord as usize, ord);
        assert_eq!((c.font_name.as_str(), c.font_size), ("Arial", 20));
        assert!(!(c.sticky || c.rtl || c.plain_text || c.collapsed || c.exclude_from_search || c.prevent_deletion));
        assert_eq!((c.description.as_str(), c.tag), ("", None));
        ids.push(c.id.expect("a field id"));
    }
    for template in &stored.templates {
        let c = &template.config;
        assert_eq!((template.mtime, template.usn), (EpochSecs(0), Usn(0)));
        assert_eq!((c.q_format_browser.as_str(), c.a_format_browser.as_str()), ("", ""));
        assert_eq!(
            (c.target_deck_id, c.browser_font_name.as_str(), c.browser_font_size),
            (0, "", 0)
        );
        ids.push(c.id.expect("a template id"));
    }
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), 4, "fresh ids");
    // curModel: the new id, as a local config change; no schema change.
    let current = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .config_entry("curModel")
        .unwrap()
        .expect("curModel")
        .entry;
    assert_eq!(current.value, added.id.0.to_string().into_bytes());
    assert_eq!((current.usn, current.mtime.0), (Usn(-1), now_s));
    assert_eq!(scm(&h), schema);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some(ADD_NOTE_TYPE)
    );
}

#[test]
fn card_requirements_follow_the_fronts() {
    // N6 and render/template-syntax T11.3's examples.
    let h = Harness::open();
    let id = add(
        &h,
        new_notetype(
            "Reqs",
            &["A", "B", "C"],
            vec![
                template("1", "{{A}}{{B}}", ""),
                template("2", "{{#A}}{{B}}{{/A}}", ""),
                template("3", "{{^A}}{{B}}{{/A}}", ""),
                template("4", "{{^A}}{{#B}}{{C}}{{/B}}{{/A}}", ""),
                template("5", "{{^C}}{{#A}}{{B}}{{/A}}{{/C}}", ""),
            ],
        ),
    )
    .unwrap()
    .id;
    let reqs: Vec<(u32, i32, Vec<u32>)> = read(&h, id)
        .config
        .reqs
        .iter()
        .map(|r| (r.card_ord, r.kind, r.field_ords.clone()))
        .collect();
    assert_eq!(
        reqs,
        [
            (0, 1, vec![0, 1]),
            (1, 2, vec![0, 1]),
            (2, 1, vec![1]),
            (3, 2, vec![1, 2]),
            (4, 2, vec![0, 1]),
        ]
    );
}

#[test]
fn names_are_normalized_as_n4_says() {
    let h = Harness::open();
    no_notetypes(&h);
    let first = add(&h, pair("Pair")).unwrap();
    assert_eq!(first.name, "Pair");
    // Step 7 with Pair stored: pair+, then PAIR++, then Pa"ir gives Pair+++.
    assert_eq!(add(&h, pair("pair")).unwrap().name, "pair+");
    assert_eq!(add(&h, pair("PAIR")).unwrap().name, "PAIR++");
    assert_eq!(add(&h, pair("Pa\"ir")).unwrap().name, "Pair+++");
    // Steps 1 and 2: NFC, no trimming.
    let nfd = add(&h, pair("Cafe\u{301}")).unwrap();
    assert_eq!(nfd.name, "Caf\u{e9}");
    assert_eq!(read(&h, nfd.id).name, "Caf\u{e9}");
    assert_eq!(add(&h, pair("  spaced  ")).unwrap().name, "  spaced  ");
    // Steps 3 and 5 on fields, 4 and 5 on templates.
    let names = add(
        &h,
        new_notetype(
            "Names",
            &["  # /^ Fr:o{n}t\"", "a #", "front", "FRONT", "Stra\u{df}e", "STRASSE"],
            vec![
                template("Card", "{{Front}}", ""),
                template("card", "{{a #}}", ""),
                template("\"Q\"", "{{Stra\u{df}e}}", ""),
                template("  ", "{{STRASSE+}}", ""),
            ],
        ),
    )
    .unwrap();
    let stored = read(&h, names.id);
    assert_eq!(
        stored.fields.iter().map(|f| f.name.as_str()).collect::<Vec<_>>(),
        ["Front", "a #", "front+", "FRONT++", "Stra\u{df}e", "STRASSE+"]
    );
    assert_eq!(
        stored.templates.iter().map(|t| t.name.as_str()).collect::<Vec<_>>(),
        ["Card", "card+", "Q", "  "]
    );
    // A case-only rename keeps its spelling: a note type never collides with
    // itself.
    let mut draft = edit(&h, first.id);
    draft.name = "PAIR".to_owned();
    assert_eq!(save(&h, draft).unwrap().name, "PAIR");
    let mut draft = edit(&h, first.id);
    draft.name = "Caf\u{e9}".to_owned();
    assert_eq!(save(&h, draft).unwrap().name, "Caf\u{e9}+");
    let alpha = add(&h, pair("Alpha")).unwrap();
    let mut draft = edit(&h, alpha.id);
    draft.name = "ALPHA".to_owned();
    assert_eq!(save(&h, draft).unwrap().name, "ALPHA");
}

#[test]
fn names_that_end_up_empty_refuse_the_save() {
    // V2 (R942): invalid_notetype, nothing written.
    let h = Harness::open();
    let count = || {
        notetypes::read::ids(&*h.collection.reader(ReaderRole::Query).unwrap())
            .unwrap()
            .len()
    };
    let before = count();
    assert_eq!(
        refusal(add(&h, pair("\"\""))),
        NotetypeRefusal::Invalid(InvalidNotetype::EmptyName)
    );
    let colons = new_notetype("Colons", &["::"], vec![template("One", "{{Front}}", "")]);
    assert_eq!(
        refusal(add(&h, colons)),
        NotetypeRefusal::Invalid(InvalidNotetype::EmptyFieldName { ord: 0 })
    );
    let quotes = new_notetype("Quotes", &["Front"], vec![template("\"\"", "{{Front}}", "")]);
    assert_eq!(
        refusal(add(&h, quotes)),
        NotetypeRefusal::Invalid(InvalidNotetype::EmptyTemplateName { ord: 0 })
    );
    assert_eq!(
        refusal(add(&h, new_notetype("NoField", &[], vec![template("One", "x", "")]))),
        NotetypeRefusal::Invalid(InvalidNotetype::NoField)
    );
    assert_eq!(
        refusal(add(&h, new_notetype("NoTemplate", &["F"], vec![]))),
        NotetypeRefusal::Invalid(InvalidNotetype::NoTemplate)
    );
    assert_eq!(count(), before);
}

#[test]
fn the_template_checks_refuse_what_v3_lists() {
    // R942, R943, through checked adds.
    let h = Harness::open();
    let dup = new_notetype(
        "Dup",
        &["F", "B"],
        vec![template("A", "{{F}}", "{{B}}"), template("B", "{{F}}", "{{B}}")],
    );
    assert_eq!(kind_of(add(&h, dup)), "duplicate_front");
    let with_card = new_notetype(
        "DupCard",
        &["F", "B"],
        vec![
            template("A", "{{F}}{{ Card }}", "{{B}}"),
            template("B", "{{F}}{{ Card }}", "{{B}}"),
        ],
    );
    add(&h, with_card).unwrap();
    let no_field = new_notetype("NoFront", &["F"], vec![template("A", "<!--{{F}}-->", "{{F}}")]);
    assert_eq!(kind_of(add(&h, no_field)), "no_front_field");
    let unknown = new_notetype("Unknown", &["Front"], vec![template("A", "{{front}}", "")]);
    assert_eq!(kind_of(add(&h, unknown)), "unknown_field");
    let parse = new_notetype("Parse", &["F"], vec![template("A", "{{F}}", "{{/F}}")]);
    assert_eq!(kind_of(add(&h, parse)), "template_parse_error");
    let cloze = NewNotetype {
        kind: NotetypeKind::Cloze,
        ..new_notetype("Cloze", &["T"], vec![template("C", "{{cloze:T}}", "{{T}}")])
    };
    assert_eq!(kind_of(add(&h, cloze)), "missing_cloze");
    let specials = new_notetype(
        "Specials",
        &["F"],
        vec![template(
            "A",
            "{{F}}{{Tags}}{{Type}}{{Deck}}{{Subdeck}}{{CardFlag}}{{CardID}}{{#c1}}x{{/c1}}{{}}",
            "{{FrontSide}}",
        )],
    );
    add(&h, specials).unwrap();
}

#[test]
fn the_editor_sees_every_failure_in_the_order_of_v3() {
    // V4: repeated fronts first, then per template the first of 2.1 to 2.3,
    // then the cloze failure.
    let h = Harness::open();
    let mut notetype = read(&h, add(&h, pair("Pair")).unwrap().id);
    let set = |t: &mut meguri_model::notetype::NotetypeTemplate, front: &str, back: &str| {
        t.config.q_format = front.to_owned();
        t.config.a_format = back.to_owned();
    };
    let extra = notetype.templates[0].clone();
    notetype.templates.push(extra);
    let [a, b, c] = &mut notetype.templates[..] else {
        unreachable!()
    };
    set(a, "{{Front}}{{Nope}}", "{{Back}}");
    set(b, "{{Front}}{{Nope}}", "{{#Back}}");
    set(c, "plain", "{{Other}}");
    notetype.config.kind = 1;
    let failures = template_failures(&notetype);
    let expected = vec![
        TemplateFailure {
            kind: CheckKind::DuplicateFront,
            template: 1,
            side: Some(TemplateSide::Front),
            name: None,
            earlier: Some(0),
        },
        TemplateFailure {
            kind: CheckKind::UnknownField,
            template: 0,
            side: Some(TemplateSide::Front),
            name: Some("Nope".to_owned()),
            earlier: None,
        },
        TemplateFailure {
            kind: CheckKind::TemplateParseError,
            template: 1,
            side: Some(TemplateSide::Back),
            name: None,
            earlier: None,
        },
        TemplateFailure {
            kind: CheckKind::NoFrontField,
            template: 2,
            side: Some(TemplateSide::Front),
            name: None,
            earlier: None,
        },
        TemplateFailure {
            kind: CheckKind::MissingCloze,
            template: 0,
            side: Some(TemplateSide::Front),
            name: None,
            earlier: None,
        },
    ];
    assert_eq!(failures, expected);
}

/// A note type whose second template now references a field it does not
/// have, written past the checks (as a sync could bring it).
fn broken(h: &Harness) -> NotetypeId {
    let id = add(h, pair("Broken")).unwrap().id;
    let mut notetype = read(h, id);
    notetype.templates[1].config.a_format = "{{Gone}}".to_owned();
    put_as_given(h, notetype);
    id
}

#[test]
fn an_unchecked_save_skips_the_template_checks() {
    // V1: a field-editor save of a note type whose templates fail V3
    // succeeds; a rename of it, a checked save, fails.
    let h = Harness::open();
    let id = broken(&h);
    let mut draft = edit(&h, id);
    draft.fields[0].config.rtl = true;
    assert_eq!(
        kind_of(notetypes::save(&h.collection, draft.clone(), SaveOptions::default())),
        "unknown_field"
    );
    notetypes::save(&h.collection, draft, SaveOptions { skip_checks: true }).unwrap();
    assert!(read(&h, id).fields[0].config.rtl);
    let mut renamed = edit(&h, id);
    renamed.name = "Renamed".to_owned();
    assert_eq!(kind_of(save(&h, renamed)), "unknown_field");
    assert_eq!(read(&h, id).name, "Broken");
}

#[test]
fn a_plain_save_refuses_structure_changes() {
    // U34 owns field and template list changes, field renames, template
    // text and the sort field (notetype-ops.md T5).
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    let stored = edit(&h, id);
    let before = rows(&h, id);
    let invalid = |draft: Draft| matches!(save(&h, draft), Err(EngineError::InvalidArgument { .. }));
    let mut draft = stored.clone();
    draft.fields.pop();
    assert!(invalid(draft));
    let mut draft = stored.clone();
    draft.fields[1].name = "Answer".to_owned();
    assert!(invalid(draft));
    let mut draft = stored.clone();
    draft.templates.swap(0, 1);
    assert!(invalid(draft));
    let mut draft = stored.clone();
    draft.templates[0].config.a_format = "{{Back}}!".to_owned();
    assert!(invalid(draft));
    let mut draft = stored.clone();
    draft.config.sort_field_idx = 1;
    assert!(invalid(draft));
    assert_eq!(rows(&h, id), before);
}

#[test]
fn a_typed_field_name_is_checked_as_v5_says() {
    let names = ["Front", "Back"];
    let check = |typed: &str, renaming| check_typed_field_name(typed, &names, renaming);
    assert_eq!(
        check("  \"Extra\"  ", None),
        FieldNameCheck::Accepted("Extra".to_owned())
    );
    assert_eq!(check(" \" ", None), FieldNameCheck::Ignored);
    for bad in ["#x", "^x", "/x"] {
        assert_eq!(
            check(bad, None),
            FieldNameCheck::Refused(FieldNameRefusal::BadFirstCharacter)
        );
    }
    for bad in ["a:b", "a{b", "a}b"] {
        assert_eq!(
            check(bad, None),
            FieldNameCheck::Refused(FieldNameRefusal::BadCharacter)
        );
    }
    assert_eq!(
        check("BACK", None),
        FieldNameCheck::Refused(FieldNameRefusal::Duplicate)
    );
    assert_eq!(
        check("BACK", Some(0)),
        FieldNameCheck::Refused(FieldNameRefusal::Duplicate)
    );
    // A case-only rename of the field itself is accepted.
    assert_eq!(check("BACK", Some(1)), FieldNameCheck::Accepted("BACK".to_owned()));
}

/// Appends an unknown protobuf field (number 20, varint 7) to a blob.
fn with_unknown<M: meguri_model::proto::BlobMessage>(blob: &ProtoBlob<M>) -> ProtoBlob<M> {
    let mut bytes = blob.to_bytes().into_owned();
    bytes.extend_from_slice(&[0xa0, 0x01, 0x07]);
    ProtoBlob::decode(&bytes).unwrap()
}

fn has_unknown(fields: &meguri_model::proto::UnknownFields) -> bool {
    fields
        .iter()
        .any(|f| f.number() == 20 && f.bytes() == [0xa0, 0x01, 0x07])
}

#[test]
fn unknown_fields_and_latex_values_survive_a_styling_change() {
    // N1, N3.2 (R904, R1485): deviation notetype-unknown-fields-kept.
    let h = Harness::open();
    let id = add(&h, pair("Pair")).unwrap().id;
    let mut notetype = read(&h, id);
    notetype.config.latex_pre = "\\documentclass{minimal}".to_owned();
    notetype.config.latex_svg = true;
    notetype.config = with_unknown(&notetype.config);
    notetype.fields[0].config = with_unknown(&notetype.fields[0].config);
    notetype.templates[0].config = with_unknown(&notetype.templates[0].config);
    put_as_given(&h, notetype);
    let mut draft = edit(&h, id);
    draft.config.css = "z".to_owned();
    save(&h, draft).unwrap();
    let stored = read(&h, id);
    assert_eq!(stored.config.css, "z");
    assert_eq!(stored.config.latex_pre, "\\documentclass{minimal}");
    assert!(stored.config.latex_svg);
    assert!(has_unknown(stored.config.unknown_fields()));
    assert!(has_unknown(stored.fields[0].config.unknown_fields()));
    assert!(has_unknown(stored.templates[0].config.unknown_fields()));
}

#[test]
fn a_clone_keeps_field_and_template_ids_and_clears_the_original_id() {
    // N7 (R905, R916).
    let h = Harness::open();
    no_notetypes(&h);
    let id = add(&h, pair("Pair")).unwrap().id;
    let mut source = read(&h, id);
    source.config.original_id = Some(42);
    source.templates[0].mtime = EpochSecs(123);
    source.templates[0].usn = Usn(7);
    source.fields[1].config = with_unknown(&source.fields[1].config);
    put_as_given(&h, source);
    let source = read(&h, id);
    h.clock.step_ms(5_000);
    let cloned = notetypes::clone_notetype(&h.collection, id, None).unwrap();
    assert_eq!(cloned.undo.map(|u| u.name), Some(ADD_NOTE_TYPE));
    let cloned = cloned.value;
    assert_eq!(cloned.name, "Pair copy");
    assert_ne!(cloned.id, id);
    let copy = read(&h, cloned.id);
    assert_eq!(copy.config.original_id, None);
    assert_eq!((copy.mtime.0, copy.usn), (h.clock.now_ms() / 1000, Usn(-1)));
    let field_ids = |n: &Notetype| n.fields.iter().map(|f| f.config.id).collect::<Vec<_>>();
    let template_ids = |n: &Notetype| n.templates.iter().map(|t| t.config.id).collect::<Vec<_>>();
    assert_eq!(field_ids(&copy), field_ids(&source));
    assert_eq!(template_ids(&copy), template_ids(&source));
    assert!(
        copy.templates
            .iter()
            .all(|t| (t.mtime, t.usn) == (EpochSecs(0), Usn(0)))
    );
    assert!(has_unknown(copy.fields[1].config.unknown_fields()));
    assert_eq!(copy.config.css, source.config.css);
    assert_eq!(copy.config.reqs.len(), 2);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let current = reader.config_entry("curModel").unwrap().unwrap().entry;
    assert_eq!(current.value, cloned.id.0.to_string().into_bytes());
    drop(reader);
    // A second clone of the same name gets '+', a named one keeps its name.
    assert_eq!(
        notetypes::clone_notetype(&h.collection, id, None).unwrap().value.name,
        "Pair copy+"
    );
    assert_eq!(
        notetypes::clone_notetype(&h.collection, id, Some("Twin".to_owned()))
            .unwrap()
            .value
            .name,
        "Twin"
    );
    // A clone is a checked save.
    let broken = broken(&h);
    assert_eq!(
        kind_of(notetypes::clone_notetype(&h.collection, broken, None)),
        "unknown_field"
    );
}

/// Removes every note type of the fixture, so a test sees only its own.
fn no_notetypes(h: &Harness) {
    h.collection
        .run(
            OpSpec::named("test_clear_notetypes", OpSource::UserNonUndoable).skip_undo(),
            |ctx| {
                let ids = notetypes::read::ids(ctx.scope())?;
                let mut report = AuthoredStructureReport::default();
                if !ids.is_empty() {
                    report.reasons.insert(StructureReason::NotetypeRemoved);
                }
                report.removed.extend(ids.iter().map(|id| id.0));
                let token = ctx.scope().start_structure_report(report)?;
                for id in ids {
                    ctx.scope().remove_notetype(id)?;
                }
                ctx.scope().complete_structure_report(token)?;
                Ok(())
            },
        )
        .unwrap();
}

#[test]
fn the_image_occlusion_note_type_is_added_on_demand() {
    // N9 (R913).
    let h = Harness::open();
    no_notetypes(&h);
    let plain = add(&h, pair("Image Occlusion")).unwrap().id;
    let current = || {
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .config_entry("curModel")
            .unwrap()
            .unwrap()
            .entry
            .value
    };
    let selected = current();
    h.clock.step_ms(60_000);
    let ensured = notetypes::ensure_image_occlusion(&h.collection).unwrap();
    assert_eq!(ensured.undo.map(|u| u.name), Some(ADD_NOTE_TYPE));
    let ensured = ensured.value;
    assert!(ensured.written);
    assert_eq!(ensured.name, "Image Occlusion+");
    let stored = read(&h, ensured.id);
    assert_eq!(stored.config.original_stock_kind, 6);
    assert_eq!((stored.mtime.0, stored.usn), (h.clock.now_ms() / 1000, Usn(-1)));
    assert_eq!(current(), selected, "curModel keeps its value");
    assert_ne!(ensured.id, plain);
    // With one present, nothing is written.
    let again = notetypes::ensure_image_occlusion(&h.collection).unwrap();
    assert_eq!((again.value.id, again.value.written), (ensured.id, false));
    assert!(again.changes.is_empty());
    // A kind-6 note type with fewer than 4 fields does not count.
    let old = read(&h, ensured.id);
    let mut short = old.clone();
    short.fields.truncate(3);
    h.collection
        .run(
            OpSpec::named("test_put_notetype", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                let mut report = AuthoredStructureReport::default();
                report.reasons.insert(StructureReason::FieldsChanged);
                // This stock IO fixture keeps its first three field positions
                // and its single template while dropping the fourth field.
                report.layouts.insert(
                    short.id.0,
                    AuthoredLayoutReport {
                        before: LayoutShape::of(&old)?,
                        after: LayoutShape::of(&short)?,
                        field_map: vec![0, 1, 2],
                        template_map: vec![0],
                    },
                );
                let token = ctx.scope().start_structure_report(report)?;
                ctx.scope().put_notetype(&short)?;
                ctx.scope().complete_structure_report(token)?;
                Ok(())
            },
        )
        .unwrap();
    let third = notetypes::ensure_image_occlusion(&h.collection).unwrap().value;
    assert!(third.written);
    assert_eq!(third.name, "Image Occlusion++");
    assert_eq!(
        notetypes::read::image_occlusion(&*h.collection.reader(ReaderRole::Query).unwrap())
            .unwrap()
            .map(|n| (n.id, n.config.original_stock_kind)),
        Some((third.id, StockKind::ImageOcclusion.stored()))
    );
}

#[test]
fn opening_a_collection_writes_no_note_type() {
    // N10 (R1586, R1590): open and close leave every note type row and
    // col.scm as they were.
    let h = Harness::open();
    let ids = notetypes::read::ids(&*h.collection.reader(ReaderRole::Query).unwrap()).unwrap();
    assert!(!ids.is_empty(), "the fixture holds note types");
    let snapshot = |h: &Harness| -> (Vec<Vec<RowImage>>, i64) { (ids.iter().map(|id| rows(h, *id)).collect(), scm(h)) };
    let before = snapshot(&h);
    let Harness {
        dir,
        path,
        engine,
        collection,
        ..
    } = h;
    drop(collection);
    drop(engine);
    let reopened = Harness::open_path(dir, path);
    reopened.clock.step_ms(86_400_000);
    assert_eq!(snapshot(&reopened), before);
}
