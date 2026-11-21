//! Find and Replace (`specs/browser/find-replace.md` F1 to F11) and the bulk
//! framework it runs on (`specs/browser/bulk-framework.md` K1 to K6), beyond
//! what the conformance scenarios show: one undo step that restores every
//! row exactly, the refusals in F11's order with nothing written and both
//! histories kept, cancellation before the commit, a failure inside the
//! write phase, a note edited between the compute phase and the commit,
//! the unchanged-match rule (F6 step 6, F7 step 5), the job and its result,
//! a search-handle selection as target, and `selection_summary` (F3).

mod common;

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use common::Harness;
use meguri_engine::browse::search::{SearchEvent, SearchListener, SearchMode, SortDirection, SortSpec};
use meguri_engine::bulk::find_replace::{self, Edit, FindIn, FindReplace, Preview, Refusal};
use meguri_engine::bulk::summary::{SelectionSummary, selection_summary};
use meguri_engine::bulk::{BulkCounts, NoteTarget};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind};
use meguri_engine::{
    CancelToken, Collection, EngineError, EventKind, JobOutcome, OpResult, OpSpec, Progress, ProgressListener,
    ProgressPhase, ProgressSink, UndoScope, notes,
};
use meguri_model::changes::OpSource;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_storage::{Cell, ConfigValue, ReadRows, ReaderRole, RowKey, StampMode, Table};

// --- Helpers ---------------------------------------------------------------

fn made(h: &Harness, name: &str, kind: NotetypeKind, fields: &[&str], fronts: &[&str]) -> NotetypeId {
    let cloze = kind == NotetypeKind::Cloze;
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: name.to_owned(),
            kind,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            templates: fronts
                .iter()
                .enumerate()
                .map(|(i, front)| NewTemplate {
                    name: format!("Card {}", i + 1),
                    front: (*front).to_owned(),
                    back: if cloze { (*front).to_owned() } else { String::new() },
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

/// Vocab: Word, Meaning; one template `{{Word}}`.
fn vocab(h: &Harness) -> NotetypeId {
    made(h, "Vocab", NotetypeKind::Standard, &["Word", "Meaning"], &["{{Word}}"])
}

/// Plain: Text, Extra; templates `{{Text}}` and `{{#Extra}}{{Text}}{{/Extra}}`.
fn plain(h: &Harness) -> NotetypeId {
    made(
        h,
        "Plain",
        NotetypeKind::Standard,
        &["Text", "Extra"],
        &["{{Text}}", "{{#Extra}}{{Text}}{{/Extra}}"],
    )
}

fn gaps(h: &Harness) -> NotetypeId {
    made(h, "Gaps", NotetypeKind::Cloze, &["Text", "Back Extra"], &["{{cloze:Text}}"])
}

fn add(h: &Harness, mid: NotetypeId, fields: &[&str], tags: &[&str]) -> NoteId {
    notes::add(
        &h.collection,
        notes::AddNote {
            notetype_id: mid,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            tags: tags.iter().map(|s| (*s).to_owned()).collect(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value
    .id
}

/// A request whose confirmation admits every count; [`run_on`] passes the
/// preview's count instead.
fn request(find: &str, replace: &str, within: FindIn) -> FindReplace {
    FindReplace {
        find: find.to_owned(),
        replace: replace.to_owned(),
        regex: false,
        ignore_case: true,
        whole_words: false,
        within,
        confirmed: u64::MAX,
    }
}

fn field(name: &str) -> FindIn {
    FindIn::Field(name.to_owned())
}

fn preview_of(c: &Collection, target: &NoteTarget, request: &FindReplace) -> Result<Preview, EngineError> {
    find_replace::preview(c, target, request, &CancelToken::new(), &ProgressSink::new())
}

/// The sheet's sequence: the preview, then the run confirmed with the
/// preview's count. On the same rows the preview's counts are the run's,
/// and a refused preview is refused the same way by the run. (Only an
/// injected write-phase failure lets a run fail after a preview passed.)
fn run_on(
    c: &Collection,
    target: NoteTarget,
    request: &FindReplace,
) -> Result<OpResult<BulkCounts>, EngineError> {
    let preview = preview_of(c, &target, request);
    let confirmed = FindReplace {
        confirmed: preview.as_ref().map_or(u64::MAX, |shown| shown.changed),
        ..request.clone()
    };
    let result = find_replace::run(c, &target, &confirmed, CancelToken::new(), ProgressSink::new());
    match (&preview, &result) {
        (Ok(shown), Ok(done)) => assert_eq!(
            (shown.changed, shown.considered),
            (done.value.affected, done.value.considered),
            "the preview counts what the run writes"
        ),
        (Err(refused), Err(error)) => assert_eq!(refused, error, "the preview refuses as the run does"),
        (Err(refused), Ok(_)) => panic!("the preview refused ({refused:?}) a run that commits"),
        // An injected failure; never the confirmation itself, which would
        // mean the preview counted fewer notes than the run writes.
        (Ok(_), Err(error)) => assert!(
            !matches!(error, EngineError::FindReplace(Refusal::CountChanged { .. })),
            "the run refused the preview's own count: {error:?}"
        ),
    }
    result
}

fn run(h: &Harness, notes: &[NoteId], request: &FindReplace) -> Result<BulkCounts, EngineError> {
    run_on(&h.collection, NoteTarget::Notes(notes.to_vec()), request).map(|r| r.value)
}

fn counts(affected: u64, considered: u64) -> BulkCounts {
    BulkCounts { affected, considered }
}

fn note(h: &Harness, id: NoteId) -> meguri_model::note::Note {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(id)
        .unwrap()
        .expect("the note exists")
}

fn fields_of(h: &Harness, id: NoteId) -> Vec<String> {
    note(h, id).field_values().map(str::to_owned).collect()
}

fn card_count(h: &Harness, id: NoteId) -> usize {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .cards_of_note(id)
        .unwrap()
        .len()
}

fn undo_name(h: &Harness) -> Option<String> {
    h.collection
        .peek_undo(UndoScope::Global)
        .unwrap()
        .name()
        .map(str::to_owned)
}

fn redo_name(h: &Harness) -> Option<String> {
    h.collection
        .peek_redo(UndoScope::Global)
        .unwrap()
        .name()
        .map(str::to_owned)
}

/// Every row of the tables an op of this file can change, `col` included:
/// equal images mean the collection is as it was.
#[derive(Debug, PartialEq)]
struct Rows {
    notes: Vec<Vec<Cell>>,
    cards: Vec<Vec<Cell>>,
    tags: Vec<Vec<Cell>>,
    config: Vec<Vec<Cell>>,
    col: Vec<Cell>,
    graves: u64,
}

fn table(r: &impl ReadRows, table: Table) -> Vec<Vec<Cell>> {
    let mut out: Vec<Vec<Cell>> = Vec::new();
    let mut after: Option<RowKey> = None;
    loop {
        let page = r.row_page(table, after.as_ref()).unwrap();
        let Some(last) = page.last() else {
            return out;
        };
        after = Some(match &last[0] {
            Cell::Integer(id) => RowKey::Id(*id),
            Cell::Text(bytes) => RowKey::Text(bytes.clone()),
            other => panic!("unexpected key {other:?}"),
        });
        out.extend(page);
    }
}

fn rows(h: &Harness) -> Rows {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    use meguri_storage::Record;
    Rows {
        notes: table(&*r, Table::Notes),
        cards: table(&*r, Table::Cards),
        tags: table(&*r, Table::Tags),
        config: table(&*r, Table::Config),
        col: r.col().unwrap().to_cells(),
        graves: r.row_count(Table::Graves).unwrap(),
    }
}

/// Moves the clock a minute and marks the rows sent (usn 0), so a write
/// shows a new `mod` and `usn` -1, without recording an undo step.
fn settle(h: &Harness) {
    h.clock.step_ms(60_000);
    h.collection
        .run(
            OpSpec::named("settle", OpSource::Rollover).stamp(StampMode::Raw),
            |ctx| {
                let scope = ctx.scope();
                let ids = scope.ids_after(Table::Notes, None, usize::MAX)?;
                for id in ids {
                    let mut n = scope.note(NoteId(id))?.unwrap();
                    n.usn = meguri_model::usn::Usn(0);
                    scope.put(&n)?;
                }
                Ok(())
            },
        )
        .unwrap();
}

/// Writes a note row as stored, bypassing every save rule (a damaged or
/// foreign row).
fn raw_note(h: &Harness, id: NoteId, edit: impl FnOnce(&mut meguri_model::note::Note) + Send + 'static) {
    h.collection
        .run(OpSpec::named("raw_edit", OpSource::UserUndoable), move |ctx| {
            let mut n = ctx.scope().note(id)?.unwrap();
            edit(&mut n);
            ctx.scope().put(&n)?;
            Ok(())
        })
        .unwrap();
}

/// One raw statement on the writer, outside any op (a row no op may write:
/// a note of a note type that does not exist).
fn raw_sql(h: &Harness, sql: String) {
    h.collection
        .submit_maintenance(
            meguri_engine::executor::catalog::ANALYZE,
            Duration::from_millis(10),
            move |w| {
                w.execute_raw(&sql, &[])?;
                Ok(())
            },
        )
        .wait()
        .unwrap();
}

fn set_normalize(h: &Harness, on: bool) {
    h.collection
        .run(
            OpSpec::named("set_config", OpSource::Rollover).stamp(StampMode::Raw),
            move |ctx| Ok(ctx.scope().set_config("normalize_note_text", &ConfigValue::Bool(on))?),
        )
        .unwrap();
}

fn refusal(error: &EngineError) -> &Refusal {
    match error {
        EngineError::FindReplace(refusal) => refusal,
        other => panic!("expected a Find and Replace refusal, got {other:?}"),
    }
}

fn is_invalid_argument(error: &EngineError) -> bool {
    matches!(error, EngineError::InvalidArgument { .. })
}

/// A progress listener that runs `each` on every delivered update.
struct Listener<F: Fn(&Progress) + Send + Sync>(F);

impl<F: Fn(&Progress) + Send + Sync> ProgressListener for Listener<F> {
    fn on_progress(&self, progress: &Progress) {
        (self.0)(progress)
    }
}

/// A clock for progress sinks that moves a second on every read, so no
/// report is throttled away.
fn zip_time() -> Duration {
    static NOW: AtomicU64 = AtomicU64::new(0);
    Duration::from_secs(NOW.fetch_add(1, Ordering::SeqCst))
}

// --- AE47, F6, F9: the count, untouched rows, one undo step ------------------

/// AE47: ten notes of two note types, four with a `Meaning` field and three
/// of those holding `cost`. Literal `cost` → `$5` in `Meaning`: three
/// notes written ({3 of 10}), `$5` stored literally, the other seven keep
/// `mod` and `usn`; one "Find and Replace" step, and one undo gives back
/// every row and `col.mod`.
#[test]
fn ae47_three_of_ten_and_one_undo_restores_every_row() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let plain = plain(&h);
    let mut ids = vec![
        add(&h, vocab, &["rate", "the cost of it"], &[]),
        add(&h, vocab, &["charge", "Cost cost"], &[]),
        add(&h, vocab, &["cost", "worth"], &[]),
        add(&h, vocab, &["fee", "a COST"], &[]),
    ];
    for i in 0..6 {
        ids.push(add(&h, plain, &[&format!("cost here {i}"), "x"], &[]));
    }
    settle(&h);
    let before = rows(&h);
    let result = run_on(
        &h.collection,
        NoteTarget::Notes(ids.clone()),
        &request("cost", "$5", field("meaning")),
    )
    .unwrap();
    assert_eq!(result.value, counts(3, 10));
    let undo = result.undo.expect("one step");
    assert_eq!(undo.name, "Find and Replace");
    assert_eq!(undo_name(&h).as_deref(), Some("Find and Replace"));
    assert_eq!(fields_of(&h, ids[0]), ["rate", "the $5 of it"]);
    assert_eq!(fields_of(&h, ids[1]), ["charge", "$5 $5"]);
    assert_eq!(fields_of(&h, ids[2]), ["cost", "worth"]);
    assert_eq!(fields_of(&h, ids[3]), ["fee", "a $5"]);
    let after = rows(&h);
    for (i, (b, a)) in before.notes.iter().zip(&after.notes).enumerate() {
        let written = [0, 1, 3].contains(&i);
        assert_eq!(b == a, !written, "note {i}");
        if written {
            // mod (column 3) moves, usn (column 4) is -1.
            assert_ne!(b[3], a[3]);
            assert_eq!(a[4], Cell::Integer(-1));
        }
    }
    assert_ne!(before.col, after.col, "col.mod moves");
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), before, "one undo restores every row and col.mod");
    assert_eq!(redo_name(&h).as_deref(), Some("Find and Replace"));
}

/// The data-safety fixture: field text with markup, entities, media
/// references, cloze deletions and Unicode; regular expressions with groups;
/// Match Case on and off; the Tags target. Every run is one step, and its
/// undo gives back every row exactly (cards and tag rows the run added go
/// too).
#[test]
fn every_kind_of_run_is_one_step_that_undo_restores_exactly() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let gaps = gaps(&h);
    let markup = add(
        &h,
        vocab,
        &["<b>Cost</b>&nbsp;of <i>it</i>", "<img src=\"cost.png\"> [sound:cost.mp3]"],
        &["Zorb::Quill", "plinth"],
    );
    let cloze = add(&h, gaps, &["{{c1::vorp}} ka {{c1::cost}}", "Ülvïrë 😀 cost"], &["plinth"]);
    let unicode = add(&h, vocab, &["glaßo", "brïmé 12kg"], &["Zorb::Quill"]);
    let all = vec![markup, cloze, unicode];
    settle(&h);
    let runs: Vec<(FindReplace, u64, &str)> = vec![
        // Literal, Match Case off: markup and entities are matched as text.
        (request("&nbsp;", " ", FindIn::AllFields), 1, "Find and Replace"),
        // Regex with groups, Match Case on.
        (
            FindReplace {
                regex: true,
                ignore_case: false,
                ..request(r"(\d+)kg", "${1} kilograms", FindIn::AllFields)
            },
            1,
            "Find and Replace",
        ),
        // Media references rewritten as text.
        (
            FindReplace {
                regex: true,
                ..request(r#"src="(\w+)\.png""#, r#"src="${1}.jpg""#, FindIn::AllFields)
            },
            1,
            "Find and Replace",
        ),
        // A cloze number changed: a card is added.
        (request("{{c1::cost}}", "{{c2::cost}}", field("Text")), 1, "Find and Replace"),
        // Match Case on misses the capitalised word.
        (
            FindReplace {
                ignore_case: false,
                ..request("cost", "price", FindIn::AllFields)
            },
            2,
            "Find and Replace",
        ),
        // Tags: merged into the registered spelling, a duplicate merged.
        (request("plinth", "Zorb::quill", FindIn::Tags), 2, "Update Tag"),
        // Unicode in a regex class.
        (
            FindReplace {
                regex: true,
                ..request(r"[ïé]", "?", FindIn::AllFields)
            },
            2,
            "Find and Replace",
        ),
    ];
    for (i, (req, affected, step)) in runs.iter().enumerate() {
        let before = rows(&h);
        let undo_before = undo_name(&h);
        let result = run_on(&h.collection, NoteTarget::Notes(all.clone()), req).unwrap();
        assert_eq!(result.value, counts(*affected, 3), "run {i}");
        assert_eq!(undo_name(&h).as_deref(), Some(*step), "run {i}");
        assert_ne!(rows(&h), before, "run {i} writes");
        h.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(rows(&h), before, "run {i}: one undo restores every row exactly");
        assert_eq!(undo_name(&h), undo_before, "run {i}");
    }
    // The cloze run did add a card, and Tags registered no new row for an
    // existing spelling.
    let before = rows(&h);
    run(&h, &all, &request("{{c1::cost}}", "{{c2::cost}}", field("Text"))).unwrap();
    assert_eq!(card_count(&h, cloze), 2);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(card_count(&h, cloze), 1);
    assert_eq!(rows(&h), before);
    run(&h, &all, &request("plinth", "Zorb::quill", FindIn::Tags)).unwrap();
    assert_eq!(note(&h, markup).tags, " Zorb::Quill ", "merged into the registered spelling");
    assert_eq!(note(&h, cloze).tags, " Zorb::Quill ");
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), before);
}

// --- F6: fields ---------------------------------------------------------------

/// F6 steps 4 and 5: a rewritten note is normalized and stamped, its tags
/// re-joined with canonical spacing (not sorted, not registered); the cards
/// it now requires are added; a field emptied deletes no card.
#[test]
fn f6_saves_respace_tags_add_cards_and_delete_none() {
    let h = common::fresh();
    let plain = plain(&h);
    let a = add(&h, plain, &["alpha", ""], &[]);
    let b = add(&h, plain, &["beta", "keep"], &[]);
    raw_note(&h, a, |n| n.tags = "  Zed\u{3000}A  ".into());
    assert_eq!(card_count(&h, a), 1);
    assert_eq!(card_count(&h, b), 2);
    let tags_before = rows(&h).tags;
    let done = run(
        &h,
        &[a, b],
        &FindReplace {
            regex: true,
            ..request("^$", "filled", field("Extra"))
        },
    )
    .unwrap();
    assert_eq!(done, counts(1, 2));
    assert_eq!(fields_of(&h, a), ["alpha", "filled"]);
    assert_eq!(note(&h, a).tags, " Zed A ", "spacing canonical, order and case kept");
    assert_eq!(rows(&h).tags, tags_before, "nothing registered");
    assert_eq!(card_count(&h, a), 2, "the second card is added");
    run(&h, &[b], &request("keep", "", field("Extra"))).unwrap();
    assert_eq!(fields_of(&h, b), ["beta", ""]);
    assert_eq!(card_count(&h, b), 2, "no card is deleted");
}

/// F6 step 2: a field name under `unicase`; a note type without the field
/// is skipped; a name no note type has is not an error.
#[test]
fn f6_named_field_unicase_and_skips() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let plain = plain(&h);
    let v = add(&h, vocab, &["cost", "cost"], &[]);
    let p = add(&h, plain, &["cost", "cost"], &[]);
    assert_eq!(run(&h, &[v, p], &request("cost", "x", field("MEANING"))).unwrap(), counts(1, 2));
    assert_eq!(fields_of(&h, v), ["cost", "x"]);
    assert_eq!(fields_of(&h, p), ["cost", "cost"]);
    assert_eq!(run(&h, &[v, p], &request("cost", "x", field("Nowhere"))).unwrap(), counts(0, 2));
}

/// F6 step 6 (deviation `find-replace-unchanged-match`): a match whose
/// rewrite leaves the note as stored writes nothing and counts nothing, in
/// fields and in tags; no step is recorded.
#[test]
fn f6_f7_unchanged_matches_are_not_written() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let n = add(&h, vocab, &["a x", "b"], &["kept"]);
    settle(&h);
    let before = rows(&h);
    let undo = undo_name(&h);
    for req in [
        request("a", "a", FindIn::AllFields),
        FindReplace {
            regex: true,
            ..request("(x)", "$1", FindIn::AllFields)
        },
        request("", "", field("Word")),
        request("kept", "kept", FindIn::Tags),
    ] {
        assert_eq!(run(&h, &[n], &req).unwrap(), counts(0, 1), "{req:?}");
        assert_eq!(rows(&h), before, "{req:?}");
        assert_eq!(undo_name(&h), undo, "{req:?}");
    }
}

/// F4 step 4 with the key on and off: a decomposed literal finds the
/// composed text only when `normalize_note_text` is on, and then the save
/// composes the replaced field; with Tags there is no NFC step.
#[test]
fn f4_nfc_with_the_key_on_and_off() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let n = add(&h, vocab, &["café", "x"], &["café"]);
    let decomposed = "cafe\u{301}";
    set_normalize(&h, false);
    assert_eq!(run(&h, &[n], &request(decomposed, "tea", field("Word"))).unwrap(), counts(0, 1));
    set_normalize(&h, true);
    assert_eq!(run(&h, &[n], &request(decomposed, "tea", field("Word"))).unwrap(), counts(1, 1));
    assert_eq!(fields_of(&h, n), ["tea", "x"]);
    assert_eq!(run(&h, &[n], &request(decomposed, "tea", FindIn::Tags)).unwrap(), counts(0, 1));
}

/// F8: the all-notes target reads every note; its considered count is the
/// notes read. A deleted note in a list is skipped but considered.
#[test]
fn f8_all_notes_and_deleted_ids() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["none", "x"], &[]);
    let c = add(&h, vocab, &["cost", "y"], &[]);
    let total = h.collection.reader(ReaderRole::Query).unwrap().row_count(Table::Notes).unwrap();
    let done = run_on(&h.collection, NoteTarget::AllNotes, &request("cost", "fee", FindIn::AllFields)).unwrap();
    assert_eq!(done.value, counts(2, total));
    notes::delete(&h.collection, vec![c], OpSource::UserUndoable).unwrap();
    assert_eq!(run(&h, &[a, b, c], &request("fee", "cost", field("Word"))).unwrap(), counts(1, 3));
}

// --- F7: tags ------------------------------------------------------------------

/// F7 steps 3 and 4: per-tag matching with anchors, empty results dropped,
/// T2 and T3 (spelling from the registry and registration), duplicates
/// merged, sorted; a note without tags never matches, also with an empty
/// find text; fields are not touched.
#[test]
fn f7_tags_rewrite_spell_and_register() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["one", "x"], &["keep", "old::x"]);
    let b = add(&h, vocab, &["two", "x"], &["new::x", "old::x"]);
    let c = add(&h, vocab, &["three", "x"], &[]);
    let d = add(&h, vocab, &["four", "x"], &["drop", "stay"]);
    let fields_before: Vec<_> = [a, b, c, d].iter().map(|n| note(&h, *n).fields).collect();
    assert_eq!(run(&h, &[a, b, c], &request("old", "new", FindIn::Tags)).unwrap(), counts(2, 3));
    assert_eq!(note(&h, a).tags, " keep new::x ");
    assert_eq!(note(&h, b).tags, " new::x ");
    assert_eq!(run(&h, &[d], &request("drop", "", FindIn::Tags)).unwrap(), counts(1, 1));
    assert_eq!(note(&h, d).tags, " stay ");
    assert_eq!(
        run(&h, &[c], &request("", "x", FindIn::Tags)).unwrap(),
        counts(0, 1),
        "no tags: no match"
    );
    // A new name is registered once, spelled from the registry.
    assert_eq!(run(&h, &[d], &request("stay", "Keep::Fresh", FindIn::Tags)).unwrap(), counts(1, 1));
    assert_eq!(note(&h, d).tags, " keep::Fresh ", "the registered parent lends its spelling");
    let registered = h.collection.reader(ReaderRole::Query).unwrap().tag("keep::Fresh").unwrap();
    assert!(registered.is_some());
    let after: Vec<_> = [a, b, c, d].iter().map(|n| note(&h, *n).fields).collect();
    assert_eq!(after, fields_before, "Tags never touches fields");
}

// --- F9, F11: refusals, no match, histories ------------------------------------

/// F11's refusals, each before anything is written, in F11's order; after
/// each the rows are unchanged and both histories as they were.
#[test]
fn f11_refusals_write_nothing_and_keep_both_histories() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &["t"]);
    let b = add(&h, vocab, &["other", "x"], &[]);
    // An undo step and a redo step to keep.
    run(&h, &[a], &request("x", "y", FindIn::AllFields)).unwrap();
    run(&h, &[a], &request("y", "z", FindIn::AllFields)).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    let before = rows(&h);
    let (undo, redo) = (undo_name(&h), redo_name(&h));
    assert!(undo.is_some() && redo.is_some());
    let check = |error: EngineError, what: &str| {
        assert_eq!(rows(&h), before, "{what}: nothing written");
        assert_eq!((undo_name(&h), redo_name(&h)), (undo.clone(), redo.clone()), "{what}: histories kept");
        error
    };
    let fr = |notes: &[NoteId], req: FindReplace| run(&h, notes, &req).unwrap_err();
    assert!(is_invalid_argument(&check(fr(&[], request("cost", "x", FindIn::AllFields)), "empty list")));
    assert!(is_invalid_argument(&check(fr(&[a, b, a], request("cost", "x", FindIn::AllFields)), "repeated")));
    assert!(is_invalid_argument(&check(fr(&[a], request("cost", "x", field(""))), "empty field name")));
    // InvalidArgument comes before the separator and the pattern.
    assert!(is_invalid_argument(&check(fr(&[a, a], request("(", "a b", FindIn::Tags)), "order")));
    for space in ["a b", "a\u{3000}b"] {
        let e = check(fr(&[a], request("t", space, FindIn::Tags)), "separator");
        assert_eq!(refusal(&e), &Refusal::SeparatorInReplacement);
    }
    // The separator comes before the pattern.
    let e = check(fr(&[a], FindReplace { regex: true, ..request("(", "a b", FindIn::Tags) }), "order");
    assert_eq!(refusal(&e), &Refusal::SeparatorInReplacement);
    for within in [FindIn::AllFields, field("Word"), FindIn::Tags] {
        let e = check(
            fr(&[a], FindReplace { regex: true, ..request("(", "x", within.clone()) }),
            "invalid regex",
        );
        assert!(matches!(refusal(&e), Refusal::InvalidRegex { message } if !message.is_empty()), "{e:?}");
    }
    // A replacement referring to a group the pattern does not have is not a
    // refusal: the reference becomes nothing (F5).
    assert_eq!(
        run(&h, &[b], &FindReplace { regex: true, ..request("(oth)er", "$2-${name}", FindIn::AllFields) })
            .unwrap(),
        counts(1, 1)
    );
    assert_eq!(fields_of(&h, b), ["-", "x"]);
}

/// F6 step 7: a matched note whose stored field count differs from its note
/// type's refuses the whole run, also when the match is in a value beyond
/// the note type's count; an unmatched damaged note does not. A note of a
/// missing note type refuses whether or not it would match; Tags reads no
/// note type and has no such refusal. The first in F6 step 1's order wins.
#[test]
fn f6_damaged_notes_refuse_in_step_one_order() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &["t"]);
    let damaged = add(&h, vocab, &["plain", "x"], &[]);
    raw_note(&h, damaged, |n| n.fields = "plain\u{1f}x\u{1f}cost".into());
    let before = rows(&h);
    let e = run(&h, &[a, damaged], &request("cost", "fee", FindIn::AllFields)).unwrap_err();
    assert_eq!(refusal(&e), &Refusal::FieldCountMismatch { note: damaged });
    assert_eq!(rows(&h), before);
    // Two damaged notes of one note type: the lower note id comes first,
    // whatever the list's order.
    let second = add(&h, vocab, &["plain", "x"], &[]);
    raw_note(&h, second, |n| n.fields = "cost".into());
    let e = run(&h, &[second, damaged], &request("cost", "fee", FindIn::AllFields)).unwrap_err();
    assert_eq!(refusal(&e), &Refusal::FieldCountMismatch { note: damaged });
    raw_note(&h, second, |n| n.fields = "plain\u{1f}x".into());
    // The damaged note does not match: the run goes on.
    assert_eq!(
        run(&h, &[a, damaged], &request("cost", "fee", field("Word"))).unwrap(),
        counts(1, 2)
    );
    // A note whose note type does not exist.
    let orphan = add(&h, vocab, &["nothing", "here"], &["t"]);
    raw_sql(&h, format!("UPDATE notes SET mid = 1 WHERE id = {}", orphan.0));
    let before = rows(&h);
    let e = run(&h, &[a, orphan], &request("zzz", "fee", FindIn::AllFields)).unwrap_err();
    assert_eq!(refusal(&e), &Refusal::NotetypeMissing { note: orphan });
    assert_eq!(rows(&h), before);
    // Both kinds: the lower note type id comes first (id 1 below Vocab's).
    let e = run(&h, &[damaged, orphan], &request("cost", "fee", FindIn::AllFields)).unwrap_err();
    assert_eq!(refusal(&e), &Refusal::NotetypeMissing { note: orphan });
    assert_eq!(run(&h, &[a, orphan], &request("t", "u", FindIn::Tags)).unwrap(), counts(2, 2));
}

/// F9: a run that writes nothing records no step and keeps `col.mod`; the
/// earlier step stays undoable; the redo history is emptied (A2).
#[test]
fn f9_no_match_records_nothing_and_empties_redo() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &["t"]);
    run(&h, &[a], &request("t", "u", FindIn::Tags)).unwrap();
    run(&h, &[a], &request("cost", "fee", FindIn::AllFields)).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(redo_name(&h).as_deref(), Some("Find and Replace"));
    let before = rows(&h);
    let result = run_on(&h.collection, NoteTarget::Notes(vec![a]), &request("nothing", "x", FindIn::AllFields)).unwrap();
    assert_eq!(result.value, counts(0, 1));
    assert!(result.undo.is_none());
    assert_eq!(rows(&h), before, "col.mod kept");
    assert_eq!(undo_name(&h).as_deref(), Some("Update Tag"));
    assert_eq!(redo_name(&h), None);
}

// --- K3, K4: cancellation, failure, concurrent edits ------------------------------

/// K4: a cancel before the run, and one during the compute phase, write
/// nothing and record no step.
#[test]
fn a_cancel_before_the_commit_writes_nothing() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let inputs = (0..2_500)
        .map(|i| notes::AddNote {
            notetype_id: vocab,
            fields: vec![format!("cost {i}"), "x".to_owned()],
            tags: Vec::new(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        })
        .collect();
    let ids: Vec<NoteId> = notes::bulk_add(&h.collection, inputs, OpSource::UserUndoable)
        .unwrap()
        .value
        .into_iter()
        .map(|added| added.id)
        .collect();
    let before = rows(&h);
    let undo = undo_name(&h);
    let token = CancelToken::new();
    token.cancel();
    let e = find_replace::run(
        &h.collection,
        &NoteTarget::Notes(ids.clone()),
        &request("cost", "fee", FindIn::AllFields),
        token,
        ProgressSink::new(),
    )
    .unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert_eq!(rows(&h), before);
    // Cancelled from the first progress report of the compute phase.
    let token = CancelToken::new();
    let progress = ProgressSink::with_clock(zip_time);
    let cancel = Arc::clone(&token);
    progress.set_listener(Arc::new(Listener(move |p: &Progress| {
        if p.phase == ProgressPhase::Planning {
            cancel.cancel();
        }
    })));
    let e = find_replace::run(
        &h.collection,
        &NoteTarget::AllNotes,
        &request("cost", "fee", FindIn::AllFields),
        token,
        progress,
    )
    .unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert_eq!(rows(&h), before);
    assert_eq!(undo_name(&h), undo);
    // The same run then commits, as the catalog's bulk op with a declared
    // write (2,500 notes); undo and redo give back each side exactly.
    let subscription = h.engine.stream().subscribe();
    let done = run(&h, &ids, &request("cost", "fee", FindIn::AllFields)).unwrap();
    assert_eq!(done, counts(2_500, 2_500));
    let delays: Vec<&'static str> = common::drain(&subscription, Duration::from_millis(200))
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::WriteDelay { op, .. } => Some(*op),
            _ => None,
        })
        .collect();
    assert_eq!(delays, ["bulk_op"]);
    let after = rows(&h);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), before, "undo of the long write");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h).notes, after.notes, "redo writes the same rows");
}

/// O5: a cancel requested once the write phase started is ignored: the run
/// commits and reports that it was past the cancel point.
#[test]
fn a_cancel_during_the_commit_is_ignored() {
    static TOKEN: Mutex<Option<Arc<CancelToken>>> = Mutex::new(None);
    fn cancel_now() {
        if let Some(token) = TOKEN.lock().unwrap().as_ref() {
            token.cancel();
        }
    }
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let token = CancelToken::new();
    *TOKEN.lock().unwrap() = Some(Arc::clone(&token));
    meguri_model::faultpoint::arm("bulk.find_replace.write_phase", meguri_model::faultpoint::Fault::Callback(cancel_now));
    let result = find_replace::run(
        &h.collection,
        &NoteTarget::Notes(vec![a]),
        &request("cost", "fee", FindIn::AllFields),
        Arc::clone(&token),
        ProgressSink::new(),
    );
    meguri_model::faultpoint::disarm_all();
    let result = result.unwrap();
    assert!(token.is_cancelled());
    assert!(result.past_cancel_point);
    assert_eq!(result.value, counts(1, 1));
    assert_eq!(fields_of(&h, a), ["fee", "x"]);
}

/// F6 steps 1 and 5: a note that needs a new position takes the next
/// counter value, one per note, in ascending note type id then note id,
/// whatever the list's order and the notes' own ids.
#[test]
fn f6_new_cards_take_positions_in_step_one_order() {
    let h = common::fresh();
    let first = plain(&h);
    let second = made(
        &h,
        "Plain2",
        NotetypeKind::Standard,
        &["Text", "Extra"],
        &["{{Text}}", "{{#Extra}}{{Text}}{{/Extra}}"],
    );
    assert!(first < second);
    // The note of the higher note type has the lower note id.
    let late = add(&h, second, &["b", ""], &[]);
    let early = add(&h, first, &["a", ""], &[]);
    assert!(late < early);
    // Their only cards are reviews: no new sibling lends a position (P2).
    raw_sql(
        &h,
        format!("UPDATE cards SET type = 2, queue = 2 WHERE nid IN ({}, {})", late.0, early.0),
    );
    let counter = |h: &Harness| match h.collection.reader(ReaderRole::Query).unwrap().config_value("nextPos").unwrap() {
        Some(value) => value.as_i64().unwrap(),
        None => 0,
    };
    let next = counter(&h);
    let done = run(
        &h,
        &[late, early],
        &FindReplace {
            regex: true,
            ..request("^$", "filled", field("Extra"))
        },
    )
    .unwrap();
    assert_eq!(done, counts(2, 2));
    let new_due = |note: NoteId| {
        let cards = h.collection.reader(ReaderRole::Query).unwrap().cards_of_note(note).unwrap();
        let card = cards.iter().find(|c| c.ord == 1).expect("the added card");
        i64::from(card.due)
    };
    assert_eq!(new_due(early), next, "the lower note type first");
    assert_eq!(new_due(late), next + 1);
    assert_eq!(counter(&h), next + 2);
}

/// F7 steps 4 and 5, F9: a new tag row registered by a run goes with its
/// undo; a stored tag no row registers, replaced by itself, comes out as
/// stored: count 0, no step, no row registered.
#[test]
fn f7_registered_rows_go_with_undo_and_unchanged_notes_register_nothing() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["one", "x"], &["keep"]);
    let b = add(&h, vocab, &["two", "x"], &[]);
    raw_note(&h, b, |n| n.tags = " loose ".into());
    let before = rows(&h);
    let undo = undo_name(&h);
    assert_eq!(run(&h, &[b], &request("loose", "loose", FindIn::Tags)).unwrap(), counts(0, 1));
    assert_eq!(rows(&h), before, "nothing registered or written");
    assert_eq!(undo_name(&h), undo);
    assert_eq!(run(&h, &[a], &request("keep", "Fresh::Leaf", FindIn::Tags)).unwrap(), counts(1, 1));
    assert!(h.collection.reader(ReaderRole::Query).unwrap().tag("Fresh::Leaf").unwrap().is_some());
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), before, "the registered row goes with the undo");
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(note(&h, a).tags, " Fresh::Leaf ");
    assert!(h.collection.reader(ReaderRole::Query).unwrap().tag("Fresh::Leaf").unwrap().is_some());
}

/// K4: a failure inside the write phase rolls the whole transaction back:
/// no note written, no card added, no step. The same run then commits.
#[test]
fn a_failure_in_the_write_phase_rolls_everything_back() {
    let h = common::fresh();
    let plain = plain(&h);
    let a = add(&h, plain, &["cost", ""], &[]);
    let b = add(&h, plain, &["cost", "y"], &["t"]);
    let before = rows(&h);
    let undo = undo_name(&h);
    meguri_model::faultpoint::arm(
        "bulk.find_replace.write_phase",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let req = FindReplace {
        regex: true,
        ..request("^$", "filled", field("Extra"))
    };
    let e = run(&h, &[a, b], &req).unwrap_err();
    let tags_e = run(&h, &[b], &request("t", "fresh", FindIn::Tags)).unwrap_err();
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(e, EngineError::Internal { .. }), "{e:?}");
    assert!(matches!(tags_e, EngineError::Internal { .. }), "{tags_e:?}");
    assert_eq!(rows(&h), before, "no note, card or tag row written");
    assert_eq!(undo_name(&h), undo);
    assert_eq!(run(&h, &[a, b], &req).unwrap(), counts(1, 2));
    assert_eq!(card_count(&h, a), 2);
}

/// K3, O7: a note edited after the compute phase read it is not
/// overwritten: the commit's precondition fails, the run plans again on a
/// new snapshot and writes what that one finds.
#[test]
fn a_note_edited_between_the_compute_phase_and_the_commit_is_not_overwritten() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["cost", "y"], &[]);
    h.clock.step_ms(60_000);
    let progress = ProgressSink::with_clock(zip_time);
    let edited = Arc::new(AtomicBool::new(false));
    let commits = Arc::new(AtomicU64::new(0));
    let (collection, flag, count) = (h.collection.clone(), Arc::clone(&edited), Arc::clone(&commits));
    progress.set_listener(Arc::new(Listener(move |p: &Progress| {
        if p.phase == ProgressPhase::Committing {
            // K3: the phase change alone, no counts from the transaction.
            assert_eq!((p.completed, p.total), (0, None));
            count.fetch_add(1, Ordering::SeqCst);
        }
        if p.phase == ProgressPhase::Planning && p.completed == 0 {
            if !flag.swap(true, Ordering::SeqCst) {
                // Another writer's edit, after the snapshot was taken.
                notes::update::update(
                    &collection,
                    notes::update::UpdateNote {
                        id: b,
                        fields: vec!["price".into(), "y".into()],
                        tags: Vec::new(),
                    },
                    OpSource::UserUndoable,
                )
                .unwrap();
            }
        }
    })));
    let done = find_replace::run(
        &h.collection,
        &NoteTarget::Notes(vec![a, b]),
        &request("cost", "fee", field("Word")),
        CancelToken::new(),
        progress,
    )
    .unwrap();
    assert_eq!(commits.load(Ordering::SeqCst), 2, "the first commit was stale: planned again");
    assert_eq!(done.value, counts(1, 2));
    assert_eq!(fields_of(&h, a), ["fee", "x"]);
    assert_eq!(fields_of(&h, b), ["price", "y"], "the edit stands");
}

// --- K2, K5: the job --------------------------------------------------------------

/// K2, K5: the start call returns a handle at once; the job ends with one
/// `JobFinished`, and its result holds the counts, the seq and the step.
/// A refused job fails with the refusal; a job cancelled before it starts
/// is `Cancelled` and writes nothing.
#[test]
fn the_job_reports_its_counts_and_step() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["none", "x"], &[]);
    let subscription = h.engine.stream().subscribe();
    let job = find_replace::start(
        &h.engine,
        h.collection.clone(),
        NoteTarget::Notes(vec![a, b]),
        request("cost", "fee", FindIn::AllFields),
    )
    .unwrap();
    let outcome = job.wait(Duration::from_secs(30)).expect("a result").unwrap();
    assert_eq!(outcome.counts, counts(1, 2));
    assert_eq!(outcome.undo.map(|u| u.name), Some("Find and Replace"));
    assert!(outcome.changes.notes.iter().any(|change| change.id == a));
    let finished = loop {
        let event = subscription.recv_timeout(Duration::from_secs(10)).expect("JobFinished");
        if let EventKind::JobFinished { id, outcome } = &event.kind
            && *id == job.id()
        {
            break outcome.clone();
        }
    };
    assert_eq!(finished, JobOutcome::Succeeded);
    let refused = find_replace::start(
        &h.engine,
        h.collection.clone(),
        NoteTarget::Notes(vec![a]),
        FindReplace {
            regex: true,
            ..request("(", "x", FindIn::AllFields)
        },
    )
    .unwrap();
    let e = refused.wait(Duration::from_secs(30)).expect("a result").unwrap_err();
    assert!(matches!(refusal(&e), Refusal::InvalidRegex { .. }));
    let before = rows(&h);
    let gate = common::Gate::new();
    // Hold the writer so a cancel lands before the job's commit at the latest.
    let blocked = common::block_writer(&h.collection, OpSource::UserNonUndoable, &gate);
    gate.wait_entered();
    let cancelled = find_replace::start(
        &h.engine,
        h.collection.clone(),
        NoteTarget::Notes(vec![a, b]),
        request("fee", "cost", FindIn::AllFields),
    )
    .unwrap();
    cancelled.cancel();
    gate.open();
    blocked.wait().unwrap();
    let e = cancelled.wait(Duration::from_secs(30)).expect("a result").unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert_eq!(rows(&h).notes, before.notes);
}

/// K1: a selection of a search handle, resolved inside the run to the
/// target notes (H12); a stale selection is refused with nothing written.
#[test]
fn a_handle_selection_resolves_to_its_notes() {
    struct Events(Mutex<Vec<SearchEvent>>);
    impl SearchListener for Events {
        fn event(&self, event: SearchEvent) {
            self.0.lock().unwrap().push(event);
        }
    }
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost a", "x"], &[]);
    let b = add(&h, vocab, &["cost b", "x"], &[]);
    let c = add(&h, vocab, &["cost c", "x"], &[]);
    let events = Arc::new(Events(Mutex::new(Vec::new())));
    let session = h.engine.open_search_session(events.clone());
    session.start_search("", SearchMode::Notes, SortSpec::new("noteFld", SortDirection::Ascending), None);
    // At most 30 s of 5 ms waits.
    let mut waits = 6_000;
    let (handle, revision) = loop {
        let ended = events.0.lock().unwrap().iter().find_map(|event| match event {
            SearchEvent::Complete { handle, revision, .. } => Some(Ok((*handle, *revision))),
            SearchEvent::Failed { .. } | SearchEvent::Cancelled { .. } => Some(Err(format!("{event:?}"))),
            _ => None,
        });
        match ended {
            Some(Ok(found)) => break found,
            Some(Err(event)) => panic!("the search ended without a result: {event}"),
            None => {
                waits -= 1;
                assert!(waits > 0, "the search did not end");
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    let selection = session.selection(handle, revision, vec![0..1, 2..3]);
    // A selection of this collection never acts on another one, even where
    // its note ids name notes there too.
    let other = common::fresh();
    let elsewhere = self::vocab(&other);
    let stranger = add(&other, elsewhere, &["cost q", "x"], &[]);
    let other_before = rows(&other);
    let e = run_on(
        &other.collection,
        NoteTarget::Selection(selection.clone()),
        &request("cost", "fee", FindIn::AllFields),
    )
    .unwrap_err();
    assert!(is_invalid_argument(&e), "{e:?}");
    assert_eq!(rows(&other), other_before);
    assert_eq!(fields_of(&other, stranger)[0], "cost q");
    let done = run_on(
        &h.collection,
        NoteTarget::Selection(selection.clone()),
        &request("cost", "fee", FindIn::AllFields),
    )
    .unwrap();
    assert_eq!(done.value, counts(2, 2));
    assert_eq!(fields_of(&h, a)[0], "fee a");
    assert_eq!(fields_of(&h, b)[0], "cost b");
    assert_eq!(fields_of(&h, c)[0], "fee c");
    drop(session);
    let before = rows(&h);
    let e = run_on(&h.collection, NoteTarget::Selection(selection), &request("fee", "x", FindIn::AllFields))
        .unwrap_err();
    assert!(is_invalid_argument(&e), "{e:?}");
    assert_eq!(rows(&h), before);
}

// --- U265, V9, F10: the preview --------------------------------------------------

fn spans(entry: &find_replace::PreviewEntry) -> Vec<(Edit, &str)> {
    entry.diff.iter().map(|span| (span.edit, span.text.as_str())).collect()
}

/// V9: the preview counts the notes the run writes (an unchanged match does
/// not count), lists one entry per changed field in result order then field
/// order, each with the note's Sort Field, the field's name and the
/// character diff of the texts with HTML removed; Tags are one field. It
/// writes nothing: rows, `col.mod` and both histories stay. The run that
/// follows writes exactly the notes it listed.
#[test]
fn v9_the_preview_lists_what_the_run_then_writes() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let plain = plain(&h);
    let a = add(&h, vocab, &["<b>rate</b>", "the cost of it"], &["keep"]);
    let b = add(&h, plain, &["cost here", "Cost too"], &[]);
    let same = add(&h, vocab, &["cost", "x"], &[]);
    let none = add(&h, vocab, &["nothing", "x"], &[]);
    // An undo step and a redo step to keep.
    run(&h, &[none], &request("x", "y", FindIn::AllFields)).unwrap();
    run(&h, &[none], &request("y", "z", FindIn::AllFields)).unwrap();
    h.collection.undo(UndoScope::Global).unwrap();
    let before = rows(&h);
    let histories = (undo_name(&h), redo_name(&h));
    // `same`'s match gives back what is stored (F6 step 6).
    let regex = FindReplace {
        regex: true,
        ..request("(cost)", "$1", FindIn::AllFields)
    };
    assert_eq!(preview_of(&h.collection, &NoteTarget::Notes(vec![same]), &regex).unwrap().changed, 0);
    let target = NoteTarget::Notes(vec![b, none, same, a]);
    let shown = preview_of(&h.collection, &target, &request("cost", "$5", FindIn::AllFields)).unwrap();
    assert_eq!(rows(&h), before, "the preview writes nothing");
    assert_eq!((undo_name(&h), redo_name(&h)), histories, "both histories as they were");
    assert_eq!((shown.changed, shown.considered), (3, 4));
    let listed: Vec<_> = shown
        .entries
        .iter()
        .map(|entry| (entry.note, entry.sort_field.as_str(), entry.field.as_deref()))
        .collect();
    assert_eq!(
        listed,
        [
            (b, "cost here", Some("Text")),
            (b, "cost here", Some("Extra")),
            (same, "cost", Some("Word")),
            (a, "rate", Some("Meaning")),
        ],
        "result order, then field order; the Sort Field without its HTML"
    );
    assert_eq!(spans(&shown.entries[1]), [(Edit::Removed, "Cost"), (Edit::Inserted, "$5"), (Edit::Kept, " too")]);
    assert_eq!(
        spans(&shown.entries[3]),
        [(Edit::Kept, "the "), (Edit::Removed, "cost"), (Edit::Inserted, "$5"), (Edit::Kept, " of it")]
    );
    assert_eq!((shown.entries[3].old_text(), shown.entries[3].new_text()), ("the cost of it".into(), "the $5 of it".into()));
    // The run, confirmed with that count, writes exactly those notes.
    let done = run_on(&h.collection, target, &FindReplace {
        confirmed: shown.changed,
        ..request("cost", "$5", FindIn::AllFields)
    })
    .unwrap();
    let written: std::collections::BTreeSet<NoteId> = done.changes.notes.iter().map(|change| change.id).collect();
    let listed: std::collections::BTreeSet<NoteId> = shown.entries.iter().map(|entry| entry.note).collect();
    assert_eq!(written, listed);
    assert_eq!(fields_of(&h, b), ["$5 here", "$5 too"]);
    assert_eq!(fields_of(&h, a)[1], "the $5 of it");
    // Tags: one entry per note, the tags as C7.10 shows them, no field name.
    let shown =
        preview_of(&h.collection, &NoteTarget::Notes(vec![a]), &request("keep", "keep::new", FindIn::Tags)).unwrap();
    assert_eq!(shown.changed, 1);
    assert_eq!(shown.entries.len(), 1);
    assert_eq!(shown.entries[0].field, None);
    assert_eq!(shown.entries[0].sort_field, "rate");
    assert_eq!(spans(&shown.entries[0]), [(Edit::Kept, "keep"), (Edit::Inserted, "::new")]);
}

/// V9, F7 step 5: the Tags preview spells as the run will, registering
/// nothing: from the registry and from the rows the notes before it in
/// ascending note id would register, so a tag the run would come out as
/// stored does not count.
#[test]
fn v9_the_tags_preview_spells_as_the_run_does() {
    let h = common::fresh();
    let vocab = vocab(&h);
    // The lower note id registers `Fresh::One`; the higher one, listed
    // first, spells `fresh::one` from it and comes out as stored.
    let first = add(&h, vocab, &["one", "x"], &["alpha"]);
    let second = add(&h, vocab, &["two", "x"], &[]);
    raw_note(&h, second, |n| n.tags = " Fresh::One ".into());
    let before = rows(&h);
    let target = NoteTarget::Notes(vec![second, first]);
    let request = FindReplace {
        regex: true,
        ..request("^(alpha|fresh::one)$", "Fresh::One", FindIn::Tags)
    };
    let shown = preview_of(&h.collection, &target, &request).unwrap();
    assert_eq!(rows(&h), before, "nothing registered");
    assert_eq!(shown.changed, 1);
    assert_eq!(shown.entries.iter().map(|entry| entry.note).collect::<Vec<_>>(), [first]);
    assert_eq!(shown.entries[0].new_text(), "Fresh::One");
    assert_eq!(run(&h, &[second, first], &request).unwrap(), counts(1, 2));
    assert_eq!(note(&h, first).tags, " Fresh::One ");
    assert_eq!(note(&h, second).tags, " Fresh::One ");
}

/// V9, V17: the list holds the first 100 changed notes in result order
/// while the count goes on; the all-notes target lists by note id.
#[test]
fn v9_the_preview_lists_the_first_hundred_notes() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let inputs = (0..150)
        .map(|i| notes::AddNote {
            notetype_id: vocab,
            fields: vec![format!("cost {i}"), "x".to_owned()],
            tags: Vec::new(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        })
        .collect();
    let mut ids: Vec<NoteId> = notes::bulk_add(&h.collection, inputs, OpSource::UserUndoable)
        .unwrap()
        .value
        .into_iter()
        .map(|added| added.id)
        .collect();
    ids.sort_unstable();
    let reversed: Vec<NoteId> = ids.iter().rev().copied().collect();
    let replace = request("cost", "fee", field("Word"));
    let shown = preview_of(&h.collection, &NoteTarget::Notes(reversed.clone()), &replace).unwrap();
    assert_eq!((shown.changed, shown.considered), (150, 150));
    let listed: Vec<NoteId> = shown.entries.iter().map(|entry| entry.note).collect();
    assert_eq!(listed, reversed[..find_replace::PREVIEW_NOTES], "the list's order");
    let all = preview_of(&h.collection, &NoteTarget::AllNotes, &replace).unwrap();
    assert_eq!(all.changed, 150);
    let listed: Vec<NoteId> = all.entries.iter().map(|entry| entry.note).collect();
    assert_eq!(listed, ids[..find_replace::PREVIEW_NOTES], "ascending note id");
}

/// V9's Whole Words, in the preview and the run alike.
#[test]
fn v9_whole_words_in_the_preview_and_the_run() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["pim", "pims and a pim"], &["pim", "pims"]);
    let b = add(&h, vocab, &["spim", "C++x"], &[]);
    let words = |find: &str, replace: &str, within: FindIn| FindReplace {
        whole_words: true,
        ..request(find, replace, within)
    };
    assert_eq!(run(&h, &[a, b], &words("pim", "rok", FindIn::AllFields)).unwrap(), counts(1, 2));
    assert_eq!(fields_of(&h, a), ["rok", "pims and a rok"]);
    assert_eq!(fields_of(&h, b), ["spim", "C++x"]);
    assert_eq!(run(&h, &[a, b], &words("C++", "x", FindIn::AllFields)).unwrap(), counts(0, 2));
    assert_eq!(run(&h, &[a], &words("pim", "rok", FindIn::Tags)).unwrap(), counts(1, 1));
    assert_eq!(note(&h, a).tags, " pims rok ");
}

/// V9: All Found is every note of the current results, as a selection of
/// the handle's whole range: in cards mode a note with two cards counts
/// once.
#[test]
fn v9_all_found_is_the_notes_of_every_result_row() {
    struct Events(Mutex<Vec<SearchEvent>>);
    impl SearchListener for Events {
        fn event(&self, event: SearchEvent) {
            self.0.lock().unwrap().push(event);
        }
    }
    let h = common::fresh();
    let plain = plain(&h);
    let two = add(&h, plain, &["cost a", "extra"], &[]);
    let one = add(&h, plain, &["cost b", ""], &[]);
    assert_eq!(card_count(&h, two), 2);
    let events = Arc::new(Events(Mutex::new(Vec::new())));
    let session = h.engine.open_search_session(events.clone());
    session.start_search("cost", SearchMode::Cards, SortSpec::new("noteFld", SortDirection::Ascending), None);
    // At most 30 s of 5 ms waits.
    let mut waits = 6_000;
    let (handle, revision, count) = loop {
        let ended = events.0.lock().unwrap().iter().find_map(|event| match event {
            SearchEvent::Complete {
                handle,
                revision,
                count,
                ..
            } => Some(Ok((*handle, *revision, *count))),
            SearchEvent::Failed { .. } | SearchEvent::Cancelled { .. } => Some(Err(format!("{event:?}"))),
            _ => None,
        });
        match ended {
            Some(Ok(found)) => break found,
            Some(Err(event)) => panic!("the search ended without a result: {event}"),
            None => {
                waits -= 1;
                assert!(waits > 0, "the search did not end");
            }
        }
        std::thread::sleep(Duration::from_millis(5));
    };
    assert_eq!(count, 3, "three cards");
    let whole: Vec<std::ops::Range<u64>> = std::iter::once(0..count).collect();
    let found = NoteTarget::Selection(session.selection(handle, revision, whole));
    let shown = preview_of(&h.collection, &found, &request("cost", "fee", field("Text"))).unwrap();
    assert_eq!((shown.changed, shown.considered), (2, 2));
    let done = run_on(&h.collection, found, &request("cost", "fee", field("Text"))).unwrap();
    assert_eq!(done.value, counts(2, 2));
    assert_eq!((fields_of(&h, two)[0].as_str(), fields_of(&h, one)[0].as_str()), ("fee a", "fee b"));
}

/// F10: a preview cancelled before it starts, or from its first progress
/// report (the next keystroke superseding it), ends `Cancelled` and writes
/// nothing.
#[test]
fn a_cancelled_preview_ends_without_a_result() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let inputs = (0..2_500)
        .map(|i| notes::AddNote {
            notetype_id: vocab,
            fields: vec![format!("cost {i}"), "x".to_owned()],
            tags: Vec::new(),
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        })
        .collect();
    notes::bulk_add(&h.collection, inputs, OpSource::UserUndoable).unwrap();
    let before = rows(&h);
    let replace = request("cost", "fee", FindIn::AllFields);
    let token = CancelToken::new();
    token.cancel();
    let e = find_replace::preview(&h.collection, &NoteTarget::AllNotes, &replace, &token, &ProgressSink::new())
        .unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    let token = CancelToken::new();
    let progress = ProgressSink::with_clock(zip_time);
    let cancel = Arc::clone(&token);
    let reports = Arc::new(AtomicU64::new(0));
    let seen = Arc::clone(&reports);
    progress.set_listener(Arc::new(Listener(move |_: &Progress| {
        seen.fetch_add(1, Ordering::SeqCst);
        cancel.cancel();
    })));
    let e = find_replace::preview(&h.collection, &NoteTarget::AllNotes, &replace, &token, &progress).unwrap_err();
    assert_eq!(e, EngineError::Cancelled);
    assert!(reports.load(Ordering::SeqCst) >= 1);
    assert_eq!(rows(&h), before);
    // Not cancelled, the same preview counts every note.
    let shown = find_replace::preview(&h.collection, &NoteTarget::AllNotes, &replace, &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    assert_eq!(shown.changed, 2_500);
}

// --- The confirmed count (D8's pattern, ruled 2026-11-21) -------------------------

fn count_changed(error: &EngineError) -> u64 {
    match refusal(error) {
        Refusal::CountChanged { notes } => *notes,
        other => panic!("expected CountChanged, got {other:?}"),
    }
}

/// A note that starts to match between the preview and the run makes the
/// run write more than was confirmed: refused with the fresh count, nothing
/// written, no step recorded, both histories kept. A listed note edited to
/// match, and a note added under the all-notes target, both count.
#[test]
fn a_run_that_would_write_more_than_was_confirmed_is_refused() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["price", "x"], &[]);
    let replace = request("cost", "fee", field("Word"));
    let listed = NoteTarget::Notes(vec![a, b]);
    let shown = preview_of(&h.collection, &listed, &replace).unwrap();
    assert_eq!(shown.changed, 1);
    h.clock.step_ms(60_000);
    notes::update::update(
        &h.collection,
        notes::update::UpdateNote {
            id: b,
            fields: vec!["cost too".into(), "x".into()],
            tags: Vec::new(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    let before = rows(&h);
    let histories = (undo_name(&h), redo_name(&h));
    let confirmed = FindReplace {
        confirmed: shown.changed,
        ..replace.clone()
    };
    let e = find_replace::run(&h.collection, &listed, &confirmed, CancelToken::new(), ProgressSink::new()).unwrap_err();
    assert_eq!(count_changed(&e), 2, "the fresh count");
    assert_eq!(rows(&h), before, "nothing written");
    assert_eq!((undo_name(&h), redo_name(&h)), histories, "no step recorded");
    // The all-notes target: a note added after the preview.
    let shown = preview_of(&h.collection, &NoteTarget::AllNotes, &replace).unwrap();
    assert_eq!(shown.changed, 2);
    add(&h, vocab, &["cost three", "x"], &[]);
    let before = rows(&h);
    let e = find_replace::run(
        &h.collection,
        &NoteTarget::AllNotes,
        &FindReplace {
            confirmed: shown.changed,
            ..replace.clone()
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap_err();
    assert_eq!(count_changed(&e), 3);
    assert_eq!(rows(&h), before);
    // Shown again with the fresh count, the run goes through.
    let done = find_replace::run(
        &h.collection,
        &NoteTarget::AllNotes,
        &FindReplace { confirmed: 3, ..replace },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert_eq!(done.value.affected, 3);
}

/// A match set that shrank, or stayed as large with other notes, since the
/// preview proceeds: the confirmation is a count, not a list of notes.
#[test]
fn a_run_that_writes_fewer_or_as_many_notes_proceeds() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["cost", "y"], &[]);
    let c = add(&h, vocab, &["price", "z"], &[]);
    let replace = request("cost", "fee", field("Word"));
    let listed = NoteTarget::Notes(vec![a, b, c]);
    let shown = preview_of(&h.collection, &listed, &replace).unwrap();
    assert_eq!(shown.changed, 2);
    notes::delete(&h.collection, vec![b], OpSource::UserUndoable).unwrap();
    let confirmed = FindReplace {
        confirmed: shown.changed,
        ..replace.clone()
    };
    let done = find_replace::run(&h.collection, &listed, &confirmed, CancelToken::new(), ProgressSink::new()).unwrap();
    assert_eq!(done.value, counts(1, 3), "a smaller count proceeds");
    // As many notes, but another one: proceeds too.
    let shown = preview_of(&h.collection, &listed, &request("fee", "cost", field("Word"))).unwrap();
    assert_eq!(shown.changed, 1);
    h.clock.step_ms(60_000);
    for (id, word) in [(a, "kept"), (c, "fee")] {
        notes::update::update(
            &h.collection,
            notes::update::UpdateNote {
                id,
                fields: vec![word.into(), "x".into()],
                tags: Vec::new(),
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    }
    let done = find_replace::run(
        &h.collection,
        &listed,
        &FindReplace {
            confirmed: shown.changed,
            ..request("fee", "cost", field("Word"))
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert_eq!(done.value.affected, 1);
    assert_eq!(fields_of(&h, c)[0], "cost");
}

/// A note that starts to match between the compute phase and the commit:
/// the commit's precondition fails first (`Stale` wins), the run plans
/// again, and the new compute phase refuses with the fresh count. Nothing
/// is written and no step recorded.
#[test]
fn a_note_matching_after_the_compute_phase_is_refused_on_the_new_plan() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["cost", "x"], &[]);
    let b = add(&h, vocab, &["price", "y"], &[]);
    h.clock.step_ms(60_000);
    let progress = ProgressSink::with_clock(zip_time);
    let edited = Arc::new(AtomicBool::new(false));
    let commits = Arc::new(AtomicU64::new(0));
    let (collection, flag, count) = (h.collection.clone(), Arc::clone(&edited), Arc::clone(&commits));
    progress.set_listener(Arc::new(Listener(move |p: &Progress| {
        if p.phase == ProgressPhase::Committing {
            count.fetch_add(1, Ordering::SeqCst);
        }
        if p.phase == ProgressPhase::Planning && p.completed == 0 && !flag.swap(true, Ordering::SeqCst) {
            notes::update::update(
                &collection,
                notes::update::UpdateNote {
                    id: b,
                    fields: vec!["cost".into(), "y".into()],
                    tags: Vec::new(),
                },
                OpSource::UserUndoable,
            )
            .unwrap();
        }
    })));
    let e = find_replace::run(
        &h.collection,
        &NoteTarget::Notes(vec![a, b]),
        &FindReplace {
            confirmed: 1,
            ..request("cost", "fee", field("Word"))
        },
        CancelToken::new(),
        progress,
    )
    .unwrap_err();
    assert_eq!(count_changed(&e), 2);
    assert_eq!(commits.load(Ordering::SeqCst), 1, "one commit, stale, then the new plan refused");
    assert_eq!(fields_of(&h, a), ["cost", "x"], "nothing written");
    assert_eq!(fields_of(&h, b), ["cost", "y"]);
    assert_eq!(undo_name(&h).as_deref(), Some("Update Note"), "no Find and Replace step");
}

/// The write transaction's own count: a Tags run whose spelling changes
/// with the registry between the compute phase and the commit (the
/// preconditions hold the notes, not the registry) would write a note the
/// compute phase found unchanged. Counted again before the first write, it
/// is refused with the fresh count and writes nothing.
#[test]
fn the_write_transaction_counts_again_before_its_first_write() {
    let h = common::fresh();
    let vocab = vocab(&h);
    let a = add(&h, vocab, &["one", "x"], &["solo"]);
    let replace = request("solo", "solo", FindIn::Tags);
    let shown = preview_of(&h.collection, &NoteTarget::Notes(vec![a]), &replace).unwrap();
    assert_eq!(shown.changed, 0, "the tag comes out as stored");
    let progress = ProgressSink::with_clock(zip_time);
    let plans = Arc::new(AtomicU64::new(0));
    let commits = Arc::new(AtomicU64::new(0));
    let (collection, planned, committed) = (h.collection.clone(), Arc::clone(&plans), Arc::clone(&commits));
    progress.set_listener(Arc::new(Listener(move |p: &Progress| {
        if p.phase == ProgressPhase::Committing {
            committed.fetch_add(1, Ordering::SeqCst);
        }
        // Each compute phase reports its first page once.
        if p.phase == ProgressPhase::Planning && (p.completed, p.total) == (0, Some(1)) {
            if planned.fetch_add(1, Ordering::SeqCst) == 0 {
                // The row's spelling changes; no note is touched.
                collection
                    .run(OpSpec::named("respell", OpSource::UserUndoable), |ctx| {
                        let scope = ctx.scope();
                        scope.remove_tag("solo")?;
                        scope.put(&meguri_engine::tags::registry::new_row("SOLO"))?;
                        Ok(())
                    })
                    .unwrap();
            }
        }
    })));
    let before = rows(&h);
    let e = find_replace::run(
        &h.collection,
        &NoteTarget::Notes(vec![a]),
        &FindReplace {
            confirmed: shown.changed,
            ..replace.clone()
        },
        CancelToken::new(),
        progress,
    )
    .unwrap_err();
    assert_eq!(plans.load(Ordering::SeqCst), 1, "one compute phase: no new plan");
    assert_eq!(commits.load(Ordering::SeqCst), 1, "the write transaction refused, not the compute phase");
    assert_eq!(count_changed(&e), 1);
    assert_eq!(rows(&h).notes, before.notes, "nothing written");
    assert_eq!(note(&h, a).tags, " solo ");
    assert_ne!(undo_name(&h).as_deref(), Some("Update Tag"), "no Find and Replace step");
    // Shown again, the run spells from the live row.
    assert_eq!(run(&h, &[a], &replace).unwrap(), counts(1, 1));
    assert_eq!(note(&h, a).tags, " SOLO ");
}

// --- K6, F3: selection_summary --------------------------------------------------

/// F3's table and Meguri's all-notes listing: one name per `unicase` name,
/// spelled by the lowest note type id, in `unicase` order; deleted notes and
/// notes of missing note types add nothing.
#[test]
fn k6_selection_summary_lists_field_names() {
    let h = common::fresh();
    let vocab = made(&h, "VocabS", NotetypeKind::Standard, &["Word", "Meaning"], &["{{Word}}"]);
    let gloss = made(&h, "Gloss", NotetypeKind::Standard, &["Text", "Aside"], &["{{Text}}"]);
    let plain = made(&h, "PlainS", NotetypeKind::Standard, &["text", "Extra"], &["{{text}}"]);
    let v = add(&h, vocab, &["a", "b"], &[]);
    let g = add(&h, gloss, &["a", "b"], &[]);
    let p = add(&h, plain, &["a", "b"], &[]);
    let summary = |target: NoteTarget| selection_summary(&h.collection, &target).unwrap();
    assert_eq!(
        summary(NoteTarget::Notes(vec![g, v])),
        SelectionSummary {
            field_names: vec!["Aside".into(), "Meaning".into(), "Text".into(), "Word".into()],
            notetypes: vec![vocab, gloss],
            notes: 2,
        }
    );
    // `text` (PlainS) and `Text` (Gloss): one entry, Gloss's spelling (the
    // lower id).
    assert_eq!(
        summary(NoteTarget::Notes(vec![p, g])).field_names,
        ["Aside", "Extra", "Text"]
    );
    notes::delete(&h.collection, vec![p], OpSource::UserUndoable).unwrap();
    let gone = summary(NoteTarget::Notes(vec![p]));
    assert_eq!(gone, SelectionSummary::default());
    let all = summary(NoteTarget::AllNotes);
    for name in ["Aside", "Extra", "Meaning", "Text", "Word"] {
        assert!(all.field_names.iter().any(|n| n == name), "{name}");
    }
    assert!(all.notetypes.contains(&plain), "every note type, with notes or not");
    assert_eq!(
        all.notes,
        h.collection.reader(ReaderRole::Query).unwrap().row_count(Table::Notes).unwrap()
    );
}
