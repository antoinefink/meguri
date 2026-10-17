mod common;
use common::notes::{add, input, kind};
use common::tagging::fresh;
use meguri_engine::{EngineError, OpSpec, UndoScope, notes};
use meguri_model::{
    changes::OpSource,
    grave::GraveKind,
    ids::{NoteId, NotetypeId},
};
use meguri_storage::{ConfigValue, ReadRows, ReaderRole};

macro_rules! scenario {
    ($name:ident, $id:literal) => {
        #[test]
        fn $name() {
            common::note_scenarios::play(concat!("notes/", $id));
        }
    };
}
scenario!(records_add, "records/add");
scenario!(untouched_fields, "save/untouched-fields");
scenario!(normalize_off, "save/normalize-off");
scenario!(same_after_normalizing, "save/same-after-normalizing");
scenario!(field_states, "fields-check/states");
scenario!(duplicates, "fields-check/duplicates");
scenario!(field_count_repair, "fields/count-repair");

#[test]
fn repairs_follow_f1_including_empty_extras() {
    for (values, count, expected) in [
        (vec!["three", "b", "c"], 2, vec!["three", "b; c"]),
        (vec!["one"], 2, vec!["one", ""]),
        (vec!["four", "b", "", "d"], 2, vec!["four", "b; ; d"]),
        (vec!["single", "x", "y"], 1, vec!["single; x; y"]),
    ] {
        let mut values: Vec<String> = values.into_iter().map(str::to_owned).collect();
        notes::fields::repair(&mut values, count).unwrap();
        assert_eq!(values, expected);
    }
    assert!(notes::fields::repair(&mut vec![], 0).is_err());
}

#[test]
fn bulk_add_guid_defaults_and_undo_redo_are_one_step() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let before = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .config_entry("curModel")
        .unwrap();
    h.clock.step_ms(3_600_000);
    let mut first = input(nt, &["<b>私</b>", "I"]);
    first.guid = Some("c~MJPzD".to_owned());
    first.tags = vec!["Verb".to_owned(), "verb".to_owned()];
    let result = notes::bulk_add(
        &h.collection,
        vec![first, input(nt, &["second", ""])],
        OpSource::LocalApi,
    )
    .unwrap()
    .value;
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(r.note(result[0].id).unwrap().unwrap().guid, "c~MJPzD");
    assert_eq!(r.config_entry("curModel").unwrap(), before);
    assert!(r.config_entry(&format!("_nt_{}_lastDeck", nt.0)).unwrap().is_none());
    drop(r);
    let mut screen = input(nt, &["screen", ""]);
    screen.update_add_defaults = true;
    let added = add(&h, screen);
    assert_eq!(
        h.collection.peek_undo(UndoScope::Global).unwrap().name(),
        Some("Add Note")
    );
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.note(added.id).unwrap().is_none());
    assert!(r.card(added.cards[0]).unwrap().is_none());
    assert!(r.grave(added.id.0, GraveKind::Note).unwrap().is_none());
    assert_eq!(r.config_entry("curModel").unwrap(), before);
    drop(r);
    h.collection.redo(UndoScope::Global).unwrap();
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(added.id)
            .unwrap()
            .is_some()
    );
    let all = notes::bulk_add(
        &h.collection,
        vec![input(nt, &["a", ""]), input(nt, &["b", ""])],
        OpSource::UserUndoable,
    )
    .unwrap()
    .value;
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(all.iter().all(|n| r.note(n.id).unwrap().is_none()));
}

#[test]
fn unchanged_saves_preserve_stamps_derived_columns_and_opaque_values() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let added = add(&h, input(nt, &["café", "<span  class='x'>keep</span><br/>"]));
    common::tagging::raw(
        &h.collection,
        "UPDATE notes SET usn=7, flags=123, data='opaque', sfld='stale', csum=1 WHERE id=?",
        &[meguri_storage::raw::SqlValue::Integer(added.id.0)],
    );
    let before = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(added.id)
        .unwrap()
        .unwrap();
    let update = notes::UpdateNote {
        id: added.id,
        fields: vec![
            "cafe\u{301}\u{1}".to_owned(),
            "<span  class='x'>keep</span><br/>".to_owned(),
        ],
        tags: vec![],
    };
    let result = notes::update(&h.collection, update, OpSource::UserUndoable).unwrap();
    assert!(!result.value.written);
    assert_eq!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .note(added.id)
            .unwrap()
            .unwrap(),
        before
    );
    let result = notes::update(
        &h.collection,
        notes::UpdateNote {
            id: added.id,
            fields: vec!["edited".to_owned(), "<span  class='x'>keep</span><br/>".to_owned()],
            tags: vec![],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(result.value.written);
    let after = h
        .collection
        .reader(ReaderRole::Query)
        .unwrap()
        .note(added.id)
        .unwrap()
        .unwrap();
    assert_eq!((after.flags, after.data.as_str(), after.usn.0), (123, "opaque", -1));
    assert!(after.fields.ends_with("\u{1f}<span  class='x'>keep</span><br/>"));
}

#[test]
fn refuses_invalid_inputs_atomically_and_missing_note_is_distinct() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    for bad in [input(NotetypeId(-99), &["x", ""]), input(nt, &["x"])] {
        assert!(matches!(
            notes::bulk_add(
                &h.collection,
                vec![input(nt, &["valid", ""]), bad],
                OpSource::UserUndoable
            ),
            Err(EngineError::InvalidArgument { .. })
        ));
        assert!(
            h.collection
                .reader(ReaderRole::Query)
                .unwrap()
                .notes_after(None, 10)
                .unwrap()
                .is_empty()
        );
    }
    assert!(matches!(
        notes::update(
            &h.collection,
            notes::UpdateNote {
                id: NoteId(-99),
                fields: vec![],
                tags: vec![]
            },
            OpSource::UserUndoable
        ),
        Err(EngineError::NoteNotFound { .. })
    ));
}

#[test]
fn deletion_logs_once_and_undo_restores_rows_and_graves() {
    let h = fresh();
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    let mut first = input(nt, &["私", "I"]);
    first.guid = Some("c~MJPzD".to_owned());
    let a = add(&h, first);
    let b = add(&h, input(nt, &["tab\there", "line\nbreak"]));
    notes::delete(
        &h.collection,
        vec![b.id, NoteId(-9), a.id, a.id],
        OpSource::UserUndoable,
    )
    .unwrap();
    let path = h.path.with_file_name("deleted.txt");
    let text = std::fs::read_to_string(&path).unwrap();
    assert!(text.starts_with(&format!(
        "#guid column:1\n#notetype column:2\n#nid\tmid\tfields\nc~MJPzD\t{}\t私\tI\n",
        nt.0
    )));
    assert!(text.ends_with("\ttab\there\tline\nbreak\n"));
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.note(a.id).unwrap().is_none());
    assert!(r.grave(a.id.0, GraveKind::Note).unwrap().is_some());
    assert!(r.grave(a.cards[0].0, GraveKind::Card).unwrap().is_some());
    drop(r);
    h.collection.undo(UndoScope::Global).unwrap();
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.note(a.id).unwrap().is_some());
    assert!(r.grave(a.id.0, GraveKind::Note).unwrap().is_none());
    drop(r);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
    let c = add(&h, input(nt, &["third", ""]));
    notes::delete(&h.collection, vec![c.id], OpSource::UserUndoable).unwrap();
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .matches("#guid column:1")
            .count(),
        1
    );
}

#[test]
fn log_failure_removes_nothing_and_empty_deletion_creates_no_file() {
    let h = fresh();
    assert!(
        notes::delete(&h.collection, vec![NoteId(-9)], OpSource::UserUndoable)
            .unwrap()
            .is_none()
    );
    assert!(!h.path.with_file_name("deleted.txt").exists());
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    let a = add(&h, input(nt, &["kept"]));
    std::fs::create_dir(h.path.with_file_name("deleted.txt")).unwrap();
    assert!(matches!(
        notes::delete(&h.collection, vec![a.id], OpSource::UserUndoable),
        Err(EngineError::Storage(_))
    ));
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.note(a.id).unwrap().is_some());
    assert!(r.card(a.cards[0]).unwrap().is_some());
}

#[test]
fn nfc_switch_is_read_each_save_and_check_does_not_remove_controls() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    h.collection
        .run(OpSpec::named("config", OpSource::UserNonUndoable), |ctx| {
            ctx.scope()
                .set_config("normalize_note_text", &ConfigValue::Bool(false))?;
            Ok(())
        })
        .unwrap();
    let a = add(&h, input(nt, &["cafe\u{301}"]));
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let stored = r.note(a.id).unwrap().unwrap();
    assert_eq!(stored.checksum, 922075179);
    assert_eq!(stored.fields, "cafe\u{301}");
    let nt = r.notetype(nt).unwrap().unwrap();
    assert_eq!(
        notes::check(&*r, &nt, &["café".to_owned()], None).unwrap().state,
        notes::FieldState::Normal
    );
    assert_eq!(
        notes::check(&*r, &nt, &["cafe\u{301}".to_owned()], None)
            .unwrap()
            .duplicates,
        vec![a.id]
    );
    assert_eq!(
        notes::check(&*r, &nt, &["cafe\u{301}\u{1}".to_owned()], None)
            .unwrap()
            .state,
        notes::FieldState::Normal
    );
}

#[test]
fn add_note_p95_at_150k() {
    let generated = meguri_testkit::bench::fixtures::profile_collection("150k").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    std::fs::copy(generated.dir.join("collection.anki2"), &path).unwrap();
    let h = common::Harness::open_path(dir, path);
    let nt = kind(&h, false, &["Front", "Back"], &["{{Front}}"]);
    add(&h, input(nt, &["warm", "meaning"]));
    let mut samples = Vec::new();
    for i in 0..100 {
        let input = input(nt, &[&format!("sample {i}"), "meaning"]);
        let start = meguri_engine::monotonic::now();
        add(&h, input);
        samples.push(meguri_engine::monotonic::now().saturating_sub(start));
    }
    samples.sort_unstable();
    let p95 = samples[94];
    eprintln!("add-note p95 at 150k cards: {p95:?}");
    assert!(p95 < std::time::Duration::from_millis(5), "{p95:?}");
}

#[test]
fn missing_deletion_preserves_redo() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    add(&h, input(nt, &["redo me"]));
    h.collection.undo(UndoScope::Global).unwrap();
    assert!(
        notes::delete(&h.collection, vec![NoteId(-9)], OpSource::UserUndoable)
            .unwrap()
            .is_none()
    );
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap().name(),
        Some("Add Note")
    );
    h.collection.redo(UndoScope::Global).unwrap();
}

#[test]
fn log_layout_example_and_existing_empty_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let note = meguri_model::note::Note {
        guid: "c~MJPzD".to_owned(),
        notetype_id: NotetypeId(1894701600006),
        fields: "私\u{1f}I".to_owned(),
        ..Default::default()
    };
    notes::deleted_log::append(&path, std::slice::from_ref(&note)).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("deleted.txt")).unwrap(),
        "#guid column:1\n#notetype column:2\n#nid\tmid\tfields\nc~MJPzD\t1894701600006\t私\tI\n"
    );
    std::fs::write(dir.path().join("deleted.txt"), "").unwrap();
    notes::deleted_log::append(&path, &[note]).unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("deleted.txt")).unwrap(),
        "c~MJPzD\t1894701600006\t私\tI\n"
    );
}

#[cfg(unix)]
#[test]
fn log_readonly_directory() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let readonly = dir.path().join("readonly");
    std::fs::create_dir(&readonly).unwrap();
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o555)).unwrap();
    let mut command = std::process::Command::new(std::env::current_exe().unwrap());
    command
        .args(["--exact", "log_readonly_probe"])
        .env("MEGURI_U27_LOG_PROBE", &readonly);
    // Linux CI runs as root; use an unprivileged child to exercise actual
    // filesystem denial, without changing the test process's credentials.
    #[cfg(target_os = "linux")]
    {
        use std::os::unix::{fs::MetadataExt, process::CommandExt};
        if std::fs::metadata("/proc/self").unwrap().uid() == 0 {
            command.uid(65534);
        }
    }
    let result = command.output().unwrap();
    std::fs::set_permissions(&readonly, std::fs::Permissions::from_mode(0o755)).unwrap();
    assert!(result.status.success(), "{}", String::from_utf8_lossy(&result.stdout));
    assert!(!readonly.join("deleted.txt").exists());
}

#[cfg(unix)]
#[test]
fn log_readonly_probe() {
    let Some(dir) = std::env::var_os("MEGURI_U27_LOG_PROBE") else {
        return;
    };
    let note = meguri_model::note::Note {
        fields: "kept".to_owned(),
        ..Default::default()
    };
    let error =
        notes::deleted_log::append(&std::path::PathBuf::from(dir).join("collection.anki2"), &[note]).unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
}

#[test]
fn duplicate_check_hashes_stripped_text_only_once() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    let added = add(&h, input(nt, &["&lt;b&gt;literal&lt;/b&gt;"]));
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let nt = reader.notetype(nt).unwrap().unwrap();
    assert_eq!(
        notes::check(&*reader, &nt, &["<i>&lt;b&gt;literal&lt;/b&gt;</i>".to_owned()], None)
            .unwrap()
            .duplicates,
        vec![added.id]
    );
}

#[test]
fn no_op_does_not_repair_the_tag_registry() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    let mut draft = input(nt, &["same"]);
    draft.tags = vec!["lost".to_owned()];
    let added = add(&h, draft);
    common::tagging::raw(&h.collection, "DELETE FROM tags WHERE tag = 'lost'", &[]);
    let saved = notes::update(
        &h.collection,
        notes::UpdateNote {
            id: added.id,
            fields: vec!["same".to_owned()],
            tags: vec!["lost".to_owned()],
        },
        OpSource::UserUndoable,
    )
    .unwrap();
    assert!(!saved.value.written);
    assert!(
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .tag("lost")
            .unwrap()
            .is_none()
    );
}

#[test]
fn damaged_default_deck_rolls_back_the_whole_add() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    common::tagging::raw(&h.collection, "DELETE FROM decks WHERE id = 1", &[]);
    let before = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    let mut draft = input(nt, &["kept out"]);
    draft.tags = vec!["uncommitted".to_owned()];
    assert!(notes::add(&h.collection, draft, OpSource::UserUndoable).is_err());
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(reader.col().unwrap(), before);
    assert!(reader.notes_after(None, 100).unwrap().is_empty());
    assert!(reader.tag("uncommitted").unwrap().is_none());
}

#[test]
fn generated_rows_have_current_stamps_in_a_composite_keep_mtime_scope() {
    let h = fresh();
    let nt = kind(&h, false, &["Front"], &["{{Front}}"]);
    h.clock.step_ms(3_600_000);
    let input = input(nt, &["imported new note"]);
    let added = h
        .collection
        .run(
            OpSpec::named("composite add", OpSource::UserUndoable).stamp(meguri_storage::StampMode::KeepMtime),
            move |ctx| {
                let (scope, random) = ctx.scope_and_random();
                notes::add_in(scope, random, &input)
            },
        )
        .unwrap()
        .value;
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let note = reader.note(added.id).unwrap().unwrap();
    let card = reader.card(added.cards[0]).unwrap().unwrap();
    assert_eq!((note.mtime.0, note.usn.0), (1894705200, -1));
    assert_eq!((card.mtime.0, card.usn.0), (1894705200, -1));
}
