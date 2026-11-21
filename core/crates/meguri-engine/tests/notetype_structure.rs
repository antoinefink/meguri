//! The draft apply op (`notetypes/structure-changes` S1 to S13) beyond what
//! the scenarios show: the refusals of S1.2 and their order, `Stale` before
//! planning and during the safety backup, the safety backup itself (`package/backup-triggers`
//! T6.1, T6.2, T10), atomic rollback of the write phase, one undo step, the
//! hand-off of rename-only drafts (S1), the sort field reason by identity
//! (S2) and the removal preview (S11).
//!
//! The backup provider's hook runs on the op's thread after planning and
//! before the write transaction: what it commits lands between the plan and
//! the write phase.

mod common;

use std::collections::BTreeSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{Harness, NOW_MS};
use meguri_engine::notetypes::structure::{
    self, DraftChange, DraftField, DraftTemplate, FieldOptions, Losses, StructureDraft, StructureRefusal,
    StructureSaved,
};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind, rename};
use meguri_engine::ports::{BackupFailureCause, BackupProtection, BackupPublication, BackupReceipt, BackupService};
use meguri_engine::ports::{GuardedBackup, SafetyBackupFailure};
use meguri_engine::{
    CancelToken, Collection, Engine, EngineConfig, EngineError, OpResult, OpSpec, Ports, ProgressSink,
    SchemaReason, UndoScope, notes,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::grave::GraveKind;
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochMillis;
use meguri_storage::{ReadRows, ReaderRole, StampMode};

type Hook = Box<dyn FnOnce(&Collection) + Send>;

/// A counted safety backup provider: an optional hook, then success or an
/// injected disk-full failure.
#[derive(Default)]
struct Backups {
    calls: AtomicUsize,
    hook: Mutex<Option<Hook>>,
    fail: Mutex<bool>,
}

impl BackupService for Backups {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        panic!("structure ops use the safety seam")
    }

    fn before_safety_operation(&self, c: &Collection, cancel: &CancelToken) -> Result<GuardedBackup, EngineError> {
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let hook = self.hook.lock().unwrap().take();
        if let Some(hook) = hook {
            hook(c);
        }
        cancel.check()?;
        if *self.fail.lock().unwrap() {
            return Err(EngineError::SafetyBackupFailed(Box::new(SafetyBackupFailure {
                cause: BackupFailureCause::DiskFull,
                publication: BackupPublication::Unpublished,
            })));
        }
        Ok(GuardedBackup {
            receipt: Some(BackupReceipt {
                name: format!("safety-{count}"),
                size_bytes: 1,
                instant: EpochMillis(NOW_MS - 60_000),
                snapshot_modified: EpochMillis(0),
            }),
            protection: Some(Arc::new(())),
        })
    }

    fn protect_backup(&self, _: &str) -> Result<Option<Arc<dyn BackupProtection>>, EngineError> {
        Ok(Some(Arc::new(())))
    }
}

impl Backups {
    fn calls(&self) -> usize {
        self.calls.load(Ordering::SeqCst)
    }

    fn then(&self, hook: impl FnOnce(&Collection) + Send + 'static) {
        *self.hook.lock().unwrap() = Some(Box::new(hook));
    }
}

/// A bootstrapped collection (the stock kinds, the default deck) opened with
/// the counted provider.
fn harness() -> (Harness, Arc<Backups>) {
    let backups = Arc::new(Backups::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    meguri_engine::bootstrap::create(&path, &*clock, &SeededRandom::new(7)).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let config = meguri_engine::OpenConfig::new(clock.clone(), Arc::new(SeededRandom::new(7)))
        .ports(Ports::new().backup_service(backups.clone()));
    let collection = engine.open_raw(&path, config).unwrap();
    (
        Harness {
            dir,
            path,
            clock,
            engine,
            collection,
        },
        backups,
    )
}

/// A standard note type `Trio` (Front, Back, Extra) with templates
/// Forward `{{Front}}` and Reverse `{{Back}}`.
fn trio(h: &Harness) -> NotetypeId {
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: "Trio".to_owned(),
            kind: NotetypeKind::Standard,
            fields: vec!["Front".into(), "Back".into(), "Extra".into()],
            templates: vec![
                NewTemplate {
                    name: "Forward".into(),
                    front: "{{Front}}".into(),
                    back: "{{Back}}".into(),
                    ..Default::default()
                },
                NewTemplate {
                    name: "Reverse".into(),
                    front: "{{Back}}".into(),
                    back: "{{Front}}".into(),
                    ..Default::default()
                },
            ],
            css: Some(String::new()),
        },
    )
    .unwrap()
    .value
    .id
}

fn add(h: &Harness, mid: NotetypeId, fields: &[&str]) -> NoteId {
    notes::add(
        &h.collection,
        notes::AddNote {
            notetype_id: mid,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            tags: Vec::new(),
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

fn run(h: &Harness, draft: StructureDraft) -> Result<OpResult<StructureSaved>, EngineError> {
    structure::apply(&h.collection, draft, CancelToken::new(), ProgressSink::new())
}

fn without(draft: &mut StructureDraft, field: &str) {
    draft.fields.retain(|f| f.name != field);
}

fn without_template(draft: &mut StructureDraft, template: &str) {
    draft.templates.retain(|t| t.name != template);
}

/// The losses `draft` would cause on the rows as stored, as a caller with no
/// question passes them (`notetypes/draft-save` D8).
fn confirm(h: &Harness, draft: &mut StructureDraft) {
    let nt = read(h, draft.notetype);
    let kept_fields: Vec<u32> = draft.fields.iter().filter_map(|f| f.origin).collect();
    let kept_templates: Vec<u32> = draft.templates.iter().filter_map(|t| t.origin).collect();
    let removed: BTreeSet<u32> = (0..nt.templates.len() as u32).filter(|o| !kept_templates.contains(o)).collect();
    draft.confirmed.templates = structure::preview_removal(&h.collection, nt.id, &removed).unwrap().cards;
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    for note in reader.notetype_notes_after(nt.id, None, 100_000).unwrap() {
        let values: Vec<&str> = note.field_values().collect();
        for field in (0..nt.fields.len() as u32).filter(|o| !kept_fields.contains(o)) {
            let filled = values
                .get(field as usize)
                .is_some_and(|v| !meguri_text::field_empty::counts_as_empty(v));
            *draft.confirmed.fields.entry(field).or_default() += u64::from(filled);
        }
    }
}

/// Everything a structure save may change, as stored: the note type, the
/// notes, the cards, the graves and `col.scm`.
#[derive(Debug, PartialEq)]
struct State {
    notetype: String,
    notes: Vec<(i64, String, i64, i64, String, u32)>,
    cards: Vec<(i64, i64, u16, i64, i64)>,
    graves: u64,
    scm: i64,
    /// Every stored column of the notes and cards, tags and scheduling
    /// included, for whole-state comparisons.
    rows: Vec<meguri_storage::RowImage>,
}

fn state(h: &Harness, id: NotetypeId) -> State {
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    let nt = reader.notetype(id).unwrap().unwrap();
    let notetype = format!(
        "{} {:?} {:?} {:?}",
        nt.name,
        nt.config.to_bytes(),
        nt.fields.iter().map(|f| (f.ord, f.name.clone(), f.config.to_bytes().into_owned())).collect::<Vec<_>>(),
        nt.templates
            .iter()
            .map(|t| (t.ord, t.name.clone(), t.mtime, t.usn, t.config.to_bytes().into_owned()))
            .collect::<Vec<_>>()
    );
    let notes = reader.notetype_notes_after(id, None, 10_000).unwrap();
    let mut cards = Vec::new();
    for note in &notes {
        for card in reader.cards_of_note(note.id).unwrap() {
            cards.push((card.id.0, card.note_id.0, card.ord, card.mtime.0, i64::from(card.usn.0)));
        }
    }
    cards.sort_unstable();
    let mut rows = Vec::new();
    for note in &notes {
        rows.push(reader.row_image(meguri_storage::Table::Notes, &meguri_storage::RowKey::Id(note.id.0)).unwrap().unwrap());
    }
    for card in &cards {
        rows.push(reader.row_image(meguri_storage::Table::Cards, &meguri_storage::RowKey::Id(card.0)).unwrap().unwrap());
    }
    State {
        rows,
        notetype,
        notes: notes
            .iter()
            .map(|n| (n.id.0, n.fields.clone(), n.mtime.0, i64::from(n.usn.0), n.sort_field.clone(), n.checksum))
            .collect(),
        cards,
        graves: reader.row_count(meguri_storage::Table::Graves).unwrap(),
        scm: reader.col().unwrap().schema_modified.0,
    }
}

fn undo_name(h: &Harness) -> Option<String> {
    h.collection
        .peek_undo(UndoScope::Global)
        .unwrap()
        .name()
        .map(str::to_owned)
}

/// S1.2 item 6: a failed backup stops the op before its first write.
#[test]
fn a_failed_safety_backup_writes_nothing() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    *backups.fail.lock().unwrap() = true;
    let before = state(&h, id);
    let undo = undo_name(&h);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    let error = run(&h, draft).unwrap_err();
    assert!(matches!(error, EngineError::SafetyBackupFailed(_)), "{error:?}");
    assert_eq!(state(&h, id), before);
    assert_eq!(undo_name(&h), undo);
}

/// S1.2 item 1: a draft made before another save is `Stale` on the planning
/// snapshot (no backup), and a change committed while the backup runs is
/// `Stale` from the write transaction (the backup kept, nothing written).
#[test]
fn stale_before_planning_and_during_the_backup() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut old = draft(&h, id);
    without(&mut old, "Extra");
    // A styling edit in the same second as the previous note type write.
    let mut styled = notetypes::Draft::of(read(&h, id));
    styled.config.css = "x".into();
    notetypes::save(&h.collection, styled, notetypes::SaveOptions::default()).unwrap();
    assert!(matches!(run(&h, old).unwrap_err(), EngineError::Stale { .. }));
    assert_eq!(backups.calls(), 0);

    let mut current = draft(&h, id);
    without(&mut current, "Extra");
    backups.then(move |c| {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut styled = notetypes::Draft::of(reader.notetype(id).unwrap().unwrap());
        drop(reader);
        styled.config.css = "y".into();
        notetypes::save(c, styled, notetypes::SaveOptions::default()).unwrap();
    });
    let before = state(&h, id);
    assert!(matches!(run(&h, current).unwrap_err(), EngineError::Stale { .. }));
    assert_eq!(backups.calls(), 1, "one backup, no replan");
    let after = state(&h, id);
    assert_eq!((&after.notes, &after.cards, after.graves, after.scm), (&before.notes, &before.cards, before.graves, before.scm));
    let nt = read(&h, id);
    assert_eq!(nt.fields.len(), 3);
    assert_eq!(nt.config.css, "y");
}

/// S1.1: repeated origins are `InvalidArgument`, before `Stale`.
#[test]
fn repeated_origins_are_invalid_before_stale() {
    let (h, _) = harness();
    let id = trio(&h);
    let mut draft = draft(&h, id);
    draft.base = notetypes::AggregateDigest([0; 32]);
    draft.fields[1].origin = Some(0);
    assert!(matches!(run(&h, draft.clone()).unwrap_err(), EngineError::InvalidArgument { .. }));
    draft.fields[1].origin = Some(1);
    assert!(matches!(run(&h, draft.clone()).unwrap_err(), EngineError::Stale { .. }));
    let mut out_of_range = StructureDraft::of(&read(&h, id));
    out_of_range.fields[2].origin = Some(9);
    assert!(matches!(run(&h, out_of_range.clone()).unwrap_err(), EngineError::InvalidArgument { .. }));
    // Before `Stale` too (S1.2: "before them all").
    out_of_range.base = notetypes::AggregateDigest([0; 32]);
    assert!(matches!(run(&h, out_of_range).unwrap_err(), EngineError::InvalidArgument { .. }));
}

/// S1.2 items 2 and 3: the last field or template, and a protected field.
#[test]
fn last_parts_and_protected_fields_are_refused() {
    let (h, backups) = harness();
    let id = trio(&h);
    let mut none = draft(&h, id);
    none.fields.clear();
    assert!(matches!(run(&h, none).unwrap_err(), EngineError::NotetypeRefused(r) if r.kind() == "invalid_notetype"));
    let mut none = draft(&h, id);
    none.templates.clear();
    assert!(matches!(run(&h, none).unwrap_err(), EngineError::NotetypeRefused(r) if r.kind() == "invalid_notetype"));

    let cloze = {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        notetypes::read::by_name(&*reader, "Cloze").unwrap().unwrap()
    };
    assert!(cloze.fields[0].config.prevent_deletion);
    let mut draft = StructureDraft::of(&cloze);
    draft.fields.remove(0);
    assert_eq!(
        run(&h, draft).unwrap_err(),
        EngineError::NotetypeStructure(StructureRefusal::ProtectedField { field: 0 })
    );
    assert_eq!(backups.calls(), 0);
}

/// Seeds a note of `mid` storing `fields` as given, with no card.
fn seed_note(h: &Harness, mid: NotetypeId, fields: &str) -> NoteId {
    let mut note = common::note(&[]);
    note.notetype_id = mid;
    note.fields = fields.to_owned();
    note.guid = format!("seed{fields}");
    h.collection
        .run(
            OpSpec::named("seed", OpSource::UserNonUndoable).skip_undo(),
            move |ctx| Ok(ctx.scope().add_note(&note)?),
        )
        .unwrap()
        .value
}

/// S1.2 item 4: a note with more values than the base has fields, on the
/// planning snapshot and when one is added after planning.
#[test]
fn notes_with_extra_values_are_refused_at_planning_and_in_the_write_phase() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let bad = seed_note(&h, id, "1\u{1f}2\u{1f}3\u{1f}4");
    let mut draft = draft(&h, id);
    draft.fields.swap(0, 1);
    assert_eq!(
        run(&h, draft.clone()).unwrap_err(),
        EngineError::NotetypeStructure(StructureRefusal::NoteFieldCountMismatch { note: bad })
    );
    assert_eq!(backups.calls(), 0);
    h.collection
        .run(OpSpec::named("repair", OpSource::UserNonUndoable).skip_undo(), move |ctx| {
            ctx.scope().remove_note(bad)?;
            Ok(())
        })
        .unwrap();
    let before = state(&h, id);
    backups.then(move |c| {
        let mut note = common::note(&[]);
        note.notetype_id = id;
        note.fields = "5\u{1f}6\u{1f}7\u{1f}8".into();
        note.guid = "late".into();
        c.run(OpSpec::named("seed", OpSource::UserNonUndoable).skip_undo(), move |ctx| {
            Ok(ctx.scope().add_note(&note)?)
        })
        .unwrap();
    });
    let error = run(&h, draft).unwrap_err();
    assert!(
        matches!(error, EngineError::NotetypeStructure(StructureRefusal::NoteFieldCountMismatch { .. })),
        "{error:?}"
    );
    let after = state(&h, id);
    assert_eq!(after.notetype, before.notetype);
    assert_eq!(after.notes[0], before.notes[0], "the existing note is not rewritten");
}

/// S10: an injected failure in the write phase, after notes and cards were
/// written, rolls everything back; no undo step is recorded.
#[test]
fn a_failure_in_the_write_phase_rolls_everything_back() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "x"]);
    add(&h, id, &["c", "", ""]);
    let before = state(&h, id);
    let undo = undo_name(&h);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    without_template(&mut draft, "Reverse");
    confirm(&h, &mut draft);
    meguri_model::faultpoint::arm(
        "notetypes.structure.write_phase",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let error = run(&h, draft.clone()).unwrap_err();
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(error, EngineError::Internal { .. }), "{error:?}");
    assert_eq!(backups.calls(), 1);
    assert_eq!(state(&h, id), before);
    assert_eq!(undo_name(&h), undo);
    // Without the fault the same draft commits.
    let saved = run(&h, draft).unwrap().value;
    assert_eq!((saved.cards_removed, saved.notes_rewritten), (1, 2));
}

/// S6, S13: one step; undo brings the cards back and removes the graves,
/// `scm` returns; redo removes them again.
#[test]
fn a_template_removal_is_one_step_that_undo_and_redo_restore() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    add(&h, id, &["", "only back", ""]);
    h.clock.step_ms(60_000);
    let before = state(&h, id);
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Forward");
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.cards_removed, 1);
    assert_eq!(saved.cards_renumbered, 2, "Reverse cards move from 1 to 0");
    assert_eq!(saved.notes_without_cards, 0);
    assert_eq!(saved.reasons, [SchemaReason::TemplatesChanged]);
    let after = state(&h, id);
    assert_eq!(after.graves, 1);
    assert!(after.scm > before.scm);
    assert!(after.cards.iter().all(|c| c.2 == 0));
    h.collection.undo(UndoScope::Global).unwrap();
    let undone = state(&h, id);
    assert_eq!(undone.cards, before.cards);
    assert_eq!(undone.notes, before.notes);
    assert_eq!((undone.graves, undone.scm), (before.graves, before.scm));
    assert_eq!(undone.notetype, before.notetype);
    h.collection.redo(UndoScope::Global).unwrap();
    let redone = state(&h, id);
    assert_eq!((redone.cards, redone.graves, redone.scm), (after.cards, after.graves, after.scm));
}

/// T6.1 coalescing: a second schema save within 10 minutes reuses the
/// backup; rename-only, text-only and unchanged saves take none; undo and
/// redo take none.
#[test]
fn backups_follow_t6_1() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut unchanged = draft(&h, id);
    unchanged.skip_checks = false;
    let saved = run(&h, unchanged).unwrap().value;
    assert!(!saved.written);
    let mut renamed = draft(&h, id);
    renamed.fields[2].name = "Notes".into();
    run(&h, renamed).unwrap();
    let mut text = draft(&h, id);
    text.templates[0].back = "{{Back}}<hr>".into();
    run(&h, text).unwrap();
    assert_eq!(backups.calls(), 0);
    let mut moved = draft(&h, id);
    moved.fields.swap(1, 2);
    run(&h, moved).unwrap();
    assert_eq!(backups.calls(), 1);
    h.clock.step_ms(9 * 60_000);
    let mut added = draft(&h, id);
    added.fields.push(DraftField {
        origin: None,
        name: "New".into(),
        options: FieldOptions::new_field(),
    });
    run(&h, added).unwrap();
    assert_eq!(backups.calls(), 1, "coalesced within 10 minutes");
    h.collection.undo(UndoScope::Global).unwrap();
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(backups.calls(), 1, "undo and redo take none");
    h.clock.step_ms(60_000);
    let mut again = draft(&h, id);
    again.fields.swap(0, 1);
    run(&h, again).unwrap();
    assert_eq!(backups.calls(), 2, "a fresh one at 10 minutes");
}

/// S1: a rename-only draft has exactly the field rename op's result.
#[test]
fn a_rename_only_draft_is_the_field_rename_op() {
    let (a, _) = harness();
    let (b, _) = harness();
    let ia = trio(&a);
    let ib = trio(&b);
    assert_eq!(ia, ib);
    for h in [&a, &b] {
        add(h, ia, &["a", "b", ""]);
    }
    let mut draft = draft(&a, ia);
    draft.fields[0].name = "Question".into();
    let saved = run(&a, draft).unwrap().value;
    assert!(saved.written && saved.reasons.is_empty());
    let nt = read(&b, ib);
    rename::rename_fields(
        &b.collection,
        rename::RenameFields {
            notetype: ib,
            base: notetypes::AggregateDigest::of(&nt),
            fields: vec![rename::FieldRename {
                field: rename::FieldIdentity::of(&nt.fields[0]),
                name: "Question".into(),
            }],
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert_eq!(state(&a, ia), state(&b, ib));
    assert_eq!(undo_name(&a), undo_name(&b));
}

/// S2: the sort field is followed by identity: a move that keeps it reports
/// no `sort_field_changed`; removing it does.
#[test]
fn the_sort_field_reason_follows_the_field() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut moved = draft(&h, id);
    let front = moved.fields.remove(0);
    moved.fields.push(front);
    let saved = run(&h, moved).unwrap().value;
    assert_eq!(saved.reasons, [SchemaReason::FieldsChanged]);
    let nt = read(&h, id);
    assert_eq!(nt.fields[nt.config.sort_field_idx as usize].name, "Front");
    let mut removed = draft(&h, id);
    without(&mut removed, "Front");
    confirm(&h, &mut removed);
    let saved = run(&h, removed).unwrap().value;
    assert_eq!(saved.reasons, [SchemaReason::FieldsChanged, SchemaReason::SortFieldChanged]);
}

/// S2, S6, S7: a template removed and another added at the same count
/// changes the templates: graves, `scm`, a backup, new cards where the
/// front requires them.
#[test]
fn replacing_a_template_at_the_same_count_changes_the_templates() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "e"]);
    let scm = state(&h, id).scm;
    h.clock.step_ms(1_000);
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.templates.push(DraftTemplate {
        origin: None,
        name: "Extra card".into(),
        front: "{{Extra}}".into(),
        back: String::new(),
        browser_front: String::new(),
        browser_back: String::new(),
        deck: 0,
        browser_font_name: String::new(),
        browser_font_size: 0,
    });
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.reasons, [SchemaReason::TemplatesChanged]);
    assert_eq!((saved.cards_removed, saved.cards_added), (1, 1));
    assert_eq!(backups.calls(), 1);
    let after = state(&h, id);
    assert_eq!(after.graves, 1);
    assert!(after.scm > scm);
    let nt = read(&h, id);
    assert!(nt.templates[1].config.id.is_some(), "a new template gets a fresh id");
    assert_ne!(nt.templates[1].config.id, nt.templates[0].config.id);
}

/// S10, S5: a note added after planning is rewritten with the new layout.
#[test]
fn a_note_added_after_planning_is_rewritten() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    draft.fields.swap(0, 2);
    backups.then(move |c| {
        notes::add(
            c,
            notes::AddNote {
                notetype_id: id,
                fields: vec!["late front".into(), "late back".into(), "late extra".into()],
                tags: Vec::new(),
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    });
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.notes_rewritten, 2);
    let after = state(&h, id);
    assert!(after.notes.iter().any(|n| n.1 == "late extra\u{1f}late back\u{1f}late front"));
}

/// S10: a cancelled plan writes nothing and takes no backup.
#[test]
fn a_cancelled_plan_writes_nothing() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let before = state(&h, id);
    let mut draft = draft(&h, id);
    draft.fields.swap(0, 1);
    let cancel = CancelToken::new();
    cancel.cancel();
    let error = structure::apply(&h.collection, draft, cancel, ProgressSink::new()).unwrap_err();
    assert_eq!(error, EngineError::Cancelled);
    assert_eq!(backups.calls(), 0);
    assert_eq!(state(&h, id), before);
}

/// S11: the cards of each listed template and the notes left with none.
#[test]
fn the_removal_preview_counts_cards_and_notes_left_with_none() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    for i in 0..5 {
        add(&h, id, &["", &format!("back {i}"), ""]);
    }
    let preview = structure::preview_removal(&h.collection, id, &BTreeSet::from([1])).unwrap();
    assert_eq!(preview.cards.get(&1), Some(&6));
    assert_eq!(preview.notes_without_cards, 5);
    let preview = structure::preview_removal(&h.collection, id, &BTreeSet::from([0])).unwrap();
    assert_eq!(preview.cards.get(&0), Some(&1));
    assert_eq!(preview.notes_without_cards, 0);
}

/// G4: the op takes no confirmation input: a schema change with
/// `scm <= ls` commits at once.
#[test]
fn a_schema_change_commits_without_a_confirmation() {
    let (h, _) = harness();
    let id = trio(&h);
    h.collection
        .run(
            OpSpec::named("synced", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::Raw),
            |ctx| {
                let mut col = ctx.scope().col()?;
                col.last_sync = col.schema_modified;
                ctx.scope().put_col(&col)?;
                Ok(())
            },
        )
        .unwrap();
    h.clock.step_ms(1_000);
    let mut draft = draft(&h, id);
    draft.fields.swap(0, 1);
    run(&h, draft).unwrap();
    let col = h.collection.reader(ReaderRole::Query).unwrap().col().unwrap();
    assert!(col.schema_modified.0 > col.last_sync.0);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.grave(0, GraveKind::Card).unwrap().is_none());
}

fn template(name: &str, front: &str) -> DraftTemplate {
    DraftTemplate {
        origin: None,
        name: name.into(),
        front: front.into(),
        back: String::new(),
        browser_front: String::new(),
        browser_back: String::new(),
        deck: 0,
        browser_font_name: String::new(),
        browser_font_size: 0,
    }
}

/// S2, S6 row 4, S7: a template removed and a new one with the same front
/// added at its position is a template added: its cards are generated.
#[test]
fn a_template_re_added_with_the_same_front_generates_its_cards() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    add(&h, id, &["c", "d", ""]);
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.templates.push(template("Reverse again", "{{Back}}"));
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!((saved.cards_removed, saved.cards_added), (2, 2));
    let after = state(&h, id);
    assert_eq!(after.cards.iter().filter(|c| c.2 == 1).count(), 2);
    assert_eq!(after.graves, 2);
}

/// S6 step 2: a renumbered card changes only its `ord` and stamps; its
/// other stored columns stay as stored, even ones a typed card would
/// rewrite.
#[test]
fn renumbering_keeps_every_other_stored_column() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let card = {
        let reader = h.collection.reader(ReaderRole::Query).unwrap();
        let note = reader.notetype_notes_after(id, None, 10).unwrap().remove(0);
        reader.cards_of_note(note.id).unwrap().into_iter().find(|c| c.ord == 1).unwrap().id
    };
    let key = meguri_storage::RowKey::Id(card.0);
    h.collection
        .run(
            OpSpec::named("seed odd data", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::Raw),
            move |ctx| {
                let mut image = ctx.scope().row_image(meguri_storage::Table::Cards, &key)?.unwrap();
                image[17] = meguri_storage::Cell::text("not json");
                ctx.scope().restore(meguri_storage::Table::Cards, &key, Some(&image))?;
                Ok(())
            },
        )
        .unwrap();
    let image = |h: &Harness| {
        h.collection
            .reader(ReaderRole::Query)
            .unwrap()
            .row_image(meguri_storage::Table::Cards, &meguri_storage::RowKey::Id(card.0))
            .unwrap()
            .unwrap()
    };
    let before = image(&h);
    let mut draft = draft(&h, id);
    draft.templates.swap(0, 1);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.cards_renumbered, 2);
    let after = image(&h);
    assert_eq!(after[3], meguri_storage::Cell::Integer(0));
    for column in (0..after.len()).filter(|c| ![3, 4, 5].contains(c)) {
        assert_eq!(after[column], before[column], "column {column}");
    }
}

/// S5, S13: a save that rewrites every note is one step; undo gives back
/// each note's values and stamps, redo the rewritten ones.
#[test]
fn undo_and_redo_of_a_note_rewrite() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "x"]);
    add(&h, id, &["c", "", "y"]);
    h.clock.step_ms(60_000);
    let before = state(&h, id);
    let mut draft = draft(&h, id);
    let extra = draft.fields.remove(2);
    draft.fields.insert(0, extra);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.notes_rewritten, 2);
    let after = state(&h, id);
    assert!(after.notes.iter().any(|n| n.1 == "x\u{1f}a\u{1f}b"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(state(&h, id), before);
    h.collection.redo(UndoScope::Global).unwrap();
    let redone = state(&h, id);
    assert_eq!((redone.notes, redone.scm), (after.notes, after.scm));
}

/// S5 (O7 re-derivation): a field edit committed while the save planned
/// survives, moved to its field's new position.
#[test]
fn a_field_edit_during_planning_survives_at_its_new_position() {
    let (h, backups) = harness();
    let id = trio(&h);
    let nid = add(&h, id, &["a", "b", "x"]);
    let mut draft = draft(&h, id);
    draft.fields.swap(0, 2);
    backups.then(move |c| {
        notes::update::update(
            c,
            notes::update::UpdateNote {
                id: nid,
                fields: vec!["edited".into(), "b".into(), "x".into()],
                tags: Vec::new(),
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    });
    run(&h, draft).unwrap();
    let note = h.collection.reader(ReaderRole::Query).unwrap().note(nid).unwrap().unwrap();
    assert_eq!(note.fields, "x\u{1f}b\u{1f}edited");
}

/// N4 step 7 on the live rows: another note type that took the draft's new
/// name while the save backed up gets the name; the save stores `+` after
/// it and reports the stored name.
#[test]
fn a_name_taken_during_the_backup_is_uniqued_on_the_live_rows() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    draft.name = "Renamed".into();
    draft.fields.swap(0, 1);
    backups.then(|c| {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut other = notetypes::Draft::of(notetypes::read::by_name(&*reader, "Basic").unwrap().unwrap());
        drop(reader);
        other.name = "renamed".into();
        notetypes::save(c, other, notetypes::SaveOptions::default()).unwrap();
    });
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.name, "Renamed+");
    assert_eq!(read(&h, id).name, "Renamed+");
}

/// A card the save removes keeps its id: a card G9 adds in the same save
/// never gets it, so the grave and review log stay the removed card's.
/// Under the test's frozen clock the allocator would otherwise pick the
/// removed card's id (the largest one plus one, once it is gone).
#[test]
fn a_removed_card_id_is_never_given_to_a_generated_card() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let removed = state(&h, id).cards.iter().find(|c| c.2 == 1).unwrap().0;
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.templates.push(template("Reverse again", "{{Back}}"));
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!((saved.cards_removed, saved.cards_added), (1, 1));
    let after = state(&h, id);
    assert!(after.cards.iter().all(|c| c.0 != removed), "{:?}", after.cards);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.grave(removed, GraveKind::Card).unwrap().is_some());
}

/// V1: a checked draft runs V3 also when it only renames fields (the
/// field rename op skips it) and when it changes nothing (N3).
#[test]
fn checked_drafts_run_the_template_checks_on_every_path() {
    let (h, _) = harness();
    let id = trio(&h);
    let mut broken = draft(&h, id);
    broken.templates[1].back = "{{Missing}}".into();
    run(&h, broken).unwrap();
    let mut renamed = StructureDraft::of(&read(&h, id));
    renamed.fields[2].name = "Notes".into();
    let error = run(&h, renamed.clone()).unwrap_err();
    assert!(matches!(&error, EngineError::NotetypeRefused(r) if r.kind() == "unknown_field"), "{error:?}");
    renamed.skip_checks = true;
    run(&h, renamed).unwrap();
    let unchanged = StructureDraft::of(&read(&h, id));
    let error = run(&h, unchanged.clone()).unwrap_err();
    assert!(matches!(&error, EngineError::NotetypeRefused(r) if r.kind() == "unknown_field"), "{error:?}");
    let mut unchecked = unchanged;
    unchecked.skip_checks = true;
    assert!(!run(&h, unchecked).unwrap().value.written);
}

/// A removed card with the largest possible id keeps it: the card the same
/// save generates gets a free id inside I3's window instead.
#[test]
fn no_generated_card_takes_the_largest_removed_id() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let old = state(&h, id).cards.iter().find(|c| c.2 == 1).unwrap().0;
    h.collection
        .run(
            OpSpec::named("seed largest id", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::Raw),
            move |ctx| {
                let key = meguri_storage::RowKey::Id(old);
                let mut image = ctx.scope().row_image(meguri_storage::Table::Cards, &key)?.unwrap();
                image[0] = meguri_storage::Cell::Integer(i64::MAX);
                ctx.scope().restore(meguri_storage::Table::Cards, &key, None)?;
                ctx.scope()
                    .restore(meguri_storage::Table::Cards, &meguri_storage::RowKey::Id(i64::MAX), Some(&image))?;
                Ok(())
            },
        )
        .unwrap();
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.templates.push(template("Reverse again", "{{Back}}"));
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!((saved.cards_removed, saved.cards_added), (1, 1));
    let after = state(&h, id);
    assert!(after.cards.iter().all(|c| c.0 != i64::MAX), "{:?}", after.cards);
    let reader = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(reader.grave(i64::MAX, GraveKind::Card).unwrap().is_some());
}

/// N4 step 7 from the requested name: a name another note type frees while
/// the save backs up is stored without the `+` planning gave it.
#[test]
fn a_name_freed_during_the_backup_is_stored_as_requested() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    draft.name = "Basic".into();
    draft.fields.swap(0, 1);
    backups.then(|c| {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut other = notetypes::Draft::of(notetypes::read::by_name(&*reader, "Basic").unwrap().unwrap());
        drop(reader);
        other.name = "Elsewhere".into();
        notetypes::save(c, other, notetypes::SaveOptions::default()).unwrap();
    });
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.name, "Basic");
    assert_eq!(read(&h, id).name, "Basic");
}

/// N6: a field replaced by a new one of the same name recomputes the
/// requirements, though names and fronts read the same.
#[test]
fn a_field_replaced_under_the_same_name_recomputes_requirements() {
    let (h, _) = harness();
    let id = trio(&h);
    let mut seeded = read(&h, id);
    seeded.config.reqs[0].kind = 0;
    seeded.config.reqs[0].field_ords.clear();
    h.collection
        .run(
            OpSpec::named("stale reqs", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::CallerSupplied),
            move |ctx| {
                ctx.scope().put_notetype(&seeded)?;
                Ok(())
            },
        )
        .unwrap();
    let mut draft = draft(&h, id);
    let extra = draft.fields.remove(2);
    draft.fields.push(DraftField {
        origin: None,
        name: extra.name,
        options: FieldOptions::new_field(),
    });
    run(&h, draft).unwrap();
    let nt = read(&h, id);
    assert_eq!((nt.config.reqs[0].kind, nt.config.reqs[0].field_ords.clone()), (1, vec![0]));
}

fn edit(c: &Collection, nid: NoteId, fields: &[&str]) {
    notes::update::update(
        c,
        notes::update::UpdateNote {
            id: nid,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            tags: Vec::new(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
}

fn losses(error: EngineError) -> meguri_engine::notetypes::structure::DraftSummary {
    match error {
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(losses)) => match *losses {
            Losses::Draft(summary) => summary,
            other => panic!("not a draft summary: {other:?}"),
        },
        other => panic!("not LossesChanged: {other:?}"),
    }
}

/// A note that gains content in a removed field between the summary read
/// and the commit: the write transaction counts again, writes nothing and
/// returns `LossesChanged` with the new count. The safety backup taken
/// before stays; the draft's step is not recorded.
#[test]
fn a_note_that_gains_content_in_a_removed_field_before_the_commit_is_losses_changed() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "x"]);
    let late = add(&h, id, &["c", "d", ""]);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    // The summary read: one note has content in Extra.
    draft.confirmed.fields.insert(2, 1);
    backups.then(move |c| edit(c, late, &["c", "d", "late"]));
    let error = run(&h, draft.clone()).unwrap_err();
    let summary = losses(error);
    assert!(summary.loses_content);
    assert!(summary.changes.contains(&DraftChange::FieldRemoved {
        ord: 2,
        name: "Extra".into(),
        notes_with_content: 2,
    }));
    assert_eq!(summary.reasons, [SchemaReason::FieldsChanged]);
    assert_eq!(backups.calls(), 1, "the backup ran before the write phase and stays");
    let after = state(&h, id);
    assert_eq!(read(&h, id).fields.len(), 3, "nothing of the draft was written");
    assert!(after.notes.iter().any(|n| n.1 == "c\u{1f}d\u{1f}late"));
    assert_eq!(undo_name(&h).as_deref(), Some("Update Note"), "no Update Note Type step");

    // Confirmed with the new count, the same draft saves.
    let mut draft = StructureDraft::of(&read(&h, id));
    draft.skip_checks = true;
    without(&mut draft, "Extra");
    draft.confirmed.fields.insert(2, 2);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.notes_rewritten, 2);
    assert_eq!(saved.reasons, [SchemaReason::FieldsChanged]);
    assert_eq!(read(&h, id).fields.len(), 2);
    assert_eq!(undo_name(&h).as_deref(), Some("Update Note Type"));
}

/// A card that appears under a removed template between the summary read
/// and the commit: `LossesChanged`, nothing written.
#[test]
fn a_card_that_appears_under_a_removed_template_before_the_commit_is_losses_changed() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.confirmed.templates.insert(1, 1);
    // A note added meanwhile gets a Reverse card.
    backups.then(move |c| {
        notes::add(
            c,
            notes::AddNote {
                notetype_id: id,
                fields: vec!["c".into(), "d".into(), String::new()],
                tags: Vec::new(),
                deck_id: DeckId(1),
                guid: None,
                update_add_defaults: false,
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    });
    let before_templates = read(&h, id).templates.len();
    let summary = losses(run(&h, draft).unwrap_err());
    assert!(summary.changes.contains(&DraftChange::TemplateRemoved {
        ord: 1,
        name: "Reverse".into(),
        cards_removed: 2,
        notes_without_cards: 0,
    }));
    let after = state(&h, id);
    assert_eq!(read(&h, id).templates.len(), before_templates);
    assert_eq!(after.cards.len(), 4, "both notes keep both cards");
    assert_eq!(after.graves, 0);
    assert_eq!(undo_name(&h).as_deref(), Some("Add Note"));
}

/// Losses already larger on the planning snapshot: refused before the
/// backup, which is never taken.
#[test]
fn losses_larger_on_the_planning_snapshot_take_no_backup() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "x"]);
    let before = state(&h, id);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    let summary = losses(run(&h, draft).unwrap_err());
    assert!(summary.loses_content);
    assert_eq!(backups.calls(), 0);
    assert_eq!(state(&h, id), before);
}

/// Content that counts as empty (E1) is not a loss; a missing value is not
/// either.
#[test]
fn empty_values_are_no_loss() {
    let (h, backups) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", "<br> "]);
    add(&h, id, &["a2", "b2", ""]);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.notes_rewritten, 2);
    assert_eq!(backups.calls(), 1);
}

/// D8: a field removed with no sheet (confirmed 0) that gains content
/// between the read and the commit refuses too.
#[test]
fn a_field_confirmed_at_zero_that_gains_content_is_losses_changed() {
    let (h, backups) = harness();
    let id = trio(&h);
    let nid = add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    backups.then(move |c| edit(c, nid, &["a", "b", "now filled"]));
    let before_fields = read(&h, id).fields.len();
    let summary = losses(run(&h, draft).unwrap_err());
    assert!(summary.changes.contains(&DraftChange::FieldRemoved {
        ord: 2,
        name: "Extra".into(),
        notes_with_content: 1,
    }));
    assert_eq!(read(&h, id).fields.len(), before_fields);
    assert_eq!(undo_name(&h).as_deref(), Some("Update Note"));
}

/// D8: the confirmation is by count: a count that shrinks (or stays equal
/// while other notes hold the content) does not refuse.
#[test]
fn a_count_that_shrinks_still_commits() {
    let (h, backups) = harness();
    let id = trio(&h);
    let first = add(&h, id, &["a", "b", "x"]);
    add(&h, id, &["c", "d", "y"]);
    let third = add(&h, id, &["e", "f", ""]);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    draft.confirmed.fields.insert(2, 2);
    backups.then(move |c| {
        edit(c, first, &["a", "b", ""]);
        edit(c, third, &["e", "f", "moved here"]);
    });
    let saved = run(&h, draft).unwrap().value;
    assert_eq!(saved.notes_rewritten, 3);
    assert_eq!(read(&h, id).fields.len(), 2);
}

/// D8: when the base changed and a loss grew too, `Stale` wins; nothing is
/// written, no step recorded, the backup stays.
#[test]
fn stale_wins_over_losses_changed() {
    let (h, backups) = harness();
    let id = trio(&h);
    let nid = add(&h, id, &["a", "b", ""]);
    let mut draft = draft(&h, id);
    without(&mut draft, "Extra");
    backups.then(move |c| {
        edit(c, nid, &["a", "b", "content"]);
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut styled = notetypes::Draft::of(reader.notetype(id).unwrap().unwrap());
        drop(reader);
        styled.config.css = "changed".into();
        notetypes::save(c, styled, notetypes::SaveOptions::default()).unwrap();
    });
    let before_scm = state(&h, id).scm;
    assert!(matches!(run(&h, draft).unwrap_err(), EngineError::Stale { .. }));
    assert_eq!(backups.calls(), 1);
    let after = state(&h, id);
    assert_eq!(read(&h, id).fields.len(), 3, "nothing of the draft written");
    assert_eq!(after.scm, before_scm, "no schema change");
    assert!(after.notes.iter().all(|n| n.1.split('\u{1f}').count() == 3));
}

/// S1.2 (round 9 review): a note type removed before the save is `Stale`.
#[test]
fn a_vanished_note_type_is_stale() {
    let (h, _) = harness();
    let id = trio(&h);
    let draft = draft(&h, id);
    meguri_engine::notetypes::remove::remove(
        &h.collection,
        meguri_engine::notetypes::remove::RemoveNotetype {
            notetype: id,
            confirmed_notes: 0,
            confirmed_cards: 0,
        },
        CancelToken::new(),
        ProgressSink::new(),
    )
    .unwrap();
    assert!(matches!(run(&h, draft).unwrap_err(), EngineError::Stale { .. }));
}

/// I3 and S6/S7: a removed card dated more than a day ahead (a foreign or
/// damaged collection) keeps its id, and does not push the generated card
/// past I3's one-day window: the new card takes the clock's id.
#[test]
fn a_removed_card_far_ahead_does_not_move_the_generated_id() {
    let (h, _) = harness();
    let id = trio(&h);
    add(&h, id, &["a", "b", ""]);
    h.clock.step_ms(5_000);
    let ahead = NOW_MS + 5_000 + 2 * 86_400_000;
    let old = state(&h, id).cards.iter().find(|c| c.2 == 1).unwrap().0;
    h.collection
        .run(
            OpSpec::named("seed future id", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(StampMode::Raw),
            move |ctx| {
                let key = meguri_storage::RowKey::Id(old);
                let mut image = ctx.scope().row_image(meguri_storage::Table::Cards, &key)?.unwrap();
                image[0] = meguri_storage::Cell::Integer(ahead);
                ctx.scope().restore(meguri_storage::Table::Cards, &key, None)?;
                ctx.scope()
                    .restore(meguri_storage::Table::Cards, &meguri_storage::RowKey::Id(ahead), Some(&image))?;
                Ok(())
            },
        )
        .unwrap();
    let mut draft = draft(&h, id);
    without_template(&mut draft, "Reverse");
    draft.templates.push(template("Reverse again", "{{Back}}"));
    confirm(&h, &mut draft);
    let saved = run(&h, draft).unwrap().value;
    assert_eq!((saved.cards_removed, saved.cards_added), (1, 1));
    let new = state(&h, id).cards.iter().find(|c| c.2 == 1).unwrap().0;
    assert_ne!(new, ahead);
    assert!(new <= NOW_MS + 5_000 + 86_400_000, "inside I3's window: {new}");
    assert_eq!(new, NOW_MS + 5_000, "the clock's id, as with nothing retired");
}
