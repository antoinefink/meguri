//! Tags (`specs/tags/tags.md`): what a note save stores, the management
//! operations through the engine's ops, their undo steps, and what no
//! scenario can show (T6's skipped notes, T8's skipped sources mixed with
//! moving ones, T12's deviations, T15's undo, `config` kept). The scenarios
//! themselves are `tests/tags_conformance.rs`.

mod common;

use std::thread;
use std::time::Duration;

use common::tagging::{fresh, names, raw, resave_note, row, rows, save_note, setup_spec, stored_note, tags_of};
use common::{Gate, Harness};
use meguri_engine::tags::{self, RunOptions, TagNode};
use meguri_engine::undo::names::{CLEAR_UNUSED_TAGS, EXPAND_COLLAPSE, REMOVE_TAG, RENAME_TAG, UPDATE_TAG};
use meguri_engine::{EngineError, UndoScope, UndoStatus, marked};
use meguri_model::usn::Usn;
use meguri_storage::ReadRows;
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
fn a_new_tag_takes_the_spelling_of_an_implicit_parent() {
    // T3 step 2: the longest leading part that the tree shows lends its
    // spelling, an implicit parent included (the name itself or an
    // ancestor), spelled as the first row below it in unicase order.
    let h = fresh();
    let c = &h.collection;
    save_note(c, &["Pod::x"]);
    let child = save_note(c, &["pod::y"]);
    let itself = save_note(c, &["POD"]);
    assert_eq!(tags_of(c, child), " Pod::y ");
    assert_eq!(tags_of(c, itself), " Pod ");
    // Within one save, a tag registered earlier makes its parent a node for
    // the tags after it.
    let list = save_note(c, &["qq::R", "QQ"]);
    assert_eq!(tags_of(c, list), " qq qq::R ");
    // The longest node wins over a shorter registered one.
    save_note(c, &["W"]);
    save_note(c, &["W::V::m"]);
    let deep = save_note(c, &["w::v::n"]);
    assert_eq!(tags_of(c, deep), " W::V::n ");
    // Rows another client spelled unlike each other: the first in unicase
    // order spells the implicit parents.
    raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES \
         ('sea::NORTH::fjord', 0, 1, NULL), ('SEA::north::bay', 0, 1, NULL)",
        &[],
    );
    let sea = save_note(c, &["Sea::North::cove"]);
    assert_eq!(tags_of(c, sea), " SEA::north::cove ");
    // `k:::m` has the components `k` and `:m`: `k` is its implicit parent,
    // `k:` is no node.
    save_note(c, &["k:::m"]);
    let colon = save_note(c, &["K:"]);
    let letter = save_note(c, &["K"]);
    assert_eq!((tags_of(c, colon), tags_of(c, letter)), (" K: ".into(), " k ".into()));
    assert_eq!(
        names(c),
        [
            "K:",
            "Pod",
            "Pod::x",
            "Pod::y",
            "SEA::north::bay",
            "SEA::north::cove",
            "W",
            "W::V::m",
            "W::V::n",
            "k",
            "k:::m",
            "qq",
            "qq::R",
            "sea::NORTH::fjord"
        ]
    );
    let made = row(c, "Pod").unwrap();
    assert_eq!((made.usn, made.collapsed, made.config), (Usn(-1), true, None));
}

#[test]
fn rename_onto_an_implicit_parent_takes_its_spelling_and_refuses_a_space() {
    // The plan's setup: `Bar::x` is the only row near the new name.
    let h = fresh();
    let c = &h.collection;
    let n1 = save_note(c, &["foo", "foo::c"]);
    let n2 = save_note(c, &["Bar::x"]);
    raw(c, "UPDATE tags SET usn = 0", &[]);
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'04' WHERE tag = 'foo'",
        &[],
    );
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
    // Each new row carries its source's columns, with usn -1 (T7 step 5).
    let bar = row(c, "Bar").unwrap();
    assert_eq!((bar.usn, bar.collapsed, bar.config), (Usn(-1), false, Some(vec![4])));
    let kid = row(c, "Bar::c").unwrap();
    assert_eq!((kid.usn, kid.collapsed, kid.config), (Usn(-1), true, None));
    assert_eq!(
        row(c, "Bar::x").unwrap().usn,
        Usn(0),
        "the implicit parent's row is untouched"
    );
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
    raw(c, "UPDATE tags SET usn = 0", &[]);
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'0801' WHERE tag = 'verbs'",
        &[],
    );
    raw(c, "UPDATE tags SET config = X'02' WHERE tag = 'verbs::x'", &[]);
    let before = (tags_of(c, n), rows(c));
    assert_eq!(tags::rename(c, "verbs", "Verbs", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Verbs Verbs::x ");
    // Each row keeps its state and config under the new spelling (T7 step
    // 5): an expanded `verbs` stays expanded as `Verbs`.
    let renamed: Vec<_> = rows(c)
        .into_iter()
        .map(|r| (r.name, r.usn, r.collapsed, r.config))
        .collect();
    assert_eq!(
        renamed,
        [
            ("Verbs".to_owned(), Usn(-1), false, Some(vec![8, 1])),
            ("Verbs::x".to_owned(), Usn(-1), true, Some(vec![2])),
        ]
    );
    // The rows went and came back under keys that compare equal: one undo
    // brings back the old spelling and every column.
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
    let n4 = save_note(c, &["foo::a"]);
    raw(c, "UPDATE tags SET usn = 0", &[]);
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'01' WHERE tag = 'bar'",
        &[],
    );
    raw(
        c,
        "UPDATE tags SET collapsed = 1, config = X'05' WHERE tag = 'foo'",
        &[],
    );
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'03' WHERE tag = 'foo::a'",
        &[],
    );
    tags::set_collapsed(c, &["foo::orphan"], false).unwrap();
    let untouched = common::tagging::note(c, n3);
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::rename(c, "FOO", "bar", &opts()).unwrap().value, 2);
    assert_eq!(tags_of(c, n1), " bar bar ");
    assert_eq!(tags_of(c, n4), " bar::a ");
    assert_eq!(common::tagging::note(c, n3), untouched, "foobar does not match foo");
    // A merge into a row that the rename left (T7 step 6): that row keeps
    // every column, and the columns of `foo` go.
    let bar = row(c, "bar").unwrap();
    assert_eq!((bar.usn, bar.collapsed, bar.config), (Usn(0), false, Some(vec![1])));
    // The descendant's new row carries the columns of `foo::a` (step 5).
    let kid = row(c, "bar::a").unwrap();
    assert_eq!((kid.usn, kid.collapsed, kid.config), (Usn(-1), false, Some(vec![3])));
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

/// Every registry row as (name, usn, collapsed, config), by name.
fn columns(c: &meguri_engine::Collection) -> Vec<(String, Usn, bool, Option<Vec<u8>>)> {
    rows(c)
        .into_iter()
        .map(|r| (r.name, r.usn, r.collapsed, r.config))
        .collect()
}

#[test]
fn a_reparent_carries_its_rows_columns_and_one_undo_restores_them() {
    // T8 step 4 with T7 step 5: each moved row takes the columns of the row
    // it replaces, with usn -1.
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["ra", "ra::kid", "rb"]);
    raw(c, "UPDATE tags SET usn = 0", &[]);
    raw(c, "UPDATE tags SET collapsed = 0, config = X'05' WHERE tag = 'ra'", &[]);
    raw(c, "UPDATE tags SET collapsed = 0 WHERE tag = 'ra::kid'", &[]);
    let before = (common::tagging::note(c, n), columns(c));
    let col_before = common::col_mod(c);
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::reparent(c, &["ra"], Some("rb"), &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " rb::ra rb::ra::kid rb ");
    assert_eq!(
        columns(c),
        [
            ("rb".to_owned(), Usn(0), true, None),
            ("rb::ra".to_owned(), Usn(-1), false, Some(vec![5])),
            ("rb::ra::kid".to_owned(), Usn(-1), false, None),
        ]
    );
    assert_eq!(top_step(&h), Some(RENAME_TAG));
    undo(&h);
    assert_eq!((common::tagging::note(c, n), columns(c)), before);
    assert_eq!(common::col_mod(c), col_before);
}

#[test]
fn two_tags_giving_one_new_name_take_the_columns_of_the_first_in_note_order() {
    // `foo` and `x::foo` onto `p` both give `p::foo`: the first matching tag
    // in note id order, then stored order, registers it with its row's
    // columns (T7 steps 4 and 5).
    for (first, second, carried) in [
        (&["x::foo"][..], &["foo", "x::foo"][..], (true, Some(vec![2]))),
        (&["foo"][..], &["x::foo", "foo"][..], (false, Some(vec![1]))),
    ] {
        let h = fresh();
        let c = &h.collection;
        let n1 = save_note(c, first);
        let n2 = save_note(c, second);
        // The second save sorted its tags; store them in the given order, as
        // the rewrite keeps stored order (T6).
        common::tagging::raw(
            c,
            "UPDATE notes SET tags = ?1 WHERE id = ?2",
            &[
                SqlValue::Text(format!(" {} ", second.join(" "))),
                SqlValue::Integer(n2.0),
            ],
        );
        raw(
            c,
            "UPDATE tags SET collapsed = 0, config = X'01' WHERE tag = 'foo'",
            &[],
        );
        raw(
            c,
            "UPDATE tags SET collapsed = 1, config = X'02' WHERE tag = 'x::foo'",
            &[],
        );
        assert_eq!(
            tags::reparent(c, &["foo", "x::foo"], Some("p"), &opts()).unwrap().value,
            2
        );
        assert_eq!(tags_of(c, n1), " p::foo ");
        assert_eq!(tags_of(c, n2), " p::foo p::foo ");
        assert_eq!(
            columns(c),
            [("p::foo".to_owned(), Usn(-1), carried.0, carried.1)],
            "{first:?} first"
        );
    }
}

#[test]
fn a_matching_tag_that_no_row_lists_gets_a_new_rows_columns() {
    // Another client stored `ghost::kid` without registering it: no row
    // gives it columns, so its new row has T3's (T7 step 5).
    let h = fresh();
    let c = &h.collection;
    let spelled = save_note(c, &["ghost"]);
    let other = stored_note(c, " ghost::kid ");
    raw(
        c,
        "UPDATE tags SET collapsed = 0, config = X'07' WHERE tag = 'ghost'",
        &[],
    );
    assert_eq!(tags::rename(c, "ghost", "spirit", &opts()).unwrap().value, 2);
    assert_eq!(tags_of(c, spelled), " spirit ");
    assert_eq!(tags_of(c, other), " spirit::kid ");
    assert_eq!(
        columns(c),
        [
            ("spirit".to_owned(), Usn(-1), false, Some(vec![7])),
            ("spirit::kid".to_owned(), Usn(-1), true, None),
        ]
    );
}

#[test]
fn a_reparent_onto_an_implicit_parent_takes_its_spelling() {
    // `foo` onto `bar` while only `Bar::x` is registered: `Bar` is the
    // tree's spelling of the target (T3, T8 step 4).
    let h = fresh();
    let c = &h.collection;
    let n = save_note(c, &["foo", "foo::k"]);
    save_note(c, &["Bar::x"]);
    assert_eq!(tags::reparent(c, &["foo"], Some("bar"), &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Bar::foo Bar::foo::k ");
    assert_eq!(names(c), ["Bar::foo", "Bar::foo::k", "Bar::x"]);
}

#[test]
fn delete_removes_descendants_from_notes_and_rows_even_unused() {
    let h = fresh();
    let c = &h.collection;
    let keep = save_note(c, &["vine", "keep"]);
    let gone = save_note(c, &["vine::leaf", "VINE::leaf::tip"]);
    let vineyard = save_note(c, &["vineyard"]);
    tags::set_collapsed(c, &["vine::bud"], false).unwrap();
    h.clock.step_ms(3_600_000);
    assert_eq!(tags::delete(c, "Vine", &opts()).unwrap().value, 2);
    assert_eq!(
        (tags_of(c, keep), tags_of(c, gone), tags_of(c, vineyard)),
        (" keep ".into(), String::new(), " vineyard ".into())
    );
    assert_eq!(names(c), ["keep", "vineyard"]);
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
    let n1 = save_note(c, &["Pine", "elm", "OAK"]);
    assert_eq!(tags_of(c, n1), " elm OAK Pine ");
    let n2 = save_note(c, &[]);
    save_note(c, &["birch"]);
    let unsorted = stored_note(c, " yew ash yew ");
    let done = stored_note(c, " b a ");
    h.clock.step_ms(3_600_000);
    // Each tag takes its registered spelling, the tab goes (T2), and the
    // missing tags join the stored ones, deduplicated and sorted.
    let added = tags::add_to_notes(c, &[n1, n2, unsorted], "pine BIRCH oak fir\tcone", &opts()).unwrap();
    assert_eq!(added.value, 3);
    assert_eq!(tags_of(c, n1), " birch elm fircone OAK Pine ");
    assert_eq!(tags_of(c, n2), " birch fircone OAK Pine ");
    assert_eq!(tags_of(c, unsorted), " ash birch fircone OAK Pine yew ");
    assert_eq!(top_step(&h), Some(UPDATE_TAG));

    // A note that already has the tags is not written, even unsorted, and
    // the tag is registered all the same.
    let untouched = common::tagging::note(c, done);
    assert_eq!(tags::add_to_notes(c, &[done], "A", &opts()).unwrap().value, 0);
    assert_eq!(common::tagging::note(c, done), untouched);
    assert_eq!(tags::add_to_notes(c, &[done], "fresh", &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, done), " a b fresh ");

    let removed = tags::remove_from_notes(c, &[n1, n2, done], "BIRCH fresh", &opts()).unwrap();
    assert_eq!(removed.value, 3);
    assert_eq!(tags_of(c, n1), " elm fircone OAK Pine ");
    assert_eq!(tags_of(c, done), " a b ");
    assert!(row(c, "birch").is_some() && row(c, "fresh").is_some(), "rows stay");
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

    // With only `Marked::x` registered, `marked` is its implicit parent and
    // takes the tree's spelling (M2, T3).
    let h = fresh();
    let c = &h.collection;
    save_note(c, &["Marked::x"]);
    let n = save_note(c, &["q"]);
    assert_eq!(marked::mark(c, &[n], &opts()).unwrap().value, 1);
    assert_eq!(tags_of(c, n), " Marked q ");
    assert_eq!(names(c), ["Marked", "Marked::x", "q"]);
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

#[test]
fn a_toggle_decides_on_the_current_note_as_its_write_phase_finds_it() {
    // Another write marks the current note after the toggle has read it
    // unmarked and before the toggle's write phase: the toggle unmarks it,
    // as the note stands when it writes (M2), instead of re-adding a tag the
    // note already has.
    let h = fresh();
    let c = &h.collection;
    let current = save_note(c, &["q"]);
    let gate = Gate::new();
    let concurrent = {
        let inner = gate.clone();
        c.submit(setup_spec(), move |ctx| {
            let mut note = ctx.scope().note(current)?.expect("the note exists");
            note.tags = " marked q ".to_owned();
            ctx.scope().put(&note)?;
            // Uncommitted until the gate opens: readers still see ` q `.
            inner.enter_and_wait();
            Ok(())
        })
    };
    gate.wait_entered();
    let toggle = {
        let c = c.clone();
        thread::spawn(move || marked::toggle_mark(&c, current, &[current], &RunOptions::default()))
    };
    // The toggle reads the note unmarked and queues behind the held writer.
    thread::sleep(Duration::from_millis(50));
    gate.open();
    let concurrent = concurrent.wait().unwrap();
    let toggled = toggle.join().unwrap().unwrap();
    assert!(toggled.seq > concurrent.seq, "the toggle wrote after the other write");
    assert_eq!(toggled.value, 1);
    assert_eq!(tags_of(c, current), " q ");
    assert_eq!(top_step(&h), Some(REMOVE_TAG));
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
    save_note(c, &["lamp", "lamp-post", "lamp::shade"]);
    // Rows another client wrote, spelled unlike each other (a note save
    // would have spelled them alike, T3).
    raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES ('KITE::wing::right', 0, 1, NULL), \
         ('kite::WING::left', 0, 1, NULL), ('Bar::x', 0, 1, NULL), ('bar::y', 0, 1, NULL)",
        &[],
    );
    raw(
        c,
        "INSERT INTO tags (tag, usn, collapsed, config) VALUES ('a!', 0, 1, NULL), ('B2', 0, 0, NULL), ('b2::c', 0, 0, NULL)",
        &[],
    );
    // Expanding an implicit parent registers it with the tree's spelling,
    // whatever case it is given in (T12 step 1).
    tags::set_collapsed(c, &["KITE::wing"], false).unwrap();
    let tree = tags::tag_tree(c).unwrap();
    // `-` sorts before `:`, yet `lamp::shade` stays under `lamp`, and
    // `lamp-post` comes after `lamp` (T13 step 3).
    assert_eq!(
        show(&tree),
        "a!* B2(c) Bar*(x* y*) kite*(WING(left* right*)) lamp*(shade*) lamp-post*"
    );
    let bar = &tree[2];
    assert!(bar.implicit);
    assert_eq!(bar.children[1].full_name, "bar::y");
    let kite = &tree[3];
    assert!(kite.implicit && !kite.children[0].implicit);
    assert_eq!(kite.children[0].full_name, "kite::WING");
}

#[test]
fn rename_touching_ten_thousand_of_150k_notes_writes_only_those_and_undoes_in_one_step() {
    use meguri_storage::ReaderRole;
    use meguri_testkit::bench::fixtures;

    // Profiles are sized in cards: 175k cards give over 150k notes (152,120
    // at generator version 4), the collection the plan's scenario names.
    let generated = fixtures::profile_collection("175k").expect("the 175k-card collection");
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
    assert!(all.len() >= 150_000, "{} notes", all.len());
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
