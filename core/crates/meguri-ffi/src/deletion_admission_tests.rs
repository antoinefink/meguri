//! Destructive admission with colliding synthetic identities in A and B.
use super::*;
use crate::notetype_structure::{NotetypeRemoval, remove_notetype_on};
use crate::tags::delete_tags_on;
use meguri_engine::{
    Collection, EngineError, Generation, notetypes, notes, tags,
    undo::{UndoScope, UndoStatus},
};
use meguri_model::{
    changes::OpSource,
    ids::{DeckId, NoteId, NotetypeId},
};
use meguri_storage::{
    ReaderRole,
    raw::{RawRows, SqlValue},
};
use std::{
    path::{Path, PathBuf},
    time::Duration,
};

fn seed(host: &meguri_host::CoreHost, label: &str) -> (NotetypeId, NoteId) {
    let c = host.collection().unwrap();
    let nt = notetypes::add(
        &c,
        notetypes::NewNotetype {
            name: "Shared Type".into(),
            fields: vec!["Front".into(), "Back".into()],
            templates: vec![notetypes::NewTemplate {
                name: "Shared Card".into(),
                front: "{{Front}}".into(),
                back: "{{Back}}".into(),
                ..Default::default()
            }],
            ..Default::default()
        },
    )
    .unwrap()
    .value
    .id;
    let note = notes::add(
        &c,
        notes::AddNote {
            notetype_id: nt,
            fields: vec![label.into(), "answer".into()],
            tags: vec!["Shared".into()],
            deck_id: DeckId(1),
            guid: None,
            update_add_defaults: false,
        },
        OpSource::UserUndoable,
    )
    .unwrap()
    .value
    .id;
    // Retain both an undo and a redo step: a refused command must preserve both.
    tags::add_to_notes(&c, &[note], "Redo", &tags::RunOptions::default()).unwrap();
    c.undo(UndoScope::Global).unwrap();
    (nt, note)
}

fn files(path: &Path) -> Vec<(PathBuf, Vec<u8>)> {
    let mut result = Vec::new();
    if path.is_dir() {
        for entry in std::fs::read_dir(path).unwrap() {
            result.extend(files(&entry.unwrap().path()));
        }
    } else if path.is_file() {
        result.push((path.to_owned(), std::fs::read(path).unwrap()));
    }
    result.sort_by(|a, b| a.0.cmp(&b.0));
    result
}

#[derive(Debug, PartialEq)]
struct Facts {
    rows: Vec<(String, RawRows)>,
    seq: u64,
    undo: UndoStatus,
    redo: UndoStatus,
    files: Vec<(PathBuf, Vec<u8>)>,
}
fn facts(c: &Collection, root: &Path) -> Facts {
    let reader = c.reader(ReaderRole::Query).unwrap();
    let tables = reader
        .execute_raw("SELECT name FROM sqlite_master WHERE type = 'table' ORDER BY name", &[])
        .unwrap();
    let rows = tables
        .rows
        .into_iter()
        .map(|row| {
            let SqlValue::Text(name) = &row[0] else {
                panic!("table name")
            };
            let data = reader
                .execute_raw(&format!("SELECT * FROM \"{}\"", name.replace('"', "\"\"")), &[])
                .unwrap();
            (name.clone(), data)
        })
        .collect();
    Facts {
        rows,
        seq: c.last_seq().0,
        undo: c.peek_undo(UndoScope::Global).unwrap(),
        redo: c.peek_redo(UndoScope::Global).unwrap(),
        files: [files(&root.join("backups")), files(&root.join("installation"))].concat(),
    }
}
fn stale<T>(result: Result<T, CoreError>, planned: Generation, current: Generation) {
    assert!(
        matches!(result, Err(CoreError::StaleGeneration { planned: p, current: c }) if p == planned.0 && c == current.0)
    );
}
fn removal(id: NotetypeId) -> NotetypeRemoval {
    NotetypeRemoval {
        notetype_id: id.0,
        confirmed_notes: 1,
        confirmed_cards: 1,
    }
}
fn finish(host: &meguri_host::CoreHost, events: &meguri_engine::changes::stream::Subscription, id: u64) {
    loop {
        let event = events.recv_timeout(Duration::from_secs(20)).unwrap();
        if let meguri_engine::changes::stream::EventKind::JobFinished { id: got, outcome } = &event.kind
            && got.0 == id
        {
            assert_eq!(*outcome, meguri_engine::JobOutcome::Succeeded);
            break;
        }
    }
    assert!(host.derived().wait_idle(Duration::from_secs(5)));
}

#[test]
fn replacement_before_deletion_admission_preserves_b_with_the_same_tag_and_type() {
    let root = tempfile::tempdir().unwrap();
    let host = host(root.path());
    let old = host.collection().unwrap();
    let planned = old.generation();
    let identities = seed(&host, "A only");

    let replacement_root = tempfile::tempdir().unwrap();
    let replacement_config = configuration(replacement_root.path());
    let replacement = meguri_host::CoreHost::open(replacement_config.clone()).unwrap();
    assert_eq!(seed(&replacement, "B sentinel"), identities);
    replacement.close().unwrap();
    let replacement_path = replacement_config.collection_directory.join("collection.anki2");
    let slot = host.engine().acquire_slot(meguri_engine::LongOp::Restore).unwrap();
    slot.enter_exclusive(false)
        .unwrap()
        .replace(move |live| {
            std::fs::rename(replacement_path, live).map_err(meguri_storage::StorageError::from)?;
            Ok(())
        })
        .unwrap();
    drop(slot);

    let c = host.collection().unwrap();
    let current = c.generation();
    assert_ne!(current, planned);
    assert_eq!(old.generation(), planned); // An immutable handle stamp is insufficient.
    assert_eq!(old.current_generation(), current);
    // Make B's undo/redo nonempty after replacement cleared the old history.
    tags::add_to_notes(&c, &[identities.1], "Undo sentinel", &tags::RunOptions::default()).unwrap();
    tags::add_to_notes(&c, &[identities.1], "Redo sentinel", &tags::RunOptions::default()).unwrap();
    c.undo(UndoScope::Global).unwrap();
    let connection = owner(host.clone())
        .configure_meguri("https://deletion.synthetic.example/".into())
        .unwrap();
    assert!(host.derived().wait_idle(Duration::from_secs(5)));
    let before = facts(&c, root.path());
    assert!(matches!(before.undo, UndoStatus::Available(_)) && matches!(before.redo, UndoStatus::Available(_)));

    // These are the exact helpers called by the public scoped FFI starters,
    // after all Swift suspension points and before any job is registered.
    stale(delete_tags_on(host.clone(), "Shared".into(), planned.0), planned, current);
    assert_eq!(facts(&c, root.path()), before);
    stale(
        remove_notetype_on(host.clone(), removal(identities.0), planned.0),
        planned,
        current,
    );
    assert_eq!(facts(&c, root.path()), before);

    for reachable in [true, false] {
        let credentials = ProviderCredentials {
            connection: connection.information().unwrap(),
            username: "synthetic".into(),
            hostkey: "synthetic-key".into(),
        };
        stale(
            connection.start_structure_save_for_generation(
                credentials,
                structure::ProviderStructureOp::Remove {
                    request: removal(identities.0),
                },
                reachable,
                planned.0,
            ),
            planned,
            current,
        );
        assert_eq!(facts(&c, root.path()), before);
        // Independently exercise the actual host admission, bypassing the
        // facade's early check. Neither access nor the save may be invoked.
        let result = host.start_meguri_structure_save_for_generation(
            planned,
            Arc::new(|| panic!("stale admission requested provider access")),
            Box::new(|_, _| panic!("stale admission invoked removal")),
            reachable,
        );
        assert!(matches!(result, Err(meguri_host::MeguriStructureRefusal::Engine(
            EngineError::StaleGeneration { planned: p, current: c })) if p == planned && c == current));
        assert_eq!(facts(&c, root.path()), before);
    }
    // Current-generation controls prove both starters really perform their
    // write; returning a no-op or suppressing delivery cannot pass this test.
    let events = host.engine().stream().subscribe();
    let deleted = delete_tags_on(host.clone(), "Shared".into(), current.0).unwrap();
    finish(&host, &events, deleted.handle.id);
    assert_eq!(deleted.result.result().unwrap().unwrap().count, 1);
    c.undo(UndoScope::Global).unwrap();
    let removed = remove_notetype_on(host.clone(), removal(identities.0), current.0).unwrap();
    finish(&host, &events, removed.handle.id);
    assert!(removed.result.result().unwrap().unwrap().removed);
    c.undo(UndoScope::Global).unwrap();
    assert!(
        c.reader(ReaderRole::Query)
            .unwrap()
            .execute_raw("SELECT flds FROM notes", &[])
            .unwrap()
            .rows
            .iter()
            .any(|row| row == &[SqlValue::Text("B sentinel\u{1f}answer".into())])
    );
    host.close().unwrap();
}

#[test]
fn a_closed_host_cannot_admit_deletion_to_its_process_successor() {
    let root = tempfile::tempdir().unwrap();
    let config = configuration(root.path());
    let process = meguri_host::ProcessCore::new(config.engine.clone()).unwrap();
    let original = Arc::new(process.open(config.clone()).unwrap());
    let identities = seed(&original, "B sentinel");
    original.close().unwrap();
    let successor = Arc::new(process.open(config).unwrap());
    let c = successor.collection().unwrap();
    assert!(successor.derived().wait_idle(Duration::from_secs(5)));
    let before = facts(&c, root.path());
    // Even supplying the successor's live generation cannot grant authority
    // through an old retained host session on the same process engine.
    assert!(delete_tags_on(original.clone(), "Shared".into(), c.generation().0).is_err());
    assert!(remove_notetype_on(original, removal(identities.0), c.generation().0).is_err());
    assert_eq!(facts(&c, root.path()), before);
    successor.close().unwrap();
}
