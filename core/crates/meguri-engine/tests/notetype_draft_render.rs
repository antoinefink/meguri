//! The Cards tab's live preview (U283, `notetypes/workspace` W9;
//! `render/preview` V2 to V11): which note renders, unsaved templates and
//! styling, the draft as the save would store it (renames, reorders), Fill
//! Empty Fields, cloze cards, template errors, the simulated type-in, the
//! refusals, and that no preview writes a row or touches the undo history.

mod common;

use common::Harness;
use meguri_engine::EngineError;
use meguri_engine::notes;
use meguri_engine::notetypes::draft_render::{DraftRender, SAMPLE_CLOZE, note_ids, render_draft};
use meguri_engine::notetypes::structure::StructureDraft;
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind, StockKind};
use meguri_engine::rendering::{Outcome, Platform};
use meguri_model::changes::OpSource;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, Table};

fn pair_type(h: &Harness, name: &str) -> NotetypeId {
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: name.to_owned(),
            kind: NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into(), "Extra".into()],
            templates: vec![NewTemplate {
                name: "Forward".into(),
                front: "<b>{{Front}}</b>".into(),
                back: "{{FrontSide}}<hr id=answer>{{Back}}".into(),
                ..Default::default()
            }],
            css: Some(".card { color: teal; }".into()),
        },
    )
    .unwrap()
    .value
    .id
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

fn draft(h: &Harness, id: NotetypeId) -> StructureDraft {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    StructureDraft::of(&reader.notetype(id).unwrap().unwrap())
}

fn render(h: &Harness, d: &StructureDraft, note: Option<NoteId>, target: u32, fill: bool) -> DraftRender {
    render_draft(&h.collection, d, note, target, fill, Platform::Mac).unwrap()
}

fn question(r: &DraftRender) -> &str {
    r.card.question.as_deref().unwrap()
}

fn answer(r: &DraftRender) -> &str {
    r.card.answer.as_deref().unwrap()
}

/// Every row a preview could change, the collection row and the undo and
/// redo names: equal before and after, so nothing was written and the
/// history is the one it was.
fn everything(h: &Harness) -> (Vec<RowImage>, Option<String>, Option<String>) {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let mut rows = vec![reader.row_image(Table::Col, &RowKey::Id(1)).unwrap().unwrap()];
    for id in notetypes::read::ids(&*reader).unwrap() {
        rows.push(reader.row_image(Table::Notetypes, &RowKey::Id(id.0)).unwrap().unwrap());
        for note in reader.notetype_notes_after(id, None, 100_000).unwrap() {
            rows.push(reader.row_image(Table::Notes, &RowKey::Id(note.id.0)).unwrap().unwrap());
            for card in reader.cards_of_note(note.id).unwrap() {
                rows.push(reader.row_image(Table::Cards, &RowKey::Id(card.id.0)).unwrap().unwrap());
            }
        }
    }
    let name = |scope| h.collection.peek_undo(scope).unwrap().name().map(str::to_owned);
    let redo = h
        .collection
        .peek_redo(meguri_engine::UndoScope::Global)
        .unwrap()
        .name()
        .map(str::to_owned);
    (rows, name(meguri_engine::UndoScope::Global), redo)
}

#[test]
fn the_given_note_renders_and_another_types_note_is_a_new_empty_one() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let first = add(&h, id, &["neko", "cat", ""], &["animal", "marked"]);
    let newest = add(&h, id, &["inu", "dog", ""], &[]);
    let other_type = pair_type(&h, "Other");
    let foreign = add(&h, other_type, &["x", "y", ""], &[]);
    let d = draft(&h, id);

    let shown = render(&h, &d, Some(newest), 0, false);
    assert_eq!(shown.note, Some(newest));
    assert!(question(&shown).contains("<b>inu</b>"), "{}", question(&shown));
    assert!(answer(&shown).contains("dog"));
    // The draft's CSS in the style block, and the stored card behind CardID.
    assert!(question(&shown).contains(".card { color: teal; }"));
    assert_ne!(shown.card.card_id.0, 0);
    assert!(matches!(shown.card.comparable.outcome, Outcome::Rendered));

    let tagged = render(&h, &d, Some(first), 0, false);
    assert!(tagged.card.marked);

    // Another type's note, and one that does not exist: the new empty note.
    for note in [Some(foreign), Some(NoteId(424_242)), None] {
        let empty = render(&h, &d, note, 0, false);
        assert_eq!(empty.note, None);
        assert_eq!(empty.card.card_id.0, 0);
        assert!(matches!(empty.card.comparable.outcome, Outcome::Blank(_)));
    }
    assert_eq!(note_ids(&h.collection, id).unwrap(), vec![first, newest]);
    assert!(matches!(note_ids(&h.collection, NotetypeId(9)), Err(EngineError::NotFound { .. })));
}

#[test]
fn unsaved_templates_styling_renames_and_reorders_render_as_the_save_would_store_them() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let note = add(&h, id, &["neko", "cat", "pet"], &[]);
    let mut d = draft(&h, id);
    d.templates[0].front = "<i>{{Front}}</i> {{Card}} {{Type}}".into();
    d.templates[0].name = "Ahead".into();
    d.name = "Pair Draft".into();
    d.css = ".card { color: plum; }".into();
    // Renamed in the draft: the text still says {{Front}}, which the save
    // rewrites; Extra moves first and keeps its value.
    d.fields[0].name = "Prompt".into();
    let extra = d.fields.pop().unwrap();
    d.fields.insert(0, extra);
    let r = render(&h, &d, Some(note), 0, false);
    let q = question(&r);
    assert!(q.contains("<i>neko</i> Ahead Pair Draft"), "{q}");
    assert!(q.contains(".card { color: plum; }"));
    assert!(!q.contains("teal"));
    assert_eq!(r.card.css, ".card { color: plum; }");

    // A removed field's reference goes as the save's rewrite removes it.
    let mut removed = draft(&h, id);
    removed.fields.retain(|f| f.name != "Back");
    let r = render(&h, &removed, Some(note), 0, false);
    assert!(!answer(&r).contains("cat"));
    assert!(matches!(r.card.comparable.outcome, Outcome::Rendered));
}

#[test]
fn fill_empty_fields_names_each_empty_field_and_fills_cloze_references_with_the_sample() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let note = add(&h, id, &["neko", "", ""], &[]);
    let mut d = draft(&h, id);
    d.templates[0].back = "{{FrontSide}}<hr id=answer>{{Back}}|{{#Extra}}has {{Extra}}{{/Extra}}".into();
    let r = render(&h, &d, Some(note), 0, true);
    assert!(answer(&r).contains("(Back)|has (Extra)"), "{}", answer(&r));
    assert!(question(&r).contains("<b>neko</b>"));
    // The new empty note with Fill Empty Fields on: no blank card.
    let empty = render(&h, &d, None, 0, true);
    assert!(question(&empty).contains("<b>(Front)</b>"));
    assert!(matches!(empty.card.comparable.outcome, Outcome::Rendered));
    // Off: review's blank front.
    assert!(matches!(render(&h, &d, None, 0, false).card.comparable.outcome, Outcome::Blank(_)));

    // A standard question that names a field through `cloze:` gets the sample.
    let mut clozed = draft(&h, id);
    clozed.templates[0].front = "{{cloze:Front}}".into();
    let r = render(&h, &clozed, None, 0, true);
    assert!(question(&r).contains("cloze deletion"), "{}", question(&r));
    assert_eq!(SAMPLE_CLOZE, "This is a {{c1::sample}} cloze deletion.");
}

#[test]
fn a_cloze_draft_renders_its_cloze_card_and_lists_its_numbers() {
    let h = common::fresh();
    let id = notetypes::add_stock(&h.collection, StockKind::Cloze, Some("Gaps".into())).unwrap().value.id;
    let note = add(&h, id, &["{{c1::Paris}} and {{c3::Rome}}", ""], &[]);
    let d = draft(&h, id);
    let first = render(&h, &d, Some(note), 0, false);
    assert_eq!(first.cloze_numbers, vec![1, 3]);
    // Card 1 hides Paris and shows Rome; card 3 the other way round.
    assert!(question(&first).contains("[...]</span> and") && question(&first).contains(">Rome</span>"), "{}", question(&first));
    let third = render(&h, &d, Some(note), 2, false);
    assert!(question(&third).contains(">Paris</span> and") && question(&third).contains("[...]"), "{}", question(&third));
    assert_eq!(third.card.ordinal, 2);
    // A number the note does not have: listed, and the card is blank.
    let second = render(&h, &d, Some(note), 1, false);
    assert_eq!(second.cloze_numbers, vec![1, 2, 3]);
    assert!(matches!(second.card.comparable.outcome, Outcome::Blank(_)));
    // No notes: the sample shows card 1, and the list comes from the unfilled values.
    let sample = render(&h, &d, None, 0, true);
    assert!(question(&sample).contains("cloze deletion"), "{}", question(&sample));
    assert_eq!(sample.cloze_numbers, vec![1]);
}

#[test]
fn template_errors_are_an_outcome_and_the_type_in_is_simulated() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let note = add(&h, id, &["neko", "cat", ""], &[]);
    let mut broken = draft(&h, id);
    broken.templates[0].front = "{{#Front}}open".into();
    let r = render(&h, &broken, Some(note), 0, false);
    assert!(matches!(r.card.comparable.outcome, Outcome::Error { .. }));
    let mut unknown = draft(&h, id);
    unknown.templates[0].back = "{{Nowhere}}".into();
    assert!(matches!(render(&h, &unknown, Some(note), 0, false).card.comparable.outcome, Outcome::Error { .. }));

    let mut typed = draft(&h, id);
    typed.templates[0].front = "{{Front}} [[type:Back]]".into();
    typed.templates[0].back = "{{FrontSide}}<hr id=answer>[[type:Back]]".into();
    let r = render(&h, &typed, Some(note), 0, false);
    assert!(question(&r).contains("value='example' readonly='readonly'"), "{}", question(&r));
    assert!(answer(&r).contains("<span class=typeBad>s</span>"));
    assert_eq!(r.card.type_in, None);
}

#[test]
fn refusals_come_before_any_render() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let d = draft(&h, id);
    assert!(matches!(
        render_draft(&h.collection, &d, None, 1, false, Platform::Mac),
        Err(EngineError::NotFound { .. })
    ));
    assert!(matches!(
        render_draft(&h.collection, &d, None, 65_535, false, Platform::Mac),
        Err(EngineError::InvalidArgument { .. })
    ));
    let mut stale = d.clone();
    stale.base = notetypes::AggregateDigest([0; 32]);
    assert!(matches!(
        render_draft(&h.collection, &stale, None, 0, false, Platform::Mac),
        Err(EngineError::Stale { .. })
    ));
}

#[test]
fn a_preview_writes_nothing_and_keeps_the_undo_history() {
    let h = common::fresh();
    let id = pair_type(&h, "Pair");
    let note = add(&h, id, &["neko", "", ""], &["animal"]);
    let before = everything(&h);
    assert_eq!(before.1.as_deref(), Some("Add Note"));
    let mut d = draft(&h, id);
    d.templates[0].front = "{{Front}}{{Back}}".into();
    d.fields[1].name = "Answer".into();
    for fill in [false, true] {
        for target in [0, 3] {
            let _ = render_draft(&h.collection, &d, Some(note), target, fill, Platform::Ios);
        }
    }
    let _ = note_ids(&h.collection, id).unwrap();
    assert_eq!(everything(&h), before);
}
