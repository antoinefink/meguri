//! Change Note Type (`specs/notetypes/change-notetype.md` M1 to M12) beyond
//! what the scenarios show: the confirmed losses counted again on the
//! planning snapshot and inside the write transaction (M8 as changed
//! 2026-11-20, `notetypes/draft-save` D8), the order of the refusals, the
//! safety backup (`package/backup-triggers` T6.1, T6.2), M9.1's cards
//! without a template, G6 placement, the stamps, undo and redo, progress
//! and the structure report (M10).
//!
//! As in `notetype_structure.rs`, the backup provider's hook runs after
//! planning and before the write transaction: a change made there is one
//! made between the user's confirmation and the commit.

mod common;

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use common::{Harness, NOW_MS};
use meguri_engine::notetypes::change::{
    self, ChangeInfo, ChangeLosses, ChangeNotetype, ChangePreview, NotetypeChanged,
};
use meguri_engine::notetypes::structure::{Losses, StructureRefusal};
use meguri_engine::notetypes::{self, NewNotetype, NewTemplate, NotetypeKind};
use meguri_engine::ports::{BackupFailureCause, BackupProtection, BackupPublication, BackupReceipt, BackupService};
use meguri_engine::ports::{ChangeObserver, GuardedBackup, SafetyBackupFailure};
use meguri_engine::{
    CancelToken, Collection, CommitPayload, Engine, EngineConfig, EngineError, OpSpec, Ports, ProgressSink,
    UndoScope, notes,
};
use meguri_model::changes::OpSource;
use meguri_model::clock::{Clock, StepClock, UtcOffset};
use meguri_model::ids::{CardId, DeckId, NoteId, NotetypeId};
use meguri_model::rng::SeededRandom;
use meguri_model::time::EpochMillis;
use meguri_storage::journal::StructureReason;
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
        panic!("Change Note Type uses the safety seam")
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

#[derive(Default)]
struct Capture(Mutex<Vec<CommitPayload>>);

impl ChangeObserver for Capture {
    fn name(&self) -> &str {
        "change_notetype_capture"
    }

    fn observe(&self, commit: &meguri_engine::ObservedCommit<'_>) -> Result<(), EngineError> {
        self.0.lock().unwrap().push(commit.payload.clone());
        Ok(())
    }
}

fn harness() -> (Harness, Arc<Backups>) {
    let (h, backups, _) = harness_observed();
    (h, backups)
}

fn harness_observed() -> (Harness, Arc<Backups>, Arc<Capture>) {
    let backups = Arc::new(Backups::default());
    let capture = Arc::new(Capture::default());
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("collection.anki2");
    let clock = Arc::new(StepClock::new(NOW_MS, UtcOffset::UTC, "UTC"));
    meguri_engine::bootstrap::create(&path, &*clock, &SeededRandom::new(7)).unwrap();
    let engine = Engine::new(EngineConfig::default()).unwrap();
    let config = meguri_engine::OpenConfig::new(clock.clone(), Arc::new(SeededRandom::new(7)))
        .ports(Ports::new().backup_service(backups.clone()).observer(capture.clone()));
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
        capture,
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

/// Pair: Front, Back; one template `{{Front}}`.
fn pair(h: &Harness) -> NotetypeId {
    made(h, "Pair", false, &["Front", "Back"], &["{{Front}}"])
}

/// Duo: Front, Back; templates `{{Front}}` and `{{Back}}`.
fn duo(h: &Harness) -> NotetypeId {
    made(h, "Duo", false, &["Front", "Back"], &["{{Front}}", "{{Back}}"])
}

fn cloze(h: &Harness) -> NotetypeId {
    made(h, "Gaps", true, &["Text", "Extra"], &["{{cloze:Text}}"])
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

fn edit(c: &Collection, id: NoteId, fields: &[&str]) {
    notes::update::update(
        c,
        notes::update::UpdateNote {
            id,
            fields: fields.iter().map(|s| (*s).to_owned()).collect(),
            tags: Vec::new(),
        },
        OpSource::UserUndoable,
    )
    .unwrap();
}

/// A raw card of `ord` for `note`, as a damaged or imported collection may
/// hold one (no template generates it).
fn raw_card(c: &Collection, note: NoteId, ord: u16) -> CardId {
    c.run(OpSpec::named("raw_card", OpSource::UserUndoable), move |ctx| {
        Ok(ctx.scope().add_card(&common::card(note, ord))?)
    })
    .unwrap()
    .value
}

fn info(h: &Harness, notes: &[NoteId], new: NotetypeId) -> Result<ChangeInfo, EngineError> {
    change::info(&h.collection, notes, new, &CancelToken::new(), &ProgressSink::new())
}

/// The change the screen saves after opening on `notes` with default maps,
/// confirming what the preview showed.
fn request(info: &ChangeInfo, notes: &[NoteId]) -> ChangeNotetype {
    ChangeNotetype {
        notes: notes.to_vec(),
        old_notetype: info.old.id,
        new_notetype: info.new.id,
        fields: info.fields.clone(),
        templates: info.templates.clone(),
        schema: info.schema,
        layout: info.layout,
        confirmed: info.preview.losses.clone(),
    }
}

/// `request` with other maps, confirming their preview.
fn mapped(
    info: &ChangeInfo,
    notes: &[NoteId],
    fields: Vec<Option<u32>>,
    templates: Option<Vec<Option<u32>>>,
) -> ChangeNotetype {
    let confirmed = info.preview(&fields, templates.as_deref()).losses;
    ChangeNotetype {
        fields,
        templates,
        confirmed,
        ..request(info, notes)
    }
}

fn run(h: &Harness, request: ChangeNotetype) -> Result<NotetypeChanged, EngineError> {
    change::change(&h.collection, request, CancelToken::new(), ProgressSink::new()).map(|r| r.value)
}

fn refusal(error: &EngineError) -> Option<&StructureRefusal> {
    match error {
        EngineError::NotetypeStructure(refusal) => Some(refusal),
        _ => None,
    }
}

fn losses_changed(error: &EngineError) -> ChangePreview {
    match refusal(error) {
        Some(StructureRefusal::LossesChanged(losses)) => match &**losses {
            Losses::Change(preview) => preview.clone(),
            other => panic!("not a Change Note Type preview: {other:?}"),
        },
        _ => panic!("not LossesChanged: {error:?}"),
    }
}

fn undo_name(h: &Harness) -> Option<String> {
    h.collection.peek_undo(UndoScope::Global).unwrap().name().map(str::to_owned)
}

/// Every row the change may write, as stored: notes, cards, graves, the
/// `col` row and `nextPos`.
#[derive(Debug, PartialEq)]
struct Rows {
    notes: Vec<Vec<meguri_storage::Cell>>,
    cards: Vec<Vec<meguri_storage::Cell>>,
    graves: u64,
    col: meguri_model::col::Col,
    next_pos: Option<ConfigValue>,
}

fn rows(h: &Harness) -> Rows {
    rows_of(&h.collection)
}

fn rows_of(c: &Collection) -> Rows {
    let r = c.reader(ReaderRole::Query).unwrap();
    let images = |table: Table| {
        let mut out = Vec::new();
        let mut after = None;
        loop {
            let page = r.row_page(table, after.as_ref()).unwrap();
            let Some(last) = page.last() else { break };
            after = Some(meguri_storage::RowKey::Id(last[0].as_i64().unwrap()));
            out.extend(page);
        }
        out
    };
    Rows {
        notes: images(Table::Notes),
        cards: images(Table::Cards),
        graves: r.row_count(Table::Graves).unwrap(),
        col: r.col().unwrap(),
        next_pos: r.config_value("nextPos").unwrap(),
    }
}

/// (ord, card id, deck) of a note's cards, by ordinal then id.
fn cards_of(h: &Harness, note: NoteId) -> Vec<(u16, CardId, DeckId)> {
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    let mut cards: Vec<_> = r
        .cards_of_note(note)
        .unwrap()
        .into_iter()
        .map(|c| (c.ord, c.id, c.deck_id))
        .collect();
    cards.sort();
    cards
}

fn note(h: &Harness, id: NoteId) -> meguri_model::note::Note {
    h.collection.reader(ReaderRole::Query).unwrap().note(id).unwrap().unwrap()
}

fn scm(h: &Harness) -> i64 {
    h.collection.reader(ReaderRole::Query).unwrap().col().unwrap().schema_modified.0
}

fn bump_scm(c: &Collection) {
    c.run(OpSpec::named("other_schema_change", OpSource::UserNonUndoable), |ctx| {
        let mut col = ctx.scope().col()?;
        col.schema_modified = EpochMillis(col.schema_modified.0 + 1_000);
        ctx.scope().put_col(&col)?;
        Ok(())
    })
    .unwrap();
}

// --- M5: the info query -------------------------------------------------

/// M2, M3, M5: default maps, M5's content count (any stored text, `<br>`
/// and a space included), card counts per ordinal, M6 for the defaults.
#[test]
fn the_info_query_counts_any_stored_text_and_every_card_ordinal() {
    let (h, _) = harness();
    let duo = duo(&h);
    let target = made(&h, "Target", false, &["Back", "Notes", "Front"], &["{{Front}}"]);
    let a = add(&h.collection, duo, &["a", "<br>"]);
    let b = add(&h.collection, duo, &["b", " "]);
    let c = add(&h.collection, duo, &["c", ""]);
    let d = add(&h.collection, duo, &["d", "back"]);
    raw_card(&h.collection, c, 4);
    let read = info(&h, &[a, b, c, d, a], target).unwrap();
    assert_eq!(read.old.name, "Duo");
    assert_eq!(read.new.fields, ["Back", "Notes", "Front"]);
    assert_eq!(read.fields, [Some(1), None, Some(0)]);
    assert_eq!(read.templates, Some(vec![Some(0)]), "Card 1 by name");
    assert_eq!(read.field_content, [4, 3], "`<br>` and a space count; the duplicate counts once");
    // Card 2 needs a non-empty Back under E1: only d has one (`<br>` and a
    // space are empty there, not under M5).
    let mut counts = std::collections::BTreeMap::new();
    counts.insert(0u16, 4u64);
    counts.insert(1, 1);
    counts.insert(4, 1);
    assert_eq!(read.card_counts, counts);
    assert_eq!(read.preview.discarded_fields, Vec::<u32>::new());
    assert_eq!(read.preview.dropped_templates, Some(vec![1]));
    // Card 2's card goes; the ord 4 card is past the old templates and no
    // entry sits at position 4: kept.
    assert_eq!(read.preview.removed_cards, 1);
    assert_eq!(read.preview.losses.templates.get(&1), Some(&1));
    assert_eq!(read.preview.losses.other_cards, 0);
    assert_eq!(read.schema, scm(&h));
}

/// M4, M5 refusals, in order.
#[test]
fn the_info_query_refuses_in_m4_order() {
    let (h, _) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let other = add(&h.collection, duo, &["x", "y"]);
    let gone = add(&h.collection, pair, &["g", "h"]);
    notes::delete(&h.collection, vec![gone], OpSource::UserUndoable).unwrap();
    assert!(matches!(info(&h, &[], duo), Err(EngineError::InvalidArgument { .. })));
    assert!(matches!(info(&h, &[gone, a], duo), Err(EngineError::NotFound { table: Table::Notes, .. })));
    // A missing note other than the first is `multiple_notetypes` here.
    let e = info(&h, &[a, gone], duo).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::MultipleNotetypes));
    let e = info(&h, &[a, other], duo).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::MultipleNotetypes));
    // M4's refusals come before the missing new note type.
    let e = info(&h, &[a, other], NotetypeId(424_242)).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::MultipleNotetypes));
    assert!(matches!(
        info(&h, &[a], NotetypeId(424_242)),
        Err(EngineError::NotFound { table: Table::Notetypes, .. })
    ));
}

/// M6 for an edited mapping, from the one info result.
#[test]
fn the_preview_of_any_mapping_comes_from_one_info_result() {
    let (h, _) = harness();
    let duo = duo(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let b = add(&h.collection, duo, &["c", ""]);
    let read = info(&h, &[a, b], duo).unwrap();
    let p = read.preview(&[Some(0), None], Some(&[None, Some(0)]));
    assert_eq!(p.discarded_fields, [1]);
    assert_eq!(p.dropped_templates, Some(vec![1]));
    assert_eq!(p.losses.fields.get(&1), Some(&1));
    assert_eq!(p.losses.templates.get(&1), Some(&1));
    assert_eq!(p.removed_cards, 1);
}

// --- M8: refusals, backup, order ----------------------------------------

/// M8 check 1: shape errors are invalid arguments; a missing note type is
/// `not_found`. None takes a backup or writes.
#[test]
fn shape_errors_are_refused_before_anything() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let gaps = cloze(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    let before = rows(&h);
    let invalid = |r: ChangeNotetype| matches!(run(&h, r), Err(EngineError::InvalidArgument { .. }));
    assert!(invalid(ChangeNotetype {
        notes: Vec::new(),
        ..request(&read, &[a])
    }));
    assert!(invalid(mapped(&read, &[a], vec![Some(0)], read.templates.clone())), "wrong length");
    assert!(invalid(mapped(&read, &[a], vec![Some(0), Some(2)], read.templates.clone())), "no old field 2");
    assert!(invalid(mapped(&read, &[a], read.fields.clone(), Some(vec![Some(0), Some(0)]))), "twice");
    assert!(invalid(mapped(&read, &[a], read.fields.clone(), Some(vec![Some(1), None]))), "no old template 1");
    assert!(invalid(mapped(&read, &[a], read.fields.clone(), None)), "two standard types need a map");
    let to_cloze = info(&h, &[a], gaps).unwrap();
    assert!(invalid(mapped(&to_cloze, &[a], to_cloze.fields.clone(), Some(vec![Some(0)]))));
    let missing = ChangeNotetype {
        new_notetype: NotetypeId(424_242),
        ..request(&read, &[a])
    };
    assert!(matches!(run(&h, missing), Err(EngineError::NotFound { table: Table::Notetypes, .. })));
    let missing = ChangeNotetype {
        old_notetype: NotetypeId(424_242),
        ..request(&read, &[a])
    };
    assert!(matches!(run(&h, missing), Err(EngineError::NotFound { table: Table::Notetypes, .. })));
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
}

/// M7: a cloze note type to itself with the identity field map is
/// `no_changes`, before the stamp is compared; no backup.
#[test]
fn a_cloze_note_type_to_itself_changes_nothing() {
    let (h, backups) = harness();
    let gaps = cloze(&h);
    let a = add(&h.collection, gaps, &["{{c1::x}}", ""]);
    let read = info(&h, &[a], gaps).unwrap();
    assert_eq!(read.templates, None);
    let before = rows(&h);
    let stale = ChangeNotetype {
        schema: read.schema - 1,
        ..request(&read, &[a])
    };
    assert_eq!(refusal(&run(&h, stale).unwrap_err()), Some(&StructureRefusal::NoChanges));
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
    // A standard note type to itself with identity maps too.
    let duo = duo(&h);
    let b = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[b], duo).unwrap();
    assert_eq!(refusal(&run(&h, request(&read, &[b])).unwrap_err()), Some(&StructureRefusal::NoChanges));
    assert_eq!(backups.calls(), 0);
}

/// M8 checks 3 and 4, first run: a stale stamp or a deleted note found on
/// the planning snapshot takes no backup.
#[test]
fn a_stale_screen_found_at_planning_takes_no_backup() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let b = add(&h.collection, pair, &["c", "d"]);
    let read = info(&h, &[a, b], duo).unwrap();
    bump_scm(&h.collection);
    let before = rows(&h);
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
    let read = info(&h, &[a, b], duo).unwrap();
    notes::delete(&h.collection, vec![b], OpSource::UserUndoable).unwrap();
    let before = rows(&h);
    // A missing note other than the first is `not_found` at the change.
    assert!(matches!(
        run(&h, request(&read, &[a, b])),
        Err(EngineError::NotFound { table: Table::Notes, .. })
    ));
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
}

/// M8 check 4: a listed note of another note type is `multiple_notetypes`;
/// a missing one is `not_found` even when listed after it.
#[test]
fn the_change_checks_existence_before_note_types() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let other = add(&h.collection, duo, &["x", "y"]);
    let gone = add(&h.collection, pair, &["g", "h"]);
    let read = info(&h, &[a, gone], duo).unwrap();
    notes::delete(&h.collection, vec![gone], OpSource::UserUndoable).unwrap();
    let read = ChangeNotetype {
        schema: scm(&h),
        ..request(&read, &[a])
    };
    let e = run(&h, ChangeNotetype { notes: vec![a, other], ..read.clone() }).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::MultipleNotetypes));
    assert!(matches!(
        run(&h, ChangeNotetype { notes: vec![a, other, gone], ..read }),
        Err(EngineError::NotFound { table: Table::Notes, .. })
    ));
    assert_eq!(backups.calls(), 0);
}

/// M8 check 5: a failed backup refuses the change and writes nothing.
#[test]
fn a_failed_safety_backup_writes_nothing() {
    let (h, backups) = harness();
    let duo = duo(&h);
    let pair = pair(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[a], pair).unwrap();
    let before = rows(&h);
    let undo = undo_name(&h);
    *backups.fail.lock().unwrap() = true;
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert!(matches!(e, EngineError::SafetyBackupFailed(_)), "{e:?}");
    assert_eq!(backups.calls(), 1);
    assert_eq!(rows(&h), before);
    assert_eq!(undo_name(&h), undo);
}

/// M8 check 3, second run: a schema change committed while the backup ran
/// refuses inside the transaction; the backup stays, nothing is written.
#[test]
fn a_schema_change_during_the_backup_is_schema_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    backups.then(bump_scm);
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(backups.calls(), 1);
    assert_eq!(note(&h, a).notetype_id, pair);
}

/// M8 check 4, second run: a note deleted while the backup ran.
#[test]
fn a_note_deleted_during_the_backup_is_not_found() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let b = add(&h.collection, pair, &["c", "d"]);
    let read = info(&h, &[a, b], duo).unwrap();
    backups.then(move |c| {
        notes::delete(c, vec![b], OpSource::UserUndoable).unwrap();
    });
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    assert!(matches!(e, EngineError::NotFound { table: Table::Notes, .. }), "{e:?}");
    assert_eq!(note(&h, a).notetype_id, pair, "the other note keeps its note type");
    assert_eq!(undo_name(&h).as_deref(), Some("Delete Note"));
}

// --- M8: the confirmed losses ---------------------------------------------

/// A discarded field gains content in another selected note between the
/// confirmation and the commit, a `<br>`-only value included (M5's count):
/// `LossesChanged` with the new preview, nothing written, no step, the
/// backup stays. Confirmed again, the change commits.
#[test]
fn a_discarded_field_that_gains_content_is_losses_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", "back"]);
    let b = add(&h.collection, pair, &["b", ""]);
    let read = info(&h, &[a, b], single).unwrap();
    assert_eq!(read.preview.losses.fields.get(&1), Some(&1));
    backups.then(move |c| edit(c, b, &["b", "<br>"]));
    let before_undo = "Update Note";
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    let live = losses_changed(&e);
    assert_eq!(live.discarded_fields, [1]);
    assert_eq!(live.losses.fields.get(&1), Some(&2), "`<br>` counts");
    assert_eq!(backups.calls(), 1);
    assert_eq!(note(&h, a).notetype_id, pair);
    assert_eq!(note(&h, b).fields, "b\u{1f}<br>");
    assert_eq!(undo_name(&h).as_deref(), Some(before_undo));
    // Confirmed again with the counts the sheet now shows.
    let again = ChangeNotetype {
        confirmed: live.losses,
        ..request(&read, &[a, b])
    };
    assert_eq!(run(&h, again).unwrap().notes, 2);
    assert_eq!(note(&h, b).notetype_id, single);
    assert_eq!(undo_name(&h).as_deref(), Some("Change Note Type"));
}

/// A field confirmed at 0 (empty in every note) that gains content.
#[test]
fn a_field_confirmed_empty_that_gains_content_is_losses_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", ""]);
    let read = info(&h, &[a], single).unwrap();
    assert_eq!(read.preview.losses.fields.get(&1), Some(&0));
    backups.then(move |c| edit(c, a, &["a", " "]));
    let live = losses_changed(&run(&h, request(&read, &[a])).unwrap_err());
    assert_eq!(live.losses.fields.get(&1), Some(&1));
    assert_eq!(note(&h, a).notetype_id, pair);
}

/// A dropped template gains a card.
#[test]
fn a_dropped_template_that_gains_a_card_is_losses_changed() {
    let (h, backups) = harness();
    let duo = duo(&h);
    let pair = pair(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let b = add(&h.collection, duo, &["c", ""]);
    let read = info(&h, &[a, b], pair).unwrap();
    assert_eq!(read.preview.dropped_templates, Some(vec![1]));
    assert_eq!(read.preview.losses.templates.get(&1), Some(&1));
    // b's Back filled: Card 2 is generated for it.
    backups.then(move |c| edit(c, b, &["c", "d"]));
    let before = cards_of(&h, b).len();
    let live = losses_changed(&run(&h, request(&read, &[a, b])).unwrap_err());
    assert_eq!(live.losses.templates.get(&1), Some(&2));
    assert_eq!(live.removed_cards, 2);
    assert_eq!(cards_of(&h, b).len(), before + 1, "the new card is kept, nothing changed");
    assert_eq!(note(&h, a).notetype_id, duo);
}

/// M9.1: a card without a template appears at a renumbering target.
#[test]
fn a_card_without_a_template_at_a_renumbering_target_is_losses_changed() {
    let (h, backups) = harness();
    let duo = duo(&h);
    let trio = made(&h, "Trio", false, &["Front", "Back"], &["{{Front}}", "{{Back}}", "{{Back}}{{Front}}"]);
    let a = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[a], trio).unwrap();
    // Duo's Card 2 goes to Trio's position 2.
    let change = mapped(&read, &[a], vec![Some(0), Some(1)], Some(vec![Some(0), None, Some(1)]));
    assert_eq!(change.confirmed.other_cards, 0);
    backups.then(move |c| {
        raw_card(c, a, 2);
    });
    let live = losses_changed(&run(&h, change.clone()).unwrap_err());
    assert_eq!(live.losses.other_cards, 1);
    assert_eq!(live.removed_cards, 1);
    assert_eq!(note(&h, a).notetype_id, duo);
    // Confirmed again: the card without a template goes with its grave, the
    // renumbered card takes ordinal 2 and keeps its id.
    let card2 = cards_of(&h, a).iter().find(|c| c.0 == 1).unwrap().1;
    let orphan = cards_of(&h, a).iter().find(|c| c.0 == 2).unwrap().1;
    let graves = rows(&h).graves;
    let change = ChangeNotetype {
        confirmed: live.losses,
        ..change
    };
    run(&h, change).unwrap();
    let after = cards_of(&h, a);
    assert!(after.iter().any(|c| c.0 == 2 && c.1 == card2));
    assert!(!after.iter().any(|c| c.1 == orphan));
    assert_eq!(after.iter().filter(|c| c.0 == 2).count(), 1);
    assert_eq!(rows(&h).graves, graves + 1);
    // One Query reader at a time: the pool may hold a single connection.
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.grave(orphan.0, meguri_model::grave::GraveKind::Card).unwrap().is_some());
}

/// M9.2: cloze to standard cards appear (a cloze number added).
#[test]
fn cloze_cards_past_the_new_templates_appearing_is_losses_changed() {
    let (h, backups) = harness();
    let gaps = cloze(&h);
    let duo = duo(&h);
    let a = add(&h.collection, gaps, &["{{c1::x}} {{c2::y}}", ""]);
    let read = info(&h, &[a], duo).unwrap();
    assert_eq!(read.templates, None);
    assert_eq!(read.preview.losses.other_cards, 0);
    backups.then(move |c| edit(c, a, &["{{c1::x}} {{c2::y}} {{c3::z}}", ""]));
    let live = losses_changed(&run(&h, request(&read, &[a])).unwrap_err());
    assert_eq!(live.losses.other_cards, 1);
    assert_eq!(live.removed_cards, 1);
    assert_eq!(note(&h, a).notetype_id, gaps);
    assert_eq!(cards_of(&h, a).len(), 3);
}

/// `schema_changed` wins over `LossesChanged` (M8, round 9 review).
#[test]
fn schema_changed_wins_over_losses_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", ""]);
    let read = info(&h, &[a], single).unwrap();
    backups.then(move |c| {
        edit(c, a, &["a", "lost"]);
        bump_scm(c);
    });
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    // And on the planning snapshot too: no backup.
    let read = info(&h, &[a], single).unwrap();
    edit(&h.collection, a, &["a", "more"]);
    let stale = ChangeNotetype {
        confirmed: ChangeLosses::default(),
        ..request(&read, &[a])
    };
    bump_scm(&h.collection);
    let calls = backups.calls();
    assert_eq!(refusal(&run(&h, stale).unwrap_err()), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(backups.calls(), calls);
}

/// A loss already larger on the planning snapshot is refused before the
/// backup.
#[test]
fn losses_larger_at_planning_take_no_backup() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", ""]);
    let read = info(&h, &[a], single).unwrap();
    edit(&h.collection, a, &["a", "x"]);
    let live = losses_changed(&run(&h, request(&read, &[a])).unwrap_err());
    assert_eq!(live.losses.fields.get(&1), Some(&1));
    assert_eq!(backups.calls(), 0);
}

/// A count that shrinks, or other notes holding the content now, still
/// commits: the confirmation is by count, per category.
#[test]
fn a_count_that_shrinks_still_commits() {
    let (h, backups) = harness();
    let duo = duo(&h);
    let pair = pair(&h);
    let a = add(&h.collection, duo, &["a", "x"]);
    let b = add(&h.collection, duo, &["b", ""]);
    let read = info(&h, &[a, b], pair).unwrap();
    let change = mapped(&read, &[a, b], vec![Some(0), None], Some(vec![Some(0)]));
    assert_eq!(change.confirmed.fields.get(&1), Some(&1));
    assert_eq!(change.confirmed.templates.get(&1), Some(&1));
    // b takes the content and a gives it up: the field's count stays 1.
    // A raw write, so no card is generated for b and Card 2's count stays.
    backups.then(move |c| {
        c.run(OpSpec::named("swap_text", OpSource::UserUndoable), move |ctx| {
            let mut na = ctx.scope().note(a)?.unwrap();
            let mut nb = ctx.scope().note(b)?.unwrap();
            na.fields = "a\u{1f}".into();
            nb.fields = "b\u{1f}x".into();
            ctx.scope().put(&na)?;
            ctx.scope().put(&nb)?;
            Ok(())
        })
        .unwrap();
    });
    assert_eq!(run(&h, change).unwrap().notes, 2);
    assert_eq!(note(&h, a).notetype_id, pair);
    // And a count that shrinks to zero.
    let x = add(&h.collection, duo, &["x", "gone soon"]);
    let read = info(&h, &[x], pair).unwrap();
    let change = mapped(&read, &[x], vec![Some(0), None], Some(vec![Some(0)]));
    backups.then(move |c| edit(c, x, &["x", ""]));
    h.clock.step_ms(11 * 60_000);
    run(&h, change).unwrap();
    assert_eq!(note(&h, x).notetype_id, pair);
}

/// A refusal leaves the undo and redo history as it was and takes no step.
#[test]
fn a_refusal_keeps_the_undo_and_redo_history() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", ""]);
    let b = add(&h.collection, pair, &["b", ""]);
    h.clock.step_ms(61_000);
    edit(&h.collection, b, &["b", "z"]);
    h.collection.undo(UndoScope::Global).unwrap();
    let undo = undo_name(&h);
    let redo = h.collection.peek_redo(UndoScope::Global).unwrap().name().map(str::to_owned);
    assert_eq!(redo.as_deref(), Some("Update Note"));
    let read = info(&h, &[a], single).unwrap();
    let seen = Arc::new(Mutex::new(None));
    let kept = seen.clone();
    backups.then(move |c| {
        c.run(OpSpec::named("raw_edit", OpSource::Rollover).stamp(meguri_storage::StampMode::Raw), move |ctx| {
            let mut n = ctx.scope().note(a)?.unwrap();
            n.fields = "a\u{1f}late".into();
            ctx.scope().put(&n)?;
            Ok(())
        })
        .unwrap();
        *kept.lock().unwrap() = Some(rows_of(c));
    });
    losses_changed(&run(&h, request(&read, &[a])).unwrap_err());
    let before = seen.lock().unwrap().take().unwrap();
    assert_eq!(rows(&h), before, "nothing written after the late edit");
    assert_eq!(undo_name(&h), undo);
    assert_eq!(
        h.collection.peek_redo(UndoScope::Global).unwrap().name().map(str::to_owned),
        redo
    );
}

/// No partial writes: a failure injected after notes and cards were
/// written rolls everything back; no step. The same change then commits.
#[test]
fn a_failure_in_the_write_phase_rolls_everything_back() {
    let (h, backups) = harness();
    let duo = duo(&h);
    let pair = pair(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let b = add(&h.collection, duo, &["c", "d"]);
    let read = info(&h, &[a, b], pair).unwrap();
    let before = rows(&h);
    let undo = undo_name(&h);
    meguri_model::faultpoint::arm(
        "notetypes.change.write_phase",
        meguri_model::faultpoint::Fault::IoError(std::io::ErrorKind::Other),
    );
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    meguri_model::faultpoint::disarm_all();
    assert!(matches!(e, EngineError::Internal { .. }), "{e:?}");
    assert_eq!(backups.calls(), 1);
    assert_eq!(rows(&h), before);
    assert_eq!(undo_name(&h), undo);
    assert_eq!(run(&h, request(&read, &[a, b])).unwrap().notes, 2);
}

// --- M9: effects -----------------------------------------------------------

/// M9 with default maps, then one undo step restoring cards, notes, graves
/// and `scm`, and redo applying them again. No confirmation input exists;
/// the change commits at once while `scm <= ls`.
#[test]
fn the_change_is_one_step_that_undo_and_redo_restore() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    h.collection
        .run(OpSpec::named("after_sync", OpSource::UserNonUndoable), |ctx| {
            let mut col = ctx.scope().col()?;
            col.last_sync = col.schema_modified;
            ctx.scope().put_col(&col)?;
            Ok(())
        })
        .unwrap();
    let a = add(&h.collection, pair, &["a", "b"]);
    let b = add(&h.collection, pair, &["c", ""]);
    h.clock.step_ms(60_000);
    let read = info(&h, &[a, b, a], duo).unwrap();
    let before = rows(&h);
    let old_card = cards_of(&h, a)[0];
    let result = change::change(&h.collection, request(&read, &[b, a, b]), CancelToken::new(), ProgressSink::new())
        .unwrap();
    assert_eq!(result.value.notes, 2, "duplicates count once");
    assert!(result.changes.collection_row);
    assert!(result.changes.notetypes.is_empty());
    assert_eq!(backups.calls(), 1);
    let after = rows(&h);
    assert!(after.col.schema_modified.0 > before.col.schema_modified.0);
    assert!(after.col.schema_modified.0 > after.col.last_sync.0);
    assert_eq!(note(&h, a).notetype_id, duo);
    let cards = cards_of(&h, a);
    assert_eq!(cards.len(), 2);
    assert_eq!(cards[0], old_card, "card 1 untouched");
    assert_eq!(cards_of(&h, b).len(), 1, "an empty Back makes no Card 2");
    assert_eq!(undo_name(&h).as_deref(), Some("Change Note Type"));
    h.collection.undo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), before);
    h.collection.redo(UndoScope::Global).unwrap();
    assert_eq!(rows(&h), after);
}

/// M9 step 3: fields remapped, control characters other than TAB and LF
/// removed without NFC, `sfld`/`csum` from the new sort field, `mid` set,
/// the note stamped; a dropped template's card goes with its grave and a
/// regenerated card never takes its id.
#[test]
fn a_note_is_rewritten_and_dropped_cards_never_lend_their_ids() {
    let (h, _) = harness();
    let duo = duo(&h);
    let a = add(&h.collection, duo, &["front", "back"]);
    h.collection
        .run(OpSpec::named("raw_text", OpSource::UserUndoable), move |ctx| {
            let mut n = ctx.scope().note(a)?.unwrap();
            n.fields = "e\u{301}\u{1}x\u{1f}b\tc\nd\r\u{7f}".into();
            ctx.scope().put(&n)?;
            Ok(())
        })
        .unwrap();
    h.clock.step_ms(60_000);
    let read = info(&h, &[a], duo).unwrap();
    let card2 = cards_of(&h, a)[1].1;
    // Swap the fields, drop Card 2 and take it again as a new card.
    let change = mapped(&read, &[a], vec![Some(1), Some(0)], Some(vec![Some(0), None]));
    run(&h, change).unwrap();
    let n = note(&h, a);
    assert_eq!(n.fields, "b\tc\nd\u{1f}e\u{301}x");
    assert_eq!(n.sort_field, "b\tc\nd");
    assert_eq!(n.checksum, meguri_text::checksum::note_checksum("b\tc\nd"));
    assert_eq!(n.mtime.0, h.clock.now_ms() / 1000);
    assert_eq!(n.usn, meguri_model::usn::Usn::PENDING);
    let cards = cards_of(&h, a);
    assert_eq!(cards.len(), 2);
    assert!(cards.iter().all(|c| c.1 != card2), "the regenerated card has a fresh id");
    let r = h.collection.reader(ReaderRole::Query).unwrap();
    assert!(r.grave(card2.0, meguri_model::grave::GraveKind::Card).unwrap().is_some());
}

/// M9.1: a card without a template at a position whose entry is none stays
/// untouched; one at a renumbering target is deleted and counted (M6).
#[test]
fn cards_without_a_template_are_kept_unless_at_a_renumbering_target() {
    let (h, _) = harness();
    let duo = duo(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let kept = raw_card(&h.collection, a, 3);
    let read = info(&h, &[a], duo).unwrap();
    let swap = mapped(&read, &[a], vec![Some(0), Some(1)], Some(vec![Some(1), Some(0)]));
    assert_eq!(swap.confirmed.other_cards, 0);
    run(&h, swap).unwrap();
    assert!(cards_of(&h, a).iter().any(|c| c.0 == 3 && c.1 == kept));
}

/// M9 step 3: a note whose every card is dropped keeps a G6 card of ordinal
/// 0 in the new note type's last deck; `nextPos` is in the change set.
#[test]
fn a_note_left_with_no_card_gets_its_g6_card_in_the_new_last_deck() {
    let (h, _) = harness();
    let pair = pair(&h);
    let target = made(&h, "Blank", false, &["Front", "Back"], &["{{Back}}"]);
    let deck = meguri_engine::decks::add(&h.collection, "Target deck".into()).unwrap().value.id;
    h.collection
        .run(OpSpec::named("last_deck", OpSource::UserUndoable), move |ctx| {
            ctx.scope()
                .set_config(&format!("_nt_{}_lastDeck", target.0), &ConfigValue::from(deck.0))?;
            Ok(())
        })
        .unwrap();
    let a = add(&h.collection, pair, &["a", ""]);
    let read = info(&h, &[a], target).unwrap();
    let change = mapped(&read, &[a], vec![Some(0), Some(1)], Some(vec![None]));
    assert_eq!(change.confirmed.templates.get(&0), Some(&1));
    let result = change::change(&h.collection, change, CancelToken::new(), ProgressSink::new()).unwrap();
    let cards = cards_of(&h, a);
    assert_eq!(cards.len(), 1);
    assert_eq!((cards[0].0, cards[0].2), (0, deck));
    assert!(result.changes.config_keys.iter().any(|k| k.key == "nextPos"));
}

/// M9 step 3: every selected note is stamped when only the template map
/// changes.
#[test]
fn every_note_is_stamped_when_only_the_template_map_changes() {
    let (h, _) = harness();
    let duo = duo(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let before = note(&h, a);
    h.clock.step_ms(5_000);
    let read = info(&h, &[a], duo).unwrap();
    run(&h, mapped(&read, &[a], vec![Some(0), Some(1)], Some(vec![Some(1), Some(0)]))).unwrap();
    let after = note(&h, a);
    assert_eq!((after.fields.as_str(), after.notetype_id), (before.fields.as_str(), duo));
    assert!(after.mtime.0 > before.mtime.0);
}

/// M10: progress is notes done out of notes selected.
#[test]
fn the_change_reports_progress() {
    let (h, _) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let ids: Vec<NoteId> = (0..5).map(|i| add(&h.collection, pair, &[&format!("n{i}"), ""])).collect();
    let read = info(&h, &ids, duo).unwrap();
    let progress = ProgressSink::with_clock(one_second_per_read);
    let seen = Arc::new(Recorder::default());
    progress.set_listener(seen.clone());
    change::change(&h.collection, request(&read, &ids), CancelToken::new(), progress).unwrap();
    let seen = seen.0.lock().unwrap();
    assert!(seen.iter().any(|p| (p.completed, p.total) == (5, Some(5))), "{seen:?}");
}

#[derive(Default)]
struct Recorder(Mutex<Vec<meguri_engine::Progress>>);

impl meguri_engine::ProgressListener for Recorder {
    fn on_progress(&self, progress: &meguri_engine::Progress) {
        self.0.lock().unwrap().push(progress.clone());
    }
}

/// A clock a second further on at every read, so no report is throttled.
fn one_second_per_read() -> std::time::Duration {
    static TICKS: AtomicUsize = AtomicUsize::new(0);
    std::time::Duration::from_secs(TICKS.fetch_add(1, Ordering::SeqCst) as u64)
}

/// T6.1's coalescing: a second change within 10 minutes takes no new
/// backup; one after 10 minutes takes one.
#[test]
fn backups_coalesce_within_ten_minutes() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    run(&h, request(&read, &[a])).unwrap();
    assert_eq!(backups.calls(), 1);
    h.clock.step_ms(60_000);
    let read = info(&h, &[a], pair).unwrap();
    run(&h, request(&read, &[a])).unwrap();
    assert_eq!(backups.calls(), 1);
    h.clock.step_ms(11 * 60_000);
    let read = info(&h, &[a], duo).unwrap();
    run(&h, request(&read, &[a])).unwrap();
    assert_eq!(backups.calls(), 2);
}

// --- M10: the structure report ---------------------------------------------

fn structure(capture: &Capture) -> meguri_storage::journal::StructureReport {
    capture.0.lock().unwrap().last().unwrap().structure_report().unwrap().clone()
}

/// Reason 6 and the `type_changes` entry: between two standard note types
/// (M1's maps), from a cloze note type (an empty template map), to a cloze
/// one (the identity of the old template count).
#[test]
fn the_change_reports_its_type_change() {
    let (h, _, capture) = harness_observed();
    let duo = duo(&h);
    let pair = pair(&h);
    let gaps = cloze(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[a], pair).unwrap();
    run(&h, mapped(&read, &[a], vec![Some(1), None], Some(vec![Some(1)]))).unwrap();
    let report = structure(&capture);
    assert_eq!(report.reasons, [StructureReason::NotesNotetypeChanged].into());
    let pairing = &report.type_changes[&(duo.0, pair.0)];
    assert_eq!((pairing.field_map.as_slice(), pairing.template_map.as_slice()), (&[1, -1][..], &[1][..]));
    assert!(report.layouts.is_empty() && report.removed.is_empty());
    h.clock.step_ms(1_000);
    let read = info(&h, &[a], gaps).unwrap();
    run(&h, request(&read, &[a])).unwrap();
    let to_cloze = &structure(&capture).type_changes[&(pair.0, gaps.0)];
    assert_eq!(to_cloze.template_map, [0]);
    h.clock.step_ms(1_000);
    let read = info(&h, &[a], duo).unwrap();
    run(&h, request(&read, &[a])).unwrap();
    let from_cloze = &structure(&capture).type_changes[&(gaps.0, duo.0)];
    assert!(from_cloze.template_map.is_empty());
}

/// A change to the same note type reports reason 6. M10 asks for an entry
/// with old id = new id too; the storage report holds entries only for
/// pairs of different note types, so none is reported (see the PR).
#[test]
fn a_change_to_the_same_note_type_reports_reason_six() {
    let (h, _, capture) = harness_observed();
    let duo = duo(&h);
    let a = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    run(&h, mapped(&read, &[a], vec![Some(1), Some(0)], Some(vec![Some(0), Some(1)]))).unwrap();
    let report = structure(&capture);
    assert_eq!(report.reasons, [StructureReason::NotesNotetypeChanged].into());
    assert!(report.type_changes.is_empty());
}

/// M1 allows a field map that names one old field twice (R945). Between two
/// note types that is not built yet (the structure report refuses a
/// repeated `type_changes` origin): the change refuses it at planning as
/// `Unsupported`, before any backup, and writes nothing. To the same note
/// type it commits, since no `type_changes` entry is reported.
#[test]
fn a_repeated_field_map_between_two_note_types_is_refused_before_the_backup() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    let before = rows(&h);
    let undo = undo_name(&h);
    let e = run(&h, mapped(&read, &[a], vec![Some(0), Some(0)], Some(vec![Some(0), None]))).unwrap_err();
    assert!(matches!(e, EngineError::Unsupported { .. }), "{e:?}");
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
    assert_eq!(undo_name(&h), undo);
    let read = info(&h, &[a], pair).unwrap();
    run(&h, mapped(&read, &[a], vec![Some(0), Some(0)], Some(vec![Some(0)]))).unwrap();
    assert_eq!(note(&h, a).fields, "a\u{1f}a");
}

/// Stores `fields` raw as `note`'s values (a damaged note).
fn raw_fields(c: &Collection, note: NoteId, fields: &'static str) {
    c.run(OpSpec::named("raw_fields", OpSource::UserUndoable), move |ctx| {
        let mut n = ctx.scope().note(note)?.unwrap();
        n.fields = fields.into();
        ctx.scope().put(&n)?;
        Ok(())
    })
    .unwrap();
}

/// A selected note that stores more values than its note type has fields
/// would lose the extra values uncounted: refused at planning (no backup)
/// and, for a note damaged while the backup ran, inside the transaction;
/// nothing written either time.
#[test]
fn a_note_with_more_values_than_fields_is_refused_at_planning_and_in_the_transaction() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let b = add(&h.collection, pair, &["c", "d"]);
    raw_fields(&h.collection, b, "c\u{1f}d\u{1f}extra");
    let read = info(&h, &[a, b], duo).unwrap();
    let before = rows(&h);
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::NoteFieldCountMismatch { note: b }));
    assert_eq!(backups.calls(), 0);
    assert_eq!(rows(&h), before);
    raw_fields(&h.collection, b, "c\u{1f}d");
    let read = info(&h, &[a, b], duo).unwrap();
    backups.then(move |c| raw_fields(c, a, "a\u{1f}b\u{1f}late"));
    let e = run(&h, request(&read, &[a, b])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::NoteFieldCountMismatch { note: a }));
    assert_eq!(backups.calls(), 1);
    assert_eq!(note(&h, a).notetype_id, pair);
    assert_eq!(note(&h, a).fields, "a\u{1f}b\u{1f}late");
}

/// Removes the old note type's last field with no `scm` change, as an
/// incoming apply can (a schema change in the info query's millisecond).
fn drop_last_field(c: &Collection, id: NotetypeId) {
    c.run(
        OpSpec::named("incoming_field_removal", OpSource::CloudIncoming).stamp(meguri_storage::StampMode::Raw),
        move |ctx| {
            let mut nt = ctx.scope().notetype(id)?.unwrap();
            nt.fields.pop();
            ctx.scope().put_notetype(&nt)?;
            Ok(())
        },
    )
    .unwrap();
}

/// M8 as ruled 2026-11-21: the maps are checked against the layout the
/// info query read. A screen whose layout changed since, here the old note
/// type losing the field the map names while `col.scm` stays equal, is
/// `schema_changed` at planning, before any backup, not an invalid
/// argument; an ordinal bad for the layout the user saw stays
/// `InvalidArgument`.
#[test]
fn a_stale_layout_is_schema_changed_and_a_bad_ordinal_on_a_fresh_one_is_invalid() {
    let (h, backups) = harness();
    let trio = made(&h, "Trio", false, &["Front", "Back", "Extra"], &["{{Front}}"]);
    let target = made(&h, "Target", false, &["Front", "Back", "Extra"], &["{{Front}}"]);
    let a = add(&h.collection, trio, &["a", "b", ""]);
    let read = info(&h, &[a], target).unwrap();
    assert_eq!(read.fields, [Some(0), Some(1), Some(2)]);
    let before_scm = scm(&h);
    drop_last_field(&h.collection, trio);
    assert_eq!(scm(&h), before_scm);
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(backups.calls(), 0);
    let fresh = info(&h, &[a], target).unwrap();
    let bad = mapped(&fresh, &[a], vec![Some(0), Some(1), Some(2)], Some(vec![Some(0)]));
    assert!(matches!(run(&h, bad), Err(EngineError::InvalidArgument { .. })));
    assert_eq!(backups.calls(), 0);
    assert_eq!(note(&h, a).notetype_id, trio);
}

/// A reorder of the new note type's fields committed while the backup ran,
/// in the same millisecond as the info query's stamp so `col.scm` reads as
/// before: the write phase compares the parts as planned and refuses with
/// `schema_changed`; nothing is written.
#[test]
fn a_layout_change_that_leaves_the_stamp_equal_is_schema_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let duo = duo(&h);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], duo).unwrap();
    backups.then(move |c| {
        c.run(
            OpSpec::named("incoming_reorder", OpSource::CloudIncoming).stamp(meguri_storage::StampMode::Raw),
            move |ctx| {
                let mut nt = ctx.scope().notetype(duo)?.unwrap();
                nt.fields.swap(0, 1);
                for (ord, field) in nt.fields.iter_mut().enumerate() {
                    field.ord = ord as u32;
                }
                ctx.scope().put_notetype(&nt)?;
                Ok(())
            },
        )
        .unwrap();
    });
    let before_scm = scm(&h);
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert_eq!(scm(&h), before_scm, "the stamp the screen holds still matches");
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(note(&h, a).notetype_id, pair);
}

/// `ChangeInfo::preview` ignores entries the new note type does not have
/// and origins the old one does not have (the change refuses such maps).
#[test]
fn the_preview_ignores_entries_outside_both_note_types() {
    let (h, _) = harness();
    let duo = duo(&h);
    let single = made(&h, "Single", false, &["Front"], &["{{Front}}"]);
    let a = add(&h.collection, duo, &["a", "b"]);
    let read = info(&h, &[a], single).unwrap();
    let p = read.preview(&[Some(0), Some(1)], Some(&[Some(0), Some(1)]));
    assert_eq!(p.discarded_fields, [1], "the second entry is past the new fields");
    assert_eq!(p.dropped_templates, Some(vec![1]));
    let p = read.preview(&[Some(5)], Some(&[Some(9)]));
    assert_eq!(p.discarded_fields, [0, 1]);
    assert_eq!(p.dropped_templates, Some(vec![0, 1]));
}

/// A note type the plan found, deleted while the backup ran in the same
/// millisecond (so `col.scm` reads as the screen's stamp): the screen is
/// stale, `schema_changed`, not `not_found`; nothing is written.
#[test]
fn a_note_type_deleted_during_the_backup_is_schema_changed() {
    let (h, backups) = harness();
    let pair = pair(&h);
    let target = made(&h, "Target", false, &["Front", "Back"], &["{{Front}}"]);
    let a = add(&h.collection, pair, &["a", "b"]);
    let read = info(&h, &[a], target).unwrap();
    backups.then(move |c| {
        let request = meguri_engine::notetypes::remove::RemoveNotetype {
            notetype: target,
            confirmed_notes: 0,
            confirmed_cards: 0,
        };
        meguri_engine::notetypes::remove::remove(c, request, CancelToken::new(), ProgressSink::new()).unwrap();
    });
    let stamp = read.schema;
    let e = run(&h, request(&read, &[a])).unwrap_err();
    assert_eq!(scm(&h), stamp, "the removal kept the stamp's millisecond");
    assert_eq!(refusal(&e), Some(&StructureRefusal::SchemaChanged));
    assert_eq!(note(&h, a).notetype_id, pair);
}
