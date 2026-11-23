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
