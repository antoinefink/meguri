//! The Note Types screens' reads (U267): A2's ordered list and note counts,
//! W3.1's notes with content, D2's `draft_summary` (the save's own count),
//! W8's notes without a card and W12's delete counts, each against a
//! collection with known answers, and that none of them writes.
//!
//! Emptiness: W3.1, D2 and the draft save's confirmed losses all test
//! `text/field-emptiness` E1 on the stored value, so `<br>` alone is empty
//! and `<br  >` (two spaces) is content. W8 tests G2's non-empty names, E1
//! too. (Change Note Type's `field_content` is another rule, stored text,
//! and is not read here.)

mod common;

use std::sync::{Arc, Mutex};

use common::Harness;
use meguri_engine::notetypes::counts::{
    self, BlankFront, FieldContentCounts, RemovalCounts, field_content_counts, notes_without_card, removal_counts,
};
use meguri_engine::notetypes::remove::{RemoveNotetype, remove};
use meguri_engine::notetypes::structure::{
    self, ConfirmedLosses, DraftChange, DraftSummary, DraftTemplate, Losses, StructureDraft, StructureRefusal,
};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind, StockKind};
use meguri_engine::{CancelToken, EngineError, OpSpec, Progress, ProgressListener, ProgressSink, SchemaReason, notes};
use meguri_model::changes::OpSource;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_storage::{ReadRows, ReaderRole, RowImage, RowKey, Table};

fn new_type(h: &Harness, name: &str, fields: &[&str], templates: &[(&str, &str)]) -> NotetypeId {
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: name.to_owned(),
            kind: NotetypeKind::Standard,
            fields: fields.iter().map(|f| (*f).to_owned()).collect(),
            templates: templates
                .iter()
                .map(|(name, front)| NewTemplate {
                    name: (*name).to_owned(),
                    front: (*front).to_owned(),
                    back: "{{FrontSide}}".to_owned(),
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

/// `Trio`: fields Front, Back, Extra; Forward `{{Front}}`, Reverse `{{Back}}`.
fn trio(h: &Harness) -> NotetypeId {
    new_type(h, "Trio", &["Front", "Back", "Extra"], &[("Forward", "{{Front}}"), ("Reverse", "{{Back}}")])
}

fn add_tagged(h: &Harness, mid: NotetypeId, fields: &[&str], tags: &[&str]) -> NoteId {
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

fn add(h: &Harness, mid: NotetypeId, fields: &[&str]) -> NoteId {
    add_tagged(h, mid, fields, &[])
}

/// A raw note, stored as given with no card (an import or a damaged
/// collection may hold one).
fn seed(h: &Harness, mid: NotetypeId, fields: &str, guid: &str) -> NoteId {
    let mut note = common::note(&[]);
    note.notetype_id = mid;
    note.fields = fields.to_owned();
    note.guid = guid.to_owned();
    h.collection
        .run(OpSpec::named("seed", OpSource::UserNonUndoable).skip_undo(), move |ctx| {
            Ok(ctx.scope().add_note(&note)?)
        })
        .unwrap()
        .value
}

fn raw_card(h: &Harness, note: NoteId, ord: u16) {
    h.collection
        .run(OpSpec::named("raw_card", OpSource::UserNonUndoable).skip_undo(), move |ctx| {
            Ok(ctx.scope().add_card(&common::card(note, ord))?)
        })
        .unwrap();
}

fn read(h: &Harness, id: NotetypeId) -> Notetype {
    h.collection
        .reader(ReaderRole::Query)
        .unwrap()
        .notetype(id)
        .unwrap()
        .unwrap()
}

fn draft(h: &Harness, id: NotetypeId) -> StructureDraft {
    let mut draft = StructureDraft::of(&read(h, id));
    draft.skip_checks = true;
    draft
}

fn template(name: &str, front: &str) -> DraftTemplate {
    DraftTemplate {
        origin: None,
        name: name.to_owned(),
        front: front.to_owned(),
        back: "{{FrontSide}}".to_owned(),
        browser_front: String::new(),
        browser_back: String::new(),
        deck: 0,
        browser_font_name: String::new(),
        browser_font_size: 0,
    }
}

fn summary(h: &Harness, draft: &StructureDraft) -> Result<DraftSummary, EngineError> {
    structure::draft_summary(&h.collection, draft, &CancelToken::new(), &ProgressSink::new())
}

fn contents(h: &Harness, id: NotetypeId) -> Result<FieldContentCounts, EngineError> {
    field_content_counts(&h.collection, id, &CancelToken::new(), &ProgressSink::new())
}

fn blank(h: &Harness, draft: &StructureDraft) -> Result<Vec<BlankFront>, EngineError> {
    notes_without_card(&h.collection, draft, &CancelToken::new(), &ProgressSink::new())
}

fn census(h: &Harness, id: NotetypeId) -> Result<RemovalCounts, EngineError> {
    removal_counts(&h.collection, id, &CancelToken::new(), &ProgressSink::new())
}

/// The confirmed losses a caller passes after showing `summary` (D5 step 5).
fn confirmed(summary: &DraftSummary) -> ConfirmedLosses {
    let mut confirmed = ConfirmedLosses::default();
    for change in &summary.changes {
        match change {
            DraftChange::FieldRemoved { ord, notes_with_content, .. } => {
                confirmed.fields.insert(*ord, *notes_with_content);
            }
            DraftChange::TemplateRemoved { ord, cards_removed, .. } => {
                confirmed.templates.insert(*ord, *cards_removed);
            }
            _ => {}
        }
    }
    confirmed
}

/// Every stored row a read could change, with the collection row and the
/// undo state: equal before and after a read that writes nothing.
fn everything(h: &Harness) -> (Vec<RowImage>, Option<String>) {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let mut rows = vec![reader.row_image(Table::Col, &RowKey::Id(1)).unwrap().unwrap()];
    for id in notetypes::read::ids(&*reader).unwrap() {
        for note in reader.notetype_notes_after(id, None, 100_000).unwrap() {
            rows.push(reader.row_image(Table::Notes, &RowKey::Id(note.id.0)).unwrap().unwrap());
            for card in reader.cards_of_note(note.id).unwrap() {
                rows.push(reader.row_image(Table::Cards, &RowKey::Id(card.id.0)).unwrap().unwrap());
            }
        }
    }
    let undo = h
        .collection
        .peek_undo(meguri_engine::UndoScope::Global)
        .unwrap()
        .name()
        .map(str::to_owned);
    (rows, undo)
}

#[derive(Default)]
struct Recorder(Mutex<Vec<Progress>>);

impl ProgressListener for Recorder {
    fn on_progress(&self, progress: &Progress) {
        self.0.lock().unwrap().push(progress.clone());
    }
}

/// A2: names by `unicase` (the spec's example), and a note count for every
/// note type; a note whose `mid` names no note type is counted nowhere.
#[test]
fn the_list_is_in_unicase_order_with_a_note_count_for_every_type() {
    let h = common::fresh();
    let mut ids = Vec::new();
    for name in ["Zeta", "Äbc", "Straße", "_under", "alpha", "Strasse2"] {
        ids.push((name, new_type(&h, name, &["Front"], &[("Card 1", "{{Front}}")])));
    }
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let names: Vec<String> = counts::ordered_names(&*reader).unwrap().into_iter().map(|(_, n)| n).collect();
    let example = [
        "_under",
        "alpha",
        "Basic",
        "Basic (and reversed card)",
        "Basic (optional reversed card)",
        "Basic (type in the answer)",
        "Cloze",
        "Image Occlusion",
        "Straße",
        "Strasse2",
        "Zeta",
        "Äbc",
    ];
    let expected: Vec<&str> = example.iter().copied().filter(|n| names.iter().any(|m| m == n)).collect();
    assert_eq!(names, expected);
    assert!(names.len() >= 6 + 5, "{names:?}");
    drop(reader);

    let id = |name: &str| ids.iter().find(|(n, _)| *n == name).unwrap().1;
    add(&h, id("alpha"), &["one"]);
    add(&h, id("alpha"), &["two"]);
    add(&h, id("Zeta"), &["three"]);
    seed(&h, NotetypeId(424_242), "orphan", "orphan");
    let before = everything(&h);
    let counted = counts::list_note_counts(&h.collection).unwrap();
    assert_eq!(everything(&h), before);
    assert_eq!(counted.get(&id("alpha")), Some(&2));
    assert_eq!(counted.get(&id("Zeta")), Some(&1));
    assert_eq!(counted.get(&id("Äbc")), Some(&0));
    assert!(!counted.contains_key(&NotetypeId(424_242)));
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let all = notetypes::read::ids(&*reader).unwrap();
    assert_eq!(counted.keys().copied().collect::<Vec<_>>(), all);
    assert_eq!(counted.values().sum::<u64>(), 3);
}

/// W3.1: notes with content per stored ordinal by E1 (`<br>` and spacer
/// runs are empty; `&nbsp;`, media and `<br  >` are content), a note with
/// fewer values, a type with no notes, a cloze type; D2's removed-field
/// counts are the same numbers.
#[test]
fn notes_with_content_follow_e1_and_match_the_save_summary() {
    let h = common::fresh();
    let id = trio(&h);
    add(&h, id, &["a", "<br>", "x"]);
    add(&h, id, &["<img src=\"pic.jpg\">", "&nbsp;", "<BR /><div></div>\n "]);
    add(&h, id, &["c", "b", "<br  >"]);
    // One stored value only: Back and Extra are missing, not content.
    seed(&h, id, "only", "short");
    let before = everything(&h);
    let recorder = Arc::new(Recorder::default());
    let progress = ProgressSink::new();
    progress.set_listener(recorder.clone());
    let read = field_content_counts(&h.collection, id, &CancelToken::new(), &progress).unwrap();
    assert_eq!(everything(&h), before);
    assert_eq!(read, FieldContentCounts { notes: 4, fields: vec![4, 2, 2] });
    assert!(recorder.0.lock().unwrap().iter().any(|p| p.total == Some(4)));

    for (field, expected) in [("Back", 2), ("Extra", 2)] {
        let mut d = draft(&h, id);
        d.fields.retain(|f| f.name != field);
        let s = summary(&h, &d).unwrap();
        let removed: Vec<u64> = s
            .changes
            .iter()
            .filter_map(|c| match c {
                DraftChange::FieldRemoved { notes_with_content, .. } => Some(*notes_with_content),
                _ => None,
            })
            .collect();
        assert_eq!(removed, [expected]);
        assert!(s.loses_content);
    }

    let empty = new_type(&h, "Unused", &["Front", "Back"], &[("Card 1", "{{Front}}")]);
    assert_eq!(contents(&h, empty).unwrap(), FieldContentCounts { notes: 0, fields: vec![0, 0] });

    let cloze = notetypes::add_stock(&h.collection, StockKind::Cloze, Some("Gaps".into())).unwrap().value.id;
    add(&h, cloze, &["{{c1::alpha}} beta", "<br>"]);
    add(&h, cloze, &["{{c1::gamma}}", "note"]);
    assert_eq!(contents(&h, cloze).unwrap(), FieldContentCounts { notes: 2, fields: vec![2, 1] });

    assert!(matches!(contents(&h, NotetypeId(9_999)), Err(EngineError::NotFound { .. })));
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        field_content_counts(&h.collection, id, &cancel, &ProgressSink::new()),
        Err(EngineError::Cancelled)
    );
}

/// D2: the summary of a draft that removes a field with content, renames a
/// field, adds a card type and removes one that leaves a note with no card;
/// it writes nothing, and it is exactly what the save counts: the save with
/// no confirmation refuses with the same summary, and with the summary's
/// counts it commits with the same effects.
#[test]
fn the_draft_summary_is_the_saves_own_count() {
    let h = common::fresh();
    let id = trio(&h);
    add(&h, id, &["a", "b", "x"]);
    add(&h, id, &["c", "", "y"]);
    add(&h, id, &["e", "f", "<br>"]);
    // A note whose only card is a Reverse card.
    let lone = seed(&h, id, "\u{1f}g\u{1f}z", "lone");
    raw_card(&h, lone, 1);
    let mut d = draft(&h, id);
    d.fields.retain(|f| f.name != "Extra");
    d.fields[1].name = "Answer".to_owned();
    d.templates.retain(|t| t.name != "Reverse");
    d.templates.push(template("Third", "{{Front}}"));
    let before = everything(&h);
    let read = summary(&h, &d).unwrap();
    assert_eq!(everything(&h), before);
    assert_eq!(read.reasons, [SchemaReason::FieldsChanged, SchemaReason::TemplatesChanged]);
    assert_eq!(
        read.changes,
        [
            DraftChange::FieldRemoved {
                ord: 2,
                name: "Extra".into(),
                notes_with_content: 3,
            },
            DraftChange::FieldRenamed {
                from: "Back".into(),
                to: "Answer".into(),
            },
            // The lone note has no card left, so generation skips it (S7).
            DraftChange::TemplateAdded {
                name: "Third".into(),
                cards_created: 3,
            },
            DraftChange::TemplateRemoved {
                ord: 1,
                name: "Reverse".into(),
                cards_removed: 3,
                notes_without_cards: 1,
            },
        ]
    );
    assert!(read.loses_content);

    // The save's own read of the same rows, when nothing was confirmed.
    let refused = structure::apply(&h.collection, d.clone(), CancelToken::new(), ProgressSink::new()).unwrap_err();
    match refused {
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(losses)) => {
            assert_eq!(*losses, Losses::Draft(read.clone()));
        }
        other => panic!("not LossesChanged: {other:?}"),
    }
    assert_eq!(everything(&h), before);

    d.confirmed = confirmed(&read);
    let saved = structure::apply(&h.collection, d, CancelToken::new(), ProgressSink::new()).unwrap().value;
    assert_eq!(saved.reasons, read.reasons);
    assert_eq!((saved.cards_removed, saved.notes_without_cards, saved.cards_added), (3, 1, 3));
}

/// D2's other rows and boundaries: an unchanged draft, a rename alone (the
/// field rename op's draft), a field empty everywhere by E1, a type with no
/// notes, moves and the sort field, a cloze type, a removed card type with
/// no cards, and the planner's refusals.
#[test]
fn draft_summary_rows_and_refusals() {
    let h = common::fresh();
    let id = trio(&h);
    add(&h, id, &["a", "<br>", ""]);
    add(&h, id, &["c", "<div></div>", "<br/>"]);

    let unchanged = summary(&h, &draft(&h, id)).unwrap();
    assert_eq!(unchanged, DraftSummary::default());

    let mut renamed = draft(&h, id);
    renamed.fields[2].name = "Notes".into();
    let s = summary(&h, &renamed).unwrap();
    assert!(s.reasons.is_empty() && !s.loses_content);
    assert_eq!(s.changes, [DraftChange::FieldRenamed { from: "Extra".into(), to: "Notes".into() }]);

    // Back holds only spacer tags: no loss, though the fields changed.
    let mut emptied = draft(&h, id);
    emptied.fields.retain(|f| f.name != "Back");
    let s = summary(&h, &emptied).unwrap();
    assert_eq!(s.reasons, [SchemaReason::FieldsChanged]);
    assert!(!s.loses_content);
    assert!(s.changes.contains(&DraftChange::FieldRemoved { ord: 1, name: "Back".into(), notes_with_content: 0 }));

    // Front moved last and made the sort field again by identity: a move,
    // no sort change; then Back as the sort field.
    let mut moved = draft(&h, id);
    let front = moved.fields.remove(0);
    moved.fields.push(front);
    moved.sort_field = Some(2);
    let s = summary(&h, &moved).unwrap();
    assert_eq!(s.changes, [DraftChange::FieldsReordered]);
    moved.sort_field = Some(0);
    let s = summary(&h, &moved).unwrap();
    assert_eq!(s.changes, [DraftChange::FieldsReordered, DraftChange::SortFieldChanged { name: "Back".into() }]);
    assert_eq!(s.reasons, [SchemaReason::FieldsChanged, SchemaReason::SortFieldChanged]);

    // A card type with no cards removed: listed with no loss ("No cards.").
    let pair = new_type(&h, "Pair", &["Front", "Back"], &[("One", "{{Front}}"), ("Two", "{{Back}}")]);
    add(&h, pair, &["a", ""]);
    let mut d = draft(&h, pair);
    d.templates.retain(|t| t.name != "Two");
    let s = summary(&h, &d).unwrap();
    assert!(!s.loses_content);
    assert_eq!(
        s.changes,
        [DraftChange::TemplateRemoved { ord: 1, name: "Two".into(), cards_removed: 0, notes_without_cards: 0 }]
    );

    // A type with no notes.
    let unused = new_type(&h, "Unused", &["Front", "Back"], &[("Card 1", "{{Front}}")]);
    let mut d = draft(&h, unused);
    d.fields.pop();
    let s = summary(&h, &d).unwrap();
    assert_eq!(s.changes, [DraftChange::FieldRemoved { ord: 1, name: "Back".into(), notes_with_content: 0 }]);
    assert!(!s.loses_content);

    // A cloze type: its second field removed.
    let cloze = notetypes::add_stock(&h.collection, StockKind::Cloze, Some("Gaps".into())).unwrap().value.id;
    add(&h, cloze, &["{{c1::alpha}}", "kept"]);
    add(&h, cloze, &["{{c1::beta}}", "<br>"]);
    let mut d = draft(&h, cloze);
    d.fields.pop();
    let s = summary(&h, &d).unwrap();
    assert!(s.loses_content);
    assert!(matches!(s.changes[0], DraftChange::FieldRemoved { ord: 1, notes_with_content: 1, .. }));

    // Refusals: stale, invalid, cancelled.
    let stale = draft(&h, id);
    let mut other = draft(&h, id);
    other.css = ".card { color: red; }".into();
    structure::apply(&h.collection, other, CancelToken::new(), ProgressSink::new()).unwrap();
    assert!(matches!(summary(&h, &stale), Err(EngineError::Stale { .. })));
    let mut repeated = draft(&h, id);
    repeated.fields[1].origin = Some(0);
    assert!(matches!(summary(&h, &repeated), Err(EngineError::InvalidArgument { .. })));
    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        structure::draft_summary(&h.collection, &draft(&h, id), &cancel, &ProgressSink::new()),
        Err(EngineError::Cancelled)
    );
}

/// W8: per card type of the draft, the notes its front gives no card, as
/// card generation decides (conditionals, negated sections, `{{Tags}}`, a
/// front that does not parse, a renamed field), the deciding field when
/// one decides it, a note that generates no card, and no rows for a cloze
/// type. The draft's new card types then generate exactly for the notes
/// not listed.
#[test]
fn notes_without_card_follow_card_generation() {
    let h = common::fresh();
    let id = trio(&h);
    let n1 = add(&h, id, &["a", "b", "x"]);
    let n2 = add_tagged(&h, id, &["c", "d", ""], &["topic"]);
    let n3 = add(&h, id, &["e", "<br>", ""]);
    let n4 = add(&h, id, &["f", "", "x"]);
    let mut d = draft(&h, id);
    d.templates.push(template("Cond", "{{#Extra}}{{Back}}{{/Extra}}"));
    d.templates.push(template("Neg", "{{^Extra}}{{Back}}{{/Extra}}"));
    d.templates.push(template("Tagged", "{{Tags}}{{Extra}}"));
    let before = everything(&h);
    let read = blank(&h, &d).unwrap();
    assert_eq!(everything(&h), before);
    let rows = |notes: &[NoteId], field: Option<u32>| BlankFront {
        notes: notes.to_vec(),
        field,
    };
    let expected = vec![
        rows(&[], None),
        rows(&[n3, n4], Some(1)),
        rows(&[n2, n3, n4], None),
        rows(&[n1, n3, n4], None),
        rows(&[n3], None),
    ];
    assert_eq!(read, expected);

    // A rename in the draft: the fronts are read as the save rewrites them.
    let mut renamed = d.clone();
    renamed.fields[1].name = "Answer".into();
    assert_eq!(blank(&h, &renamed).unwrap(), expected);

    // A front that does not parse gives no note a card.
    let mut broken = draft(&h, id);
    broken.templates[0].front = "{{#Front}}{{Front}}".into();
    assert_eq!(blank(&h, &broken).unwrap()[0], rows(&[n1, n2, n3, n4], None));

    // The save generates the new card types for exactly the other notes
    // (every note keeps its Forward card, so generation visits them all).
    let mut save = d;
    save.skip_checks = true;
    structure::apply(&h.collection, save, CancelToken::new(), ProgressSink::new()).unwrap();
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for (ord, front) in read.iter().enumerate().skip(2) {
        let without: Vec<NoteId> = [n1, n2, n3, n4]
            .into_iter()
            .filter(|note| !reader.cards_of_note(*note).unwrap().iter().any(|c| usize::from(c.ord) == ord))
            .collect();
        assert_eq!(without, front.notes, "card type {ord}");
    }
    drop(reader);

    // A note that generates no card: an empty Front, no card at all.
    let other = trio_named(&h, "Other");
    let kept = add(&h, other, &["k", "", ""]);
    let none = seed(&h, other, "<br>\u{1f}\u{1f}", "nocard");
    let read = blank(&h, &draft(&h, other)).unwrap();
    assert_eq!(read[0], rows(&[none], Some(0)));
    assert_eq!(read[1], rows(&[kept, none], Some(1)));

    let cloze = notetypes::add_stock(&h.collection, StockKind::Cloze, Some("Gaps".into())).unwrap().value.id;
    add(&h, cloze, &["{{c1::alpha}}", ""]);
    assert_eq!(blank(&h, &draft(&h, cloze)).unwrap(), Vec::<BlankFront>::new());

    let cancel = CancelToken::new();
    cancel.cancel();
    assert_eq!(
        notes_without_card(&h.collection, &draft(&h, id), &cancel, &ProgressSink::new()),
        Err(EngineError::Cancelled)
    );
}

fn trio_named(h: &Harness, name: &str) -> NotetypeId {
    new_type(h, name, &["Front", "Back", "Extra"], &[("Forward", "{{Front}}"), ("Reverse", "{{Back}}")])
}

/// W12: the note type's notes and cards as the removal counts them, so the
/// removal confirmed with them commits and one confirmed with fewer cards
/// is `LossesChanged`; a type with no notes; an unknown id.
#[test]
fn removal_counts_are_what_the_removal_counts_again() {
    let h = common::fresh();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    add(&h, id, &["c", "", ""]);
    let lone = seed(&h, id, "\u{1f}g\u{1f}", "lone");
    raw_card(&h, lone, 1);
    let before = everything(&h);
    let read = census(&h, id).unwrap();
    assert_eq!(everything(&h), before);
    assert_eq!(read, RemovalCounts { notes: 3, cards: 4 });

    let unused = new_type(&h, "Unused", &["Front"], &[("Card 1", "{{Front}}")]);
    assert_eq!(census(&h, unused).unwrap(), RemovalCounts { notes: 0, cards: 0 });
    assert!(matches!(census(&h, NotetypeId(9_999)), Err(EngineError::NotFound { .. })));

    let short = RemoveNotetype {
        notetype: id,
        confirmed_notes: read.notes,
        confirmed_cards: read.cards - 1,
    };
    let refused = remove(&h.collection, short, CancelToken::new(), ProgressSink::new()).unwrap_err();
    assert_eq!(
        refused,
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(Box::new(Losses::Removal {
            notes: 3,
            cards: 4,
        })))
    );
    let request = RemoveNotetype {
        notetype: id,
        confirmed_notes: read.notes,
        confirmed_cards: read.cards,
    };
    let removed = remove(&h.collection, request, CancelToken::new(), ProgressSink::new()).unwrap().value;
    assert_eq!((removed.notes_removed, removed.cards_removed), (3, 4));
}
