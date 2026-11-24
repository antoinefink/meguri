//! Deck package import rules by construction (`specs/package/apkg-import.md`
//! I6 to I11, I17): note types without a merge, notes, decks, cards with
//! and without scheduling, ids, and the damaged packages Meguri repairs.
use meguri_engine::notetypes::{NewNotetype, NewTemplate, NotetypeKind};
use meguri_engine::{EngineError, OpSource, OpSpec, UndoScope};
use meguri_model::clock::Clock;
use meguri_model::deck::Deck;
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::note::Note;
use meguri_model::notetype::Notetype;
use meguri_model::proto::ProtoBlob;
use meguri_model::proto::deck::{DayLimit, DeckKind, DeckKindMessage, FilteredDeck};
use meguri_model::time::EpochSecs;
use meguri_package::apkg::{ApkgImportError, ImportOptions, UpdateCondition};
use meguri_storage::{ReadRows, ReaderRole, StampMode, WriteScope};
use meguri_testkit::normal_support::Fixture;

use crate::apkg_import::{ALL_OFF, apply, dump, export, import, inspect, note_ids};

/// A setup write with the rows as given.
pub fn raw(f: &Fixture, write: impl Fn(&mut WriteScope<'_>) -> Result<(), EngineError> + Send + 'static) {
    f.collection
        .run(
            OpSpec::named("test setup", OpSource::Maintenance).stamp(StampMode::CallerSupplied),
            move |ctx| write(ctx.scope()),
        )
        .unwrap();
}

/// One part of a test note type: its name and its stable id.
pub type Part<'a> = (&'a str, Option<i64>);

/// A note type with chosen ids, written as a new row.
pub fn notetype(id: i64, name: &str, mtime: i64, fields: &[Part<'_>], templates: &[(Part<'_>, &str)]) -> Notetype {
    let random = meguri_model::rng::SeededRandom::new(5);
    let mut built = meguri_engine::notetypes::from_scratch(
        &NewNotetype {
            name: name.to_owned(),
            kind: NotetypeKind::Standard,
            fields: fields.iter().map(|(name, _)| (*name).to_owned()).collect(),
            templates: templates
                .iter()
                .map(|((name, _), front)| NewTemplate {
                    name: (*name).to_owned(),
                    front: (*front).to_owned(),
                    back: "{{FrontSide}}".to_owned(),
                    ..NewTemplate::default()
                })
                .collect(),
            css: None,
        },
        &random,
    );
    built.id = NotetypeId(id);
    built.mtime = EpochSecs(mtime);
    for (at, (field, (_, id))) in built.fields.iter_mut().zip(fields).enumerate() {
        field.ord = at as u32;
        field.config.id = *id;
    }
    for (at, (template, ((_, id), _))) in built.templates.iter_mut().zip(templates).enumerate() {
        template.ord = at as u32;
        template.config.id = *id;
    }
    built
}

pub fn put_notetype(f: &Fixture, built: Notetype) {
    raw(f, move |scope| {
        scope.put_notetype(&built)?;
        Ok(())
    });
}

pub fn stored_notetype(f: &Fixture, id: i64) -> Option<Notetype> {
    f.collection.reader(ReaderRole::Query).unwrap().notetype(NotetypeId(id)).unwrap()
}

pub fn notetype_count(f: &Fixture) -> usize {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    meguri_engine::notetypes::read::ids(&*reader.snapshot().unwrap()).unwrap().len()
}

/// A note with a chosen GUID, through the production add.
pub fn add(f: &Fixture, notetype: i64, guid: &str, fields: &[&str], deck: i64) -> NoteId {
    meguri_engine::notes::add(
        &f.collection,
        meguri_engine::notes::AddNote {
            notetype_id: NotetypeId(notetype),
            fields: fields.iter().map(|field| (*field).to_owned()).collect(),
            tags: vec![],
            deck_id: DeckId(deck),
            guid: Some(guid.to_owned()),
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value
    .id
}

pub fn note(f: &Fixture, id: NoteId) -> Note {
    f.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap()
}

pub fn note_by_guid(f: &Fixture, guid: &str) -> Option<Note> {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    reader.notes_after(None, 100_000).unwrap().into_iter().find(|note| note.guid == guid)
}

pub fn scm(f: &Fixture) -> i64 {
    f.collection.reader(ReaderRole::Query).unwrap().col().unwrap().schema_modified.0
}

pub fn deck_named(f: &Fixture, human: &str) -> Option<Deck> {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    reader.deck_rows().unwrap().into_iter().find(|deck| deck.human_name() == human)
}

pub fn two_fields() -> Vec<Part<'static>> {
    vec![("Front", Some(11)), ("Back", Some(12))]
}

pub fn one_template() -> Vec<(Part<'static>, &'static str)> {
    vec![(("Card 1", Some(21)), "{{Front}}")]
}

fn set_mtime(f: &Fixture, id: NoteId, mtime: i64) {
    crate::apkg_import::edit_note(f, id, move |note| note.mtime.0 = mtime);
}

#[test]
fn a_note_type_the_target_lacks_is_added_under_its_id_with_its_stamps() {
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let current = target.collection.reader(ReaderRole::Query).unwrap().config_value("curModel").unwrap();
    let before = scm(&target);
    let result = import(&target, &package, ALL_OFF).unwrap();
    let added = stored_notetype(&target, 5000).unwrap();
    assert_eq!((added.name.as_str(), added.mtime.0, added.usn.0), ("Pair", 100, -1));
    assert_eq!(added.config.original_id, Some(5000));
    assert_eq!(result.counts.notetypes_added, 1, "a note type no package note uses is not taken");
    assert_eq!(scm(&target), before, "adding a note type sets no scm");
    assert_eq!(target.collection.reader(ReaderRole::Query).unwrap().config_value("curModel").unwrap(), current);
    assert!(result.safety_backup.is_none());
    let stored = note_by_guid(&target, "g-one").unwrap();
    assert_eq!(target.collection.reader(ReaderRole::Query).unwrap().cards_of_note(stored.id).unwrap().len(), 1);
}

#[test]
fn a_same_schema_note_type_is_updated_only_when_the_rule_holds() {
    for (rule, target_mtime, updated) in [
        (UpdateCondition::IfNewer, 100, true),
        (UpdateCondition::IfNewer, 200, false),
        (UpdateCondition::IfNewer, 300, false),
        (UpdateCondition::Always, 300, true),
        (UpdateCondition::Always, 200, false),
        (UpdateCondition::Never, 100, false),
    ] {
        let source = Fixture::new(0);
        let fields = [("Front", Some(11)), ("Answer", Some(12))];
        put_notetype(&source, notetype(5000, "Pair", 200, &fields, &one_template()));
        add(&source, 5000, "g-one", &["sun", "soleil"], 1);
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(0);
        put_notetype(&target, notetype(5000, "Pair", target_mtime, &two_fields(), &one_template()));
        let kept = add(&target, 5000, "g-other", &["moon", "lune"], 1);
        let stored = note(&target, kept);
        let before = scm(&target);
        let count = notetype_count(&target);
        import(&target, &package, ImportOptions { update_notetypes: rule, ..ALL_OFF }).unwrap();
        let after = stored_notetype(&target, 5000).unwrap();
        if updated {
            assert_eq!((after.fields[1].name.as_str(), after.mtime.0, after.usn.0), ("Answer", 200, -1), "{rule:?}");
        } else {
            assert_eq!((after.fields[1].name.as_str(), after.mtime.0), ("Back", target_mtime), "{rule:?}");
        }
        assert_eq!(notetype_count(&target), count, "no copy is added");
        assert_eq!(scm(&target), before);
        assert_eq!(note(&target, kept), stored, "an update of equal layout rewrites no note");
        assert_eq!(note_by_guid(&target, "g-one").unwrap().notetype_id.0, 5000);
    }
}

#[test]
fn another_schema_adds_a_copy_once_and_its_guid_matches_are_conflicting() {
    let source = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(13))];
    put_notetype(&source, notetype(5000, "Pair", 200, &wider, &one_template()));
    add(&source, 5000, "g-shared", &["sun", "soleil", "note"], 1);
    add(&source, 5000, "g-new", &["star", "etoile", ""], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    target.clock.step_ms(10_000);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let held = add(&target, 5000, "g-shared", &["sun", "local"], 1);
    set_mtime(&target, held, 50);
    let stored = note(&target, held);
    let options = ImportOptions {
        update_notes: UpdateCondition::Always,
        ..ALL_OFF
    };
    let result = import(&target, &package, options).unwrap();
    assert_eq!(result.counts.notetypes_copied, 1);
    assert_eq!(result.log.conflicting.len(), 1);
    assert_eq!(result.log.conflicting[0].existing_id, Some(held.0));
    assert_eq!(note(&target, held), stored, "a conflicting target note keeps its bytes");
    let fresh = note_by_guid(&target, "g-new").unwrap();
    let copy = stored_notetype(&target, fresh.notetype_id.0).unwrap();
    assert_ne!(copy.id.0, 5000);
    assert_eq!((copy.name.as_str(), copy.config.original_id, copy.fields.len()), ("Pair+", Some(5000), 3));
    let current = target.collection.reader(ReaderRole::Query).unwrap().config_value("curModel").unwrap();
    assert_eq!(current.and_then(|value| value.as_i64()), Some(copy.id.0));
    assert_eq!(stored_notetype(&target, 5000).unwrap().fields.len(), 2);
    // Importing again finds the copy: no third note type, the note a duplicate.
    let count = notetype_count(&target);
    let again = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!(notetype_count(&target), count);
    assert_eq!((again.counts.notetypes_copied, again.log.duplicate.len(), again.log.conflicting.len()), (0, 1, 1));
}

#[test]
fn an_update_with_another_sort_field_rewrites_the_notes_and_marks_the_schema() {
    let source = Fixture::new(0);
    let mut incoming = notetype(5000, "Pair", 200, &two_fields(), &one_template());
    incoming.config.sort_field_idx = 1;
    put_notetype(&source, incoming);
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let held = add(&target, 5000, "g-one", &["sun", "soleil"], 1);
    let other = add(&target, 5000, "g-two", &["moon", "lune"], 1);
    set_mtime(&target, held, 50);
    set_mtime(&target, other, 50);
    target.clock.step_ms(30_000);
    let before = dump(&target);
    let old = scm(&target);
    let result = import(&target, &package, ALL_OFF).unwrap();
    let now = target.clock.now_ms() / 1000;
    let rewritten = note(&target, other);
    assert_eq!((rewritten.sort_field.as_str(), rewritten.mtime.0, rewritten.usn.0), ("lune", now, -1));
    assert!(scm(&target) > old, "a sort field change is a schema change");
    assert!(result.safety_backup.is_some(), "a structural edit takes its safety backup");
    // I9 compares against the rewritten mod: the older package note is a duplicate.
    assert_eq!((result.log.duplicate.len(), result.log.updated.len()), (1, 0));
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), before, "undo restores scm and every rewritten note");
}

#[test]
fn a_changed_front_gives_target_notes_their_missing_cards_at_the_sibling_position() {
    let templates = |second: &'static str| vec![(("Card 1", Some(21)), "{{Front}}"), (("Card 2", Some(22)), second)];
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 200, &two_fields(), &templates("{{Back}}")));
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12))];
    put_notetype(&target, notetype(5000, "Pair", 100, &wider, &templates("{{#Front}}{{/Front}}")));
    let held = add(&target, 5000, "g-two", &["moon", "lune"], 1);
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let cards = reader.cards_of_note(held).unwrap();
    assert_eq!(cards.len(), 1);
    let position = cards[0].due;
    let counter = reader.config_value("nextPos").unwrap();
    drop(reader);
    let old = scm(&target);
    import(&target, &package, ALL_OFF).unwrap();
    let fresh = note_by_guid(&target, "g-one").unwrap();
    assert_eq!(scm(&target), old, "a front change alone marks no schema");
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let cards = reader.cards_of_note(held).unwrap();
    assert_eq!(cards.iter().map(|card| card.ord).collect::<Vec<_>>(), vec![0, 1]);
    assert_eq!(cards[1].due, position, "the new card takes its sibling's position");
    assert_eq!(reader.config_value("nextPos").unwrap(), counter);
    // The incoming note's own cards arrive too, one per template.
    assert_eq!(reader.cards_of_note(fresh.id).unwrap().len(), 2);
}

#[test]
fn a_refused_note_type_record_ends_the_import_with_nothing_written() {
    for name in ["\"", "Pair"] {
        let source = Fixture::new(0);
        let fields = if name == "Pair" { vec![("::", Some(11)), ("Back", Some(12))] } else { two_fields() };
        put_notetype(&source, notetype(5000, name, 200, &fields, &one_template()));
        add(&source, 5000, "g-one", &["sun", "soleil"], 1);
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(1);
        let before = dump(&target);
        assert!(matches!(import(&target, &package, ALL_OFF), Err(ApkgImportError::Corrupt { .. })));
        assert_eq!(dump(&target), before);
    }
}

#[test]
fn damaged_packages_are_repaired_or_logged_instead_of_failing() {
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let first = add(&source, 5000, "g-twice", &["sun", "soleil"], 1);
    source.clock.step_ms(1000);
    // Multi-byte text: a separator count in characters would miscount it.
    let second = add(&source, 5000, "g-other", &["m\u{f4}me", "\u{e9}t\u{e9}"], 1);
    source.clock.step_ms(1000);
    put_notetype(&source, notetype(6000, "Gone", 100, &two_fields(), &one_template()));
    let orphan = add(&source, 6000, "g-orphan", &["star", "etoile"], 1);
    source.clock.step_ms(1000);
    let lost = add(&source, 5000, "g-lost", &["cloud", "nuage"], 1);
    source.clock.step_ms(1000);
    let wide = add(&source, 5000, "g-wide", &["rain", "pluie"], 1);
    let ahead = source.clock.now_ms() + 3 * 86_400_000;
    raw(&source, move |scope| {
        // Two notes with one GUID.
        let mut note = scope.note(second)?.unwrap();
        note.guid = "g-twice".into();
        scope.put(&note)?;
        // A note of a note type the package lacks: its rows are removed.
        let mut report = meguri_storage::journal::StructureReport::default();
        report.reasons.insert(meguri_storage::journal::StructureReason::NotetypeRemoved);
        report.removed.insert(6000);
        let token = scope.start_structure_report(report)?;
        scope.remove_notetype(NotetypeId(6000))?;
        scope.complete_structure_report(token)?;
        // A card in a deck the package lacks.
        let mut card = scope.cards_of_note(lost)?.remove(0);
        card.deck_id = DeckId(4242);
        scope.put(&card)?;
        // A note with one value too many, and an id three days ahead.
        let mut note = scope.note(wide)?.unwrap();
        note.fields = "rain\u{1f}plu\u{ef}e\u{1f}extra".into();
        scope.put(&note)?;
        let mut card = scope.cards_of_note(wide)?.remove(0);
        scope.remove_card(card.id)?;
        scope.remove_note(wide)?;
        note.id = NoteId(ahead);
        scope.put(&note)?;
        card.note_id = note.id;
        scope.put(&card)?;
        Ok(())
    });
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    target.clock.step_ms(5000);
    let now = target.clock.now_ms();
    let session = inspect(&target, &package).unwrap();
    let repaired = session.facts().repaired;
    assert_eq!((repaired.note_ids, repaired.field_counts), (1, 1));
    let result = apply(&target, &session, ALL_OFF).unwrap();
    assert_eq!(result.log.found_notes, 5);
    assert_eq!(result.log.new.len(), 3);
    assert_eq!(result.log.duplicate.len(), 1);
    assert_eq!(result.log.duplicate[0].note, first.0, "the second of one GUID names the first");
    assert_eq!(result.log.missing_notetype.len(), 1);
    assert_eq!(result.log.missing_notetype[0].note, orphan.0);
    assert_eq!(result.log.missing_deck.len(), 1);
    assert_eq!(result.log.missing_deck[0].note, lost.0);
    let fixed = note_by_guid(&target, "g-wide").unwrap();
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.note(orphan).unwrap().is_none());
    assert_eq!(reader.cards_of_note(lost).unwrap()[0].deck_id.0, 1);
    assert_eq!(fixed.id.0, now, "an id past the cutoff is repaired from now upward");
    assert_eq!(fixed.fields, "rain\u{1f}plu\u{ef}e; extra");
    assert_eq!(reader.cards_of_note(fixed.id).unwrap().len(), 1);
}

#[test]
fn an_id_the_target_holds_shifts_the_incoming_note_and_card_by_999() {
    let source = Fixture::new(1);
    let id = note_ids(&source)[0];
    crate::apkg_import::edit_note(&source, id, |note| note.guid = "another-guid".into());
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(1);
    let held = note(&target, id);
    let card = target.cards[0];
    let result = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!(result.log.new[0].note, id.0 + 999);
    assert_eq!(note(&target, id), held);
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let cards = reader.cards_of_note(NoteId(id.0 + 999)).unwrap();
    assert_eq!(cards.iter().map(|card| card.id).collect::<Vec<_>>(), vec![CardId(card.0 + 999)]);
}

#[test]
fn existing_pairs_are_skipped_and_a_duplicate_note_gains_its_missing_card() {
    let templates = vec![(("Card 1", Some(21)), "{{Front}}"), (("Card 2", Some(22)), "{{Back}}")];
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &templates));
    let id = add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &templates));
    let held = add(&target, 5000, "g-one", &["sun", "soleil"], 1);
    assert_eq!(held, id);
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let cards = reader.cards_of_note(held).unwrap();
    drop(reader);
    let (kept, gone) = (cards[0].clone(), cards[1].id);
    raw(&target, move |scope| {
        scope.remove_card(gone)?;
        Ok(())
    });
    let result = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!((result.log.duplicate.len(), result.counts.cards_added), (1, 1));
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let after = reader.cards_of_note(held).unwrap();
    assert_eq!(after.iter().map(|card| card.ord).collect::<Vec<_>>(), vec![0, 1]);
    use meguri_storage::Record;
    assert_eq!(after[0].to_cells(), kept.to_cells(), "an existing card row is never changed");
}

fn answered_source() -> (Fixture, NoteId) {
    let source = Fixture::new(2);
    source.clock.step_ms(1000);
    source.answer(source.cards[0], None);
    let id = note_ids(&source)[0];
    raw(&source, move |scope| {
        // A graduated review card due on day 20, flagged, in the package's
        // own day count.
        let mut card = scope.cards_of_note(id)?.remove(0);
        card.card_type = meguri_model::card::CardType::Review;
        card.queue = meguri_model::card::CardQueue::Review;
        card.due = 20;
        card.interval = 12;
        card.flags = 3;
        scope.put(&card)?;
        let mut note = scope.note(id)?.unwrap();
        note.guid = "g-reviewed".into();
        note.tags = " leech marked topic ".into();
        scope.put(&note)?;
        // The package's collection is seven days older than the target's.
        let mut col = scope.col()?;
        col.created = EpochSecs(col.created.0 - 7 * 86_400);
        scope.put_col(&col)?;
        Ok(())
    });
    (source, id)
}

#[test]
fn with_scheduling_cards_keep_their_state_shifted_to_the_target_day_and_bring_their_review_log() {
    let (source, id) = answered_source();
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let session = inspect(&target, &package).unwrap();
    assert!(session.facts().has_scheduling);
    let options = ImportOptions {
        with_scheduling: true,
        ..ALL_OFF
    };
    let result = apply(&target, &session, options).unwrap();
    assert_eq!(result.counts.revlog_added, 1);
    let stored = note_by_guid(&target, "g-reviewed").unwrap();
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(stored.id, id);
    assert_eq!(stored.tags, " leech marked topic ");
    let card = reader.cards_of_note(id).unwrap().remove(0);
    assert_eq!((card.card_type.stored(), card.queue.stored(), card.interval, card.flags), (2, 2, 12, 3));
    assert_eq!(card.due, 13, "the package is 7 days ahead of the target");
    assert_eq!(card.usn.0, -1);
    let log = reader.revlog_after(None, 10).unwrap();
    assert_eq!(log.len(), 1);
    assert_eq!((log[0].card_id, log[0].usn.0), (card.id, -1));
}

#[test]
fn without_scheduling_cards_are_new_and_marks_flags_and_review_log_stay_behind() {
    let (source, id) = answered_source();
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let counter = reader.config_value("nextPos").unwrap();
    drop(reader);
    let result = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!(result.counts.revlog_added, 0);
    assert_eq!(note_by_guid(&target, "g-reviewed").unwrap().tags, " topic ");
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let card = reader.cards_of_note(id).unwrap().remove(0);
    assert_eq!((card.card_type.stored(), card.queue.stored(), card.interval, card.flags), (0, 0, 0, 0));
    assert!(card.data.memory_state.is_none());
    assert!(reader.revlog_after(None, 10).unwrap().is_empty());
    assert_eq!(reader.config_value("nextPos").unwrap(), counter, "the target's counter is not used");
}

fn filtered(name: &str) -> Deck {
    let mut deck = meguri_engine::decks::new_deck(name.to_owned());
    deck.kind = ProtoBlob::new(DeckKindMessage::new(DeckKind::Filtered(FilteredDeck::default())));
    deck
}

#[test]
fn decks_match_by_name_take_the_ancestor_spelling_and_step_aside_for_the_other_kind() {
    let source = Fixture::new(0);
    let verbs = meguri_engine::decks::ops::add(&source.collection, "japanese::verbs".into()).unwrap().value;
    let n5 = meguri_engine::decks::ops::add(&source.collection, "Kanji::N5".into()).unwrap().value;
    let lone = meguri_engine::decks::ops::add(&source.collection, "A::B".into()).unwrap().value;
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&source, 5000, "g-verb", &["eat", "manger"], verbs.id.0);
    add(&source, 5000, "g-kanji", &["sun", "soleil"], n5.id.0);
    add(&source, 5000, "g-lone", &["moon", "lune"], lone.id.0);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    target.clock.step_ms(60_000);
    meguri_engine::decks::ops::add(&target.collection, "Japanese".into()).unwrap();
    raw(&target, |scope| {
        scope.add_deck(&filtered("Kanji"))?;
        Ok(())
    });
    import(&target, &package, ALL_OFF).unwrap();
    let suffix = target.clock.now_ms() / 1000;
    let deck_of = |guid: &str| {
        let id = note_by_guid(&target, guid).unwrap().id;
        let reader = target.collection.reader(ReaderRole::Query).unwrap();
        let deck = reader.cards_of_note(id).unwrap()[0].deck_id;
        reader.deck(deck).unwrap().unwrap().human_name()
    };
    assert_eq!(deck_of("g-verb"), "Japanese::verbs");
    assert_eq!(deck_of("g-kanji"), format!("Kanji {suffix}::N5"));
    assert_eq!(deck_of("g-lone"), "A::B");
    assert!(deck_named(&target, "A").is_some(), "the missing parent is created");
    assert!(deck_named(&target, "japanese").is_none());
    let stand_in = deck_named(&target, &format!("Kanji {suffix}")).unwrap();
    assert!(matches!(stand_in.kind.kind, DeckKind::Normal(_)));
    assert!(matches!(deck_named(&target, "Kanji").unwrap().kind.kind, DeckKind::Filtered(_)));
    assert_eq!((stand_in.usn.0, stand_in.mtime.0), (-1, suffix));
}

#[test]
fn today_limits_move_to_the_target_day_and_presets_come_only_when_asked_and_absent() {
    let source = Fixture::new(0);
    let deck = meguri_engine::decks::ops::add(&source.collection, "Limits".into()).unwrap().value;
    let preset = meguri_engine::deck_config::ops::add(&source.collection, "Fast".into()).unwrap().value;
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil"], deck.id.0);
    let id = deck.id;
    raw(&source, move |scope| {
        let mut deck = scope.deck(id)?.unwrap();
        if let DeckKind::Normal(normal) = &mut deck.kind.kind {
            normal.config_id = preset.0;
            // The package's today is 10 at the import: its crt is moved 7
            // days back below, and the import runs 3 days from now.
            normal.new_limit_today = Some(DayLimit::new(5, 10));
            normal.review_limit_today = Some(DayLimit::new(9, 3));
        }
        scope.put(&deck)?;
        let mut col = scope.col()?;
        col.created = EpochSecs(col.created.0 - 7 * 86_400);
        scope.put_col(&col)?;
        Ok(())
    });
    let package = export(&source, "deck.apkg");
    // With scheduling, presets off: the limits arrive, the preset does not.
    let target = Fixture::new(0);
    target.clock.step_ms(3 * 86_400_000);
    let options = ImportOptions {
        with_scheduling: true,
        ..ALL_OFF
    };
    let result = import(&target, &package, options).unwrap();
    assert_eq!(result.counts.presets_added, 0);
    let stored = deck_named(&target, "Limits").unwrap();
    let DeckKind::Normal(normal) = &stored.kind.kind else { panic!("a normal deck") };
    assert_eq!(normal.config_id, 1);
    let today = target.collection.today().unwrap();
    assert_eq!(today, 3);
    let limit = normal.new_limit_today.as_ref().unwrap();
    assert_eq!((limit.limit, limit.today), (5, today), "an active limit moves to the target's today");
    let stale = normal.review_limit_today.as_ref().unwrap();
    assert_eq!((stale.limit, stale.today), (9, 2), "a stale one stays before today");
    // Presets on: added under its id; a second import leaves it alone.
    let other = Fixture::new(0);
    let options = ImportOptions {
        with_scheduling: true,
        with_deck_configs: true,
        ..ALL_OFF
    };
    let result = import(&other, &package, options).unwrap();
    assert_eq!(result.counts.presets_added, 1);
    let reader = other.collection.reader(ReaderRole::Query).unwrap();
    let added = reader.deck_config(preset).unwrap().unwrap();
    assert_eq!((added.name.as_str(), added.usn.0), ("Fast", -1));
    drop(reader);
    let DeckKind::Normal(normal) = &deck_named(&other, "Limits").unwrap().kind.kind.clone() else { panic!() };
    assert_eq!(normal.config_id, preset.0);
    let again = import(&other, &package, options).unwrap();
    assert_eq!(again.counts.presets_added, 0);
}

#[test]
fn a_failed_safety_backup_refuses_a_structural_import_with_nothing_written() {
    let source = Fixture::new(0);
    let mut incoming = notetype(5000, "Pair", 200, &two_fields(), &one_template());
    incoming.config.sort_field_idx = 1;
    put_notetype(&source, incoming);
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&target, 5000, "g-two", &["moon", "lune"], 1);
    target.clock.step_ms(30_000);
    let before = dump(&target);
    // The backups folder cannot take a file: a plain file stands in its place.
    let backups = target.dir.path().join("backups");
    let _ = std::fs::remove_dir_all(&backups);
    std::fs::write(&backups, b"in the way").unwrap();
    let session = inspect(&target, &package).unwrap();
    let refusal = apply(&target, &session, ALL_OFF).unwrap_err();
    assert!(
        matches!(&refusal, ApkgImportError::Engine(error) if matches!(**error, EngineError::SafetyBackupFailed(_))),
        "{refusal:?}"
    );
    assert_eq!(dump(&target), before);
    // The session stays open: once the folder is back the same apply imports.
    std::fs::remove_file(&backups).unwrap();
    std::fs::create_dir(&backups).unwrap();
    let result = apply(&target, &session, ALL_OFF).unwrap();
    assert!(result.safety_backup.is_some());
}

#[test]
fn a_note_type_of_the_other_kind_is_never_updated_in_place() {
    let source = Fixture::new(0);
    let mut cloze = notetype(5000, "Pair", 200, &two_fields(), &one_template());
    cloze.config.kind = NotetypeKind::Cloze.stored();
    put_notetype(&source, cloze);
    add(&source, 5000, "g-new", &["{{c1::star}}", "etoile"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&target, 5000, "g-other", &["moon", "lune"], 1);
    let result = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!((result.counts.notetypes_copied, result.counts.notetypes_updated), (1, 0));
    assert_eq!(stored_notetype(&target, 5000).unwrap().config.kind, NotetypeKind::Standard.stored());
    let fresh = note_by_guid(&target, "g-new").unwrap();
    assert_eq!(stored_notetype(&target, fresh.notetype_id.0).unwrap().config.kind, NotetypeKind::Cloze.stored());
}

#[test]
fn a_filtered_ancestor_matched_only_by_case_folding_gets_its_stand_in() {
    let source = Fixture::new(0);
    // U+1E9E folds to `ss`, the name of the target's filtered deck.
    let child = meguri_engine::decks::ops::add(&source.collection, "\u{1e9e}::x".into()).unwrap().value;
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil"], child.id.0);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    target.clock.step_ms(60_000);
    raw(&target, |scope| {
        scope.add_deck(&filtered("ss"))?;
        Ok(())
    });
    import(&target, &package, ALL_OFF).unwrap();
    let suffix = target.clock.now_ms() / 1000;
    let id = note_by_guid(&target, "g-one").unwrap().id;
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let deck = reader.cards_of_note(id).unwrap()[0].deck_id;
    let name = reader.deck(deck).unwrap().unwrap().human_name();
    assert!(name.ends_with("::x") && name.contains(&suffix.to_string()), "{name}");
    assert!(matches!(reader.deck(deck).unwrap().unwrap().kind.kind, DeckKind::Normal(_)));
}

fn backup_count(f: &Fixture) -> usize {
    std::fs::read_dir(f.dir.path().join("backups")).map_or(0, |entries| entries.count())
}

/// A package and a target whose import changes note type 5000's sort field:
/// a structural import that takes the safety backup.
fn sort_field_case() -> (Fixture, std::path::PathBuf, Fixture) {
    let source = Fixture::new(0);
    let mut incoming = notetype(5000, "Pair", 200, &two_fields(), &one_template());
    incoming.config.sort_field_idx = 1;
    put_notetype(&source, incoming);
    add(&source, 5000, "g-one", &["<img src=\"a.png\">", "soleil"], 1);
    let package = crate::apkg_import::with_media(&export(&source, "deck.apkg"), "media.apkg", &[("a.png", b"pixels")]);
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&target, 5000, "g-two", &["moon", "lune"], 1);
    target.clock.step_ms(30_000);
    (source, package, target)
}

#[test]
fn the_structure_report_carries_reason_8_and_what_the_note_types_went_through() {
    // An import that adds notes only: reason 8 alone.
    let source = Fixture::new(1);
    source.clock.step_ms(1000);
    crate::apkg_import::add_note(&source, source.notetype, &["fresh", "back"]);
    let target = Fixture::new(1);
    let plain = import(&target, &export(&source, "deck.apkg"), ALL_OFF).unwrap().structure;
    assert_eq!(plain.reasons, [8]);
    assert!(plain.layouts.is_empty() && plain.removed.is_empty() && plain.type_changes.is_empty());

    // A sort field change without a merge: 8 and 3, with that note type's layout.
    let (_source, package, target) = sort_field_case();
    let sorted = import(&target, &package, ALL_OFF).unwrap().structure;
    assert_eq!(sorted.reasons, [3, 8]);
    assert_eq!(sorted.layouts.iter().map(|(id, _)| *id).collect::<Vec<_>>(), [5000]);

    // An update that only renames a field: 8 and an identity layout.
    let source = Fixture::new(0);
    let renamed = [("Front", Some(11)), ("Answer", Some(12))];
    put_notetype(&source, notetype(5000, "Pair", 200, &renamed, &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let rename = import(&target, &export(&source, "deck.apkg"), ALL_OFF).unwrap().structure;
    assert_eq!(rename.reasons, [8]);
    assert_eq!(rename.layouts, [(5000, true)]);

    let merge = ImportOptions {
        merge_notetypes: true,
        ..ALL_OFF
    };
    // A merge without an effect (equal layouts, same names): 8 alone.
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 200, &two_fields(), &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil"], 1);
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let quiet = import(&target, &export(&source, "deck.apkg"), merge).unwrap().structure;
    assert_eq!(quiet.reasons, [8]);
    assert!(quiet.layouts.is_empty());

    // A merge that adds a field: 8 and 9, with T's layout.
    let source = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(13))];
    put_notetype(&source, notetype(5000, "Pair", 200, &wider, &one_template()));
    add(&source, 5000, "g-one", &["sun", "soleil", "more"], 1);
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let grown = import(&target, &export(&source, "deck.apkg"), merge).unwrap().structure;
    assert_eq!(grown.reasons, [8, 9]);
    assert_eq!(grown.layouts, [(5000, false)]);
}

#[test]
fn the_safety_backup_is_written_before_the_import_commits() {
    let (_source, package, target) = sort_field_case();
    let notes_before = note_ids(&target).len() as u64;
    let stores = backup_count(&target);
    let result = import(&target, &package, ALL_OFF).unwrap();
    assert_eq!(backup_count(&target), stores + 1);
    let name = result.safety_backup.unwrap();
    // The backup holds the collection as it was before the op: none of the
    // import's notes, and the note type's old sort field.
    let inspected = meguri_package::inspect_collection_package(
        &target.dir.path().join("backups").join(&name),
        &meguri_package::CollectionPackageInspectContext {
            collection_directory: target.collection.path().parent().unwrap(),
            live_collection_size: 0,
            reserved_space_bytes: 0,
            clock: target.clock.as_ref(),
            random: target.random.as_ref(),
            space: &meguri_storage::space::StatvfsSpace,
            is_cancelled: &|| false,
        },
    )
    .unwrap();
    assert_eq!(inspected.facts().notes, notes_before);
    assert_eq!(note_ids(&target).len() as u64, notes_before + 1);
    assert!(inspected.facts().modified < target.collection.reader(ReaderRole::Query).unwrap().col().unwrap().modified.0);
}

/// I8 as `sync/meguri-provider` MP7.1 needs it: a caller that asks to
/// confirm structure gets a plan's structural reason before anything is
/// written, even with no merge and no preview, and the token covers it.
#[test]
fn a_caller_that_asks_gets_the_structural_reason_before_anything_is_written() {
    use meguri_package::apkg::{StructuralReason, apply_confirming};
    let run = |target: &Fixture, session: &_, token: Option<&_>, ask: bool| {
        apply_confirming(target.host.engine(), session, ALL_OFF, token, ask, &meguri_engine::CancelToken::new(),
            &meguri_engine::ProgressSink::new())
    };
    let (_source, package, target) = sort_field_case();
    let (before, old, stores) = (dump(&target), scm(&target), backup_count(&target));
    let session = inspect(&target, &package).unwrap();
    let refusal = run(&target, &session, None, true).unwrap_err();
    let ApkgImportError::ConfirmationRequired { preview, reasons, token } = refusal else {
        panic!("the reason to confirm, not {refusal:?}");
    };
    assert!(preview.is_empty());
    assert_eq!(reasons, [StructuralReason::SortFieldChanged]);
    // Nothing was written, staged or backed up, and the session is open.
    assert_eq!((dump(&target), scm(&target), backup_count(&target)), (before, old, stores));
    assert!(crate::apkg_import::media_files(&target).is_empty());
    // A token for other options covers nothing.
    let other = ImportOptions { with_deck_configs: true, ..ALL_OFF };
    let again = apply_confirming(target.host.engine(), &session, other, Some(&*token), true,
        &meguri_engine::CancelToken::new(), &meguri_engine::ProgressSink::new());
    assert!(matches!(again, Err(ApkgImportError::ConfirmationRequired { .. })));
    assert_eq!(scm(&target), old);
    // The token of this confirmation covers the reason.
    let result = run(&target, &session, Some(&*token), true).unwrap();
    assert_eq!(result.structure.reasons, [3, 8]);
    assert!(scm(&target) > old && result.safety_backup.is_some());

    // A caller that does not ask (iCloud, Off, no ready binding) is asked
    // nothing for an empty preview, as before.
    let (_source, package, target) = sort_field_case();
    let session = inspect(&target, &package).unwrap();
    assert_eq!(run(&target, &session, None, false).unwrap().structure.reasons, [3, 8]);

    // A plan without a structural reason is never asked about structure.
    let source = Fixture::new(1);
    source.clock.step_ms(1000);
    crate::apkg_import::add_note(&source, source.notetype, &["fresh", "back"]);
    let target = Fixture::new(1);
    let old = scm(&target);
    let session = inspect(&target, &export(&source, "deck.apkg")).unwrap();
    assert_eq!(run(&target, &session, None, true).unwrap().structure.reasons, [8]);
    assert_eq!(scm(&target), old);
}

#[test]
fn a_cancel_at_the_backup_stage_writes_nothing() {
    let (_source, package, target) = sort_field_case();
    let before = dump(&target);
    let stores = backup_count(&target);
    let session = inspect(&target, &package).unwrap();
    let cancel = meguri_engine::CancelToken::new();
    let stop = cancel.clone();
    let (sink, _) = crate::apkg_import::at_stage("backup", move || stop.cancel());
    let refusal = meguri_package::apkg::apply(target.host.engine(), &session, ALL_OFF, None, &cancel, &sink);
    assert!(matches!(refusal, Err(ApkgImportError::Cancelled)), "{refusal:?}");
    assert_eq!(dump(&target), before);
    assert_eq!(backup_count(&target), stores);
    assert!(crate::apkg_import::media_files(&target).is_empty());
    // The session is still open.
    assert!(apply(&target, &session, ALL_OFF).unwrap().safety_backup.is_some());
}

#[test]
fn a_deck_name_past_the_nesting_limit_is_refused_in_the_plan_at_no_cost() {
    let (source, _, target) = sort_field_case();
    // A deck thirteen levels deep whose ancestors the package lacks.
    let deep = meguri_engine::decks::ops::add(&source.collection, "deep".into()).unwrap().value;
    let name: Vec<String> = (1..=13).map(|level| format!("level{level}")).collect();
    let (id, native) = (deep.id, name.join("\u{1f}"));
    raw(&source, move |scope| {
        let mut deck = scope.deck(id)?.unwrap();
        deck.name.clone_from(&native);
        scope.put(&deck)?;
        Ok(())
    });
    add(&source, 5000, "g-deep", &["<img src=\"a.png\">", "profond"], deep.id.0);
    let package = crate::apkg_import::with_media(&export(&source, "deck.apkg"), "deep.apkg", &[("a.png", b"pixels")]);
    let before = dump(&target);
    let stores = backup_count(&target);
    let session = inspect(&target, &package).unwrap();
    let staged = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = staged.clone();
    let (sink, _) = crate::apkg_import::at_stage("media", move || seen.store(true, std::sync::atomic::Ordering::SeqCst));
    let refusal =
        meguri_package::apkg::apply(target.host.engine(), &session, ALL_OFF, None, &meguri_engine::CancelToken::new(), &sink);
    let Err(ApkgImportError::DeckTooDeep { name }) = refusal else {
        panic!("a nesting refusal, not {refusal:?}");
    };
    assert!(name.starts_with("level1::level2"), "{name}");
    assert!(!staged.load(std::sync::atomic::Ordering::SeqCst), "refused before media staging");
    assert_eq!(backup_count(&target), stores, "and before any safety backup");
    assert_eq!(dump(&target), before);
}

/// Enough space until `low` is set, then almost none.
struct Switch(std::sync::atomic::AtomicBool, std::sync::atomic::AtomicUsize, usize);
impl meguri_storage::space::SpaceProvider for Switch {
    fn available_bytes(&self, _: &std::path::Path) -> Result<u64, meguri_storage::StorageError> {
        let call = self.1.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        let low = self.0.load(std::sync::atomic::Ordering::SeqCst) || call >= self.2;
        Ok(if low { 1024 } else { u64::MAX / 4 })
    }
}

#[test]
fn low_space_at_the_extraction_and_at_the_media_preflight_refuses_before_writing() {
    let (_source, package, target) = sort_field_case();
    let before = dump(&target);
    let files = crate::apkg_import::listing(&target);
    // The second check of an inspect is the extraction's.
    let mut context = crate::apkg_import::context(&target);
    context.space = std::sync::Arc::new(Switch(false.into(), 0.into(), 1));
    let refusal = meguri_package::apkg::inspect(
        &package,
        "media.apkg",
        &context,
        &meguri_engine::CancelToken::new(),
        &meguri_engine::ProgressSink::new(),
    );
    assert!(matches!(refusal, Err(ApkgImportError::InsufficientSpace { available: 1024, .. })));
    assert_eq!(crate::apkg_import::listing(&target), files, "the source copy is removed");
    // The third: the media preflight of the apply.
    let space = std::sync::Arc::new(Switch(false.into(), 0.into(), usize::MAX));
    context.space = space.clone();
    let session = meguri_package::apkg::inspect(
        &package,
        "media.apkg",
        &context,
        &meguri_engine::CancelToken::new(),
        &meguri_engine::ProgressSink::new(),
    )
    .unwrap();
    space.0.store(true, std::sync::atomic::Ordering::SeqCst);
    let stores = backup_count(&target);
    let refusal = apply(&target, &session, ALL_OFF);
    assert!(matches!(refusal, Err(ApkgImportError::InsufficientSpace { available: 1024, .. })), "{refusal:?}");
    assert_eq!(dump(&target), before);
    assert_eq!(backup_count(&target), stores);
    assert!(crate::apkg_import::media_files(&target).is_empty());
    space.0.store(false, std::sync::atomic::Ordering::SeqCst);
    assert_eq!(apply(&target, &session, ALL_OFF).unwrap().counts.notes_added, 1);
}

/// Reports the free space it was last given.
struct Level(std::sync::atomic::AtomicU64);
impl meguri_storage::space::SpaceProvider for Level {
    fn available_bytes(&self, _: &std::path::Path) -> Result<u64, meguri_storage::StorageError> {
        Ok(self.0.load(std::sync::atomic::Ordering::SeqCst))
    }
}

#[test]
fn the_media_preflight_counts_the_safety_backup_a_structural_import_takes() {
    use std::sync::atomic::Ordering::SeqCst;
    // What the media preflight asks for, read from its refusal.
    let needed = |target: &Fixture, package: &std::path::Path| -> (u64, meguri_package::apkg::ImportSession, std::sync::Arc<Level>) {
        let space = std::sync::Arc::new(Level(u64::MAX.into()));
        let mut context = crate::apkg_import::context(target);
        context.space = space.clone();
        let session = meguri_package::apkg::inspect(
            package,
            "media.apkg",
            &context,
            &meguri_engine::CancelToken::new(),
            &meguri_engine::ProgressSink::new(),
        )
        .unwrap();
        space.0.store(1024, SeqCst);
        let Err(ApkgImportError::InsufficientSpace { needed, available: 1024 }) = apply(target, &session, ALL_OFF) else {
            panic!("a space refusal at the media preflight");
        };
        (needed, session, space)
    };
    let (_source, package, structural) = sort_field_case();
    // The same package into a target without the note type: it is added,
    // nothing structural changes, and no backup is taken.
    let plain = Fixture::new(0);
    let (without, ..) = needed(&plain, &package);
    let size = |path: std::path::PathBuf| std::fs::metadata(path).map_or(0, |m| m.len());
    let live = structural.collection.path().to_owned();
    let mut wal = live.clone().into_os_string();
    wal.push("-wal");
    let backup = 2 * (size(live) + size(wal.into())) + 4 * 1024 * 1024;
    let (with, session, space) = needed(&structural, &package);
    assert_eq!(with, without + backup, "the backup's need on top of the media's");

    // One byte short refuses before the backup; the exact need imports.
    let stores = backup_count(&structural);
    space.0.store(with - 1, SeqCst);
    assert!(matches!(apply(&structural, &session, ALL_OFF), Err(ApkgImportError::InsufficientSpace { .. })));
    assert_eq!(backup_count(&structural), stores);
    space.0.store(with, SeqCst);
    assert!(apply(&structural, &session, ALL_OFF).unwrap().safety_backup.is_some());
}

#[test]
fn a_package_past_a_size_limit_is_refused_at_the_inspect_and_leaves_no_staging() {
    use meguri_package::apkg::LIMITS;
    // Each case edits one stored value of an otherwise small package.
    type Edit = fn(&mut WriteScope<'_>, NoteId) -> Result<(), EngineError>;
    let cases: [(&str, u64, Edit); 4] = [
        ("guid bytes", LIMITS.guid_bytes, |scope, id| {
            let mut note = scope.note(id)?.unwrap();
            note.guid = "g".repeat(256);
            scope.put_note_forced(&note)?;
            Ok(())
        }),
        ("note bytes", LIMITS.note_bytes, |scope, id| {
            let mut note = scope.note(id)?.unwrap();
            note.fields = format!("{}\u{1f}back", "w".repeat(2 * 1024 * 1024));
            scope.put_note_forced(&note)?;
            Ok(())
        }),
        ("note bytes", LIMITS.note_bytes, |scope, id| {
            let mut note = scope.note(id)?.unwrap();
            note.tags = format!(" {} ", "t".repeat(2 * 1024 * 1024));
            scope.put_note_forced(&note)?;
            Ok(())
        }),
        ("card data bytes", LIMITS.card_data_bytes, |scope, id| {
            let mut card = scope.cards_of_note(id)?.remove(0);
            card.data = meguri_model::card_data::CardData::parse(&format!("{{\"pad\":\"{}\"}}", "d".repeat(70_000)));
            scope.put(&card)?;
            Ok(())
        }),
    ];
    for (name, limit, edit) in cases {
        let source = Fixture::new(0);
        let id = crate::apkg_import::add_note(&source, source.notetype, &["front", "back"]);
        raw(&source, move |scope| edit(scope, id));
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(0);
        let files = crate::apkg_import::listing(&target);
        let before = dump(&target);
        let refusal = inspect(&target, &package);
        let Err(ApkgImportError::TooLarge { what, found, limit: stated }) = refusal else {
            panic!("{name}: a size refusal, not {:?}", refusal.map(|_| ()));
        };
        assert_eq!((what, stated), (name, limit));
        assert!(found > limit, "{name}: {found}");
        assert_eq!(crate::apkg_import::listing(&target), files, "{name}: the staging is removed");
        assert_eq!(dump(&target), before);
    }
}

#[test]
fn tag_uses_count_tags_and_not_their_bytes_or_padding() {
    // One note with four tags of two-byte letters, stored padded.
    let source = Fixture::new(0);
    let id = crate::apkg_import::add_note(&source, source.notetype, &["front", "back"]);
    raw(&source, move |scope| {
        let mut note = scope.note(id)?.unwrap();
        // The first holds a NUL, where a text length would stop counting.
        note.tags = format!(" {0}\0z {0}a {0}b {0}c ", "é".repeat(100));
        scope.put_note_forced(&note)?;
        Ok(())
    });
    let package = export(&source, "deck.apkg");
    let staged = tempfile::tempdir().unwrap();
    let mut archive = zip::ZipArchive::new(std::fs::File::open(&package).unwrap()).unwrap();
    let bytes = zstd::decode_all(&crate::apkg_import::entry(&mut archive, "collection.anki21b")[..]).unwrap();
    let path = staged.path().join("collection.anki2");
    std::fs::write(&path, bytes).unwrap();
    let limits = |tag_uses| meguri_storage::deck_package::DeckCandidateLimits {
        tag_uses,
        ..meguri_package::apkg::LIMITS
    };
    let prepare = |tag_uses| {
        meguri_storage::deck_package::prepare_deck_candidate(
            &path,
            &meguri_storage::package_inspection::CandidateContext {
                clock: source.clock.as_ref(),
                random: source.random.as_ref(),
                space: &meguri_storage::space::StatvfsSpace,
                is_cancelled: &|| false,
            },
            &meguri_storage::deck_package::DeckCandidateRepair {
                minutes_west_at: &|_| 0,
                repair_fields: &|fields, _| fields,
                limits: limits(tag_uses),
            },
        )
    };
    assert!(matches!(
        prepare(3),
        Err(meguri_storage::package_inspection::CandidateError::TooLarge {
            what: "tag uses",
            found: 4,
            limit: 3
        })
    ));
    assert_eq!(prepare(4).unwrap().notes, 1);
}
