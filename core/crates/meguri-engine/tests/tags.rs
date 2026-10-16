//! Tags (`specs/tags/tags.md`): what a note save stores, the management
//! operations through the engine's ops, their undo steps, and what no
//! scenario can show (T6's skipped notes, T8's skipped sources mixed with
//! moving ones, T12's deviations, T15's undo, `config` kept). The scenarios
//! themselves are `tests/tags_conformance.rs`.

mod common;

use common::Harness;
use common::tagging::{fresh, names, raw, resave_note, row, rows, save_note, stored_note, tags_of};
use meguri_engine::tags::{self, RunOptions, TagNode};
use meguri_engine::undo::names::{CLEAR_UNUSED_TAGS, EXPAND_COLLAPSE, REMOVE_TAG, RENAME_TAG, UPDATE_TAG};
use meguri_engine::{EngineError, UndoScope, UndoStatus, marked};
use meguri_model::usn::Usn;
use meguri_storage::raw::SqlValue;

fn opts() -> RunOptions {
    RunOptions::default()
}

fn top_step(h: &Harness) -> Option<&'static str> {
    h.collection.peek_undo(UndoScope::Global).unwrap().name()
}

fn undo(h: &Harness) {
    h.collection.undo(UndoScope::Global).unwrap();
}

#[test]
fn a_new_child_adopts_its_registered_parents_spelling_ae37() {
    let h = fresh();
    let c = &h.collection;
    save_note(c, &["German::Verb"]);
    let n = save_note(c, &["german::verb::modal"]);
    assert_eq!(tags_of(c, n), " German::Verb::modal ");
    assert_eq!(names(c), ["German::Verb", "German::Verb::modal"]);
    let new = row(c, "German::Verb::modal").unwrap();
    assert_eq!((new.usn, new.collapsed, new.config), (Usn(-1), true, None));
}

#[test]
fn saving_removes_separators_and_controls_and_keeps_quotes() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["a\u{3000}b\tc\"d", "x::::y"]);
    assert_eq!(tags_of(c, n), " a bc\"d x::blank::y ");
    assert_eq!(names(c), ["a", "bc\"d", "x::blank::y"]);
}

#[test]
fn saving_dedupes_ignoring_case_sorts_and_registers() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["b", "A", "a"]);
    assert_eq!(tags_of(c, n), " A b ");
    assert_eq!(names(c), ["A", "b"]);
    let m = save_note(c, &["Stra\u{df}e", "STRASSE", "z", "_x", "[", "Y"]);
    assert_eq!(tags_of(c, m), " [ _x Stra\u{df}e Y z ");
    let empty = save_note(c, &["", "\u{3000}"]);
    assert_eq!(tags_of(c, empty), "");
}

#[test]
fn rename_takes_a_registered_spelling_and_refuses_a_space() {
    let h = fresh();
    let c = &h.collection;
    let n1 = save_note(c, &["foo", "foo::c"]);
    let n2 = save_note(c, &["Bar::x"]);
    save_note(c, &["Bar"]);
    let before = common::col_mod(c);
    for refused in ["has space", "", "   ", "a\u{3000}b", "\t"] {
        assert!(
            matches!(
                tags::rename(c, "foo", refused, &opts()),
                Err(EngineError::InvalidArgument { .. })
            ),
            "{refused:?}"
        );
    }
    assert_eq!(common::col_mod(c), before, "a refusal changes nothing");
    assert_eq!(tags_of(c, n1), " foo foo::c ");

    h.clock.step_ms(3_600_000);
    let renamed = tags::rename(c, "foo", "bar", &opts()).unwrap();
    assert_eq!(renamed.value, 1);
    assert_eq!(tags_of(c, n1), " Bar Bar::c ");
    assert_eq!(tags_of(c, n2), " Bar::x ");
    assert_eq!(names(c), ["Bar", "Bar::c", "Bar::x"]);
    let UndoStatus::Available(step) = c.peek_undo(UndoScope::Global).unwrap() else {
        panic!("the rename recorded a step")
    };
    assert_eq!(
        (step.name, step.long_write),
        (RENAME_TAG, None),
        "a small rename declares nothing"
    );
}

#[test]
fn a_case_only_rename_and_a_rename_below_a_new_casing_adopt_the_new_casing() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["verbs", "verbs::x"]);
    let before = (tags_of(c, n), rows(c));
    assert_eq!(tags::rename(c, "verbs", "Verbs", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Verbs Verbs::x ");
    assert_eq!(names(c), ["Verbs", "Verbs::x"]);
    // The rows went and came back under keys that compare equal: one undo
    // brings back the old spelling.
    undo(&h);
    assert_eq!((tags_of(c, n), rows(c)), before);

    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["verbs", "verbs::x"]);
    assert_eq!(tags::rename(c, "verbs", "Verbs::regular", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Verbs::regular Verbs::regular::x ");
    assert_eq!(names(c), ["Verbs::regular", "Verbs::regular::x"]);
}

#[test]
fn rename_rewrites_in_place_without_sorting_and_keeps_a_merged_rows_columns() {
    let h = fresh();
    let c = &h.collection;
    let n1 = save_note(c, &["bar", "foo"]);
    let n2 = save_note(c, &["a", "zzz"]);
    let n3 = save_note(c, &["foobar"]);
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'01' WHERE tag = 'bar'",
        &[],
    );
    tags::set_collapsed(c, &["foo::orphan"], false).unwrap();
    let untouched = common::tagging::note(c, n3);
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::rename(c, "FOO", "bar", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n1), " bar bar ");
    assert_eq!(common::tagging::note(c, n3), untouched, "foobar does not match foo");
    let bar = row(c, "bar").unwrap();
    assert_eq!((bar.collapsed, bar.config), (false, Some(vec![1])));
    assert!(row(c, "foo::orphan").is_none(), "registry-only descendants go");
    assert!(row(c, "bar::orphan").is_none(), "and are not made again");

    assert_eq!(tags::rename(c, "a", "zzz2", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n2), " zzz2 zzz ");
}

#[test]
fn a_matching_note_whose_text_does_not_change_is_not_written() {
    // T6: the registry spells `foo`; one note stores `FOO` (another client).
    let h = fresh();
    let c = &h.collection;
    let spelled = save_note(c, &["foo"]);
    let other = stored_note(c, " FOO ");
    let before = common::tagging::note(c, other);
    h.clock.step_ms(3_600_000);
    // The rows of `foo` go before `FOO` is spelled, so `FOO` is registered
    // and the second note's text stays ` FOO `.
    let renamed = tags::rename(c, "foo", "FOO", &opts()).unwrap();
    assert_eq!(renamed.value, 1, "only the note whose text changed counts");
    assert_eq!(tags_of(c, spelled), " FOO ");
    assert_eq!(common::tagging::note(c, other), before, "mod and usn untouched");
    assert_eq!(names(c), ["FOO"]);
}

#[test]
fn reparent_follows_the_t8_rules() {
    let h = fresh();
    let c = &h.collection;
    let ra = save_note(c, &["ra"]);
    let kid = save_note(c, &["ra::kid"]);
    save_note(c, &["rb"]);
    let sb = save_note(c, &["sb::bar"]);
    let leaf = save_note(c, &["tc::leaf"]);
    let ud = save_note(c, &["ud", "ud::e"]);
    let va = save_note(c, &["va::b"]);
    let wa = save_note(c, &["wa"]);
    let pc = save_note(c, &["Pc::bar"]);
    let mx = save_note(c, &["ma::mx"]);
    save_note(c, &["mx"]);

    let count = |sources: &[&str], target: Option<&str>| tags::reparent(c, sources, target, &opts()).unwrap().value;
    assert_eq!(count(&["ra"], Some("rb")), 2);
    assert_eq!(
        (tags_of(c, ra), tags_of(c, kid)),
        (" rb::ra ".into(), " rb::ra::kid ".into())
    );
    assert_eq!(count(&["sb::bar"], Some("sbaz")), 1);
    assert_eq!(tags_of(c, sb), " sbaz::bar ");
    assert_eq!(count(&["tc::leaf"], None), 1);
    assert_eq!(tags_of(c, leaf), " leaf ");
    assert_eq!(count(&["ud"], Some("ud::e")), 0, "onto its own child");
    assert_eq!(count(&["ud"], Some("ud")), 0, "onto itself");
    assert_eq!(count(&["ud::e"], Some("ud")), 0, "the same name");
    assert_eq!(tags_of(c, ud), " ud ud::e ");
    assert_eq!(
        count(&["va::b"], Some("va::bc")),
        0,
        "a text prefix with the same first component"
    );
    assert_eq!(tags_of(c, va), " va::b ");
    assert_eq!(count(&["wa"], Some("wab")), 1);
    assert_eq!(tags_of(c, wa), " wab::wa ");
    assert_eq!(count(&["Pc::bar"], Some("pc")), 1);
    assert_eq!(tags_of(c, pc), " pc::bar ");
    assert_eq!(count(&["ma::mx"], None), 1, "merges into a registered top-level tag");
    assert_eq!(tags_of(c, mx), " mx ");
    assert_eq!(top_step(&h), Some(RENAME_TAG));
}

#[test]
fn a_skipped_source_stays_while_the_other_sources_move() {
    // T8 deviation: Anki fails the whole operation here.
    let h = fresh();
    let c = &h.collection;
    let ud = save_note(c, &["ud"]);
    let x = save_note(c, &["x", "x::y"]);
    let both = save_note(c, &["ud", "x"]);
    let moved = tags::reparent(c, &["ud", "x"], Some("ud::e"), &opts()).unwrap();
    assert_eq!(moved.value, 2);
    assert_eq!(tags_of(c, ud), " ud ");
    assert_eq!(tags_of(c, x), " ud::e::x ud::e::x::y ");
    assert_eq!(tags_of(c, both), " ud ud::e::x ");
    assert_eq!(names(c), ["ud", "ud::e::x", "ud::e::x::y"]);
}

#[test]
fn the_first_source_in_list_order_decides() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["foo::bar::x"]);
    assert_eq!(
        tags::reparent(c, &["foo", "foo::bar"], Some("p"), &opts())
            .unwrap()
            .value,
        1
    );
    assert_eq!(tags_of(c, n), " p::foo::bar::x ");
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["foo::bar::x"]);
    assert_eq!(
        tags::reparent(c, &["foo::bar", "foo"], Some("p"), &opts())
            .unwrap()
            .value,
        1
    );
    assert_eq!(tags_of(c, n), " p::bar::x ");
}

#[test]
fn delete_removes_descendants_from_notes_and_rows_even_unused() {
    let h = fresh();
    let c = &h.collection;
    let keep = save_note(c, &["a", "keep"]);
    let gone = save_note(c, &["a::b", "A::b::c"]);
    let ab = save_note(c, &["ab"]);
    tags::set_collapsed(c, &["a::zz"], false).unwrap();
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::delete(c, "A", &opts()).unwrap().value, 2);
    assert_eq!(
        (tags_of(c, keep), tags_of(c, gone), tags_of(c, ab)),
        (" keep ".into(), String::new(), " ab ".into())
    );
    assert_eq!(names(c), ["ab", "keep"]);
    assert_eq!(top_step(&h), Some(REMOVE_TAG));

    // A registry-only tag: no note written, but the row goes and col.mod
    // moves.
    tags::set_collapsed(c, &["lone"], false).unwrap();
    h.clock.step_ms(3_600_000);
    let deleted = tags::delete(c, "lone", &opts()).unwrap();
    assert_eq!(deleted.value, 0);
    assert_eq!(common::col_mod(c), common::NOW_MS + 2 * 3_600_000);
    assert!(row(c, "lone").is_none());
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::delete(c, "nothing", &opts()).unwrap().value, 0);
    assert_eq!(common::col_mod(c), common::NOW_MS + 2 * 3_600_000, "nothing changed");
}

#[test]
fn clear_unused_keeps_parents_of_used_tags_and_their_columns() {
    let h = fresh();
    let c = &h.collection;
    save_note(c, &["a::c", "z"]);
    tags::set_collapsed(c, &["a", "q", "a::b"], false).unwrap();
    stored_note(c, " STRASSE ");
    raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES ('Stra\u{df}e', 0, 0, X'01')",
        &[],
    );
    let cleared = tags::clear_unused(c, &opts()).unwrap();
    assert_eq!(cleared.value, 2);
    assert_eq!(names(c), ["Stra\u{df}e", "a", "a::c", "z"]);
    let kept = row(c, "a").unwrap();
    assert_eq!((kept.collapsed, kept.usn), (false, Usn(-1)));
    assert_eq!(row(c, "Stra\u{df}e").unwrap().config, Some(vec![1]));
    assert_eq!(top_step(&h), Some(CLEAR_UNUSED_TAGS));
    assert_eq!(tags::clear_unused(c, &opts()).unwrap().value, 0);
}

#[test]
fn set_collapsed_writes_one_row_and_one_undo_restores_it() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["a", "b"]);
    raw(c, "UPDATE tags SET usn = 5, config = X'0102'", &[]);
    let note_before = common::tagging::note(c, n);
    let rows_before = rows(c);
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::set_collapsed(c, &["a"], false).unwrap().value, 1);
    let after = rows(c);
    assert_eq!(after[0].name, "a");
    assert_eq!(
        (after[0].collapsed, after[0].usn, &after[0].config),
        (false, Usn(-1), &Some(vec![1, 2]))
    );
    assert_eq!(after[1], rows_before[1], "b untouched");
    assert_eq!(common::tagging::note(c, n), note_before);
    assert_eq!(top_step(&h), Some(EXPAND_COLLAPSE));
    undo(&h);
    assert_eq!(rows(c), rows_before);
}

#[test]
fn set_collapsed_skips_an_unchanged_row_and_sets_the_row_it_registers() {
    // T12's deviations from Anki.
    let h = fresh();
    let c = &h.collection;
    save_note(c, &["a"]);
    raw(c, "UPDATE tags SET usn = 5", &[]);
    let before = common::col_mod(c);
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::set_collapsed(c, &["A"], true).unwrap().value, 0);
    assert_eq!(row(c, "a").unwrap().usn, Usn(5));
    assert_eq!(common::col_mod(c), before);
    // Normalization changes the text: the row made for it gets the state.
    tags::set_collapsed(c, &["n e\u{1}w::"], false).unwrap();
    let made = row(c, "new::blank").unwrap();
    assert_eq!((made.name.as_str(), made.collapsed), ("new::blank", false));
}

#[test]
fn add_and_remove_change_only_the_notes_that_need_it() {
    let h = fresh();
    let c = &h.collection;
    let n1 = save_note(c, &["XYZ", "deF", "aaa"]);
    let n2 = save_note(c, &[]);
    save_note(c, &["abc"]);
    let unsorted = stored_note(c, " zz a zz ");
    let done = stored_note(c, " b a ");
    h.clock.step_ms(3_600_000);
    let added = tags::add_to_notes(c, &[n1, n2, unsorted], "xyz abc DEF new\tone", &opts()).unwrap();
    assert_eq!(added.value, 3);
    assert_eq!(tags_of(c, n1), " aaa abc deF newone XYZ ");
    assert_eq!(tags_of(c, n2), " abc deF newone XYZ ");
    assert_eq!(tags_of(c, unsorted), " a abc deF newone XYZ zz ");
    assert_eq!(top_step(&h), Some(UPDATE_TAG));

    // A note that already has the tags is not written, even unsorted, and
    // the tag is registered all the same.
    let untouched = common::tagging::note(c, done);
    assert_eq!(tags::add_to_notes(c, &[done], "A", &opts()).unwrap().value, 0);
    assert_eq!(common::tagging::note(c, done), untouched);
    assert_eq!(tags::add_to_notes(c, &[done], "fresh", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, done), " a b fresh ");

    let removed = tags::remove_from_notes(c, &[n1, n2, done], "ABC fresh", &opts()).unwrap();
    assert_eq!(removed.value, 3);
    assert_eq!(tags_of(c, done), " a b ");
    assert!(row(c, "abc").is_some() && row(c, "fresh").is_some(), "rows stay");
    assert_eq!(top_step(&h), Some(REMOVE_TAG));
    let stored = stored_note(c, " c b a b ");
    assert_eq!(tags::remove_from_notes(c, &[stored], "a", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, stored), " c b b ");
}

#[test]
fn one_undo_restores_a_delete_with_the_rows_columns() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["x", "x::y", "keep"]);
    raw(
        c,
        "UPDATE tags SET collapsed = 0, usn = 3, config = X'0A0B' WHERE tag = 'x'",
        &[],
    );
    let note_before = common::tagging::note(c, n);
    let rows_before = rows(c);
    let col_before = common::col_mod(c);
    h.clock.step_ms(3_600_000);
    tags::delete(c, "x", &opts()).unwrap();
    assert_eq!(tags_of(c, n), " keep ");
    undo(&h);
    assert_eq!(rows(c), rows_before, "config restored too (R1467)");
    assert_eq!(common::tagging::note(c, n), note_before);
    assert_eq!(common::col_mod(c), col_before);
}

#[test]
fn every_tag_operation_is_one_undo_step() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["a::b", "c", "d"]);
    let start = (tags_of(c, n), rows(c));
    tags::rename(c, "a", "q", &opts()).unwrap();
    tags::reparent(c, &["c"], Some("q"), &opts()).unwrap();
    tags::add_to_notes(c, &[n], "e", &opts()).unwrap();
    tags::remove_from_notes(c, &[n], "d", &opts()).unwrap();
    tags::delete(c, "e", &opts()).unwrap();
    tags::set_collapsed(c, &["q"], false).unwrap();
    tags::set_collapsed(c, &["unused"], false).unwrap();
    tags::clear_unused(c, &opts()).unwrap();
    for step in [
        CLEAR_UNUSED_TAGS,
        EXPAND_COLLAPSE,
        EXPAND_COLLAPSE,
        REMOVE_TAG,
        REMOVE_TAG,
        UPDATE_TAG,
        RENAME_TAG,
        RENAME_TAG,
    ] {
        assert_eq!(top_step(&h), Some(step));
        undo(&h);
    }
    assert_eq!((tags_of(c, n), rows(c)), start);
}

#[test]
fn unmarking_removes_marked_in_any_case_and_its_descendants() {
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["Marked", "marked::x", "markedly", "q"]);
    assert_eq!(tags_of(c, n), " Marked Marked::x markedly q ");
    assert!(marked::is_marked(&tags_of(c, n)));
    assert_eq!(marked::unmark(c, &[n], &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " markedly q ");
    assert!(!marked::is_marked(&tags_of(c, n)));
    assert_eq!(marked::mark(c, &[n], &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Marked markedly q ", "the registered spelling");
}

#[test]
fn toggling_a_mark_follows_the_current_note() {
    let h = fresh();
    let c = &h.collection;
    let current = save_note(c, &["q"]);
    let other = save_note(c, &["marked"]);
    // Review: the current note alone.
    marked::toggle_mark(c, current, &[current], &opts()).unwrap();
    assert_eq!(tags_of(c, current), " marked q ");
    assert_eq!(top_step(&h), Some(UPDATE_TAG));
    marked::toggle_mark(c, current, &[current], &opts()).unwrap();
    assert_eq!(tags_of(c, current), " q ");
    assert_eq!(top_step(&h), Some(REMOVE_TAG));
    // Browse: the focused note decides for every selected note.
    marked::toggle_mark(c, current, &[current, other], &opts()).unwrap();
    assert_eq!(
        (tags_of(c, current), tags_of(c, other)),
        (" marked q ".into(), " marked ".into())
    );
    resave_note(c, current, &["q"]);
    marked::toggle_mark(c, other, &[current, other], &opts()).unwrap();
    assert_eq!((tags_of(c, current), tags_of(c, other)), (" q ".into(), String::new()));
}

fn show(nodes: &[TagNode]) -> String {
    nodes
        .iter()
        .map(|n| {
            let star = if n.collapsed { "*" } else { "" };
            if n.children.is_empty() {
                format!("{}{star}", n.name)
            } else {
                format!("{}{star}({})", n.name, show(&n.children))
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}

#[test]
fn the_tree_adds_implicit_parents_and_orders_children_by_unicase() {
    let h = fresh();
    let c = &h.collection;
    assert!(tags::tag_tree(c).unwrap().is_empty());
    save_note(c, &["foo::BAR::a"]);
    save_note(c, &["FOO::bar::b"]);
    save_note(c, &["one", "one1", "one::two"]);
    save_note(c, &["Bar::x"]);
    save_note(c, &["bar::y"]);
    raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES ('a!', 0, 1, NULL), ('B2', 0, 0, NULL), ('b2::c', 0, 0, NULL)",
        &[],
    );
    tags::set_collapsed(c, &["foo::BAR"], false).unwrap();
    let tree = tags::tag_tree(c).unwrap();
    assert_eq!(show(&tree), "a!* B2(c) Bar*(x* y*) foo*(BAR(a* b*)) one*(two*) one1*");
    let bar = &tree[2];
    assert!(bar.implicit);
    assert_eq!(bar.children[1].full_name, "bar::y");
    assert_eq!(tree[3].children[0].full_name, "foo::BAR");
}

#[test]
fn rename_touching_ten_thousand_of_a_large_collection_writes_only_those_and_undoes_in_one_step() {
    use meguri_storage::{ReadRows, ReaderRole};
    use meguri_testkit::bench::fixtures;

    let generated = fixtures::profile_collection("150k").expect("the 150k collection");
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(generated.dir.join("collection.anki2"), &path).unwrap();
    let h = Harness::open_path(dir, path);
    let c = &h.collection;
    let all = c
        .reader(ReaderRole::Query)
        .unwrap()
        .ids_after(meguri_storage::Table::Notes, None, usize::MAX)
        .unwrap();
    assert!(all.len() >= 50_000, "{} notes", all.len());
    let chosen: Vec<meguri_model::ids::NoteId> = all
        .iter()
        .step_by(all.len() / 10_000)
        .take(10_000)
        .map(|id| meguri_model::ids::NoteId(*id))
        .collect();
    assert_eq!(chosen.len(), 10_000);
    // A parent row with a collapsed state and config, which the rename
    // removes and the undo must bring back whole (R1467).
    common::tagging::raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES ('U30bulk', 4, 0, X'99')",
        &[],
    );
    c.run(common::tagging::setup_spec(), {
        let chosen = chosen.clone();
        move |ctx| tags::add_in(ctx.scope(), &chosen, "U30bulk::child")
    })
    .unwrap();
    let dump = |sql: &str| c.reader(ReaderRole::Query).unwrap().execute_raw(sql, &[]).unwrap().rows;
    let notes_before = dump("SELECT id, mod, usn, tags FROM notes ORDER BY id");
    let rows_before = dump("SELECT tag, usn, collapsed, config FROM tags ORDER BY tag");
    let col_before = common::col_mod(c);

    h.clock.step_ms(3_600_000);
    let renamed = tags::rename(c, "u30bulk", "Renamed", &opts()).unwrap();
    assert_eq!(renamed.value, 10_000);
    assert!(renamed.timing.rows >= 10_000);
    let notes_after = dump("SELECT id, mod, usn, tags FROM notes ORDER BY id");
    let chosen_ids: std::collections::HashSet<i64> = chosen.iter().map(|n| n.0).collect();
    let now_s = (common::NOW_MS + 3_600_000) / 1000;
    for (before, after) in notes_before.iter().zip(&notes_after) {
        let SqlValue::Integer(id) = before[0] else { panic!() };
        if chosen_ids.contains(&id) {
            assert_eq!(after[1], SqlValue::Integer(now_s), "note {id} mod");
            assert_eq!(after[2], SqlValue::Integer(-1), "note {id} usn");
            let SqlValue::Text(tags) = &after[3] else { panic!() };
            assert!(tags.contains(" Renamed::child "), "{tags}");
        } else {
            assert_eq!(before, after, "note {id} is untouched");
        }
    }
    let UndoStatus::Available(step) = c.peek_undo(UndoScope::Global).unwrap() else {
        panic!("the rename recorded a step")
    };
    assert_eq!(step.name, RENAME_TAG);
    assert!(
        step.long_write.is_some(),
        "a 10k-note rename declares its write (op-model O8)"
    );

    undo(&h);
    assert_eq!(dump("SELECT id, mod, usn, tags FROM notes ORDER BY id"), notes_before);
    assert_eq!(
        dump("SELECT tag, usn, collapsed, config FROM tags ORDER BY tag"),
        rows_before
    );
    assert_eq!(common::col_mod(c), col_before);
}
