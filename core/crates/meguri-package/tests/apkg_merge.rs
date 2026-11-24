//! Merge note types on deck package import
//! (`specs/package/notetype-merge.md` J15.2), with packages built here.
use meguri_engine::notetypes::NotetypeKind;
use meguri_engine::{CancelToken, ProgressSink, UndoScope};
use meguri_model::clock::Clock;
use meguri_model::ids::NoteId;
use meguri_package::apkg::{self, ApkgImportError, ImportOptions, UpdateCondition};
use meguri_storage::{ReadRows, ReaderRole};
use meguri_testkit::normal_support::Fixture;

use crate::apkg_import::{ALL_OFF, apply, dump, edit_note, export, import, inspect};
use crate::apkg_rules::{
    Part, add, note, note_by_guid, notetype, notetype_count, one_template, put_notetype, scm, stored_notetype,
    two_fields,
};

const MERGE: ImportOptions = ImportOptions {
    merge_notetypes: true,
    ..ALL_OFF
};

fn backups(f: &Fixture) -> usize {
    std::fs::read_dir(f.dir.path().join("backups")).map_or(0, |entries| entries.count())
}

fn names(f: &Fixture, id: i64) -> (Vec<String>, Vec<String>) {
    let stored = stored_notetype(f, id).unwrap();
    (
        stored.fields.iter().map(|field| field.name.clone()).collect(),
        stored.templates.iter().map(|template| template.name.clone()).collect(),
    )
}

fn cards(f: &Fixture, id: NoteId) -> Vec<u16> {
    let reader = f.collection.reader(ReaderRole::Query).unwrap();
    reader.cards_of_note(id).unwrap().iter().map(|card| card.ord).collect()
}

fn set_mtime(f: &Fixture, id: NoteId, mtime: i64) {
    edit_note(f, id, move |note| note.mtime.0 = mtime);
}

#[test]
fn an_extra_incoming_field_is_appended_and_the_layout_change_marks_the_schema() {
    for (rule, updated) in [(UpdateCondition::IfNewer, false), (UpdateCondition::Always, true)] {
        let source = Fixture::new(0);
        // The incoming places its new field between the two the target has.
        let wider = [("Front", Some(11)), ("Extra", Some(13)), ("Back", Some(12))];
        let mut incoming = notetype(5000, "Pair", 200, &wider, &one_template());
        incoming.config.sort_field_idx = 2;
        put_notetype(&source, incoming);
        let shared = add(&source, 5000, "g-shared", &["sun", "added", "soleil"], 1);
        set_mtime(&source, shared, 900);
        add(&source, 5000, "g-new", &["star", "more", "etoile"], 1);
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(0);
        target.clock.step_ms(20_000);
        put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
        let held = add(&target, 5000, "g-shared", &["sun", "local"], 1);
        let other = add(&target, 5000, "g-other", &["moon", "lune"], 1);
        set_mtime(&target, held, 50);
        target.clock.step_ms(20_000);
        let (before, old) = (dump(&target), scm(&target));
        let stores = backups(&target);
        let result = import(&target, &package, ImportOptions { update_notes: rule, ..MERGE }).unwrap();
        assert_eq!(names(&target, 5000).0, ["Front", "Back", "Extra"], "T's fields keep their positions");
        let merged = stored_notetype(&target, 5000).unwrap();
        assert_eq!(merged.config.sort_field_idx, 2, "the governing record's number, as a position");
        assert_eq!(merged.mtime.0, target.clock.now_ms() / 1000);
        assert!(scm(&target) > old);
        assert_eq!(backups(&target), stores + 1, "a structural merge takes its safety backup");
        assert!(result.safety_backup.is_some());
        // Every note of T was rewritten and stamped before I9 compared.
        assert_eq!(note(&target, other).fields, "moon\u{1f}lune\u{1f}");
        assert_eq!(note(&target, other).mtime.0, target.clock.now_ms() / 1000);
        let stored = note(&target, held);
        if updated {
            assert_eq!(result.log.updated.len(), 1);
            assert_eq!((stored.fields.as_str(), stored.mtime.0), ("sun\u{1f}soleil\u{1f}added", 900));
            assert_eq!(stored.sort_field, "added");
        } else {
            assert_eq!(result.log.duplicate.len(), 1, "IfNewer after a layout change updates no note of T");
            assert_eq!(stored.fields, "sun\u{1f}local\u{1f}");
        }
        // The new note went through the field map 0, 2, 1.
        assert_eq!(note_by_guid(&target, "g-new").unwrap().fields, "star\u{1f}etoile\u{1f}more");
        assert_eq!(notetype_count(&target) as i64, notetype_count(&source) as i64);
        target.collection.undo(UndoScope::Global).unwrap();
        assert_eq!(dump(&target), before, "undo restores the note type, the notes and scm");
    }
}

#[test]
fn equal_layouts_replace_the_record_without_touching_notes_or_the_schema() {
    for (rule, target_mtime, governs) in [
        (UpdateCondition::IfNewer, 100, true),
        (UpdateCondition::IfNewer, 200, false),
        (UpdateCondition::Always, 200, true),
        (UpdateCondition::Never, 100, false),
    ] {
        let source = Fixture::new(0);
        let renamed = [("Front", Some(11)), ("Answer", Some(12))];
        put_notetype(&source, notetype(5000, "Pair renamed", 200, &renamed, &[(("Face", Some(21)), "{{Front}}")]));
        add(&source, 5000, "g-new", &["star", "etoile"], 1);
        let package = export(&source, "deck.apkg");
        let target = Fixture::new(0);
        put_notetype(&target, notetype(5000, "Pair", target_mtime, &two_fields(), &one_template()));
        let held = add(&target, 5000, "g-other", &["moon", "lune"], 1);
        let stored = note(&target, held);
        target.clock.step_ms(5000);
        let (old, stores) = (scm(&target), backups(&target));
        import(&target, &package, ImportOptions { update_notetypes: rule, ..MERGE }).unwrap();
        let merged = stored_notetype(&target, 5000).unwrap();
        let expected = if governs { ("Pair renamed", "Answer", "Face") } else { ("Pair", "Back", "Card 1") };
        assert_eq!(
            (merged.name.as_str(), merged.fields[1].name.as_str(), merged.templates[0].name.as_str()),
            expected,
            "{rule:?} {target_mtime}"
        );
        assert_eq!(merged.fields.len(), 2, "matched by id: no second field");
        assert_eq!(merged.mtime.0, target.clock.now_ms() / 1000, "every merged note type is stamped");
        assert_eq!(scm(&target), old, "no structural effect, no scm");
        assert_eq!(backups(&target), stores, "and no safety backup");
        assert_eq!(note(&target, held), stored);
    }
}

#[test]
fn fields_without_ids_match_by_name_and_a_new_template_gives_target_notes_its_card() {
    let source = Fixture::new(0);
    let fields: [Part<'_>; 3] = [("Front", None), ("Back", None), ("Hint", None)];
    let templates = [(("Card 1", None), "{{Front}}"), (("Reverse", None), "{{Back}}")];
    put_notetype(&source, notetype(5000, "Pair", 200, &fields, &templates));
    add(&source, 5000, "g-new", &["star", "etoile", "hint"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let local: [Part<'_>; 2] = [("Front", None), ("Back", None)];
    put_notetype(&target, notetype(5000, "Pair", 100, &local, &[(("Card 1", None), "{{Front}}")]));
    let held = add(&target, 5000, "g-other", &["moon", "lune"], 1);
    import(&target, &package, MERGE).unwrap();
    assert_eq!(names(&target, 5000), (
        vec!["Front".to_owned(), "Back".to_owned(), "Hint".to_owned()],
        vec!["Card 1".to_owned(), "Reverse".to_owned()],
    ));
    assert_eq!(cards(&target, held), [0, 1], "G9 adds the new template's card to T's notes");
    assert_eq!(cards(&target, note_by_guid(&target, "g-new").unwrap().id), [0, 1]);
}

#[test]
fn a_moved_template_maps_incoming_cards_to_the_merged_ordinal() {
    let source = Fixture::new(0);
    let incoming = [(("Card 2", Some(22)), "{{Back}}"), (("Card 3", Some(23)), "{{Front}}{{Back}}")];
    put_notetype(&source, notetype(5000, "Pair", 200, &two_fields(), &incoming));
    add(&source, 5000, "g-new", &["star", "etoile"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let local = [(("Card 1", Some(21)), "{{Front}}"), (("Card 2", Some(22)), "{{Back}}")];
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &local));
    let held = add(&target, 5000, "g-other", &["moon", "lune"], 1);
    import(&target, &package, MERGE).unwrap();
    assert_eq!(names(&target, 5000).1, ["Card 1", "Card 2", "Card 3"]);
    assert_eq!(cards(&target, held), [0, 1, 2]);
    // The package note's cards 0 and 1 arrive as ordinals 1 and 2.
    assert_eq!(cards(&target, note_by_guid(&target, "g-new").unwrap().id), [1, 2]);
}

#[test]
fn a_standard_and_a_cloze_note_type_never_merge() {
    let source = Fixture::new(0);
    let mut cloze = notetype(5000, "Pair", 200, &two_fields(), &one_template());
    cloze.config.kind = NotetypeKind::Cloze.stored();
    put_notetype(&source, cloze);
    add(&source, 5000, "g-new", &["{{c1::star}}", "etoile"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&target, 5000, "g-other", &["moon", "lune"], 1);
    let (before, stores) = (dump(&target), backups(&target));
    assert!(matches!(
        import(&target, &package, MERGE),
        Err(ApkgImportError::NotetypeKindConflict {
            incoming: 5000,
            target: 5000
        })
    ));
    assert_eq!(dump(&target), before);
    assert_eq!(backups(&target), stores);
    // Without a merge the same package imports, as a copy.
    import(&target, &package, ALL_OFF).unwrap();
}

#[test]
fn a_dropped_field_needs_a_confirmation_that_survives_other_writes() {
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 200, &two_fields(), &one_template()));
    let shared = add(&source, 5000, "g-shared", &["sun", "soleil"], 1);
    set_mtime(&source, shared, 900);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Notes", Some(13))];
    put_notetype(&target, notetype(5000, "Pair", 100, &wider, &one_template()));
    let held = add(&target, 5000, "g-shared", &["sun", "local", "my own words"], 1);
    set_mtime(&target, held, 50);
    target.clock.step_ms(5000);
    let (before, old, stores) = (dump(&target), scm(&target), backups(&target));
    let session = inspect(&target, &package).unwrap();
    let refusal = apply(&target, &session, MERGE).unwrap_err();
    let ApkgImportError::ConfirmationRequired { preview, token, .. } = refusal else {
        panic!("a preview to confirm, not {refusal:?}");
    };
    assert_eq!(preview.emptied_fields.len(), 1);
    let entry = &preview.emptied_fields[0];
    assert_eq!((entry.notetype, entry.position, entry.field_name.as_str(), entry.notes), (5000, 2, "Notes", 1));
    assert!(preview.removed_notetypes.is_empty() && preview.dropped_values.is_empty());
    // Declining is not applying again: nothing changed, no backup.
    assert_eq!(dump(&target), before);
    assert_eq!(backups(&target), stores);
    // A token for other options is not a confirmation of these.
    let other = ImportOptions {
        with_scheduling: true,
        ..MERGE
    };
    let again = apkg::apply(target.host.engine(), &session, other, Some(&*token), &CancelToken::new(), &ProgressSink::new());
    assert!(matches!(again, Err(ApkgImportError::ConfirmationRequired { .. })));
    // A write elsewhere between the two calls keeps the token valid.
    let unrelated = add(&target, target.notetype.0, "g-unrelated", &["cloud", "nuage"], 1);
    let result =
        apkg::apply(target.host.engine(), &session, MERGE, Some(&*token), &CancelToken::new(), &ProgressSink::new())
            .unwrap();
    assert_eq!(result.log.updated.len(), 1);
    assert_eq!(note(&target, held).fields, "sun\u{1f}soleil\u{1f}");
    assert_eq!(scm(&target), old, "nothing changed T's layout");
    assert_eq!(backups(&target), stores + 1, "a confirmed preview takes the safety backup");
    assert!(note_by_guid(&target, "g-unrelated").is_some());
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(note(&target, held).fields, "sun\u{1f}local\u{1f}my own words");
    let _ = unrelated;
}

#[test]
fn an_earlier_copy_is_merged_back_removed_and_undo_restores_it() {
    let source = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(13))];
    put_notetype(&source, notetype(5000, "Pair", 200, &wider, &one_template()));
    add(&source, 5000, "g-new", &["star", "etoile", "extra"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let own = add(&target, 5000, "g-other", &["moon", "lune"], 1);
    // Without a merge: the copy `Pair+` takes the package's note.
    import(&target, &package, ALL_OFF).unwrap();
    let moved = note_by_guid(&target, "g-new").unwrap();
    let copy = moved.notetype_id.0;
    assert_ne!(copy, 5000);
    target.clock.step_ms(700_000);
    let before = dump(&target);
    let types = notetype_count(&target);
    // With a merge: a removed note type is in the preview.
    let session = inspect(&target, &package).unwrap();
    let ApkgImportError::ConfirmationRequired { preview, token, .. } = apply(&target, &session, MERGE).unwrap_err() else {
        panic!("the removal needs a confirmation");
    };
    assert_eq!(preview.removed_notetypes.len(), 1);
    let removed = &preview.removed_notetypes[0];
    assert_eq!((removed.notetype, removed.name.as_str(), removed.moved_to, removed.notes), (copy, "Pair+", 5000, 1));
    let result =
        apkg::apply(target.host.engine(), &session, MERGE, Some(&*token), &CancelToken::new(), &ProgressSink::new())
            .unwrap();
    assert_eq!((result.counts.notes_moved, result.counts.notetypes_removed), (1, 1));
    assert!(stored_notetype(&target, copy).is_none());
    assert_eq!(notetype_count(&target), types - 1);
    assert_eq!(names(&target, 5000).0, ["Front", "Back", "Extra"]);
    let back = note(&target, moved.id);
    assert_eq!((back.notetype_id.0, back.fields.as_str()), (5000, "star\u{1f}etoile\u{1f}extra"));
    assert_eq!(note(&target, own).fields, "moon\u{1f}lune\u{1f}");
    let reader = target.collection.reader(ReaderRole::Query).unwrap();
    let current = reader.config_value("curModel").unwrap().and_then(|value| value.as_i64()).unwrap();
    assert!(reader.notetype(meguri_model::ids::NotetypeId(current)).unwrap().is_some());
    drop(reader);
    let after = dump(&target);
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), before, "undo restores the copy, its notes, curModel and scm");
    target.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), after);
}

#[test]
fn a_note_held_under_another_note_type_brings_both_together() {
    let source = Fixture::new(0);
    put_notetype(&source, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let shared = add(&source, 5000, "g-shared", &["sun", "soleil"], 1);
    set_mtime(&source, shared, 900);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    let local = [("Front", Some(11)), ("Back", Some(12)), ("Local", Some(31))];
    put_notetype(&target, notetype(7000, "Triple", 100, &local, &one_template()));
    let own = add(&target, 5000, "g-own", &["moon", "lune"], 1);
    let held = add(&target, 7000, "g-shared", &["sun", "local", "kept words"], 1);
    set_mtime(&target, held, 50);
    let before = dump(&target);
    // Update notes Never shows the move alone.
    let options = ImportOptions {
        update_notes: UpdateCondition::Never,
        ..MERGE
    };
    let session = inspect(&target, &package).unwrap();
    let ApkgImportError::ConfirmationRequired { preview, token, .. } = apply(&target, &session, options).unwrap_err() else {
        panic!("removing the emptied note type needs a confirmation");
    };
    assert_eq!(preview.removed_notetypes[0].notetype, 7000);
    let result =
        apkg::apply(target.host.engine(), &session, options, Some(&*token), &CancelToken::new(), &ProgressSink::new())
            .unwrap();
    assert!(result.log.conflicting.is_empty(), "with a merge no note is conflicting");
    assert_eq!(result.log.duplicate.len(), 1);
    assert_eq!(names(&target, 5000).0, ["Front", "Back", "Local"], "E's own field is appended to I");
    let joined = note(&target, held);
    assert_eq!((joined.notetype_id.0, joined.fields.as_str()), (5000, "sun\u{1f}local\u{1f}kept words"));
    assert_eq!(note(&target, own).fields, "moon\u{1f}lune\u{1f}", "I's own notes keep their values");
    assert!(stored_notetype(&target, 7000).is_none(), "the emptied E is removed");
    target.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(dump(&target), before);
}

#[test]
fn a_repeated_field_id_is_matched_one_to_one() {
    let source = Fixture::new(0);
    // A damaged record: its third field repeats the first one's id.
    let damaged = [("Front", Some(11)), ("Back", Some(12)), ("Again", Some(11))];
    put_notetype(&source, notetype(5000, "Pair", 100, &damaged, &one_template()));
    add(&source, 5000, "g-new", &["star", "etoile", "third"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 200, &two_fields(), &one_template()));
    import(&target, &package, MERGE).unwrap();
    assert_eq!(names(&target, 5000).0, ["Front", "Back", "Again"], "the repeat is new and appended");
    assert_eq!(note_by_guid(&target, "g-new").unwrap().fields, "star\u{1f}etoile\u{1f}third");
    // A second import matches each repeat with its own earlier copy.
    target.clock.step_ms(700_000);
    import(&target, &package, MERGE).unwrap();
    assert_eq!(names(&target, 5000).0, ["Front", "Back", "Again"]);
}

#[test]
fn the_name_of_a_copy_being_removed_stays_taken_during_the_pass() {
    let source = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(13))];
    put_notetype(&source, notetype(5000, "Pair", 200, &wider, &one_template()));
    put_notetype(&source, notetype(6000, "Pair spare", 200, &two_fields(), &one_template()));
    add(&source, 5000, "g-new", &["star", "etoile", "extra"], 1);
    add(&source, 6000, "g-spare", &["cloud", "nuage"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    // An earlier copy of 5000 that holds the name the package's 6000 asks for.
    let mut copy = notetype(5500, "Pair spare", 100, &wider, &one_template());
    copy.config.original_id = Some(5000);
    put_notetype(&target, copy);
    add(&target, 5500, "g-held", &["moon", "lune", "kept"], 1);
    let session = inspect(&target, &package).unwrap();
    let ApkgImportError::ConfirmationRequired { token, .. } = apply(&target, &session, MERGE).unwrap_err() else {
        panic!("the copy's removal needs a confirmation");
    };
    apkg::apply(target.host.engine(), &session, MERGE, Some(&*token), &CancelToken::new(), &ProgressSink::new())
        .unwrap();
    assert!(stored_notetype(&target, 5500).is_none());
    assert_eq!(stored_notetype(&target, 6000).unwrap().name, "Pair spare+");
    assert_eq!(note_by_guid(&target, "g-held").unwrap().fields, "moon\u{1f}lune\u{1f}kept");
}

#[test]
fn the_preview_counts_the_notes_the_import_really_updates_after_a_layout_change() {
    let source = Fixture::new(0);
    let incoming = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(14))];
    put_notetype(&source, notetype(5000, "Pair", 200, &incoming, &one_template()));
    let shared = add(&source, 5000, "g-shared", &["sun", "soleil", "more"], 1);
    // An incoming `mod` ahead of the clock: newer even than the stamp the
    // layout change gives T's notes.
    let ahead = source.clock.now_ms() / 1000 + 1_000_000;
    set_mtime(&source, shared, ahead);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    let local = [("Front", Some(11)), ("Back", Some(12)), ("Notes", Some(13))];
    put_notetype(&target, notetype(5000, "Pair", 100, &local, &one_template()));
    let held = add(&target, 5000, "g-shared", &["sun", "local", "my own words"], 1);
    let session = inspect(&target, &package).unwrap();
    let ApkgImportError::ConfirmationRequired { preview, token, .. } = apply(&target, &session, MERGE).unwrap_err() else {
        panic!("the update empties a field: it needs a confirmation");
    };
    assert_eq!(preview.emptied_fields.len(), 1);
    assert_eq!((preview.emptied_fields[0].field_name.as_str(), preview.emptied_fields[0].notes), ("Notes", 1));
    let result =
        apkg::apply(target.host.engine(), &session, MERGE, Some(&*token), &CancelToken::new(), &ProgressSink::new())
            .unwrap();
    assert_eq!(result.log.updated.len(), 1);
    assert_eq!(names(&target, 5000).0, ["Front", "Back", "Notes", "Extra"]);
    assert_eq!(note(&target, held).fields, "sun\u{1f}soleil\u{1f}\u{1f}more");
}

/// I8 with MP7.1's structure question: a confirmation given for a preview
/// alone does not cover a structural reason once the caller asks, and the
/// new confirmation names the same preview and the reason.
#[test]
fn a_preview_confirmation_does_not_cover_a_reason_the_caller_then_asks_about() {
    use meguri_package::apkg::{StructuralReason, apply_confirming};
    let source = Fixture::new(0);
    let wider = [("Front", Some(11)), ("Back", Some(12)), ("Extra", Some(13))];
    put_notetype(&source, notetype(5000, "Pair", 200, &wider, &one_template()));
    add(&source, 5000, "g-new", &["star", "etoile", "extra"], 1);
    let package = export(&source, "deck.apkg");
    let target = Fixture::new(0);
    put_notetype(&target, notetype(5000, "Pair", 100, &two_fields(), &one_template()));
    add(&target, 5000, "g-other", &["moon", "lune"], 1);
    // The copy `Pair+`, which a merge then removes: a preview with an effect.
    import(&target, &package, ALL_OFF).unwrap();
    target.clock.step_ms(700_000);
    let (before, stores) = (dump(&target), backups(&target));
    let session = inspect(&target, &package).unwrap();
    let run = |token: Option<&_>, ask: bool| {
        apply_confirming(target.host.engine(), &session, MERGE, token, ask, &CancelToken::new(), &ProgressSink::new())
    };
    // Asked without the structure question: the preview, and no reasons.
    let ApkgImportError::ConfirmationRequired { preview, reasons, token: unasked } = run(None, false).unwrap_err() else {
        panic!("the removal needs a confirmation");
    };
    assert_eq!(preview.removed_notetypes.len(), 1);
    assert!(reasons.is_empty());
    // The binding became ready before the confirmation was accepted.
    let ApkgImportError::ConfirmationRequired { preview: shown, reasons, token } = run(Some(&*unasked), true).unwrap_err()
    else {
        panic!("the reason needs a confirmation of its own");
    };
    assert_eq!((shown, reasons), (preview, vec![StructuralReason::NotetypesMerged]));
    assert_eq!((dump(&target), backups(&target)), (before, stores), "nothing was written or backed up");
    // That confirmation covers both.
    let result = run(Some(&*token), true).unwrap();
    assert_eq!(result.structure.reasons, [8, 9]);
    assert_eq!(backups(&target), stores + 1);
}
