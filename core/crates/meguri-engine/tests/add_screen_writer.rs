//! Production Add-screen writer, using U27 persistence and undo throughout.
mod common;

use common::notes::{input, kind};
use common::{Harness, fresh};
use meguri_engine::{Collection, EngineError, OpSource, UndoScope, notes, notetypes};
use meguri_model::ids::{DeckId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, Table};

fn request(c: &Collection, id: NotetypeId) -> notes::AddScreenNote {
    let nt = c.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap();
    notes::AddScreenNote {
        expected_generation: c.generation(),
        notetype_id: id,
        kind: nt.config.kind,
        field_layout: nt.fields.iter().map(|f| notes::AddFieldIdentity {
            ord: f.ord, id: f.config.id, name: f.name.clone(),
        }).collect(),
        notetype_base: notetypes::AggregateDigest::of(&nt),
        fields: vec!["cafe\u{301}\u{1}".into(), "<b>answer</b>".into()],
        tags: vec!["WriterGuard".into()],
        deck_id: DeckId(1),
    }
}

// Whole persisted note/card/tag/config/col images, including allocation
// counters and defaults. Refusals must preserve stamps and register no tags.
fn rows(c: &Collection) -> Vec<RowImage> {
    let r = c.reader(ReaderRole::Query).unwrap();
    let mut rows = vec![r.row_image(Table::Col, &RowKey::Id(1)).unwrap().unwrap()];
    for table in [Table::Notes, Table::Cards] {
        for id in r.ids_after(table, None, 100).unwrap() {
            rows.push(r.row_image(table, &RowKey::Id(id)).unwrap().unwrap());
        }
    }
    for key in r.config_keys().unwrap() {
        rows.push(r.row_image(Table::Config, &RowKey::text(&key)).unwrap().unwrap());
    }
    for tag in r.tag_rows().unwrap() {
        rows.push(r.row_image(Table::Tags, &RowKey::text(&tag.name)).unwrap().unwrap());
    }
    rows
}

fn refused(c: &Collection, input: notes::AddScreenNote, error: EngineError) {
    let before = rows(c);
    let undo = c.peek_undo(UndoScope::Global).unwrap();
    let redo = c.peek_redo(UndoScope::Global).unwrap();
    let seq = c.last_seq();
    assert_eq!(notes::add_for_screen(c, input).unwrap_err(), error);
    assert_eq!(rows(c), before);
    assert_eq!(c.peek_undo(UndoScope::Global).unwrap(), undo);
    assert_eq!(c.peek_redo(UndoScope::Global).unwrap(), redo);
    assert_eq!(c.last_seq(), seq, "no successful op or change-stream sequence");
}

fn stale(id: NotetypeId) -> EngineError {
    EngineError::Stale { table: Table::Notetypes, key: id.0.to_string() }
}

#[test]
fn replaced_generation_refuses_even_when_successor_has_identical_ids_and_layout() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let captured = request(&h.collection, id);
    let expected = captured.expected_generation;
    h.engine.close().unwrap();
    let successor = h.engine.open_raw(&h.path, common::open_config(&h.clock)).unwrap();
    assert_ne!(successor.generation(), expected);
    // Models the FFI's single current Collection resolved after replacement.
    refused(&successor, captured.clone(), EngineError::StaleGeneration {
        planned: expected, current: successor.generation(),
    });
    // Also retain the executor's generation protection on an old handle.
    assert!(matches!(notes::add_for_screen(&h.collection, captured),
        Err(EngineError::StaleGeneration { .. })));
}

#[test]
fn same_count_reorder_checks_stored_names_ids_and_ordinals_independently_of_digest() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let captured = request(&h.collection, id);
    let nt = h.collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap();
    // Fixture-only incoming structural change, preserving the identities as
    // they move. The operation under test is always the production writer.
    h.collection.run(common::tagging::setup_spec(), move |ctx| {
        let mut reordered = nt.clone();
        reordered.fields.swap(0, 1);
        for (ord, field) in reordered.fields.iter_mut().enumerate() {
            field.ord = ord as u32;
        }
        let mut layout = meguri_storage::journal::LayoutReport::identity(&nt)?;
        layout.field_map = vec![1, 0];
        let mut report = meguri_storage::journal::StructureReport::default();
        report.layouts.insert(id.0, layout);
        let token = ctx.scope().start_structure_report(report)?;
        ctx.scope().put_notetype(&reordered)?;
        ctx.scope().complete_structure_report(token)?;
        Ok(())
    }).unwrap();
    let current = request(&h.collection, id);
    assert_eq!(current.field_layout.len(), captured.field_layout.len());
    assert_ne!(current.field_layout, captured.field_layout);
    let mut old_layout = captured;
    // Isolate exact field identity checking from the aggregate safeguard.
    old_layout.notetype_base = current.notetype_base;
    refused(&h.collection, old_layout, stale(id));
    for member in 0..4 {
        let mut changed = current.clone();
        match member {
            0 => changed.field_layout[0].ord += 1,
            1 => changed.field_layout[0].id = None,
            2 => changed.field_layout[0].name.push_str(" changed"),
            _ => changed.kind ^= 1,
        }
        refused(&h.collection, changed, stale(id));
    }
}

#[test]
fn cloze_template_edit_invalidates_captured_consent_with_unchanged_field_layout() {
    let h = fresh();
    let id = kind(&h, true, &["Front", "Back"], &["{{cloze:Front}}"]);
    let captured = request(&h.collection, id);
    let nt = h.collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap().unwrap();
    notetypes::update_template_formats(&h.collection, notetypes::TemplateFormatsUpdate {
        notetype: id,
        base: notetypes::AggregateDigest::of(&nt),
        template: notetypes::TemplateIdentity::of(&nt.templates[0]),
        front: "{{cloze:Back}}".into(),
        back: nt.templates[0].config.a_format.clone(),
        css: None,
    }, meguri_engine::CancelToken::new(), meguri_engine::ProgressSink::new()).unwrap();
    assert_eq!(request(&h.collection, id).field_layout, captured.field_layout);
    refused(&h.collection, captured, stale(id));
}

#[test]
fn invalid_decks_refuse_without_fallback_and_preserve_redo() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let mut missing = request(&h.collection, id);
    missing.deck_id = DeckId(987654);
    // Legacy A1 explicitly tolerates the same missing deck and creates cards.
    let mut legacy = input(id, &["legacy", "back"]);
    legacy.deck_id = missing.deck_id;
    let added = notes::add(&h.collection, legacy, OpSource::UserUndoable).unwrap();
    assert_eq!(added.value.cards.len(), 1);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(h.collection.peek_redo(UndoScope::Global).unwrap().name(), Some("Add Note"));
    refused(&h.collection, missing, EngineError::NotFound {
        table: Table::Decks, key: "987654".into(),
    });
    h.collection.run(common::tagging::setup_spec(), |ctx| {
        let mut deck = ctx.scope().deck(DeckId(1))?.unwrap();
        deck.kind = meguri_model::proto::ProtoBlob::new(meguri_model::proto::deck::DeckKindMessage::new(
            meguri_model::proto::deck::DeckKind::Filtered(Default::default()),
        ));
        ctx.scope().put(&deck)?;
        Ok(())
    }).unwrap();
    refused(&h.collection, request(&h.collection, id),
        EngineError::DeckRefused(meguri_engine::decks::Refusal::FilteredDeck));
}

#[test]
fn missing_notetype_refuses_before_any_note_tag_or_default_write() {
    let h = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let mut missing = request(&h.collection, id);
    missing.notetype_id = NotetypeId(987654);
    refused(&h.collection, missing, EngineError::NotFound {
        table: Table::Notetypes, key: "987654".into(),
    });
}

#[test]
fn valid_screen_add_persists_real_note_cards_defaults_and_one_undo_step() {
    let h: Harness = fresh();
    let id = kind(&h, false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let deck = meguri_engine::decks::add(&h.collection, "Chosen Add deck".into()).unwrap().value.id;
    let before = rows(&h.collection);
    let previous = h.collection.peek_undo(UndoScope::Global).unwrap();
    let mut selected = request(&h.collection, id);
    selected.deck_id = deck;
    let saved = notes::add_for_screen(&h.collection, selected).unwrap();
    assert_eq!(saved.undo.unwrap().name, "Add Note");
    assert_eq!(saved.changes.source, OpSource::UserUndoable);
    assert_eq!(saved.changes.notes.len(), 1);
    assert_eq!(saved.changes.cards.len(), 2);
    assert_eq!(saved.value.cards.len(), 2);
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let note = r.note(saved.value.id).unwrap().unwrap();
    assert_eq!(note.notetype_id, id);
    assert_eq!(note.fields, "café\u{1f}<b>answer</b>");
    for (ord, card) in saved.value.cards.iter().enumerate() {
        let card = r.card(*card).unwrap().unwrap();
        assert_eq!(card.note_id, note.id);
        assert_eq!(card.deck_id, deck);
        assert_eq!(usize::from(card.ord), ord);
    }
    for (key, value) in [(format!("_nt_{}_lastDeck", id.0), deck.0),
        (format!("_deck_{}_lastNotetype", deck.0), id.0), ("curModel".into(), id.0)] {
        assert_eq!(r.config_value(&key).unwrap(), Some(meguri_storage::ConfigValue::from(value)));
    }
    drop(r);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h.collection), before);
    assert_eq!(h.collection.peek_undo(UndoScope::Global).unwrap(), previous);
    h.collection.redo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.note(saved.value.id).unwrap().is_some());
    assert!(saved.value.cards.iter().all(|id| r.card(*id).unwrap().is_some()));
}
