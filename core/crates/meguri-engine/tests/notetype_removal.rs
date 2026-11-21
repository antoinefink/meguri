//! Note type removal (`notetypes/structure-changes` S12) and Restore to
//! Default (`notetypes/restore-to-default` D1 to D7.2) beyond what the
//! scenarios show: their refusals, the confirmed losses counted again on the
//! planning snapshot and inside the write transaction (S12, D7, D7.1 and
//! `notetypes/draft-save` D8), their safety backups (T6.1: a removal never
//! coalesces), D6's single replan and its no-silent-replan rule for a
//! confirmed restore, D3's fresh parts, D4.1's kind change and one undo step
//! each.
//!
//! As in `notetype_structure.rs`, the backup provider's hook runs after
//! planning and before the write transaction.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{Harness, NOW_MS};
use meguri_engine::notetypes::remove::{RemoveNotetype, remove};
use meguri_engine::notetypes::restore::{RestoreNotetype, restore, restore_summary};
use meguri_engine::notetypes::structure::{ConfirmedLosses, DraftChange, Losses, RestoreSummary, StructureRefusal};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind, StockKind};
use meguri_engine::ports::{BackupFailureCause, BackupProtection, BackupPublication, BackupReceipt, BackupService};
use meguri_engine::ports::{GuardedBackup, SafetyBackupFailure};
use meguri_engine::{
    CancelToken, Collection, Engine, EngineConfig, EngineError, Ports, ProgressSink, SchemaReason, UndoScope, notes,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::{StepClock, UtcOffset};
use meguri_model::ids::{DeckId, NoteId, NotetypeId};
use meguri_model::notetype::Notetype;
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochMillis;
use meguri_storage::{ConfigValue, ReadRows, ReaderRole, Table};

type Hook = Box<dyn FnOnce(&Collection) + Send>;

#[derive(Default)]
struct Backups {
    calls: AtomicUsize,
    hooks: Mutex<Vec<Hook>>,
    fail: Mutex<bool>,
}

impl BackupService for Backups {
    fn before_deck_delete(&self) -> Result<(), EngineError> {
        panic!("structure ops use the safety seam")
    }

    fn before_safety_operation(&self, c: &Collection, cancel: &CancelToken) -> Result<GuardedBackup, EngineError> {
        let count = self.calls.fetch_add(1, Ordering::SeqCst) + 1;
        let hook = {
            let mut hooks = self.hooks.lock().unwrap();
            (!hooks.is_empty()).then(|| hooks.remove(0))
        };
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
        self.hooks.lock().unwrap().push(Box::new(hook));
    }
}

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

fn made(h: &Harness, name: &str, cloze: bool, fields: &[&str], fronts: &[&str]) -> NotetypeId {
    notetypes::add(
        &h.collection,
        NewNotetype {
            name: name.to_owned(),
            kind: if cloze {
                NotetypeKind::Cloze
            } else {
                NotetypeKind::Standard
            },
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            templates: fronts
                .iter()
                .enumerate()
                .map(|(i, front)| NewTemplate {
                    name: format!("Card {}", i + 1),
                    front: (*front).to_owned(),
                    back: if cloze { (*front).to_owned() } else { String::new() },
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

fn add(c: &Collection, mid: NotetypeId, fields: &[&str]) -> NoteId {
    notes::add(
        c,
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

fn read(h: &Harness, id: NotetypeId) -> Option<Notetype> {
    h.collection.reader(ReaderRole::Query).unwrap().notetype(id).unwrap()
}

fn counts(h: &Harness, id: NotetypeId) -> (u64, u64, u64, i64) {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let notes = r.notetype_note_count(id).unwrap();
    let mut cards = 0;
    for note in r.notetype_notes_after(id, None, 10_000).unwrap() {
        cards += r.cards_of_note(note.id).unwrap().len() as u64;
    }
    (notes, cards, r.row_count(Table::Graves).unwrap(), r.col().unwrap().schema_modified.0)
}

fn removal(id: NotetypeId, notes: u64, cards: u64) -> RemoveNotetype {
    RemoveNotetype {
        notetype: id,
        confirmed_notes: notes,
        confirmed_cards: cards,
    }
}

fn run_remove(h: &Harness, request: RemoveNotetype) -> Result<meguri_engine::notetypes::remove::Removed, EngineError> {
    remove(&h.collection, request, CancelToken::new(), ProgressSink::new()).map(|r| r.value)
}

fn run_restore(
    h: &Harness,
    request: RestoreNotetype,
) -> Result<meguri_engine::notetypes::structure::StructureSaved, EngineError> {
    restore(&h.collection, request, CancelToken::new(), ProgressSink::new()).map(|r| r.value)
}

fn undo_name(h: &Harness) -> Option<String> {
    h.collection.peek_undo(UndoScope::Global).unwrap().name().map(str::to_owned)
}

/// S12: a note added between the delete question and the commit makes the
/// removal write nothing and return the new counts.
#[test]
fn a_note_added_before_the_removal_commits_is_losses_changed() {
    let (h, backups) = harness();
    let id = made(&h, "Pair", false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    add(&h.collection, id, &["a", "b"]);
    backups.then(move |c| {
        add(c, id, &["c", ""]);
    });
    let error = run_remove(&h, removal(id, 1, 2)).unwrap_err();
    assert_eq!(
        error,
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(Box::new(Losses::Removal {
            notes: 2,
            cards: 3
        })))
    );
    assert_eq!(backups.calls(), 1);
    assert!(read(&h, id).is_some());
    assert_eq!(counts(&h, id).0, 2);
    assert_eq!(undo_name(&h).as_deref(), Some("Add Note"));
    // A card that appears under it (a note's second card) counts too.
    let note = add(&h.collection, id, &["e", ""]);
    backups.then(move |c| {
        notes::update::update(
            c,
            notes::update::UpdateNote {
                id: note,
                fields: vec!["e".into(), "f".into()],
                tags: Vec::new(),
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    });
    let error = run_remove(&h, removal(id, 3, 4)).unwrap_err();
    assert!(matches!(
        error,
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(ref l)) if **l == Losses::Removal { notes: 3, cards: 5 }
    ));
    let removed = run_remove(&h, removal(id, 3, 5)).unwrap();
    assert!(removed.removed);
    assert_eq!((removed.notes_removed, removed.cards_removed), (3, 5));
    assert!(read(&h, id).is_none());
    assert_eq!(undo_name(&h).as_deref(), Some("Remove Note Type"));
}

/// S12: graves for every card and note, the `_nt_<id>_` keys gone,
/// `curModel` moved, `scm` set; undo restores all of it.
#[test]
fn a_removal_is_one_step_that_undo_restores() {
    let (h, backups) = harness();
    let id = made(&h, "Pair", false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    let alpha = made(&h, "Alpha", false, &["Text"], &["{{Text}}"]);
    add(&h.collection, id, &["a", "b"]);
    add(&h.collection, alpha, &["s"]);
    h.collection
        .run(
            meguri_engine::OpSpec::named("select", OpSource::UserNonUndoable).skip_undo(),
            move |ctx| {
                ctx.scope().set_config(notetypes::CURRENT_NOTETYPE_KEY, &ConfigValue::from(id.0))?;
                ctx.scope().set_config(&format!("_nt_{}_lastDeck", id.0), &ConfigValue::from(1i64))?;
                Ok(())
            },
        )
        .unwrap();
    h.clock.step_ms(60_000);
    let before = counts(&h, id);
    let removed = run_remove(&h, removal(id, 1, 2)).unwrap();
    assert!(removed.removed);
    assert_eq!(backups.calls(), 1);
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(r.row_count(Table::Graves).unwrap(), before.2 + 3);
    assert!(r.config_value(&format!("_nt_{}_lastDeck", id.0)).unwrap().is_none());
    assert_eq!(r.config_value(notetypes::CURRENT_NOTETYPE_KEY).unwrap(), Some(ConfigValue::from(alpha.0)));
    assert!(r.col().unwrap().schema_modified.0 > before.3);
    drop(r);
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(counts(&h, id), before);
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(r.config_value(notetypes::CURRENT_NOTETYPE_KEY).unwrap(), Some(ConfigValue::from(id.0)));
    assert!(r.config_value(&format!("_nt_{}_lastDeck", id.0)).unwrap().is_some());
}

/// S12's refusals and no-op: the only note type, a failed backup, a missing
/// note type (no backup); and a removal never coalesces its backup.
#[test]
fn removal_refusals_and_backups() {
    let (h, backups) = harness();
    let id = made(&h, "Pair", false, &["Front", "Back"], &["{{Front}}"]);
    add(&h.collection, id, &["a", "b"]);
    *backups.fail.lock().unwrap() = true;
    assert!(matches!(run_remove(&h, removal(id, 1, 1)).unwrap_err(), EngineError::SafetyBackupFailed(_)));
    assert!(read(&h, id).is_some());
    *backups.fail.lock().unwrap() = false;
    let missing = run_remove(&h, removal(NotetypeId(1), 0, 0)).unwrap();
    assert!(!missing.removed);
    assert_eq!(backups.calls(), 1);
    run_remove(&h, removal(id, 1, 1)).unwrap();
    let other = made(&h, "Other", false, &["F"], &["{{F}}"]);
    run_remove(&h, removal(other, 0, 0)).unwrap();
    assert_eq!(backups.calls(), 3, "every removal takes its own backup");
    // Remove all but one note type, then the last is refused.
    let ids: Vec<NotetypeId> = notetypes::read::ids(&*h.collection.reader(ReaderRole::Query).unwrap()).unwrap();
    for nt in &ids[1..] {
        let (notes, cards, ..) = counts(&h, *nt);
        run_remove(&h, removal(*nt, notes, cards)).unwrap();
    }
    assert_eq!(
        run_remove(&h, removal(ids[0], 0, 0)).unwrap_err(),
        EngineError::NotetypeStructure(StructureRefusal::LastNotetype)
    );
}

fn restoring(id: NotetypeId, kind: Option<StockKind>) -> RestoreNotetype {
    RestoreNotetype {
        notetype: id,
        kind,
        confirmed: ConfirmedLosses::default(),
        digest: None,
    }
}

fn summary(h: &Harness, id: NotetypeId, kind: Option<StockKind>) -> RestoreSummary {
    restore_summary(&h.collection, id, kind, &CancelToken::new(), &ProgressSink::new()).unwrap()
}

/// What a caller does after showing D7.1's question.
fn confirmed(h: &Harness, id: NotetypeId, kind: Option<StockKind>) -> RestoreNotetype {
    let read = summary(h, id, kind);
    RestoreNotetype {
        notetype: id,
        kind,
        confirmed: read.confirmed(),
        digest: Some(read.digest),
    }
}

fn restore_losses(error: EngineError) -> RestoreSummary {
    match error {
        EngineError::NotetypeStructure(StructureRefusal::LossesChanged(losses)) => match *losses {
            Losses::Restore(summary) => summary,
            other => panic!("not a restore summary: {other:?}"),
        },
        other => panic!("not LossesChanged: {other:?}"),
    }
}

/// D7: a note that gains content in a field past the default's count
/// between the question and the commit: `LossesChanged` with a new
/// `restore_summary`, nothing written; the backup stays.
#[test]
fn a_restore_whose_field_losses_grow_before_the_commit_is_losses_changed() {
    let (h, backups) = harness();
    let id = made(&h, "Plain", false, &["Q", "A", "Extra"], &["{{Q}}"]);
    add(&h.collection, id, &["a", "b", "c"]);
    let late = add(&h.collection, id, &["d", "e", ""]);
    let request = confirmed(&h, id, None);
    assert_eq!(request.confirmed.fields.get(&2), Some(&1));
    backups.then(move |c| {
        notes::update::update(
            c,
            notes::update::UpdateNote {
                id: late,
                fields: vec!["d".into(), "e".into(), "late".into()],
                tags: Vec::new(),
            },
            OpSource::UserUndoable,
        )
        .unwrap();
    });
    let before = read(&h, id).unwrap();
    let fresh = restore_losses(run_restore(&h, request).unwrap_err());
    assert!(fresh.summary.changes.contains(&DraftChange::FieldRemoved {
        ord: 2,
        name: "Extra".into(),
        notes_with_content: 2
    }));
    assert_eq!(fresh.summary.reasons, [SchemaReason::NotetypeRestored]);
    assert_eq!(backups.calls(), 1, "the backup stays");
    let after = read(&h, id).unwrap();
    assert_eq!(after.fields.len(), before.fields.len());
    assert_eq!(after.config.to_bytes(), before.config.to_bytes());
    assert_eq!(undo_name(&h).as_deref(), Some("Update Note"), "no step");
    // Confirmed again from the new question, it commits.
    let saved = run_restore(&h, confirmed(&h, id, None)).unwrap();
    assert_eq!(saved.reasons, [SchemaReason::NotetypeRestored]);
    assert_eq!(read(&h, id).unwrap().fields.len(), 2);
}

/// D7: a card that appears under a template past the default's count.
#[test]
fn a_restore_whose_template_losses_grow_before_the_commit_is_losses_changed() {
    let (h, backups) = harness();
    let id = made(&h, "Plain", false, &["Front", "Back"], &["{{Front}}", "{{Back}}"]);
    add(&h.collection, id, &["a", "b"]);
    let request = confirmed(&h, id, None);
    assert_eq!(request.confirmed.templates.get(&1), Some(&1));
    backups.then(move |c| {
        add(c, id, &["c", "d"]);
    });
    let fresh = restore_losses(run_restore(&h, request).unwrap_err());
    assert!(fresh.summary.changes.iter().any(|c| matches!(c, DraftChange::TemplateRemoved { ord: 1, cards_removed: 2, .. })));
    assert_eq!(read(&h, id).unwrap().templates.len(), 2);
    assert_eq!(counts(&h, id).1, 4);
}

/// D4.1, D7, D7.1: Basic given to a Cloze note type with cards at ords 0,
/// 1 and 2: two kind-change cards per such note are the confirmed losses;
/// one more such card after the question is `LossesChanged`; confirmed
/// again, ord 0 is kept and ords 1 and 2 go with graves, schema-classified
/// although both counts stay; D3's fresh ids; undo restores everything.
#[test]
fn a_cloze_note_type_restored_as_basic_confirms_its_kind_change_cards() {
    let (h, backups) = harness();
    let id = made(&h, "Gaps", true, &["Text", "Extra"], &["{{cloze:Text}}"]);
    let note = add(&h.collection, id, &["{{c1::a}} {{c2::b}} {{c3::c}}", ""]);
    let base = read(&h, id).unwrap();
    let (_, cards, graves, scm) = counts(&h, id);
    assert_eq!(cards, 3);
    let read_summary = summary(&h, id, Some(StockKind::Basic));
    assert_eq!(read_summary.kind_change_cards_removed, 2);
    assert!(read_summary.summary.loses_content);
    let request = confirmed(&h, id, Some(StockKind::Basic));
    assert_eq!(request.confirmed.kind_change_cards, 2);
    backups.then(move |c| {
        add(c, id, &["{{c1::x}} {{c2::y}}", ""]);
    });
    h.clock.step_ms(1_000);
    let fresh = restore_losses(run_restore(&h, request).unwrap_err());
    assert_eq!(fresh.kind_change_cards_removed, 3);
    assert_eq!(counts(&h, id).1, 5, "nothing written");
    let saved = run_restore(&h, confirmed(&h, id, Some(StockKind::Basic))).unwrap();
    assert_eq!(saved.reasons, [SchemaReason::NotetypeRestored]);
    assert_eq!((saved.cards_removed, saved.notes_without_cards), (3, 0));
    let after = counts(&h, id);
    assert_eq!((after.1, after.2), (2, graves + 3));
    assert!(after.3 > scm);
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert_eq!(r.cards_of_note(note).unwrap()[0].ord, 0);
    drop(r);
    let nt = read(&h, id).unwrap();
    assert_eq!(nt.config.kind, 0);
    assert_eq!(nt.config.original_stock_kind, StockKind::Basic.stored());
    assert_ne!(nt.fields[0].config.id, base.fields[0].config.id);
    assert_ne!(nt.templates[0].config.id, base.templates[0].config.id);
    assert_eq!(nt.name, "Gaps");
    h.collection.undo(UndoScope::Global).unwrap();
    let undone = counts(&h, id);
    assert_eq!((undone.1, undone.2, undone.3), (5, graves, scm));
}

/// D6 (round 9 review): a confirmed restore never replans silently: a
/// change of the note type after the question (here a field reorder that
/// keeps every count) is `LossesChanged` at planning, and the same change
/// during the backup is `LossesChanged` from the write transaction.
#[test]
fn a_confirmed_restore_after_a_digest_change_is_losses_changed() {
    let (h, backups) = harness();
    let id = made(&h, "Three", false, &["A", "B", "C"], &["{{A}}"]);
    // Both fields past the stock count hold content, so a reorder keeps
    // every count while the note type changed.
    add(&h.collection, id, &["a", "b", "c"]);
    let swap = move |c: &Collection| {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut draft = meguri_engine::notetypes::structure::StructureDraft::of(&reader.notetype(id).unwrap().unwrap());
        drop(reader);
        draft.skip_checks = true;
        draft.fields.swap(1, 2);
        meguri_engine::notetypes::structure::apply(c, draft, CancelToken::new(), ProgressSink::new()).unwrap();
    };
    let request = confirmed(&h, id, None);
    assert_eq!(request.confirmed.fields.get(&2), Some(&1));
    swap(&h.collection);
    let calls = backups.calls();
    let fresh = restore_losses(run_restore(&h, request).unwrap_err());
    assert_eq!(backups.calls(), calls, "refused before the backup");
    assert!(fresh.summary.changes.iter().any(|c| matches!(c, DraftChange::FieldRemoved { ord: 2, notes_with_content: 1, .. })));
    let request = confirmed(&h, id, None);
    backups.then(swap);
    // Past T6.1's coalescing window, so the restore takes its own backup
    // and the hook runs between its plan and its write phase.
    h.clock.step_ms(11 * 60_000);
    let calls = backups.calls();
    let fresh = restore_losses(run_restore(&h, request).unwrap_err());
    assert!(backups.calls() > calls, "the restore's backup ran and stays");
    assert_eq!(fresh.digest, notetypes::AggregateDigest::of(&read(&h, id).unwrap()), "read in the transaction");
    assert_eq!(read(&h, id).unwrap().fields.len(), 3, "nothing written");
}

/// D6: a restore with no confirmed losses replans once, silently, when the
/// note type changes during its backup; the replan takes no second backup.
#[test]
fn an_unconfirmed_restore_replans_once() {
    let (h, backups) = harness();
    let id = made(&h, "Three", false, &["A", "B", "C"], &["{{A}}"]);
    add(&h.collection, id, &["a", "", ""]);
    backups.then(move |c| {
        let reader = c.reader(ReaderRole::Query).unwrap();
        let mut styled = notetypes::Draft::of(reader.notetype(id).unwrap().unwrap());
        drop(reader);
        styled.config.css = "changed".into();
        notetypes::save(c, styled, notetypes::SaveOptions::default()).unwrap();
    });
    let saved = run_restore(&h, restoring(id, None)).unwrap();
    assert_eq!(saved.reasons, [SchemaReason::NotetypeRestored]);
    assert_eq!(backups.calls(), 1, "the replan takes no second backup");
    assert_eq!(read(&h, id).unwrap().fields.len(), 2);
}

/// D2, D7.2: NeedsStockKind for a cloze note type made from scratch and for
/// a standard one naming Cloze; NotFound for a missing id with no
/// confirmation, Stale with one; a failed backup writes nothing.
#[test]
fn restore_refusals() {
    let (h, backups) = harness();
    let cloze = made(&h, "Gaps", true, &["Text"], &["{{cloze:Text}}"]);
    assert_eq!(
        run_restore(&h, restoring(cloze, None)).unwrap_err(),
        EngineError::NotetypeStructure(StructureRefusal::NeedsStockKind)
    );
    let standard = made(&h, "Plain", false, &["Front"], &["{{Front}}"]);
    h.collection
        .run(
            meguri_engine::OpSpec::named("mark", OpSource::UserNonUndoable)
                .skip_undo()
                .stamp(meguri_storage::StampMode::Raw),
            move |ctx| {
                let mut nt = ctx.scope().notetype(standard)?.unwrap();
                nt.config.original_stock_kind = StockKind::Cloze.stored();
                let mut report = meguri_storage::journal::StructureReport::default();
                report
                    .layouts
                    .insert(nt.id.0, meguri_storage::journal::LayoutReport::identity(&nt)?);
                let token = ctx.scope().start_structure_report(report)?;
                ctx.scope().put_notetype(&nt)?;
                ctx.scope().complete_structure_report(token)?;
                Ok(())
            },
        )
        .unwrap();
    assert_eq!(
        run_restore(&h, restoring(standard, None)).unwrap_err(),
        EngineError::NotetypeStructure(StructureRefusal::NeedsStockKind)
    );
    assert!(matches!(
        run_restore(&h, restoring(NotetypeId(42), None)).unwrap_err(),
        EngineError::NotFound { .. }
    ));
    let mut gone = restoring(NotetypeId(42), None);
    gone.digest = Some(meguri_engine::notetypes::AggregateDigest([1; 32]));
    assert!(matches!(run_restore(&h, gone).unwrap_err(), EngineError::Stale { .. }));
    *backups.fail.lock().unwrap() = true;
    let before = read(&h, cloze).unwrap();
    assert!(matches!(
        run_restore(&h, restoring(cloze, Some(StockKind::Cloze))).unwrap_err(),
        EngineError::SafetyBackupFailed(_)
    ));
    assert_eq!(read(&h, cloze).unwrap().config.to_bytes(), before.config.to_bytes());
}

/// D5: a restore that keeps both counts and the kind is not a schema change
/// and takes no backup.
#[test]
fn a_restore_that_keeps_the_counts_takes_no_backup() {
    let (h, backups) = harness();
    let id = made(&h, "Plain", false, &["Front", "Back"], &["{{#Back}}{{Front}}{{/Back}}"]);
    add(&h.collection, id, &["a", ""]);
    let scm = counts(&h, id).3;
    let saved = run_restore(&h, restoring(id, None)).unwrap();
    assert!(saved.reasons.is_empty());
    assert_eq!(backups.calls(), 0);
    assert_eq!(counts(&h, id).3, scm);
}
