//! The name index provider (`specs/search/editing-services.md` D6.7): built
//! when the collection opens, then kept current from each change set that
//! touches decks, tags, note types, presets, `savedFilters` or
//! `flagLabels`, a whole-collection change included. After every change the
//! index equals one built afresh from a new snapshot. The values and their
//! order are meguri-search's `tests/name_index.rs`.

mod common;

use std::sync::Arc;

use common::Harness;
use common::tagging::setup_spec;
use meguri_engine::browse::NameIndexProvider;
use meguri_engine::{Ports, browser_config, deck_config, decks, flags, notetypes};
use meguri_model::ids::NotetypeId;
use meguri_search::name_index::NameIndex;
use meguri_storage::raw::PlainDatabase;
use meguri_storage::{ReadRows, ReaderRole};

fn open() -> (Harness, Arc<NameIndexProvider>) {
    let provider = Arc::new(NameIndexProvider::new());
    let ports = Ports::new()
        .open_step(provider.clone())
        .observer(provider.clone())
        .generation_listener(provider.clone());
    (Harness::open_with(ports), provider)
}

/// An index built afresh from a new snapshot of the collection.
fn rebuilt(h: &Harness) -> NameIndex {
    let fresh = NameIndexProvider::new();
    fresh.rebuild(&h.collection).unwrap();
    (*fresh.index()).clone()
}

#[track_caller]
fn assert_current(h: &Harness, provider: &NameIndexProvider) {
    assert_eq!(*provider.index(), rebuilt(h));
}

fn deck_names(index: &NameIndex) -> Vec<(String, bool)> {
    index.decks().iter().map(|d| (d.name.clone(), d.has_subdecks)).collect()
}

#[test]
fn the_index_is_built_at_open_and_follows_every_change() {
    let (h, provider) = open();
    let c = &h.collection;
    let index = provider.index();
    assert!(index.decks().iter().any(|d| d.name == "Default"));
    assert!(!index.notetypes().is_empty());
    assert_eq!(index.flag_names()[0], "Red");
    assert_current(&h, &provider);

    // Decks: an added subdeck marks its parent; a rename moves it.
    let french = decks::ops::add(c, "Langs::French".into()).unwrap().value;
    let names = deck_names(&provider.index());
    assert!(names.contains(&("Langs".into(), true)), "{names:?}");
    assert!(names.contains(&("Langs::French".into(), false)), "{names:?}");
    assert_current(&h, &provider);
    decks::ops::rename(c, french.id, "Other".into()).unwrap();
    let names = deck_names(&provider.index());
    assert!(names.contains(&("Langs".into(), false)), "{names:?}");
    assert!(names.contains(&("Other".into(), false)), "{names:?}");
    assert_current(&h, &provider);

    // Tags: registered by a note save, then removed from the registry.
    common::tagging::save_note(c, &["alpha", "beta::gamma"]);
    assert!(provider.index().tags().iter().any(|t| t == "beta::gamma"));
    assert_current(&h, &provider);
    c.run(setup_spec(), move |ctx| Ok(ctx.scope().remove_tag("alpha")?)).unwrap();
    assert!(!provider.index().tags().iter().any(|t| t == "alpha"));
    assert_current(&h, &provider);

    // Presets, saved searches and flag names.
    deck_config::ops::add(c, "Evening".into()).unwrap();
    assert!(provider.index().presets().iter().any(|p| p.name == "Evening"));
    browser_config::save_search(c, "Leeches", "tag:leech").unwrap();
    assert!(provider.index().saved_searches().iter().any(|s| s.name == "Leeches" && s.text == "tag:leech"));
    flags::rename_flag(c, 1, "Hard").unwrap();
    assert_eq!(provider.index().flag_names()[0], "Hard");
    assert_current(&h, &provider);

    // Note types: added, renamed with a field renamed, removed.
    let source = provider.index().notetypes()[0].id;
    let added = notetypes::clone_notetype(c, source, Some("Copy".into())).unwrap().value;
    let id: NotetypeId = added.id;
    assert!(provider.index().notetypes().iter().any(|n| n.id == id && n.name == "Copy"));
    assert_current(&h, &provider);
    c.run(setup_spec(), move |ctx| {
        let mut notetype = ctx.scope().notetype(id)?.unwrap();
        notetype.name = "Copied".into();
        notetype.fields[0].name = "Prompt".into();
        Ok(ctx.scope().put_notetype(&notetype)?)
    })
    .unwrap();
    let index = provider.index();
    let copied = index.notetypes().iter().find(|n| n.id == id).unwrap();
    assert_eq!((copied.name.as_str(), copied.fields[0].as_str()), ("Copied", "Prompt"));
    assert_current(&h, &provider);
    c.run(setup_spec(), move |ctx| Ok(ctx.scope().remove_notetype(id)?)).unwrap();
    assert!(!provider.index().notetypes().iter().any(|n| n.id == id));
    assert_current(&h, &provider);

    // A reader keeps the index it was handed.
    let held = provider.index();
    decks::ops::add(c, "Later".into()).unwrap();
    assert!(!deck_names(&held).iter().any(|(name, _)| name == "Later"));
    assert!(deck_names(&provider.index()).iter().any(|(name, _)| name == "Later"));
}

#[test]
fn a_whole_collection_change_rereads_everything() {
    let (h, provider) = open();
    let id = {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        reader.deck_rows().unwrap()[0].id
    };
    let db = PlainDatabase::open(&h.path).unwrap();
    db.execute(
        "UPDATE decks SET name = 'Outside' WHERE id = ?1",
        &[meguri_storage::raw::RawValue::Integer(id.0)],
    )
    .unwrap();
    db.execute("INSERT INTO tags (tag, usn, collapsed, config) VALUES ('external', 0, 0, NULL)", &[])
        .unwrap();
    drop(db);
    assert!(h.engine.resume().unwrap().external_change);
    let index = provider.index();
    assert!(index.decks().iter().any(|d| d.id == id && d.name == "Outside"));
    assert!(index.tags().iter().any(|t| t == "external"));
    assert_current(&h, &provider);
}
